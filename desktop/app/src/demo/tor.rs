//! The demo's Tor and tunnels: made up, nothing connects.
//!
//! Tor starts on at 84 ms, as the design draws it. Switched off it goes off at once; switched
//! on it plays a start (a few steps of progress, a second in all) and comes on at 84 ms again.
//! Its log is a few made-up lines in Tor's own words. web-7731 has two saved tunnels (Grafana
//! on, a SOCKS proxy off), db-main one that can't get its port; a tunnel switched on comes on
//! at once, unless it asks for a port the demo says is taken (5432, as a local database would).
//! A look at web-7731 finds Grafana, the shop and its admin, and two ports that aren't
//! websites; db-main runs PostgreSQL only.

use std::time::Duration;

use sshee_app::model::{SavedTunnel, SavedTunnels};
use sshee_app::tor::Status;
use sshee_app::tunnels::{local, reverse, socks, Ask, Reply, Scan, State, WebApp, SERVER};
use sshee_ui::screens::settings::TorCommand;
use sshee_ui::shell::Remote;

/// The round trip the design shows.
pub const LATENCY_MS: u32 = 84;

/// A port the demo says something on this computer already uses.
const TAKEN: u16 = 5432;

/// What Tor's log says in the demo: a start, in Tor's words (no address, as Tor writes it).
pub fn log() -> Vec<String> {
    [
        "SSHeesh 1.0.0 on this computer, its clock says 1794600000 (seconds since 1970)",
        "Looking for a consensus.",
        "Downloading certificates for consensus.",
        "Downloading microdescriptors (we are missing 412).",
        "Marked consensus usable.",
        "Enough information to build circuits.",
        "Sufficiently bootstrapped; system SOCKS now functional.",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Tells the window Tor is on, as the scene has it.
pub fn start(remote: &Remote) {
    remote.tor_status(Status::On);
    remote.tor_latency(Some(LATENCY_MS));
}

/// Settings' switch and the popover's: off at once, on after a made-up start.
pub fn command(remote: &Remote, command: TorCommand) {
    match command {
        TorCommand::Off => remote.tor_status(Status::Off),
        TorCommand::NewRoutes => {}
        TorCommand::On | TorCommand::StartAfresh => {
            let remote = remote.clone();
            let _ = std::thread::Builder::new().name("demo-tor".into()).spawn(move || {
                for percent in [0, 25, 60, 90] {
                    remote.tor_status(Status::Starting { percent, note: None });
                    std::thread::sleep(Duration::from_millis(250));
                }
                start(&remote);
            });
        }
    }
}

/// The saved tunnels the demo opens with, by host id.
pub fn saved() -> SavedTunnels {
    let tunnel = |id: &str, name: Option<&str>, forward, on| SavedTunnel { id: id.into(), name: name.map(str::to_owned), forward, on, ..SavedTunnel::default() };
    let mut list = SavedTunnels::default();
    list.by_host.insert(
        "demo-web".into(),
        vec![
            tunnel("tun-1", Some("Grafana"), local(3000, "", 3000), true),
            tunnel("tun-2", None, socks(1080), false),
            tunnel("tun-3", Some("My dev site, for the webhook"), reverse(9000, 5173), true),
        ],
    );
    list.by_host.insert("demo-db".into(), vec![tunnel("tun-4", Some("Postgres"), local(TAKEN, "", 5432), true)]);
    list
}

/// Why a port here can't be a tunnel's, as the demo has it.
pub fn port_problem(port: u16) -> Option<String> {
    (port == TAKEN).then(|| format!("Port {TAKEN} on this computer is already in use."))
}

/// What a look at a server finds in the demo, by host id.
pub fn scan(host: &str) -> Result<Scan, String> {
    let app = |port: u16, process: &str, title: Option<&str>, known_as: Option<&str>, web: bool| WebApp {
        port,
        remote_host: SERVER.into(),
        process: Some(process.into()),
        known_as: known_as.map(str::to_owned),
        title: title.map(str::to_owned),
        scheme: web.then(|| "http".to_owned()),
        status: web.then_some(200),
        web,
        public: false,
    };
    match host {
        "demo-web" => Ok(Scan {
            apps: vec![
                app(3000, "grafana", Some("Grafana"), None, true),
                app(8080, "node", Some("Shop: storefront"), None, true),
                WebApp { status: Some(401), ..app(8081, "node", Some("Shop admin"), None, true) },
                app(5432, "postgres", None, Some("PostgreSQL"), false),
                app(6379, "redis-server", None, Some("Redis"), false),
            ],
            curl: true,
        }),
        "demo-db" => Ok(Scan { apps: vec![app(5432, "postgres", None, Some("PostgreSQL"), false)], curl: true }),
        _ => Err("Couldn't reach the server.".into()),
    }
}

/// What a tunnel switched on becomes in the demo.
fn started(port: u16) -> State {
    if port == TAKEN {
        State::Failed { message: format!("Port {TAKEN} on this computer is already in use.") }
    } else {
        State::On { warning: None }
    }
}

/// The list from "last time", and the ones on.
pub fn tunnels_start(remote: &Remote) {
    let list = saved();
    remote.tunnels(Reply::Loaded(list.clone()));
    for t in list.by_host.values().flatten().filter(|t| t.on) {
        let port = sshee_app::tunnels::listens_here(&t.forward).unwrap_or_default();
        remote.tunnels(Reply::State { id: t.id.clone(), state: Some(started(port)) });
    }
}

/// A start comes on at once (or finds its port taken); a stop stops; the list is kept in memory
/// by the window only.
pub fn tunnels(remote: &Remote, ask: Ask) {
    match ask {
        Ask::Start { id, forward, .. } => {
            let port = sshee_app::tunnels::listens_here(&forward).unwrap_or_default();
            remote.tunnels(Reply::State { id, state: Some(started(port)) });
        }
        Ask::Stop { id } => remote.tunnels(Reply::State { id, state: None }),
        Ask::Save(_) => {}
        Ask::Scan { host } => {
            let result = scan(&host);
            remote.tunnels(Reply::Scanned { host, result });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demos_tunnels_are_made_up_and_show_every_state() {
        let list = saved();
        assert_eq!(list.by_host.len(), 2);
        assert!(list.by_host.values().flatten().any(|t| !t.on));
        assert_eq!(started(TAKEN), State::Failed { message: "Port 5432 on this computer is already in use.".into() });
        assert_eq!(started(3000), State::On { warning: None });
        assert_eq!(port_problem(TAKEN).as_deref(), Some("Port 5432 on this computer is already in use."));
        assert_eq!(port_problem(3000), None);
        // web-7731 runs websites and other programs; db-main only a database.
        let web = scan("demo-web").unwrap();
        assert!(web.web().count() >= 2 && web.other().count() >= 1);
        assert_eq!(scan("demo-db").unwrap().web().count(), 0);
        // Nothing in Tor's made-up log names a place.
        assert!(log().iter().all(|l| !l.contains("203.0.113") && !l.contains('@')));
    }
}
