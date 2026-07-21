// Last-session restoration for tabbed mode.
//
// The tab model (see app::tabs) lives entirely in Rust, so persisting a session
// is just a matter of snapshotting the ordered tab list + active index to a
// small JSON file in the app data dir and reloading it on the next launch.
//
// We deliberately store only what is needed to reopen tabs (URL, title, order,
// active index) — never live webviews, cookies, form state, or navigation
// history. Writes are atomic (temp file + rename) and debounced so bursty tab
// activity does not thrash the disk.

use crate::app::tabs::TabsState;
use crate::util::get_data_dir;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const SESSION_FILE: &str = "pake-session.json";
const SETTINGS_FILE: &str = "pake-settings.json";
const SAVE_DEBOUNCE_MS: u64 = 400;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SessionData {
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
    #[serde(default)]
    pub active: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionSettings {
    pub restore_session: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        // Tabbed mode is a browsing surface, so restoring the previous set of
        // tabs is the least-surprising default for it.
        Self {
            restore_session: true,
        }
    }
}

// Runtime state managed by Tauri while the app is running.
pub struct SessionState {
    pub dir: PathBuf,
    // Suppresses persistence while we are actively rebuilding tabs on startup,
    // so a partial restore can never overwrite the saved session with a broken
    // subset (prevents restore loops).
    pub restoring: AtomicBool,
    // Monotonic token used to debounce/coalesce bursts of save requests.
    pub generation: AtomicU64,
    pub settings: Mutex<SessionSettings>,
}

impl SessionState {
    pub fn new(dir: PathBuf) -> Self {
        let settings = load_settings(&dir);
        Self {
            dir,
            restoring: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            settings: Mutex::new(settings),
        }
    }

    pub fn session_path(&self) -> PathBuf {
        self.dir.join(SESSION_FILE)
    }

    pub fn settings_path(&self) -> PathBuf {
        self.dir.join(SETTINGS_FILE)
    }
}

// True for pages that must never be persisted or restored (tab strip, bookmark
// manager, blank/data pages). Restoring these would resurrect internal UI.
fn is_internal_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() {
        return true;
    }
    let lower = u.to_ascii_lowercase();
    lower.starts_with("paketabs:")
        || lower.starts_with("pakebookmarks:")
        || lower.starts_with("about:")
        || lower.starts_with("data:")
        || lower.contains("paketabs.localhost")
        || lower.contains("pakebookmarks.localhost")
}

// A restorable URL must parse and use a normal web scheme.
pub fn is_restorable_url(url: &str) -> bool {
    if is_internal_url(url) {
        return false;
    }
    matches!(
        tauri::Url::parse(url.trim()).ok().map(|u| u.scheme().to_ascii_lowercase()),
        Some(scheme) if scheme == "http" || scheme == "https" || scheme == "file"
    )
}

fn package_name(app: &AppHandle) -> String {
    app.try_state::<crate::app::window::MultiWindowState>()
        .and_then(|s| s.tauri_config.product_name.clone())
        .unwrap_or_else(|| "pake".to_string())
}

fn manage_dir(app: &AppHandle) -> Option<PathBuf> {
    get_data_dir(app, package_name(app)).ok()
}

// Register SessionState. Idempotent-ish: only the first call installs state.
pub fn init(app: &AppHandle) {
    if app.try_state::<SessionState>().is_some() {
        return;
    }
    if let Some(dir) = manage_dir(app) {
        app.manage(SessionState::new(dir));
    }
}

fn load_settings(dir: &std::path::Path) -> SessionSettings {
    match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => SessionSettings::default(),
    }
}

// Atomic write: serialize to a sibling temp file, then rename over the target.
fn write_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

pub fn restore_enabled(app: &AppHandle) -> bool {
    app.try_state::<SessionState>()
        .map(|s| s.settings.lock().map(|g| g.restore_session).unwrap_or(true))
        .unwrap_or(false)
}

// Read the current tab model and persist it. Skips internal pages and never
// writes while restoring.
fn write_current(app: &AppHandle) {
    let Some(session) = app.try_state::<SessionState>() else {
        return;
    };
    if session.restoring.load(Ordering::SeqCst) {
        return;
    }
    let Some(tabs_state) = app.try_state::<TabsState>() else {
        return;
    };

    let (tabs, active_label) = {
        let model = match tabs_state.0.lock() {
            Ok(m) => m,
            Err(_) => return,
        };
        (model.tabs.clone(), model.active.clone())
    };

    let mut session_tabs = Vec::new();
    let mut active_index = 0usize;
    for tab in &tabs {
        if !is_restorable_url(&tab.url) {
            continue;
        }
        if tab.label == active_label {
            active_index = session_tabs.len();
        }
        session_tabs.push(SessionTab {
            url: tab.url.clone(),
            title: tab.title.clone(),
        });
    }

    // Nothing worth persisting (e.g. only the bookmark manager is open): keep
    // the last good session on disk rather than clobbering it with an empty one.
    if session_tabs.is_empty() {
        return;
    }

    let data = SessionData {
        tabs: session_tabs,
        active: active_index,
    };
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        if let Err(e) = write_atomic(&session.session_path(), &json) {
            eprintln!("[Pake][session] save error: {e}");
        }
    }
}

// Debounced save. Coalesces bursts: each call bumps the generation and the
// spawned task only writes if it is still the newest request after the delay.
pub fn request_save(app: &AppHandle) {
    let Some(session) = app.try_state::<SessionState>() else {
        return;
    };
    if session.restoring.load(Ordering::SeqCst) {
        return;
    }
    let my_gen = session.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(SAVE_DEBOUNCE_MS)).await;
        if let Some(session) = app.try_state::<SessionState>() {
            if session.generation.load(Ordering::SeqCst) != my_gen {
                return; // superseded by a newer change
            }
        }
        write_current(&app);
    });
}

// Synchronous save for shutdown paths, bypassing the debounce.
pub fn save_now(app: &AppHandle) {
    write_current(app);
}

// Load a previously saved session, filtering out anything unsafe/unrestorable.
// Returns None when restoration is disabled or no usable session exists.
pub fn load_for_restore(app: &AppHandle) -> Option<SessionData> {
    if !restore_enabled(app) {
        return None;
    }
    let session = app.try_state::<SessionState>()?;
    let raw = std::fs::read_to_string(session.session_path()).ok()?;
    // Malformed JSON must never crash startup — treat it as "no session".
    let data: SessionData = serde_json::from_str(&raw).ok()?;
    let tabs: Vec<SessionTab> = data
        .tabs
        .into_iter()
        .filter(|t| is_restorable_url(&t.url))
        .collect();
    if tabs.is_empty() {
        return None;
    }
    let active = data.active.min(tabs.len() - 1);
    Some(SessionData { tabs, active })
}

pub fn set_restoring(app: &AppHandle, value: bool) {
    if let Some(session) = app.try_state::<SessionState>() {
        session.restoring.store(value, Ordering::SeqCst);
    }
}

// --- Commands exposed to the tab-strip UI ---

#[tauri::command]
pub fn session_get_restore(app: AppHandle) -> bool {
    restore_enabled(&app)
}

#[tauri::command]
pub fn session_set_restore(app: AppHandle, enabled: bool) {
    let Some(session) = app.try_state::<SessionState>() else {
        return;
    };
    if let Ok(mut settings) = session.settings.lock() {
        settings.restore_session = enabled;
        if let Ok(json) = serde_json::to_string_pretty(&*settings) {
            let _ = write_atomic(&session.settings_path(), &json);
        }
    }
    // Persist the current tabs immediately so enabling restore later behaves
    // sensibly even if the app is closed abnormally.
    if enabled {
        save_now(&app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_internal_and_malformed_urls() {
        assert!(!is_restorable_url(""));
        assert!(!is_restorable_url("about:blank"));
        assert!(!is_restorable_url("paketabs://localhost/"));
        assert!(!is_restorable_url("http://pakebookmarks.localhost/"));
        assert!(!is_restorable_url("data:text/html,hi"));
        assert!(!is_restorable_url("not a url"));
        assert!(!is_restorable_url("javascript:alert(1)"));
    }

    #[test]
    fn accepts_normal_web_urls() {
        assert!(is_restorable_url("https://example.com"));
        assert!(is_restorable_url("http://example.com/path?q=1"));
    }

    #[test]
    fn active_index_is_clamped_on_load() {
        let data = SessionData {
            tabs: vec![
                SessionTab {
                    url: "https://a.com".into(),
                    title: "A".into(),
                },
                SessionTab {
                    url: "https://b.com".into(),
                    title: "B".into(),
                },
            ],
            active: 9,
        };
        let clamped = data.active.min(data.tabs.len() - 1);
        assert_eq!(clamped, 1);
    }

    #[test]
    fn malformed_session_json_is_ignored() {
        let parsed: Result<SessionData, _> = serde_json::from_str("{ not json ]");
        assert!(parsed.is_err());
    }

    #[test]
    fn default_settings_enable_restore() {
        assert!(SessionSettings::default().restore_session);
    }
}
