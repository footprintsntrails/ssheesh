//! Finding the web view stores below the [`Places`](super::Places) roots.
//!
//! Folder names below the roots differ between web view versions (WebKit's hashed per-origin
//! folders, Chromium's move from LevelDB to SQLite), so nothing below a root is assumed. Each
//! root is walked a few levels deep and every file or folder that looks like a localStorage
//! store is listed. Whether it holds 0.10's records is decided later, by reading a copy.
//!
//! The walk only looks at names: it opens no file except WebKit's tiny `origin` file (and that
//! only if it is a regular file, without waiting on it), never follows a symbolic link, skips
//! the web views' big cache folders and the folder the copies are made in, and stops after
//! [`MAX_ENTRIES`] entries, so a strange folder can't keep it busy.

use super::copy::open_file;
use super::places::SCRATCH_FOLDER;
use crate::raw::RawSource;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// How deep below a root a store may be. The deepest known one is newer WebKit's
/// `WebsiteData/Default/<hash>/<hash>/LocalStorage/localstorage.sqlite3`: six levels.
pub(crate) const MAX_DEPTH: usize = 7;

/// How many folder entries one search looks at, at most.
pub(crate) const MAX_ENTRIES: usize = 50_000;

/// Folders that hold caches and other storage, never localStorage. Skipped to keep the walk
/// short. Chromium's `Session Storage` is LevelDB too, but holds sessionStorage. The copies
/// being read ([`SCRATCH_FOLDER`]) are skipped so they aren't taken for stores: on Linux 1.0's
/// data folder, where they are made, is also where 0.10's web view kept its own.
const SKIPPED_FOLDERS: [&str; 14] = [
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "GraphiteDawnCache",
    "ShaderCache",
    "Crashpad",
    "Service Worker",
    "IndexedDB",
    "Session Storage",
    "CacheStorage",
    SCRATCH_FOLDER,
];

/// Where the stores are in every web view version read so far, relative to a root. They are
/// looked at first, so a profile big enough to use up the walk's budget can't hide them.
const KNOWN_SPOTS: [&str; 5] = [
    "Default/Local Storage/leveldb",
    "Default/LocalStorage",
    "localstorage",
    "WebsiteData/LocalStorage",
    "WebsiteData/Default",
];

/// Every localStorage store below `roots`: first those at the [`KNOWN_SPOTS`], then any other
/// the walk finds, each once, in the order found (roots in order, then by name).
pub(crate) fn find_candidates(roots: &[PathBuf]) -> Vec<RawSource> {
    let mut found = Vec::new();
    let mut budget = MAX_ENTRIES;
    for root in roots {
        for spot in KNOWN_SPOTS {
            let path = root.join(spot);
            let Ok(meta) = fs::symlink_metadata(&path) else { continue };
            if meta.is_file() {
                found.extend(classify_file(&path, &file_name(&path)));
            } else if meta.is_dir() && spot.ends_with("leveldb") {
                found.push(RawSource::WebView2LevelDb(path));
            } else if meta.is_dir() {
                // A folder of per-origin stores: a short walk of its own.
                walk(&path, MAX_DEPTH - 4, &mut budget, &mut found);
            }
        }
    }
    for root in roots {
        walk(root, 0, &mut budget, &mut found);
    }
    // A handful of stores at most, so a plain search is fine.
    let mut unique: Vec<RawSource> = Vec::with_capacity(found.len());
    for source in found {
        if !unique.contains(&source) {
            unique.push(source);
        }
    }
    unique
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn walk(dir: &Path, depth: usize, budget: &mut usize, found: &mut Vec<RawSource>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        // `file_type` doesn't follow symbolic links: a link is neither a file nor a folder here.
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            if name == "leveldb" && parent_name(&path).as_deref() == Some("Local Storage") {
                found.push(RawSource::WebView2LevelDb(path));
            } else if !SKIPPED_FOLDERS.contains(&name.as_str()) {
                walk(&path, depth + 1, budget, found);
            }
        } else if kind.is_file() {
            if let Some(candidate) = classify_file(&path, &name) {
                found.push(candidate);
            }
        }
    }
}

/// What a file is, by its name: Chromium's SQLite localStorage is one file named exactly
/// `LocalStorage`; WebKit's are `<origin id>.localstorage` (older) and
/// `LocalStorage/localstorage.sqlite3` (newer).
fn classify_file(path: &Path, name: &str) -> Option<RawSource> {
    if name == "LocalStorage" {
        Some(RawSource::WebView2Sqlite(path.to_path_buf()))
    } else if name.ends_with(".localstorage") || name == "localstorage.sqlite3" {
        Some(RawSource::WebKitSqlite(path.to_path_buf()))
    } else {
        None
    }
}

/// The origin a WebKit store's location names: older stores by their file name, newer ones by
/// the `origin` file in the folder above their `LocalStorage` folder.
pub(crate) fn webkit_origin(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    if let Some(id) = name.strip_suffix(".localstorage") {
        return origin_from_database_identifier(id);
    }
    origin_from_origin_file(&path.parent()?.parent()?.join("origin"))
}

fn parent_name(path: &Path) -> Option<String> {
    path.parent()?.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// WebKit's "database identifier" of an origin, `<scheme>_<host>_<port>` with port 0 for
/// none, back to the origin: `tauri_localhost_0` is `tauri://localhost`,
/// `http_localhost_1420` is `http://localhost:1420`.
pub(crate) fn origin_from_database_identifier(id: &str) -> Option<String> {
    let (scheme, rest) = id.split_once('_')?;
    let (host, port) = rest.rsplit_once('_')?;
    if scheme.is_empty() || host.is_empty() || port.parse::<u16>().is_err() {
        return None;
    }
    Some(if port == "0" { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{port}") })
}

/// WebKit writes each origin's folder an `origin` file (`WebCore::StorageUtilities::
/// writeOriginToFile`): the top-level origin, then the origin, each as scheme, host and port in
/// WebKit's persistent encoding. A string is a 32-bit little-endian length, a byte that is 1
/// for 8-bit characters (0 for UTF-16), then the characters; the port is a presence byte and,
/// when present, a 16-bit little-endian number. WebKitGTK 2.52 wrote exactly that for
/// `tauri://localhost` (see the Linux fixture). Should a later WebKit change the layout, a file
/// that still names `tauri` and `localhost` counts as 0.10's; anything else stays unknown.
pub(crate) fn origin_from_origin_file(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    open_file(path, false).ok()?.take(4096).read_to_end(&mut bytes).ok()?;
    if let Some(origin) = decode_origin_file(&bytes) {
        return Some(origin);
    }
    let names = |word: &str| {
        let utf16: Vec<u8> = word.encode_utf16().flat_map(u16::to_le_bytes).collect();
        contains(&bytes, word.as_bytes()) || contains(&bytes, &utf16)
    };
    (names("tauri") && names("localhost")).then(|| "tauri://localhost".to_owned())
}

/// Decodes the two origins of an `origin` file and returns the second (the one whose data
/// the folder holds). `None` unless the whole file is exactly two origins.
pub(crate) fn decode_origin_file(bytes: &[u8]) -> Option<String> {
    let mut rest = bytes;
    let _top = read_origin(&mut rest)?;
    let origin = read_origin(&mut rest)?;
    rest.is_empty().then_some(origin)
}

fn read_origin(rest: &mut &[u8]) -> Option<String> {
    let scheme = read_string(rest)?;
    let host = read_string(rest)?;
    let port = match take(rest, 1)?[0] {
        0 => None,
        1 => {
            let b = take(rest, 2)?;
            Some(u16::from_le_bytes([b[0], b[1]]))
        }
        _ => return None,
    };
    if scheme.is_empty() {
        return None;
    }
    Some(match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

fn read_string(rest: &mut &[u8]) -> Option<String> {
    let len = take(rest, 4)?;
    let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
    if len > 1024 {
        return None;
    }
    match take(rest, 1)?[0] {
        1 => Some(take(rest, len)?.iter().map(|&b| char::from(b)).collect()),
        0 => {
            let units = take(rest, len.checked_mul(2)?)?;
            let units = units.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair));
            char::decode_utf16(units).collect::<Result<String, _>>().ok()
        }
        _ => None,
    }
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if rest.len() < n {
        return None;
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Some(head)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webkit_database_identifiers_name_their_origin() {
        assert_eq!(
            origin_from_database_identifier("tauri_localhost_0").as_deref(),
            Some("tauri://localhost")
        );
        assert_eq!(
            origin_from_database_identifier("http_localhost_1420").as_deref(),
            Some("http://localhost:1420")
        );
        assert_eq!(
            origin_from_database_identifier("https_web_7731.example_8443").as_deref(),
            Some("https://web_7731.example:8443")
        );
        assert_eq!(origin_from_database_identifier("tauri"), None);
        assert_eq!(origin_from_database_identifier("tauri_localhost_x"), None);
    }

    #[test]
    fn files_are_told_apart_by_name() {
        let c = classify_file(Path::new("/x/Default/LocalStorage"), "LocalStorage").unwrap();
        assert_eq!(c, RawSource::WebView2Sqlite(PathBuf::from("/x/Default/LocalStorage")));
        let p = Path::new("/x/localstorage/tauri_localhost_0.localstorage");
        assert_eq!(
            classify_file(p, "tauri_localhost_0.localstorage"),
            Some(RawSource::WebKitSqlite(p.to_path_buf()))
        );
        assert_eq!(webkit_origin(p).as_deref(), Some("tauri://localhost"));
        assert!(classify_file(Path::new("/x/LocalStorage-wal"), "LocalStorage-wal").is_none());
        assert!(classify_file(Path::new("/x/a.localstorage-wal"), "a.localstorage-wal").is_none());
        assert!(classify_file(Path::new("/x/LocalStorage.Migrating"), "LocalStorage.Migrating").is_none());
    }

    #[test]
    fn origin_files_decode() {
        // What WebKitGTK 2.52.6 wrote for 0.10.14's page, byte for byte.
        let real =
            b"\x05\0\0\0\x01tauri\x09\0\0\0\x01localhost\0\x05\0\0\0\x01tauri\x09\0\0\0\x01localhost\0";
        assert_eq!(decode_origin_file(real).as_deref(), Some("tauri://localhost"));
        let mut dev = Vec::new();
        for _ in 0..2 {
            dev.extend_from_slice(b"\x04\0\0\0\x01http\x09\0\0\0\x01localhost\x01");
            dev.extend_from_slice(&1420u16.to_le_bytes());
        }
        assert_eq!(decode_origin_file(&dev).as_deref(), Some("http://localhost:1420"));
        assert_eq!(decode_origin_file(&real[..20]), None);
    }
}
