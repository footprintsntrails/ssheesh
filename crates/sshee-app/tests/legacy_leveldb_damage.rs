//! Damage in a LevelDB store is never read past in silence. The LevelDB library skips a table
//! block or a log record that fails its checksum without a word, so `legacy` checks every copy
//! itself first. Each test flips bits in a copy of the real Chromium fixture and holds every
//! answer to one rule: the store is refused, or the damage is named (`ReadPastDamage`, and the
//! import is doubtful), or what was read is exactly what an undamaged store gives (or, for the
//! last record of a log, what a write cut short there gives).
//!
//! The flips are spread out to keep the run short; `LEGACY_FLIP_STRIDE=1` tries every byte.

use sshee_app::legacy::{self, LegacyError, Places};
use sshee_app::raw::RawSource;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const ID: &str = "io.github.footprintsntrails.ssheesh";

fn leveldb_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/legacy/windows-leveldb/Local")
        .join(ID)
        .join("EBWebView/Default/Local Storage/leveldb")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

/// Every `n`th byte, unless `LEGACY_FLIP_STRIDE` says otherwise.
fn stride(default: usize) -> usize {
    std::env::var("LEGACY_FLIP_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

/// What reading a LevelDB folder gave: the app origin's records, or why it was refused.
#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Read(BTreeMap<String, String>),
    Damage(String),
    Refused(String),
}

fn read(dir: &Path) -> Outcome {
    match legacy::read_store(&RawSource::WebView2LevelDb(dir.to_path_buf())) {
        Ok(stores) => {
            let app = stores.into_iter().find(|s| s.is_app_origin());
            Outcome::Read(app.map(|s| s.stores).unwrap_or_default())
        }
        Err(LegacyError::ReadPastDamage { detail, .. }) => Outcome::Damage(detail),
        Err(LegacyError::ReaderFailed { detail, .. }) => panic!("the library panicked: {detail}"),
        Err(e) => Outcome::Refused(e.to_string()),
    }
}

/// Where each record of a log (or manifest) starts. The fixture's are shorter than one block.
fn record_starts(log: &[u8]) -> Vec<usize> {
    assert!(log.len() < 32 * 1024);
    let mut starts = Vec::new();
    let mut at = 0;
    while at + 7 <= log.len() {
        starts.push(at);
        at += 7 + usize::from(u16::from_le_bytes([log[at + 4], log[at + 5]]));
    }
    assert_eq!(at, log.len(), "the fixture's log ends on a whole record");
    starts
}

/// Flips one bit of `name` at each offset in `offsets` and holds the outcome to the rule in the
/// module notes. `silent_ok(at)` is what reading may give, damage unnoticed, after a flip at
/// `at`. Returns how many flips were refused, noted as damage and read the same.
fn flip_each(
    name: &str,
    offsets: impl Iterator<Item = usize>,
    silent_ok: &dyn Fn(usize) -> Outcome,
) -> [usize; 3] {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("leveldb");
    copy_dir(&leveldb_fixture(), &dir);
    let whole = fs::read(dir.join(name)).unwrap();
    let mut counts = [0; 3];
    for at in offsets {
        let mut bytes = whole.clone();
        bytes[at] ^= 1 << (at % 8);
        fs::write(dir.join(name), &bytes).unwrap();
        match read(&dir) {
            Outcome::Refused(_) => counts[0] += 1,
            Outcome::Damage(_) => counts[1] += 1,
            read => {
                assert_eq!(
                    read,
                    silent_ok(at),
                    "{name}: a flip at byte {at} went unnoticed and changed what was read"
                );
                counts[2] += 1;
            }
        }
    }
    fs::write(dir.join(name), &whole).unwrap();
    counts
}

/// What a store gives with `name` cut short at each of `cuts`.
fn cut_outcomes(name: &str, cuts: &[usize]) -> Vec<Outcome> {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("leveldb");
    copy_dir(&leveldb_fixture(), &dir);
    let whole = fs::read(dir.join(name)).unwrap();
    cuts.iter()
        .map(|&cut| {
            fs::write(dir.join(name), &whole[..cut]).unwrap();
            read(&dir)
        })
        .collect()
}

fn baseline() -> Outcome {
    let found = read(&leveldb_fixture());
    match &found {
        Outcome::Read(stores) => assert_eq!(stores.len(), 6),
        other => panic!("{other:?}"),
    }
    found
}

#[test]
fn damage_in_a_table_is_never_read_past_in_silence() {
    let table = fs::read(leveldb_fixture().join("000003.ldb")).unwrap();
    // The block with the app's records (the reviewer's seven offsets were in it), closely;
    // the rest of the table, sparsely.
    let app = table.windows(23).position(|w| w == b"_http://tauri.localhost").unwrap();
    let near = (app.saturating_sub(1500)..(app + 1500).min(table.len())).step_by(stride(37));
    let all = (0..table.len()).step_by(stride(1999));
    let whole = baseline();
    let counts = flip_each("000003.ldb", near.chain(all), &|_| whole.clone());
    eprintln!("table flips: {} refused, {} damage noted, {} read the same", counts[0], counts[1], counts[2]);
    assert!(counts[1] > 0, "flips in data blocks are noted as damage");
    assert!(counts[0] > 0, "flips in the index or footer refuse the table");
}

#[test]
fn damage_in_the_log_is_never_read_past_in_silence() {
    let log = fs::read(leveldb_fixture().join("000004.log")).unwrap();
    let starts = record_starts(&log);
    let cuts = cut_outcomes("000004.log", &starts);
    // Every record's header, and its data sparsely.
    let headers = starts.iter().flat_map(|&s| s..s + 7);
    let data = (0..log.len()).step_by(stride(13));
    let counts = flip_each("000004.log", headers.chain(data), &|at| {
        let record = starts.iter().rposition(|&s| s <= at).unwrap();
        // Unnoticed is only fine in the last record, and only as if it were cut short there.
        assert_eq!(record, starts.len() - 1, "a flip at byte {at}, in record {record}, went unnoticed");
        assert!(matches!(cuts[record], Outcome::Read(_)), "{:?}", cuts[record]);
        cuts[record].clone()
    });
    eprintln!(
        "log flips: {} refused, {} damage noted, {} read as cut short",
        counts[0], counts[1], counts[2]
    );
    assert!(counts[1] > 0);
}

#[test]
fn damage_in_the_manifest_refuses_the_store() {
    let manifest = fs::read(leveldb_fixture().join("MANIFEST-000001")).unwrap();
    let starts = record_starts(&manifest);
    let cuts = cut_outcomes("MANIFEST-000001", &starts);
    let counts = flip_each("MANIFEST-000001", (0..manifest.len()).step_by(stride(1)), &|at| {
        let record = starts.iter().rposition(|&s| s <= at).unwrap();
        assert_eq!(record, starts.len() - 1, "a flip at byte {at}, in record {record}, went unnoticed");
        assert!(
            matches!(cuts[record], Outcome::Read(_)),
            "a flip at {at} was read, where a cut there gives {:?}",
            cuts[record]
        );
        cuts[record].clone()
    });
    eprintln!(
        "manifest flips: {} refused, {} damage noted, {} read as cut short",
        counts[0], counts[1], counts[2]
    );
    assert!(counts[0] > 0);
}

/// The reviewer's case: one bit at byte 20 of the log, in the first of its records. The library
/// stops at it and drops every later write: the deleted typing counts come back, and a terminal
/// leaves the workspace.
#[test]
fn a_damaged_record_early_in_the_log_makes_the_import_doubtful() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("Local");
    let dir = local.join(ID).join("EBWebView/Default/Local Storage/leveldb");
    copy_dir(&leveldb_fixture(), &dir);
    let mut log = fs::read(dir.join("000004.log")).unwrap();
    log[20] ^= 0x10;
    fs::write(dir.join("000004.log"), &log).unwrap();

    let found = legacy::find(&Places::windows(&local, tmp.path()));
    // What could be read is there, from the table; the damage is a problem beside it.
    let best = found.best().unwrap();
    assert!(best.stores.contains_key("sshee-settings"));
    match &found.problems[..] {
        [LegacyError::ReadPastDamage { path, detail }] => {
            assert_eq!(path, &dir);
            assert_eq!(detail, "000004.log: the record at byte 0 fails its checksum");
        }
        other => panic!("{other:?}"),
    }
    assert!(!found.is_complete(), "a store read past damage is a doubt");
    // The damage is named by file and place, never by a value.
    assert!(!format!("{:?}", found.problems).contains("m3c8v0p6rd"));
}

#[test]
fn a_table_claiming_a_terabyte_block_is_refused_without_reading_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("leveldb");
    copy_dir(&leveldb_fixture(), &dir);
    let mut table = fs::read(dir.join("000003.ldb")).unwrap();
    let footer = table.len() - 48;
    // The footer's handles carry no checksum: keep the meta-index's, make the index 2^40 long.
    let mut at = footer;
    for _ in 0..3 {
        while table[at] & 0x80 != 0 {
            at += 1;
        }
        at += 1;
    }
    let mut handles = table[footer..at].to_vec();
    let mut size: u64 = 1 << 40;
    while size >= 0x80 {
        handles.push(size as u8 | 0x80);
        size >>= 7;
    }
    handles.push(size as u8);
    handles.resize(40, 0);
    table[footer..footer + 40].copy_from_slice(&handles);
    fs::write(dir.join("000003.ldb"), &table).unwrap();
    // Before the check, this aborted the whole process ("memory allocation ... failed").
    match read(&dir) {
        Outcome::Refused(why) => assert!(why.contains("outside the file"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// A manifest record with a good checksum can still say anything: here, a comparator name 2^40
/// bytes long, which the library would allocate before reading.
#[test]
fn a_manifest_claiming_a_huge_field_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("leveldb");
    copy_dir(&leveldb_fixture(), &dir);
    let edit = [1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x20, b'x'];
    let crc = crc32c::crc32c_append(crc32c::crc32c(&[1]), &edit);
    let masked = crc.rotate_right(15).wrapping_add(0xa282_ead8);
    let mut record = masked.to_le_bytes().to_vec();
    record.extend_from_slice(&(edit.len() as u16).to_le_bytes());
    record.push(1);
    record.extend_from_slice(&edit);
    let mut manifest = fs::read(dir.join("MANIFEST-000001")).unwrap();
    manifest.extend_from_slice(&record);
    fs::write(dir.join("MANIFEST-000001"), &manifest).unwrap();
    match read(&dir) {
        Outcome::Refused(why) => assert!(why.contains("version edit"), "{why}"),
        other => panic!("{other:?}"),
    }
}
