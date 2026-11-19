//! Adding and managing servers, without a screen and without ssh: what the "Add a server" form
//! takes and refuses, what the three imports offer (`~/.ssh/config`, XPipe, 0.10's own list),
//! and the words for what happened. A port of the rules in 0.10's `AddServerDialog.tsx`,
//! `src/lib/address.ts`, `ConfigImport.tsx` and `XpipeImport.tsx`.
//!
//! The window asks its backend with an [`Ask`] and hears back an [`Answer`]: saving, the
//! keychain and reading other programs' files all wait, so they never run on the UI thread.
//! The program does them on sshee-core; a demo and the tests answer from memory.
//!
//! The form also offers the keys in `~/.ssh` and the password managers that hold keys
//! ([`LocalAuth`], 0.10's `local_auth`), and checks a server before it's added ([`Tested`],
//! 0.10's `test_connection`): does it answer, is its identity known, does signing in work, is
//! tmux there.

use crate::labels::{Face, Listed, Look, Looks, Picture};
use crate::model::{EnvLabel, HostLook};

/// An icon, a colour or a picture picked in a form (0.10's `HostLook`, without what this
/// version doesn't know).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickedLook {
    pub icon: String,
    /// `#rrggbb`.
    pub color: String,
    /// A data URL ([`crate::labels::image_data_url`]).
    pub image: Option<String>,
}

impl PickedLook {
    /// As the store keeps it.
    pub fn kept(&self) -> HostLook {
        HostLook { icon: self.icon.clone(), color: self.color.clone(), image: self.image.clone(), ..HostLook::default() }
    }
}
use crate::workbench::{Server, ServerKey};
use indexmap::IndexMap;
use std::fmt;

/// A password on its way to the keychain. Never printed: its `Debug` is dots. Wiped from
/// memory when dropped.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Password(String);

impl Drop for Password {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

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
    /// Your usual keys, one of the keys in `~/.ssh`, or a key file given.
    #[default]
    Key,
    Password,
    /// A password manager that holds keys (1Password, Bitwarden): ssh asks it through its
    /// socket, and you approve each sign-in there.
    Manager,
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
    /// The password manager's socket, when signing in with one ([`KeyAgent::socket`]).
    pub agent: String,
    /// The icon, colour or picture picked in the form; `None`: the server gets the icon it's
    /// guessed to need and a colour of its own.
    pub look: Option<PickedLook>,
    pub env: Option<EnvLabel>,
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
    /// A password manager's socket for ssh's `IdentityAgent`.
    pub identity_agent: Option<String>,
    /// Its look and label in the list, given right away.
    pub look: Option<PickedLook>,
    pub env: Option<EnvLabel>,
}

/// What connecting to a form's server depends on ([`Form::connection`]): host, user, port, way
/// of signing in, key file, password manager, and the password's length.
pub type Connection = (String, String, String, SignIn, String, String, usize);

/// Why a port isn't one.
pub const BAD_PORT: &str = "The port is a number from 1 to 65535.";
/// The address box is empty.
pub const NO_ADDRESS: &str = "Enter a server address, like root@203.0.113.10";
/// The address isn't a host.
pub const BAD_ADDRESS: &str = "That doesn't look like a server address";
/// "Password manager" chosen, and none of them picked.
pub const NO_MANAGER: &str = "Choose the password manager that holds the key.";

/// A key pair in `~/.ssh` (sshee-core's `keys::LocalKey`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalKey {
    /// `~/.ssh/id_ed25519`.
    pub path: String,
    pub name: String,
    /// "ED25519", "RSA", "ED25519-SK" (a hardware key)…
    pub kind: String,
    pub comment: String,
}

/// A key agent running on this computer (sshee-core's `keys::KeyAgent`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyAgent {
    /// "ssh-agent", "1Password", "Bitwarden".
    pub name: String,
    /// The socket to point `IdentityAgent` at; `None` for the system's own agent.
    pub socket: Option<String>,
    /// How many keys it holds now, if it answered.
    pub keys: Option<usize>,
}

/// The keys in `~/.ssh` and the key agents on this computer, for "Sign in with".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalAuth {
    pub keys: Vec<LocalKey>,
    pub agents: Vec<KeyAgent>,
}

impl LocalAuth {
    /// The password managers: agents with a socket of their own.
    pub fn managers(&self) -> Vec<&KeyAgent> {
        self.agents.iter().filter(|a| a.socket.is_some()).collect()
    }

    /// The line under "Your usual keys".
    pub fn usual_keys_note(&self) -> String {
        match self.agents.iter().find(|a| a.socket.is_none()).and_then(|a| a.keys).filter(|n| *n > 0) {
            Some(n) => format!("ssh picks one, including {} loaded in ssh-agent", plural(n, "key", "keys")),
            None => "ssh picks from the keys in ~/.ssh".into(),
        }
    }

    /// The line under a key: its comment, else where it is.
    pub fn key_note(key: &LocalKey) -> &str {
        if key.comment.trim().is_empty() {
            &key.path
        } else {
            &key.comment
        }
    }

    /// The line under a password manager.
    pub fn manager_note(agent: &KeyAgent) -> String {
        match agent.keys {
            None => "Unlock it to use its keys".into(),
            Some(n) => format!("{} · you approve each sign-in there", plural(n, "key", "keys")),
        }
    }
}

/// What a server's identity is to this computer, when a check reached it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyState {
    /// Known and the same as before.
    Known,
    /// Never seen: its fingerprints, `(kind, SHA256:…)`, to compare before trusting it.
    Unknown { fingerprints: Vec<(String, String)> },
    /// Not what it was the last time.
    Changed,
    /// Nothing answered, with ssh's words when it gave any.
    Unreachable { message: Option<String> },
}

/// What "Check before adding" found (0.10's `ConnectionTest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tested {
    pub host_key: HostKeyState,
    pub signed_in: bool,
    /// Whether tmux is there, once signed in.
    pub tmux: Option<bool>,
    /// Why signing in didn't work.
    pub message: Option<String>,
}

/// How a step of the check went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepTone {
    Ok,
    Bad,
    Warn,
    /// A question for you: is this really your server?
    Ask,
}

/// One line of the check's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub tone: StepTone,
    pub title: String,
    pub detail: Option<String>,
    /// A command that fixes it, to copy (tmux's install).
    pub command: Option<String>,
}

/// The command offered when tmux isn't on a server.
pub const INSTALL_TMUX: &str = "sudo apt install tmux";

impl Tested {
    /// The result, one step a line, in 0.10's words.
    pub fn steps(&self) -> Vec<Step> {
        let step = |tone, title: &str, detail: Option<String>| Step { tone, title: title.to_owned(), detail, command: None };
        let mut out = Vec::new();
        match &self.host_key {
            HostKeyState::Unreachable { message } => {
                out.push(step(StepTone::Bad, "Couldn't reach the server", Some(message.clone().unwrap_or_else(|| "Check the address and port, and that the server is on.".into()))));
                return out;
            }
            _ => out.push(step(StepTone::Ok, "The server answered", None)),
        }
        match &self.host_key {
            HostKeyState::Unknown { .. } => out.push(step(StepTone::Ask, "First time here. Is this really your server?", Some("Compare this fingerprint with the one in your hosting provider's console.".into()))),
            HostKeyState::Changed => out.push(step(StepTone::Bad, "This server's identity changed since you last connected", Some("If you didn't reinstall it, don't add it. It may not be your server.".into()))),
            HostKeyState::Known if self.signed_in => out.push(step(StepTone::Ok, "Signed in", None)),
            HostKeyState::Known => out.push(step(StepTone::Bad, "Couldn't sign in", self.message.clone())),
            HostKeyState::Unreachable { .. } => {}
        }
        match self.tmux {
            Some(true) if self.signed_in => out.push(step(StepTone::Ok, "tmux is installed: sessions survive dropped connections", None)),
            Some(false) if self.signed_in => out.push(Step {
                command: Some(INSTALL_TMUX.to_owned()),
                ..step(StepTone::Warn, "tmux isn't installed", Some("Terminals work, but a dropped connection ends them. Install it on the server with:".into()))
            }),
            _ => {}
        }
        out
    }

    /// The fingerprints to compare, when the server is new here.
    pub fn fingerprints(&self) -> &[(String, String)] {
        match &self.host_key {
            HostKeyState::Unknown { fingerprints } => fingerprints,
            _ => &[],
        }
    }

    /// "Trust it and continue" is offered: the server is new here.
    pub fn can_trust(&self) -> bool {
        matches!(self.host_key, HostKeyState::Unknown { .. })
    }
}

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
        let identity_agent = match self.sign_in {
            SignIn::Manager => Some(Some(self.agent.trim().to_owned()).filter(|a| !a.is_empty()).ok_or(NO_MANAGER)?),
            _ => None,
        };
        Ok(NewServer {
            name: Some(self.name.trim().to_owned()).filter(|n| !n.is_empty()),
            address,
            identity_file: if self.sign_in == SignIn::Key { p.identity_file.filter(|k| !k.trim().is_empty()) } else { None },
            remember_password: self.remember && password.is_some(),
            password,
            identity_agent,
            look: self.look.clone(),
            env: self.env.clone(),
        })
    }

    /// The form as far as connecting goes: a check made of it still holds while this is the
    /// same (the name, the look and the label don't matter to it). The password is in it, as
    /// its length: a changed password makes the check stale.
    pub fn connection(&self) -> Connection {
        let p = self.parsed();
        (p.host, p.user.unwrap_or_default(), p.port.unwrap_or_default(), self.sign_in, p.identity_file.unwrap_or_default(), self.agent.clone(), self.password.expose().len())
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
    /// Another SSH app's saved sessions (PuTTY, WinSCP…).
    App(App),
}

/// Another SSH app on this computer whose saved servers can be brought in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum App {
    Putty,
    Kitty,
    WinScp,
    MobaXterm,
    FileZilla,
    SecureCrt,
    Remmina,
}

impl App {
    /// The app's own name, as it writes it.
    pub fn name(self) -> &'static str {
        match self {
            App::Putty => "PuTTY",
            App::Kitty => "KiTTY",
            App::WinScp => "WinSCP",
            App::MobaXterm => "MobaXterm",
            App::FileZilla => "FileZilla",
            App::SecureCrt => "SecureCRT",
            App::Remmina => "Remmina",
        }
    }

    /// A word for keys in the window (`add-from-putty`).
    pub fn key(self) -> &'static str {
        match self {
            App::Putty => "putty",
            App::Kitty => "kitty",
            App::WinScp => "winscp",
            App::MobaXterm => "mobaxterm",
            App::FileZilla => "filezilla",
            App::SecureCrt => "securecrt",
            App::Remmina => "remmina",
        }
    }

    fn label(self) -> &'static str {
        match self {
            App::Putty => "Import from PuTTY",
            App::Kitty => "Import from KiTTY",
            App::WinScp => "Import from WinSCP",
            App::MobaXterm => "Import from MobaXterm",
            App::FileZilla => "Import from FileZilla",
            App::SecureCrt => "Import from SecureCRT",
            App::Remmina => "Import from Remmina",
        }
    }

    fn promise(self) -> &'static str {
        match self {
            App::Putty => "Pick the ones to add. PuTTY isn't changed.",
            App::Kitty => "Pick the ones to add. KiTTY isn't changed.",
            App::WinScp => "Pick the ones to add. WinSCP isn't changed.",
            App::MobaXterm => "Pick the ones to add. MobaXterm isn't changed.",
            App::FileZilla => "Pick the ones to add. FileZilla isn't changed.",
            App::SecureCrt => "Pick the ones to add. SecureCRT isn't changed.",
            App::Remmina => "Pick the ones to add. Remmina isn't changed.",
        }
    }
}

/// What another app offers: its servers not here yet, and what stays out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppOffer {
    pub app: App,
    pub servers: Vec<Candidate>,
    /// Its servers that are here already.
    pub here: usize,
    /// Reached through another server or a proxy, which SSHeesh can't set yet.
    pub through_jump: usize,
    /// Signed in with a PuTTY key (`.ppk`) OpenSSH can't read: they come without it.
    pub ppk_keys: usize,
}

impl Source {
    /// The row that opens it in "Add a server".
    pub fn label(self) -> &'static str {
        match self {
            Source::SshConfig => "From ~/.ssh/config",
            Source::Xpipe => "Import from XPipe",
            Source::Old => "Bring over your servers from 0.10",
            Source::App(app) => app.label(),
        }
    }

    /// The line under its list: what happens, and what doesn't.
    pub fn promise(self) -> &'static str {
        match self {
            Source::SshConfig => "Choose which ones show up in SSHeesh. Your config file isn't changed.",
            Source::Xpipe => "Pick the ones to add. XPipe isn't changed.",
            Source::Old => "Pick the ones to add, with their labels and icons. SSHeesh 0.10 keeps its own list.",
            Source::App(app) => app.promise(),
        }
    }
}

/// A server one of the sources offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidate {
    /// What names it within its source: a host id for `~/.ssh/config` and 0.10, the address
    /// for XPipe and the other apps.
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
    /// The other SSH apps that have servers, in a steady order.
    pub apps: Vec<AppOffer>,
    /// Servers here, brought over from 0.10, whose password 0.10 keeps in its keychain and
    /// this copy doesn't have: what "Use 0.10's saved passwords" would bring over.
    pub old_passwords: usize,
}

/// The question before 0.10's keychain is read: reading it may make the system ask you too,
/// so it's asked first, in plain words.
pub fn old_passwords_question(n: usize) -> String {
    format!("SSHeesh 0.10 saved passwords for {}. Use them here?", plural(n, "server", "servers"))
}

/// What the question means, under it.
pub const OLD_PASSWORDS_MEANING: &str = "They're copied from 0.10's keychain entry to this one's. Your system may ask you to allow it. 0.10 keeps its own.";

/// What bringing 0.10's passwords over did.
pub fn old_passwords_note(copied: usize, failed: &[String]) -> String {
    let mut note = if copied == 0 { "No password was brought over.".to_owned() } else { format!("Brought over the saved passwords of {}.", plural(copied, "server", "servers")) };
    if !failed.is_empty() {
        note.push_str(&format!(" Couldn't read {}'s: {} will ask the first time.", failed.join(", "), if failed.len() == 1 { "it" } else { "they" }));
    }
    note
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
            Source::App(app) => self.app(app).map_or(&[], |o| &o.servers),
        }
    }

    /// What one other app offers.
    pub fn app(&self, app: App) -> Option<&AppOffer> {
        self.apps.iter().find(|o| o.app == app)
    }

    /// The sources worth a row in "Add a server", each with how many it offers.
    pub fn offers(&self) -> Vec<(Source, String)> {
        let mut out = Vec::new();
        if !self.old.is_empty() {
            out.push((Source::Old, plural(self.old.len(), "server", "servers")));
        }
        if !self.config.is_empty() {
            out.push((Source::SshConfig, if self.config_new > 0 { plural(self.config_new, "new server", "new servers") } else { plural(self.config.len(), "server", "servers") }));
        }
        if !self.xpipe.is_empty() {
            out.push((Source::Xpipe, plural(self.xpipe.len(), "new server", "new servers")));
        }
        for offer in self.apps.iter().filter(|o| !o.servers.is_empty()) {
            out.push((Source::App(offer.app), plural(offer.servers.len(), "new server", "new servers")));
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
            Source::App(app) => format!("Found {} in {}", plural(n, "new server", "new servers"), app.name()),
        }
    }

    /// The small print under a source's list (0.10's notes under XPipe's).
    pub fn notes(&self, source: Source) -> Vec<String> {
        match source {
            Source::SshConfig => vec![],
            Source::Old => vec!["Their saved passwords are in 0.10's keychain: SSHeesh asks before it reads them.".into()],
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
            Source::App(app) => {
                let Some(offer) = self.app(app) else { return vec![] };
                let mut notes = Vec::new();
                if offer.here > 0 {
                    notes.push(format!("{} in SSHeesh already.", plural(offer.here, "server is", "servers are")));
                }
                if offer.through_jump > 0 {
                    let n = offer.through_jump;
                    notes.push(format!(
                        "{} through another server or a proxy, which SSHeesh can't do yet, so {} out.",
                        plural(n, "server goes", "servers go"),
                        if n == 1 { "it stays" } else { "they stay" }
                    ));
                }
                if offer.ppk_keys > 0 {
                    let n = offer.ppk_keys;
                    notes.push(format!(
                        "{} a PuTTY key (.ppk), which SSHeesh can't use, so {} without it. PuTTYgen makes an OpenSSH copy: Conversions, then Export OpenSSH key.",
                        plural(n, "server signs in with", "servers sign in with"),
                        if n == 1 { "it comes" } else { "they come" }
                    ));
                }
                if offer.servers.iter().any(|c| c.had_password) {
                    notes.push(format!("Saved passwords stay in {}: a server that needs one asks the first time.", app.name()));
                }
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
        Source::App(app) => app.name(),
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
    /// The keys in `~/.ssh` and the key agents running here, for "Sign in with".
    LocalAuth,
    /// Save a server typed into the form (its password goes to this copy's keychain name), with
    /// the look and label picked there.
    Add(NewServer),
    /// "Check before adding": reach the server, see whether its identity is known, sign in,
    /// look for tmux. Nothing is saved.
    Test(NewServer),
    /// "Trust it and continue": keep the identity the check showed, then check again.
    TrustTest(NewServer),
    /// Bring in the candidates with these ids; `offered` are all the ids the list showed.
    Import { source: Source, picks: Vec<String>, offered: Vec<String> },
    /// You said yes to "Use 0.10's saved passwords": copy them from 0.10's keychain entry.
    UseOldPasswords,
    Rename { server: ServerKey, name: String },
    SetIcon { server: ServerKey, icon: String },
    /// Any colour (`#rrggbb`).
    SetColor { server: ServerKey, color: String },
    /// A picture file of this computer's, made into a small square icon.
    SetImage { server: ServerKey, path: std::path::PathBuf },
    /// The picture goes; the icon is back.
    ClearImage { server: ServerKey },
    /// "Automatic": the guessed icon in the server's own colour.
    Automatic { server: ServerKey },
    /// Look at the server to guess what it's for ([`crate::roles`]): `first` when its
    /// terminal just connected (the backend waits a moment, so the shell goes first), else
    /// "Check the server now" from the picker.
    Detect { server: ServerKey, first: bool },
    SetEnv { server: ServerKey, env: Option<EnvLabel> },
    /// "Forget saved password": out of the keychain, and the server asks again.
    ForgetPassword { server: ServerKey },
    /// "Save password…", or the password typed once while a terminal signs in: kept for this
    /// run, and in the keychain when `remember` is set, so the server stops asking. Any
    /// server, one from `~/.ssh/config` too.
    SavePassword { server: ServerKey, password: Password, remember: bool },
    /// Take a server out of the list: a saved one is removed and its password forgotten, one
    /// from `~/.ssh/config` is hidden.
    Remove { server: ServerKey },
    /// The sidebar's servers, in the order they were dragged into.
    Reorder { order: Vec<ServerKey> },
}

impl Ask {
    /// What the window was doing, for the words over an error ("Couldn't rename").
    pub fn doing(&self) -> &'static str {
        match self {
            Ask::Sources | Ask::LocalAuth => "Couldn't look for servers",
            Ask::Add(_) => "Couldn't add the server",
            Ask::Test(_) | Ask::TrustTest(_) => "Couldn't check the server",
            Ask::Import { .. } => "Couldn't bring the servers in",
            Ask::UseOldPasswords => "Couldn't bring the passwords over",
            Ask::Rename { .. } => "Couldn't rename the server",
            Ask::SetIcon { .. } | Ask::SetColor { .. } | Ask::Automatic { .. } | Ask::ClearImage { .. } => "Couldn't change the server's look",
            Ask::SetImage { .. } => "Couldn't use that picture",
            Ask::Detect { .. } => "Couldn't check the server",
            Ask::SetEnv { .. } => "Couldn't change the label",
            Ask::ForgetPassword { .. } => "Couldn't forget the password",
            Ask::SavePassword { .. } => "Couldn't save the password",
            Ask::Remove { .. } => "Couldn't remove the server",
            Ask::Reorder { .. } => "Couldn't keep the servers' order",
        }
    }

    /// The server a menu's ask is about.
    pub fn server(&self) -> Option<&str> {
        match self {
            Ask::Rename { server, .. }
            | Ask::SetIcon { server, .. }
            | Ask::SetColor { server, .. }
            | Ask::SetImage { server, .. }
            | Ask::ClearImage { server }
            | Ask::Automatic { server }
            | Ask::Detect { server, .. }
            | Ask::SetEnv { server, .. }
            | Ask::ForgetPassword { server }
            | Ask::SavePassword { server, .. }
            | Ask::Remove { server } => Some(server),
            _ => None,
        }
    }
}

/// What the backend says back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Sources(Sources),
    LocalAuth(LocalAuth),
    /// The server is in the list; `warning` when something went half right (the keychain
    /// refused the password, say).
    Added { server: ServerKey, name: String, warning: Option<String> },
    /// What "Check before adding" found.
    Tested(Tested),
    /// `old_passwords`: of the servers brought over, how many have a password 0.10 saved,
    /// which SSHeesh offers to use (it asks first).
    Imported { source: Source, note: String, old_passwords: usize },
    /// A server left the list.
    Removed { server: ServerKey },
    /// A name, a look or a label changed; the list that comes with it says the rest.
    Changed,
    /// A look at the server is over: `icon` what it guessed, `None` when it couldn't tell.
    Detected { server: ServerKey, icon: Option<String> },
    /// Done, with something to say ("Forgot the saved password for web-7731.").
    Note(String),
    /// It didn't work, in words for the screen that asked.
    Failed(String),
    /// A change from a server's menu didn't work: what was being done ([`Ask::doing`]) and
    /// why, to show where the menu was even after it closed.
    Refused { server: ServerKey, doing: String, why: String },
}

impl Answer {
    /// The answer to `ask` when it didn't work: a menu's change says which server and what, the
    /// rest say why to the dialog that asked.
    pub fn failed(ask: &Ask, why: impl Into<String>) -> Answer {
        match ask.server() {
            Some(server) => Answer::Refused { server: server.to_owned(), doing: ask.doing().to_owned(), why: why.into() },
            None => Answer::Failed(why.into()),
        }
    }
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
    /// The keys and agents "found" here.
    pub local: LocalAuth,
    /// Host ids with a saved password.
    pub passwords: Vec<String>,
    /// Addresses whose identity is known, for the check.
    pub trusted: Vec<String>,
    /// How many servers were added here, for their ids.
    added: u32,
}

/// The made-up fingerprint a [`Book`] shows for a server it doesn't know yet.
pub const MADE_UP_FINGERPRINT: &str = "SHA256:Zb2m0Wf7Kq4YJm1Xc8vT3n5hRk9pLs6dQa1uEw0yVgI";

impl Book {
    pub fn new(hosts: Vec<Listed>, looks: Looks, sources: Sources) -> Book {
        Book { hosts, looks, sources, ..Book::default() }
    }

    /// The sidebar's servers.
    pub fn servers(&self) -> Vec<Server> {
        self.looks.servers(&self.hosts)
    }

    pub fn colors(&self) -> IndexMap<ServerKey, String> {
        self.looks.colors(&self.hosts)
    }

    /// Each shown server's look, picture and saved password, its picture made by `picture`.
    pub fn faces(&self, picture: impl Fn(&Look) -> Option<Picture>) -> IndexMap<ServerKey, Face> {
        faces(&self.looks.shown_looks(&self.hosts), |key| host_id(key).is_some_and(|id| self.passwords.iter().any(|p| p == id)), picture)
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

    /// The check, made up: a server it hasn't been told to trust shows a fingerprint; one
    /// named `…down…` doesn't answer; a password of "wrong" doesn't sign in; one named
    /// `…bare…` has no tmux.
    fn test(&self, new: &NewServer) -> Tested {
        let host = parse_address(&new.address).host;
        if host.contains("down") {
            return Tested { host_key: HostKeyState::Unreachable { message: Some(format!("ssh: connect to host {host} port 22: Connection timed out")) }, signed_in: false, tmux: None, message: None };
        }
        if !self.trusted.contains(&host) {
            let fingerprints = vec![("ED25519".to_owned(), MADE_UP_FINGERPRINT.to_owned())];
            return Tested { host_key: HostKeyState::Unknown { fingerprints }, signed_in: false, tmux: None, message: None };
        }
        if new.password.as_ref().is_some_and(|p| p.expose() == "wrong") {
            return Tested { host_key: HostKeyState::Known, signed_in: false, tmux: None, message: Some("Permission denied (publickey,password).".into()) };
        }
        Tested { host_key: HostKeyState::Known, signed_in: true, tmux: Some(!host.contains("bare")), message: None }
    }

    /// The guess, made up from the server's name.
    fn guess(&self, server: &str) -> Option<String> {
        let name = self.listed(server)?.name.to_lowercase();
        Some(if name.contains("web") || name.contains("shop") {
            "globe"
        } else if name.contains("db") || name.contains("data") {
            "database"
        } else if name.contains("docker") || name.contains("box") {
            "box"
        } else {
            "server"
        }
        .to_owned())
    }

    /// Does what is asked, at once.
    pub fn handle(&mut self, ask: Ask) -> Answer {
        match ask {
            Ask::Sources => Answer::Sources(self.offered()),
            Ask::LocalAuth => Answer::LocalAuth(self.local.clone()),
            Ask::Add(new) => {
                let id = self.add(new.name.clone(), &new.address);
                let name = self.hosts.last().map(|h| h.name.clone()).unwrap_or_default();
                if new.remember_password {
                    self.passwords.push(id.clone());
                }
                if let Some(look) = new.look {
                    self.looks.set_look(&id, look.kept());
                }
                if new.env.is_some() {
                    self.looks.set_env(&id, new.env);
                }
                Answer::Added { server: format!("host:{id}"), name, warning: None }
            }
            Ask::Test(new) => Answer::Tested(self.test(&new)),
            Ask::TrustTest(new) => {
                self.trusted.push(parse_address(&new.address).host);
                Answer::Tested(self.test(&new))
            }
            Ask::Import { source: Source::SshConfig, picks, offered } => {
                self.looks.choose_config(&offered, &picks);
                Answer::Imported { source: Source::SshConfig, note: imported_note(Source::SshConfig, picks.len(), &[], 0), old_passwords: 0 }
            }
            Ask::Import { source, picks, .. } => {
                let (pool, here) = match source {
                    Source::Xpipe => (&mut self.sources.xpipe, Some(&mut self.sources.xpipe_here)),
                    Source::App(app) => match self.sources.apps.iter_mut().find(|o| o.app == app) {
                        Some(o) => (&mut o.servers, Some(&mut o.here)),
                        None => return Answer::Failed(format!("{} has no servers to add.", app.name())),
                    },
                    _ => (&mut self.sources.old, None),
                };
                let (chosen, left): (Vec<Candidate>, Vec<Candidate>) = std::mem::take(pool).into_iter().partition(|c| picks.contains(&c.id));
                *pool = left;
                if let Some(here) = here {
                    *here += chosen.len();
                }
                let with_password = chosen.iter().filter(|c| c.had_password).count();
                for c in &chosen {
                    self.add(Some(c.name.clone()), &c.address);
                }
                // 0.10's passwords can come along: SSHeesh asks first.
                let old_passwords = if source == Source::Old { with_password } else { 0 };
                self.sources.old_passwords += old_passwords;
                let note = imported_note(source, chosen.len(), &[], with_password - old_passwords);
                Answer::Imported { source, note, old_passwords }
            }
            Ask::UseOldPasswords => {
                let n = std::mem::take(&mut self.sources.old_passwords);
                Answer::Note(old_passwords_note(n, &[]))
            }
            Ask::Reorder { order } => {
                self.looks.reorder(&order);
                Answer::Changed
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
            Ask::SetColor { ref server, ref color } => match (host_id(server), crate::labels::parse_color(color)) {
                (Some(id), Some(color)) => {
                    self.looks.set_color(id, &color);
                    Answer::Changed
                }
                _ => Answer::failed(&ask, "That isn't a color. Type it as #2389e2."),
            },
            Ask::SetImage { ref server, ref path } => match host_id(server) {
                // Nothing is read here: the picture is made of the file's name.
                Some(id) => {
                    let id = id.to_owned();
                    self.looks.set_image(&id, Some(crate::labels::image_data_url(path.to_string_lossy().as_bytes())));
                    Answer::Changed
                }
                None => Answer::failed(&ask, "This computer has no picture of its own."),
            },
            Ask::ClearImage { server } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_image(id, None);
                }
                Answer::Changed
            }
            Ask::Automatic { server } => {
                if let Some(id) = host_id(&server) {
                    self.looks.automatic(id);
                }
                Answer::Changed
            }
            Ask::Detect { server, .. } => {
                let icon = self.guess(&server);
                if let (Some(id), Some(icon)) = (host_id(&server), &icon) {
                    self.looks.set_guess(id, icon);
                }
                Answer::Detected { server, icon }
            }
            Ask::SetEnv { server, env } => {
                if let Some(id) = host_id(&server) {
                    self.looks.set_env(id, env);
                }
                Answer::Changed
            }
            Ask::ForgetPassword { server } => {
                let Some(host) = self.listed(&server).cloned() else { return Answer::failed(&Ask::ForgetPassword { server }, "That server isn't in your list anymore.") };
                self.passwords.retain(|p| *p != host.id);
                Answer::Note(forgot_note(self.looks.name(&host)))
            }
            Ask::SavePassword { ref server, ref password, remember } => {
                let Some(host) = self.listed(server).cloned() else { return Answer::failed(&ask, "That server isn't in your list anymore.") };
                if password.is_empty() {
                    return Answer::failed(&ask, "Type the password first.");
                }
                // A demo keeps no password, only that there is one.
                if remember && !self.passwords.contains(&host.id) {
                    self.passwords.push(host.id.clone());
                }
                Answer::Note(saved_note(self.looks.name(&host), remember))
            }
            Ask::Remove { server } => {
                let Some(host) = self.listed(&server).cloned() else { return Answer::failed(&Ask::Remove { server }, "That server isn't in your list anymore.") };
                if host.from_config {
                    self.looks.hide(&host.id);
                } else {
                    self.hosts.retain(|h| h.id != host.id);
                    self.passwords.retain(|p| *p != host.id);
                    self.looks.forget(&host.id);
                }
                Answer::Removed { server }
            }
        }
    }
}

/// What "Forget saved password" did.
pub fn forgot_note(name: &str) -> String {
    format!("Forgot the saved password for {name}. It asks the next time.")
}

/// Whether ssh asks for a server's password on this line of a terminal: "user@host's
/// password:", "(user@host) Password:", a bare "Password:". The rule askpass reads a prompt by
/// (sshee-core's `secrets::is_password_prompt`), so never a key's passphrase ("Enter
/// passphrase for key …:"), a host key's question or a one-time code.
pub fn asks_for_password(line: &str) -> bool {
    let p = line.trim().to_lowercase();
    !p.contains("passphrase") && (p.ends_with("password:") || p.contains("'s password"))
}

/// What "Save password" did: kept in the keychain (`remember`), or until SSHeesh quits.
pub fn saved_note(name: &str, remember: bool) -> String {
    if remember {
        format!("Saved the password for {name}. It signs in without asking.")
    } else {
        format!("SSHeesh uses the password for {name} until it quits.")
    }
}

/// The window's faces of the servers in `looks`: each with its picture made by `picture` (only
/// when its look has one) and whether `saved` says a password is kept for it.
pub fn faces(looks: &IndexMap<ServerKey, Look>, saved: impl Fn(&str) -> bool, picture: impl Fn(&Look) -> Option<Picture>) -> IndexMap<ServerKey, Face> {
    looks
        .iter()
        .map(|(key, look)| {
            let picture = look.image.as_ref().and_then(|_| picture(look));
            (key.clone(), Face { look: look.clone(), picture, saved_password: saved(key) })
        })
        .collect()
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
        assert_eq!(offers, [(Source::Old, "2 servers".to_owned()), (Source::SshConfig, "1 new server".to_owned()), (Source::Xpipe, "1 new server".to_owned())]);
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
    fn other_apps_are_offered_and_brought_in_like_xpipe() {
        let c = |name: &str, had_password: bool| Candidate { id: format!("root@{name}"), name: name.into(), address: format!("root@{name}"), had_password, picked: true, ..Candidate::default() };
        let putty = AppOffer { app: App::Putty, servers: vec![c("web-7731", false), c("db-7731", false)], here: 1, through_jump: 1, ppk_keys: 2 };
        let winscp = AppOffer { app: App::WinScp, servers: vec![c("files-7731", true)], here: 0, through_jump: 0, ppk_keys: 0 };
        // An app with nothing new gets no row.
        let empty = AppOffer { app: App::Remmina, servers: vec![], here: 3, through_jump: 0, ppk_keys: 0 };
        let sources = Sources { apps: vec![putty, winscp, empty], ..Sources::default() };
        assert_eq!(
            sources.offers(),
            [(Source::App(App::Putty), "2 new servers".to_owned()), (Source::App(App::WinScp), "1 new server".to_owned())]
        );
        assert_eq!(Source::App(App::Putty).label(), "Import from PuTTY");
        assert_eq!(Source::App(App::WinScp).promise(), "Pick the ones to add. WinSCP isn't changed.");
        assert_eq!(sources.title(Source::App(App::Putty)), "Found 2 new servers in PuTTY");
        assert_eq!(
            sources.notes(Source::App(App::Putty)),
            [
                "1 server is in SSHeesh already.",
                "1 server goes through another server or a proxy, which SSHeesh can't do yet, so it stays out.",
                "2 servers sign in with a PuTTY key (.ppk), which SSHeesh can't use, so they come without it. PuTTYgen makes an OpenSSH copy: Conversions, then Export OpenSSH key.",
            ]
        );
        assert_eq!(sources.notes(Source::App(App::WinScp)), ["Saved passwords stay in WinSCP: a server that needs one asks the first time."]);
        assert!(sources.of(Source::App(App::FileZilla)).is_empty());

        let mut book = Book::new(vec![], Looks::default(), sources);
        let answer = book.handle(Ask::Import { source: Source::App(App::WinScp), picks: vec!["root@files-7731".into()], offered: vec!["root@files-7731".into()] });
        assert_eq!(
            answer,
            Answer::Imported { source: Source::App(App::WinScp), note: "Added 1 server from WinSCP. 1 server will ask for a password the first time.".into(), old_passwords: 0 }
        );
        assert_eq!(book.servers().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["files-7731"]);
        let Answer::Sources(after) = book.handle(Ask::Sources) else { panic!() };
        assert_eq!(after.offers(), [(Source::App(App::Putty), "2 new servers".to_owned())]);
        assert_eq!(after.app(App::WinScp).map(|o| o.here), Some(1));
        assert!(matches!(book.handle(Ask::Import { source: Source::App(App::Kitty), picks: vec![], offered: vec![] }), Answer::Failed(_)));
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
        assert_eq!(from_xpipe, Answer::Imported { source: Source::Xpipe, note: "Added 1 server from XPipe. 1 server will ask for a password the first time.".into(), old_passwords: 0 });
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
        assert!(matches!(book.handle(Ask::Remove { server: "host:gone".into() }), Answer::Refused { .. }));
        assert_eq!(host_id("local"), None);
    }

    #[test]
    fn a_menus_change_that_fails_says_which_server_and_what() {
        let ask = Ask::Rename { server: "host:saved:1".into(), name: "Shop".into() };
        assert_eq!(Answer::failed(&ask, "disk full"), Answer::Refused { server: "host:saved:1".into(), doing: "Couldn't rename the server".into(), why: "disk full".into() });
        // The dialog's own asks fail to the dialog.
        assert_eq!(Answer::failed(&Ask::Sources, "nope"), Answer::Failed("nope".into()));
        let mut book = Book::default();
        let refused = book.handle(Ask::SetColor { server: "host:saved:1".into(), color: "blue".into() });
        assert!(matches!(refused, Answer::Refused { ref doing, .. } if doing == "Couldn't change the server's look"));
    }

    #[test]
    fn a_form_signs_in_with_a_password_manager_once_one_is_chosen() {
        let mut form = Form::new();
        form.address = "deploy@web-7731".into();
        form.sign_in = SignIn::Manager;
        assert_eq!(form.check(), Err(NO_MANAGER.into()));
        form.agent = "/home/someone/.1password/agent.sock".into();
        form.key_file = "~/.ssh/id_ed25519".into();
        let new = form.check().unwrap();
        assert_eq!((new.identity_agent.as_deref(), new.identity_file, new.password), (Some("/home/someone/.1password/agent.sock"), None, None));
        // A look and a label picked in the form go with it.
        form.look = Some(PickedLook { icon: "globe".into(), color: "#7577e3".into(), image: None });
        form.env = Some(EnvLabel::Staging);
        let new = form.check().unwrap();
        assert_eq!((new.look.unwrap().icon.as_str(), new.env), ("globe", Some(EnvLabel::Staging)));
        // What connecting depends on: a name doesn't make a check stale, a password does.
        let before = form.connection();
        form.name = "Shop".into();
        assert_eq!(form.connection(), before);
        form.password = Password::new("x");
        assert_ne!(form.connection(), before);
    }

    #[test]
    fn the_keys_and_managers_found_here_are_told_in_plain_words() {
        let mut local = LocalAuth {
            keys: vec![LocalKey { path: "~/.ssh/id_ed25519".into(), name: "id_ed25519".into(), kind: "ED25519".into(), comment: String::new() }],
            agents: vec![KeyAgent { name: "ssh-agent".into(), socket: None, keys: Some(2) }, KeyAgent { name: "1Password".into(), socket: Some("/tmp/op.sock".into()), keys: None }],
        };
        assert_eq!(local.usual_keys_note(), "ssh picks one, including 2 keys loaded in ssh-agent");
        assert_eq!(LocalAuth::key_note(&local.keys[0]), "~/.ssh/id_ed25519");
        assert_eq!(local.managers().len(), 1);
        assert_eq!(LocalAuth::manager_note(local.managers()[0]), "Unlock it to use its keys");
        assert_eq!(LocalAuth::manager_note(&KeyAgent { keys: Some(1), ..KeyAgent::default() }), "1 key · you approve each sign-in there");
        local.agents.clear();
        assert_eq!(local.usual_keys_note(), "ssh picks from the keys in ~/.ssh");
    }

    #[test]
    fn a_check_walks_through_every_step() {
        let unknown = Tested { host_key: HostKeyState::Unknown { fingerprints: vec![("ED25519".into(), MADE_UP_FINGERPRINT.into())] }, signed_in: false, tmux: None, message: None };
        let titles = |t: &Tested| t.steps().iter().map(|s| (s.tone, s.title.clone())).collect::<Vec<_>>();
        assert_eq!(titles(&unknown), [(StepTone::Ok, "The server answered".to_owned()), (StepTone::Ask, "First time here. Is this really your server?".to_owned())]);
        assert!(unknown.can_trust() && unknown.fingerprints().len() == 1);
        let down = Tested { host_key: HostKeyState::Unreachable { message: None }, signed_in: false, tmux: None, message: None };
        assert_eq!(down.steps()[0].detail.as_deref(), Some("Check the address and port, and that the server is on."));
        assert_eq!(down.steps().len(), 1);
        let bare = Tested { host_key: HostKeyState::Known, signed_in: true, tmux: Some(false), message: None };
        assert_eq!(bare.steps().iter().map(|s| s.tone).collect::<Vec<_>>(), [StepTone::Ok, StepTone::Ok, StepTone::Warn]);
        assert!(bare.steps()[2].detail.as_deref().unwrap().ends_with("Install it on the server with:"));
        assert_eq!(bare.steps()[2].command.as_deref(), Some(INSTALL_TMUX));
        let refused = Tested { host_key: HostKeyState::Known, signed_in: false, tmux: None, message: Some("Permission denied (publickey).".into()) };
        assert_eq!(refused.steps()[1], Step { tone: StepTone::Bad, title: "Couldn't sign in".into(), detail: Some("Permission denied (publickey).".into()), command: None });
        let changed = Tested { host_key: HostKeyState::Changed, signed_in: false, tmux: None, message: None };
        assert_eq!(changed.steps()[1].tone, StepTone::Bad);
        assert!(!changed.can_trust());
        // The list in memory: a fingerprint, trust, signed in.
        let mut book = Book::default();
        let new = NewServer { address: "deploy@web-7731".into(), ..NewServer::default() };
        assert!(matches!(book.handle(Ask::Test(new.clone())), Answer::Tested(t) if t.can_trust()));
        assert!(matches!(book.handle(Ask::TrustTest(new.clone())), Answer::Tested(t) if t.signed_in && t.tmux == Some(true)));
        assert!(matches!(book.handle(Ask::Test(new)), Answer::Tested(t) if t.signed_in));
    }

    #[test]
    fn a_server_is_added_with_its_look_and_its_password_can_be_forgotten() {
        let mut book = Book::default();
        let look = PickedLook { icon: "database".into(), color: "#9d69d2".into(), image: None };
        let new = NewServer { address: "postgres@db-7731".into(), password: Some(Password::new("pw")), remember_password: true, look: Some(look), env: Some(EnvLabel::Prod), ..NewServer::default() };
        let Answer::Added { server, .. } = book.handle(Ask::Add(new)) else { panic!() };
        let faces = book.faces(|_| None);
        assert_eq!((faces[&server].look.icon.as_str(), faces[&server].look.color.as_str(), faces[&server].saved_password), ("database", "#9d69d2", true));
        assert!(book.servers()[0].is_prod());
        assert_eq!(book.handle(Ask::ForgetPassword { server: server.clone() }), Answer::Note(forgot_note("db-7731")));
        assert!(!book.faces(|_| None)[&server].saved_password);
        // Saved again from its menu, or from the terminal that asked for it.
        let save = Ask::SavePassword { server: server.clone(), password: Password::new("made-up-password"), remember: true };
        assert!(!format!("{save:?}").contains("made-up"));
        assert_eq!(save.server(), Some(server.as_str()));
        assert_eq!(book.handle(save), Answer::Note(saved_note("db-7731", true)));
        assert!(book.faces(|_| None)[&server].saved_password);
        assert!(matches!(book.handle(Ask::SavePassword { server: server.clone(), password: Password::default(), remember: true }), Answer::Refused { .. }));
        assert_eq!(saved_note("db-7731", false), "SSHeesh uses the password for db-7731 until it quits.");
        // A picture, then back to the icon; "Automatic", then a guess.
        book.handle(Ask::SetImage { server: server.clone(), path: "/pictures/db.png".into() });
        let picture = Picture { width: 1, height: 1, rgba: std::sync::Arc::from(&[1u8, 2, 3, 4][..]) };
        assert_eq!(book.faces(|_| Some(picture.clone()))[&server].picture.as_ref(), Some(&picture));
        book.handle(Ask::ClearImage { server: server.clone() });
        assert_eq!(book.faces(|_| Some(picture.clone()))[&server].picture, None);
        book.handle(Ask::Automatic { server: server.clone() });
        assert!(!book.faces(|_| None)[&server].look.picked);
        assert_eq!(book.handle(Ask::Detect { server: server.clone(), first: true }), Answer::Detected { server: server.clone(), icon: Some("database".into()) });
        assert_eq!(book.faces(|_| None)[&server].look.icon, "database");
    }

    #[test]
    fn a_line_asks_for_a_password_only_as_ssh_words_it() {
        for line in ["deploy@203.0.113.17's password: ", "(deploy@web-7731) Password:", "Password:", "  root@db's password:   "] {
            assert!(asks_for_password(line), "{line}");
        }
        for line in [
            "Enter passphrase for key '/home/demo/.ssh/id_ed25519': ",
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
            "Verification code: ",
            "Permission denied, please try again.",
            "deploy@web-7731:~$ ",
            "",
        ] {
            assert!(!asks_for_password(line), "{line}");
        }
    }

    #[test]
    fn passwords_0_10_saved_are_offered_after_its_servers_come_and_used_only_on_a_yes() {
        let c = |name: &str, had_password: bool| Candidate { id: name.into(), name: name.into(), address: format!("root@{name}"), had_password, picked: true, ..Candidate::default() };
        let mut book = Book::new(vec![], Looks::default(), Sources { old: vec![c("db-main", true), c("web-7731", true), c("mail", false)], ..Sources::default() });
        let answer = book.handle(Ask::Import { source: Source::Old, picks: vec!["db-main".into(), "web-7731".into(), "mail".into()], offered: vec![] });
        assert_eq!(answer, Answer::Imported { source: Source::Old, note: "Added 3 servers from SSHeesh 0.10.".into(), old_passwords: 2 });
        assert_eq!(old_passwords_question(2), "SSHeesh 0.10 saved passwords for 2 servers. Use them here?");
        assert_eq!(book.offered().old_passwords, 2);
        assert_eq!(book.handle(Ask::UseOldPasswords), Answer::Note("Brought over the saved passwords of 2 servers.".into()));
        assert_eq!(book.offered().old_passwords, 0);
        assert_eq!(old_passwords_note(1, &["mail".into()]), "Brought over the saved passwords of 1 server. Couldn't read mail's: it will ask the first time.");
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
