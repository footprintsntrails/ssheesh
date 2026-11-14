//! Where 0.10.x kept its web view storage and its window state, on each system.
//!
//! Every path here comes from reading the code 0.10.14 was built with (Tauri 2.12.5, wry
//! 0.57.1, tauri-plugin-window-state 2.5.0) and the web views' own sources:
//!
//! - **Windows (WebView2).** Tauri gives every web view the data folder `app_local_data_dir`
//!   unless the app picks one, and 0.10 never did (`tauri/src/manager/webview.rs`, "in
//!   `windows`, we need to force a data_directory"). That is
//!   `%LOCALAPPDATA%\io.github.footprintsntrails.ssheesh`. WebView2 puts its browser profile in
//!   `EBWebView\` below it, and wry never names a profile, so it's `Default`. Inside, Chromium
//!   keeps localStorage in `Local Storage\leveldb\` (LevelDB), or, once Chromium has moved it,
//!   in one SQLite file named `LocalStorage` (`DomStorageDatabase::GetLevelDbPath` and
//!   `GetSqlitePath`). The page's origin is `http://tauri.localhost`: on Windows wry rewrites
//!   `tauri://localhost` to `http://tauri.localhost`, and 0.10 never asked for https.
//! - **macOS (WKWebView).** Tauri forces no folder there, so WebKit uses its default store for
//!   the app's bundle id: `~/Library/WebKit/io.github.footprintsntrails.ssheesh/WebsiteData/`
//!   (`WebsiteDataStoreCocoa.mm`). Newer WebKit keeps each origin's localStorage in
//!   `Default/<hash>/<hash>/LocalStorage/localstorage.sqlite3` with an `origin` file beside;
//!   older WebKit in `LocalStorage/tauri_localhost_0.localstorage`. The origin is
//!   `tauri://localhost`.
//! - **Linux (WebKitGTK).** Tauri passes the same `app_local_data_dir`,
//!   `~/.local/share/io.github.footprintsntrails.ssheesh`, to WebKitGTK as its base data folder
//!   (`wry/src/webkitgtk/web_context.rs`). The webkit2gtk-4.1 API keeps localStorage in
//!   `localstorage/tauri_localhost_0.localstorage` below it; WebKitGTK 6 would use the newer
//!   `storage/<hash>/<hash>/LocalStorage/` layout. The origin is `tauri://localhost`.
//! - **Window state, everywhere.** tauri-plugin-window-state writes `.window-state.json` in
//!   `app_config_dir`: `%APPDATA%\<id>\` on Windows, `~/Library/Application Support/<id>/` on
//!   macOS, `~/.config/<id>/` on Linux.
//!
//! The exact folder names below the roots change with web view versions, so the roots are only
//! where the search starts (see `scan`); nothing below them is taken on trust.

use std::path::{Path, PathBuf};

/// The id 0.10.x was built with. Its folders, and the web view's, are named after it.
pub const APP_IDENTIFIER: &str = "io.github.footprintsntrails.ssheesh";

/// The origins 0.10's pages ran under, as the web views write them (no trailing slash).
/// `http://tauri.localhost` is WebView2's; `https://tauri.localhost` is what WebView2 would have
/// used had 0.10 ever turned on Tauri's `useHttpsScheme` (it didn't); `tauri://localhost` is
/// WebKit's on macOS and Linux.
pub const APP_ORIGINS: [&str; 3] = ["http://tauri.localhost", "https://tauri.localhost", "tauri://localhost"];

/// The file tauri-plugin-window-state keeps the window's size and place in.
pub const WINDOW_STATE_FILE: &str = ".window-state.json";

/// The folder, in 1.0's own data folder, that stores are copied into to be read (see
/// [`Places::scratch`]). The search for stores never goes into it.
pub const SCRATCH_FOLDER: &str = "import-scratch";

/// Where 0.10.x may have left its web view storage and its window state.
///
/// [`Places::this_computer`] fills it in for the system 1.0 runs on. The per-system
/// constructors take the base folders as arguments, so they work (and are tested) anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    /// Folders a web view store may be under, most likely first. Each is searched a few levels
    /// deep (see `scan::MAX_DEPTH`); a missing one is skipped.
    pub web_roots: Vec<PathBuf>,
    /// Where `.window-state.json` may be, most likely first. The first one there is read.
    pub window_state: Vec<PathBuf>,
    /// The folder the stores are copied into to be read, made (owner-only) if missing. Each
    /// reading makes a folder of its own inside and deletes it when done, and copies a reading
    /// left behind (the process was killed in the middle) are deleted by the next search. The
    /// copies can hold secrets typed into snippets, so this should be private to the user:
    /// [`Places::this_computer`] picks [`SCRATCH_FOLDER`] in 1.0's own data folder, which on
    /// Windows is under `%LOCALAPPDATA%` and only the user (and administrators) can open.
    ///
    /// `None`, as the per-system constructors leave it so that a test pointed at fixtures never
    /// writes beside them, uses the system's temporary folder. That is private on macOS and
    /// Linux (the copies are made 0700), but on Windows only as private as `%TEMP%` happens to
    /// be.
    pub scratch: Option<PathBuf>,
}

impl Places {
    /// The places on the computer this runs on, from the same `dirs` lookups Tauri used
    /// (`XDG_DATA_HOME` and `XDG_CONFIG_HOME` are honoured on Linux, as they were by 0.10).
    /// Empty when the system's folders can't be found.
    pub fn this_computer() -> Places {
        // Tauri's `app_local_data_dir`, 1.0's own data folder on every system.
        let scratch = dirs::data_local_dir().map(|d| d.join(APP_IDENTIFIER).join(SCRATCH_FOLDER));
        Places { scratch, ..Places::for_this_system() }
    }

    fn for_this_system() -> Places {
        #[cfg(target_os = "windows")]
        {
            match (dirs::data_local_dir(), dirs::config_dir()) {
                (Some(local), Some(roaming)) => Places::windows(&local, &roaming),
                _ => Places::default(),
            }
        }
        #[cfg(target_os = "macos")]
        {
            match dirs::home_dir() {
                Some(home) => Places::macos(&home),
                None => Places::default(),
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            match (dirs::data_local_dir(), dirs::config_dir()) {
                (Some(data), Some(config)) => Places::linux(&data, &config),
                _ => Places::default(),
            }
        }
    }

    /// Windows: `local_app_data` is `%LOCALAPPDATA%` (where WebView2's folder is),
    /// `roaming_app_data` is `%APPDATA%` (where the window state is).
    pub fn windows(local_app_data: &Path, roaming_app_data: &Path) -> Places {
        Places {
            web_roots: vec![local_app_data.join(APP_IDENTIFIER).join("EBWebView")],
            window_state: vec![roaming_app_data.join(APP_IDENTIFIER).join(WINDOW_STATE_FILE)],
            scratch: None,
        }
    }

    /// macOS, from the home folder. Besides the bundle's own WebKit folder this looks where a
    /// sandboxed build would have kept it, and where a build run outside its bundle (a developer
    /// running the binary straight from `target/`) did: WebKit names the folder after the
    /// process then. 0.10 releases were neither, so those come last.
    pub fn macos(home: &Path) -> Places {
        let library = home.join("Library");
        Places {
            web_roots: vec![
                library.join("WebKit").join(APP_IDENTIFIER),
                library.join("Containers").join(APP_IDENTIFIER).join("Data").join("Library").join("WebKit"),
                library.join("WebKit").join("ssheesh"),
            ],
            window_state: vec![library
                .join("Application Support")
                .join(APP_IDENTIFIER)
                .join(WINDOW_STATE_FILE)],
            scratch: None,
        }
    }

    /// Linux: `data_home` is `$XDG_DATA_HOME` (usually `~/.local/share`), `config_home` is
    /// `$XDG_CONFIG_HOME` (usually `~/.config`).
    pub fn linux(data_home: &Path, config_home: &Path) -> Places {
        Places {
            web_roots: vec![data_home.join(APP_IDENTIFIER)],
            window_state: vec![config_home.join(APP_IDENTIFIER).join(WINDOW_STATE_FILE)],
            scratch: None,
        }
    }
}

/// Whether `origin` is one 0.10's released app ran under (see [`APP_ORIGINS`]). A developer's
/// `http://localhost:1420` is not.
pub fn is_app_origin(origin: &str) -> bool {
    APP_ORIGINS.contains(&origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_system_points_at_the_folders_tauri_used() {
        let w = Places::windows(
            Path::new("C:/Users/ann/AppData/Local"),
            Path::new("C:/Users/ann/AppData/Roaming"),
        );
        assert_eq!(
            w.web_roots,
            vec![PathBuf::from("C:/Users/ann/AppData/Local/io.github.footprintsntrails.ssheesh/EBWebView")]
        );
        assert_eq!(
            w.window_state,
            vec![PathBuf::from(
                "C:/Users/ann/AppData/Roaming/io.github.footprintsntrails.ssheesh/.window-state.json"
            )]
        );

        let m = Places::macos(Path::new("/Users/ann"));
        assert_eq!(
            m.web_roots[0],
            PathBuf::from("/Users/ann/Library/WebKit/io.github.footprintsntrails.ssheesh")
        );
        assert_eq!(
            m.window_state,
            vec![PathBuf::from(
                "/Users/ann/Library/Application Support/io.github.footprintsntrails.ssheesh/.window-state.json"
            )]
        );

        let l = Places::linux(Path::new("/home/ann/.local/share"), Path::new("/home/ann/.config"));
        assert_eq!(
            l.web_roots,
            vec![PathBuf::from("/home/ann/.local/share/io.github.footprintsntrails.ssheesh")]
        );
        assert_eq!(
            l.window_state,
            vec![PathBuf::from("/home/ann/.config/io.github.footprintsntrails.ssheesh/.window-state.json")]
        );
    }

    #[test]
    fn only_the_released_apps_origins_count_as_the_app() {
        assert!(is_app_origin("http://tauri.localhost"));
        assert!(is_app_origin("tauri://localhost"));
        assert!(!is_app_origin("http://localhost:1420"));
        assert!(!is_app_origin("http://tauri.localhost/"));
    }
}
