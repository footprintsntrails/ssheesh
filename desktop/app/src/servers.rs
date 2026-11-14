//! This copy's own list of servers: the ones added here (`hosts.json` in the copy's own
//! folder, `~/.sshee-preview`), the ones in `~/.ssh/config` you said yes to, and how each
//! looks and what it's called (`looks.json` beside it). Saved passwords go to the copy's own
//! keychain name (`AppIdentity::keychain_service`), never into a file.
//!
//! 0.10's list is never written and never used as this copy's own. It is read to offer
//! "Bring over your servers from 0.10": each server you tick is added here under a new id,
//! with the label, icon and colour 0.10 kept for it. 0.10's saved passwords stay in 0.10's
//! keychain; a server that needs one asks the first time. XPipe's vault and `~/.ssh/config`
//! are read the same way: read, offered, never changed.
//!
//! Everything here reads files or asks the keychain, so the window never calls it: the
//! backend runs [`ServerStore::handle`] on a thread and sends the answer through `Remote`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use sshee_app::labels::{Listed, Looks};
use sshee_app::legacy::APP_IDENTIFIER;
use sshee_app::model::Labels;
use sshee_app::servers::{host_id, imported_note, Answer, Ask, Candidate, NewServer, Source, Sources};
use sshee_app::workbench::{Server, ServerKey};
use sshee_core::secrets::{PasswordStore, Secret};
use sshee_core::{AppIdentity, Host, HostSource, HostStore, NewHost};

/// How each server looks and what it's called, in the copy's folder.
const LOOKS_FILE: &str = "looks.json";

/// Where saved passwords go. The program's is the system keychain under this copy's own name
/// ([`PasswordStore`]); a test's is a list in memory.
pub trait Keychain: Send + Sync {
    /// Keeps a password for this run, and in the keychain when `remember` is set.
    fn set(&self, host_id: &str, password: Secret, remember: bool) -> Result<(), String>;
    /// The password for a host, when there is one (`remembered`: the list says the keychain
    /// has it).
    fn get(&self, host_id: &str, remembered: bool) -> Option<Secret>;
    fn forget(&self, host_id: &str);
}

impl Keychain for PasswordStore {
    fn set(&self, host_id: &str, password: Secret, remember: bool) -> Result<(), String> {
        PasswordStore::set(self, host_id, password, remember)
    }

    fn get(&self, host_id: &str, remembered: bool) -> Option<Secret> {
        PasswordStore::get(self, host_id, remembered)
    }

    fn forget(&self, host_id: &str) {
        PasswordStore::forget(self, host_id);
    }
}

/// The folders and files the store reads, and the one folder it writes.
pub struct Places {
    /// This copy's own folder: the only place written to.
    pub dir: PathBuf,
    /// The ssh config to list servers from; `None`: `~/.ssh/config`.
    pub ssh_config: Option<PathBuf>,
    /// 0.10's data folder, where its `hosts.json` is. Only ever read.
    pub old_dir: Option<PathBuf>,
    /// XPipe's vaults. Only ever read.
    pub xpipe: Vec<PathBuf>,
    /// The labels 0.10 kept (slow: it copies a database to read it).
    pub old_labels: Box<dyn Fn() -> Option<Labels> + Send>,
}

impl Places {
    /// The places of the copy `identity` is, on this computer. A copy with no folder of its
    /// own (no home folder) keeps its list for this run only, in a folder that goes with it.
    pub fn of(identity: &AppIdentity, ssh_config: Option<PathBuf>) -> Places {
        let dir = identity.dir().map(Path::to_owned).unwrap_or_else(|| std::env::temp_dir().join(format!("sshee-preview-{}", std::process::id())));
        Places { dir, ssh_config, old_dir: dirs::data_dir().map(|d| d.join(APP_IDENTIFIER)), xpipe: sshee_core::xpipe::vaults(), old_labels: Box::new(labels_of_0_10) }
    }
}

/// What the window shows of the list.
pub struct Shown {
    pub servers: Vec<Server>,
    /// Each server's own colour, by its key in the sidebar.
    pub colors: IndexMap<ServerKey, String>,
    /// Servers in the ssh config not said yes or no to yet.
    pub config_new: usize,
    /// Servers 0.10 saved that aren't here yet.
    pub old_new: usize,
    /// Every host, for the sessions.
    pub hosts: Vec<Host>,
}

/// The list, its looks and its passwords.
pub struct ServerStore {
    places: Places,
    hosts: HostStore,
    looks: Looks,
    /// `looks.json` was there and didn't read: it's set aside before the first save.
    looks_unread: bool,
    keychain: Arc<dyn Keychain>,
}

/// `user@host`, and `:port` when it isn't 22 (0.10's `hostAddress`).
fn address(h: &Host) -> String {
    let user = h.user.as_deref().map(|u| format!("{u}@")).unwrap_or_default();
    let port = h.port.filter(|p| *p != 22).map(|p| format!(":{p}")).unwrap_or_default();
    format!("{user}{}{port}", h.hostname)
}

/// [`address`] as the list's "add" reads it back: an IPv6 host in brackets, so its colons
/// aren't taken for a port's.
fn address_to_add(h: &Host) -> String {
    if !h.hostname.contains(':') {
        return address(h);
    }
    let user = h.user.as_deref().map(|u| format!("{u}@")).unwrap_or_default();
    let port = h.port.filter(|p| *p != 22).map(|p| format!(":{p}")).unwrap_or_default();
    format!("{user}[{}]{port}", h.hostname)
}

fn listed(h: &Host) -> Listed {
    Listed { id: h.id.clone(), name: h.name.clone(), address: address(h), from_config: h.source == HostSource::SshConfig }
}

/// Writes a file whole or not at all, readable by you alone: names of servers are the
/// user's own business.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

impl ServerStore {
    /// Opens the list in `places.dir`. Nothing is created until something is saved.
    pub fn open(places: Places, keychain: Arc<dyn Keychain>) -> ServerStore {
        let mut hosts = HostStore::new(&places.dir);
        if let Some(config) = &places.ssh_config {
            hosts = hosts.with_ssh_config(Some(config.clone()));
        }
        let (looks, looks_unread) = match std::fs::read_to_string(places.dir.join(LOOKS_FILE)) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(looks) => (looks, false),
                Err(_) => (Looks::default(), true),
            },
            Err(_) => (Looks::default(), false),
        };
        ServerStore { places, hosts, looks, looks_unread, keychain }
    }

    /// Every host: the ssh config's and the ones added here.
    pub fn hosts(&mut self) -> Vec<Host> {
        self.hosts.list()
    }

    /// What the window shows now.
    pub fn shown(&mut self) -> Shown {
        let hosts = self.hosts();
        let list: Vec<Listed> = hosts.iter().map(listed).collect();
        Shown {
            servers: self.looks.servers(&list),
            colors: self.looks.colors(&list),
            config_new: self.looks.undecided(&list).len(),
            old_new: self.old_candidates(&hosts).len(),
            hosts,
        }
    }

    /// The password to sign in to a host with, when one was saved or typed this run. Asks
    /// the keychain: call it from a session's own thread.
    pub fn password(&mut self, host_id: &str) -> Option<Secret> {
        let remembered = self.hosts.get(host_id).is_some_and(|h| h.remembered_password);
        self.keychain.get(host_id, remembered)
    }

    fn save_looks(&mut self) -> Result<(), String> {
        let path = self.places.dir.join(LOOKS_FILE);
        if std::mem::take(&mut self.looks_unread) {
            // What didn't read is kept beside it, not written over.
            let _ = std::fs::rename(&path, path.with_extension("json.unread"));
        }
        let json = serde_json::to_vec_pretty(&self.looks).map_err(|e| e.to_string())?;
        write_private(&path, &json).map_err(|e| format!("Couldn't save how your servers look: {e}"))
    }

    /// 0.10's saved servers. Its ssh config hosts are in this list already.
    fn old_hosts(&self) -> Vec<Host> {
        match &self.places.old_dir {
            // A list is only read: `HostStore` writes on add and remove, never here.
            Some(dir) => HostStore::new(dir).with_ssh_config(None).list(),
            None => Vec::new(),
        }
    }

    /// 0.10's servers that aren't here yet (by address).
    fn old_candidates(&self, mine: &[Host]) -> Vec<Candidate> {
        self.old_hosts()
            .iter()
            .filter(|h| !mine.iter().any(|m| address(m) == address(h)))
            .map(|h| Candidate { id: h.id.clone(), name: h.name.clone(), address: address(h), identity_file: h.identity_file.clone(), had_password: h.remembered_password, picked: true })
            .collect()
    }

    fn sources(&mut self) -> Sources {
        let mine = self.hosts();
        let list: Vec<Listed> = mine.iter().map(listed).collect();
        let config = list
            .iter()
            .filter(|h| h.from_config)
            .map(|h| Candidate { id: h.id.clone(), name: h.name.clone(), address: h.address.clone(), picked: self.looks.shown(h) || !self.looks.labels.config_seen.contains(&h.id), ..Candidate::default() })
            .collect();
        let scan = sshee_core::xpipe::scan(&self.places.xpipe);
        let found = scan.servers.len();
        let xpipe: Vec<Candidate> = scan
            .servers
            .into_iter()
            .filter(|x| !mine.iter().any(|m| address(m) == x.address))
            .map(|x| Candidate { id: x.address.clone(), name: x.name, address: x.address, identity_file: x.identity_file, had_password: x.had_password, picked: true })
            .collect();
        Sources {
            config,
            config_new: self.looks.undecided(&list).len(),
            xpipe_here: found - xpipe.len(),
            xpipe,
            xpipe_locked: scan.locked,
            xpipe_through_jump: scan.through_jump,
            old: self.old_candidates(&mine),
        }
    }

    /// Adds one server to this copy's list.
    fn add(&mut self, new: NewServer, identity_agent: Option<String>) -> Result<(Host, Option<String>), String> {
        let password = new.password.as_ref().filter(|p| !p.is_empty()).map(|p| Secret::new(p.expose()));
        let remember = new.remember_password;
        let input = NewHost { name: new.name, address: new.address, identity_file: new.identity_file, password: password.clone(), remember_password: remember, identity_agent };
        let mut host = self.hosts.add(input)?;
        let mut warning = None;
        if let Some(password) = password {
            // The keychain refused: the server is added all the same, and its password works
            // until the app quits (0.10's `add_host`).
            if let Err(why) = self.keychain.set(&host.id, password, remember) {
                self.hosts.set_remembered_password(&host.id, false)?;
                host.remembered_password = false;
                warning = Some(why);
            }
        }
        Ok((host, warning))
    }

    fn import(&mut self, source: Source, picks: &[String], offered: &[String]) -> Result<String, String> {
        match source {
            Source::SshConfig => {
                self.looks.choose_config(offered, picks);
                self.save_looks()?;
                Ok(imported_note(source, picks.len(), &[], 0))
            }
            Source::Xpipe => {
                let (mut added, mut failed, mut with_password) = (0, Vec::new(), 0);
                for x in sshee_core::xpipe::scan(&self.places.xpipe).servers.into_iter().filter(|x| picks.contains(&x.address)) {
                    let new = NewServer { name: Some(x.name.clone()), address: x.address, identity_file: x.identity_file, ..NewServer::default() };
                    match self.add(new, None) {
                        Ok(_) => {
                            added += 1;
                            with_password += usize::from(x.had_password);
                        }
                        Err(_) => failed.push(x.name),
                    }
                }
                Ok(imported_note(source, added, &failed, with_password))
            }
            Source::Old => {
                let (mut moved, mut failed, mut with_password) = (Vec::new(), Vec::new(), 0);
                for old in self.old_hosts().into_iter().filter(|h| picks.contains(&h.id)) {
                    let new = NewServer { name: Some(old.name.clone()), address: address_to_add(&old), identity_file: old.identity_file.clone(), ..NewServer::default() };
                    match self.add(new, old.identity_agent.clone()) {
                        Ok((host, _)) => {
                            moved.push((old.id, host.id));
                            with_password += usize::from(old.remembered_password);
                        }
                        Err(_) => failed.push(old.name),
                    }
                }
                // Their labels, icons and colours, under the ids they have here.
                if !moved.is_empty() {
                    if let Some(labels) = (self.places.old_labels)() {
                        self.looks.carry(&labels, &moved);
                        self.save_looks()?;
                    }
                }
                Ok(imported_note(source, moved.len(), &failed, with_password))
            }
        }
    }

    /// Does what the window asked. Slow: files, the keychain.
    pub fn handle(&mut self, ask: Ask) -> Answer {
        let done = |saved: Result<(), String>| saved.map_or_else(Answer::Failed, |()| Answer::Changed);
        match ask {
            Ask::Sources => Answer::Sources(self.sources()),
            Ask::Add(new) => match self.add(new, None) {
                Ok((host, warning)) => Answer::Added { server: format!("host:{}", host.id), name: host.name, warning },
                Err(why) => Answer::Failed(why),
            },
            Ask::Import { source, picks, offered } => match self.import(source, &picks, &offered) {
                Ok(note) => Answer::Imported { source, note },
                Err(why) => Answer::Failed(why),
            },
            Ask::Rename { server, name } => {
                if let Some(host) = host_id(&server).and_then(|id| self.hosts.get(id)) {
                    self.looks.rename(&listed(&host), &name);
                }
                done(self.save_looks())
            }
            Ask::SetIcon { server, icon } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_icon(id, &icon);
                }
                done(self.save_looks())
            }
            Ask::SetColor { server, color } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_color(id, &color);
                }
                done(self.save_looks())
            }
            Ask::SetEnv { server, env } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_env(id, env);
                }
                done(self.save_looks())
            }
            Ask::Remove { server } => {
                let Some(host) = host_id(&server).and_then(|id| self.hosts.list().into_iter().find(|h| h.id == id)) else {
                    return Answer::Failed("That server isn't in your list anymore.".into());
                };
                if host.source == HostSource::SshConfig {
                    // It stays in the file; here it's hidden.
                    self.looks.hide(&host.id);
                } else {
                    self.keychain.forget(&host.id);
                    if let Err(why) = self.hosts.remove(&host.id) {
                        return Answer::Failed(why);
                    }
                    self.looks.forget(&host.id);
                }
                match self.save_looks() {
                    Ok(()) => Answer::Removed { server },
                    Err(why) => Answer::Failed(why),
                }
            }
        }
    }
}

/// The labels 0.10 kept, if its web view storage is found and reads: copied into this copy's
/// own folder to be read (the copy can hold what was typed into snippets, so it is private
/// and deleted when read). Slow enough for a thread of its own.
pub fn labels_of_0_10() -> Option<Labels> {
    import_of_0_10().1.map(|imported| imported.state.labels)
}

/// What 0.10 left in its web view's storage, and what the import made of it when a storage
/// was found: copied into this copy's own folder to be read (the copy can hold what was typed
/// into snippets, so it is private and deleted when read). Slow enough for a thread of its
/// own. The settings at a first start, and the labels of servers brought over, come from it.
pub fn import_of_0_10() -> (sshee_app::legacy::Found, Option<sshee_app::import::Imported>) {
    let scratch = sshee_core::app_identity::current().dir().map(|d| d.join("import-scratch"));
    let places = sshee_app::legacy::Places { scratch, ..sshee_app::legacy::Places::this_computer() };
    let found = sshee_app::legacy::find(&places);
    let options = sshee_app::import::ImportOptions { tor_on: crate::settings::tor_left_on_in_0_10(), ..Default::default() };
    let imported = found.best().is_some().then(|| sshee_app::import::import_found(&found, &options));
    (found, imported)
}

/// Whether 0.10 ever ran on this computer: its data folder is there (its server list lives
/// in it). Without one, 1.0 starting with no settings is a new install, not a doubt.
pub fn ran_0_10_here() -> bool {
    dirs::data_dir().map(|d| d.join(APP_IDENTIFIER)).is_some_and(|dir| dir.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::model::{EnvLabel, HostLook};
    use sshee_app::servers::Password;
    use std::sync::Mutex;

    /// A keychain in memory, which can be told to refuse.
    #[derive(Default)]
    struct FakeKeychain {
        kept: Mutex<Vec<(String, String, bool)>>,
        refuses: bool,
    }

    impl Keychain for FakeKeychain {
        fn set(&self, host_id: &str, password: Secret, remember: bool) -> Result<(), String> {
            if self.refuses {
                return Err("Couldn't save the password in your keychain (locked). It works until you quit SSHeesh.".into());
            }
            self.kept.lock().unwrap().push((host_id.to_owned(), password.expose().to_owned(), remember));
            Ok(())
        }

        fn get(&self, host_id: &str, _: bool) -> Option<Secret> {
            self.kept.lock().unwrap().iter().find(|(id, ..)| id == host_id).map(|(_, pw, _)| Secret::new(pw.clone()))
        }

        fn forget(&self, host_id: &str) {
            self.kept.lock().unwrap().retain(|(id, ..)| id != host_id);
        }
    }

    /// A made-up computer in a folder of this test's own: the copy's folder, an ssh config,
    /// 0.10's data folder and an XPipe vault.
    struct World {
        root: PathBuf,
        keychain: Arc<FakeKeychain>,
    }

    impl World {
        fn new(name: &str) -> World {
            let root = std::env::temp_dir().join(format!("sshee-preview-servers-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("old")).unwrap();
            std::fs::write(root.join("ssh_config"), "Host web-7731\n  HostName 203.0.113.17\n  User deploy\n\nHost old-box\n  HostName 203.0.113.99\n  Port 2200\n").unwrap();
            World { root, keychain: Arc::new(FakeKeychain::default()) }
        }

        fn refusing(name: &str) -> World {
            World { keychain: Arc::new(FakeKeychain { refuses: true, ..FakeKeychain::default() }), ..World::new(name) }
        }

        fn store(&self) -> ServerStore {
            // The copy's own folder comes from its identity, as in the program.
            let identity = AppIdentity::in_dir(self.root.join("copy"), "io.example.ssheesh.test");
            let places = Places {
                ssh_config: Some(self.root.join("ssh_config")),
                old_dir: Some(self.root.join("old")),
                xpipe: vec![self.root.join("xpipe")],
                old_labels: Box::new(|| {
                    let mut labels = Labels::default();
                    labels.env.insert("saved:old-db".into(), EnvLabel::Prod);
                    labels.looks.insert("saved:old-db".into(), HostLook { icon: "database".into(), color: "#0997b4".into(), ..HostLook::default() });
                    labels.config_seen.push("cfg:web-7731".into());
                    Some(labels)
                }),
                ..Places::of(&identity, None)
            };
            ServerStore::open(places, self.keychain.clone())
        }

        /// 0.10's own list, as its `hosts.json`.
        fn old_list(&self, json: &str) {
            std::fs::write(self.root.join("old").join("hosts.json"), json).unwrap();
        }

        fn copy_file(&self, name: &str) -> String {
            std::fs::read_to_string(self.root.join("copy").join(name)).unwrap_or_default()
        }

        fn names(&self, store: &mut ServerStore) -> Vec<String> {
            store.shown().servers.iter().map(|s| s.name.clone()).collect()
        }
    }

    const OLD_LIST: &str = r#"[
      {"id":"saved:old-db","name":"db-main","hostname":"203.0.113.31","user":"postgres","port":2222,"identityFile":null,"source":"saved","rememberedPassword":true},
      {"id":"saved:old-v6","name":"backups","hostname":"2001:db8::24","user":"backup","port":null,"identityFile":"~/.ssh/backup_key","source":"saved"}
    ]"#;

    fn by_hand(address: &str, password: Option<&str>, remember: bool) -> Ask {
        Ask::Add(NewServer { name: None, address: address.into(), identity_file: None, password: password.map(Password::new), remember_password: remember })
    }

    #[test]
    fn a_server_added_by_hand_is_saved_in_the_copys_own_folder_and_its_password_in_the_keychain() {
        let w = World::new("add");
        let mut store = w.store();
        assert!(w.names(&mut store).is_empty());
        let Answer::Added { server, name, warning } = store.handle(by_hand("root@203.0.113.40:2222", Some("made-up-password"), true)) else { panic!("not added") };
        assert_eq!((name.as_str(), warning), ("203.0.113.40", None));
        assert_eq!(w.names(&mut store), ["203.0.113.40"]);
        let id = host_id(&server).unwrap().to_owned();
        assert_eq!(*w.keychain.kept.lock().unwrap(), [(id.clone(), "made-up-password".to_owned(), true)]);
        assert_eq!(store.password(&id).map(|p| p.expose().to_owned()).as_deref(), Some("made-up-password"));
        // The list notes that the keychain has it; no file holds the password itself.
        let list = w.copy_file("hosts.json");
        assert!(list.contains("\"rememberedPassword\": true") && list.contains("203.0.113.40"), "{list}");
        for entry in std::fs::read_dir(w.root.join("copy")).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
            assert!(!text.contains("made-up-password"));
        }
        // It's there after a restart, and sessions connect as it says.
        let mut again = w.store();
        let host = again.hosts().into_iter().find(|h| h.id == id).unwrap();
        assert_eq!((host.hostname.as_str(), host.user.as_deref(), host.port), ("203.0.113.40", Some("root"), Some(2222)));
        // A wrong address is refused in sshee-core's own words, and nothing is added.
        assert_eq!(again.handle(by_hand("203.0.113.40:ssh", None, false)), Answer::Failed("\"ssh\" isn't a valid port".into()));
        assert_eq!(w.names(&mut again).len(), 1);
    }

    #[test]
    fn a_keychain_that_refuses_still_adds_the_server_and_says_so() {
        let w = World::refusing("refuse");
        let mut store = w.store();
        let Answer::Added { warning, server, .. } = store.handle(by_hand("203.0.113.41", Some("made-up-password"), true)) else { panic!("not added") };
        assert!(warning.unwrap().contains("keychain"));
        let host = store.hosts().into_iter().find(|h| Some(h.id.as_str()) == host_id(&server)).unwrap();
        assert!(!host.remembered_password);
        assert!(!w.copy_file("hosts.json").contains("made-up-password"));
    }

    #[test]
    fn servers_from_the_ssh_config_are_offered_and_show_once_picked() {
        let w = World::new("config");
        let mut store = w.store();
        let shown = store.shown();
        assert!(shown.servers.is_empty());
        assert_eq!(shown.config_new, 2);
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        let offered: Vec<(&str, &str, bool)> = sources.config.iter().map(|c| (c.name.as_str(), c.address.as_str(), c.picked)).collect();
        assert_eq!(offered, [("web-7731", "deploy@203.0.113.17", true), ("old-box", "203.0.113.99:2200", true)]);
        let ids: Vec<String> = sources.config.iter().map(|c| c.id.clone()).collect();
        let answer = store.handle(Ask::Import { source: Source::SshConfig, picks: ids[..1].to_vec(), offered: ids.clone() });
        assert_eq!(answer, Answer::Imported { source: Source::SshConfig, note: "1 server shows from ~/.ssh/config.".into() });
        assert_eq!(w.names(&mut store), ["web-7731"]);
        assert_eq!(store.shown().config_new, 0);
        // Hidden from its menu; the choice is kept in the copy's folder, the config untouched.
        store.handle(Ask::Import { source: Source::SshConfig, picks: ids.clone(), offered: ids });
        assert_eq!(store.handle(Ask::Remove { server: "host:cfg:old-box".into() }), Answer::Removed { server: "host:cfg:old-box".into() });
        assert_eq!(w.names(&mut w.store()), ["web-7731"]);
        assert!(std::fs::read_to_string(w.root.join("ssh_config")).unwrap().contains("Host old-box"));
        // A session to it goes by its alias, as the config says.
        let host = store.hosts().into_iter().find(|h| h.id == "cfg:web-7731").unwrap();
        assert_eq!(host.target().destination, "web-7731");
    }

    #[test]
    fn the_servers_of_0_10_are_brought_over_with_their_looks_and_its_list_is_never_written() {
        let w = World::new("old");
        w.old_list(OLD_LIST);
        let before = std::fs::read(w.root.join("old").join("hosts.json")).unwrap();
        let mut store = w.store();
        assert_eq!(store.shown().old_new, 2);
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        let offered: Vec<(&str, &str, bool)> = sources.old.iter().map(|c| (c.name.as_str(), c.address.as_str(), c.had_password)).collect();
        assert_eq!(offered, [("db-main", "postgres@203.0.113.31:2222", true), ("backups", "backup@2001:db8::24", false)]);
        let picks: Vec<String> = sources.old.iter().map(|c| c.id.clone()).collect();
        let answer = store.handle(Ask::Import { source: Source::Old, picks: picks.clone(), offered: picks });
        assert_eq!(answer, Answer::Imported { source: Source::Old, note: "Added 2 servers from SSHeesh 0.10. 1 server will ask for a password the first time.".into() });
        // Here under new ids, with the label and look 0.10 kept, and nothing of its keychain.
        let shown = store.shown();
        let db = shown.servers.iter().find(|s| s.name == "db-main").unwrap();
        assert!(db.is_prod() && db.icon == "database" && db.address == "postgres@203.0.113.31:2222");
        assert_ne!(db.key(), "host:saved:old-db");
        assert_eq!(shown.colors[&db.key()], "#0997b4");
        assert!(w.keychain.kept.lock().unwrap().is_empty());
        let v6 = shown.hosts.iter().find(|h| h.name == "backups").unwrap();
        assert_eq!((v6.hostname.as_str(), v6.user.as_deref(), v6.identity_file.as_deref()), ("2001:db8::24", Some("backup"), Some("~/.ssh/backup_key")));
        assert!(!v6.remembered_password);
        // 0.10's choice for the ssh config came too.
        assert!(shown.servers.iter().any(|s| s.name == "web-7731"));
        // Nothing is offered twice, and 0.10's file is byte for byte what it was.
        assert_eq!(store.shown().old_new, 0);
        assert_eq!(std::fs::read(w.root.join("old").join("hosts.json")).unwrap(), before);
        assert!(!w.root.join("old").join(LOOKS_FILE).exists());
    }

    #[test]
    fn xpipes_servers_are_offered_once_and_added_without_their_passwords() {
        let w = World::new("xpipe");
        let put = |id: &str, entry: serde_json::Value, store: serde_json::Value| {
            let dir = w.root.join("xpipe").join("stores").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("entry.json"), entry.to_string()).unwrap();
            std::fs::write(dir.join("store.json"), store.to_string()).unwrap();
        };
        put("a", serde_json::json!({"uuid": "a", "name": "mail"}), serde_json::json!({"type": "ssh", "host": "203.0.113.25", "port": 22}));
        put("b", serde_json::json!({"uuid": "b", "name": "locked"}), serde_json::json!({"secrets": "…"}));
        let mut store = w.store();
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!((sources.xpipe.len(), sources.xpipe_locked, sources.xpipe_here), (1, 1, 0));
        let picks = vec![sources.xpipe[0].id.clone()];
        let answer = store.handle(Ask::Import { source: Source::Xpipe, picks: picks.clone(), offered: picks });
        assert_eq!(answer, Answer::Imported { source: Source::Xpipe, note: "Added 1 server from XPipe.".into() });
        assert_eq!(w.names(&mut store), ["mail"]);
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!((sources.xpipe.len(), sources.xpipe_here), (0, 1));
    }

    #[test]
    fn a_name_a_look_and_a_label_are_kept_and_a_removed_server_leaves_nothing() {
        let w = World::new("looks");
        let mut store = w.store();
        let Answer::Added { server, .. } = store.handle(by_hand("deploy@203.0.113.17", Some("made-up-password"), false)) else { panic!() };
        for ask in [
            Ask::Rename { server: server.clone(), name: "Shop".into() },
            Ask::SetIcon { server: server.clone(), icon: "globe".into() },
            Ask::SetColor { server: server.clone(), color: "#7e8590".into() },
            Ask::SetEnv { server: server.clone(), env: Some(EnvLabel::Prod) },
        ] {
            assert_eq!(store.handle(ask), Answer::Changed);
        }
        // After a restart.
        let mut again = w.store();
        let shown = again.shown();
        let shop = &shown.servers[0];
        assert_eq!((shop.name.as_str(), shop.icon.as_str(), shop.is_prod(), shown.colors[&server].as_str()), ("Shop", "globe", true, "#7e8590"));
        // The file with names in it is yours alone.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(w.root.join("copy").join(LOOKS_FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Removed: out of the list, its password forgotten, nothing kept about it.
        assert_eq!(again.handle(Ask::Remove { server: server.clone() }), Answer::Removed { server: server.clone() });
        assert!(w.names(&mut again).is_empty());
        assert!(w.keychain.kept.lock().unwrap().is_empty());
        assert!(!w.copy_file(LOOKS_FILE).contains("Shop"));
        assert!(matches!(again.handle(Ask::Remove { server }), Answer::Failed(_)));
    }

    #[test]
    fn a_looks_file_that_does_not_read_is_set_aside_not_written_over() {
        let w = World::new("unread");
        std::fs::create_dir_all(w.root.join("copy")).unwrap();
        std::fs::write(w.root.join("copy").join(LOOKS_FILE), "{ not json").unwrap();
        let mut store = w.store();
        let Answer::Added { server, .. } = store.handle(by_hand("203.0.113.17", None, false)) else { panic!() };
        store.handle(Ask::SetEnv { server, env: Some(EnvLabel::Staging) });
        assert_eq!(w.copy_file("looks.json.unread"), "{ not json");
        assert!(w.copy_file(LOOKS_FILE).contains("staging"));
    }

    #[test]
    fn an_address_is_shown_as_0_10_showed_it() {
        let host = |hostname: &str, user: Option<&str>, port: Option<u16>| Host {
            id: "saved:1".into(),
            name: "web-7731".into(),
            hostname: hostname.into(),
            user: user.map(str::to_owned),
            port,
            identity_file: None,
            source: HostSource::Saved,
            remembered_password: false,
            identity_agent: None,
        };
        assert_eq!(address(&host("203.0.113.17", Some("deploy"), None)), "deploy@203.0.113.17");
        assert_eq!(address(&host("203.0.113.31", Some("postgres"), Some(2222))), "postgres@203.0.113.31:2222");
        assert_eq!(address(&host("backups", None, Some(22))), "backups");
        assert_eq!(address_to_add(&host("2001:db8::24", Some("backup"), Some(2200))), "backup@[2001:db8::24]:2200");
        assert_eq!(address_to_add(&host("203.0.113.17", None, None)), "203.0.113.17");
    }
}
