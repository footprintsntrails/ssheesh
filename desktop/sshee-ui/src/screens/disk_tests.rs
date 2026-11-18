//! "Free up disk space" without a screen (`Runtime::with_text`): a fake backend that answers
//! at once through the window's queue, clicks by element key, and the screen reader's tree.

use std::sync::{Arc, Mutex};

use rune::{Env, Key, KeyEvent, Mods, Runtime, Tx};
use sshee_app::disk::{Ask, Cleanup, Choice, Reply, Report, Usage, Volume};
use sshee_app::labels::{Listed, Looks};
use sshee_app::model::{EnvLabel, Settings};
use sshee_app::recipes::Outcome;
use sshee_app::servers::{Book, Sources};
use sshee_app::settings_store::SettingsStore;
use sshee_app::workbench::{Server, Tab, Workbench};
use sshee_term::engine::Size;
use sshee_term::model::Model;

use crate::shell::view::view;
use crate::shell::{Backend, Options, Post, Remote, Shell};
use crate::terminal::{PaneOptions, StatusSink, TermLink};
use crate::theme::Frame;

const WEB: &str = "host:web";
const STAGING: &str = "host:staging";
const GB: u64 = 1 << 30;

struct NoLink;

impl TermLink for NoLink {
    fn open(&mut self, _: Arc<Model>, _: Size, _: StatusSink) {}
    fn write(&self, _: &[u8]) {}
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {}
    fn close(&self) {}
    fn end(&self) {}
}

/// The made-up disk: `/` at 71 of 80 GB until a cleanup takes what it frees off.
#[derive(Default)]
struct World {
    asks: Vec<Ask>,
    freed: u64,
    /// Cleanups that ran are no longer offered, as on a server.
    cleaned: Vec<String>,
}

struct FakeBackend {
    world: Arc<Mutex<World>>,
    remote: Option<Remote>,
}

fn cleanups() -> Vec<Cleanup> {
    let choice = |label: &str, days: u32, bytes: u64| Choice { label: label.into(), command: format!("sudo find /tmp -mtime +{days} -delete"), bytes: Some(bytes) };
    vec![
        Cleanup { id: "journal".into(), title: "Old system logs".into(), detail: "Trims the journal of web-7731.".into(), command: "sudo journalctl --vacuum-size=200M".into(), bytes: Some(GB), choices: Vec::new() },
        Cleanup {
            id: "temp".into(),
            title: "Temporary files".into(),
            detail: "Old files in /tmp.".into(),
            command: "sudo find /tmp -mtime +7 -delete".into(),
            bytes: Some(GB / 2),
            choices: vec![choice("Older than a day", 1, 2 * GB), choice("Older than a week", 7, GB / 2)],
        },
    ]
}

impl Backend for FakeBackend {
    fn start(&mut self, remote: Remote) {
        self.remote = Some(remote);
    }
    fn terminal(&mut self, _: Option<&Server>, _: &Tab) -> Box<dyn TermLink> {
        Box::new(NoLink)
    }
    fn settings(&mut self) -> SettingsStore {
        SettingsStore::in_memory(Settings { onboarding_version: sshee_app::tour::ONBOARDING_VERSION, ..Settings::default() })
    }
    fn disk(&mut self, ask: Ask) {
        let mut world = self.world.lock().unwrap();
        world.asks.push(ask.clone());
        let reply = match ask {
            Ask::Report { server } => Reply::Report {
                server,
                result: Ok(Report {
                    volumes: vec![Volume { mount: "/srv/web-7731".into(), size: 80 * GB, used: 71 * GB - world.freed }],
                    docker: None,
                    journal: Some(GB),
                    cleanups: cleanups().into_iter().filter(|c| !world.cleaned.contains(&c.id)).collect(),
                    as_root: false,
                }),
            },
            Ask::Biggest { server, path } => {
                let at = |name: &str| if path == "/" { format!("/{name}") } else { format!("{path}/{name}") };
                let list = vec![Usage { path: at("var"), name: "var".into(), bytes: 30 * GB, is_dir: true }, Usage { path: at("swapfile"), name: "swapfile".into(), bytes: 2 * GB, is_dir: false }];
                Reply::Biggest { server, path, result: Ok(list) }
            }
            Ask::Clean { server, id, .. } => {
                world.freed += GB;
                world.cleaned.push(id.clone());
                Reply::Cleaned { server, id, outcome: Outcome::Done { output: String::new() } }
            }
        };
        drop(world);
        if let Some(remote) = &self.remote {
            remote.disk(reply);
        }
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
}

impl Window {
    fn open() -> Window {
        crate::fonts::declare();
        let host = |id: &str, name: &str, address: &str| Listed { id: id.into(), name: name.into(), address: address.into(), from_config: false };
        let mut looks = Looks::default();
        looks.set_env("web", Some(EnvLabel::Prod));
        let book = Book::new(vec![host("web", "web-7731", "deploy@203.0.113.17"), host("staging", "web-staging", "deploy@203.0.113.18")], looks, Sources::default());
        let world = Arc::new(Mutex::new(World::default()));
        let posted: Queue = Arc::new(Mutex::new(Vec::new()));
        let queue = posted.clone();
        let post: Post = Arc::new(move |f| queue.lock().unwrap().push(f));
        let options = Options { pane: PaneOptions { blink: false, ..Options::default().pane }, frame: Frame::Linux, mac_keys: false, ..Options::default() };
        let backend = FakeBackend { world: world.clone(), remote: None };
        let shell = Shell::new(Workbench::new(book.servers()), Box::new(backend), options, Some(Remote::new(post)));
        let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
        let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
        rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
        rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
        let mut w = Window { rt, world, posted };
        w.settle();
        w
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

    fn cleans(&self) -> Vec<String> {
        self.asks().into_iter().filter_map(|a| if let Ask::Clean { command, .. } = a { Some(command) } else { None }).collect()
    }

    fn open_on(&mut self, server: &'static str) {
        self.rt.update(move |s, tx| s.open_disk(server, tx));
        self.settle();
    }

    fn focused(&self) -> Option<String> {
        self.rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key)
    }
}

#[test]
fn the_sidebar_drive_shows_the_disks_cleanups_and_biggest_folders() {
    let mut w = Window::open();
    w.click("server-2");
    w.click("disk");
    assert!(w.has("disk-dialog"));
    assert_eq!(w.asks()[..2], [Ask::Report { server: STAGING.into() }, Ask::Biggest { server: STAGING.into(), path: "/".into() }]);
    let tree = w.tree();
    for said in ["Free up disk space", "web-staging · nothing is deleted until you run a cleanup", "/srv/web-7731: 71 GB of 80 GB · 89%", "Old system logs", "frees about 1.0 GB", "sudo journalctl --vacuum-size=200M", "var, 30 GB", "System logs (journal): 1.0 GB", "Some of this needs root."] {
        assert!(tree.contains(said), "{said}: {tree}");
    }
    // A folder opens on a press; the crumbs go back up.
    w.click("big-0");
    assert_eq!(w.asks().last(), Some(&Ask::Biggest { server: STAGING.into(), path: "/var".into() }));
    assert!(w.tree().contains("/var/var") || w.has("disk-crumb-1"));
    w.click("disk-crumb-0");
    assert_eq!(w.asks().last(), Some(&Ask::Biggest { server: STAGING.into(), path: "/".into() }));
    // A file doesn't open.
    let before = w.asks().len();
    w.click("big-1");
    assert_eq!(w.asks().len(), before);
    w.key(Key::Escape);
    assert!(!w.has("disk-dialog") && w.cleans().is_empty());
}

#[test]
fn a_cleanup_runs_after_one_yes_says_what_it_freed_and_production_asks_twice() {
    let mut w = Window::open();
    w.open_on(STAGING);
    w.click("clean-run-0");
    assert!(w.tree().contains("Run this on web-staging?"));
    assert_eq!(w.focused().as_deref(), Some("clean-no-0"), "the question opens on Cancel");
    // Return never says yes.
    w.key(Key::Enter);
    assert!(w.cleans().is_empty());
    if !w.has("clean-yes-0") {
        w.click("clean-run-0");
    }
    w.click("clean-yes-0");
    assert_eq!(w.cleans(), ["sudo journalctl --vacuum-size=200M"]);
    // Looked at again: no longer offered, and what it freed is said under its name.
    assert!(w.has("clean-done-journal"), "{}", w.tree());
    assert!(w.tree().contains("Done. Freed 1.0 GB."), "{}", w.tree());
    w.key(Key::Escape);

    // Production: one yes only leads to the second question, which Return doesn't press.
    w.open_on(WEB);
    assert!(w.tree().contains("Cleanups here ask twice."));
    w.click("clean-run-0");
    w.click("clean-yes-0");
    assert_eq!(w.cleans().len(), 1, "one yes isn't enough on Production");
    assert!(w.tree().contains("web-7731 is a Production server"));
    assert_eq!(w.focused().as_deref(), Some("clean-no-0"));
    w.key(Key::Enter);
    assert_eq!(w.cleans().len(), 1, "Return doesn't press the Production yes");
    if !w.has("clean-prod-0") {
        w.click("clean-run-0");
        w.click("clean-yes-0");
    }
    w.click("clean-prod-0");
    assert_eq!(w.cleans().len(), 2);
}

#[test]
fn a_strength_picks_the_command_that_runs() {
    let mut w = Window::open();
    w.open_on(STAGING);
    assert!(w.tree().contains("sudo find /tmp -mtime +7 -delete"));
    w.rt.update(|s, _| {
        s.screens.disk.picks.insert("temp".into(), 0);
    });
    w.settle();
    assert!(w.tree().contains("frees about 2.0 GB"));
    w.click("clean-run-1");
    // Picking another strength while asked drops the question.
    w.rt.update(|s, _| s.disk_pick("temp", 1));
    w.settle();
    assert!(!w.has("clean-ask-1"));
    w.click("clean-run-1");
    w.click("clean-yes-1");
    assert_eq!(w.cleans(), ["sudo find /tmp -mtime +7 -delete"]);
}

#[test]
fn streamer_mode_hides_the_server_and_paths_that_name_it() {
    let mut w = Window::open();
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.settle();
    w.open_on(STAGING);
    let tree = w.tree();
    assert!(!tree.contains("web-staging") && !tree.contains("web-7731") && !tree.contains("203.0.113"), "{tree}");
    assert!(tree.contains("Server 2 · nothing is deleted"), "{tree}");
}

#[test]
fn from_the_main_menu_it_asks_which_server_and_the_server_menu_opens_it() {
    let mut w = Window::open();
    w.click("disk");
    assert!(w.tree().contains("Which server?") && w.asks().is_empty());
    w.click("disk-pick-0");
    assert_eq!(w.asks()[0], Ask::Report { server: WEB.into() });
    w.key(Key::Escape);
    // A server's menu: the row after Tunnels.
    let (x, y) = w.rt.find("server-2").expect("the sidebar's web-staging");
    w.rt.secondary_down(x, y);
    w.rt.secondary_up(x, y);
    w.settle();
    assert!(w.tree().contains("Free up disk space…"));
    w.click("sm-6");
    assert_eq!(w.rt.state.screens.disk.open.as_deref(), Some(STAGING));
}

#[test]
fn dockers_tiles_have_a_question_mark_that_says_what_each_is() {
    let mut w = Window::open();
    w.open_on(STAGING);
    assert!(!w.has("disk-docker-about"), "no Docker, no tiles");
    w.rt.update(|s, _| s.screens.disk.report.as_mut().unwrap().docker = Some(sshee_app::disk::Docker { images: 4 * GB, images_reclaimable: GB, ..Default::default() }));
    w.settle();
    assert!(w.has("disk-docker-about"));
    assert_eq!(crate::screens::help::heard(&w.rt, "Docker's disk use").as_deref(), Some(crate::screens::help::DOCKER_DISK));
}
