//! File browsing on servers (over ssh, using the shared connection) and on
//! this computer.

use crate::error::{ErrorCode, OpError};
use crate::scrub::{self, CleanCopy, Cleaning};
use crate::ssh::{remote_command, run_remote, run_remote_read, shell_quote, spawn_error, SshEnv, SshTarget, Target};
use serde::Serialize;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    Dir,
    File,
    /// A symlink whose target is missing.
    Link,
    Other,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub kind: FileKind,
    pub is_link: bool,
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
    /// Like `drwxr-xr-x`.
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    /// Absolute path after resolving `~`.
    pub path: String,
    pub entries: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFile {
    pub path: String,
    /// None when the file is binary or too large to edit here.
    pub content: Option<String>,
    pub binary: bool,
    pub size: u64,
    /// Not valid UTF-8: `content` shows it with stand-ins for the bytes it
    /// couldn't read, and saving would change those, so it's read-only.
    pub lossy: bool,
    /// Lines end in `\r\n`. `content` has plain `\n` for editing; put the
    /// `\r\n` back to save it.
    pub crlf: bool,
}

pub const MAX_TEXT: u64 = 2 * 1024 * 1024;

fn ssh_only(target: &Target) -> Option<&SshTarget> {
    match target {
        Target::Ssh(t) => Some(t),
        Target::Local => None,
    }
}

// ---------------------------------------------------------------- listing

pub fn list(target: &Target, env: &SshEnv, path: &str) -> Result<DirListing, OpError> {
    match ssh_only(target) {
        Some(t) => list_remote(t, env, path),
        None => list_local(path),
    }
}

/// `cd` into a path that may start with `~`.
fn cd_script(path: &str) -> String {
    format!(
        r#"P={}; case "$P" in ""|"~") cd -- "$HOME" ;; "~/"*) cd -- "$HOME/${{P#"~/"}}" ;; *) cd -- "$P" ;; esac 2>/dev/null || {{ echo "This folder doesn't exist or you can't open it." >&2; exit 3; }}"#,
        shell_quote(path)
    )
}

fn list_remote(t: &SshTarget, env: &SshEnv, path: &str) -> Result<DirListing, OpError> {
    // One `find` lists and describes everything. Only when it fails (it
    // also does over an entry it can't read) does a second one check that
    // it's GNU find, which `-printf` needs.
    let script = format!(
        "{}\npwd\nLC_ALL=C find . -mindepth 1 -maxdepth 1 -printf '%y\\t%Y\\t%s\\t%T@\\t%M\\t%f\\0' 2>/dev/null || find . -maxdepth 0 -printf '' 2>/dev/null || {{ echo 'Browsing files needs GNU find on the server (most Linux servers have it).' >&2; exit 4; }}\nexit 0",
        cd_script(path)
    );
    let out = run_remote_read(t, env, &script)?;
    let (path, mut entries) = parse_find(&out);
    sort_entries(&mut entries);
    Ok(DirListing { path, entries })
}

fn parse_find(out: &[u8]) -> (String, Vec<FileEntry>) {
    let text = String::from_utf8_lossy(out);
    let (pwd, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let mut entries = Vec::new();
    for rec in rest.split('\0') {
        let f: Vec<&str> = rec.splitn(6, '\t').collect();
        if f.len() < 6 {
            continue;
        }
        let is_link = f[0] == "l";
        let kind = match f[1] {
            "d" => FileKind::Dir,
            "f" => FileKind::File,
            "N" | "L" => FileKind::Link,
            _ if is_link => FileKind::Link,
            _ => FileKind::Other,
        };
        entries.push(FileEntry {
            name: f[5].to_string(),
            kind,
            is_link,
            size: f[2].parse().unwrap_or(0),
            modified: f[3].split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0),
            mode: f[4].to_string(),
        });
    }
    (pwd.trim().to_string(), entries)
}

fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by_cached_key(|e| (e.kind != FileKind::Dir, e.name.to_lowercase()));
}

fn resolve_local(path: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    if path.is_empty() || path == "~" {
        home
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else if path.len() == 2 && path.ends_with(':') {
        // "C:" alone means "current folder on C:", we want the drive root.
        PathBuf::from(format!("{path}/"))
    } else {
        PathBuf::from(path)
    }
}

/// Paths shown in the UI always use `/`. On Windows that turns
/// `\\?\C:\Users\me` into `C:/Users/me`, which Windows also accepts.
fn display_path(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) {
        s.trim_start_matches(r"\\?\").replace('\\', "/")
    } else {
        s
    }
}

fn list_local(path: &str) -> Result<DirListing, OpError> {
    let dir = resolve_local(path)
        .canonicalize()
        .map_err(|_| OpError::new(ErrorCode::NotFound, "This folder doesn't exist or you can't open it."))?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(lmeta) = entry.path().symlink_metadata() else { continue };
        let is_link = lmeta.file_type().is_symlink();
        let meta = std::fs::metadata(entry.path()).ok();
        let kind = match &meta {
            Some(m) if m.is_dir() => FileKind::Dir,
            Some(m) if m.is_file() => FileKind::File,
            Some(_) => FileKind::Other,
            None => FileKind::Link,
        };
        let m = meta.as_ref().unwrap_or(&lmeta);
        let modified = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push(FileEntry { name, kind, is_link, size: m.len(), modified, mode: local_mode(&lmeta, kind) });
    }
    sort_entries(&mut entries);
    Ok(DirListing { path: display_path(&dir), entries })
}

#[cfg(unix)]
fn local_mode(meta: &std::fs::Metadata, kind: FileKind) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let t = if meta.file_type().is_symlink() {
        'l'
    } else if kind == FileKind::Dir {
        'd'
    } else {
        '-'
    };
    let mut s = String::from(t);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

#[cfg(not(unix))]
fn local_mode(_meta: &std::fs::Metadata, _kind: FileKind) -> String {
    String::new()
}

// ---------------------------------------------------------------- reading and writing

pub fn read_text(target: &Target, env: &SshEnv, path: &str) -> Result<TextFile, OpError> {
    let (size, bytes) = match ssh_only(target) {
        Some(t) => {
            let script = format!(
                r#"F={}; [ -f "$F" ] || {{ echo "This isn't a regular file." >&2; exit 4; }}; S=$(wc -c < "$F"); S=${{S##* }}; echo "$S"; [ "$S" -gt {MAX_TEXT} ] || cat -- "$F""#,
                shell_quote(path)
            );
            let out = run_remote_read(t, env, &script)?;
            let nl = out.iter().position(|&b| b == b'\n').unwrap_or(out.len());
            let size = String::from_utf8_lossy(&out[..nl]).trim().parse().unwrap_or(0);
            (size, out.get(nl + 1..).unwrap_or_default().to_vec())
        }
        None => {
            let p = resolve_local(path);
            let meta = std::fs::metadata(&p)?;
            // A device or a pipe (/dev/zero, a FIFO) never ends.
            if !meta.is_file() {
                return Err(OpError::failed("This isn't a regular file."));
            }
            let mut bytes = Vec::new();
            if meta.len() <= MAX_TEXT {
                // One byte past the cap tells a file that grew since.
                File::open(&p)?.take(MAX_TEXT + 1).read_to_end(&mut bytes)?;
            }
            (meta.len(), bytes)
        }
    };
    Ok(decode_text(path, size, bytes))
}

/// A file's bytes as text to show and edit.
fn decode_text(path: &str, size: u64, bytes: Vec<u8>) -> TextFile {
    let size = size.max(bytes.len() as u64);
    let mut file = TextFile { path: path.into(), content: None, binary: false, size, lossy: false, crlf: false };
    if size > MAX_TEXT {
        return file;
    }
    if bytes.iter().take(8000).any(|&b| b == 0) {
        file.binary = true;
        return file;
    }
    let text = String::from_utf8(bytes).unwrap_or_else(|e| {
        file.lossy = true;
        String::from_utf8_lossy(e.as_bytes()).into_owned()
    });
    // Windows line endings, when most lines have them.
    let crlf = text.matches("\r\n").count();
    file.crlf = crlf * 2 > text.matches('\n').count();
    file.content = Some(if file.crlf { text.replace("\r\n", "\n") } else { text });
    file
}

/// The script `write_text` runs on a server; the content comes on stdin.
/// It lands in a temporary file first and is copied over the file only once
/// all of it arrived, so a dropped connection never leaves the file cut off.
/// Copying (not moving) keeps the file's owner and permissions.
fn save_script(path: &str, size: usize) -> String {
    format!(
        r#"u=$(umask); umask 077; S=$(mktemp 2>/dev/null) || S="${{TMPDIR:-/tmp}}/.sshee-save.$$"
cat > "$S" || {{ rm -f -- "$S"; exit 1; }}
umask "$u"
[ $(($(wc -c < "$S"))) -eq {size} ] || {{ rm -f -- "$S"; echo 'Saving stopped before the end. The file was left as it was.' >&2; exit 7; }}
cat -- "$S" > {f}; r=$?; rm -f -- "$S"; exit $r"#,
        f = shell_quote(path)
    )
}

pub fn write_text(target: &Target, env: &SshEnv, path: &str, content: &str) -> Result<(), OpError> {
    match ssh_only(target) {
        Some(t) => {
            run_remote(t, env, &save_script(path, content.len()), Some(content.as_bytes().to_vec()))?;
            Ok(())
        }
        None => Ok(std::fs::write(resolve_local(path), content)?),
    }
}

// ---------------------------------------------------------------- folder operations

pub fn make_dir(target: &Target, env: &SshEnv, path: &str) -> Result<(), OpError> {
    match ssh_only(target) {
        Some(t) => run_remote(t, env, &format!("mkdir -p -- {}", shell_quote(path)), None).map(|_| ()),
        None => Ok(std::fs::create_dir_all(resolve_local(path))?),
    }
}

pub fn rename(target: &Target, env: &SshEnv, from: &str, to: &str) -> Result<(), OpError> {
    let exists = || OpError::new(ErrorCode::Exists, "Something with that name already exists here.");
    match ssh_only(target) {
        Some(t) => {
            let script = format!(
                r#"[ -e {to} ] && {{ echo EXISTS >&2; exit 5; }}; mv -- {from} {to}"#,
                from = shell_quote(from),
                to = shell_quote(to)
            );
            run_remote(t, env, &script, None).map(|_| ()).map_err(|e| if e.message == "EXISTS" { exists() } else { e })
        }
        None => {
            let to = resolve_local(to);
            if to.exists() {
                return Err(exists());
            }
            Ok(std::fs::rename(resolve_local(from), to)?)
        }
    }
}

fn copy_here(from: &Path, dir: &Path, overwrite: bool) -> Result<(), OpError> {
    let name = from.file_name().ok_or_else(|| OpError::failed("That path has no file name."))?;
    let dest = dir.join(name);
    if dest == from {
        return Err(OpError::failed("That's already in this folder."));
    }
    // The same checks on the real paths, past links and "..". A folder
    // copied into itself, or a folder inside it, would never end.
    if let (Ok(src), Ok(into)) = (from.canonicalize(), dir.canonicalize()) {
        if into.join(name) == src {
            return Err(OpError::failed("That's already in this folder."));
        }
        if src.is_dir() && into.starts_with(&src) {
            return Err(OpError::failed("A folder can't be copied into itself."));
        }
    }
    if dest.exists() && !overwrite {
        return Err(OpError::new(ErrorCode::Exists, exists_message(&[name.to_string_lossy().into_owned()])));
    }
    // The link you picked is followed; links inside a folder are copied as
    // links, so one that points back up can't loop.
    fn copy_all(from: &Path, to: &Path, follow: bool) -> std::io::Result<()> {
        let meta = if follow { std::fs::metadata(from)? } else { std::fs::symlink_metadata(from)? };
        if meta.is_symlink() {
            copy_link(from, to)
        } else if meta.is_dir() {
            std::fs::create_dir_all(to)?;
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                copy_all(&entry.path(), &to.join(entry.file_name()), false)?;
            }
            Ok(())
        } else {
            std::fs::copy(from, to).map(|_| ())
        }
    }
    Ok(copy_all(from, &dest, true)?)
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
        std::fs::remove_file(to)?;
    }
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

/// Making links on Windows needs extra rights: a linked file is copied, a
/// linked folder left out.
#[cfg(not(unix))]
fn copy_link(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::metadata(from).is_ok_and(|m| m.is_file()) {
        std::fs::copy(from, to).map(|_| ())
    } else {
        Ok(())
    }
}

/// Shell that sets `"$@"` to `items` (quoted names) for tar, a link to a
/// folder as `link/.`: tar then packs the folder it points to, under the
/// link's name. Links further in stay links, so nothing loops.
fn tar_items(items: &str) -> String {
    format!(r#"set --; for n in {items}; do [ -L "$n" ] && [ -d "$n" ] && n="$n/."; set -- "$@" "$n"; done"#)
}

/// `tar_items` for a path on this computer.
fn local_tar_item(path: &Path, name: &str) -> String {
    let link = path.symlink_metadata().is_ok_and(|m| m.is_symlink());
    if link && path.is_dir() {
        format!("{name}/.")
    } else {
        name.to_string()
    }
}

/// Packs `names` (all inside `dir`) into `dir/archive` as a .tar.gz.
pub fn archive(target: &Target, env: &SshEnv, dir: &str, names: &[String], archive: &str) -> Result<(), OpError> {
    if names.is_empty() {
        return Err(OpError::failed("Pick something to put in the archive first."));
    }
    let exists = || OpError::new(ErrorCode::Exists, "Something with that name already exists here.");
    match ssh_only(target) {
        Some(t) => {
            let items = names.iter().map(|n| shell_quote(n)).collect::<Vec<_>>().join(" ");
            let script = format!(
                // "./": GNU tar takes a name with ":" in it as host:file and runs ssh to reach it.
                r#"cd -- {dir} || exit 1; [ -e {name} ] && {{ echo EXISTS >&2; exit 5; }}; {set}; tar -czf ./{name} -- "$@""#,
                dir = shell_quote(dir),
                name = shell_quote(archive),
                set = tar_items(&items),
            );
            run_remote(t, env, &script, None).map(|_| ()).map_err(|e| if e.message == "EXISTS" { exists() } else { e })
        }
        None => {
            let base = resolve_local(dir);
            if base.join(archive).exists() {
                return Err(exists());
            }
            // tar ships with macOS, Linux and Windows 10 and newer.
            let items = names.iter().map(|n| local_tar_item(&base.join(n), n));
            let mut cmd = std::process::Command::new(crate::ssh::program("tar"));
            cmd.current_dir(&base).arg("-czf").arg(Path::new(".").join(archive)).arg("--").args(items);
            crate::ssh::hide_console(&mut cmd);
            let out = cmd.output()?;
            if out.status.success() {
                Ok(())
            } else {
                Err(OpError::failed(format!("Couldn't make the archive: {}", String::from_utf8_lossy(&out.stderr).trim())))
            }
        }
    }
}

pub fn delete(target: &Target, env: &SshEnv, path: &str) -> Result<(), OpError> {
    let trimmed = path.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "~" || trimmed == "/" || !trimmed.contains('/') {
        return Err(OpError::failed("Refusing to delete that path."));
    }
    match ssh_only(target) {
        Some(t) => run_remote(t, env, &format!("rm -rf -- {}", shell_quote(path)), None).map(|_| ()),
        None => {
            let p = resolve_local(path);
            let meta = p.symlink_metadata()?;
            if meta.is_dir() {
                std::fs::remove_dir_all(p)?;
            } else {
                std::fs::remove_file(p)?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- transfers

/// Where a transfer stands, for the app to show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Progress {
    pub done: u64,
    /// 0 while the size isn't known.
    pub total: u64,
    /// The connection dropped; it carries on once the server answers again.
    pub waiting: bool,
    /// What came out of the files so far; None when they go uncleaned.
    pub cleaning: Option<Cleaning>,
}

/// An upload or download the app shows while it runs: bytes moved out of the
/// total, and a way to stop it from another thread.
pub struct Transfer {
    cancelled: AtomicBool,
    waiting: AtomicBool,
    total: AtomicU64,
    done: AtomicU64,
    last_report: Mutex<Option<Instant>>,
    cleaning: Mutex<Option<Cleaning>>,
    report: Box<dyn Fn(Progress) + Send + Sync>,
}

impl Transfer {
    /// `report` hears about progress a few times a second, and at once when
    /// the connection drops or comes back.
    pub fn new(report: impl Fn(Progress) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            total: AtomicU64::new(0),
            done: AtomicU64::new(0),
            last_report: Mutex::new(None),
            cleaning: Mutex::new(None),
            report: Box::new(report),
        })
    }

    /// A transfer nobody watches.
    pub fn quiet() -> Arc<Self> {
        Self::new(|_| {})
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Err once cancelled, for checks between steps.
    pub fn check(&self) -> Result<(), OpError> {
        if self.is_cancelled() {
            Err(OpError::new(ErrorCode::Cancelled, "Cancelled."))
        } else {
            Ok(())
        }
    }

    pub fn add_total(&self, bytes: u64) {
        self.total.fetch_add(bytes, Ordering::Relaxed);
        self.report_now();
    }

    fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    /// Progress during a copy; reported at most ten times a second.
    fn set_done(&self, bytes: u64) {
        self.done.store(bytes, Ordering::Relaxed);
        {
            let mut last = self.last_report.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < Duration::from_millis(100)) {
                return;
            }
            *last = Some(Instant::now());
        }
        self.report_now();
    }

    /// Progress after a whole item, always reported.
    fn settle(&self, bytes: u64) {
        self.done.store(bytes, Ordering::Relaxed);
        self.report_now();
    }

    /// The total, once a file turned out another size than counted: its
    /// cleaned copy.
    fn retotal(&self, was: u64, now: u64) {
        if now >= was {
            self.total.fetch_add(now - was, Ordering::Relaxed);
        } else {
            self.total.fetch_sub(was - now, Ordering::Relaxed);
        }
    }

    /// Counts what cleaning came to for one more file or folder, and says so at once.
    fn cleaned(&self, count: impl FnOnce(&mut Cleaning)) {
        count(self.cleaning.lock().unwrap().get_or_insert_with(Cleaning::default));
        self.report_now();
    }

    fn set_waiting(&self, waiting: bool) {
        if self.waiting.swap(waiting, Ordering::Relaxed) != waiting {
            self.report_now();
        }
    }

    fn report_now(&self) {
        (self.report)(Progress {
            done: self.done(),
            total: self.total.load(Ordering::Relaxed),
            waiting: self.waiting.load(Ordering::Relaxed),
            cleaning: self.cleaning.lock().unwrap().clone(),
        });
    }
}

/// How long a transfer waits for a dropped connection to come back, counted
/// from the last time it got anywhere.
const RECONNECT_FOR: Duration = Duration::from_secs(10 * 60);

/// Runs `attempt`, and runs it again whenever the connection drops midway,
/// once every server in `on` answers again. Each attempt picks up where the
/// last one left off (or starts over, for what can't be picked up). Gives up
/// after `RECONNECT_FOR` without getting any further, or when stopped.
fn resuming<T>(on: &[&SshTarget], env: &SshEnv, xfer: &Transfer, mut attempt: impl FnMut() -> Result<T, OpError>) -> Result<T, OpError> {
    let mut lost: Option<(Instant, u64)> = None;
    loop {
        let dropped = match attempt() {
            Err(e) if e.code == ErrorCode::Network && !xfer.is_cancelled() => e,
            other => return other,
        };
        // A drop after real progress gets the whole wait again.
        let since = match lost {
            Some((at, done)) if done == xfer.done() => at,
            _ => Instant::now(),
        };
        lost = Some((since, xfer.done()));
        xfer.set_waiting(true);
        let back = wait_for(on, env, xfer, since);
        xfer.set_waiting(false);
        back.map_err(|e| if e.code == ErrorCode::Cancelled { e } else { dropped })?;
    }
}

/// Waits until every one of `servers` answers again, asking more slowly as
/// time goes on. Err when stopped or once `RECONNECT_FOR` has passed `since`.
fn wait_for(servers: &[&SshTarget], env: &SshEnv, xfer: &Transfer, since: Instant) -> Result<(), OpError> {
    let mut pause = Duration::from_secs(1);
    loop {
        let until = Instant::now() + pause;
        while Instant::now() < until {
            xfer.check()?;
            std::thread::sleep(Duration::from_millis(100));
        }
        if servers.iter().all(|t| answers(t, env, xfer)) {
            return Ok(());
        }
        xfer.check()?;
        if since.elapsed() > RECONNECT_FOR {
            return Err(OpError::new(ErrorCode::Network, "The server didn't come back."));
        }
        pause = (pause * 2).min(Duration::from_secs(5));
    }
}

/// Whether the server takes a command now. A connection that hangs instead
/// of failing counts as no after 20 seconds; Stop ends the wait at once.
fn answers(t: &SshTarget, env: &SshEnv, xfer: &Transfer) -> bool {
    let mut cmd = remote_command(t, env, "true");
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let Ok(mut ssh) = cmd.spawn() else { return false };
    let start = Instant::now();
    loop {
        match ssh.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if !xfer.is_cancelled() && start.elapsed() < Duration::from_secs(20) => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = ssh.kill();
                let _ = ssh.wait();
                return false;
            }
        }
    }
}

/// Moves bytes from `from` to `to` on a thread of its own, telling `moved`
/// the running count. `to` is dropped at the end, which closes the pipe it
/// feeds.
fn spawn_copy<R, W, F>(mut from: R, mut to: W, xfer: Arc<Transfer>, moved: F) -> JoinHandle<io::Result<()>>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
    F: Fn(&Transfer, u64) + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 128 * 1024];
        let mut count = 0u64;
        while !xfer.is_cancelled() {
            let n = match from.read(&mut buf) {
                Ok(0) => return to.flush(),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            to.write_all(&buf[..n])?;
            count += n as u64;
            moved(&xfer, count);
        }
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
    })
}

/// Waits for a copy while watching for Cancel. Cancelling ends `procs` (ssh,
/// tar), which also frees a copy stuck on a stalled connection.
fn wait_copy(xfer: &Transfer, copy: JoinHandle<io::Result<()>>, procs: &mut [&mut Child]) -> Result<io::Result<()>, OpError> {
    let mut stopped: Option<Instant> = None;
    loop {
        if stopped.is_none() && xfer.is_cancelled() {
            for p in procs.iter_mut() {
                let _ = p.kill();
                let _ = p.wait();
            }
            stopped = Some(Instant::now());
        }
        // A copy that doesn't notice within a moment holds nothing but pipes;
        // leave it to finish on its own.
        if copy.is_finished() || stopped.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    xfer.check()?;
    Ok(copy.join().unwrap_or_else(|_| Err(io::Error::other("the copy stopped unexpectedly"))))
}

/// Collects a process's error output on the side, so a chatty one can't
/// stall on a full pipe while we're busy with its other end.
fn drain(stderr: Option<ChildStderr>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(mut e) = stderr {
            let _ = e.read_to_end(&mut out);
        }
        String::from_utf8_lossy(&out).into_owned()
    })
}

fn joined(errors: JoinHandle<String>) -> String {
    errors.join().unwrap_or_default()
}

/// 64 random bits, from the randomly keyed hasher std seeds from the OS.
pub(crate) fn unguessable() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new().build_hasher().finish()
}

/// A hidden name for a download in progress, unique within this run.
fn scratch_name(name: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(".{name}.{}-{}.sshee-part", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Size of a server file, or roughly of a folder (its disk usage), for the
/// progress bar. 0 when it can't be told.
fn remote_size(t: &SshTarget, env: &SshEnv, path: &str, is_dir: bool) -> u64 {
    run_remote(t, env, &size_script(path, is_dir), None)
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out).split_whitespace().next()?.parse::<u64>().ok())
        .map_or(0, |n| if is_dir { n * 1024 } else { n })
}

/// The script for `remote_size`. `/.` makes du count the folder a link
/// points to (as `download` packs it), not the link.
fn size_script(path: &str, is_dir: bool) -> String {
    let p = shell_quote(path);
    if is_dir {
        format!("du -sk -- {p}/.")
    } else {
        // GNU stat, then BSD stat.
        format!("stat -L -c %s -- {p} 2>/dev/null || stat -L -f %z -- {p}")
    }
}

/// The script `download` runs to pack a folder: `name` inside `parent`,
/// as a tar stream on stdout.
fn pack_script(parent: &str, name: &str) -> String {
    format!("cd -- {} || exit 1; {}; tar -cf - -- \"$@\"", shell_quote(parent), tar_items(&shell_quote(name)))
}

/// What a local file or folder holds: bytes of file content, and the size of
/// the tar stream that carries it (a 512-byte header per entry, content
/// padded to 512, two empty blocks at the end, all padded to 10 KiB). The
/// path itself may be a link, which is followed, as `upload` does.
fn local_tree_size(path: &Path) -> (u64, u64) {
    fn walk(p: &Path, acc: &mut (u64, u64), follow: bool) {
        let meta = if follow { p.metadata() } else { p.symlink_metadata() };
        let Ok(meta) = meta else { return };
        acc.1 += 512;
        if meta.is_file() {
            acc.0 += meta.len();
            acc.1 += meta.len().div_ceil(512) * 512;
        } else if meta.is_dir() {
            for entry in std::fs::read_dir(p).into_iter().flatten().flatten() {
                walk(&entry.path(), acc, false);
            }
        }
    }
    let mut acc = (0, 0);
    walk(path, &mut acc, true);
    (acc.0, (acc.1 + 1024).div_ceil(10240) * 10240)
}

/// How many bytes `upload` will send for this path, for the transfer's total.
pub fn upload_size(path: &Path) -> u64 {
    local_tree_size(path).0
}

/// `"a.txt" already exists in this folder.`, or a few names and how many
/// more.
fn exists_message(names: &[String]) -> String {
    let q = |n: &String| format!("\"{n}\"");
    match names {
        [] => String::new(),
        [one] => format!("{} already exists in this folder.", q(one)),
        [rest @ .., last] if names.len() <= 5 => {
            format!("{} and {} already exist in this folder.", rest.iter().map(q).collect::<Vec<_>>().join(", "), q(last))
        }
        _ => format!(
            "{} and {} more already exist in this folder.",
            names[..4].iter().map(q).collect::<Vec<_>>().join(", "),
            names.len() - 4
        ),
    }
}

/// Refuses an upload into `dir` that would replace anything there, naming
/// all of it, so one question covers the whole batch before anything moves.
pub fn check_upload(target: &Target, env: &SshEnv, local_paths: &[PathBuf], dir: &str) -> Result<(), OpError> {
    let names: Vec<String> = local_paths.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    match ssh_only(target) {
        Some(t) => check_free(t, env, &names, dir),
        None => {
            let dir = resolve_local(dir);
            let taken: Vec<String> = names.into_iter().filter(|n| dir.join(n).symlink_metadata().is_ok()).collect();
            if taken.is_empty() {
                Ok(())
            } else {
                Err(OpError::new(ErrorCode::Exists, exists_message(&taken)))
            }
        }
    }
}

/// `check_upload` on a server, for names.
fn check_free(t: &SshTarget, env: &SshEnv, names: &[String], dir: &str) -> Result<(), OpError> {
    let mut taken = Vec::new();
    // In batches, to stay well inside the longest command a server takes.
    for batch in names.chunks(200) {
        let out = run_remote_read(t, env, &taken_script(dir, batch))?;
        taken.extend(taken_in(&out, batch));
    }
    if taken.is_empty() {
        Ok(())
    } else {
        Err(OpError::new(ErrorCode::Exists, exists_message(&taken)))
    }
}

/// A script that prints the place in `names` of each one that's in `dir`,
/// a line each. Nothing when the folder can't be opened: the upload says why.
fn taken_script(dir: &str, names: &[String]) -> String {
    let items = names.iter().map(|n| shell_quote(n)).collect::<Vec<_>>().join(" ");
    format!(
        r#"cd -- {} 2>/dev/null || exit 0; i=0; for n in {items}; do if [ -e "$n" ] || [ -L "$n" ]; then echo "$i"; fi; i=$((i+1)); done"#,
        shell_quote(dir)
    )
}

/// The names `taken_script` printed.
fn taken_in(out: &[u8], names: &[String]) -> Vec<String> {
    String::from_utf8_lossy(out).lines().filter_map(|l| names.get(l.trim().parse::<usize>().ok()?).cloned()).collect()
}

/// The script `upload` runs on a server for one file; the file comes on
/// stdin, and `s` and `d` (quoted) are where it's staged and where it goes.
/// It arrives from byte `from` on: after a dropped connection the rest is
/// added to what's staged, as long as that's still `from` bytes (else it
/// says CHANGED and exits 6). If ssh goes away midway, cat on the server just
/// sees the end of its input, so the size check is what tells a whole file
/// from a cut one; a cut one stays staged to be picked up. A file that's
/// there is then written over in place, which keeps its owner and
/// permissions (and a link that points to it). When that can't be done (no
/// write permission, a program running from it) or nothing's there yet,
/// the staged file takes its place.
fn upload_file_script(guard: &str, s: &str, d: &str, size: u64, from: u64) -> String {
    // The staged file is made new (set -C refuses one that's there), so in a
    // folder others can write to, such as /tmp, nobody else's file under
    // that name receives the upload; nor does a link, when picking it up.
    let receive = if from == 0 {
        format!("rm -f -- {s}; (set -C; : > {s}) 2>/dev/null && [ -f {s} ] && [ ! -L {s} ] || {{ echo \"Something else already holds this upload's temporary file.\" >&2; exit 1; }}\ncat >> {s} || exit 1")
    } else {
        format!("{{ [ -f {s} ] && [ ! -L {s} ] && [ $(($(wc -c < {s}))) -eq {from} ]; }} || {{ echo CHANGED >&2; exit 6; }}\ncat >> {s} || exit 1")
    };
    format!(
        "{guard}[ -d {d} ] && {{ echo 'A folder with this name is already here.' >&2; exit 1; }}
{receive}
[ $(($(wc -c < {s}))) -eq {size} ] || {{ echo 'The upload stopped before the end.' >&2; exit 7; }}
if [ -f {d} ] && cat -- {s} 2>/dev/null > {d}; then rm -f -- {s}; else mv -f -- {s} {d} || {{ rm -f -- {s}; exit 1; }}; fi"
    )
}

/// How much of a file upload is already staged on the server, once it
/// stops growing: the server may still be writing what arrived just before
/// the connection dropped. None when there's nothing to pick up.
fn staged_size(t: &SshTarget, env: &SshEnv, staging: &str) -> Option<u64> {
    let s = shell_quote(staging);
    let script = format!("[ -f {s} ] || exit 0; a=$(($(wc -c < {s}))); sleep 1; b=$(($(wc -c < {s}))); [ \"$a\" = \"$b\" ] && echo \"$b\"");
    String::from_utf8_lossy(&run_remote(t, env, &script, None).ok()?).trim().parse().ok()
}

/// Removes the hidden folder of an upload that didn't finish, in the
/// background. tar on the server can run on for a moment after we hang up,
/// but once its folder is gone it can't write anything back, so one retry
/// covers it.
fn discard_remote(t: &SshTarget, env: &SshEnv, path: &str) {
    let (t, env, p) = (t.clone(), env.clone(), shell_quote(path));
    std::thread::spawn(move || {
        let _ = run_remote(&t, &env, &format!("rm -rf -- {p} 2>/dev/null || {{ sleep 1; rm -rf -- {p}; }}"), None);
    });
}

/// A server file's size, and a stamp that changes when the file does (its
/// size and modification time), so a download only picks up where it left
/// off on the very same file.
fn file_ident(t: &SshTarget, env: &SshEnv, path: &str) -> Option<(u64, String)> {
    let out = run_remote(t, env, &ident_script(path), None).ok()?;
    let id = String::from_utf8_lossy(&out).trim().to_string();
    let size = id.split(':').next()?.parse().ok()?;
    Some((size, id))
}

/// The script for `file_ident`: GNU and BusyBox stat, then BSD stat.
fn ident_script(path: &str) -> String {
    let p = shell_quote(path);
    format!("stat -L -c %s:%Y -- {p} 2>/dev/null || stat -L -f %z:%m -- {p}")
}

/// Sends the rest of a file from byte `from` on, if it's still the file
/// stamped `id`; otherwise says CHANGED and exits 6.
fn resume_script(path: &str, id: &str, from: u64) -> String {
    format!(
        "[ \"$({})\" = {} ] || {{ echo CHANGED >&2; exit 6; }}; tail -c +{} -- {}",
        ident_script(path),
        shell_quote(id),
        from + 1,
        shell_quote(path)
    )
}

/// Copies a remote file or folder into `dest_dir` (usually Downloads) and
/// returns where it landed. Never overwrites: picks "name (1)" instead. If
/// the connection drops, it waits for the server and goes on: a file from
/// where it stopped, a folder from the start.
pub fn download(target: &Target, env: &SshEnv, remote_path: &str, is_dir: bool, dest_dir: &Path, xfer: &Arc<Transfer>) -> Result<String, OpError> {
    let t = ssh_only(target).ok_or_else(|| OpError::new(ErrorCode::Unsupported, "This file is already on your computer."))?;
    let remote_name = base_name(remote_path);
    let name = local_name(&remote_name, cfg!(windows)).ok_or_else(|| OpError::failed(format!("\"{remote_name}\" can't be saved under that name on this computer.")))?;
    std::fs::create_dir_all(dest_dir)?;
    let scratch = dest_dir.join(scratch_name(&name));

    if !is_dir {
        let mut ident = file_ident(t, env, remote_path);
        xfer.add_total(ident.as_ref().map_or(0, |i| i.0));
        xfer.check()?;
        let base = xfer.done();
        File::create(&scratch)?;
        let result = resuming(&[t], env, xfer, || loop {
            let mut have = std::fs::metadata(&scratch)?.len();
            let script = match &ident {
                Some((_, id)) if have > 0 => resume_script(remote_path, id, have),
                _ => {
                    have = 0;
                    File::create(&scratch)?;
                    format!("cat -- {}", shell_quote(remote_path))
                }
            };
            xfer.settle(base + have);
            let file = std::fs::OpenOptions::new().append(true).open(&scratch)?;
            let mut cmd = remote_command(t, env, &script);
            cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut ssh = cmd.spawn().map_err(spawn_error)?;
            let errors = drain(ssh.stderr.take());
            let stream = ssh.stdout.take().expect("piped stdout");
            let copy = spawn_copy(stream, file, xfer.clone(), move |x, n| x.set_done(base + have + n));
            let copied = wait_copy(xfer, copy, &mut [&mut ssh])?;
            let status = ssh.wait()?;
            if !status.success() {
                let e = OpError::from_ssh(status.code(), &joined(errors));
                if e.message == "CHANGED" {
                    // Changed on the server meanwhile: start over with the new one.
                    ident = file_ident(t, env, remote_path);
                    File::create(&scratch)?;
                    continue;
                }
                return Err(e);
            }
            copied?;
            return Ok(());
        })
        .and_then(|()| {
            let final_path = unique_path(dest_dir, &name);
            std::fs::rename(&scratch, &final_path)?;
            xfer.settle(xfer.done());
            Ok(final_path.display().to_string())
        });
        if result.is_err() {
            let _ = std::fs::remove_file(&scratch);
        }
        return result;
    }

    // Folders travel as a tar stream: remote tar -> local tar. A tar stream
    // can't be picked up midway, so after a drop the folder starts over.
    xfer.add_total(remote_size(t, env, remote_path, true));
    xfer.check()?;
    let base = xfer.done();
    let result = resuming(&[t], env, xfer, || {
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch)?;
        xfer.settle(base);
        let mut untar = Command::new(crate::ssh::program("tar"));
        untar.args(["-xf", "-", "-C"]).arg(&scratch).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        crate::ssh::hide_console(&mut untar);
        let mut untar = untar.spawn().map_err(|e| OpError::failed(format!("Couldn't run tar on this computer: {e}")))?;
        let mut cmd = remote_command(t, env, &pack_script(&parent_dir(remote_path), &remote_name));
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut ssh = match cmd.spawn() {
            Ok(ssh) => ssh,
            Err(e) => {
                let _ = untar.kill();
                let _ = untar.wait();
                return Err(spawn_error(e));
            }
        };
        let (errors, unpack_errors) = (drain(ssh.stderr.take()), drain(untar.stderr.take()));
        let stream = ssh.stdout.take().expect("piped stdout");
        let unpack = untar.stdin.take().expect("piped stdin");
        let copy = spawn_copy(stream, unpack, xfer.clone(), move |x, n| x.set_done(base + n));
        let copied = wait_copy(xfer, copy, &mut [&mut ssh, &mut untar])?;
        let (status, unpacked) = (ssh.wait()?, untar.wait()?);
        if !status.success() {
            return Err(OpError::from_ssh(status.code(), &joined(errors)));
        }
        if !unpacked.success() {
            return Err(OpError::failed(format!("Unpacking failed: {}", joined(unpack_errors).trim())));
        }
        copied?;
        Ok(())
    })
    .and_then(|()| {
        // The one thing tar put there, under the server's name for it.
        let mut unpacked = std::fs::read_dir(&scratch)?;
        let (Some(folder), None) = (unpacked.next().transpose()?, unpacked.next()) else {
            return Err(OpError::failed("Unpacking failed: the folder didn't arrive as one folder."));
        };
        let final_path = unique_path(dest_dir, &name);
        std::fs::rename(folder.path(), &final_path)?;
        xfer.settle(xfer.done());
        Ok(final_path.display().to_string())
    });
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// Copies a file or folder from this computer into `dir` on the target: over
/// ssh for a server, a plain copy for this computer's own file manager.
/// Progress adds up in `xfer`, whose total the caller sets (`upload_size`).
/// With `clean`, a server gets a file without what it says about where it
/// came from (`scrub`), and a folder without its dates, owner or Mac extras.
pub fn upload(target: &Target, env: &SshEnv, local_path: &Path, remote_dir: &str, overwrite: bool, clean: bool, xfer: &Arc<Transfer>) -> Result<(), OpError> {
    xfer.check()?;
    let base = xfer.done();
    let (size, tar_size) = local_tree_size(local_path);
    let Some(t) = ssh_only(target) else {
        copy_here(local_path, &resolve_local(remote_dir), overwrite)?;
        xfer.settle(base + size);
        return Ok(());
    };
    let name = local_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| OpError::failed("That path has no file name."))?;
    let dest = join_remote(remote_dir, &name);
    // Both kinds land under a hidden name first, so a stopped or broken
    // upload never leaves a cut-off file or half a folder in its place.
    // A part nobody can guess, so nobody else can have a file (or a pipe) waiting under the name.
    let staging = join_remote(remote_dir, &format!(".{name}.{:016x}.sshee-part", unguessable()));
    let (d, s) = (shell_quote(&dest), shell_quote(&staging));
    let guard = if overwrite {
        String::new()
    } else {
        format!("{{ [ -e {d} ] || [ -L {d} ]; }} && {{ echo EXISTS >&2; exit 5; }}; ")
    };
    let exists = |e: OpError| {
        if e.message == "EXISTS" {
            OpError::new(ErrorCode::Exists, exists_message(std::slice::from_ref(&name)))
        } else {
            e
        }
    };

    if local_path.is_file() {
        // A cleaned copy goes instead, made once, so picking up after a drop
        // goes on with the very same bytes. Only the content travels, so the
        // server's file is dated by the upload, with nothing else attached.
        let copy = if clean { clean_for_upload(local_path, &name, xfer)? } else { None };
        let (source, size) = match &copy {
            Some(c) => {
                xfer.retotal(size, c.len());
                (c.path(), c.len())
            }
            None => (local_path, size),
        };
        let mut first = true;
        let result = resuming(&[t], env, xfer, || {
            // After a drop, the rest goes on from what's staged.
            let mut from = if first { 0 } else { staged_size(t, env, &staging).filter(|&n| n <= size).unwrap_or(0) };
            first = false;
            loop {
                xfer.settle(base + from);
                let mut source = File::open(source)?;
                source.seek(SeekFrom::Start(from))?;
                let mut cmd = remote_command(t, env, &upload_file_script(&guard, &s, &d, size, from));
                cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
                let mut ssh = cmd.spawn().map_err(spawn_error)?;
                let errors = drain(ssh.stderr.take());
                let input = ssh.stdin.take().expect("piped stdin");
                let start = base + from;
                let copy = spawn_copy(source, input, xfer.clone(), move |x, n| x.set_done((start + n).min(base + size)));
                let copied = wait_copy(xfer, copy, &mut [&mut ssh])?;
                let status = ssh.wait()?;
                match copied {
                    // The server hanging up early is explained by its exit status.
                    Err(e) if e.kind() != io::ErrorKind::BrokenPipe => return Err(e.into()),
                    _ => {}
                }
                if !status.success() {
                    let e = exists(OpError::from_ssh(status.code(), &joined(errors)));
                    if e.message == "CHANGED" && from > 0 {
                        from = 0;
                        continue;
                    }
                    return Err(e);
                }
                return Ok(());
            }
        });
        return match result {
            Ok(()) => {
                xfer.settle(base + size);
                Ok(())
            }
            Err(e) => {
                discard_remote(t, env, &staging);
                Err(e)
            }
        };
    }

    // A new folder unpacks into the hidden one and moves into place once
    // everything arrived. Replacing unpacks over the folder that's there.
    // A tar stream can't be picked up midway, so after a drop it starts over.
    // Cleaned, what it unpacks is dated now and owned by whoever unpacks it.
    if clean {
        xfer.cleaned(|c| c.folders += 1);
    }
    let unpack = if clean { "tar -xmof -" } else { "tar -xf -" };
    let script = if overwrite {
        format!("cd -- {} && {unpack}", shell_quote(remote_dir))
    } else {
        format!("{guard}rm -rf -- {s} && mkdir -- {s} && cd -- {s} && {unpack}")
    };
    let parent = local_path.parent().unwrap_or(Path::new("."));
    // The tar stream is a little bigger than the files in it; count in file bytes.
    let scaled = move |n: u64| if tar_size == 0 { 0 } else { (n as u128 * size as u128 / tar_size as u128).min(size as u128) as u64 };
    let result = resuming(&[t], env, xfer, || {
        xfer.settle(base);
        let mut pack = Command::new(crate::ssh::program("tar"));
        if clean && cfg!(target_os = "macos") {
            // The Mac's tar brings extended attributes along, as ._ files
            // among them: quarantine, and the address a file was downloaded from.
            pack.args(["--no-mac-metadata", "--no-xattrs", "--no-acls", "--no-fflags"]).env("COPYFILE_DISABLE", "1");
        }
        pack.arg("-C").arg(parent).args(["-cf", "-", "--"]).arg(local_tar_item(local_path, &name)).stdout(Stdio::piped()).stderr(Stdio::piped());
        crate::ssh::hide_console(&mut pack);
        let mut pack = pack.spawn().map_err(|e| OpError::failed(format!("Couldn't run tar on this computer: {e}")))?;
        let mut cmd = remote_command(t, env, &script);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut ssh = match cmd.spawn() {
            Ok(ssh) => ssh,
            Err(e) => {
                let _ = pack.kill();
                let _ = pack.wait();
                return Err(spawn_error(e));
            }
        };
        let (errors, pack_errors) = (drain(ssh.stderr.take()), drain(pack.stderr.take()));
        let stream = pack.stdout.take().expect("piped stdout");
        let input = ssh.stdin.take().expect("piped stdin");
        let copy = spawn_copy(stream, input, xfer.clone(), move |x, n| x.set_done(base + scaled(n)));
        let copied = wait_copy(xfer, copy, &mut [&mut pack, &mut ssh])?;
        let (status, packed) = (ssh.wait()?, pack.wait()?);
        match copied {
            Err(e) if e.kind() != io::ErrorKind::BrokenPipe => return Err(e.into()),
            _ => {}
        }
        if !status.success() {
            return Err(exists(OpError::from_ssh(status.code(), &joined(errors))));
        }
        if !packed.success() {
            return Err(OpError::failed(format!("Couldn't pack the folder: {}", joined(pack_errors).trim())));
        }
        if !overwrite {
            let finish = format!("{guard}mv -- {s}/{n} {d} && rmdir -- {s}", n = shell_quote(&name));
            run_remote(t, env, &finish, None).map_err(exists)?;
        }
        Ok(())
    });
    match result {
        Ok(()) => {
            xfer.settle(base + size);
            Ok(())
        }
        Err(e) => {
            if !overwrite {
                discard_remote(t, env, &staging);
            }
            Err(e)
        }
    }
}

/// Cleans a copy of a file for `upload` and counts what came out. None when
/// the file itself goes: nothing came out, or it's a kind it doesn't clean.
fn clean_for_upload(path: &Path, name: &str, xfer: &Transfer) -> Result<Option<CleanCopy>, OpError> {
    let (scrub, copy) = scrub::clean_copy(path).map_err(|e| {
        OpError::failed(format!(
            "Couldn't clean \"{name}\" before sending it: {e}. To send it as it is, turn off Clean files before uploading in Settings → Privacy."
        ))
    })?;
    xfer.cleaned(|c| c.add(&scrub));
    xfer.check()?;
    Ok(copy)
}

/// The scripts `copy_between` runs: what to pack on the first server (the
/// `names` in `parent`, quoted), how big that is, and how to unpack on the
/// second, always into the hidden folder `staging`: the stream is whatever
/// the first server sends, so it never unpacks straight into `dest`.
fn copy_scripts(parent: &str, names: &str, staging: &str) -> (String, String, String) {
    let (p, s) = (shell_quote(parent), shell_quote(staging));
    let pack = format!("cd -- {p} || exit 1; {}; tar -cf - -- \"$@\"", tar_items(names));
    let size = format!("cd -- {p} || exit 1; {}; du -sk -- \"$@\"", tar_items(names));
    // After a dropped connection it starts over, so a hidden folder from before goes first.
    let unpack = format!("rm -rf -- {s}; mkdir -- {s} && cd -- {s} && tar -xf -");
    (pack, size, unpack)
}

/// Once all of it arrived in `staging`, puts each of `names` (and nothing
/// else that came along) into `dest`: moved, still never over anything that
/// turned up meanwhile, or when replacing, unpacked over what's there.
fn copy_finish_script(names: &str, dest: &str, staging: &str, overwrite: bool) -> String {
    let (d, s) = (shell_quote(dest), shell_quote(staging));
    if overwrite {
        return format!(
            r#"for n in {names}; do [ -e {s}/"$n" ] || [ -L {s}/"$n" ] || {{ echo "$n didn't arrive." >&2; exit 1; }}; done; (cd -- {s} && tar -cf - -- {names}) | (cd -- {d} && tar -xf -) || exit 1; rm -rf -- {s}"#
        );
    }
    format!(
        r#"for n in {names}; do if [ -e {d}/"$n" ] || [ -L {d}/"$n" ]; then echo EXISTS >&2; exit 5; fi; done; for n in {names}; do mv -- {s}/"$n" {d}/"$n" || exit 1; done; rmdir -- {s}"#
    )
}

/// Copies files and folders from one folder on a machine into `dest_dir`
/// on a server. Between two servers it goes straight through: tar on one
/// streams into tar on the other by way of this computer, so the servers
/// needn't reach each other, and nothing waits on this computer's disk.
/// From this computer it's an upload. As with an upload, nothing is
/// replaced unless `overwrite` (one check names everything first), and
/// what arrives stays under a hidden name until all of it is there; and
/// from this computer, it's cleaned first with `clean`.
#[allow(clippy::too_many_arguments)]
pub fn copy_between(from: &Target, to: &Target, env: &SshEnv, paths: &[String], dest_dir: &str, overwrite: bool, clean: bool, xfer: &Arc<Transfer>) -> Result<(), OpError> {
    let b = ssh_only(to).ok_or_else(|| OpError::new(ErrorCode::Unsupported, "Copies go to a server; use Download for this computer."))?;
    let Some(a) = ssh_only(from) else {
        let local: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        if !overwrite {
            check_upload(to, env, &local, dest_dir)?;
        }
        xfer.add_total(local.iter().map(|p| upload_size(p)).sum());
        for p in &local {
            upload(to, env, p, dest_dir, overwrite, clean, xfer)?;
        }
        return Ok(());
    };
    let Some(first) = paths.first() else { return Ok(()) };
    let parent = parent_dir(first);
    if paths.iter().any(|p| parent_dir(p) != parent) {
        return Err(OpError::failed("Pick files from one folder to copy together."));
    }
    let names: Vec<String> = paths.iter().map(|p| base_name(p)).collect();
    if !overwrite {
        check_free(b, env, &names, dest_dir)?;
    }
    xfer.check()?;
    let quoted = names.iter().map(|n| shell_quote(n)).collect::<Vec<_>>().join(" ");
    let staging = join_remote(dest_dir, &scratch_name("copy"));
    let (pack, size, unpack) = copy_scripts(&parent, &quoted, &staging);
    let total = run_remote(a, env, &size, None)
        .map(|out| String::from_utf8_lossy(&out).lines().filter_map(|l| l.split_whitespace().next()?.parse::<u64>().ok()).sum::<u64>() * 1024)
        .unwrap_or(0);
    xfer.add_total(total);
    xfer.check()?;
    let base = xfer.done();
    // The tar stream is a little bigger than what du counts; the bar stops at the total.
    let cap = if total == 0 { u64::MAX } else { total };

    // A tar stream can't be picked up midway: after a drop, once both
    // servers answer again, it starts over.
    let result = resuming(&[a, b], env, xfer, || {
        xfer.settle(base);
        let mut src = remote_command(a, env, &pack);
        src.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut src = src.spawn().map_err(spawn_error)?;
        let mut dst = remote_command(b, env, &unpack);
        dst.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut dst = match dst.spawn() {
            Ok(dst) => dst,
            Err(e) => {
                let _ = src.kill();
                let _ = src.wait();
                return Err(spawn_error(e));
            }
        };
        let (src_errors, dst_errors) = (drain(src.stderr.take()), drain(dst.stderr.take()));
        let stream = src.stdout.take().expect("piped stdout");
        let input = dst.stdin.take().expect("piped stdin");
        let copy = spawn_copy(stream, input, xfer.clone(), move |x, n| x.set_done(base + n.min(cap)));
        let copied = wait_copy(xfer, copy, &mut [&mut src, &mut dst])?;
        let (packed, unpacked) = (src.wait()?, dst.wait()?);
        match copied {
            Err(e) if e.kind() != io::ErrorKind::BrokenPipe => return Err(e.into()),
            _ => {}
        }
        if !packed.success() {
            return Err(OpError::from_ssh(packed.code(), &joined(src_errors)));
        }
        if !unpacked.success() {
            return Err(OpError::from_ssh(unpacked.code(), &joined(dst_errors)));
        }
        run_remote(b, env, &copy_finish_script(&quoted, dest_dir, &staging, overwrite), None).map_err(|e| {
            if e.message == "EXISTS" {
                OpError::new(ErrorCode::Exists, exists_message(&names))
            } else {
                e
            }
        })?;
        Ok(())
    });
    match result {
        Ok(()) => {
            xfer.settle(base + total);
            Ok(())
        }
        Err(e) => {
            discard_remote(b, env, &staging);
            Err(e)
        }
    }
}

/// Current directory of a pane's terminal (when it runs in tmux on the server).
pub fn session_cwd(target: &Target, env: &SshEnv, session_key: &str) -> Result<String, OpError> {
    let t = ssh_only(target).ok_or_else(|| OpError::new(ErrorCode::Unsupported, "Not a server session."))?;
    let name = crate::session::tmux_session_for(session_key);
    let out = run_remote_read(t, env, &format!("tmux display-message -p -t {} '#{{pane_current_path}}'", shell_quote(&name)))?;
    let path = String::from_utf8_lossy(&out).trim().to_string();
    if path.is_empty() {
        Err(OpError::new(ErrorCode::NotFound, "Couldn't find the terminal's folder."))
    } else {
        Ok(path)
    }
}

/// Where a relative path a program printed (`src/main.rs`, `docs/notes.md`)
/// is: in `cwd` or a folder above it, in a folder one of your programs runs
/// in (an agent prints paths from its project's folder, whatever the shell's
/// is), in home, or a few levels down from home. The script for `find_path`.
fn find_path_script(rel: &str, cwd: &str) -> String {
    format!(
        r#"set -f; p={rel}; d={cwd}; case "$d" in "~") d=$HOME ;; "~/"*) d="$HOME/${{d#\~/}}" ;; esac
hit() {{ case "$1" in /) f="/$p" ;; *) f="$1/$p" ;; esac; [ -e "$f" ] && {{ printf '%s\n' "$f"; exit 0; }}; }}
while [ -n "$d" ]; do hit "$d"; [ "$d" = / ] && break; d=$(dirname -- "$d"); done
u=$(id -u); ws=$(find /proc -mindepth 2 -maxdepth 2 -name cwd -user "$u" -printf '%l\n' 2>/dev/null | sort -u)
[ -n "$ws" ] || ws=$(for pid in $(pgrep -u "$u" 2>/dev/null); do readlink "/proc/$pid/cwd"; done 2>/dev/null | sort -u)
IFS='
'; for w in $ws; do hit "$w"; done; unset IFS
hit "$HOME"
pe=$(printf '%s' "$p" | sed 's/[][*?\\]/\\&/g')
t=; if timeout 1 true 2>/dev/null; then t="timeout 3"; elif timeout -t 1 true 2>/dev/null; then t="timeout -t 3"; fi
$t find "$HOME" -maxdepth 5 \( -name node_modules -o -name .git -o -name .cache -o -name Library -o -name .npm -o -name .cargo \) -prune -o -path "*/$pe" -print 2>/dev/null | head -n 1"#,
        rel = shell_quote(rel.trim_start_matches("./")),
        cwd = shell_quote(cwd),
    )
}

pub fn find_path(target: &Target, env: &SshEnv, rel: &str, cwd: &str) -> Result<String, OpError> {
    let t = ssh_only(target).ok_or_else(|| OpError::new(ErrorCode::Unsupported, "Not a server."))?;
    let out = run_remote_read(t, env, &find_path_script(rel, cwd))?;
    let path = String::from_utf8_lossy(&out).lines().next().unwrap_or("").trim().to_string();
    if path.is_empty() {
        Err(OpError::new(ErrorCode::NotFound, "Couldn't find it on the server."))
    } else {
        Ok(path)
    }
}

// ---------------------------------------------------------------- path helpers

pub fn base_name(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path).to_string()
}

/// The name a server's file gets on this computer, or None if it can't have
/// one. On Windows `\` and `:` are path syntax, so a Linux file named
/// `x\..\..\AppData\…\Startup\evil.bat` would otherwise land in Startup:
/// there they and the other characters Windows refuses become `_`. Whatever
/// is left must be one plain name inside the download folder.
fn local_name(name: &str, windows: bool) -> Option<String> {
    let mut name = name.to_string();
    if windows {
        name = name.chars().map(|c| if c < ' ' || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
        // Windows drops trailing dots and spaces, which would make ".." of "...".
        name.truncate(name.trim_end_matches(['.', ' ']).len());
        let device = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
        let numbered = device.len() == 4 && (device.starts_with("COM") || device.starts_with("LPT")) && device.as_bytes()[3].is_ascii_digit();
        if numbered || matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
            name.insert(0, '_');
        }
    }
    let mut parts = Path::new(&name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(n)), None) if n == name.as_str() => Some(name),
        _ => None,
    }
}

pub fn parent_dir(path: &str) -> String {
    let p = path.trim_end_matches('/');
    match p.rfind('/') {
        Some(0) => "/".into(),
        Some(i) => p[..i].into(),
        None => ".".into(),
    }
}

pub fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("some free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the `find_path` script with `sh`, home set to `home`.
    #[cfg(unix)]
    fn find_with_sh(home: &Path, rel: &str, cwd: &str) -> String {
        let out = Command::new("sh").arg("-c").arg(find_path_script(rel, cwd)).env("HOME", home).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[cfg(unix)]
    #[test]
    fn find_path_finds_what_an_agent_printed_from_its_project() {
        let home = std::env::temp_dir().join(format!("sshee-find-{}", std::process::id()));
        let file = home.join("proj/out/reports/NOTES.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x").unwrap();
        let want = file.to_string_lossy().to_string();
        // From a folder inside the project: found by going up.
        assert_eq!(find_with_sh(&home, "out/reports/NOTES.txt", &home.join("proj/src").to_string_lossy()), want);
        // From home, as a shell without tmux reports: found by looking under home.
        assert_eq!(find_with_sh(&home, "out/reports/NOTES.txt", "~"), want);
        assert_eq!(find_with_sh(&home, "./out/reports/NOTES.txt", "~/proj"), want);
        // Not there at all: nothing.
        assert_eq!(find_with_sh(&home, "out/reports/MISSING.txt", "~"), "");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn copies_into_a_folder_on_this_computer() {
        let base = std::env::temp_dir().join(format!("sshee-copy-{}", std::process::id()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(src.join("site")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("site/index.html"), "hi").unwrap();
        let env = SshEnv::default();
        let d = dst.display().to_string();
        let xfer = Transfer::quiet();
        upload(&Target::Local, &env, &src.join("site"), &d, false, false, &xfer).unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("site/index.html")).unwrap(), "hi");
        assert_eq!(xfer.done(), 2);
        let again = upload(&Target::Local, &env, &src.join("site"), &d, false, false, &xfer);
        assert_eq!(again.unwrap_err().code, ErrorCode::Exists);
        xfer.cancel();
        let stopped = upload(&Target::Local, &env, &src.join("site"), &d, true, false, &xfer);
        assert_eq!(stopped.unwrap_err().code, ErrorCode::Cancelled);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn archives_files_on_this_computer() {
        let dir = std::env::temp_dir().join(format!("sshee-archive-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("conf")).unwrap();
        std::fs::write(dir.join("conf/app.toml"), "port = 8080").unwrap();
        std::fs::write(dir.join("notes.txt"), "hi").unwrap();
        let d = dir.display().to_string();
        let env = SshEnv::default();
        archive(&Target::Local, &env, &d, &["conf".into(), "notes.txt".into()], "backup.tar.gz").unwrap();
        assert!(dir.join("backup.tar.gz").metadata().unwrap().len() > 0);
        // A second archive with the same name is refused, not overwritten.
        let again = archive(&Target::Local, &env, &d, &["notes.txt".into()], "backup.tar.gz");
        assert_eq!(again.unwrap_err().code, ErrorCode::Exists);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Runs a script with `sh`, `input` on stdin; its exit status.
    #[cfg(unix)]
    fn sh_with_input(script: &str, input: &[u8]) -> std::process::ExitStatus {
        let mut child = Command::new("sh").arg("-c").arg(script).stdin(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait().unwrap()
    }

    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().unwrap().permissions().mode() & 0o777
    }

    /// A folder `real` with a file, a subfolder and a link inside that points
    /// back up, and `link` pointing to `real`.
    #[cfg(unix)]
    fn linked_folder(base: &Path) {
        std::fs::create_dir_all(base.join("real/sub")).unwrap();
        std::fs::write(base.join("real/a.txt"), "hello").unwrap();
        std::fs::write(base.join("real/sub/b.txt"), "x").unwrap();
        std::os::unix::fs::symlink("..", base.join("real/sub/loop")).unwrap();
        std::os::unix::fs::symlink("real", base.join("link")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_downloads_as_the_folder() {
        let base = std::env::temp_dir().join(format!("sshee-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        linked_folder(&base);
        let out = base.join("out");
        std::fs::create_dir_all(&out).unwrap();
        // The server's side packs, this computer's side unpacks, as `download` does.
        let packed = Command::new("sh").arg("-c").arg(pack_script(&base.to_string_lossy(), "link")).output().unwrap();
        assert!(packed.status.success());
        let mut untar = Command::new("tar").args(["-xf", "-", "-C"]).arg(&out).stdin(Stdio::piped()).spawn().unwrap();
        untar.stdin.take().unwrap().write_all(&packed.stdout).unwrap();
        assert!(untar.wait().unwrap().success());
        assert_eq!(std::fs::read_to_string(out.join("link/a.txt")).unwrap(), "hello");
        assert_eq!(std::fs::read_to_string(out.join("link/sub/b.txt")).unwrap(), "x");
        // The link inside stays a link instead of looping.
        assert!(out.join("link/sub/loop").symlink_metadata().unwrap().is_symlink());
        // A plain folder still packs as itself.
        let plain = Command::new("sh").arg("-c").arg(format!("{} | tar -tf -", pack_script(&base.to_string_lossy(), "real"))).output().unwrap();
        assert!(String::from_utf8_lossy(&plain.stdout).lines().any(|l| l == "real/a.txt"));
        // And its size is the folder's, not the link's.
        let du = Command::new("sh").arg("-c").arg(size_script(&base.join("link").to_string_lossy(), true)).output().unwrap();
        let kb: u64 = String::from_utf8_lossy(&du.stdout).split_whitespace().next().unwrap().parse().unwrap();
        assert!(kb > 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_between_servers_lands_whole_and_replaces_nothing() {
        let base = std::env::temp_dir().join(format!("sshee-between-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        linked_folder(&base);
        std::fs::write(base.join("notes.txt"), "n").unwrap();
        let dest = base.join("there");
        std::fs::create_dir_all(&dest).unwrap();
        let (dest_s, staging) = (dest.to_string_lossy().to_string(), dest.join(".copy.part").to_string_lossy().to_string());
        let names = "'notes.txt' 'link'";
        let sh = |script: &str| Command::new("sh").arg("-c").arg(script).output().unwrap();
        // One server packs, the other unpacks under the hidden name, then it moves into place.
        let (pack, size, unpack) = copy_scripts(&base.to_string_lossy(), names, &staging);
        assert!(sh(&format!("({pack}) | ({unpack})")).status.success());
        assert!(!dest.join("notes.txt").exists(), "nothing in place before it's all there");
        assert!(sh(&copy_finish_script(names, &dest_s, &staging, false)).status.success());
        assert_eq!(std::fs::read_to_string(dest.join("notes.txt")).unwrap(), "n");
        assert_eq!(std::fs::read_to_string(dest.join("link/sub/b.txt")).unwrap(), "x");
        assert!(!Path::new(&staging).exists());
        let kb: u64 = String::from_utf8_lossy(&sh(&size).stdout).lines().filter_map(|l| l.split_whitespace().next()?.parse::<u64>().ok()).sum();
        assert!(kb > 0);
        // Something that turned up meanwhile is never replaced.
        let (pack, _, unpack) = copy_scripts(&base.to_string_lossy(), names, &staging);
        assert!(sh(&format!("({pack}) | ({unpack})")).status.success());
        let finish = sh(&copy_finish_script(names, &dest_s, &staging, false));
        assert_eq!(finish.status.code(), Some(5));
        assert_eq!(String::from_utf8_lossy(&finish.stderr).trim(), "EXISTS");
        std::fs::remove_dir_all(&staging).unwrap();
        // Replacing unpacks over what's there, keeping what only the old one had.
        std::fs::write(base.join("notes.txt"), "new").unwrap();
        std::fs::write(dest.join("link/sub/mine.txt"), "kept").unwrap();
        let (pack, _, unpack) = copy_scripts(&base.to_string_lossy(), names, &staging);
        assert!(sh(&format!("({pack}) | ({unpack})")).status.success());
        assert!(sh(&copy_finish_script(names, &dest_s, &staging, true)).status.success());
        assert_eq!(std::fs::read_to_string(dest.join("notes.txt")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(dest.join("link/sub/b.txt")).unwrap(), "x");
        assert_eq!(std::fs::read_to_string(dest.join("link/sub/mine.txt")).unwrap(), "kept");
        assert!(!Path::new(&staging).exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_between_servers_brings_only_what_was_picked() {
        let base = std::env::temp_dir().join(format!("sshee-between-forged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (from, dest) = (base.join("from"), base.join("home"));
        std::fs::create_dir_all(from.join(".ssh")).unwrap();
        std::fs::create_dir_all(dest.join("app")).unwrap();
        std::fs::create_dir_all(from.join("app")).unwrap();
        std::fs::write(from.join("app/main.py"), "new").unwrap();
        std::fs::write(from.join(".ssh/authorized_keys"), "the other server's key").unwrap();
        let (dest_s, staging) = (dest.to_string_lossy().to_string(), dest.join(".copy.part").to_string_lossy().to_string());
        let sh = |script: &str| Command::new("sh").arg("-c").arg(script).output().unwrap();
        // The first server was asked for app, and slipped .ssh into the stream.
        let forged = format!("tar -cf - -C {} -- app .ssh", shell_quote(&from.to_string_lossy()));
        let (_, _, unpack) = copy_scripts("/unused", "'app'", &staging);
        for overwrite in [true, false] {
            if !overwrite {
                std::fs::remove_dir_all(dest.join("app")).unwrap();
            }
            assert!(sh(&format!("({forged}) | ({unpack})")).status.success());
            // Moving leaves the extra behind in the hidden folder, so that copy ends in an error.
            let finish = sh(&copy_finish_script("'app'", &dest_s, &staging, overwrite));
            assert_eq!(finish.status.success(), overwrite, "{}", String::from_utf8_lossy(&finish.stderr));
            assert_eq!(std::fs::read_to_string(dest.join("app/main.py")).unwrap(), "new");
            assert!(!dest.join(".ssh").exists(), "only app arrives (overwrite: {overwrite})");
            let _ = std::fs::remove_dir_all(&staging);
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn archives_the_folder_a_link_points_to() {
        let base = std::env::temp_dir().join(format!("sshee-archive-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        linked_folder(&base);
        let env = SshEnv::default();
        archive(&Target::Local, &env, &base.to_string_lossy(), &["link".into()], "link.tar.gz").unwrap();
        let list = Command::new("tar").arg("-tzf").arg(base.join("link.tar.gz")).output().unwrap();
        let names = String::from_utf8_lossy(&list.stdout).to_string();
        assert!(names.lines().any(|l| l.ends_with("a.txt")), "{names}");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn saving_on_a_server_keeps_the_file_whole_and_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sshee-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("secret.env");
        std::fs::write(&f, "OLD=1\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        let p = f.to_string_lossy();
        assert!(sh_with_input(&save_script(&p, 6), b"NEW=2\n").success());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "NEW=2\n");
        assert_eq!(mode_of(&f), 0o600);
        // Cut off on the way: the file stays as it was.
        assert!(!sh_with_input(&save_script(&p, 100), b"NEW=3\n").success());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "NEW=2\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacing_an_upload_keeps_the_old_files_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sshee-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (d, s) = (dir.join("key.pem"), dir.join(".key.pem.sshee-part"));
        std::fs::write(&d, "old").unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o600)).unwrap();
        let (dq, sq) = (shell_quote(&d.to_string_lossy()), shell_quote(&s.to_string_lossy()));
        assert!(sh_with_input(&upload_file_script("", &sq, &dq, 3, 0), b"new").success());
        assert_eq!(std::fs::read_to_string(&d).unwrap(), "new");
        assert_eq!(mode_of(&d), 0o600);
        assert!(!s.exists());
        // Cut off on the way: the old file stays, and what arrived waits to be picked up.
        assert!(!sh_with_input(&upload_file_script("", &sq, &dq, 10, 0), b"cut").success());
        assert_eq!(std::fs::read_to_string(&d).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(&s).unwrap(), "cut");
        // The rest comes after the connection's back, and then it's whole.
        assert!(sh_with_input(&upload_file_script("", &sq, &dq, 10, 3), b" it out").success());
        assert_eq!(std::fs::read_to_string(&d).unwrap(), "cut it out");
        assert!(!s.exists());
        // If what's staged isn't what was there before, it says so instead of adding on.
        std::fs::write(&s, "xy").unwrap();
        let out = Command::new("sh").arg("-c").arg(upload_file_script("", &sq, &dq, 10, 3)).stdin(Stdio::null()).output().unwrap();
        assert_eq!(out.status.code(), Some(6));
        assert_eq!(std::fs::read_to_string(&d).unwrap(), "cut it out");
        std::fs::remove_file(&s).unwrap();
        // A new file just moves into place.
        let (n, nq) = (dir.join("fresh.txt"), shell_quote(&dir.join("fresh.txt").to_string_lossy()));
        assert!(sh_with_input(&upload_file_script("", &sq, &nq, 2, 0), b"hi").success());
        assert_eq!(std::fs::read_to_string(&n).unwrap(), "hi");
        // A link planted under the staged name gets nothing written through it.
        let other = dir.join("someone-elses.txt");
        std::fs::write(&other, "theirs").unwrap();
        std::os::unix::fs::symlink(&other, &s).unwrap();
        let out = Command::new("sh").arg("-c").arg(upload_file_script("", &sq, &nq, 10, 3)).stdin(Stdio::null()).output().unwrap();
        assert_eq!(out.status.code(), Some(6), "a link isn't picked up");
        assert!(sh_with_input(&upload_file_script("", &sq, &nq, 6, 0), b"secret").success());
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "theirs");
        assert_eq!(std::fs::read_to_string(&n).unwrap(), "secret");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_text_as_it_can_be_saved() {
        let t = decode_text("a", 9, b"one\r\ntwo\r\n".to_vec());
        assert!(t.crlf && !t.lossy);
        assert_eq!(t.content.as_deref(), Some("one\ntwo\n"));
        let t = decode_text("a", 8, b"one\ntwo\n".to_vec());
        assert!(!t.crlf);
        assert_eq!(t.content.as_deref(), Some("one\ntwo\n"));
        // Latin-1 isn't UTF-8: shown, but not to be saved back.
        let t = decode_text("a", 4, b"caf\xe9".to_vec());
        assert!(t.lossy);
        assert!(t.content.is_some());
        let t = decode_text("a", 3, b"a\0b".to_vec());
        assert!(t.binary && t.content.is_none());
        let t = decode_text("a", MAX_TEXT + 1, Vec::new());
        assert!(t.content.is_none() && !t.binary);
    }

    #[cfg(unix)]
    #[test]
    fn previews_only_regular_files_here() {
        let env = SshEnv::default();
        assert!(read_text(&Target::Local, &env, "/dev/zero").is_err());
    }

    #[test]
    fn names_what_an_upload_would_replace() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(exists_message(&names(&["a"])), "\"a\" already exists in this folder.");
        assert_eq!(exists_message(&names(&["a", "b", "c"])), "\"a\", \"b\" and \"c\" already exist in this folder.");
        assert_eq!(
            exists_message(&names(&["a", "b", "c", "d", "e", "f", "g"])),
            "\"a\", \"b\", \"c\", \"d\" and 3 more already exist in this folder."
        );
        let base = std::env::temp_dir().join(format!("sshee-check-{}", std::process::id()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        for n in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(src.join(n), n).unwrap();
        }
        std::fs::write(dst.join("a.txt"), "old").unwrap();
        std::fs::write(dst.join("c.txt"), "old").unwrap();
        let paths: Vec<PathBuf> = ["a.txt", "b.txt", "c.txt"].iter().map(|n| src.join(n)).collect();
        let err = check_upload(&Target::Local, &SshEnv::default(), &paths, &dst.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, ErrorCode::Exists);
        assert_eq!(err.message, "\"a.txt\" and \"c.txt\" already exist in this folder.");
        // The same on a server, with a name a shell could trip on.
        #[cfg(unix)]
        {
            std::fs::write(dst.join("it's -x"), "").unwrap();
            let batch = names(&["a.txt", "b.txt", "it's -x", "c.txt"]);
            let out = Command::new("sh").arg("-c").arg(taken_script(&dst.to_string_lossy(), &batch)).output().unwrap();
            assert_eq!(taken_in(&out.stdout, &batch), names(&["a.txt", "it's -x", "c.txt"]));
            let out = Command::new("sh").arg("-c").arg(taken_script("/no/such/folder", &batch)).output().unwrap();
            assert!(out.status.success() && taken_in(&out.stdout, &batch).is_empty());
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn copies_here_without_looping() {
        let base = std::env::temp_dir().join(format!("sshee-loop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        linked_folder(&base);
        let env = SshEnv::default();
        let xfer = Transfer::quiet();
        // Into itself, or a folder inside it: refused.
        let into = upload(&Target::Local, &env, &base.join("real"), &base.join("real/sub").to_string_lossy(), false, false, &xfer);
        assert!(into.is_err());
        assert!(!base.join("real/sub/real").exists());
        // A link inside is copied as a link; the picked link is followed.
        let dst = base.join("dst");
        std::fs::create_dir_all(&dst).unwrap();
        upload(&Target::Local, &env, &base.join("link"), &dst.to_string_lossy(), false, false, &xfer).unwrap();
        assert!(dst.join("link").symlink_metadata().unwrap().is_dir());
        assert!(dst.join("link/sub/loop").symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(dst.join("link/a.txt")).unwrap(), "hello");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_file_counts_its_target() {
        let base = std::env::temp_dir().join(format!("sshee-linked-file-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("real.txt"), "12345").unwrap();
        std::os::unix::fs::symlink("real.txt", base.join("link.txt")).unwrap();
        assert_eq!(upload_size(&base.join("link.txt")), 5);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn parses_find_output() {
        let out = b"/home/me\nd\td\t4096\t1700000000.5\tdrwxr-xr-x\tsrc\0f\tf\t12\t1700000001.0\t-rw-r--r--\tnotes\twith tab.txt\0l\tN\t7\t1700000002.0\tlrwxrwxrwx\tbroken\0";
        let (pwd, e) = parse_find(out);
        assert_eq!(pwd, "/home/me");
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].kind, FileKind::Dir);
        assert_eq!(e[1].name, "notes\twith tab.txt");
        assert_eq!(e[1].modified, 1700000001);
        assert!(e[2].is_link);
        assert_eq!(e[2].kind, FileKind::Link);
    }

    #[test]
    fn a_download_keeps_to_the_download_folder() {
        // A Linux name that is a path on Windows stays one name there.
        let escape = r"x\..\..\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\evil.bat";
        assert_eq!(local_name(escape, true).unwrap(), "x_.._.._AppData_Roaming_Microsoft_Windows_Start Menu_Programs_Startup_evil.bat");
        assert_eq!(local_name(r"C:\Windows\evil.exe", true).unwrap(), "C__Windows_evil.exe");
        assert_eq!(local_name("report 10:30.txt", true).unwrap(), "report 10_30.txt");
        assert_eq!(local_name("notes. . ", true).unwrap(), "notes");
        assert_eq!(local_name("con.txt", true).unwrap(), "_con.txt");
        assert_eq!(local_name("COM1", true).unwrap(), "_COM1");
        assert_eq!(local_name("COMPANY.txt", true).unwrap(), "COMPANY.txt");
        for gone in ["", ".", "..", "...", " . "] {
            assert_eq!(local_name(gone, true), None, "{gone:?}");
        }
        // Elsewhere only `/` separates, and base_name already took the last part.
        assert_eq!(local_name(r"a\b:c", false).unwrap(), r"a\b:c");
        assert_eq!(local_name("..", false), None);
        assert_eq!(local_name(".", false), None);
        assert_eq!(local_name("", false), None);
    }

    #[test]
    fn path_helpers() {
        assert_eq!(base_name("/var/log/"), "log");
        assert_eq!(parent_dir("/var/log"), "/var");
        assert_eq!(parent_dir("/var"), "/");
        assert_eq!(join_remote("/", "etc"), "/etc");
        assert_eq!(join_remote("/home/me", "a"), "/home/me/a");
    }

    #[test]
    fn refuses_dangerous_deletes() {
        let env = SshEnv::default();
        for p in ["/", "", "~", "relative"] {
            assert!(delete(&Target::Local, &env, p).is_err());
        }
    }
}
