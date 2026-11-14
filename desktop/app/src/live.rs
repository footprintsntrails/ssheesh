//! The window's backend on sshee-core: a session for every terminal tab (a shell on this
//! computer, or on a server over ssh, in tmux there when the server has it), the vital signs
//! of the machine on screen, and Tor's state.
//!
//! Every terminal has its own session with one shared `SessionManager`, under its tab's
//! number. Output is parsed into the pane's model on the session's reader thread
//! (`Delivery::Direct`), which wakes the window; status goes to the tab through the pane's
//! status sink. What a Files tab asks for is done in `crate::files`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sshee_app::workbench::{Server, Tab, Tor};
use sshee_core::session::{Delivery, EventSink, SessionEvent, SessionManager, SessionSpec, SessionState};
use sshee_core::ssh::{SshEnv, Target};
use sshee_core::tor::TorStatus;
use sshee_core::welcome::Welcome;
use sshee_core::Host;
use sshee_term::engine::Size;
use sshee_term::model::Model;
use sshee_ui::shell::{Backend, Remote};
use sshee_ui::terminal::{LinkStatus, StatusSink, TermLink};

use crate::vitals::Watcher;

/// History the terminal keeps itself: a little with tmux (tmux keeps the rest on the server),
/// more without.
const HISTORY_TMUX: usize = 1_000;
const HISTORY_OWN: usize = 10_000;

pub struct LiveBackend {
    manager: Arc<SessionManager>,
    env: SshEnv,
    /// The servers by their key in the sidebar.
    hosts: HashMap<String, Host>,
    /// `-F FILE` for every ssh, when the program was given a config of its own.
    ssh_config: Option<PathBuf>,
    watcher: Option<Watcher>,
    files: crate::files::Files,
}

impl LiveBackend {
    /// `hosts`: the list the sidebar was made from. Shared connections live in this copy's own
    /// folder (`~/.sshee-preview/cm`).
    pub fn new(hosts: &[Host], ssh_config: Option<PathBuf>) -> Self {
        let mut env = SshEnv::with_control_dir(sshee_core::app_identity::current().control_dir());
        // ssh asks this program for a saved password (`secrets::askpass_main`).
        env.askpass = std::env::current_exe().ok();
        LiveBackend::with_env(hosts, ssh_config, env)
    }

    fn with_env(hosts: &[Host], ssh_config: Option<PathBuf>, env: SshEnv) -> Self {
        let hosts = hosts.iter().map(|h| (format!("host:{}", h.id), h.clone())).collect();
        let files = crate::files::Files::new(env.clone());
        LiveBackend { manager: Arc::new(SessionManager::new()), env, hosts, ssh_config, watcher: None, files }
    }

    /// Where a machine's shells run: this computer, or ssh's target for a server. `None`: a
    /// server that isn't in the list anymore.
    fn target(&self, server: Option<&Server>) -> Option<Target> {
        self.target_of(&server?.key())
    }

    /// [`Self::target`] by a machine's key in the sidebar.
    fn target_of(&self, key: &str) -> Option<Target> {
        if key == "local" {
            return Some(Target::Local);
        }
        let mut target = self.hosts.get(key)?.target();
        if let Some(config) = &self.ssh_config {
            target.extra_args.extend(["-F".to_owned(), config.display().to_string()]);
        }
        Some(Target::Ssh(target))
    }
}

/// Tor's state as the sidebar shows it. How long a round trip through it takes isn't measured
/// yet.
pub fn tor_shown(status: &TorStatus) -> Tor {
    match status {
        TorStatus::Off => Tor::Off,
        TorStatus::Starting { .. } => Tor::Starting,
        TorStatus::On => Tor::On { latency_ms: None },
        TorStatus::Failed { .. } => Tor::Failed,
    }
}

impl Backend for LiveBackend {
    fn start(&mut self, remote: Remote) {
        self.watcher = Some(Watcher::start(self.env.clone(), remote.clone()));
        self.files.start(remote.clone());
        // Tor, if it was left on in this copy (`~/.sshee-preview/tor/on`): every connection
        // from here on goes through it. The ports are the servers' own.
        let ports = self.hosts.values().filter_map(|h| h.port).collect();
        sshee_core::tor::watch(move |status| remote.set_tor(tor_shown(&status)));
        sshee_core::tor::init(std::env::current_exe().unwrap_or_default(), ports);
    }

    fn terminal(&mut self, server: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        Box::new(LiveLink {
            manager: self.manager.clone(),
            id: format!("tab-{}", tab.id.0),
            target: self.target(server),
            key: tab.session_key.clone().unwrap_or_default(),
            env: self.env.clone(),
            bytes: Arc::new(AtomicU64::new(0)),
        })
    }

    fn watch(&mut self, server: Option<&Server>) {
        if let Some(watcher) = &self.watcher {
            watcher.watch(server.and_then(|s| Some((s.key(), self.target(Some(s))?))));
        }
    }

    fn files(&mut self, ask: sshee_app::files::Ask) {
        let target = self.target_of(&ask.server);
        self.files.ask(ask, target);
    }
}

/// One terminal's session.
struct LiveLink {
    manager: Arc<SessionManager>,
    /// The session's id with the manager.
    id: String,
    target: Option<Target>,
    /// Names the tmux session on the server, so a reconnect finds the same shell.
    key: String,
    env: SshEnv,
    /// Output so far, in bytes (counted on the reader thread).
    bytes: Arc<AtomicU64>,
}

/// What a tab says of a session's state. sshee-core's failure texts name no host.
pub fn link_status(state: &SessionState) -> LinkStatus {
    match state {
        SessionState::Connecting { .. } => LinkStatus::Connecting,
        SessionState::Connected { durable, .. } => LinkStatus::Connected { durable: *durable },
        SessionState::Reconnecting { attempt, .. } => LinkStatus::Reconnecting { note: format!("reconnecting (try {attempt})") },
        SessionState::Disconnected { .. } => LinkStatus::Waiting { note: "disconnected".into() },
        SessionState::Asleep => LinkStatus::Waiting { note: "asleep".into() },
        SessionState::Ended { code } => LinkStatus::Gone { note: code.map_or("ended".into(), |c| format!("ended ({c})")) },
        SessionState::Failed { message } => LinkStatus::Gone { note: message.clone() },
    }
}

impl TermLink for LiveLink {
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
        let Some(target) = self.target.clone() else {
            status(LinkStatus::Gone { note: "That server isn't in your list anymore.".into() });
            return;
        };
        let spec = SessionSpec {
            target,
            key: self.key.clone(),
            env: self.env.clone(),
            run: None,
            welcome: Welcome { mode: "ssheesh".into(), parts: vec!["logo".into(), "system".into(), "usage".into()], note: String::new() },
        };
        // On the reader thread (output, a server's Connected) or the session's (the rest): the
        // model locks what it shares.
        let bytes = self.bytes.clone();
        let sink: EventSink = Arc::new(move |event| match event {
            SessionEvent::Output { data } => {
                bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                model.feed(data.as_bytes());
            }
            SessionEvent::Status { state } => {
                match &state {
                    SessionState::Connected { durable, .. } => model.connected(*durable, if *durable { HISTORY_TMUX } else { HISTORY_OWN }),
                    SessionState::Connecting { .. } => {}
                    _ => model.disconnected(),
                }
                status(link_status(&state));
            }
        });
        self.manager.open_with(&self.id, size.cols, size.rows, move || Ok(spec), sink, Delivery::Direct);
    }

    fn write(&self, bytes: &[u8]) {
        self.manager.write(&self.id, bytes);
    }

    fn resize(&self, size: Size) {
        self.manager.resize(&self.id, size.cols, size.rows);
    }

    fn reconnect(&self) {
        self.manager.reconnect_now(&self.id);
    }

    fn close(&self) {
        self.manager.close(&self.id);
    }

    /// The tab was closed on purpose: its tmux session on the server ends too, on a thread of
    /// its own (it's an ssh round trip), as 0.10's `close_session` does.
    fn end(&self) {
        self.manager.close(&self.id);
        if let Some(target @ Target::Ssh(_)) = self.target.clone() {
            let (env, key) = (self.env.clone(), self.key.clone());
            let _ = std::thread::Builder::new().name("session-end".into()).spawn(move || {
                let _ = sshee_core::session::end_remote(&target, &env, &key);
            });
        }
    }

    fn bytes_received(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_core::HostSource;

    #[test]
    fn statuses_for_a_tab() {
        assert_eq!(link_status(&SessionState::Connected { durable: true, tor: false }), LinkStatus::Connected { durable: true });
        assert!(matches!(link_status(&SessionState::Asleep), LinkStatus::Waiting { .. }));
        assert!(link_status(&SessionState::Asleep).wants_reconnect());
        assert!(!link_status(&SessionState::Ended { code: Some(0) }).wants_reconnect());
        assert_eq!(link_status(&SessionState::Failed { message: "refused".into() }), LinkStatus::Gone { note: "refused".into() });
    }

    #[test]
    fn tor_as_the_sidebar_shows_it() {
        assert_eq!(tor_shown(&TorStatus::Off), Tor::Off);
        assert_eq!(tor_shown(&TorStatus::Starting { percent: 15, note: None }), Tor::Starting);
        assert_eq!(tor_shown(&TorStatus::On), Tor::On { latency_ms: None });
        assert_eq!(tor_shown(&TorStatus::Failed { message: "no internet".into() }), Tor::Failed);
    }

    #[test]
    fn a_server_connects_as_its_host_says_with_the_config_given() {
        let host = Host {
            id: "cfg:web-7731".into(),
            name: "web-7731".into(),
            hostname: "203.0.113.17".into(),
            user: None,
            port: None,
            identity_file: None,
            source: HostSource::SshConfig,
            remembered_password: false,
            identity_agent: None,
        };
        let backend = LiveBackend::with_env(std::slice::from_ref(&host), Some("/tmp/p/ssh_config".into()), SshEnv::default());
        let server = Server::host("cfg:web-7731", "web-7731", "203.0.113.17");
        match backend.target(Some(&server)) {
            Some(Target::Ssh(t)) => {
                // A host from the ssh config is named to ssh as written there.
                assert_eq!(t.destination, "web-7731");
                assert_eq!(t.extra_args, ["-F", "/tmp/p/ssh_config"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(backend.target(Some(&Server::this_computer())), Some(Target::Local)));
        // A server that left the list, and no server at all.
        assert!(backend.target(Some(&Server::host("saved:gone", "gone", ""))).is_none());
        assert!(backend.target(None).is_none());
    }
}
