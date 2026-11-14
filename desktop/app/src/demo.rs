//! `--demo`: the window on made-up servers and tabs, with made-up terminal output played from
//! fixtures. Nothing is read from this computer (no server list, no settings, no keychain)
//! and nothing connects. It's the scene the 1.0 design was drawn with
//! (research_notes/v1-plan/fresh-apple.html), so a screenshot can be laid beside the design,
//! and it's what the headless tests and README pictures run.
//!
//! Every name and address is made up: `web-7731`, `203.0.113.x` (a range set aside for
//! documentation). Secrets in the output are made at run time, never written in a fixture.
//! Each server's files are a made-up tree in memory (`files`).

mod files;

use std::path::Path;

use serde_json::Value;
use sshee_app::labels::{Listed, Looks};
use sshee_app::model::{EnvLabel, Settings};
use sshee_app::settings_store::SettingsStore;
use sshee_app::servers::{Ask, Book, Candidate, Sources};
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
/// not picked yet, two XPipe would offer and one 0.10 would. All in memory.
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
        ..Sources::default()
    };
    Book::new(hosts, looks, sources)
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
    bench.set_tor(Tor::On { latency_ms: Some(84) });
    bench.set_vitals(
        WEB,
        Some(Vitals { cpu: Some(0.38), memory: 0.6, disk: Some((0.44, None)), uptime_secs: 37 * 86_400 + 5 * 3_600, down: Some(1.6 * 1_048_576.0), up: Some(315.0 * 1024.0) }),
    );
    bench.set_vitals(
        DB,
        Some(Vitals { cpu: Some(0.12), memory: 0.71, disk: Some((0.92, None)), uptime_secs: 212 * 86_400 + 9 * 3_600, down: Some(48.0 * 1024.0), up: Some(22.0 * 1024.0) }),
    );
    bench
}

/// The demo's settings: 0.10's defaults, kept in memory, following the system's light or
/// dark so that `--theme` picks the picture.
pub fn settings() -> Settings {
    Settings { theme_follows_system: true, ..Settings::default() }
}

/// Sessions that play fixtures, and a list of servers kept in memory: adding, importing and
/// renaming work, and are gone when the window closes; and files that live in memory.
pub struct DemoBackend {
    book: Book,
    remote: Option<Remote>,
    files: files::DemoFiles,
}

impl Default for DemoBackend {
    fn default() -> Self {
        DemoBackend::new()
    }
}

impl DemoBackend {
    pub fn new() -> DemoBackend {
        DemoBackend { book: book(), remote: None, files: files::DemoFiles::new(WEB, STAGING, BACKUPS, DB) }
    }

    /// Tells the window what the list holds now.
    fn show(&self, answer: Option<sshee_app::servers::Answer>) {
        let Some(remote) = &self.remote else { return };
        let (servers, colors, config_new, old_new) = (self.book.servers(), self.book.colors(), self.book.config_new(), self.book.sources.old.len());
        remote.update(move |s| {
            let mut all = vec![Server::this_computer()];
            all.extend(servers);
            s.bench.set_servers(all);
            s.set_server_looks(colors, config_new, old_new);
            if let Some(answer) = answer {
                s.servers_answer(answer);
            }
        });
    }
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
        self.remote = Some(remote);
        self.show(None);
    }

    fn servers(&mut self, ask: Ask) {
        let answer = self.book.handle(ask);
        self.show(Some(answer));
    }

    /// Answered at once from the made-up tree (nothing here waits), but still through the
    /// window's queue, as a real backend's answers arrive: after the handler that asked.
    fn files(&mut self, ask: sshee_app::files::Ask) {
        let replies = self.files.answer(&ask);
        if let Some(remote) = &self.remote {
            for reply in replies {
                remote.files(reply);
            }
        }
    }

    fn settings(&mut self) -> SettingsStore {
        SettingsStore::in_memory(settings())
    }

    /// Tor's switch in Settings: the made-up Tor goes on and off at once.
    fn tor(&mut self, command: TorCommand) {
        if let Some(remote) = &self.remote {
            remote.set_tor(if command == TorCommand::Off { Tor::Off } else { Tor::On { latency_ms: Some(84) } });
        }
    }

    fn terminal(&mut self, server: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let fixture = match (tab.server.as_str(), tab.title.as_deref()) {
            (WEB, Some("~/shop")) => fixture(SHOP),
            (DB, Some("Migration 0042")) => fixture(AGENT),
            // The first terminal there; the ones after it are plain prompts.
            (STAGING, _) if tab.n == 1 => log(),
            _ => match server {
                Some(s) if s.is_local() => prompt("dev", "laptop-7731"),
                // The user is the made-up address's.
                Some(s) => prompt(s.address.split('@').next().filter(|u| !u.is_empty()).unwrap_or("dev"), &s.name),
                None => prompt("dev", "unknown"),
            },
        };
        Box::new(ReplayLink::new(fixture, None))
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
