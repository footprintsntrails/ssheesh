//! The Containers and Stats tabs in a window without a screen: a fake backend that answers
//! from made-up machines through the window's queue, as a real one does from its threads.

use std::sync::{Arc, Mutex};

use rune::{Env, Runtime, Tx};
use sshee_app::containers::{Action, Answer, Ask, Container, ContainerList, Want};
use sshee_app::files::{Failure, FailureKind};
use sshee_app::model::{ContainerRun, Engine, EnvLabel};
use sshee_app::stats::{Facts, Process};
use sshee_app::workbench::{Link, Server, Tab, TabKind, Vitals, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use crate::shell::{Backend, Options, Post, Remote, Shell};
use crate::terminal::{LinkStatus, PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

const WEB: &str = "host:web";
const DB: &str = "host:db";

struct Link0;

impl TermLink for Link0 {
    fn open(&mut self, model: Arc<Model>, _: Size, status: StatusSink) {
        model.connected(false, 1_000);
        status(LinkStatus::Connected { durable: false });
    }
    fn write(&self, _: &[u8]) {}
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {}
    fn close(&self) {}
}

/// What the backend was asked, and the remote to answer through.
#[derive(Default)]
struct World {
    asks: Vec<Ask>,
    programs: Vec<sshee_app::stats::Ask>,
    terminals: Vec<Tab>,
    remote: Option<Remote>,
    /// web's containers; db answers that Docker isn't there.
    web: Vec<Container>,
}

struct Fake(Arc<Mutex<World>>);

fn c(id: &str, name: &str, state: &str, status: &str, project: Option<&str>) -> Container {
    Container {
        engine: Engine::Docker,
        id: id.into(),
        name: name.into(),
        image: format!("registry.example/{name}:1"),
        running: state == "running",
        state: state.into(),
        status: status.into(),
        ports: "0.0.0.0:8080->8080/tcp".into(),
        project: project.map(str::to_owned),
    }
}

impl Backend for Fake {
    fn start(&mut self, remote: Remote) {
        self.0.lock().unwrap().remote = Some(remote);
    }
    fn terminal(&mut self, _: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        self.0.lock().unwrap().terminals.push(tab.clone());
        Box::new(Link0)
    }
    fn replays(&self) -> bool {
        true
    }
    fn containers(&mut self, ask: Ask) {
        let mut w = self.0.lock().unwrap();
        w.asks.push(ask.clone());
        let answer = match &ask.want {
            Want::List { ticket } if ask.server == WEB => Answer::Listed { ticket: *ticket, result: Ok(ContainerList { engines: vec![Engine::Docker], containers: w.web.clone(), problems: vec![] }) },
            Want::List { ticket } => Answer::Listed { ticket: *ticket, result: Ok(ContainerList::default()) },
            Want::Act { id, name, action, .. } => {
                if let Some(c) = w.web.iter_mut().find(|c| c.id == *id) {
                    let on = *action != Action::Stop;
                    (c.running, c.state, c.status) = (on, if on { "running" } else { "exited" }.into(), if on { "Up 1 second" } else { "Exited (0) 1 second ago" }.into());
                }
                let result = if name == "stuck" { Err(Failure::new(FailureKind::Failed, "Error response from daemon: timeout")) } else { Ok(()) };
                Answer::Acted { id: id.clone(), name: name.clone(), action: *action, result }
            }
        };
        if let Some(remote) = &w.remote {
            remote.containers(ask.reply(answer));
        }
    }
    fn stats(&mut self, ask: sshee_app::stats::Ask) {
        let mut w = self.0.lock().unwrap();
        w.programs.push(ask.clone());
        if let Some(remote) = &w.remote {
            remote.stats(ask.reply(Ok(vec![Process { pid: 7, cpu: 12.5, memory: 300 << 20, command: "node".into() }, Process { pid: 9, cpu: 1.0, memory: 2 << 30, command: "postgres".into() }])));
        }
    }
    fn settings(&mut self) -> sshee_app::settings_store::SettingsStore {
        let settings = sshee_app::model::Settings { cursor_blink: false, ..Default::default() };
        sshee_app::settings_store::SettingsStore::in_memory(settings)
    }
}

type Queue = Arc<Mutex<Vec<Box<dyn FnOnce(&mut Shell, &mut Tx) -> bool + Send>>>>;

struct Window {
    rt: Runtime<Shell>,
    world: Arc<Mutex<World>>,
    posted: Queue,
}

impl Window {
    fn open() -> Window {
        crate::fonts::declare();
        let world = Arc::new(Mutex::new(World {
            web: vec![
                c("a1b2c3d4e5f6", "shop-api-1", "running", "Up 3 days (healthy)", Some("shop")),
                c("b2c3d4e5f6a1", "shop-db-1", "running", "Up 3 days", Some("shop")),
                c("c3d4e5f6a1b2", "nightly-backup", "exited", "Exited (0) 6 hours ago", None),
                c("d4e5f6a1b2c3", "stuck", "running", "Up 2 hours", None),
            ],
            ..World::default()
        }));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
        let servers = vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("db", "db-main", "postgres@203.0.113.31").icon("database"),
        ];
        let shell = Shell::new(Workbench::new(servers), Box::new(Fake(world.clone())), options, Some(Remote::new(post)));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, crate::shell::view::view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, world, posted };
        w.settle();
        w
    }

    /// Frames, with what the backend posted run in between, as the window's loop does.
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
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_down(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_up(x, y);
        self.settle();
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

    fn open_tab(&mut self, kind: TabKind, server: &str) {
        self.rt.update(|s, tx| s.show_tab(kind, server, None, tx));
        self.settle();
    }

    fn lists(&self) -> usize {
        self.world.lock().unwrap().asks.iter().filter(|a| matches!(a.want, Want::List { .. })).count()
    }
}

#[test]
fn a_containers_tab_lists_them_by_project_and_looks_again_while_on_screen() {
    let mut w = Window::open();
    w.open_tab(TabKind::Containers, WEB);
    assert_eq!(w.lists(), 1, "a look as the tab comes up");
    let tree = w.tree();
    for said in ["Containers", "4 containers, 3 running", "shop", "2 running", "Not in a project", "shop-api-1, Up 3 days", "nightly-backup, Stopped 6 hours ago"] {
        assert!(tree.contains(said), "{said} isn't in: {tree}");
    }
    // Ten seconds later, another look; none while the tab is out of sight.
    w.rt.frame(10.5);
    w.settle();
    assert_eq!(w.lists(), 2);
    w.rt.update(|s, tx| s.show_home(tx));
    w.settle();
    w.rt.frame(30.0);
    w.settle();
    assert_eq!(w.lists(), 2, "a hidden tab doesn't look");
}

#[test]
fn buttons_start_stop_and_restart_and_say_how_it_went() {
    let mut w = Window::open();
    w.open_tab(TabKind::Containers, WEB);
    // Rows: shop-api-1, shop-db-1, then stuck (running first) and nightly-backup (stopped).
    w.click("ctr-0-restart");
    assert!(w.world.lock().unwrap().asks.iter().any(|a| matches!(&a.want, Want::Act { name, action: Action::Restart, .. } if name == "shop-api-1")));
    assert!(w.tree().contains("Restarted shop-api-1"));
    // Done: it looks again at once.
    assert!(w.lists() >= 2);
    w.click("ctr-3-start");
    assert!(w.tree().contains("Started nightly-backup"), "{}", w.tree());
    assert!(w.tree().contains("4 containers, 4 running"), "{}", w.tree());
    // A failure says what the engine said. Running now, nightly-backup comes before stuck,
    // whose Stop is where Start was: a second later, so it isn't a double click.
    w.rt.frame(1.0);
    w.click("ctr-3-stop");
    assert!(w.tree().contains("Couldn't stop stuck: Error response from daemon: timeout"), "{}", w.tree());
    w.click("containers-note-close");
    assert!(!w.tree().contains("Couldn't stop stuck"));
}

#[test]
fn logs_and_a_shell_open_in_new_terminal_tabs_inside_the_container() {
    let mut w = Window::open();
    w.open_tab(TabKind::Containers, WEB);
    w.click("ctr-0-logs");
    let tab = w.rt.state.bench.active().cloned().unwrap();
    assert_eq!((tab.kind, tab.label(), tab.server.as_str()), (TabKind::Terminal, "shop-api-1 logs".to_owned(), WEB));
    let inside = tab.container.clone().unwrap();
    assert_eq!((inside.id.as_str(), inside.run.clone(), inside.engine.clone()), ("a1b2c3d4e5f6", ContainerRun::Logs, Engine::Docker));
    // The backend was asked for a terminal into it.
    assert_eq!(w.world.lock().unwrap().terminals.last().and_then(|t| t.container.clone()), Some(inside));
    w.rt.update(|s, tx| s.open_containers(WEB, tx));
    w.settle();
    w.click("ctr-1-shell");
    assert_eq!(w.rt.state.bench.active().unwrap().label(), "shop-db-1");
    // A stopped container offers no shell.
    w.rt.update(|s, tx| s.open_containers(WEB, tx));
    w.settle();
    assert!(w.rt.find("ctr-3-shell").is_none() && w.rt.find("ctr-3-logs").is_some());
}

/// "Duplicate tab" on a terminal into a container opens another one into it, under its name,
/// not a shell on the server.
#[test]
fn a_duplicated_container_terminal_runs_in_the_container_too() {
    let mut w = Window::open();
    w.open_tab(TabKind::Containers, WEB);
    w.click("ctr-1-shell");
    let like = w.rt.state.bench.active().cloned().unwrap();
    w.rt.update(|s, tx| s.duplicate_tab(like.id, tx));
    w.settle();
    let copy = w.rt.state.bench.active().cloned().unwrap();
    assert_ne!(copy.id, like.id);
    assert_eq!((copy.kind, copy.label(), copy.server.as_str()), (TabKind::Terminal, "shop-db-1".to_owned(), WEB));
    assert_eq!(copy.container, like.container);
    assert_eq!(w.world.lock().unwrap().terminals.last().and_then(|t| t.container.clone()), like.container);
}

#[test]
fn a_machine_without_docker_says_how_to_install_it() {
    let mut w = Window::open();
    w.open_tab(TabKind::Containers, DB);
    let tree = w.tree();
    assert!(tree.contains("No Docker here") && tree.contains(sshee_app::containers::DOCKER_INSTALL), "{tree}");
    // "Type it in a terminal" types it at a prompt there, without Return.
    w.click("containers-fix-type");
    let active = w.rt.state.bench.active().unwrap();
    assert_eq!((active.kind, active.server.as_str()), (TabKind::Terminal, DB));
}

#[test]
fn streamer_mode_hides_names_in_containers() {
    let mut w = Window::open();
    w.world.lock().unwrap().web.push(c("e5f6a1b2c3d4", "web-7731-proxy", "running", "Up 1 hour", None));
    w.open_tab(TabKind::Containers, WEB);
    assert!(w.tree().contains("web-7731-proxy"));
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.settle();
    assert!(!w.tree().contains("web-7731-proxy"), "{}", w.tree());
}

#[test]
fn the_stats_tab_shows_numbers_graphs_and_the_busiest_programs() {
    let mut w = Window::open();
    // Before a connection: what to do.
    w.open_tab(TabKind::Stats, WEB);
    assert!(w.tree().contains("Connect first"), "{}", w.tree());
    assert!(w.world.lock().unwrap().programs.is_empty(), "no programs are asked of a machine not connected");
    // Connected, with numbers and the facts of a full look.
    w.rt.update(|s, _| {
        let id = s.bench.tabs()[0].id;
        let other = s.bench.open(TabKind::Terminal, WEB);
        s.bench.set_link(other, Link::Connected { durable: true });
        s.bench.focus(id);
        for cpu in [0.2, 0.3, 0.38] {
            s.bench.set_vitals(WEB, Some(Vitals { cpu: Some(cpu), memory: 0.6, disk: Some((0.44, None)), uptime_secs: 3_600, down: Some(1000.0), up: Some(10.0) }));
        }
        let facts = Facts { hostname: "web-7731".into(), kernel: "Linux 6.8.0".into(), cores: 8, load: [0.42, 0.38, 0.3], mem_total: 8 << 30, mem_available: 3 << 30, ..Facts::default() };
        crate::screens::stats::set_facts(s, WEB.into(), Ok(facts));
    });
    w.settle();
    let tree = w.tree();
    for said in ["CPU 38%", "Load 0.42  0.38  0.30", "Memory 60%", "Busiest programs", "node", "12.5%", "Last 5 minutes", "web-7731, Linux 6.8.0, up 1 h 0 min"] {
        assert!(tree.contains(said), "{said} isn't in: {tree}");
    }
    assert_eq!(w.world.lock().unwrap().programs.len(), 1);
    // Programs again after five seconds while the tab is on screen.
    w.rt.frame(5.5);
    w.settle();
    assert_eq!(w.world.lock().unwrap().programs.len(), 2);
    // While streaming, the machine's own name isn't said: the window's alias is.
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.settle();
    assert!(w.tree().contains("Server 1, Linux 6.8.0"), "{}", w.tree());
}

#[test]
fn the_servers_card_shows_its_load_while_it_is_looked_at() {
    let mut w = Window::open();
    w.rt.update(|s, _| {
        let id = s.bench.open(TabKind::Terminal, WEB);
        s.bench.set_vitals(WEB, Some(Vitals { cpu: Some(0.38), memory: 0.6, disk: Some((0.92, Some("/var".into()))), uptime_secs: 3_600, down: None, up: None }));
        s.bench.set_link(id, Link::Connected { durable: true });
    });
    w.settle();
    let card = |s: &Shell, t| crate::screens::stats::card_load(s, WEB, t).is_some();
    assert!(card(&w.rt.state, crate::theme::Theme::of(true)));
    // Gone: no load, rather than old numbers.
    w.rt.update(|s, _| {
        let id = s.bench.tabs()[0].id;
        s.bench.set_link(id, Link::Disconnected);
    });
    assert!(!card(&w.rt.state, crate::theme::Theme::of(true)));
}
