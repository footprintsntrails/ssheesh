//! Servers saved in XPipe, read from its vault folder to import them.
//!
//! XPipe keeps each connection in `~/.xpipe/storage/stores/<id>/`: its name in
//! `entry.json`, the connection in `store.json`. Either file is plain JSON, or
//! `{"secrets": …}` when encrypted, which only XPipe can open. Passwords are
//! always encrypted, so they don't come along: you're asked on first connect.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// XPipe's own machine: a connection that goes "through" it goes directly.
const LOCAL_ID: &str = "f0ec68aa-63f5-405c-b178-9a4454556d6b";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XpipeServer {
    pub name: String,
    /// `user@host:port`, as the Add server form takes it.
    pub address: String,
    pub identity_file: Option<String>,
    /// XPipe has a password for it, which stays there.
    pub had_password: bool,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XpipeScan {
    /// An XPipe vault is there at all.
    pub found: bool,
    pub servers: Vec<XpipeServer>,
    /// Encrypted entries only XPipe can read.
    pub locked: usize,
    /// Reached through another server (a jump host), which SSHeesh can't set yet.
    pub through_jump: usize,
}

/// XPipe's vaults on this computer: the release's and the test build's.
pub fn vaults() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    [".xpipe", ".xpipe-ptb"].iter().map(|d| home.join(d).join("storage")).collect()
}

/// Every plain SSH connection in the vaults, in name order. Ones from
/// ~/.ssh/config are left out: SSHeesh reads that file itself.
pub fn scan(vaults: &[PathBuf]) -> XpipeScan {
    let mut out = XpipeScan::default();
    for vault in vaults {
        let stores = vault.join("stores");
        let Ok(dirs) = std::fs::read_dir(&stores) else { continue };
        out.found = true;
        for dir in dirs.flatten() {
            match read_entry(&stores, &dir.path()) {
                Entry::Server(s) if !out.servers.contains(&s) => out.servers.push(s),
                Entry::Locked => out.locked += 1,
                Entry::ThroughJump => out.through_jump += 1,
                _ => {}
            }
        }
    }
    out.servers.sort_by_key(|s| s.name.to_lowercase());
    out
}

enum Entry {
    Server(XpipeServer),
    Locked,
    ThroughJump,
    /// Not an SSH server (a container, a script, the local machine…), or unreadable.
    Other,
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// `{"secrets": …}` and nothing else: XPipe's encrypted form.
fn locked(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.len() == 1 && o.contains_key("secrets"))
}

/// A value XPipe may wrap as `{"value": …}` or `{"selected": …}`.
fn text(v: Option<&Value>) -> Option<String> {
    let mut v = v?;
    for _ in 0..4 {
        match v {
            Value::Object(o) => v = o.get("value").or_else(|| o.get("selected"))?,
            Value::String(s) => return Some(s.trim().to_string()).filter(|s| !s.is_empty()),
            Value::Number(n) => return Some(n.to_string()),
            _ => return None,
        }
    }
    None
}

fn read_entry(stores: &Path, dir: &Path) -> Entry {
    let (Some(entry), Some(store)) = (read_json(&dir.join("entry.json")), read_json(&dir.join("store.json"))) else {
        return Entry::Other;
    };
    if locked(&entry) || locked(&store) {
        return Entry::Locked;
    }
    if entry.get("template").and_then(Value::as_bool) == Some(true) || store.get("type").and_then(Value::as_str) != Some("ssh") {
        return Entry::Other;
    }
    let (Some(name), Some(host)) = (text(entry.get("name")), text(store.get("host"))) else {
        return Entry::Other;
    };
    if text(store.get("gateway")).is_some_and(|g| g != LOCAL_ID) {
        return Entry::ThroughJump;
    }
    let port = text(store.get("port")).and_then(|p| p.parse::<u16>().ok()).filter(|&p| p != 22);
    let identity = identity(stores, store.get("identity"));
    let user = identity.as_ref().and_then(|i| text(i.get("username")));
    let identity_file = identity.as_ref().and_then(|i| i.get("sshIdentity")).filter(|k| text(k.get("type")).as_deref() == Some("file")).and_then(|k| text(k.get("file")));
    let had_password = identity.as_ref().and_then(|i| i.get("password")).is_some_and(|p| !p.is_null());
    // A bare IPv6 address needs brackets before a port.
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host };
    let address = format!(
        "{}{host}{}",
        user.map(|u| format!("{u}@")).unwrap_or_default(),
        port.map(|p| format!(":{p}")).unwrap_or_default()
    );
    Entry::Server(XpipeServer { name, address, identity_file, had_password })
}

/// The user name and key of a connection: written in it (`inPlace`), or an
/// identity of its own in the vault (`ref`).
fn identity(stores: &Path, v: Option<&Value>) -> Option<Value> {
    let v = v?;
    match v.get("type").and_then(Value::as_str) {
        Some("ref") => {
            let id = text(v.get("ref"))?;
            // An id, never a path: nothing outside the vault is read.
            if id.contains(['/', '\\', '.']) {
                return None;
            }
            let store = read_json(&stores.join(id).join("store.json"))?;
            if locked(&store) {
                return None;
            }
            find_identity(&store, 0)
        }
        _ => find_identity(v, 0),
    }
}

/// The object that holds `username`, a few levels in at most.
fn find_identity(v: &Value, depth: usize) -> Option<Value> {
    if depth > 4 {
        return None;
    }
    match v {
        Value::Object(o) if o.contains_key("username") => Some(v.clone()),
        Value::Object(o) => o.values().find_map(|c| find_identity(c, depth + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vault(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sshee-xpipe-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("stores")).unwrap();
        dir
    }

    fn put(vault: &Path, id: &str, entry: Value, store: Value) {
        let d = vault.join("stores").join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("entry.json"), entry.to_string()).unwrap();
        std::fs::write(d.join("store.json"), store.to_string()).unwrap();
    }

    #[test]
    fn reads_plain_ssh_connections() {
        let v = vault("plain");
        put(
            &v,
            "a",
            json!({"uuid": "a", "name": "web-1"}),
            json!({"type": "ssh", "host": {"value": "203.0.113.10", "available": ["203.0.113.10"]}, "port": 2222, "gateway": null,
                   "identity": {"type": "inPlace", "identityStore": {"type": "localIdentity", "username": "deploy",
                     "sshIdentity": {"type": "file", "file": "~/.ssh/web"}, "password": {"secrets": []}}}}),
        );
        put(&v, "b", json!({"uuid": "b", "name": "db"}), json!({"type": "ssh", "host": "198.51.100.7", "port": 22, "gateway": LOCAL_ID}));
        put(&v, "id", json!({"uuid": "id", "name": "me"}), json!({"type": "localIdentity", "username": {"value": "root"}, "sshIdentity": {"type": "agent"}}));
        put(&v, "c", json!({"uuid": "c", "name": "v6"}), json!({"type": "ssh", "host": "2001:db8::1", "port": null, "identity": {"type": "ref", "ref": "id"}}));
        let scan = scan(std::slice::from_ref(&v));
        assert!(scan.found);
        assert_eq!(
            scan.servers,
            vec![
                XpipeServer { name: "db".into(), address: "198.51.100.7".into(), identity_file: None, had_password: false },
                XpipeServer { name: "v6".into(), address: "root@[2001:db8::1]".into(), identity_file: None, had_password: false },
                XpipeServer { name: "web-1".into(), address: "deploy@203.0.113.10:2222".into(), identity_file: Some("~/.ssh/web".into()), had_password: true },
            ]
        );
        for s in &scan.servers {
            crate::hosts::parse_address(&s.address).unwrap();
        }
        let _ = std::fs::remove_dir_all(&v);
    }

    #[test]
    fn counts_what_it_cant_bring() {
        let v = vault("skip");
        put(&v, "lock", json!({"secrets": [{"x": 1}]}), json!({"secrets": []}));
        put(&v, "lock2", json!({"uuid": "lock2", "name": "x"}), json!({"secrets": []}));
        put(&v, "jump", json!({"uuid": "jump", "name": "inner"}), json!({"type": "ssh", "host": "10.0.0.5", "gateway": "a"}));
        put(&v, "cfg", json!({"uuid": "cfg", "name": "from-config"}), json!({"type": "sshConfigHost", "host": "x"}));
        put(&v, "docker", json!({"uuid": "docker", "name": "box"}), json!({"type": "docker", "containerName": "x"}));
        put(&v, "tpl", json!({"uuid": "tpl", "name": "t", "template": true}), json!({"type": "ssh", "host": "x"}));
        // A ref that tries to leave the vault reads nothing.
        put(&v, "esc", json!({"uuid": "esc", "name": "esc"}), json!({"type": "ssh", "host": "x", "identity": {"type": "ref", "ref": "../../etc"}}));
        let scan = scan(std::slice::from_ref(&v));
        assert_eq!((scan.locked, scan.through_jump), (2, 1));
        assert_eq!(scan.servers.iter().map(|s| s.address.as_str()).collect::<Vec<_>>(), ["x"]);
        let _ = std::fs::remove_dir_all(&v);
    }

    #[test]
    fn no_vault_is_no_vault() {
        let scan = scan(&[std::env::temp_dir().join("sshee-xpipe-none-here")]);
        assert!(!scan.found && scan.servers.is_empty());
    }
}
