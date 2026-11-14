//! The agents in the window without a screen (`Runtime::with_text`): fake sessions whose
//! "agents" print questions, real frames, the pointer and the keyboard, and what reached each
//! session afterwards. The rules themselves are tested where they live (`sshee_app::agents`,
//! `sshee_term::question`); here, that the window feeds and obeys them.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, Runtime, Tx};
use sshee_app::model::EnvLabel;
use sshee_app::workbench::{Server, Tab, TabId, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use crate::shell::state::{Backend, Options, Post, Remote, Shell};
use crate::shell::view::view;
use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

const STAGING: &str = "host:staging";
const DB: &str = "host:db";

#[derive(Default)]
struct Seen {
    model: Option<Arc<Model>>,
    written: Vec<u8>,
    status: Option<StatusSink>,
}

struct FakeLink(Arc<Mutex<Seen>>);

impl TermLink for FakeLink {
    fn open(&mut self, model: Arc<Model>, _: Size, status: StatusSink) {
        let mut seen = self.0.lock().unwrap();
        model.connected(false, 1_000);
        seen.model = Some(model);
        status(LinkStatus::Connected { durable: false });
        seen.status = Some(status);
    }
    fn write(&self, bytes: &[u8]) {
        self.0.lock().unwrap().written.extend_from_slice(bytes);
    }
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {}
    fn close(&self) {}
}

/// What the window asked of its backend.
#[derive(Default)]
struct World {
    sessions: Vec<(TabId, Arc<Mutex<Seen>>)>,
    notified: Vec<(TabId, String, String)>,
    agents_on: Vec<Vec<String>>,
}

struct FakeBackend(Arc<Mutex<World>>);

impl Backend for FakeBackend {
    fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        self.0.lock().unwrap().sessions.push((tab.id, seen.clone()));
        Box::new(FakeLink(seen))
    }
    fn agents_on(&mut self, servers: &[Server]) {
        self.0.lock().unwrap().agents_on.push(servers.iter().map(|s| s.name.clone()).collect());
    }
    fn notify(&mut self, tab: &Tab, note: &sshee_term::attention::Notification) {
        self.0.lock().unwrap().notified.push((tab.id, note.title.clone(), note.body.clone()));
    }
    fn replays(&self) -> bool {
        true
    }
}

type Queue = Arc<Mutex<Vec<Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>>>>;

struct Window {
    rt: Runtime<Shell>,
    world: Arc<Mutex<World>>,
    posted: Queue,
    remote: Remote,
}

/// Claude Code's question about `command`, as its screen shows it, and the bell it rings.
fn claude_asks(command: &str) -> String {
    format!(
        "\x1b[2J\x1b[H> Apply migration 0042.\r\n\r\n────────────────────────────────────────────────────────────\r\n Bash command\r\n\r\n   {command}\r\n   Apply migration 0042\r\n\r\n Do you want to proceed?\r\n ❯ 1. Yes\r\n   2. Yes, and don't ask again for psql commands in /srv/shop-db\r\n   3. No, and tell Claude what to do differently (esc)\r\n\x07"
    )
}

const MIGRATION: &str = "psql -h db-main -d shop -f migrations/0042_orders_index.sql";
/// The agent's screen once it stopped asking.
const WORKING: &str = "\x1b[2J\x1b[H> Apply migration 0042.\r\n\r\n● Running…\r\n";

impl Window {
    fn open() -> Window {
        crate::fonts::declare();
        let world = Arc::new(Mutex::new(World::default()));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let remote = Remote::new(post);
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
        let servers = vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("staging", "web-staging", "deploy@203.0.113.18").env(EnvLabel::Staging),
            Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Prod).icon("database"),
        ];
        let shell = Shell::new(Workbench::new(servers), Box::new(FakeBackend(world.clone())), options, Some(remote.clone()));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, world, posted, remote };
        w.frames(4);
        w
    }

    /// Frames of a 60 Hz screen, with what sessions posted run in between.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
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

    /// Time passing.
    fn wait(&mut self, secs: f64) {
        self.frames((secs * 60.0).ceil() as usize);
    }

    fn click(&mut self, key: &str) {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_move(x, y);
        self.rt.pointer_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_up(x, y);
        self.frames(4);
    }

    fn point_at(&mut self, key: &str) {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_move(x, y);
        self.frames(2);
    }

    fn key(&mut self, key: Key, held: &str) {
        let ctrl = held.contains('c');
        let mods = Mods { shift: held.contains('s'), alt: false, ctrl, logo: false, cmd: ctrl };
        self.rt.key(KeyEvent { key, mods });
        self.frames(4);
    }

    fn typed(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Text(c.to_string()), "");
        }
    }

    fn session(&self, tab: TabId) -> Arc<Mutex<Seen>> {
        self.world.lock().unwrap().sessions.iter().find(|(id, _)| *id == tab).unwrap_or_else(|| panic!("no session for {tab:?}")).1.clone()
    }

    fn written(&self, tab: TabId) -> String {
        String::from_utf8_lossy(&self.session(tab).lock().unwrap().written).into_owned()
    }

    /// Output from the program in `tab` arrives, on the window's own clock (the panes run on
    /// it here, so a busy machine can't make a frame come "before" its output).
    fn arrives(&mut self, tab: TabId, out: &str) {
        let model = self.session(tab).lock().unwrap().model.clone().expect("the session opened");
        let now = self.rt.state.panes[&tab].now();
        model.feed_at(out.as_bytes(), now);
    }

    /// Output from the program in `tab`, and the frames that show it.
    fn feed(&mut self, tab: TabId, out: &str) {
        self.arrives(tab, out);
        // The model's gate holds a first burst for a moment.
        self.frames(12);
    }

    fn status(&mut self, tab: TabId, status: LinkStatus) {
        let sink = self.session(tab).lock().unwrap().status.clone().expect("the session opened");
        sink(status);
        self.frames(4);
    }

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

    fn shown(&self, key: &str) -> bool {
        self.rt.find(key).is_some()
    }

    fn agent(&self) -> &super::State {
        &self.rt.state.screens.agent
    }

    /// A terminal on `server` with Claude Code in front, then one on this computer, which is
    /// the tab on screen. Returns (the agent's tab, the tab on screen).
    fn with_agent_on(server: &str, place: usize) -> (Window, TabId, TabId) {
        let mut w = Window::open();
        w.click(&format!("server-{place}"));
        let agent = w.rt.state.bench.active_id().unwrap();
        assert_eq!(w.rt.state.bench.tab(agent).unwrap().server, server);
        let key = w.rt.state.bench.tab(agent).unwrap().session_key.clone().unwrap();
        // tmux on the server says what runs in that terminal.
        w.remote.set_foreground(server.to_owned(), vec![(key, "claude".into())]);
        w.click("server-0");
        let here = w.rt.state.bench.active_id().unwrap();
        (w, agent, here)
    }

    /// The same with the agent asking about the migration, long enough ago for a click to count.
    fn asking_on(server: &str, place: usize) -> (Window, TabId, TabId) {
        let (mut w, agent, here) = Window::with_agent_on(server, place);
        w.feed(agent, &claude_asks(MIGRATION));
        (w, agent, here)
    }

    /// Opens the agent's card by resting the pointer on its tab, and waits out its first moment.
    fn rest_on_tab(&mut self, tab: TabId) {
        self.point_at(&format!("tab-{}", tab.0));
        self.wait(1.0);
        assert!(self.shown("agent-card"), "the card didn't open under the pointer");
    }
}

#[test]
fn a_tab_is_named_after_its_agent_and_shows_it_working() {
    let (mut w, agent, _) = Window::with_agent_on(STAGING, 2);
    let tree = w.tree();
    assert!(tree.contains("Terminal (Claude Code), web-staging"), "{tree}");
    // The backend was told which servers have a connected terminal to ask.
    assert_eq!(w.world.lock().unwrap().agents_on.last().unwrap(), &["web-staging"]);
    // Output comes: the working mark; it stops: the mark goes.
    w.feed(agent, "● Reading migrations/0042_orders_index.sql\r\n");
    assert!(w.agent().working(agent));
    assert!(w.shown("working"));
    assert!(w.tree().contains("Terminal (Claude Code), web-staging, working"));
    w.wait(4.5);
    assert!(!w.agent().working(agent));
    assert!(!w.shown("working"));
    // Back at the shell the tab is a terminal again.
    let key = w.rt.state.bench.tab(agent).unwrap().session_key.clone().unwrap();
    w.remote.set_foreground(STAGING.to_owned(), vec![(key, "zsh".into())]);
    w.frames(4);
    assert!(w.tree().contains("Terminal, web-staging"));
}

#[test]
fn a_bell_says_needs_you_and_hands_a_safe_notification_to_the_backend() {
    let (mut w, agent, _) = Window::with_agent_on(DB, 3);
    // The window is in the background, and the agent says something with a token in it.
    w.rt.update(|s, tx| s.window_focus(false, tx));
    let token = format!("ghp_{}", "a1B2c3D4e5".repeat(4));
    w.feed(agent, &format!("\x1b]9;Claude Code: push with {token}?\x07"));
    assert!(w.rt.state.bench.tab(agent).unwrap().needs_you);
    assert!(w.shown("waits"), "no dot on the server's icon");
    assert!(w.tree().contains("Terminal (Claude Code), db-main, Needs you"));
    let notified = w.world.lock().unwrap().notified.clone();
    assert_eq!(notified.len(), 1);
    assert_eq!((notified[0].0, notified[0].1.as_str()), (agent, "db-main needs you"));
    assert!(!notified[0].2.contains(&token) && notified[0].2.contains('•'), "{}", notified[0].2);
    // It rings again while it waits: one notification per wait.
    w.feed(agent, "\x07");
    assert_eq!(w.world.lock().unwrap().notified.len(), 1);
    // While streaming the server has no name outside the app.
    w.rt.update(|s, tx| {
        s.focus_tab(agent, tx);
        s.show_home(tx);
        s.screens.agent.streaming = true;
    });
    w.feed(agent, "\x07");
    assert_eq!(w.world.lock().unwrap().notified.last().unwrap().1, "Server 3 needs you");
}

#[test]
fn resting_on_a_waiting_tab_opens_its_card_and_keys_stay_the_terminals() {
    let (mut w, agent, here) = Window::asking_on(STAGING, 2);
    assert!(!w.shown("agent-card"));
    w.rest_on_tab(agent);
    let tree = w.tree();
    for said in ["Claude Code", "on web-staging", MIGRATION, "Do you want to proceed?", "Yes, and don't ask again for psql commands in /srv/shop-db", "No, and tell Claude what to do differently…", "Click an answer. Keys still go to the terminal."] {
        assert!(tree.contains(said), "{said} isn't on the card: {tree}");
    }
    assert!(!w.agent().has_keyboard());
    // Digits and Return are typed into the terminal on screen, as before; nothing goes to the agent.
    w.typed("1");
    w.key(Key::Enter, "");
    assert_eq!(w.written(here), "1\r");
    assert_eq!(w.written(agent), "");
    // The pointer leaves the tab and the card: it goes away.
    w.point_at("status");
    w.wait(0.6);
    assert!(!w.shown("agent-card"));
    assert_eq!(w.written(agent), "");
}

#[test]
fn pointing_at_answers_sends_nothing() {
    let (mut w, agent, _) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    for key in ["answer-1", "answer-2", "answer-3", "answer-1"] {
        w.point_at(key);
        w.wait(0.5);
    }
    assert!(w.shown("agent-card"));
    assert_eq!(w.written(agent), "");
}

#[test]
fn a_click_on_an_answer_types_its_digit_into_the_agents_terminal() {
    let (mut w, agent, here) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    w.click("answer-1");
    assert_eq!(w.written(agent), "1");
    assert_eq!(w.written(here), "");
    assert!(w.tree().contains("Sent 1 to Claude Code"), "{}", w.tree());
    // You stayed on your tab, and the card leaves by itself.
    assert_eq!(w.rt.state.bench.active_id(), Some(here));
    w.wait(3.0);
    assert!(!w.shown("agent-card"));
    assert_eq!(w.written(agent), "1");
}

#[test]
fn a_click_in_the_cards_first_moment_is_ignored() {
    let (mut w, agent, _) = Window::asking_on(STAGING, 2);
    w.point_at(&format!("tab-{}", agent.0));
    // It opens 0.45 s after the pointer came to rest; a click right then is too early.
    w.wait(0.47);
    assert!(w.shown("agent-card"));
    w.click("answer-1");
    assert_eq!(w.written(agent), "");
    w.wait(0.4);
    w.click("answer-1");
    assert_eq!(w.written(agent), "1");
}

#[test]
fn on_production_return_alone_sends_nothing_and_a_digit_does() {
    let (mut w, agent, here) = Window::asking_on(DB, 3);
    w.rest_on_tab(agent);
    assert!(w.tree().contains("Prod"));
    // A click on the card gives it the keyboard.
    w.click("who");
    assert!(w.agent().has_keyboard());
    assert!(w.tree().contains("Press 1, 2 or 3, or click an answer"));
    w.key(Key::Enter, "");
    assert_eq!(w.written(agent), "");
    // Letters go nowhere: not to the agent, not to the terminal behind.
    w.typed("yes");
    assert_eq!((w.written(agent), w.written(here)), (String::new(), String::new()));
    // A standing permission wants the same answer twice.
    w.typed("2");
    assert_eq!(w.written(agent), "");
    assert!(w.tree().contains("2 again sends it. Claude Code won't ask next time."));
    w.typed("2");
    assert_eq!(w.written(agent), "2");
    assert!(w.tree().contains("Sent 2 to Claude Code"));
}

#[test]
fn off_production_return_sends_the_first_answer_and_arrows_move_the_choice() {
    let (mut w, agent, _) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    w.click("who");
    w.key(Key::Down, "");
    w.key(Key::Up, "");
    w.key(Key::Enter, "");
    assert_eq!(w.written(agent), "1");
}

#[test]
fn escape_gives_the_keyboard_back_to_the_terminal() {
    let (mut w, agent, here) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    w.click("who");
    w.key(Key::Escape, "");
    assert!(!w.shown("agent-card"));
    w.typed("ls");
    assert_eq!(w.written(here), "ls");
    assert_eq!(w.written(agent), "");
    // The agent still waits: nothing was decided.
    assert!(w.rt.state.bench.tab(agent).unwrap().needs_you);
}

#[test]
fn your_own_words_follow_once_the_agent_stopped_asking() {
    let (mut w, agent, here) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    w.click("answer-3");
    assert!(w.shown("agent-words"), "the third answer didn't turn into a field");
    assert!(w.tree().contains("Return sends your words"));
    assert_eq!(w.written(agent), "");
    w.typed("Use CONCURRENTLY");
    // Esc goes back to the answers, and the field comes back empty.
    w.key(Key::Escape, "");
    assert!(!w.shown("agent-words") && w.shown("answer-3"));
    w.click("answer-3");
    w.typed("Use a transaction");
    w.key(Key::Enter, "");
    // The answer's key went; the words wait for the question to leave the agent's screen.
    assert_eq!(w.written(agent), "3");
    assert!(w.tree().contains("Sent 3. Your words follow when Claude Code is ready."));
    w.feed(agent, WORKING);
    assert_eq!(w.written(agent), "3Use a transaction\r");
    assert!(w.tree().contains("Sent 3 and your words to Claude Code"));
    assert_eq!(w.written(here), "");
}

#[test]
fn a_question_that_changed_under_the_pointer_is_not_answered() {
    let (mut w, agent, _) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    // The agent asks something else, and the click lands before the window drew it.
    let (x, y) = w.rt.find("answer-1").unwrap();
    w.arrives(agent, &claude_asks("rm -rf /srv/shop-db/backups"));
    w.rt.pointer_move(x, y);
    w.rt.pointer_down(x, y);
    w.rt.pointer_up(x, y);
    w.frames(20);
    assert_eq!(w.written(agent), "", "an answer went to a question that wasn't the one drawn");
    let tree = w.tree();
    assert!(tree.contains("rm -rf /srv/shop-db/backups"), "{tree}");
    assert!(tree.contains("The question changed while you read. Nothing was sent."), "{tree}");
    // Read, and answered knowingly.
    w.wait(0.4);
    w.click("answer-1");
    assert_eq!(w.written(agent), "1");
}

#[test]
fn a_tab_that_is_reconnecting_gets_nothing() {
    let (mut w, agent, _) = Window::asking_on(STAGING, 2);
    w.status(agent, LinkStatus::Reconnecting { note: "reconnecting (try 2)".into() });
    w.rest_on_tab(agent);
    w.click("answer-1");
    assert_eq!(w.written(agent), "");
    assert!(w.tree().contains("Not sent: the tab is reconnecting. Your answer is kept."), "{}", w.tree());
    // Connected again: nothing goes by itself; Return sends the answer that was kept.
    w.status(agent, LinkStatus::Connected { durable: false });
    w.wait(1.0);
    assert_eq!(w.written(agent), "");
    w.key(Key::Enter, "");
    assert_eq!(w.written(agent), "1");
}

#[test]
fn a_screen_that_cannot_be_read_says_open_the_tab() {
    let (mut w, agent, _) = Window::with_agent_on(STAGING, 2);
    // Something is printed after the answers: the reader won't guess.
    w.feed(agent, &format!("{}postgres@web-staging:~$ ", claude_asks(MIGRATION).replace('\x07', "\r\n\x07")));
    w.rest_on_tab(agent);
    let tree = w.tree();
    assert!(tree.contains("Claude Code is waiting for you.") && tree.contains("Open the tab to answer"), "{tree}");
    assert!(!w.shown("answer-1"));
    w.click("who");
    w.typed("1");
    w.key(Key::Enter, "");
    assert_eq!(w.written(agent), "");
}

#[test]
fn the_agent_stopped_asking_while_the_card_was_open() {
    let (mut w, agent, _) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    w.feed(agent, WORKING);
    assert!(w.tree().contains("Claude Code stopped asking."), "{}", w.tree());
    assert!(!w.shown("answer-1") || w.written(agent).is_empty());
    w.click("who");
    w.typed("1");
    assert_eq!(w.written(agent), "");
    w.wait(3.0);
    assert!(!w.shown("agent-card"));
}

#[test]
fn while_streaming_the_card_shows_no_question_and_no_answers() {
    let (mut w, agent, _) = Window::asking_on(DB, 3);
    w.rt.update(|s, _| s.screens.agent.streaming = true);
    w.rest_on_tab(agent);
    let tree = w.tree();
    assert!(tree.contains("Its question is hidden while streaming."), "{tree}");
    assert!(tree.contains("on Server 3"), "{tree}");
    for hidden in [MIGRATION, "Do you want to proceed?", "db-main, in"] {
        assert!(!tree.lines().any(|l| l == hidden), "{hidden} is on a card while streaming");
    }
    assert!(!w.shown("answer-1"));
    w.click("who");
    w.typed("1");
    assert_eq!(w.written(agent), "");
}

#[test]
fn the_list_has_the_waiting_agent_first_and_return_opens_its_card_with_the_keyboard() {
    let (mut w, agent, here) = Window::asking_on(STAGING, 2);
    w.key(Key::Command('a'), "cs");
    assert!(w.agent().list_open());
    let tree = w.tree();
    assert!(tree.contains("Terminal (Claude Code), web-staging, Needs you"), "{tree}");
    w.key(Key::Enter, "");
    assert!(!w.agent().list_open());
    assert!(w.shown("agent-card") && w.agent().has_keyboard());
    // The keys are the card's at once.
    w.wait(0.4);
    w.typed("1");
    assert_eq!(w.written(agent), "1");
    assert_eq!(w.written(here), "");
}

#[test]
fn the_list_goes_to_an_agent_that_is_not_waiting_and_escape_closes_it() {
    let (mut w, agent, here) = Window::with_agent_on(STAGING, 2);
    w.key(Key::Command('a'), "cs");
    assert!(w.tree().contains("Terminal (Claude Code), web-staging, quiet"), "{}", w.tree());
    // Esc: back in the terminal.
    w.key(Key::Escape, "");
    assert!(!w.agent().list_open());
    w.typed("x");
    assert_eq!(w.written(here), "x");
    // Again, and Return: its tab.
    w.key(Key::Command('a'), "cs");
    w.key(Key::Enter, "");
    assert_eq!(w.rt.state.bench.active_id(), Some(agent));
    assert!(!w.agent().list_open());
    // With no agent anywhere the list says so.
    let mut w = Window::open();
    w.key(Key::Command('a'), "cs");
    assert!(w.tree().contains("No agent is running in a terminal."));
    w.key(Key::Command('a'), "cs");
    assert!(!w.agent().list_open());
}

#[test]
fn going_to_the_tab_puts_the_card_away_and_marks_it_seen() {
    let (mut w, agent, _) = Window::asking_on(STAGING, 2);
    w.rest_on_tab(agent);
    w.click(&format!("tab-{}", agent.0));
    assert_eq!(w.rt.state.bench.active_id(), Some(agent));
    assert!(!w.shown("agent-card"));
    assert!(!w.rt.state.bench.tab(agent).unwrap().needs_you);
    assert_eq!(w.agent().agents.waiting_since(agent), None);
    assert_eq!(w.written(agent), "");
}
