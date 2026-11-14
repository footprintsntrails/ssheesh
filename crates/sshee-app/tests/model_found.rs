//! `import_found`: what reading 0.10's web view storage couldn't settle reaches the import as
//! doubts, so privacy goes on, and a value that couldn't be decoded is unreadable, not missing.

mod model_support;

use model_support::*;
use sshee_app::import::{import_found, ImportOptions, Outcome};
use sshee_app::legacy::{self, Found, LegacyError, Places, WebStore};
use sshee_app::model::{Json, Labels, Platform, Settings, Store, Workspace};
use sshee_app::raw::RawSource;
use std::fs;
use std::path::{Path, PathBuf};

const ID: &str = "io.github.footprintsntrails.ssheesh";

fn linux() -> ImportOptions {
    ImportOptions { platform: Platform::Linux, tor_on: false }
}

/// The day-of-use stores with streamer mode off, so turning it on shows.
fn in_use() -> Json {
    let mut stores = get(&fixture("ts-in-use.json"), &["stores"]).clone();
    get_mut(&mut stores, &[Settings::KEY, "state"]).insert("streamerMode".into(), Json::Bool(false));
    stores
}

/// A store as `legacy::find` gives it: from the released app, read while it held still.
fn found_with(stores: &Json) -> Found {
    let raw = raw_from(stores);
    Found {
        stores: vec![WebStore {
            source: RawSource::WebKitSqlite(PathBuf::from("/nonexistent/tauri_localhost_0.localstorage")),
            origin: Some("tauri://localhost".into()),
            modified: None,
            settled: true,
            stores: raw.stores,
            unreadable_keys: vec![],
        }],
        ..Found::default()
    }
}

#[test]
fn a_clean_reading_is_no_doubt() {
    let imported = import_found(&found_with(&in_use()), &linux());
    assert_eq!(imported.report.privacy, None);
    assert!(!imported.state.settings.streamer_mode);
}

#[test]
fn a_value_that_could_not_be_decoded_is_unreadable_and_not_written_over() {
    let mut stores = in_use();
    let Json::Object(map) = &mut stores else { unreachable!() };
    map.shift_remove(Labels::KEY);
    let mut found = found_with(&stores);
    found.stores[0].unreadable_keys = vec![Labels::KEY.into()];
    let imported = import_found(&found, &linux());
    let labels = imported.report.store(Labels::KEY).unwrap();
    assert_eq!(labels.outcome, Outcome::Unreadable { why: "couldn't be decoded".into() });
    let privacy = imported.report.privacy.as_ref().unwrap();
    assert_eq!(privacy.reasons, ["sshee-labels couldn't be read (couldn't be decoded)"]);
    assert_eq!(privacy.turned_on, ["streamerMode"]);
    assert!(imported.state.settings.streamer_mode);
    // 0.10's own web view may read what this code couldn't: nothing goes back over it.
    let back = imported.state.to_web_stores();
    assert!(!back.contains_key(Labels::KEY));
    assert_eq!(back.len(), 6);
    assert!(imported.report.to_string().contains("sshee-labels: unreadable (couldn't be decoded), defaults."));
}

/// Makes a clean reading less so.
type Spoil = fn(&mut Found);

#[test]
fn a_reading_that_was_not_clean_is_a_doubt() {
    let cases: [(&str, Spoil); 5] = [
        ("0.10 was writing to its storage while it was read", |f| f.stores[0].settled = false),
        ("1 web view storage(s) had damage that reading went past", |f| {
            let path = PathBuf::from("/nonexistent/leveldb");
            f.problems.push(LegacyError::ReadPastDamage { path, detail: "/nonexistent/000004.log at 32768".into() });
        }),
        ("the storage found is a developer build's", |f| f.stores[0].origin = Some("http://localhost:1420".into())),
        ("the storage's origin couldn't be read", |f| f.stores[0].origin = None),
        ("1 web view storage(s) couldn't be read", |f| {
            let path = PathBuf::from("/nonexistent/leveldb");
            f.problems.push(LegacyError::Damaged { path, detail: "not a store".into() });
        }),
    ];
    for (reason, spoil) in cases {
        let mut found = found_with(&in_use());
        spoil(&mut found);
        assert!(!found.is_complete(), "{reason}");
        let imported = import_found(&found, &ImportOptions { platform: Platform::Linux, tor_on: true });
        let privacy = imported.report.privacy.clone().unwrap_or_else(|| panic!("{reason}"));
        assert_eq!(privacy.reasons, [reason]);
        let s = &imported.state.settings;
        assert!(s.streamer_mode && s.hide_secrets && s.tor_hide_typing, "{reason}");
        // The reasons are shown: no path, which would name the user.
        assert!(!imported.report.to_string().contains("nonexistent"), "{reason}");
    }
}

#[test]
fn nothing_found_is_defaults_and_private() {
    let mut found = Found::default();
    found.problems.push(LegacyError::TooBig { path: PathBuf::from("/nonexistent/x"), bytes: 1 << 30 });
    let imported = import_found(&found, &linux());
    assert!(imported.report.stores.iter().all(|s| s.outcome == Outcome::Missing));
    let privacy = imported.report.privacy.unwrap();
    assert_eq!(privacy.reasons, ["1 web view storage(s) couldn't be read", "no settings were found", "no tabs were found"]);
}

/// A WebKit localStorage database exactly as `SQLiteStorageArea` makes one (as in
/// `legacy_damage.rs`).
fn webkit_db(path: &Path, items: &[(&str, Vec<u8>)]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL ON CONFLICT FAIL);",
    )
    .unwrap();
    for (key, value) in items {
        conn.execute("INSERT INTO ItemTable VALUES (?1, ?2)", rusqlite::params![key, value]).unwrap();
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn a_value_legacy_could_not_decode_turns_privacy_on_end_to_end() {
    // `legacy_damage`'s case: the labels' value is an odd number of bytes, not UTF-16.
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let stores = in_use();
    let text = |key: &str| utf16(&get(&stores, &[key]).to_text());
    webkit_db(
        &data.join(ID).join("localstorage/tauri_localhost_0.localstorage"),
        &[
            (Settings::KEY, text(Settings::KEY)),
            (Workspace::KEY, text(Workspace::KEY)),
            (Labels::KEY, vec![0x7b, 0x00, 0x22]),
        ],
    );
    let found = legacy::find(&Places::linux(&data, tmp.path()));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(found.best().unwrap().unreadable_keys, [Labels::KEY]);
    let imported = import_found(&found, &linux());
    assert!(matches!(imported.report.store(Labels::KEY).unwrap().outcome, Outcome::Unreadable { .. }));
    assert!(imported.state.settings.streamer_mode);
    let report = imported.report.to_string();
    assert!(!report.contains("Privacy settings came over as they were."), "{report}");
    assert_eq!(imported.report.sessions.len(), 8);
}
