//! More than one window: Shift+click on a server (or its menu's "Open in new window") opens
//! it in a window of its own, as Shift+click on a link does in a browser (0.10's
//! `windows.ts`). Every window is a [`Shell`] of its own, in the same process, on the same UI
//! thread (Rune's `open_window`): one backend, one settings file, one Tor, one tray.
//!
//! ```text
//!   background work ── Remote (routed) ──▶ Hub ─┬─ a tab's answer ─▶ the window whose range the
//!                                               │                    tab's id is in (TabId::window)
//!                                               ├─ news of the app ─▶ every window
//!                                               └─ anything else ───▶ the window in front
//!   window N ── Shared(N) ──▶ Core: the one backend, and what each window asked of it
//! ```
//!
//! - **Ids.** Each window numbers its tabs in a range of its own (`Workbench::for_window`), so
//!   no two windows ever hold the same id and an answer finds its window by the number alone.
//! - **[`Hub`]**: the open windows by number, how to reach each (its own [`Remote`]), which is
//!   in front, and which window opened each file in an editor elsewhere.
//! - **[`Shared`]**: a window's way to the one backend. What only one of each exists of (the
//!   machine whose vital signs are watched, the servers asked about their programs, the tray's
//!   list of who needs you, whether a window is on screen) is merged from every window's.
//!   Another window doesn't start the backend, keep its tabs or its place, or show the welcome.
//! - **Settings** are one file: each window has a store ([`SettingsStore::follower`]) and hands
//!   its changes to the others, which take them ([`SettingsStore::mirror`]) and apply them.
//!   Snippets and Files' bookmarks are handed on the same way.
//!
//! With only the main window open, every path here comes back to it: it runs as it always did.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rune::Tx;
use sshee_app::settings_store::SettingsStore;
use sshee_app::workbench::{SavedTabs, Server, Tab, TabId};

use super::desktop::Waiting;
use super::state::{Backend, Options, Remote, Shell};
use crate::terminal::TermLink;

/// A change for one window, run on the UI thread; true when it must draw again.
pub type Job = Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>;

/// A change for every window: run once in each.
pub type Every = Arc<dyn Fn(&mut Shell, &mut Tx) -> bool + Send + Sync>;

/// The main window's number. Others count up from 1.
pub const MAIN: u16 = 0;

/// The open windows, as background work and the other windows reach them. Shared by every
/// window and the backend's [`Remote`]; any thread.
#[derive(Default)]
pub struct Hub {
    state: Mutex<Doors>,
}

#[derive(Default)]
struct Doors {
    /// Each open window's number and its own way in, in the order they opened.
    open: Vec<(u16, Remote)>,
    /// The window in front (the last to have it), for what answers whoever asked.
    front: u16,
    /// The last number given to a window.
    last: u16,
    /// Files open in an editor elsewhere, by session id: the window that opened each.
    edits: HashMap<String, u16>,
}

impl Hub {
    pub fn new() -> Arc<Hub> {
        Arc::new(Hub::default())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Doors> {
        // Nothing here is left half-changed by a panic: carry on with what there is.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A window opened; `remote` reaches it alone.
    pub fn join(&self, no: u16, remote: Remote) {
        let mut doors = self.lock();
        doors.open.retain(|(n, _)| *n != no);
        doors.open.push((no, remote));
        doors.last = doors.last.max(no);
    }

    /// A window closed: nothing reaches it any more.
    pub fn leave(&self, no: u16) {
        let mut doors = self.lock();
        doors.open.retain(|(n, _)| *n != no);
        doors.edits.retain(|_, n| *n != no);
        if doors.front == no {
            doors.front = doors.open.first().map_or(MAIN, |(n, _)| *n);
        }
    }

    /// The number for a window about to open: never one used before in this run.
    pub fn next_window(&self) -> u16 {
        let mut doors = self.lock();
        doors.last += 1;
        doors.last
    }

    /// The window came to the front.
    pub fn set_front(&self, no: u16) {
        let mut doors = self.lock();
        if doors.open.iter().any(|(n, _)| *n == no) {
            doors.front = no;
        }
    }

    pub fn front(&self) -> u16 {
        self.lock().front
    }

    /// How many windows are open.
    pub fn count(&self) -> usize {
        self.lock().open.len()
    }

    /// The numbers of the open windows, in the order they opened.
    pub fn windows(&self) -> Vec<u16> {
        self.lock().open.iter().map(|(n, _)| *n).collect()
    }

    pub(crate) fn door(&self, no: u16) -> Option<Remote> {
        self.lock().open.iter().find(|(n, _)| *n == no).map(|(_, r)| r.clone())
    }

    /// The window in front; the first open one when that one is gone.
    pub(crate) fn front_door(&self) -> Option<Remote> {
        let doors = self.lock();
        doors.open.iter().find(|(n, _)| *n == doors.front).or(doors.open.first()).map(|(_, r)| r.clone())
    }

    pub(crate) fn doors(&self) -> Vec<(u16, Remote)> {
        self.lock().open.clone()
    }

    /// A file opened in an editor elsewhere from window `no`: its news goes there.
    pub(crate) fn note_edit(&self, id: String, no: u16) {
        self.lock().edits.insert(id, no);
    }

    pub(crate) fn edit_owner(&self, id: &str) -> Option<u16> {
        self.lock().edits.get(id).copied()
    }

    /// Settings changed in window `from`: the other windows take them (each compares, so the
    /// one that has them already stops there). Nothing to do with one window.
    pub(crate) fn settings_changed(&self, from: u16, settings: &sshee_app::model::Settings, more: &sshee_app::settings_store::Additions) {
        let others: Vec<Remote> = self.lock().open.iter().filter(|(n, _)| *n != from).map(|(_, r)| r.clone()).collect();
        for other in others {
            let (settings, more) = (settings.clone(), more.clone());
            other.update(move |s| s.mirror_settings(settings, more));
        }
    }

    /// Every window but `from` runs `f` (a list window `from` changed and kept).
    pub(crate) fn to_others(&self, from: u16, f: Every) {
        for (n, door) in self.doors() {
            if n != from {
                let f = f.clone();
                door.post_job(Box::new(move |s, tx| f(s, tx)));
            }
        }
    }
}

/// What one window last asked of the backend that there is only one of.
#[derive(Clone)]
struct Seen {
    watch: Option<Server>,
    agents: Vec<Server>,
    waiting: Vec<Waiting>,
    shown: bool,
}

impl Default for Seen {
    fn default() -> Seen {
        Seen { watch: None, agents: Vec::new(), waiting: Vec::new(), shown: true }
    }
}

/// The program's one backend, and what each window asked of it.
struct Core {
    backend: Box<dyn Backend>,
    windows: BTreeMap<u16, Seen>,
    /// The window whose machine the backend was last told to watch.
    watcher: u16,
}

impl Core {
    fn seen(&mut self, no: u16) -> &mut Seen {
        self.windows.entry(no).or_default()
    }

    /// Every window's servers asked about their programs, each once, in the windows' order.
    fn agents(&self) -> Vec<Server> {
        let mut all: Vec<Server> = Vec::new();
        for server in self.windows.values().flat_map(|w| w.agents.iter()) {
            if !all.iter().any(|s| s.key() == server.key()) {
                all.push(server.clone());
            }
        }
        all
    }

    fn waiting(&self) -> Vec<Waiting> {
        self.windows.values().flat_map(|w| w.waiting.iter().cloned()).collect()
    }

    fn shown(&self) -> bool {
        self.windows.values().any(|w| w.shown)
    }
}

/// A window's way to the program's one backend ([`Backend`] itself, so the window calls it as
/// it always did).
pub struct Shared {
    core: Rc<RefCell<Core>>,
    no: u16,
    hub: Arc<Hub>,
    /// Another window: opened on one server, it doesn't start the backend, keep its tabs or
    /// its place, or show the welcome.
    secondary: bool,
    /// Another window's settings, handed to it once as it starts.
    settings: Option<SettingsStore>,
}

impl Shared {
    /// The main window's: the backend is the program's.
    pub fn main(backend: Box<dyn Backend>, hub: Arc<Hub>) -> Shared {
        let core = Core { backend, windows: BTreeMap::from([(MAIN, Seen::default())]), watcher: MAIN };
        Shared { core: Rc::new(RefCell::new(core)), no: MAIN, hub, secondary: false, settings: None }
    }

    /// Window `no`'s, opened from this one, with its settings.
    pub fn another(&self, no: u16, settings: SettingsStore) -> Shared {
        self.core.borrow_mut().windows.insert(no, Seen::default());
        Shared { core: self.core.clone(), no, hub: self.hub.clone(), secondary: true, settings: Some(settings) }
    }

    pub fn window(&self) -> u16 {
        self.no
    }

    pub fn is_secondary(&self) -> bool {
        self.secondary
    }

    pub fn hub(&self) -> &Arc<Hub> {
        &self.hub
    }

    /// The window came to the front: with more than one open, the machine it shows is the one
    /// watched (the others' still come with their programs: [`Backend::agents_on`]).
    pub fn came_to_front(&mut self) {
        self.hub.set_front(self.no);
        let mut core = self.core.borrow_mut();
        if core.windows.len() < 2 || core.watcher == self.no {
            return;
        }
        core.watcher = self.no;
        let watch = core.seen(self.no).watch.clone();
        core.backend.watch(watch.as_ref());
    }

    /// The window closed: what it asked of the backend is taken back, and what is merged from
    /// every window is told again without it.
    pub fn leave(&mut self) {
        self.hub.leave(self.no);
        let mut core = self.core.borrow_mut();
        if core.windows.remove(&self.no).is_none() || core.windows.is_empty() {
            return;
        }
        if core.watcher == self.no {
            let (no, seen) = core.windows.iter().next().map(|(n, s)| (*n, s.watch.clone())).unwrap_or_default();
            core.watcher = no;
            core.backend.watch(seen.as_ref());
        }
        let (agents, waiting, shown) = (core.agents(), core.waiting(), core.shown());
        core.backend.agents_on(&agents);
        core.backend.waiting_changed(&waiting);
        core.backend.window_shown(shown);
    }

    fn backend(&self) -> std::cell::RefMut<'_, Box<dyn Backend>> {
        std::cell::RefMut::map(self.core.borrow_mut(), |c| &mut c.backend)
    }
}

impl Backend for Shared {
    fn start(&mut self, remote: Remote) {
        if !self.secondary {
            self.backend().start(remote);
        }
    }

    fn terminal(&mut self, server: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        self.backend().terminal(server, tab)
    }

    fn watch(&mut self, server: Option<&Server>) {
        let mut core = self.core.borrow_mut();
        core.seen(self.no).watch = server.cloned();
        core.watcher = self.no;
        core.backend.watch(server);
    }

    fn tabs_changed(&mut self, saved: &SavedTabs) {
        // Another window's tabs are its own, for as long as it's open (0.10 kept them in the
        // page's session storage): only the main window's are kept for the next run.
        if !self.secondary {
            self.backend().tabs_changed(saved);
        }
    }

    fn agents_on(&mut self, servers: &[Server]) {
        let mut core = self.core.borrow_mut();
        core.seen(self.no).agents = servers.to_vec();
        let all = core.agents();
        core.backend.agents_on(&all);
    }

    fn notify(&mut self, tab: &Tab, note: &sshee_term::attention::Notification) {
        self.backend().notify(tab, note);
    }

    fn waiting_changed(&mut self, waiting: &[Waiting]) {
        let mut core = self.core.borrow_mut();
        core.seen(self.no).waiting = waiting.to_vec();
        let all = core.waiting();
        core.backend.waiting_changed(&all);
    }

    fn window_shown(&mut self, shown: bool) {
        let mut core = self.core.borrow_mut();
        core.seen(self.no).shown = shown;
        let any = core.shown();
        core.backend.window_shown(any);
    }

    fn tab_of_link(&self, link: &str) -> Option<TabId> {
        self.core.borrow().backend.tab_of_link(link)
    }

    fn files(&mut self, ask: sshee_app::files::Ask) {
        self.backend().files(ask);
    }

    fn folder_under_pointer(&mut self) -> Option<std::path::PathBuf> {
        self.backend().folder_under_pointer()
    }

    fn containers(&mut self, ask: sshee_app::containers::Ask) {
        self.backend().containers(ask);
    }

    fn stats(&mut self, ask: sshee_app::stats::Ask) {
        self.backend().stats(ask);
    }

    fn edit(&mut self, ask: sshee_app::files::edit::EditAsk) {
        self.backend().edit(ask);
    }

    fn settings(&mut self) -> SettingsStore {
        match self.settings.take() {
            Some(settings) => settings,
            None => self.backend().settings(),
        }
    }

    fn settings_changed(&mut self, settings: &sshee_app::model::Settings) {
        self.backend().settings_changed(settings);
    }

    fn tor(&mut self, command: crate::screens::settings::TorCommand) {
        self.backend().tor(command);
    }

    fn tor_ask(&mut self, ask: sshee_app::tor::Ask) {
        self.backend().tor_ask(ask);
    }

    fn tunnels(&mut self, ask: sshee_app::tunnels::Ask) {
        // The list kept is one: the other windows take this one's, as they took it at the start.
        if let sshee_app::tunnels::Ask::Save(list) = &ask {
            let list = list.clone();
            self.hub.to_others(
                self.no,
                Arc::new(move |s, _| {
                    s.screens.tunnels.tunnels.deliver(sshee_app::tunnels::Reply::Loaded(list.clone()));
                    true
                }),
            );
        }
        self.backend().tunnels(ask);
    }

    fn port_problem(&mut self, port: u16) -> Option<String> {
        self.backend().port_problem(port)
    }

    fn free_port(&mut self, preferred: u16, avoid: &[u16]) -> Result<u16, String> {
        self.backend().free_port(preferred, avoid)
    }

    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        self.backend().servers(ask);
    }

    fn restart_to_update(&mut self) -> Result<(), String> {
        self.backend().restart_to_update()
    }

    fn recipes(&mut self, ask: sshee_app::recipes::Ask) {
        self.backend().recipes(ask);
    }

    fn disk(&mut self, ask: sshee_app::disk::Ask) {
        self.backend().disk(ask);
    }

    fn bookmarks(&mut self) -> Option<sshee_app::model::Bookmarks> {
        self.backend().bookmarks()
    }

    fn bookmarks_changed(&mut self, b: &sshee_app::model::Bookmarks) {
        self.backend().bookmarks_changed(b);
        let b = b.clone();
        self.hub.to_others(
            self.no,
            Arc::new(move |s, _| {
                s.screens.bookmarks = b.clone();
                true
            }),
        );
    }

    fn snippets(&mut self) -> Option<sshee_app::model::Snippets> {
        self.backend().snippets()
    }

    fn snippets_changed(&mut self, list: &sshee_app::model::Snippets) {
        self.backend().snippets_changed(list);
        let list = list.clone();
        self.hub.to_others(
            self.no,
            Arc::new(move |s, _| {
                s.screens.snippets.list = list.clone();
                true
            }),
        );
    }

    fn welcome(&mut self) -> bool {
        !self.secondary && self.backend().welcome()
    }

    fn version(&mut self) -> Option<String> {
        self.backend().version()
    }

    fn window_placed(&mut self, place: &sshee_app::windows::Placement) {
        // The main window's place is the one kept: another opens where the system puts it.
        if !self.secondary {
            self.backend().window_placed(place);
        }
    }

    fn replays(&self) -> bool {
        self.core.borrow().backend.replays()
    }
}

impl Shell {
    /// Shift+click on a server, or its menu's "Open in new window": it opens in a window of
    /// its own. Where no window can open (a headless run) it opens here.
    pub fn open_in_new_window(&mut self, server: &str, tx: &mut Tx) {
        self.screens.server_card.hide();
        self.screens.server_menu.open = None;
        if self.bench.server(server).is_none() {
            return;
        }
        if !super::open_window(self, server) {
            self.go_to(server, tx);
        }
    }

    /// Another window, opened from this one on `server`: number `no`, reached through
    /// `remote` (its own). It has this window's servers and what is known of them, its
    /// settings, snippets and bookmarks, and one tab: a terminal on `server`.
    pub(crate) fn window_for(&mut self, no: u16, server: &str, remote: Remote) -> Shell {
        self.hub().join(no, remote.clone());
        let mut bench = self.bench.for_window(no);
        bench.go_to(server);
        let backend = self.shared().another(no, self.settings.follower());
        // Its own place: where the system puts a new window.
        let options = Options { place: None, ..self.options.clone() };
        let mut shell = Shell::build(bench, backend, options, Some(remote));
        shell.inherit(self);
        shell
    }

    /// What a window opened from `from` knows from the start, as `from` learnt it since the
    /// program started: the servers' looks and pictures, this computer's name, Tor, the
    /// tunnels, the looks at the machines, the editors found, your snippets and bookmarks.
    fn inherit(&mut self, from: &Shell) {
        let home = &from.screens.home;
        self.set_server_looks(home.colors.clone(), home.config_new, home.old_new);
        self.set_server_faces(from.screens.server_menu.faces.clone());
        if let Some(host) = from.streamer.local_host.clone() {
            self.learn_local_host(host);
        }
        self.screens.tor.detail = from.screens.tor.detail.clone();
        self.screens.tor.follow = from.screens.tor.follow.clone();
        self.screens.tunnels.tunnels = from.screens.tunnels.tunnels.clone();
        self.screens.machines = from.screens.machines.clone();
        self.screens.edits.found = from.screens.edits.found.clone();
        self.screens.edits.custom = from.screens.edits.custom.clone();
        self.screens.snippets.list = from.screens.snippets.list.clone();
        self.screens.bookmarks = from.screens.bookmarks.clone();
        crate::screens::files::edit::hosts_changed(self);
        self.apply_settings();
    }

    /// Settings changed in another window: this one takes them and applies them (what the
    /// terminals show, at the next frame).
    pub fn mirror_settings(&mut self, settings: sshee_app::model::Settings, more: sshee_app::settings_store::Additions) {
        if self.settings.mirror(settings, more) {
            self.apply_settings();
            self.look_stale = true;
        }
    }

    /// No other window is open: what ends with the program ends with this one.
    pub(crate) fn is_last_window(&self) -> bool {
        self.hub().windows().iter().all(|n| *n == self.window_no())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rune::Tx;
    use sshee_app::workbench::{Server, Tab, TabId, TabKind, Workbench};
    use sshee_term::model::Model;

    use super::Job;
    use crate::shell::desktop::Waiting;
    use crate::shell::state::{Backend, Options, Remote, Shell};
    use crate::terminal::{LinkStatus, StatusSink, TermLink};

    const WEB: &str = "host:web";
    const DB: &str = "host:db";

    /// What the one backend was asked, by every window.
    #[derive(Default)]
    struct World {
        remote: Option<Remote>,
        started: usize,
        /// Each terminal asked for: its tab, and whether its shell was ended.
        terminals: Vec<(TabId, Arc<Mutex<bool>>)>,
        watching: Vec<Option<String>>,
        agents: Vec<Vec<String>>,
        waiting: Vec<Vec<TabId>>,
        tabs_kept: usize,
        placed: usize,
        snippets_kept: usize,
    }

    struct Link(Arc<Mutex<bool>>);

    impl TermLink for Link {
        fn open(&mut self, _: Arc<Model>, _: sshee_term::engine::Size, status: StatusSink) {
            status(LinkStatus::Connected { durable: true });
        }
        fn write(&self, _: &[u8]) {}
        fn resize(&self, _: sshee_term::engine::Size) {}
        fn reconnect(&self) {}
        fn close(&self) {}
        fn end(&self) {
            *self.0.lock().unwrap() = true;
        }
    }

    struct Fake(Arc<Mutex<World>>);

    impl Backend for Fake {
        fn start(&mut self, remote: Remote) {
            let mut w = self.0.lock().unwrap();
            w.remote = Some(remote);
            w.started += 1;
        }
        fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
            let ended = Arc::new(Mutex::new(false));
            self.0.lock().unwrap().terminals.push((tab.id, ended.clone()));
            Box::new(Link(ended))
        }
        fn watch(&mut self, server: Option<&Server>) {
            self.0.lock().unwrap().watching.push(server.map(Server::key));
        }
        fn agents_on(&mut self, servers: &[Server]) {
            self.0.lock().unwrap().agents.push(servers.iter().map(Server::key).collect());
        }
        fn waiting_changed(&mut self, waiting: &[Waiting]) {
            self.0.lock().unwrap().waiting.push(waiting.iter().map(|w| w.tab).collect());
        }
        fn tabs_changed(&mut self, _: &sshee_app::workbench::SavedTabs) {
            self.0.lock().unwrap().tabs_kept += 1;
        }
        fn window_placed(&mut self, _: &sshee_app::windows::Placement) {
            self.0.lock().unwrap().placed += 1;
        }
        fn snippets_changed(&mut self, _: &sshee_app::model::Snippets) {
            self.0.lock().unwrap().snippets_kept += 1;
        }
        fn welcome(&mut self) -> bool {
            true
        }
        fn replays(&self) -> bool {
            true
        }
    }

    type Queue = Arc<Mutex<Vec<Job>>>;

    /// A window's own `Remote`, and what reached it.
    fn door() -> (Remote, Queue) {
        let queue: Queue = Arc::default();
        let q = queue.clone();
        (Remote::new(Arc::new(move |job| q.lock().unwrap().push(job))), queue)
    }

    /// Runs what reached a window; how many there were.
    fn drain(queue: &Queue, s: &mut Shell) -> usize {
        let jobs: Vec<Job> = std::mem::take(&mut *queue.lock().unwrap());
        let n = jobs.len();
        for job in jobs {
            job(s, &mut Tx::default());
        }
        n
    }

    struct Two {
        world: Arc<Mutex<World>>,
        main: Shell,
        main_q: Queue,
        second: Shell,
        second_q: Queue,
    }

    impl Two {
        /// The main window on web-7731, and another opened on db-main.
        fn open() -> Two {
            let world = Arc::new(Mutex::new(World::default()));
            let (remote, main_q) = door();
            let servers = vec![Server::host("web", "web-7731", "deploy@203.0.113.17"), Server::host("db", "db-main", "postgres@203.0.113.31")];
            let mut main = Shell::new(Workbench::new(servers), Box::new(Fake(world.clone())), Options::default(), Some(remote));
            main.go_to(WEB, &mut Tx::default());
            let (remote, second_q) = door();
            let no = main.hub().next_window();
            let second = main.window_for(no, DB, remote);
            Two { world, main, main_q, second, second_q }
        }

        fn backend_remote(&self) -> Remote {
            self.world.lock().unwrap().remote.clone().unwrap()
        }

        /// Runs what reached each window until nothing more comes.
        fn settle(&mut self) {
            for _ in 0..10 {
                if drain(&self.main_q, &mut self.main) + drain(&self.second_q, &mut self.second) == 0 {
                    return;
                }
            }
            panic!("the windows kept telling each other");
        }
    }

    #[test]
    fn another_window_opens_on_its_server_and_its_tab_ids_are_its_own() {
        let mut t = Two::open();
        assert_eq!(t.second.window_no(), 1);
        assert!(t.second.is_secondary() && !t.main.is_secondary());
        let tab = t.second.bench.active().unwrap().clone();
        assert_eq!((tab.server.as_str(), tab.kind), (DB, TabKind::Terminal));
        // Its terminal was asked of the same backend, which started once.
        let w = t.world.lock().unwrap();
        assert_eq!(w.started, 1);
        assert!(w.terminals.iter().any(|(id, _)| *id == tab.id));
        drop(w);
        // More tabs in both: never one id twice.
        t.main.new_terminal(&mut Tx::default());
        t.second.new_terminal(&mut Tx::default());
        t.second.open_files(&mut Tx::default());
        let ids: Vec<TabId> = t.main.bench.tabs().iter().chain(t.second.bench.tabs()).map(|t| t.id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");
        assert!(t.main.bench.tabs().iter().all(|t| t.id.window() == 0));
        assert!(t.second.bench.tabs().iter().all(|t| t.id.window() == 1));
        // The servers are the main window's.
        assert_eq!(t.second.bench.servers(), t.main.bench.servers());
    }

    #[test]
    fn an_answer_for_a_tab_reaches_the_window_that_has_it() {
        let mut t = Two::open();
        t.settle();
        let remote = t.backend_remote();
        let (mine, theirs) = (t.main.bench.active_id().unwrap(), t.second.bench.active_id().unwrap());
        remote.through_tor(theirs, false);
        assert_eq!((t.main_q.lock().unwrap().len(), t.second_q.lock().unwrap().len()), (0, 1));
        t.settle();
        assert!(t.second.screens.tor.direct.contains(&theirs) && t.main.screens.tor.direct.is_empty());
        remote.through_tor(mine, false);
        assert_eq!((t.main_q.lock().unwrap().len(), t.second_q.lock().unwrap().len()), (1, 0));
        t.settle();
        // The snippet form's look is addressed in the window's own range too.
        let reply = |tab| sshee_app::containers::Reply { tab, server: DB.into(), answer: sshee_app::containers::Answer::Listed { ticket: 1, result: Ok(Default::default()) } };
        remote.containers(reply(sshee_app::snippets::form_tab(1)));
        assert_eq!((t.main_q.lock().unwrap().len(), t.second_q.lock().unwrap().len()), (0, 1));
        t.settle();
        // News of the app reaches both; a notification's click, the window with the tab.
        remote.set_tor(sshee_app::workbench::Tor::Starting);
        assert_eq!((t.main_q.lock().unwrap().len(), t.second_q.lock().unwrap().len()), (1, 1));
        t.settle();
        assert_eq!((t.main.bench.tor(), t.second.bench.tor()), (sshee_app::workbench::Tor::Starting, sshee_app::workbench::Tor::Starting));
        remote.open_tab(theirs, None);
        assert_eq!((t.main_q.lock().unwrap().len(), t.second_q.lock().unwrap().len()), (0, 1));
        t.settle();
        // Anything else goes to the window in front.
        remote.update(|s| s.look_stale = true);
        assert_eq!(t.main_q.lock().unwrap().len(), 1);
        t.settle();
        t.second.window_focus(true, &mut Tx::default());
        remote.update(|s| s.look_stale = true);
        assert_eq!((t.main_q.lock().unwrap().len(), t.second_q.lock().unwrap().len()), (0, 1));
        t.settle();
        // A window that closed hears nothing more.
        t.second.close();
        remote.through_tor(theirs, true);
        assert!(t.second_q.lock().unwrap().is_empty() && t.main_q.lock().unwrap().is_empty());
    }

    #[test]
    fn a_setting_changed_in_one_window_is_the_others_too() {
        let mut t = Two::open();
        t.settle();
        t.second.change_settings(&mut Tx::default(), |st| st.set(|v| v.terminal_font_size = 17.0));
        t.settle();
        assert_eq!(t.main.settings.get().terminal_font_size, 17.0);
        // And applied: the terminals use it.
        assert_eq!(t.main.options.pane.font_size, t.second.options.pane.font_size);
        t.main.change_settings(&mut Tx::default(), |st| st.set(|v| v.cursor_blink = false));
        t.settle();
        assert!(!t.second.settings.get().cursor_blink);
        // Snippets saved in one window are the other's list too, and kept once.
        let mut list = t.second.screens.snippets.list.clone();
        list.snippets.truncate(1);
        t.second.backend_mut().snippets_changed(&list);
        t.settle();
        assert_eq!(t.main.screens.snippets.list, list);
        assert_eq!(t.world.lock().unwrap().snippets_kept, 1);
    }

    #[test]
    fn another_window_keeps_no_tabs_and_no_place_and_shows_no_welcome() {
        let mut t = Two::open();
        let kept = t.world.lock().unwrap().tabs_kept;
        assert!(kept > 0, "the main window keeps its tabs");
        t.second.new_terminal(&mut Tx::default());
        t.second.close_active(&mut Tx::default());
        t.second.close();
        let w = t.world.lock().unwrap();
        assert_eq!((w.tabs_kept, w.placed), (kept, 0));
        drop(w);
        assert!(!t.second.screens.welcome.open);
        assert!(!t.second.backend_mut().welcome() && t.main.backend_mut().welcome());
    }

    #[test]
    fn closing_another_window_ends_its_shells_and_leaves_the_main_windows() {
        let mut t = Two::open();
        let (mine, theirs) = (t.main.bench.active_id().unwrap(), t.second.bench.active_id().unwrap());
        // Both servers are watched for their programs while both windows show them.
        let (web, db) = (t.main.bench.servers()[1].clone(), t.main.bench.servers()[2].clone());
        t.main.backend_mut().agents_on(&[web]);
        t.second.backend_mut().agents_on(&[db]);
        assert_eq!(t.world.lock().unwrap().agents.last().unwrap(), &[WEB.to_owned(), DB.to_owned()]);
        t.second.close();
        let w = t.world.lock().unwrap();
        let ended = |id: TabId| w.terminals.iter().filter(|(t, _)| *t == id).any(|(_, e)| *e.lock().unwrap());
        assert!(ended(theirs) && !ended(mine));
        // What it asked of the backend is taken back.
        assert_eq!(w.agents.last().unwrap(), &[WEB.to_owned()]);
        drop(w);
        assert_eq!(t.main.hub().windows(), [0]);
        assert!(t.main.is_last_window());
    }

    /// The names of the `fn`s in the block that starts at `head`, up to its closing brace.
    fn fns_of(source: &str, head: &str) -> Vec<String> {
        let block = source.split_once(head).map(|(_, rest)| rest).unwrap_or_else(|| panic!("no {head}"));
        let block = &block[..block.find("\n}\n").unwrap_or(block.len())];
        block.lines().filter_map(|l| l.trim().strip_prefix("fn ")).map(|f| f.split(['(', '<']).next().unwrap_or_default().to_owned()).collect()
    }

    /// A request `Shared` doesn't pass on falls to the trait's default, which drops it.
    #[test]
    fn every_window_passes_every_request_to_the_backend() {
        let wanted = fns_of(include_str!("state.rs"), "pub trait Backend");
        let passed = fns_of(include_str!("windows.rs"), "impl Backend for Shared {");
        let missing: Vec<_> = wanted.iter().filter(|f| !passed.contains(f)).collect();
        assert!(wanted.len() > 20, "{wanted:?}");
        assert!(missing.is_empty(), "Shared doesn't pass these on: {missing:?}");
    }
}
