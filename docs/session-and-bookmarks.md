# Session restore & bookmarks (tabbed mode)

Both features apply to same-window **tabbed mode** (`"tabs": true`), where Rust
owns the tab model (see `src-tauri/src/app/tabs.rs`).

## Last-session restore

- Module: `src-tauri/src/app/session.rs`.
- Storage: `<config_dir>/<product_name>/pake-session.json` — ordered
  `{ url, title }` tabs + `active` index. Setting lives in `pake-settings.json`.
- Saved (debounced, atomic temp+rename) on tab open/close/switch/navigate, and
  synchronously on normal shutdown (`RunEvent::ExitRequested`).
- Only normal `http(s)`/`file` pages are stored/restored; internal pages
  (`paketabs://`, `pakebookmarks://`, `about:`, `data:`) are skipped.
- Setting **"Restore previous session on startup"** — the `⟳` toggle in the tab
  strip. Default **on** for tabbed mode. The last good session is still written
  even when the setting is off, so enabling it later behaves sensibly.
- Malformed/partial session data never crashes startup (treated as "no
  session"); a single bad tab is skipped, not fatal. Saves are suppressed until
  restoration completes to prevent restore loops.

## Bookmarks

- Module: `src-tauri/src/app/bookmarks.rs`; manager page
  `src-tauri/src/inject/bookmarks_manager.html` served from the
  `pakebookmarks://` scheme.
- Storage: `<config_dir>/<product_name>/pake-bookmarks.json` — flat list of
  `{ id, title, url, created, updated }`. Atomic writes; kept separate from
  session state. Invalid/legacy records are dropped on load, not fatal.
- Tab-strip controls: `★`/`☆` bookmarks (or un-bookmarks) the active page and
  reflects its state; `📑` opens the manager tab.
- Manager: search by title/URL, open in a tab, edit title+URL, delete.
- Duplicates for the same **normalized** URL (lowercase scheme/host, no trailing
  slash, fragment dropped) are prevented on add and on edit.
