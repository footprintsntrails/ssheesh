//! Putting a checked build in place of the running one, and going back when it doesn't start.
//! Files only, next to the program (`Places`): no network, no window, so every step is
//! tried on a folder of made-up programs in the tests.
//!
//! 1. **Stage** (in the background, after the download is checked): the new program is
//!    written to `<name>.new.part`, flushed to the disk, made executable, then renamed to
//!    `<name>.new`. A crash leaves at most a `.part`, never a program half there.
//! 2. **Install** (on "Restart to update"): the running program keeps a second name,
//!    `<name>.old`, and `<name>.new` takes its place. macOS and Linux: a hard link (a copy
//!    where links can't be made) and one atomic rename, so the program's name always holds a
//!    whole program. Windows won't replace a running program but lets it be renamed: it steps
//!    aside as `<name>.old.exe`, the new one takes the name, and if that fails it steps back.
//! 3. **Trial** (`watch`, run by the old program as a helper once the window has closed): the
//!    new program starts and has [`TRIAL`] to say it started (a marker file it writes once
//!    its window has run a few seconds). If it exits first, can't be started, or says
//!    nothing in time, the old program goes back in place ([`roll_back`]) and starts again.
//! 4. **Clean** (at the next start): the `.old`, `.failed` and stale files go.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a new build has to say it started.
pub const TRIAL: Duration = Duration::from_secs(60);

/// The program and the names beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// The running program.
    pub exe: PathBuf,
    /// The checked new build, waiting for a restart.
    pub new: PathBuf,
    /// The new build while it's written.
    pub part: PathBuf,
    /// The build from before, kept until the new one has started.
    pub old: PathBuf,
    /// A new build that didn't start, set aside (Windows; elsewhere it's replaced outright).
    pub failed: PathBuf,
}

/// "ssheesh-preview.exe" → "ssheesh-preview.<tag>.exe" next to it; "ssheesh-preview" →
/// "ssheesh-preview.<tag>". The extension stays last so Windows still runs the old one.
fn beside(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    match exe.extension() {
        Some(ext) => exe.with_file_name(format!("{stem}.{tag}.{}", ext.to_string_lossy())),
        None => exe.with_file_name(format!("{stem}.{tag}")),
    }
}

impl Places {
    pub fn of(exe: &Path) -> Places {
        Places { exe: exe.to_path_buf(), new: beside(exe, "new"), part: beside(exe, "new.part"), old: beside(exe, "old"), failed: beside(exe, "failed") }
    }
}

/// Runs a file step again for a few seconds while something else holds the file (an
/// antivirus looking at a new program, a backup app reading it), as 0.10's portable update.
fn retrying<T>(mut step: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    // 32: the file is in use by another program (Windows).
    let held = |e: &std::io::Error| e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(32);
    let mut tries = 0;
    loop {
        match step() {
            Err(e) if cfg!(windows) && held(&e) && tries < 8 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(if cfg!(test) { 1 } else { 500 }));
            }
            done => return done,
        }
    }
}

fn remove(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        done => done,
    }
}

/// The folder's own record of a rename, on the disk (where the system has one).
fn sync_folder(file: &Path) {
    #[cfg(unix)]
    if let Some(dir) = file.parent() {
        let _ = fs::File::open(dir).and_then(|d| d.sync_all());
    }
    #[cfg(not(unix))]
    let _ = file;
}

/// Whether a program can be written next to `exe`: its folder takes a new file.
pub fn writable(places: &Places) -> bool {
    let probe = beside(&places.exe, "probe");
    let made = fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_ok();
    let _ = fs::remove_file(&probe);
    made
}

/// Makes `path` runnable as the program at `like` is (its mode, with every execute bit), and
/// drops macOS's quarantine mark should anything have put one there (a file this program
/// writes gets none, but a folder that marks everything would stop it from starting).
fn make_runnable(path: &Path, like: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(like).map(|m| m.permissions().mode() & 0o7777).unwrap_or(0o755) | 0o111;
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o0775))?;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
            // SAFETY: two NUL-terminated strings that live across the call.
            unsafe { libc::removexattr(c.as_ptr(), c"com.apple.quarantine".as_ptr(), libc::XATTR_NOFOLLOW) };
        }
    }
    let _ = like;
    Ok(())
}

/// Writes the checked new build next to the program as `<name>.new`, whole or not at all.
pub fn stage(places: &Places, program: &[u8]) -> std::io::Result<()> {
    remove(&places.part)?;
    remove(&places.new)?;
    let written = write_staged(places, program);
    if written.is_err() {
        let _ = fs::remove_file(&places.part);
    }
    written
}

fn write_staged(places: &Places, program: &[u8]) -> std::io::Result<()> {
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&places.part)?;
    f.write_all(program)?;
    f.sync_all()?;
    drop(f);
    make_runnable(&places.part, &places.exe)?;
    // An antivirus may take a new program away as it's saved.
    if !places.part.exists() {
        return Err(std::io::Error::other("the new build was taken away right after it was saved, most likely by an antivirus"));
    }
    retrying(|| fs::rename(&places.part, &places.new))?;
    sync_folder(&places.new);
    Ok(())
}

/// Whether the staged build is still the one checked (its SHA-256, `sha256`), read again.
fn unchanged(places: &Places, sha256: &str) -> Result<(), String> {
    match fs::read(&places.new) {
        Ok(bytes) if super::verify::sha256_hex(&bytes).eq_ignore_ascii_case(sha256) => Ok(()),
        Ok(_) => Err("the new build changed after it was checked".into()),
        Err(e) => Err(format!("the new build can't be read: {e}")),
    }
}

/// Puts the staged build in place of the program, keeping the program as `<name>.old`. The
/// staged build is read again right before it takes the program's name, and must still be
/// the one checked (`sha256`, recorded when it was staged). On failure the program is where
/// it was.
pub fn install(places: &Places, sha256: &str) -> Result<(), String> {
    if !places.new.is_file() {
        return Err("the new build isn't next to this one any more".into());
    }
    remove(&places.old).map_err(|e| format!("the build from before can't be cleared away: {e}"))?;
    if cfg!(windows) {
        retrying(|| fs::rename(&places.exe, &places.old)).map_err(|e| format!("this build can't step aside: {e}"))?;
        if let Err(e) = unchanged(places, sha256) {
            let _ = retrying(|| fs::rename(&places.old, &places.exe));
            return Err(e);
        }
        if let Err(e) = retrying(|| fs::rename(&places.new, &places.exe)) {
            let _ = retrying(|| fs::rename(&places.old, &places.exe));
            return Err(format!("the new build can't take its place: {e}"));
        }
    } else {
        // The running program under a second name: a link to the same file, or a copy.
        if fs::hard_link(&places.exe, &places.old).is_err() {
            fs::copy(&places.exe, &places.old).and_then(|_| fs::File::open(&places.old)?.sync_all()).map_err(|e| format!("this build can't be kept: {e}"))?;
            make_runnable(&places.old, &places.exe).map_err(|e| e.to_string())?;
        }
        if let Err(e) = unchanged(places, sha256) {
            let _ = fs::remove_file(&places.old);
            return Err(e);
        }
        if let Err(e) = fs::rename(&places.new, &places.exe) {
            let _ = fs::remove_file(&places.old);
            return Err(format!("the new build can't take its place: {e}"));
        }
    }
    sync_folder(&places.exe);
    Ok(())
}

/// Puts the build from before back in place of a new one that didn't start.
pub fn roll_back(places: &Places) -> Result<(), String> {
    if !places.old.is_file() {
        return Err("the build from before is gone".into());
    }
    if cfg!(windows) && places.exe.exists() {
        // The new one steps aside first: Windows' rename won't write over a program.
        remove(&places.failed).map_err(|e| e.to_string())?;
        retrying(|| fs::rename(&places.exe, &places.failed)).map_err(|e| format!("the new build can't step aside: {e}"))?;
    }
    retrying(|| fs::rename(&places.old, &places.exe)).map_err(|e| format!("the build from before can't go back: {e}"))?;
    sync_folder(&places.exe);
    let _ = fs::remove_file(&places.failed);
    Ok(())
}

/// Clears what an update leaves next to the program: the build from before, one that failed,
/// a write cut off. `keep_new` keeps a staged build that's still newer than this one.
pub fn clean(places: &Places, keep_new: bool) {
    for path in [&places.old, &places.failed, &places.part] {
        let _ = fs::remove_file(path);
    }
    if !keep_new {
        let _ = fs::remove_file(&places.new);
    }
}

/// A started program, as the trial watches it.
pub trait Started {
    /// It has exited.
    fn exited(&mut self) -> bool;
    fn kill(&mut self);
}

impl Started for std::process::Child {
    fn exited(&mut self) -> bool {
        !matches!(self.try_wait(), Ok(None))
    }

    fn kill(&mut self) {
        let _ = std::process::Child::kill(self);
        let _ = self.wait();
    }
}

/// How a trial ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trial {
    /// The new build said it started.
    Started,
    /// It didn't, and the build from before is back in place; why, one line.
    RolledBack(String),
    /// It didn't, and going back failed too: the new build stays.
    Stuck(String),
}

/// What an update replaced, as the trial starts it and puts it back: a program
/// ([`Places`]), or a whole Mac app (`bundle::Bundle`).
pub trait Replaced {
    /// What `watch` hands its `launch`: the new program, or the new app.
    fn program(&self) -> PathBuf;
    /// Puts the build from before back in place.
    fn roll_back(&self) -> Result<(), String>;
}

impl Replaced for Places {
    fn program(&self) -> PathBuf {
        self.exe.clone()
    }

    fn roll_back(&self) -> Result<(), String> {
        roll_back(self)
    }
}

/// Starts the new build (`replaced.program()`) through `launch` and waits up to `wait` for
/// `marker` to appear; when it doesn't, the build from before goes back in place.
pub fn watch<S: Started>(replaced: &impl Replaced, marker: &Path, wait: Duration, poll: Duration, launch: impl FnOnce(&Path) -> std::io::Result<S>) -> Trial {
    let why = match launch(&replaced.program()) {
        Err(e) => format!("it couldn't be started: {e}"),
        Ok(mut child) => {
            let since = Instant::now();
            loop {
                if marker.exists() {
                    return Trial::Started;
                }
                if child.exited() {
                    // It may have said so on its way out.
                    if marker.exists() {
                        return Trial::Started;
                    }
                    break "it closed as it started".to_owned();
                }
                if since.elapsed() >= wait {
                    child.kill();
                    break format!("it didn't start within {} seconds", wait.as_secs());
                }
                std::thread::sleep(poll);
            }
        }
    };
    match replaced.roll_back() {
        Ok(()) => Trial::RolledBack(why),
        Err(e) => Trial::Stuck(format!("{why}, and {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder with a made-up program in it, and a new build staged next to it.
    fn folder(name: &str) -> (tempfile::TempDir, Places) {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() });
        fs::write(&exe, b"the build from before").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o750)).unwrap();
        }
        let places = Places::of(&exe);
        stage(&places, b"the new build").unwrap();
        (dir, places)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|_| "(gone)".into())
    }

    #[test]
    fn names_sit_beside_the_program() {
        let p = Places::of(Path::new("apps/ssheesh-preview.exe"));
        assert_eq!(p.old, Path::new("apps/ssheesh-preview.old.exe"));
        assert_eq!(p.new, Path::new("apps/ssheesh-preview.new.exe"));
        let p = Places::of(Path::new("apps/ssheesh-preview"));
        assert_eq!((p.old.as_path(), p.part.as_path()), (Path::new("apps/ssheesh-preview.old"), Path::new("apps/ssheesh-preview.new.part")));
    }

    #[test]
    fn a_staged_build_is_whole_and_runnable() {
        let (_dir, places) = folder("ssheesh-preview");
        assert_eq!(read(&places.new), "the new build");
        assert!(!places.part.exists());
        assert!(writable(&places));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The program's own mode, every execute bit on, never writable by others.
            assert_eq!(fs::metadata(&places.new).unwrap().permissions().mode() & 0o777, 0o751);
        }
        // Staged again: the newer one wins, nothing is left half written.
        stage(&places, b"an even newer build").unwrap();
        assert_eq!(read(&places.new), "an even newer build");
    }

    #[test]
    fn install_puts_the_new_build_in_place_and_keeps_the_old() {
        let (_dir, places) = folder("ssheesh-preview");
        install(&places, &checked()).unwrap();
        assert_eq!(read(&places.exe), "the new build");
        assert_eq!(read(&places.old), "the build from before");
        assert!(!places.new.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&places.exe).unwrap().permissions().mode() & 0o111, 0o111);
            assert_eq!(fs::metadata(&places.old).unwrap().permissions().mode() & 0o777, 0o750);
        }
        // Without a staged build there's nothing to install, and the program stays.
        assert!(install(&places, &checked()).is_err());
        assert_eq!(read(&places.exe), "the new build");
    }

    /// The SHA-256 recorded when `folder`'s new build was staged.
    fn checked() -> String {
        super::super::verify::sha256_hex(b"the new build")
    }

    /// Changed after it was checked (between "Restart to update" and the swap): it doesn't
    /// take the program's place, and the program stays as it was.
    #[test]
    fn a_staged_build_changed_after_its_check_is_not_installed() {
        let (_dir, places) = folder("ssheesh-preview");
        fs::write(&places.new, b"something else").unwrap();
        assert_eq!(install(&places, &checked()), Err("the new build changed after it was checked".into()));
        assert_eq!(read(&places.exe), "the build from before");
        assert!(!places.old.exists());
        assert_eq!(read(&places.new), "something else");
    }

    #[test]
    fn roll_back_puts_the_old_build_back() {
        let (_dir, places) = folder("ssheesh-preview");
        install(&places, &checked()).unwrap();
        roll_back(&places).unwrap();
        assert_eq!(read(&places.exe), "the build from before");
        assert!(!places.old.exists() && !places.failed.exists());
        // Nothing to go back to: said, and the program stays.
        assert!(roll_back(&places).is_err());
        assert_eq!(read(&places.exe), "the build from before");
    }

    #[test]
    fn clean_takes_what_an_update_left() {
        let (_dir, places) = folder("ssheesh-preview");
        fs::write(&places.part, b"cut off").unwrap();
        clean(&places, true);
        assert!(places.new.exists() && !places.part.exists());
        install(&places, &checked()).unwrap();
        clean(&places, false);
        assert!(!places.old.exists() && !places.new.exists());
        assert_eq!(read(&places.exe), "the new build");
    }

    /// A made-up started program: it says it started (writes the marker) on its `n`th look,
    /// exits on its `exit_at`th, or never does either.
    struct Fake {
        looks: u32,
        marker: PathBuf,
        says_at: Option<u32>,
        exit_at: Option<u32>,
        killed: bool,
    }

    impl Started for Fake {
        fn exited(&mut self) -> bool {
            self.looks += 1;
            if self.says_at == Some(self.looks) {
                fs::write(&self.marker, b"").unwrap();
            }
            self.exit_at.is_some_and(|n| self.looks >= n)
        }

        fn kill(&mut self) {
            self.killed = true;
        }
    }

    fn fake(marker: &Path, says_at: Option<u32>, exit_at: Option<u32>) -> Fake {
        Fake { looks: 0, marker: marker.to_path_buf(), says_at, exit_at, killed: false }
    }

    const POLL: Duration = Duration::from_millis(1);

    #[test]
    fn a_build_that_says_it_started_stays() {
        let (dir, places) = folder("ssheesh-preview");
        let marker = dir.path().join("started-125");
        install(&places, &checked()).unwrap();
        let trial = watch(&places, &marker, Duration::from_secs(5), POLL, |exe| {
            assert_eq!(read(exe), "the new build");
            Ok(fake(&marker, Some(3), None))
        });
        assert_eq!(trial, Trial::Started);
        assert_eq!(read(&places.exe), "the new build");
    }

    #[test]
    fn a_build_that_closes_at_once_is_rolled_back() {
        let (dir, places) = folder("ssheesh-preview");
        install(&places, &checked()).unwrap();
        let trial = watch(&places, &dir.path().join("started-125"), Duration::from_secs(5), POLL, |_| Ok(fake(&dir.path().join("started-125"), None, Some(2))));
        assert_eq!(trial, Trial::RolledBack("it closed as it started".into()));
        assert_eq!(read(&places.exe), "the build from before");
    }

    #[test]
    fn a_build_that_says_nothing_in_time_is_stopped_and_rolled_back() {
        let (dir, places) = folder("ssheesh-preview");
        install(&places, &checked()).unwrap();
        let mut killed = false;
        let trial = watch(&places, &dir.path().join("started-125"), Duration::from_millis(30), POLL, |_| {
            Ok(KillSpy { inner: fake(&dir.path().join("started-125"), None, None), killed: &mut killed })
        });
        assert!(matches!(trial, Trial::RolledBack(ref why) if why.contains("didn't start within")), "{trial:?}");
        assert!(killed);
        assert_eq!(read(&places.exe), "the build from before");
    }

    struct KillSpy<'a> {
        inner: Fake,
        killed: &'a mut bool,
    }

    impl Started for KillSpy<'_> {
        fn exited(&mut self) -> bool {
            self.inner.exited()
        }
        fn kill(&mut self) {
            self.inner.kill();
            *self.killed = self.inner.killed;
        }
    }

    #[test]
    fn a_build_that_cant_be_started_is_rolled_back() {
        let (dir, places) = folder("ssheesh-preview");
        install(&places, &checked()).unwrap();
        let trial = watch::<Fake>(&places, &dir.path().join("started-125"), Duration::from_secs(5), POLL, |_| Err(std::io::Error::other("not a program")));
        assert_eq!(trial, Trial::RolledBack("it couldn't be started: not a program".into()));
        assert_eq!(read(&places.exe), "the build from before");
    }

    #[test]
    fn with_nothing_to_go_back_to_the_new_build_stays() {
        let (dir, places) = folder("ssheesh-preview");
        install(&places, &checked()).unwrap();
        fs::remove_file(&places.old).unwrap();
        let trial = watch(&places, &dir.path().join("started-125"), Duration::from_secs(5), POLL, |_| Ok(fake(&dir.path().join("started-125"), None, Some(1))));
        assert!(matches!(trial, Trial::Stuck(_)));
        assert_eq!(read(&places.exe), "the new build");
    }

    /// The real thing on macOS and Linux: made-up programs as shell scripts, started as the
    /// helper starts the new build.
    #[cfg(unix)]
    #[test]
    fn real_programs_are_started_and_watched() {
        use std::process::{Command, Stdio};
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("started-125");
        let launch = |exe: &Path| Command::new(exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        for (n, (script, started)) in [(format!("#!/bin/sh\n: > '{}'\nsleep 5\n", marker.display()), true), ("#!/bin/sh\nexit 3\n".to_owned(), false)].into_iter().enumerate() {
            let exe = dir.path().join(format!("ssheesh-preview-{n}"));
            let _ = fs::remove_file(&marker);
            fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
            }
            let places = Places::of(&exe);
            stage(&places, script.as_bytes()).unwrap();
            install(&places, &super::super::verify::sha256_hex(script.as_bytes())).unwrap();
            let trial = watch(&places, &marker, Duration::from_secs(20), Duration::from_millis(20), launch);
            if started {
                assert_eq!(trial, Trial::Started);
                assert_eq!(read(&exe), script);
            } else {
                assert_eq!(trial, Trial::RolledBack("it closed as it started".into()));
                assert_eq!(read(&exe), "#!/bin/sh\nexit 0\n");
            }
            clean(&places, false);
        }
    }
}
