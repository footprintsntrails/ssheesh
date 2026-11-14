//! What 0.10.x left on disk, exactly as stored, before anything is parsed.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The seven zustand stores 0.10.x keeps in the web view's localStorage.
pub const STORE_KEYS: [&str; 7] = [
    "sshee-settings",
    "sshee-labels",
    "sshee-workspace",
    "sshee-snippets",
    "sshee-bookmarks",
    "sshee-tunnels",
    "sshee-typing-counts",
];

/// Where a [`RawWebState`] came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawSource {
    /// WebView2 (Windows): Chromium's localStorage in LevelDB.
    WebView2LevelDb(PathBuf),
    /// WebView2 (Windows) after Chromium moved localStorage to SQLite.
    WebView2Sqlite(PathBuf),
    /// WebKit (macOS WKWebView, Linux WebKitGTK): a SQLite `ItemTable`.
    WebKitSqlite(PathBuf),
    /// Built in a test.
    Fixture,
}

/// The raw stores of one 0.10.x profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawWebState {
    /// Store key (one of [`STORE_KEYS`]) to its value as the web view stored it: a JSON string
    /// written by zustand's `persist`, decoded to UTF-8 but otherwise untouched.
    pub stores: BTreeMap<String, String>,
    /// Contents of tauri-plugin-window-state's `.window-state.json`, if there was one.
    pub window_state: Option<String>,
    pub source: RawSource,
}
