//! AI coding agents installed on a server (Claude Code, Codex, OpenCode,
//! oh-my-pi), so a terminal can offer to start one in the folder you're in.

use crate::error::{ErrorCode, OpError};
use crate::ssh::{remote_command, run_remote, shell_quote, spawn_error, SshEnv, SshTarget, Target};
use serde::Serialize;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// The agents SSHeesh knows, by the command that starts each one.
pub const KNOWN: [&str; 4] = ["claude", "codex", "opencode", "omp"];

/// Which of `KNOWN` the server has. Looks where their installers put them
/// (~/.local/bin, nvm, bun, npm's global folder…), then asks your login
/// shell, whose PATH is often only set up in ~/.bashrc or ~/.zshrc.
pub fn detect(target: &Target, env: &SshEnv) -> Result<Vec<String>, OpError> {
    let Target::Ssh(t) = target else {
        return Ok(Vec::new());
    };
    let names = KNOWN.join(" ");
    let script = format!(
        r#"for c in {names}; do for d in "$HOME/.local/bin" "$HOME/.opencode/bin" "$HOME/.bun/bin" "$HOME/.npm-global/bin" "$HOME/.claude/local" "$HOME"/.nvm/versions/node/*/bin /usr/local/bin /usr/bin /opt/homebrew/bin; do [ -x "$d/$c" ] && {{ echo "sshee-agent:$c"; break; }}; done; done; "${{SHELL:-/bin/sh}}" -lic 'for c in {names}; do command -v "$c" >/dev/null 2>&1 && echo "sshee-agent:$c"; done' </dev/null 2>/dev/null; true"#
    );
    // A shell startup file that waits on something mustn't hang the check;
    // the folder checks print first, so whatever arrived still counts.
    let out = run_with_timeout(t, env, &script, Duration::from_secs(8))?;
    Ok(parse(&out))
}

fn parse(out: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(out);
    KNOWN
        .iter()
        .filter(|k| text.lines().any(|l| l.trim().strip_prefix("sshee-agent:") == Some(**k)))
        .map(|k| k.to_string())
        .collect()
}

/// Runs a script and returns what it printed; ends it after `limit`.
pub(crate) fn run_with_timeout(t: &SshTarget, env: &SshEnv, script: &str, limit: Duration) -> Result<Vec<u8>, OpError> {
    let mut cmd = remote_command(t, env, script);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(spawn_error)?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let start = Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(reader.join().unwrap_or_default())
}

/// What a terminal is doing right now: its foreground program and folder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneInfo {
    /// "bash", "zsh", "vim", "node"…
    pub command: String,
    pub cwd: String,
    /// The shell is waiting at its prompt, so typing a command is safe.
    pub idle: bool,
}

/// Asks tmux about a terminal's pane. Fails for sessions without tmux.
pub fn pane_info(target: &Target, env: &SshEnv, session_key: &str) -> Result<PaneInfo, OpError> {
    let Target::Ssh(t) = target else {
        return Err(OpError::new(ErrorCode::Unsupported, "Not a server session."));
    };
    let name = crate::session::tmux_session_for(session_key);
    let out = run_remote(t, env, &format!("tmux display-message -p -t {} '#{{pane_current_command}}\t#{{pane_current_path}}'", shell_quote(&name)), None)?;
    parse_pane(&String::from_utf8_lossy(&out)).ok_or_else(|| OpError::new(ErrorCode::NotFound, "Couldn't read the terminal's state."))
}

fn parse_pane(out: &str) -> Option<PaneInfo> {
    let (command, cwd) = out.trim_end_matches(['\r', '\n']).split_once('\t')?;
    let shell = command.trim_start_matches('-');
    let idle = ["sh", "bash", "zsh", "fish", "dash", "ksh", "mksh", "ash", "tcsh", "csh", "nu", "xonsh"].contains(&shell);
    Some(PaneInfo { command: command.to_string(), cwd: cwd.to_string(), idle })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_agents_among_shell_noise() {
        let out = b"sshee-agent:claude\nWelcome to Ubuntu\nsshee-agent:omp\nsshee-agent:claude\nsshee-agent:claude-old\n";
        assert_eq!(parse(out), vec!["claude", "omp"]);
    }

    #[test]
    fn reads_tmux_pane() {
        let p = parse_pane("zsh\t/srv/app\n").unwrap();
        assert!(p.idle);
        assert_eq!(p.cwd, "/srv/app");
        let busy = parse_pane("node\t/home/me\n").unwrap();
        assert!(!busy.idle);
        assert!(parse_pane("").is_none());
    }
}
