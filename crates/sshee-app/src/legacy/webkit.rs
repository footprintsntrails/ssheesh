//! WebKit's localStorage, as WKWebView (macOS) and WebKitGTK (Linux) keep it.
//!
//! One SQLite database per origin with one table, `ItemTable(key TEXT UNIQUE ON CONFLICT
//! REPLACE, value BLOB NOT NULL ON CONFLICT FAIL)`; the value is the string's UTF-16 code
//! units, little-endian, with no prefix (`NetworkProcess/storage/SQLiteStorageArea.cpp`, which
//! binds values with `bindBlob(String)`). The database runs in WAL mode, and while the app is
//! open its records are mostly in the `-wal` file: in the store a running 0.10.14 left on Linux,
//! the database itself held only the empty table.
//!
//! Checked against stores the real 0.10.14 wrote under WebKitGTK 2.52.6 (see the fixtures).

use super::copy::{copy_sqlite, Copied, Scratch};
use super::{decode_utf16le, open_sqlite_copy, LegacyError, OriginRecords, MAX_VALUE_BYTES};
use crate::raw::STORE_KEYS;
use rusqlite::types::ValueRef;
use rusqlite::OptionalExtension;
use std::path::Path;

/// Reads the WebKit store `file` from a copy in `scratch`. `origin` is what its location says
/// it belongs to (see `scan`).
pub(crate) fn read(
    file: &Path,
    origin: Option<String>,
    scratch: &Scratch,
) -> Result<(OriginRecords, Copied), LegacyError> {
    let copy_dir = scratch.path().join("webkit");
    let copied = copy_sqlite(file, &copy_dir)?;
    let name = file.file_name().unwrap_or_default();
    let sql_err = |e: rusqlite::Error| LegacyError::Damaged {
        path: file.to_path_buf(),
        detail: format!("SQLite: {e}"),
    };

    let conn = open_sqlite_copy(file, &copy_dir.join(name))?;
    let mut stmt = conn.prepare("SELECT value FROM ItemTable WHERE key = ?1").map_err(sql_err)?;
    let mut records = OriginRecords { origin, ..Default::default() };
    for key in STORE_KEYS {
        let value = stmt
            .query_row([key], |row| {
                Ok(match row.get_ref(0)? {
                    ValueRef::Blob(b) if b.len() <= MAX_VALUE_BYTES => decode_utf16le(b),
                    // Not something WebKit writes; taken as it is if it ever turns up.
                    ValueRef::Text(t) if t.len() <= MAX_VALUE_BYTES => String::from_utf8(t.to_vec()).ok(),
                    _ => None,
                })
            })
            .optional()
            .map_err(sql_err)?;
        match value {
            None => {}
            Some(Some(text)) => {
                records.stores.insert(key.to_owned(), text);
            }
            Some(None) => records.unreadable_keys.push(key.to_owned()),
        }
    }
    Ok((records, copied))
}
