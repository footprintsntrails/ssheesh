//! Chromium's localStorage, as WebView2 keeps it on Windows.
//!
//! Two formats, both read from Chromium's own source (`components/services/storage/
//! dom_storage/`) and checked against stores a real Chrome (Chrome for Testing 158) wrote:
//!
//! - **LevelDB**, in `<profile>/Local Storage/leveldb/`. One record per localStorage item, its
//!   key `'_' + origin + 0x00 + item key` (`local_storage_leveldb.cc`, `GetMapPrefix`), the
//!   origin written as `http://tauri.localhost` (no trailing slash). Other keys there
//!   (`VERSION`, `META:<origin>`, `METAACCESS:<origin>`) are bookkeeping and skipped.
//! - **SQLite**, in one file `<profile>/LocalStorage` (`sqlite/local_storage_sqlite.cc`), the
//!   format Chromium is moving to (Chrome 158 already uses it for new profiles). Table `maps`
//!   names each origin's map (`storage_key` = `http://tauri.localhost/`, with a slash), table
//!   `map_entries` holds `(map_id, value_compression_type, key, value)`. Values of 350 bytes or
//!   more may be compressed: type 1 is zstd, type 2 Snappy, 0 none (`public/cpp/compression.h`).
//!
//! In both, item keys and values are Blink's encoding of a JavaScript string: a first byte 0
//! means UTF-16 little-endian follows, 1 means Latin-1 (one byte per character, U+0000 to
//! U+00FF), which Blink picks whenever every character fits (`cached_storage_area.cc`,
//! `StringToUint8Vector`).

use super::copy::{copy_settled, copy_sqlite, files_in, Copied, Scratch};
use super::{
    decode_utf16le, leveldb, open_sqlite_copy, LegacyError, OriginRecords, StoreRead, MAX_VALUE_BYTES,
};
use crate::raw::STORE_KEYS;
use rusqlite::OptionalExtension;
use rusty_leveldb::LdbIterator;
use std::collections::BTreeMap;
use std::path::Path;

/// Reads the LevelDB store in `dir` (a `Local Storage/leveldb` folder) from a copy in `scratch`.
///
/// A folder with none of LevelDB's database files in it is no store at all, the way Chromium
/// sees it too (`dom_storage_rollout.cc`, `CheckOnDiskLevelDbState`): a WebView2 that moved to
/// SQLite can leave the empty folder, or one with only `LOCK` and `LOG`, behind.
pub(crate) fn read_leveldb(dir: &Path, scratch: &Scratch) -> Result<StoreRead, LegacyError> {
    let copy = scratch.path().join("leveldb");
    let keep = |name: &str| leveldb::is_database_file(name);
    let listed = files_in(dir, &keep).map_err(|error| LegacyError::Io { path: dir.to_path_buf(), error })?;
    if listed.is_empty() {
        let copied = Copied { modified: None, settled: true };
        return Ok(StoreRead { records: Vec::new(), copied, damage: Vec::new() });
    }
    let copied = copy_settled(dir, &|| files_in(dir, &keep), &copy)?;
    // What the library would step over in silence, or choke on, is looked for first.
    let damage = leveldb::check(&copy).map_err(|detail| LegacyError::Damaged {
        path: dir.to_path_buf(),
        detail: format!("LevelDB: {detail}"),
    })?;
    let records = read_leveldb_copy(dir, &copy)?;
    Ok(StoreRead { records, copied, damage })
}

fn read_leveldb_copy(original: &Path, copy: &Path) -> Result<Vec<OriginRecords>, LegacyError> {
    let damaged = |detail: String| LegacyError::Damaged { path: original.to_path_buf(), detail };
    let options =
        rusty_leveldb::Options { create_if_missing: false, paranoid_checks: false, ..Default::default() };
    // Opening replays the log into memory and may tidy files: all of it in the copy, which
    // `leveldb::check` has been through, so only files in it are opened.
    let mut db = rusty_leveldb::DB::open(copy, options).map_err(|e| damaged(format!("LevelDB: {e}")))?;
    let mut iter = db.new_iter().map_err(|e| damaged(format!("LevelDB: {e}")))?;

    let mut by_origin: BTreeMap<String, OriginRecords> = BTreeMap::new();
    while let Some((key, value)) = LdbIterator::next(&mut iter) {
        let Some((origin, item)) = split_map_key(&key) else { continue };
        // Only the item key is decoded to decide; the value of anything not ours is never looked at.
        let Some(name) = store_key(item) else { continue };
        let entry = by_origin
            .entry(origin.clone())
            .or_insert_with(|| OriginRecords { origin: Some(origin), ..Default::default() });
        match (value.len() <= MAX_VALUE_BYTES).then(|| decode_blink_string(&value)).flatten() {
            Some(text) => {
                entry.stores.insert(name.to_owned(), text);
            }
            None => entry.unreadable_keys.push(name.to_owned()),
        }
    }
    Ok(by_origin.into_values().collect())
}

/// Splits a LevelDB key `'_' + origin + 0x00 + item key` into the origin and the item key.
/// `None` for the bookkeeping keys, and for the partitioned keys of embedded third-party
/// frames (their origin part has `^` in it), which 0.10's own page never had.
pub(crate) fn split_map_key(key: &[u8]) -> Option<(String, &[u8])> {
    let rest = key.strip_prefix(b"_")?;
    let separator = rest.iter().position(|&b| b == 0)?;
    let origin = std::str::from_utf8(&rest[..separator]).ok()?;
    if origin.contains('^') {
        return None;
    }
    Some((origin.to_owned(), &rest[separator + 1..]))
}

/// Reads the SQLite store `file` (a profile's `LocalStorage`) from a copy in `scratch`.
pub(crate) fn read_sqlite(file: &Path, scratch: &Scratch) -> Result<StoreRead, LegacyError> {
    let copy_dir = scratch.path().join("chromium-sqlite");
    let copied = copy_sqlite(file, &copy_dir)?;
    let name = file.file_name().unwrap_or_default();
    let records = read_sqlite_copy(file, &copy_dir.join(name))?;
    Ok(StoreRead { records, copied, damage: Vec::new() })
}

fn read_sqlite_copy(original: &Path, copy: &Path) -> Result<Vec<OriginRecords>, LegacyError> {
    let sql_err = |e: rusqlite::Error| LegacyError::Damaged {
        path: original.to_path_buf(),
        detail: format!("SQLite: {e}"),
    };
    let conn = open_sqlite_copy(original, copy)?;

    let maps: Vec<(i64, Vec<u8>)> = {
        let mut stmt = conn.prepare("SELECT row_id, storage_key FROM maps").map_err(sql_err)?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(sql_err)?;
        rows.collect::<Result<_, _>>().map_err(sql_err)?
    };

    let mut stmt = conn
        .prepare("SELECT value_compression_type, value FROM map_entries WHERE map_id = ?1 AND key = ?2")
        .map_err(sql_err)?;
    let mut out = Vec::new();
    for (map_id, storage_key) in maps {
        // `http://tauri.localhost/`: an origin with its slash. Partitioned keys have `^` parts.
        let Ok(storage_key) = String::from_utf8(storage_key) else { continue };
        if storage_key.contains('^') {
            continue;
        }
        let mut records = OriginRecords {
            origin: Some(storage_key.trim_end_matches('/').to_owned()),
            ..Default::default()
        };
        for name in STORE_KEYS {
            // Blink writes these ASCII keys as Latin-1; a UTF-16 copy is looked for too.
            for key in [encode_latin1(name), encode_utf16(name)] {
                let row: Option<(i64, Vec<u8>)> = stmt
                    .query_row(rusqlite::params![map_id, key], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()
                    .map_err(sql_err)?;
                let Some((compression, value)) = row else { continue };
                match decompress(compression, value).and_then(|v| decode_blink_string(&v)) {
                    Some(text) => {
                        records.stores.insert(name.to_owned(), text);
                    }
                    None => records.unreadable_keys.push(name.to_owned()),
                }
                break;
            }
        }
        out.push(records);
    }
    Ok(out)
}

/// A value from `map_entries`, uncompressed. `None` for an unknown compression or data that
/// doesn't decompress, or would grow past [`MAX_VALUE_BYTES`].
pub(crate) fn decompress(kind: i64, data: Vec<u8>) -> Option<Vec<u8>> {
    match kind {
        0 => (data.len() <= MAX_VALUE_BYTES).then_some(data),
        1 => zstd::bulk::decompress(&data, MAX_VALUE_BYTES).ok(),
        2 => {
            let size = snap::raw::decompress_len(&data).ok()?;
            if size > MAX_VALUE_BYTES {
                return None;
            }
            snap::raw::Decoder::new().decompress_vec(&data).ok()
        }
        _ => None,
    }
}

/// Which of 0.10's keys an encoded item key is, if any.
fn store_key(encoded: &[u8]) -> Option<&'static str> {
    // Keys are short; anything long is someone else's and not worth decoding.
    if encoded.len() > 64 {
        return None;
    }
    let key = decode_blink_string(encoded)?;
    STORE_KEYS.iter().copied().find(|k| *k == key)
}

/// A string as Blink stores it in Chromium's database: format byte, then the characters.
pub(crate) fn decode_blink_string(bytes: &[u8]) -> Option<String> {
    let (&format, rest) = bytes.split_first()?;
    match format {
        0 => decode_utf16le(rest),
        1 => Some(rest.iter().map(|&b| char::from(b)).collect()),
        _ => None,
    }
}

fn encode_latin1(ascii: &str) -> Vec<u8> {
    let mut out = vec![1];
    out.extend_from_slice(ascii.as_bytes());
    out
}

fn encode_utf16(text: &str) -> Vec<u8> {
    let mut out = vec![0];
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blink_strings_decode_from_both_forms() {
        assert_eq!(decode_blink_string(b"\x01sshee-settings").as_deref(), Some("sshee-settings"));
        // Latin-1 is not UTF-8: one byte, 0xE9, is "é".
        assert_eq!(decode_blink_string(b"\x01Caf\xe9").as_deref(), Some("Café"));
        let utf16: Vec<u8> =
            std::iter::once(0).chain("Токен ☕".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode_blink_string(&utf16).as_deref(), Some("Токен ☕"));
        assert_eq!(decode_blink_string(b"\x00abc"), None, "odd UTF-16 is damage");
        assert_eq!(decode_blink_string(b"\x07abc"), None, "unknown format");
        assert_eq!(decode_blink_string(b""), None);
    }

    #[test]
    fn leveldb_keys_split_into_origin_and_item() {
        let (origin, item) = split_map_key(b"_http://tauri.localhost\x00\x01sshee-workspace").unwrap();
        assert_eq!(origin, "http://tauri.localhost");
        assert_eq!(store_key(item), Some("sshee-workspace"));
        assert!(split_map_key(b"META:http://tauri.localhost").is_none());
        assert!(split_map_key(b"VERSION").is_none());
        assert!(split_map_key(b"_https://a.example/^0https://b.example\x00\x01k").is_none());
        assert_eq!(store_key(b"\x01not-ours"), None);
        assert_eq!(store_key(&encode_utf16("sshee-labels")), Some("sshee-labels"));
    }

    #[test]
    fn values_decompress_within_limits() {
        let text = b"{\"state\":{},\"version\":4}".repeat(40);
        let z = zstd::bulk::compress(&text, 3).unwrap();
        assert_eq!(decompress(1, z).as_deref(), Some(&text[..]));
        let s = snap::raw::Encoder::new().compress_vec(&text).unwrap();
        assert_eq!(decompress(2, s).as_deref(), Some(&text[..]));
        assert_eq!(decompress(0, text.clone()).as_deref(), Some(&text[..]));
        assert_eq!(decompress(9, text), None);
        assert_eq!(decompress(1, b"not zstd".to_vec()), None);
        // A small bomb: 64 MiB of zeros in a few kilobytes. It must not be unpacked.
        let bomb = zstd::bulk::compress(&vec![0u8; 64 << 20], 3).unwrap();
        assert_eq!(decompress(1, bomb), None);
    }
}
