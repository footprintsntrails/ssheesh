//! The installer's file steps in the install folder, made so a power loss at any point leaves
//! a program that starts (plan §6.2 step 5):
//!
//! 1. the new program is written as `ssheesh.exe.new` (through `.new.part`, renamed when
//!    whole);
//! 2. (the caller puts a `RunOnce` entry that runs `ssheesh.exe.new --finish-install <folder>`);
//! 3. `ssheesh.exe` moves to `previous\ssheesh.exe` (or to `ssheesh.exe.old` when `previous\`
//!    already keeps a version to go back to);
//! 4. `ssheesh.exe.new` takes the name `ssheesh.exe`;
//! 5. (the caller removes the `RunOnce` entry).
//!
//! The program in use is renamed, never written over: Windows lets a running program's file be
//! renamed, and 0.10's file may still run as an ssh helper (askpass, the Tor proxy).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::names::{KEEP_DAYS, KEPT_NOTE, MAIN_EXE, NEW_SUFFIX, NSIS_UNINSTALLER, OLD_SUFFIX, PREVIOUS};

/// The install folder's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub new: PathBuf,
    pub old: PathBuf,
    pub previous: PathBuf,
}

impl Layout {
    pub fn of(dir: &Path) -> Layout {
        Layout {
            dir: dir.to_path_buf(),
            exe: dir.join(MAIN_EXE),
            new: dir.join(format!("{MAIN_EXE}{NEW_SUFFIX}")),
            old: dir.join(format!("{MAIN_EXE}{OLD_SUFFIX}")),
            previous: dir.join(PREVIOUS),
        }
    }

    pub fn previous_exe(&self) -> PathBuf {
        self.previous.join(MAIN_EXE)
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// Step 1: `program` (this installer's own file) written whole as `ssheesh.exe.new`. Nothing
/// to write when the installer already is that file (1.x updating itself starts the staged
/// program as its installer).
pub fn stage(layout: &Layout, program: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(&layout.dir)?;
    if same_file(program, &layout.new) {
        return Ok(());
    }
    let part = layout.dir.join(format!("{MAIN_EXE}{NEW_SUFFIX}.part"));
    let bytes = std::fs::read(program)?;
    {
        let mut f = std::fs::File::create(&part)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&part, &layout.new)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// What step 3 did with the program that was there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// There was none: a fresh install.
    Nothing,
    /// Moved to `previous\`, with 0.10's uninstaller copied beside it.
    Previous,
    /// `previous\` already keeps an older version to go back to: this one went to `.old`, and
    /// goes once the new one has started.
    Old,
}

/// Step 3: the program in the folder moves out of the way. `version` is noted with it.
pub fn keep_previous(layout: &Layout, version: Option<&str>) -> std::io::Result<Kept> {
    if !layout.exe.exists() {
        return Ok(Kept::Nothing);
    }
    if layout.previous_exe().exists() {
        let _ = std::fs::remove_file(&layout.old);
        std::fs::rename(&layout.exe, &layout.old)?;
        return Ok(Kept::Old);
    }
    std::fs::create_dir_all(&layout.previous)?;
    let nsis = layout.dir.join(NSIS_UNINSTALLER);
    if nsis.exists() {
        std::fs::copy(&nsis, layout.previous.join(NSIS_UNINSTALLER))?;
    }
    std::fs::write(layout.previous.join(KEPT_NOTE), format!("since {}\nversion {}\n", now_secs(), version.unwrap_or("unknown")))?;
    std::fs::rename(&layout.exe, layout.previous_exe())?;
    Ok(Kept::Previous)
}

/// Step 4.
pub fn take_name(layout: &Layout) -> std::io::Result<()> {
    std::fs::rename(&layout.new, &layout.exe)
}

/// What `--finish-install` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finished {
    /// Nothing was left half done.
    Nothing,
    /// The new program took its name.
    Done,
}

/// The `RunOnce` entry's work: steps 3 and 4 if they didn't happen. Safe to run any number of
/// times.
pub fn finish(layout: &Layout) -> std::io::Result<Finished> {
    if !layout.new.exists() {
        return Ok(Finished::Nothing);
    }
    if layout.exe.exists() {
        keep_previous(layout, None)?;
    }
    take_name(layout)?;
    Ok(Finished::Done)
}

/// When `previous\` was made (its note), if it was.
pub fn kept_since(layout: &Layout) -> Option<SystemTime> {
    let note = std::fs::read_to_string(layout.previous.join(KEPT_NOTE)).ok()?;
    let secs: u64 = note.lines().find_map(|l| l.strip_prefix("since "))?.trim().parse().ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

/// Whether `previous\` has been kept its 30 days at `now`. A folder with no readable note isn't
/// due (it may be a person's own).
pub fn previous_due(layout: &Layout, now: SystemTime) -> bool {
    kept_since(layout).is_some_and(|since| now.duration_since(since).is_ok_and(|d| d >= Duration::from_secs(KEEP_DAYS * 24 * 3600)))
}

/// Removes `previous\` (and a leftover `.old`).
pub fn remove_previous(layout: &Layout) -> std::io::Result<()> {
    let _ = std::fs::remove_file(&layout.old);
    match std::fs::remove_dir_all(&layout.previous) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The size of the program's files in KB, as "Installed apps" shows it.
pub fn size_kb(layout: &Layout) -> u32 {
    let bytes = std::fs::metadata(&layout.exe).map(|m| m.len()).unwrap_or_default();
    u32::try_from(bytes.div_ceil(1024)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> (tempfile::TempDir, Layout, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("SSHeesh");
        std::fs::create_dir_all(&dir).unwrap();
        let installer = root.path().join("SSHeesh-1.0.0-installer.exe");
        std::fs::write(&installer, b"1.0.0").unwrap();
        (root, Layout::of(&dir), installer)
    }

    #[test]
    fn an_update_from_0_10_keeps_it_in_previous() {
        let (_root, l, installer) = folder();
        std::fs::write(&l.exe, b"0.10.14").unwrap();
        std::fs::write(l.dir.join(NSIS_UNINSTALLER), b"nsis").unwrap();
        stage(&l, &installer).unwrap();
        assert_eq!(std::fs::read(&l.new).unwrap(), b"1.0.0");
        assert_eq!(keep_previous(&l, Some("0.10.14")).unwrap(), Kept::Previous);
        take_name(&l).unwrap();
        assert_eq!(std::fs::read(&l.exe).unwrap(), b"1.0.0");
        assert_eq!(std::fs::read(l.previous_exe()).unwrap(), b"0.10.14");
        assert_eq!(std::fs::read(l.previous.join(NSIS_UNINSTALLER)).unwrap(), b"nsis");
        // 0.10's own uninstaller stays where "Installed apps" points.
        assert!(l.dir.join(NSIS_UNINSTALLER).exists());
        assert!(!l.new.exists());
        let since = kept_since(&l).unwrap();
        assert!(!previous_due(&l, since + Duration::from_secs(29 * 24 * 3600)));
        assert!(previous_due(&l, since + Duration::from_secs(30 * 24 * 3600)));
        remove_previous(&l).unwrap();
        assert!(!l.previous.exists());
        remove_previous(&l).unwrap();
    }

    #[test]
    fn a_later_update_keeps_the_version_to_go_back_to() {
        let (_root, l, installer) = folder();
        std::fs::create_dir_all(&l.previous).unwrap();
        std::fs::write(l.previous_exe(), b"0.10.14").unwrap();
        std::fs::write(&l.exe, b"1.0.0").unwrap();
        std::fs::write(&installer, b"1.0.1").unwrap();
        stage(&l, &installer).unwrap();
        assert_eq!(keep_previous(&l, Some("1.0.0")).unwrap(), Kept::Old);
        take_name(&l).unwrap();
        assert_eq!(std::fs::read(l.previous_exe()).unwrap(), b"0.10.14");
        assert_eq!(std::fs::read(&l.old).unwrap(), b"1.0.0");
        assert_eq!(std::fs::read(&l.exe).unwrap(), b"1.0.1");
    }

    #[test]
    fn a_fresh_install_has_nothing_to_keep() {
        let (_root, l, installer) = folder();
        stage(&l, &installer).unwrap();
        assert_eq!(keep_previous(&l, None).unwrap(), Kept::Nothing);
        take_name(&l).unwrap();
        assert!(l.exe.exists() && !l.previous.exists());
        assert_eq!(size_kb(&l), 1);
    }

    /// The staged program run as its own installer (1.x updating an installed copy): nothing
    /// to copy.
    #[test]
    fn the_staged_program_needs_no_copy_of_itself() {
        let (_root, l, _) = folder();
        std::fs::write(&l.new, b"1.0.1").unwrap();
        stage(&l, &l.new).unwrap();
        assert_eq!(std::fs::read(&l.new).unwrap(), b"1.0.1");
    }

    /// A power loss after each step: the `RunOnce` entry finishes it, any number of times.
    #[test]
    fn a_replacement_cut_off_anywhere_finishes() {
        // After step 1: both programs there.
        let (_root, l, installer) = folder();
        std::fs::write(&l.exe, b"0.10.14").unwrap();
        stage(&l, &installer).unwrap();
        assert_eq!(finish(&l).unwrap(), Finished::Done);
        assert_eq!(std::fs::read(&l.exe).unwrap(), b"1.0.0");
        assert_eq!(std::fs::read(l.previous_exe()).unwrap(), b"0.10.14");
        assert_eq!(finish(&l).unwrap(), Finished::Nothing);
        // After step 3: no ssheesh.exe at all for a moment.
        let (_root, l, installer) = folder();
        std::fs::write(&l.exe, b"0.10.14").unwrap();
        stage(&l, &installer).unwrap();
        keep_previous(&l, None).unwrap();
        assert!(!l.exe.exists());
        assert_eq!(finish(&l).unwrap(), Finished::Done);
        assert_eq!(std::fs::read(&l.exe).unwrap(), b"1.0.0");
        // Before step 1 finished: only the part, which is never taken.
        let (_root, l, _) = folder();
        std::fs::write(&l.exe, b"0.10.14").unwrap();
        std::fs::write(l.dir.join("ssheesh.exe.new.part"), b"1.0").unwrap();
        assert_eq!(finish(&l).unwrap(), Finished::Nothing);
        assert_eq!(std::fs::read(&l.exe).unwrap(), b"0.10.14");
    }
}
