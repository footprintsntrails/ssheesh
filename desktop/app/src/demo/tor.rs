//! The demo's Tor and tunnels: made up, nothing connects.
//!
//! Tor starts on at 84 ms, as the design draws it. Switched off it goes off at once; switched
//! on it plays a start (a few steps of progress, a second in all) and comes on at 84 ms again.
//! Its log is a few made-up lines in Tor's own words. web-7731 has two saved tunnels (Grafana
//! on, a SOCKS proxy off), db-main one that can't get its port; a tunnel switched on comes on
//! at once, unless it asks for a port the demo says is taken (5432, as a local database would).

use std::time::Duration;

use sshee_app::model::{SavedTunnel, SavedTunnels};
use sshee_app::tor::Status;
use sshee_app::tunnels::{local, reverse, socks, Ask, Reply, State};
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
        // Nothing in Tor's made-up log names a place.
        assert!(log().iter().all(|l| !l.contains("203.0.113") && !l.contains('@')));
    }
}
