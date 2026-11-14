//! The workbench: the machines in the sidebar, every open tab in one row, and the tab you're
//! on. What the 1.0 window shows and does, without a screen.
//!
//! A port of the rules in 0.10's `src/lib/store.ts` and `src/lib/tabs.ts` for tabs along the
//! top (one row for every machine's tabs):
//!
//! - a new tab opens right after the tab you were last on at that machine; a machine's first
//!   tab goes to the end of the row (`openIn`);
//! - "Terminal", "Terminal 2", ...: a tab takes the lowest number free among its machine's
//!   tabs of its kind and keeps it (`freeNumber`, `tabLabel`);
//! - closing the tab you're on brings up the next tab of that machine, else the one before
//!   it, else the neighbour in the row, else the main menu (`closePane`);
//! - going to a machine brings up the tab you were last on there, or opens a terminal
//!   (`goTo`); the last 20 closed tabs can be brought back, newest first (`reopenClosed`);
//! - landing on a tab marks what it asked you for as seen (`focusing`).
//!
//! Tabs are saved and brought back ([`Workbench::saved`], [`Workbench::restore`]) with their
//! session keys, so a terminal on a server finds its shell in tmux again after a restart, as
//! in 0.10. Not here yet: tabs side by side (0.10's layouts and `hidden`). The window owns one
//! `Workbench`; every change goes through its methods, so the rules are tested here, without a
//! window.

pub mod shortcuts;
pub mod vitals;

use crate::model::{EnvLabel, PaneKind, TargetRef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use vitals::{Reading, Vital, Vitals};

/// How many closed tabs are kept to bring back (0.10's `CLOSED_KEPT`).
pub const CLOSED_KEPT: usize = 20;

/// A machine's key, as 0.10 files its things: `local` or `host:<host id>` ([`TargetRef::key`]).
pub type ServerKey = String;

/// A machine in the sidebar: this computer, or a server from the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    pub target: TargetRef,
    /// As the list names it.
    pub name: String,
    /// `user@host` (and `:port` when it isn't 22), for the card under its icon. Empty for this
    /// computer.
    pub address: String,
    /// Production, Staging or Development, if labelled.
    pub env: Option<EnvLabel>,
    /// Its icon's id (`globe`, `database`, ...; `laptop` for this computer).
    pub icon: String,
}

impl Server {
    /// This computer, always first in the sidebar.
    pub fn this_computer() -> Server {
        Server { target: TargetRef::local(), name: "This computer".into(), address: String::new(), env: None, icon: "laptop".into() }
    }

    /// A server from the list, by its host id.
    pub fn host(host_id: &str, name: &str, address: &str) -> Server {
        Server { target: TargetRef::host(host_id), name: name.into(), address: address.into(), env: None, icon: "server".into() }
    }

    pub fn env(mut self, env: EnvLabel) -> Server {
        self.env = Some(env);
        self
    }

    pub fn icon(mut self, icon: &str) -> Server {
        self.icon = icon.into();
        self
    }

    pub fn key(&self) -> ServerKey {
        self.target.key()
    }

    pub fn is_local(&self) -> bool {
        self.key() == "local"
    }

    /// Labelled Production: the red "Prod" beside its name.
    pub fn is_prod(&self) -> bool {
        self.env == Some(EnvLabel::Prod)
    }
}

/// What a tab holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabKind {
    Terminal,
    Files,
    Stats,
}

impl TabKind {
    /// "Terminal", "Files", "Stats": what a tab is called until something names it.
    pub fn label(self) -> &'static str {
        match self {
            TabKind::Terminal => "Terminal",
            TabKind::Files => "Files",
            TabKind::Stats => "Stats",
        }
    }

    /// 0.10's kind for it.
    pub fn pane_kind(self) -> PaneKind {
        match self {
            TabKind::Terminal => PaneKind::Terminal,
            TabKind::Files => PaneKind::Files,
            TabKind::Stats => PaneKind::Stats,
        }
    }

    /// From 0.10's kind; its empty start page isn't a tab, nor is a kind this version doesn't
    /// know.
    pub fn from_pane_kind(kind: &PaneKind) -> Option<TabKind> {
        match kind {
            PaneKind::Terminal => Some(TabKind::Terminal),
            PaneKind::Files => Some(TabKind::Files),
            PaneKind::Stats => Some(TabKind::Stats),
            PaneKind::Empty | PaneKind::Other(_) => None,
        }
    }
}

/// A tab's id: a number that is never used twice in one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

/// Where a tab's connection is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Link {
    /// Nothing started yet (the tab hasn't been shown).
    #[default]
    Idle,
    Connecting,
    /// `durable`: the shell lives in tmux on the server.
    Connected { durable: bool },
    /// The connection dropped and is being brought back.
    Reconnecting,
    /// It didn't come back, or it sleeps: a click or a key tries again.
    Disconnected,
    /// The shell ended, or never started.
    Ended,
}

/// What a tab says after its name, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    /// An AI agent or another program waits for an answer.
    NeedsYou,
    Reconnecting,
    Disconnected,
    Ended,
}

impl Badge {
    pub fn label(self) -> &'static str {
        match self {
            Badge::NeedsYou => "Needs you",
            Badge::Reconnecting => "Reconnecting…",
            Badge::Disconnected => "Disconnected",
            Badge::Ended => "Ended",
        }
    }
}

/// One tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: TabId,
    /// The machine it's on.
    pub server: ServerKey,
    pub kind: TabKind,
    /// "Terminal 2": its number among its machine's tabs of its kind when it opened. It never
    /// changes.
    pub n: u32,
    /// A name of its own (a folder, what an agent is doing); else its kind and number.
    pub title: Option<String>,
    /// A terminal's: names the tmux session on the server, so reconnecting finds it again.
    pub session_key: Option<String>,
    /// Files: the folder it shows.
    pub path: Option<String>,
    pub link: Link,
    /// Something in it waits for you.
    pub needs_you: bool,
    /// The AI agent running in it ("Claude Code"), when one is.
    pub agent: Option<String>,
}

impl Tab {
    /// Its name in the row: its own title, else "Terminal", "Terminal 2", "Files".
    pub fn label(&self) -> String {
        match &self.title {
            Some(title) => title.clone(),
            None if self.n > 1 => format!("{} {}", self.kind.label(), self.n),
            None => self.kind.label().to_owned(),
        }
    }

    /// What it says after its name. Waiting for you comes first: it's what you act on.
    pub fn badge(&self) -> Option<Badge> {
        if self.needs_you {
            return Some(Badge::NeedsYou);
        }
        match self.link {
            Link::Reconnecting => Some(Badge::Reconnecting),
            Link::Disconnected => Some(Badge::Disconnected),
            Link::Ended => Some(Badge::Ended),
            Link::Idle | Link::Connecting | Link::Connected { .. } => None,
        }
    }
}

/// A tab that was closed, to bring back.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedTab {
    pub server: ServerKey,
    pub kind: TabKind,
    pub title: Option<String>,
    pub path: Option<String>,
}

/// What a new tab starts with, beyond its kind and machine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Open {
    pub title: Option<String>,
    pub path: Option<String>,
    /// The agent running in it, when it's known from the start.
    pub agent: Option<String>,
}

/// The tabs as they're kept between runs: the row in order, the tab you were on, and whether
/// the sidebar showed. A file of 1.0's own; 0.10's workspace store isn't written.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedTabs {
    pub tabs: Vec<SavedTab>,
    /// The place in `tabs` of the tab on screen; `None` on the main menu.
    pub active: Option<usize>,
    /// The sidebar was hidden.
    pub sidebar_hidden: bool,
}

/// One saved tab.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedTab {
    pub server: ServerKey,
    /// 0.10's word for the kind: "terminal", "files", "stats".
    pub kind: PaneKind,
    pub n: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Tor, as the sidebar and the status bar show it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tor {
    #[default]
    Off,
    Starting,
    /// `latency_ms`: how long a round trip through it takes, once measured.
    On { latency_ms: Option<u32> },
    /// It didn't start: nothing connects until it does.
    Failed,
}

/// A tab as the row draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct StripTab<'a> {
    pub tab: &'a Tab,
    /// Its machine's name ("Unknown server" for one no longer in the list).
    pub server_name: &'a str,
    /// The tab you're on.
    pub active: bool,
    /// The tab before it is another machine's: a divider goes between them.
    pub starts_group: bool,
    /// The red "Prod" goes on this tab: one label per machine, on its first tab in the row,
    /// unless it's the machine on screen, whose label is in the window's title.
    pub prod: bool,
}

/// The servers, the tabs and where you are.
#[derive(Debug, Clone, PartialEq)]
pub struct Workbench {
    servers: Vec<Server>,
    /// Every tab, in the row's order.
    tabs: Vec<Tab>,
    /// The tab you're on; on the main menu, the one you'd come back to.
    focused: Option<TabId>,
    /// The main menu is showing.
    home: bool,
    /// The tab you were last on at each machine.
    last_on: HashMap<ServerKey, TabId>,
    /// Newest first.
    closed: Vec<ClosedTab>,
    next_id: u64,
    sidebar: bool,
    tor: Tor,
    vitals: HashMap<ServerKey, Vitals>,
}

const UNKNOWN_SERVER: &str = "Unknown server";

impl Workbench {
    /// A workbench with these machines and no tabs, on the main menu. This computer is always
    /// in the list, first unless `servers` places it.
    pub fn new(servers: Vec<Server>) -> Workbench {
        let mut w = Workbench {
            servers: vec![],
            tabs: vec![],
            focused: None,
            home: true,
            last_on: HashMap::new(),
            closed: vec![],
            next_id: 1,
            sidebar: true,
            tor: Tor::Off,
            vitals: HashMap::new(),
        };
        w.set_servers(servers);
        w
    }

    /// The list changed (a server added, renamed, labelled). Tabs of a machine that left the
    /// list stay open under "Unknown server": their shells still run.
    pub fn set_servers(&mut self, mut servers: Vec<Server>) {
        if !servers.iter().any(Server::is_local) {
            servers.insert(0, Server::this_computer());
        }
        // One entry per machine: a second with the same key could never be told apart.
        let mut seen = std::collections::HashSet::new();
        servers.retain(|s| seen.insert(s.key()));
        self.servers = servers;
    }

    pub fn servers(&self) -> &[Server] {
        &self.servers
    }

    pub fn server(&self, key: &str) -> Option<&Server> {
        self.servers.iter().find(|s| s.key() == key)
    }

    /// Every tab, in the row's order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// The main menu is showing: asked for, or no tab is open.
    pub fn showing_home(&self) -> bool {
        self.home || self.focused.is_none()
    }

    /// The tab on screen, or None on the main menu. Shortcuts act on this.
    pub fn active(&self) -> Option<&Tab> {
        if self.showing_home() {
            return None;
        }
        self.focused.and_then(|id| self.tab(id))
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active().map(|t| t.id)
    }

    /// The machine on screen, or None on the main menu.
    pub fn current_server(&self) -> Option<&Server> {
        self.active().and_then(|t| self.server(&t.server))
    }

    /// The key of the machine on screen (also one no longer in the list).
    pub fn current_key(&self) -> Option<&str> {
        self.active().map(|t| t.server.as_str())
    }

    /// The window's title: the machine on screen and whether it's Production; "SSHeesh" on the
    /// main menu.
    pub fn window_title(&self) -> (&str, bool) {
        match self.active() {
            Some(tab) => self.server(&tab.server).map_or((UNKNOWN_SERVER, false), |s| (s.name.as_str(), s.is_prod())),
            None => ("SSHeesh", false),
        }
    }

    /// The row of tabs as it's drawn.
    pub fn strip(&self) -> Vec<StripTab<'_>> {
        let active = self.active_id();
        let current = self.current_key();
        let mut labelled: Vec<&str> = Vec::new();
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let server = self.server(&tab.server);
                let first_of_its_machine = !labelled.contains(&tab.server.as_str());
                if first_of_its_machine {
                    labelled.push(&tab.server);
                }
                StripTab {
                    tab,
                    server_name: server.map_or(UNKNOWN_SERVER, |s| s.name.as_str()),
                    active: active == Some(tab.id),
                    starts_group: i > 0 && self.tabs[i - 1].server != tab.server,
                    prod: first_of_its_machine && current != Some(tab.server.as_str()) && server.is_some_and(Server::is_prod),
                }
            })
            .collect()
    }

    /// A machine's tabs, in the row's order.
    pub fn tabs_on<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Tab> + 'a {
        self.tabs.iter().filter(move |t| t.server == key)
    }

    /// Something on this machine waits for you: the orange dot on its icon.
    pub fn server_needs_you(&self, key: &str) -> bool {
        self.tabs_on(key).any(|t| t.needs_you)
    }

    /// The lowest number no other tab of this kind on this machine has.
    fn free_number(&self, kind: TabKind, server: &str) -> u32 {
        let mut n = 1;
        while self.tabs.iter().any(|t| t.kind == kind && t.server == server && t.n == n) {
            n += 1;
        }
        n
    }

    /// The tab a new one opens next to, and the one going to a machine lands on: the tab you
    /// were last on there, else the machine's first.
    fn anchor_on(&self, key: &str) -> Option<TabId> {
        let last = self.last_on.get(key).copied().filter(|id| self.tab(*id).is_some_and(|t| t.server == key));
        last.or_else(|| self.tabs_on(key).next().map(|t| t.id))
    }

    /// Opens a tab on a machine and lands on it.
    pub fn open(&mut self, kind: TabKind, server: &str) -> TabId {
        self.open_with(kind, server, Open::default())
    }

    /// [`Self::open`] with a title, a folder or an agent from the start.
    pub fn open_with(&mut self, kind: TabKind, server: &str, open: Open) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        let tab = Tab {
            id,
            server: server.to_owned(),
            kind,
            n: self.free_number(kind, server),
            title: open.title,
            session_key: (kind == TabKind::Terminal).then(new_session_key),
            path: open.path,
            link: Link::Idle,
            needs_you: false,
            agent: open.agent,
        };
        // Next to the tab you were on there; a machine's first tab ends the row.
        let at = self.anchor_on(server).and_then(|a| self.index_of(a)).map_or(self.tabs.len(), |i| i + 1);
        self.tabs.insert(at, tab);
        self.land_on(id);
        id
    }

    /// A new terminal on the machine on screen; on the main menu, on the machine you were on
    /// last, else on this computer.
    pub fn new_terminal(&mut self) -> TabId {
        let server = self.focused.and_then(|id| self.tab(id)).map_or_else(|| "local".to_owned(), |t| t.server.clone());
        self.open(TabKind::Terminal, &server)
    }

    /// Goes to a machine: the tab you were last on there, or a new terminal.
    pub fn go_to(&mut self, server: &str) -> TabId {
        match self.anchor_on(server) {
            Some(id) => {
                self.land_on(id);
                id
            }
            None => self.open(TabKind::Terminal, server),
        }
    }

    /// Goes to a tab. False when there's no such tab.
    pub fn focus(&mut self, id: TabId) -> bool {
        if self.tab(id).is_none() {
            return false;
        }
        self.land_on(id);
        true
    }

    /// What every way of landing on a tab shares: it's on screen, it's the one you'd come back
    /// to on its machine, and what it asked you for is seen.
    fn land_on(&mut self, id: TabId) {
        let Some(tab) = self.tab_mut(id) else { return };
        tab.needs_you = false;
        let server = tab.server.clone();
        self.last_on.insert(server, id);
        self.focused = Some(id);
        self.home = false;
    }

    /// Shows the main menu; the tab you were on is the one you'd come back to.
    pub fn show_home(&mut self) {
        self.home = true;
    }

    /// Closes a tab and gives it back (the window ends its session). Closing the one you're on
    /// brings up the next, as in a browser: the tab after it on its machine, else the one
    /// before; a machine's last tab goes to its neighbour in the row; the last tab of all, to
    /// the main menu.
    pub fn close(&mut self, id: TabId) -> Option<Tab> {
        let at = self.index_of(id)?;
        let next = if self.focused == Some(id) {
            let same = |t: &&Tab| t.server == self.tabs[at].server;
            let after = self.tabs[at + 1..].iter().find(same);
            let before = self.tabs[..at].iter().rev().find(same);
            after.or(before).or(self.tabs.get(at + 1)).or(at.checked_sub(1).and_then(|i| self.tabs.get(i))).map(|t| t.id)
        } else {
            self.focused
        };
        let tab = self.tabs.remove(at);
        self.last_on.retain(|_, last| *last != id);
        self.closed.insert(0, ClosedTab { server: tab.server.clone(), kind: tab.kind, title: tab.title.clone(), path: tab.path.clone() });
        self.closed.truncate(CLOSED_KEPT);
        if self.focused == Some(id) {
            // On the main menu the menu stays; only the tab to come back to changes.
            let was_home = self.home;
            match next {
                Some(next) => self.land_on(next),
                None => self.focused = None,
            }
            self.home = was_home || next.is_none();
        }
        Some(tab)
    }

    /// Closes the tab on screen, if one is.
    pub fn close_active(&mut self) -> Option<Tab> {
        self.close(self.active_id()?)
    }

    /// The tabs closed lately, newest first.
    pub fn closed(&self) -> &[ClosedTab] {
        &self.closed
    }

    /// Brings back the tab closed last, as a new tab of the same kind on the same machine. One
    /// whose machine left the list is skipped.
    pub fn reopen_closed(&mut self) -> Option<TabId> {
        while !self.closed.is_empty() {
            let c = self.closed.remove(0);
            if self.server(&c.server).is_some() {
                return Some(self.open_with(c.kind, &c.server, Open { title: c.title, path: c.path, agent: None }));
            }
        }
        None
    }

    /// Ctrl+Tab and Ctrl+Shift+Tab: the next (`1`) or previous (`-1`) tab in the row, around
    /// its ends, every machine's. On the main menu it comes back to the tab you were on.
    pub fn step(&mut self, step: i32) -> Option<TabId> {
        let Some(at) = self.focused.and_then(|id| self.index_of(id)) else {
            let first = self.tabs.first()?.id;
            self.land_on(first);
            return Some(first);
        };
        let n = self.tabs.len() as i32;
        let to = if self.showing_home() { at } else { (at as i32 + step).rem_euclid(n) as usize };
        let id = self.tabs[to].id;
        self.land_on(id);
        Some(id)
    }

    /// Jumps to the `n`th tab in the row (1 to 8); 9 is the last one, as in a browser.
    pub fn jump(&mut self, n: usize) -> Option<TabId> {
        let tab = match n {
            1..=8 => self.tabs.get(n - 1),
            9 => self.tabs.last(),
            _ => None,
        }?;
        let id = tab.id;
        self.land_on(id);
        Some(id)
    }

    /// Moves a tab to place `to` in the row (0 is first; past the end is last). False when
    /// there's no such tab.
    pub fn move_tab(&mut self, id: TabId, to: usize) -> bool {
        let Some(from) = self.index_of(id) else { return false };
        let tab = self.tabs.remove(from);
        self.tabs.insert(to.min(self.tabs.len()), tab);
        true
    }

    /// A tab's connection changed.
    pub fn set_link(&mut self, id: TabId, link: Link) {
        if let Some(tab) = self.tab_mut(id) {
            tab.link = link;
        }
    }

    /// Something in a tab waits for you, or no longer does. A tab you're looking at never
    /// says so: you can see it.
    pub fn set_needs_you(&mut self, id: TabId, needs_you: bool) {
        let on_screen = self.active_id() == Some(id);
        if let Some(tab) = self.tab_mut(id) {
            tab.needs_you = needs_you && !on_screen;
        }
    }

    /// Names a tab, or gives it its kind's name back.
    pub fn set_title(&mut self, id: TabId, title: Option<String>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.title = title.filter(|t| !t.trim().is_empty());
        }
    }

    /// An AI agent started or left a terminal.
    pub fn set_agent(&mut self, id: TabId, agent: Option<String>) {
        if let Some(tab) = self.tab_mut(id) {
            tab.agent = agent;
        }
    }

    /// What to keep for the next run.
    pub fn saved(&self) -> SavedTabs {
        SavedTabs {
            tabs: self
                .tabs
                .iter()
                .map(|t| SavedTab { server: t.server.clone(), kind: t.kind.pane_kind(), n: t.n, title: t.title.clone(), session_key: t.session_key.clone(), path: t.path.clone() })
                .collect(),
            active: self.active_id().and_then(|id| self.index_of(id)),
            sidebar_hidden: !self.sidebar,
        }
    }

    /// Brings back the tabs of the last run, into a workbench that has none. Each terminal
    /// keeps its session key: its shell in tmux on the server is found again. A tab of a kind
    /// this version doesn't know is left out; one whose server left the list stays (its shell
    /// may still run). A terminal saved without a key gets a new one.
    pub fn restore(&mut self, saved: &SavedTabs) {
        if !self.tabs.is_empty() {
            return;
        }
        let mut active = None;
        for (i, t) in saved.tabs.iter().enumerate() {
            let Some(kind) = TabKind::from_pane_kind(&t.kind) else { continue };
            let id = TabId(self.next_id);
            self.next_id += 1;
            let session_key = match kind {
                TabKind::Terminal => Some(t.session_key.clone().filter(|k| !k.is_empty()).unwrap_or_else(new_session_key)),
                TabKind::Files | TabKind::Stats => None,
            };
            self.tabs.push(Tab {
                id,
                server: t.server.clone(),
                kind,
                n: t.n.max(1),
                title: t.title.clone().filter(|t| !t.trim().is_empty()),
                session_key,
                path: t.path.clone(),
                link: Link::Idle,
                needs_you: false,
                agent: None,
            });
            if saved.active == Some(i) {
                active = Some(id);
            }
        }
        self.sidebar = !saved.sidebar_hidden;
        match active.or(self.tabs.first().map(|t| t.id)) {
            // The main menu was showing: it shows again, with a tab to come back to.
            Some(id) if saved.active.is_none() => {
                self.land_on(id);
                self.home = true;
            }
            Some(id) => self.land_on(id),
            None => {}
        }
    }

    pub fn sidebar_shown(&self) -> bool {
        self.sidebar
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar = !self.sidebar;
    }

    pub fn tor(&self) -> Tor {
        self.tor
    }

    pub fn set_tor(&mut self, tor: Tor) {
        self.tor = tor;
    }

    /// A machine's vital signs changed, or are gone (`None`: it can't be asked).
    pub fn set_vitals(&mut self, server: &str, vitals: Option<Vitals>) {
        match vitals {
            Some(v) => {
                self.vitals.insert(server.to_owned(), v);
            }
            None => {
                self.vitals.remove(server);
            }
        }
    }

    /// The vital signs of the machine on screen, when they're known.
    pub fn current_vitals(&self) -> Option<&Vitals> {
        self.vitals.get(self.current_key()?)
    }
}

/// A new terminal's session key: ten letters and digits, as 0.10 makes them
/// (`Math.random().toString(36).slice(2, 12)`). The tmux session on the server is named after
/// it, so two terminals must never share one.
pub fn new_session_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    // The standard library's own randomness: every `RandomState` is keyed from the system's
    // random numbers (and a counter), so its hash of nothing is a fresh random number.
    let mut x = std::collections::hash_map::RandomState::new().build_hasher().finish();
    (0..10)
        .map(|_| {
            let d = (x % 36) as u8;
            x /= 36;
            char::from(if d < 10 { b'0' + d } else { b'a' + d - 10 })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB: &str = "host:web";
    const DB: &str = "host:db";
    const STAGING: &str = "host:staging";

    fn bench() -> Workbench {
        Workbench::new(vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("staging", "web-staging", "deploy@203.0.113.18").env(EnvLabel::Staging),
            Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Prod).icon("database"),
        ])
    }

    /// The row as "label@server" with a `*` on the tab on screen and `|` between machines.
    fn row(w: &Workbench) -> String {
        w.strip()
            .iter()
            .map(|s| format!("{}{}{}@{}", if s.starts_group { "| " } else { "" }, if s.active { "*" } else { "" }, s.tab.label(), s.server_name))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn it_starts_on_the_main_menu_with_this_computer_first() {
        let w = bench();
        assert!(w.showing_home());
        assert_eq!(w.active(), None);
        assert_eq!(w.window_title(), ("SSHeesh", false));
        let names: Vec<&str> = w.servers().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["This computer", "web-7731", "web-staging", "db-main"]);
        assert!(w.servers()[0].is_local());
        // A list that places this computer itself keeps its place, and a repeated machine is one.
        let w = Workbench::new(vec![Server::host("a", "a", ""), Server::this_computer(), Server::host("a", "again", "")]);
        let names: Vec<&str> = w.servers().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["a", "This computer"]);
    }

    #[test]
    fn going_to_a_machine_opens_a_terminal_once_and_then_comes_back_to_it() {
        let mut w = bench();
        let first = w.go_to(WEB);
        assert_eq!(row(&w), "*Terminal@web-7731");
        assert_eq!(w.window_title(), ("web-7731", true));
        let local = w.go_to("local");
        assert_eq!(row(&w), "Terminal@web-7731 | *Terminal@This computer");
        assert_eq!(w.window_title(), ("This computer", false));
        // Back to the server: the tab that's there, not a new one.
        assert_eq!(w.go_to(WEB), first);
        assert_eq!(row(&w), "*Terminal@web-7731 | Terminal@This computer");
        assert_ne!(first, local);
        assert_eq!(w.current_server().map(|s| s.name.as_str()), Some("web-7731"));
    }

    #[test]
    fn going_to_a_machine_lands_on_the_tab_you_were_last_on_there() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let c = w.new_terminal();
        assert!(w.focus(b));
        w.go_to(DB);
        assert_eq!(w.go_to(WEB), b);
        // That tab closed while you were away: the machine's first one.
        w.go_to(DB);
        w.close(b);
        assert_eq!(w.go_to(WEB), a);
        assert_ne!(a, c);
    }

    #[test]
    fn a_new_tab_opens_next_to_the_tab_you_were_on_at_that_machine() {
        let mut w = bench();
        let t1 = w.go_to(WEB);
        w.go_to(DB);
        assert_eq!(row(&w), "Terminal@web-7731 | *Terminal@db-main");
        // "+" on db-main: after its tab. A machine's tabs stay together.
        w.new_terminal();
        assert_eq!(row(&w), "Terminal@web-7731 | Terminal@db-main *Terminal 2@db-main");
        // Back on web's first tab, a new one opens right after it, before db's.
        w.focus(t1);
        w.new_terminal();
        w.focus(t1);
        w.new_terminal();
        assert_eq!(row(&w), "Terminal@web-7731 *Terminal 3@web-7731 Terminal 2@web-7731 | Terminal@db-main Terminal 2@db-main");
        // Files for web, opened while on db: next to the tab you were last on at web.
        w.go_to(DB);
        w.open(TabKind::Files, WEB);
        assert_eq!(row(&w), "Terminal@web-7731 Terminal 3@web-7731 *Files@web-7731 Terminal 2@web-7731 | Terminal@db-main Terminal 2@db-main");
    }

    #[test]
    fn a_tab_keeps_its_number_and_a_new_one_takes_the_lowest_free() {
        let mut w = bench();
        let t1 = w.go_to(WEB);
        let t2 = w.new_terminal();
        let t3 = w.new_terminal();
        let label = |w: &Workbench, id| w.tab(id).unwrap().label();
        assert_eq!((label(&w, t1), label(&w, t2), label(&w, t3)), ("Terminal".into(), "Terminal 2".into(), "Terminal 3".into()));
        // Closing 2 renames nobody; the next terminal is 2 again.
        w.close(t2);
        assert_eq!(label(&w, t3), "Terminal 3");
        let again = w.new_terminal();
        assert_eq!(label(&w, again), "Terminal 2");
        // Numbers are per machine and per kind.
        let files = w.open(TabKind::Files, WEB);
        assert_eq!(label(&w, files), "Files");
        let other = w.go_to(DB);
        assert_eq!(label(&w, other), "Terminal");
        // A title of its own wins, and an empty one gives the kind's name back.
        w.set_title(other, Some("Migration 0042".into()));
        assert_eq!(label(&w, other), "Migration 0042");
        w.set_title(other, Some("  ".into()));
        assert_eq!(label(&w, other), "Terminal");
    }

    #[test]
    fn every_terminal_gets_a_session_key_of_its_own() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let files = w.open(TabKind::Files, WEB);
        let key = |id| w.tab(id).unwrap().session_key.clone();
        let (ka, kb) = (key(a).unwrap(), key(b).unwrap());
        assert_ne!(ka, kb);
        for k in [&ka, &kb] {
            assert_eq!(k.len(), 10);
            assert!(k.bytes().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()), "{k}");
        }
        assert_eq!(key(files), None);
        let many: std::collections::HashSet<String> = (0..500).map(|_| new_session_key()).collect();
        assert_eq!(many.len(), 500);
    }

    #[test]
    fn closing_the_tab_you_are_on_brings_up_the_next_one_as_a_browser_does() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let c = w.new_terminal();
        let d = w.go_to(DB);
        assert_eq!(row(&w), "Terminal@web-7731 Terminal 2@web-7731 Terminal 3@web-7731 | *Terminal@db-main");
        // A tab in the middle of its machine's: the one after it.
        w.focus(b);
        assert_eq!(w.close(b).map(|t| t.id), Some(b));
        assert_eq!(w.active_id(), Some(c));
        // The machine's last in the row: the one before it.
        w.close(c);
        assert_eq!(w.active_id(), Some(a));
        // The machine's only tab: its neighbour in the row, another machine's.
        w.close(a);
        assert_eq!(w.active_id(), Some(d));
        assert_eq!(w.window_title(), ("db-main", true));
        // The last tab of all: the main menu.
        w.close(d);
        assert!(w.showing_home());
        assert_eq!(w.active_id(), None);
        assert!(w.tabs().is_empty());
        assert_eq!(w.close(d), None);
        assert_eq!(w.close_active(), None);
    }

    #[test]
    fn closing_a_machines_only_tab_prefers_the_neighbour_after_it() {
        let mut w = bench();
        w.go_to(WEB);
        let mid = w.go_to(STAGING);
        let last = w.go_to(DB);
        w.focus(mid);
        w.close(mid);
        assert_eq!(w.active_id(), Some(last));
    }

    #[test]
    fn closing_another_tab_leaves_you_where_you_are() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        w.close(a);
        assert_eq!(w.active_id(), Some(b));
        assert_eq!(row(&w), "*Terminal 2@web-7731");
    }

    #[test]
    fn closing_from_the_main_menu_keeps_the_menu() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        w.show_home();
        assert!(w.showing_home());
        assert_eq!(w.active(), None);
        // The tab to come back to closes: the menu stays, and the next tab is the one to come back to.
        w.close(b);
        assert!(w.showing_home());
        assert_eq!(w.step(1), Some(a));
        assert!(!w.showing_home());
    }

    #[test]
    fn a_closed_tab_comes_back_newest_first_and_twenty_are_kept() {
        let mut w = bench();
        w.go_to(WEB);
        let files = w.open_with(TabKind::Files, WEB, Open { path: Some("/srv/shop".into()), ..Open::default() });
        let named = w.open_with(TabKind::Terminal, DB, Open { title: Some("psql shop".into()), ..Open::default() });
        w.close(files);
        w.close(named);
        assert_eq!(w.closed().len(), 2);
        let back = w.reopen_closed().unwrap();
        assert_eq!((w.tab(back).unwrap().label(), w.tab(back).unwrap().server.as_str()), ("psql shop".into(), DB));
        assert_eq!(w.active_id(), Some(back));
        let back = w.reopen_closed().unwrap();
        assert_eq!((w.tab(back).unwrap().kind, w.tab(back).unwrap().path.as_deref()), (TabKind::Files, Some("/srv/shop")));
        assert_eq!(w.reopen_closed(), None);
        // A machine that left the list: its closed tabs are skipped.
        let gone = w.go_to(STAGING);
        w.close(gone);
        let web = w.go_to(WEB);
        w.close(web);
        w.set_servers(bench().servers().iter().filter(|s| s.key() != WEB).cloned().collect());
        let back = w.reopen_closed().unwrap();
        assert_eq!(w.tab(back).unwrap().server, STAGING);
        // Twenty at most.
        for _ in 0..30 {
            let t = w.new_terminal();
            w.close(t);
        }
        assert_eq!(w.closed().len(), CLOSED_KEPT);
    }

    #[test]
    fn stepping_goes_through_every_machines_tabs_and_around_the_ends() {
        let mut w = bench();
        assert_eq!(w.step(1), None);
        let a = w.go_to(WEB);
        let b = w.go_to(DB);
        let c = w.go_to("local");
        assert_eq!(w.step(1), Some(a));
        assert_eq!(w.step(1), Some(b));
        assert_eq!(w.step(-1), Some(a));
        assert_eq!(w.step(-1), Some(c));
        // From the main menu a step comes back to where you were.
        w.show_home();
        assert_eq!(w.step(1), Some(c));
        assert!(!w.showing_home());
    }

    #[test]
    fn jumping_by_number_counts_along_the_row_and_nine_is_the_last() {
        let mut w = bench();
        assert_eq!(w.jump(1), None);
        for _ in 0..11 {
            w.new_terminal();
        }
        let in_row: Vec<TabId> = w.tabs().iter().map(|t| t.id).collect();
        assert_eq!(in_row.len(), 11);
        assert_eq!(w.jump(1), Some(in_row[0]));
        assert_eq!(w.jump(8), Some(in_row[7]));
        assert_eq!(w.jump(9), Some(in_row[10]));
        assert_eq!(w.jump(0), None);
        assert_eq!(w.jump(10), None);
        // Past the end of a short row: nothing, and you stay.
        let mut w = bench();
        let only = w.go_to(WEB);
        assert_eq!(w.jump(3), None);
        assert_eq!(w.active_id(), Some(only));
    }

    #[test]
    fn a_tab_can_be_moved_along_the_row() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.go_to(DB);
        let c = w.go_to("local");
        assert!(w.move_tab(c, 0));
        assert_eq!(row(&w), "*Terminal@This computer | Terminal@web-7731 | Terminal@db-main");
        assert!(w.move_tab(a, 99));
        assert_eq!(w.tabs().iter().map(|t| t.id).collect::<Vec<_>>(), [c, b, a]);
        assert!(!w.move_tab(TabId(999), 0));
        // You stay on the tab you were on.
        assert_eq!(w.active_id(), Some(c));
    }

    #[test]
    fn one_prod_label_per_machine_and_the_current_machines_is_in_the_title() {
        let mut w = bench();
        w.go_to(WEB);
        w.open(TabKind::Files, WEB);
        let db = w.go_to(DB);
        w.new_terminal();
        w.go_to(STAGING);
        // On staging: web and db each carry the label on their first tab.
        let prod: Vec<bool> = w.strip().iter().map(|s| s.prod).collect();
        assert_eq!(prod, [true, false, true, false, false]);
        assert_eq!(w.window_title(), ("web-staging", false));
        // On db: its label moves to the title.
        w.focus(db);
        let prod: Vec<bool> = w.strip().iter().map(|s| s.prod).collect();
        assert_eq!(prod, [true, false, false, false, false]);
        assert_eq!(w.window_title(), ("db-main", true));
        let groups: Vec<bool> = w.strip().iter().map(|s| s.starts_group).collect();
        assert_eq!(groups, [false, false, true, false, true]);
    }

    #[test]
    fn needs_you_shows_until_you_land_on_the_tab() {
        let mut w = bench();
        let web = w.go_to(WEB);
        let db = w.go_to(DB);
        // The tab on screen never says it: you're looking at it.
        w.set_needs_you(db, true);
        assert_eq!(w.tab(db).unwrap().badge(), None);
        w.set_needs_you(web, true);
        assert_eq!(w.tab(web).unwrap().badge(), Some(Badge::NeedsYou));
        assert!(w.server_needs_you(WEB));
        assert!(!w.server_needs_you(DB));
        // It wins over a dropped connection, and landing on the tab clears it.
        w.set_link(web, Link::Reconnecting);
        assert_eq!(w.tab(web).unwrap().badge(), Some(Badge::NeedsYou));
        w.go_to(WEB);
        assert_eq!(w.tab(web).unwrap().badge(), Some(Badge::Reconnecting));
        assert!(!w.server_needs_you(WEB));
        // On the main menu no tab is on screen, so it shows.
        w.show_home();
        w.set_needs_you(web, true);
        assert!(w.server_needs_you(WEB));
    }

    #[test]
    fn a_tab_says_how_its_connection_is() {
        let mut w = bench();
        let t = w.go_to(WEB);
        let badge = |w: &mut Workbench, link| {
            w.set_link(t, link);
            w.tab(t).unwrap().badge().map(Badge::label)
        };
        assert_eq!(badge(&mut w, Link::Connecting), None);
        assert_eq!(badge(&mut w, Link::Connected { durable: true }), None);
        assert_eq!(badge(&mut w, Link::Reconnecting), Some("Reconnecting…"));
        assert_eq!(badge(&mut w, Link::Disconnected), Some("Disconnected"));
        assert_eq!(badge(&mut w, Link::Ended), Some("Ended"));
        // A tab that's gone is ignored.
        w.set_link(TabId(999), Link::Ended);
    }

    #[test]
    fn a_new_terminal_from_the_main_menu_goes_to_the_machine_you_were_on() {
        let mut w = bench();
        // Nothing open: this computer.
        let first = w.new_terminal();
        assert_eq!(w.tab(first).unwrap().server, "local");
        w.go_to(DB);
        w.show_home();
        let t = w.new_terminal();
        assert_eq!(w.tab(t).unwrap().server, DB);
        assert!(!w.showing_home());
    }

    #[test]
    fn tabs_of_a_machine_that_left_the_list_stay_open() {
        let mut w = bench();
        let t = w.go_to(WEB);
        w.set_servers(vec![Server::host("db", "db-main", "")]);
        assert_eq!(w.active_id(), Some(t));
        assert_eq!(row(&w), "*Terminal@Unknown server");
        assert_eq!(w.window_title(), ("Unknown server", false));
        assert_eq!(w.current_server(), None);
        assert_eq!(w.current_key(), Some(WEB));
    }

    #[test]
    fn the_sidebar_tor_and_vitals_follow_what_they_are_told() {
        let mut w = bench();
        assert!(w.sidebar_shown());
        w.toggle_sidebar();
        assert!(!w.sidebar_shown());
        assert_eq!(w.tor(), Tor::Off);
        w.set_tor(Tor::On { latency_ms: Some(84) });
        assert_eq!(w.tor(), Tor::On { latency_ms: Some(84) });
        // Vitals are the machine on screen's.
        let v = Vitals { memory: 0.6, uptime_secs: 60, ..Vitals::default() };
        w.set_vitals(WEB, Some(v.clone()));
        assert_eq!(w.current_vitals(), None);
        w.go_to(WEB);
        assert_eq!(w.current_vitals(), Some(&v));
        w.go_to(DB);
        assert_eq!(w.current_vitals(), None);
        w.set_vitals(WEB, None);
        w.go_to(WEB);
        assert_eq!(w.current_vitals(), None);
    }

    #[test]
    fn tabs_come_back_after_a_restart_with_their_shells_keys() {
        let mut w = bench();
        let a = w.go_to(WEB);
        w.open_with(TabKind::Files, WEB, Open { path: Some("/srv/shop".into()), ..Open::default() });
        let c = w.open_with(TabKind::Terminal, DB, Open { title: Some("psql shop".into()), ..Open::default() });
        w.new_terminal();
        w.focus(c);
        w.toggle_sidebar();
        let saved = w.saved();
        // As a file: 0.10's words for kinds, nothing empty written.
        let json = serde_json::to_string(&saved).unwrap();
        assert!(json.contains(r#""kind":"terminal""#) && json.contains(r#""kind":"files""#) && json.contains(r#""sidebarHidden":true"#), "{json}");
        assert!(!json.contains("null"), "{json}");
        let saved: SavedTabs = serde_json::from_str(&json).unwrap();

        let mut again = bench();
        again.restore(&saved);
        assert_eq!(row(&again), "Terminal@web-7731 Files@web-7731 | *psql shop@db-main Terminal 2@db-main");
        assert!(!again.sidebar_shown());
        let key = |w: &Workbench, i: usize| w.tabs()[i].session_key.clone();
        assert_eq!(key(&again, 0), w.tab(a).unwrap().session_key);
        assert_eq!(key(&again, 2), w.tab(c).unwrap().session_key);
        assert_eq!((key(&again, 1), again.tabs()[1].path.as_deref()), (None, Some("/srv/shop")));
        // New tabs carry on from there: the next terminal on db is 3, next to the one on screen.
        again.new_terminal();
        assert_eq!(row(&again), "Terminal@web-7731 Files@web-7731 | psql shop@db-main *Terminal 3@db-main Terminal 2@db-main");
        // Restoring into a workbench that already has tabs changes nothing.
        let before = again.clone();
        again.restore(&saved);
        assert_eq!(again, before);
    }

    #[test]
    fn a_saved_file_that_is_odd_still_restores_what_it_can() {
        // Saved on the main menu; a kind from a later version; a terminal without a key; a
        // server that left the list.
        let json = r#"{"tabs":[
            {"server":"host:web","kind":"terminal","n":0},
            {"server":"host:web","kind":"notes","n":1},
            {"server":"host:gone","kind":"terminal","n":1,"sessionKey":"abcdefghij","title":" "}
        ],"active":null,"later":"field"}"#;
        let saved: SavedTabs = serde_json::from_str(json).unwrap();
        let mut w = bench();
        w.restore(&saved);
        assert!(w.showing_home());
        assert_eq!(w.tabs().len(), 2);
        assert_eq!(w.tabs()[0].n, 1);
        assert_eq!(w.tabs()[0].session_key.as_ref().map(String::len), Some(10));
        assert_eq!((w.tabs()[1].session_key.as_deref(), w.tabs()[1].label()), (Some("abcdefghij"), "Terminal".to_owned()));
        // A step comes back to the first tab.
        assert_eq!(w.step(1), Some(w.tabs()[0].id));
        // Nothing saved at all: the main menu, as a first start.
        let mut w = bench();
        w.restore(&SavedTabs::default());
        assert!(w.showing_home() && w.tabs().is_empty() && w.sidebar_shown());
        // On the main menu the saved place is `None`.
        w.go_to(WEB);
        w.show_home();
        assert_eq!(w.saved().active, None);
    }

    #[test]
    fn kinds_map_to_0_10s() {
        for kind in [TabKind::Terminal, TabKind::Files, TabKind::Stats] {
            assert_eq!(TabKind::from_pane_kind(&kind.pane_kind()), Some(kind));
        }
        assert_eq!(TabKind::from_pane_kind(&PaneKind::Empty), None);
        assert_eq!(TabKind::from_pane_kind(&PaneKind::Other("notes".into())), None);
    }
}
