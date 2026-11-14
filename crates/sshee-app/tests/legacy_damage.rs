//! `legacy` on stores that are damaged, half-written, doubled up or not stores at all. Each
//! case starts from a copy of a real fixture (or a WebKit store built here in WebKit's exact
//! format) in a temporary folder.

use sshee_app::legacy::{self, LegacyError, Places};
use sshee_app::raw::RawSource;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const ID: &str = "io.github.footprintsntrails.ssheesh";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fx").join(name)
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn leveldb_dir(local: &Path) -> PathBuf {
    local.join(ID).join("EBWebView/Default/Local Storage/leveldb")
}

fn set_mtime(path: &Path, when: SystemTime) {
    fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
}

/// A tiny deterministic random number generator, so damage is the same on every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// A WebKit localStorage database exactly as `SQLiteStorageArea` makes one.
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
fn a_half_written_leveldb_log_keeps_what_was_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    copy_tree(&fixture("windows-leveldb/Local"), &local);
    let log = leveldb_dir(&local).join("000004.log");
    let whole = fs::read(&log).unwrap();
    for cut in (0..whole.len()).step_by(61) {
        fs::write(&log, &whole[..cut]).unwrap();
        let found = legacy::find(&Places::windows(&local, tmp.path()));
        assert!(found.problems.is_empty(), "cut at {cut}: {:?}", found.problems);
        let best = found.best().unwrap_or_else(|| panic!("cut at {cut}: nothing found"));
        // Whatever the log lost, the table still has the first session's records.
        assert!(best.stores.contains_key("sshee-settings"), "cut at {cut}");
        let workspace = &best.stores["sshee-workspace"];
        assert!(workspace.contains("q8x2m4k7za"), "cut at {cut}");
        assert!(serde_json::from_str::<serde_json::Value>(workspace).is_ok(), "cut at {cut}");
    }
}

#[test]
fn a_leveldb_missing_its_table_is_a_problem_not_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    copy_tree(&fixture("windows-leveldb/Local"), &local);
    fs::remove_file(leveldb_dir(&local).join("000003.ldb")).unwrap();
    let found = legacy::find(&Places::windows(&local, tmp.path()));
    assert!(found.stores.is_empty());
    assert_eq!(found.problems.len(), 1);
    assert!(matches!(&found.problems[0], LegacyError::Damaged { path, .. } if path == &leveldb_dir(&local)));
    assert!(!found.is_complete());
}

#[test]
fn garbage_where_a_store_should_be_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    let profile = local.join(ID).join("EBWebView/Default");
    fs::create_dir_all(profile.join("Local Storage/leveldb")).unwrap();
    fs::write(profile.join("Local Storage/leveldb/CURRENT"), b"MANIFEST-000009\n").unwrap();
    fs::write(profile.join("Local Storage/leveldb/MANIFEST-000009"), [0x5a; 300]).unwrap();
    fs::write(profile.join("LocalStorage"), b"this is not a database at all, just words").unwrap();
    let found = legacy::find(&Places::windows(&local, tmp.path()));
    assert!(found.stores.is_empty());
    assert_eq!(found.problems.len(), 2, "{:?}", found.problems);

    let data = tmp.path().join("data");
    fs::create_dir_all(data.join(ID).join("localstorage")).unwrap();
    fs::write(data.join(ID).join("localstorage/tauri_localhost_0.localstorage"), [0u8; 4096]).unwrap();
    let found = legacy::find(&Places::linux(&data, tmp.path()));
    assert!(found.stores.is_empty());
    assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
    assert!(found.problems[0].to_string().contains("tauri_localhost_0.localstorage"));
}

#[test]
fn flipped_bytes_in_any_store_never_panic() {
    let stores = [
        (
            "windows-leveldb",
            "Local/io.github.footprintsntrails.ssheesh/EBWebView/Default/Local Storage/leveldb",
        ),
        ("windows-sqlite-running", "Local/io.github.footprintsntrails.ssheesh/EBWebView/Default"),
        ("linux-webkitgtk-running", "data/io.github.footprintsntrails.ssheesh/localstorage"),
    ];
    // More rounds for a longer hunt: LEGACY_FLIP_ROUNDS=2000 cargo test ... flipped_bytes
    let rounds: usize = std::env::var("LEGACY_FLIP_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(25);
    let mut rng = Lcg(7);
    let mut outcomes = [0usize; 4];
    for (name, dir) in stores {
        for _ in 0..rounds {
            let tmp = tempfile::tempdir().unwrap();
            let copy = tmp.path().join("store");
            copy_tree(&fixture(name).join(dir), &copy);
            let mut files: Vec<_> = fs::read_dir(&copy).unwrap().map(|e| e.unwrap().path()).collect();
            files.sort();
            let victim = &files[(rng.next() as usize) % files.len()];
            let mut bytes = fs::read(victim).unwrap();
            if bytes.is_empty() {
                continue;
            }
            for _ in 0..(1 + rng.next() % 8) {
                let at = (rng.next() as usize) % bytes.len();
                bytes[at] ^= 1 << (rng.next() % 8);
            }
            fs::write(victim, &bytes).unwrap();
            let source = if name.starts_with("windows-leveldb") {
                RawSource::WebView2LevelDb(copy.clone())
            } else if name.starts_with("windows-sqlite") {
                RawSource::WebView2Sqlite(copy.join("LocalStorage"))
            } else {
                RawSource::WebKitSqlite(copy.join("tauri_localhost_0.localstorage"))
            };
            // Any answer is fine; returning at all is the test. Whether damage that was read
            // past is always named is `legacy_leveldb_damage`'s question.
            match legacy::read_store(&source) {
                Ok(_) => outcomes[0] += 1,
                Err(LegacyError::ReadPastDamage { .. }) => outcomes[1] += 1,
                Err(LegacyError::ReaderFailed { .. }) => outcomes[2] += 1,
                Err(_) => outcomes[3] += 1,
            }
        }
    }
    eprintln!(
        "flipped bytes: {} read, {} read with damage named, {} reader panics caught, {} reported damaged",
        outcomes[0], outcomes[1], outcomes[2], outcomes[3]
    );
}

#[test]
fn a_dev_server_store_loses_to_the_released_apps() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let dir = data.join(ID).join("localstorage");
    fs::create_dir_all(&dir).unwrap();
    let real = fixture("linux-webkitgtk/data").join(ID).join("localstorage/tauri_localhost_0.localstorage");
    fs::copy(&real, dir.join("tauri_localhost_0.localstorage")).unwrap();
    fs::copy(&real, dir.join("http_localhost_1420.localstorage")).unwrap();
    let now = SystemTime::now();
    set_mtime(&dir.join("tauri_localhost_0.localstorage"), now - Duration::from_secs(86_400));
    set_mtime(&dir.join("http_localhost_1420.localstorage"), now);
    let found = legacy::find(&Places::linux(&data, tmp.path()));
    assert_eq!(found.stores.len(), 2);
    assert_eq!(found.stores[0].origin.as_deref(), Some("tauri://localhost"));
    assert_eq!(found.stores[1].origin.as_deref(), Some("http://localhost:1420"));
    assert!(!found.stores[1].is_app_origin());
}

#[test]
fn the_store_still_being_written_wins_when_a_profile_has_both_formats() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    copy_tree(&fixture("windows-leveldb/Local"), &local);
    let sqlite = local.join(ID).join("EBWebView/Default/LocalStorage");
    fs::copy(fixture("windows-sqlite/Local").join(ID).join("EBWebView/Default/LocalStorage"), &sqlite)
        .unwrap();

    let now = SystemTime::now();
    let age = |path: &Path, secs: u64| set_mtime(path, now - Duration::from_secs(secs));
    let leveldb_files: Vec<_> =
        fs::read_dir(leveldb_dir(&local)).unwrap().map(|e| e.unwrap().path()).collect();
    for f in &leveldb_files {
        age(f, 60);
    }
    age(&sqlite, 3600);
    let found = legacy::find(&Places::windows(&local, tmp.path()));
    assert_eq!(found.stores.len(), 2);
    assert!(matches!(found.stores[0].source, RawSource::WebView2LevelDb(_)));

    for f in &leveldb_files {
        age(f, 7200);
    }
    let found = legacy::find(&Places::windows(&local, tmp.path()));
    assert!(matches!(found.stores[0].source, RawSource::WebView2Sqlite(_)));
}

#[test]
fn only_0_10s_keys_are_taken_and_a_bad_value_is_named() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let db = data.join(ID).join("localstorage/tauri_localhost_0.localstorage");
    let settings = r#"{"state":{"welcomeNote":"🦀 web-7731"},"version":4}"#;
    webkit_db(
        &db,
        &[
            ("sshee-settings", utf16(settings)),
            ("sshee-labels", vec![0x7b, 0x00, 0x22]), // odd length: not UTF-16
            ("sshee-settings-backup", utf16("{}")),
            ("not-ours", utf16("a value nobody here should read")),
        ],
    );
    let found = legacy::find(&Places::linux(&data, tmp.path()));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let best = found.best().unwrap();
    assert_eq!(best.stores.len(), 1);
    assert_eq!(best.stores["sshee-settings"], settings);
    assert_eq!(best.unreadable_keys, ["sshee-labels"]);
    assert!(!found.is_complete(), "a value that couldn't be read is a doubt");
}

#[test]
fn a_store_without_0_10s_keys_is_not_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    webkit_db(
        &data.join(ID).join("localstorage/tauri_localhost_0.localstorage"),
        &[("not-ours", utf16("x"))],
    );
    let found = legacy::find(&Places::linux(&data, tmp.path()));
    assert!(found.stores.is_empty() && found.problems.is_empty());
}

#[test]
fn window_state_cases() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing.json");
    let folder = tmp.path().join("folder.json");
    fs::create_dir(&folder).unwrap();
    let good = tmp.path().join("good.json");
    fs::write(&good, r#"{"main":{"width":1320}}"#).unwrap();
    let bad = tmp.path().join("bad.json");
    fs::write(&bad, [0xff, 0xfe, 0x00]).unwrap();

    assert_eq!(legacy::read_window_state(&[]).unwrap(), None);
    assert_eq!(legacy::read_window_state(&[missing.clone(), folder.clone()]).unwrap(), None);
    assert_eq!(
        legacy::read_window_state(&[missing, folder, good, bad.clone()]).unwrap().as_deref(),
        Some(r#"{"main":{"width":1320}}"#)
    );
    assert!(matches!(
        legacy::read_window_state(std::slice::from_ref(&bad)),
        Err(LegacyError::Damaged { .. })
    ));

    // Through `find`: a broken window state is named, and doesn't make the settings doubtful.
    let found = legacy::find(&Places {
        web_roots: vec![fixture("linux-webkitgtk/data").join(ID)],
        window_state: vec![bad],
        ..Places::default()
    });
    assert!(found.problems.is_empty());
    assert!(matches!(found.window_state_problem, Some(LegacyError::Damaged { .. })));
    assert!(found.is_complete());
}

#[cfg(unix)]
#[test]
fn links_in_a_store_folder_are_not_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    copy_tree(&fixture("windows-leveldb/Local"), &local);
    // Were it followed, this would be read until the size limit.
    std::os::unix::fs::symlink("/dev/zero", leveldb_dir(&local).join("000009.ldb")).unwrap();
    std::os::unix::fs::symlink(fixture("linux-webkitgtk/data"), local.join(ID).join("elsewhere")).unwrap();
    let found = legacy::find(&Places::windows(&local, tmp.path()));
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert_eq!(found.stores.len(), 1, "the linked WebKit folder isn't searched");
    assert_eq!(found.stores[0].origin.as_deref(), Some("http://tauri.localhost"));
}

#[test]
fn a_fixture_source_has_nothing_to_read() {
    assert!(legacy::read_store(&RawSource::Fixture).unwrap().is_empty());
}

/// Chromium takes a `leveldb` folder with no database files in it as no database, and uses
/// SQLite (`dom_storage_rollout.cc`). A WebView2 that moved over can leave such a folder.
#[test]
fn an_empty_leveldb_folder_beside_a_sqlite_store_is_no_store() {
    for leftovers in [&[][..], &["LOCK"], &["LOCK", "LOG", "LOG.old"]] {
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("Local");
        copy_tree(&fixture("windows-sqlite/Local"), &local);
        let leveldb = leveldb_dir(&local);
        fs::create_dir_all(&leveldb).unwrap();
        for name in leftovers {
            fs::write(leveldb.join(name), b"").unwrap();
        }
        let found = legacy::find(&Places::windows(&local, tmp.path()));
        assert!(found.problems.is_empty(), "{leftovers:?}: {:?}", found.problems);
        assert_eq!(found.stores.len(), 1);
        assert!(matches!(found.stores[0].source, RawSource::WebView2Sqlite(_)));
        assert!(found.is_complete(), "{leftovers:?}");
    }
    // A log with no CURRENT to say what it belongs to is still a damaged store.
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    copy_tree(&fixture("windows-leveldb/Local"), &local);
    fs::remove_file(leveldb_dir(&local).join("CURRENT")).unwrap();
    let found = legacy::find(&Places::windows(&local, tmp.path()));
    assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
    assert!(found.problems[0].to_string().contains("CURRENT"));
}

/// `CURRENT` names the manifest, and the library opens whatever path it names. Anything but a
/// manifest in the folder itself is refused before the library sees it.
#[test]
fn a_current_file_naming_anything_else_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    copy_tree(&fixture("windows-leveldb/Local"), &local);
    let current = leveldb_dir(&local).join("CURRENT");
    let elsewhere = tmp.path().join("MANIFEST-000001");
    fs::copy(leveldb_dir(&local).join("MANIFEST-000001"), &elsewhere).unwrap();
    for text in [
        format!("{}\n", elsewhere.display()),
        "../MANIFEST-000001\n".to_owned(),
        "MANIFEST-000001\r\n".to_owned(),
        "MANIFEST-000001".to_owned(),
        "MANIFEST-000002\n".to_owned(),
        "LOCK\n".to_owned(),
    ] {
        fs::write(&current, &text).unwrap();
        let found = legacy::find(&Places::windows(&local, tmp.path()));
        assert!(found.stores.is_empty(), "{text:?}");
        assert!(
            matches!(&found.problems[..], [LegacyError::Damaged { detail, .. }] if detail.contains("CURRENT")),
            "{text:?}: {:?}",
            found.problems
        );
    }
}

/// Runs `find` on a thread and gives up after a while, so a test of something that used to
/// hang fails instead of hanging.
#[cfg(unix)]
fn find_within(places: Places, seconds: u64) -> legacy::Found {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(legacy::find(&places)).unwrap());
    rx.recv_timeout(Duration::from_secs(seconds)).expect("find is still waiting on something")
}

#[cfg(unix)]
fn mkfifo(path: &Path) {
    let made = std::process::Command::new("mkfifo").arg(path).status().unwrap();
    assert!(made.success());
}

/// A FIFO opens only once something writes to it, so opening one as a file waits, maybe for
/// ever. None is ever opened that way: not WebKit's `origin` file, not a manifest that
/// `CURRENT` names outside the copy.
#[cfg(unix)]
#[test]
fn a_fifo_where_a_file_should_be_is_not_waited_on() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let origin_dir = data.join(ID).join("storage/h1/h1");
    let real = fixture("linux-webkitgtk/data").join(ID).join("localstorage/tauri_localhost_0.localstorage");
    fs::create_dir_all(origin_dir.join("LocalStorage")).unwrap();
    fs::copy(&real, origin_dir.join("LocalStorage/localstorage.sqlite3")).unwrap();
    mkfifo(&origin_dir.join("origin"));
    let found = find_within(Places::linux(&data, tmp.path()), 20);
    assert_eq!(found.stores.len(), 1);
    assert_eq!(found.stores[0].origin, None, "an origin file that isn't a file names nothing");

    let local = tmp.path().join("Local");
    copy_tree(&fixture("windows-leveldb/Local"), &local);
    let fifo = tmp.path().join("outside-fifo");
    mkfifo(&fifo);
    fs::write(leveldb_dir(&local).join("CURRENT"), format!("{}\n", fifo.display())).unwrap();
    let found = find_within(Places::windows(&local, tmp.path()), 20);
    assert_eq!(found.problems.len(), 1, "{:?}", found.problems);

    // The window state may be a link (a dotfile manager's), but not to a FIFO.
    let state = tmp.path().join("state-fifo");
    mkfifo(&state);
    let places = Places { window_state: vec![state], ..Places::default() };
    let found = find_within(places, 20);
    assert!(found.window_state.is_none());
}

/// SQLite rolls a hot rollback journal back on first read, and a journal can name a
/// "super-journal" anywhere on the disk, which SQLite then opens and may delete. So a journal
/// is never copied; one beside the store only marks the copy as taken mid-write.
#[test]
fn a_rollback_journal_beside_a_store_can_reach_nothing_outside() {
    let tmp = tempfile::tempdir().unwrap();
    let victim = tmp.path().join("notes-of-the-user.txt");
    fs::write(&victim, "a file that belongs to no store").unwrap();

    // A rollback-mode store with a hot journal: a copy taken while a write was spilling.
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    let db = work.join("tauri_localhost_0.localstorage");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = DELETE;
             CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL ON CONFLICT FAIL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ItemTable VALUES ('sshee-settings', ?1)",
            [utf16(r#"{"state":{},"version":4}"#)],
        )
        .unwrap();
        for i in 0..50 {
            conn.execute("INSERT INTO ItemTable VALUES (?1, zeroblob(4000))", [format!("pad{i}")]).unwrap();
        }
    }
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("PRAGMA cache_size = 2; BEGIN IMMEDIATE; UPDATE ItemTable SET value = zeroblob(4100) WHERE key LIKE 'pad%';")
        .unwrap();
    let store = tmp.path().join("data").join(ID).join("localstorage");
    fs::create_dir_all(&store).unwrap();
    fs::copy(&db, store.join("tauri_localhost_0.localstorage")).unwrap();
    let mut journal = fs::read(work.join("tauri_localhost_0.localstorage-journal")).unwrap();
    drop(conn);
    // The super-journal record SQLite looks for at a journal's end: page number, name, length,
    // checksum, magic.
    let name = victim.to_str().unwrap().as_bytes();
    journal.extend_from_slice(&((1u32 << 30) / 4096 + 1).to_be_bytes());
    journal.extend_from_slice(name);
    journal.extend_from_slice(&(name.len() as u32).to_be_bytes());
    let sum = name.iter().fold(0u32, |sum, &b| sum.wrapping_add(b as i8 as i32 as u32));
    journal.extend_from_slice(&sum.to_be_bytes());
    journal.extend_from_slice(&[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]);
    fs::write(store.join("tauri_localhost_0.localstorage-journal"), &journal).unwrap();

    let found = legacy::find(&Places::linux(&tmp.path().join("data"), tmp.path()));
    assert!(victim.exists(), "reading a store deleted a file outside it");
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let best = found.best().unwrap();
    assert!(best.stores.contains_key("sshee-settings"));
    assert!(!best.settled, "a store with a write under way isn't settled");
    assert!(!found.is_complete());
}

/// Copies a reading left behind (the process was killed mid-read) are deleted by the next
/// search; a reading going on at the same moment, whose folder is fresh, is left alone.
#[cfg(unix)]
#[test]
fn copies_left_behind_are_swept_away() {
    let tmp = tempfile::tempdir().unwrap();
    let scratch = tmp.path().join("app-data").join(legacy::SCRATCH_FOLDER);
    let stale = scratch.join("ssheesh-0.10-left");
    let fresh = scratch.join("ssheesh-0.10-busy");
    let other = scratch.join("not-ours");
    for dir in [&stale, &fresh, &other] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("LocalStorage"), "tok_made_up_4f7a1c9e2b").unwrap();
    }
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    for dir in [&stale, &other] {
        fs::File::open(dir).unwrap().set_modified(hour_ago).unwrap();
    }

    let mut places = Places::linux(&fixture("linux-webkitgtk/data"), &fixture("linux-webkitgtk/config"));
    places.scratch = Some(scratch.clone());
    let found = legacy::find(&places);
    assert!(found.is_complete(), "{:?}", found.problems);
    assert!(!stale.exists(), "a leftover copy is deleted");
    assert!(fresh.exists(), "a copy being read right now is left alone");
    assert!(other.exists(), "only folders named like ours are touched");
    // This search's own copy was made there and is gone again.
    let mut names: Vec<_> = fs::read_dir(&scratch).unwrap().map(|e| e.unwrap().file_name()).collect();
    names.sort();
    assert_eq!(names, ["not-ours", "ssheesh-0.10-busy"]);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(fs::metadata(&scratch).unwrap().permissions().mode() & 0o777, 0o700);
}

/// The fixtures are the web views' files byte for byte, whatever a checkout's line endings are
/// (their `.gitattributes`): LevelDB's `CURRENT` is plain text, and with a CR in it the library
/// looks for a manifest that isn't there.
#[test]
fn fixtures_keep_their_line_endings() {
    let current = fs::read(leveldb_dir(&fixture("windows-leveldb/Local")).join("CURRENT")).unwrap();
    assert_eq!(current, b"MANIFEST-000001\n");
    let attributes = fs::read_to_string(fixture(".gitattributes")).unwrap();
    assert!(attributes.lines().any(|l| l.trim() == "* -text"), "{attributes}");
}
