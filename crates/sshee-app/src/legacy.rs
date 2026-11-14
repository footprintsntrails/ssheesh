//! Finding and reading what 0.10.x left behind: the seven `sshee-*` records in its web view's
//! localStorage, and the window's size and place.
//!
//! 0.10 was a web page inside the system's web view, and its settings, server labels, tabs and
//! layout (with the tmux session of every open terminal), snippets, bookmarks, tunnels and
//! typing counts lived in that page's localStorage. 1.0 has no web view and no way to ask one,
//! so it opens the web view's own files. This is the main way a user's 0.10 setup gets into
//! 1.0: there is no 0.10 release that exports it first.
//!
//! Where the files are on each system is in [`Places`] (and `places.rs`, with the evidence);
//! below those folders nothing is assumed, and the stores are found by looking (`scan`). Three
//! formats are read: Chromium's LevelDB and SQLite (WebView2, Windows; `chromium`) and WebKit's
//! SQLite (macOS and Linux; `webkit`).
//!
//! Reading is careful, because the old app may still be running and the files are the user's:
//!
//! - Nothing is opened where it lies. Each store is copied, file by file, into a private
//!   folder ([`Places::scratch`]), and the copy is opened (`copy`). The originals are only
//!   read: never written, locked, renamed or deleted. Nothing a store holds can point the
//!   readers at a file outside the copy: LevelDB's `CURRENT` is checked, and SQLite's rollback
//!   journal (which can name another file to delete) is never copied.
//! - Only the seven keys of [`STORE_KEYS`](crate::raw::STORE_KEYS) are taken. Any other record
//!   in the store (another page's, or a key 0.10 didn't write) is skipped without decoding its
//!   value.
//! - A store that is locked, half-written, damaged or not a store at all ends up in
//!   [`Found::problems`] with the reason, and the search goes on. A LevelDB store with damage
//!   the library can read past (a block or a record that fails its checksum) is read, and the
//!   damage named among the problems as well ([`LegacyError::ReadPastDamage`]): the LevelDB
//!   library skips such damage in silence, so every copy is checked before it is opened
//!   (`leveldb`). Nothing here panics on bad
//!   input, and a panic inside the LevelDB library is caught and reported like the rest. No
//!   size a LevelDB file states is believed before it is checked against the file, so a file
//!   can't make the library allocate past memory and abort the process, which no catching
//!   could stop.
//! - Nothing waits on a file: a FIFO or a device where a store file should be is refused, not
//!   opened and waited on.
//! - Sizes are bounded: a store over 256 MB is not copied, a value over 32 MB is not decoded,
//!   a compressed value can't unpack past that.
//!
//! The result is raw: each value is the JSON string zustand wrote, decoded to UTF-8 and nothing
//! more. Parsing and migrating it is `import`'s job.

mod chromium;
mod copy;
mod leveldb;
mod places;
mod scan;
mod webkit;

pub use places::{is_app_origin, Places, APP_IDENTIFIER, APP_ORIGINS, SCRATCH_FOLDER, WINDOW_STATE_FILE};

use crate::raw::{RawSource, RawWebState};
use copy::{Copied, Scratch};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The most one value may be once decoded and decompressed. Chromium allows a whole origin
/// 10 MB, WebKit 5 MB, so a real 0.10 value is far smaller.
pub(crate) const MAX_VALUE_BYTES: usize = 32 << 20;

/// The most `.window-state.json` may be. Real ones are a few hundred bytes.
const MAX_WINDOW_STATE_BYTES: u64 = 1 << 20;

/// One origin's 0.10 records in one web view store.
#[derive(Clone, PartialEq, Eq)]
pub struct WebStore {
    /// The store, and which kind it is.
    pub source: RawSource,
    /// The page origin the records belong to, as the web view wrote it but without a trailing
    /// slash: `http://tauri.localhost` (WebView2) or `tauri://localhost` (WebKit) for 0.10's
    /// releases, `http://localhost:1420` for a developer's build. `None` when a newer WebKit
    /// store's `origin` file couldn't be read.
    pub origin: Option<String>,
    /// When the store last changed: the newest modification time among its files.
    pub modified: Option<SystemTime>,
    /// Whether the store held still while it was copied. `false` means the old app was writing
    /// to it at the time, and the newest second of changes may be missing.
    pub settled: bool,
    /// Store key (one of [`STORE_KEYS`](crate::raw::STORE_KEYS)) to its value, decoded to
    /// UTF-8.
    pub stores: BTreeMap<String, String>,
    /// Keys of [`STORE_KEYS`](crate::raw::STORE_KEYS) that were there but whose value couldn't
    /// be decoded (damaged, unknown compression, too big).
    pub unreadable_keys: Vec<String>,
}

impl WebStore {
    /// Whether the records are from one of 0.10's released builds rather than a dev server.
    pub fn is_app_origin(&self) -> bool {
        self.origin.as_deref().is_some_and(is_app_origin)
    }
}

/// The values stay out of debug output: snippets can hold passwords and tokens.
impl fmt::Debug for WebStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sizes: BTreeMap<&str, usize> = self.stores.iter().map(|(k, v)| (k.as_str(), v.len())).collect();
        f.debug_struct("WebStore")
            .field("source", &self.source)
            .field("origin", &self.origin)
            .field("modified", &self.modified)
            .field("settled", &self.settled)
            .field("value_bytes", &sizes)
            .field("unreadable_keys", &self.unreadable_keys)
            .finish()
    }
}

/// Why a store or file couldn't be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum LegacyError {
    /// A file couldn't be read or copied. On Windows this is usually the old app still holding
    /// it; trying again after it has closed should work.
    Io { path: PathBuf, error: io::Error },
    /// Bigger than any 0.10 store or file could be, so it was left alone.
    TooBig { path: PathBuf, bytes: u64 },
    /// Not the database it looked like, or damaged past reading.
    Damaged { path: PathBuf, detail: String },
    /// The LevelDB library stopped on something it couldn't handle (a panic, caught here).
    ReaderFailed { path: PathBuf, detail: String },
    /// Read, but only by going past damage in it: in a LevelDB store, a table block or a log
    /// record that fails its checksum. What it held is missing, and an older value (or a record
    /// since deleted) may show in its place. [`find`] still puts what could be read in
    /// [`Found::stores`], and this in [`Found::problems`] beside it; [`read_store`] gives only
    /// this. `detail` names each file and where in it, never a value.
    ReadPastDamage { path: PathBuf, detail: String },
}

impl LegacyError {
    /// The store or file it is about.
    pub fn path(&self) -> &Path {
        match self {
            LegacyError::Io { path, .. }
            | LegacyError::TooBig { path, .. }
            | LegacyError::Damaged { path, .. }
            | LegacyError::ReaderFailed { path, .. }
            | LegacyError::ReadPastDamage { path, .. } => path,
        }
    }
}

impl fmt::Display for LegacyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LegacyError::Io { path, error } => write!(f, "couldn't read {}: {error}", path.display()),
            LegacyError::TooBig { path, bytes } => {
                write!(f, "{} is {bytes} bytes, too big to be 0.10's", path.display())
            }
            LegacyError::Damaged { path, detail } => write!(f, "{} can't be read: {detail}", path.display()),
            LegacyError::ReaderFailed { path, detail } => {
                write!(f, "reading {} stopped unexpectedly: {detail}", path.display())
            }
            LegacyError::ReadPastDamage { path, detail } => {
                write!(f, "{} was read, but past damage in it: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for LegacyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LegacyError::Io { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Everything [`find`] turned up.
#[derive(Debug, Default)]
pub struct Found {
    /// Every store holding at least one of 0.10's keys, best first (see [`find`] for the order).
    pub stores: Vec<WebStore>,
    /// Stores that were there but couldn't be read, and why. A store that could be read only by
    /// going past damage is here ([`LegacyError::ReadPastDamage`]) as well as in
    /// [`Found::stores`], so whatever counts problems as doubts counts it.
    pub problems: Vec<LegacyError>,
    /// Contents of `.window-state.json`, if there was one.
    pub window_state: Option<String>,
    /// Why `.window-state.json` couldn't be read, when it was there but couldn't. Only the
    /// window's size and place hang on it, so it doesn't count against [`Found::is_complete`].
    pub window_state_problem: Option<LegacyError>,
}

impl Found {
    /// The store 1.0 imports: the first in [`Found::stores`].
    pub fn best(&self) -> Option<&WebStore> {
        self.stores.first()
    }

    /// What 1.0 imports, in the shape `import` takes: the best store's records and the window
    /// state. `None` when no store had any of 0.10's keys.
    pub fn raw(&self) -> Option<RawWebState> {
        let best = self.best()?;
        Some(RawWebState {
            stores: best.stores.clone(),
            window_state: self.window_state.clone(),
            source: best.source.clone(),
        })
    }

    /// Whether the settings import can be taken as complete. `false` when no store was found, a
    /// store couldn't be read (or was read past damage), the best store is from a dev server,
    /// wasn't holding still, or has a value that couldn't be decoded. 1.0 then starts with
    /// privacy turned up (plan, section 6.5: streamer mode and secret hiding on) and says what
    /// it couldn't bring over.
    ///
    /// Damage counts in any store, not only the best: it can take the very records that would
    /// have made a store the best one (its `sshee-settings`, say), and another store wins only
    /// because of it.
    pub fn is_complete(&self) -> bool {
        let Some(best) = self.best() else { return false };
        self.problems.is_empty() && best.is_app_origin() && best.settled && best.unreadable_keys.is_empty()
    }
}

/// Looks in `places` for 0.10's web view stores and window state, and reads them.
///
/// Stores are ordered so the first is the one to import: records from 0.10's released app
/// before a developer's dev server; then a store that has `sshee-settings` (written as soon as
/// anything was set); then the most recently changed store, so where a web view moved from one
/// format to the other the store it still writes wins; then the one with more keys.
pub fn find(places: &Places) -> Found {
    // Copies a killed or crashed reading left behind go first (they can hold secrets).
    copy::sweep(places.scratch.as_deref());
    let mut found = Found::default();
    for source in scan::find_candidates(&places.web_roots) {
        match read_store_in(&source, places.scratch.as_deref()) {
            Ok((stores, damage)) => {
                found.stores.extend(stores);
                found.problems.extend(damage);
            }
            Err(error) => found.problems.push(error),
        }
    }
    found.stores.retain(|s| !s.stores.is_empty() || !s.unreadable_keys.is_empty());
    found.stores.sort_by_key(|s| std::cmp::Reverse(rank(s)));
    match read_window_state(&places.window_state) {
        Ok(state) => found.window_state = state,
        Err(error) => found.window_state_problem = Some(error),
    }
    found
}

fn rank(s: &WebStore) -> (bool, bool, Option<SystemTime>, usize) {
    (s.is_app_origin(), s.stores.contains_key("sshee-settings"), s.modified, s.stores.len())
}

/// Reads one store, from a private copy in the system's temporary folder: every origin in it
/// that has any of 0.10's keys (a WebKit store has one origin; a Chromium store one per site
/// the web view ever showed). Origins with none of the keys are left out. A store that could
/// only be read past damage is [`LegacyError::ReadPastDamage`]. [`RawSource::Fixture`] has no
/// files to read and gives nothing.
pub fn read_store(source: &RawSource) -> Result<Vec<WebStore>, LegacyError> {
    match read_store_in(source, None)? {
        (stores, None) => Ok(stores),
        (_, Some(damage)) => Err(damage),
    }
}

/// [`read_store`], copying into a scratch folder made in `scratch` (see [`Places::scratch`]),
/// and giving what was read past damage as well as the damage.
fn read_store_in(
    source: &RawSource,
    scratch: Option<&Path>,
) -> Result<(Vec<WebStore>, Option<LegacyError>), LegacyError> {
    let Some(path) = source_path(source) else { return Ok((Vec::new(), None)) };
    let scratch = Scratch::new_in(scratch)?;
    let read = guarded(path, || match source {
        RawSource::WebView2LevelDb(dir) => chromium::read_leveldb(dir, &scratch),
        RawSource::WebView2Sqlite(file) => chromium::read_sqlite(file, &scratch),
        RawSource::WebKitSqlite(file) => webkit::read(file, scan::webkit_origin(file), &scratch)
            .map(|(r, copied)| StoreRead { records: vec![r], copied, damage: Vec::new() }),
        RawSource::Fixture => unreachable!("returned above"),
    })?;
    // `scratch` goes out of scope here, and the copies with it.
    let StoreRead { records, copied, damage } = read;
    let stores: Vec<WebStore> = records
        .into_iter()
        .filter(|r| !r.stores.is_empty() || !r.unreadable_keys.is_empty())
        .map(|r| WebStore {
            source: source.clone(),
            origin: r.origin,
            modified: copied.modified,
            settled: copied.settled,
            stores: r.stores,
            unreadable_keys: r.unreadable_keys,
        })
        .collect();
    // Said even when no origin had 0.10's keys: the damage may be what took them.
    let damage = (!damage.is_empty())
        .then(|| LegacyError::ReadPastDamage { path: path.to_path_buf(), detail: damage.join("; ") });
    Ok((stores, damage))
}

/// The store's file or folder; `None` for [`RawSource::Fixture`].
fn source_path(source: &RawSource) -> Option<&Path> {
    match source {
        RawSource::WebView2LevelDb(p) | RawSource::WebView2Sqlite(p) | RawSource::WebKitSqlite(p) => Some(p),
        RawSource::Fixture => None,
    }
}

/// Reads tauri-plugin-window-state's `.window-state.json` from the first of `paths` that has
/// one, as it is (it's JSON, which `import` parses). `Ok(None)` when there is none: 0.10 writes
/// it when the window closes, so a user who never closed the window normally has none.
pub fn read_window_state(paths: &[PathBuf]) -> Result<Option<String>, LegacyError> {
    for path in paths {
        let meta = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(LegacyError::Io { path: path.clone(), error }),
        };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > MAX_WINDOW_STATE_BYTES {
            return Err(LegacyError::TooBig { path: path.clone(), bytes: meta.len() });
        }
        let mut bytes = Vec::new();
        // A link is followed (a dotfile manager may have put one there), but only to a file.
        copy::open_file(path, true)
            .and_then(|f| f.take(MAX_WINDOW_STATE_BYTES).read_to_end(&mut bytes))
            .map_err(|error| LegacyError::Io { path: path.clone(), error })?;
        return String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| LegacyError::Damaged { path: path.clone(), detail: "not UTF-8 text".into() });
    }
    Ok(None)
}

/// What a reader brought back from one store.
pub(crate) struct StoreRead {
    /// Each origin's records.
    pub records: Vec<OriginRecords>,
    pub copied: Copied,
    /// Damage reading went past, one line per place (see [`LegacyError::ReadPastDamage`]).
    pub damage: Vec<String>,
}

/// One origin's records as a reader found them, before they become a [`WebStore`].
#[derive(Debug, Default)]
pub(crate) struct OriginRecords {
    pub origin: Option<String>,
    pub stores: BTreeMap<String, String>,
    pub unreadable_keys: Vec<String>,
}

/// Runs a reader, turning a panic in it into [`LegacyError::ReaderFailed`]. The readers only
/// touch their own copies and values they build, so nothing is left half-changed by a panic.
pub(crate) fn guarded<T>(
    path: &Path,
    read: impl FnOnce() -> Result<T, LegacyError>,
) -> Result<T, LegacyError> {
    match panic::catch_unwind(AssertUnwindSafe(read)) {
        Ok(result) => result,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".to_owned());
            Err(LegacyError::ReaderFailed { path: path.to_path_buf(), detail })
        }
    }
}

/// Opens a copied SQLite database. Read-write, because SQLite finishes a copied WAL or rolls
/// back a half-done write on first read, and needs to write the copy for that; the copy is
/// ours, and `query_only` keeps this code itself from changing anything. `original` is for
/// errors.
pub(crate) fn open_sqlite_copy(original: &Path, copy: &Path) -> Result<rusqlite::Connection, LegacyError> {
    use rusqlite::OpenFlags;
    let damaged = |e: rusqlite::Error| LegacyError::Damaged {
        path: original.to_path_buf(),
        detail: format!("SQLite: {e}"),
    };
    let conn = rusqlite::Connection::open_with_flags(
        copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(damaged)?;
    // Not a database (or a damaged header) shows at the first read, so read now.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).map_err(damaged)?;
    conn.execute_batch("PRAGMA query_only = ON; PRAGMA trusted_schema = OFF;").map_err(damaged)?;
    Ok(conn)
}

/// UTF-16 little-endian to a Rust string. A lone surrogate (which `JSON.stringify` never
/// writes) becomes U+FFFD; an odd number of bytes is damage.
pub(crate) fn decode_utf16le(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair));
    Some(char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect())
}
