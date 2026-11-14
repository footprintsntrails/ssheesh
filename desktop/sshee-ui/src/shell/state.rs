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
use sshee_app::model::Keys;
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
        self.update(move |s| s.bench.set_servers(servers));
    }

    /// The program in front in each of a server's terminals, by session key (tmux's answer):
    /// tabs are named after the agent working in them.
    pub fn set_foreground(&self, server: ServerKey, running: Vec<(String, String)>) {
        self.update(move |s| crate::screens::agent::foreground(s, &server, &running));
    }

    /// The answer to a Files request, or news of a transfer (`screens::files::deliver`).
    pub fn files(&self, reply: sshee_app::files::Reply) {
        self.update(move |s| crate::screens::files::deliver(s, reply));
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

    /// The servers with a connected terminal changed: whose terminals to ask about their
    /// foreground programs, every few seconds. The answers come back through
    /// [`Remote::set_foreground`].
    fn agents_on(&mut self, servers: &[Server]) {
        let _ = servers;
    }

    /// A terminal needs you, or went quiet, while the window was in the background: the
    /// system notification for it. The text is already safe to leave the app (secrets
    /// dotted, names too while streaming: `sshee_term::attention`). The seam for the step
    /// that shows it and brings `tab` forward on a click; until then nothing is shown.
    fn notify(&mut self, tab: &Tab, note: &sshee_term::attention::Notification) {
        let _ = (tab, note);
    }

    /// A Files tab asks for something: a folder's listing, a file, a change, a transfer.
    /// Never answered here: the work runs on a thread of its own and the answer comes back
    /// through [`Remote::files`], addressed with `ask.reply(..)`.
    fn files(&mut self, ask: sshee_app::files::Ask) {
        let _ = ask;
    }

    /// Adding and managing servers (`screens::add_server`, `screens::server_menu`): saving,
    /// the keychain and reading other programs' files wait, so the backend does what is
    /// asked on a thread of its own and answers through `Remote` (`Shell::servers_answer`,
    /// `Remote::set_servers`, `Shell::set_server_looks`).
    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        let _ = ask;
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
    /// Which tabs are shown side by side (`split.rs`), and where the pair is drawn.
    pub layout: sshee_app::workbench::layout::Layout,
    pub(crate) sides: super::split::Sides,
    /// The row of tabs' own state: its menu, a drag, what is in sight (`tabs.rs`).
    pub(crate) row: super::tabs::Row,
    /// Streamer mode (`streamer.rs`).
    pub(crate) streamer: super::streamer::Streamer,
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
        let mut shell = Shell {
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
            layout: Default::default(),
            sides: Default::default(),
            row: Default::default(),
            streamer: Default::default(),
        };
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

    /// Rune's waker, for a screen's own background work (a picture being decoded).
    pub(crate) fn waker(&self) -> Option<rune::Waker> {
        self.waker.clone()
    }

    /// The pane of the tab on screen, when that tab is a terminal.
    pub fn active_pane(&mut self) -> Option<&mut TermPane> {
        let id = self.bench.active_id()?;
        self.panes.get_mut(&id)
    }

    pub(crate) fn pane(&self, id: TabId) -> Option<&TermPane> {
        self.panes.get(&id)
    }

    /// The backend, for a screen that asks it for something.
    pub(crate) fn backend_mut(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
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
        // Side by side there are two terminals on the page: each takes the keyboard by its own name.
        pane.set_focus_key(format!("pane-{}", id.0));
        if self.streaming() {
            pane.view.borrow_mut().set_look(self.look());
        }
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

    /// Starts the terminal of a tab that hasn't been on screen (an agent waits there and its
    /// question is read without going to it).
    pub(crate) fn open_unseen(&mut self, id: TabId) {
        if let Some(tab) = self.bench.tab(id).filter(|t| t.kind == TabKind::Terminal && !self.panes.contains_key(&id)).cloned() {
            let mut pane = self.make_pane(&tab);
            pane.focused = false;
            pane.open_unseen(sshee_term::engine::Size::new(80, 24));
            self.panes.insert(id, pane);
        }
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
        self.row.retain(|id| bench.tab(id).is_some());
        self.layout.prune(bench);
        // A Files tab shown for the first time gets its state and lists its folder.
        crate::screens::files::shown(self);
        // The tab on screen, and the one shown beside it.
        for id in self.tabs_on_screen() {
            if let Some(tab) = self.bench.tab(id).filter(|t| t.kind == TabKind::Terminal && !self.panes.contains_key(&t.id)).cloned() {
                let pane = self.make_pane(&tab);
                self.panes.insert(tab.id, pane);
            }
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
        let want = self.bench.active_id().filter(|id| self.window_focused && self.panes.contains_key(id));
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
    /// terminal on screen again, whatever was clicked.
    pub fn settle(&mut self, tx: &mut Tx) {
        self.sync_panes();
        self.sync_watch();
        self.sync_keyboard(tx);
        self.save_tabs();
        crate::screens::agent::settled(self, tx);
        if self.screens.any_open() {
            // What is open over the window keeps the keyboard.
        } else if let Some(id) = self.bench.active_id().filter(|id| self.panes.contains_key(id)) {
            tx.focus(format!("pane-{}", id.0), false);
        } else if self.bench.showing_home() {
            // The main menu takes arrows, Return and typing, whatever was clicked.
            tx.focus("home-page", false);
        }
        // Tabs come and go at once, as in a browser: nothing slides.
        tx.instant();
    }

    /// Settings' two copy switches, for every terminal open now and opened later: whether a
    /// selection that rests is copied, and whether a program may write the clipboard (OSC 52).
    /// A program that copies what you select in it (OpenCode, tmux) and SSHeesh's own copy on
    /// select would otherwise both write.
    pub fn set_copy_modes(&mut self, copy_on_select: bool, program_clipboard: bool) {
        self.options.pane.copy_on_select = copy_on_select;
        self.options.pane.program_clipboard = program_clipboard;
        for pane in self.panes.values_mut() {
            pane.options.copy_on_select = copy_on_select;
            pane.options.program_clipboard = program_clipboard;
        }
    }

    /// Tells the backend what to keep for the next run, when the row of tabs changed.
    pub(crate) fn save_tabs(&mut self) {
        let saved = self.bench.saved();
        if saved != self.kept {
            self.backend.tabs_changed(&saved);
            self.kept = saved;
        }
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
        self.settle(tx);
    }

    /// The logo: the main menu.
    pub fn show_home(&mut self, tx: &mut Tx) {
        self.bench.show_home();
        self.settle(tx);
    }

    /// Files on the machine on screen (this computer from the main menu): its Files tab, or a
    /// new one when it has none.
    pub fn open_files(&mut self, tx: &mut Tx) {
        let server = self.bench.current_key().unwrap_or("local").to_owned();
        self.open_files_on(&server, None, tx);
    }

    /// Files on `server` (its one Files tab, or a new one), in `folder` when one is given: the
    /// palette's "Files" row, a server's menu. Returns the tab.
    pub fn open_files_on(&mut self, server: &str, folder: Option<&str>, tx: &mut Tx) -> TabId {
        let id = self.bench.open_files(server);
        // A tab not shown yet starts there; one already showing another folder goes there.
        let fresh = !self.screens.files.contains_key(&id);
        if let (true, Some(folder)) = (fresh, folder) {
            self.bench.set_path(id, Some(folder.to_owned()));
        }
        self.settle(tx);
        if let (false, Some(folder)) = (fresh, folder) {
            crate::screens::files::go(self, id, folder);
        }
        id
    }

    /// Sends a Files request to the backend. The answer arrives later through
    /// [`Remote::files`]; `screens::files` has a function for everything the screen does.
    pub fn files_ask(&mut self, ask: sshee_app::files::Ask) {
        self.backend.files(ask);
    }

    /// Sessions and files are replays (a demo, a test): time stands still for Files too.
    pub(crate) fn replays(&self) -> bool {
        self.clock_base.is_some()
    }

    fn run(&mut self, shortcut: Shortcut, tx: &mut Tx) {
        match shortcut {
            Shortcut::NewTerminal => self.new_terminal(tx),
            Shortcut::CloseTab => self.close_active(tx),
            Shortcut::ReopenTab => self.reopen_closed(tx),
            Shortcut::ToggleSidebar => self.toggle_sidebar(tx),
            Shortcut::OpenFiles => self.open_files(tx),
            Shortcut::StepTab(step) => self.step_tab(step, tx),
            Shortcut::JumpToTab(n) => self.jump_to_tab(n, tx),
            Shortcut::Find { .. } => {
                if let Some(pane) = self.active_pane() {
                    pane.open_find(tx);
                }
            }
            Shortcut::Streamer => self.toggle_streamer(tx),
            Shortcut::SideBySide => self.toggle_side_by_side(tx),
            Shortcut::Palette | Shortcut::PaletteAlt => self.toggle_palette(tx),
            Shortcut::AddServer => self.open_add_server(tx),
            Shortcut::Agents => crate::screens::agent::toggle_list(self, tx),
        }
    }

    /// The window's shortcuts, before a key reaches the terminal. True: it was one.
    pub fn shortcut(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let Some(name) = super::keys::key_name(ev, tx.physical_key(), tx.key_base()) else { return false };
        let pressed = Pressed { key: &name, ctrl: ev.mods.ctrl, meta: ev.mods.logo, alt: ev.mods.alt, shift: ev.mods.shift };
        let Some(shortcut) = match_shortcut(&pressed, self.options.mac_keys, &self.options.shortcuts) else { return false };
        // Ctrl+F alone is also a program's key (vim, less): the terminal on screen says whose.
        if shortcut == (Shortcut::Find { always: false }) && !self.active_pane().is_some_and(|p| p.find_is_open() || p.takes_find_key()) {
            return false;
        }
        // A held key walks through the tabs; it doesn't open or close one after another.
        if !tx.key_repeat() || matches!(shortcut, Shortcut::StepTab(_)) {
            self.run(shortcut, tx);
        }
        true
    }

    /// A key with nothing focused (`AppSpec::on_key`): a shortcut, else the terminal on
    /// screen's.
    pub fn key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        // Behind the palette, a dialog or a menu, keys are theirs alone.
        if self.screens.any_open() {
            return false;
        }
        self.shortcut(ev, tx) || self.active_pane().is_some_and(|p| p.key(ev, tx)) || self.home_key(ev, tx) || crate::screens::files::key(self, ev, tx)
    }

    pub fn paste(&mut self, content: &ClipboardContent, tx: &mut Tx) {
        if let Some(pane) = self.active_pane() {
            pane.paste(content, tx);
        }
    }

    pub fn ime(&mut self, ev: &Ime, tx: &mut Tx) -> bool {
        self.active_pane().is_some_and(|p| p.ime(ev, tx))
    }

    /// The window came to the front or left it (`App::on_focus`).
    pub fn window_focus(&mut self, focused: bool, tx: &mut Tx) {
        self.window_focused = focused;
        self.sync_keyboard(tx);
    }

    /// The window closes: connections go, shells in tmux stay.
    pub fn close(&mut self) {
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
        crate::screens::agent::linked(self);
    }

    /// The start of every frame (`AppSpec::poll`): every pane takes what its session sent, the
    /// hidden ones too (a program's questions are answered, and switching to a tab shows it as
    /// it is now). True when the view must run again.
    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        // The screens' own clockwork: who started to wait, a server's card coming up.
        let ticked = crate::screens::home::tick(self, clock, tx);
        let active = self.bench.active_id();
        let mut changed = false;
        let mut asked = Vec::new();
        let mut wishes = Vec::new();
        let beside = active.and_then(|id| self.layout.partner(id));
        for (id, pane) in &mut self.panes {
            let drew = pane.poll(clock, tx);
            changed |= drew && (Some(*id) == active || Some(*id) == beside);
            if pane.take_wants_you() {
                asked.push(*id);
            }
            wishes.extend(pane.take_wishes().into_iter().map(|wish| (*id, wish)));
        }
        // What panes ask of the window (`tabs.rs`), and names to hide that are new.
        changed |= self.grant(wishes, tx);
        changed |= self.keep_streaming(tx);
        // A program rang or sent a notification: its tab says so, unless you're looking at it.
        for id in asked {
            let before = self.bench.tab(id).is_some_and(|t| t.needs_you);
            crate::screens::agent::asked(self, id, clock);
            changed |= self.bench.tab(id).is_some_and(|t| t.needs_you) != before;
        }
        // Agents: who works, who went quiet, and the card of the one being answered.
        let agents = crate::screens::agent::poll(self, clock, tx);
        changed || ticked || agents
    }

    /// A pane's burst is held until `at` (`Model::set_hold_waker`): poll then, draw nothing now.
    pub fn wake_at(&mut self, id: TabId, at: Instant, tx: &mut Tx) {
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.wake_at(at, tx);
        }
    }
}
