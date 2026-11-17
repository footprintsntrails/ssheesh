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
    assert!(w.tree().contains("Tunnels…"));
    w.click("sm-5");
    assert!(w.has("tunnels-dialog"));
    // Nothing saved: the form is open, on the server's port.
    assert!(w.has("tunnels-form") && !w.has("tunnels-empty"));
    w.type_text("3000");
    assert!(w.tree().contains("localhost:3000 on this computer opens the server's port 3000."), "{}", w.tree());
    w.click("tun-save");
    let asks = w.told.lock().unwrap().tunnels.clone();
    assert!(matches!(&asks[0], TunnelAsk::Start { id, host, forward } if id == "tun-1" && host == "web" && forward.local_port == 3000), "{asks:?}");
    assert!(matches!(&asks[1], TunnelAsk::Save(list) if list.by_host["web"].len() == 1));
    assert!(w.has("tunnel-tun-1") && !w.has("tunnels-form"));
    assert!(w.tree().contains("Starting…"));
    w.backend(|r| r.tunnels(Reply::State { id: "tun-1".into(), state: Some(State::On { warning: None }) }));
    assert!(w.tree().contains("On at localhost:3000") && w.tree().contains("1 on"));

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
