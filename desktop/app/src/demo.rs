//! `--demo`: the window on made-up servers and tabs, with made-up terminal output played from
//! fixtures. Nothing is read from this computer (no server list, no settings, no keychain)
//! and nothing connects. It's the scene the 1.0 design was drawn with
//! (research_notes/v1-plan/fresh-apple.html), so a screenshot can be laid beside the design,
//! and it's what the headless tests and README pictures run.
//!
//! Every name and address is made up: `web-7731`, `203.0.113.x` (a range set aside for
//! documentation). Secrets in the output are made at run time, never written in a fixture.
//! Each server's files are a made-up tree in memory (`files`).

mod containers;
mod disk;
mod edit;
mod files;
mod recipes;
mod tor;

use std::path::Path;

use serde_json::Value;
use sshee_app::labels::{Listed, Looks};
use sshee_app::model::{EnvLabel, Settings};
use sshee_app::settings_store::SettingsStore;
use sshee_app::servers::{App, AppOffer, Ask, Book, Candidate, KeyAgent, LocalAuth, LocalKey, Sources};
use sshee_app::workbench::{Open, Server, Tab, TabKind, Tor, Vitals, Workbench};
use sshee_ui::screens::settings::TorCommand;
use sshee_ui::shell::{Backend, Remote};
use sshee_ui::terminal::replay::{Fixture, ReplayLink};
use sshee_ui::terminal::TermLink;

const WEB: &str = "host:demo-web";
const STAGING: &str = "host:demo-staging";
const BACKUPS: &str = "host:demo-backups";
const DB: &str = "host:demo-db";

/// How many lines web-staging's log has: several screens of history.
const LOG_LINES: usize = 240;

/// The shop server's terminal, and the agent's on the database.
const SHOP: &str = include_str!("../fixtures/shop.json");
const AGENT: &str = include_str!("../fixtures/agent.json");

/// The made-up list of servers: four in the sidebar, two more in a made-up `~/.ssh/config`
/// not picked yet, two XPipe would offer, two PuTTY would and one 0.10 would. All in memory.
pub fn book() -> Book {
    let host = |id: &str, name: &str, address: &str, from_config: bool| Listed { id: id.into(), name: name.into(), address: address.into(), from_config };
    let hosts = vec![
        host("demo-web", "web-7731", "deploy@203.0.113.17", false),
        host("demo-staging", "web-staging", "deploy@203.0.113.18", false),
        host("demo-backups", "backups", "backup@203.0.113.24", false),
        host("demo-db", "db-main", "postgres@203.0.113.31", false),
        host("cfg:cache-7731", "cache-7731", "deploy@203.0.113.40", true),
        host("cfg:build-box", "build-box", "ci@203.0.113.52:2222", true),
    ];
    let mut looks = Looks::default();
    for (id, icon, env) in [("demo-web", "globe", Some(EnvLabel::Prod)), ("demo-staging", "flask", Some(EnvLabel::Staging)), ("demo-backups", "backup", None), ("demo-db", "database", Some(EnvLabel::Prod))] {
        looks.set_icon(id, icon);
        looks.set_env(id, env);
    }
    let offer = |name: &str, address: &str, had_password: bool| Candidate { id: address.into(), name: name.into(), address: address.into(), identity_file: None, had_password, picked: true };
    let sources = Sources {
        xpipe: vec![offer("mail-7731", "root@203.0.113.25", true), offer("git-7731", "git@203.0.113.26:2200", false)],
        xpipe_through_jump: 1,
        old: vec![offer("metrics", "ops@203.0.113.60", false)],
        apps: vec![AppOffer {
            app: App::Putty,
            servers: vec![offer("files-7731", "ops@198.51.100.70", false), offer("vpn-7731", "admin@198.51.100.71:2222", true)],
            here: 1,
            through_jump: 0,
            ppk_keys: 1,
        }],
        ..Sources::default()
    };
    let mut book = Book::new(hosts, looks, sources);
    // Keys and a password manager "found" here, and a password saved for the database.
    book.local = LocalAuth {
        keys: vec![
            LocalKey { path: "~/.ssh/id_ed25519".into(), name: "id_ed25519".into(), kind: "ED25519".into(), comment: "someone@laptop".into() },
            LocalKey { path: "~/.ssh/deploy_rsa".into(), name: "deploy_rsa".into(), kind: "RSA".into(), comment: String::new() },
        ],
        agents: vec![KeyAgent { name: "ssh-agent".into(), socket: None, keys: Some(1) }, KeyAgent { name: "1Password".into(), socket: Some("~/.1password/agent.sock".into()), keys: Some(3) }],
    };
    book.passwords.push("demo-db".into());
    // The web server's identity is known; a new one shows a fingerprint first.
    book.trusted.push("203.0.113.17".into());
    book
}

/// The made-up servers, tabs and numbers.
pub fn bench() -> Workbench {
    let mut servers = vec![Server::this_computer()];
    servers.extend(book().servers());
    let mut bench = Workbench::new(servers);
    let shop = bench.open_with(TabKind::Terminal, WEB, Open { title: Some("~/shop".into()), ..Open::default() });
    bench.open(TabKind::Files, WEB);
    let agent = bench.open_with(TabKind::Terminal, DB, Open { title: Some("Migration 0042".into()), agent: Some("Claude Code".into()), ..Open::default() });
    bench.focus(shop);
    bench.set_needs_you(agent, true);
    bench.set_tor(Tor::On { latency_ms: Some(tor::LATENCY_MS) });
    let web = Vitals { cpu: Some(0.38), memory: 0.6, disk: Some((0.44, None)), uptime_secs: 37 * 86_400 + 5 * 3_600, down: Some(1.6 * 1_048_576.0), up: Some(315.0 * 1024.0) };
    let db = Vitals { cpu: Some(0.12), memory: 0.71, disk: Some((0.92, None)), uptime_secs: 212 * 86_400 + 9 * 3_600, down: Some(48.0 * 1024.0), up: Some(22.0 * 1024.0) };
    // Five made-up minutes before now, for the Stats page's graphs.
    for (server, now, seed) in [(WEB, web, 1), (DB, db, 2)] {
        for v in containers::history(&now, seed) {
            bench.set_vitals(server, Some(v));
        }
        bench.set_vitals(server, Some(now));
    }
    bench
}

/// The demo's settings: 0.10's defaults, kept in memory, following the system's light or
/// dark so that `--theme` picks the picture.
pub fn settings() -> Settings {
    // Welcomed already, so the scene is the design's (`--demo-welcome` shows the welcome).
    Settings { theme_follows_system: true, onboarded: true, onboarding_version: sshee_app::tour::ONBOARDING_VERSION, ..Settings::default() }
}

/// Sessions that play fixtures, and a list of servers kept in memory: adding, importing and
/// renaming work, and are gone when the window closes; and files that live in memory.
pub struct DemoBackend {
    book: Book,
    remote: Option<Remote>,
    files: files::DemoFiles,
    /// "Open in your own editor", without an editor: the status flow only.
    edit: edit::DemoEdit,
    /// A made-up newer test build is ready at start (`--demo-update`).
    update: bool,
    /// `--demo-welcome`: the welcome, as at a first start.
    welcome: bool,
    /// `--demo-whats-new`: "What's new" opens, as after an update from 0.10.14.
    whats_new: bool,
    /// What the first terminal on backups shows (`--demo-card`): by default, its key's
    /// question, as a server seen for the first time.
    card: String,
    /// Each machine's made-up containers.
    containers: containers::DemoContainers,
    /// What each machine's made-up disks have had cleaned.
    disk: disk::DemoDisk,
}

impl Default for DemoBackend {
    fn default() -> Self {
        DemoBackend::new()
    }
}

impl DemoBackend {
    /// Two copies "left from an earlier run" are offered at start (`--demo-leftovers`).
    pub fn with_leftovers(mut self, on: bool) -> DemoBackend {
        self.edit.leftovers = on;
        self
    }

    /// A made-up newer test build is ready at start (`--demo-update`); restarting into it
    /// says why it can't.
    pub fn with_update(mut self, on: bool) -> DemoBackend {
        self.update = on;
        self
    }

    pub fn new() -> DemoBackend {
        DemoBackend { book: book(), remote: None, files: files::DemoFiles::new(WEB, STAGING, BACKUPS, DB), edit: edit::DemoEdit::default(), welcome: false, whats_new: false, card: "hostkey-new".into(), containers: containers::DemoContainers::new(), disk: disk::DemoDisk::default(), update: false }
    }

    /// The welcome at the start (`--demo-welcome`), "What's new" (`--demo-whats-new`).
    pub fn with_first_run(mut self, welcome: bool, whats_new: bool) -> DemoBackend {
        self.welcome = welcome;
        self.whats_new = whats_new;
        self
    }

    /// The first terminal on backups shows `card` (`--demo-card`), not its key's question.
    pub fn with_card(mut self, card: Option<String>) -> DemoBackend {
        if let Some(card) = card {
            self.card = card;
        }
        self
    }

    /// Tells the window what the list holds now.
    fn show(&self, answer: Option<sshee_app::servers::Answer>) {
        let Some(remote) = &self.remote else { return };
        let (servers, colors, config_new, old_new) = (self.book.servers(), self.book.colors(), self.book.config_new(), self.book.sources.old.len());
        let faces = self.book.faces(crate::servers::picture);
        remote.update(move |s| {
            let mut all = vec![Server::this_computer()];
            all.extend(servers);
            s.bench.set_servers(all);
            s.set_server_looks(colors, config_new, old_new);
            s.set_server_faces(faces);
            if let Some(answer) = answer {
                s.servers_answer(answer);
            }
        });
    }
}

/// A made-up picture for a server: a small square of soft colour, as a PNG data URL.
pub fn made_up_picture() -> String {
    let side = sshee_app::labels::IMAGE_SIDE;
    let picture = image::RgbaImage::from_fn(side, side, |x, y| image::Rgba([40 + (x * 3) as u8, 120 + (y * 2) as u8, 200, 255]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(picture).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).expect("a PNG in memory");
    sshee_app::labels::image_data_url(&png)
}

/// A fixture written in this crate: it reads, or the build is broken.
fn fixture(json: &str) -> Fixture {
    let value: Value = serde_json::from_str(json).expect("a fixture of this crate is JSON");
    Fixture::from_json(&value, Path::new("."), 1).expect("a fixture of this crate reads")
}

/// A shell prompt on `name` that echoes what is typed: every other terminal of the demo.
fn prompt(user: &str, name: &str) -> Fixture {
    let out = format!("\u{1b}[32m{user}@{name}\u{1b}[0m:\u{1b}[34m~\u{1b}[0m$ ");
    let value = serde_json::json!({ "events": [ { "t": 0.0, "out": out }, { "t": 0.0, "echo": true } ] });
    Fixture::from_json(&value, Path::new("."), 1).expect("a prompt reads")
}

/// web-staging's first terminal: a worker's log, long enough to scroll back through and with
/// things to find and click in it (requests, warnings, errors, a web link, a link a program
/// marked itself, file paths, a secret), then a prompt. The shell says which folder it is in
/// (OSC 7), so the tab is named after it. Three seconds in, two more lines arrive: what "Jump to
/// the bottom" is for when the view is scrolled up by then.
fn log() -> Fixture {
    let mut out = String::from("\u{1b}]7;file://web-staging/home/deploy/shop\u{7}");
    out.push_str("\u{1b}[32mdeploy@web-staging\u{1b}[0m:\u{1b}[34m~/shop\u{1b}[0m$ tail -n 240 /var/log/shop/worker.log\r\n");
    for n in 1..=LOG_LINES {
        let time = format!("\u{1b}[90m14:{:02}:{:02}\u{1b}[0m", n / 60, n % 60);
        let line = match n {
            40 => "\u{1b}[33mWARN \u{1b}[0m Slow query (812 ms): see src/db/orders.rs:88".to_owned(),
            90 => "\u{1b}[1;31mERROR\u{1b}[0m Payment provider timed out, details at https://status.shop.example/incidents/42".to_owned(),
            120 => "\u{1b}[36mINFO \u{1b}[0m Reloaded /etc/shop/worker.toml".to_owned(),
            150 => "\u{1b}[36mINFO \u{1b}[0m Token rotated: API_TOKEN={{SECRET_A}}".to_owned(),
            200 => "\u{1b}[1;31mERROR\u{1b}[0m Payment provider timed out, retrying in 30 s".to_owned(),
            230 => "\u{1b}[36mINFO \u{1b}[0m Runbook: \u{1b}]8;;https://docs.shop.example/runbooks/payments\u{1b}\\payments runbook\u{1b}]8;;\u{1b}\\".to_owned(),
            _ => format!("\u{1b}[36mINFO \u{1b}[0m GET /api/orders/{} 200 {} ms", 1000 + n, 8 + n % 23),
        };
        out.push_str(&format!("{time} {line}\r\n"));
    }
    let prompt = "\u{1b}[32mdeploy@web-staging\u{1b}[0m:\u{1b}[34m~/shop\u{1b}[0m$ ";
    out.push_str(prompt);
    let late = format!("\r\n\u{1b}[90m14:04:12\u{1b}[0m \u{1b}[36mINFO \u{1b}[0m Worker restarted\r\n\u{1b}[90m14:04:13\u{1b}[0m \u{1b}[36mINFO \u{1b}[0m Listening on 8080\r\n{prompt}");
    let value = serde_json::json!({ "events": [ { "t": 0.0, "out": out }, { "t": 0.0, "echo": true }, { "t": 3.0, "out": late } ] });
    Fixture::from_json(&value, Path::new("."), 1).expect("the log reads")
}

/// backups' first terminal: a prompt in tmux, then the session in `state` (`replay::status_named`).
/// Connected (`--demo-card connected`), a backup runs and says how far it is (OSC 9;4): its tab
/// and the line along the terminal's top show it.
fn card(state: &str) -> Fixture {
    let listed = "\u{1b}[32mbackup@backups\u{1b}[0m:\u{1b}[34m~\u{1b}[0m$ ls /srv/backups\r\n2026-11-12.tar.zst  2026-11-13.tar.zst  2026-11-14.tar.zst\r\n\u{1b}[32mbackup@backups\u{1b}[0m:\u{1b}[34m~\u{1b}[0m$ ";
    let running = format!("{listed}./backup.sh\r\nDumping shop-db to 2026-11-15.tar.zst… 62%\u{1b}]9;4;1;62\u{7}");
    let out = if state == "connected" { running.as_str() } else { listed };
    // Nothing is on screen before a server's key is trusted: no session has started.
    let out = if state.starts_with("hostkey") { "" } else { out };
    let value = serde_json::json!({ "durable": true, "events": [ { "t": 0.0, "out": out }, { "t": 0.0, "echo": true }, { "t": 0.0, "status": state } ] });
    Fixture::from_json(&value, Path::new("."), 1).expect("a card's fixture reads")
}

/// A terminal into a made-up container: a root prompt there, or its log as it comes.
fn container(inside: &sshee_app::model::ContainerRef) -> Fixture {
    let events = if inside.run == sshee_app::model::ContainerRun::Logs {
        let line = |t: &str, what: &str| format!("\u{1b}[90m2026-11-14T{t}Z\u{1b}[0m {what}\r\n");
        let first: String = [
            line("10:02:11", "\u{1b}[36mINFO \u{1b}[0m listening on :8080"),
            line("10:02:14", "\u{1b}[36mINFO \u{1b}[0m GET /api/health 200 2 ms"),
            line("10:02:19", "\u{1b}[36mINFO \u{1b}[0m GET /api/orders/1042 200 18 ms"),
            line("10:02:21", "\u{1b}[33mWARN \u{1b}[0m slow query (412 ms): orders by customer"),
            line("10:02:24", "\u{1b}[36mINFO \u{1b}[0m POST /api/checkout 201 64 ms"),
        ]
        .concat();
        serde_json::json!({ "events": [ { "t": 0.0, "out": first }, { "t": 2.0, "out": line("10:02:26", "\u{1b}[36mINFO \u{1b}[0m GET /api/orders/1043 200 15 ms") } ] })
    } else {
        let id: String = inside.id.chars().take(12).collect();
        let out = format!("\u{1b}[32mroot@{id}\u{1b}[0m:\u{1b}[34m/app\u{1b}[0m# ");
        serde_json::json!({ "events": [ { "t": 0.0, "out": out }, { "t": 0.0, "echo": true } ] })
    };
    Fixture::from_json(&events, Path::new("."), 1).expect("a container's fixture reads")
}

impl Backend for DemoBackend {
    fn start(&mut self, remote: Remote) {
        // The scene's agent has waited two minutes, as the design's card says.
        remote.update(|s| {
            if let Some(waits) = s.bench.tabs().iter().find(|t| t.needs_you).map(|t| t.id) {
                s.backdate_wait(waits, 2.0 * 60.0);
            }
            // The agents' own clock of it: the card under the tab says "waiting 2 min" too.
            sshee_ui::screens::agent::seed_wait(s, 130);
        });
        if self.update {
            remote.update(|s| sshee_ui::screens::canary::show(s, sshee_app::canary::Notice::Ready { version: "1.0.0-canary.125".into() }));
        }
        tor::start(&remote);
        tor::tunnels_start(&remote);
        // What a full look at the machines with numbers would say (the Stats page).
        for (server, facts) in containers::facts() {
            remote.set_facts(server.to_owned(), Ok(facts));
        }
        self.remote = Some(remote);
        self.show(None);
    }

    fn servers(&mut self, ask: Ask) {
        // Nothing is read: a picture picked becomes a made-up one.
        let answer = match ask {
            Ask::SetImage { server, .. } => match sshee_app::servers::host_id(&server) {
                Some(id) => {
                    self.book.looks.set_image(id, Some(made_up_picture()));
                    sshee_app::servers::Answer::Changed
                }
                None => self.book.handle(Ask::SetImage { server, path: Default::default() }),
            },
            ask => self.book.handle(ask),
        };
        self.show(Some(answer));
    }

    /// Answered at once from the made-up tree (nothing here waits), but still through the
    /// window's queue, as a real backend's answers arrive: after the handler that asked.
    /// No editor starts and nothing is copied: a file's status goes through what a real
    /// session says, through the window's queue as a real backend's news arrives.
    fn edit(&mut self, ask: sshee_app::files::edit::EditAsk) {
        let replies = self.edit.answer(ask, &mut self.files);
        if let Some(remote) = &self.remote {
            for reply in replies {
                remote.edit(reply);
            }
        }
    }

    fn files(&mut self, ask: sshee_app::files::Ask) {
        let replies = self.files.answer(&ask);
        if let Some(remote) = &self.remote {
            for reply in replies {
                remote.files(reply);
            }
        }
    }

    /// Answered from the made-up machines at once, through the window's queue.
    fn containers(&mut self, ask: sshee_app::containers::Ask) {
        let answer = self.containers.answer(&ask);
        if let Some(remote) = &self.remote {
            remote.containers(ask.reply(answer));
        }
    }

    fn stats(&mut self, ask: sshee_app::stats::Ask) {
        if let Some(remote) = &self.remote {
            remote.stats(ask.reply(Ok(containers::programs(&ask.server))));
        }
    }

    fn settings(&mut self) -> SettingsStore {
        let mut settings = settings();
        if self.welcome {
            settings.onboarding_version = 2;
        }
        if self.whats_new {
            settings.last_seen_version = "0.10.14".into();
        }
        SettingsStore::in_memory(settings)
    }

    /// Made-up findings, fixes that run at once, a made-up key (`recipes`): through the
    /// window's queue, after the handler that asked.
    fn recipes(&mut self, ask: sshee_app::recipes::Ask) {
        if let Some(remote) = &self.remote {
            remote.recipes(recipes::answer(ask));
        }
    }

    /// Made-up disks, folders and cleanups (`disk`): through the window's queue.
    fn disk(&mut self, ask: sshee_app::disk::Ask) {
        let reply = self.disk.answer(ask);
        if let Some(remote) = &self.remote {
            remote.disk(reply);
        }
    }

    fn snippets(&mut self) -> Option<sshee_app::model::Snippets> {
        Some(recipes::snippets())
    }

    fn welcome(&mut self) -> bool {
        self.welcome
    }

    fn version(&mut self) -> Option<String> {
        self.whats_new.then(|| env!("CARGO_PKG_VERSION").to_owned())
    }

    /// Tor's switch: the made-up Tor goes off at once, and on after a made-up start.
    fn tor(&mut self, command: TorCommand) {
        if let Some(remote) = &self.remote {
            tor::command(remote, command);
        }
    }

    /// Tor's made-up log; there's no folder to show.
    fn tor_ask(&mut self, ask: sshee_app::tor::Ask) {
        if let (Some(remote), sshee_app::tor::Ask::ReadLog) = (&self.remote, ask) {
            remote.tor_log(tor::log());
        }
    }

    fn tunnels(&mut self, ask: sshee_app::tunnels::Ask) {
        if let Some(remote) = &self.remote {
            tor::tunnels(remote, ask);
        }
    }

    fn terminal(&mut self, server: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        if let Some(inside) = &tab.container {
            return Box::new(ReplayLink::new(container(inside), None));
        }
        let fixture = match (tab.server.as_str(), tab.title.as_deref()) {
            (WEB, Some("~/shop")) => fixture(SHOP),
            (DB, Some("Migration 0042")) => fixture(AGENT),
            // The first terminal there; the ones after it are plain prompts.
            (STAGING, _) if tab.n == 1 => log(),
            // A server seen for the first time: its key's question (or `--demo-card`'s state).
            (BACKUPS, _) if tab.n == 1 => card(&self.card),
            _ => match server {
                Some(s) if s.is_local() => prompt("dev", "laptop-7731"),
                // The user is the made-up address's.
                Some(s) => prompt(s.address.split('@').next().filter(|u| !u.is_empty()).unwrap_or("dev"), &s.name),
                None => prompt("dev", "unknown"),
            },
        };
        Box::new(ReplayLink::new(fixture, None))
    }

    fn restart_to_update(&mut self) -> Result<(), String> {
        Err("the demo doesn't install anything".into())
    }

    fn replays(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_is_the_designs() {
        let bench = bench();
        let names: Vec<&str> = bench.servers().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["This computer", "web-7731", "web-staging", "backups", "db-main"]);
        let row: Vec<(String, &str, bool)> = bench.strip().iter().map(|s| (s.tab.label(), s.server_name, s.active)).collect();
        assert_eq!(row, [("~/shop".into(), "web-7731", true), ("Files".into(), "web-7731", false), ("Migration 0042".into(), "db-main", false)]);
        assert_eq!(bench.window_title(), ("web-7731", true));
        assert!(bench.server_needs_you(DB) && !bench.server_needs_you(WEB));
        let shown: Vec<String> = bench.current_vitals().unwrap().items().iter().map(|v| format!("{} {}", v.label, v.value)).collect();
        assert_eq!(shown, ["CPU 38%", "Memory 60%", "Disk 44%", "Uptime 37 d 5 h", "Down 1.6 MB/s", "Up 315 KB/s"]);
        // Only documentation addresses.
        assert!(bench.servers().iter().all(|s| s.address.is_empty() || s.address.contains("@203.0.113.")));
    }

    #[test]
    fn every_fixture_reads_and_holds_no_secret() {
        for (name, json) in [("shop", SHOP), ("agent", AGENT)] {
            assert!(!fixture(json).steps.is_empty(), "{name}");
            // A token-shaped string must never sit in the repository: secrets are placeholders.
            assert!(!json.contains("ghp_") && !json.contains("sk_live"), "{name}");
        }
        assert!(SHOP.contains("{{SECRET_A:4:17}}") && SHOP.contains("{{SECRET_B}}"));
        assert_eq!(prompt("deploy", "web-staging").steps.len(), 2);
        // The shop's connection drops and comes back with two new lines, for the design's
        // "While you were away" line; the log is long enough to scroll.
        assert!(fixture(SHOP).durable && SHOP.contains("\"drop\"") && SHOP.contains("\"away_min\": 41"));
        let log = log();
        assert_eq!(log.steps.len(), 3);
        assert!(matches!(&log.steps[0].1, sshee_ui::terminal::replay::Step::Out(bytes) if bytes.iter().filter(|b| **b == b'\n').count() > LOG_LINES));
    }

    #[test]
    fn the_demos_settings_are_0_10s_defaults_following_the_system() {
        let mut backend = DemoBackend::default();
        let store = backend.settings();
        assert!(store.get().theme_follows_system && !store.get().streamer_mode && store.get().hide_secrets);
        assert_eq!(store.notice(), None);
        // Nothing of it reaches a disk.
        assert_eq!(store.save_error(), None);
    }
}
