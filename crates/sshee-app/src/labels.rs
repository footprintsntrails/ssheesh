//! A server's looks and labels as 1.0 keeps them: the icon for what it's for, its colour, its
//! Production, Staging or Development label, a name of your own, and which servers from
//! `~/.ssh/config` show at all. A port of the rules in 0.10's `src/lib/labels.ts`, on the same
//! store ([`Labels`]), so what 0.10 kept carries over as it is.
//!
//! New in 1.0: a server can be renamed here without touching the list it comes from. A server
//! from `~/.ssh/config` is named to ssh by its alias, so its name in the file can't change;
//! the name you give it is kept beside the labels ([`Looks::names`]).
//!
//! Nothing here reads or writes a file: the program keeps a [`Looks`] in its own folder and
//! turns it, with the list of hosts, into the sidebar's [`Server`]s.

use crate::model::{EnvLabel, HostLook, Labels};
use crate::workbench::{Server, ServerKey};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The icons a server can have, as 0.10 lists them (`HOST_ICONS`): the id that is saved, and
/// the word shown for it.
pub const HOST_ICONS: [(&str, &str); 30] = [
    ("server", "Server"),
    ("cloud", "Cloud"),
    ("globe", "Website"),
    ("database", "Database"),
    ("box", "Containers"),
    ("home", "Home"),
    ("router", "Network"),
    ("drive", "Storage"),
    ("cpu", "Compute"),
    ("shield", "Security"),
    ("vpn", "VPN"),
    ("rocket", "Launch"),
    ("flask", "Lab"),
    ("bot", "Bot"),
    ("ai", "AI"),
    ("terminal", "Dev box"),
    ("git", "Git"),
    ("mail", "Mail"),
    ("chat", "Chat"),
    ("shop", "Shop"),
    ("vault", "Passwords"),
    ("chart", "Monitoring"),
    ("backup", "Backups"),
    ("game", "Game server"),
    ("film", "Media"),
    ("music", "Music"),
    ("camera", "Cameras"),
    ("iot", "Smart home"),
    ("desktop", "Workstation"),
    ("laptop", "Laptop"),
];

/// Icon colours: blue, indigo, violet, magenta, cyan and grey (0.10's `HOST_COLORS`). Never
/// red, orange, yellow or green: those say how a server is doing.
pub const HOST_COLORS: [&str; 6] = ["#2389e2", "#7577e3", "#9d69d2", "#ba5db3", "#0997b4", "#7e8590"];

/// The word for an icon ("Website" for `globe`); "Server" for an id this version doesn't know.
pub fn icon_name(id: &str) -> &'static str {
    HOST_ICONS.iter().find(|(i, _)| *i == id).map_or("Server", |(_, name)| name)
}

/// A server's own colour before you pick one: the same every time, so servers differ at a
/// glance. Not grey: it looks switched off. The hash is 0.10's (`hostColor`), over UTF-16
/// units as JavaScript counts them, so a server keeps the colour it had there.
pub fn host_color(host_id: &str) -> &'static str {
    let h = host_id.encode_utf16().fold(0i32, |h, unit| h.wrapping_mul(31).wrapping_add(i32::from(unit)));
    HOST_COLORS[(h as u32 % (HOST_COLORS.len() as u32 - 1)) as usize]
}

/// The labels in the order menus list them.
pub fn env_order() -> [EnvLabel; 3] {
    [EnvLabel::Prod, EnvLabel::Staging, EnvLabel::Dev]
}

/// "Production", "Staging", "Development": a label's whole name.
pub fn env_name(env: &EnvLabel) -> &str {
    match env {
        EnvLabel::Prod => "Production",
        EnvLabel::Staging => "Staging",
        EnvLabel::Dev => "Development",
        EnvLabel::Other(other) => other,
    }
}

/// "Prod", "Staging", "Dev": the word on a chip.
pub fn env_short(env: &EnvLabel) -> &str {
    match env {
        EnvLabel::Prod => "Prod",
        EnvLabel::Staging => "Staging",
        EnvLabel::Dev => "Dev",
        EnvLabel::Other(other) => other,
    }
}

/// A server as the list has it: sshee-core's `Host`, told without sshee-core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// `saved:<random>` or `cfg:<Host alias>`.
    pub id: String,
    /// As the list names it.
    pub name: String,
    /// `user@host`, and `:port` when it isn't 22.
    pub address: String,
    /// It comes from `~/.ssh/config`: it can be hidden, not removed.
    pub from_config: bool,
}

/// A server's icon and colour as shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Look {
    pub icon: String,
    /// `#rrggbb`.
    pub color: String,
    /// You picked it; else the icon is the guessed one and the colour the server's own.
    pub picked: bool,
    /// A picture of your own instead of the icon: a small PNG as a data URL (0.10's own
    /// shape). Never drawn while streaming.
    pub image: Option<String>,
    /// What SSHeesh guessed the server is for ([`crate::roles`]), once it has looked: the
    /// icon "Automatic" goes back to.
    pub guessed: Option<String>,
}

/// A picture as the window draws it: straight-alpha RGBA, rows top first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: std::sync::Arc<[u8]>,
}

/// What the window needs to draw one server and offer its menu: its look, its picture made
/// ready to draw (the backend decodes it, off the UI thread), and whether a password is saved
/// for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Face {
    pub look: Look,
    pub picture: Option<Picture>,
    pub saved_password: bool,
}

/// How big a server's picture is kept: 0.10 made every picture this square (`hostImageFrom`).
pub const IMAGE_SIDE: u32 = 64;

/// The biggest picture file read to make a server's icon from.
pub const IMAGE_FILE_MAX: u64 = 8 * 1024 * 1024;

/// More colours than the six, for "Your own colour": hues and greys any server can wear. The
/// six of [`HOST_COLORS`] come first in the picker.
pub const MORE_COLORS: [&str; 12] =
    ["#e5484d", "#f76b15", "#ffb224", "#46a758", "#12a594", "#3e63dd", "#8e4ec6", "#d6409f", "#a18072", "#6e56cf", "#29a383", "#1f2328"];

/// A colour as typed: `#2389e2`, `2389E2`, or `#28e` read as `#2288ee` (lower case, six
/// digits). `None` when it isn't one.
pub fn parse_color(text: &str) -> Option<String> {
    let hex = text.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let six = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => hex.to_owned(),
        _ => return None,
    };
    Some(format!("#{}", six.to_ascii_lowercase()))
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A PNG's bytes as the data URL 0.10 keeps a server's picture in (`data:image/png;base64,…`).
pub fn image_data_url(png: &[u8]) -> String {
    let mut out = String::from("data:image/png;base64,");
    for chunk in png.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(BASE64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes of a picture kept as a data URL (`data:image/…;base64,…`). `None` for anything
/// else: a link to somewhere is never followed.
pub fn image_bytes(url: &str) -> Option<Vec<u8>> {
    let (head, body) = url.strip_prefix("data:image/")?.split_once(',')?;
    if !head.ends_with(";base64") {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() / 4 * 3);
    let (mut n, mut bits) = (0u32, 0);
    for b in body.bytes().filter(|b| !b.is_ascii_whitespace()) {
        if b == b'=' {
            break;
        }
        let v = BASE64.iter().position(|c| *c == b)? as u32;
        n = ((n << 6) | v) & 0xff_ffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((n >> bits) as u8);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Everything 1.0 keeps about how servers look and what they're called.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Looks {
    /// 0.10's store, as it was: labels, picked and guessed looks, hidden and seen hosts.
    pub labels: Labels,
    /// Names given here, by host id. A host without one keeps the list's name.
    pub names: IndexMap<String, String>,
}

impl Looks {
    /// What a server is called: the name you gave it, else the list's.
    pub fn name<'a>(&'a self, host: &'a Listed) -> &'a str {
        self.names.get(&host.id).map_or(host.name.as_str(), String::as_str)
    }

    /// Names a server. An empty name, or the list's own, gives the list's name back.
    pub fn rename(&mut self, host: &Listed, name: &str) {
        let name = name.trim();
        if name.is_empty() || name == host.name {
            self.names.shift_remove(&host.id);
        } else {
            self.names.insert(host.id.clone(), name.to_owned());
        }
    }

    /// The look a server shows: yours, else the guessed icon (or the plain server) in the
    /// server's own colour (0.10's `lookOf`).
    pub fn look(&self, host_id: &str) -> Look {
        let auto = self.automatic_look(host_id);
        match self.labels.looks.get(host_id) {
            Some(mine) => Look {
                icon: if mine.icon.is_empty() { "server".into() } else { mine.icon.clone() },
                color: if mine.color.is_empty() { host_color(host_id).into() } else { mine.color.clone() },
                picked: true,
                // A picture that doesn't read is left in the file, and not drawn.
                image: mine.image.clone().filter(|i| image_bytes(i).is_some()),
                guessed: auto.guessed,
            },
            None => auto,
        }
    }

    /// The look "Automatic" shows: the guessed icon (or the plain server) in the server's own
    /// colour.
    pub fn automatic_look(&self, host_id: &str) -> Look {
        let guessed = self.labels.auto_looks.get(host_id).map(|l| l.icon.clone()).filter(|i| !i.is_empty());
        Look { icon: guessed.clone().unwrap_or_else(|| "server".into()), color: host_color(host_id).into(), picked: false, image: None, guessed }
    }

    /// Picks an icon; the colour stays as it shows and a picture goes (0.10's look picker:
    /// picking an icon or a colour drops the picture).
    pub fn set_icon(&mut self, host_id: &str, icon: &str) {
        let shown = self.look(host_id);
        let look = self.labels.looks.entry(host_id.to_owned()).or_insert_with(HostLook::default);
        look.icon = icon.to_owned();
        look.image = None;
        if look.color.is_empty() {
            look.color = shown.color;
        }
    }

    /// Picks a colour (anything [`parse_color`] reads); the icon stays as it shows and a
    /// picture goes. A colour that doesn't read changes nothing.
    pub fn set_color(&mut self, host_id: &str, color: &str) {
        let Some(color) = parse_color(color) else { return };
        let shown = self.look(host_id);
        let look = self.labels.looks.entry(host_id.to_owned()).or_insert_with(HostLook::default);
        look.color = color;
        look.image = None;
        if look.icon.is_empty() {
            look.icon = shown.icon;
        }
    }

    /// A picture of your own instead of the icon (a data URL: [`image_data_url`]), or `None`
    /// to take it away. Taking away a picture put on "Automatic" goes back to it (0.10's
    /// `LookPopover`).
    pub fn set_image(&mut self, host_id: &str, image: Option<String>) {
        let shown = self.look(host_id);
        match image {
            Some(image) => {
                let look = self.labels.looks.entry(host_id.to_owned()).or_insert_with(HostLook::default);
                look.icon = shown.icon;
                look.color = shown.color;
                look.image = Some(image);
            }
            None => {
                let auto = self.automatic_look(host_id);
                if shown.icon == auto.icon && shown.color == auto.color {
                    self.labels.looks.shift_remove(host_id);
                } else if let Some(look) = self.labels.looks.get_mut(host_id) {
                    look.image = None;
                }
            }
        }
    }

    /// "Automatic": the look you picked goes, and the server shows what SSHeesh guessed, in
    /// its own colour.
    pub fn automatic(&mut self, host_id: &str) {
        self.labels.looks.shift_remove(host_id);
    }

    /// What SSHeesh guessed a server is for (0.10's `setAutoLook`): kept for good, even "just a
    /// server", so it isn't looked at again on every connect. A look you picked still wins.
    pub fn set_guess(&mut self, host_id: &str, icon: &str) {
        self.labels.auto_looks.entry(host_id.to_owned()).or_default().icon = icon.to_owned();
    }

    /// Whether a server is worth looking at to guess its icon: you haven't picked one and
    /// SSHeesh hasn't guessed yet (0.10's `startRoleDetection`).
    pub fn wants_guess(&self, host_id: &str) -> bool {
        !self.labels.looks.contains_key(host_id) && !self.labels.auto_looks.contains_key(host_id)
    }

    /// A look given when a server is added: what was picked in the form.
    pub fn set_look(&mut self, host_id: &str, look: HostLook) {
        let mut look = look;
        look.color = parse_color(&look.color).unwrap_or_else(|| host_color(host_id).into());
        self.labels.looks.insert(host_id.to_owned(), look);
    }

    pub fn env(&self, host_id: &str) -> Option<&EnvLabel> {
        self.labels.env.get(host_id)
    }

    /// Labels a server, or takes its label off.
    pub fn set_env(&mut self, host_id: &str, env: Option<EnvLabel>) {
        match env {
            Some(env) => {
                self.labels.env.insert(host_id.to_owned(), env);
            }
            None => {
                self.labels.env.shift_remove(host_id);
            }
        }
    }

    /// Whether a server is in the sidebar: not hidden, and one from `~/.ssh/config` only once
    /// you've said yes to it (0.10's `visibleHosts`).
    pub fn shown(&self, host: &Listed) -> bool {
        !self.labels.hidden.contains(&host.id) && (!host.from_config || self.labels.config_seen.contains(&host.id))
    }

    /// Servers in `~/.ssh/config` you haven't said yes or no to yet.
    pub fn undecided<'a>(&self, hosts: &'a [Listed]) -> Vec<&'a Listed> {
        hosts.iter().filter(|h| h.from_config && !self.labels.config_seen.contains(&h.id)).collect()
    }

    /// Records your choice for some `~/.ssh/config` servers: those in `keep` show, the rest of
    /// `ids` stay hidden (0.10's `chooseConfig`).
    pub fn choose_config(&mut self, ids: &[String], keep: &[String]) {
        for id in ids {
            if !self.labels.config_seen.contains(id) {
                self.labels.config_seen.push(id.clone());
            }
            self.labels.hidden.retain(|h| h != id);
            if !keep.contains(id) {
                self.labels.hidden.push(id.clone());
            }
        }
    }

    /// Takes a server from `~/.ssh/config` out of the sidebar. It stays in the file.
    pub fn hide(&mut self, host_id: &str) {
        if !self.labels.hidden.iter().any(|h| h == host_id) {
            self.labels.hidden.push(host_id.to_owned());
        }
    }

    /// A server left the list for good: nothing is kept about it.
    pub fn forget(&mut self, host_id: &str) {
        self.names.shift_remove(host_id);
        self.labels.env.shift_remove(host_id);
        self.labels.looks.shift_remove(host_id);
        self.labels.auto_looks.shift_remove(host_id);
        self.labels.hidden.retain(|h| h != host_id);
        self.labels.config_seen.retain(|h| h != host_id);
    }

    /// Brings over what 0.10 kept. `moved` pairs a host's id there with its id here (a saved
    /// server gets a new id when it's added to this copy's list); a host from `~/.ssh/config`
    /// has the same id in both. What is already set here stays.
    pub fn carry(&mut self, from: &Labels, moved: &[(String, String)]) {
        let here = |id: &str| moved.iter().find(|(old, _)| old == id).map(|(_, new)| new.clone()).or_else(|| id.starts_with("cfg:").then(|| id.to_owned()));
        for (id, env) in &from.env {
            if let Some(id) = here(id) {
                self.labels.env.entry(id).or_insert_with(|| env.clone());
            }
        }
        for (id, look) in &from.looks {
            if let Some(id) = here(id) {
                self.labels.looks.entry(id).or_insert_with(|| look.clone());
            }
        }
        for (id, look) in &from.auto_looks {
            if let Some(id) = here(id) {
                self.labels.auto_looks.entry(id).or_insert_with(|| look.clone());
            }
        }
        for id in from.config_seen.iter().filter(|id| id.starts_with("cfg:")) {
            if !self.labels.config_seen.contains(id) {
                self.labels.config_seen.push(id.clone());
                if from.hidden.contains(id) {
                    self.hide(id);
                }
            }
        }
    }

    /// The sidebar's servers: the hosts that show, named, labelled and with their icons.
    pub fn servers(&self, hosts: &[Listed]) -> Vec<Server> {
        hosts
            .iter()
            .filter(|h| self.shown(h))
            .map(|h| {
                let mut server = Server::host(&h.id, self.name(h), &h.address).icon(&self.look(&h.id).icon);
                server.env = self.env(&h.id).cloned();
                server
            })
            .collect()
    }

    /// Each shown server's colour, by its key in the sidebar.
    pub fn colors(&self, hosts: &[Listed]) -> IndexMap<ServerKey, String> {
        hosts.iter().filter(|h| self.shown(h)).map(|h| (format!("host:{}", h.id), self.look(&h.id).color)).collect()
    }

    /// Each shown server's whole look, by its key in the sidebar.
    pub fn shown_looks(&self, hosts: &[Listed]) -> IndexMap<ServerKey, Look> {
        hosts.iter().filter(|h| self.shown(h)).map(|h| (format!("host:{}", h.id), self.look(&h.id))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AutoLook;

    fn saved(id: &str, name: &str) -> Listed {
        Listed { id: format!("saved:{id}"), name: name.into(), address: format!("deploy@203.0.113.{}", id.len() + 10), from_config: false }
    }

    fn config(name: &str) -> Listed {
        Listed { id: format!("cfg:{name}"), name: name.into(), address: name.into(), from_config: true }
    }

    #[test]
    fn thirty_icons_with_ids_of_their_own() {
        let ids: std::collections::HashSet<&str> = HOST_ICONS.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), 30);
        assert_eq!(icon_name("globe"), "Website");
        assert_eq!(icon_name("backup"), "Backups");
        assert_eq!(icon_name("something-new"), "Server");
    }

    #[test]
    fn a_servers_own_colour_is_0_10s() {
        // 0.10: hostColor("saved:1") and friends, run in the browser.
        assert_eq!(host_color("saved:1"), "#0997b4");
        assert_eq!(host_color("cfg:web-7731"), "#9d69d2");
        assert_eq!(host_color(""), "#2389e2");
        // Never grey, and the same every time.
        for id in ["a", "saved:18f3a", "cfg:db-main", "сервер"] {
            assert_ne!(host_color(id), HOST_COLORS[5], "{id}");
            assert_eq!(host_color(id), host_color(id));
        }
    }

    #[test]
    fn a_look_is_yours_else_the_guessed_icon_in_the_servers_own_colour() {
        let mut looks = Looks::default();
        assert_eq!(looks.look("saved:1"), Look { icon: "server".into(), color: host_color("saved:1").into(), picked: false, ..Look::default() });
        looks.labels.auto_looks.insert("saved:1".into(), AutoLook { icon: "database".into(), ..AutoLook::default() });
        assert_eq!(looks.look("saved:1").icon, "database");
        // Picking an icon keeps the colour it showed; picking a colour keeps the icon.
        looks.set_icon("saved:1", "globe");
        let guessed = Some("database".to_owned());
        assert_eq!(looks.look("saved:1"), Look { icon: "globe".into(), color: host_color("saved:1").into(), picked: true, guessed: guessed.clone(), image: None });
        looks.set_color("saved:1", "#7E8590");
        assert_eq!(looks.look("saved:1"), Look { icon: "globe".into(), color: "#7e8590".into(), picked: true, guessed, image: None });
        looks.set_color("saved:2", "#2389e2");
        assert_eq!(looks.look("saved:2").icon, "server");
        // A colour that isn't one changes nothing.
        looks.set_color("saved:1", "blue");
        assert_eq!(looks.look("saved:1").color, "#7e8590");
    }

    #[test]
    fn any_colour_reads_as_six_lower_case_digits() {
        assert_eq!(parse_color("#2389E2").as_deref(), Some("#2389e2"));
        assert_eq!(parse_color(" 2389e2 ").as_deref(), Some("#2389e2"));
        assert_eq!(parse_color("#28e").as_deref(), Some("#2288ee"));
        for bad in ["", "#", "#12345", "#1234567", "#ggg000", "blue", "##123456"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
        // The extra colours are colours, and none is one of the six.
        for c in MORE_COLORS {
            assert_eq!(parse_color(c).as_deref(), Some(c));
            assert!(!HOST_COLORS.contains(&c));
        }
    }

    #[test]
    fn a_picture_is_kept_as_0_10_keeps_it_and_read_back() {
        for bytes in [&b"\x89PNG"[..], b"\x89PNG\r", b"\x89PNG\r\n", &[0u8, 255, 128, 7, 9]] {
            let url = image_data_url(bytes);
            assert!(url.starts_with("data:image/png;base64,"));
            assert_eq!(image_bytes(&url).as_deref(), Some(bytes), "{url}");
        }
        // As a browser writes it.
        assert_eq!(image_data_url(b"Man"), "data:image/png;base64,TWFu");
        assert_eq!(image_data_url(b"Ma"), "data:image/png;base64,TWE=");
        assert_eq!(image_bytes("data:image/jpeg;base64,TWE="), Some(b"Ma".to_vec()));
        // Only a picture carried in the URL itself: nothing is fetched.
        for not in ["https://example.com/a.png", "data:text/plain;base64,TWFu", "data:image/png,Man", "data:image/png;base64,T!Fu", "data:image/png;base64,"] {
            assert_eq!(image_bytes(not), None, "{not}");
        }
    }

    #[test]
    fn a_picture_replaces_the_icon_until_an_icon_or_a_colour_is_picked() {
        let mut looks = Looks::default();
        let picture = image_data_url(b"\x89PNG");
        looks.set_image("saved:1", Some(picture.clone()));
        let look = looks.look("saved:1");
        assert_eq!((look.picked, look.image.as_deref(), look.icon.as_str()), (true, Some(picture.as_str()), "server"));
        // Taking it away from "Automatic" goes back to it.
        looks.set_image("saved:1", None);
        assert!(!looks.look("saved:1").picked);
        // Over an icon you picked, the icon stays.
        looks.set_icon("saved:1", "globe");
        looks.set_image("saved:1", Some(picture.clone()));
        looks.set_image("saved:1", None);
        assert_eq!((looks.look("saved:1").icon.as_str(), looks.look("saved:1").picked), ("globe", true));
        // An icon or a colour drops it.
        looks.set_image("saved:1", Some(picture.clone()));
        looks.set_color("saved:1", "#7577e3");
        assert_eq!(looks.look("saved:1").image, None);
        // One that doesn't read isn't drawn.
        looks.labels.looks.get_mut("saved:1").unwrap().image = Some("https://example.com/a.png".into());
        assert_eq!(looks.look("saved:1").image, None);
    }

    #[test]
    fn automatic_goes_back_to_the_guess_and_a_guess_is_asked_for_once() {
        let mut looks = Looks::default();
        assert!(looks.wants_guess("saved:1"));
        looks.set_guess("saved:1", "server");
        assert!(!looks.wants_guess("saved:1"));
        looks.set_guess("saved:1", "globe");
        assert_eq!(looks.look("saved:1").icon, "globe");
        looks.set_icon("saved:1", "database");
        assert_eq!(looks.look("saved:1").guessed.as_deref(), Some("globe"));
        looks.automatic("saved:1");
        assert_eq!(looks.look("saved:1"), looks.automatic_look("saved:1"));
        assert_eq!(looks.look("saved:1").icon, "globe");
        // A picked look means nothing is guessed.
        looks.set_look("saved:2", HostLook { icon: "shop".into(), color: "#ABC".into(), ..HostLook::default() });
        assert!(!looks.wants_guess("saved:2"));
        assert_eq!(looks.look("saved:2").color, "#aabbcc");
        looks.set_look("saved:3", HostLook { icon: "shop".into(), color: "nope".into(), ..HostLook::default() });
        assert_eq!(looks.look("saved:3").color, host_color("saved:3"));
        assert_eq!(looks.shown_looks(&[saved("3", "shop")])["host:saved:3"].icon, "shop");
    }

    #[test]
    fn a_name_of_your_own_wins_and_an_empty_one_gives_the_lists_back() {
        let host = config("web-7731");
        let mut looks = Looks::default();
        assert_eq!(looks.name(&host), "web-7731");
        looks.rename(&host, "  Shop  ");
        assert_eq!(looks.name(&host), "Shop");
        looks.rename(&host, " ");
        assert_eq!(looks.name(&host), "web-7731");
        looks.rename(&host, "web-7731");
        assert!(looks.names.is_empty());
    }

    #[test]
    fn servers_from_the_ssh_config_show_once_you_said_yes() {
        let hosts = [saved("1", "db-main"), config("web-7731"), config("old-box")];
        let mut looks = Looks::default();
        assert_eq!(looks.servers(&hosts).iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["db-main"]);
        assert_eq!(looks.undecided(&hosts).len(), 2);
        let ids: Vec<String> = hosts[1..].iter().map(|h| h.id.clone()).collect();
        looks.choose_config(&ids, &ids[..1]);
        assert_eq!(looks.servers(&hosts).iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["db-main", "web-7731"]);
        assert!(looks.undecided(&hosts).is_empty());
        // Choosing again can bring the other back and hide the first.
        looks.choose_config(&ids, &ids[1..]);
        assert_eq!(looks.servers(&hosts).iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["db-main", "old-box"]);
        looks.hide("cfg:old-box");
        looks.hide("cfg:old-box");
        assert_eq!(looks.labels.hidden.iter().filter(|h| *h == "cfg:old-box").count(), 1);
        assert_eq!(looks.servers(&hosts).len(), 1);
    }

    #[test]
    fn the_sidebars_servers_carry_name_label_icon_and_colour() {
        let hosts = [saved("1", "db-main"), saved("22", "web-7731")];
        let mut looks = Looks::default();
        looks.set_env("saved:1", Some(EnvLabel::Prod));
        looks.set_icon("saved:1", "database");
        looks.rename(&hosts[1], "Shop");
        let servers = looks.servers(&hosts);
        assert_eq!((servers[0].name.as_str(), servers[0].icon.as_str(), servers[0].is_prod()), ("db-main", "database", true));
        assert_eq!((servers[1].name.as_str(), servers[1].icon.as_str(), servers[1].is_prod()), ("Shop", "server", false));
        assert_eq!(servers[1].address, hosts[1].address);
        let colors = looks.colors(&hosts);
        assert_eq!(colors["host:saved:22"], host_color("saved:22"));
        looks.set_env("saved:1", None);
        assert!(!looks.servers(&hosts)[0].is_prod());
        assert_eq!((env_name(&EnvLabel::Prod), env_short(&EnvLabel::Prod), env_short(&EnvLabel::Dev)), ("Production", "Prod", "Dev"));
    }

    #[test]
    fn a_removed_server_leaves_nothing_behind() {
        let host = saved("1", "db-main");
        let mut looks = Looks::default();
        looks.set_env(&host.id, Some(EnvLabel::Staging));
        looks.set_icon(&host.id, "database");
        looks.rename(&host, "Main database");
        looks.forget(&host.id);
        assert_eq!(looks, Looks::default());
    }

    #[test]
    fn what_0_10_kept_comes_over_under_the_new_ids() {
        let mut old = Labels::default();
        old.env.insert("saved:old1".into(), EnvLabel::Prod);
        old.env.insert("saved:gone".into(), EnvLabel::Dev);
        old.env.insert("cfg:web-7731".into(), EnvLabel::Staging);
        old.looks.insert("saved:old1".into(), HostLook { icon: "database".into(), color: "#0997b4".into(), ..HostLook::default() });
        old.auto_looks.insert("cfg:web-7731".into(), AutoLook { icon: "globe".into(), ..AutoLook::default() });
        old.config_seen = vec!["cfg:web-7731".into(), "cfg:old-box".into()];
        old.hidden = vec!["cfg:old-box".into()];
        let mut looks = Looks::default();
        // Already labelled here: it stays.
        looks.set_env("cfg:web-7731", Some(EnvLabel::Prod));
        looks.carry(&old, &[("saved:old1".into(), "saved:new1".into())]);
        assert_eq!(looks.env("saved:new1"), Some(&EnvLabel::Prod));
        assert_eq!(looks.look("saved:new1"), Look { icon: "database".into(), color: "#0997b4".into(), picked: true, ..Look::default() });
        assert_eq!(looks.env("cfg:web-7731"), Some(&EnvLabel::Prod));
        assert_eq!(looks.look("cfg:web-7731").icon, "globe");
        // A saved server that wasn't brought over brings nothing.
        assert_eq!(looks.env("saved:gone"), None);
        let hosts = [config("web-7731"), config("old-box")];
        assert_eq!(looks.servers(&hosts).iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["web-7731"]);
        // As a file and back.
        let json = serde_json::to_string(&looks).unwrap();
        assert_eq!(serde_json::from_str::<Looks>(&json).unwrap(), looks);
    }
}
