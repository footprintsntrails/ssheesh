//! Building `ssh` invocations and running one-off remote commands.

use crate::error::OpError;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Where a session or file operation runs.
#[derive(Debug, Clone)]
pub enum Target {
    Local,
    Ssh(SshTarget),
}

#[derive(Debug, Clone, Default)]
pub struct SshTarget {
    /// Host alias from ~/.ssh/config, or a plain hostname/IP.
    pub destination: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    /// Extra raw ssh arguments (used by tests to point at a custom config).
    pub extra_args: Vec<String>,
}

/// Shared settings for every ssh process the app starts.
#[derive(Debug, Clone, Default)]
pub struct SshEnv {
    /// Directory for connection-sharing sockets. With it, the file browser
    /// and metrics reuse the terminal's login instead of signing in again.
    /// Not supported by Windows OpenSSH, so it's ignored there.
    pub control_dir: Option<PathBuf>,
}

impl SshEnv {
    pub fn with_control_dir(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        Self { control_dir: Some(dir) }
    }
}

pub fn ssh_program() -> &'static str {
    "ssh"
}

/// Options shared by interactive sessions and one-off commands.
/// Everything before the destination.
pub fn common_args(t: &SshTarget, env: &SshEnv) -> Vec<String> {
    let mut a: Vec<String> = t.extra_args.clone();
    let mut opt = |v: &str| {
        a.push("-o".into());
        a.push(v.into());
    };
    // Notice a dead connection within ~15s instead of hanging forever.
    opt("ServerAliveInterval=5");
    opt("ServerAliveCountMax=3");
    opt("ConnectTimeout=10");
    if cfg!(unix) {
        if let Some(dir) = &env.control_dir {
            opt("ControlMaster=auto");
            opt(&format!("ControlPath={}/%C", dir.display()));
            opt("ControlPersist=300");
        }
    }
    if let Some(u) = &t.user {
        a.push("-l".into());
        a.push(u.clone());
    }
    if let Some(p) = t.port {
        a.push("-p".into());
        a.push(p.to_string());
    }
    if let Some(i) = &t.identity_file {
        a.push("-i".into());
        a.push(expand_tilde(i));
    }
    a
}

pub fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    p.to_string()
}

/// POSIX single-quote escaping. Also safe for fish, which the remote login
/// shell might be.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// tmux session name for a pane's durable session key.
pub fn session_name(key: &str) -> String {
    let clean: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(48)
        .collect();
    format!("sshee-{clean}")
}

/// The remote command for an interactive session. If tmux exists on the
/// server, the shell lives inside a named tmux session that outlives the
/// connection, and reconnecting attaches to it again. tmux's status bar is
/// hidden so it looks like a plain terminal. Without tmux it's a normal shell.
pub fn durable_command(key: &str, run: Option<&[String]>) -> String {
    let name = session_name(key);
    // With a command: tmux runs it as the session's only window; without
    // tmux it's exec'd directly. Without one: the login shell.
    let (in_tmux, plain) = match run {
        Some(argv) => {
            let cmd = argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
            (format!(" {}", shell_quote(&cmd)), cmd)
        }
        None => (String::new(), r#""${SHELL:-/bin/sh}" -l"#.to_string()),
    };
    let script = format!(
        r#"if command -v tmux >/dev/null 2>&1; then printf "\033]7777;sshee=durable\007"; exec tmux new-session -A -s {name}{in_tmux} \; set-option status off \; set-option mouse on; else printf "\033]7777;sshee=plain\007"; exec {plain}; fi"#
    );
    format!("sh -c {}", shell_quote(&script))
}

/// Builds a non-interactive ssh command that runs `script` with `sh`.
pub fn remote_command(t: &SshTarget, env: &SshEnv, script: &str) -> Command {
    let mut cmd = Command::new(ssh_program());
    cmd.args(common_args(t, env))
        .args(["-o", "BatchMode=yes", "-T", "--"])
        .arg(&t.destination)
        .arg(format!("sh -c {}", shell_quote(script)));
    hide_console(&mut cmd);
    cmd
}

/// Runs a script on the server and returns its stdout.
pub fn run_remote(t: &SshTarget, env: &SshEnv, script: &str, stdin: Option<Vec<u8>>) -> Result<Vec<u8>, OpError> {
    let mut cmd = remote_command(t, env, script);
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(spawn_error)?;
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        });
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(OpError::from_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr)))
    }
}

/// Runs a script on this computer with `sh` (used for "This computer").
pub fn run_local(script: &str) -> Result<Vec<u8>, OpError> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script).stdin(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| OpError::failed(format!("Couldn't run sh: {e}")))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(OpError::from_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr)))
    }
}

pub fn spawn_error(e: std::io::Error) -> OpError {
    if e.kind() == std::io::ErrorKind::NotFound {
        OpError::failed("The ssh program isn't installed on this computer.")
    } else {
        OpError::failed(format!("Couldn't start ssh: {e}"))
    }
}

pub fn hide_console(_cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        _cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Default directory for connection-sharing sockets. Kept short on purpose:
/// unix socket paths are limited to ~104 bytes.
pub fn default_control_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".sshee").join("cm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn session_names_are_sanitized() {
        assert_eq!(session_name("ab c;rm -rf/"), "sshee-abcrm-rf");
    }

    #[test]
    fn durable_command_runs_through_sh() {
        let c = durable_command("k1", None);
        assert!(c.starts_with("sh -c '"));
        assert!(c.contains("tmux new-session -A -s sshee-k1"));
    }
}
