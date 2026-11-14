//! The edit folder on disk, touched only in ways that can't be led out of it.
//!
//! Every copy of a server's file lives in `edit/<id>/`. What happens there has
//! to stay there: a link someone left in a session folder must not make
//! SSHeesh read `~/.ssh/id_ed25519` and upload it to the server as your
//! edited file, and a folder swapped for a link mustn't make a clean-up
//! delete something else. So on Unix nothing here goes by a path past the
//! edit folder itself: the folder is opened once (and checked to be the one
//! SSHeesh made, by device and inode), and everything below it goes through
//! that handle with `O_NOFOLLOW` (`openat`, `unlinkat`, `renameat`). A link is
//! never followed, only removed as itself.
//!
//! Windows has no `openat` in the standard library, so there the same checks
//! are made by path, each right before its step: a link or junction is
//! refused, a file is opened with `FILE_FLAG_OPEN_REPARSE_POINT` so a link
//! put there between the check and the open is opened as itself, and
//! deleting uses the standard library's `remove_dir_all`, which doesn't
//! follow links. The folder is inside your user profile, which only you,
//! SYSTEM and administrators can write to.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;

/// What a name in a folder is, as itself (a link is a link, not its target).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Link,
    Other,
}

#[derive(Debug, Clone, Copy)]
pub struct Meta {
    pub kind: Kind,
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
    /// Hard links to it. A copy SSHeesh made has one; more means the name
    /// also reaches some other file (Unix only; always 1 on Windows).
    pub links: u64,
}

/// One plain name: no separators, not "." or "..", not empty. Anything a
/// folder hands back that isn't one is skipped, never used as a path.
pub fn is_plain_name(name: &OsStr) -> bool {
    let s = name.to_string_lossy();
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', '\0'])
}

fn bad_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a plain file name")
}

#[cfg(unix)]
pub use unix::Dir;
#[cfg(windows)]
pub use windows::Dir;

/// Free space on the disk that `path` is on, for this user, in bytes.
#[cfg(unix)]
pub fn free_space(path: &std::path::Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| bad_name())?;
    // SAFETY: `c` is a valid C string and `stats` a plain struct statvfs fills in.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut stats) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[allow(clippy::unnecessary_cast)] // the widths differ between Linux and macOS
    Ok((stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
}

/// Free space on the disk that `path` is on, for this user, in bytes.
#[cfg(windows)]
pub fn free_space(path: &std::path::Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use ::windows::core::PCWSTR;
    use ::windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }.map_err(|e| io::Error::other(e.to_string()))?;
    Ok(available)
}

#[cfg(unix)]
mod unix {
    use super::{bad_name, is_plain_name, Kind, Meta};
    use std::ffi::{CStr, CString, OsStr, OsString};
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;

    /// How deep a clean-up goes. A session folder holds one file and
    /// SSHeesh's two; an editor may leave a backup folder. Anything deeper is
    /// left alone.
    const MAX_DEPTH: u32 = 4;

    /// An open folder. Everything below it is reached through this handle,
    /// never by a path.
    pub struct Dir {
        fd: OwnedFd,
        id: (u64, u64),
    }

    const DIR_FLAGS: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

    fn c_name(name: &OsStr) -> io::Result<CString> {
        if !is_plain_name(name) {
            return Err(bad_name());
        }
        CString::new(name.as_bytes()).map_err(|_| bad_name())
    }

    fn check(ret: libc::c_int) -> io::Result<libc::c_int> {
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(ret)
        }
    }

    fn fstat(fd: libc::c_int) -> io::Result<libc::stat> {
        // SAFETY: `st` is a plain struct fstat fills in; `fd` is open.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        check(unsafe { libc::fstat(fd, &mut st) })?;
        Ok(st)
    }

    #[allow(clippy::unnecessary_cast)] // the widths differ between Linux and macOS
    fn meta_of(st: &libc::stat) -> Meta {
        let kind = match st.st_mode & libc::S_IFMT {
            libc::S_IFREG => Kind::File,
            libc::S_IFDIR => Kind::Dir,
            libc::S_IFLNK => Kind::Link,
            _ => Kind::Other,
        };
        Meta { kind, size: st.st_size as u64, modified: st.st_mtime as i64, links: st.st_nlink as u64 }
    }

    #[allow(clippy::unnecessary_cast)]
    fn id_of(st: &libc::stat) -> (u64, u64) {
        (st.st_dev as u64, st.st_ino as u64)
    }

    impl Dir {
        /// Opens the folder at `path`, which must be a real folder (not a
        /// link) that belongs to this user and that nobody else can get into.
        pub fn open(path: &Path) -> io::Result<Dir> {
            let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| bad_name())?;
            // SAFETY: `c` is a valid C string; the fd is owned right after.
            let fd = unsafe { OwnedFd::from_raw_fd(check(libc::open(c.as_ptr(), DIR_FLAGS))?) };
            let st = fstat(fd.as_raw_fd())?;
            // SAFETY: geteuid can't fail.
            if st.st_uid != unsafe { libc::geteuid() } {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the folder belongs to another user"));
            }
            if st.st_mode & 0o077 != 0 {
                // Ours, so ours to fix: an old umask or a copy made by hand.
                // SAFETY: `fd` is open.
                check(unsafe { libc::fchmod(fd.as_raw_fd(), 0o700) })?;
            }
            Ok(Dir { id: id_of(&st), fd })
        }

        /// Which folder this is (device, inode): the same folder opened
        /// again has the same, a folder swapped in under the name doesn't.
        pub fn id(&self) -> (u64, u64) {
            self.id
        }

        /// The folder `name` inside this one, never through a link.
        pub fn child(&self, name: &OsStr) -> io::Result<Dir> {
            let c = c_name(name)?;
            // SAFETY: valid fd and C string; the fd is owned right after.
            let fd = unsafe { OwnedFd::from_raw_fd(check(libc::openat(self.fd.as_raw_fd(), c.as_ptr(), DIR_FLAGS))?) };
            let st = fstat(fd.as_raw_fd())?;
            Ok(Dir { id: id_of(&st), fd })
        }

        /// Makes the folder `name`, which nobody but you can get into. Fails
        /// if anything already has the name.
        pub fn make_dir(&self, name: &OsStr) -> io::Result<()> {
            let c = c_name(name)?;
            // SAFETY: valid fd and C string.
            check(unsafe { libc::mkdirat(self.fd.as_raw_fd(), c.as_ptr(), 0o700) })?;
            // The umask may have taken bits away, never added: still check.
            let made = self.child(name)?;
            // SAFETY: `made.fd` is open.
            check(unsafe { libc::fchmod(made.fd.as_raw_fd(), 0o700) })?;
            Ok(())
        }

        fn open_at(&self, name: &OsStr, flags: libc::c_int) -> io::Result<File> {
            let c = c_name(name)?;
            let flags = flags | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            // SAFETY: valid fd and C string; mode is read only with O_CREAT.
            let fd = check(unsafe { libc::openat(self.fd.as_raw_fd(), c.as_ptr(), flags, 0o600 as libc::c_uint) })?;
            // SAFETY: `fd` was just opened and is owned by the File.
            Ok(unsafe { File::from_raw_fd(fd) })
        }

        /// A new file only you can read, where nothing had the name.
        pub fn create(&self, name: &OsStr) -> io::Result<File> {
            self.open_at(name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)
        }

        /// The session's lock file, made if it isn't there.
        pub fn open_lock(&self, name: &OsStr) -> io::Result<File> {
            let file = self.open_at(name, libc::O_RDWR | libc::O_CREAT)?;
            match meta_of(&fstat(file.as_raw_fd())?).kind {
                Kind::File => Ok(file),
                _ => Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular file")),
            }
        }

        /// Opens `name` to read, only if it's a regular file. A link isn't
        /// followed (the open fails), and a pipe put there can't hold the
        /// read up: it's opened without waiting and then refused.
        pub fn open_read(&self, name: &OsStr) -> io::Result<(File, Meta)> {
            let file = self.open_at(name, libc::O_RDONLY | libc::O_NONBLOCK)?;
            let meta = meta_of(&fstat(file.as_raw_fd())?);
            if meta.kind != Kind::File {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular file"));
            }
            // Back to blocking reads now that it's known to be a file.
            // SAFETY: valid fd.
            unsafe {
                let flags = libc::fcntl(file.as_raw_fd(), libc::F_GETFL);
                libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK);
            }
            Ok((file, meta))
        }

        /// What `name` is, as itself; None when nothing has the name.
        pub fn stat(&self, name: &OsStr) -> io::Result<Option<Meta>> {
            let c = c_name(name)?;
            // SAFETY: valid fd and C string; `st` is filled in.
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            match check(unsafe { libc::fstatat(self.fd.as_raw_fd(), c.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) }) {
                Ok(_) => Ok(Some(meta_of(&st))),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        /// Renames `from` to `to`, both in this folder.
        pub fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            let (a, b) = (c_name(from)?, c_name(to)?);
            let fd = self.fd.as_raw_fd();
            // SAFETY: valid fd and C strings.
            check(unsafe { libc::renameat(fd, a.as_ptr(), fd, b.as_ptr()) }).map(drop)
        }

        /// Removes the name `name` (a link goes as a link); a name that's
        /// already gone is fine.
        pub fn unlink(&self, name: &OsStr) -> io::Result<()> {
            let c = c_name(name)?;
            // SAFETY: valid fd and C string.
            match check(unsafe { libc::unlinkat(self.fd.as_raw_fd(), c.as_ptr(), 0) }) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        }

        /// The names in this folder.
        pub fn entries(&self) -> io::Result<Vec<OsString>> {
            // fdopendir takes the fd over, so it gets a copy of its own.
            // SAFETY: valid fd; the copy is closed by closedir (or by hand
            // when fdopendir fails).
            let copy = check(unsafe { libc::fcntl(self.fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) })?;
            let dir = unsafe { libc::fdopendir(copy) };
            if dir.is_null() {
                let e = io::Error::last_os_error();
                unsafe { libc::close(copy) };
                return Err(e);
            }
            let mut names = Vec::new();
            // SAFETY: `dir` is a valid DIR*; each entry's name is a C string
            // valid until the next readdir. A copy shares the read position
            // with every other copy, so start from the top.
            unsafe {
                libc::rewinddir(dir);
                loop {
                    let entry = libc::readdir(dir);
                    if entry.is_null() {
                        break;
                    }
                    let name = CStr::from_ptr((*entry).d_name.as_ptr()).to_bytes().to_vec();
                    let name = OsString::from_vec(name);
                    if is_plain_name(&name) {
                        names.push(name);
                    }
                }
                libc::closedir(dir);
            }
            Ok(names)
        }

        /// Removes `name` and, if it's a folder, everything in it, through
        /// handles all the way: links inside are removed as links, nothing
        /// they point at is touched. A name already gone is fine.
        pub fn remove_tree(&self, name: &OsStr) -> io::Result<()> {
            self.remove_tree_at(name, 0)
        }

        fn remove_tree_at(&self, name: &OsStr, depth: u32) -> io::Result<()> {
            match self.stat(name)? {
                None => Ok(()),
                Some(Meta { kind: Kind::Dir, .. }) => {
                    if depth >= MAX_DEPTH {
                        return Err(io::Error::other("folder nested too deep to remove"));
                    }
                    let inner = self.child(name)?;
                    for entry in inner.entries()? {
                        inner.remove_tree_at(&entry, depth + 1)?;
                    }
                    let c = c_name(name)?;
                    // SAFETY: valid fd and C string.
                    match check(unsafe { libc::unlinkat(self.fd.as_raw_fd(), c.as_ptr(), libc::AT_REMOVEDIR) }) {
                        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                        _ => Ok(()),
                    }
                }
                Some(_) => self.unlink(name),
            }
        }
    }

    impl std::fmt::Debug for Dir {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Dir").field("id", &self.id).finish()
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::{bad_name, is_plain_name, Kind, Meta};
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};

    /// Opens a link as itself instead of what it points at.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    /// A folder, checked to be a real one (not a link or junction) each time
    /// it's used. See the module's notes on why Windows goes by path.
    pub struct Dir {
        path: PathBuf,
    }

    fn real_dir(path: &Path) -> io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a real folder"));
        }
        Ok(())
    }

    fn meta_of(meta: &std::fs::Metadata) -> Meta {
        let t = meta.file_type();
        let kind = if t.is_symlink() {
            Kind::Link
        } else if t.is_dir() {
            Kind::Dir
        } else if t.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        let modified = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
        Meta { kind, size: meta.len(), modified, links: 1 }
    }

    impl Dir {
        pub fn open(path: &Path) -> io::Result<Dir> {
            real_dir(path)?;
            Ok(Dir { path: path.to_path_buf() })
        }

        pub fn id(&self) -> (u64, u64) {
            (0, 0)
        }

        fn at(&self, name: &OsStr) -> io::Result<PathBuf> {
            if !is_plain_name(name) {
                return Err(bad_name());
            }
            real_dir(&self.path)?;
            Ok(self.path.join(name))
        }

        pub fn child(&self, name: &OsStr) -> io::Result<Dir> {
            let path = self.at(name)?;
            real_dir(&path)?;
            Ok(Dir { path })
        }

        pub fn make_dir(&self, name: &OsStr) -> io::Result<()> {
            std::fs::create_dir(self.at(name)?)
        }

        pub fn create(&self, name: &OsStr) -> io::Result<File> {
            OpenOptions::new().write(true).create_new(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(self.at(name)?)
        }

        pub fn open_lock(&self, name: &OsStr) -> io::Result<File> {
            let path = self.at(name)?;
            if let Some(Meta { kind: Kind::Link | Kind::Dir | Kind::Other, .. }) = self.stat(name)? {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular file"));
            }
            OpenOptions::new().read(true).write(true).create(true).truncate(false).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)
        }

        pub fn open_read(&self, name: &OsStr) -> io::Result<(File, Meta)> {
            let path = self.at(name)?;
            let file = OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
            let meta = meta_of(&file.metadata()?);
            if meta.kind != Kind::File {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular file"));
            }
            Ok((file, meta))
        }

        pub fn stat(&self, name: &OsStr) -> io::Result<Option<Meta>> {
            match std::fs::symlink_metadata(self.at(name)?) {
                Ok(meta) => Ok(Some(meta_of(&meta))),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        pub fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            std::fs::rename(self.at(from)?, self.at(to)?)
        }

        pub fn unlink(&self, name: &OsStr) -> io::Result<()> {
            let path = self.at(name)?;
            let result = match self.stat(name)? {
                None => return Ok(()),
                // A link to a folder is removed as a folder name on Windows.
                Some(Meta { kind: Kind::Link, .. }) => std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir(&path)),
                Some(_) => std::fs::remove_file(&path),
            };
            match result {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        }

        pub fn entries(&self) -> io::Result<Vec<OsString>> {
            real_dir(&self.path)?;
            Ok(std::fs::read_dir(&self.path)?.filter_map(|e| e.ok()).map(|e| e.file_name()).filter(|n| is_plain_name(n)).collect())
        }

        pub fn remove_tree(&self, name: &OsStr) -> io::Result<()> {
            match self.stat(name)? {
                None => Ok(()),
                // remove_dir_all doesn't follow links or junctions inside.
                Some(Meta { kind: Kind::Dir, .. }) => match std::fs::remove_dir_all(self.at(name)?) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                    _ => Ok(()),
                },
                Some(_) => self.unlink(name),
            }
        }
    }

    impl std::fmt::Debug for Dir {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Dir").field("path", &self.path).finish()
        }
    }
}

/// Reads all of `file`, refusing more than `limit` bytes.
pub fn read_capped(mut file: File, limit: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    (&mut file).take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::FileTooLarge, "file too large"));
    }
    Ok(bytes)
}

/// A name as an OsStr, for the calls above.
pub fn os(name: &str) -> &OsStr {
    OsStr::new(name)
}

/// The names of a folder's entries as strings, for callers that only
/// care about names SSHeesh made (all ASCII).
pub fn names(entries: Vec<OsString>) -> Vec<String> {
    entries.into_iter().filter_map(|n| n.into_string().ok()).collect()
}
