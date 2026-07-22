// Local bookmark manager.
//
// A small, flat bookmark store — no folders, tags, sync, or import/export. Rust
// owns the list (single source of truth, mirroring the tabs model) and persists
// it atomically to pake-bookmarks.json in the app data dir. The tab strip drives
// "bookmark the active page", and a dedicated manager page (served from the
// pakebookmarks:// scheme) lists/searches/edits/opens/deletes bookmarks.

use crate::util::get_data_dir;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const BOOKMARKS_FILE: &str = "pake-bookmarks.json";

// The bookmark manager page. Self-contained (inline script) so it works as a
// normal content tab without touching the shared injection chain.
pub const MANAGER_SCHEME: &str = "pakebookmarks";
pub const MANAGER_HTML: &str = include_str!("../inject/bookmarks_manager.html");

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub url: String,
    pub created: u64,
    pub updated: u64,
}

pub struct BookmarksState {
    pub dir: PathBuf,
    pub items: Mutex<Vec<Bookmark>>,
    pub counter: AtomicU64,
}

impl BookmarksState {
    pub fn path(&self) -> PathBuf {
        self.dir.join(BOOKMARKS_FILE)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// Pake's own pages (tab strip, bookmark manager). Their custom schemes resolve
// as `http://<scheme>.localhost/` on Windows, so the host form has to be
// excluded too or the manager can bookmark itself.
fn is_internal_page(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("paketabs:")
        || lower.starts_with("pakebookmarks:")
        || lower.contains("paketabs.localhost")
        || lower.contains("pakebookmarks.localhost")
}

// A bookmarkable/openable URL must parse, use a normal web scheme, and not be
// one of Pake's own internal pages.
pub fn is_valid_url(url: &str) -> bool {
    if is_internal_page(url) {
        return false;
    }
    matches!(
        tauri::Url::parse(url.trim()).ok().map(|u| u.scheme().to_ascii_lowercase()),
        Some(scheme) if scheme == "http" || scheme == "https" || scheme == "file"
    )
}

// Canonical form used for duplicate detection: lowercase scheme + host, no
// trailing slash on the path, fragment dropped. Query is preserved.
pub fn normalize_url(url: &str) -> String {
    match tauri::Url::parse(url.trim()) {
        Ok(u) => {
            let scheme = u.scheme().to_ascii_lowercase();
            let host = u.host_str().unwrap_or("").to_ascii_lowercase();
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            let path = u.path().trim_end_matches('/');
            let query = u.query().map(|q| format!("?{q}")).unwrap_or_default();
            format!("{scheme}://{host}{port}{path}{query}")
        }
        Err(_) => url.trim().to_ascii_lowercase(),
    }
}

fn load(dir: &std::path::Path) -> Vec<Bookmark> {
    match std::fs::read_to_string(dir.join(BOOKMARKS_FILE)) {
        // Legacy/partial/malformed records must not crash startup: drop
        // individual entries that fail to parse, and fall back to empty on a
        // wholly malformed file.
        Ok(raw) => serde_json::from_str::<Vec<Bookmark>>(&raw)
            .unwrap_or_default()
            .into_iter()
            .filter(|b| !b.id.trim().is_empty() && is_valid_url(&b.url))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn write_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

fn persist(state: &BookmarksState, items: &[Bookmark]) {
    if let Ok(json) = serde_json::to_string_pretty(items) {
        if let Err(e) = write_atomic(&state.path(), &json) {
            eprintln!("[Pake][bookmarks] save error: {e}");
        }
    }
}

pub fn init(app: &AppHandle) {
    if app.try_state::<BookmarksState>().is_some() {
        return;
    }
    let package_name = app
        .try_state::<crate::app::window::MultiWindowState>()
        .and_then(|s| s.tauri_config.product_name.clone())
        .unwrap_or_else(|| "pake".to_string());
    if let Ok(dir) = get_data_dir(app, package_name) {
        let items = load(&dir);
        app.manage(BookmarksState {
            dir,
            items: Mutex::new(items),
            counter: AtomicU64::new(0),
        });
    }
}

// --- Commands ---

#[tauri::command]
pub fn bookmark_list(app: AppHandle) -> Vec<Bookmark> {
    app.try_state::<BookmarksState>()
        .and_then(|s| s.items.lock().ok().map(|g| g.clone()))
        .unwrap_or_default()
}

#[tauri::command]
pub fn bookmark_is(app: AppHandle, url: String) -> bool {
    let Some(state) = app.try_state::<BookmarksState>() else {
        return false;
    };
    let target = normalize_url(&url);
    state
        .items
        .lock()
        .map(|items| items.iter().any(|b| normalize_url(&b.url) == target))
        .unwrap_or(false)
}

#[tauri::command]
pub fn bookmark_add(
    app: AppHandle,
    url: String,
    title: Option<String>,
) -> Result<Bookmark, String> {
    if !is_valid_url(&url) {
        return Err("Invalid URL".to_string());
    }
    let state = app
        .try_state::<BookmarksState>()
        .ok_or_else(|| "Bookmarks unavailable".to_string())?;
    let mut items = state
        .items
        .lock()
        .map_err(|_| "lock poisoned".to_string())?;

    let target = normalize_url(&url);
    // Prevent accidental duplicates for the same normalized URL.
    if let Some(existing) = items.iter().find(|b| normalize_url(&b.url) == target) {
        return Ok(existing.clone());
    }

    let now = now_millis();
    let n = state.counter.fetch_add(1, Ordering::SeqCst);
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| url.trim().to_string());
    let bookmark = Bookmark {
        id: format!("bm-{now}-{n}"),
        title,
        url: url.trim().to_string(),
        created: now,
        updated: now,
    };
    items.push(bookmark.clone());
    persist(&state, &items);
    Ok(bookmark)
}

#[tauri::command]
pub fn bookmark_remove(app: AppHandle, id: String) {
    let Some(state) = app.try_state::<BookmarksState>() else {
        return;
    };
    let Ok(mut items) = state.items.lock() else {
        return;
    };
    items.retain(|b| b.id != id);
    persist(&state, &items);
}

// Remove by URL (used by the tab-strip star to un-bookmark the active page).
#[tauri::command]
pub fn bookmark_remove_url(app: AppHandle, url: String) {
    let Some(state) = app.try_state::<BookmarksState>() else {
        return;
    };
    let target = normalize_url(&url);
    let Ok(mut items) = state.items.lock() else {
        return;
    };
    items.retain(|b| normalize_url(&b.url) != target);
    persist(&state, &items);
}

#[tauri::command]
pub fn bookmark_update(
    app: AppHandle,
    id: String,
    title: String,
    url: String,
) -> Result<Bookmark, String> {
    if !is_valid_url(&url) {
        return Err("Invalid URL".to_string());
    }
    let state = app
        .try_state::<BookmarksState>()
        .ok_or_else(|| "Bookmarks unavailable".to_string())?;
    let mut items = state
        .items
        .lock()
        .map_err(|_| "lock poisoned".to_string())?;

    let target = normalize_url(&url);
    // Don't let an edit collide with a different existing bookmark.
    if items
        .iter()
        .any(|b| b.id != id && normalize_url(&b.url) == target)
    {
        return Err("Another bookmark already uses that URL".to_string());
    }

    let updated = {
        let Some(b) = items.iter_mut().find(|b| b.id == id) else {
            return Err("Bookmark not found".to_string());
        };
        let trimmed = title.trim();
        b.title = if trimmed.is_empty() {
            url.trim().to_string()
        } else {
            trimmed.to_string()
        };
        b.url = url.trim().to_string();
        b.updated = now_millis();
        b.clone()
    };
    persist(&state, &items);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_for_dedup() {
        assert_eq!(
            normalize_url("https://Example.com/"),
            normalize_url("https://example.com")
        );
        assert_eq!(
            normalize_url("https://example.com/path#frag"),
            "https://example.com/path"
        );
        assert_ne!(
            normalize_url("https://example.com/a"),
            normalize_url("https://example.com/b")
        );
        assert_eq!(
            normalize_url("https://example.com/p?q=1"),
            "https://example.com/p?q=1"
        );
    }

    #[test]
    fn validates_schemes() {
        assert!(is_valid_url("https://example.com"));
        assert!(is_valid_url("http://example.com/x"));
        assert!(!is_valid_url("javascript:alert(1)"));
        assert!(!is_valid_url("pakebookmarks://localhost/"));
        assert!(!is_valid_url("not a url"));
        assert!(!is_valid_url(""));
    }

    #[test]
    fn rejects_internal_pages_in_their_windows_host_form() {
        // On Windows the custom schemes resolve as http://<scheme>.localhost/,
        // which would otherwise pass the plain http/https check.
        assert!(!is_valid_url("http://pakebookmarks.localhost/"));
        assert!(!is_valid_url("http://paketabs.localhost/"));
        assert!(!is_valid_url("HTTP://PakeBookmarks.localhost/"));
        assert!(!is_valid_url("paketabs://localhost/"));
    }
}
