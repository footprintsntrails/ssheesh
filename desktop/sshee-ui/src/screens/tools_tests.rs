//! The security check, recipes, snippets, the welcome, the tour and "What's new", without a
//! screen (`Runtime::with_text`): a fake backend that answers at once through the window's
//! queue, fake sessions that keep what was typed, clicks by element key, and the screen
//! reader's tree. Nothing here touches a file, a keychain or a server.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, Runtime, Tx};
use sshee_app::labels::{Listed, Looks};
use sshee_app::model::{EnvLabel, Settings, Snippets};
use sshee_app::recipes::checkup::{Finding, Report, Verdict};
use sshee_app::recipes::{Ask, Facts, LocalKey, Outcome, Reply, SshHost};
use sshee_app::servers::{Book, Candidate, Sources};
use sshee_app::settings_store::SettingsStore;
use sshee_app::workbench::{Server, Tab, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use crate::shell::view::view;
use crate::shell::{Backend, Options, Post, Remote, Shell};
use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

const WEB: &str = "host:web";
const STAGING: &str = "host:staging";
const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeFakeFakeFakeFakeFakeFakeFakeFakeFake01 id_ed25519";

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

#[derive(Default)]
struct World {
    sessions: Vec<(String, Arc<Mutex<Seen>>)>,
    asks: Vec<Ask>,
    saved: Vec<Snippets>,
}

/// What the fake backend starts with.
#[derive(Clone, Default)]
struct Start {
    welcome: bool,
    version: Option<String>,
    settings: Settings,
}

struct FakeBackend {
    world: Arc<Mutex<World>>,
    book: Book,
    remote: Option<Remote>,
    start: Start,
}

fn finding(id: &str, verdict: Verdict, fix: Option<&str>) -> Finding {
    Finding { id: id.into(), title: format!("{id} title"), verdict, detail: format!("{id} on deploy@203.0.113.17"), fix: fix.map(str::to_owned), fix_note: None }
}

impl Backend for FakeBackend {
    fn start(&mut self, remote: Remote) {
        let (servers, colors, config_new, old_new) = (self.book.servers(), self.book.colors(), self.book.config_new(), self.book.sources.old.len());
        remote.update(move |s| {
            s.bench.set_servers(servers);
            s.set_server_looks(colors, config_new, old_new);
        });
        self.remote = Some(remote);
    }
    fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let seen = Arc::new(Mutex::new(Seen::default()));
        self.world.lock().unwrap().sessions.push((tab.server.clone(), seen.clone()));
        Box::new(FakeLink(seen))
    }
    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        let answer = self.book.handle(ask);
        if let Some(remote) = &self.remote {
            remote.update(move |s| s.servers_answer(answer));
        }
    }
    fn settings(&mut self) -> SettingsStore {
        SettingsStore::in_memory(self.start.settings.clone())
    }
    fn recipes(&mut self, ask: Ask) {
        self.world.lock().unwrap().asks.push(ask.clone());
        let reply = match ask {
            Ask::Check { server } => Reply::Checked {
                server,
                result: Ok(Report {
                    findings: vec![
                        finding("reboot", Verdict::Good, None),
                        finding("firewall", Verdict::Bad, Some("sudo ufw --force enable")),
                        finding("open-ports", Verdict::Warn, None),
                    ],
                    as_root: false,
                }),
            },
            Ask::Fix { server, check, .. } => Reply::Fixed { server, check, outcome: Outcome::Done { output: "Firewall is active".into() } },
            Ask::Facts { server } => Reply::Facts {
                server,
                facts: Facts {
                    kernel: Some("Linux 6.8.0-45-generic".into()),
                    host: Some(SshHost { name: "web-7731".into(), hostname: "203.0.113.17".into(), port: None, user: Some("deploy".into()), identity_file: None, from_config: false }),
                    keys: vec![LocalKey { path: "~/.ssh/id_ed25519".into(), name: "id_ed25519".into(), kind: "ED25519".into() }],
                    windows: false,
                    tor: None,
                },
            },
            Ask::ReadKey { server, path } => Reply::Key { server, path, line: Some(KEY.into()) },
        };
        if let Some(remote) = &self.remote {
            remote.recipes(reply);
        }
    }
    fn snippets_changed(&mut self, list: &Snippets) {
        self.world.lock().unwrap().saved.push(list.clone());
    }
    fn welcome(&mut self) -> bool {
        self.start.welcome
    }
    fn version(&mut self) -> Option<String> {
        self.start.version.clone()
    }
    fn replays(&self) -> bool {
        true
    }
}

/// web-7731 (Production) and web-staging in the sidebar; 0.10 offers one more, the ssh
/// config one.
fn book() -> Book {
    let host = |id: &str, name: &str, address: &str, from_config: bool| Listed { id: id.into(), name: name.into(), address: address.into(), from_config };
    let hosts = vec![host("web", "web-7731", "deploy@203.0.113.17", false), host("staging", "web-staging", "deploy@203.0.113.18", false), host("cfg:cache-7731", "cache-7731", "deploy@203.0.113.40", true)];
    let mut looks = Looks::default();
    looks.set_env("web", Some(EnvLabel::Prod));
    let offer = |name: &str, address: &str| Candidate { id: address.into(), name: name.into(), address: address.into(), identity_file: None, had_password: false, picked: true };
    Book::new(hosts, looks, Sources { old: vec![offer("metrics", "ops@203.0.113.60")], ..Sources::default() })
}

type Queue = Arc<Mutex<Vec<Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>>>>;

struct Window {
    rt: Runtime<Shell>,
    world: Arc<Mutex<World>>,
    posted: Queue,
}

impl Window {
    fn open_with(start: Start) -> Window {
        crate::fonts::declare();
        let world = Arc::new(Mutex::new(World::default()));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
        let backend = FakeBackend { world: world.clone(), book: book(), remote: None, start };
        let shell = Shell::new(Workbench::new(book().servers()), Box::new(backend), options, Some(Remote::new(post)));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, world, posted };
        w.settle();
        w
    }

    fn open() -> Window {
        Window::open_with(Start { settings: Settings { onboarding_version: sshee_app::tour::ONBOARDING_VERSION, ..Settings::default() }, ..Start::default() })
    }

    fn settle(&mut self) {
        for _ in 0..6 {
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
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}\n{}", self.tree()));
        self.rt.pointer_move(x, y);
        self.rt.pointer_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    fn key(&mut self, key: Key) {
        self.rt.key(KeyEvent { key, mods: Mods::default() });
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Text(c.to_string()));
        }
    }

    fn has(&self, key: &str) -> bool {
        self.rt.find(key).is_some()
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

    fn asks(&self) -> Vec<Ask> {
        self.world.lock().unwrap().asks.clone()
    }

    fn fixes(&self) -> usize {
        self.asks().iter().filter(|a| matches!(a, Ask::Fix { .. })).count()
    }

    /// What the last session on `server` was sent.
    fn written_on(&self, server: &str) -> String {
        let world = self.world.lock().unwrap();
        let seen = world.sessions.iter().rev().find(|(s, _)| s == server).map(|(_, seen)| seen.clone()).expect("a session there");
        let out = String::from_utf8_lossy(&seen.lock().unwrap().written).into_owned();
        out
    }
}

// ---------------------------------------------------------------- the security check

#[test]
fn the_shield_checks_the_server_on_screen_worst_first() {
    let mut w = Window::open();
    w.click("server-2");
    w.click("check");
    assert!(w.has("check-dialog"), "the shield opens the check");
    assert_eq!(w.asks()[0], Ask::Check { server: STAGING.into() });
    let tree = w.tree();
    for said in ["Security check", "web-staging · nothing changes until you run a fix", "1 to fix", "1 worth a look", "1 good", "To fix: firewall title", "Some of this needs root."] {
        assert!(tree.contains(said), "{said}: {tree}");
    }
    // Worst first: the firewall, then open ports, then what's fine.
    let y = |key: &str| w.rt.find(key).unwrap().1;
    assert!(y("find-0") < y("find-1") && y("find-1") < y("find-2"));
    assert!(w.has("fix-0") && !w.has("fix-1"), "only a finding with a fix offers it");
    // Esc closes it, and nothing ran.
    w.key(Key::Escape);
    assert!(!w.has("check-dialog") && w.fixes() == 0);
}

#[test]
fn a_fix_runs_after_one_yes_and_production_asks_twice() {
    let mut w = Window::open();
    // Staging: one yes.
    w.click("server-2");
    w.click("check");
    w.click("fix-run-0");
    assert!(w.tree().contains("Run this on web-staging?"));
    assert_eq!(w.fixes(), 0, "nothing runs before the yes");
    w.click("fix-no-0");
    assert!(!w.has("fix-ask-0") && w.fixes() == 0);
    w.click("fix-run-0");
    w.click("fix-yes-0");
    assert_eq!(w.fixes(), 1);
    assert!(w.tree().contains("Done"), "{}", w.tree());
    // The server is checked again after a fix that worked.
    assert_eq!(w.asks().iter().filter(|a| matches!(a, Ask::Check { .. })).count(), 2);
    w.key(Key::Escape);

    // Production: the first yes only leads to the second question.
    w.click("server-1");
    w.click("check");
    assert!(w.tree().contains("Prod"));
    w.click("fix-run-0");
    w.click("fix-yes-0");
    assert_eq!(w.fixes(), 1, "one yes isn't enough on Production");
    assert!(w.tree().contains("web-7731 is a Production server"), "{}", w.tree());
    // Return doesn't press the Production yes.
    w.key(Key::Enter);
    assert_eq!(w.fixes(), 1);
    w.click("fix-yes-0");
    assert_eq!(w.fixes(), 2);
    assert!(matches!(&w.asks()[w.asks().len() - 2], Ask::Fix { server, command, .. } if server == WEB && command == "sudo ufw --force enable"));
}

#[test]
fn from_the_main_menu_the_check_asks_which_server() {
    let mut w = Window::open();
    w.click("check");
    assert!(w.tree().contains("Which server?"));
    assert!(w.asks().is_empty());
    w.click("check-pick-1");
    assert_eq!(w.asks(), [Ask::Check { server: STAGING.into() }]);
}

#[test]
fn streamer_mode_hides_names_in_the_check() {
    let mut w = Window::open();
    w.click("server-2");
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.settle();
    w.click("check");
    let tree = w.tree();
    assert!(!tree.contains("web-staging") && !tree.contains("203.0.113.17") && !tree.contains("deploy"), "{tree}");
    assert!(tree.contains("Server 2 · nothing changes"), "{tree}");
}

// ---------------------------------------------------------------- recipes

#[test]
fn a_recipe_pastes_its_step_without_return_and_steps_aside() {
    let mut w = Window::open();
    w.click("server-2");
    w.click("check");
    w.click("recipe-0");
    assert!(w.has("recipes") && w.tree().contains("Turn on the firewall"), "{}", w.tree());
    // The system was guessed from the kernel, the SSH port filled in.
    assert!(w.tree().contains("Guessed from the server's kernel"));
    assert_eq!(w.rt.state.screens.recipes.edits.get("sshPort").map(|e| e.text.as_str()), Some("22"));
    w.click("paste-1");
    assert!(!w.has("recipes") && w.has("recipe-aside"), "it steps aside after a paste");
    let typed = w.written_on(STAGING);
    assert!(typed.contains("sudo apt-get install -y ufw") && !typed.ends_with('\r'), "{typed:?}");
    w.click("aside-back");
    assert!(w.has("recipes"));
    w.click("tick-0");
    assert!(w.tree().contains("1 of 8 steps done"), "{}", w.tree());
    // "Start over" (at the foot of a long list, out of this window's sight).
    assert!(w.has("recipes-over"));
    w.rt.update(|s, _| s.recipes_start_over());
    w.settle();
    assert!(w.tree().contains("0 of 8 steps done"));
}

#[test]
fn the_key_recipe_reads_the_usual_key() {
    let mut w = Window::open();
    w.click("server-2");
    w.rt.update(|s, tx| s.open_recipes(STAGING, Some("ssh-keys"), tx));
    w.settle();
    assert!(w.asks().contains(&Ask::ReadKey { server: STAGING.into(), path: "~/.ssh/id_ed25519".into() }));
    let tree = w.tree();
    assert!(tree.contains("SSHeesh found a key on this computer, so you can skip this step."), "{tree}");
    // The command that adds it holds the key, under its file's name.
    let cmd = w.rt.state.screens.recipes.typed.values().next().unwrap().get("publicKey").cloned().unwrap();
    assert_eq!(cmd, KEY);
}

// ---------------------------------------------------------------- snippets

#[test]
fn a_snippet_kept_for_one_server_runs_there_and_asks_on_production() {
    let mut w = Window::open();
    w.click("server-2");
    w.rt.update(|s, tx| s.open_snippets(tx));
    w.settle();
    w.click("snip-new");
    w.type_text("Tail it");
    w.click("snip-command");
    w.type_text("tail -n {{lines:20}} {{file}}");
    w.click("snip-scope-here");
    w.click("snip-save");
    let saved = w.world.lock().unwrap().saved.last().cloned().expect("kept by the backend");
    let mine = saved.snippets.last().unwrap();
    assert_eq!((mine.name.as_str(), sshee_app::snippets::only_on(mine)), ("Tail it", Some(STAGING)));
    // Offered here, and in the palette here; not on web-7731.
    let n = saved.snippets.len() - 1;
    assert!(w.has(&format!("snip-run-{n}")));
    w.click(&format!("snip-run-{n}"));
    assert!(w.has("blank-lines") && w.has("blank-file"), "{}", w.tree());
    w.click("blank-file");
    w.type_text("/var/log/syslog");
    w.click("snip-go");
    assert!(!w.has("snippets"));
    assert!(w.written_on(STAGING).contains("tail -n 20 /var/log/syslog"), "{:?}", w.written_on(STAGING));
    w.click("server-1");
    w.rt.update(|s, tx| s.open_snippets(tx));
    w.settle();
    assert!(!w.has(&format!("snip-run-{n}")), "kept for web-staging only");
    // On Production a snippet that runs with Return asks first.
    w.click("snip-run-0");
    assert!(w.has("snip-ask"));
    w.click("snip-yes");
    assert!(w.written_on(WEB).contains("df -h -x tmpfs -x devtmpfs\r"), "{:?}", w.written_on(WEB));
}

// ---------------------------------------------------------------- welcome, tour, what's new

#[test]
fn the_welcome_offers_0_10s_servers_then_the_tour() {
    let mut w = Window::open_with(Start { welcome: true, ..Start::default() });
    assert!(w.has("welcome"));
    let tree = w.tree();
    assert!(tree.contains("Your servers") && tree.contains("Your 2 servers are here.") && tree.contains("Bring over your server from SSHeesh 0.10"), "{tree}");
    // No key reaches a terminal behind it.
    assert!(w.rt.state.screens.any_open());
    w.click("welcome-old");
    assert!(!w.has("welcome") && w.has("add-dialog"));
    assert_eq!(w.rt.state.screens.add_server.page, crate::screens::add_server::Page::List(sshee_app::servers::Source::Old));
    assert_eq!(w.rt.state.settings.get().onboarding_version, sshee_app::tour::ONBOARDING_VERSION);
    // It doesn't come back.
    let mut again = Window::open_with(Start { welcome: true, settings: w.rt.state.settings.get().clone(), ..Start::default() });
    assert!(!again.has("welcome"));
    again.rt.update(|s, tx| s.open_tour(0, tx));
    again.settle();
    assert!(again.tree().contains("Find anything"), "{}", again.tree());
    for _ in 0..5 {
        again.click("tour-next");
    }
    assert!(again.tree().contains("Streaming? Hide it all"));
    again.key(Key::Enter);
    assert!(!again.has("tour"));
}

#[test]
fn whats_new_shows_once_after_an_update() {
    let settings = Settings { onboarded: true, onboarding_version: sshee_app::tour::ONBOARDING_VERSION, last_seen_version: "0.10.14".into(), ..Settings::default() };
    let w = Window::open_with(Start { version: Some("1.0.0-canary".into()), settings: settings.clone(), ..Start::default() });
    assert!(w.has("news"));
    assert!(w.tree().contains("What's new in SSHeesh 1.0.0"), "{}", w.tree());
    assert_eq!(w.rt.state.settings.get().last_seen_version, "1.0.0-canary");
    // The same version again: nothing.
    let again = Window::open_with(Start { version: Some("1.0.0-canary".into()), settings: w.rt.state.settings.get().clone(), ..Start::default() });
    assert!(!again.has("news"));
    // A new install says hello instead.
    let fresh = Window::open_with(Start { version: Some("1.0.0".into()), welcome: true, ..Start::default() });
    assert!(fresh.has("welcome") && !fresh.has("news"));
}
