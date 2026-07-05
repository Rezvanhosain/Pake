// Same-window browser tabs.
//
// Instead of opening extra OS windows, tabbed mode builds ONE native window
// that hosts multiple child webviews (Tauri's `unstable` multi-webview API):
//
//   +--------------------------------------------------+
//   | pake-chrome  (tab strip, ~44px, data: URL)       |
//   +--------------------------------------------------+
//   | pake-content-N  (the active site; others hidden) |
//   |                                                   |
//   +--------------------------------------------------+
//
// The tab strip is its own tiny webview pinned to the top, so it can never
// overlap or clip the site's own header (e.g. YouTube's fixed masthead) — the
// site renders in a separate webview below it. All content webviews share one
// data directory, so cookies/session are shared across tabs. Rust is the single
// source of truth for the tab list; it pushes state to the chrome webview via
// the `tabs://state` event and receives commands (`tab_new`, `tab_switch`,
// `tab_close`, `tab_report`, `tab_ready`) back over IPC.

use crate::app::config::PakeConfig;
use crate::app::window::MultiWindowState;
use crate::util::get_data_dir;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::webview::{DownloadEvent, WebviewBuilder};
use tauri::window::WindowBuilder;
use tauri::{
    AppHandle, Config, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl,
};

pub const SHELL_LABEL: &str = "pake";
pub const CHROME_LABEL: &str = "pake-chrome";
pub const CHROME_SCHEME: &str = "paketabs";
const TAB_BAR_HEIGHT: f64 = 44.0;

// The tab-strip page. Served from a custom `paketabs://` scheme (a local
// origin the app capability covers) rather than a data: URL, because a data:
// URL's opaque origin is not covered by any capability, which disables its
// Tauri IPC (invoke/listen) entirely. The strip's logic lives in the injected
// tabs_chrome.js initialization script.
pub const CHROME_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>tabs</title></head><body></body></html>"#;

#[derive(Clone, serde::Serialize)]
pub struct TabInfo {
    pub label: String,
    pub title: String,
    pub url: String,
}

#[derive(Default)]
pub struct TabsModel {
    pub tabs: Vec<TabInfo>,
    pub active: String,
    pub counter: u32,
}

pub struct TabsState(pub Mutex<TabsModel>);

#[derive(Clone, serde::Serialize)]
struct TabsStatePayload {
    tabs: Vec<TabInfo>,
    active: String,
}

fn chrome_url() -> WebviewUrl {
    // Served by the `paketabs` custom URI scheme registered in lib.rs. On
    // Windows the scheme resolves as `http://paketabs.localhost/`.
    let candidates = [
        format!("{CHROME_SCHEME}://localhost/"),
        format!("http://{CHROME_SCHEME}.localhost/"),
    ];
    for candidate in candidates {
        if let Ok(url) = Url::parse(&candidate) {
            return WebviewUrl::External(url);
        }
    }
    WebviewUrl::External(Url::parse("about:blank").unwrap())
}

fn home_url(config: &PakeConfig) -> String {
    config
        .windows
        .first()
        .map(|w| w.url.clone())
        .unwrap_or_default()
}

// Build a content-tab webview builder carrying the same injection chain and
// browser tuning as a normal single-webview Pake window, plus two tab-only
// scripts: the tab's own label and the title/URL reporter.
fn content_builder<'a>(
    app: &AppHandle,
    config: &'a PakeConfig,
    tauri_config: &Config,
    label: &str,
    url: WebviewUrl,
) -> tauri::Result<WebviewBuilder<tauri::Wry>> {
    let window_config = config
        .windows
        .first()
        .ok_or_else(|| {
            tauri::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "pake.json must define at least one window configuration",
            ))
        })?;

    let package_name = tauri_config
        .product_name
        .clone()
        .unwrap_or_else(|| "pake".to_string());
    let data_dir = get_data_dir(app, package_name).map_err(tauri::Error::Io)?;

    let user_agent = config.user_agent.get();
    let config_script = format!(
        "window.pakeConfig = {}",
        serde_json::to_string(&window_config).unwrap_or_else(|_| "{}".to_string())
    );
    let label_script = format!("window.__PAKE_TAB_LABEL__ = {:?};", label);

    let mut builder = WebviewBuilder::new(label, url)
        .user_agent(user_agent)
        .incognito(window_config.incognito)
        .data_directory(data_dir)
        .initialization_script(&config_script)
        .initialization_script(&label_script)
        .initialization_script(include_str!("../inject/adblock.js"))
        .initialization_script(include_str!("../inject/find.js"))
        .initialization_script(include_str!("../inject/toast.js"))
        .initialization_script(include_str!("../inject/fullscreen.js"))
        .initialization_script(include_str!("../inject/event.js"))
        .initialization_script(include_str!("../inject/toolbar.js"))
        .initialization_script(include_str!("../inject/tabs_content.js"))
        .initialization_script(include_str!("../inject/style.js"))
        .initialization_script(include_str!("../inject/theme_refresh.js"))
        .initialization_script(include_str!("../inject/auth.js"))
        .initialization_script(include_str!("../inject/custom.js"));

    #[cfg(target_os = "windows")]
    {
        builder = builder.additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-blink-features=AutomationControlled",
        );
    }

    if !window_config.enable_drag_drop {
        builder = builder.disable_drag_drop_handler();
    }

    // Route webview-initiated downloads to the OS Downloads folder, matching the
    // single-window path in window.rs.
    let download_handle = app.clone();
    builder = builder.on_download(move |_webview, event| {
        if let DownloadEvent::Requested { url, destination } = event {
            if let Ok(download_dir) = download_handle.path().download_dir() {
                let filename = destination
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .filter(|n| !n.is_empty())
                    .or_else(|| {
                        url.path_segments()
                            .and_then(|mut s| s.next_back())
                            .map(|s| s.to_string())
                            .filter(|s| !s.is_empty())
                    })
                    .unwrap_or_else(|| "download".to_string());
                let target = download_dir.join(filename);
                if let Some(path_str) = target.to_str() {
                    *destination = PathBuf::from(crate::util::check_file_or_append(path_str));
                }
            }
        }
        true
    });

    Ok(builder)
}

fn window_logical_size(window: &tauri::Window) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let phys = window
        .inner_size()
        .unwrap_or(tauri::PhysicalSize::new(1200, 780));
    (phys.width as f64 / scale, phys.height as f64 / scale)
}

fn content_position() -> LogicalPosition<f64> {
    LogicalPosition::new(0.0, TAB_BAR_HEIGHT)
}

fn content_size(lw: f64, lh: f64) -> LogicalSize<f64> {
    LogicalSize::new(lw.max(1.0), (lh - TAB_BAR_HEIGHT).max(1.0))
}

// Reposition/resize the tab strip and every content webview to match the
// current window size. Called on window resize.
pub fn relayout(app: &AppHandle) {
    let Some(window) = app.get_window(SHELL_LABEL) else {
        return;
    };
    let (lw, lh) = window_logical_size(&window);
    for webview in window.webviews() {
        if webview.label() == CHROME_LABEL {
            let _ = webview.set_position(LogicalPosition::new(0.0, 0.0));
            let _ = webview.set_size(LogicalSize::new(lw.max(1.0), TAB_BAR_HEIGHT));
        } else {
            let _ = webview.set_position(content_position());
            let _ = webview.set_size(content_size(lw, lh));
        }
    }
}

fn show_only(app: &AppHandle, active: &str) {
    let Some(window) = app.get_window(SHELL_LABEL) else {
        return;
    };
    for webview in window.webviews() {
        let label = webview.label().to_string();
        if label == CHROME_LABEL {
            continue;
        }
        if label == active {
            let _ = webview.show();
            let _ = webview.set_focus();
        } else {
            let _ = webview.hide();
        }
    }
}

fn emit_state(app: &AppHandle) {
    let state = app.state::<TabsState>();
    let model = state.0.lock().unwrap();
    let payload = TabsStatePayload {
        tabs: model.tabs.clone(),
        active: model.active.clone(),
    };
    if let Err(e) = app.emit("tabs-state", payload) {
        eprintln!("[Pake][tabs] emit ERROR: {e}");
    }
}

// Create a new content tab, add it to the window, make it active, and record it.
fn spawn_tab(app: &AppHandle, url: String) -> tauri::Result<String> {
    let state_mw = app.state::<MultiWindowState>();
    let config = state_mw.pake_config.clone();
    let tauri_config = state_mw.tauri_config.clone();

    let Some(window) = app.get_window(SHELL_LABEL) else {
        return Err(tauri::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "tab shell window missing",
        )));
    };

    let label = {
        let state = app.state::<TabsState>();
        let mut model = state.0.lock().unwrap();
        model.counter += 1;
        format!("pake-content-{}", model.counter)
    };

    let parsed = Url::parse(&url).unwrap_or_else(|_| {
        Url::parse(&home_url(&config)).unwrap_or_else(|_| Url::parse("about:blank").unwrap())
    });
    let builder = content_builder(
        app,
        &config,
        &tauri_config,
        &label,
        WebviewUrl::External(parsed),
    )?;

    let (lw, lh) = window_logical_size(&window);
    if let Err(e) = window.add_child(builder, content_position(), content_size(lw, lh)) {
        eprintln!("[Pake][tabs] add_child failed for {label}: {e}");
        return Err(e);
    }

    {
        let state = app.state::<TabsState>();
        let mut model = state.0.lock().unwrap();
        model.tabs.push(TabInfo {
            label: label.clone(),
            title: "New Tab".to_string(),
            url,
        });
        model.active = label.clone();
    }

    show_only(app, &label);
    emit_state(app);
    Ok(label)
}

// Build the tabbed window: parent window + chrome strip + first content tab.
pub fn setup_tabbed_window(
    app: &AppHandle,
    config: &PakeConfig,
    tauri_config: &Config,
) -> tauri::Result<()> {
    let window_config = config.windows.first().ok_or_else(|| {
        tauri::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "pake.json must define at least one window configuration",
        ))
    })?;

    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let logical_width = window_config.width / scale;
    let logical_height = window_config.height / scale;

    let effective_title = window_config
        .title
        .as_deref()
        .unwrap_or_else(|| tauri_config.product_name.as_deref().unwrap_or("Pake"));

    let mut wb = WindowBuilder::new(app, SHELL_LABEL)
        .title(effective_title)
        .inner_size(logical_width, logical_height)
        .resizable(window_config.resizable)
        .visible(false);

    if window_config.min_width > 0.0 || window_config.min_height > 0.0 {
        let min_w = if window_config.min_width > 0.0 {
            window_config.min_width
        } else {
            logical_width
        };
        let min_h = if window_config.min_height > 0.0 {
            window_config.min_height
        } else {
            logical_height
        };
        wb = wb.min_inner_size(min_w, min_h);
    }
    if window_config.maximize {
        wb = wb.maximized(true);
    }

    let window = wb.build()?;

    app.manage(TabsState(Mutex::new(TabsModel::default())));

    // Chrome tab strip.
    let (lw, lh) = window_logical_size(&window);
    let chrome = WebviewBuilder::new(CHROME_LABEL, chrome_url())
        .initialization_script(include_str!("../inject/tabs_chrome.js"));
    window.add_child(
        chrome,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(lw.max(1.0), TAB_BAR_HEIGHT),
    )?;

    // First tab.
    spawn_tab(app, home_url(config))?;

    // Keep the layout in sync with window size.
    let resize_handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Resized(_) = event {
            relayout(&resize_handle);
        }
    });

    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

#[tauri::command]
pub fn tab_ready(app: AppHandle) {
    emit_state(&app);
}

// Async so it runs off the main thread: add_child creates a WebView2
// controller asynchronously and needs the event loop to keep pumping, which a
// sync command (which blocks the loop) would deadlock.
#[tauri::command]
pub async fn tab_new(app: AppHandle, url: Option<String>) {
    let target = url
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| {
            let state = app.state::<MultiWindowState>();
            home_url(&state.pake_config)
        });
    if let Err(e) = spawn_tab(&app, target) {
        eprintln!("[Pake][tabs] failed to open tab: {e}");
    }
}

#[tauri::command]
pub async fn tab_switch(app: AppHandle, label: String) {
    {
        let state = app.state::<TabsState>();
        let mut model = state.0.lock().unwrap();
        if model.tabs.iter().any(|t| t.label == label) {
            model.active = label.clone();
        }
    }
    show_only(&app, &label);
    emit_state(&app);
}

#[tauri::command]
pub async fn tab_close(app: AppHandle, label: String) {
    // Close the underlying webview.
    if let Some(window) = app.get_window(SHELL_LABEL) {
        if let Some(webview) = window.webviews().into_iter().find(|w| w.label() == label) {
            let _ = webview.close();
        }
    }

    let (next_active, empty) = {
        let state = app.state::<TabsState>();
        let mut model = state.0.lock().unwrap();
        if let Some(idx) = model.tabs.iter().position(|t| t.label == label) {
            model.tabs.remove(idx);
            if model.active == label {
                let neighbor = idx.saturating_sub(if idx >= model.tabs.len() { 1 } else { 0 });
                model.active = model
                    .tabs
                    .get(neighbor)
                    .map(|t| t.label.clone())
                    .unwrap_or_default();
            }
        }
        (model.active.clone(), model.tabs.is_empty())
    };

    if empty {
        // Never leave the window with zero tabs — open a fresh home tab.
        let state = app.state::<MultiWindowState>();
        let home = home_url(&state.pake_config);
        drop(state);
        let _ = spawn_tab(&app, home);
        return;
    }

    show_only(&app, &next_active);
    emit_state(&app);
}

#[tauri::command]
pub fn tab_report(app: AppHandle, label: String, title: String, url: String) {
    {
        let state = app.state::<TabsState>();
        let mut model = state.0.lock().unwrap();
        if let Some(tab) = model.tabs.iter_mut().find(|t| t.label == label) {
            if !title.trim().is_empty() {
                tab.title = title;
            }
            if !url.trim().is_empty() {
                tab.url = url;
            }
        }
    }
    emit_state(&app);
}
