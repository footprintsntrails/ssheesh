//! The window's backend on sshee-core: a session for every terminal tab (a shell on this
//! computer, or on a server over ssh, in tmux there when the server has it), the vital signs
//! of the machine on screen, and Tor's state.
//!
//! Every terminal has its own session with one shared `SessionManager`, under its tab's
//! number. Output is parsed into the pane's model on the session's reader thread
//! (`Delivery::Direct`), which wakes the window; status goes to the tab through the pane's
//! status sink.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sshee_app::workbench::{Server, Tab, Tor};
use sshee_core::session::{Delivery, EventSink, SessionEvent, SessionManager, SessionSpec, SessionState};
use sshee_core::ssh::{SshEnv, Target};
use sshee_core::tor::TorStatus;
use sshee_core::welcome::Welcome;
use sshee_core::Host;
use sshee_term::engine::Size;
use sshee_term::model::Model;
use sshee_ui::shell::{Backend, Remote};
use sshee_ui::terminal::overlays::{Answer, Ask, Reply};
use sshee_ui::terminal::{LinkStatus, StatusSink, TermLink};

use crate::servers::{ServerStore, Shown};
use crate::vitals::Watcher;

/// History the terminal keeps itself: a little with tmux (tmux keeps the rest on the server),
/// more without.
const HISTORY_TMUX: usize = 1_000;
const HISTORY_OWN: usize = 10_000;

pub struct LiveBackend {
    manager: Arc<SessionManager>,
    env: SshEnv,
    /// The servers by their key in the sidebar. Shared with the thread that changes the list
    /// (`servers`), so a server added a moment ago connects.
    hosts: Arc<Mutex<HashMap<String, Host>>>,
    /// This copy's own list, when the program keeps one: adding, importing, renaming.
    store: Option<Arc<Mutex<ServerStore>>>,
    remote: Option<Remote>,
    /// `-F FILE` for every ssh, when the program was given a config of its own.
    ssh_config: Option<PathBuf>,
    watcher: Option<Watcher>,
    /// The thread that asks servers what their terminals run (`crate::agents`).
    agents: Option<crate::agents::Watcher>,
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
        LiveBackend { manager: Arc::new(SessionManager::new()), env, hosts: Arc::new(Mutex::new(by_key(hosts))), ssh_config, watcher: None, agents: None, store: None, remote: None }
    }

    /// With this copy's own list of servers behind it (`Backend::servers`).
    pub fn with_store(mut self, store: Arc<Mutex<ServerStore>>) -> Self {
        self.store = Some(store);
        self
    }

    /// Where a machine's shells run: this computer, or ssh's target for a server. `None`: a
    /// server that isn't in the list anymore.
    fn target(&self, server: Option<&Server>) -> Option<Target> {
        let server = server?;
        if server.is_local() {
            return Some(Target::Local);
        }
        let mut target = self.hosts.lock().ok()?.get(&server.key())?.target();
        if let Some(config) = &self.ssh_config {
            target.extra_args.extend(["-F".to_owned(), config.display().to_string()]);
        }
        Some(Target::Ssh(target))
    }
}

/// The hosts by their key in the sidebar.
fn by_key(hosts: &[Host]) -> HashMap<String, Host> {
    hosts.iter().map(|h| (format!("host:{}", h.id), h.clone())).collect()
}

/// Tells the window what the list holds now.
fn show(remote: &Remote, shown: Shown, answer: Option<sshee_app::servers::Answer>) {
    remote.update(move |s| {
        s.bench.set_servers(shown.servers);
        s.set_server_looks(shown.colors, shown.config_new, shown.old_new);
        if let Some(answer) = answer {
            s.servers_answer(answer);
        }
    });
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
        self.agents = Some(crate::agents::Watcher::start(self.env.clone(), remote.clone()));
        self.remote = Some(remote.clone());
        // The servers' colours, and what 0.10 and the ssh config still offer: files to read,
        // so on a thread, and the main menu says so when it's known.
        if let Some(store) = self.store.clone() {
            let remote = remote.clone();
            let _ = std::thread::Builder::new().name("servers".into()).spawn(move || {
                let shown = store.lock().unwrap_or_else(|e| e.into_inner()).shown();
                show(&remote, shown, None);
            });
        }
        // Tor, if it was left on in this copy (`~/.sshee-preview/tor/on`): every connection
        // from here on goes through it. The ports are the servers' own.
        let ports = self.hosts.lock().map(|h| h.values().filter_map(|h| h.port).collect()).unwrap_or_default();
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
            durable: Arc::new(AtomicBool::new(false)),
            saved: self.store.clone().zip(server.and_then(|s| sshee_app::servers::host_id(&s.key()).map(str::to_owned))),
        })
    }

    /// Adding, importing and changing servers: files and the keychain, so on a thread of its
    /// own. The answer and the new list are posted while the list is still held, so they
    /// reach the window in the order the list changed.
    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        let (Some(store), Some(remote)) = (self.store.clone(), self.remote.clone()) else { return };
        let hosts = self.hosts.clone();
        let failed = remote.clone();
        let spawned = std::thread::Builder::new().name("servers".into()).spawn(move || {
            let mut store = store.lock().unwrap_or_else(|e| e.into_inner());
            let answer = store.handle(ask);
            let shown = store.shown();
            if let Ok(mut hosts) = hosts.lock() {
                *hosts = by_key(&shown.hosts);
            }
            show(&remote, shown, Some(answer));
        });
        if let Err(e) = spawned {
            failed.update(move |s| s.servers_answer(sshee_app::servers::Answer::Failed(format!("Couldn't start: {e}"))));
        }
    }

    fn watch(&mut self, server: Option<&Server>) {
        if let Some(watcher) = &self.watcher {
            watcher.watch(server.and_then(|s| Some((s.key(), self.target(Some(s))?))));
        }
    }

    fn agents_on(&mut self, servers: &[Server]) {
        if let Some(agents) = &self.agents {
            agents.watch(servers.iter().filter_map(|s| Some((s.key(), self.target(Some(s))?))).collect());
        }
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
    /// The shell runs in tmux on the server, as its last status said.
    durable: Arc<AtomicBool>,
    /// A server of this copy's own list, by its host id: its saved password is asked of the
    /// keychain when the session starts, on the session's thread.
    saved: Option<(Arc<Mutex<ServerStore>>, String)>,
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
        let saved = self.saved.clone();
        let mut spec = SessionSpec {
            target,
            key: self.key.clone(),
            env: self.env.clone(),
            run: None,
            welcome: Welcome { mode: "ssheesh".into(), parts: vec!["logo".into(), "system".into(), "usage".into()], note: String::new() },
        };
        // On the reader thread (output, a server's Connected) or the session's (the rest): the
        // model locks what it shares.
        let bytes = self.bytes.clone();
        let durable = self.durable.clone();
        let sink: EventSink = Arc::new(move |event| match event {
            SessionEvent::Output { data } => {
                bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                model.feed(data.as_bytes());
            }
            SessionEvent::Status { state } => {
                durable.store(matches!(state, SessionState::Connected { durable: true, .. }), Ordering::Relaxed);
                match &state {
                    SessionState::Connected { durable, .. } => model.connected(*durable, if *durable { HISTORY_TMUX } else { HISTORY_OWN }),
                    SessionState::Connecting { .. } => {}
                    _ => model.disconnected(),
                }
                status(link_status(&state));
            }
        });
        // The keychain may take a while, or ask to be unlocked: asked here, off the UI thread.
        let prepare = move || {
            if let (Target::Ssh(target), Some((store, host_id))) = (&mut spec.target, saved) {
                target.password = store.lock().unwrap_or_else(|e| e.into_inner()).password(&host_id);
            }
            Ok(spec)
        };
        self.manager.open_with(&self.id, size.cols, size.rows, prepare, sink, Delivery::Direct);
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

    /// tmux's rows, history or scroll position, read over the server's shared connection on a
    /// thread of its own (each is an ssh round trip). Only for a shell in tmux on a server.
    fn ask(&self, ask: Ask, done: Answer) -> bool {
        let Some(target @ Target::Ssh(_)) = self.target.clone() else { return false };
        if !self.durable.load(Ordering::Relaxed) {
            return false;
        }
        let (env, key) = (self.env.clone(), self.key.clone());
        let work = move || {
            use sshee_core::session;
            let reply = match ask {
                Ask::Rows => session::rows(&target, &env, &key).ok().map(Reply::Rows),
                Ask::History { lines } => session::history(&target, &env, &key, lines).ok().map(|text| Reply::History { text, away_ms: None }),
                Ask::Scroll { position } => session::tmux_scroll(&target, &env, &key, position).ok().map(|s| {
                    Reply::Scroll(sshee_term::scroll::TmuxScroll { in_mode: s.in_mode, position: s.position, history: s.history, height: s.height, limit: s.limit })
                }),
            };
            done(reply);
        };
        // No thread to be had: the pane is told it couldn't be read.
        std::thread::Builder::new().name("session-ask".into()).spawn(work).is_ok()
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
