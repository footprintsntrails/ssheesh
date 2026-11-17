//! Tunnels: a server's port here on this computer, a SOCKS proxy through the server, or a
//! port on the server that leads back here (0.10's `Tunnels.tsx` and `src/lib/tunnels.ts`,
//! with the reverse kind 1.0 adds).
//!
//! Saved tunnels are kept per server, by host id, with their switch (`model::SavedTunnels`,
//! 0.10's own shape): the ones left on start again when the app opens. Each one's ssh runs in
//! the backend (sshee-core's `tunnels`); its state comes back as a [`State`]. Nothing here
//! knows about a window or a process: the form, the words, and the list's changes.

use std::collections::HashMap;

use crate::model::{Forward, ForwardKind, SavedTunnel, SavedTunnels};

/// The server itself, as the address a tunnel leads to.
pub const SERVER: &str = "127.0.0.1";

/// How a tunnel is doing (sshee-core's `TunnelState`). None in the map: off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    /// `warning`: why the last connection through it went nowhere.
    On { warning: Option<String> },
    Reconnecting { attempt: u32, retry_in_ms: u64, reason: String },
    /// Stopped for good: trying again by itself can't help.
    Failed { message: String },
}

/// Starting, on or reconnecting: it holds its port.
pub fn is_active(state: Option<&State>) -> bool {
    matches!(state, Some(State::Starting | State::On { .. } | State::Reconnecting { .. }))
}

/// What the window asks the backend.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// Starts tunnel `id` to the server `host` (its host id), or starts it again the new way.
    Start { id: String, host: String, forward: Forward },
    Stop { id: String },
    /// The whole list, to keep for next time.
    Save(SavedTunnels),
}

/// What the backend tells the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// The list kept from last time; the ones on are starting.
    Loaded(SavedTunnels),
    /// A tunnel's state; `None` when it stopped.
    State { id: String, state: Option<State> },
}

/// The tunnels the window knows: the saved ones and how each is doing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tunnels {
    pub saved: SavedTunnels,
    pub states: HashMap<String, State>,
}

impl Tunnels {
    /// A server's saved tunnels, in the order they were added.
    pub fn of(&self, host: &str) -> &[SavedTunnel] {
        self.saved.by_host.get(host).map_or(&[], Vec::as_slice)
    }

    /// How many of a server's tunnels are on, starting or reconnecting.
    pub fn active(&self, host: &str) -> usize {
        self.of(host).iter().filter(|t| is_active(self.states.get(&t.id))).count()
    }

    fn find_mut(&mut self, host: &str, id: &str) -> Option<&mut SavedTunnel> {
        self.saved.by_host.get_mut(host)?.iter_mut().find(|t| t.id == id)
    }

    /// An id no saved tunnel has.
    pub fn new_id(&self) -> String {
        let taken: Vec<&str> = self.saved.by_host.values().flatten().map(|t| t.id.as_str()).collect();
        (1..).map(|n| format!("tun-{n}")).find(|id| !taken.contains(&id.as_str())).unwrap_or_default()
    }

    /// Why `forward` can't be saved as tunnel `id` (`None` for a new one): another saved
    /// tunnel listens on the same port here.
    pub fn clash(&self, id: Option<&str>, forward: &Forward) -> Option<String> {
        let port = listens_here(forward)?;
        let other = self.saved.by_host.values().flatten().any(|t| Some(t.id.as_str()) != id && listens_here(&t.forward) == Some(port));
        other.then(|| format!("Another saved tunnel already uses port {port} on this computer."))
    }

    /// Saves a new tunnel, switched on, or changes one. What the backend is to do: start it,
    /// or start an on one again with its new settings; and keep the list.
    pub fn save(&mut self, host: &str, id: Option<&str>, name: &str, forward: Forward) -> Vec<Ask> {
        let name = Some(name.trim().to_owned()).filter(|n| !n.is_empty());
        let (id, start) = match id.and_then(|id| self.find_mut(host, id)) {
            Some(t) => {
                t.name = name;
                t.forward = forward.clone();
                (t.id.clone(), t.on)
            }
            None => {
                let id = self.new_id();
                self.saved.by_host.entry(host.to_owned()).or_default().push(SavedTunnel { id: id.clone(), name, forward: forward.clone(), on: true, ..SavedTunnel::default() });
                (id, true)
            }
        };
        let mut asks = Vec::new();
        if start {
            self.states.insert(id.clone(), State::Starting);
            asks.push(Ask::Start { id, host: host.to_owned(), forward });
        }
        asks.push(Ask::Save(self.saved.clone()));
        asks
    }

    /// The switch of a saved tunnel.
    pub fn set_on(&mut self, host: &str, id: &str, on: bool) -> Vec<Ask> {
        let Some(t) = self.find_mut(host, id) else { return vec![] };
        t.on = on;
        let forward = t.forward.clone();
        let first = if on {
            self.states.insert(id.to_owned(), State::Starting);
            Ask::Start { id: id.to_owned(), host: host.to_owned(), forward }
        } else {
            self.states.remove(id);
            Ask::Stop { id: id.to_owned() }
        };
        vec![first, Ask::Save(self.saved.clone())]
    }

    /// Starts a tunnel that failed again, as it is.
    pub fn retry(&mut self, host: &str, id: &str) -> Vec<Ask> {
        let Some(t) = self.of(host).iter().find(|t| t.id == id && t.on) else { return vec![] };
        let forward = t.forward.clone();
        self.states.insert(id.to_owned(), State::Starting);
        vec![Ask::Start { id: id.to_owned(), host: host.to_owned(), forward }]
    }

    /// Takes a tunnel out of the list, stopped.
    pub fn remove(&mut self, host: &str, id: &str) -> Vec<Ask> {
        if let Some(list) = self.saved.by_host.get_mut(host) {
            list.retain(|t| t.id != id);
            if list.is_empty() {
                self.saved.by_host.shift_remove(host);
            }
        }
        self.states.remove(id);
        vec![Ask::Stop { id: id.to_owned() }, Ask::Save(self.saved.clone())]
    }

    /// The backend's news.
    pub fn deliver(&mut self, reply: Reply) {
        match reply {
            Reply::Loaded(saved) => {
                for t in saved.by_host.values().flatten().filter(|t| t.on) {
                    self.states.entry(t.id.clone()).or_insert(State::Starting);
                }
                self.saved = saved;
            }
            // A tunnel switched off meanwhile keeps quiet: its late news isn't shown.
            Reply::State { id, state: Some(state) } => {
                if self.saved.by_host.values().flatten().any(|t| t.id == id && t.on) {
                    self.states.insert(id, state);
                }
            }
            Reply::State { id, state: None } => {
                self.states.remove(&id);
            }
        }
    }
}

/// The port a tunnel listens on here: none for a reverse one.
pub fn listens_here(forward: &Forward) -> Option<u16> {
    (forward.kind != ForwardKind::Remote).then_some(forward.local_port)
}

/// A local forward to `remote_port` on `remote_host` (empty: the server itself).
pub fn local(local_port: u16, remote_host: &str, remote_port: u16) -> Forward {
    Forward { kind: ForwardKind::Local, local_port, remote_host: Some(remote_host.to_owned()), remote_port: Some(remote_port), ..Forward::default() }
}

/// A SOCKS proxy on `local_port`.
pub fn socks(local_port: u16) -> Forward {
    Forward { kind: ForwardKind::Socks, local_port, ..Forward::default() }
}

/// The server's `remote_port` leading back to `local_port` here.
pub fn reverse(remote_port: u16, local_port: u16) -> Forward {
    Forward { kind: ForwardKind::Remote, local_port, remote_port: Some(remote_port), ..Forward::default() }
}

/// The address a local forward leads to, as the server sees it.
pub fn remote_host(forward: &Forward) -> &str {
    match forward.remote_host.as_deref().map(str::trim) {
        None | Some("") => SERVER,
        Some(h) => h,
    }
}

/// "localhost:8080 on this computer opens the server's port 3000", in plain words.
pub fn describe(forward: &Forward) -> String {
    let port = forward.local_port;
    let remote = forward.remote_port.unwrap_or_default();
    match forward.kind {
        ForwardKind::Socks => format!("Apps set to use localhost:{port} as a SOCKS proxy go out through the server."),
        ForwardKind::Remote => format!("Port {remote} on the server opens localhost:{port} on this computer."),
        _ => {
            let host = remote_host(forward);
            let there = if host == SERVER || host == "localhost" { format!("the server's port {remote}") } else { format!("port {remote} on {host}, reached through the server") };
            format!("localhost:{port} on this computer opens {there}.")
        }
    }
}

/// A short name for a saved tunnel without one.
pub fn title(t: &SavedTunnel) -> String {
    if let Some(name) = t.name.as_deref().filter(|n| !n.trim().is_empty()) {
        return name.to_owned();
    }
    let f = &t.forward;
    let remote = f.remote_port.unwrap_or_default();
    match f.kind {
        ForwardKind::Socks => "Browse through the server".into(),
        ForwardKind::Remote => format!("This computer's port {} on the server", f.local_port),
        _ if remote_host(f) == SERVER => format!("Server port {remote}"),
        _ => format!("{}:{remote}", remote_host(f)),
    }
}

/// How a tunnel's state reads, and in which tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Quiet,
    Good,
    Warn,
    Bad,
}

/// "On at localhost:3000", "Starting…", why it failed. A tunnel switched off is "Off".
pub fn status_words(forward: &Forward, state: Option<&State>) -> (String, Tone) {
    match state {
        None => ("Off".into(), Tone::Quiet),
        Some(State::Starting) => ("Starting…".into(), Tone::Quiet),
        Some(State::On { .. }) => match forward.kind {
            ForwardKind::Remote => (format!("On at port {} on the server", forward.remote_port.unwrap_or_default()), Tone::Good),
            _ => (format!("On at localhost:{}", forward.local_port), Tone::Good),
        },
        Some(State::Reconnecting { reason, retry_in_ms, .. }) => (format!("{reason}. Trying again in {}s…", retry_in_ms.div_ceil(1000).max(1)), Tone::Warn),
        Some(State::Failed { message }) => (message.clone(), Tone::Bad),
    }
}

/// Why a port typed in the form won't do.
pub fn port_error(text: &str, what: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return Some(format!("Enter {what}."));
    }
    match text.parse::<u32>() {
        Ok(1..=65535) => None,
        _ => {
            let mut cap = what.chars();
            let what = cap.next().map(|c| c.to_uppercase().collect::<String>() + cap.as_str()).unwrap_or_default();
            Some(format!("{what} is a number from 1 to 65535."))
        }
    }
}

/// Why an address as the server sees it won't do. Empty is the server itself.
pub fn host_error(text: &str) -> Option<String> {
    let h = text.trim();
    if h.is_empty() {
        return None;
    }
    let fine = !h.starts_with('-') && h.len() <= 253 && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '%' | '[' | ']' | '-'));
    (!fine).then(|| format!("\"{h}\" isn't an address SSHeesh can use."))
}

/// Only digits, at most five: what a port field keeps of what's typed.
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).take(5).collect()
}

/// The add-or-edit form, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub kind: Kind,
    /// The server's port: where a local forward leads, or where a reverse one listens.
    pub remote_port: String,
    /// The port here. Empty: it follows the server's port (or 1080 for a proxy).
    pub local_port: String,
    pub remote_host: String,
    pub name: String,
}

/// The form's three kinds, in the order the segmented control shows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Local,
    Socks,
    Reverse,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Local, Kind::Socks, Kind::Reverse];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Local => "A server's port here",
            Kind::Socks => "Browse through it",
            Kind::Reverse => "A port here, on the server",
        }
    }
}

impl Form {
    /// The form for a saved tunnel.
    pub fn of(t: &SavedTunnel) -> Form {
        let f = &t.forward;
        let kind = match f.kind {
            ForwardKind::Socks => Kind::Socks,
            ForwardKind::Remote => Kind::Reverse,
            _ => Kind::Local,
        };
        Form {
            kind,
            remote_port: f.remote_port.map(|p| p.to_string()).unwrap_or_default(),
            local_port: f.local_port.to_string(),
            remote_host: f.remote_host.clone().filter(|h| h != SERVER).unwrap_or_default(),
            name: t.name.clone().unwrap_or_default(),
        }
    }

    /// The port here as it will be: what was typed, or what it follows.
    pub fn local_text(&self) -> String {
        match (self.local_port.trim(), self.kind) {
            ("", Kind::Socks) => "1080".into(),
            ("", _) => self.remote_port.trim().to_owned(),
            (typed, _) => typed.to_owned(),
        }
    }

    /// The tunnel the form describes, or why it can't be one yet.
    pub fn forward(&self) -> Result<Forward, String> {
        let here = self.local_text();
        let port = |text: &str| text.trim().parse::<u16>().unwrap_or_default();
        match self.kind {
            Kind::Socks => {
                if let Some(e) = port_error(&here, "a port on this computer") {
                    return Err(e);
                }
                Ok(socks(port(&here)))
            }
            Kind::Local => {
                if let Some(e) = port_error(&self.remote_port, "the server's port").or_else(|| host_error(&self.remote_host)).or_else(|| port_error(&here, "a port on this computer")) {
                    return Err(e);
                }
                let host = self.remote_host.trim();
                Ok(local(port(&here), if host.is_empty() { SERVER } else { host }, port(&self.remote_port)))
            }
            Kind::Reverse => {
                if let Some(e) = port_error(&self.remote_port, "the server's port").or_else(|| port_error(&here, "a port on this computer")) {
                    return Err(e);
                }
                Ok(reverse(port(&self.remote_port), port(&here)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_tunnel_is_saved_on_and_starts() {
        let mut t = Tunnels::default();
        let asks = t.save("demo-web", None, " Grafana ", local(3000, SERVER, 3000));
        let saved = &t.of("demo-web")[0];
        assert_eq!((saved.id.as_str(), saved.name.as_deref(), saved.on), ("tun-1", Some("Grafana"), true));
        assert!(matches!(&asks[0], Ask::Start { id, host, .. } if id == "tun-1" && host == "demo-web"));
        assert!(matches!(&asks[1], Ask::Save(list) if list.by_host["demo-web"].len() == 1));
        assert_eq!(t.states.get("tun-1"), Some(&State::Starting));
        assert_eq!(t.active("demo-web"), 1);
        // The next one gets the next id.
        t.save("demo-web", None, "", socks(1080));
        assert_eq!(t.of("demo-web")[1].id, "tun-2");
        assert_eq!(t.of("demo-web")[1].name, None);
    }

    #[test]
    fn switching_editing_and_removing_tell_the_backend() {
        let mut t = Tunnels::default();
        t.save("demo-db", None, "", local(5432, SERVER, 5432));
        let off = t.set_on("demo-db", "tun-1", false);
        assert_eq!(off[0], Ask::Stop { id: "tun-1".into() });
        assert_eq!(t.active("demo-db"), 0);
        // Late news of a tunnel switched off isn't shown.
        t.deliver(Reply::State { id: "tun-1".into(), state: Some(State::On { warning: None }) });
        assert_eq!(t.states.get("tun-1"), None);
        // An off one changed stays off: only the list is kept.
        let edit = t.save("demo-db", Some("tun-1"), "Postgres", local(15432, SERVER, 5432));
        assert!(matches!(edit.as_slice(), [Ask::Save(_)]));
        assert_eq!(t.of("demo-db")[0].forward.local_port, 15432);
        // On again, it starts with its new settings.
        let on = t.set_on("demo-db", "tun-1", true);
        assert!(matches!(&on[0], Ask::Start { forward, .. } if forward.local_port == 15432));
        t.deliver(Reply::State { id: "tun-1".into(), state: Some(State::Failed { message: "Port 15432 on this computer is already in use.".into() }) });
        assert!(!is_active(t.states.get("tun-1")));
        assert!(matches!(t.retry("demo-db", "tun-1").as_slice(), [Ask::Start { .. }]));
        let gone = t.remove("demo-db", "tun-1");
        assert_eq!(gone[0], Ask::Stop { id: "tun-1".into() });
        assert!(t.of("demo-db").is_empty() && !t.saved.by_host.contains_key("demo-db"));
    }

    #[test]
    fn the_list_from_last_time_shows_the_ones_left_on_as_starting() {
        let mut saved = SavedTunnels::default();
        saved.by_host.insert("demo-web".into(), vec![
            SavedTunnel { id: "a".into(), forward: local(3000, SERVER, 3000), on: true, ..SavedTunnel::default() },
            SavedTunnel { id: "b".into(), forward: socks(1080), on: false, ..SavedTunnel::default() },
        ]);
        let mut t = Tunnels::default();
        t.deliver(Reply::Loaded(saved));
        assert_eq!(t.states.get("a"), Some(&State::Starting));
        assert_eq!(t.states.get("b"), None);
        assert_eq!(t.new_id(), "tun-1");
    }

    #[test]
    fn two_tunnels_never_listen_on_one_port_here() {
        let mut t = Tunnels::default();
        t.save("demo-web", None, "", local(8080, SERVER, 80));
        assert!(t.clash(None, &local(8080, SERVER, 8080)).unwrap().contains("port 8080"));
        assert_eq!(t.clash(Some("tun-1"), &local(8080, SERVER, 8080)), None, "its own port is its own");
        // A reverse tunnel listens on the server, not here.
        assert_eq!(t.clash(None, &reverse(9000, 8080)), None);
    }

    #[test]
    fn the_form_follows_the_servers_port_until_one_is_typed() {
        let mut f = Form { remote_port: "3000".into(), ..Form::default() };
        assert_eq!(f.local_text(), "3000");
        assert_eq!(f.forward().unwrap(), local(3000, SERVER, 3000));
        f.local_port = "8080".into();
        f.remote_host = "db.internal".into();
        assert_eq!(f.forward().unwrap(), local(8080, "db.internal", 3000));
        f.remote_host = "-oProxyCommand=sh".into();
        assert!(f.forward().unwrap_err().contains("isn't an address"));
        let proxy = Form { kind: Kind::Socks, ..Form::default() };
        assert_eq!(proxy.forward().unwrap(), socks(1080));
        let back = Form { kind: Kind::Reverse, remote_port: "9000".into(), local_port: "3000".into(), ..Form::default() };
        assert_eq!(back.forward().unwrap(), reverse(9000, 3000));
        assert_eq!(Form { kind: Kind::Local, ..Form::default() }.forward().unwrap_err(), "Enter the server's port.");
        assert_eq!(Form { kind: Kind::Local, remote_port: "70000".into(), ..Form::default() }.forward().unwrap_err(), "The server's port is a number from 1 to 65535.");
        // A saved one comes back into the form as it was typed.
        let saved = SavedTunnel { id: "x".into(), name: Some("Grafana".into()), forward: local(13000, SERVER, 3000), on: true, ..SavedTunnel::default() };
        let again = Form::of(&saved);
        assert_eq!((again.remote_port.as_str(), again.local_port.as_str(), again.remote_host.as_str(), again.name.as_str()), ("3000", "13000", "", "Grafana"));
        assert_eq!(digits("80a80 9"), "80809");
    }

    #[test]
    fn tunnels_read_in_plain_words() {
        assert_eq!(describe(&local(8080, "", 3000)), "localhost:8080 on this computer opens the server's port 3000.");
        assert_eq!(describe(&local(5432, "10.0.0.5", 5432)), "localhost:5432 on this computer opens port 5432 on 10.0.0.5, reached through the server.");
        assert!(describe(&socks(1080)).contains("SOCKS proxy"));
        assert_eq!(describe(&reverse(9000, 3000)), "Port 9000 on the server opens localhost:3000 on this computer.");
        let t = |forward| SavedTunnel { forward, ..SavedTunnel::default() };
        assert_eq!(title(&t(local(3000, SERVER, 3000))), "Server port 3000");
        assert_eq!(title(&t(local(5432, "10.0.0.5", 5432))), "10.0.0.5:5432");
        assert_eq!(title(&t(socks(1080))), "Browse through the server");
        assert_eq!(title(&t(reverse(9000, 3000))), "This computer's port 3000 on the server");
        assert_eq!(status_words(&local(3000, SERVER, 3000), Some(&State::On { warning: None })), ("On at localhost:3000".into(), Tone::Good));
        assert_eq!(status_words(&reverse(9000, 3000), Some(&State::On { warning: None })).0, "On at port 9000 on the server");
        let again = State::Reconnecting { attempt: 2, retry_in_ms: 2500, reason: "The server stopped responding".into() };
        assert_eq!(status_words(&socks(1080), Some(&again)), ("The server stopped responding. Trying again in 3s…".into(), Tone::Warn));
        assert_eq!(status_words(&socks(1080), None).1, Tone::Quiet);
    }
}
