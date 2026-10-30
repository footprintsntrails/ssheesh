//! Server identity (host keys), with a clear question instead of ssh's
//! yes/no prompt in the terminal.
//!
//! Before the first terminal to a server opens, we look the server up in
//! known_hosts. If it isn't there, ssh itself fetches the key the server
//! presents (through the user's own config, so ProxyJump works) into a
//! scratch file, and the app shows its fingerprint. Trusting it copies
//! exactly those lines into ~/.ssh/known_hosts. A key that changed is caught
//! by ssh when connecting; then we show the old and new fingerprints.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;

use crate::ssh::{common_args, hide_console, ssh_program, SshEnv, SshTarget};
use crate::OpError;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    /// "ED25519", "ECDSA", "RSA".
    pub kind: String,
    /// "SHA256:…"
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HostKeyCheck {
    /// In known_hosts, or the user's config says not to check.
    Known,
    /// First visit: these are the keys the server presented.
    Unknown { fingerprints: Vec<Fingerprint> },
    /// The key differs from the one saved before.
    Changed { old: Vec<Fingerprint>, new: Vec<Fingerprint> },
    /// Couldn't ask the server (offline, wrong address). Connecting will say why.
    Unreachable { message: String },
}

/// Keys fetched for a pending question, by host id, so "Trust" saves exactly
/// what was shown.
#[derive(Default)]
pub struct PendingKeys(Mutex<HashMap<String, Vec<String>>>);

/// What `ssh -G` resolves for a destination.
struct Resolved {
    /// The name known_hosts uses: "host" or "[host]:port".
    spec: String,
    files: Vec<PathBuf>,
    strict: String,
}

fn resolve(t: &SshTarget) -> Result<Resolved, OpError> {
    let mut cmd = Command::new(ssh_program());
    cmd.args(common_args(t, &SshEnv::default())).arg("-G").arg("--").arg(&t.destination);
    cmd.stdin(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().map_err(crate::ssh::spawn_error)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let get = |key: &str| text.lines().find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_string()));
    let host = get("hostkeyalias ").or_else(|| get("hostname ")).unwrap_or_else(|| t.destination.clone());
    let port = get("port ").and_then(|p| p.parse::<u16>().ok()).unwrap_or(22);
    let spec = if port == 22 { host } else { format!("[{host}]:{port}") };
    let files = get("userknownhostsfile ")
        .map(|v| v.split_whitespace().map(expand_home).collect())
        .unwrap_or_else(|| vec![default_known_hosts()]);
    let strict = get("stricthostkeychecking ").unwrap_or_else(|| "ask".into());
    Ok(Resolved { spec, files, strict })
}

fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(p),
    }
}

fn default_known_hosts() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".ssh").join("known_hosts")
}

fn keygen(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("ssh-keygen");
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Fingerprints listed by `ssh-keygen -l`: "256 SHA256:abc… comment (ED25519)".
fn parse_fingerprints(text: &str) -> Vec<Fingerprint> {
    let mut out: Vec<Fingerprint> = text
        .lines()
        .filter_map(|l| {
            let sha256 = l.split_whitespace().find(|w| w.starts_with("SHA256:"))?.to_string();
            // "-l -f" ends with "(ED25519)"; "-l -F" names the type as a word.
            let kind = l
                .rsplit_once('(')
                .map(|(_, k)| k.trim_end_matches(')').to_string())
                .or_else(|| l.split_whitespace().find(|w| ["ED25519", "ECDSA", "RSA", "DSA"].contains(w)).map(str::to_string))
                .unwrap_or_default();
            Some(Fingerprint { kind, sha256 })
        })
        .collect();
    out.dedup();
    out
}

fn known_fingerprints(r: &Resolved) -> Vec<Fingerprint> {
    r.files
        .iter()
        .filter(|f| f.exists())
        .filter_map(|f| keygen(&["-l", "-F", &r.spec, "-f", &f.display().to_string()]))
        .flat_map(|t| parse_fingerprints(&t))
        .collect()
}

/// Asks the server for its keys through ssh, with the user's config, and
/// returns the known_hosts lines ssh wrote for them.
fn fetch_keys(t: &SshTarget, scratch: &Path) -> Result<Vec<String>, OpError> {
    let _ = std::fs::remove_file(scratch);
    let mut cmd = Command::new(ssh_program());
    // ssh keeps the first value it sees for an option, so these go first:
    // the key must land in the scratch file, never in your known_hosts.
    cmd.args(["-o", "BatchMode=yes", "-o", "PreferredAuthentications=none", "-o", "ControlPath=none"])
        .args(["-o", "StrictHostKeyChecking=accept-new", "-o", "GlobalKnownHostsFile=/dev/null"])
        .arg("-o")
        .arg(format!("UserKnownHostsFile={}", scratch.display()))
        .args(common_args(t, &SshEnv::default()))
        .args(["-T", "--"])
        .arg(&t.destination)
        .arg("true");
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    hide_console(&mut cmd);
    let out = cmd.output().map_err(crate::ssh::spawn_error)?;
    let lines: Vec<String> = std::fs::read_to_string(scratch)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    let _ = std::fs::remove_file(scratch);
    if lines.is_empty() {
        return Err(OpError::from_ssh(out.status.code(), &String::from_utf8_lossy(&out.stderr)));
    }
    Ok(lines)
}

fn fingerprints_of(lines: &[String], scratch: &Path) -> Vec<Fingerprint> {
    if std::fs::write(scratch, lines.join("\n") + "\n").is_err() {
        return Vec::new();
    }
    let fps = keygen(&["-l", "-f", &scratch.display().to_string()]).map(|t| parse_fingerprints(&t)).unwrap_or_default();
    let _ = std::fs::remove_file(scratch);
    fps
}

fn scratch_file(host_id: &str) -> PathBuf {
    let clean: String = host_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    std::env::temp_dir().join(format!("ssheesh-hostkey-{}-{clean}", std::process::id()))
}

impl PendingKeys {
    /// Is this server's key known? If not, fetches it to show you.
    /// `changed` is set after ssh refused the saved key.
    pub fn check(&self, host_id: &str, t: &SshTarget, changed: bool) -> Result<HostKeyCheck, OpError> {
        let r = resolve(t)?;
        if matches!(r.strict.as_str(), "false" | "no" | "off") {
            return Ok(HostKeyCheck::Known);
        }
        let old = known_fingerprints(&r);
        if !old.is_empty() && !changed {
            return Ok(HostKeyCheck::Known);
        }
        let scratch = scratch_file(host_id);
        let lines = match fetch_keys(t, &scratch) {
            Ok(l) => l,
            Err(e) => return Ok(HostKeyCheck::Unreachable { message: e.message }),
        };
        let new = fingerprints_of(&lines, &scratch);
        self.0.lock().unwrap().insert(host_id.to_string(), lines);
        Ok(if old.is_empty() { HostKeyCheck::Unknown { fingerprints: new } } else { HostKeyCheck::Changed { old, new } })
    }

    /// Saves the keys you were shown to ~/.ssh/known_hosts, replacing the
    /// old entry for this server if its key changed.
    pub fn trust(&self, host_id: &str, t: &SshTarget) -> Result<(), OpError> {
        let lines = self
            .0
            .lock()
            .unwrap()
            .remove(host_id)
            .ok_or_else(|| OpError::failed("Check the server's fingerprint again before trusting it."))?;
        let r = resolve(t)?;
        let file = r.files.first().cloned().unwrap_or_else(default_known_hosts);
        if !known_fingerprints(&r).is_empty() {
            for f in r.files.iter().filter(|f| f.exists()) {
                keygen(&["-R", &r.spec, "-f", &f.display().to_string()]);
            }
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&(lines.join("\n") + "\n"));
        std::fs::write(&file, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_keygen_fingerprints() {
        let out = "256 SHA256:abcDEF123 root@web (ED25519)\n3072 SHA256:xyz root@web (RSA)\n";
        let fps = parse_fingerprints(out);
        assert_eq!(fps.len(), 2);
        assert_eq!(fps[0], Fingerprint { kind: "ED25519".into(), sha256: "SHA256:abcDEF123".into() });
        assert_eq!(fps[1].kind, "RSA");
    }
}
