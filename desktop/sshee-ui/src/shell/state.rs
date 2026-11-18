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
    /// Brings the window forward, hides it, quits (`shell::desktop`); none in tests.
    pub(crate) window: Option<super::desktop::WindowControl>,
}

impl Remote {
    pub fn new(post: Post) -> Remote {
        Remote { post, window: None }
    }

    /// Runs `f` on the UI thread with the change's `Tx` (to move the keyboard, say); the
    /// window draws again after it.
    pub fn update_tx(&self, f: impl FnOnce(&mut Shell, &mut Tx) + Send + 'static) {
        (self.post)(Box::new(move |s, tx| {
            f(s, tx);
            true
        }));
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

    /// The program in front in each of a server's terminals, by session key (tmux's answer):
    /// tabs are named after the agent working in them.
    pub fn set_foreground(&self, server: ServerKey, running: Vec<(String, String)>) {
        self.update(move |s| crate::screens::agent::foreground(s, &server, &running));
    }

    /// The answer to a Files request, or news of a transfer (`screens::files::deliver`).
    pub fn files(&self, reply: sshee_app::files::Reply) {
        self.update(move |s| crate::screens::files::deliver(s, reply));
    }

    /// The answer to the security check, a fix, or what a recipe asked
    /// (`screens::checkup::deliver`).
    pub fn recipes(&self, reply: sshee_app::recipes::Reply) {
        self.update(move |s| crate::screens::checkup::deliver(s, reply));
    }

    /// "Free up disk space": a look, a folder's biggest entries, how a cleanup went
    /// (`screens::disk::deliver`).
    pub fn disk(&self, reply: sshee_app::disk::Reply) {
        self.update(move |s| crate::screens::disk::deliver(s, reply));
    }

    /// A Containers tab's answer: a look, or how a start, stop or restart went
    /// (`screens::containers::deliver`, which times the next look).
    pub fn containers(&self, reply: sshee_app::containers::Reply) {
        (self.post)(Box::new(move |s, tx| {
            // The snippet form's look, for a `{{container}}` blank.
            if reply.tab == sshee_app::snippets::FORM_TAB {
                crate::screens::snippets::containers_listed(s, reply);
            } else {
                crate::screens::containers::deliver(s, reply, tx);
            }
            true
        }));
    }

    /// The programs on a machine, for a Stats tab (`screens::stats::deliver`).
    pub fn stats(&self, reply: sshee_app::stats::Reply) {
        (self.post)(Box::new(move |s, tx| {
            crate::screens::stats::deliver(s, reply, tx);
            true
        }));
    }

    /// What a look at a machine told beyond its vital signs (name, kernel, load, every disk),
    /// or why it can't be asked.
    pub fn set_facts(&self, server: ServerKey, facts: Result<sshee_app::stats::Facts, String>) {
        self.update(move |s| crate::screens::stats::set_facts(s, server, facts));
    }

    /// News from the edit bridge: a file open in an editor elsewhere, an answer to what was
    /// asked of it (`screens::files::edit::deliver`).
    pub fn edit(&self, reply: sshee_app::files::edit::EditReply) {
        self.update(move |s| crate::screens::files::edit::deliver(s, reply));
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
    /// system notification for it (only when Settings says to tell you:
    /// `Shell::notify_system`). The text is already safe to leave the app (secrets dotted,
    /// names too while streaming: `sshee_term::attention`). Its click comes back as
    /// `Remote::open_tab` (or, on Windows, a link: [`Backend::tab_of_link`]).
    fn notify(&mut self, tab: &Tab, note: &sshee_term::attention::Notification) {
        let _ = (tab, note);
    }

    /// The tabs that need you changed (the tray lists them; a tab you've seen takes its
    /// notification away). Names are as the window shows them (`shell::desktop::Waiting`).
    fn waiting_changed(&mut self, waiting: &[super::desktop::Waiting]) {
        let _ = waiting;
    }

    /// The window was hidden or shown again from the tray, or came to the front.
    fn window_shown(&mut self, shown: bool) {
        let _ = shown;
    }

    /// The tab a link the system opened with the app names (a Windows toast's click), if it
    /// is one of the backend's notifications'.
    fn tab_of_link(&self, link: &str) -> Option<TabId> {
        let _ = link;
        None
    }

    /// A Files tab asks for something: a folder's listing, a file, a change, a transfer.
    /// Never answered here: the work runs on a thread of its own and the answer comes back
    /// through [`Remote::files`], addressed with `ask.reply(..)`.
    fn files(&mut self, ask: sshee_app::files::Ask) {
        let _ = ask;
    }

    /// The folder on this computer under the pointer, outside the window, where the system can
    /// say (Windows: the desktop or an Explorer window): rows dragged out of Files and let go
    /// there download into it. Asked the moment they're let go. `None`: into Downloads.
    fn folder_under_pointer(&mut self) -> Option<std::path::PathBuf> {
        None
    }

    /// A Containers tab asks for a look or a start, stop or restart. Done on a thread of its
    /// own; the answer comes back through [`Remote::containers`].
    fn containers(&mut self, ask: sshee_app::containers::Ask) {
        let _ = ask;
    }

    /// A Stats tab asks which programs run. Answered through [`Remote::stats`].
    fn stats(&mut self, ask: sshee_app::stats::Ask) {
        let _ = ask;
    }

    /// "Open in your own editor": the backend's one edit bridge does what is asked, those
    /// that wait on a thread of its own, and answers through [`Remote::edit`]. `CloseAll` is
    /// done before this returns: the window is going.
    fn edit(&mut self, ask: sshee_app::files::edit::EditAsk) {
        let _ = ask;
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

    /// The Tor popover's other asks: its log's last lines (answered through
    /// [`Remote::tor_log`]), and the log's folder shown.
    fn tor_ask(&mut self, ask: sshee_app::tor::Ask) {
        let _ = ask;
    }

    /// Tunnels: start one, stop one, keep the list, look at what a server runs. Each tunnel's
    /// state comes back through [`Remote::tunnels`], as do the list kept from last time, at
    /// the start, and what a look found.
    fn tunnels(&mut self, ask: sshee_app::tunnels::Ask) {
        let _ = ask;
    }

    /// Why a tunnel can't listen on `port` on this computer (taken, or one only an
    /// administrator may use), checked before a tunnel is saved. Quick: a bind and a close.
    fn port_problem(&mut self, port: u16) -> Option<String> {
        let _ = port;
        None
    }

    /// A free port on this computer for a server's `preferred` port: the same number when it's
    /// free, else a look-alike (80 → 8080, 3000 → 13000), never one of `avoid` (claimed by
    /// other tunnels). For "Open" on a web app.
    fn free_port(&mut self, preferred: u16, avoid: &[u16]) -> Result<u16, String> {
        if !avoid.contains(&preferred) {
            return Ok(preferred);
        }
        [preferred.checked_add(8000).filter(|_| preferred < 1024), preferred.checked_add(10000)].into_iter().flatten().find(|p| !avoid.contains(p)).ok_or_else(|| "Couldn't find a free port on this computer.".to_owned())
    }


    /// Adding and managing servers (`screens::add_server`, `screens::server_menu`): saving,
    /// the keychain and reading other programs' files wait, so the backend does what is
    /// asked on a thread of its own and answers through `Remote` (`Shell::servers_answer`,
    /// `Remote::set_servers`, `Shell::set_server_looks`).
    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        let _ = ask;
    }

    /// "Restart to update" on a newer test build (`screens::canary`): puts it in place and
    /// has it started once the window has closed. `Ok` closes the window; an error is said on
    /// the line instead and nothing has changed.
    fn restart_to_update(&mut self) -> Result<(), String> {
        Err("this copy doesn't update itself".into())
    }

    /// The security check, its fixes and what recipes need to know (`screens::checkup`,
    /// `screens::recipes`): each waits on a server or a file, so the backend does it on a
    /// thread of its own and answers through [`Remote::recipes`].
    fn recipes(&mut self, ask: sshee_app::recipes::Ask) {
        let _ = ask;
    }

    /// "Free up disk space" (`screens::disk`): a look at the disks, a folder's biggest
    /// entries, a cleanup you said yes to. Each waits on a server, so the backend does it on
    /// a thread of its own and answers through [`Remote::disk`].
    fn disk(&mut self, ask: sshee_app::disk::Ask) {
        let _ = ask;
    }

    /// Once, when the window starts: Files' pinned and recent folders kept between runs.
    fn bookmarks(&mut self) -> Option<sshee_app::model::Bookmarks> {
        None
    }

    /// Files' bookmarks or recent folders changed: keep them.
    fn bookmarks_changed(&mut self, b: &sshee_app::model::Bookmarks) {
        let _ = b;
    }

    /// Once, when the window starts: the snippets kept between runs (`None`: 0.10's
    /// starters).
    fn snippets(&mut self) -> Option<sshee_app::model::Snippets> {
        None
    }

    /// The snippets changed: keep them.
    fn snippets_changed(&mut self, list: &sshee_app::model::Snippets) {
        let _ = list;
    }

    /// Whether this start may show the welcome (when it's due): the program's yes; a demo and
    /// a test start without it.
    fn welcome(&mut self) -> bool {
        false
    }

    /// The program's version, for "What's new" (`None`: it never opens by itself).
    fn version(&mut self) -> Option<String> {
        None
    }

    /// The window closes: where it is, to open there at the next start (`place.rs`). Not
    /// told where the system doesn't say (Wayland).
    fn window_placed(&mut self, place: &sshee_app::windows::Placement) {
        let _ = place;
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
    /// Which build this is, for Settings → About.
    pub build: sshee_app::canary::Build,
    /// Windows: Settings' "Use the Windows title bar", as saved when the program started.
    pub native_title_bar: bool,
    /// Where the window was when it last closed (`place.rs`): it opens there.
    pub place: Option<sshee_app::windows::Placement>,
    /// The system the terminal's "System" font is picked for (SF Mono on a Mac, Cascadia
    /// Mono on Windows). Not `frame`: the Windows title bar draws a Linux frame. The crate's
    /// own tests lay terminals out with the bundled font on every system.
    pub platform: sshee_app::model::Platform,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            pane: PaneOptions { scheme: crate::terminal::palette::Scheme::Fresh, ..PaneOptions::default() },
            frame: Frame::current(),
            mac_keys: cfg!(target_os = "macos"),
            shortcuts: IndexMap::new(),
            sheet: false,
            build: Default::default(),
            native_title_bar: false,
            place: None,
            platform: if cfg!(test) { sshee_app::model::Platform::Linux } else { sshee_app::model::Platform::current() },
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
    pub(crate) remote: Option<Remote>,
    /// Rune's waker, for panes made later.
    waker: Option<rune::Waker>,
    /// The window is in front.
    pub(crate) window_focused: bool,
    /// The pane that was told it has the keyboard.
    keyboard: Option<TabId>,
    /// The machine the backend was last told to watch.
    watching: Option<ServerKey>,
    /// The width the title and the row of tabs share in the top bar, from layout.
    pub(crate) bar_room: Option<f32>,
    /// The status bar's width, from layout: the vital signs that fit in it are shown.
    pub(crate) status_room: Option<f32>,
    /// Panes run on Rune's clock from this instant (replays).
    clock_base: Option<Instant>,
    /// The tabs as the backend was last told to keep them.
    kept: SavedTabs,
    /// Every screen's own state (`crate::screens`).
    pub screens: crate::screens::States,
    /// The settings. Changed through [`Shell::change_settings`], which applies them at once.
    pub settings: SettingsStore,
    /// The words terminals colour, as Settings has them (`screens::settings::apply`).
    pub(crate) highlights: std::sync::Arc<sshee_term::highlight::Highlights>,
    /// The panes' look changed where no `Tx` was at hand (a setting from another thread): the
    /// next poll hands it to them.
    pub(crate) look_stale: bool,
    /// Where the splits on screen are drawn (`split.rs`); which tabs share the page is the
    /// workbench's (`Workbench::layout`).
    pub(crate) sides: super::split::Sides,
    /// A drag of the sidebar's edge (`rail.rs`).
    pub(crate) rail: super::rail::RailDrag,
    /// Compact mode's bars that slid out (`compact.rs`).
    pub(crate) compact: super::compact::Compact,
    /// The row of tabs' own state: its menu, a drag, what is in sight (`tabs.rs`).
    pub(crate) row: super::tabs::Row,
    /// Streamer mode (`streamer.rs`).
    pub(crate) streamer: super::streamer::Streamer,
    /// The terminal files from another app are dragged over (`file_drop.rs`).
    pub(crate) drop_over: Option<TabId>,
    /// Files dropped on a server's terminal, asking which folder they go to (`file_drop.rs`).
    pub(crate) upload: Option<super::file_drop::UploadAsk>,
    /// The native window, once it's made: where it is, for `place.rs`.
    pub(crate) window: Option<rune::WindowHandle>,
    /// Terminal tabs restored from the last run whose pane hasn't been made yet.
    opened_before: Vec<Tab>,
}

/// A session's status as a tab's.
fn link_of(status: &LinkStatus) -> Link {
    match status {
        LinkStatus::Idle => Link::Idle,
        LinkStatus::Connecting => Link::Connecting,
        LinkStatus::Connected { durable } => Link::Connected { durable: *durable },
        LinkStatus::Reconnecting { .. } => Link::Reconnecting,
        LinkStatus::Waiting { .. } => Link::Disconnected,
        // Signing in failed: the tab says what the card over the terminal does.
        LinkStatus::Gone { .. } => Link::Failed,
        LinkStatus::Ended { .. } => Link::Ended,
        LinkStatus::Retrying { .. } | LinkStatus::Rejoining { .. } => Link::Reconnecting,
        LinkStatus::Asleep => Link::Asleep,
        LinkStatus::HostKey(_) => Link::Connecting,
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
            highlights: Default::default(),
            look_stale: false,
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
            bar_room: None,
            status_room: None,
            clock_base,
            sides: Default::default(),
            rail: Default::default(),
            compact: Default::default(),
            row: Default::default(),
            streamer: Default::default(),
            drop_over: None,
            upload: None,
            window: None,
            opened_before: Vec::new(),
        };
        shell.opened_before = shell.bench.tabs().iter().filter(|t| t.kind == TabKind::Terminal && t.session_key.is_some()).cloned().collect();
        shell.apply_settings();
        shell.sync_panes();
        shell.sync_watch();
        // The editors on this computer, and copies an earlier run left unsent.
        crate::screens::files::edit::start(&mut shell);
        // Your snippets; the welcome or "What's new", when one is due.
        crate::screens::snippets::start(&mut shell);
        crate::screens::files::start(&mut shell);
        crate::screens::welcome::start(&mut shell);
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
        // The window's key rules are the pane's too: a test of a Linux window running on a Mac
        // gets Ctrl+click and Shift+drag, as a real Mac window gets Cmd+click.
        let here = sshee_term::input::Platform::current();
        pane.set_platform(match (self.options.mac_keys, here) {
            (true, _) => sshee_term::input::Platform::Mac,
            (false, sshee_term::input::Platform::Mac) => sshee_term::input::Platform::Other,
            (false, other) => other,
        });
        // Side by side there are two terminals on the page: each takes the keyboard by its own name.
        pane.set_focus_key(format!("pane-{}", id.0));
        if let Some(server) = self.bench.server(&tab.server) {
            pane.set_machine(!server.is_local(), &self.server_shown(&tab.server, &server.name));
        }
        pane.view.borrow_mut().set_look(self.look());
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
        // A terminal closed before it was ever shown this run still has a shell in tmux on its
        // server: it ends too, as a shown one's does.
        for tab in self.opened_before.iter().filter(|t| self.bench.tab(t.id).is_none() && !self.panes.contains_key(&t.id)) {
            self.backend.terminal(self.bench.server(&tab.server), tab).end();
        }
        self.opened_before.retain(|t| self.bench.tab(t.id).is_some() && !self.panes.contains_key(&t.id));
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
        // A Files tab shown for the first time gets its state and lists its folder.
        crate::screens::files::shown(self);
        // A server's last tab closed: its files open elsewhere stop being edited.
        crate::screens::files::edit::hosts_changed(self);
        // The tab on screen, and the ones sharing the page with it.
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
        self.save_tabs();
        crate::screens::agent::settled(self, tx);
        // The tray's list of tabs that need you (`desktop.rs`).
        self.sync_waiting();
        if self.screens.any_open() || self.screens.settings.open {
            // What is open over the window, and Settings over the page, keep the keyboard.
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
        let again = self.bench.tab(id).is_some_and(|t| matches!(t.link, Link::Disconnected | Link::Asleep | Link::Ended | Link::Failed));
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

    /// Sends a request to the backend's edit bridge (`screens::files::edit`).
    pub fn edit_ask(&mut self, ask: sshee_app::files::edit::EditAsk) {
        self.backend.edit(ask);
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
            Shortcut::Settings => self.toggle_settings(tx),
            Shortcut::ToggleVitals => self.change_settings(tx, |st| st.set(|s| s.show_vitals = !s.show_vitals)),
            Shortcut::StepTab(step) => self.step_tab(step, tx),
            Shortcut::JumpToTab(n) => self.jump_to_tab(n, tx),
            Shortcut::Find { .. } => {
                if let Some(pane) = self.active_pane() {
                    pane.open_find(tx);
                }
            }
            Shortcut::Streamer => self.toggle_streamer(tx),
            Shortcut::SideBySide => self.toggle_side_by_side(tx),
            Shortcut::SplitRight => self.split_pane(sshee_app::workbench::layout::Dir::Row, tx),
            Shortcut::SplitDown => self.split_pane(sshee_app::workbench::layout::Dir::Col, tx),
            Shortcut::Zoom => self.zoom_active(tx),
            Shortcut::Compact => self.toggle_compact(tx),
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
        if self.tip_key(ev, tx) {
            return true;
        }
        // Behind the palette, a dialog or a menu, keys are theirs alone; streamer mode's
        // shortcut works over all of them (names must hide at once).
        if self.screens.any_open() || self.upload.is_some() {
            return self.streamer_key(ev, tx);
        }
        // Settings, while it is over the page, takes what is left: nothing reaches a terminal
        // nobody sees.
        self.shortcut(ev, tx)
            || crate::screens::settings::key(self, ev, tx)
            || self.active_pane().is_some_and(|p| p.key(ev, tx))
            || self.home_key(ev, tx)
            || crate::screens::files::key(self, ev, tx)
    }

    pub fn paste(&mut self, content: &ClipboardContent, tx: &mut Tx) {
        if self.screens.settings.open {
            return;
        }
        // Files copied on this computer, into the folder on screen.
        if crate::screens::files::paste(self, content) {
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
        if focused {
            // In front, so on screen: the tray offers Hide (`desktop.rs`).
            self.window_shown(true);
        }
        self.sync_keyboard(tx);
    }

    /// The window closes: connections go, shells in tmux stay.
    pub fn close(&mut self) {
        // Where the window is, to open there next time.
        if let Some(place) = super::place::now(self) {
            self.backend.window_placed(&place);
        }
        // Every change so far is in the settings file before the program ends.
        self.settings.flush();
        // Files open in an editor elsewhere end; copies with unsent changes stay.
        crate::screens::files::edit::close_all(self);
        for pane in self.panes.values_mut() {
            pane.close();
        }
    }

    /// A session told its status (from its thread, through [`Remote`]).
    pub fn link_status(&mut self, id: TabId, status: LinkStatus) {
        self.bench.set_link(id, link_of(&status));
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.set_status(status);
            pane.ask_agents();
        }
        // Its server is back: files open elsewhere waiting to upload try at once.
        crate::screens::files::edit::reconnected(self, id);
        // Signed in: Files there that asked you to sign in try again.
        crate::screens::files::signed_in(self, id);
        self.sync_watch();
        crate::screens::agent::linked(self);
        // The first connect: what the server is for, for its icon.
        crate::screens::server_menu::linked(self, id);
    }

    /// The start of every frame (`AppSpec::poll`): every pane takes what its session sent, the
    /// hidden ones too (a program's questions are answered, and switching to a tab shows it as
    /// it is now). True when the view must run again.
    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        // The screens' own clockwork: who started to wait, a server's card coming up.
        let ticked = crate::screens::home::tick(self, clock, tx);
        // A folder pointed at in Files is listed ahead; a picture it opened is decoded.
        let pictured = crate::screens::files::tick(self, clock, tx);
        // A hint whose moment came; "Add a server" opened from the welcome, at its list.
        let ticked = crate::screens::tour::tick(self, clock) | crate::screens::welcome::tick(self) | ticked;
        let winked = crate::looks::tick_logo(self, clock, tx);
        // A setting brought more tabs onto the page ("Tabs on screen"): their terminals start.
        let unmade = self.tabs_on_screen().into_iter().any(|id| !self.panes.contains_key(&id) && self.bench.tab(id).is_some_and(|t| t.kind == TabKind::Terminal));
        if unmade {
            self.settle_in_place(tx);
        }
        // Containers and Stats tabs on screen look again when it's time.
        let ticked = crate::screens::containers::tick(self, clock, tx) | crate::screens::rail_tree::tick(self, clock, tx) | crate::screens::stats::tick(self, clock) | ticked;
        let mut changed = false;
        let mut asked = Vec::new();
        let mut wishes = Vec::new();
        let shown = self.tabs_on_screen();
        for (id, pane) in &mut self.panes {
            pane.set_shown(shown.contains(id));
            let drew = pane.poll(clock, tx);
            changed |= drew && shown.contains(id);
            if pane.take_wants_you() {
                asked.push(*id);
            }
            wishes.extend(pane.take_wishes().into_iter().map(|wish| (*id, wish)));
        }
        // What panes ask of the window (`tabs.rs`), and names to hide that are new.
        changed |= self.grant(wishes, tx);
        changed |= self.keep_streaming(tx);
        // A program rang or sent a notification: its tab says so, unless you're looking at it
        // or Settings says not to be told.
        let notify = self.settings.get().notify_attention;
        for id in asked.into_iter().filter(|_| notify) {
            let before = self.bench.tab(id).is_some_and(|t| t.needs_you);
            crate::screens::agent::asked(self, id, clock);
            changed |= self.bench.tab(id).is_some_and(|t| t.needs_you) != before;
        }
        // Agents: who works, who went quiet, and the card of the one being answered.
        let agents = crate::screens::agent::poll(self, clock, tx);
        // Tor that takes too long to start says so.
        let tor = crate::screens::tor::tick(self, clock, tx);
        // Toasts whose time is up (`notes.rs`), and the tray's list of who needs you.
        let notes = super::notes::tick(self, clock, tx);
        self.sync_waiting();
        changed || ticked || agents || winked || notes || tor || pictured
    }

    /// A pane's burst is held until `at` (`Model::set_hold_waker`): poll then, draw nothing now.
    pub fn wake_at(&mut self, id: TabId, at: Instant, tx: &mut Tx) {
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.wake_at(at, tx);
        }
    }
}
