//! Building `ssh` invocations and running one-off remote commands.

use crate::secrets::{askpass_env, Secret};
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
    /// Handed to ssh through askpass when the server asks for a password.
    pub password: Option<Secret>,
    /// A password manager's key agent (1Password, Bitwarden) to sign in with.
    pub identity_agent: Option<String>,
}

/// Shared settings for every ssh process the app starts.
#[derive(Debug, Clone, Default)]
pub struct SshEnv {
    /// Directory for connection-sharing sockets. With it, the file browser
    /// and metrics reuse the terminal's login instead of signing in again.
    /// Not supported by Windows OpenSSH, so it's ignored there.
    pub control_dir: Option<PathBuf>,
    /// The program ssh runs to ask for a password: the app itself.
    pub askpass: Option<PathBuf>,
}

impl SshEnv {
    pub fn with_control_dir(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        Self { control_dir: Some(dir), askpass: None }
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
    if t.password.is_some() {
        // A wrong saved password should fail once, not three times.
        opt("NumberOfPasswordPrompts=1");
    }
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
    if let Some(agent) = &t.identity_agent {
        a.push("-o".into());
        a.push(format!("IdentityAgent={}", expand_tilde(agent)));
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

/// Prints the server's welcome message (motd), which ssh leaves out when it
/// runs a command instead of a plain login. Skipped with ~/.hushlogin, as a
/// login would. Each file once, even when one links to another.
const MOTD: &str = r#"if [ ! -e "$HOME/.hushlogin" ]; then seen=; for f in /run/motd.dynamic /etc/motd /run/motd.d/* /etc/motd.d/* /usr/lib/motd.d/*; do [ -f "$f" ] && [ -r "$f" ] || continue; r=$(readlink -f "$f" 2>/dev/null || echo "$f"); case " $seen " in *" $r "*) continue ;; esac; seen="$seen $r"; cat "$f"; done 2>/dev/null; fi"#;

/// tmux reports the scroll position through the window title, which the
/// app reads to draw a scrollbar: in copy mode or not, lines scrolled up,
/// lines of history, pane height and the history limit.
pub const SCROLL_TITLE: &str = "sshee-scroll #{pane_in_mode} #{scroll_position} #{history_size} #{pane_height} #{history_limit}";

/// The remote command for an interactive session. If tmux exists on the
/// server, the shell lives inside a named tmux session that outlives the
/// connection, and reconnecting attaches to it again. tmux's status bar is
/// hidden so it looks like a plain terminal. Without tmux it's a normal shell.
/// A new session starts with the server's welcome message, as a login does.
///
/// Tuned for programs like Claude Code, OpenCode and vim: full 24-bit color
/// (ssh doesn't pass COLORTERM on, and tmux 3.2+ needs telling the terminal
/// has it), Esc without tmux waiting for more keys, focus reports, screen
/// updates drawn in one go, and copying (a mouse selection, or a program's
/// own copy) reaching your clipboard. With tmux 3.2+ it also reports where
/// you've scrolled to, and binds a key (ESC [ 9006 ~, which no keyboard
/// sends) that leaves scrolling, so typing goes back to the prompt.
pub fn durable_command(key: &str, run: Option<&[String]>) -> String {
    let name = session_name(key);
    // With a command: tmux runs it as the session's only window; without
    // tmux it's exec'd directly. Without one: the login shell, after the
    // welcome message. tmux runs a session's command with the login shell,
    // which may be fish, hence the `sh -c`.
    let (in_tmux, plain) = match run {
        Some(argv) => {
            let cmd = argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
            (format!(" {}", shell_quote(&cmd)), format!("exec {cmd}"))
        }
        None => {
            let login = format!(r#"{MOTD}; exec "${{SHELL:-/bin/sh}}" -l"#);
            (format!(" {}", shell_quote(&format!("sh -c {}", shell_quote(&login)))), login)
        }
    };
    let script = format!(
        r#"export COLORTERM=truecolor; if command -v tmux >/dev/null 2>&1; then printf "\033]7777;sshee=durable\007"; tmux set-environment -g COLORTERM truecolor 2>/dev/null; case "$(tmux -V)" in "tmux 1."*|"tmux 2."*|"tmux 3.0"*|"tmux 3.1"*) new= ;; *) new=1 ;; esac; if [ -n "$new" ]; then set -- -T RGB,sync; else set --; fi; set -- "$@" new-session -A -s {name}{in_tmux} \; set-option status off \; set-option mouse on \; set-option -s escape-time 10 \; set-option -s focus-events on \; set-option -s set-clipboard on; if [ -n "$new" ]; then E=$(printf '\033'); set -- "$@" \; set-option set-titles on \; set-option set-titles-string '{SCROLL_TITLE}' \; set-option -s 'user-keys[906]' "${{E}}[9006~" \; bind-key -n User906 if-shell -F '#{{pane_in_mode}}' 'send-keys -X cancel'; fi; exec tmux "$@"; else printf "\033]7777;sshee=plain\007"; {plain}; fi"#
    );
    format!("sh -c {}", shell_quote(&script))
}

/// Environment that lets ssh log in with the target's password, if it has one.
pub fn password_env(t: &SshTarget, env: &SshEnv) -> Option<Vec<(&'static str, String)>> {
    Some(askpass_env(env.askpass.as_deref()?, t.password.as_ref()?))
}

/// Builds a non-interactive ssh command that runs `script` with `sh`.
pub fn remote_command(t: &SshTarget, env: &SshEnv, script: &str) -> Command {
    let mut cmd = Command::new(ssh_program());
    cmd.args(common_args(t, env));
    // BatchMode stops ssh from asking anything. With a password we let it ask,
    // and askpass answers the password prompt and refuses everything else.
    match password_env(t, env) {
        Some(vars) => {
            cmd.envs(vars);
        }
        None => {
            cmd.args(["-o", "BatchMode=yes"]);
        }
    }
    cmd.args(["-T", "--"]).arg(&t.destination).arg(format!("sh -c {}", shell_quote(script)));
    hide_console(&mut cmd);
    cmd
}

/// Runs a script on the server and returns its stdout. Without input it
/// goes through a shell that's already signed in (see `shell_pool`).
pub fn run_remote(t: &SshTarget, env: &SshEnv, script: &str, stdin: Option<Vec<u8>>) -> Result<Vec<u8>, OpError> {
    if stdin.is_none() {
        return crate::shell_pool::run(t, env, script, None);
    }
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

/// Like `run_remote`, for commands that only look: if the shell turns out to
/// have lost its connection it's asked again on a fresh one, and an answer
/// that takes over a minute counts as no answer.
pub fn run_remote_read(t: &SshTarget, env: &SshEnv, script: &str) -> Result<Vec<u8>, OpError> {
    crate::shell_pool::run(t, env, script, Some(std::time::Duration::from_secs(60)))
}

/// Like `run_remote_read`, for polls that repeat every few seconds (metrics,
/// what each terminal runs). They take turns and never hold up something
/// you asked for; see `shell_pool::run_poll`.
pub fn run_remote_poll(t: &SshTarget, env: &SshEnv, script: &str) -> Result<Vec<u8>, OpError> {
    crate::shell_pool::run_poll(t, env, script, std::time::Duration::from_secs(60))
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
        assert!(c.contains(r#"set -- "$@" new-session -A -s sshee-k1"#));
        assert!(c.contains(r#"exec tmux "$@""#));
        assert!(c.contains("export COLORTERM=truecolor"));
        assert!(c.contains("motd.dynamic"));
        assert!(c.contains("sshee-scroll #{pane_in_mode}"));
    }
}
