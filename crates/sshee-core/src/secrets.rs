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
        let pw = keyring::Entry::new(SERVICE, host_id).ok()?.get_password().ok()?;
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
        keyring::Entry::new(SERVICE, host_id)
            .and_then(|e| e.set_password(password.expose()))
            .map_err(|e| format!("Couldn't save the password in your keychain ({e}). It works until you quit SSHeesh."))
    }

    pub fn forget(&self, host_id: &str) {
        self.memory.lock().unwrap().remove(host_id);
        if let Ok(e) = keyring::Entry::new(SERVICE, host_id) {
            let _ = e.delete_credential();
        }
    }
}

/// Environment for an ssh process that should log in with `password`.
pub fn askpass_env(program: &std::path::Path, password: &Secret) -> Vec<(&'static str, String)> {
    vec![
        ("SSH_ASKPASS", program.display().to_string()),
        ("SSH_ASKPASS_REQUIRE", "force".into()),
        // Older OpenSSH only uses askpass when DISPLAY is set.
        ("DISPLAY", std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into())),
        (ASKPASS_FLAG, "1".into()),
        (PASSWORD_VAR, password.expose().to_string()),
    ]
}

/// True for ssh's password prompts: "user@host's password:", "Password:".
pub fn is_password_prompt(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    p.ends_with("password:") || p.contains("'s password")
}

/// When this executable was started by ssh as its askpass helper, answers
/// the prompt and returns the exit code. Returns None for a normal launch.
pub fn askpass_main() -> Option<i32> {
    if std::env::var(ASKPASS_FLAG).ok()? != "1" {
        return None;
    }
    let prompt = std::env::args().nth(1).unwrap_or_default();
    match std::env::var(PASSWORD_VAR) {
        Ok(pw) if is_password_prompt(&prompt) => {
            println!("{pw}");
            Some(0)
        }
        _ => Some(1),
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
