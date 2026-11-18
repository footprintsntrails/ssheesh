//! Tunnels: a server's port here on this computer, a SOCKS proxy through the server, or a
//! port on the server that leads back here (0.10's `Tunnels.tsx` and `src/lib/tunnels.ts`,
//! with the reverse kind 1.0 adds).
//!
//! Saved tunnels are kept per server, by host id, with their switch (`model::SavedTunnels`,
//! 0.10's own shape): the ones left on start again when the app opens. Each one's ssh runs in
//! the backend (sshee-core's `tunnels`); its state comes back as a [`State`]. Nothing here
//! knows about a window or a process: the form, the words, and the list's changes.
//!
//! "Web apps on this server" (0.10's `WebApps`): a look at what listens on the server, each
//! page's title asked for ([`Scan`]). "Open" makes one reachable here through a temporary
//! tunnel ([`Temp`], never saved) and opens it in the browser once the tunnel is on; "Keep"
//! saves that tunnel, "Stop" stops it.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::model::{Forward, ForwardKind, SavedTunnel, SavedTunnels, Scheme};

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
    /// A look at what listens on the server `host` (its host id), with each page's title.
    Scan { host: String },
}

/// What the backend tells the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// The list kept from last time; the ones on are starting.
    Loaded(SavedTunnels),
    /// A tunnel's state; `None` when it stopped.
    State { id: String, state: Option<State> },
    /// What a look at the server `host` found, or why it couldn't look.
    Scanned { host: String, result: Result<Scan, String> },
}

/// Something listening on TCP on the server (sshee-core's `WebApp`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebApp {
    pub port: u16,
    /// Where a tunnel to it connects, as the server sees it: 127.0.0.1 unless it listens on
    /// one other address only.
    pub remote_host: String,
    /// The program listening ("node", "nginx"), when the server says.
    pub process: Option<String>,
    /// What the port or program is known as ("PostgreSQL", "Docker").
    pub known_as: Option<String>,
    /// The page's `<title>`.
    pub title: Option<String>,
    /// "http" or "https", when it answered like a website.
    pub scheme: Option<String>,
    /// The HTTP status of its front page.
    pub status: Option<u16>,
    /// Likely a website.
    pub web: bool,
    /// Also listens on an address other computers can reach.
    pub public: bool,
}

/// What a look at a server found (sshee-core's `WebAppScan`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Likely websites first, then by port.
    pub apps: Vec<WebApp>,
    /// curl is on the server, so pages could be asked for their titles.
    pub curl: bool,
}

impl Scan {
    /// The websites.
    pub fn web(&self) -> impl Iterator<Item = &WebApp> {
        self.apps.iter().filter(|a| a.web)
    }

    /// Other programs that listen.
    pub fn other(&self) -> impl Iterator<Item = &WebApp> {
        self.apps.iter().filter(|a| !a.web)
    }
}

/// A look at a server: on its way, and the last one's answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Looking {
    pub loading: bool,
    pub result: Option<Result<Scan, String>>,
}

/// A tunnel "Open" or "Connect" made for a web app: not saved, gone when SSHeesh closes
/// unless it's kept (0.10's `TempTunnel`).
#[derive(Clone, Debug, PartialEq)]
pub struct Temp {
    pub id: String,
    pub forward: Forward,
    pub scheme: Option<String>,
    /// The app's title or program: the name it's kept by.
    pub label: Option<String>,
}

/// A tunnel that leads to a web app: a temporary one or a saved one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct To {
    pub id: String,
    /// The port here.
    pub port: u16,
    pub saved: bool,
}

/// The tunnels the window knows: the saved ones, the temporary ones, how each is doing, and
/// the last look at each server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tunnels {
    pub saved: SavedTunnels,
    pub states: HashMap<String, State>,
    /// Each server's temporary tunnels, by host id.
    pub temp: IndexMap<String, Vec<Temp>>,
    /// Each server's last look, by host id.
    pub scans: HashMap<String, Looking>,
    /// Tunnels to open in the browser once they're on: the address, by tunnel id.
    pub opening: HashMap<String, String>,
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

    /// An id no saved or temporary tunnel has.
    pub fn new_id(&self) -> String {
        let saved = self.saved.by_host.values().flatten().map(|t| t.id.as_str());
        let taken: Vec<&str> = saved.chain(self.temp.values().flatten().map(|t| t.id.as_str())).collect();
        (1..).map(|n| format!("tun-{n}")).find(|id| !taken.contains(&id.as_str())).unwrap_or_default()
    }

    /// Why `forward` can't be saved as tunnel `id` (`None` for a new one): another saved
    /// tunnel listens on the same port here.
    pub fn clash(&self, id: Option<&str>, forward: &Forward) -> Option<String> {
        let port = listens_here(forward)?;
        let other = self.saved.by_host.values().flatten().any(|t| Some(t.id.as_str()) != id && listens_here(&t.forward) == Some(port));
        other.then(|| format!("Another saved tunnel already uses port {port} on this computer."))
    }

    /// The port here to check is free before `forward` is saved as tunnel `id` (`None` for a
    /// new one): none for a reverse tunnel, or when that tunnel is on and holds it already.
    pub fn port_to_check(&self, host: &str, id: Option<&str>, forward: &Forward) -> Option<u16> {
        let port = listens_here(forward)?;
        let saved = id.and_then(|id| self.of(host).iter().find(|t| t.id == id));
        let holds = saved.is_some_and(|t| t.on && listens_here(&t.forward) == Some(port) && is_active(self.states.get(&t.id)));
        (!holds).then_some(port)
    }

    /// Ports here other tunnels have claimed, on or off, so a new one picks another.
    pub fn claimed(&self) -> Vec<u16> {
        let saved = self.saved.by_host.values().flatten().filter_map(|t| listens_here(&t.forward));
        saved.chain(self.temp.values().flatten().filter_map(|t| listens_here(&t.forward))).collect()
    }

    /// A server's temporary tunnels.
    pub fn temp_of(&self, host: &str) -> &[Temp] {
        self.temp.get(host).map_or(&[], Vec::as_slice)
    }

    /// A look at what listens on `host`: what to ask the backend.
    pub fn scan(&mut self, host: &str) -> Ask {
        self.scans.entry(host.to_owned()).or_default().loading = true;
        Ask::Scan { host: host.to_owned() }
    }

    /// A tunnel of this server's that already leads to `app`, preferring one that's on.
    pub fn tunnel_to(&self, host: &str, app: &WebApp) -> Option<To> {
        let leads = |f: &Forward| f.kind == ForwardKind::Local && f.remote_port == Some(app.port) && remote_host(f) == app.remote_host;
        let temp = self.temp_of(host).iter().filter(|t| leads(&t.forward)).map(|t| To { id: t.id.clone(), port: t.forward.local_port, saved: false });
        let saved = self.of(host).iter().filter(|t| leads(&t.forward)).map(|t| To { id: t.id.clone(), port: t.forward.local_port, saved: true });
        let all: Vec<To> = temp.chain(saved).collect();
        all.iter().find(|t| is_active(self.states.get(&t.id))).or(all.first()).cloned()
    }

    /// "Open" (`browse`) or "Connect" on a web app: through a tunnel already leading there,
    /// else a new temporary one on a free port here (`port` asks the backend for one, given
    /// the ports other tunnels claim). What the backend is to do, and the address to open now
    /// when the tunnel is on already; one that is starting opens once it's on
    /// ([`Tunnels::deliver`]).
    pub fn open_app(&mut self, host: &str, app: &WebApp, browse: bool, port: impl FnOnce(&[u16]) -> Result<u16, String>) -> Result<(Vec<Ask>, Option<String>), String> {
        let scheme = app.scheme.clone();
        if let Some(to) = self.tunnel_to(host, app) {
            let url = app_url(scheme.as_deref(), to.port);
            if matches!(self.states.get(&to.id), Some(State::On { .. })) {
                return Ok((vec![], browse.then_some(url)));
            }
            if browse {
                self.opening.insert(to.id.clone(), url);
            }
            if is_active(self.states.get(&to.id)) {
                return Ok((vec![], None));
            }
            if to.saved {
                return Ok((self.set_on(host, &to.id, true), None));
            }
            let forward = self.temp_of(host).iter().find(|t| t.id == to.id).map(|t| t.forward.clone()).unwrap_or_default();
            self.states.insert(to.id.clone(), State::Starting);
            return Ok((vec![Ask::Start { id: to.id, host: host.to_owned(), forward }], None));
        }
        let port = port(&self.claimed())?;
        let id = self.new_id();
        let forward = local(port, &app.remote_host, app.port);
        let label = app.title.clone().or_else(|| app.known_as.clone()).or_else(|| app.process.clone());
        self.temp.entry(host.to_owned()).or_default().push(Temp { id: id.clone(), forward: forward.clone(), scheme: scheme.clone(), label });
        self.states.insert(id.clone(), State::Starting);
        if browse {
            self.opening.insert(id.clone(), app_url(scheme.as_deref(), port));
        }
        Ok((vec![Ask::Start { id, host: host.to_owned(), forward }], None))
    }

    /// "Stop" on a web app's tunnel: a temporary one stops and is forgotten, a saved one is
    /// switched off.
    pub fn stop_app(&mut self, host: &str, id: &str) -> Vec<Ask> {
        self.opening.remove(id);
        if self.of(host).iter().any(|t| t.id == id) {
            return self.set_on(host, id, false);
        }
        if let Some(list) = self.temp.get_mut(host) {
            list.retain(|t| t.id != id);
            if list.is_empty() {
                self.temp.shift_remove(host);
            }
        }
        self.states.remove(id);
        vec![Ask::Stop { id: id.to_owned() }]
    }

    /// "Keep": a temporary tunnel becomes a saved one, on, without starting again.
    pub fn keep(&mut self, host: &str, id: &str) -> Vec<Ask> {
        let Some(list) = self.temp.get_mut(host) else { return vec![] };
        let Some(at) = list.iter().position(|t| t.id == id) else { return vec![] };
        let t = list.remove(at);
        if list.is_empty() {
            self.temp.shift_remove(host);
        }
        let scheme = t.scheme.as_deref().map(Scheme::from);
        self.saved.by_host.entry(host.to_owned()).or_default().push(SavedTunnel { id: t.id, name: t.label, forward: t.forward, on: true, scheme, ..SavedTunnel::default() });
        vec![Ask::Save(self.saved.clone())]
    }

    /// Temporary tunnels to apps the last look didn't find (the app stopped, say), as apps,
    /// so they can still be stopped or kept.
    pub fn orphans(&self, host: &str) -> Vec<WebApp> {
        let Some(Ok(scan)) = self.scans.get(host).and_then(|l| l.result.as_ref()) else { return vec![] };
        self.temp_of(host)
            .iter()
            .filter(|t| !scan.apps.iter().any(|a| Some(a.port) == t.forward.remote_port && a.remote_host == remote_host(&t.forward)))
            .map(|t| WebApp { port: t.forward.remote_port.unwrap_or_default(), remote_host: remote_host(&t.forward).to_owned(), title: t.label.clone(), scheme: t.scheme.clone(), web: true, ..WebApp::default() })
            .collect()
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
            self.opening.remove(id);
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
        self.opening.remove(id);
        vec![Ask::Stop { id: id.to_owned() }, Ask::Save(self.saved.clone())]
    }

    /// A server left the list: its tunnels stop and are forgotten (0.10's
    /// `forgetHostTunnels`). Nothing to do when it had none.
    pub fn forget(&mut self, host: &str) -> Vec<Ask> {
        let Some(list) = self.saved.by_host.shift_remove(host) else { return vec![] };
        let mut asks: Vec<Ask> = list
            .iter()
            .map(|t| {
                self.states.remove(&t.id);
                Ask::Stop { id: t.id.clone() }
            })
            .collect();
        asks.push(Ask::Save(self.saved.clone()));
        asks
    }

    /// The backend's news. The address to open in the browser now, when a tunnel waiting to
    /// be opened came on.
    pub fn deliver(&mut self, reply: Reply) -> Option<String> {
        match reply {
            Reply::Loaded(saved) => {
                for t in saved.by_host.values().flatten().filter(|t| t.on) {
                    self.states.entry(t.id.clone()).or_insert(State::Starting);
                }
                self.saved = saved;
            }
            // A tunnel switched off or stopped meanwhile keeps quiet: its late news isn't shown.
            Reply::State { id, state: Some(state) } => {
                let saved_on = self.saved.by_host.values().flatten().any(|t| t.id == id && t.on);
                let temp = self.temp.values().flatten().any(|t| t.id == id);
                if saved_on || temp {
                    let (on, failed) = (matches!(state, State::On { .. }), matches!(state, State::Failed { .. }));
                    self.states.insert(id.clone(), state);
                    if on {
                        return self.opening.remove(&id);
                    }
                    if failed {
                        self.opening.remove(&id);
                    }
                }
            }
            Reply::State { id, state: None } => {
                self.states.remove(&id);
                self.opening.remove(&id);
            }
            Reply::Scanned { host, result } => {
                let look = self.scans.entry(host).or_default();
                look.loading = false;
                look.result = Some(result);
            }
        }
        None
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

/// The address a web app opens at here: `http://localhost:3000/`.
pub fn app_url(scheme: Option<&str>, port: u16) -> String {
    format!("{}://localhost:{port}/", scheme.unwrap_or("http"))
}

/// A web app's name: its page's title, else what it's known as, its program, its port.
pub fn app_title(app: &WebApp) -> String {
    app.title.clone().or_else(|| app.known_as.clone()).or_else(|| app.process.clone()).unwrap_or_else(|| format!("Port {}", app.port))
}

/// The line under a web app's name: "port 3000 · node · answers 404 · also open to other
/// computers".
pub fn app_facts(app: &WebApp) -> String {
    let program = if app.title.is_some() { app.known_as.clone().or_else(|| app.process.clone()) } else { app.known_as.as_ref().and(app.process.clone()) };
    let mut facts = vec![format!("port {}", app.port)];
    facts.extend(program);
    if app.remote_host != SERVER {
        facts.push(format!("at {}", app.remote_host));
    }
    if let Some(code) = app.status.filter(|c| *c >= 400) {
        facts.push(format!("answers {code}"));
    }
    if app.public {
        facts.push("also open to other computers".into());
    }
    facts.join(" · ")
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
    fn a_server_taken_out_of_the_list_takes_its_tunnels_with_it() {
        let mut t = Tunnels::default();
        t.save("demo-web", None, "", local(3000, SERVER, 3000));
        t.save("demo-web", None, "", socks(1080));
        t.save("demo-db", None, "", local(5432, SERVER, 5432));
        let asks = t.forget("demo-web");
        assert_eq!(asks[..2], [Ask::Stop { id: "tun-1".into() }, Ask::Stop { id: "tun-2".into() }]);
        assert!(matches!(&asks[2], Ask::Save(list) if !list.by_host.contains_key("demo-web") && list.by_host.contains_key("demo-db")));
        assert!(t.of("demo-web").is_empty() && !t.states.contains_key("tun-1"));
        assert_eq!(t.active("demo-db"), 1);
        // A server without tunnels: nothing to ask.
        assert!(t.forget("demo-web").is_empty());
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

    fn grafana() -> WebApp {
        WebApp { port: 3000, remote_host: SERVER.into(), process: Some("grafana".into()), title: Some("Grafana".into()), scheme: Some("http".into()), web: true, ..WebApp::default() }
    }

    #[test]
    fn a_web_app_opens_through_a_temporary_tunnel_once_it_is_on() {
        let mut t = Tunnels::default();
        t.save("demo-web", None, "", local(3000, SERVER, 3000));
        let ask = t.scan("demo-web");
        assert_eq!(ask, Ask::Scan { host: "demo-web".into() });
        assert!(t.scans["demo-web"].loading);
        t.deliver(Reply::Scanned { host: "demo-web".into(), result: Ok(Scan { apps: vec![grafana(), WebApp { port: 8080, remote_host: SERVER.into(), web: true, ..WebApp::default() }], curl: true }) });
        assert!(!t.scans["demo-web"].loading);
        // Port 8080 has no tunnel: a temporary one, on the port the backend picked, avoiding
        // the ports saved tunnels claim.
        let app = WebApp { port: 8080, remote_host: SERVER.into(), web: true, ..WebApp::default() };
        let (asks, now) = t
            .open_app("demo-web", &app, true, |claimed| {
                assert_eq!(claimed, [3000]);
                Ok(8080)
            })
            .unwrap();
        assert_eq!(now, None, "it opens once it's on");
        assert!(matches!(&asks[..], [Ask::Start { id, forward, .. }] if id == "tun-2" && forward.local_port == 8080 && forward.remote_port == Some(8080)));
        assert_eq!(t.temp_of("demo-web").len(), 1);
        assert_eq!(t.tunnel_to("demo-web", &app).map(|to| (to.port, to.saved)), Some((8080, false)));
        assert_eq!(t.deliver(Reply::State { id: "tun-2".into(), state: Some(State::On { warning: None }) }), Some("http://localhost:8080/".into()));
        // Open again: at once, through the same tunnel.
        let (asks, now) = t.open_app("demo-web", &app, true, |_| panic!("no new port")).unwrap();
        assert!(asks.is_empty());
        assert_eq!(now.as_deref(), Some("http://localhost:8080/"));
        // Grafana goes through the saved tunnel, switched on when it's off.
        t.set_on("demo-web", "tun-1", false);
        let (asks, _) = t.open_app("demo-web", &grafana(), true, |_| panic!("no new port")).unwrap();
        assert!(matches!(&asks[0], Ask::Start { id, .. } if id == "tun-1"));
        assert!(t.of("demo-web")[0].on);
        // A failed start opens nothing.
        assert_eq!(t.deliver(Reply::State { id: "tun-1".into(), state: Some(State::Failed { message: "Port 3000 on this computer is already in use.".into() }) }), None);
        assert!(t.opening.is_empty());
    }

    #[test]
    fn a_temporary_tunnel_is_kept_or_stopped() {
        let mut t = Tunnels::default();
        t.open_app("demo-web", &grafana(), false, |_| Ok(13000)).unwrap();
        assert_eq!(t.claimed(), [13000]);
        let id = t.temp_of("demo-web")[0].id.clone();
        // Kept: saved, on, named after the app, with its scheme; nothing starts again.
        let asks = t.keep("demo-web", &id);
        assert!(matches!(&asks[..], [Ask::Save(list)] if list.by_host["demo-web"].len() == 1));
        let kept = &t.of("demo-web")[0];
        assert_eq!((kept.name.as_deref(), kept.on, kept.scheme.as_ref().map(|s| s.as_str())), (Some("Grafana"), true, Some("http")));
        assert!(t.temp_of("demo-web").is_empty());
        // Another, stopped: forgotten.
        t.open_app("demo-db", &WebApp { port: 5432, remote_host: SERVER.into(), ..WebApp::default() }, false, |_| Ok(15432)).unwrap();
        let id = t.temp_of("demo-db")[0].id.clone();
        assert_eq!(t.stop_app("demo-db", &id), vec![Ask::Stop { id: id.clone() }]);
        assert!(t.temp_of("demo-db").is_empty() && !t.states.contains_key(&id));
        // A saved one is switched off instead.
        let kept = t.of("demo-web")[0].id.clone();
        assert!(matches!(&t.stop_app("demo-web", &kept)[0], Ask::Stop { .. }));
        assert!(!t.of("demo-web")[0].on);
    }

    #[test]
    fn a_temporary_tunnel_whose_app_went_away_still_shows() {
        let mut t = Tunnels::default();
        t.open_app("demo-web", &grafana(), false, |_| Ok(3000)).unwrap();
        t.deliver(Reply::Scanned { host: "demo-web".into(), result: Ok(Scan { apps: vec![], curl: true }) });
        let orphans = t.orphans("demo-web");
        assert_eq!((orphans[0].port, orphans[0].title.as_deref(), orphans[0].web), (3000, Some("Grafana"), true));
    }

    #[test]
    fn a_port_here_is_checked_unless_its_tunnel_holds_it() {
        let mut t = Tunnels::default();
        assert_eq!(t.port_to_check("demo-web", None, &local(3000, SERVER, 3000)), Some(3000));
        assert_eq!(t.port_to_check("demo-web", None, &reverse(9000, 3000)), None, "a reverse tunnel listens on the server");
        t.save("demo-web", None, "", local(3000, SERVER, 3000));
        assert_eq!(t.port_to_check("demo-web", Some("tun-1"), &local(3000, SERVER, 3000)), None, "it's on and holds its port");
        assert_eq!(t.port_to_check("demo-web", Some("tun-1"), &local(3001, SERVER, 3000)), Some(3001));
        t.set_on("demo-web", "tun-1", false);
        assert_eq!(t.port_to_check("demo-web", Some("tun-1"), &local(3000, SERVER, 3000)), Some(3000));
    }

    #[test]
    fn web_apps_read_in_plain_words() {
        assert_eq!(app_title(&grafana()), "Grafana");
        assert_eq!(app_facts(&grafana()), "port 3000 · grafana");
        let bare = WebApp { port: 9000, remote_host: "10.0.0.5".into(), status: Some(404), public: true, ..WebApp::default() };
        assert_eq!(app_title(&bare), "Port 9000");
        assert_eq!(app_facts(&bare), "port 9000 · at 10.0.0.5 · answers 404 · also open to other computers");
        let db = WebApp { port: 5432, remote_host: SERVER.into(), process: Some("postgres".into()), known_as: Some("PostgreSQL".into()), ..WebApp::default() };
        assert_eq!((app_title(&db), app_facts(&db)), ("PostgreSQL".into(), "port 5432 · postgres".into()));
        assert_eq!(app_url(Some("https"), 8443), "https://localhost:8443/");
        assert_eq!(app_url(None, 80), "http://localhost:80/");
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
