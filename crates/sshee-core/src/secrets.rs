//! Server passwords. Remembered ones live in the system keychain (Keychain
//! on macOS, Credential Manager on Windows, Secret Service on Linux); the
//! rest stay in memory until the app quits. Nothing is written to our files.
//!
//! ssh gets the password through SSH_ASKPASS: it runs our own executable,
//! which answers password prompts and nothing else. A host key question or a
//! key passphrase prompt gets no answer, so those still fail safe.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;

const SERVICE: &str = "io.github.footprintsntrails.ssheesh";

/// Set on the ssh process so our executable knows it was started as askpass.
pub const ASKPASS_FLAG: &str = "SSHEESH_ASKPASS";
const PASSWORD_VAR: &str = "SSHEESH_PASSWORD";
const FOR_VAR: &str = "SSHEESH_PASSWORD_FOR";
const HOPS_VAR: &str = "SSHEESH_PASSWORD_HOPS";
const LABELS_VAR: &str = "SSHEESH_PASSWORD_LABELS";

/// A password that never shows up in logs or debug output.
#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(••••)")
    }
}

/// Runs a keychain call on a thread of its own. On Linux the keychain client
/// starts its own async runtime, which panics inside Tauri's (every async
/// command runs on one), so it never runs on the caller's thread.
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::spawn(f).join().ok()
}

#[derive(Default)]
pub struct PasswordStore {
    memory: Mutex<HashMap<String, Secret>>,
    /// Hosts already looked for in the keychain by `recover` this run.
    looked: Mutex<HashSet<String>>,
}

impl PasswordStore {
    /// The password for a host: from memory, or from the keychain when it was
    /// remembered there. Keychain reads are cached for the rest of the run.
    pub fn get(&self, host_id: &str, remembered: bool) -> Option<Secret> {
        if let Some(s) = self.memory.lock().unwrap().get(host_id) {
            return Some(s.clone());
        }
        if !remembered {
            return None;
        }
        let id = host_id.to_string();
        let pw = off_runtime(move || keyring::Entry::new(SERVICE, &id).ok()?.get_password().ok()).flatten()?;
        let s = Secret::new(pw);
        self.memory.lock().unwrap().insert(host_id.to_string(), s.clone());
        Some(s)
    }

    /// Looks in the keychain for a host whose saved password went unnoted:
    /// 0.3.4 and earlier saved it there but told the server list it wasn't.
    /// Asks the keychain at most once per host per run.
    pub fn recover(&self, host_id: &str) -> Option<Secret> {
        if !self.looked.lock().unwrap().insert(host_id.to_string()) {
            return None;
        }
        self.get(host_id, true)
    }

    /// Keeps a password for this run, and in the keychain when `remember` is
    /// set. If the keychain refuses, the password still works until you quit.
    pub fn set(&self, host_id: &str, password: Secret, remember: bool) -> Result<(), String> {
        self.memory.lock().unwrap().insert(host_id.to_string(), password.clone());
        if !remember {
            return Ok(());
        }
        let (id, pw) = (host_id.to_string(), password.expose().to_string());
        off_runtime(move || keyring::Entry::new(SERVICE, &id).and_then(|e| e.set_password(&pw)).map_err(|e| e.to_string()))
            .unwrap_or_else(|| Err("the keychain stopped".into()))
            .map_err(|e| format!("Couldn't save the password in your keychain ({e}). It works until you quit SSHeesh."))
    }

    pub fn forget(&self, host_id: &str) {
        self.memory.lock().unwrap().remove(host_id);
        let id = host_id.to_string();
        off_runtime(move || {
            if let Ok(e) = keyring::Entry::new(SERVICE, &id) {
                let _ = e.delete_credential();
            }
        });
    }
}

/// Who a saved password is for, as ssh names them in its prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordFor {
    /// "user@host", with the host as `ssh -G` resolves it (its HostKeyAlias if set).
    pub who: String,
    /// The connection goes through another server first (ProxyJump,
    /// ProxyCommand), whose own ssh inherits this environment and asks too.
    pub hops: bool,
    /// ssh labels a server's own questions with the account, "(user@host)
    /// Password:" (OpenSSH 8.4 on). Before that a hop could word its
    /// question exactly like this server's password prompt.
    pub labels_questions: bool,
}

/// Environment for an ssh process that should log in to `to` with `password`.
pub fn askpass_env(program: &std::path::Path, password: &Secret, to: &PasswordFor) -> Vec<(&'static str, String)> {
    vec![
        ("SSH_ASKPASS", program.display().to_string()),
        ("SSH_ASKPASS_REQUIRE", "force".into()),
        // Older OpenSSH only uses askpass when DISPLAY is set.
        ("DISPLAY", std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into())),
        (ASKPASS_FLAG, "1".into()),
        (PASSWORD_VAR, password.expose().to_string()),
        (FOR_VAR, to.who.clone()),
        (HOPS_VAR, if to.hops { "1" } else { "0" }.into()),
        (LABELS_VAR, if to.labels_questions { "1" } else { "0" }.into()),
    ]
}

/// True for ssh's password prompts: "user@host's password:", "Password:".
pub fn is_password_prompt(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    p.ends_with("password:") || p.contains("'s password")
}

/// Whether `prompt` asks for the password of `who`. ssh names the account
/// it asks for ("user@host's password:", and "(user@host) Password:" from
/// OpenSSH 8.4 on), so a hop's ssh asking for its own account gets nothing.
/// Only an older ssh's bare "Password:" names nobody: that one is answered
/// when there's no hop that could be asking. Through a hop, an older ssh
/// can't tell a hop's question worded as "user@host's password:" from the
/// real prompt, so neither is answered.
pub fn answers(prompt: &str, to: &PasswordFor) -> bool {
    let p = prompt.trim().to_lowercase();
    let who = to.who.to_lowercase();
    if p.starts_with(&format!("{who}'s password:")) {
        return !to.hops || to.labels_questions;
    }
    // Before OpenSSH 8.4 ssh adds no label, so one in the prompt is the server's own wording.
    if let Some(rest) = p.strip_prefix(&format!("({who}) ")) {
        return (!to.hops || to.labels_questions) && is_password_prompt(rest);
    }
    !to.hops && !p.starts_with('(') && !p.contains("'s password") && is_password_prompt(&p)
}

/// (major, minor) from `ssh -V`: "OpenSSH_10.2p1 …", "OpenSSH_for_Windows_8.1p1, …".
pub fn openssh_version(banner: &str) -> Option<(u32, u32)> {
    let rest = banner.split("OpenSSH_").nth(1)?;
    let rest = rest.strip_prefix("for_Windows_").unwrap_or(rest);
    let (major, rest) = rest.split_once('.')?;
    let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// When this executable was started by ssh as its askpass helper, answers
/// the prompt and returns the exit code. Returns None for a normal launch.
pub fn askpass_main() -> Option<i32> {
    if std::env::var(ASKPASS_FLAG).ok()? != "1" {
        return None;
    }
    let prompt = std::env::args().nth(1).unwrap_or_default();
    // Unset means the cautious answer: there may be a hop, and an older ssh.
    let to = PasswordFor {
        who: std::env::var(FOR_VAR).unwrap_or_default(),
        hops: std::env::var(HOPS_VAR).as_deref() != Ok("0"),
        labels_questions: std::env::var(LABELS_VAR).as_deref() == Ok("1"),
    };
    match std::env::var(PASSWORD_VAR) {
        Ok(pw) if answers(&prompt, &to) => {
            println!("{pw}");
            Some(0)
        }
        _ => {
            if to.hops && !to.labels_questions && is_password_prompt(&prompt) {
                eprintln!("SSHeesh didn't type the saved password: through a jump host, an ssh older than OpenSSH 8.4 can't show which server is asking. Update OpenSSH, or sign in with a key.");
            }
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_only_password_prompts() {
        assert!(is_password_prompt("deploy@203.0.113.10's password: "));
        assert!(is_password_prompt("(deploy@web) Password: "));
        assert!(is_password_prompt("Password:"));
        assert!(!is_password_prompt("Are you sure you want to continue connecting (yes/no/[fingerprint])? "));
        assert!(!is_password_prompt("Enter passphrase for key '/home/me/.ssh/id_ed25519': "));
        assert!(!is_password_prompt("Verification code: "));
    }

    #[test]
    fn answers_only_the_server_the_password_is_for() {
        let direct = PasswordFor { who: "deploy@203.0.113.10".into(), hops: false, labels_questions: true };
        assert!(answers("deploy@203.0.113.10's password: ", &direct));
        assert!(answers("(deploy@203.0.113.10) Password: ", &direct));
        assert!(answers("Password:", &direct), "an older ssh's keyboard-interactive prompt");
        assert!(!answers("root@203.0.113.10's password: ", &direct));
        assert!(!answers("(deploy@203.0.113.10) Verification code: ", &direct));
        // Behind a ProxyJump, the hop's ssh asks for its own account: it gets nothing.
        let behind = PasswordFor { who: "deploy@10.0.0.5".into(), hops: true, labels_questions: true };
        assert!(answers("deploy@10.0.0.5's password: ", &behind));
        assert!(answers("(deploy@10.0.0.5) Password: ", &behind));
        assert!(!answers("jump@bastion.example's password: ", &behind));
        assert!(!answers("(jump@bastion.example) Password: ", &behind));
        assert!(!answers("Password:", &behind), "can't tell which server asks");
        // An older ssh shows the hop's own question unlabeled, worded as it likes.
        let old = PasswordFor { labels_questions: false, ..behind.clone() };
        assert!(!answers("deploy@10.0.0.5's password: ", &old));
        assert!(!answers("(deploy@10.0.0.5) Password: ", &old), "the hop can write the label itself");
        // Without a hop an older ssh is fine: only the server itself can ask.
        assert!(answers("deploy@203.0.113.10's password: ", &PasswordFor { labels_questions: false, ..direct }));
    }

    #[test]
    fn reads_the_openssh_version() {
        assert_eq!(openssh_version("OpenSSH_10.2p1 Ubuntu-2ubuntu3.8, OpenSSL 3.5.5 27 Jan 2026"), Some((10, 2)));
        assert_eq!(openssh_version("OpenSSH_for_Windows_8.1p1, LibreSSL 3.0.2"), Some((8, 1)));
        assert_eq!(openssh_version("OpenSSH_8.4p1 Debian-5+deb11u3, OpenSSL 1.1.1w"), Some((8, 4)));
        assert_eq!(openssh_version("Sun_SSH_1.1"), None);
    }

    #[test]
    fn secrets_stay_out_of_debug_output() {
        let s = Secret::new("hunter2");
        assert!(!format!("{s:?}").contains("hunter2"));
    }

    #[test]
    fn session_passwords_stay_in_memory() {
        let store = PasswordStore::default();
        store.set("saved:x", Secret::new("pw"), false).unwrap();
        assert_eq!(store.get("saved:x", false).unwrap().expose(), "pw");
        store.forget("saved:x");
        assert!(store.get("saved:x", false).is_none());
    }
}
