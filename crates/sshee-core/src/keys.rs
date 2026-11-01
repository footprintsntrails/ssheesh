//! SSH keys and key agents on this computer, so the "Add a server" form can
//! offer them instead of asking for a file path.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::ssh::hide_console;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalKey {
    /// "~/.ssh/id_ed25519"
    pub path: String,
    pub name: String,
    /// "ED25519", "RSA", "ECDSA", "ED25519-SK" (a hardware key)…
    pub kind: String,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyAgent {
    /// "ssh-agent", "1Password", "Bitwarden"
    pub name: String,
    /// Socket to point IdentityAgent at; None for the default agent.
    pub socket: Option<String>,
    /// Keys it holds right now, if it answered.
    pub keys: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAuth {
    pub keys: Vec<LocalKey>,
    pub agents: Vec<KeyAgent>,
}

pub fn discover() -> LocalAuth {
    let Some(home) = dirs::home_dir() else { return LocalAuth::default() };
    LocalAuth { keys: keys_in(&home.join(".ssh")), agents: agents(&home) }
}

fn kind_of(algo: &str) -> String {
    match algo {
        "ssh-ed25519" => "ED25519".into(),
        "ssh-rsa" => "RSA".into(),
        "sk-ssh-ed25519@openssh.com" => "ED25519-SK".into(),
        "sk-ecdsa-sha2-nistp256@openssh.com" => "ECDSA-SK".into(),
        a if a.starts_with("ecdsa-") => "ECDSA".into(),
        a => a.trim_start_matches("ssh-").to_uppercase(),
    }
}

/// Key pairs in ~/.ssh: a `.pub` file with its private key next to it.
pub fn keys_in(dir: &Path) -> Vec<LocalKey> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut keys: Vec<LocalKey> = entries
        .flatten()
        .filter_map(|e| {
            let pub_path = e.path();
            if pub_path.extension()? != "pub" {
                return None;
            }
            let private = pub_path.with_extension("");
            if !private.is_file() {
                return None;
            }
            let text = std::fs::read_to_string(&pub_path).ok()?;
            let mut parts = text.split_whitespace();
            let algo = parts.next()?;
            let _blob = parts.next()?;
            let comment = parts.collect::<Vec<_>>().join(" ");
            let name = private.file_name()?.to_string_lossy().into_owned();
            Some(LocalKey { path: format!("~/.ssh/{name}"), name, kind: kind_of(algo), comment })
        })
        .collect();
    // The usual default first, then by name.
    keys.sort_by_key(|k| (k.name != "id_ed25519", k.name != "id_rsa", k.name.clone()));
    keys
}

/// How many keys an agent holds: Some(n) when it answered, None when it didn't.
fn agent_keys(socket: Option<&Path>) -> Option<usize> {
    let mut cmd = Command::new(crate::ssh::program("ssh-add"));
    cmd.arg("-l").stdin(Stdio::null()).stderr(Stdio::null());
    if let Some(s) = socket {
        cmd.env("SSH_AUTH_SOCK", s);
    }
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    match out.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.trim().is_empty()).count()),
        Some(1) => Some(0),
        _ => None,
    }
}

fn agents(home: &Path) -> Vec<KeyAgent> {
    let mut out = Vec::new();
    if cfg!(windows) || std::env::var_os("SSH_AUTH_SOCK").is_some() {
        if let Some(n) = agent_keys(None) {
            out.push(KeyAgent { name: "ssh-agent".into(), socket: None, keys: Some(n) });
        }
    }
    // Password managers that hold SSH keys and approve each use with Touch ID or Windows Hello.
    let candidates: [(&str, PathBuf); 5] = [
        ("1Password", home.join("Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock")),
        ("1Password", home.join(".1password/agent.sock")),
        ("Bitwarden", home.join(".bitwarden-ssh-agent.sock")),
        ("Bitwarden", home.join("snap/bitwarden/current/.bitwarden-ssh-agent.sock")),
        ("Bitwarden", home.join(".var/app/com.bitwarden.desktop/data/.bitwarden-ssh-agent.sock")),
    ];
    for (name, sock) in candidates {
        if sock.exists() && !out.iter().any(|a: &KeyAgent| a.name == name) {
            out.push(KeyAgent { name: name.into(), socket: Some(sock.display().to_string()), keys: agent_keys(Some(&sock)) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_key_pairs_only() {
        let dir = std::env::temp_dir().join(format!("sshee-keys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("id_ed25519"), "private").unwrap();
        std::fs::write(dir.join("id_ed25519.pub"), "ssh-ed25519 AAAAC3Nz me@laptop\n").unwrap();
        std::fs::write(dir.join("work.pub"), "ssh-rsa AAAAB3 orphan\n").unwrap();
        std::fs::write(dir.join("known_hosts"), "").unwrap();
        let keys = keys_in(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].kind, "ED25519");
        assert_eq!(keys[0].comment, "me@laptop");
        assert_eq!(keys[0].path, "~/.ssh/id_ed25519");
    }
}
