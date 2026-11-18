//! Tor's popover and a server's tunnels, without a screen (`Runtime::with_text`): a fake
//! backend that keeps what it was asked and answers as sshee-core would, through `Remote`.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, Runtime, Tx};
use sshee_app::tor::Status;
use sshee_app::tunnels::{Ask as TunnelAsk, Reply, State};
use sshee_app::workbench::{Server, Tab, Tor, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use crate::screens::settings::TorCommand;
use crate::shell::view::view;
use crate::shell::{Backend, Options, Post, Remote, Shell};
use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

struct Quiet;

impl TermLink for Quiet {
    fn open(&mut self, model: Arc<Model>, _: Size, status: StatusSink) {
        model.connected(false, 1_000);
        status(LinkStatus::Connected { durable: false });
    }
    fn write(&self, _: &[u8]) {}
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {}
    fn close(&self) {}
    fn end(&self) {}
}

/// What the window asked of its backend.
#[derive(Default)]
struct Told {
    tor: Vec<TorCommand>,
    logs: usize,
    shown: usize,
    tunnels: Vec<TunnelAsk>,
}

struct Fake {
    told: Arc<Mutex<Told>>,
    remote: Option<Remote>,
}

impl Backend for Fake {
    fn start(&mut self, remote: Remote) {
        remote.tor_status(Status::Off);
        self.remote = Some(remote);
    }
    fn terminal(&mut self, _: Option<&Server>, _: &Tab) -> Box<dyn TermLink> {
        Box::new(Quiet)
    }
    fn tor(&mut self, command: TorCommand) {
        self.told.lock().unwrap().tor.push(command);
    }
    fn tor_ask(&mut self, ask: sshee_app::tor::Ask) {
        let mut told = self.told.lock().unwrap();
        match ask {
            sshee_app::tor::Ask::ReadLog => {
                told.logs += 1;
                if let Some(remote) = &self.remote {
                    remote.tor_log(vec!["Looking for a consensus.".into(), "Warning: Problem while bootstrapping".into()]);
                }
            }
            sshee_app::tor::Ask::ShowLog => told.shown += 1,
        }
    }
    fn tunnels(&mut self, ask: TunnelAsk) {
        self.told.lock().unwrap().tunnels.push(ask);
    }
    /// Port 5432 here is taken, as by a local database.
    fn port_problem(&mut self, port: u16) -> Option<String> {
        (port == 5432).then(|| "Port 5432 on this computer is already in use.".to_owned())
    }
    fn replays(&self) -> bool {
        true
    }
}

type Queue = Arc<Mutex<Vec<Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>>>>;

struct Window {
    rt: Runtime<Shell>,
    told: Arc<Mutex<Told>>,
    posted: Queue,
    remote: Remote,
}

impl Window {
    fn open() -> Window {
        crate::fonts::declare();
        let told = Arc::new(Mutex::new(Told::default()));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let remote = Remote::new(post);
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
        let bench = Workbench::new(vec![Server::this_computer(), Server::host("web", "web-7731", "deploy@203.0.113.17")]);
        let shell = Shell::new(bench, Box::new(Fake { told: told.clone(), remote: None }), options, Some(remote.clone()));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, told, posted, remote };
        w.settle();
        w
    }

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

    /// What the backend says, as from its threads.
    fn backend(&mut self, f: impl FnOnce(&Remote)) {
        f(&self.remote);
        self.settle();
    }
}

#[test]
fn the_onion_opens_tor_and_its_switch_starts_it() {
    let mut w = Window::open();
    assert!(!w.has("tor-panel"));
    // A right click opens the popover alone.
    w.right_click("tor");
    assert!(w.has("tor-panel") && w.tree().contains("Tor is off"));
    // The log is read as it opens; off, it isn't shown.
    assert_eq!(w.told.lock().unwrap().logs, 1);
    assert!(!w.has("tor-log"));
    w.key(Key::Escape);
    // A click turns Tor on, as 0.10's button did, and opens the popover to watch the start.
    w.click("tor");
    assert_eq!(w.told.lock().unwrap().tor, [TorCommand::On]);
    assert!(w.has("tor-panel"));

    // sshee-core's start, step by step: the sidebar, the popover and the log follow.
    w.backend(|r| r.tor_status(Status::Starting { percent: 47, note: None }));
    assert_eq!(w.rt.state.bench.tor(), Tor::Starting);
    let tree = w.tree();
    assert!(tree.contains("Tor is starting") && tree.contains("47% of the way") && crate::screens::tor::button_help(&w.rt.state).starts_with("Tor: starting, 47%"), "{tree}");
    assert!(w.has("tor-bar") && w.has("tor-log"));
    assert_eq!(w.told.lock().unwrap().logs, 3, "opened twice, and a new state reads the log again");
    assert!(w.tree().contains("Problem while bootstrapping"));
    // Held up: said in Tor's words, and Start fresh is offered.
    w.backend(|r| r.tor_status(Status::Starting { percent: 47, note: Some("A VPN, firewall or antivirus can do this.".into()) }));
    assert!(w.tree().contains("A VPN, firewall or antivirus can do this."));
    assert!(w.has("tor-afresh"));

    // On, with the round trip in the status bar: "Tor 84 ms".
    w.backend(|r| {
        r.tor_status(Status::On);
        r.tor_latency(Some(84));
    });
    assert_eq!(w.rt.state.bench.tor(), Tor::On { latency_ms: Some(84) });
    assert!(w.has("tor-time") && w.tree().contains("84 ms round trip"));
    assert!(!w.has("tor-bar") && !w.has("tor-afresh"));
    w.click("tor-new-routes");
    w.click("tor-show-log");
    assert_eq!(w.told.lock().unwrap().tor, [TorCommand::On, TorCommand::NewRoutes]);
    assert_eq!(w.told.lock().unwrap().shown, 1);

    // Esc closes it; the status bar's "Tor 84 ms" opens it again.
    w.key(Key::Escape);
    assert!(!w.has("tor-panel"));
    w.click("tor-time");
    assert!(w.has("tor-panel"));
    w.click("tor-switch");
    assert_eq!(w.told.lock().unwrap().tor.last(), Some(&TorCommand::Off));
}

#[test]
fn a_tor_that_cant_start_says_why_and_offers_a_fresh_start() {
    let mut w = Window::open();
    w.backend(|r| r.tor_status(Status::Failed { message: "Can't reach the Tor network. Check your internet connection.".into() }));
    assert_eq!(w.rt.state.bench.tor(), Tor::Failed);
    assert!(w.tree().contains("Tor: can't start"));
    w.right_click("tor");
    let tree = w.tree();
    assert!(tree.contains("Tor can't start") && tree.contains("Can't reach the Tor network. Check your internet connection. Nothing connects until Tor does"), "{tree}");
    w.click("tor-afresh");
    assert_eq!(w.told.lock().unwrap().tor, [TorCommand::StartAfresh]);
    // A switch that couldn't note itself for next time says so.
    w.backend(|r| r.tor_warning(Some("Tor is on, but SSHeesh couldn't note that for next time.".into())));
    assert!(w.has("tor-warning"));
    // Switched off, the latency measured before is gone.
    w.backend(|r| {
        r.tor_status(Status::On);
        r.tor_latency(Some(120));
        r.tor_status(Status::Off);
    });
    assert_eq!(w.rt.state.bench.tor(), Tor::Off);
    assert!(!w.has("tor-time"));
}

#[test]
fn a_servers_tunnels_are_added_switched_and_removed() {
    let mut w = Window::open();
    w.right_click("server-1");
    assert!(w.tree().contains("Web apps and tunnels…"));
    w.click("sm-5");
    assert!(w.has("tunnels-dialog"));
    // It looks at what the server runs; nothing saved yet.
    assert_eq!(w.told.lock().unwrap().tunnels.first(), Some(&TunnelAsk::Scan { host: "web".into() }));
    assert!(w.has("tunnels-empty") && w.has("tunnels-looking"));
    w.click("tunnels-add");
    assert!(w.has("tunnels-form"));
    w.type_text("3000");
    assert!(w.tree().contains("localhost:3000 on this computer opens the server's port 3000."), "{}", w.tree());
    w.click("tun-save");
    let asks: Vec<TunnelAsk> = w.told.lock().unwrap().tunnels.iter().filter(|a| !matches!(a, TunnelAsk::Scan { .. })).cloned().collect();
    assert!(matches!(&asks[0], TunnelAsk::Start { id, host, forward } if id == "tun-1" && host == "web" && forward.local_port == 3000), "{asks:?}");
    assert!(matches!(&asks[1], TunnelAsk::Save(list) if list.by_host["web"].len() == 1));
    assert!(w.has("tunnel-tun-1") && !w.has("tunnels-form"));
    assert!(w.tree().contains("Starting…"));
    w.backend(|r| r.tunnels(Reply::State { id: "tun-1".into(), state: Some(State::On { warning: None }) }));
    assert!(w.tree().contains("On at") && w.tree().contains("1 on"));
    // On: its address copies with a click, and it opens in the browser.
    w.click("tunnel-tun-1-copy");
    assert_eq!(w.rt.state.screens.notes.titles(), ["Copied"]);
    w.click("tunnel-tun-1-browse");
    assert_eq!(w.rt.take_url_requests(), ["http://localhost:3000/"]);

    // A port here something else uses is refused before it's saved.
    w.click("tunnels-add");
    w.type_text("5432");
    w.click("tun-save");
    assert!(w.tree().contains("Port 5432 on this computer is already in use."), "{}", w.tree());
    w.click("tun-cancel");

    // A second one on the same port here is refused, in words.
    w.click("tunnels-add");
    w.type_text("3000");
    w.click("tun-save");
    assert!(w.tree().contains("Another saved tunnel already uses port 3000 on this computer."));
    w.click("tun-cancel");

    // The switch stops it; the bin takes it out of the list.
    w.click("tunnel-tun-1-switch");
    assert_eq!(w.told.lock().unwrap().tunnels.last().map(|a| matches!(a, TunnelAsk::Save(_))), Some(true));
    assert!(w.told.lock().unwrap().tunnels.contains(&TunnelAsk::Stop { id: "tun-1".into() }));
    assert!(w.tree().contains("None on"));
    w.click("tunnel-tun-1-remove");
    assert!(!w.has("tunnel-tun-1") && w.has("tunnels-empty"));
    w.click("tunnels-done");
    assert!(!w.has("tunnels-dialog"));
}

#[test]
fn a_web_app_opens_in_the_browser_through_a_tunnel_that_can_be_kept() {
    use sshee_app::tunnels::{Scan, WebApp, SERVER};
    let mut w = Window::open();
    w.rt.update(|s, tx| s.open_tunnels("host:web", tx));
    w.settle();
    let app = |port: u16, title: Option<&str>, known_as: Option<&str>, web: bool| WebApp { port, remote_host: SERVER.into(), title: title.map(str::to_owned), known_as: known_as.map(str::to_owned), scheme: web.then(|| "http".into()), web, ..WebApp::default() };
    let scan = Scan { apps: vec![app(8080, Some("Shop: storefront"), None, true), app(5432, None, Some("PostgreSQL"), false)], curl: true };
    w.backend(|r| r.tunnels(Reply::Scanned { host: "web".into(), result: Ok(scan) }));
    let tree = w.tree();
    assert!(tree.contains("Web apps on this server") && tree.contains("Shop: storefront") && tree.contains("port 8080"), "{tree}");
    // Not websites: under "Other ports", with Connect.
    assert!(!tree.contains("PostgreSQL") && tree.contains("Other ports (1)"));
    w.click("tunnels-other");
    assert!(w.tree().contains("PostgreSQL") && w.has("other-0-connect"));

    // Open: a temporary tunnel on the same port, and the browser once it's on.
    w.click("app-0-open");
    let start = w.told.lock().unwrap().tunnels.last().cloned();
    let Some(TunnelAsk::Start { id, forward, .. }) = start else { panic!("{start:?}") };
    assert_eq!((forward.local_port, forward.remote_port), (8080, Some(8080)));
    assert!(w.tree().contains("Starting…"));
    assert!(w.rt.take_url_requests().is_empty());
    w.backend(|r| r.tunnels(Reply::State { id: id.clone(), state: Some(State::On { warning: None }) }));
    assert_eq!(w.rt.take_url_requests(), ["http://localhost:8080/"]);
    // On: its address (a click copies it), Open, Keep and Stop.
    assert!(w.has("app-0-copy") && w.has("app-0-keep") && w.has("app-0-stop"));
    w.click("app-0-copy");
    assert_eq!(w.rt.state.screens.notes.titles(), ["Copied"]);
    // Kept: it's a saved tunnel now, named after the page, and nothing starts again.
    w.click("app-0-keep");
    assert!(matches!(w.told.lock().unwrap().tunnels.last(), Some(TunnelAsk::Save(list)) if list.by_host["web"][0].name.as_deref() == Some("Shop: storefront")));
    assert!(w.has(&format!("tunnel-{id}")) && !w.has("app-0-keep"));
    // Stop switches the saved one off.
    w.click("app-0-stop");
    assert!(w.told.lock().unwrap().tunnels.contains(&TunnelAsk::Stop { id: id.clone() }));
    assert!(w.has("app-0-open"));
}

#[test]
fn a_server_that_cant_be_looked_at_says_why() {
    let mut w = Window::open();
    w.rt.update(|s, tx| s.open_tunnels("host:web", tx));
    w.settle();
    w.backend(|r| r.tunnels(Reply::Scanned { host: "web".into(), result: Err("Couldn't reach the server.".into()) }));
    assert!(w.has("tunnels-scan-error") && w.tree().contains("Couldn't reach the server."));
    // Look again.
    w.click("tunnels-reload");
    assert_eq!(w.told.lock().unwrap().tunnels.iter().filter(|a| matches!(a, TunnelAsk::Scan { .. })).count(), 2);
}

#[test]
fn tor_says_what_it_did_in_toasts() {
    let mut w = Window::open();
    let titles = |w: &Window| w.rt.state.screens.notes.titles().iter().map(|t| t.to_string()).collect::<Vec<_>>();
    // Turned on: said once it is.
    w.click("tor");
    w.backend(|r| r.tor_status(Status::Starting { percent: 20, note: None }));
    assert!(titles(&w).is_empty());
    w.backend(|r| r.tor_status(Status::On));
    assert_eq!(titles(&w), ["Tor is on"]);
    // Off, and the terminals without tmux that stayed as they were.
    w.backend(|r| r.tor_switched(TorCommand::Off, 2));
    let toast = w.rt.state.screens.notes.toasts().last().map(|(_, t)| t.clone()).unwrap();
    assert_eq!(toast.title, "Tor is off");
    assert_eq!(toast.body.as_deref(), Some("Servers see your own address again. 2 terminals on servers without tmux stay as they are until they reconnect."));
    w.backend(|r| r.tor_switched(TorCommand::NewRoutes, 0));
    assert_eq!(titles(&w).last().map(String::as_str), Some("New Tor routes"));
}

#[test]
fn a_tor_start_that_takes_too_long_says_so_with_its_log() {
    let mut w = Window::open();
    w.backend(|r| r.tor_status(Status::Starting { percent: 5, note: Some("Your network may block Tor.".into()) }));
    for _ in 0..27 {
        w.rt.frame(1.0);
    }
    w.settle();
    let toast = w.rt.state.screens.notes.toasts().find(|(_, t)| t.title == "Tor isn't connected yet").map(|(id, t)| (id, t.clone()));
    let (id, toast) = toast.unwrap_or_else(|| panic!("{:?}", w.rt.state.screens.notes.titles()));
    assert!(toast.body.as_deref().unwrap().starts_with("Your network may block Tor. Your servers wait until it is"));
    assert_eq!(toast.action.as_ref().map(|(l, _)| l.as_str()), Some("Show log"));
    w.rt.update(move |s, _| s.act_on_toast(id));
    assert_eq!(w.told.lock().unwrap().shown, 1);
}

#[test]
fn each_terminal_on_a_server_shows_whether_it_goes_through_tor() {
    let mut w = Window::open();
    w.click("server-1");
    let tab = w.rt.state.bench.active_id().unwrap();
    // Tor off: no mark.
    assert!(!w.has(&format!("tor-mark-{}", tab.0)));
    w.backend(|r| r.tor_status(Status::On));
    assert!(w.has(&format!("tor-mark-{}", tab.0)));
    assert!(w.tree().contains("Through Tor: this server sees a Tor address, not yours"), "{}", w.tree());
    // It connected around Tor: orange, and says why.
    let remote = w.remote.clone();
    w.backend(move |_| remote.through_tor(tab, false));
    assert!(w.tree().contains("This terminal doesn't go through Tor"), "{}", w.tree());
    let remote = w.remote.clone();
    w.backend(move |_| remote.through_tor(tab, true));
    assert!(!w.tree().contains("This terminal doesn't go through Tor"));
    // This computer's terminals have none.
    w.click("server-0");
    let local = w.rt.state.bench.active_id().unwrap();
    assert!(!w.has(&format!("tor-mark-{}", local.0)));
}

#[test]
fn the_list_from_last_time_arrives_with_its_states() {
    let mut w = Window::open();
    let mut saved = sshee_app::model::SavedTunnels::default();
    saved.by_host.insert(
        "web".into(),
        vec![sshee_app::model::SavedTunnel { id: "a".into(), name: Some("Grafana".into()), forward: sshee_app::tunnels::local(3000, "", 3000), on: true, ..Default::default() }],
    );
    w.backend(|r| {
        r.tunnels(Reply::Loaded(saved));
        r.tunnels(Reply::State { id: "a".into(), state: Some(State::Failed { message: "Port 3000 on this computer is already in use.".into() }) });
    });
    w.right_click("server-1");
    w.click("sm-5");
    assert!(!w.has("tunnels-form"), "a server with tunnels opens on its list");
    assert!(w.tree().contains("Grafana") && w.tree().contains("Port 3000 on this computer is already in use."));
    w.click("tunnel-a-retry");
    assert!(matches!(w.told.lock().unwrap().tunnels.last(), Some(TunnelAsk::Start { id, .. }) if id == "a"));
}
