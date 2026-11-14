//! The window without a screen (`Runtime::with_text`): fake sessions, real frames, clicks and
//! keys, and what the tabs, the sessions and the screen reader's tree hold afterwards.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, PointerButton, Runtime, Tx};
use sshee_app::model::EnvLabel;
use sshee_app::workbench::{Badge, Link, Server, Tab, TabKind, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use super::state::{Backend, Options, Post, Remote, Shell};
use super::view::view;
use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

const WEB: &str = "host:web";
const DB: &str = "host:db";

/// What one fake session saw.
#[derive(Default)]
struct Seen {
    model: Option<Arc<Model>>,
    size: Option<Size>,
    written: Vec<u8>,
    status: Option<StatusSink>,
    closed: bool,
    ended: bool,
}

struct FakeLink(Arc<Mutex<Seen>>);

impl TermLink for FakeLink {
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
        let mut seen = self.0.lock().unwrap();
        model.connected(false, 1_000);
        seen.model = Some(model);
        seen.size = Some(size);
        status(LinkStatus::Connected { durable: false });
        seen.status = Some(status);
    }
    fn write(&self, bytes: &[u8]) {
        self.0.lock().unwrap().written.extend_from_slice(bytes);
    }
    fn resize(&self, size: Size) {
        self.0.lock().unwrap().size = Some(size);
    }
    fn reconnect(&self) {
        self.0.lock().unwrap().written.extend_from_slice(b"<reconnect>");
    }
    fn close(&self) {
        self.0.lock().unwrap().closed = true;
    }
    fn end(&self) {
        self.0.lock().unwrap().ended = true;
    }
}

/// Every session the window asked for, by its tab's place in the order they were made, and
/// the machine the window last asked to watch.
#[derive(Default)]
struct World {
    sessions: Vec<(String, Arc<Mutex<Seen>>)>,
    watching: Option<String>,
}

struct FakeBackend(Arc<Mutex<World>>);

impl Backend for FakeBackend {
    fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        self.0.lock().unwrap().sessions.push((tab.server.clone(), seen.clone()));
        Box::new(FakeLink(seen))
    }
    fn watch(&mut self, server: Option<&Server>) {
        self.0.lock().unwrap().watching = server.map(Server::key);
    }
    fn replays(&self) -> bool {
        true
    }
}

type Queue = Arc<Mutex<Vec<Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>>>>;

struct Window {
    rt: Runtime<Shell>,
    world: Arc<Mutex<World>>,
    /// What sessions posted from "their threads", until [`Window::settle`] runs it.
    posted: Queue,
}

fn servers() -> Vec<Server> {
    vec![
        Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
        Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Prod).icon("database"),
    ]
}

impl Window {
    fn open(frame: Frame, mac_keys: bool) -> Window {
        crate::fonts::declare();
        let world = Arc::new(Mutex::new(World::default()));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame, mac_keys, ..Options::default() };
        let shell = Shell::new(Workbench::new(servers()), Box::new(FakeBackend(world.clone())), options, Some(Remote::new(post)));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, world, posted };
        w.settle();
        w
    }

    /// A few frames, with what sessions posted run in between, as the window's loop does.
    fn settle(&mut self) {
        for _ in 0..4 {
            let posted: Vec<_> = std::mem::take(&mut *self.posted.lock().unwrap());
            if !posted.is_empty() {
                self.rt.update(|s, tx| {
                    for f in posted {
                        f(s, tx);
                    }
                });
            }
            self.rt.frame(1.0 / 60.0);
        }
    }

    fn click(&mut self, key: &str) {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    fn key(&mut self, key: Key, held: &str) {
        let ctrl = held.contains('c');
        let logo = held.contains('m');
        let mods = Mods { shift: held.contains('s'), alt: held.contains('a'), ctrl, logo, cmd: if cfg!(target_os = "macos") { logo } else { ctrl } };
        self.rt.key(KeyEvent { key, mods });
        self.settle();
    }

    fn session(&self, n: usize) -> Arc<Mutex<Seen>> {
        self.world.lock().unwrap().sessions[n].1.clone()
    }

    fn sessions(&self) -> usize {
        self.world.lock().unwrap().sessions.len()
    }

    fn written(&self, n: usize) -> String {
        String::from_utf8_lossy(&self.session(n).lock().unwrap().written).into_owned()
    }

    /// Output from session `n`'s program.
    fn feed(&mut self, n: usize, bytes: &[u8]) {
        let model = self.session(n).lock().unwrap().model.clone().expect("the session opened");
        // On the panes' own clock (virtual here), so a busy machine can't make a frame come
        // "before" its output.
        let pane = self.rt.state.panes.values().find(|p| Arc::ptr_eq(&p.model, &model));
        match pane.map(|p| p.now()) {
            Some(now) => model.feed_at(bytes, now),
            None => model.feed(bytes),
        }
        // The model's gate holds a first burst for a moment.
        for _ in 0..12 {
            self.rt.frame(1.0 / 60.0);
        }
    }

    /// Where an element is drawn: `[x, y, width, height]` in window points.
    fn rect(&self, key: &str) -> Option<[f32; 4]> {
        self.rt.presented(key).map(|(rect, _)| rect)
    }

    fn row(&self) -> String {
        let b = &self.rt.state.bench;
        b.strip().iter().map(|s| format!("{}{}@{}", if s.active { "*" } else { "" }, s.tab.label(), s.server_name)).collect::<Vec<_>>().join(" ")
    }

    /// Every name and value a screen reader gets.
    fn tree(&self) -> String {
        let tree = self.rt.accessibility_tree();
        let mut out = String::new();
        for (_, node) in &tree.nodes {
            for t in [node.value(), node.label()].into_iter().flatten() {
                out.push_str(t);
                out.push('\n');
            }
        }
        out
    }
}

fn linux() -> Window {
    Window::open(Frame::Linux, false)
}

#[test]
fn it_starts_on_the_main_menu_with_the_servers_in_the_sidebar() {
    let w = linux();
    assert!(w.rt.state.bench.showing_home());
    assert_eq!(w.sessions(), 0);
    let tree = w.tree();
    for name in ["This computer", "web-7731", "db-main", "Search", "Settings", "Tor", "Security check", "New terminal", "Main menu"] {
        assert!(tree.contains(name), "{name} isn't named in: {tree}");
    }
    // The main menu is the page: its search field and its tiles.
    assert!(tree.contains("Search servers, files, commands and settings") && tree.contains("Add a server"), "{tree}");
    for key in ["server-0", "server-1", "server-2", "new-tab", "sidebar", "home", "rail", "status"] {
        assert!(w.rt.find(key).is_some(), "{key}");
    }
}

#[test]
fn a_click_on_a_server_opens_its_terminal_at_the_size_the_page_holds() {
    let mut w = linux();
    w.click("server-1");
    assert_eq!(w.row(), "*Terminal@web-7731");
    assert_eq!(w.sessions(), 1);
    // 1280 × 800: the page is 1228 × 732 points under a 44-point bar, beside a 52-point
    // sidebar, over a 24-point status bar; less 12 points of padding each side, in cells of
    // 7 × 21.
    assert_eq!(w.session(0).lock().unwrap().size, Some(Size::new(172, 33)));
    // The grid starts on a whole pixel at every scale the terminal is tested at.
    let [x, y, ..] = w.rect("term").expect("the terminal is laid out");
    assert_eq!((x, y), (52.0, 44.0));
    // Its session said it's connected: the tab says nothing after its name.
    assert_eq!(w.rt.state.bench.active().unwrap().link, Link::Connected { durable: false });
    // A second click comes back to the same tab instead of opening another.
    w.click("server-2");
    w.click("server-1");
    assert_eq!(w.row(), "*Terminal@web-7731 Terminal@db-main");
    assert_eq!(w.sessions(), 2);
}

#[test]
fn keys_reach_the_terminal_on_screen_and_only_that_one() {
    let mut w = linux();
    w.click("server-1");
    w.key(Key::Text("l".into()), "");
    w.key(Key::Text("s".into()), "");
    w.key(Key::Enter, "");
    w.click("server-2");
    w.key(Key::Text("x".into()), "");
    assert_eq!(w.written(0), "ls\r");
    assert_eq!(w.written(1), "x");
    // The shell's own Ctrl keys stay the terminal's: Ctrl+T, Ctrl+W, Ctrl+B.
    w.key(Key::Command('t'), "c");
    w.key(Key::Command('w'), "c");
    w.key(Key::Command('b'), "c");
    assert_eq!(w.written(1), "x\u{14}\u{17}\u{2}");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal@db-main");
}

#[test]
fn a_hidden_tab_keeps_its_session_and_its_output() {
    let mut w = linux();
    w.click("server-1");
    w.feed(0, b"web-7731:~$ ");
    assert!(w.tree().contains("web-7731:~$"));
    w.click("server-2");
    assert!(!w.tree().contains("web-7731:~$"), "the hidden terminal is still read out");
    // Output arrives while it's hidden; nothing was closed.
    w.feed(0, b"tail -f app.log\r\nline 1\r\n");
    assert!(!w.session(0).lock().unwrap().closed && !w.session(0).lock().unwrap().ended);
    // Back: the same session, with what came meanwhile on screen at once.
    let first = w.rt.state.bench.tabs()[0].id;
    w.click(&format!("tab-{}", first.0));
    assert_eq!(w.sessions(), 2);
    let tree = w.tree();
    assert!(tree.contains("tail -f app.log") && tree.contains("line 1"), "{tree}");
    w.key(Key::Text("q".into()), "");
    assert_eq!(w.written(0), "q");
}

#[test]
fn plus_and_its_shortcut_open_a_terminal_on_the_machine_on_screen() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 2@web-7731");
    w.key(Key::Command('t'), "cs");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal 2@web-7731 *Terminal 3@web-7731");
    assert_eq!(w.sessions(), 3);
    // Nothing of the shortcut reached a shell.
    assert!((0..3).all(|n| w.written(n).is_empty()));
}

#[test]
fn on_a_mac_the_shortcuts_are_commands() {
    let mut w = Window::open(Frame::Mac, true);
    w.click("server-1");
    w.key(Key::Command('t'), "m");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 2@web-7731");
    w.key(Key::Command('1'), "m");
    assert_eq!(w.row(), "*Terminal@web-7731 Terminal 2@web-7731");
    w.key(Key::Command('w'), "m");
    assert_eq!(w.row(), "*Terminal 2@web-7731");
    w.key(Key::Command('t'), "ms");
    assert_eq!(w.row(), "Terminal 2@web-7731 *Terminal@web-7731");
    // The logo is in the sidebar there, not in the bar.
    assert!(w.rt.find("bar-logo").is_none() && w.rt.find("home").is_some());
}

#[test]
fn tabs_close_by_cross_middle_click_and_shortcut_and_their_sessions_end() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    w.click("new-tab");
    let ids: Vec<u64> = w.rt.state.bench.tabs().iter().map(|t| t.id.0).collect();
    // The shortcut closes the tab on screen (the third); the second comes up.
    w.key(Key::Command('w'), "cs");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 2@web-7731");
    assert!(w.session(2).lock().unwrap().ended);
    // A middle click on the first tab closes it without going to it.
    let (x, y) = w.rt.find(&format!("tab-{}", ids[0])).unwrap();
    w.rt.button_down(x, y, PointerButton::Middle);
    w.rt.button_up(x, y, PointerButton::Middle);
    w.settle();
    assert_eq!(w.row(), "*Terminal 2@web-7731");
    assert!(w.session(0).lock().unwrap().ended);
    // The cross at the end of the tab you're on closes the last one: the main menu.
    let [x, y, tw, th] = w.rect(&format!("tab-{}", ids[1])).unwrap();
    let (cx, cy) = (x + tw - 12.0, y + th / 2.0);
    w.rt.pointer_down(cx, cy);
    w.rt.frame(1.0 / 60.0);
    w.rt.pointer_up(cx, cy);
    w.settle();
    assert_eq!(w.row(), "");
    assert!(w.rt.state.bench.showing_home());
    assert!(w.session(1).lock().unwrap().ended);
    // Ended, not merely closed: a tab closed on purpose takes its shell on the server with it.
    assert!((0..3).all(|n| !w.session(n).lock().unwrap().closed));
    // And the closed one comes back as a new session.
    w.key(Key::Command('z'), "cs");
    assert_eq!(w.row(), "*Terminal@web-7731");
    assert_eq!(w.sessions(), 4);
}

#[test]
fn ctrl_tab_walks_the_row_and_numbers_jump() {
    let mut w = linux();
    w.click("server-1");
    w.click("server-2");
    w.click("server-0");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal@db-main *Terminal@This computer");
    w.key(Key::Tab, "c");
    assert_eq!(w.row(), "*Terminal@web-7731 Terminal@db-main Terminal@This computer");
    w.key(Key::Tab, "cs");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal@db-main *Terminal@This computer");
    w.key(Key::Command('2'), "cs");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal@db-main Terminal@This computer");
    w.key(Key::Command('9'), "cs");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal@db-main *Terminal@This computer");
    // Plain Tab is the shell's.
    w.key(Key::Tab, "");
    assert_eq!(w.written(2), "\t");
}

#[test]
fn the_sidebar_hides_and_the_terminal_takes_its_room() {
    let mut w = linux();
    w.click("server-1");
    let before = w.session(0).lock().unwrap().size.unwrap();
    w.click("sidebar");
    assert!(w.rt.find("rail").is_none());
    // The resize waits out its debounce.
    for _ in 0..12 {
        w.rt.frame(1.0 / 60.0);
    }
    let after = w.session(0).lock().unwrap().size.unwrap();
    assert_eq!((before.cols, after.cols), (172, 179));
    w.key(Key::Command('b'), "cs");
    assert!(w.rt.find("rail").is_some());
}

#[test]
fn a_tab_says_when_its_connection_drops_and_a_click_tries_again() {
    let mut w = linux();
    w.click("server-1");
    w.click("server-2");
    let status = w.session(0).lock().unwrap().status.clone().unwrap();
    // From the session's thread: reconnecting, then given up.
    status(LinkStatus::Reconnecting { note: "reconnecting (try 1)".into() });
    w.settle();
    let first = w.rt.state.bench.tabs()[0].clone();
    assert_eq!(first.badge(), Some(Badge::Reconnecting));
    assert!(w.tree().contains("Reconnecting…"));
    status(LinkStatus::Waiting { note: "disconnected".into() });
    w.settle();
    assert!(w.tree().contains("Disconnected"));
    w.click(&format!("tab-{}", first.id.0));
    assert_eq!(w.written(0), "<reconnect>");
}

#[test]
fn a_bell_in_a_hidden_tab_marks_it_and_its_server_until_you_look() {
    let mut w = linux();
    w.click("server-1");
    w.click("server-2");
    w.feed(0, b"\x07");
    let web = w.rt.state.bench.tabs()[0].clone();
    assert_eq!(web.badge(), Some(Badge::NeedsYou));
    assert!(w.rt.state.bench.server_needs_you(WEB));
    assert!(w.tree().contains("Needs you"));
    assert!(w.rt.find("waits").is_some(), "the server's icon has no dot");
    // A bell in the tab you're looking at marks nothing.
    w.feed(1, b"\x07");
    assert!(!w.rt.state.bench.server_needs_you(DB));
    // Going there clears it.
    w.click("server-1");
    assert!(!w.rt.state.bench.server_needs_you(WEB));
    assert!(w.rt.find("waits").is_none());
}

#[test]
fn the_machine_on_screen_is_watched_once_its_terminal_is_connected() {
    let mut w = linux();
    assert_eq!(w.world.lock().unwrap().watching, None);
    // This computer needs no connection.
    w.click("server-0");
    assert_eq!(w.world.lock().unwrap().watching.as_deref(), Some("local"));
    // A server: watched because its session connected.
    w.click("server-1");
    assert_eq!(w.world.lock().unwrap().watching.as_deref(), Some(WEB));
    // Its connection drops: nothing is watched (a look must never sign in on its own).
    let status = w.session(1).lock().unwrap().status.clone().unwrap();
    status(LinkStatus::Waiting { note: "disconnected".into() });
    w.settle();
    assert_eq!(w.world.lock().unwrap().watching, None);
    // The main menu watches nothing.
    w.click("home");
    assert!(w.rt.state.bench.showing_home());
    assert_eq!(w.world.lock().unwrap().watching, None);
}

#[test]
fn a_tab_that_is_not_a_terminal_shows_its_placeholder_and_starts_no_session() {
    let mut w = linux();
    w.rt.update(|s, tx| {
        s.bench.open(TabKind::Stats, WEB);
        s.settle(tx);
    });
    w.settle();
    assert_eq!(w.row(), "*Stats@web-7731");
    assert_eq!(w.sessions(), 0);
    assert!(w.tree().contains("This page isn't in the preview yet."));
    assert!(w.rt.find("term").is_none());
}

#[test]
fn a_crowded_row_names_each_server_once() {
    let mut w = linux();
    w.click("server-1");
    for _ in 0..7 {
        w.click("new-tab");
    }
    w.click("server-2");
    w.settle();
    let tree = w.tree();
    // Every tab is still there to click, and the row is inside the window.
    for tab in w.rt.state.bench.tabs() {
        let [x, _, tw, _] = w.rect(&format!("tab-{}", tab.id.0)).expect("a tab was dropped");
        assert!(x >= 0.0 && x + tw <= 1280.0, "tab {} is off the window: {x} + {tw}", tab.id.0);
    }
    assert!(w.rt.find("new-tab").is_some_and(|(x, _)| x < 1280.0));
    assert!(tree.contains("Terminal 8"), "{tree}");
}

#[test]
fn the_window_closing_closes_connections_but_ends_no_shell() {
    let mut w = linux();
    w.click("server-1");
    w.click("server-2");
    w.rt.update(|s, _| s.close());
    for n in 0..2 {
        let seen = w.session(n);
        let seen = seen.lock().unwrap();
        assert!(seen.closed && !seen.ended, "session {n}");
    }
}

/// What came after the first window: find, links, scrolling, the row of tabs, side by side.
mod more;
