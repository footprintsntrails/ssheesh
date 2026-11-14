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
use sshee_app::model::EnvLabel;
use sshee_app::workbench::{Open, Server, Tab, TabKind, Tor, Vitals, Workbench};
use sshee_ui::shell::{Backend, Remote};
use sshee_ui::terminal::replay::{Fixture, ReplayLink};
use sshee_ui::terminal::TermLink;

const WEB: &str = "host:demo-web";
const STAGING: &str = "host:demo-staging";
const BACKUPS: &str = "host:demo-backups";
const DB: &str = "host:demo-db";

/// The shop server's terminal, and the agent's on the database.
const SHOP: &str = include_str!("../fixtures/shop.json");
const AGENT: &str = include_str!("../fixtures/agent.json");

/// The made-up servers, tabs and numbers.
pub fn bench() -> Workbench {
    let mut bench = Workbench::new(vec![
        Server::this_computer(),
        Server::host("demo-web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
        Server::host("demo-staging", "web-staging", "deploy@203.0.113.18").env(EnvLabel::Staging).icon("flask"),
        Server::host("demo-backups", "backups", "backup@203.0.113.24").icon("backup"),
        Server::host("demo-db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Prod).icon("database"),
    ]);
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

/// Sessions that play fixtures, and files that live in memory.
pub struct DemoBackend {
    remote: Option<Remote>,
    files: files::DemoFiles,
}

impl Default for DemoBackend {
    fn default() -> Self {
        DemoBackend { remote: None, files: files::DemoFiles::new(WEB, STAGING, BACKUPS, DB) }
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

impl Backend for DemoBackend {
    fn start(&mut self, remote: Remote) {
        self.remote = Some(remote);
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

    fn terminal(&mut self, server: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let fixture = match (tab.server.as_str(), tab.title.as_deref()) {
            (WEB, Some("~/shop")) => fixture(SHOP),
            (DB, Some("Migration 0042")) => fixture(AGENT),
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
    }
}
