//! Copying a store into a private folder before anything opens it.
//!
//! The old app may still be running while 1.0 reads its store (on Windows the updater starts
//! 1.0's installer and 0.10 closes a moment later), and its web view keeps writing. So a store
//! is never opened where it lies: its files are copied, and the copy is what the database
//! libraries see. Their recovery work (replaying a log, checkpointing a WAL) then happens on
//! the copy, and the originals are only ever read, never written, locked or renamed.
//!
//! A web view writes several files at once (a LevelDB log and its manifest, a SQLite database
//! and its WAL), and a copy taken mid-write can mix two moments. Each copy therefore notes the
//! files' sizes and times before and after, and tries again, a few times, if anything moved.
//! A copy that never held still is still used (the readers cope with a torn tail), and says so
//! in [`Copied::settled`].

use super::LegacyError;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

/// The most one store's files may add up to. A 0.10 profile's localStorage is a few hundred
/// kilobytes; Chromium allows one origin 10 MB, WebKit 5 MB. Anything bigger is not 0.10's.
pub(crate) const MAX_STORE_BYTES: u64 = 256 << 20;

/// How many times a copy is tried while the originals keep changing.
const ATTEMPTS: usize = 4;
/// The wait between tries: a little longer than 0.10's 300 ms save delay would be ideal, but
/// the web view's own write follows at its own pace, so a short pause and a retry is enough.
const PAUSE: Duration = Duration::from_millis(150);

/// What every scratch folder's name starts with, so [`sweep`] knows its own.
const SCRATCH_PREFIX: &str = "ssheesh-0.10-";

/// How long a scratch folder may sit unchanged before [`sweep`] takes it for a leftover. A
/// reading takes a second or two; this leaves a second 1.0 that is reading at the same moment
/// alone.
const STALE_AFTER: Duration = Duration::from_secs(10 * 60);

/// A private temporary folder for copies, deleted with everything in it when this is dropped,
/// so the copies, which can hold secrets typed into snippets, don't outlive the reading.
///
/// It is made inside a folder of 1.0's own when there is one ([`Places::scratch`](
/// super::Places::scratch)): in the user's own data folder, which on Windows only they (and the
/// system's administrators) can open, as every folder below `%LOCALAPPDATA%` inherits. Otherwise,
/// or if that folder can't be made, it is made in the system's temporary folder, which on
/// Windows is only as private as `%TEMP%` happens to be. On macOS and Linux it is created 0700
/// either way. A reading that never got to drop it (the process was killed, say) leaves it
/// behind, and [`sweep`] clears it away the next time.
pub(crate) struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    pub(crate) fn new_in(parent: Option<&Path>) -> Result<Scratch, LegacyError> {
        if let Some(parent) = parent {
            if let Ok(dir) = make_private_dir(parent).and_then(|()| builder().tempdir_in(parent)) {
                return Ok(Scratch { dir });
            }
        }
        let dir =
            builder().tempdir().map_err(|error| LegacyError::Io { path: std::env::temp_dir(), error })?;
        Ok(Scratch { dir })
    }

    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

fn builder() -> tempfile::Builder<'static, 'static> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(SCRATCH_PREFIX);
    // Set as the folder is made, so there is no moment when others could open it.
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    builder
}

/// Makes `dir` and any folder missing above it, owner-only on macOS and Linux.
fn make_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    // One that was already there (made by hand, say) is made owner-only too.
    #[cfg(unix)]
    fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

/// Deletes the scratch folders an earlier reading left behind, in `parent` (see
/// [`Scratch::new_in`]) and in the system's temporary folder: folders named like ours that
/// haven't changed for [`STALE_AFTER`]. Anything it can't delete (another user's, in a shared
/// `/tmp`) is left as it is.
pub(crate) fn sweep(parent: Option<&Path>) {
    let temp = std::env::temp_dir();
    for dir in parent.into_iter().chain([temp.as_path()]) {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with(SCRATCH_PREFIX) {
                continue;
            }
            // Neither `file_type` nor `metadata` of an entry follows a link: a link is left alone.
            let Ok(meta) = entry.metadata() else { continue };
            let age = meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok());
            if meta.is_dir() && age.is_some_and(|age| age > STALE_AFTER) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// What a copy brought over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Copied {
    /// When the originals last changed: the newest of their modification times.
    pub modified: Option<SystemTime>,
    /// Whether the originals held still while the last copy was made.
    pub settled: bool,
}

/// Copies the files `list` names (regular files, all in one folder) into `dest`, under the same
/// names, retrying while they change. `list` is asked again on every try, because LevelDB
/// replaces whole files as it works. `origin` is the store's path, for errors.
pub(crate) fn copy_settled(
    origin: &Path,
    list: &dyn Fn() -> io::Result<Vec<PathBuf>>,
    dest: &Path,
) -> Result<Copied, LegacyError> {
    let io_err = |error: io::Error| LegacyError::Io { path: origin.to_path_buf(), error };
    let mut last: Option<Result<Copied, LegacyError>> = None;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            thread::sleep(PAUSE);
        }
        // A fresh folder for each try, so nothing from a torn try is left in it.
        if dest.exists() {
            fs::remove_dir_all(dest).map_err(io_err)?;
        }
        fs::create_dir_all(dest).map_err(io_err)?;

        let files = list().map_err(io_err)?;
        let before = stat_all(&files);
        let bytes: u64 = before.iter().flatten().map(|s| s.0).sum();
        if bytes > MAX_STORE_BYTES {
            return Err(LegacyError::TooBig { path: origin.to_path_buf(), bytes });
        }

        let mut moved = before.iter().any(Option::is_none);
        let mut failure = None;
        for file in &files {
            let Some(name) = file.file_name() else { continue };
            match copy_file(file, &dest.join(name)) {
                Ok(()) => {}
                // Replaced or removed while copying: the store moved on, try again.
                Err(e) if e.kind() == io::ErrorKind::NotFound => moved = true,
                Err(error) => {
                    failure = Some(LegacyError::Io { path: file.clone(), error });
                    break;
                }
            }
        }
        if let Some(error) = failure {
            // Locked (Windows' sharing rules) or unreadable: maybe only for a moment.
            last = Some(Err(error));
            continue;
        }

        let after = stat_all(&files);
        let modified = after.iter().flatten().filter_map(|s| s.1).max();
        if !moved && before == after {
            return Ok(Copied { modified, settled: true });
        }
        last = Some(Ok(Copied { modified, settled: false }));
    }
    last.unwrap_or(Ok(Copied { modified: None, settled: false }))
}

/// Size and modification time of each file, `None` for one that's gone.
fn stat_all(files: &[PathBuf]) -> Vec<Option<(u64, Option<SystemTime>)>> {
    files.iter().map(|f| fs::metadata(f).ok().map(|m| (m.len(), m.modified().ok()))).collect()
}

/// Reads `from` and writes its bytes to `to`. `File::open` asks for read access only, and
/// Rust opens files on Windows sharing read, write and delete, so this works while the web
/// view has the file open for writing (where `CopyFile` might refuse).
fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    let mut src = open_file(from, false)?;
    let mut dst = File::create(to)?;
    // Never more than the store limit, even if the file grows while being copied.
    io::copy(&mut (&mut src).take(MAX_STORE_BYTES), &mut dst)?;
    dst.sync_all()
}

/// Opens a regular file to read, and only that. On macOS and Linux a FIFO put where a file
/// was expected would hold `open` until something writes to it, which may be never, and a
/// device such as `/dev/zero` would read without end: the file is opened without waiting, and
/// what was opened must be a regular file. Checking what was opened, not the name beforehand,
/// means swapping the file in between doesn't get past it. Unless `follow`, a symbolic link
/// (on Windows, any reparse point) in the last place is refused rather than followed.
pub(crate) fn open_file(path: &Path, follow: bool) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(if follow { libc::O_NONBLOCK } else { libc::O_NONBLOCK | libc::O_NOFOLLOW });
    }
    #[cfg(windows)]
    if !follow {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open the link itself, which then isn't a regular file.
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
    }
    Ok(file)
}

/// The regular files directly in `dir` that `keep` accepts by name. Symbolic links and
/// folders are left out.
pub(crate) fn files_in(dir: &Path, keep: &dyn Fn(&str) -> bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        if keep(&entry.file_name().to_string_lossy()) {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

/// Copies a SQLite database and its write-ahead log (see [`sqlite_files`]) into `dest`. A
/// rollback journal beside it isn't copied, but one that holds a write still under way (only
/// possible outside WAL mode) means the copy may be half of that write, so it counts as not
/// settled.
pub(crate) fn copy_sqlite(db: &Path, dest: &Path) -> Result<Copied, LegacyError> {
    let mut copied = copy_settled(db, &|| sqlite_files(db), dest)?;
    if hot_journal(db) {
        copied.settled = false;
    }
    Ok(copied)
}

/// A SQLite database and the file that finishes it: the write-ahead log (`-wal`), which holds
/// recent writes until they are checkpointed. WebKit and Chromium both run localStorage in WAL
/// mode, and a running app's newest records are often only there.
///
/// Left out: the shared-memory index (`-shm`), which SQLite rebuilds from the WAL, and the
/// rollback journal (`-journal`), which WAL mode doesn't use. A journal is not copied even if
/// one is there, because SQLite acts on it: rolling it back on first read, it opens a
/// "super-journal" whose path the journal names, anywhere on the disk, and may delete it. A
/// journal dropped beside a store could then delete any file of the user's.
pub(crate) fn sqlite_files(db: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = vec![db.to_path_buf()];
    if !fs::symlink_metadata(db)?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
    }
    let wal = beside(db, "-wal");
    if fs::symlink_metadata(&wal).map(|m| m.is_file()).unwrap_or(false) {
        files.push(wal);
    }
    Ok(files)
}

/// Whether `db` has a rollback journal beside it that SQLite would roll back: one that isn't
/// empty and whose header hasn't been zeroed (`hasHotJournal` in SQLite's `pager.c`).
fn hot_journal(db: &Path) -> bool {
    let Ok(mut journal) = open_file(&beside(db, "-journal"), false) else { return false };
    let mut first = [0u8];
    matches!(journal.read(&mut first), Ok(1) if first[0] != 0)
}

/// `db`'s path with `suffix` added to its name.
fn beside(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_private_and_goes_away() {
        let scratch = Scratch::new_in(None).unwrap();
        let path = scratch.path().to_path_buf();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "only the owner may open the copies");
        }
        drop(scratch);
        assert!(!path.exists(), "the copies are deleted after reading");
    }

    #[test]
    fn copies_keep_names_and_bytes_and_leave_the_originals_alone() {
        let src = tempfile::tempdir().unwrap();
        let db = src.path().join("tauri_localhost_0.localstorage");
        fs::write(&db, b"db bytes").unwrap();
        fs::write(src.path().join("tauri_localhost_0.localstorage-wal"), b"wal bytes").unwrap();
        fs::write(src.path().join("tauri_localhost_0.localstorage-shm"), b"shm").unwrap();
        let scratch = Scratch::new_in(None).unwrap();
        let dest = scratch.path().join("copy");
        let copied = copy_settled(&db, &|| sqlite_files(&db), &dest).unwrap();
        assert!(copied.settled);
        assert!(copied.modified.is_some());
        assert_eq!(fs::read(dest.join("tauri_localhost_0.localstorage")).unwrap(), b"db bytes");
        assert_eq!(fs::read(dest.join("tauri_localhost_0.localstorage-wal")).unwrap(), b"wal bytes");
        assert!(!dest.join("tauri_localhost_0.localstorage-shm").exists());
        assert_eq!(fs::read(&db).unwrap(), b"db bytes");
    }

    #[test]
    fn a_rollback_journal_is_never_copied_and_a_hot_one_unsettles_the_copy() {
        let src = tempfile::tempdir().unwrap();
        let db = src.path().join("LocalStorage");
        fs::write(&db, b"db bytes").unwrap();
        let journal = src.path().join("LocalStorage-journal");
        let scratch = Scratch::new_in(None).unwrap();
        let dest = scratch.path().join("copy");
        // Zeroed header: what SQLite leaves after a commit in PERSIST mode. Not hot.
        fs::write(&journal, [0u8; 512]).unwrap();
        assert!(copy_sqlite(&db, &dest).unwrap().settled);
        assert!(!dest.join("LocalStorage-journal").exists());
        // A header that starts with the journal's magic: a write SQLite would roll back.
        fs::write(&journal, [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]).unwrap();
        assert!(!copy_sqlite(&db, &dest).unwrap().settled);
        assert!(!dest.join("LocalStorage-journal").exists());
        assert_eq!(sqlite_files(&db).unwrap(), std::slice::from_ref(&db));
    }

    #[test]
    fn a_scratch_folder_is_made_where_asked_or_falls_back_to_temp() {
        let base = tempfile::tempdir().unwrap();
        let parent = base.path().join("app").join("import-scratch");
        let scratch = Scratch::new_in(Some(&parent)).unwrap();
        assert_eq!(scratch.path().parent(), Some(parent.as_path()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&parent).unwrap().permissions().mode() & 0o777, 0o700);
        }
        drop(scratch);
        // A file where the folder should be: the copies go to the system's temporary folder.
        let blocked = base.path().join("blocked");
        fs::write(&blocked, b"").unwrap();
        let scratch = Scratch::new_in(Some(&blocked)).unwrap();
        assert!(scratch.path().starts_with(std::env::temp_dir()));
    }

    #[cfg(unix)]
    #[test]
    fn only_regular_files_are_opened() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("CURRENT");
        fs::write(&file, b"MANIFEST-000001\n").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let fifo = dir.path().join("fifo");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());

        assert!(open_file(&file, false).is_ok());
        assert!(open_file(&link, false).is_err(), "a link isn't followed");
        assert!(open_file(&link, true).is_ok(), "unless asked");
        // Returns at once: no writer is waited for.
        assert!(open_file(&fifo, true).is_err());
        assert!(open_file(dir.path(), true).is_err());
        assert!(open_file(Path::new("/dev/zero"), true).is_err());
    }

    #[test]
    fn a_folder_only_gives_up_the_files_asked_for() {
        let src = tempfile::tempdir().unwrap();
        for name in ["CURRENT", "LOCK", "LOG", "000004.log"] {
            fs::write(src.path().join(name), name).unwrap();
        }
        fs::create_dir(src.path().join("lost")).unwrap();
        let files = files_in(src.path(), &|n| n != "LOCK" && n != "LOG").unwrap();
        let names: Vec<_> =
            files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["000004.log", "CURRENT"]);
    }
}
