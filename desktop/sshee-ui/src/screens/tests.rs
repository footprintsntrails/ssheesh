//! The main menu, the command palette, "Add a server" and a server's menu and card, without a
//! screen (`Runtime::with_text`): fake sessions, a list of servers kept in memory, real
//! frames, clicks and keys, and what the tabs, the sessions, the backend and the screen
//! reader's tree hold afterwards. Nothing here touches a file or a keychain.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, Runtime, Tx};
use sshee_app::home::Spot;
use sshee_app::labels::{Listed, Looks};
use sshee_app::model::EnvLabel;
use sshee_app::servers::{Ask, Book, Candidate, Sources};
use sshee_app::workbench::{Open, Server, Tab, TabKind, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use crate::shell::view::view;
use crate::shell::{Backend, Options, Post, Remote, Shell};
use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

const WEB: &str = "host:web";
const DB: &str = "host:db";

/// What one fake session was sent.
#[derive(Default)]
struct Seen {
    written: Vec<u8>,
}

struct FakeLink(Arc<Mutex<Seen>>);

impl TermLink for FakeLink {
    fn open(&mut self, model: Arc<Model>, _: Size, status: StatusSink) {
        model.connected(false, 1_000);
        status(LinkStatus::Connected { durable: false });
    }
    fn write(&self, bytes: &[u8]) {
        self.0.lock().unwrap().written.extend_from_slice(bytes);
    }
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {}
    fn close(&self) {}
    fn end(&self) {}
}

/// What the window asked of its backend.
#[derive(Default)]
struct World {
    /// Every session, with its tab's machine, in the order they were made.
    sessions: Vec<(String, Arc<Mutex<Seen>>)>,
    asks: Vec<Ask>,
}

/// A backend with fake sessions and the server list in memory.
struct FakeBackend {
    world: Arc<Mutex<World>>,
    book: Book,
    remote: Option<Remote>,
}

impl FakeBackend {
    fn show(&self, answer: Option<sshee_app::servers::Answer>) {
        let Some(remote) = &self.remote else { return };
        let (servers, colors, config_new, old_new) = (self.book.servers(), self.book.colors(), self.book.config_new(), self.book.sources.old.len());
        remote.update(move |s| {
            s.bench.set_servers(servers);
            s.set_server_looks(colors, config_new, old_new);
            if let Some(answer) = answer {
                s.servers_answer(answer);
            }
        });
    }
}

impl Backend for FakeBackend {
    fn start(&mut self, remote: Remote) {
        self.remote = Some(remote);
        self.show(None);
    }
    fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        self.world.lock().unwrap().sessions.push((tab.server.clone(), seen.clone()));
        Box::new(FakeLink(seen))
    }
    fn servers(&mut self, ask: Ask) {
        self.world.lock().unwrap().asks.push(ask.clone());
        let answer = self.book.handle(ask);
        self.show(Some(answer));
    }
    fn replays(&self) -> bool {
        true
    }
}

/// The made-up list: two servers in the sidebar, one from a made-up ssh config not picked
/// yet, one XPipe offers and one 0.10 does.
fn book() -> Book {
    let host = |id: &str, name: &str, address: &str, from_config: bool| Listed { id: id.into(), name: name.into(), address: address.into(), from_config };
    let hosts = vec![host("web", "web-7731", "deploy@203.0.113.17", false), host("db", "db-main", "postgres@203.0.113.31", false), host("cfg:cache-7731", "cache-7731", "deploy@203.0.113.40", true)];
    let mut looks = Looks::default();
    looks.set_icon("web", "globe");
    looks.set_env("web", Some(EnvLabel::Prod));
    looks.set_icon("db", "database");
    let offer = |name: &str, address: &str| Candidate { id: address.into(), name: name.into(), address: address.into(), identity_file: None, had_password: false, picked: true };
    let sources = Sources { xpipe: vec![offer("mail-7731", "root@203.0.113.25")], old: vec![offer("metrics", "ops@203.0.113.60")], ..Sources::default() };
    Book::new(hosts, looks, sources)
}

type Queue = Arc<Mutex<Vec<Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>>>>;

struct Window {
    rt: Runtime<Shell>,
    world: Arc<Mutex<World>>,
    /// What the backend posted from "its threads", until [`Window::settle`] runs it.
    posted: Queue,
}

impl Window {
    fn open(mac_keys: bool) -> Window {
        crate::fonts::declare();
        let world = Arc::new(Mutex::new(World::default()));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: if mac_keys { Frame::Mac } else { Frame::Linux }, mac_keys, ..Options::default() };
        let backend = FakeBackend { world: world.clone(), book: book(), remote: None };
        let shell = Shell::new(Workbench::new(book().servers()), Box::new(backend), options, Some(Remote::new(post)));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, world, posted };
        w.settle();
        w
    }

    /// A few frames, with what the backend posted run in between, as the window's loop does.
    fn settle(&mut self) {
        for _ in 0..5 {
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
        self.rt.pointer_move(x, y);
        self.rt.pointer_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    fn right_click(&mut self, key: &str) {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.secondary_down(x, y);
        self.rt.secondary_up(x, y);
        self.settle();
    }

    fn key(&mut self, key: Key, held: &str) {
        let ctrl = held.contains('c');
        let logo = held.contains('m');
        let mods = Mods { shift: held.contains('s'), alt: held.contains('a'), ctrl, logo, cmd: if cfg!(target_os = "macos") { logo } else { ctrl } };
        self.rt.key(KeyEvent { key, mods });
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Text(c.to_string()), "");
        }
    }

    fn written(&self, n: usize) -> String {
        String::from_utf8_lossy(&self.world.lock().unwrap().sessions[n].1.lock().unwrap().written).into_owned()
    }

    fn sessions(&self) -> usize {
        self.world.lock().unwrap().sessions.len()
    }

    fn asks(&self) -> Vec<Ask> {
        self.world.lock().unwrap().asks.clone()
    }

    fn row(&self) -> String {
        let b = &self.rt.state.bench;
        b.strip().iter().map(|s| format!("{}{}@{}", if s.active { "*" } else { "" }, s.tab.label(), s.server_name)).collect::<Vec<_>>().join(" ")
    }

    fn servers(&self) -> Vec<String> {
        self.rt.state.bench.servers().iter().map(|s| s.name.clone()).collect()
    }

    fn has(&self, key: &str) -> bool {
        self.rt.find(key).is_some()
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

    /// A terminal on web-7731 on screen, an agent's tab waiting on db-main, a closed Files tab.
    fn busy() -> Window {
        let mut w = Window::open(false);
        w.rt.update(|s, tx| {
            s.bench.go_to(WEB);
            let files = s.bench.open_with(TabKind::Files, WEB, Open { path: Some("~/shop/releases".into()), ..Open::default() });
            let agent = s.bench.open_with(TabKind::Terminal, DB, Open { title: Some("Migration 0042".into()), agent: Some("Claude Code".into()), ..Open::default() });
            s.bench.close(files);
            s.bench.go_to(WEB);
            s.bench.set_needs_you(agent, true);
            s.settle(tx);
        });
        w.settle();
        w
    }
}

// ---------------------------------------------------------------- the main menu

#[test]
fn the_main_menu_lists_servers_agents_and_closed_tabs() {
    let mut w = Window::busy();
    w.click("home");
    assert!(w.rt.state.bench.showing_home());
    let tree = w.tree();
    for said in ["This computer", "web-7731", "db-main", "Add a server", "Running now", "Migration 0042", "on db-main", "Needs you", "Recently closed", "on web-7731 · ~/shop/releases"] {
        assert!(tree.contains(said), "{said} isn't on the main menu: {tree}");
    }
    for key in ["home-search", "tile-0", "tile-1", "tile-2", "tile-add", "run-0", "closed-0", "home-config", "home-old"] {
        assert!(w.has(key), "{key}");
    }
    // The servers the ssh config and 0.10 offer are counted, not shown as tiles.
    assert!(!w.has("tile-3"));
    assert!(tree.contains("1 server in ~/.ssh/config to pick from") && tree.contains("1 server from SSHeesh 0.10 to bring over"), "{tree}");
    // A click on the agent's row goes to its tab; it's seen now.
    w.click("run-0");
    assert_eq!(w.row(), "Terminal@web-7731 *Migration 0042@db-main");
    // A click on the closed tab brings it back.
    w.click("home");
    w.click("closed-0");
    assert_eq!(w.row(), "Terminal@web-7731 *Files@web-7731 Migration 0042@db-main");
}

#[test]
fn on_the_main_menu_arrows_move_a_highlight_and_return_opens() {
    let mut w = Window::open(false);
    assert!(w.rt.state.bench.showing_home());
    // Nothing is highlighted until a key moves.
    assert!(!w.rt.state.screens.home.keys);
    w.key(Key::Down, "");
    assert_eq!((w.rt.state.screens.home.spot, w.rt.state.screens.home.keys), (Spot::Tile(0), true));
    w.key(Key::Right, "");
    w.key(Key::Right, "");
    assert_eq!(w.rt.state.screens.home.spot, Spot::Tile(2));
    w.key(Key::Enter, "");
    assert_eq!(w.row(), "*Terminal@db-main");
    assert_eq!(w.sessions(), 1);
    // Back on the menu, the last tile is "Add a server".
    w.click("home");
    w.key(Key::Right, "");
    assert_eq!(w.rt.state.screens.home.spot, Spot::Add);
    w.key(Key::Enter, "");
    assert!(w.rt.state.screens.add_server.open);
}

#[test]
fn on_the_main_menu_a_digit_opens_its_tile_and_a_letter_starts_a_search() {
    let mut w = Window::open(false);
    w.key(Key::Text("2".into()), "");
    assert_eq!(w.row(), "*Terminal@web-7731");
    w.click("home");
    // No tile has 7: it's a search for "7".
    w.key(Key::Text("7".into()), "");
    assert!(w.rt.state.screens.palette.open);
    assert_eq!(w.rt.state.screens.palette.query.text, "7");
    w.key(Key::Escape, "");
    assert!(!w.rt.state.screens.palette.open);
    w.key(Key::Text("d".into()), "");
    w.type_text("b");
    assert_eq!(w.rt.state.screens.palette.query.text, "db");
    // Return goes to db-main: the first row that runs nothing.
    w.key(Key::Enter, "");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal@db-main");
    // Return with nothing highlighted is the search field: it opens the palette.
    w.click("home");
    w.key(Key::Enter, "");
    assert!(w.rt.state.screens.palette.open);
}

// ---------------------------------------------------------------- the palette

#[test]
fn the_palette_opens_by_its_shortcuts_and_by_the_servers_name() {
    let mut w = Window::busy();
    for (key, held) in [('l', "cs"), ('k', "cs")] {
        w.key(Key::Command(key), held);
        assert!(w.rt.state.screens.palette.open, "Ctrl+Shift+{key}");
        assert!(w.tree().contains("Search or run on web-7731"), "{}", w.tree());
        // The same keys close it.
        w.key(Key::Command(key), held);
        assert!(!w.rt.state.screens.palette.open, "Ctrl+Shift+{key} again");
    }
    w.click("title");
    assert!(w.rt.state.screens.palette.open);
    w.key(Key::Escape, "");
    w.click("search");
    assert!(w.rt.state.screens.palette.open);
    // It hangs under the title, like an address bar.
    let (bar, palette) = (w.rt.presented("title").unwrap().0, w.rt.presented("palette").unwrap().0);
    assert!((palette[0] - bar[0]).abs() < 1.0 && palette[1] >= bar[1] + bar[3], "{bar:?} {palette:?}");
    assert_eq!(palette[2], 560.0);
    // Nothing of all that reached the shell.
    assert_eq!(w.written(0), "");
    // On a Mac it's ⌘L, and ⌘K stays the terminal's.
    let mut mac = Window::open(true);
    mac.key(Key::Command('l'), "m");
    assert!(mac.rt.state.screens.palette.open);
}

#[test]
fn the_palettes_first_row_is_files_and_arrows_and_return_go_places() {
    let mut w = Window::busy();
    w.key(Key::Command('l'), "cs");
    let tree = w.tree();
    for said in ["On web-7731", "Files", "recent: ~/shop/releases", "Go to", "Migration 0042", "Needs you", "db-main", "Window", "New terminal", "Settings"] {
        assert!(tree.contains(said), "{said} isn't in the palette: {tree}");
    }
    assert_eq!(w.rt.state.screens.palette.hot, Some(0));
    // Return on the first row: Files, at the folder it was last in.
    w.key(Key::Enter, "");
    assert!(!w.rt.state.screens.palette.open);
    assert_eq!(w.row(), "Terminal@web-7731 *Files@web-7731 Migration 0042@db-main");
    assert_eq!(w.rt.state.bench.active().unwrap().path.as_deref(), Some("~/shop/releases"));
    // A prefix narrows it; the tab that waits is first among places to go.
    w.key(Key::Command('l'), "cs");
    w.type_text("srv:");
    assert_eq!(w.rt.state.screens.palette.hot, Some(0));
    assert!(!w.tree().contains("New terminal"));
    w.key(Key::Enter, "");
    assert_eq!(w.row(), "Terminal@web-7731 Files@web-7731 *Migration 0042@db-main");
    // The arrows go around the ends; a click picks a row too.
    w.key(Key::Command('l'), "cs");
    w.type_text("srv:");
    w.key(Key::Up, "");
    let last = w.rt.state.screens.palette.hot.unwrap();
    assert!(last > 0);
    w.key(Key::Down, "");
    assert_eq!(w.rt.state.screens.palette.hot, Some(0));
    w.click(&format!("pal-{last}"));
    assert_eq!(w.rt.state.bench.window_title().0, "db-main");
}

#[test]
fn a_command_never_runs_without_return_on_its_own_row() {
    let mut w = Window::busy();
    w.key(Key::Command('l'), "cs");
    w.type_text("uptime");
    // The only row is the command itself, for web-7731, labelled Production: not chosen.
    assert_eq!(w.rt.state.screens.palette.hot, None);
    let tree = w.tree();
    assert!(tree.contains("run on web-7731") && tree.contains("Prod") && tree.contains("Nothing runs by itself"), "{tree}");
    w.key(Key::Enter, "");
    assert!(w.rt.state.screens.palette.open, "Return with no row chosen closed the palette");
    assert_eq!(w.written(0), "");
    // Down chooses it; Return there runs it in the terminal on screen.
    w.key(Key::Down, "");
    assert_eq!(w.rt.state.screens.palette.hot, Some(0));
    w.key(Key::Enter, "");
    assert!(!w.rt.state.screens.palette.open);
    assert_eq!(w.written(0), "uptime\r");
    // Typing after that goes to the shell again.
    w.type_text("ls");
    assert_eq!(w.written(0), "uptime\rls");
}

#[test]
fn a_snippet_is_listed_but_not_chosen_and_one_with_blanks_is_typed_not_run() {
    let mut w = Window::busy();
    w.key(Key::Command('l'), "cs");
    w.type_text("cmd: disks");
    // "How full the disks are" and the text itself: both run on the server, none chosen.
    assert_eq!(w.rt.state.screens.palette.hot, None);
    assert!(w.tree().contains("df -h -x tmpfs -x devtmpfs"), "{}", w.tree());
    w.key(Key::Down, "");
    w.key(Key::Enter, "");
    assert_eq!(w.written(0), "df -h -x tmpfs -x devtmpfs\r");
    // A snippet with blanks is put on the command line for you to finish.
    w.key(Key::Command('l'), "cs");
    w.type_text("cmd: follow");
    w.key(Key::Down, "");
    w.key(Key::Enter, "");
    assert_eq!(w.written(0), "df -h -x tmpfs -x devtmpfs\rdocker logs -f --tail {{lines:100}} {{container}}");
}

#[test]
fn a_command_for_a_server_whose_terminal_is_not_on_screen_waits_for_it() {
    let mut w = Window::open(false);
    // Only Files is open on web-7731: the command opens a terminal there and runs once it's up.
    w.rt.update(|s, tx| {
        s.bench.open(TabKind::Files, WEB);
        s.settle(tx);
    });
    w.settle();
    w.key(Key::Command('l'), "cs");
    w.type_text("uptime");
    w.key(Key::Down, "");
    w.key(Key::Enter, "");
    w.settle();
    assert_eq!(w.row(), "Files@web-7731 *Terminal@web-7731");
    assert_eq!(w.written(0), "uptime\r");
}

#[test]
fn esc_closes_the_palette_and_the_terminal_has_the_keyboard_again() {
    let mut w = Window::busy();
    w.type_text("a");
    w.key(Key::Command('l'), "cs");
    w.type_text("zzz");
    // What is typed in the palette never reaches the shell, and no shortcut fires behind it.
    w.key(Key::Command('t'), "cs");
    assert_eq!(w.written(0), "a");
    assert_eq!(w.rt.state.bench.tabs().len(), 2);
    assert!(w.tree().contains("Nothing found.") || w.rt.state.screens.palette.hot.is_none());
    w.key(Key::Escape, "");
    assert!(!w.rt.state.screens.palette.open && !w.has("palette"));
    w.type_text("b");
    assert_eq!(w.written(0), "ab");
    // A Settings row opens Settings, where the palette was: no key reaches the terminal.
    w.key(Key::Command('l'), "cs");
    w.type_text("set: tor");
    w.key(Key::Enter, "");
    assert!(!w.rt.state.screens.palette.open && w.rt.state.screens.settings.open);
    assert_eq!(w.rt.state.screens.settings.section, sshee_app::settings_store::catalog::Section::Tor);
    w.type_text("c");
    assert_eq!(w.written(0), "ab");
}

// ---------------------------------------------------------------- adding a server

#[test]
fn a_server_typed_by_hand_is_added_and_its_terminal_opens() {
    let mut w = Window::busy();
    w.click("add-server");
    assert!(w.rt.state.screens.add_server.open);
    // The backend was asked what could be brought in, and the dialog offers it.
    assert_eq!(w.asks(), [Ask::Sources]);
    let tree = w.tree();
    for said in ["Add a server", "Bring over your servers from 0.10", "From ~/.ssh/config", "Import from XPipe", "Add and connect"] {
        assert!(tree.contains(said), "{said} isn't in the dialog: {tree}");
    }
    // The address field has the keyboard.
    w.type_text("ssh root@203.0.113.77 -p 2222");
    assert!(w.tree().contains("Reads as root@203.0.113.77:2222"), "{}", w.tree());
    assert_eq!(w.written(0), "", "typing in the dialog reached the shell");
    w.click("add-name");
    w.type_text("queue");
    w.key(Key::Enter, "");
    let added = w.asks().into_iter().find_map(|a| match a {
        Ask::Add(new) => Some(new),
        _ => None,
    });
    let added = added.expect("the backend was asked to add it");
    assert_eq!((added.address.as_str(), added.name.as_deref(), added.password), ("root@203.0.113.77:2222", Some("queue"), None));
    // It's in the sidebar, the dialog is gone, and its terminal is on screen.
    assert!(!w.rt.state.screens.add_server.open);
    assert_eq!(w.servers(), ["This computer", "web-7731", "db-main", "queue"]);
    assert_eq!(w.rt.state.bench.window_title().0, "queue");
    assert_eq!(w.sessions(), 2);
    w.type_text("x");
    assert_eq!(w.written(1), "x");
}

#[test]
fn the_form_says_what_is_wrong_and_asks_nothing_of_the_backend() {
    let mut w = Window::open(false);
    w.key(Key::Command('n'), "cs");
    assert!(w.rt.state.screens.add_server.open);
    // Nothing typed: the button is off, and Return adds nothing.
    w.key(Key::Enter, "");
    assert!(w.rt.state.screens.add_server.open);
    w.type_text("web-7731");
    w.click("add-port");
    w.type_text("70000");
    w.key(Key::Enter, "");
    assert!(w.tree().contains("The port is a number from 1 to 65535."), "{}", w.tree());
    assert!(w.has("add-error"));
    assert!(!w.asks().iter().any(|a| matches!(a, Ask::Add(_))));
    // No shortcut fires behind the dialog, and a click outside doesn't close it.
    w.key(Key::Command('t'), "cs");
    assert_eq!(w.rt.state.bench.tabs().len(), 0);
    let (x, y) = w.rt.find("status").unwrap();
    w.rt.pointer_down(x, y);
    w.rt.pointer_up(x, y);
    w.settle();
    assert!(w.rt.state.screens.add_server.open);
    // Esc does, and what was typed is gone.
    w.key(Key::Escape, "");
    assert!(!w.rt.state.screens.add_server.open);
    assert_eq!(w.rt.state.screens.add_server.address.text, "");
}

#[test]
fn a_password_goes_to_the_backend_and_nowhere_a_reader_could_see_it() {
    let mut w = Window::open(false);
    w.click("tile-add");
    w.type_text("deploy@203.0.113.78");
    w.click("add-password");
    assert!(w.has("add-secret") && !w.has("add-key-file"));
    w.click("add-secret");
    w.type_text("made-up-password");
    assert!(!w.tree().contains("made-up-password"), "the password is read out");
    assert!(w.tree().contains("Saved in your system keychain"));
    w.click("add-remember");
    assert!(w.tree().contains("Kept only until you quit SSHeesh."));
    w.click("add-save");
    let added = w.asks().into_iter().find_map(|a| match a {
        Ask::Add(new) => Some(new),
        _ => None,
    });
    let added = added.expect("the backend was asked to add it");
    assert_eq!(added.password.as_ref().map(|p| p.expose()), Some("made-up-password"));
    assert!(!added.remember_password);
    assert_eq!(added.identity_file, None);
    // The dialog closed and kept nothing.
    assert_eq!(w.rt.state.screens.add_server.password.text, "");
    assert!(!format!("{:?}", w.asks()).contains("made-up-password"));
}

#[test]
fn servers_are_brought_in_from_the_ssh_config_xpipe_and_0_10() {
    let mut w = Window::open(false);
    w.click("home-config");
    assert!(w.rt.state.screens.add_server.open);
    // ~/.ssh/config: its one host comes ticked; "Done" shows it.
    w.click("add-from-config");
    let tree = w.tree();
    assert!(tree.contains("Servers from ~/.ssh/config") && tree.contains("cache-7731") && tree.contains("Your config file isn't changed."), "{tree}");
    w.click("add-import");
    assert_eq!(w.servers(), ["This computer", "web-7731", "db-main", "cache-7731"]);
    assert!(w.tree().contains("1 server shows from ~/.ssh/config."), "{}", w.tree());
    // XPipe: untick it, and nothing comes.
    w.click("add-from-xpipe");
    assert!(w.tree().contains("Found 1 new server in XPipe"));
    w.click("pick-0");
    assert!(w.tree().contains("Add none"));
    w.click("add-all");
    assert!(w.tree().contains("Add 1 server"));
    w.click("add-none");
    w.click("add-back");
    assert!(w.has("add-address"));
    // 0.10: its server comes over.
    w.click("add-from-old");
    assert!(w.tree().contains("Found 1 server in SSHeesh 0.10"));
    w.click("add-import");
    assert_eq!(w.servers(), ["This computer", "web-7731", "db-main", "cache-7731", "metrics"]);
    // Offered no more.
    assert!(!w.has("add-from-old"));
    w.click("add-cancel");
    assert!(!w.rt.state.screens.add_server.open && !w.has("home-old") && !w.has("home-config"));
}

// ---------------------------------------------------------------- a server's menu and card

#[test]
fn a_right_click_on_a_server_opens_its_menu() {
    let mut w = Window::busy();
    w.right_click("server-2");
    let tree = w.tree();
    for said in ["New terminal", "Files", "Rename…", "Icon and colour…", "Production", "Staging", "Development", "Remove server…"] {
        assert!(tree.contains(said), "{said} isn't in the menu: {tree}");
    }
    // Arrows and Return: the second row is Files.
    w.key(Key::Down, "");
    w.key(Key::Down, "");
    w.key(Key::Enter, "");
    assert!(w.rt.state.screens.server_menu.open.is_none());
    assert_eq!(w.rt.state.bench.active().map(|t| (t.kind, t.server.as_str())), Some((TabKind::Files, DB)));
    // A label: db-main becomes Production, and a second pick takes it off.
    w.right_click("server-2");
    w.click("sm-4");
    assert!(w.rt.state.bench.server(DB).unwrap().is_prod());
    assert!(w.asks().contains(&Ask::SetEnv { server: DB.into(), env: Some(EnvLabel::Prod) }));
    w.right_click("server-2");
    w.click("sm-4");
    assert!(!w.rt.state.bench.server(DB).unwrap().is_prod());
    // This computer has only the first two rows; Esc closes.
    w.right_click("server-0");
    assert!(w.has("sm-1") && !w.has("sm-2"));
    w.key(Key::Escape, "");
    assert!(w.rt.state.screens.server_menu.open.is_none());
    // The tile on the main menu has the same menu.
    w.click("home");
    w.right_click("tile-1");
    assert_eq!(w.rt.state.screens.server_menu.open.as_ref().map(|o| o.server.as_str()), Some(WEB));
}

#[test]
fn a_server_is_renamed_given_a_look_and_removed_after_a_question_by_name() {
    let mut w = Window::open(false);
    // Rename: the name is selected, so typing replaces it.
    w.right_click("server-2");
    w.click("sm-2");
    assert!(w.tree().contains("Rename db-main"));
    w.type_text("Main database");
    w.key(Key::Enter, "");
    assert_eq!(w.servers(), ["This computer", "web-7731", "Main database"]);
    // Icon and colour.
    w.right_click("server-2");
    w.click("sm-3");
    assert!(w.tree().contains("Icon and colour for Main database"));
    w.click("icon-rocket");
    assert_eq!(w.rt.state.bench.server(DB).unwrap().icon, "rocket");
    w.click("color-5");
    assert_eq!(w.rt.state.screens.home.colors[DB], "#7e8590");
    w.click("sm-done");
    assert!(w.rt.state.screens.server_menu.open.is_none());
    // Remove: asked by name; Return is Cancel.
    w.right_click("server-2");
    w.click("sm-7");
    let tree = w.tree();
    assert!(tree.contains("Remove Main database?") && tree.contains("Nothing changes on the server."), "{tree}");
    w.key(Key::Enter, "");
    assert!(w.rt.state.screens.server_menu.open.is_none());
    assert_eq!(w.servers().len(), 3);
    w.right_click("server-2");
    w.click("sm-7");
    w.click("sm-remove");
    assert_eq!(w.servers(), ["This computer", "web-7731"]);
    assert!(w.asks().contains(&Ask::Remove { server: DB.into() }));
}

#[test]
fn a_servers_card_shows_while_the_pointer_rests_on_its_icon() {
    let mut w = Window::busy();
    assert!(!w.has("server-card"));
    let (x, y) = w.rt.find("server-2").unwrap();
    w.rt.pointer_move(x, y);
    w.settle();
    // Not at once: the pointer has to rest there a moment, as for a tooltip.
    assert!(!w.has("server-card"));
    for _ in 0..30 {
        w.rt.frame(1.0 / 60.0);
    }
    assert!(w.has("server-card"));
    let tree = w.tree();
    for said in ["db-main", "postgres@203.0.113.31", "1 tab open", "Claude Code needs you, now"] {
        assert!(tree.contains(said), "{said} isn't on the card: {tree}");
    }
    // It sits beside the icon, and goes when the pointer leaves.
    let (icon, card) = (w.rt.presented("server-2").unwrap().0, w.rt.presented("server-card").unwrap().0);
    assert!(card[0] >= icon[0] + icon[2], "{icon:?} {card:?}");
    assert_eq!(card[2], 262.0);
    // Two minutes on, the card says so; through Tor, the address does.
    w.rt.update(|s, _| {
        let waits = s.bench.tabs().iter().find(|t| t.needs_you).map(|t| t.id).unwrap();
        s.backdate_wait(waits, 125.0);
        s.bench.set_tor(sshee_app::workbench::Tor::On { latency_ms: None });
    });
    w.settle();
    let tree = w.tree();
    assert!(tree.contains("Claude Code needs you, 2 min") && tree.contains("postgres@203.0.113.31, through Tor"), "{tree}");
    w.rt.pointer_move(640.0, 400.0);
    w.settle();
    assert!(!w.has("server-card"));
}
