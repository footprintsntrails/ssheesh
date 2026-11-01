//! Running a check-up fix or a disk cleanup for you. Most run on their own,
//! without a terminal, and come back with what they printed. The ones that
//! need you (sudo wants your password, a package manager asks before it
//! goes ahead) are left for a terminal, with a plain word on what to answer.

use std::process::Stdio;

use serde::Serialize;

use crate::error::{ErrorCode, OpError};
use crate::ssh::{remote_command, shell_quote, SshEnv, Target};

/// What became of a command run for you.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunOutcome {
    /// It ran to the end; what it printed.
    Done { output: String },
    /// It stopped with an error; what it printed, and its exit code.
    Failed { output: String, code: Option<i32> },
    /// It has to run in a terminal, where you can answer it: `ask` says
    /// what it will want from you, in plain words.
    NeedsYou { ask: String },
}

/// Printed by the script below when sudo wants a password.
const NEEDS_PASSWORD: &str = "__SSHEESH_NEEDS_PASSWORD__";
/// Printed by the script below with the command's exit code after it.
const EXIT: &str = "__SSHEESH_EXIT=";
/// How much of what it printed comes back: the end, where the outcome is.
const KEEP: usize = 4000;

/// What a command will ask before it goes on, when it asks anything: a
/// package manager's "Do you want to continue? [Y/n]" without its yes flag.
pub fn asks_before(command: &str) -> Option<&'static str> {
    let c = format!(" {command} ");
    let upgrade = [" apt-get upgrade", " apt upgrade", " apt-get dist-upgrade", " apt full-upgrade", " dnf upgrade", " yum update", " pacman -Syu"];
    let yes = [" -y ", " --yes ", " --assume-yes ", " --noconfirm "];
    (upgrade.iter().any(|u| c.contains(u)) && !yes.iter().any(|y| c.contains(y)))
        .then_some("It lists what it's about to install and asks whether to go ahead: type y and press Enter.")
}

/// Runs `command` on the server with nobody there to answer it.
pub fn run_unattended(target: &Target, env: &SshEnv, command: &str) -> Result<RunOutcome, OpError> {
    let Target::Ssh(t) = target else {
        return Err(OpError::new(ErrorCode::Unsupported, "These are for servers."));
    };
    if let Some(ask) = asks_before(command) {
        return Ok(RunOutcome::NeedsYou { ask: ask.into() });
    }
    // Asked first, so a password prompt never waits on nobody.
    let ask_sudo = if command.contains("sudo ") {
        format!(r#"if [ "$(id -u)" != 0 ] && ! sudo -n true 2>/dev/null; then echo {NEEDS_PASSWORD}; exit 0; fi; "#)
    } else {
        String::new()
    };
    let script = format!("{ask_sudo}(sh -c {}) 2>&1 </dev/null; echo \"{EXIT}$?\"", shell_quote(command));
    let mut cmd = remote_command(t, env, &script);
    cmd.stdin(Stdio::null());
    let out = cmd.output().map_err(crate::ssh::spawn_error)?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(outcome(&text, &String::from_utf8_lossy(&out.stderr), out.status.code()))
}

/// Reads what the script printed.
fn outcome(stdout: &str, stderr: &str, ssh_code: Option<i32>) -> RunOutcome {
    if stdout.lines().any(|l| l.trim() == NEEDS_PASSWORD) {
        return RunOutcome::NeedsYou { ask: password_ask() };
    }
    let (printed, code) = match stdout.rfind(EXIT) {
        Some(at) => (&stdout[..at], stdout[at + EXIT.len()..].trim().parse::<i32>().ok()),
        // The connection went before it finished: a reboot, or a drop.
        None => (stdout, None),
    };
    let mut output = format!("{printed}{stderr}").trim().to_string();
    if output.len() > KEEP {
        let cut = output.ceil_char_boundary(output.len() - KEEP);
        output = format!("…{}", &output[cut..]);
    }
    let lower = output.to_lowercase();
    if lower.contains("a terminal is required") || lower.contains("a password is required") {
        return RunOutcome::NeedsYou { ask: password_ask() };
    }
    match code {
        Some(0) => RunOutcome::Done { output },
        Some(code) => RunOutcome::Failed { output, code: Some(code) },
        None => RunOutcome::Failed { output, code: ssh_code },
    }
}

fn password_ask() -> String {
    "sudo asks for your password on the server, the one you log in with: type it and press Enter. Nothing shows while you type; that's normal.".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_which_commands_ask_before_going_on() {
        assert!(asks_before("sudo apt-get update && sudo apt-get upgrade --with-new-pkgs").is_some());
        assert!(asks_before("sudo dnf upgrade --security").is_some());
        assert!(asks_before("sudo pacman -Syu").is_some());
        assert!(asks_before("sudo apt-get install -y fail2ban").is_none());
        assert!(asks_before("sudo apt-get upgrade -y").is_none());
        assert!(asks_before("sudo journalctl --vacuum-time=7d").is_none());
    }

    #[test]
    fn reads_how_it_went() {
        assert_eq!(outcome("Vacuuming done, freed 1.2G.\n__SSHEESH_EXIT=0\n", "", Some(0)), RunOutcome::Done { output: "Vacuuming done, freed 1.2G.".into() });
        assert_eq!(
            outcome("E: Could not get lock\n__SSHEESH_EXIT=100\n", "", Some(0)),
            RunOutcome::Failed { output: "E: Could not get lock".into(), code: Some(100) }
        );
        assert!(matches!(outcome("__SSHEESH_NEEDS_PASSWORD__\n", "", Some(0)), RunOutcome::NeedsYou { .. }));
        assert!(matches!(outcome("sudo: a terminal is required to read the password\n__SSHEESH_EXIT=1\n", "", Some(0)), RunOutcome::NeedsYou { .. }));
        // A reboot takes the connection with it.
        assert_eq!(
            outcome("", "Connection to 203.0.113.10 closed by remote host.", Some(255)),
            RunOutcome::Failed { output: "Connection to 203.0.113.10 closed by remote host.".into(), code: Some(255) }
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_script_runs_the_command_and_reports_its_code() {
        let script = |command: &str| format!("(sh -c {}) 2>&1 </dev/null; echo \"{EXIT}$?\"", shell_quote(command));
        let run = |command: &str| {
            let out = std::process::Command::new("sh").arg("-c").arg(script(command)).output().unwrap();
            outcome(&String::from_utf8_lossy(&out.stdout), "", out.status.code())
        };
        assert_eq!(run("echo \"it's done\""), RunOutcome::Done { output: "it's done".into() });
        assert_eq!(run("echo nope >&2; exit 3"), RunOutcome::Failed { output: "nope".into(), code: Some(3) });
    }
}
