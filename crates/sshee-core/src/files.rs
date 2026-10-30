//! File browsing on servers (over ssh, using the shared connection) and on
//! this computer.

use crate::error::{ErrorCode, OpError};
use crate::ssh::{remote_command, run_remote, shell_quote, spawn_error, SshEnv, SshTarget, Target};
use serde::Serialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    let script = format!(
        "{}\nfind . -maxdepth 0 -printf '' 2>/dev/null || {{ echo 'Browsing files needs GNU find on the server (most Linux servers have it).' >&2; exit 4; }}\npwd\nLC_ALL=C find . -mindepth 1 -maxdepth 1 -printf '%y\\t%Y\\t%s\\t%T@\\t%M\\t%f\\0' 2>/dev/null\nexit 0",
        cd_script(path)
    );
    let out = run_remote(t, env, &script, None)?;
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
    entries.sort_by(|a, b| {
        let ad = a.kind == FileKind::Dir;
        let bd = b.kind == FileKind::Dir;
        bd.cmp(&ad).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
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
                r#"F={}; [ -f "$F" ] || {{ echo "This isn't a regular file." >&2; exit 4; }}; S=$(wc -c < "$F" | tr -d ' '); echo "$S"; [ "$S" -gt {MAX_TEXT} ] || cat -- "$F""#,
                shell_quote(path)
            );
            let out = run_remote(t, env, &script, None)?;
            let nl = out.iter().position(|&b| b == b'\n').unwrap_or(out.len());
            let size = String::from_utf8_lossy(&out[..nl]).trim().parse().unwrap_or(0);
            (size, out.get(nl + 1..).unwrap_or_default().to_vec())
        }
        None => {
            let p = resolve_local(path);
            let size = std::fs::metadata(&p)?.len();
            let bytes = if size > MAX_TEXT { Vec::new() } else { std::fs::read(&p)? };
            (size, bytes)
        }
    };
    if size > MAX_TEXT {
        return Ok(TextFile { path: path.into(), content: None, binary: false, size });
    }
    let binary = bytes.iter().take(8000).any(|&b| b == 0);
    let content = if binary { None } else { Some(String::from_utf8_lossy(&bytes).into_owned()) };
    Ok(TextFile { path: path.into(), content, binary, size })
}

pub fn write_text(target: &Target, env: &SshEnv, path: &str, content: &str) -> Result<(), OpError> {
    match ssh_only(target) {
        Some(t) => {
            run_remote(t, env, &format!("cat > {}", shell_quote(path)), Some(content.as_bytes().to_vec()))?;
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
    if dest.exists() && !overwrite {
        return Err(OpError::new(ErrorCode::Exists, format!("\"{}\" already exists in this folder.", name.to_string_lossy())));
    }
    fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
        if from.is_dir() {
            std::fs::create_dir_all(to)?;
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                copy_all(&entry.path(), &to.join(entry.file_name()))?;
            }
            Ok(())
        } else {
            std::fs::copy(from, to).map(|_| ())
        }
    }
    Ok(copy_all(from, &dest)?)
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
                r#"cd -- {dir} || exit 1; [ -e {name} ] && {{ echo EXISTS >&2; exit 5; }}; tar -czf {name} -- {items}"#,
                dir = shell_quote(dir),
                name = shell_quote(archive),
            );
            run_remote(t, env, &script, None).map(|_| ()).map_err(|e| if e.message == "EXISTS" { exists() } else { e })
        }
        None => {
            let base = resolve_local(dir);
            if base.join(archive).exists() {
                return Err(exists());
            }
            // tar ships with macOS, Linux and Windows 10 and newer.
            let mut cmd = std::process::Command::new("tar");
            cmd.current_dir(&base).arg("-czf").arg(archive).arg("--").args(names);
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

/// Copies a remote file or folder into `dest_dir` (usually Downloads) and
/// returns where it landed. Never overwrites: picks "name (1)" instead.
pub fn download(target: &Target, env: &SshEnv, remote_path: &str, is_dir: bool, dest_dir: &Path) -> Result<String, OpError> {
    let t = ssh_only(target).ok_or_else(|| OpError::new(ErrorCode::Unsupported, "This file is already on your computer."))?;
    let name = base_name(remote_path);
    std::fs::create_dir_all(dest_dir)?;
    let final_path = unique_path(dest_dir, &name);

    if !is_dir {
        let part = dest_dir.join(format!(".{name}.sshee-part"));
        let mut cmd = remote_command(t, env, &format!("cat -- {}", shell_quote(remote_path)));
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(spawn_error)?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let copied = File::create(&part).and_then(|mut f| std::io::copy(&mut stdout, &mut f));
        let out = child.wait_with_output()?;
        if let Err(e) = copied {
            let _ = std::fs::remove_file(&part);
            return Err(e.into());
        }
        if !out.status.success() {
            let _ = std::fs::remove_file(&part);
            return Err(OpError::from_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr)));
        }
        std::fs::rename(&part, &final_path)?;
        return Ok(final_path.display().to_string());
    }

    // Folders travel as a tar stream: remote tar -> local tar.
    let tmp = dest_dir.join(format!(".sshee-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let script = format!("cd -- {} && tar -cf - -- {}", shell_quote(&parent_dir(remote_path)), shell_quote(&name));
    let mut remote = remote_command(t, env, &script);
    remote.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut remote = remote.spawn().map_err(spawn_error)?;
    let stream = remote.stdout.take().expect("piped stdout");
    let mut untar = Command::new("tar");
    untar.args(["-xf", "-", "-C"]).arg(&tmp).stdin(Stdio::from(stream)).stdout(Stdio::null()).stderr(Stdio::piped());
    crate::ssh::hide_console(&mut untar);
    let local = untar.output();
    let rout = remote.wait_with_output()?;
    let result = (|| {
        let local = local.map_err(|e| OpError::failed(format!("Couldn't run tar on this computer: {e}")))?;
        if !rout.status.success() {
            return Err(OpError::from_ssh(rout.status.code(), &String::from_utf8_lossy(&rout.stderr)));
        }
        if !local.status.success() {
            return Err(OpError::failed(format!("Unpacking failed: {}", String::from_utf8_lossy(&local.stderr).trim())));
        }
        std::fs::rename(tmp.join(&name), &final_path)?;
        Ok(final_path.display().to_string())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Copies a local file or folder into `remote_dir`.
/// Copies a file or folder from this computer into `dir` on the target: over
/// ssh for a server, a plain copy for this computer's own file manager.
pub fn upload(target: &Target, env: &SshEnv, local_path: &Path, remote_dir: &str, overwrite: bool) -> Result<(), OpError> {
    let Some(t) = ssh_only(target) else {
        return copy_here(local_path, &resolve_local(remote_dir), overwrite);
    };
    let name = local_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| OpError::failed("That path has no file name."))?;
    let dest = join_remote(remote_dir, &name);
    let guard = if overwrite {
        String::new()
    } else {
        format!("[ -e {} ] && {{ echo EXISTS >&2; exit 5; }}; ", shell_quote(&dest))
    };
    let exists = |e: OpError| {
        if e.message == "EXISTS" {
            OpError::new(ErrorCode::Exists, format!("\"{name}\" already exists in this folder."))
        } else {
            e
        }
    };

    if local_path.is_file() {
        let mut cmd = remote_command(t, env, &format!("{guard}cat > {}", shell_quote(&dest)));
        cmd.stdin(Stdio::from(File::open(local_path)?)).stdout(Stdio::null()).stderr(Stdio::piped());
        let out = cmd.spawn().map_err(spawn_error)?.wait_with_output()?;
        if !out.status.success() {
            return Err(exists(OpError::from_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr))));
        }
        return Ok(());
    }

    let parent = local_path.parent().unwrap_or(Path::new("."));
    let mut pack = Command::new("tar");
    pack.arg("-C").arg(parent).args(["-cf", "-", "--"]).arg(&name).stdout(Stdio::piped()).stderr(Stdio::piped());
    crate::ssh::hide_console(&mut pack);
    let mut pack = pack.spawn().map_err(|e| OpError::failed(format!("Couldn't run tar on this computer: {e}")))?;
    let stream = pack.stdout.take().expect("piped stdout");
    let mut cmd = remote_command(t, env, &format!("{guard}cd -- {} && tar -xf -", shell_quote(remote_dir)));
    cmd.stdin(Stdio::from(stream)).stdout(Stdio::null()).stderr(Stdio::piped());
    let out = cmd.spawn().map_err(spawn_error)?.wait_with_output()?;
    let _ = pack.wait();
    if !out.status.success() {
        return Err(exists(OpError::from_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr))));
    }
    Ok(())
}

/// Current directory of a pane's terminal (when it runs in tmux on the server).
pub fn session_cwd(target: &Target, env: &SshEnv, session_key: &str) -> Result<String, OpError> {
    let t = ssh_only(target).ok_or_else(|| OpError::new(ErrorCode::Unsupported, "Not a server session."))?;
    let name = crate::session::tmux_session_for(session_key);
    let out = run_remote(t, env, &format!("tmux display-message -p -t {} '#{{pane_current_path}}'", shell_quote(&name)), None)?;
    let path = String::from_utf8_lossy(&out).trim().to_string();
    if path.is_empty() {
        Err(OpError::new(ErrorCode::NotFound, "Couldn't find the terminal's folder."))
    } else {
        Ok(path)
    }
}

// ---------------------------------------------------------------- path helpers

pub fn base_name(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path).to_string()
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

    #[test]
    fn copies_into_a_folder_on_this_computer() {
        let base = std::env::temp_dir().join(format!("sshee-copy-{}", std::process::id()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(src.join("site")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("site/index.html"), "hi").unwrap();
        let env = SshEnv::default();
        let d = dst.display().to_string();
        upload(&Target::Local, &env, &src.join("site"), &d, false).unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("site/index.html")).unwrap(), "hi");
        let again = upload(&Target::Local, &env, &src.join("site"), &d, false);
        assert_eq!(again.unwrap_err().code, ErrorCode::Exists);
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
