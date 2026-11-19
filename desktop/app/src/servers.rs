//! This copy's own list of servers: the ones added here (`hosts.json` in the copy's own
//! folder, `~/.sshee-preview`), the ones in `~/.ssh/config` you said yes to, and how each
//! looks and what it's called (`looks.json` beside it). Saved passwords go to the copy's own
//! keychain name (`AppIdentity::keychain_service`), never into a file.
//!
//! 0.10's list is never written and never used as this copy's own. It is read to offer
//! "Bring over your servers from 0.10": each server you tick is added here under a new id,
//! with the label, icon and colour 0.10 kept for it. 0.10's saved passwords are copied only
//! on "Use old passwords"; the released copy, which shares 0.10's keychain name, has its
//! `~/.ssh/config` servers' passwords already and only brings over 0.10's note of them.
//! XPipe's vault, other SSH apps' saved sessions (PuTTY, WinSCP… `sshee_core::other_apps`)
//! and `~/.ssh/config` are read the same way: read, offered, never changed.
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
use sshee_app::servers::{forgot_note, host_id, imported_note, old_passwords_note, saved_note, Answer, App, AppOffer, Ask, Candidate, NewServer, Source, Sources};
use sshee_core::other_apps;
use sshee_app::workbench::{Server, ServerKey};
use sshee_core::secrets::{PasswordStore, Secret};
use sshee_core::{AppIdentity, Host, HostSource, HostStore, NewHost};

/// How each server looks and what it's called, in the copy's folder.
const LOOKS_FILE: &str = "looks.json";

/// The `~/.ssh/config` servers whose "password in the keychain" note was brought over from
/// 0.10's folder already, in the copy's folder: each is brought once, so a password forgotten
/// here stays forgotten.
const OLD_NOTED_FILE: &str = "remembered-from-0.10.json";

/// Where saved passwords go. The program's is the system keychain under this copy's own name
/// ([`PasswordStore`]); a test's is a list in memory.
pub trait Keychain: Send + Sync {
    /// Keeps a password for this run, and in the keychain when `remember` is set.
    fn set(&self, host_id: &str, password: Secret, remember: bool) -> Result<(), String>;
    /// The password for a host, when there is one (`remembered`: the list says the keychain
    /// has it).
    fn get(&self, host_id: &str, remembered: bool) -> Option<Secret>;
    fn forget(&self, host_id: &str);
    /// A password SSHeesh 0.10 saved, under its own keychain name: read only after you said
    /// yes (the system may ask you too).
    fn old(&self, host_id: &str) -> Option<Secret>;
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

    fn old(&self, host_id: &str) -> Option<Secret> {
        sshee_core::secrets::read_saved(AppIdentity::standard().keychain_service(), host_id)
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
    /// Where other SSH apps keep their sessions (and the registry, on Windows). Only ever read.
    pub apps: other_apps::Places,
    /// The labels 0.10 kept (slow: it copies a database to read it).
    pub old_labels: Box<dyn Fn() -> Option<Labels> + Send>,
    /// This copy keeps its passwords under 0.10's keychain name (the released 1.x is the
    /// standard copy): what 0.10 saved is already where this copy looks.
    pub shares_old_keychain: bool,
}

impl Places {
    /// The places of the copy `identity` is, on this computer. A copy with no folder of its
    /// own (no home folder) keeps its list for this run only, in a folder that goes with it.
    pub fn of(identity: &AppIdentity, ssh_config: Option<PathBuf>) -> Places {
        let dir = identity.dir().map(Path::to_owned).unwrap_or_else(|| std::env::temp_dir().join(format!("sshee-preview-{}", std::process::id())));
        Places {
            dir,
            ssh_config,
            old_dir: dirs::data_dir().map(|d| d.join(APP_IDENTIFIER)),
            xpipe: sshee_core::xpipe::vaults(),
            apps: other_apps::Places::here(),
            old_labels: Box::new(labels_of_0_10),
            shares_old_keychain: identity.keychain_service() == AppIdentity::standard().keychain_service(),
        }
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
    /// Each shown server's look, picture (decoded here, off the UI thread) and saved password.
    pub faces: IndexMap<ServerKey, sshee_app::labels::Face>,
}

/// A server's picture, decoded for the window: the data URL 0.10 keeps, as RGBA.
pub fn picture(look: &sshee_app::labels::Look) -> Option<sshee_app::labels::Picture> {
    let bytes = sshee_app::labels::image_bytes(look.image.as_deref()?)?;
    let decoded = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    (width > 0 && height > 0 && width <= 1024 && height <= 1024).then(|| sshee_app::labels::Picture { width, height, rgba: decoded.into_raw().into() })
}

/// A picture file made into a server's icon, as 0.10 did (`hostImageFrom`): the middle square,
/// [`sshee_app::labels::IMAGE_SIDE`] pixels, as a PNG data URL.
pub fn picture_from_file(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("Couldn't read that picture: {e}"))?.len();
    if size > sshee_app::labels::IMAGE_FILE_MAX {
        return Err("That picture is too big. Pick one under 8 MB.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read that picture: {e}"))?;
    let decoded = image::load_from_memory(&bytes).map_err(|_| "That file isn't a picture SSHeesh can read. Try a PNG or a JPEG.".to_owned())?;
    let side = decoded.width().min(decoded.height());
    let square = decoded.crop_imm((decoded.width() - side) / 2, (decoded.height() - side) / 2, side, side);
    let small = square.resize_exact(sshee_app::labels::IMAGE_SIDE, sshee_app::labels::IMAGE_SIDE, image::imageops::FilterType::Lanczos3);
    let mut png = Vec::new();
    small.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| format!("Couldn't make the icon: {e}"))?;
    Ok(sshee_app::labels::image_data_url(&png))
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
/// The window's name for an app core reads.
fn app_of(app: other_apps::App) -> App {
    match app {
        other_apps::App::Putty => App::Putty,
        other_apps::App::Kitty => App::Kitty,
        other_apps::App::WinScp => App::WinScp,
        other_apps::App::MobaXterm => App::MobaXterm,
        other_apps::App::FileZilla => App::FileZilla,
        other_apps::App::SecureCrt => App::SecureCrt,
        other_apps::App::Remmina => App::Remmina,
    }
}

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
        let mut store = ServerStore { places, hosts, looks, looks_unread, keychain };
        store.note_old_config_passwords();
        store
    }

    /// The `~/.ssh/config` servers whose password 0.10 kept in its keychain: the ids in its
    /// `remembered-passwords.json`. Only ever read.
    fn old_remembered_config(&self) -> Vec<String> {
        match &self.places.old_dir {
            Some(dir) => HostStore::new(dir).with_ssh_config(None).remembered_ids().into_iter().filter(|id| id.starts_with("cfg:")).collect(),
            None => Vec::new(),
        }
    }

    /// A copy under 0.10's keychain name has 0.10's passwords for its `~/.ssh/config` servers
    /// already (the same entry, `cfg:<alias>`); only 0.10's note that they're there is in
    /// 0.10's folder. Brings that note over, once for each server this copy lists. Never reads
    /// a password, and a note that can't be written is tried again next time.
    fn note_old_config_passwords(&mut self) {
        if !self.places.shares_old_keychain {
            return;
        }
        let old = self.old_remembered_config();
        if old.is_empty() {
            return;
        }
        let path = self.places.dir.join(OLD_NOTED_FILE);
        let mut noted: Vec<String> = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let listed = self.hosts();
        let before = noted.len();
        for id in old {
            if noted.contains(&id) {
                continue;
            }
            let Some(host) = listed.iter().find(|h| h.id == id && h.source == HostSource::SshConfig) else { continue };
            if host.remembered_password || self.hosts.set_remembered_password(&id, true).is_ok() {
                noted.push(id);
            }
        }
        if noted.len() > before {
            if let Ok(json) = serde_json::to_vec_pretty(&noted) {
                let _ = write_private(&path, &json);
            }
        }
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
            faces: sshee_app::servers::faces(&self.looks.shown_looks(&list), |key| hosts.iter().any(|h| format!("host:{}", h.id) == key && h.remembered_password), picture),
            hosts,
        }
    }

    /// This copy's servers brought over from 0.10 whose password 0.10 kept and this copy
    /// doesn't have: `(id here, id in 0.10, name)`, matched by address. A `~/.ssh/config`
    /// server is the same id in both; a copy under 0.10's keychain name has those passwords
    /// already ([`ServerStore::note_old_config_passwords`]), so only another copy is offered them.
    fn old_passwords(&mut self) -> Vec<(String, String, String)> {
        let old = self.old_hosts();
        let old_config = if self.places.shares_old_keychain { Vec::new() } else { self.old_remembered_config() };
        self.hosts()
            .into_iter()
            .filter(|h| !h.remembered_password)
            .filter_map(|h| {
                let from = if h.source == HostSource::SshConfig {
                    old_config.iter().find(|id| **id == h.id)?.clone()
                } else {
                    old.iter().find(|o| o.remembered_password && address(o) == address(&h))?.id.clone()
                };
                Some((h.id.clone(), from, h.name.clone()))
            })
            .collect()
    }

    /// A look at a server is over: what it guessed is kept, for good.
    pub fn guessed(&mut self, server: &str, icon: Option<&str>) -> Answer {
        if let (Some(id), Some(icon)) = (host_id(server), icon) {
            self.looks.set_guess(id, icon);
            if let Err(why) = self.save_looks() {
                return Answer::Refused { server: server.to_owned(), doing: "Couldn't keep the server's icon".into(), why };
            }
        }
        Answer::Detected { server: server.to_owned(), icon: icon.map(str::to_owned) }
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
            apps: self.app_offers(&mine),
            old_passwords: self.old_passwords().len(),
        }
    }

    /// What the other SSH apps hold that isn't here yet (by address).
    fn app_offers(&self, mine: &[Host]) -> Vec<AppOffer> {
        other_apps::scan_all(&self.places.apps)
            .into_iter()
            .map(|(app, scan)| {
                let found = scan.servers.len();
                let servers: Vec<Candidate> = scan
                    .servers
                    .into_iter()
                    .filter(|x| !mine.iter().any(|m| address(m) == x.address))
                    .map(|x| Candidate { id: x.address.clone(), name: x.name, address: x.address, identity_file: x.identity_file, had_password: x.had_password, picked: true })
                    .collect();
                AppOffer { app: app_of(app), here: found - servers.len(), servers, through_jump: scan.through_jump, ppk_keys: scan.ppk_keys }
            })
            .collect()
    }

    /// Adds one server to this copy's list.
    fn add(&mut self, new: NewServer, identity_agent: Option<String>) -> Result<(Host, Option<String>), String> {
        let password = new.password.as_ref().filter(|p| !p.is_empty()).map(|p| Secret::new(p.expose()));
        let remember = new.remember_password;
        let identity_agent = new.identity_agent.or(identity_agent);
        let input = NewHost { name: new.name, address: new.address, identity_file: new.identity_file, password: password.clone(), remember_password: remember, identity_agent };
        let mut host = self.hosts.add(input)?;
        // The look and label picked in the form, right away.
        let mut looks_failed = None;
        if new.look.is_some() || new.env.is_some() {
            if let Some(look) = &new.look {
                self.looks.set_look(&host.id, look.kept());
            }
            if new.env.is_some() {
                self.looks.set_env(&host.id, new.env.clone());
            }
            // Added all the same; it says so.
            looks_failed = self.save_looks().err();
        }
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
        Ok((host, warning.or(looks_failed)))
    }

    fn import(&mut self, source: Source, picks: &[String], offered: &[String]) -> Result<(String, usize), String> {
        match source {
            Source::SshConfig => {
                self.looks.choose_config(offered, picks);
                self.save_looks()?;
                Ok((imported_note(source, picks.len(), &[], 0), 0))
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
                Ok((imported_note(source, added, &failed, with_password), 0))
            }
            Source::App(app) => {
                let (mut added, mut failed, mut with_password) = (0, Vec::new(), 0);
                let theirs = other_apps::App::ALL.into_iter().find(|&a| app_of(a) == app).ok_or("That app isn't one SSHeesh reads.")?;
                for x in other_apps::scan(theirs, &self.places.apps).servers.into_iter().filter(|x| picks.contains(&x.address)) {
                    // Never a password: a server that needs one asks the first time.
                    let new = NewServer { name: Some(x.name.clone()), address: x.address, identity_file: x.identity_file, ..NewServer::default() };
                    match self.add(new, None) {
                        Ok(_) => {
                            added += 1;
                            with_password += usize::from(x.had_password);
                        }
                        Err(_) => failed.push(x.name),
                    }
                }
                Ok((imported_note(source, added, &failed, with_password), 0))
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
                // Their passwords are offered next, asked first.
                Ok((imported_note(source, moved.len(), &failed, 0), with_password))
            }
        }
    }

    /// Does what the window asked. Slow: files, the keychain. A check of a server, a look at
    /// what it runs and the keys on this computer need ssh: the backend does those
    /// ([`crate::live`]) and only keeps what they found here.
    pub fn handle(&mut self, ask: Ask) -> Answer {
        let looks = |store: &mut ServerStore, ask: &Ask, change: &dyn Fn(&mut Looks, &str)| {
            if let Some(id) = ask.server().and_then(host_id) {
                change(&mut store.looks, id);
            }
            store.save_looks().map_or_else(|why| Answer::failed(ask, why), |()| Answer::Changed)
        };
        match ask {
            Ask::Sources => Answer::Sources(self.sources()),
            Ask::Add(new) => match self.add(new, None) {
                Ok((host, warning)) => Answer::Added { server: format!("host:{}", host.id), name: host.name, warning },
                Err(why) => Answer::Failed(why),
            },
            Ask::Import { source, picks, offered } => match self.import(source, &picks, &offered) {
                Ok((note, old_passwords)) => Answer::Imported { source, note, old_passwords },
                Err(why) => Answer::Failed(why),
            },
            Ask::UseOldPasswords => {
                let (mut copied, mut failed) = (0, Vec::new());
                for (here, there, name) in self.old_passwords() {
                    match self.keychain.old(&there).map(|pw| self.keychain.set(&here, pw, true)) {
                        Some(Ok(())) => {
                            let _ = self.hosts.set_remembered_password(&here, true);
                            copied += 1;
                        }
                        _ => failed.push(name),
                    }
                }
                Answer::Note(old_passwords_note(copied, &failed))
            }
            Ask::Reorder { ref order } => {
                self.looks.reorder(order);
                self.save_looks().map_or_else(|why| Answer::failed(&ask, why), |()| Answer::Changed)
            }
            Ask::Rename { ref server, ref name } => {
                if let Some(host) = host_id(server).and_then(|id| self.hosts.get(id)) {
                    self.looks.rename(&listed(&host), name);
                }
                self.save_looks().map_or_else(|why| Answer::failed(&ask, why), |()| Answer::Changed)
            }
            Ask::SetIcon { ref icon, .. } => {
                let icon = icon.clone();
                looks(self, &ask, &|l, id| l.set_icon(id, &icon))
            }
            Ask::SetColor { ref color, .. } => match sshee_app::labels::parse_color(color) {
                Some(color) => looks(self, &ask, &|l, id| l.set_color(id, &color)),
                None => Answer::failed(&ask, "That isn't a color. Type it as #2389e2."),
            },
            Ask::SetImage { ref path, .. } => match picture_from_file(path) {
                Ok(url) => looks(self, &ask, &|l, id| l.set_image(id, Some(url.clone()))),
                Err(why) => Answer::failed(&ask, why),
            },
            Ask::ClearImage { .. } => looks(self, &ask, &|l, id| l.set_image(id, None)),
            Ask::Automatic { .. } => looks(self, &ask, &|l, id| l.automatic(id)),
            Ask::SetEnv { ref env, .. } => {
                let env = env.clone();
                looks(self, &ask, &|l, id| l.set_env(id, env.clone()))
            }
            Ask::ForgetPassword { ref server } => {
                let Some(host) = host_id(server).and_then(|id| self.hosts.get(id)) else { return Answer::failed(&ask, "That server isn't in your list anymore.") };
                self.keychain.forget(&host.id);
                if let Err(why) = self.hosts.set_remembered_password(&host.id, false) {
                    return Answer::failed(&ask, why);
                }
                Answer::Note(forgot_note(self.looks.name(&listed(&host))))
            }
            Ask::SavePassword { ref server, ref password, remember } => {
                let Some(host) = host_id(server).and_then(|id| self.hosts.get(id)) else { return Answer::failed(&ask, "That server isn't in your list anymore.") };
                if password.is_empty() {
                    return Answer::failed(&ask, "Type the password first.");
                }
                // Only for this run: an older one in the keychain goes, or the next run would
                // sign in with it.
                if !remember {
                    self.keychain.forget(&host.id);
                }
                // The keychain refused: it still works until the app quits, and says so.
                if let Err(why) = self.keychain.set(&host.id, Secret::new(password.expose()), remember) {
                    let _ = self.hosts.set_remembered_password(&host.id, false);
                    return Answer::failed(&ask, why);
                }
                if let Err(why) = self.hosts.set_remembered_password(&host.id, remember) {
                    return Answer::failed(&ask, why);
                }
                Answer::Note(saved_note(self.looks.name(&listed(&host)), remember))
            }
            Ask::Remove { server } => {
                let Some(host) = host_id(&server).and_then(|id| self.hosts.list().into_iter().find(|h| h.id == id)) else {
                    return Answer::failed(&Ask::Remove { server }, "That server isn't in your list anymore.");
                };
                if host.source == HostSource::SshConfig {
                    // It stays in the file; here it's hidden.
                    self.looks.hide(&host.id);
                } else {
                    self.keychain.forget(&host.id);
                    if let Err(why) = self.hosts.remove(&host.id) {
                        return Answer::failed(&Ask::Remove { server }, why);
                    }
                    self.looks.forget(&host.id);
                }
                match self.save_looks() {
                    Ok(()) => Answer::Removed { server },
                    Err(why) => Answer::failed(&Ask::Remove { server }, why),
                }
            }
            // ssh's work: the backend does it (`live.rs`).
            Ask::LocalAuth | Ask::Test(_) | Ask::TrustTest(_) | Ask::Detect { .. } => Answer::failed(&ask, "This needs the program's backend."),
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

/// The keys in `~/.ssh` and the key agents here (0.10's `local_auth`), told without sshee-core.
pub fn local_auth() -> sshee_app::servers::LocalAuth {
    let found = sshee_core::keys::discover();
    sshee_app::servers::LocalAuth {
        keys: found.keys.into_iter().map(|k| sshee_app::servers::LocalKey { path: k.path, name: k.name, kind: k.kind, comment: k.comment }).collect(),
        agents: found.agents.into_iter().map(|a| sshee_app::servers::KeyAgent { name: a.name, socket: a.socket, keys: a.keys }).collect(),
    }
}

/// "Check before adding" (0.10's `test_connection`): is the server there, is its identity
/// known, does signing in work, is tmux installed. `trust`: keep the identity shown before
/// first ("Trust it and continue"). Nothing is saved but a trusted identity.
pub fn test(keys: &sshee_core::hostkeys::PendingKeys, env: &sshee_core::ssh::SshEnv, new: &NewServer, trust: bool) -> Answer {
    use sshee_app::servers::{HostKeyState, Tested};
    use sshee_core::hostkeys::HostKeyCheck;
    let input = NewHost {
        name: new.name.clone(),
        address: new.address.clone(),
        identity_file: new.identity_file.clone(),
        password: new.password.as_ref().map(|p| Secret::new(p.expose())),
        remember_password: false,
        identity_agent: new.identity_agent.clone(),
    };
    let host = match sshee_core::hosts::preview(&input) {
        Ok(host) => host,
        Err(why) => return Answer::Failed(why),
    };
    let mut t = host.target();
    t.password = input.password.clone().filter(|p| !p.expose().is_empty());
    if trust {
        if let Err(e) = keys.trust(&host.id, &t) {
            return Answer::Failed(e.message);
        }
    }
    let prints = |list: Vec<sshee_core::hostkeys::Fingerprint>| list.into_iter().map(|f| (f.kind, f.sha256)).collect();
    let host_key = match keys.check(&host.id, &t, false) {
        Ok(HostKeyCheck::Known) => HostKeyState::Known,
        Ok(HostKeyCheck::Unknown { fingerprints }) => HostKeyState::Unknown { fingerprints: prints(fingerprints) },
        Ok(HostKeyCheck::Changed { .. }) => HostKeyState::Changed,
        Ok(HostKeyCheck::Unreachable { message }) => HostKeyState::Unreachable { message: Some(message).filter(|m| !m.is_empty()) },
        Err(e) => HostKeyState::Unreachable { message: Some(e.message) },
    };
    if host_key != HostKeyState::Known {
        return Answer::Tested(Tested { host_key, signed_in: false, tmux: None, message: None });
    }
    let probe = "if command -v tmux >/dev/null 2>&1; then echo tmux=yes; else echo tmux=no; fi";
    Answer::Tested(match sshee_core::ssh::run_remote(&t, env, probe, None) {
        Ok(out) => Tested { host_key, signed_in: true, tmux: Some(String::from_utf8_lossy(&out).contains("tmux=yes")), message: None },
        Err(e) => Tested { host_key, signed_in: false, tmux: None, message: Some(e.message) },
    })
}

/// A look at what a server runs, for its icon (0.10's `detectRole`): its listening ports and
/// its containers. `None` when it couldn't tell, so it's looked at again another time.
pub fn detect(target: &sshee_core::ssh::Target, env: &sshee_core::ssh::SshEnv) -> Option<&'static str> {
    use sshee_app::roles::{Listening, Scan};
    let scan = match sshee_core::tunnels::scan(target, env) {
        Ok(s) => Ok(Some(Scan {
            apps: s.apps.into_iter().map(|a| Listening { port: a.port, process: a.process, known_as: a.known_as, scheme: a.scheme, web: a.web }).collect(),
            curl: s.curl,
        })),
        // A server without ss or netstat goes by its containers alone.
        Err(e) if e.code == sshee_core::error::ErrorCode::Unsupported => Ok(None),
        Err(_) => Err(()),
    };
    let containers = sshee_core::containers::list(target, env).map(|l| l.containers.iter().any(|c| c.running)).map_err(|_| ());
    sshee_app::roles::guess(&(scan, containers))
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
        /// Each `get`: the host, and whether the list said the keychain has its password.
        asked: Mutex<Vec<(String, bool)>>,
    }

    impl Keychain for FakeKeychain {
        fn set(&self, host_id: &str, password: Secret, remember: bool) -> Result<(), String> {
            if self.refuses {
                return Err("Couldn't save the password in your keychain (locked). It works until you quit SSHeesh.".into());
            }
            self.kept.lock().unwrap().push((host_id.to_owned(), password.expose().to_owned(), remember));
            Ok(())
        }

        fn get(&self, host_id: &str, remembered: bool) -> Option<Secret> {
            self.asked.lock().unwrap().push((host_id.to_owned(), remembered));
            self.kept.lock().unwrap().iter().find(|(id, ..)| id == host_id).map(|(_, pw, _)| Secret::new(pw.clone()))
        }

        fn forget(&self, host_id: &str) {
            self.kept.lock().unwrap().retain(|(id, ..)| id != host_id);
        }

        fn old(&self, host_id: &str) -> Option<Secret> {
            // 0.10 kept a password for its server "old-db" and for the ssh config's "web-7731".
            match host_id {
                "saved:old-db" => Some(Secret::new("from-0.10")),
                "cfg:web-7731" => Some(Secret::new("config-from-0.10")),
                _ => None,
            }
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
            self.store_as(false)
        }

        /// The store of a copy that shares 0.10's keychain name (the release) or not (Preview).
        fn store_as(&self, shares_old_keychain: bool) -> ServerStore {
            // The copy's own folder comes from its identity, as in the program.
            let identity = AppIdentity::in_dir(self.root.join("copy"), "io.example.ssheesh.test");
            let places = Places {
                ssh_config: Some(self.root.join("ssh_config")),
                old_dir: Some(self.root.join("old")),
                xpipe: vec![self.root.join("xpipe")],
                apps: other_apps::Places::under(&self.root.join("home")),
                old_labels: Box::new(|| {
                    let mut labels = Labels::default();
                    labels.env.insert("saved:old-db".into(), EnvLabel::Prod);
                    labels.looks.insert("saved:old-db".into(), HostLook { icon: "database".into(), color: "#0997b4".into(), ..HostLook::default() });
                    labels.config_seen.push("cfg:web-7731".into());
                    Some(labels)
                }),
                shares_old_keychain,
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
        Ask::Add(NewServer { name: None, address: address.into(), identity_file: None, password: password.map(Password::new), remember_password: remember, ..NewServer::default() })
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
        assert_eq!(answer, Answer::Imported { source: Source::SshConfig, note: "1 server shows from ~/.ssh/config.".into(), old_passwords: 0 });
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
    fn a_password_saved_for_a_server_from_the_ssh_config_is_used_after_a_restart() {
        let w = World::new("save-password");
        let mut store = w.store();
        let ids = vec!["cfg:web-7731".to_owned()];
        store.handle(Ask::Import { source: Source::SshConfig, picks: ids.clone(), offered: ids });
        let server = "host:cfg:web-7731".to_owned();
        assert!(!store.shown().faces[&server].saved_password);
        assert_eq!(store.password("cfg:web-7731"), None);
        let ask = Ask::SavePassword { server: server.clone(), password: Password::new("made-up-password"), remember: true };
        // Never printed, even inside the ask.
        assert!(!format!("{ask:?}").contains("made-up"));
        assert_eq!(store.handle(ask), Answer::Note(saved_note("web-7731", true)));
        assert_eq!(*w.keychain.kept.lock().unwrap(), [("cfg:web-7731".to_owned(), "made-up-password".to_owned(), true)]);
        // After a restart the list says the keychain has it, and a session signs in with it.
        let mut again = w.store();
        assert!(again.shown().faces[&server].saved_password);
        assert!(again.hosts().iter().any(|h| h.id == "cfg:web-7731" && h.remembered_password));
        assert_eq!(again.password("cfg:web-7731").map(|p| p.expose().to_owned()).as_deref(), Some("made-up-password"));
        // No file holds it; the ssh config is untouched.
        for entry in std::fs::read_dir(w.root.join("copy")).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
            assert!(!text.contains("made-up-password"));
        }
        assert!(!std::fs::read_to_string(w.root.join("ssh_config")).unwrap().contains("made-up"));
        // Forgotten again: the next run asks.
        assert!(matches!(again.handle(Ask::ForgetPassword { server: server.clone() }), Answer::Note(_)));
        assert!(!w.store().shown().faces[&server].saved_password);
        // Nothing typed, or a server not in the list, is refused in words.
        assert!(matches!(again.handle(Ask::SavePassword { server: server.clone(), password: Password::default(), remember: true }), Answer::Refused { .. }));
        assert!(matches!(again.handle(Ask::SavePassword { server: "host:cfg:gone".into(), password: Password::new("x"), remember: true }), Answer::Refused { .. }));
    }

    /// The window offers to save a password at the prompts askpass answers, and no others.
    #[test]
    fn the_window_reads_a_password_prompt_as_askpass_does() {
        for line in [
            "deploy@203.0.113.17's password: ",
            "(deploy@web-7731) Password:",
            "Password:",
            "Enter passphrase for key '/home/demo/.ssh/id_ed25519': ",
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
            "Verification code: ",
            "deploy@web-7731:~$ ",
        ] {
            assert_eq!(sshee_app::servers::asks_for_password(line), sshee_core::secrets::is_password_prompt(line), "{line}");
        }
    }

    #[test]
    fn a_keychain_that_refuses_a_saved_password_says_so() {
        let w = World::refusing("save-refused");
        let mut store = w.store();
        let answer = store.handle(Ask::SavePassword { server: "host:cfg:web-7731".into(), password: Password::new("made-up-password"), remember: true });
        assert!(matches!(&answer, Answer::Refused { why, .. } if why.contains("keychain")), "{answer:?}");
        assert!(!store.hosts().iter().any(|h| h.remembered_password));
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
        assert_eq!(answer, Answer::Imported { source: Source::Old, note: "Added 2 servers from SSHeesh 0.10.".into(), old_passwords: 1 });
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
        // 0.10's password is offered, and read only on a yes.
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!(sources.old_passwords, 1);
        assert_eq!(store.handle(Ask::UseOldPasswords), Answer::Note("Brought over the saved passwords of 1 server.".into()));
        assert_eq!(w.keychain.kept.lock().unwrap()[0].1, "from-0.10");
        assert!(store.shown().faces[&db.key()].saved_password);
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!(sources.old_passwords, 0);
        // Forgotten: out of the keychain, and the server asks again.
        assert!(matches!(store.handle(Ask::ForgetPassword { server: db.key() }), Answer::Note(_)));
        assert!(w.keychain.kept.lock().unwrap().is_empty() && !store.shown().faces[&db.key()].saved_password);
    }

    impl World {
        /// 0.10's note of the ssh config servers whose password it kept.
        fn old_remembered(&self, ids: &[&str]) {
            std::fs::write(self.root.join("old").join("remembered-passwords.json"), serde_json::to_string(ids).unwrap()).unwrap();
        }

        fn remembered(store: &mut ServerStore, id: &str) -> Option<bool> {
            store.hosts().into_iter().find(|h| h.id == id).map(|h| h.remembered_password)
        }
    }

    #[test]
    fn the_release_keeps_0_10s_passwords_for_ssh_config_servers_without_asking_once() {
        let w = World::new("old-config-release");
        // 0.10 kept passwords for web-7731 (in the config) and gone-box (not in it any more).
        w.old_remembered(&["cfg:web-7731", "cfg:gone-box"]);
        let old_note = std::fs::read(w.root.join("old").join("remembered-passwords.json")).unwrap();
        let mut store = w.store_as(true);
        assert_eq!(World::remembered(&mut store, "cfg:web-7731"), Some(true));
        assert_eq!(World::remembered(&mut store, "cfg:old-box"), Some(false));
        assert_eq!(World::remembered(&mut store, "cfg:gone-box"), None);
        // No password was read or copied: it's in the same keychain entry, and a session asks
        // for it there.
        assert!(w.keychain.kept.lock().unwrap().is_empty());
        let _ = store.password("cfg:web-7731");
        assert!(w.keychain.asked.lock().unwrap().contains(&("cfg:web-7731".to_owned(), true)));
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!(sources.old_passwords, 0);
        assert_eq!(serde_json::from_str::<Vec<String>>(&w.copy_file("remembered-passwords.json")).unwrap(), ["cfg:web-7731"]);
        // A second start changes nothing more.
        let (note, noted) = (w.copy_file("remembered-passwords.json"), w.copy_file(OLD_NOTED_FILE));
        let mut again = w.store_as(true);
        assert_eq!((w.copy_file("remembered-passwords.json"), w.copy_file(OLD_NOTED_FILE)), (note, noted));
        // Forgotten here, it stays forgotten after a restart.
        assert!(matches!(again.handle(Ask::ForgetPassword { server: "host:cfg:web-7731".into() }), Answer::Note(_)));
        assert_eq!(World::remembered(&mut w.store_as(true), "cfg:web-7731"), Some(false));
        // 0.10's folder is only read.
        assert_eq!(std::fs::read(w.root.join("old").join("remembered-passwords.json")).unwrap(), old_note);
        assert!(!w.root.join("old").join("hosts.json").exists());
    }

    #[test]
    fn a_preview_offers_0_10s_passwords_for_ssh_config_servers_and_copies_them_on_a_yes() {
        let w = World::new("old-config-preview");
        w.old_remembered(&["cfg:web-7731", "cfg:gone-box"]);
        let mut store = w.store();
        // Its keychain is its own: nothing is noted before you say yes.
        assert_eq!(World::remembered(&mut store, "cfg:web-7731"), Some(false));
        assert!(!w.root.join("copy").join(OLD_NOTED_FILE).exists());
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!(sources.old_passwords, 1);
        assert_eq!(store.handle(Ask::UseOldPasswords), Answer::Note("Brought over the saved passwords of 1 server.".into()));
        assert_eq!(*w.keychain.kept.lock().unwrap(), [("cfg:web-7731".to_owned(), "config-from-0.10".to_owned(), true)]);
        assert_eq!(World::remembered(&mut store, "cfg:web-7731"), Some(true));
        assert_eq!(store.password("cfg:web-7731").map(|p| p.expose().to_owned()).as_deref(), Some("config-from-0.10"));
        // Nothing more is offered, now or after a restart.
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!(sources.old_passwords, 0);
        let Answer::Sources(sources) = w.store().handle(Ask::Sources) else { panic!() };
        assert_eq!(sources.old_passwords, 0);
        assert_eq!(w.keychain.kept.lock().unwrap().len(), 1);
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
        assert_eq!(answer, Answer::Imported { source: Source::Xpipe, note: "Added 1 server from XPipe.".into(), old_passwords: 0 });
        assert_eq!(w.names(&mut store), ["mail"]);
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert_eq!((sources.xpipe.len(), sources.xpipe_here), (0, 1));
    }

    #[test]
    fn other_apps_servers_are_offered_once_and_added_without_their_passwords() {
        let w = World::new("apps");
        let put = |rel: &str, text: &str| {
            let path = w.root.join("home").join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        // web-7731 is in the ssh config already, by the same address.
        put(".putty/sessions/web-7731", "HostName=203.0.113.17\nUserName=deploy\n");
        put(".putty/sessions/mail-7731", "HostName=203.0.113.25\nUserName=root\nPortNumber=2222\n");
        put(".config/WinSCP.ini", "[Sessions\\files-7731]\nHostName=198.51.100.25\nUserName=ops\nPassword=A35C\n");
        let mut store = w.store();
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        let putty = sources.app(App::Putty).unwrap();
        assert_eq!((putty.servers.len(), putty.here), (1, 1));
        assert_eq!(sources.offers().iter().filter(|(s, _)| matches!(s, Source::App(_))).count(), 2);
        let picks = vec!["ops@198.51.100.25".to_owned()];
        let answer = store.handle(Ask::Import { source: Source::App(App::WinScp), picks: picks.clone(), offered: picks });
        assert_eq!(
            answer,
            Answer::Imported { source: Source::App(App::WinScp), note: "Added 1 server from WinSCP. 1 server will ask for a password the first time.".into(), old_passwords: 0 }
        );
        // Its password stayed in WinSCP: nothing in the keychain.
        assert!(w.keychain.kept.lock().unwrap().is_empty());
        let Answer::Sources(sources) = store.handle(Ask::Sources) else { panic!() };
        assert!(sources.app(App::WinScp).is_some_and(|o| o.servers.is_empty() && o.here == 1));
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
        assert!(matches!(again.handle(Ask::Remove { server }), Answer::Refused { .. }));
    }

    #[test]
    fn the_servers_order_is_kept_after_a_restart() {
        let w = World::new("order");
        let mut store = w.store();
        let Answer::Added { server: first, .. } = store.handle(by_hand("deploy@203.0.113.17", None, false)) else { panic!() };
        let Answer::Added { server: second, .. } = store.handle(by_hand("dev@198.51.100.12", None, false)) else { panic!() };
        let order = vec![second.clone(), "local".to_owned(), first.clone()];
        assert_eq!(store.handle(Ask::Reorder { order: order.clone() }), Answer::Changed);
        let mut again = w.store();
        let keys: Vec<String> = again.shown().servers.iter().map(Server::key).collect();
        assert_eq!(keys, order);
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
