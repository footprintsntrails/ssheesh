//! The window's state: the workbench (servers, tabs, where you are), the terminal panes of
//! the tabs that have been shown, and the backend that gives them sessions. Every change of
//! the workbench from the screen goes through one of the methods here, so each ends the same
//! way ([`Shell::settle`]): panes of closed tabs end, the tab on screen has its pane and the
//! keyboard, and the backend knows which machine to watch.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use indexmap::IndexMap;
use rune::{ClipboardContent, Ime, KeyEvent, Tx};
use sshee_app::model::{Keys, Settings};
use sshee_app::settings_store::SettingsStore;
use sshee_app::workbench::shortcuts::{match_shortcut, Pressed, Shortcut};
use sshee_app::workbench::{Link, SavedTabs, Server, ServerKey, Tab, TabId, TabKind, Tor, Vitals, Workbench};

use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink, TermPane};
use crate::theme::Frame;

/// How another thread reaches the window's state: a closure run on the UI thread, in order.
/// It returns whether the window must draw again (Rune's `AppHandle::update_if`).
pub type Post = Arc<dyn Fn(Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>) + Send + Sync>;

/// The window, as background work reaches it: a session's status, a server's vital signs,
/// Tor's state. Cheap to clone; every call may come from any thread.
#[derive(Clone)]
pub struct Remote {
    post: Post,
}

impl Remote {
    pub fn new(post: Post) -> Remote {
        Remote { post }
    }

    /// Runs `f` on the UI thread; the window draws again after it.
    pub fn update(&self, f: impl FnOnce(&mut Shell) + Send + 'static) {
        (self.post)(Box::new(move |s, _| {
            f(s);
            true
        }));
    }

    /// Runs `f` on the UI thread without drawing: for what only sets a timer.
    pub fn quietly(&self, f: impl FnOnce(&mut Shell, &mut Tx) + Send + 'static) {
        (self.post)(Box::new(move |s, tx| {
            f(s, tx);
            false
        }));
    }

    /// A machine's vital signs, or `None` when it can't be asked.
    pub fn set_vitals(&self, server: ServerKey, vitals: Option<Vitals>) {
        self.update(move |s| s.bench.set_vitals(&server, vitals));
    }

    pub fn set_tor(&self, tor: Tor) {
        self.update(move |s| s.bench.set_tor(tor));
    }

    /// The list of servers changed.
    pub fn set_servers(&self, servers: Vec<Server>) {
        self.update(move |s| {
            s.bench.set_servers(servers);
            // Streamer mode hides the new names too.
            s.apply_settings();
        });
    }
}

/// What the window needs from outside: sessions for its terminals and news of its machines.
/// The program's is sshee-core; a demo's and a test's play fixtures.
pub trait Backend {
    /// Once, when the window starts: how background work reaches it.
    fn start(&mut self, remote: Remote) {
        let _ = remote;
    }

    /// The session for a terminal tab on `server` (`None`: a machine no longer in the list).
    /// Nothing connects yet: the pane opens the link when it is first laid out.
    fn terminal(&mut self, server: Option<&Server>, tab: &Tab) -> Box<dyn TermLink>;

    /// The machine on screen changed (`None`: the main menu): whose vital signs to watch.
    fn watch(&mut self, server: Option<&Server>) {
        let _ = server;
    }

    /// The row of tabs changed (one opened, closed, moved, or another is on screen): what to
    /// keep for the next run.
    fn tabs_changed(&mut self, saved: &SavedTabs) {
        let _ = saved;
    }

    /// Once, when the window starts: the settings it runs with, and keeps. The program's are
    /// the copy's own file; a demo's and a test's live in memory.
    fn settings(&mut self) -> SettingsStore {
        SettingsStore::in_memory(Settings::default())
    }

    /// A setting changed (and once at the start): what only the backend can act on, such as
    /// whether sessions reconnect by themselves.
    fn settings_changed(&mut self, settings: &Settings) {
        let _ = settings;
    }

    /// Settings' Tor switch and its buttons.
    fn tor(&mut self, command: crate::screens::settings::TorCommand) {
        let _ = command;
    }

    /// Sessions are replays on the window's own clock (a demo, a test): panes then run on
    /// Rune's clock, virtual in a headless run, so every run draws the same.
    fn replays(&self) -> bool {
        false
    }
}

/// What the program decides once, at the start.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub pane: PaneOptions,
    /// Who draws the window's buttons.
    pub frame: Frame,
    /// Shortcuts as on a Mac (⌘) or as elsewhere (Ctrl+Shift).
    pub mac_keys: bool,
    /// Shortcuts changed in Settings, by 0.10's ids.
    pub shortcuts: IndexMap<String, Option<Keys>>,
    /// Show the component sheet instead of the window (`--sheet`).
    pub sheet: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            pane: PaneOptions { scheme: crate::terminal::palette::Scheme::Fresh, ..PaneOptions::default() },
            frame: Frame::current(),
            mac_keys: cfg!(target_os = "macos"),
            shortcuts: IndexMap::new(),
            sheet: false,
        }
    }
}

/// The window's state.
pub struct Shell {
    pub bench: Workbench,
    /// The panes of terminal tabs that have been on screen, by tab. A tab that hasn't been
    /// shown yet has none (and no session).
    pub(crate) panes: HashMap<TabId, TermPane>,
    backend: Box<dyn Backend>,
    pub options: Options,
    remote: Option<Remote>,
    /// Rune's waker, for panes made later.
    waker: Option<rune::Waker>,
    /// The window is in front.
    pub(crate) window_focused: bool,
    /// The pane that was told it has the keyboard.
    keyboard: Option<TabId>,
    /// The machine the backend was last told to watch.
    watching: Option<ServerKey>,
    /// The width the row of tabs has, from layout.
    pub(crate) strip_width: Option<f32>,
    /// Panes run on Rune's clock from this instant (replays).
    clock_base: Option<Instant>,
    /// The tabs as the backend was last told to keep them.
    kept: SavedTabs,
    /// Every screen's own state (`crate::screens`).
    pub screens: crate::screens::States,
    /// The settings. Changed through [`Shell::change_settings`], which applies them at once.
    pub settings: SettingsStore,
    /// What panes hide and colour, as the settings say (`screens::settings::apply`).
    pub(crate) look: sshee_term::present::Look,
}

/// A session's status as a tab's.
fn link_of(status: &LinkStatus) -> Link {
    match status {
        LinkStatus::Idle => Link::Idle,
        LinkStatus::Connecting => Link::Connecting,
        LinkStatus::Connected { durable } => Link::Connected { durable: *durable },
        LinkStatus::Reconnecting { .. } => Link::Reconnecting,
        LinkStatus::Waiting { .. } => Link::Disconnected,
        LinkStatus::Gone { .. } => Link::Ended,
    }
}

impl Shell {
    pub fn new(bench: Workbench, mut backend: Box<dyn Backend>, options: Options, remote: Option<Remote>) -> Shell {
        if let Some(remote) = &remote {
            backend.start(remote.clone());
        }
        let clock_base = backend.replays().then(Instant::now);
        let kept = bench.saved();
        let settings = backend.settings();
        let mut shell = Shell {
            settings,
            look: sshee_term::present::Look::default(),
            screens: crate::screens::States::default(),
            kept,
            bench,
            panes: HashMap::new(),
            backend,
            options,
            remote,
            waker: None,
            window_focused: true,
            keyboard: None,
            watching: None,
            strip_width: None,
            clock_base,
        };
        shell.apply_settings();
        shell.sync_panes();
        shell.sync_watch();
        shell
    }

    /// Rune's waker (`AppSpec::setup`): output in any pane wakes the window.
    pub fn set_waker(&mut self, waker: rune::Waker) {
        for pane in self.panes.values_mut() {
            pane.set_waker(waker.clone());
        }
        self.waker = Some(waker);
    }

    /// The pane of the tab on screen, when that tab is a terminal.
    pub fn active_pane(&mut self) -> Option<&mut TermPane> {
        let id = self.bench.active_id()?;
        self.panes.get_mut(&id)
    }

    pub(crate) fn pane(&self, id: TabId) -> Option<&TermPane> {
        self.panes.get(&id)
    }

    /// A pane for `tab`, with a link from the backend and the window's wakers.
    fn make_pane(&mut self, tab: &Tab) -> TermPane {
        let id = tab.id;
        let link = self.backend.terminal(self.bench.server(&tab.server), tab);
        let status: StatusSink = match &self.remote {
            Some(remote) => {
                let remote = remote.clone();
                Arc::new(move |status| remote.update(move |s| s.link_status(id, status)))
            }
            None => Arc::new(|_| {}),
        };
        let mut pane = TermPane::new(link, status, self.options.pane.clone());
        pane.set_look(self.look.clone());
        if let Some(waker) = &self.waker {
            pane.set_waker(waker.clone());
        }
        // A burst the gate holds: told when to look from the session's thread, with no frame now.
        if let Some(remote) = self.remote.clone() {
            pane.model.set_hold_waker(Some(Arc::new(move |at| remote.quietly(move |s, tx| s.wake_at(id, at, tx)))));
        }
        if let Some(base) = self.clock_base {
            pane.use_virtual_clock(base);
        }
        pane
    }

    /// Ends the panes of tabs that are gone, and makes the pane of the tab on screen.
    fn sync_panes(&mut self) {
        let gone: Vec<TabId> = self.panes.keys().filter(|id| self.bench.tab(**id).is_none()).copied().collect();
        for id in gone {
            if let Some(mut pane) = self.panes.remove(&id) {
                pane.end();
            }
            if self.keyboard == Some(id) {
                self.keyboard = None;
            }
        }
        let bench = &self.bench;
        self.screens.retain(|id| bench.tab(id).is_some());
        if let Some(tab) = self.bench.active().filter(|t| t.kind == TabKind::Terminal && !self.panes.contains_key(&t.id)).cloned() {
            let pane = self.make_pane(&tab);
            self.panes.insert(tab.id, pane);
        }
    }

    /// Tells the backend which machine to watch: the one on screen, once a terminal there is
    /// connected (this computer needs none). A look at a server's vital signs reuses that
    /// connection, and must never sign in or ask about a host key on its own.
    fn sync_watch(&mut self) {
        let server = self.bench.current_server().filter(|s| {
            let key = s.key();
            s.is_local() || self.bench.tabs_on(&key).any(|t| matches!(t.link, Link::Connected { .. }))
        });
        let now = server.map(Server::key);
        if now != self.watching {
            self.backend.watch(server);
            self.watching = now;
        }
    }

    /// The pane of the tab on screen has the keyboard while the window is in front; the pane
    /// that had it is told it lost it (a program that asked for focus reports hears both).
    fn sync_keyboard(&mut self, tx: &mut Tx) {
        // While Settings is over the page, no terminal has the keyboard.
        let want = self.bench.active_id().filter(|id| self.window_focused && !self.screens.settings.open && self.panes.contains_key(id));
        // A pane starts out believing it has the keyboard.
        for (id, pane) in &mut self.panes {
            if pane.focused && Some(*id) != want && Some(*id) != self.keyboard {
                pane.focus(false, tx);
            }
        }
        if want == self.keyboard {
            return;
        }
        if let Some(pane) = self.keyboard.and_then(|id| self.panes.get_mut(&id)) {
            pane.focus(false, tx);
        }
        if let Some(pane) = want.and_then(|id| self.panes.get_mut(&id)) {
            pane.focus(true, tx);
        }
        self.keyboard = want;
    }

    /// The end of every change made from the screen: see the module's docs. Keys go to the
    /// terminal on screen again, whatever was clicked. Going anywhere leaves Settings.
    pub fn settle(&mut self, tx: &mut Tx) {
        self.screens.settings.open = false;
        self.settle_in_place(tx);
    }

    /// [`Shell::settle`] for a change that goes nowhere (the sidebar shown or hidden, Settings
    /// opened): a page over the tab's stays, and keeps the keyboard.
    pub(crate) fn settle_in_place(&mut self, tx: &mut Tx) {
        self.sync_panes();
        self.sync_watch();
        self.sync_keyboard(tx);
        let saved = self.bench.saved();
        if saved != self.kept {
            self.backend.tabs_changed(&saved);
            self.kept = saved;
        }
        if self.active_pane().is_some() && !self.screens.settings.open {
            tx.focus("term", false);
        }
        // Tabs come and go at once, as in a browser: nothing slides.
        tx.instant();
    }

    // ---- what the screen does

    /// A click on a server: the tab you were last on there, or a new terminal.
    pub fn go_to(&mut self, server: &str, tx: &mut Tx) {
        self.bench.go_to(server);
        self.settle(tx);
    }

    /// "+" and its shortcut: a new terminal on the machine on screen.
    pub fn new_terminal(&mut self, tx: &mut Tx) {
        self.bench.new_terminal();
        self.settle(tx);
    }

    /// A click on a tab. A tab whose connection is gone tries again.
    pub fn focus_tab(&mut self, id: TabId, tx: &mut Tx) {
        let again = self.bench.tab(id).is_some_and(|t| t.link == Link::Disconnected);
        self.bench.focus(id);
        self.settle(tx);
        if again {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.reconnect();
            }
        }
    }

    /// A tab's cross, a middle click on it, or the shortcut for the tab on screen.
    pub fn close_tab(&mut self, id: TabId, tx: &mut Tx) {
        self.bench.close(id);
        self.settle(tx);
    }

    pub fn close_active(&mut self, tx: &mut Tx) {
        if let Some(id) = self.bench.active_id() {
            self.close_tab(id, tx);
        }
    }

    pub fn reopen_closed(&mut self, tx: &mut Tx) {
        self.bench.reopen_closed();
        self.settle(tx);
    }

    pub fn step_tab(&mut self, step: i32, tx: &mut Tx) {
        self.bench.step(step);
        self.settle(tx);
    }

    pub fn jump_to_tab(&mut self, n: usize, tx: &mut Tx) {
        self.bench.jump(n);
        self.settle(tx);
    }

    pub fn toggle_sidebar(&mut self, tx: &mut Tx) {
        self.bench.toggle_sidebar();
        // Settings keeps it too, as 0.10 did.
        let shown = self.bench.sidebar_shown();
        self.settings.update(|s| s.sidebar_visible = shown);
        self.settle_in_place(tx);
    }

    /// The logo: the main menu.
    pub fn show_home(&mut self, tx: &mut Tx) {
        self.bench.show_home();
        self.settle(tx);
    }

    fn run(&mut self, shortcut: Shortcut, tx: &mut Tx) {
        match shortcut {
            Shortcut::NewTerminal => self.new_terminal(tx),
            Shortcut::CloseTab => self.close_active(tx),
            Shortcut::ReopenTab => self.reopen_closed(tx),
            Shortcut::ToggleSidebar => self.toggle_sidebar(tx),
            Shortcut::Settings => self.toggle_settings(tx),
            Shortcut::ToggleVitals => self.change_settings(tx, |st| st.set(|s| s.show_vitals = !s.show_vitals)),
            Shortcut::StreamerMode => self.change_settings(tx, |st| st.set(|s| s.streamer_mode = !s.streamer_mode)),
            Shortcut::StepTab(step) => self.step_tab(step, tx),
            Shortcut::JumpToTab(n) => self.jump_to_tab(n, tx),
        }
    }

    /// The window's shortcuts, before a key reaches the terminal. True: it was one.
    pub fn shortcut(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let Some(name) = super::keys::key_name(ev, tx.physical_key(), tx.key_base()) else { return false };
        let pressed = Pressed { key: &name, ctrl: ev.mods.ctrl, meta: ev.mods.logo, alt: ev.mods.alt, shift: ev.mods.shift };
        let Some(shortcut) = match_shortcut(&pressed, self.options.mac_keys, &self.options.shortcuts) else { return false };
        // A held key walks through the tabs; it doesn't open or close one after another.
        if !tx.key_repeat() || matches!(shortcut, Shortcut::StepTab(_)) {
            self.run(shortcut, tx);
        }
        true
    }

    /// A key with nothing focused (`AppSpec::on_key`): a shortcut, else the terminal on
    /// screen's.
    pub fn key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        // Settings, while it is over the page, takes what is left: nothing reaches a terminal
        // nobody sees.
        self.shortcut(ev, tx) || crate::screens::settings::key(self, ev, tx) || self.active_pane().is_some_and(|p| p.key(ev, tx))
    }

    pub fn paste(&mut self, content: &ClipboardContent, tx: &mut Tx) {
        if self.screens.settings.open {
            return;
        }
        if let Some(pane) = self.active_pane() {
            pane.paste(content, tx);
        }
    }

    pub fn ime(&mut self, ev: &Ime, tx: &mut Tx) -> bool {
        !self.screens.settings.open && self.active_pane().is_some_and(|p| p.ime(ev, tx))
    }

    /// The window came to the front or left it (`App::on_focus`).
    pub fn window_focus(&mut self, focused: bool, tx: &mut Tx) {
        self.window_focused = focused;
        self.sync_keyboard(tx);
    }

    /// The backend, for a screen that asks it something.
    pub(crate) fn backend(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
    }

    /// The window closes: connections go, shells in tmux stay.
    pub fn close(&mut self) {
        // Every change so far is in the settings file before the program ends.
        self.settings.flush();
        for pane in self.panes.values_mut() {
            pane.close();
        }
    }

    /// A session told its status (from its thread, through [`Remote`]).
    pub fn link_status(&mut self, id: TabId, status: LinkStatus) {
        self.bench.set_link(id, link_of(&status));
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.status = status;
        }
        self.sync_watch();
    }

    /// The start of every frame (`AppSpec::poll`): every pane takes what its session sent, the
    /// hidden ones too (a program's questions are answered, and switching to a tab shows it as
    /// it is now). True when the view must run again.
    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let active = self.bench.active_id();
        let mut changed = false;
        let mut asked = Vec::new();
        for (id, pane) in &mut self.panes {
            let drew = pane.poll(clock, tx);
            changed |= drew && Some(*id) == active;
            if pane.take_wants_you() {
                asked.push(*id);
            }
        }
        // A program rang or sent a notification: its tab says so, unless you're looking at it
        // or Settings says not to be told.
        for id in asked.into_iter().filter(|_| self.settings.get().notify_attention) {
            let before = self.bench.tab(id).is_some_and(|t| t.needs_you);
            self.bench.set_needs_you(id, true);
            changed |= self.bench.tab(id).is_some_and(|t| t.needs_you) != before;
        }
        changed
    }

    /// A pane's burst is held until `at` (`Model::set_hold_waker`): poll then, draw nothing now.
    pub fn wake_at(&mut self, id: TabId, at: Instant, tx: &mut Tx) {
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.wake_at(at, tx);
        }
    }
}
