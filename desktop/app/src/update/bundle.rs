//! macOS: a release replaces the whole `SSHeesh.app`, never the program inside it. The bundle's
//! signature covers everything in it, and the Keychain and notifications trust the app by that
//! signature, so a new program in an old bundle would break both.
//!
//! Every step is a folder renamed next to the app, on the same disk, so each one is a single
//! `rename` (`Bundle`):
//!
//! 1. **Where** ([`route`]): the app must sit in a folder this user can write, on a disk that
//!    isn't read-only, and not be translocated (Gatekeeper's App Translocation runs an app
//!    opened from Downloads from a random read-only path until it's moved). Otherwise the line
//!    only offers the download, saying why.
//! 2. **Stage** ([`stage`], in the background, after the archive's size, SHA-256 and signature
//!    were checked): the archive is unpacked into `.SSHeesh-update.part/`, only folders and
//!    plain files, all under one `*.app` at the top. The bundle must name our bundle id, the
//!    program every 1.x keeps (`ssheesh`) and the manifest's version, and, when this app is
//!    signed, pass `codesign --verify --deep --strict` against this app's own designated
//!    requirement (the Keychain's trust follows it). It then waits as `.SSHeesh-update`, a
//!    hidden folder without `.app`, so Launch Services never takes it for a second SSHeesh.
//! 3. **Install** ([`install`], on "Restart to update"): the staged bundle is hashed again and
//!    must be the one checked, and passes the same check again (bundle id, program, version,
//!    signature: the digest alone sits in a file this user can write); this app steps aside
//!    as `.SSHeesh-previous` (kept, unlike 0.10's updater, which deleted it right after the
//!    swap), and the new one takes its name.
//! 4. **Trial**: the helper (`update::watch_main`) starts the new app with `open -n` and gives
//!    it a minute to say it started; if it doesn't, [`roll_back`] puts the previous one back.
//! 5. **Clean** ([`clean`], [`keep_previous`]): leftovers go at the next start; the previous
//!    version goes 30 days after the update, as Windows keeps 0.10.
//!
//! Plain files and folders only, so every step runs on Linux in the tests; the few macOS calls
//! (`codesign`, the quarantine mark, `statvfs`) are behind [`Disk`] and [`Codesign`].

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::manifest::MOST_BYTES;

/// The bundle id every SSHeesh has had (0.10's too): the Keychain and notifications know it.
pub const BUNDLE_ID: &str = "io.github.footprintsntrails.ssheesh";

/// How long the version from before stays next to the app.
pub const KEEP_PREVIOUS: Duration = Duration::from_secs(30 * 24 * 3600);

/// Since when the version from before has been kept, in the update's own folder.
const PREVIOUS_SINCE: &str = "previous-app-since";

/// The app and the folders an update keeps beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The running app: `/Applications/SSHeesh.app`.
    pub app: PathBuf,
    /// The program's name in `Contents/MacOS`.
    pub program: String,
    /// The checked new bundle, waiting for a restart: `.SSHeesh-update`.
    pub staged: PathBuf,
    /// Where the archive is unpacked and checked: `.SSHeesh-update.part`.
    pub part: PathBuf,
    /// The version from before, kept for going back: `.SSHeesh-previous`.
    pub previous: PathBuf,
    /// A new version that didn't start, set aside while the previous one goes back.
    pub failed: PathBuf,
}

impl Bundle {
    /// The app `exe` runs from (`….app/Contents/MacOS/<program>`), if it's in one.
    pub fn containing(exe: &Path) -> Option<Bundle> {
        let program = exe.file_name()?.to_string_lossy().into_owned();
        let macos = exe.parent()?;
        let contents = macos.parent()?;
        let app = contents.parent()?;
        let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
        if !(is(macos, "MacOS") && is(contents, "Contents") && app.extension().is_some_and(|e| e == "app")) {
            return None;
        }
        let stem = app.file_stem()?.to_string_lossy().into_owned();
        let beside = |tag: &str| app.with_file_name(format!(".{stem}-{tag}"));
        Some(Bundle { app: app.to_path_buf(), program, staged: beside("update"), part: beside("update.part"), previous: beside("previous"), failed: beside("failed") })
    }

    /// The program inside the app.
    pub fn exe(&self) -> PathBuf {
        self.app.join("Contents/MacOS").join(&self.program)
    }

    fn folder(&self) -> &Path {
        self.app.parent().unwrap_or(Path::new("."))
    }
}

impl super::swap::Replaced for Bundle {
    fn program(&self) -> PathBuf {
        self.app.clone()
    }

    fn roll_back(&self) -> Result<(), String> {
        roll_back(self)
    }
}

/// What the disk under the app allows.
pub trait Disk {
    /// The disk `dir` is on is mounted read-only (a disk image, a translocated app).
    fn read_only(&self, dir: &Path) -> bool;
    /// This user can rename `app` within its folder and put a new folder next to it.
    fn can_replace(&self, app: &Path) -> bool;
}

/// How an app in a bundle gets a newer version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// In place, by renames next to it.
    Swap,
    /// Only by hand: it runs from a disk image or from where Gatekeeper translocated it, and
    /// should be moved to Applications first.
    MoveToApplications,
    /// Only by hand: its folder can't be written by this user (Applications set up by another
    /// administrator, a folder of another user's).
    NotWritable,
}

/// Whether `app` was translocated: Gatekeeper runs it from a random read-only path,
/// `…/AppTranslocation/<id>/d/SSHeesh.app`, until it's moved out of where it was downloaded.
pub fn translocated(app: &Path) -> bool {
    app.components().any(|c| c.as_os_str() == "AppTranslocation")
}

pub fn route(bundle: &Bundle, disk: &impl Disk) -> Route {
    if translocated(&bundle.app) || disk.read_only(bundle.folder()) {
        Route::MoveToApplications
    } else if !disk.can_replace(&bundle.app) {
        Route::NotWritable
    } else {
        Route::Swap
    }
}

/// What the new bundle must be.
#[derive(Debug, Clone, Copy)]
pub struct Expect<'a> {
    pub bundle_id: &'a str,
    pub program: &'a str,
    /// The manifest's version, as `CFBundleShortVersionString` carries it.
    pub version: &'a str,
}

/// `codesign`, as the checks ask it.
pub trait Codesign {
    /// The designated requirement `app` is signed with; `None` when it's unsigned or ad-hoc
    /// signed (a build made on someone's computer), and nothing is required of the new one.
    fn requirement(&self, app: &Path) -> Option<String>;
    /// `codesign --verify --deep --strict`, the bundle tested against `requirement`.
    fn verify(&self, app: &Path, requirement: &str) -> Result<(), String>;
}

/// Why a bundle wasn't staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unstaged {
    /// It isn't the bundle it must be: not ours, another version, not signed as this one.
    Refused(String),
    /// Writing it out failed.
    Failed(String),
}

impl std::fmt::Display for Unstaged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unstaged::Refused(why) | Unstaged::Failed(why) => f.write_str(why),
        }
    }
}

fn remove_tree(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
    }
}

/// The folder's own record of a rename, on the disk.
fn sync_folder(dir: &Path) {
    #[cfg(unix)]
    let _ = fs::File::open(dir).and_then(|d| d.sync_all());
    #[cfg(not(unix))]
    let _ = dir;
}

/// Unpacks the checked `archive` next to the app, checks the bundle in it against `expect`
/// and this app's signature, and leaves it waiting as `bundle.staged`. Returns its
/// [`digest`], which [`install`] checks again. On failure nothing is left behind.
pub fn stage(bundle: &Bundle, archive: &[u8], expect: &Expect<'_>, codesign: &impl Codesign) -> Result<String, Unstaged> {
    let failed = |e: std::io::Error| Unstaged::Failed(e.to_string());
    remove_tree(&bundle.part).map_err(failed)?;
    remove_tree(&bundle.staged).map_err(failed)?;
    let done = stage_in(bundle, archive, expect, codesign);
    let _ = remove_tree(&bundle.part);
    if done.is_err() {
        let _ = remove_tree(&bundle.staged);
    }
    done
}

fn stage_in(bundle: &Bundle, archive: &[u8], expect: &Expect<'_>, codesign: &impl Codesign) -> Result<String, Unstaged> {
    let failed = |e: std::io::Error| Unstaged::Failed(e.to_string());
    fs::create_dir(&bundle.part).map_err(failed)?;
    let top = unpack(archive, &bundle.part)?;
    let new_app = bundle.part.join(&top);
    set_mode(&new_app, 0o755).map_err(failed)?;
    check(&new_app, expect, &bundle.app, codesign)?;
    clear_quarantine(&new_app);
    fs::rename(&new_app, &bundle.staged).map_err(failed)?;
    sync_folder(bundle.folder());
    digest(&bundle.staged).map_err(failed)
}

/// How much an archive may unpack to.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// The files' bytes, all together, as each entry says (a PAX `size` included).
    bytes: u64,
    /// Entries, folders included: the app has a few hundred.
    entries: u64,
    /// The whole unpacked tar stream, headers and padding included: a hard bound on what is
    /// written whatever the headers claim.
    stream: u64,
}

const LIMITS: Limits = Limits { bytes: MOST_BYTES, entries: 50_000, stream: MOST_BYTES + 64 * 1024 * 1024 };

const TOO_BIG: &str = "unpacks too big";

/// A reader that fails, rather than ends, once more than `left` bytes come through it: a
/// gzip that inflates past it is refused, never written out.
struct Capped<R> {
    inner: R,
    left: u64,
    /// Set once it failed for the cap (tar's own errors don't carry the reason through).
    tripped: std::rc::Rc<std::cell::Cell<bool>>,
}

impl<R: std::io::Read> std::io::Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            // Exactly at the cap is fine; one byte more isn't.
            let mut one = [0u8; 1];
            return match self.inner.read(&mut one)? {
                0 => Ok(0),
                _ => {
                    self.tripped.set(true);
                    Err(std::io::Error::other(TOO_BIG))
                }
            };
        }
        let want = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..want])?;
        self.left -= n as u64;
        Ok(n)
    }
}

/// Unpacks `archive` into the empty folder `into`: folders and plain files only, all under one
/// folder at the top named `*.app`, nothing outside `into`, at most [`MOST_BYTES`] in all, no
/// set-id or world-writable modes. Returns the top folder's name.
fn unpack(archive: &[u8], into: &Path) -> Result<String, Unstaged> {
    unpack_within(archive, into, LIMITS)
}

fn unpack_within(archive: &[u8], into: &Path, limits: Limits) -> Result<String, Unstaged> {
    let bad = |why: String| Unstaged::Refused(format!("the archive {why}"));
    let tripped = std::rc::Rc::new(std::cell::Cell::new(false));
    let unpacks = |e: std::io::Error| {
        if tripped.get() {
            Unstaged::Refused(format!("the archive {TOO_BIG}"))
        } else {
            Unstaged::Refused(format!("the archive doesn't unpack: {e}"))
        }
    };
    let mut tar = tar::Archive::new(Capped { inner: flate2::read::GzDecoder::new(archive), left: limits.stream, tripped: tripped.clone() });
    tar.set_preserve_permissions(false);
    tar.set_unpack_xattrs(false);
    let mut top: Option<String> = None;
    let mut total: u64 = 0;
    let mut count: u64 = 0;
    for entry in tar.entries().map_err(unpacks)? {
        let mut entry = entry.map_err(unpacks)?;
        count += 1;
        if count > limits.entries {
            return Err(bad("has too many files".into()));
        }
        let path = entry.path().map_err(unpacks)?.into_owned();
        let mut parts = Vec::new();
        for c in path.components() {
            match c {
                Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
                Component::CurDir => {}
                _ => return Err(bad(format!("has a path that leaves its folder: {}", path.display()))),
            }
        }
        let Some(first) = parts.first().cloned() else { continue };
        if !first.ends_with(".app") || first.starts_with('.') {
            return Err(bad(format!("has something besides an app at the top: {first}")));
        }
        match &top {
            Some(t) if *t != first => return Err(bad("has more than one app at the top".into())),
            Some(_) => {}
            None => top = Some(first),
        }
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(bad(format!("has a link or a special file: {}", path.display())));
        }
        // What `unpack_in` writes: the size a PAX header gives, not the plain header's field.
        total = total.saturating_add(entry.size());
        if total > limits.bytes {
            return Err(bad(TOO_BIG.into()));
        }
        if !entry.unpack_in(into).map_err(unpacks)? {
            return Err(bad(format!("has a path that leaves its folder: {}", path.display())));
        }
    }
    let top = top.ok_or_else(|| bad("is empty".into()))?;
    tighten(&into.join(&top)).map_err(|e| Unstaged::Failed(e.to_string()))?;
    Ok(top)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> std::io::Result<()> {
    Ok(())
}

/// No set-id bits and nothing writable by others, anywhere in the bundle.
fn tighten(root: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for (path, meta) in walk(root)? {
            let mode = meta.permissions().mode() & 0o7777;
            if mode & 0o7022 != 0 {
                set_mode(&path, mode & 0o755)?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
}

/// Every folder and file under `root` (itself first), in name order, without following links.
fn walk(root: &Path) -> std::io::Result<Vec<(PathBuf, fs::Metadata)>> {
    let mut out = vec![(root.to_path_buf(), fs::symlink_metadata(root)?)];
    let mut i = 0;
    while i < out.len() {
        if out[i].1.is_dir() {
            let mut children: Vec<PathBuf> = fs::read_dir(&out[i].0)?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
            children.sort();
            for child in children {
                let meta = fs::symlink_metadata(&child)?;
                out.push((child, meta));
            }
        }
        i += 1;
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// One SHA-256 over a whole bundle: every path in it, what kind it is, whether it's
/// executable, and every file's bytes (a link's target). The same bundle, moved or renamed,
/// gives the same digest; a byte or a mode changed in it gives another.
pub fn digest(root: &Path) -> std::io::Result<String> {
    let mut all = Sha256::new();
    for (path, meta) in walk(root)? {
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
        all.update(rel.as_bytes());
        all.update([0]);
        #[cfg(unix)]
        let exec = {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let exec = false;
        if meta.file_type().is_symlink() {
            all.update(b"l");
            all.update(fs::read_link(&path)?.to_string_lossy().as_bytes());
        } else if meta.is_dir() {
            all.update(b"d");
        } else {
            all.update(if exec { b"x" } else { b"f" });
            let mut file = Sha256::new();
            std::io::copy(&mut fs::File::open(&path)?, &mut file)?;
            all.update(file.finalize());
        }
        all.update([0]);
    }
    Ok(all.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// A string value of an XML property list, as the release workflow writes `Info.plist`.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let at = plist.find(&format!("<key>{key}</key>"))? + key.len() + "<key></key>".len();
    let rest = plist[at..].trim_start().strip_prefix("<string>")?;
    let value = &rest[..rest.find("</string>")?];
    Some(value.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&"))
}

/// Whether `new_app` is the bundle `expect` names, and signed as `current` is.
pub fn check(new_app: &Path, expect: &Expect<'_>, current: &Path, codesign: &impl Codesign) -> Result<(), Unstaged> {
    let refused = |why: String| Err(Unstaged::Refused(why));
    let plist = match fs::read(new_app.join("Contents/Info.plist")) {
        Ok(b) if b.starts_with(b"bplist") => return refused("its Info.plist isn't the text the release writes".into()),
        Ok(b) => String::from_utf8(b).unwrap_or_default(),
        Err(_) => return refused("it has no Info.plist".into()),
    };
    let field = |key: &str| plist_string(&plist, key).unwrap_or_default();
    let id = field("CFBundleIdentifier");
    if id != expect.bundle_id {
        return refused(format!("it's another app ({id})"));
    }
    let program = field("CFBundleExecutable");
    if program != expect.program {
        return refused(format!("its program is called {program:?}, not {:?}", expect.program));
    }
    let version = field("CFBundleShortVersionString");
    if version != expect.version {
        return refused(format!("it's version {version:?}, not {}", expect.version));
    }
    let exe = new_app.join("Contents/MacOS").join(expect.program);
    let runnable = fs::symlink_metadata(&exe).is_ok_and(|m| {
        #[cfg(unix)]
        let x = {
            use std::os::unix::fs::PermissionsExt;
            m.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let x = true;
        m.is_file() && m.len() > 0 && x
    });
    if !runnable {
        return refused("its program is missing or can't run".into());
    }
    if let Some(requirement) = codesign.requirement(current) {
        codesign.verify(new_app, &requirement).map_err(|e| Unstaged::Refused(format!("it isn't signed as this app is: {e}")))?;
    }
    Ok(())
}

/// Puts the staged bundle in place of the app, keeping the app as `bundle.previous`. The
/// staged bundle must still have `digest`, recorded when it was staged, and pass the whole
/// [`check`] again, right before it moves: the digest sits in a file this user can write, so
/// only the check says the bundle is ours, this version and signed as this app is. On failure
/// the app is where it was.
pub fn install(bundle: &Bundle, digest: &str, expect: &Expect<'_>, codesign: &impl Codesign) -> Result<(), String> {
    if !bundle.staged.is_dir() {
        return Err("the new version isn't next to this one any more".into());
    }
    remove_tree(&bundle.previous).map_err(|e| format!("the version from before the last update can't be cleared away: {e}"))?;
    let _ = remove_tree(&bundle.failed);
    match self::digest(&bundle.staged) {
        Ok(d) if d == digest => {}
        Ok(_) => return Err("the new version changed after it was checked".into()),
        Err(e) => return Err(format!("the new version can't be read: {e}")),
    }
    check(&bundle.staged, expect, &bundle.app, codesign).map_err(|e| format!("the new version isn't the one to install: {e}"))?;
    fs::rename(&bundle.app, &bundle.previous).map_err(|e| format!("this version can't step aside: {e}"))?;
    if let Err(e) = fs::rename(&bundle.staged, &bundle.app) {
        let _ = fs::rename(&bundle.previous, &bundle.app);
        return Err(format!("the new version can't take its place: {e}"));
    }
    clear_quarantine(&bundle.app);
    // A new date on the app, so Finder and Launch Services read it again.
    let _ = fs::File::open(&bundle.app).and_then(|f| f.set_modified(SystemTime::now()));
    sync_folder(bundle.folder());
    Ok(())
}

/// Puts the version from before back in place of a new one that didn't start.
pub fn roll_back(bundle: &Bundle) -> Result<(), String> {
    if !bundle.previous.is_dir() {
        return Err("the version from before is gone".into());
    }
    let aside = fs::symlink_metadata(&bundle.app).is_ok();
    if aside {
        remove_tree(&bundle.failed).map_err(|e| e.to_string())?;
        fs::rename(&bundle.app, &bundle.failed).map_err(|e| format!("the new version can't step aside: {e}"))?;
    }
    if let Err(e) = fs::rename(&bundle.previous, &bundle.app) {
        if aside {
            let _ = fs::rename(&bundle.failed, &bundle.app);
        }
        return Err(format!("the version from before can't go back: {e}"));
    }
    sync_folder(bundle.folder());
    let _ = remove_tree(&bundle.failed);
    Ok(())
}

/// Clears what an update leaves next to the app: an unpacking cut off, a version that didn't
/// start, and the staged one unless `keep_new`. The version from before is [`keep_previous`]'s.
pub fn clean(bundle: &Bundle, keep_new: bool) {
    let _ = remove_tree(&bundle.part);
    let _ = remove_tree(&bundle.failed);
    if !keep_new {
        let _ = remove_tree(&bundle.staged);
    }
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// Notes that the version from before was set aside `now` (by "Restart to update").
pub fn note_previous(folder: &Path, now: SystemTime) {
    let _ = fs::create_dir_all(folder);
    let _ = fs::write(folder.join(PREVIOUS_SINCE), secs(now).to_string());
}

/// Keeps the version from before for [`KEEP_PREVIOUS`] after it was set aside, then removes it.
/// One found without a date (the note lost) starts its 30 days now.
pub fn keep_previous(folder: &Path, bundle: &Bundle, now: SystemTime) {
    let record = folder.join(PREVIOUS_SINCE);
    if !bundle.previous.exists() {
        let _ = fs::remove_file(&record);
        return;
    }
    let since = fs::read_to_string(&record).ok().and_then(|t| t.trim().parse::<u64>().ok());
    match since {
        None => note_previous(folder, now),
        Some(since) if now.duration_since(UNIX_EPOCH + Duration::from_secs(since)).is_ok_and(|d| d >= KEEP_PREVIOUS) => {
            if remove_tree(&bundle.previous).is_ok() {
                let _ = fs::remove_file(&record);
            }
        }
        Some(_) => {}
    }
}

/// Takes macOS's quarantine mark off everything in the bundle: a marked app would be checked
/// by Gatekeeper again at its first start, and this one isn't notarized. Files this program
/// writes get none, but a folder that marks everything would stop the new version.
fn clear_quarantine(root: &Path) {
    #[cfg(target_os = "macos")]
    if let Ok(all) = walk(root) {
        use std::os::unix::ffi::OsStrExt;
        for (path, _) in all {
            if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
                // SAFETY: two NUL-terminated strings that live across the call.
                unsafe { libc::removexattr(c.as_ptr(), c"com.apple.quarantine".as_ptr(), libc::XATTR_NOFOLLOW) };
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = root;
}

/// A new app started for its trial, or the previous one started again.
pub struct Opened {
    /// `open -W`, which lasts as long as the app does.
    open: std::process::Child,
    /// The program inside the app, to stop the app itself when it hangs.
    exe: PathBuf,
}

impl super::swap::Started for Opened {
    fn exited(&mut self) -> bool {
        super::swap::Started::exited(&mut self.open)
    }

    fn kill(&mut self) {
        super::swap::Started::kill(&mut self.open);
        stop_running(&self.exe);
    }
}

/// Starts the app at `app` as the Finder would, through Launch Services: `open -n -W --env
/// <var>=<token> <app> --args <args>`. A new instance (`-n`), with `var` in its environment
/// (`open` otherwise hands the app launchd's), and `open` waiting until it quits (`-W`), so
/// the trial sees it close. Its own process, not the helper's child, so macOS asks about
/// notifications and the Keychain for the app itself. Elsewhere (only the tests reach this)
/// the program `exe` is started directly.
pub fn open(app: &Path, exe: &Path, var: &str, token: &str, args: &[std::ffi::OsString]) -> std::io::Result<Opened> {
    use std::process::{Command, Stdio};
    let mut cmd = if cfg!(target_os = "macos") {
        let mut cmd = Command::new("/usr/bin/open");
        cmd.arg("-n").arg("-W").arg("--env").arg(format!("{var}={token}")).arg(app);
        if !args.is_empty() {
            cmd.arg("--args").args(args);
        }
        cmd
    } else {
        let mut cmd = Command::new(exe);
        cmd.args(args).env(var, token);
        cmd
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    Ok(Opened { open: cmd.spawn()?, exe: exe.to_path_buf() })
}

/// Stops every process running the program at `exe` (a new app that hung in its trial), but
/// never this one.
fn stop_running(exe: &Path) {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let want = fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        let mut pids: Vec<libc::pid_t> = vec![0; 8192];
        let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        // SAFETY: the buffer holds `bytes` bytes; the call writes at most that many.
        let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let me = std::process::id() as libc::pid_t;
        for &pid in pids.iter().take(n.max(0) as usize) {
            if pid <= 0 || pid == me {
                continue;
            }
            let mut buf = [0u8; 4096];
            // SAFETY: `buf` holds 4096 bytes, PROC_PIDPATHINFO_MAXSIZE.
            let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
            if len > 0 && Path::new(std::ffi::OsStr::from_bytes(&buf[..len as usize])) == want {
                // SAFETY: a plain signal to a process found running `exe`.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = exe;
}

/// The real disk and the real `codesign`.
pub struct System;

impl Disk for System {
    fn read_only(&self, dir: &Path) -> bool {
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return false };
            // SAFETY: a NUL-terminated path and a zeroed struct statvfs fills.
            let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
                return st.f_flag & libc::ST_RDONLY != 0;
            }
            false
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = dir;
            false
        }
    }

    fn can_replace(&self, app: &Path) -> bool {
        let Some(folder) = app.parent() else { return false };
        if !fs::symlink_metadata(app).is_ok_and(|m| m.is_dir()) {
            return false;
        }
        let probe = folder.join(format!(".ssheesh-probe-{}", std::process::id()));
        let _ = fs::remove_dir(&probe);
        if fs::create_dir(&probe).is_err() {
            return false;
        }
        let mine = fs::metadata(&probe);
        let _ = fs::remove_dir(&probe);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // A folder with the sticky bit lets only an item's owner rename it.
            let sticky = fs::metadata(folder).is_ok_and(|m| m.mode() & 0o1000 != 0);
            if sticky && !matches!((mine, fs::metadata(app)), (Ok(me), Ok(a)) if me.uid() == a.uid()) {
                return false;
            }
        }
        #[cfg(not(unix))]
        let _ = mine;
        true
    }
}

impl Codesign for System {
    fn requirement(&self, app: &Path) -> Option<String> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let out = std::process::Command::new("/usr/bin/codesign").arg("-d").arg("-v").arg("-r-").arg(app).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let (info, req) = (String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
        if info.contains("Signature=adhoc") {
            return None;
        }
        // "designated => identifier "…" and certificate leaf = H"…"", on either stream.
        let line = req.lines().chain(info.lines()).find_map(|l| l.trim().strip_prefix("designated =>"))?;
        let requirement = line.trim();
        (!requirement.is_empty() && !requirement.starts_with("cdhash")).then(|| requirement.to_owned())
    }

    fn verify(&self, app: &Path, requirement: &str) -> Result<(), String> {
        // "-R=<text>": a requirement given as its text, which the leading "=" says.
        let out = std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(format!("-R={requirement}"))
            .arg(app)
            .output()
            .map_err(|e| format!("codesign didn't run: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("codesign refused it").trim().to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::swap::{self, Started, Trial};
    use std::cell::Cell;

    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>ssheesh</string>
	<key>CFBundleIdentifier</key>
	<string>io.github.footprintsntrails.ssheesh</string>
	<key>CFBundleShortVersionString</key>
	<string>@VERSION@</string>
</dict>
</plist>
"#;

    fn plist(version: &str) -> String {
        PLIST.replace("@VERSION@", version)
    }

    /// A tar.gz as the release workflow packs the Mac app: `SSHeesh.app` at the top.
    fn app_archive(version: &str, program: &[u8]) -> Vec<u8> {
        archive_of(&[("SSHeesh.app/Contents/Info.plist", plist(version).as_bytes(), 0o644), ("SSHeesh.app/Contents/MacOS/ssheesh", program, 0o755), ("SSHeesh.app/Contents/Resources/icon.icns", b"icon", 0o644)])
    }

    fn archive_of(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (name, bytes, mode) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// `/Applications` with SSHeesh.app 1.0.0 in it.
    fn applications() -> (tempfile::TempDir, Bundle) {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("SSHeesh.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"), plist("1.0.0")).unwrap();
        fs::write(app.join("Contents/MacOS/ssheesh"), b"version 1.0.0").unwrap();
        set_mode(&app.join("Contents/MacOS/ssheesh"), 0o755).unwrap();
        let bundle = Bundle::containing(&app.join("Contents/MacOS/ssheesh")).unwrap();
        (dir, bundle)
    }

    fn program_of(app: &Path) -> String {
        fs::read_to_string(app.join("Contents/MacOS/ssheesh")).unwrap_or_else(|_| "(gone)".into())
    }

    const EXPECT: Expect<'static> = Expect { bundle_id: BUNDLE_ID, program: "ssheesh", version: "1.0.1" };

    /// `codesign` made up: this app signed (`Some`) or not, and whether a new bundle passes.
    struct FakeSign {
        current: Option<&'static str>,
        passes: bool,
        asked: Cell<u32>,
    }

    impl Codesign for FakeSign {
        fn requirement(&self, _: &Path) -> Option<String> {
            self.current.map(str::to_owned)
        }
        fn verify(&self, app: &Path, requirement: &str) -> Result<(), String> {
            self.asked.set(self.asked.get() + 1);
            assert_eq!(Some(requirement), self.current);
            assert!(app.join("Contents/Info.plist").is_file(), "checked as an unpacked app");
            if self.passes {
                Ok(())
            } else {
                Err("a sealed resource is missing or invalid".into())
            }
        }
    }

    fn signed(passes: bool) -> FakeSign {
        FakeSign { current: Some(r#"identifier "io.github.footprintsntrails.ssheesh" and certificate leaf = H"ab""#), passes, asked: Cell::new(0) }
    }

    fn unsigned() -> FakeSign {
        FakeSign { current: None, passes: false, asked: Cell::new(0) }
    }

    struct FakeDisk {
        read_only: bool,
        can_replace: bool,
    }

    impl Disk for FakeDisk {
        fn read_only(&self, _: &Path) -> bool {
            self.read_only
        }
        fn can_replace(&self, _: &Path) -> bool {
            self.can_replace
        }
    }

    #[test]
    fn the_folders_sit_beside_the_app() {
        let b = Bundle::containing(Path::new("/Applications/SSHeesh.app/Contents/MacOS/ssheesh")).unwrap();
        assert_eq!(b.app, Path::new("/Applications/SSHeesh.app"));
        assert_eq!(b.exe(), Path::new("/Applications/SSHeesh.app/Contents/MacOS/ssheesh"));
        assert_eq!(b.staged, Path::new("/Applications/.SSHeesh-update"));
        assert_eq!(b.part, Path::new("/Applications/.SSHeesh-update.part"));
        assert_eq!(b.previous, Path::new("/Applications/.SSHeesh-previous"));
        // None of them is called .app: Launch Services never takes one for a second SSHeesh.
        assert!([&b.staged, &b.previous, &b.failed].iter().all(|p| p.extension().is_none()));
        assert!(Bundle::containing(Path::new("/Users/p/Downloads/ssheesh")).is_none());
        assert!(Bundle::containing(Path::new("/opt/Contents/MacOS/ssheesh")).is_none());
    }

    #[test]
    fn a_writable_folder_swaps_and_anything_else_offers_the_download() {
        let b = Bundle::containing(Path::new("/Applications/SSHeesh.app/Contents/MacOS/ssheesh")).unwrap();
        let disk = |read_only, can_replace| FakeDisk { read_only, can_replace };
        assert_eq!(route(&b, &disk(false, true)), Route::Swap);
        assert_eq!(route(&b, &disk(false, false)), Route::NotWritable);
        // A disk image, or Applications on a read-only disk.
        assert_eq!(route(&b, &disk(true, true)), Route::MoveToApplications);
        // Translocated: whatever the disk says, it's to be moved first.
        let t = Bundle::containing(Path::new("/private/var/folders/x1/abc/T/AppTranslocation/5A1B-C2/d/SSHeesh.app/Contents/MacOS/ssheesh")).unwrap();
        assert!(translocated(&t.app) && !translocated(&b.app));
        assert_eq!(route(&t, &disk(false, true)), Route::MoveToApplications);
    }

    #[test]
    fn the_real_disk_lets_its_owner_replace_an_app_and_leaves_no_probe() {
        let (dir, b) = applications();
        assert!(System.can_replace(&b.app));
        assert!(!System.can_replace(&dir.path().join("Nothing.app")));
        assert!(!System.read_only(dir.path()));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_checked_bundle_is_staged_installed_and_the_previous_kept() {
        let (dir, b) = applications();
        let sign = signed(true);
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &sign).unwrap();
        assert_eq!(sign.asked.get(), 1, "codesign checked it against this app's requirement");
        assert!(b.staged.is_dir() && !b.part.exists());
        assert_eq!(program_of(&b.staged), "version 1.0.1");
        assert_eq!(program_of(&b.app), "version 1.0.0", "staging leaves the app as it is");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(b.staged.join("Contents/MacOS/ssheesh")).unwrap().permissions().mode() & 0o777, 0o755);
            assert_eq!(fs::metadata(&b.staged).unwrap().permissions().mode() & 0o777, 0o755);
        }
        install(&b, &digest, &EXPECT, &unsigned()).unwrap();
        assert_eq!(program_of(&b.app), "version 1.0.1");
        assert_eq!(program_of(&b.previous), "version 1.0.0");
        assert!(!b.staged.exists());
        // The app, the previous version, nothing else.
        let mut left: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, [".SSHeesh-previous", "SSHeesh.app"]);
        // Without a staged bundle there's nothing to install, and the app stays.
        assert!(install(&b, &digest, &EXPECT, &unsigned()).is_err());
        assert_eq!(program_of(&b.app), "version 1.0.1");
    }

    #[test]
    fn an_unsigned_app_takes_a_new_bundle_without_asking_codesign() {
        let (_dir, b) = applications();
        let sign = unsigned();
        stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &sign).unwrap();
        assert_eq!(sign.asked.get(), 0);
    }

    /// Each refused bundle: said why, and nothing left next to the app.
    #[test]
    fn another_app_version_or_signature_is_refused() {
        let (dir, b) = applications();
        let other_id = archive_of(&[("SSHeesh.app/Contents/Info.plist", plist("1.0.1").replace(BUNDLE_ID, "com.example.other").as_bytes(), 0o644), ("SSHeesh.app/Contents/MacOS/ssheesh", b"x", 0o755)]);
        let other_program = archive_of(&[("SSHeesh.app/Contents/Info.plist", plist("1.0.1").replace("<string>ssheesh</string>", "<string>sneaky</string>").as_bytes(), 0o644), ("SSHeesh.app/Contents/MacOS/sneaky", b"x", 0o755)]);
        let no_program = archive_of(&[("SSHeesh.app/Contents/Info.plist", plist("1.0.1").as_bytes(), 0o644)]);
        let not_runnable = archive_of(&[("SSHeesh.app/Contents/Info.plist", plist("1.0.1").as_bytes(), 0o644), ("SSHeesh.app/Contents/MacOS/ssheesh", b"x", 0o644)]);
        let cases: [(&str, Vec<u8>, FakeSign, &str); 6] = [
            ("another bundle id", other_id, signed(true), "another app"),
            ("another version", app_archive("1.0.2", b"x"), signed(true), "not 1.0.1"),
            ("another program", other_program, signed(true), "its program is called"),
            ("no program", no_program, signed(true), "missing"),
            ("a program that can't run", not_runnable, signed(true), "can't run"),
            ("not signed as this app", app_archive("1.0.1", b"x"), signed(false), "isn't signed as this app is"),
        ];
        for (what, archive, sign, why) in cases {
            if what == "a program that can't run" && !cfg!(unix) {
                continue;
            }
            match stage(&b, &archive, &EXPECT, &sign) {
                Err(Unstaged::Refused(e)) => assert!(e.contains(why), "{what}: {e}"),
                other => panic!("{what}: {other:?}"),
            }
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "{what}: only the app is left");
            assert_eq!(program_of(&b.app), "version 1.0.0");
        }
    }

    #[test]
    fn an_archive_that_isnt_one_app_is_refused() {
        let (dir, b) = applications();
        let two = archive_of(&[("SSHeesh.app/Contents/Info.plist", plist("1.0.1").as_bytes(), 0o644), ("Other.app/Contents/Info.plist", b"x", 0o644)]);
        let loose = archive_of(&[("ssheesh", b"x", 0o755)]);
        let mut link = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        link.append_link(&mut header, "SSHeesh.app/Contents/MacOS/ssheesh", "/bin/sh").unwrap();
        let link = link.into_inner().unwrap().finish().unwrap();
        for (what, archive) in [("two apps", two), ("a loose program", loose), ("a link", link), ("not an archive", b"not an archive".to_vec())] {
            assert!(matches!(stage(&b, &archive, &EXPECT, &unsigned()), Err(Unstaged::Refused(_))), "{what}");
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "{what}");
        }
    }

    #[test]
    fn a_bundle_changed_after_its_check_is_not_installed() {
        let (_dir, b) = applications();
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &unsigned()).unwrap();
        fs::write(b.staged.join("Contents/MacOS/ssheesh"), b"something else").unwrap();
        assert_eq!(install(&b, &digest, &EXPECT, &unsigned()), Err("the new version changed after it was checked".into()));
        assert_eq!(program_of(&b.app), "version 1.0.0");
        assert!(!b.previous.exists());
        // A mode alone changes the digest too.
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &unsigned()).unwrap();
        #[cfg(unix)]
        {
            set_mode(&b.staged.join("Contents/Resources/icon.icns"), 0o755).unwrap();
            assert!(install(&b, &digest, &EXPECT, &unsigned()).is_err());
        }
    }

    /// staged.json is this user's to write: a bundle swapped in after staging, with its digest
    /// written there to match, still has to pass the whole check.
    #[test]
    fn a_bundle_swapped_after_staging_with_a_forged_digest_is_refused() {
        type Swap = (&'static str, fn(&Path), FakeSign);
        let swaps: [Swap; 4] = [
            ("another app", |s| fs::write(s.join("Contents/Info.plist"), plist("1.0.1").replace(BUNDLE_ID, "com.example.other")).unwrap(), signed(true)),
            ("an older version", |s| fs::write(s.join("Contents/Info.plist"), plist("0.9.0")).unwrap(), signed(true)),
            ("no program", |s| fs::remove_file(s.join("Contents/MacOS/ssheesh")).unwrap(), signed(true)),
            ("not signed as this app", |s| fs::write(s.join("Contents/MacOS/ssheesh"), b"something else").unwrap(), signed(false)),
        ];
        for (what, swap, sign) in swaps {
            let (_dir, b) = applications();
            stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &signed(true)).unwrap();
            swap(&b.staged);
            let forged = digest(&b.staged).unwrap();
            match install(&b, &forged, &EXPECT, &sign) {
                Err(e) => assert!(e.starts_with("the new version isn't the one to install"), "{what}: {e}"),
                Ok(()) => panic!("{what}: installed"),
            }
            assert_eq!(program_of(&b.app), "version 1.0.0", "{what}");
            assert!(!b.previous.exists() && b.staged.exists(), "{what}");
        }
        // The real one passes, codesign asked again at install.
        let (_dir, b) = applications();
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &signed(true)).unwrap();
        let sign = signed(true);
        install(&b, &digest, &EXPECT, &sign).unwrap();
        assert_eq!(sign.asked.get(), 1);
        assert_eq!(program_of(&b.app), "version 1.0.1");
    }

    /// Writes a PAX extended header carrying `records` ahead of the next entry.
    fn pax(builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>, records: &[(&str, &str)]) {
        let mut data = Vec::new();
        for (key, value) in records {
            let body = format!(" {key}={value}\n");
            // The record's length counts its own digits.
            let mut len = body.len() + 1;
            while format!("{len}{body}").len() != len {
                len += 1;
            }
            data.extend_from_slice(format!("{len}{body}").as_bytes());
        }
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::XHeader);
        header.set_path("PaxHeaders/x").unwrap();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();
    }

    fn file_header(name: &str, size: u64) -> tar::Header {
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path(name).unwrap();
        header.set_size(size);
        header.set_mode(0o644);
        header.set_cksum();
        header
    }

    fn written(root: &Path) -> u64 {
        walk(root).unwrap().iter().filter(|(_, m)| m.is_file()).map(|(_, m)| m.len()).sum()
    }

    const SMALL: Limits = Limits { bytes: 64 * 1024, entries: 10, stream: 1024 * 1024 };

    #[test]
    fn a_pax_size_bigger_than_its_header_counts_and_is_refused() {
        // The plain header says 0 bytes; the PAX record says 1 MB, which is what unpacks.
        let body = vec![0u8; 1024 * 1024];
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        pax(&mut builder, &[("size", &body.len().to_string())]);
        builder.append(&file_header("SSHeesh.app/Contents/MacOS/ssheesh", 0), &body[..]).unwrap();
        let archive = builder.into_inner().unwrap().finish().unwrap();
        // It really does unpack to the PAX size: the plain header can't be what's counted.
        let dir = tempfile::tempdir().unwrap();
        unpack_within(&archive, dir.path(), Limits { bytes: u64::MAX, entries: 10, stream: u64::MAX }).unwrap();
        assert_eq!(written(dir.path()), body.len() as u64);
        let dir = tempfile::tempdir().unwrap();
        match unpack_within(&archive, dir.path(), SMALL) {
            Err(Unstaged::Refused(e)) => assert!(e.contains("too big"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(written(dir.path()), 0, "nothing of it written");
    }

    #[test]
    fn an_archive_inflating_past_its_stream_cap_is_refused_while_it_unpacks() {
        // Within the files' limit, past the whole stream's: refused as it inflates.
        let archive = archive_of(&[("SSHeesh.app/Contents/MacOS/ssheesh", &[7u8; 300 * 1024], 0o755)]);
        let dir = tempfile::tempdir().unwrap();
        match unpack_within(&archive, dir.path(), Limits { bytes: u64::MAX, entries: 10, stream: 64 * 1024 }) {
            Err(Unstaged::Refused(e)) => assert!(e.contains("too big"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(written(dir.path()) <= 64 * 1024);
        // The same archive within its caps unpacks.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unpack_within(&archive, dir.path(), Limits { bytes: u64::MAX, entries: 10, stream: 1024 * 1024 }).unwrap(), "SSHeesh.app");
    }

    #[test]
    fn an_archive_with_too_many_entries_is_refused() {
        let names: Vec<String> = (0..11).map(|i| format!("SSHeesh.app/Contents/Resources/{i}")).collect();
        let files: Vec<(&str, &[u8], u32)> = names.iter().map(|n| (n.as_str(), &b"x"[..], 0o644)).collect();
        let dir = tempfile::tempdir().unwrap();
        match unpack_within(&archive_of(&files), dir.path(), SMALL) {
            Err(Unstaged::Refused(e)) => assert!(e.contains("too many"), "{e}"),
            other => panic!("{other:?}"),
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(unpack_within(&archive_of(&files[..10]), dir.path(), SMALL).is_ok());
    }

    #[test]
    fn roll_back_puts_the_previous_app_back() {
        let (dir, b) = applications();
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &unsigned()).unwrap();
        install(&b, &digest, &EXPECT, &unsigned()).unwrap();
        roll_back(&b).unwrap();
        assert_eq!(program_of(&b.app), "version 1.0.0");
        assert!(!b.previous.exists() && !b.failed.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        // Nothing to go back to: said, and the app stays.
        assert!(roll_back(&b).is_err());
        assert_eq!(program_of(&b.app), "version 1.0.0");
    }

    /// The trial, as the helper runs it: a new app that closes at once is replaced by the
    /// previous one; one that says it started stays.
    #[test]
    fn a_new_app_that_doesnt_start_is_rolled_back() {
        struct Fake(u32, Option<PathBuf>);
        impl Started for Fake {
            fn exited(&mut self) -> bool {
                self.0 += 1;
                if let Some(m) = self.1.as_ref().filter(|_| self.0 == 2) {
                    fs::write(m, b"").unwrap();
                }
                self.1.is_none() && self.0 >= 2
            }
            fn kill(&mut self) {}
        }
        let poll = Duration::from_millis(1);
        for says in [false, true] {
            let (dir, b) = applications();
            let marker = dir.path().join("started-1.0.1");
            let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &unsigned()).unwrap();
            install(&b, &digest, &EXPECT, &unsigned()).unwrap();
            let trial = swap::watch(&b, &marker, Duration::from_secs(5), poll, |app| {
                assert_eq!(app, b.app, "the app is started, not the program in it");
                Ok(Fake(0, says.then(|| marker.clone())))
            });
            if says {
                assert_eq!(trial, Trial::Started);
                assert_eq!(program_of(&b.app), "version 1.0.1");
                assert!(b.previous.exists(), "kept for 30 days");
            } else {
                assert_eq!(trial, Trial::RolledBack("it closed as it started".into()));
                assert_eq!(program_of(&b.app), "version 1.0.0");
            }
        }
    }

    #[test]
    fn the_previous_app_goes_after_30_days() {
        let (dir, b) = applications();
        let folder = dir.path().join("canary");
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &unsigned()).unwrap();
        install(&b, &digest, &EXPECT, &unsigned()).unwrap();
        let now = SystemTime::now();
        note_previous(&folder, now);
        keep_previous(&folder, &b, now + Duration::from_secs(29 * 24 * 3600));
        assert!(b.previous.exists());
        keep_previous(&folder, &b, now + Duration::from_secs(31 * 24 * 3600));
        assert!(!b.previous.exists() && !folder.join(PREVIOUS_SINCE).exists());
        // Found without its date: its 30 days start now.
        fs::create_dir(&b.previous).unwrap();
        keep_previous(&folder, &b, now);
        assert!(b.previous.exists() && folder.join(PREVIOUS_SINCE).exists());
    }

    #[test]
    fn clean_takes_what_an_update_left_but_not_the_previous() {
        let (_dir, b) = applications();
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.1"), &EXPECT, &unsigned()).unwrap();
        fs::create_dir(&b.part).unwrap();
        clean(&b, true);
        assert!(b.staged.exists() && !b.part.exists());
        install(&b, &digest, &EXPECT, &unsigned()).unwrap();
        fs::create_dir(&b.failed).unwrap();
        clean(&b, false);
        assert!(!b.failed.exists() && b.previous.exists());
        // A second update replaces the previous version with this one.
        let digest = stage(&b, &app_archive("1.0.1", b"version 1.0.2"), &EXPECT, &unsigned()).unwrap();
        install(&b, &digest, &EXPECT, &unsigned()).unwrap();
        assert_eq!(program_of(&b.previous), "version 1.0.1");
        assert_eq!(program_of(&b.app), "version 1.0.2");
    }

    #[test]
    fn the_digest_follows_the_bundle_not_its_name() {
        let (dir, b) = applications();
        let before = digest(&b.app).unwrap();
        let moved = dir.path().join(".SSHeesh-previous");
        fs::rename(&b.app, &moved).unwrap();
        assert_eq!(digest(&moved).unwrap(), before);
        fs::write(moved.join("Contents/Info.plist"), plist("1.0.9")).unwrap();
        assert_ne!(digest(&moved).unwrap(), before);
    }

    #[test]
    fn info_plist_strings_read_as_the_workflow_writes_them() {
        let p = plist("1.0.1-rc.2");
        assert_eq!(plist_string(&p, "CFBundleShortVersionString").as_deref(), Some("1.0.1-rc.2"));
        assert_eq!(plist_string(&p, "CFBundleIdentifier").as_deref(), Some(BUNDLE_ID));
        assert_eq!(plist_string(&p, "CFBundleName"), None);
        // The real one the release workflow fills in.
        let real = include_str!("../../macos/Info.plist").replace("@VERSION@", "1.0.1");
        assert_eq!(plist_string(&real, "CFBundleIdentifier").as_deref(), Some(BUNDLE_ID));
        assert_eq!(plist_string(&real, "CFBundleExecutable").as_deref(), Some("ssheesh"));
        assert_eq!(plist_string(&real, "CFBundleShortVersionString").as_deref(), Some("1.0.1"));
    }
}
