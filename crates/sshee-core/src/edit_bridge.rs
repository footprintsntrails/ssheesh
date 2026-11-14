//! Editing a server's file in an editor on this computer (VS Code, Zed, any
//! other), with every save going back to the server.
//!
//! Files' own editor reshapes the whole text on every key, so it edits files
//! up to 128 KB (`BUILT_IN_MAX`); "Open in <editor>" is how bigger ones get
//! edited, and any file can be. The bridge copies the file into a private
//! folder, starts the editor on the copy, watches the copy, and uploads each
//! save with `files::save_guarded` against the stamp of the last version both
//! sides agreed on. A save over a file someone changed on the server in the
//! meantime is never made silently: the session stops and asks (`Conflict`).
//!
//! Everything goes over the same ssh as the rest of Files (`files::read_stamped`,
//! `files::save_guarded`), so through Tor when the server is reached through
//! Tor. The editor itself is another matter: it, its extensions and its AI
//! helpers talk to the internet as they like, from your real address. The UI
//! warns about that, and about streamer mode (an editor hides no secrets).
//!
//! ## The rules
//!
//! Sizes (`Limits`):
//! - below 128 KB Files keeps its built-in editor; "Open in <editor>" works
//!   for any size;
//! - up to 50 MB a file opens straight away; above, `open` answers
//!   `Opened::NeedsConfirm { size }` and the UI asks first ("This file is
//!   230 MB. Download a copy to edit?"), then opens again with `confirmed`;
//! - above 1 GB it's refused, saying why;
//! - nothing is downloaded that would leave less than 1 GB free on the disk
//!   the edit folder is on.
//!
//! Where copies live: `<the copy's own folder>/edit/<id>/<file name>`
//! (`AppIdentity::dir`), one folder per session so the editor sees no other
//! files, 0700 with the copy 0600 on Unix (only you can read them); on
//! Windows inside your user profile, which only you, SYSTEM and
//! administrators can read. Beside the copy, SSHeesh's two files, both named
//! with a leading dot (a server's name never gets one, see `local_file_name`):
//! `.sshee-lock`, held while a session is open, and `.sshee-edit.json`, the
//! marker: which server and path, and "uploaded up to stamp X", rewritten
//! atomically (a new file renamed over the old one) after every upload. A
//! copy is fully uploaded when its SHA-256 is the marker's stamp's.
//!
//! How long a copy lives:
//! - it's deleted as soon as its session closes and its last version is on
//!   the server. A session closes when you close it ("Stop editing" in
//!   Files, `close`), when its server's tabs are all closed (`close_host`),
//!   when the app quits (`close_all`), and on its own after 24 hours with no
//!   change to the copy and nothing waiting to upload. Nobody can tell when
//!   an editor is done with a file, so nothing tries;
//! - a copy with changes the server doesn't have is never deleted on its
//!   own: closing keeps it and says where it is;
//! - at start, `sweep` deletes every leftover folder that's fully uploaded
//!   and returns the others for the UI to offer ("2 files have changes that
//!   were never uploaded: Upload / Discard / Show in folder"). One that's
//!   more than 30 days old and was offered on an earlier start
//!   (`mark_offered`) is deleted then;
//! - if the edit folder grows past 2 GB, fully uploaded copies go, oldest
//!   first, even from open sessions with nothing to upload (only the file:
//!   the session downloads a fresh copy when it's next opened, and an
//!   editor that saves it again just makes it a change to upload).
//!
//! Deleting goes by `folder`'s rules: only the session folder's own files,
//! never through a link, never through a folder swapped in under SSHeesh.
//!
//! Connection trouble never loses an edit: the copy stays as it is, the
//! session says `Offline` and tries again with the terminal's backoff (1 to
//! 15 seconds), and at once when the UI says the server is back
//! (`retry_now`, `retry_host`).

mod folder;
mod launch;

pub use launch::{EditorCommand, FILE_ARG};

use crate::error::{ErrorCode, OpError};
use crate::files::{self, SaveError, Stamp};
use crate::ssh::{SshEnv, Target};
use folder::{os, Dir, Kind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MB: u64 = 1024 * 1024;

/// Up to here Files edits a file itself; past it, its text field gets slow
/// (it reshapes the whole text on every key), and Files offers "Open in
/// <editor>" instead.
pub const BUILT_IN_MAX: u64 = 128 * 1024;

/// The marker beside a copy: where it came from and what's on the server.
const MARKER: &str = ".sshee-edit.json";
/// Held (locked) while a session is open, so a sweep from another copy of
/// the app leaves the folder alone.
const LOCK: &str = ".sshee-lock";

/// How big a file the bridge copies, and how much room it leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Above this, ask first.
    pub ask_above: u64,
    /// Above this, refuse.
    pub max: u64,
    /// Never leave less than this free on the disk.
    pub keep_free: u64,
    /// The edit folder's size past which uploaded copies are deleted.
    pub folder_cap: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { ask_above: 50 * MB, max: 1024 * MB, keep_free: 1024 * MB, folder_cap: 2048 * MB }
    }
}

/// What a file's size means for opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SizeRule {
    /// Small enough for Files' own editor (an outside one works too).
    BuiltIn,
    /// Opens in an outside editor straight away.
    Open,
    /// Opens after you say yes.
    Ask,
    /// Too big to copy.
    TooLarge,
}

impl Limits {
    pub fn rule(&self, size: u64) -> SizeRule {
        if size > self.max {
            SizeRule::TooLarge
        } else if size > self.ask_above {
            SizeRule::Ask
        } else if size < BUILT_IN_MAX {
            SizeRule::BuiltIn
        } else {
            SizeRule::Open
        }
    }
}

/// How quickly the bridge reacts. Tests make it quick.
#[derive(Debug, Clone)]
pub struct Timing {
    /// Quiet time after the last change before uploading: an editor's save
    /// is a burst (a temp file, a rename, a time change).
    pub debounce: Duration,
    /// How often the copy is looked at anyway, in case the system's watcher
    /// missed something; also the longest a burst can delay an upload.
    pub poll: Duration,
    /// A session with nothing to upload closes after this long without a
    /// change.
    pub idle_close: Duration,
    /// A leftover with unsent changes, offered once, is deleted at a start
    /// after this long without a change.
    pub leftover_age: Duration,
    /// The wait before the n-th try after a failed upload.
    pub backoff: fn(u32) -> Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            debounce: Duration::from_millis(300),
            poll: Duration::from_secs(2),
            idle_close: Duration::from_secs(24 * 3600),
            leftover_age: Duration::from_secs(30 * 24 * 3600),
            backoff: |n| Duration::from_millis(crate::session::backoff_ms(n)),
        }
    }
}

// ---------------------------------------------------------------- the server side

/// Where a session's file lives. `Server` is the real one; tests stand in.
pub trait Remote: Send + Sync {
    /// The file's size in bytes, before deciding to copy it.
    fn size(&self, path: &str) -> Result<u64, OpError>;
    /// The whole file and its stamp (`files::read_stamped`).
    fn read(&self, path: &str, limit: u64) -> Result<(Vec<u8>, Stamp), OpError>;
    /// Saves only over the file `expected` was taken of (`files::save_guarded`).
    fn save(&self, path: &str, bytes: &[u8], expected: Option<&Stamp>) -> Result<Stamp, SaveError>;
}

/// A server (or this computer) as Files reaches it: the same ssh, the same
/// Tor, the same saved password.
#[derive(Debug, Clone)]
pub struct Server {
    pub target: Target,
    pub env: SshEnv,
}

impl Remote for Server {
    fn size(&self, path: &str) -> Result<u64, OpError> {
        match &self.target {
            Target::Local => {
                let meta = std::fs::metadata(files::resolve_local(path))?;
                if !meta.is_file() {
                    return Err(OpError::failed("This isn't a regular file."));
                }
                Ok(meta.len())
            }
            Target::Ssh(t) => {
                let p = crate::ssh::shell_quote(path);
                let script = format!(
                    r#"[ -e {p} ] || {{ echo "No such file or directory" >&2; exit 4; }}; [ -f {p} ] || {{ echo "This isn't a regular file." >&2; exit 4; }}; stat -L -c %s -- {p} 2>/dev/null || stat -L -f %z -- {p}"#
                );
                let out = crate::ssh::run_remote_read(t, &self.env, &script)?;
                String::from_utf8_lossy(&out).lines().rev().find_map(|l| l.trim().parse().ok()).ok_or_else(|| OpError::failed("Couldn't read the file's size from the server."))
            }
        }
    }

    fn read(&self, path: &str, limit: u64) -> Result<(Vec<u8>, Stamp), OpError> {
        files::read_stamped(&self.target, &self.env, path, limit)
    }

    fn save(&self, path: &str, bytes: &[u8], expected: Option<&Stamp>) -> Result<Stamp, SaveError> {
        files::save_guarded(&self.target, &self.env, path, bytes, expected)
    }
}

// ---------------------------------------------------------------- what the UI hears

/// What happened to a session. The UI shows the latest as the file's status
/// line in Files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditEvent {
    /// The copy is ready (and the editor started on it, if one was given).
    Opened { local: PathBuf },
    /// A save is on its way to the server.
    Uploading,
    /// The server has the copy's version, since `at` (Unix seconds).
    Uploaded { at: u64 },
    /// The copy was replaced with the server's version ("Take theirs", or a
    /// fresh copy after one was cleared away).
    Reloaded { at: u64 },
    /// The server can't be reached. The save waits on this computer and goes
    /// at the next try, in `retry_in_ms`, or at `retry_now`.
    Offline { pending: bool, retry_in_ms: u64, message: String },
    /// The file changed on the server since the copy was made (`now` is what's
    /// there, None when it's gone). Nothing more is uploaded until you choose:
    /// `keep_mine`, `take_theirs`, `save_mine_as_copy`, with `compare` to look.
    Conflict { now: Option<Stamp> },
    /// The save didn't go, for a reason trying again won't fix by itself
    /// (no permission, a full disk on the server). It stays on this
    /// computer; the next save or `retry_now` tries again.
    Failed { message: String },
    /// The copy was deleted to keep the edit folder under its cap; it's on
    /// the server, and the next "Open" downloads it again.
    Evicted,
    /// The session ended. `kept` is the copy when it has changes the server
    /// doesn't: it stays, and the next start offers it.
    Closed { kept: Option<PathBuf> },
}

/// Hears every session's events, with the session's id.
pub type EditSink = Arc<dyn Fn(&str, EditEvent) + Send + Sync>;

/// A session as the UI shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub host: String,
    pub remote_path: String,
    /// The copy on this computer ("Show in folder").
    pub local: PathBuf,
    pub last: EditEvent,
    pub uploaded_at: Option<u64>,
    /// The copy has a version the server doesn't.
    pub pending: bool,
    pub conflict: bool,
}

/// What `open` did.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Opened {
    Session(SessionInfo),
    /// The file is over `Limits::ask_above`: ask, then open with `confirmed`.
    NeedsConfirm { size: u64 },
}

/// A session that ended.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Closed {
    pub id: String,
    pub remote_path: String,
    /// Kept because it has changes the server doesn't.
    pub kept: Option<PathBuf>,
}

/// A copy left by an earlier run with changes that never reached the server.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub id: String,
    pub host: String,
    pub remote_path: String,
    pub local: PathBuf,
    pub size: u64,
    /// When the copy last changed, Unix seconds.
    pub changed_at: i64,
    /// It was offered on an earlier start already.
    pub offered_before: bool,
}

/// The two versions in a conflict, and how they differ line by line.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub mine: String,
    pub theirs: String,
    /// Either isn't text: there's nothing to compare line by line.
    pub binary: bool,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffKind {
    Same,
    /// Only in the copy on this computer.
    Mine,
    /// Only on the server.
    Theirs,
}

/// What to open.
pub struct OpenRequest {
    /// The server, as the app knows it (its id): kept in the marker so a
    /// leftover can be offered for the right server.
    pub host: String,
    /// The file's path on the server.
    pub path: String,
    pub remote: Arc<dyn Remote>,
    /// The editor to start; None makes the copy without starting one.
    pub editor: Option<EditorCommand>,
    /// You said yes to a file over `Limits::ask_above`.
    pub confirmed: bool,
}

// ---------------------------------------------------------------- names

/// The name a server's file gets on this computer: its own name, so the
/// editor picks the right language, made safe. No separators or characters
/// a system refuses (`<>:"/\|?*`, control and direction-changing
/// characters, which can make `evil<U+202E>txt.exe` read as `evilexe.txt`), no
/// leading dot that hides it (`.env` becomes `_.env`) or dash that reads as
/// an option, no name Windows keeps for devices (`CON`, `nul.txt`, `COM1`),
/// no trailing dot or space, and at most 100 characters, keeping the
/// extension. Never one of SSHeesh's own names, which all start with a dot.
pub fn local_file_name(remote_path: &str) -> String {
    let base = files::base_name(remote_path);
    let hidden = |c: char| matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let mut name: String = base.chars().map(|c| if c.is_control() || hidden(c) || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    name.truncate(name.trim_end_matches(['.', ' ']).len());
    let name = name.trim_start_matches(' ');
    let mut name = if name.is_empty() { "file".to_string() } else { name.to_string() };
    if name.starts_with(['.', '-']) {
        name.insert(0, '_');
    }
    let device = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let numbered = device.len() == 4 && (device.starts_with("COM") || device.starts_with("LPT")) && device.as_bytes()[3].is_ascii_digit();
    if numbered || matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        name.insert(0, '_');
    }
    const MAX: usize = 100;
    if name.chars().count() > MAX {
        let ext = name.rfind('.').filter(|&i| i > 0 && name.len() - i <= 16).map(|i| name[i..].to_string()).unwrap_or_default();
        let stem: String = name.chars().take(MAX - ext.chars().count()).collect();
        name = format!("{}{ext}", stem.trim_end_matches(['.', ' ']));
    }
    name
}

/// A session's id: 32 lowercase hex digits, also its folder's name.
fn new_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system's random numbers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_session_id(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn sha256_hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// The stamp's hash, when it's a well-formed one.
fn stamp_hash(stamp: &Stamp) -> Option<String> {
    let h = stamp.sha256.as_deref()?.to_ascii_lowercase();
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn size_words(bytes: u64) -> String {
    if bytes >= 1024 * MB {
        format!("{:.1} GB", bytes as f64 / (1024 * MB) as f64)
    } else if bytes >= MB {
        format!("{} MB", bytes.div_ceil(MB))
    } else {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    }
}

/// `name.mine-20261114-1241` (UTC), the name of "Save mine as a copy".
fn copy_name(name: &str, now: u64, attempt: u32) -> String {
    let (days, secs) = ((now / 86400) as i64, now % 86400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let suffix = if attempt > 1 { format!("-{attempt}") } else { String::new() };
    format!("{name}.mine-{year:04}{month:02}{day:02}-{:02}{:02}{suffix}", secs / 3600, secs % 3600 / 60)
}

// ---------------------------------------------------------------- the marker

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marker {
    host: String,
    remote: String,
    /// The copy's file name in the folder.
    name: String,
    /// What's on the server: the copy is fully uploaded when its SHA-256 is
    /// this stamp's.
    base: Stamp,
    created: u64,
    /// When a start first offered this leftover (Unix seconds).
    #[serde(default)]
    offered: Option<u64>,
}

fn read_marker(dir: &Dir) -> Option<Marker> {
    let (file, _) = dir.open_read(os(MARKER)).ok()?;
    let bytes = folder::read_capped(file, 64 * 1024).ok()?;
    let marker: Marker = serde_json::from_slice(&bytes).ok()?;
    // A marker naming something that isn't one plain name of ours is
    // someone else's doing: it's not followed anywhere.
    (marker.name == local_file_name(&marker.name)).then_some(marker)
}

/// Writes `bytes` as `name` in `dir` all at once: a new file, flushed to
/// disk, renamed over the old one. A crash leaves the old or the new, never
/// half of one.
fn write_atomic(dir: &Dir, name: &str, bytes: &[u8]) -> io::Result<()> {
    let temp = format!(".sshee-new-{}", &new_id()[..12]);
    let written = (|| {
        let mut file = dir.create(os(&temp))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        dir.rename(os(&temp), os(name))
    })();
    if written.is_err() {
        let _ = dir.unlink(os(&temp));
    }
    written
}

fn write_marker(dir: &Dir, marker: &Marker) -> io::Result<()> {
    write_atomic(dir, MARKER, &serde_json::to_vec_pretty(marker).map_err(io::Error::other)?)
}

/// Takes the session lock of the folder, or None when someone holds it.
fn try_lock(dir: &Dir) -> io::Result<Option<File>> {
    let file = dir.open_lock(os(LOCK))?;
    // A program this process starts holds a copy of every open file for the moment between
    // its fork and its exec, our lock with them. So a lock held "right now" is asked again
    // for up to half a second before the folder counts as another copy's.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// The copy in `dir`, if it's a plain file of at most `limit` bytes with
/// one name: never through a link, never another file hard-linked in.
fn read_copy(dir: &Dir, name: &str, limit: u64) -> Result<Option<Vec<u8>>, OpError> {
    let (file, meta) = match dir.open_read(os(name)) {
        Ok(opened) => opened,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) if dir.stat(os(name)).ok().flatten().is_some_and(|m| m.kind == Kind::Link) || e.raw_os_error() == Some(libc_eloop()) => {
            return Err(OpError::failed("The copy was replaced with a link, so it wasn't uploaded. SSHeesh only uploads the file it made."));
        }
        Err(e) => return Err(OpError::failed(format!("Couldn't read the copy: {e}"))),
    };
    if meta.links > 1 {
        return Err(OpError::failed("The copy is linked to another file, so it wasn't uploaded. SSHeesh only uploads the file it made."));
    }
    if meta.size > limit {
        return Err(OpError::new(ErrorCode::TooLarge, format!("The copy grew past {}, so it wasn't uploaded.", size_words(limit))));
    }
    folder::read_capped(file, limit).map(Some).map_err(|e| OpError::failed(format!("Couldn't read the copy: {e}")))
}

/// ELOOP, what opening a link with O_NOFOLLOW says.
fn libc_eloop() -> i32 {
    #[cfg(unix)]
    {
        libc::ELOOP
    }
    #[cfg(not(unix))]
    {
        -1
    }
}

/// Whether the copy in `dir` is what the server has (or is gone, which
/// leaves nothing to upload).
fn uploaded(dir: &Dir, marker: &Marker, limit: u64) -> bool {
    match read_copy(dir, &marker.name, limit) {
        Ok(None) => true,
        Ok(Some(bytes)) => stamp_hash(&marker.base).as_deref() == Some(sha256_hex(&bytes).as_str()),
        Err(_) => false,
    }
}

/// The bytes in a folder (one level down), and when the copy in it last
/// changed: SSHeesh's own files (with a leading dot) don't count for that,
/// as the marker is rewritten at every upload.
fn folder_size(dir: &Dir) -> (u64, i64) {
    let mut size = 0;
    let mut newest = 0;
    for name in dir.entries().unwrap_or_default() {
        if let Ok(Some(meta)) = dir.stat(&name) {
            size += meta.size;
            if !name.to_string_lossy().starts_with('.') {
                newest = newest.max(meta.modified);
            }
        }
    }
    (size, newest)
}

// ---------------------------------------------------------------- the bridge

/// Where the bridge keeps copies, and its rules.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// The edit folder. Made (0700) if it isn't there; it must not be a link.
    pub root: PathBuf,
    pub limits: Limits,
    pub timing: Timing,
    /// Free bytes on the disk a folder is on; tests pretend a full disk.
    pub free_space: fn(&Path) -> io::Result<u64>,
}

impl BridgeConfig {
    /// The running copy's edit folder: `<its folder>/edit`.
    pub fn for_app() -> Option<Self> {
        let root = crate::app_identity::current().dir()?.join("edit");
        Some(Self::in_dir(root))
    }

    pub fn in_dir(root: PathBuf) -> Self {
        BridgeConfig { root, limits: Limits::default(), timing: Timing::default(), free_space: folder::free_space }
    }
}

/// What every session shares.
struct Shared {
    root: PathBuf,
    root_id: (u64, u64),
    limits: Limits,
    timing: Timing,
    free_space: fn(&Path) -> io::Result<u64>,
    sink: EditSink,
    /// When this run started: a leftover offered before it was offered on
    /// an earlier start.
    started: u64,
}

impl Shared {
    /// The edit folder, checked to be the very folder the bridge started
    /// with: one swapped in under its name is refused.
    fn root(&self) -> io::Result<Dir> {
        let dir = Dir::open(&self.root)?;
        if dir.id() != self.root_id {
            return Err(io::Error::other("the edit folder was replaced, so SSHeesh won't touch it"));
        }
        Ok(dir)
    }

    fn emit(&self, id: &str, event: EditEvent) {
        (self.sink)(id, event);
    }

    /// Refuses a download of `size` bytes that would leave less than
    /// `keep_free` on the disk.
    fn room_for(&self, size: u64) -> Result<(), OpError> {
        let free = (self.free_space)(&self.root).map_err(|e| OpError::failed(format!("Couldn't tell how much space is free on this computer: {e}")))?;
        if free < size.saturating_add(self.limits.keep_free) {
            return Err(OpError::new(
                ErrorCode::TooLarge,
                format!(
                    "There isn't enough free space on this computer: a copy of this file ({}) would leave less than {} free. Free some space, or download the file instead.",
                    size_words(size),
                    size_words(self.limits.keep_free)
                ),
            ));
        }
        Ok(())
    }
}

/// The bridge: every open session, and the edit folder they live in. One
/// per app; every call is safe from any thread, and the ones that talk to a
/// server block until it answers, so the UI calls them off its own thread.
pub struct EditBridge {
    shared: Arc<Shared>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// One `open` at a time, so opening a file twice at once makes one session.
    opening: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl EditBridge {
    /// The bridge for the running copy of the app (`BridgeConfig::for_app`).
    pub fn for_app(sink: EditSink) -> Result<Self, OpError> {
        let config = BridgeConfig::for_app().ok_or_else(|| OpError::failed("There's no home folder to keep copies in."))?;
        Self::new(config, sink)
    }

    pub fn new(config: BridgeConfig, sink: EditSink) -> Result<Self, OpError> {
        let refuse = |e: io::Error| OpError::failed(format!("Couldn't use the folder for copies ({}): {e}", config.root.display()));
        if let Some(parent) = config.root.parent() {
            std::fs::create_dir_all(parent).map_err(refuse)?;
        }
        match std::fs::symlink_metadata(&config.root) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                #[cfg(unix)]
                let builder = {
                    let mut builder = std::fs::DirBuilder::new();
                    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
                    builder
                };
                #[cfg(not(unix))]
                let builder = std::fs::DirBuilder::new();
                builder.create(&config.root).map_err(refuse)?;
            }
            Err(e) => return Err(refuse(e)),
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => return Err(refuse(io::Error::other("it isn't a real folder"))),
            Ok(_) => {}
        }
        let root_id = Dir::open(&config.root).map_err(refuse)?.id();
        let shared = Shared {
            root: config.root,
            root_id,
            limits: config.limits,
            timing: config.timing,
            free_space: config.free_space,
            sink,
            started: unix_now(),
        };
        Ok(EditBridge { shared: Arc::new(shared), sessions: Mutex::new(HashMap::new()), opening: Mutex::new(()) })
    }

    /// The edit folder.
    pub fn root(&self) -> &Path {
        &self.shared.root
    }

    pub fn limits(&self) -> Limits {
        self.shared.limits
    }

    /// The open sessions, without the ones that closed on their own.
    fn live(&self) -> Vec<Arc<Session>> {
        let mut sessions = lock(&self.sessions);
        sessions.retain(|_, s| !s.is_closed());
        sessions.values().cloned().collect()
    }

    fn session(&self, id: &str) -> Result<Arc<Session>, OpError> {
        self.live().into_iter().find(|s| s.id == id).ok_or_else(|| OpError::new(ErrorCode::NotFound, "That file isn't open in an editor anymore."))
    }

    /// Opens `request.path` in `request.editor`: copies it (asking first
    /// when it's big, see `Limits`), starts the editor and uploads every
    /// save. A file already open just gets its editor started again, with
    /// a fresh copy if the old one was cleared away.
    pub fn open(&self, request: OpenRequest) -> Result<Opened, OpError> {
        let _one = lock(&self.opening);
        if let Some(session) = self.live().into_iter().find(|s| s.host == request.host && s.remote_path == request.path) {
            session.refresh_if_gone(&request.remote)?;
            if let Some(editor) = &request.editor {
                editor.launch(&session.local)?;
            }
            return Ok(Opened::Session(session.info()));
        }
        let size = request.remote.size(&request.path)?;
        match self.shared.limits.rule(size) {
            SizeRule::TooLarge => {
                return Err(OpError::new(
                    ErrorCode::TooLarge,
                    format!(
                        "This file is {}. SSHeesh opens files up to {} in another editor; download it instead.",
                        size_words(size),
                        size_words(self.shared.limits.max)
                    ),
                ))
            }
            SizeRule::Ask if !request.confirmed => return Ok(Opened::NeedsConfirm { size }),
            _ => {}
        }
        if let Some(editor) = &request.editor {
            editor.check()?;
        }
        self.enforce_cap(size);
        self.shared.room_for(size)?;
        let limit = if request.confirmed { self.shared.limits.max } else { self.shared.limits.ask_above };
        let session = Session::create(&self.shared, &request.host, &request.path, request.remote.clone(), limit)?;
        lock(&self.sessions).insert(session.id.clone(), session.clone());
        session.start();
        self.shared.emit(&session.id, EditEvent::Opened { local: session.local.clone() });
        if let Some(editor) = &request.editor {
            if let Err(e) = editor.launch(&session.local) {
                self.close(&session.id);
                return Err(e);
            }
        }
        Ok(Opened::Session(session.info()))
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        self.live().iter().map(|s| s.info()).collect()
    }

    pub fn status(&self, id: &str) -> Option<SessionInfo> {
        self.session(id).ok().map(|s| s.info())
    }

    /// Tries a waiting upload now (the UI calls it when the server is back).
    pub fn retry_now(&self, id: &str) {
        if let Ok(s) = self.session(id) {
            s.wake(Wake::Now);
        }
    }

    /// `retry_now` for every session on `host`.
    pub fn retry_host(&self, host: &str) {
        self.live().iter().filter(|s| s.host == host).for_each(|s| s.wake(Wake::Now));
    }

    /// "Keep mine": saves the copy over what's on the server now.
    pub fn keep_mine(&self, id: &str) -> Result<(), OpError> {
        self.session(id)?.keep_mine()
    }

    /// "Take theirs": replaces the copy with what's on the server. The
    /// editor shows it when it notices the file changed (most reload on
    /// their own, or ask).
    pub fn take_theirs(&self, id: &str) -> Result<(), OpError> {
        self.session(id)?.take_theirs()
    }

    /// "Save mine as a copy": puts the copy beside the file on the server as
    /// `name.mine-YYYYMMDD-HHMM` and returns that path. The conflict stays
    /// open; the UI usually follows with `take_theirs`.
    pub fn save_mine_as_copy(&self, id: &str) -> Result<String, OpError> {
        self.session(id)?.save_mine_as_copy()
    }

    /// "Compare": both versions, and how they differ.
    pub fn compare(&self, id: &str) -> Result<Comparison, OpError> {
        self.session(id)?.compare()
    }

    /// Ends a session ("Stop editing"): one last upload if there's something
    /// to send, then the copy goes if the server has it, else it stays and
    /// `kept` says where.
    pub fn close(&self, id: &str) -> Option<Closed> {
        let session = lock(&self.sessions).remove(id)?;
        Some(session.finish(false))
    }

    /// Ends a session and deletes the copy even with changes the server
    /// doesn't have (you chose "Discard").
    pub fn discard(&self, id: &str) -> Option<Closed> {
        let session = lock(&self.sessions).remove(id)?;
        Some(session.finish(true))
    }

    /// Ends every session on `host` (its tabs are all closed).
    pub fn close_host(&self, host: &str) -> Vec<Closed> {
        let ids: Vec<String> = self.live().iter().filter(|s| s.host == host).map(|s| s.id.clone()).collect();
        self.close_many(ids)
    }

    /// Ends every session (the app is quitting). Their last uploads run side
    /// by side, so a server that's down holds the quit up for one connection
    /// timeout, not one per file.
    pub fn close_all(&self) -> Vec<Closed> {
        let ids: Vec<String> = self.live().iter().map(|s| s.id.clone()).collect();
        self.close_many(ids)
    }

    fn close_many(&self, ids: Vec<String>) -> Vec<Closed> {
        let sessions: Vec<Arc<Session>> = {
            let mut map = lock(&self.sessions);
            ids.iter().filter_map(|id| map.remove(id)).collect()
        };
        let closing: Vec<JoinHandle<Closed>> = sessions.into_iter().map(|s| std::thread::spawn(move || s.finish(false))).collect();
        closing.into_iter().filter_map(|h| h.join().ok()).collect()
    }

    /// At start: deletes every copy a previous run left that's fully
    /// uploaded, and returns the ones with changes the server doesn't have,
    /// for the UI to offer (`resume_leftover`, `discard_leftover`, or show
    /// `local` in its folder). One of those more than 30 days old that was
    /// offered on an earlier start is deleted instead. Folders another
    /// running copy of the app has open are left alone, and so is anything
    /// in the edit folder SSHeesh didn't make.
    pub fn sweep(&self) -> Vec<Leftover> {
        let mut found = Vec::new();
        let Ok(root) = self.shared.root() else { return found };
        let open: Vec<String> = self.live().iter().map(|s| s.id.clone()).collect();
        for id in folder::names(root.entries().unwrap_or_default()) {
            if !is_session_id(&id) || open.contains(&id) {
                continue;
            }
            let Ok(dir) = root.child(os(&id)) else { continue };
            let Ok(Some(_held)) = try_lock(&dir) else { continue };
            let Some(marker) = read_marker(&dir) else {
                // Made but never finished (a crash during "open"): nothing
                // in it can be uploaded, as nobody knows where to.
                let only_ours = folder::names(dir.entries().unwrap_or_default()).iter().all(|n| n.starts_with('.'));
                if only_ours {
                    let _ = root.remove_tree(os(&id));
                }
                continue;
            };
            if uploaded(&dir, &marker, self.shared.limits.max) {
                let _ = root.remove_tree(os(&id));
                continue;
            }
            let meta = dir.stat(os(&marker.name)).ok().flatten();
            let changed_at = meta.map_or(0, |m| m.modified);
            let offered_before = marker.offered.is_some_and(|t| t < self.shared.started);
            let age = unix_now().saturating_sub(changed_at.max(0) as u64);
            if offered_before && age >= self.shared.timing.leftover_age.as_secs() {
                let _ = root.remove_tree(os(&id));
                continue;
            }
            found.push(Leftover {
                local: self.shared.root.join(&id).join(&marker.name),
                id,
                host: marker.host,
                remote_path: marker.remote,
                size: meta.map_or(0, |m| m.size),
                changed_at,
                offered_before,
            });
        }
        self.enforce_cap(0);
        found
    }

    /// Records that leftovers were offered, so a later start can delete the
    /// ones that are too old.
    pub fn mark_offered(&self, ids: &[String]) {
        let Ok(root) = self.shared.root() else { return };
        for id in ids.iter().filter(|id| is_session_id(id)) {
            let Ok(dir) = root.child(os(id)) else { continue };
            let Ok(Some(_held)) = try_lock(&dir) else { continue };
            if let Some(mut marker) = read_marker(&dir) {
                if marker.offered.is_none() {
                    marker.offered = Some(unix_now());
                    let _ = write_marker(&dir, &marker);
                }
            }
        }
    }

    /// "Upload" for a leftover: opens it as a session again, which uploads
    /// it at once (and asks, as any session, if the server's file changed).
    /// With an editor, it's started on the copy too.
    pub fn resume_leftover(&self, id: &str, remote: Arc<dyn Remote>, editor: Option<&EditorCommand>) -> Result<SessionInfo, OpError> {
        if !is_session_id(id) {
            return Err(OpError::new(ErrorCode::NotFound, "There's no such copy."));
        }
        let session = Session::resume(&self.shared, id, remote)?;
        lock(&self.sessions).insert(session.id.clone(), session.clone());
        session.start();
        self.shared.emit(&session.id, EditEvent::Opened { local: session.local.clone() });
        session.wake(Wake::Now);
        if let Some(editor) = editor {
            editor.launch(&session.local)?;
        }
        Ok(session.info())
    }

    /// "Discard" for a leftover: deletes it, unsent changes and all.
    pub fn discard_leftover(&self, id: &str) -> Result<(), OpError> {
        if !is_session_id(id) {
            return Err(OpError::new(ErrorCode::NotFound, "There's no such copy."));
        }
        let root = self.shared.root()?;
        let dir = root.child(os(id))?;
        let held = try_lock(&dir)?.ok_or_else(|| OpError::failed("This copy is open in SSHeesh right now."))?;
        drop(dir);
        // Released first: Windows can't delete a file that's open.
        drop(held);
        root.remove_tree(os(id))?;
        Ok(())
    }

    /// Keeps the edit folder under `Limits::folder_cap`, with room for
    /// `incoming` more bytes: deletes fully uploaded copies, oldest change
    /// first, from finished folders and from open sessions with nothing to
    /// upload alike. A copy with unsent changes is never touched.
    pub fn enforce_cap(&self, incoming: u64) {
        let cap = self.shared.limits.folder_cap;
        let Ok(root) = self.shared.root() else { return };
        let mut items = Vec::new();
        let mut total = 0u64;
        for id in folder::names(root.entries().unwrap_or_default()).into_iter().filter(|n| is_session_id(n)) {
            let Ok(dir) = root.child(os(&id)) else { continue };
            let (size, newest) = folder_size(&dir);
            total += size;
            items.push((newest, id, size));
        }
        if total.saturating_add(incoming) <= cap {
            return;
        }
        items.sort();
        let open: HashMap<String, Arc<Session>> = self.live().into_iter().map(|s| (s.id.clone(), s)).collect();
        for (_, id, size) in items {
            if total.saturating_add(incoming) <= cap {
                break;
            }
            let freed = match open.get(&id) {
                Some(session) => session.evict_if_clean(),
                None => {
                    let Ok(dir) = root.child(os(&id)) else { continue };
                    let Ok(Some(held)) = try_lock(&dir) else { continue };
                    let clean = read_marker(&dir).is_some_and(|m| uploaded(&dir, &m, self.shared.limits.max));
                    drop(dir);
                    drop(held);
                    clean && root.remove_tree(os(&id)).is_ok()
                }
            };
            if freed {
                total = total.saturating_sub(size);
            }
        }
    }
}

impl Drop for EditBridge {
    /// Stops the watchers. Nothing is deleted: `close_all` is how a quit
    /// ends sessions.
    fn drop(&mut self) {
        for session in lock(&self.sessions).values() {
            session.stop_worker();
        }
    }
}

// ---------------------------------------------------------------- one file

enum Wake {
    /// The system said something in the folder changed.
    Changed,
    /// Try now (the server is back, or a choice was made).
    Now,
    Stop,
}

struct State {
    base: Stamp,
    marker: Marker,
    /// What the server has a newer version of, when the last save found it
    /// changed (the inner None: it's gone).
    conflict: Option<Option<Stamp>>,
    /// The copy has a version the server doesn't.
    pending: bool,
    closed: bool,
    failures: u32,
    retry_at: Option<Instant>,
    /// The copy's hash when last read, and when it last changed.
    last_hash: Option<String>,
    last_change: SystemTime,
    /// The copy's size and time when last looked at (for the poll).
    seen: Option<(u64, i64)>,
    /// The hash of a version the server refused for good (see `sync`).
    refused: Option<String>,
    last: EditEvent,
    uploaded_at: Option<u64>,
}

struct Session {
    id: String,
    host: String,
    remote_path: String,
    name: String,
    local: PathBuf,
    remote: Mutex<Arc<dyn Remote>>,
    shared: Arc<Shared>,
    /// One server call at a time per file, so saves never overtake each other.
    op: Mutex<()>,
    state: Mutex<State>,
    wake: Mutex<Option<Sender<Wake>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    held: Mutex<Option<File>>,
}

impl Session {
    /// Makes the folder and the copy. Anything that fails takes the folder
    /// away again.
    fn create(shared: &Arc<Shared>, host: &str, path: &str, remote: Arc<dyn Remote>, limit: u64) -> Result<Arc<Session>, OpError> {
        let root = shared.root()?;
        let id = new_id();
        root.make_dir(os(&id))?;
        let name = local_file_name(path);
        let made = (|| -> Result<(File, Marker), OpError> {
            let dir = root.child(os(&id))?;
            let held = try_lock(&dir)?.ok_or_else(|| OpError::failed("The new folder for the copy is in use."))?;
            let (bytes, base) = remote.read(path, limit)?;
            let mut file = dir.create(os(&name))?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            let marker = Marker { host: host.into(), remote: path.into(), name: name.clone(), base, created: unix_now(), offered: None };
            write_marker(&dir, &marker)?;
            Ok((held, marker))
        })();
        let (held, marker) = match made {
            Ok(made) => made,
            Err(e) => {
                let _ = root.remove_tree(os(&id));
                return Err(e);
            }
        };
        Ok(Self::build(shared, id, marker, remote, held, false))
    }

    /// A leftover's folder, as a session again.
    fn resume(shared: &Arc<Shared>, id: &str, remote: Arc<dyn Remote>) -> Result<Arc<Session>, OpError> {
        let dir = shared.root()?.child(os(id))?;
        let held = try_lock(&dir)?.ok_or_else(|| OpError::failed("This copy is open in SSHeesh right now."))?;
        let marker = read_marker(&dir).ok_or_else(|| OpError::failed("SSHeesh can't tell where this copy came from."))?;
        Ok(Self::build(shared, id.to_string(), marker, remote, held, true))
    }

    fn build(shared: &Arc<Shared>, id: String, marker: Marker, remote: Arc<dyn Remote>, held: File, pending: bool) -> Arc<Session> {
        let local = shared.root.join(&id).join(&marker.name);
        let state = State {
            base: marker.base.clone(),
            marker: marker.clone(),
            conflict: None,
            pending,
            closed: false,
            failures: 0,
            retry_at: None,
            last_hash: stamp_hash(&marker.base),
            last_change: SystemTime::now(),
            seen: None,
            refused: None,
            last: EditEvent::Opened { local: local.clone() },
            uploaded_at: None,
        };
        Arc::new(Session {
            id,
            host: marker.host,
            remote_path: marker.remote,
            name: marker.name,
            local,
            remote: Mutex::new(remote),
            shared: shared.clone(),
            op: Mutex::new(()),
            state: Mutex::new(state),
            wake: Mutex::new(None),
            worker: Mutex::new(None),
            held: Mutex::new(Some(held)),
        })
    }

    fn dir(&self) -> Result<Dir, OpError> {
        Ok(self.shared.root()?.child(os(&self.id))?)
    }

    fn remote(&self) -> Arc<dyn Remote> {
        lock(&self.remote).clone()
    }

    fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    fn info(&self) -> SessionInfo {
        let st = lock(&self.state);
        SessionInfo {
            id: self.id.clone(),
            host: self.host.clone(),
            remote_path: self.remote_path.clone(),
            local: self.local.clone(),
            last: st.last.clone(),
            uploaded_at: st.uploaded_at,
            pending: st.pending,
            conflict: st.conflict.is_some(),
        }
    }

    fn emit(&self, st: &mut State, event: EditEvent) {
        st.last = event.clone();
        self.shared.emit(&self.id, event);
    }

    fn wake(&self, why: Wake) {
        if let Some(tx) = lock(&self.wake).as_ref() {
            let _ = tx.send(why);
        }
    }

    /// Starts watching the folder. Watching the folder, not the file: many
    /// editors save by writing a new file and renaming it over the old one.
    /// When the system's watcher can't start, the poll alone notices saves.
    fn start(self: &Arc<Self>) {
        let (tx, rx) = channel();
        let name = self.name.clone();
        let fs_tx = tx.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            // Only events about the copy: vim's swap files, an editor's
            // temporary files and SSHeesh's own marker don't count.
            if let Ok(event) = res {
                if event.paths.is_empty() || event.paths.iter().any(|p| p.file_name().is_some_and(|n| n == name.as_str())) {
                    let _ = fs_tx.send(Wake::Changed);
                }
            }
        });
        let watcher = watcher.and_then(|mut w| {
            use notify::Watcher;
            w.watch(&self.shared.root.join(&self.id), notify::RecursiveMode::NonRecursive).map(|_| w)
        });
        *lock(&self.wake) = Some(tx);
        let me = self.clone();
        let handle = std::thread::spawn(move || {
            let _watcher = watcher.ok();
            me.run(rx);
        });
        *lock(&self.worker) = Some(handle);
    }

    fn stop_worker(&self) {
        if let Some(tx) = lock(&self.wake).take() {
            let _ = tx.send(Wake::Stop);
        }
        let handle = lock(&self.worker).take();
        if let Some(handle) = handle {
            if handle.thread().id() != std::thread::current().id() {
                let _ = handle.join();
            }
        }
    }

    /// The watcher's loop: uploads a burst of changes once it's quiet,
    /// looks at the copy every `poll` in case the system missed one, tries
    /// a waiting upload again when its time comes, and closes the session
    /// when it's been idle for `idle_close`.
    fn run(self: Arc<Self>, rx: std::sync::mpsc::Receiver<Wake>) {
        let t = self.shared.timing.clone();
        let mut burst: Option<(Instant, Instant)> = None;
        let mut next_poll = Instant::now() + t.poll;
        loop {
            let now = Instant::now();
            let mut until = next_poll;
            if let Some((first, last)) = burst {
                until = until.min(last + t.debounce).min(first + 4 * t.poll);
            }
            if let Some(at) = lock(&self.state).retry_at {
                until = until.min(at);
            }
            match rx.recv_timeout(until.saturating_duration_since(now)) {
                Ok(Wake::Changed) => {
                    let now = Instant::now();
                    burst = Some(burst.map_or((now, now), |(first, _)| (first, now)));
                }
                Ok(Wake::Now) => {
                    lock(&self.state).retry_at = None;
                    burst = None;
                    self.sync(true);
                }
                Ok(Wake::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            if let Some((first, last)) = burst {
                // A burst goes up once the copy has held still for `debounce`. Only a copy
                // written to without end (or left blank) goes up anyway, after four polls.
                let ceiling = now >= first + 4 * t.poll;
                if now >= last + t.debounce || ceiling {
                    let touched = self.copy_touched();
                    if !ceiling && (touched || self.looks_half_written()) {
                        burst = Some((first, now));
                    } else {
                        burst = None;
                        self.sync(false);
                    }
                }
            }
            let due = lock(&self.state).retry_at.is_some_and(|at| now >= at);
            if due {
                lock(&self.state).retry_at = None;
                self.sync(true);
            }
            if now >= next_poll {
                next_poll = now + t.poll;
                // A change the system's watcher missed waits for quiet like any other.
                if self.copy_touched() {
                    burst = Some(burst.map_or((now, now), |(first, _)| (first, now)));
                }
                if self.idle(&t) {
                    self.sync(false);
                    if self.idle(&t) {
                        self.finish_from_worker();
                        return;
                    }
                }
            }
        }
    }

    /// Whether the copy's size or time changed since last looked at.
    fn copy_touched(&self) -> bool {
        let now = self.dir().ok().and_then(|d| d.stat(os(&self.name)).ok().flatten()).map(|m| (m.size, m.modified));
        let mut st = lock(&self.state);
        let touched = now.is_some() && st.seen != now;
        st.seen = now;
        touched
    }

    /// The copy is empty where the server's file isn't: almost always an editor between
    /// emptying the file and writing it (a plain write does both), so it waits for the
    /// burst's ceiling rather than sending a blank file over a full one.
    fn looks_half_written(&self) -> bool {
        let st = lock(&self.state);
        st.seen.is_some_and(|(size, _)| size == 0) && st.base.size > 0
    }

    fn idle(&self, t: &Timing) -> bool {
        let st = lock(&self.state);
        !st.closed && !st.pending && st.conflict.is_none() && st.last_change.elapsed().is_ok_and(|e| e >= t.idle_close)
    }

    /// Uploads the copy if it has a version the server doesn't, unless a
    /// conflict waits for your choice. A version the server refused (no
    /// permission) is tried again only when `forced` (you asked, or the
    /// session is closing) or once it changes, not at every look.
    fn sync(&self, forced: bool) {
        let _op = lock(&self.op);
        let read = self.dir().and_then(|d| read_copy(&d, &self.name, self.shared.limits.max));
        let mut st = lock(&self.state);
        if st.closed {
            return;
        }
        let bytes = match read {
            Ok(Some(bytes)) => bytes,
            // Cleared away, or the editor removed it: nothing to send.
            Ok(None) => return,
            Err(e) => {
                st.pending = true;
                self.emit(&mut st, EditEvent::Failed { message: e.message });
                return;
            }
        };
        let hash = sha256_hex(&bytes);
        if st.last_hash.as_deref() != Some(hash.as_str()) {
            st.last_hash = Some(hash.clone());
            st.last_change = SystemTime::now();
        }
        if stamp_hash(&st.base).as_deref() == Some(hash.as_str()) {
            // Saved without a change, or changed back: the server has it.
            if st.pending && st.conflict.is_none() {
                st.pending = false;
                st.retry_at = None;
                let at = st.uploaded_at.unwrap_or(st.marker.created);
                self.emit(&mut st, EditEvent::Uploaded { at });
            }
            return;
        }
        st.pending = true;
        if st.conflict.is_some() || (!forced && st.refused.as_deref() == Some(hash.as_str())) {
            return;
        }
        let base = st.base.clone();
        self.emit(&mut st, EditEvent::Uploading);
        drop(st);
        let saved = self.remote().save(&self.remote_path, &bytes, Some(&base));
        let mut st = lock(&self.state);
        match saved {
            Ok(stamp) => self.agreed(&mut st, stamp, false),
            Err(SaveError::Conflict(c)) => {
                st.refused = None;
                st.conflict = Some(c.now.clone());
                st.retry_at = None;
                self.emit(&mut st, EditEvent::Conflict { now: c.now });
            }
            Err(SaveError::Failed(e)) if e.code == ErrorCode::Network => {
                st.failures += 1;
                let wait = (self.shared.timing.backoff)(st.failures);
                st.retry_at = Some(Instant::now() + wait);
                self.emit(&mut st, EditEvent::Offline { pending: true, retry_in_ms: wait.as_millis() as u64, message: e.message });
            }
            Err(SaveError::Failed(e)) => {
                st.failures += 1;
                st.retry_at = None;
                st.refused = Some(hash);
                self.emit(&mut st, EditEvent::Failed { message: e.message });
            }
        }
    }

    /// The server and the copy agree on `stamp`: remember it, here and in
    /// the marker.
    fn agreed(&self, st: &mut State, stamp: Stamp, reloaded: bool) {
        st.base = stamp.clone();
        st.last_hash = stamp_hash(&stamp);
        st.conflict = None;
        st.pending = false;
        st.failures = 0;
        st.retry_at = None;
        st.refused = None;
        st.marker.base = stamp;
        if let Ok(dir) = self.dir() {
            // Without the marker the copy still works; a later start would
            // just think it unsent and offer it.
            let _ = write_marker(&dir, &st.marker);
        }
        let at = unix_now();
        st.uploaded_at = Some(at);
        self.emit(st, if reloaded { EditEvent::Reloaded { at } } else { EditEvent::Uploaded { at } });
    }

    fn read_mine(&self) -> Result<Vec<u8>, OpError> {
        read_copy(&self.dir()?, &self.name, self.shared.limits.max)?.ok_or_else(|| OpError::new(ErrorCode::NotFound, "The copy on this computer isn't there anymore."))
    }

    fn keep_mine(&self) -> Result<(), OpError> {
        let _op = lock(&self.op);
        let Some(now) = lock(&self.state).conflict.clone() else { return Ok(()) };
        let bytes = self.read_mine()?;
        let saved = self.remote().save(&self.remote_path, &bytes, now.as_ref());
        let mut st = lock(&self.state);
        match saved {
            Ok(stamp) => {
                self.agreed(&mut st, stamp, false);
                Ok(())
            }
            Err(SaveError::Conflict(c)) => {
                // Changed again in the meantime: ask again.
                st.conflict = Some(c.now.clone());
                self.emit(&mut st, EditEvent::Conflict { now: c.now.clone() });
                Err(SaveError::Conflict(c).into())
            }
            Err(SaveError::Failed(e)) => Err(e),
        }
    }

    /// Writes `bytes` as the copy, all at once.
    fn write_mine(&self, bytes: &[u8]) -> Result<(), OpError> {
        let dir = self.dir()?;
        write_atomic(&dir, &self.name, bytes)?;
        Ok(())
    }

    fn take_theirs(&self) -> Result<(), OpError> {
        let _op = lock(&self.op);
        let remote = self.remote();
        let size = remote.size(&self.remote_path)?;
        if size > self.shared.limits.max {
            return Err(OpError::new(ErrorCode::TooLarge, format!("The server's version is {}, too big to open here.", size_words(size))));
        }
        self.shared.room_for(size)?;
        let (bytes, stamp) = remote.read(&self.remote_path, self.shared.limits.max)?;
        self.write_mine(&bytes)?;
        let mut st = lock(&self.state);
        st.seen = None;
        self.agreed(&mut st, stamp, true);
        Ok(())
    }

    /// After the copy was cleared away to save space: a fresh one.
    fn refresh_if_gone(&self, remote: &Arc<dyn Remote>) -> Result<(), OpError> {
        *lock(&self.remote) = remote.clone();
        let present = self.dir()?.stat(os(&self.name))?.is_some();
        if present {
            return Ok(());
        }
        let pending = lock(&self.state).pending;
        if pending {
            return Ok(());
        }
        self.take_theirs()
    }

    fn save_mine_as_copy(&self) -> Result<String, OpError> {
        let _op = lock(&self.op);
        let bytes = self.read_mine()?;
        let (dir, name) = (files::parent_dir(&self.remote_path), files::base_name(&self.remote_path));
        let now = unix_now();
        for attempt in 1..=9 {
            let path = files::join_remote(&dir, &copy_name(&name, now, attempt));
            match self.remote().save(&path, &bytes, None) {
                Ok(_) => return Ok(path),
                Err(SaveError::Conflict(_)) => continue,
                Err(SaveError::Failed(e)) => return Err(e),
            }
        }
        Err(OpError::new(ErrorCode::Exists, "There are already copies with that name beside the file."))
    }

    fn compare(&self) -> Result<Comparison, OpError> {
        let _op = lock(&self.op);
        let mine = self.read_mine()?;
        let (theirs, _) = match self.remote().read(&self.remote_path, files::MAX_TEXT) {
            Err(e) if e.code == ErrorCode::TooLarge => return Err(OpError::new(ErrorCode::TooLarge, "The server's version is too big to compare here.")),
            other => other?,
        };
        if mine.len() as u64 > files::MAX_TEXT {
            return Err(OpError::new(ErrorCode::TooLarge, "Your copy is too big to compare here."));
        }
        Ok(compare_bytes(&mine, &theirs))
    }

    /// Clears the copy away to keep the edit folder under its cap, if the
    /// server has it. The session stays: an editor that saves again makes
    /// it a change to upload, and the next "Open" downloads a fresh copy.
    fn evict_if_clean(&self) -> bool {
        let _op = lock(&self.op);
        let Ok(dir) = self.dir() else { return false };
        let read = read_copy(&dir, &self.name, self.shared.limits.max);
        let mut st = lock(&self.state);
        if st.closed || st.pending || st.conflict.is_some() {
            return false;
        }
        match read {
            Ok(Some(bytes)) if stamp_hash(&st.base).as_deref() == Some(sha256_hex(&bytes).as_str()) => {}
            _ => return false,
        }
        if dir.unlink(os(&self.name)).is_err() {
            return false;
        }
        st.seen = None;
        self.emit(&mut st, EditEvent::Evicted);
        true
    }

    fn finish_from_worker(&self) {
        // The worker is this thread: let go of its handle without joining.
        lock(&self.wake).take();
        lock(&self.worker).take();
        self.end(false);
    }

    /// Ends the session: stops watching, makes one last upload when there's
    /// something to send, and deletes the folder when the server has the
    /// copy (or `discard`). Otherwise the folder stays, for the next start
    /// to offer.
    fn finish(&self, discard: bool) -> Closed {
        self.stop_worker();
        if !discard {
            self.sync(true);
        }
        self.end(discard)
    }

    fn end(&self, discard: bool) -> Closed {
        let _op = lock(&self.op);
        let dir = self.dir();
        let mut st = lock(&self.state);
        if st.closed {
            // Closed on its own (idle) before: only ever with nothing unsent.
            return Closed { id: self.id.clone(), remote_path: self.remote_path.clone(), kept: None };
        }
        let clean = discard || (!st.pending && st.conflict.is_none() && dir.as_ref().is_ok_and(|d| uploaded(d, &st.marker, self.shared.limits.max)));
        st.closed = true;
        drop(dir);
        // Released before deleting: Windows can't delete a file that's open.
        lock(&self.held).take();
        if clean {
            // A folder that can't go now (an editor holding the file open on
            // Windows) is fully uploaded, so the next start's sweep deletes it.
            let _ = self.shared.root().and_then(|r| r.remove_tree(os(&self.id)));
        }
        let kept = (!clean).then(|| self.local.clone());
        self.emit(&mut st, EditEvent::Closed { kept: kept.clone() });
        Closed { id: self.id.clone(), remote_path: self.remote_path.clone(), kept }
    }
}

// ---------------------------------------------------------------- compare

/// Both versions and their line diff. `mine` is the copy, `theirs` the
/// server's.
pub fn compare_bytes(mine: &[u8], theirs: &[u8]) -> Comparison {
    let text = |b: &[u8]| std::str::from_utf8(b).ok().filter(|s| !s.contains('\0')).map(str::to_string);
    match (text(mine), text(theirs)) {
        (Some(m), Some(t)) => {
            let lines = line_diff(&m, &t);
            Comparison { mine: m, theirs: t, binary: false, lines }
        }
        _ => Comparison { mine: String::from_utf8_lossy(mine).into(), theirs: String::from_utf8_lossy(theirs).into(), binary: true, lines: Vec::new() },
    }
}

/// How `mine` differs from `theirs`, line by line, in order.
pub fn line_diff(mine: &str, theirs: &str) -> Vec<DiffLine> {
    use imara_diff::{Algorithm, Diff, InternedInput};
    let input = InternedInput::new(theirs, mine);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let line = |kind, token: &imara_diff::Token| DiffLine { kind, text: input.interner[*token].trim_end_matches(['\n', '\r']).to_string() };
    let mut out = Vec::new();
    let mut at = 0usize;
    for hunk in diff.hunks() {
        let (before, after) = (hunk.before.start as usize..hunk.before.end as usize, hunk.after.start as usize..hunk.after.end as usize);
        out.extend(input.before[at..before.start].iter().map(|t| line(DiffKind::Same, t)));
        out.extend(input.before[before.clone()].iter().map(|t| line(DiffKind::Theirs, t)));
        out.extend(input.after[after].iter().map(|t| line(DiffKind::Mine, t)));
        at = before.end;
    }
    out.extend(input.before[at..].iter().map(|t| line(DiffKind::Same, t)));
    out
}

#[cfg(test)]
mod tests;
