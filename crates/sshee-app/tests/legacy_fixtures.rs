//! `legacy` against stores real web views wrote (see `fixtures/legacy/README.md` for how each
//! was made): the real 0.10.14 under WebKitGTK, Chrome's LevelDB and SQLite localStorage, and
//! WebKit's newer and older macOS layouts built from the WebKitGTK files.

use serde_json::Value;
use sshee_app::legacy::{self, Found, Places};
use sshee_app::raw::{RawSource, STORE_KEYS};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy").join(name)
}

fn linux(name: &str) -> Places {
    Places::linux(&fixture(name).join("data"), &fixture(name).join("config"))
}

fn windows(name: &str) -> Places {
    Places::windows(&fixture(name).join("Local"), &fixture(name).join("Roaming"))
}

fn macos(name: &str) -> Places {
    Places::macos(&fixture(name))
}

/// Every file under `dir`, with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                stack.push(path);
            } else {
                out.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    out
}

/// Runs `find`, checking it left the fixture byte for byte as it was, with no file added
/// (no LevelDB `LOCK`, no SQLite `-shm` or `-journal`).
fn find_untouched(name: &str, places: &Places) -> Found {
    let before = snapshot(&fixture(name));
    let found = legacy::find(places);
    assert_eq!(snapshot(&fixture(name)), before, "{name}: the originals must not change");
    found
}

fn json(found: &Found, key: &str) -> Value {
    let text = &found.best().unwrap().stores[key];
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{key} isn't JSON: {e}"))
}

/// Every terminal's tmux session name in a workspace record.
fn session_keys(workspace: &Value) -> Vec<String> {
    let mut keys: Vec<String> = workspace["state"]["panes"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|p| p["sessionKey"].as_str().map(str::to_owned))
        .collect();
    keys.sort();
    keys
}

/// Each value is what zustand's `persist` writes: `{"state": {...}, "version": n}`.
fn assert_zustand_shaped(found: &Found) {
    for (key, text) in &found.best().unwrap().stores {
        let v: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{key}: {e}"));
        assert!(v["state"].is_object(), "{key} has a state");
        assert!(v["version"].is_u64(), "{key} has a version");
    }
}

#[test]
fn linux_the_real_0_10_14_store_is_found_and_read() {
    let found = find_untouched("linux-webkitgtk", &linux("linux-webkitgtk"));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(found.stores.len(), 1);
    let best = found.best().unwrap();
    assert_eq!(best.origin.as_deref(), Some("tauri://localhost"));
    assert!(best.is_app_origin() && best.settled && best.unreadable_keys.is_empty());
    match &best.source {
        RawSource::WebKitSqlite(p) => assert!(p.ends_with(
            "io.github.footprintsntrails.ssheesh/localstorage/tauri_localhost_0.localstorage"
        )),
        other => panic!("{other:?}"),
    }
    assert_eq!(best.stores.keys().map(String::as_str).collect::<Vec<_>>(), {
        let mut all = STORE_KEYS.to_vec();
        all.sort();
        all
    });
    assert_zustand_shaped(&found);

    let settings = json(&found, "sshee-settings");
    assert_eq!(settings["version"], 4);
    assert_eq!(settings["state"]["updateMode"], "manual");
    assert_eq!(settings["state"]["streamerMode"], true);
    assert_eq!(settings["state"]["theme"], "midnight");
    assert_eq!(settings["state"]["welcomeNote"], "Сервер базы — только чтение. Café ☕");
    let snippets = json(&found, "sshee-snippets");
    assert_eq!(snippets["state"]["snippets"][1]["name"], "Токен деплоя");

    // Two terminals on servers from the seeded layout, two local ones the app opened itself.
    let workspace = json(&found, "sshee-workspace");
    assert_eq!(session_keys(&workspace), ["9ruv7f27vr", "q8x2m4k7za", "s9vfdo66z2", "t5n1b9w3ce"]);

    // 0.10.14 closed under Xvfb with no window manager wrote an empty window state.
    assert_eq!(found.window_state.as_deref(), Some("{}"));
    let raw = found.raw().unwrap();
    assert_eq!(raw.stores, best.stores);
    assert_eq!(raw.source, best.source);
    assert!(found.is_complete());
}

#[test]
fn linux_a_store_copied_while_the_app_ran_has_its_newest_records_in_the_wal() {
    let found = find_untouched("linux-webkitgtk-running", &linux("linux-webkitgtk-running"));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let best = found.best().unwrap();
    assert_eq!(best.stores.len(), 7);
    let workspace = json(&found, "sshee-workspace");
    assert_eq!(session_keys(&workspace), ["9ruv7f27vr", "q8x2m4k7za", "s9vfdo66z2", "t5n1b9w3ce"]);
    // No window state yet: 0.10 writes it when the window closes.
    assert_eq!(found.window_state, None);
}

#[test]
fn windows_leveldb_from_chrome_layers_table_log_and_deletion() {
    let found = find_untouched("windows-leveldb", &windows("windows-leveldb"));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    // `http://other.localhost` filled the same database with junk; it has none of 0.10's keys.
    assert_eq!(found.stores.len(), 1);
    let best = found.best().unwrap();
    assert_eq!(best.origin.as_deref(), Some("http://tauri.localhost"));
    assert!(matches!(&best.source, RawSource::WebView2LevelDb(p) if p.ends_with("EBWebView/Default/Local Storage/leveldb")));
    // Typing counts were removed in a later session: the deletion in the log wins over the table.
    let keys: Vec<_> = best.stores.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["sshee-bookmarks", "sshee-labels", "sshee-settings", "sshee-snippets", "sshee-tunnels", "sshee-workspace"]
    );
    assert_zustand_shaped(&found);
    // Latin-1 in Blink's one-byte form, Cyrillic in its UTF-16 form.
    assert_eq!(json(&found, "sshee-settings")["state"]["welcomeNote"], "Café on db-main ☕");
    assert_eq!(json(&found, "sshee-snippets")["state"]["snippets"][1]["name"], "Токен деплоя");
    // Tunnels were written twice in one session; the second one counts.
    assert_eq!(json(&found, "sshee-tunnels")["state"]["byHost"]["cfg:db-main"][0]["id"], "tn-pg");
    // Seeded (in the table), added later (in the log), added while Chrome was still open.
    let workspace = json(&found, "sshee-workspace");
    assert_eq!(session_keys(&workspace), ["m3c8v0p6rd", "q8x2m4k7za", "t5n1b9w3ce"]);
    assert!(!best.stores.values().any(|v| v.contains("never read")), "other keys are not taken");

    let state: Value = serde_json::from_str(found.window_state.as_deref().unwrap()).unwrap();
    assert_eq!(state["main"]["maximized"], true);
}

#[test]
fn windows_sqlite_from_chrome_with_zstd_values() {
    let found = find_untouched("windows-sqlite", &windows("windows-sqlite"));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(found.stores.len(), 1);
    let best = found.best().unwrap();
    assert_eq!(best.origin.as_deref(), Some("http://tauri.localhost"));
    assert!(matches!(&best.source, RawSource::WebView2Sqlite(p) if p.ends_with("EBWebView/Default/LocalStorage")));
    assert_eq!(best.stores.len(), 6, "typing counts were deleted");
    assert_zustand_shaped(&found);
    // Settings and workspace are over 350 bytes, so Chrome stored them zstd-compressed.
    assert_eq!(json(&found, "sshee-settings")["state"]["updateMode"], "manual");
    assert_eq!(session_keys(&json(&found, "sshee-workspace")), ["q8x2m4k7za", "t5n1b9w3ce"]);
    assert_eq!(json(&found, "sshee-snippets")["state"]["snippets"][1]["name"], "Токен деплоя");
    assert_eq!(found.window_state, None);
}

#[test]
fn windows_sqlite_copied_while_chrome_ran_reads_the_wal() {
    let found = find_untouched("windows-sqlite-running", &windows("windows-sqlite-running"));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let workspace = json(&found, "sshee-workspace");
    assert_eq!(session_keys(&workspace), ["m3c8v0p6rd", "q8x2m4k7za", "t5n1b9w3ce"]);
}

#[test]
fn macos_newer_webkit_layout_names_its_origin_in_the_origin_file() {
    let found = find_untouched("macos-webkit", &macos("macos-webkit"));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let best = found.best().unwrap();
    assert_eq!(best.origin.as_deref(), Some("tauri://localhost"));
    assert!(matches!(&best.source, RawSource::WebKitSqlite(p) if p.ends_with("LocalStorage/localstorage.sqlite3")));
    assert_eq!(best.stores.len(), 7);
    let state: Value = serde_json::from_str(found.window_state.as_deref().unwrap()).unwrap();
    assert_eq!(state["main"]["width"], 1320);
}

#[test]
fn macos_older_webkit_layout() {
    let found = find_untouched("macos-webkit-older", &macos("macos-webkit-older"));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let best = found.best().unwrap();
    assert_eq!(best.origin.as_deref(), Some("tauri://localhost"));
    assert_eq!(best.stores.len(), 7);
}

#[test]
fn nothing_there_is_nothing_found() {
    let empty = tempfile::tempdir().unwrap();
    for places in [
        Places::linux(empty.path(), empty.path()),
        Places::windows(empty.path(), empty.path()),
        Places::macos(empty.path()),
        Places::default(),
    ] {
        let found = legacy::find(&places);
        assert!(found.stores.is_empty() && found.problems.is_empty() && found.window_state.is_none());
        assert!(found.window_state_problem.is_none());
        assert!(found.raw().is_none());
        assert!(!found.is_complete());
    }
}

#[test]
fn debug_output_leaves_values_out() {
    let found = legacy::find(&linux("linux-webkitgtk"));
    let shown = format!("{:?}", found.best().unwrap());
    assert!(shown.contains("sshee-snippets"));
    assert!(!shown.contains("tok_made_up"), "a snippet's secret must not reach logs: {shown}");
}
