//! The servers in the sidebar: sshee-core's list (the ssh config's hosts and the ones added in
//! the app) with the labels and looks 0.10 kept for them.
//!
//! Preview has no list of its own yet (adding a server is a screen still to build), so it
//! reads 0.10's: `hosts.json` in 0.10's data folder, only ever read, and the labels in its web
//! view's storage, read from a copy by sshee-app's import. Neither is written to.

use std::path::{Path, PathBuf};

use sshee_app::legacy::{Places, APP_IDENTIFIER};
use sshee_app::model::Labels;
use sshee_app::workbench::Server;
use sshee_core::{Host, HostStore};

/// 0.10's data folder (Tauri's `app_data_dir`), where its `hosts.json` is.
fn data_dir_of_0_10() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_IDENTIFIER))
}

/// Every server sshee-core lists: `ssh_config`'s hosts (`None`: `~/.ssh/config`), then the
/// ones 0.10 saved. Reads files only.
pub fn list(ssh_config: Option<&Path>) -> Vec<Host> {
    // A folder that isn't there lists nothing saved; nothing is created.
    let mut store = HostStore::new(&data_dir_of_0_10().unwrap_or_default());
    if let Some(config) = ssh_config {
        store = store.with_ssh_config(Some(config.to_owned()));
    }
    store.list()
}

/// `user@host`, and `:port` when it isn't 22 (0.10's `hostAddress`).
fn address(h: &Host) -> String {
    let user = h.user.as_deref().map(|u| format!("{u}@")).unwrap_or_default();
    let port = h.port.filter(|p| *p != 22).map(|p| format!(":{p}")).unwrap_or_default();
    format!("{user}{}{port}", h.hostname)
}

/// The sidebar's servers: `hosts` with 0.10's labels. A host hidden there stays hidden; an
/// icon picked by hand wins over a guessed one.
pub fn labelled(hosts: &[Host], labels: &Labels) -> Vec<Server> {
    hosts
        .iter()
        .filter(|h| !labels.hidden.contains(&h.id))
        .map(|h| {
            let mut server = Server::host(&h.id, &h.name, &address(h));
            server.env = labels.env.get(&h.id).cloned();
            let picked = labels.looks.get(&h.id).map(|l| l.icon.as_str());
            let guessed = labels.auto_looks.get(&h.id).map(|l| l.icon.as_str());
            if let Some(icon) = picked.or(guessed).filter(|i| !i.is_empty()) {
                server.icon = icon.to_owned();
            }
            server
        })
        .collect()
}

/// What 0.10 left in its web view's storage, and what the import made of it when a storage
/// was found: copied into Preview's own folder to be read (the copy can hold what was typed
/// into snippets, so it is private and deleted when read). Slow enough for a thread of its
/// own. The labels and the settings both come from this one reading.
pub fn import_of_0_10() -> (sshee_app::legacy::Found, Option<sshee_app::import::Imported>) {
    let scratch = sshee_core::app_identity::current().dir().map(|d| d.join("import-scratch"));
    let places = Places { scratch, ..Places::this_computer() };
    let found = sshee_app::legacy::find(&places);
    let options = sshee_app::import::ImportOptions { tor_on: crate::settings::tor_left_on_in_0_10(), ..Default::default() };
    let imported = found.best().is_some().then(|| sshee_app::import::import_found(&found, &options));
    (found, imported)
}

/// Whether 0.10 ever ran on this computer: its data folder is there (its server list lives
/// in it). Without one, 1.0 starting with no settings is a new install, not a doubt.
pub fn ran_0_10_here() -> bool {
    data_dir_of_0_10().is_some_and(|dir| dir.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::model::{AutoLook, EnvLabel, HostLook};
    use sshee_core::HostSource;

    fn host(id: &str, name: &str, hostname: &str, user: Option<&str>, port: Option<u16>) -> Host {
        Host {
            id: id.into(),
            name: name.into(),
            hostname: hostname.into(),
            user: user.map(str::to_owned),
            port,
            identity_file: None,
            source: HostSource::Saved,
            remembered_password: false,
            identity_agent: None,
        }
    }

    #[test]
    fn a_server_is_named_and_addressed_as_0_10_showed_it() {
        let hosts = [
            host("saved:1", "web-7731", "203.0.113.17", Some("deploy"), None),
            host("saved:2", "db-main", "203.0.113.31", Some("postgres"), Some(2222)),
            host("cfg:backups", "backups", "backups", None, Some(22)),
        ];
        let servers = labelled(&hosts, &Labels::default());
        let shown: Vec<(&str, &str, String)> = servers.iter().map(|s| (s.name.as_str(), s.address.as_str(), s.key())).collect();
        assert_eq!(
            shown,
            [
                ("web-7731", "deploy@203.0.113.17", "host:saved:1".to_owned()),
                ("db-main", "postgres@203.0.113.31:2222", "host:saved:2".to_owned()),
                ("backups", "backups", "host:cfg:backups".to_owned()),
            ]
        );
        assert!(servers.iter().all(|s| s.env.is_none() && s.icon == "server"));
    }

    #[test]
    fn labels_looks_and_hidden_hosts_come_from_0_10() {
        let hosts = [
            host("saved:1", "web-7731", "203.0.113.17", None, None),
            host("saved:2", "db-main", "203.0.113.31", None, None),
            host("cfg:old", "old-box", "old-box", None, None),
        ];
        let mut labels = Labels::default();
        labels.env.insert("saved:1".into(), EnvLabel::Prod);
        labels.looks.insert("saved:1".into(), HostLook { icon: "globe".into(), color: "#2389e2".into(), ..HostLook::default() });
        labels.auto_looks.insert("saved:1".into(), AutoLook { icon: "box".into(), ..AutoLook::default() });
        labels.auto_looks.insert("saved:2".into(), AutoLook { icon: "database".into(), ..AutoLook::default() });
        labels.hidden.push("cfg:old".into());
        let servers = labelled(&hosts, &labels);
        assert_eq!(servers.len(), 2);
        assert!(servers[0].is_prod());
        // Picked by hand wins over guessed.
        assert_eq!((servers[0].icon.as_str(), servers[1].icon.as_str()), ("globe", "database"));
        assert!(!servers[1].is_prod());
    }
}
