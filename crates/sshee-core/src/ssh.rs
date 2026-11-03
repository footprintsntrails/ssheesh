//! Building `ssh` invocations and running one-off remote commands.

use crate::secrets::{askpass_env, PasswordFor, Secret};
use crate::error::OpError;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

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

pub fn ssh_program() -> std::ffi::OsString {
    program("ssh")
}

/// The program to start for a bare `name`. On Windows that name is looked
/// up in SSHeesh's own folder first (Rust does as CreateProcess does), and
/// the portable app often runs from Downloads, where a downloaded `ssh.exe`
/// would win. So there it's looked up as Rust would minus that folder:
/// System32, then each absolute folder on PATH.
pub fn program(name: &str) -> std::ffi::OsString {
    #[cfg(windows)]
    {
        let system = std::env::var_os("SystemRoot").map(|root| PathBuf::from(root).join("System32"));
        let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
        let found = system.into_iter().chain(path).filter(|dir| dir.is_absolute()).map(|dir| dir.join(format!("{name}.exe"))).find(|exe| exe.is_file());
        if let Some(exe) = found {
            return exe.into_os_string();
        }
    }
    name.into()
}

/// Options shared by interactive sessions and one-off commands.
/// Everything before the destination. While Tor is on, they send the
/// connection through it (see `tor`).
pub fn common_args(t: &SshTarget, env: &SshEnv) -> Vec<String> {
    args(t, env, crate::tor::proxy_command_for(t))
}

/// For reading your config with `ssh -G`, which connects nowhere. While Tor
/// is on it gets Tor's ProxyCommand too: with one, ssh looks no names up.
fn config_args(t: &SshTarget) -> Vec<String> {
    args(t, &SshEnv::default(), crate::tor::proxy_command())
}

fn args(t: &SshTarget, env: &SshEnv, tor: Option<String>) -> Vec<String> {
    let mut a: Vec<String> = t.extra_args.clone();
    let mut opt = |v: &str| {
        a.push("-o".into());
        a.push(v.into());
    };
    // Notice a dead connection within ~15s instead of hanging forever.
    // Through Tor a route can stall for a few seconds and come back, and the
    // first connection may wait for Tor to start.
    opt("ServerAliveInterval=5");
    opt(if tor.is_some() { "ServerAliveCountMax=6" } else { "ServerAliveCountMax=3" });
    opt(if tor.is_some() { "ConnectTimeout=60" } else { "ConnectTimeout=10" });
    // Kerberos is tried first where a system turns it on (Debian and Ubuntu
    // do), and can cost a second or more on every connect; nobody here uses it.
    opt("GSSAPIAuthentication=no");
    if t.password.is_some() {
        // A wrong saved password should fail once, not three times.
        opt("NumberOfPasswordPrompts=1");
    }
    if cfg!(unix) {
        if let Some(dir) = &env.control_dir {
            opt("ControlMaster=auto");
            // Connections through Tor share only with each other.
            opt(&format!("ControlPath={}/{}%C", dir.display(), if tor.is_some() { TOR_SHARED } else { "" }));
            opt("ControlPersist=300");
        }
    }
    if let Some(cmd) = &tor {
        a.extend(crate::tor::ssh_options(cmd));
        // ssh sends keystrokes on a fixed beat, with fake ones between, so
        // nobody watching the network can time your typing. Through Tor that
        // nearly doubles what a key takes: 60-81 ms without it against 118-150
        // with, over three runs, and a slower beat was no better. Settings →
        // Privacy keeps it on, at that price (see `terminal_args`).
        if !crate::tor::hides_typing() && knows_keystroke_timing(ssh_version()) {
            a.extend(["-o".to_string(), "ObscureKeystrokeTiming=no".to_string()]);
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
        // Quoted: 1Password's socket on a Mac lives under "Group Containers".
        a.push(format!("IdentityAgent=\"{}\"", expand_tilde(agent)));
    }
    a
}

/// What starts the name of a shared connection that goes through Tor.
const TOR_SHARED: &str = "t";

/// Ends the shared connections (see `SshEnv::control_dir`) that go through
/// Tor, or the ones that don't: after Tor is turned on or off, nothing
/// lingers on the old way. Sessions still using one end with it.
pub fn close_shared(env: &SshEnv, tor: bool) {
    let Some(dir) = env.control_dir.as_ref().filter(|_| cfg!(unix)) else { return };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut waiting = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // ssh names them by a 40-digit hash (%C).
        let hash = name.strip_prefix(TOR_SHARED).filter(|_| tor).unwrap_or(&name);
        if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || (hash.len() == name.len()) == tor {
            continue;
        }
        let mut cmd = Command::new(ssh_program());
        cmd.arg("-o").arg(format!("ControlPath={}", entry.path().display())).args(["-O", "exit", "sshee"]);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        hide_console(&mut cmd);
        if let Ok(child) = cmd.spawn() {
            waiting.push(child);
        }
    }
    for mut child in waiting {
        let _ = child.wait();
    }
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

/// tmux reports the scroll position through the window title, which the
/// app reads to draw a scrollbar: in copy mode or not, lines scrolled up,
/// lines of history, pane height and the history limit.
pub const SCROLL_TITLE: &str = "sshee-scroll #{pane_in_mode} #{scroll_position} #{history_size} #{pane_height} #{history_limit}";

/// How long tmux waits for the rest of a key sequence, in ms. Short, so Esc
/// in vim or an agent doesn't lag. On Windows the console between SSHeesh
/// and ssh (ConPTY) passes a mouse report on in pieces, and over Wi-Fi the
/// pieces can arrive more than 10 ms apart: tmux gave up halfway, and the rest
/// landed in the shell as text ("32;29;29M"). Through Tor they can be further
/// apart still, and a key takes longer than this anyway.
fn escape_time_ms(through_console: bool) -> u32 {
    match (through_console, crate::tor::is_on()) {
        (true, true) => 300,
        (true, false) => 100,
        (false, _) => 10,
    }
}

/// How long tmux waits for the answers it asks its terminal for as it attaches,
/// and for how long after attaching it waits that long.
const ANSWERS_MS: u32 = 3000;
const ANSWERS_SECONDS: u32 = 5;

/// The remote command for an interactive session. If tmux exists on the
/// server, the shell lives inside a named tmux session that outlives the
/// connection, and reconnecting attaches to it again. tmux's status bar is
/// hidden so it looks like a plain terminal. Without tmux it's a normal shell.
/// A new session starts with the server's welcome message, as a login does.
///
/// Tuned for programs like Claude Code, OpenCode and vim: full 24-bit color
/// (ssh doesn't pass COLORTERM on, and tmux 3.2+ needs telling the terminal
/// has it), Esc without tmux waiting long for more keys, focus reports, screen
/// updates drawn in one go, and copying (a mouse selection, or a program's
/// own copy) reaching your clipboard. With tmux 3.2+ it also reports where
/// you've scrolled to, and binds a key (ESC [ 9006 ~, which no keyboard
/// sends) that leaves scrolling, so typing goes back to the prompt.
///
/// On Windows that console also answers tmux's "which terminal are you?" itself,
/// a character at a time. tmux gives an answer half a second, and through Tor
/// it can take longer: the rest landed in the shell as text, as here with
/// "61;6;7;21;22;23;24;28;32;42c0;10;1c". So for the first seconds after
/// attaching, when these answers come, it waits longer (see `ANSWERS_MS`).
pub fn durable_command(key: &str, run: Option<&[String]>, welcome: &crate::welcome::Welcome) -> String {
    durable_command_for(key, run, welcome, cfg!(windows))
}

fn durable_command_for(key: &str, run: Option<&[String]>, welcome: &crate::welcome::Welcome, through_console: bool) -> String {
    let name = session_name(key);
    let usual = escape_time_ms(through_console);
    let escape_time = if through_console {
        // From a job, not `run-shell -d`: that one is dropped if this client goes first,
        // and every other terminal on the server would be left with the long wait.
        format!(r#"set-option -s escape-time {ANSWERS_MS} \; run-shell -b 'sleep {ANSWERS_SECONDS}; tmux set-option -s escape-time {usual}'"#)
    } else {
        format!("set-option -s escape-time {usual}")
    };
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
            let login = format!("{}\nexec \"${{SHELL:-/bin/sh}}\" -l", crate::welcome::script(welcome));
            (format!(" {}", shell_quote(&format!("sh -c {}", shell_quote(&login)))), login)
        }
    };
    let script = format!(
        r#"export COLORTERM=truecolor; if command -v tmux >/dev/null 2>&1; then printf "\033]7777;sshee=durable\007"; tmux set-environment -g COLORTERM truecolor 2>/dev/null; case "$(tmux -V)" in "tmux 1."*|"tmux 2."*|"tmux 3.0"*|"tmux 3.1"*) new= ;; *) new=1 ;; esac; if [ -n "$new" ]; then set -- -T RGB,sync; else set --; fi; set -- "$@" new-session -A -s {name}{in_tmux} \; set-option status off \; set-option mouse on \; {escape_time} \; set-option -s focus-events on \; set-option -s set-clipboard on; if [ -n "$new" ]; then E=$(printf '\033'); set -- "$@" \; set-option set-titles on \; set-option set-titles-string '{SCROLL_TITLE}' \; set-option -s 'user-keys[906]' "${{E}}[9006~" \; bind-key -n User906 if-shell -F '#{{pane_in_mode}}' 'send-keys -X cancel'; fi; exec tmux "$@"; else printf "\033]7777;sshee=plain\007"; {plain}; fi"#
    );
    format!("sh -c {}", shell_quote(&script))
}

/// Environment that lets ssh log in with the target's password, if it has one.
pub fn password_env(t: &SshTarget, env: &SshEnv) -> Option<Vec<(&'static str, String)>> {
    let program = env.askpass.as_deref()?;
    let password = t.password.as_ref()?;
    Some(askpass_env(program, password, &password_for(t)))
}

/// This computer's ssh, asked of `ssh -V` once in a run.
fn ssh_version() -> Option<(u32, u32)> {
    static VERSION: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let mut cmd = Command::new(ssh_program());
        cmd.arg("-V").stdin(Stdio::null()).stdout(Stdio::null());
        hide_console(&mut cmd);
        // ssh -V prints its version to stderr.
        let banner = cmd.output().map(|o| String::from_utf8_lossy(&o.stderr).into_owned()).unwrap_or_default();
        crate::secrets::openssh_version(&banner)
    })
}

/// Whether this computer's ssh labels a server's questions with the account
/// (OpenSSH 8.4 on).
fn labels_questions() -> bool {
    ssh_version().is_some_and(|v| v >= (8, 4))
}

/// Whether ssh can be told how to send keystrokes (OpenSSH 9.5 on). An older
/// one won't start over an option it doesn't know.
fn knows_keystroke_timing(version: Option<(u32, u32)>) -> bool {
    version.is_some_and(|v| v >= (9, 5))
}

/// What `ssh -G` resolves for a target with your config: one "key value"
/// per line. It reads the config only, without connecting.
pub fn ssh_config(t: &SshTarget) -> Result<String, OpError> {
    let mut cmd = Command::new(ssh_program());
    cmd.args(config_args(t)).arg("-G").arg("--").arg(&t.destination);
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().map_err(spawn_error)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Your config for `t` as you wrote it, Tor's ProxyCommand left out, so a
/// ProxyJump or ProxyCommand of yours shows. Names aren't canonicalized, so
/// nothing is looked up (unless `CanonicalizePermittedCNAMEs` is set, which
/// `tor::check` asks first).
pub fn ssh_config_as_written(t: &SshTarget) -> Result<String, OpError> {
    let mut cmd = Command::new(ssh_program());
    cmd.args(["-o", "CanonicalizeHostname=no"]).args(args(t, &SshEnv::default(), None)).arg("-G").arg("--").arg(&t.destination);
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().map_err(spawn_error)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The value of `key` in `ssh -G` output.
pub fn config_value(config: &str, key: &str) -> Option<String> {
    config.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(' ').map(|v| v.trim().to_string()))
}

/// Who a target's password is for, as ssh names them in its prompts. Asked
/// of `ssh -G` once per target in a run.
fn password_for(t: &SshTarget) -> Arc<PasswordFor> {
    static SEEN: OnceLock<Mutex<HashMap<Vec<String>, Arc<PasswordFor>>>> = OnceLock::new();
    let mut key = config_args(t);
    key.push(t.destination.clone());
    let seen = SEEN.get_or_init(Default::default);
    if let Some(known) = seen.lock().unwrap().get(&key) {
        return known.clone();
    }
    let Ok(config) = ssh_config(t) else {
        let who = format!("{}@{}", t.user.as_deref().unwrap_or_default(), t.destination);
        return Arc::new(PasswordFor { who, hops: false, labels_questions: labels_questions() });
    };
    let get = |k: &str| config_value(&config, k);
    let user = get("user").or_else(|| t.user.clone()).unwrap_or_default();
    let host = get("hostkeyalias").or_else(|| get("hostname")).unwrap_or_else(|| t.destination.clone());
    // Through ProxyJump or ProxyCommand another ssh logs in to the hop first.
    // Not while Tor is on: its ProxyCommand logs in to nothing, and a server
    // with a hop of its own doesn't connect then (see `tor::check`).
    let hops = !crate::tor::is_on() && ["proxyjump", "proxycommand"].iter().any(|k| get(k).is_some_and(|v| v != "none"));
    let found = Arc::new(PasswordFor { who: format!("{user}@{host}"), hops, labels_questions: labels_questions() });
    seen.lock().unwrap().insert(key, found.clone());
    found
}

/// Builds a non-interactive ssh command that runs `script` with `sh`.
pub fn remote_command(t: &SshTarget, env: &SshEnv, script: &str) -> Command {
    command_with(t, env, script, crate::tor::proxy_command_for(t))
}

fn command_with(t: &SshTarget, env: &SshEnv, script: &str, tor: Option<String>) -> Command {
    let mut cmd = Command::new(ssh_program());
    cmd.args(args(t, env, tor));
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

/// What a terminal connects with. To hide your typing rhythm through Tor it
/// needs a connection of its own: ssh decides on the beat when a connection
/// opens, whatever the terminals that share it later ask for, and drops the
/// beat while anything else, Files say, runs on that connection.
pub fn terminal_args(t: &SshTarget, env: &SshEnv) -> Vec<String> {
    if crate::tor::hides_typing() && crate::tor::proxy_command_for(t).is_some() {
        return common_args(&on_its_own(t), &SshEnv { control_dir: None, ..env.clone() });
    }
    common_args(t, env)
}

/// A command that moves a file's worth of data. Through Tor it gets a
/// connection of its own where it can: on a shared one, every key typed in a
/// terminal waits behind what's on its way. A key took 1006 ms during a
/// download and 1350 during an upload that way, against 123 and 126 alone.
pub fn transfer_command(t: &SshTarget, env: &SshEnv, script: &str) -> Command {
    if !transfers_alone(t, env) {
        return remote_command(t, env, script);
    }
    command_with(&on_its_own(t), &SshEnv { control_dir: None, ..env.clone() }, script, crate::tor::transfer_proxy_command_for(t))
}

/// Whether `transfer_command` gives `t` a connection of its own, which is what
/// can then be moved to another route (see `tor::move_transfers`).
pub fn transfers_alone(t: &SshTarget, env: &SshEnv) -> bool {
    let shares = cfg!(unix) && env.control_dir.is_some();
    crate::tor::proxy_command_for(t).is_some() && (!shares || signs_in_alone(t, env))
}

/// `t` on a connection of its own, whatever your config says about sharing.
fn on_its_own(t: &SshTarget) -> SshTarget {
    let mut alone = t.clone();
    for opt in ["ControlMaster=no", "ControlPath=none"] {
        alone.extra_args.push("-o".into());
        alone.extra_args.push(opt.into());
    }
    alone
}

/// Whether a server lets a new connection in without you. One you signed in
/// to by typing a code doesn't, and a transfer can only use the connection
/// you opened. Asked once a server in a run, and only the answer is kept.
fn signs_in_alone(t: &SshTarget, env: &SshEnv) -> bool {
    static ANSWERS: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let answers = ANSWERS.get_or_init(Mutex::default);
    let key = format!("{:?} {} {:?} {:?} {:?}", t.user, t.destination, t.port, t.identity_file, t.extra_args);
    if let Some(known) = answers.lock().unwrap().get(&key) {
        return *known;
    }
    let mut cmd = remote_command(&on_its_own(t), &SshEnv { control_dir: None, ..env.clone() }, "true");
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let alone = succeeds_within(cmd, std::time::Duration::from_secs(90));
    answers.lock().unwrap().insert(key, alone);
    alone
}

/// Whether a command succeeds in time. A server that never lets it finish doesn't keep it here.
fn succeeds_within(mut cmd: Command, limit: std::time::Duration) -> bool {
    let Ok(mut child) = cmd.spawn() else { return false };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
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
    let mut cmd = Command::new(program("sh"));
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

    /// A terminal's real command in real tmux, with "which terminal are you?"
    /// answered as Windows' console does: a character at a time, too slowly for
    /// tmux's usual half a second, as happens through Tor.
    #[cfg(unix)]
    #[test]
    fn a_slow_answer_from_windows_console_stays_out_of_the_shell() {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        use std::io::Read;
        use std::time::{Duration, Instant};
        // Without tmux there's nothing to check.
        if !Command::new("tmux").arg("-V").output().is_ok_and(|o| o.status.success()) {
            return;
        }
        // Short: a tmux socket's path has a limit, the temp folder's name doesn't.
        let dir = PathBuf::from(format!("/tmp/sshee-ask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pair = native_pty_system().openpty(PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", &durable_command_for("ask", None, &Default::default(), true)]);
        cmd.env("TMUX_TMPDIR", &dir);
        cmd.env("TERM", "xterm-256color");
        cmd.env("SHELL", "/bin/sh");
        cmd.env_remove("TMUX");
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (said, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 || said.send(chunk[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut writer = pair.master.take_writer().unwrap();
        let tmux = |args: &[&str]| {
            let out = Command::new("tmux").env("TMUX_TMPDIR", &dir).args(args).output().unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        // Waits for the question.
        let (mut all, start) = (Vec::new(), Instant::now());
        while !all.windows(3).any(|w| w == b"\x1b[c") && start.elapsed() < Duration::from_secs(5) {
            if let Ok(chunk) = heard.recv_timeout(Duration::from_millis(100)) {
                all.extend(chunk);
            }
        }
        assert!(all.windows(3).any(|w| w == b"\x1b[c"), "tmux didn't ask: {:?}", String::from_utf8_lossy(&all));
        for byte in b"\x1b[?61;6;7;21;22;23;24;28;32;42c\x1b[>0;10;1c" {
            writer.write_all(&[*byte]).unwrap();
            writer.flush().unwrap();
            std::thread::sleep(Duration::from_millis(40));
        }
        std::thread::sleep(Duration::from_millis(500));
        let screen = tmux(&["capture-pane", "-p"]);
        // Afterwards Esc is quick again, whichever way things went.
        std::thread::sleep(Duration::from_secs(u64::from(ANSWERS_SECONDS) + 1));
        let escape_time = tmux(&["show-options", "-s", "escape-time"]);
        let _ = child.kill();
        let _ = child.wait();
        tmux(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!screen.contains("61;6;7") && !screen.contains("0;10;1c"), "landed in the shell: {screen}");
        assert_eq!(escape_time.trim(), format!("escape-time {}", escape_time_ms(true)));
    }

    #[cfg(unix)]
    #[test]
    fn a_command_that_never_finishes_is_given_up_on() {
        let sh = |script: &str| {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", script]);
            cmd
        };
        let limit = std::time::Duration::from_millis(200);
        assert!(succeeds_within(sh("true"), limit * 10));
        assert!(!succeeds_within(sh("false"), limit * 10));
        let start = std::time::Instant::now();
        assert!(!succeeds_within(sh("exec sleep 30"), limit));
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn only_windows_waits_longer_for_answers() {
        let welcome = Default::default();
        assert!(durable_command_for("k", None, &welcome, true).contains(&format!("escape-time {ANSWERS_MS}")));
        let elsewhere = durable_command_for("k", None, &welcome, false);
        assert!(elsewhere.contains("escape-time 10 ") && !elsewhere.contains("run-shell"), "{elsewhere}");
    }

    #[test]
    fn a_transfer_alone_ignores_any_sharing_in_your_config() {
        let t = SshTarget { destination: "example.invalid".into(), extra_args: vec!["-F".into(), "cfg".into()], ..Default::default() };
        let a = args(&on_its_own(&t), &SshEnv::default(), None);
        // The first value ssh hears wins, so these must come before anything from a config.
        assert_eq!(&a[..6], ["-F", "cfg", "-o", "ControlMaster=no", "-o", "ControlPath=none"]);
    }

    #[test]
    fn this_computers_ssh_accepts_what_it_gets_under_tor() {
        let t = SshTarget { destination: "example.invalid".into(), extra_args: vec!["-F".into(), if cfg!(windows) { "NUL" } else { "/dev/null" }.into()], ..Default::default() };
        let mut cmd = Command::new(ssh_program());
        cmd.args(args(&t, &SshEnv::default(), Some("x %h %p".into()))).arg("-G").arg(&t.destination);
        let Ok(out) = cmd.stdin(Stdio::null()).output() else { return };
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        if knows_keystroke_timing(ssh_version()) {
            assert!(String::from_utf8_lossy(&out.stdout).to_lowercase().contains("obscurekeystroketiming no"));
        }
    }

    #[test]
    fn only_an_ssh_that_knows_the_option_is_told_how_to_send_keystrokes() {
        assert!(knows_keystroke_timing(Some((9, 5))));
        assert!(knows_keystroke_timing(Some((10, 2))));
        assert!(!knows_keystroke_timing(Some((9, 4))));
        assert!(!knows_keystroke_timing(Some((8, 1))));
        assert!(!knows_keystroke_timing(None));
    }

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
        let c = durable_command("k1", None, &Default::default());
        assert!(c.starts_with("sh -c '"));
        assert!(c.contains(r#"set -- "$@" new-session -A -s sshee-k1"#));
        assert!(c.contains(r#"exec tmux "$@""#));
        assert!(c.contains("export COLORTERM=truecolor"));
        assert!(c.contains("motd.dynamic"));
        assert!(c.contains("sshee-scroll #{pane_in_mode}"));
    }
}
