//! The session's cards in a window without a screen: a fake session told each state, the
//! cards it shows, and what pressing them asks of the session.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, Runtime, Tx};
use sshee_app::prompts::{Fingerprint, HostKey};
use sshee_app::workbench::{Link, Server, Tab, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use crate::shell::{Backend, Options, Post, Remote, Shell};
use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

/// What one fake session saw.
#[derive(Default)]
struct Seen {
    model: Option<Arc<Model>>,
    status: Option<StatusSink>,
    written: Vec<u8>,
    did: Vec<String>,
    /// What the session says it's doing while it connects.
    phase: Option<crate::terminal::connecting::Phase>,
}

struct FakeLink(Arc<Mutex<Seen>>);

impl TermLink for FakeLink {
    fn open(&mut self, model: Arc<Model>, _: Size, status: StatusSink) {
        let mut seen = self.0.lock().unwrap();
        model.connected(true, 1_000);
        seen.model = Some(model);
        status(LinkStatus::Connected { durable: true });
        seen.status = Some(status);
    }
    fn write(&self, bytes: &[u8]) {
        self.0.lock().unwrap().written.extend_from_slice(bytes);
    }
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {
        self.0.lock().unwrap().did.push("reconnect".into());
    }
    fn close(&self) {}
    fn trust(&self, yes: bool) {
        self.0.lock().unwrap().did.push(format!("trust {yes}"));
    }
    fn restart(&self, review: bool) {
        self.0.lock().unwrap().did.push(format!("restart {review}"));
    }
    fn sleep(&self) {
        self.0.lock().unwrap().did.push("sleep".into());
    }
    fn phase(&self) -> Option<crate::terminal::connecting::Phase> {
        self.0.lock().unwrap().phase
    }
}

/// The sessions made, and what the window asked of the server list.
struct FakeBackend(Arc<Mutex<Vec<Arc<Mutex<Seen>>>>>, Arc<Mutex<Vec<sshee_app::servers::Ask>>>);

impl Backend for FakeBackend {
    fn terminal(&mut self, _: Option<&Server>, _: &Tab) -> Box<dyn TermLink> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        self.0.lock().unwrap().push(seen.clone());
        Box::new(FakeLink(seen))
    }
    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        self.1.lock().unwrap().push(ask);
    }
    fn replays(&self) -> bool {
        true
    }
    fn settings(&mut self) -> sshee_app::settings_store::SettingsStore {
        let settings = sshee_app::model::Settings { cursor_blink: false, idle_sleep: 15.0, ..Default::default() };
        sshee_app::settings_store::SettingsStore::in_memory(settings)
    }
}

type Queue = Arc<Mutex<Vec<Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>>>>;

struct Window {
    rt: Runtime<Shell>,
    sessions: Arc<Mutex<Vec<Arc<Mutex<Seen>>>>>,
    asks: Arc<Mutex<Vec<sshee_app::servers::Ask>>>,
    posted: Queue,
}

impl Window {
    fn open() -> Window {
        crate::fonts::declare();
        let sessions = Arc::new(Mutex::new(Vec::new()));
        let asks = Arc::new(Mutex::new(Vec::new()));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
        let servers = vec![Server::host("web", "web-7731", "deploy@203.0.113.17")];
        let shell = Shell::new(Workbench::new(servers), Box::new(FakeBackend(sessions.clone(), asks.clone())), options, Some(Remote::new(post)));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, crate::shell::view::view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, sessions, asks, posted };
        w.frames(4);
        w
    }

    /// `n` frames of a 60th of a second, with what sessions posted run in between.
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

    fn click(&mut self, key: &str) {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_up(x, y);
        self.frames(4);
    }

    fn key(&mut self, key: Key) {
        self.rt.key(KeyEvent { key, mods: Mods::default() });
        self.frames(4);
    }

    fn session(&self, n: usize) -> Arc<Mutex<Seen>> {
        self.sessions.lock().unwrap()[n].clone()
    }

    fn status(&mut self, n: usize, status: LinkStatus) {
        let sink = self.session(n).lock().unwrap().status.clone().expect("the session opened");
        sink(status);
        self.frames(4);
    }

    /// Session `n`'s program prints `bytes`, at the pane's own (virtual) time, so its frame is
    /// due at once.
    fn print(&mut self, n: usize, bytes: &[u8]) {
        let model = self.session(n).lock().unwrap().model.clone().unwrap();
        let now = self.rt.state.active_pane().unwrap().now();
        model.feed_at(bytes, now);
        self.frames(4);
    }

    fn did(&self, n: usize) -> Vec<String> {
        std::mem::take(&mut self.session(n).lock().unwrap().did)
    }

    fn written(&self, n: usize) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.session(n).lock().unwrap().written)).into_owned()
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

    /// A terminal on web-7731, connected.
    fn on_web() -> Window {
        let mut w = Window::open();
        w.click("server-1");
        w
    }
}

fn print(sha: &str) -> Fingerprint {
    Fingerprint { kind: "ED25519".into(), sha256: format!("SHA256:{sha}") }
}

#[test]
fn a_new_servers_key_is_asked_about_with_its_fingerprint_and_trusting_it_signs_in() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::HostKey(HostKey::New { fingerprints: vec![print("madeUpFingerprint1")] }));
    let tree = w.tree();
    assert!(tree.contains("New server. Is this really web-7731?"), "{tree}");
    assert!(tree.contains("SHA256:madeUpFingerprint1") && tree.contains("Trust and connect") && tree.contains("Cancel"), "{tree}");
    // Nothing is typed into a session that hasn't started.
    w.key(Key::Text("y".into()));
    assert_eq!(w.written(0), "");
    w.click("hostkey-yes");
    assert_eq!(w.did(0), ["trust true"]);
}

/// Streamer mode turned on while the key question is up hides the server's name in it, and
/// turned off, shows it again.
#[test]
fn the_key_question_names_the_server_as_streamer_mode_shows_it() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::HostKey(HostKey::New { fingerprints: vec![print("madeUpFingerprint1")] }));
    assert!(w.tree().contains("Is this really web-7731?"));
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.frames(2);
    let tree = w.tree();
    assert!(tree.contains("New server. Is this really") && !tree.contains("web-7731"), "{tree}");
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.frames(2);
    assert!(w.tree().contains("Is this really web-7731?"), "{}", w.tree());
}

#[test]
fn a_changed_key_is_red_shows_both_and_return_is_the_safe_answer() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::HostKey(HostKey::Changed { old: vec![print("oldMadeUp")], new: vec![print("newMadeUp")] }));
    let tree = w.tree();
    assert!(tree.contains("web-7731's key changed") && tree.contains("SHA256:oldMadeUp") && tree.contains("SHA256:newMadeUp"), "{tree}");
    assert!(tree.contains("Connect anyway") && tree.contains("Don't connect"));
    w.key(Key::Enter);
    assert_eq!(w.did(0), ["trust false"]);
}

#[test]
fn trusting_a_key_starts_the_other_terminals_waiting_on_it() {
    let mut w = Window::on_web();
    w.rt.update(|s, tx| s.new_terminal(tx));
    w.frames(4);
    let question = LinkStatus::HostKey(HostKey::New { fingerprints: vec![print("madeUp")] });
    w.status(0, question.clone());
    w.status(1, question);
    w.click("hostkey-yes");
    assert_eq!(w.did(1), ["trust true"]);
    assert_eq!(w.did(0), ["restart false"], "the other terminal on web-7731 starts too");
}

#[test]
fn a_dropped_connection_counts_down_and_comes_back_online() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Retrying { attempt: 2, retry_in_ms: 8000, reason: "The server stopped responding".into() });
    let tree = w.tree();
    assert!(tree.contains("The server stopped responding"), "{tree}");
    assert!(w.rt.find("card-countdown").is_some());
    assert_eq!(w.rt.state.bench.active().unwrap().link, Link::Reconnecting);
    // The text dims while there is no connection.
    let pane = w.rt.state.active_pane().unwrap();
    assert!(pane.text_opacity() < 1.0);
    w.click("card-reconnect");
    assert_eq!(w.did(0), ["reconnect"]);
    w.status(0, LinkStatus::Connected { durable: true });
    let tree = w.tree();
    assert!(tree.contains("Back online") && tree.contains("everything's where you left it"), "{tree}");
    // It goes after a moment.
    w.frames(60 * 4);
    assert!(!w.tree().contains("Back online"));
}

#[test]
fn the_pill_sits_at_the_top_right_and_steps_aside_for_what_opens_over_the_window() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Retrying { attempt: 2, retry_in_ms: 8000, reason: "The server stopped responding".into() });
    w.status(0, LinkStatus::Connected { durable: true });
    assert!(w.tree().contains("Back online"));
    // At the right, where a terminal's first line seldom reaches, clear of the middle where
    // the palette and the agents' list hang from the title.
    let (term, pill) = (w.rt.presented("term-box").unwrap().0, w.rt.presented("session-note").unwrap().0);
    assert!(pill[0] > term[0] + term[2] / 2.0, "{term:?} {pill:?}");
    assert!((term[0] + term[2] - (pill[0] + pill[2]) - 20.0).abs() < 1.0, "not lined up with the find bar: {term:?} {pill:?}");
    // Under the palette it would peek out above it: it waits until the palette goes.
    w.rt.update(|s, tx| s.open_palette("", tx));
    w.frames(2);
    assert!(w.rt.find("session-note").is_none(), "the pill shows under the palette");
    w.key(Key::Escape);
    assert!(w.rt.find("session-note").is_some());
}

#[test]
fn connecting_again_says_so_only_after_a_moment() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Rejoining { attempt: 2 });
    // (The tab says so at once.)
    assert!(w.rt.find("session-note").is_none(), "a quick return doesn't flash a card");
    w.frames(40);
    assert!(w.rt.find("session-note").is_some());
}

#[test]
fn a_drop_with_ask_shows_its_reason_and_can_reconnect_by_itself_from_now_on() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Waiting { note: "The server closed the connection".into() });
    assert!(w.tree().contains("The server closed the connection"));
    w.click("card-always");
    assert_eq!(w.rt.state.settings.get().reconnect_mode, sshee_app::model::ReconnectMode::Auto);
    assert_eq!(w.did(0), ["reconnect"]);
}

#[test]
fn a_session_that_stopped_says_why_and_starts_again() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Gone { note: "The server refused the connection".into() });
    let tree = w.tree();
    assert!(tree.contains("Couldn't connect") && tree.contains("The server refused the connection") && tree.contains("Try again"), "{tree}");
    w.click("card-again");
    assert_eq!(w.did(0), ["restart false"]);
    // A failure about the key reviews it.
    w.status(0, LinkStatus::Gone { note: "The server's identity changed since you last connected.".into() });
    assert!(w.tree().contains("Review the change"));
    w.click("card-again");
    assert_eq!(w.did(0), ["restart true"]);
    // The shell ended: Return starts a new one, Close closes the tab.
    w.status(0, LinkStatus::Ended { code: Some(0) });
    assert!(w.tree().contains("Session ended"));
    w.key(Key::Enter);
    assert_eq!(w.did(0), ["restart false"]);
    w.click("card-close");
    assert!(w.rt.state.bench.tabs().is_empty());
}

#[test]
fn an_unused_terminal_lets_go_and_the_key_that_wakes_it_goes_nowhere() {
    let mut w = Window::on_web();
    // Settings: 15 minutes. Unused for 15 minutes: it lets go of its connection.
    w.rt.frame(15.0 * 60.0 + 1.0);
    w.frames(2);
    assert_eq!(w.did(0), ["sleep"]);
    w.status(0, LinkStatus::Asleep);
    assert!(w.tree().contains("Disconnected after 15 min unused"));
    // The key that wakes it is not typed; nor is what is typed before ssh speaks.
    w.key(Key::Text("p".into()));
    assert_eq!(w.did(0), ["reconnect"]);
    w.status(0, LinkStatus::Rejoining { attempt: 2 });
    w.key(Key::Text("w".into()));
    assert_eq!(w.written(0), "");
    // ssh asks for a password: what is typed now answers it.
    let model = w.session(0).lock().unwrap().model.clone().unwrap();
    model.feed(b"deploy@203.0.113.17's password: ");
    w.frames(12);
    w.key(Key::Text("x".into()));
    assert_eq!(w.written(0), "x");
}

#[test]
fn a_programs_progress_reaches_its_pane() {
    let mut w = Window::on_web();
    let model = w.session(0).lock().unwrap().model.clone().unwrap();
    model.feed(b"\x1b]9;4;1;42\x07");
    w.frames(12);
    let progress = w.rt.state.active_pane().unwrap().progress().expect("a progress");
    assert_eq!(progress.value, 42.0);
}

// ---- "Connecting to web-7731…" (`terminal/connecting.rs`)

#[test]
fn an_empty_terminal_says_where_it_connects_until_the_shell_prints() {
    let mut w = Window::on_web();
    w.session(0).lock().unwrap().phase = Some(crate::terminal::connecting::Phase::Tor);
    w.status(0, LinkStatus::Connecting);
    // A quick connection doesn't flash it.
    assert!(w.rt.find("connect-screen").is_none());
    w.frames(12);
    assert!(w.rt.find("connect-screen").is_some());
    let tree = w.tree();
    assert!(tree.contains("Connecting to web-7731…") && tree.contains("Through Tor"), "{tree}");
    // No cursor in the empty terminal under it: nothing is there to type into yet.
    assert!(w.rt.find("cursor").is_none(), "a cursor under \"Connecting to\"");
    // In: tmux starts on the server, and it says so until the shell draws.
    w.session(0).lock().unwrap().phase = None;
    w.status(0, LinkStatus::Connected { durable: true });
    let tree = w.tree();
    assert!(tree.contains("Connecting to web-7731…") && tree.contains("Starting tmux") && !tree.contains("Through Tor"), "{tree}");
    w.print(0, b"deploy@web-7731:~$ ");
    assert!(w.rt.find("connect-screen").is_none(), "gone the moment the shell prints");
    assert!(!w.tree().contains("Connecting to"));
    assert!(w.rt.find("cursor").is_some(), "the cursor comes with the shell");
}

#[test]
fn a_password_prompt_takes_the_place_of_connecting_and_keys_answer_it() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Connecting);
    w.frames(12);
    assert!(w.rt.find("connect-screen").is_some());
    w.print(0, b"deploy@203.0.113.17's password: ");
    assert!(w.rt.find("connect-screen").is_none(), "ssh's question shows");
    w.key(Key::Text("x".into()));
    assert_eq!(w.written(0), "x");
}

// ---- "Type the password here once…" (`terminal/password.rs`)

/// What the window asked the server list to save.
fn saved(w: &Window) -> Vec<sshee_app::servers::Ask> {
    w.asks.lock().unwrap().iter().filter(|a| matches!(a, sshee_app::servers::Ask::SavePassword { .. })).cloned().collect()
}

#[test]
fn a_password_typed_above_the_prompt_answers_it_and_is_saved_once_it_got_in() {
    use sshee_app::servers::{Ask, Password};
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Connecting);
    w.print(0, b"deploy@203.0.113.17's password: ");
    assert!(w.rt.find("pw-strip").is_some(), "{}", w.tree());
    assert!(w.tree().contains(crate::terminal::password::OFFER));
    // Above the terminal, not over the prompt.
    let (_, strip_y) = w.rt.find("pw-strip").unwrap();
    let (_, term_y) = w.rt.find("term").unwrap();
    assert!(strip_y < term_y, "the strip is at {strip_y}, the terminal at {term_y}");
    // Nothing typed: Save does nothing.
    w.click("pw-strip-save");
    assert_eq!(w.written(0), "");
    w.click("pw-strip-field");
    for c in "made-up-password".chars() {
        w.key(Key::Text(c.to_string()));
    }
    assert_eq!(w.written(0), "", "typing in the strip goes nowhere yet");
    assert!(!w.tree().contains("made-up-password"), "a secure field");
    w.key(Key::Enter);
    // The prompt is answered, and the strip goes; nothing is saved before the server lets it in.
    assert_eq!(w.written(0), "made-up-password\r");
    assert!(w.rt.find("pw-strip").is_none());
    assert!(w.rt.state.active_pane().unwrap().over.password.field.text.is_empty(), "the field is wiped");
    assert!(saved(&w).is_empty());
    w.print(0, b"\r\n");
    w.status(0, LinkStatus::Connected { durable: true });
    assert_eq!(saved(&w), [Ask::SavePassword { server: "host:web".into(), password: Password::new("made-up-password"), remember: true }]);
    assert!(!format!("{:?}", saved(&w)).contains("made-up"), "never printed");
}

#[test]
fn a_password_the_server_refuses_is_never_saved_and_the_strip_says_so() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Connecting);
    w.print(0, b"deploy@203.0.113.17's password: ");
    w.click("pw-strip-field");
    w.key(Key::Text("wrong".into()));
    w.key(Key::Enter);
    assert_eq!(w.written(0), "wrong\r");
    w.print(0, b"\r\nPermission denied, please try again.\r\ndeploy@203.0.113.17's password: ");
    assert!(w.tree().contains(crate::terminal::password::REFUSED), "{}", w.tree());
    // It stops there: nothing kept.
    w.status(0, LinkStatus::Gone { note: "The server didn't accept your login.".into() });
    assert!(w.rt.find("pw-strip").is_none());
    assert!(saved(&w).is_empty());
}

#[test]
fn the_strip_goes_when_the_password_is_typed_in_the_terminal() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Connecting);
    w.print(0, b"(deploy@web-7731) Password: ");
    assert!(w.rt.find("pw-strip").is_some());
    // Typed into the terminal, as ever: the keys go to ssh, and the strip waits.
    w.key(Key::Text("x".into()));
    assert_eq!(w.written(0), "x");
    w.key(Key::Enter);
    w.print(0, b"\r\n");
    assert!(w.rt.find("pw-strip").is_none(), "the prompt is answered");
    w.status(0, LinkStatus::Connected { durable: true });
    assert!(saved(&w).is_empty(), "what is typed in the terminal is never taken");
}

#[test]
fn no_strip_for_a_passphrase_a_key_question_a_saved_password_or_a_shell() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Connecting);
    w.print(0, b"Enter passphrase for key '/home/demo/.ssh/id_ed25519': ");
    assert!(w.rt.find("pw-strip").is_none(), "a key's passphrase");
    w.print(0, b"\r\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ");
    assert!(w.rt.find("pw-strip").is_none(), "the host key's question");
    // A server with a password saved: ssh gets it by itself.
    w.rt.update(|s, _| s.active_pane().unwrap().set_saved_password(true));
    w.print(0, b"\r\ndeploy@203.0.113.17's password: ");
    assert!(w.rt.find("pw-strip").is_none(), "a saved password");
    w.rt.update(|s, _| s.active_pane().unwrap().set_saved_password(false));
    w.frames(2);
    assert!(w.rt.find("pw-strip").is_some());
    // Signed in, a program's own prompt (sudo's) is never offered.
    w.status(0, LinkStatus::Connected { durable: true });
    w.print(0, b"\r\n[sudo] password for deploy: ");
    assert!(w.rt.find("pw-strip").is_none(), "sudo's prompt");
}

#[test]
fn connecting_that_fails_shows_the_card_not_the_loader() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Connecting);
    w.frames(12);
    w.status(0, LinkStatus::Gone { note: "The server didn't accept your login. Check the user name and key, then retry.".into() });
    assert!(w.rt.find("connect-screen").is_none());
    assert!(w.tree().contains("Couldn't connect"), "{}", w.tree());
    // A drop that retries: its countdown, not the loader.
    w.status(0, LinkStatus::Retrying { attempt: 2, retry_in_ms: 8000, reason: "The server stopped responding".into() });
    assert!(w.rt.find("connect-screen").is_none() && w.rt.find("card-countdown").is_some());
}

#[test]
fn signed_in_with_nothing_printed_lets_go_of_connecting_after_a_while() {
    let mut w = Window::on_web();
    w.status(0, LinkStatus::Connecting);
    w.frames(12);
    w.status(0, LinkStatus::Connected { durable: false });
    assert!(w.tree().contains("Starting the shell"));
    w.frames(60 * 2 + 10);
    assert!(w.rt.find("connect-screen").is_none(), "a quiet program isn't hidden for good");
}

#[test]
fn connecting_again_over_what_is_on_screen_is_the_small_note() {
    let mut w = Window::on_web();
    w.print(0, b"deploy@web-7731:~$ ");
    // "Try again" after it stopped: what was on screen stays seen.
    w.status(0, LinkStatus::Connecting);
    w.frames(40);
    assert!(w.rt.find("connect-screen").is_none());
    assert!(w.rt.find("session-note").is_some() && w.tree().contains("Reconnecting…"), "{}", w.tree());
    w.status(0, LinkStatus::Connected { durable: true });
    assert!(!w.tree().contains("Reconnecting…"));
}

/// Streamer mode hides the server's name in it.
#[test]
fn connecting_names_the_server_as_streamer_mode_shows_it() {
    let mut w = Window::on_web();
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.frames(2);
    w.status(0, LinkStatus::Connecting);
    w.frames(12);
    let tree = w.tree();
    assert!(tree.contains("Connecting to") && !tree.contains("web-7731"), "{tree}");
}
