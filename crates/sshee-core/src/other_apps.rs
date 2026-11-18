//! Servers saved in other SSH apps, read to import them: PuTTY, KiTTY, WinSCP, MobaXterm,
//! FileZilla, SecureCRT and Remmina. Only ever read: nothing of theirs is written, locked or
//! moved.
//!
//! Where each keeps its sessions:
//!
//! - PuTTY: the registry, `HKCU\Software\SimonTatham\PuTTY\Sessions`; on Linux and macOS a
//!   file per session in `~/.putty/sessions` (or `~/.config/putty/sessions`).
//! - KiTTY: the registry, `HKCU\Software\9bis.com\KiTTY\Sessions`, or its folder of session
//!   files, `%APPDATA%\KiTTY\Sessions` (`Key\Value\` lines).
//! - WinSCP: `%APPDATA%\WinSCP.ini`, or the registry, `HKCU\Software\Martin Prikryl\WinSCP 2\Sessions`.
//!   SFTP and SCP sessions only.
//! - MobaXterm: `MobaXterm.ini` in `%APPDATA%\MobaXterm` or `Documents\MobaXterm`, its
//!   `[Bookmarks…]` sections. SSH and SFTP bookmarks only.
//! - FileZilla: `sitemanager.xml` in `%APPDATA%\FileZilla` or `~/.config/filezilla`. SFTP
//!   sites only.
//! - SecureCRT: a `.ini` file per session under `VanDyke\Config\Sessions` (Windows),
//!   `~/Library/Application Support/VanDyke/SecureCRT/Config/Sessions` (macOS) or
//!   `~/.vandyke/SecureCRT/Config/Sessions` (Linux). SSH sessions only.
//! - Remmina: a `.remmina` file per connection in `~/.local/share/remmina` (or `~/.remmina`,
//!   or the Flatpak's folder). SSH and SFTP only.
//!
//! Passwords never come along, saved or not: a server whose app keeps one says so
//! (`had_password`), and SSHeesh asks for it the first time. A server reached through another
//! server or a proxy stays out, counted, as XPipe's do. A broken file or entry is skipped
//! alone; nothing here panics on what it reads.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Another app servers can be brought in from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
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
    pub const ALL: [App; 7] = [App::Putty, App::Kitty, App::WinScp, App::MobaXterm, App::FileZilla, App::SecureCrt, App::Remmina];

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
}

/// A server another app keeps, as the Add server form takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub name: String,
    /// `user@host:port`.
    pub address: String,
    pub identity_file: Option<String>,
    /// The app keeps a password for it, which stays there.
    pub had_password: bool,
}

/// What one app holds.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScan {
    /// Its SSH servers, once each, in name order.
    pub servers: Vec<Found>,
    /// Reached through another server or a proxy, which SSHeesh can't set yet.
    pub through_jump: usize,
    /// Servers whose key is a PuTTY key (`.ppk`) with no OpenSSH copy beside it: they come
    /// without it, since OpenSSH can't read one.
    pub ppk_keys: usize,
}

/// The folders the apps keep their files in, and whether to look in the registry. The
/// program's are [`Places::here`]; a test's point into a folder of its own.
#[derive(Debug, Clone, Default)]
pub struct Places {
    pub home: Option<PathBuf>,
    /// `%APPDATA%` (Roaming) on Windows, `~/.config` on Linux, `~/Library/Application Support` on macOS.
    pub config: Option<PathBuf>,
    /// `~/.local/share` on Linux.
    pub data: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    /// Read the current user's registry (Windows only).
    pub registry: bool,
}

impl Places {
    /// This computer's.
    pub fn here() -> Places {
        Places { home: dirs::home_dir(), config: dirs::config_dir(), data: dirs::data_dir(), documents: dirs::document_dir(), registry: cfg!(windows) }
    }

    /// Every place under `home`, the registry left out: for tests.
    pub fn under(home: &Path) -> Places {
        Places { home: Some(home.into()), config: Some(home.join(".config")), data: Some(home.join(".local/share")), documents: Some(home.join("Documents")), registry: false }
    }

    fn join(&self, pick: Pick, rel: &str) -> Option<PathBuf> {
        pick(self).as_ref().map(|d| rel.split('/').fold(d.clone(), |p, part| p.join(part)))
    }

    /// The candidate places, once each.
    fn paths(&self, wants: &[(Pick, &str)]) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for (pick, rel) in wants {
            if let Some(p) = self.join(*pick, rel) {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }
}

/// One of [`Places`]' folders.
type Pick = fn(&Places) -> &Option<PathBuf>;

fn home(p: &Places) -> &Option<PathBuf> {
    &p.home
}
fn config(p: &Places) -> &Option<PathBuf> {
    &p.config
}
fn data(p: &Places) -> &Option<PathBuf> {
    &p.data
}
fn documents(p: &Places) -> &Option<PathBuf> {
    &p.documents
}

/// Files bigger than this aren't another app's settings: left unread.
const MAX_FILE: u64 = 16 << 20;
/// Session files read per folder, at most.
const MAX_FILES: usize = 5000;

const PUTTY_KEY: &str = r"Software\SimonTatham\PuTTY\Sessions";
const KITTY_KEY: &str = r"Software\9bis.com\KiTTY\Sessions";
const WINSCP_KEY: &str = r"Software\Martin Prikryl\WinSCP 2\Sessions";

/// Every app that holds something, in [`App::ALL`]'s order.
pub fn scan_all(places: &Places) -> Vec<(App, AppScan)> {
    App::ALL.iter().map(|&app| (app, scan(app, places))).filter(|(_, s)| !s.servers.is_empty() || s.through_jump > 0).collect()
}

/// What one app holds.
pub fn scan(app: App, places: &Places) -> AppScan {
    let mut found = Scan::new(places);
    match app {
        App::Putty | App::Kitty => {
            let (key, dirs) = if app == App::Putty {
                (PUTTY_KEY, places.paths(&[(home, ".putty/sessions"), (config, "putty/sessions"), (home, ".config/putty/sessions")]))
            } else {
                (KITTY_KEY, places.paths(&[(config, "KiTTY/Sessions")]))
            };
            if places.registry {
                for (name, values) in registry_sessions(key) {
                    found.add(putty(&name, &values));
                }
            }
            for dir in dirs {
                for (file, text) in files_in(&dir, |_| true) {
                    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    found.add(putty(&name, &putty_file(&text)));
                }
            }
        }
        App::WinScp => {
            if places.registry {
                for (name, values) in registry_sessions(WINSCP_KEY) {
                    found.add(winscp(&name, &values));
                }
            }
            for ini in places.paths(&[(config, "WinSCP.ini")]) {
                if let Some(text) = read_text(&ini) {
                    for (section, values) in ini_sections(&text) {
                        if let Some(name) = section.strip_prefix(r"Sessions\") {
                            found.add(winscp(name, &values));
                        }
                    }
                }
            }
        }
        App::MobaXterm => {
            for ini in places.paths(&[(config, "MobaXterm/MobaXterm.ini"), (documents, "MobaXterm/MobaXterm.ini"), (home, "Documents/MobaXterm/MobaXterm.ini")]) {
                if let Some(text) = read_text(&ini) {
                    for entry in mobaxterm(&text, places.home.as_deref()) {
                        found.add(entry);
                    }
                }
            }
        }
        App::FileZilla => {
            for xml in places.paths(&[
                (config, "FileZilla/sitemanager.xml"),
                (config, "filezilla/sitemanager.xml"),
                (home, ".config/filezilla/sitemanager.xml"),
                (home, ".filezilla/sitemanager.xml"),
            ]) {
                if let Some(text) = read_text(&xml) {
                    for entry in filezilla(&text) {
                        found.add(entry);
                    }
                }
            }
        }
        App::SecureCrt => {
            for dir in places.paths(&[(config, "VanDyke/Config/Sessions"), (config, "VanDyke/SecureCRT/Config/Sessions"), (home, ".vandyke/SecureCRT/Config/Sessions")]) {
                let is_session = |p: &Path| {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    name.ends_with(".ini") && name != "__folderdata__.ini" && name != "default.ini"
                };
                for (file, text) in files_deep(&dir, is_session) {
                    let name = file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    found.add(securecrt(&name, &text));
                }
            }
        }
        App::Remmina => {
            for dir in places.paths(&[(data, "remmina"), (home, ".local/share/remmina"), (home, ".remmina"), (home, ".var/app/org.remmina.Remmina/data/remmina")]) {
                let is_remmina = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("remmina"));
                for (file, text) in files_in(&dir, is_remmina) {
                    let stem = file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    found.add(remmina(&stem, &text));
                }
            }
        }
    }
    found.done()
}

/// One session, read.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Server(Session),
    ThroughJump,
    /// Not an SSH server (Telnet, RDP, FTP…), a default, or unreadable.
    Other,
}

/// A session's parts, before they're one address.
#[derive(Debug, Default, PartialEq, Eq)]
struct Session {
    name: String,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    key: Option<String>,
    had_password: bool,
}

/// What a scan has found so far.
struct Scan<'a> {
    places: &'a Places,
    out: AppScan,
}

impl<'a> Scan<'a> {
    fn new(places: &'a Places) -> Scan<'a> {
        Scan { places, out: AppScan::default() }
    }

    fn add(&mut self, entry: Entry) {
        match entry {
            Entry::ThroughJump => self.out.through_jump += 1,
            Entry::Other => {}
            Entry::Server(s) => {
                if self.out.servers.len() >= MAX_FILES {
                    return;
                }
                let Some(address) = address(&s) else { return };
                if self.out.servers.iter().any(|f| f.address == address) {
                    return;
                }
                let key = s.key.as_deref().map(str::trim).filter(|k| !k.is_empty()).and_then(|k| self.usable_key(k));
                let name = clean_name(&s.name).unwrap_or_else(|| s.host.clone());
                self.out.servers.push(Found { name, address, identity_file: key, had_password: s.had_password });
            }
        }
    }

    /// A key OpenSSH can use: a PuTTY key (`.ppk`) only when its OpenSSH copy is beside it
    /// (the same name without `.ppk`). Else none, counted.
    fn usable_key(&mut self, key: &str) -> Option<String> {
        let key = match (key.strip_prefix("~/").or_else(|| key.strip_prefix("~\\")), &self.places.home) {
            (Some(rest), Some(home)) => home.join(rest).to_string_lossy().into_owned(),
            _ => key.to_owned(),
        };
        let lower = key.to_ascii_lowercase();
        if !lower.ends_with(".ppk") {
            return Some(key);
        }
        let plain = &key[..key.len() - 4];
        if Path::new(plain).is_file() {
            return Some(plain.to_owned());
        }
        self.out.ppk_keys += 1;
        None
    }

    fn done(mut self) -> AppScan {
        self.out.servers.sort_by_key(|s| s.name.to_lowercase());
        self.out
    }
}

/// `user@host:port`, as the Add server form reads it; `None` when it wouldn't read.
fn address(s: &Session) -> Option<String> {
    let host = s.host.trim();
    let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c.is_control() || c == '@' || c == '/') {
        return None;
    }
    let user = s.user.as_deref().map(str::trim).filter(|u| !u.is_empty());
    if user.is_some_and(|u| u.contains(|c: char| c.is_whitespace() || c.is_control() || c == '@')) {
        return None;
    }
    // A bare IPv6 address needs brackets before a port.
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_owned() };
    let address = format!(
        "{}{host}{}",
        user.map(|u| format!("{u}@")).unwrap_or_default(),
        s.port.filter(|&p| p != 22 && p != 0).map(|p| format!(":{p}")).unwrap_or_default()
    );
    crate::hosts::parse_address(&address).ok().map(|_| address)
}

/// A name to show: one line, no control characters, not endless.
fn clean_name(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).take(120).collect();
    Some(name.trim().to_owned()).filter(|n| !n.is_empty())
}

/// `host:port`, `[v6]:port` or a bare host, as several apps write a server.
fn split_host_port(s: &str) -> (String, Option<u16>) {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('[') {
        if let Some((h, tail)) = rest.split_once(']') {
            return (h.to_owned(), tail.strip_prefix(':').and_then(|p| p.trim().parse().ok()));
        }
    }
    match s.matches(':').count() {
        1 => {
            let (h, p) = s.split_once(':').unwrap_or((s, ""));
            (h.to_owned(), p.trim().parse().ok())
        }
        _ => (s.to_owned(), None),
    }
}

/// `user@host` in a host field, as PuTTY lets you type it.
fn split_user(host: &str, user: Option<String>) -> (String, Option<String>) {
    match host.rsplit_once('@') {
        Some((u, h)) if user.is_none() && !u.is_empty() => (h.to_owned(), Some(u.to_owned())),
        Some((_, h)) => (h.to_owned(), user),
        None => (host.to_owned(), user),
    }
}

fn get<'m>(values: &'m BTreeMap<String, String>, key: &str) -> Option<&'m str> {
    values.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// `%XX` escapes undone, as PuTTY writes session names and WinSCP its names and values. A `%`
/// that doesn't start one stays.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- PuTTY and KiTTY -------------------------------------------------------------------------

fn putty(name: &str, values: &BTreeMap<String, String>) -> Entry {
    let name = unescape(name);
    if name == "Default Settings" {
        return Entry::Other;
    }
    if get(values, "Protocol").is_some_and(|p| !p.eq_ignore_ascii_case("ssh")) {
        return Entry::Other;
    }
    let Some(host) = get(values, "HostName") else { return Entry::Other };
    // Any proxy (an SSH jump host among them, since PuTTY 0.77).
    if get(values, "ProxyMethod").is_some_and(|m| m != "0") {
        return Entry::ThroughJump;
    }
    let (host, user) = split_user(host, get(values, "UserName").map(str::to_owned));
    let port = get(values, "PortNumber").and_then(|p| p.parse().ok());
    // PuTTY keeps no password; KiTTY can.
    let had_password = get(values, "Password").is_some();
    Entry::Server(Session { name, host, user, port, key: get(values, "PublicKeyFile").map(str::to_owned), had_password })
}

/// A session file: `Key=Value` lines (PuTTY on Linux and macOS) or `Key\Value\` (KiTTY).
fn putty_file(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some(at) = line.find(['=', '\\']) else { continue };
        let (key, value) = (&line[..at], &line[at + 1..]);
        let value = if line.as_bytes()[at] == b'\\' { value.strip_suffix('\\').unwrap_or(value) } else { value };
        if !key.is_empty() {
            out.insert(key.trim().to_owned(), value.to_owned());
        }
    }
    out
}

// ---- WinSCP ----------------------------------------------------------------------------------

fn winscp(name: &str, values: &BTreeMap<String, String>) -> Entry {
    let name = unescape(name);
    if name == "Default Settings" {
        return Entry::Other;
    }
    let values: BTreeMap<String, String> = values.iter().map(|(k, v)| (k.clone(), unescape(v))).collect();
    // 0 SCP, 1 SFTP (SCP when it can't), 2 SFTP; 5 FTP, 6 WebDAV, 7 S3. SFTP when not written.
    if get(&values, "FSProtocol").is_some_and(|p| !matches!(p, "0" | "1" | "2")) {
        return Entry::Other;
    }
    let Some(host) = get(&values, "HostName") else { return Entry::Other };
    if get(&values, "Tunnel").is_some_and(|t| t != "0") || get(&values, "ProxyMethod").is_some_and(|m| m != "0") {
        return Entry::ThroughJump;
    }
    // Folders are written into the name: "Work/web-7731".
    let name = name.rsplit('/').next().unwrap_or(&name).to_owned();
    let (host, user) = split_user(host, get(&values, "UserName").map(str::to_owned));
    let port = get(&values, "PortNumber").and_then(|p| p.parse().ok());
    let had_password = get(&values, "Password").is_some() || get(&values, "PasswordPlain").is_some();
    Entry::Server(Session { name, host, user, port, key: get(&values, "PublicKeyFile").map(str::to_owned), had_password })
}

// ---- MobaXterm -------------------------------------------------------------------------------

/// MobaXterm's escapes for the characters its bookmark lines use themselves.
fn moba_unescape(s: &str) -> String {
    s.replace("__PTVIRG__", ";").replace("__DBLQUO__", "\"").replace("__PIPE__", "|").replace("__DIEZE__", "#").replace("__PERCENT__", "%")
}

fn mobaxterm(text: &str, home: Option<&Path>) -> Vec<Entry> {
    let sections = ini_sections(text);
    // Saved passwords: `[Passwords]`, keyed `user@host` (maybe after `ssh22:`).
    let saved: Vec<String> = sections
        .iter()
        .filter(|(s, _)| s.eq_ignore_ascii_case("Passwords"))
        .flat_map(|(_, v)| v.keys())
        .map(|k| k.rsplit_once(':').map_or(k.as_str(), |(_, r)| r).to_lowercase())
        .collect();
    let mut out = Vec::new();
    for (section, values) in &sections {
        let lower = section.to_ascii_lowercase();
        if lower != "bookmarks" && !lower.starts_with("bookmarks_") {
            continue;
        }
        for (name, line) in values {
            if name == "SubRep" || name == "ImgNum" {
                continue;
            }
            out.push(moba_bookmark(name, line, home, &saved));
        }
    }
    out
}

/// One bookmark: `#icon#type%host%port%user%…`. Type 0 is SSH, 7 SFTP. For SSH, field 8 is the
/// gateway (jump) host and 14 the private key.
fn moba_bookmark(name: &str, line: &str, home: Option<&Path>, saved: &[String]) -> Entry {
    let mut parts = line.split('#');
    let (Some(""), Some(_icon), Some(fields)) = (parts.next(), parts.next(), parts.next()) else { return Entry::Other };
    let f: Vec<String> = fields.split('%').map(moba_unescape).collect();
    let field = |i: usize| f.get(i).map(|s| s.trim()).filter(|s| !s.is_empty());
    let kind = field(0);
    if kind != Some("0") && kind != Some("7") {
        return Entry::Other;
    }
    let Some(host) = field(1) else { return Entry::Other };
    let ssh = kind == Some("0");
    if ssh && field(8).is_some() {
        return Entry::ThroughJump;
    }
    let user = field(3).map(str::to_owned);
    let port = field(2).and_then(|p| p.parse().ok());
    let key = if ssh { field(14).map(|k| moba_path(k, home)) } else { None };
    let who = format!("{}@{}", user.as_deref().unwrap_or_default(), host).to_lowercase();
    let had_password = user.is_some() && saved.contains(&who);
    Entry::Server(Session { name: name.to_owned(), host: host.to_owned(), user, port, key, had_password })
}

/// MobaXterm's placeholders in a path, put back.
fn moba_path(path: &str, home: Option<&Path>) -> String {
    let mut path = path.replace("_CurrentDrive_", "C:");
    if let Some(home) = home {
        path = path.replace("_ProfileDir_", &home.to_string_lossy());
    }
    path
}

// ---- FileZilla -------------------------------------------------------------------------------

/// `sitemanager.xml`'s `<Server>`s. Protocol 1 is SFTP; the rest (FTP, FTPS, cloud) aren't SSH.
fn filezilla(xml: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = find_open(rest, "Server") {
        let body = &rest[at..];
        let Some(end) = body.find("</Server>") else { break };
        let server = &body[..end];
        rest = &body[end + "</Server>".len()..];
        if xml_text(server, "Protocol").as_deref() != Some("1") {
            out.push(Entry::Other);
            continue;
        }
        let Some(host) = xml_text(server, "Host") else {
            out.push(Entry::Other);
            continue;
        };
        let name = xml_text(server, "Name").unwrap_or_default();
        let had_password = xml_text(server, "Pass").is_some();
        let port = xml_text(server, "Port").and_then(|p| p.parse().ok());
        out.push(Entry::Server(Session { name, host, user: xml_text(server, "User"), port, key: xml_text(server, "Keyfile"), had_password }));
    }
    out
}

/// Where `<tag>` or `<tag …>` opens in `xml`, just past it.
fn find_open(xml: &str, tag: &str) -> Option<usize> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let at = from + i + open.len();
        match xml[at..].chars().next() {
            Some('>') => return Some(at + 1),
            Some(c) if c.is_whitespace() => return xml[at..].find('>').map(|e| at + e + 1),
            _ => from = at,
        }
    }
    None
}

/// The text of `<tag>…</tag>` in `xml`, its entities undone; `None` when empty.
fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let start = find_open(xml, tag)?;
    let end = xml[start..].find(&format!("</{tag}>"))?;
    let text = xml_unescape(xml[start..start + end].trim());
    Some(text).filter(|t| !t.is_empty())
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.find(';').filter(|&n| n <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ---- SecureCRT -------------------------------------------------------------------------------

/// A session file: `S:"Hostname"=…` lines, `D:` ones in hex.
fn securecrt(name: &str, text: &str) -> Entry {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let Some((kind, rest)) = line.split_once(':') else { continue };
        let Some(rest) = rest.strip_prefix('"') else { continue };
        let Some((key, value)) = rest.split_once("\"=") else { continue };
        let value = match kind.trim() {
            "D" => u32::from_str_radix(value.trim(), 16).map(|n| n.to_string()).unwrap_or_default(),
            "S" => value.to_owned(),
            _ => continue,
        };
        values.insert(key.to_owned(), value);
    }
    let protocol = get(&values, "Protocol Name").unwrap_or_default();
    if !protocol.eq_ignore_ascii_case("SSH2") && !protocol.eq_ignore_ascii_case("SSH1") {
        return Entry::Other;
    }
    let Some(host) = get(&values, "Hostname") else { return Entry::Other };
    if get(&values, "Firewall Name").is_some_and(|f| !f.eq_ignore_ascii_case("None")) {
        return Entry::ThroughJump;
    }
    let port = get(&values, &format!("[{}] Port", protocol.to_ascii_uppercase())).or_else(|| get(&values, "Port")).and_then(|p| p.parse().ok());
    // "…\id_ed25519.pub" (maybe with "::raw" after): the private key is beside it.
    let key = get(&values, "Identity Filename V2")
        .or_else(|| get(&values, "Identity Filename"))
        .map(|k| k.split("::").next().unwrap_or(k))
        .map(|k| k.strip_suffix(".pub").unwrap_or(k).to_owned());
    let had_password = get(&values, "Password V2").is_some() || get(&values, "Password").is_some();
    Entry::Server(Session { name: name.to_owned(), host: host.to_owned(), user: get(&values, "Username").map(str::to_owned), port, key, had_password })
}

// ---- Remmina ---------------------------------------------------------------------------------

fn remmina(stem: &str, text: &str) -> Entry {
    let sections = ini_sections(text);
    let Some((_, v)) = sections.iter().find(|(s, _)| s == "remmina") else { return Entry::Other };
    let protocol = get(v, "protocol").unwrap_or_default();
    if !protocol.eq_ignore_ascii_case("SSH") && !protocol.eq_ignore_ascii_case("SFTP") {
        return Entry::Other;
    }
    let Some(server) = get(v, "server").or_else(|| get(v, "ssh_server")) else { return Entry::Other };
    if get(v, "ssh_tunnel_enabled").is_some_and(|t| t != "0") {
        return Entry::ThroughJump;
    }
    let (host, port) = split_host_port(server);
    let (host, user) = split_user(&host, get(v, "username").or_else(|| get(v, "ssh_username")).map(str::to_owned));
    let name = get(v, "name").unwrap_or(stem).to_owned();
    let had_password = get(v, "password").is_some() || get(v, "ssh_password").is_some();
    Entry::Server(Session { name, host, user, port, key: get(v, "ssh_privatekey").map(str::to_owned), had_password })
}

// ---- Files, INI and the registry -------------------------------------------------------------

/// A settings file as text: UTF-8 (a BOM or not) or UTF-16 with a BOM, as Windows apps write
/// them. `None` when it isn't there or is far too big for one.
fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let utf16 = |le: bool| {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) }).collect();
        String::from_utf16_lossy(&units)
    };
    Some(match bytes.as_slice() {
        [0xFF, 0xFE, ..] => utf16(true),
        [0xFE, 0xFF, ..] => utf16(false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// The files right in `dir` that `wanted` picks, read.
fn files_in(dir: &Path, wanted: impl Fn(&Path) -> bool) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| wanted(p)).take(MAX_FILES).collect();
    paths.sort();
    paths.into_iter().filter_map(|p| read_text(&p).map(|t| (p, t))).collect()
}

/// The same, in `dir` and the folders in it (a few levels; links aren't followed).
fn files_deep(dir: &Path, wanted: impl Fn(&Path) -> bool) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut todo = vec![(dir.to_owned(), 0)];
    while let Some((dir, depth)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            if out.len() >= MAX_FILES {
                return out;
            }
            let Ok(kind) = e.file_type() else { continue };
            let path = e.path();
            if kind.is_dir() && depth < 8 {
                todo.push((path, depth + 1));
            } else if kind.is_file() && wanted(&path) {
                if let Some(text) = read_text(&path) {
                    out.push((path, text));
                }
            }
        }
    }
    out.sort();
    out
}

/// An INI file's sections, in order, each with its `key=value`s (the first `=` splits).
fn ini_sections(text: &str) -> Vec<(String, BTreeMap<String, String>)> {
    let mut out: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.to_owned(), BTreeMap::new()));
        } else if let (Some((key, value)), Some((_, values))) = (line.split_once('='), out.last_mut()) {
            if !trimmed.starts_with(';') && !trimmed.starts_with('#') {
                values.insert(key.trim().to_owned(), value.to_owned());
            }
        }
    }
    out
}

/// The sessions under `HKEY_CURRENT_USER\<key>`: each one's name and values (numbers as
/// decimal text). Opened to read only.
#[cfg(windows)]
fn registry_sessions(key: &str) -> Vec<(String, BTreeMap<String, String>)> {
    use windows_registry::{Type, CURRENT_USER};
    let Ok(sessions) = CURRENT_USER.open(key) else { return Vec::new() };
    let Ok(names) = sessions.keys() else { return Vec::new() };
    let mut out = Vec::new();
    for name in names.take(MAX_FILES) {
        let Ok(session) = sessions.open(&name) else { continue };
        let Ok(values) = session.values() else { continue };
        let mut map = BTreeMap::new();
        for (value_name, value) in values {
            let text = match value.ty() {
                Type::U32 => u32::try_from(value).map(|n| n.to_string()).ok(),
                Type::String | Type::ExpandString => String::try_from(value).ok(),
                _ => None,
            };
            if let Some(text) = text {
                map.insert(value_name, text);
            }
        }
        out.push((name, map));
    }
    out
}

#[cfg(not(windows))]
fn registry_sessions(_key: &str) -> Vec<(String, BTreeMap<String, String>)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sshee-apps-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(home: &Path, rel: &str, text: &[u8]) {
        let path = home.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn found(name: &str, address: &str, key: Option<&str>, had_password: bool) -> Found {
        Found { name: name.into(), address: address.into(), identity_file: key.map(Into::into), had_password }
    }

    fn check(scan: &AppScan) {
        for s in &scan.servers {
            crate::hosts::parse_address(&s.address).unwrap();
        }
    }

    #[test]
    fn putty_sessions_from_files() {
        let h = home("putty");
        put(&h, ".putty/sessions/web-7731", b"HostName=203.0.113.31\nUserName=deploy\nPortNumber=2222\nProtocol=ssh\nProxyMethod=0\nPublicKeyFile=~/.ssh/web\n");
        put(&h, ".putty/sessions/db%20main", b"HostName=ops@198.51.100.7\nPortNumber=22\nProtocol=ssh\n");
        put(&h, ".putty/sessions/Default%20Settings", b"HostName=\nProtocol=ssh\n");
        put(&h, ".putty/sessions/switch", b"HostName=203.0.113.40\nProtocol=telnet\n");
        put(&h, ".putty/sessions/inner", b"HostName=203.0.113.41\nProtocol=ssh\nProxyMethod=6\nProxyHost=203.0.113.1\n");
        put(&h, ".putty/sessions/ppk", b"HostName=203.0.113.42\nPublicKeyFile=/nowhere/key.ppk\n");
        put(&h, ".putty/sessions/broken", &[0xff, 0x00, 0xfe, b'\n', b'=', b'\\']);
        put(&h, ".putty/sessions/v6", b"HostName=2001:db8::5\nPortNumber=2200\n");
        let scan = scan(App::Putty, &Places::under(&h));
        assert_eq!(
            scan.servers,
            [
                found("db main", "ops@198.51.100.7", None, false),
                found("ppk", "203.0.113.42", None, false),
                found("v6", "[2001:db8::5]:2200", None, false),
                found("web-7731", "deploy@203.0.113.31:2222", Some(&h.join(".ssh/web").to_string_lossy()), false),
            ]
        );
        assert_eq!((scan.through_jump, scan.ppk_keys), (1, 1));
        check(&scan);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn kitty_session_files_and_their_passwords() {
        let h = home("kitty");
        // A .ppk with its OpenSSH copy beside it: the copy is used.
        put(&h, "keys/web", b"key");
        let key = h.join("keys/web.ppk");
        let text = format!("HostName\\203.0.113.51\\\nUserName\\root\\\nPortNumber\\22\\\nProtocol\\ssh\\\nPassword\\0a1b2c\\\nPublicKeyFile\\{}\\\n", key.display());
        put(&h, ".config/KiTTY/Sessions/mail-7731", text.as_bytes());
        let scan = scan(App::Kitty, &Places::under(&h));
        assert_eq!(scan.servers, [found("mail-7731", "root@203.0.113.51", Some(&h.join("keys/web").to_string_lossy()), true)]);
        assert_eq!(scan.ppk_keys, 0);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn winscp_ini_keeps_sftp_and_scp() {
        let h = home("winscp");
        let ini = "[Configuration]\nx=1\n\
            [Sessions\\Default%20Settings]\nHostName=203.0.113.99\n\
            [Sessions\\Work/web-7731]\nHostName=203.0.113.61\nUserName=deploy\nPortNumber=2222\nPassword=A35C4D\nPublicKeyFile=%2Fkeys%2Fdeploy\n\
            [Sessions\\scp-box]\nHostName=198.51.100.61\nFSProtocol=0\n\
            [Sessions\\ftp-box]\nHostName=198.51.100.62\nFSProtocol=5\n\
            [Sessions\\tunneled]\nHostName=198.51.100.105\nTunnel=1\nTunnelHostName=203.0.113.1\n\
            [Sessions\\empty]\nUserName=x\n";
        put(&h, ".config/WinSCP.ini", ini.as_bytes());
        let scan = scan(App::WinScp, &Places::under(&h));
        assert_eq!(scan.servers, [found("scp-box", "198.51.100.61", None, false), found("web-7731", "deploy@203.0.113.61:2222", Some("/keys/deploy"), true)]);
        assert_eq!(scan.through_jump, 1);
        check(&scan);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn mobaxterm_bookmarks() {
        let h = home("moba");
        let ini = "[Misc]\nx=1\r\n\
            [Bookmarks]\r\nSubRep=\r\nImgNum=42\r\n\
            web-7731=#109#0%203.0.113.71%2222%deploy%%-1%-1%%%22%%0%0%0%_ProfileDir_\\.ssh\\web%%-1%0#MobaFont%10#0# #-1\r\n\
            desktop=#91#4%203.0.113.72%3389%admin%0%-1#MobaFont#0# #-1\r\n\
            [Bookmarks_1]\r\nSubRep=Work\\Prod\r\nImgNum=41\r\n\
            db-7731=#109#0%198.51.100.71%22%root%%-1%-1%%%22%%0%0%0%%%-1%0#MobaFont#0# #-1\r\n\
            files-7731=#140#7%198.51.100.72%22%ops%%#MobaFont#0# #-1\r\n\
            inner=#109#0%198.51.100.109%22%root%%-1%-1%%203.0.113.1%22%jump%0%0%0%%%-1%0#MobaFont#0# #-1\r\n\
            junk=not a bookmark\r\n\
            [Passwords]\r\nssh22:root@198.51.100.71=_@9jajOXK\r\n";
        put(&h, ".config/MobaXterm/MobaXterm.ini", ini.as_bytes());
        let scan = scan(App::MobaXterm, &Places::under(&h));
        let key = format!("{}\\.ssh\\web", h.display());
        assert_eq!(
            scan.servers,
            [found("db-7731", "root@198.51.100.71", None, true), found("files-7731", "ops@198.51.100.72", None, false), found("web-7731", "deploy@203.0.113.71:2222", Some(&key), false)]
        );
        assert_eq!(scan.through_jump, 1);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn filezilla_sftp_sites() {
        let h = home("filezilla");
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<FileZilla3 version="3.66.0" platform="windows">
  <Servers>
    <Server>
      <Host>203.0.113.81</Host><Port>2222</Port><Protocol>1</Protocol><Type>0</Type>
      <User>deploy</User><Pass encoding="base64">c2VjcmV0</Pass><Logontype>1</Logontype>
      <Name>web-7731 &amp; co</Name>
    </Server>
    <Folder expanded="1">Work
      <Server>
        <Host>198.51.100.81</Host><Port>22</Port><Protocol>1</Protocol><User>ops</User>
        <Logontype>5</Logontype><Keyfile>/keys/ops</Keyfile><Name>db-7731</Name>
      </Server>
      <Server>
        <Host>198.51.100.82</Host><Port>21</Port><Protocol>0</Protocol><User>ftp</User><Name>ftp-7731</Name>
      </Server>
    </Folder>
    <Server><Host>203.0.113.83</Host><Protocol>1</Protocol>
  </Servers>
</FileZilla3>"#;
        put(&h, ".config/filezilla/sitemanager.xml", xml.as_bytes());
        let scan = scan(App::FileZilla, &Places::under(&h));
        assert_eq!(scan.servers, [found("db-7731", "ops@198.51.100.81", Some("/keys/ops"), false), found("web-7731 & co", "deploy@203.0.113.81:2222", None, true)]);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn securecrt_sessions_in_folders() {
        let h = home("securecrt");
        let dir = ".vandyke/SecureCRT/Config/Sessions";
        put(
            &h,
            &format!("{dir}/web-7731.ini"),
            b"\xEF\xBB\xBFS:\"Protocol Name\"=SSH2\r\nS:\"Hostname\"=203.0.113.91\r\nS:\"Username\"=deploy\r\nD:\"[SSH2] Port\"=000008ae\r\nS:\"Password V2\"=02:abcdef\r\nS:\"Identity Filename V2\"=/keys/web.pub::rawkey\r\nS:\"Firewall Name\"=None\r\n",
        );
        put(&h, &format!("{dir}/Prod/db-7731.ini"), b"S:\"Protocol Name\"=SSH2\nS:\"Hostname\"=198.51.100.91\nD:\"[SSH2] Port\"=00000016\nS:\"Password V2\"=\n");
        put(&h, &format!("{dir}/Prod/inner.ini"), b"S:\"Protocol Name\"=SSH2\nS:\"Hostname\"=198.51.100.109\nS:\"Firewall Name\"=Session:jump\n");
        put(&h, &format!("{dir}/serial.ini"), b"S:\"Protocol Name\"=Serial\n");
        put(&h, &format!("{dir}/Default.ini"), b"S:\"Protocol Name\"=SSH2\nS:\"Hostname\"=203.0.113.99\n");
        put(&h, &format!("{dir}/__FolderData__.ini"), b"Z:\"Folder List\"=00000000\n");
        let scan = scan(App::SecureCrt, &Places::under(&h));
        assert_eq!(scan.servers, [found("db-7731", "198.51.100.91", None, false), found("web-7731", "deploy@203.0.113.91:2222", Some("/keys/web"), true)]);
        assert_eq!(scan.through_jump, 1);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn remmina_ssh_and_sftp() {
        let h = home("remmina");
        let dir = ".local/share/remmina";
        put(&h, &format!("{dir}/1.remmina"), b"[remmina]\nname=web-7731\nprotocol=SSH\nserver=203.0.113.101:2222\nusername=deploy\nssh_privatekey=/keys/web\npassword=.\n");
        put(&h, &format!("{dir}/2.remmina"), b"[remmina]\nname=files-7731\nprotocol=SFTP\nserver=[2001:db8::7]:22\nusername=ops\n");
        put(&h, &format!("{dir}/3.remmina"), b"[remmina]\nname=desk\nprotocol=RDP\nserver=203.0.113.103\n");
        put(&h, &format!("{dir}/4.remmina"), b"[remmina]\nname=inner\nprotocol=SSH\nserver=198.51.100.104\nssh_tunnel_enabled=1\n");
        put(&h, &format!("{dir}/5.remmina"), b"not an ini at all");
        put(&h, &format!("{dir}/notes.txt"), b"[remmina]\nprotocol=SSH\nserver=203.0.113.105\n");
        let scan = scan(App::Remmina, &Places::under(&h));
        assert_eq!(scan.servers, [found("files-7731", "ops@[2001:db8::7]", None, false), found("web-7731", "deploy@203.0.113.101:2222", Some("/keys/web"), true)]);
        assert_eq!(scan.through_jump, 1);
        check(&scan);
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn utf16_files_read_and_nothing_is_nothing() {
        let h = home("utf16");
        let text = "[Sessions\\web-7731]\r\nHostName=203.0.113.111\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        put(&h, ".config/WinSCP.ini", &bytes);
        assert_eq!(scan(App::WinScp, &Places::under(&h)).servers, [found("web-7731", "203.0.113.111", None, false)]);
        let all = scan_all(&Places::under(&h));
        assert_eq!(all.iter().map(|(a, _)| *a).collect::<Vec<_>>(), [App::WinScp]);
        assert!(scan_all(&Places::under(&h.join("none"))).is_empty());
        let _ = std::fs::remove_dir_all(&h);
    }

    #[test]
    fn escapes_come_undone() {
        assert_eq!(unescape("db%20main%2"), "db main%2");
        assert_eq!(unescape("%"), "%");
        assert_eq!(unescape("100%25"), "100%");
        assert_eq!(xml_unescape("a &amp; b &lt;c&gt; &#65;&#x42; &bogus; &"), "a & b <c> AB &bogus; &");
        assert_eq!(split_host_port("[2001:db8::1]:2200"), ("2001:db8::1".into(), Some(2200)));
        assert_eq!(split_host_port("2001:db8::1"), ("2001:db8::1".into(), None));
        assert_eq!(split_host_port("web-7731:x"), ("web-7731".into(), None));
    }
}
