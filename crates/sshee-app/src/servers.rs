//! Adding and managing servers, without a screen and without ssh: what the "Add a server" form
//! takes and refuses, what the three imports offer (`~/.ssh/config`, XPipe, 0.10's own list),
//! and the words for what happened. A port of the rules in 0.10's `AddServerDialog.tsx`,
//! `src/lib/address.ts`, `ConfigImport.tsx` and `XpipeImport.tsx`.
//!
//! The window asks its backend with an [`Ask`] and hears back an [`Answer`]: saving, the
//! keychain and reading other programs' files all wait, so they never run on the UI thread.
//! The program does them on sshee-core; a demo and the tests answer from memory.

use crate::labels::{Listed, Looks};
use crate::model::EnvLabel;
use crate::workbench::{Server, ServerKey};
use indexmap::IndexMap;
use std::fmt;

/// A password on its way to the keychain. Never printed: its `Debug` is dots.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    pub fn new(text: impl Into<String>) -> Password {
        Password(text.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(••••)")
    }
}

/// What an address box held, taken apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<String>,
    pub identity_file: Option<String>,
}

/// Words of a command line: quotes group, as a shell reads them.
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut open = false;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                open = true;
            }
            None if c.is_whitespace() => {
                if open || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                    open = false;
                }
            }
            None => word.push(c),
        }
    }
    if open || !word.is_empty() {
        out.push(word);
    }
    out
}

/// `user@host:port`, `[v6]:port`, `ssh://user@host:port` taken apart.
fn split_target(target: &str) -> Parsed {
    let mut t = target.trim().trim_start_matches("ssh://").trim_end_matches('/');
    let mut user = None;
    if let Some(at) = t.rfind('@').filter(|at| *at > 0) {
        user = Some(t[..at].to_owned());
        t = &t[at + 1..];
    }
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if let Some((host, tail)) = t.strip_prefix('[').and_then(|r| r.split_once(']')) {
        let port = tail.strip_prefix(':').filter(|p| digits(p)).map(str::to_owned);
        return Parsed { host: host.to_owned(), user, port, identity_file: None };
    }
    // One colon is `host:port`; a bare IPv6 address has several.
    match t.split_once(':') {
        Some((host, port)) if !host.is_empty() && !host.contains(':') && digits(port) => Parsed { host: host.to_owned(), user, port: Some(port.to_owned()), identity_file: None },
        _ => Parsed { host: t.to_owned(), user, port: None, identity_file: None },
    }
}

/// Understands whatever people paste as a server address: `root@203.0.113.10`, `host:2222`,
/// `ssh://user@host`, or a whole `ssh -p 2222 -i key user@host` (0.10's `parseAddress`).
pub fn parse_address(input: &str) -> Parsed {
    let text = input.trim();
    let parts = words(text);
    if parts.first().map(String::as_str) != Some("ssh") {
        return split_target(text);
    }
    // ssh's options that take a value; the rest are flags.
    const WITH_VALUE: &str = "piloJFLRDWbcEmOQSwBeI";
    let (mut user, mut port, mut identity_file, mut target) = (None, None, None, None::<String>);
    let mut i = 1;
    while i < parts.len() {
        let w = &parts[i];
        i += 1;
        if w.starts_with('-') && w.len() > 1 {
            let flag = w.chars().nth(1).unwrap_or(' ');
            if !WITH_VALUE.contains(flag) {
                continue;
            }
            let glued: String = w.chars().skip(2).collect();
            let value = if glued.is_empty() {
                i += 1;
                parts.get(i - 1).cloned().unwrap_or_default()
            } else {
                glued
            };
            match flag {
                'p' => port = Some(value),
                'i' => identity_file = Some(value),
                'l' => user = Some(value),
                _ => {}
            }
            continue;
        }
        // The first plain word is the server; a second starts the command to run there.
        if target.is_some() {
            break;
        }
        target = Some(w.clone());
    }
    let t = split_target(target.as_deref().unwrap_or(""));
    Parsed { host: t.host, user: t.user.or(user), port: t.port.or(port), identity_file }
}

/// How the form signs in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SignIn {
    /// Your usual keys, or the key file given.
    #[default]
    Key,
    Password,
}

/// The "Add a server" form, as typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    /// A host, or anything [`parse_address`] reads.
    pub address: String,
    pub port: String,
    pub user: String,
    /// What it's called in the list; empty: its address.
    pub name: String,
    pub sign_in: SignIn,
    /// A key file; empty: your usual keys.
    pub key_file: String,
    pub password: Password,
    /// Keep the password in the system keychain, or only until the app quits.
    pub remember: bool,
}

/// A server to add: what [`Form::check`] makes of a form, and what an import adds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewServer {
    pub name: Option<String>,
    /// `user@host:port`, as sshee-core's list takes it.
    pub address: String,
    pub identity_file: Option<String>,
    pub password: Option<Password>,
    pub remember_password: bool,
}

/// Why a port isn't one.
pub const BAD_PORT: &str = "The port is a number from 1 to 65535.";
/// The address box is empty.
pub const NO_ADDRESS: &str = "Enter a server address, like root@203.0.113.10";
/// The address isn't a host.
pub const BAD_ADDRESS: &str = "That doesn't look like a server address";

impl Form {
    /// A form that remembers its password, as 0.10's starts.
    pub fn new() -> Form {
        Form { remember: true, ..Form::default() }
    }

    /// The address box as its parts, with the form's own port and user where the box gives
    /// none: what the server would be, as typed so far.
    pub fn parsed(&self) -> Parsed {
        let mut p = parse_address(&self.address);
        let own = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        p.user = p.user.filter(|u| !u.is_empty()).or_else(|| own(&self.user));
        p.port = p.port.or_else(|| own(&self.port));
        p.identity_file = p.identity_file.or_else(|| own(&self.key_file));
        p
    }

    /// Whether there's enough to try: a host.
    pub fn ready(&self) -> bool {
        !self.parsed().host.trim().is_empty()
    }

    /// The server this form describes, or why it can't be added, in words for the form.
    pub fn check(&self) -> Result<NewServer, String> {
        let p = self.parsed();
        let host = p.host.trim();
        if host.is_empty() {
            return Err(NO_ADDRESS.into());
        }
        if host.contains(char::is_whitespace) || host.contains('@') {
            return Err(BAD_ADDRESS.into());
        }
        // Colons are an IPv6 address (with its zone, if any) or a mistake.
        let v6 = host.contains(':');
        if v6 && host.split('%').next().unwrap_or("").parse::<std::net::Ipv6Addr>().is_err() {
            return Err(BAD_ADDRESS.into());
        }
        let port = match p.port.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(port) => Some(port.parse::<u16>().ok().filter(|p| *p > 0).ok_or(BAD_PORT)?),
            None => None,
        };
        let mut address = String::new();
        if let Some(user) = p.user.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            if user.contains(char::is_whitespace) {
                return Err("A user name has no spaces.".into());
            }
            address.push_str(user);
            address.push('@');
        }
        // An IPv6 address goes in brackets, so its colons aren't taken for a port's.
        if v6 {
            address.push_str(&format!("[{host}]"));
        } else {
            address.push_str(host);
        }
        if let Some(port) = port.filter(|p| *p != 22) {
            address.push_str(&format!(":{port}"));
        }
        let password = (self.sign_in == SignIn::Password && !self.password.is_empty()).then(|| self.password.clone());
        Ok(NewServer {
            name: Some(self.name.trim().to_owned()).filter(|n| !n.is_empty()),
            address,
            identity_file: if self.sign_in == SignIn::Key { p.identity_file.filter(|k| !k.trim().is_empty()) } else { None },
            remember_password: self.remember && password.is_some(),
            password,
        })
    }
}

/// Where servers can be brought in from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// `~/.ssh/config`: its hosts are in the list already; you choose which show.
    SshConfig,
    /// XPipe's vault on this computer.
    Xpipe,
    /// The servers 0.10 saved, with their labels and looks.
    Old,
}

impl Source {
    /// The row that opens it in "Add a server".
    pub fn label(self) -> &'static str {
        match self {
            Source::SshConfig => "From ~/.ssh/config",
            Source::Xpipe => "Import from XPipe",
            Source::Old => "Bring over your servers from 0.10",
        }
    }

    /// The line under its list: what happens, and what doesn't.
    pub fn promise(self) -> &'static str {
        match self {
            Source::SshConfig => "Choose which ones show up in SSHeesh. Your config file isn't changed.",
            Source::Xpipe => "Pick the ones to add. XPipe isn't changed.",
            Source::Old => "Pick the ones to add, with their labels and icons. SSHeesh 0.10 keeps its own list.",
        }
    }
}

/// A server one of the sources offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidate {
    /// What names it within its source: a host id for `~/.ssh/config` and 0.10, the address
    /// for XPipe.
    pub id: String,
    pub name: String,
    pub address: String,
    pub identity_file: Option<String>,
    /// Its source has a password for it, which stays there.
    pub had_password: bool,
    /// Ticked when the list opens.
    pub picked: bool,
}

/// What the sources hold that isn't in the list yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sources {
    /// Every host of `~/.ssh/config`; `picked` on the ones that show now, and on the ones
    /// not decided yet.
    pub config: Vec<Candidate>,
    /// How many of `config` you haven't said yes or no to.
    pub config_new: usize,
    pub xpipe: Vec<Candidate>,
    /// XPipe's servers that are here already, its encrypted entries, and the ones that go
    /// through another server.
    pub xpipe_here: usize,
    pub xpipe_locked: usize,
    pub xpipe_through_jump: usize,
    pub old: Vec<Candidate>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Sources {
    pub fn of(&self, source: Source) -> &[Candidate] {
        match source {
            Source::SshConfig => &self.config,
            Source::Xpipe => &self.xpipe,
            Source::Old => &self.old,
        }
    }

    /// The sources worth a row in "Add a server", each with how many it offers.
    pub fn offers(&self) -> Vec<(Source, String)> {
        let mut out = Vec::new();
        if !self.old.is_empty() {
            out.push((Source::Old, plural(self.old.len(), "server", "servers")));
        }
        if !self.config.is_empty() {
            out.push((Source::SshConfig, if self.config_new > 0 { format!("{} new", self.config_new) } else { plural(self.config.len(), "server", "servers") }));
        }
        if !self.xpipe.is_empty() {
            out.push((Source::Xpipe, plural(self.xpipe.len(), "new server", "new servers")));
        }
        out
    }

    /// A source's list title.
    pub fn title(&self, source: Source) -> String {
        let n = self.of(source).len();
        match source {
            Source::SshConfig => "Servers from ~/.ssh/config".into(),
            Source::Xpipe => format!("Found {} in XPipe", plural(n, "new server", "new servers")),
            Source::Old => format!("Found {} in SSHeesh 0.10", plural(n, "server", "servers")),
        }
    }

    /// The small print under a source's list (0.10's notes under XPipe's).
    pub fn notes(&self, source: Source) -> Vec<String> {
        match source {
            Source::SshConfig => vec![],
            Source::Old => vec!["Saved passwords stay in 0.10's keychain: a server that needs one asks the first time.".into()],
            Source::Xpipe => {
                let mut notes = Vec::new();
                if self.xpipe_here > 0 {
                    notes.push(format!("{} in SSHeesh already.", plural(self.xpipe_here, "server is", "servers are")));
                }
                if self.xpipe_through_jump > 0 {
                    let n = self.xpipe_through_jump;
                    notes.push(format!(
                        "{} through another server, which SSHeesh can't do yet, so {} out.",
                        plural(n, "server goes", "servers go"),
                        if n == 1 { "it stays" } else { "they stay" }
                    ));
                }
                if self.xpipe_locked > 0 {
                    let n = self.xpipe_locked;
                    notes.push(format!("{} encrypted, and only XPipe can open {}.", plural(n, "entry is", "entries are"), if n == 1 { "it" } else { "them" }));
                }
                notes.push("Passwords stay in XPipe: a server that needs one asks the first time.".into());
                notes
            }
        }
    }
}

/// The button that finishes a source's list, for `picked` ticks.
pub fn import_button(source: Source, picked: usize) -> String {
    match source {
        Source::SshConfig => "Done".into(),
        _ if picked == 0 => "Add none".into(),
        _ => format!("Add {}", plural(picked, "server", "servers")),
    }
}

/// What to say after an import: how many came, which didn't, and who'll ask for a password.
pub fn imported_note(source: Source, added: usize, failed: &[String], with_password: usize) -> String {
    let from = match source {
        Source::SshConfig => return format!("{} from ~/.ssh/config.", plural(added, "server shows", "servers show")),
        Source::Xpipe => "XPipe",
        Source::Old => "SSHeesh 0.10",
    };
    let mut note = format!("Added {} from {from}.", plural(added, "server", "servers"));
    if !failed.is_empty() {
        note.push_str(&format!(" Couldn't add {}.", failed.join(", ")));
    }
    if with_password > 0 {
        note.push_str(&format!(" {} for a password the first time.", plural(with_password, "server will ask", "servers will ask")));
    }
    note
}

/// The question before a server leaves the list: its title, what it means, and the button.
pub fn removal(name: &str, from_config: bool) -> (String, &'static str, &'static str) {
    if from_config {
        (format!("Hide {name}?"), "It stays in ~/.ssh/config. Add a server, then From ~/.ssh/config, brings it back.", "Hide")
    } else {
        (format!("Remove {name}?"), "Only removes it from SSHeesh. Nothing changes on the server.", "Remove")
    }
}

/// What the window asks its backend to do with the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Look at the sources: `~/.ssh/config`, XPipe's vault, 0.10's list.
    Sources,
    /// Save a server typed into the form (its password goes to this copy's keychain name).
    Add(NewServer),
    /// Bring in the candidates with these ids; `offered` are all the ids the list showed.
    Import { source: Source, picks: Vec<String>, offered: Vec<String> },
    Rename { server: ServerKey, name: String },
    SetIcon { server: ServerKey, icon: String },
    SetColor { server: ServerKey, color: String },
    SetEnv { server: ServerKey, env: Option<EnvLabel> },
    /// Take a server out of the list: a saved one is removed and its password forgotten, one
    /// from `~/.ssh/config` is hidden.
    Remove { server: ServerKey },
}

/// What the backend says back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Sources(Sources),
    /// The server is in the list; `warning` when something went half right (the keychain
    /// refused the password, say).
    Added { server: ServerKey, name: String, warning: Option<String> },
    Imported { source: Source, note: String },
    /// A server left the list.
    Removed { server: ServerKey },
    /// A name, a look or a label changed; the list that comes with it says the rest.
    Changed,
    /// It didn't work, in words for the screen that asked.
    Failed(String),
}

/// A server's host id from its key in the sidebar (`host:<id>`); `None` for this computer.
pub fn host_id(server: &str) -> Option<&str> {
    server.strip_prefix("host:")
}

/// The list kept in memory: what a demo and the tests answer an [`Ask`] from. The program's
/// own list does the same on files and the keychain. Nothing here waits, reads or writes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Book {
    pub hosts: Vec<Listed>,
    pub looks: Looks,
    /// What XPipe and 0.10 offer (`xpipe`, `old` and the counts); `~/.ssh/config`'s part is
    /// made from `hosts` each time it's asked for.
    pub sources: Sources,
    /// How many servers were added here, for their ids.
    added: u32,
}

impl Book {
    pub fn new(hosts: Vec<Listed>, looks: Looks, sources: Sources) -> Book {
        Book { hosts, looks, sources, added: 0 }
    }

    /// The sidebar's servers.
    pub fn servers(&self) -> Vec<Server> {
        self.looks.servers(&self.hosts)
    }

    pub fn colors(&self) -> IndexMap<ServerKey, String> {
        self.looks.colors(&self.hosts)
    }

    /// Servers in `~/.ssh/config` not said yes or no to yet.
    pub fn config_new(&self) -> usize {
        self.looks.undecided(&self.hosts).len()
    }

    /// What the sources hold now.
    pub fn offered(&self) -> Sources {
        let config = self
            .hosts
            .iter()
            .filter(|h| h.from_config)
            .map(|h| Candidate { id: h.id.clone(), name: h.name.clone(), address: h.address.clone(), picked: self.looks.shown(h) || !self.looks.labels.config_seen.contains(&h.id), ..Candidate::default() })
            .collect();
        Sources { config, config_new: self.config_new(), ..self.sources.clone() }
    }

    fn listed(&self, server: &str) -> Option<&Listed> {
        let id = host_id(server)?;
        self.hosts.iter().find(|h| h.id == id)
    }

    fn add(&mut self, name: Option<String>, address: &str) -> String {
        self.added += 1;
        let id = format!("saved:new{}", self.added);
        let name = name.unwrap_or_else(|| parse_address(address).host);
        self.hosts.push(Listed { id: id.clone(), name, address: address.to_owned(), from_config: false });
        id
    }

    /// Does what is asked, at once.
    pub fn handle(&mut self, ask: Ask) -> Answer {
        match ask {
            Ask::Sources => Answer::Sources(self.offered()),
            Ask::Add(new) => {
                let id = self.add(new.name.clone(), &new.address);
                let name = self.hosts.last().map(|h| h.name.clone()).unwrap_or_default();
                Answer::Added { server: format!("host:{id}"), name, warning: None }
            }
            Ask::Import { source: Source::SshConfig, picks, offered } => {
                self.looks.choose_config(&offered, &picks);
                Answer::Imported { source: Source::SshConfig, note: imported_note(Source::SshConfig, picks.len(), &[], 0) }
            }
            Ask::Import { source, picks, .. } => {
                let pool = if source == Source::Xpipe { &mut self.sources.xpipe } else { &mut self.sources.old };
                let (chosen, left): (Vec<Candidate>, Vec<Candidate>) = std::mem::take(pool).into_iter().partition(|c| picks.contains(&c.id));
                *pool = left;
                if source == Source::Xpipe {
                    self.sources.xpipe_here += chosen.len();
                }
                let with_password = chosen.iter().filter(|c| c.had_password).count();
                for c in &chosen {
                    self.add(Some(c.name.clone()), &c.address);
                }
                Answer::Imported { source, note: imported_note(source, chosen.len(), &[], with_password) }
            }
            Ask::Rename { server, name } => {
                if let Some(host) = self.listed(&server).cloned() {
                    self.looks.rename(&host, &name);
                }
                Answer::Changed
            }
            Ask::SetIcon { server, icon } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_icon(id, &icon);
                }
                Answer::Changed
            }
            Ask::SetColor { server, color } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_color(id, &color);
                }
                Answer::Changed
            }
            Ask::SetEnv { server, env } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_env(id, env);
                }
                Answer::Changed
            }
            Ask::Remove { server } => {
                let Some(host) = self.listed(&server).cloned() else { return Answer::Failed("That server isn't in your list anymore.".into()) };
                if host.from_config {
                    self.looks.hide(&host.id);
                } else {
                    self.hosts.retain(|h| h.id != host.id);
                    self.looks.forget(&host.id);
                }
                Answer::Removed { server }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(host: &str, user: Option<&str>, port: Option<&str>, key: Option<&str>) -> Parsed {
        Parsed { host: host.into(), user: user.map(str::to_owned), port: port.map(str::to_owned), identity_file: key.map(str::to_owned) }
    }

    #[test]
    fn an_address_is_read_however_it_is_pasted() {
        assert_eq!(parse_address("203.0.113.10"), parsed("203.0.113.10", None, None, None));
        assert_eq!(parse_address(" root@203.0.113.10 "), parsed("203.0.113.10", Some("root"), None, None));
        assert_eq!(parse_address("web-7731:2222"), parsed("web-7731", None, Some("2222"), None));
        assert_eq!(parse_address("ssh://deploy@web-7731:2222/"), parsed("web-7731", Some("deploy"), Some("2222"), None));
        assert_eq!(parse_address("[2001:db8::7]:2200"), parsed("2001:db8::7", None, Some("2200"), None));
        assert_eq!(parse_address("deploy@2001:db8::7"), parsed("2001:db8::7", Some("deploy"), None, None));
        // A port that isn't digits stays part of what was typed, for the check to refuse.
        assert_eq!(parse_address("web-7731:ssh"), parsed("web-7731:ssh", None, None, None));
    }

    #[test]
    fn a_whole_ssh_command_is_taken_apart() {
        assert_eq!(parse_address("ssh root@203.0.113.10 -p 2222"), parsed("203.0.113.10", Some("root"), Some("2222"), None));
        assert_eq!(parse_address("ssh -p2222 -i ~/.ssh/shop_key -l deploy web-7731"), parsed("web-7731", Some("deploy"), Some("2222"), Some("~/.ssh/shop_key")));
        // Flags without a value are skipped, quoted values keep their spaces, a command after
        // the server is left out.
        assert_eq!(
            parse_address("ssh -A -o \"StrictHostKeyChecking no\" -i 'my keys/id' deploy@web-7731 uptime -p 9"),
            parsed("web-7731", Some("deploy"), None, Some("my keys/id"))
        );
        assert_eq!(parse_address("ssh"), parsed("", None, None, None));
    }

    #[test]
    fn a_form_becomes_a_server_with_the_address_as_the_list_takes_it() {
        let mut form = Form::new();
        form.address = "203.0.113.10".into();
        form.user = "root".into();
        form.port = "2222".into();
        form.name = " Shop ".into();
        let server = form.check().unwrap();
        assert_eq!(server.address, "root@203.0.113.10:2222");
        assert_eq!(server.name.as_deref(), Some("Shop"));
        assert_eq!((server.password, server.remember_password, server.identity_file), (None, false, None));
        // The box's own user and port win over the fields; 22 isn't written; no name: none.
        form.address = "ssh deploy@web-7731 -p 22".into();
        form.name.clear();
        let server = form.check().unwrap();
        assert_eq!((server.address.as_str(), server.name), ("deploy@web-7731", None));
        // IPv6 goes in brackets.
        form.address = "2001:db8::7".into();
        form.port = "2200".into();
        form.user.clear();
        assert_eq!(form.check().unwrap().address, "[2001:db8::7]:2200");
    }

    #[test]
    fn a_form_signs_in_with_a_key_file_or_a_password_never_both() {
        let mut form = Form::new();
        form.address = "web-7731".into();
        form.key_file = "~/.ssh/shop_key".into();
        form.password = Password::new("made-up-password");
        assert_eq!(form.check().unwrap().identity_file.as_deref(), Some("~/.ssh/shop_key"));
        assert_eq!(form.check().unwrap().password, None);
        form.sign_in = SignIn::Password;
        let server = form.check().unwrap();
        assert_eq!(server.identity_file, None);
        assert_eq!(server.password.as_ref().map(Password::expose), Some("made-up-password"));
        assert!(server.remember_password);
        form.remember = false;
        assert!(!form.check().unwrap().remember_password);
        // No password typed: the server asks the first time.
        form.password = Password::default();
        form.remember = true;
        let server = form.check().unwrap();
        assert_eq!((server.password, server.remember_password), (None, false));
        // A password never shows in a debug print.
        assert!(!format!("{:?}", Password::new("made-up-password")).contains("made-up"));
    }

    #[test]
    fn a_form_says_what_is_wrong_in_plain_words() {
        let mut form = Form::new();
        assert!(!form.ready());
        assert_eq!(form.check().unwrap_err(), NO_ADDRESS);
        form.address = "ssh -p 22".into();
        assert_eq!(form.check().unwrap_err(), NO_ADDRESS);
        form.address = "web 7731".into();
        assert_eq!(form.check().unwrap_err(), BAD_ADDRESS);
        form.address = "web-7731:ssh".into();
        assert_eq!(form.check().unwrap_err(), BAD_ADDRESS);
        form.address = "web-7731".into();
        assert!(form.ready());
        for port in ["0", "65536", "ssh", "-1", "22.5"] {
            form.port = port.into();
            assert_eq!(form.check().unwrap_err(), BAD_PORT, "{port}");
        }
        form.port = " 65535 ".into();
        assert_eq!(form.check().unwrap().address, "web-7731:65535");
        form.user = "de ploy".into();
        assert!(form.check().is_err());
    }

    #[test]
    fn the_sources_say_what_they_offer_and_what_stays_out() {
        let c = |name: &str| Candidate { id: name.into(), name: name.into(), address: name.into(), ..Candidate::default() };
        let mut s = Sources::default();
        assert!(s.offers().is_empty());
        s.config = vec![c("web-7731"), c("old-box")];
        s.config_new = 1;
        s.xpipe = vec![c("db-main")];
        s.xpipe_here = 2;
        s.xpipe_locked = 1;
        s.xpipe_through_jump = 3;
        s.old = vec![c("backups"), c("web-staging")];
        let offers: Vec<(Source, String)> = s.offers();
        assert_eq!(offers, [(Source::Old, "2 servers".to_owned()), (Source::SshConfig, "1 new".to_owned()), (Source::Xpipe, "1 new server".to_owned())]);
        assert_eq!(s.title(Source::Xpipe), "Found 1 new server in XPipe");
        assert_eq!(s.title(Source::Old), "Found 2 servers in SSHeesh 0.10");
        assert_eq!(
            s.notes(Source::Xpipe),
            [
                "2 servers are in SSHeesh already.",
                "3 servers go through another server, which SSHeesh can't do yet, so they stay out.",
                "1 entry is encrypted, and only XPipe can open it.",
                "Passwords stay in XPipe: a server that needs one asks the first time.",
            ]
        );
        assert_eq!(import_button(Source::Xpipe, 2), "Add 2 servers");
        assert_eq!(import_button(Source::Old, 0), "Add none");
        assert_eq!(import_button(Source::SshConfig, 0), "Done");
        assert_eq!(imported_note(Source::Xpipe, 1, &[], 0), "Added 1 server from XPipe.");
        assert_eq!(
            imported_note(Source::Old, 2, &["db-main".into()], 1),
            "Added 2 servers from SSHeesh 0.10. Couldn't add db-main. 1 server will ask for a password the first time."
        );
        assert_eq!(imported_note(Source::SshConfig, 3, &[], 0), "3 servers show from ~/.ssh/config.");
    }

    #[test]
    fn the_list_in_memory_does_what_is_asked() {
        let listed = |id: &str, name: &str, from_config: bool| Listed { id: id.into(), name: name.into(), address: format!("deploy@{name}"), from_config };
        let c = |name: &str, had_password: bool| Candidate { id: name.into(), name: name.into(), address: format!("root@{name}"), had_password, picked: true, ..Candidate::default() };
        let sources = Sources { xpipe: vec![c("backups", true), c("mail", false)], old: vec![c("db-main", false)], ..Sources::default() };
        let mut book = Book::new(vec![listed("saved:1", "web-7731", false), listed("cfg:old-box", "old-box", true)], Looks::default(), sources);
        let names = |book: &Book| book.servers().iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&book), ["web-7731"]);
        assert_eq!(book.config_new(), 1);
        // The sources: the config's host is new, so it comes ticked.
        let Answer::Sources(offered) = book.handle(Ask::Sources) else { panic!() };
        assert_eq!((offered.config.len(), offered.config[0].picked, offered.config_new, offered.xpipe.len(), offered.old.len()), (1, true, 1, 2, 1));
        // By hand.
        let added = book.handle(Ask::Add(NewServer { name: None, address: "root@203.0.113.40:2222".into(), ..NewServer::default() }));
        assert_eq!(added, Answer::Added { server: "host:saved:new1".into(), name: "203.0.113.40".into(), warning: None });
        // From the config, XPipe and 0.10.
        book.handle(Ask::Import { source: Source::SshConfig, picks: vec!["cfg:old-box".into()], offered: vec!["cfg:old-box".into()] });
        let from_xpipe = book.handle(Ask::Import { source: Source::Xpipe, picks: vec!["backups".into()], offered: vec![] });
        assert_eq!(from_xpipe, Answer::Imported { source: Source::Xpipe, note: "Added 1 server from XPipe. 1 server will ask for a password the first time.".into() });
        book.handle(Ask::Import { source: Source::Old, picks: vec!["db-main".into()], offered: vec![] });
        assert_eq!(names(&book), ["web-7731", "old-box", "203.0.113.40", "backups", "db-main"]);
        let Answer::Sources(offered) = book.handle(Ask::Sources) else { panic!() };
        assert_eq!((offered.config_new, offered.xpipe.len(), offered.xpipe_here, offered.old.len()), (0, 1, 1, 0));
        // Looks and labels.
        assert_eq!(book.handle(Ask::Rename { server: "host:saved:1".into(), name: "Shop".into() }), Answer::Changed);
        book.handle(Ask::SetIcon { server: "host:saved:1".into(), icon: "globe".into() });
        book.handle(Ask::SetColor { server: "host:saved:1".into(), color: "#7e8590".into() });
        book.handle(Ask::SetEnv { server: "host:saved:1".into(), env: Some(EnvLabel::Prod) });
        let shop = &book.servers()[0];
        assert_eq!((shop.name.as_str(), shop.icon.as_str(), shop.is_prod()), ("Shop", "globe", true));
        assert_eq!(book.colors()["host:saved:1"], "#7e8590");
        // Leaving: a saved server goes with what was kept about it; one from the config hides.
        assert_eq!(book.handle(Ask::Remove { server: "host:saved:1".into() }), Answer::Removed { server: "host:saved:1".into() });
        book.handle(Ask::Remove { server: "host:cfg:old-box".into() });
        assert_eq!(names(&book), ["203.0.113.40", "backups", "db-main"]);
        assert!(book.looks.names.is_empty() && book.hosts.iter().any(|h| h.id == "cfg:old-box"));
        assert!(matches!(book.handle(Ask::Remove { server: "host:gone".into() }), Answer::Failed(_)));
        assert_eq!(host_id("local"), None);
    }

    #[test]
    fn leaving_the_list_is_asked_by_name() {
        let (title, body, button) = removal("web-7731", false);
        assert_eq!((title.as_str(), button), ("Remove web-7731?", "Remove"));
        assert!(body.contains("Nothing changes on the server"));
        let (title, _, button) = removal("old-box", true);
        assert_eq!((title.as_str(), button), ("Hide old-box?", "Hide"));
    }
}
