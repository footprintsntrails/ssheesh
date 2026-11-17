//! The window's backend on sshee-core: a session for every terminal tab (a shell on this
//! computer, or on a server over ssh, in tmux there when the server has it), the vital signs
//! of the machine on screen, and Tor's state.
//!
//! Every terminal has its own session with one shared `SessionManager`, under its tab's
//! number. Output is parsed into the pane's model on the session's reader thread
//! (`Delivery::Direct`), which wakes the window; status goes to the tab through the pane's
//! status sink. What a Files tab asks for is done in `crate::files`.
//!
//! A server a terminal signed in to stays signed in for everything else this run, Windows
//! included: the password that got it in is kept in memory (`crate::signin`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sshee_app::model::{ReconnectMode, Settings, WelcomeMode};
use sshee_app::settings_store::SettingsStore;
use sshee_app::workbench::{Server, Tab};
use sshee_core::session::{Delivery, EventSink, SessionEvent, SessionManager, SessionSpec, SessionState};
use sshee_core::hostkeys::{HostKeyCheck, PendingKeys};
use sshee_core::ssh::{SshEnv, Target};
use sshee_core::tor::TorStatus;
use sshee_core::welcome::Welcome;
use sshee_core::Host;
use sshee_term::engine::Size;
use sshee_term::model::Model;
use sshee_ui::screens::settings::TorCommand;
use sshee_ui::shell::{Backend, Remote};
use sshee_app::prompts::{Fingerprint, HostKey};
use sshee_ui::terminal::overlays::{Answer, Ask, Looked, Reply};
use sshee_ui::terminal::{LinkStatus, StatusSink, TermLink};

use crate::servers::{ServerStore, Shown};
use crate::signin::{Known, Watch};
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
    files: crate::files::Files,
    /// "Open in your own editor": the app's one edit bridge.
    edit: crate::edit::Edit,
    /// The tunnels' ssh processes, and the list kept for next time.
    tunnels: crate::tunnels::Tunnels,
    /// Containers tabs' looks and buttons, and the Stats page's programs.
    containers: crate::containers::Containers,
    /// What a new terminal on a server shows first, as Settings has it now.
    welcome: Welcome,
    /// Settings' "SSHeesh's the first time, then the server's", and the servers that have had
    /// theirs.
    welcome_first: bool,
    welcomed: Vec<String>,
    /// System notifications and the tray (`crate::desktop`); none in a headless run.
    desktop: Option<crate::desktop::Glue>,
    /// Writes `snippets.json`, in order.
    snippets: crate::recipes::SnippetWriter,
    /// The passwords that got a terminal in this run, by server (`crate::signin`).
    known: Known,
    /// Keys fetched for a question about a server's key, so "Trust" saves what was shown.
    keys: Arc<PendingKeys>,
    /// Servers whose key was found known, or trusted, this run.
    verified: Arc<Mutex<HashSet<String>>>,
}

/// SSHeesh's welcome with everything in it: what a terminal shows before Settings has spoken.
fn full_welcome() -> Welcome {
    Welcome { mode: "ssheesh".into(), parts: vec!["logo".into(), "system".into(), "usage".into()], note: String::new() }
}

/// Settings' welcome as sshee-core takes it. "The first time" is SSHeesh's here; the backend
/// turns it into the server's own for a server that has had it.
pub fn welcome_of(settings: &Settings) -> Welcome {
    let mode = match settings.welcome_mode {
        WelcomeMode::First => "ssheesh",
        ref other => other.as_str(),
    };
    Welcome { mode: mode.to_owned(), parts: settings.welcome_parts.iter().map(|p| p.as_str().to_owned()).collect(), note: settings.welcome_note.trim().to_owned() }
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
        let files = crate::files::Files::new(env.clone());
        let edit = crate::edit::Edit::new(env.clone());
        let tunnels = crate::tunnels::Tunnels::new(env.clone(), sshee_core::app_identity::current().dir().map(std::path::Path::to_path_buf));
        let containers = crate::containers::Containers::new(env.clone());
        LiveBackend {
            containers,
            manager: Arc::new(SessionManager::new()),
            env,
            hosts: Arc::new(Mutex::new(by_key(hosts))),
            ssh_config,
            watcher: None,
            agents: None,
            store: None,
            remote: None,
            files,
            edit,
            tunnels,
            welcome: full_welcome(),
            welcome_first: false,
            welcomed: vec![],
            desktop: None,
            snippets: crate::recipes::SnippetWriter::new(sshee_core::app_identity::current().dir().map(std::path::Path::to_path_buf)),
            known: Known::default(),
            keys: Arc::new(PendingKeys::default()),
            verified: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// With this copy's own list of servers behind it (`Backend::servers`).
    pub fn with_store(mut self, store: Arc<Mutex<ServerStore>>) -> Self {
        self.store = Some(store);
        self
    }

    /// Where a machine's shells run: this computer, or ssh's target for a server. `None`: a
    /// server that isn't in the list anymore.
    fn target(&self, server: Option<&Server>) -> Option<Target> {
        self.target_of(&server?.key())
    }

    /// Where a machine is for Files and the edit bridge: its target, and on the work's thread
    /// its saved password read into it (the keychain may wait or ask to be unlocked). Without
    /// a saved one, the password a terminal got in with this run (`target_of`).
    fn reach(&self, key: &str) -> crate::edit::Reach {
        let saved = self.store.clone().zip(sshee_app::servers::host_id(key).map(str::to_owned));
        let prepare: crate::files::Prepare = Box::new(move |target| {
            if let (Target::Ssh(target), Some((store, host_id))) = (target, saved) {
                if let Some(password) = store.lock().unwrap_or_else(|e| e.into_inner()).password(&host_id) {
                    target.password = Some(password);
                }
            }
        });
        (self.target_of(key), prepare)
    }

    /// [`Self::target`] by a machine's key in the sidebar. With the password a terminal got
    /// in with this run, if one did: in memory, so it's no wait (a saved one is read from the
    /// keychain on the work's own thread, and wins).
    fn target_of(&self, key: &str) -> Option<Target> {
        if key == "local" {
            return Some(Target::Local);
        }
        let mut target = self.hosts.lock().ok()?.get(key)?.target();
        if let Some(config) = &self.ssh_config {
            target.extra_args.extend(["-F".to_owned(), config.display().to_string()]);
        }
        target.password = self.known.get(key);
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

/// Tor's state as the window takes it (`sshee_app::tor::Status`, sshee-core's mirrored).
pub fn tor_status(status: &TorStatus) -> sshee_app::tor::Status {
    use sshee_app::tor::Status;
    match status {
        TorStatus::Off => Status::Off,
        TorStatus::Starting { percent, note } => Status::Starting { percent: *percent, note: note.clone() },
        TorStatus::On => Status::On,
        TorStatus::Failed { message } => Status::Failed { message: message.clone() },
    }
}

/// How often the round trip through Tor is looked at. Nothing is sent to measure it:
/// sshee-core times each route as a server picks it.
const LATENCY_LOOK: std::time::Duration = std::time::Duration::from_secs(2);

/// Tells the window the round trip through Tor when it changes, for as long as the program runs.
fn watch_latency(remote: Remote) {
    let _ = std::thread::Builder::new().name("tor-latency".into()).spawn(move || {
        let mut said = None;
        loop {
            std::thread::sleep(LATENCY_LOOK);
            let now = sshee_core::tor::latency_ms();
            if now != said {
                said = now;
                remote.tor_latency(now);
            }
        }
    });
}

/// The last lines of Tor's log, read on a thread: the file's end only.
fn read_tor_log(remote: Remote) {
    let _ = std::thread::Builder::new().name("tor-log".into()).spawn(move || {
        let text = sshee_core::tor::log_path().map(|p| tail_of(&p, 64 * 1024)).unwrap_or_default();
        remote.tor_log(sshee_app::tor::log_tail(&text, sshee_app::tor::LOG_LINES));
    });
}

/// The last `limit` bytes of a file, as text; empty when it can't be read.
fn tail_of(path: &std::path::Path, limit: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file.seek(SeekFrom::Start(len.saturating_sub(limit))).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = file.take(limit).read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // A line cut in two at the start isn't shown.
    if len > limit {
        text.split_once('\n').map_or_else(String::new, |(_, rest)| rest.to_owned())
    } else {
        text
    }
}

/// The folder with Tor's log, in the system's file manager: its full program name, the folder
/// as one argument, never through a shell.
fn show_tor_folder() {
    let Some(dir) = sshee_core::tor::log_path().and_then(|p| p.parent().map(std::path::Path::to_path_buf)).filter(|d| d.is_dir()) else { return };
    let program = if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else if cfg!(windows) {
        "explorer.exe"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(&dir).spawn() {
        eprintln!("ssheesh-preview: Tor's folder wasn't shown: {e}");
    }
}

impl Backend for LiveBackend {
    fn start(&mut self, remote: Remote) {
        self.remote = Some(remote.clone());
        self.watcher = Some(Watcher::start(self.env.clone(), remote.clone()));
        self.agents = Some(crate::agents::Watcher::start(self.env.clone(), remote.clone()));
        self.files.start(remote.clone());
        self.edit.start(remote.clone());
        self.containers.start(remote.clone());
        self.remote = Some(remote.clone());
        self.desktop = crate::desktop::Glue::start(remote.clone());
        // This computer's hostname, for streamer mode to hide (`hostname` may take a moment).
        let learn = remote.clone();
        let _ = std::thread::Builder::new().name("local-host".into()).spawn(move || {
            let host = sshee_core::identity::local().hostname;
            learn.update(move |s| s.learn_local_host(host));
        });
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
        let watcher = remote.clone();
        sshee_core::tor::watch(move |status| watcher.tor_status(tor_status(&status)));
        sshee_core::tor::init(std::env::current_exe().unwrap_or_default(), ports);
        // Said once now too: Tor left off says nothing to the watcher.
        remote.tor_status(tor_status(&sshee_core::tor::status()));
        watch_latency(remote.clone());
        // The tunnels left on last time, after Tor: they go through it when it's on.
        for ask in self.tunnels.start(remote) {
            self.tunnels(ask);
        }
    }

    fn terminal(&mut self, server: Option<&Server>, tab: &Tab) -> Box<dyn TermLink> {
        let mut welcome = self.welcome.clone();
        // "The first time": a server that has had SSHeesh's welcome shows its own from then on.
        if let (true, Some(host)) = (self.welcome_first, server.filter(|s| !s.is_local()).and_then(|s| s.target.host_id.clone())) {
            if self.welcomed.contains(&host) {
                welcome.mode = "auto".into();
            } else {
                self.welcomed.push(host.clone());
                if let Some(remote) = &self.remote {
                    remote.update(move |s| {
                        s.settings.update(|v| {
                            if !v.welcomed_hosts.contains(&host) {
                                v.welcomed_hosts.push(host);
                            }
                        });
                    });
                }
            }
        }
        // A terminal into a container runs `docker exec` or `docker logs` there; one whose
        // saved container doesn't read as one (an edited file) opens nothing.
        let run = tab.container.as_ref().map(crate::containers::argv);
        Box::new(LiveLink(Arc::new(Inner {
            refused: matches!(run, Some(None)),
            run: run.flatten(),
            server: server.map(Server::key).unwrap_or_default(),
            keys: self.keys.clone(),
            verified: self.verified.clone(),
            started: Mutex::new(None),
            size: Mutex::new(Size::new(80, 24)),
            manager: self.manager.clone(),
            id: format!("tab-{}", tab.id.0),
            target: self.target(server),
            key: tab.session_key.clone().unwrap_or_default(),
            env: self.env.clone(),
            bytes: Arc::new(AtomicU64::new(0)),
            welcome,
            durable: Arc::new(AtomicBool::new(false)),
            saved: self.store.clone().zip(server.and_then(|s| sshee_app::servers::host_id(&s.key()).map(str::to_owned))),
            signin: server.filter(|s| !s.is_local()).map(Server::key),
            known: self.known.clone(),
            watch: Arc::default(),
        })))
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

    fn settings(&mut self) -> SettingsStore {
        crate::settings::open()
    }

    /// What only sshee-core can act on: whether a dropped connection comes back by itself,
    /// what a new terminal shows first, ssh's steady beat through Tor, and whether uploads
    /// are cleaned.
    fn settings_changed(&mut self, settings: &Settings) {
        self.manager.set_auto_reconnect(settings.reconnect_mode == ReconnectMode::Auto);
        sshee_core::tor::set_hide_typing(settings.tor_hide_typing);
        crate::files::set_clean_uploads(settings.clean_uploads);
        self.welcome = welcome_of(settings);
        self.welcome_first = settings.welcome_mode == WelcomeMode::First;
        self.welcomed.clone_from(&settings.welcomed_hosts);
    }

    /// Settings' Tor switch and buttons. Starting Tor and writing its state take a moment:
    /// on a thread of their own; the sidebar hears through `tor::watch`.
    fn tor(&mut self, command: TorCommand) {
        let ports: Vec<u16> = self.hosts.lock().map(|h| h.values().filter_map(|h| h.port).collect()).unwrap_or_default();
        let (manager, env, remote) = (self.manager.clone(), self.env.clone(), self.remote.clone());
        let reroute_tunnels = self.tunnels.rerouter();
        let spawned = std::thread::Builder::new().name("tor-command".into()).spawn(move || {
            let switched = matches!(command, TorCommand::On | TorCommand::Off | TorCommand::NewRoutes);
            let done = match command {
                TorCommand::On => sshee_core::tor::set_on(true, ports),
                TorCommand::Off => sshee_core::tor::set_on(false, ports),
                TorCommand::NewRoutes => {
                    sshee_core::tor::new_routes();
                    Ok(())
                }
                TorCommand::StartAfresh => sshee_core::tor::start_afresh(),
            };
            if let Err(e) = &done {
                eprintln!("ssheesh-preview: Tor: {e}");
            }
            if let Some(remote) = &remote {
                remote.tor_warning(done.err());
            }
            // As 0.10 does: what's connected moves to the new way at once. Terminals in tmux
            // reconnect through (or around) Tor and find their shell again; one without tmux
            // stays on the old way until it reconnects. Tunnels and Files' shells start again.
            if switched {
                sshee_core::shell_pool::forget_all();
                let stayed = manager.reroute();
                reroute_tunnels();
                if stayed == 0 {
                    sshee_core::ssh::close_shared(&env, !sshee_core::tor::is_on());
                }
            }
        });
        if let Err(e) = spawned {
            eprintln!("ssheesh-preview: Tor: {e}");
        }
    }

    /// A terminal needs you while the window is in the background: the system's notification,
    /// whose click brings the tab forward (`crate::desktop`).
    fn notify(&mut self, tab: &Tab, note: &sshee_term::attention::Notification) {
        if let Some(desktop) = &mut self.desktop {
            desktop.notify(tab, note);
        }
    }

    fn waiting_changed(&mut self, waiting: &[sshee_ui::shell::desktop::Waiting]) {
        if let Some(desktop) = &mut self.desktop {
            desktop.waiting_changed(waiting);
        }
    }

    fn window_shown(&mut self, shown: bool) {
        if let Some(desktop) = &mut self.desktop {
            desktop.window_shown(shown);
        }
    }

    fn tab_of_link(&self, link: &str) -> Option<sshee_app::workbench::TabId> {
        crate::desktop::tab_of_link(link)
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
        // The same servers' load, for their cards and graphs.
        if let Some(watcher) = &self.watcher {
            watcher.also(servers.iter().filter_map(|s| Some((s.key(), self.target(Some(s))?))).collect());
        }
    }

    fn containers(&mut self, ask: sshee_app::containers::Ask) {
        let (target, prepare) = self.reach(&ask.server);
        self.containers.ask(ask, target, prepare);
    }

    fn stats(&mut self, ask: sshee_app::stats::Ask) {
        let (target, prepare) = self.reach(&ask.server);
        self.containers.programs(ask, target, prepare);
    }

    fn files(&mut self, ask: sshee_app::files::Ask) {
        let (target, prepare) = self.reach(&ask.server);
        self.files.ask(ask, target, prepare);
    }

    fn edit(&mut self, ask: sshee_app::files::edit::EditAsk) {
        self.edit.ask(ask, |host| self.reach(host));
    }

    /// The security check, a fix you confirmed, what a recipe needs: on a thread of its own
    /// (`crate::recipes`), with the server's saved password read there.
    fn recipes(&mut self, ask: sshee_app::recipes::Ask) {
        let Some(remote) = self.remote.clone() else { return };
        let server = match &ask {
            sshee_app::recipes::Ask::Check { server } | sshee_app::recipes::Ask::Fix { server, .. } | sshee_app::recipes::Ask::Facts { server } | sshee_app::recipes::Ask::ReadKey { server, .. } => server.clone(),
        };
        let host = self.hosts.lock().ok().and_then(|h| h.get(&server).cloned());
        let (target, prepare) = self.reach(&server);
        crate::recipes::answer(ask, target, prepare, host, self.env.clone(), remote);
    }

    fn snippets(&mut self) -> Option<sshee_app::model::Snippets> {
        sshee_core::app_identity::current().dir().and_then(crate::recipes::load_snippets)
    }

    fn snippets_changed(&mut self, list: &sshee_app::model::Snippets) {
        self.snippets.save(list);
    }

    /// The program shows the welcome when it's due.
    fn welcome(&mut self) -> bool {
        true
    }

    fn version(&mut self) -> Option<String> {
        Some(env!("CARGO_PKG_VERSION").to_owned())
    }

    fn tor_ask(&mut self, ask: sshee_app::tor::Ask) {
        match ask {
            sshee_app::tor::Ask::ReadLog => {
                if let Some(remote) = self.remote.clone() {
                    read_tor_log(remote);
                }
            }
            sshee_app::tor::Ask::ShowLog => show_tor_folder(),
        }
    }

    fn tunnels(&mut self, ask: sshee_app::tunnels::Ask) {
        let reach: crate::edit::Reach = match &ask {
            sshee_app::tunnels::Ask::Start { host, .. } => self.reach(&format!("host:{host}")),
            _ => (None, Box::new(|_: &mut Target| {})),
        };
        self.tunnels.ask(ask, reach);
    }
}

/// One terminal's session.
struct LiveLink(Arc<Inner>);

/// What a link shares with the threads that check a server's key and start its session.
struct Inner {
    manager: Arc<SessionManager>,
    /// The session's id with the manager.
    id: String,
    target: Option<Target>,
    /// Names the tmux session on the server, so a reconnect finds the same shell.
    key: String,
    env: SshEnv,
    /// Output so far, in bytes (counted on the reader thread).
    bytes: Arc<AtomicU64>,
    /// What it shows first.
    welcome: Welcome,
    /// The shell runs in tmux on the server, as its last status said.
    durable: Arc<AtomicBool>,
    /// A server of this copy's own list, by its host id: its saved password is asked of the
    /// keychain when the session starts, on the session's thread.
    saved: Option<(Arc<Mutex<ServerStore>>, String)>,
    /// The server's key in the sidebar (none for this computer): what [`Known`] keeps the
    /// password that got this terminal in under.
    signin: Option<String>,
    known: Known,
    /// What is typed at ssh's password prompt while connecting (`crate::signin`).
    watch: Arc<Mutex<Watch>>,
    /// The server's key in the sidebar: the name its key question goes by.
    server: String,
    keys: Arc<PendingKeys>,
    /// Servers whose key was found known, or trusted, this run: not checked again.
    verified: Arc<Mutex<HashSet<String>>>,
    /// What `open` was given, for starting again.
    started: Mutex<Option<(Arc<Model>, StatusSink)>>,
    size: Mutex<Size>,
    /// Inside a container: what runs there instead of a login shell.
    run: Option<Vec<String>>,
    /// A container that doesn't read as one: nothing is started.
    refused: bool,
}

/// What a terminal's sign-in leaves for the rest of the run: once it's in, the password that
/// got it there (typed at ssh's prompt, else the one it started with) is kept for `server`;
/// one that failed to sign in is forgotten.
fn signed_in(state: &SessionState, server: &str, watch: &Mutex<Watch>, known: &Known, used: &Mutex<Option<sshee_core::secrets::Secret>>) {
    let mut watch = watch.lock().unwrap_or_else(|e| e.into_inner());
    match state {
        SessionState::Connecting { .. } => watch.connecting(),
        SessionState::Connected { .. } => {
            let typed = watch.connected();
            if let Some(password) = typed.or_else(|| used.lock().unwrap_or_else(|e| e.into_inner()).clone()) {
                known.keep(server, password);
            }
        }
        // Only what can't be retried ends as Failed: a login refused, a host key that changed.
        SessionState::Failed { .. } => known.forget(server),
        _ => {}
    }
}

/// What a tab says of a session's state. sshee-core's failure texts name no host.
pub fn link_status(state: &SessionState) -> LinkStatus {
    match state {
        SessionState::Connecting { attempt } if *attempt > 1 => LinkStatus::Rejoining { attempt: *attempt },
        SessionState::Connecting { .. } => LinkStatus::Connecting,
        SessionState::Connected { durable, .. } => LinkStatus::Connected { durable: *durable },
        SessionState::Reconnecting { attempt, retry_in_ms, reason } => LinkStatus::Retrying { attempt: *attempt, retry_in_ms: *retry_in_ms, reason: reason.clone() },
        SessionState::Disconnected { reason } => LinkStatus::Waiting { note: reason.clone() },
        SessionState::Asleep => LinkStatus::Asleep,
        SessionState::Ended { code } => LinkStatus::Ended { code: *code },
        SessionState::Failed { message } => LinkStatus::Gone { note: message.clone() },
    }
}

/// sshee-core's key question as the window asks it.
fn host_key_question(check: HostKeyCheck) -> Option<HostKey> {
    let prints = |list: Vec<sshee_core::hostkeys::Fingerprint>| list.into_iter().map(|f| Fingerprint { kind: f.kind, sha256: f.sha256 }).collect();
    match check {
        HostKeyCheck::Unknown { fingerprints } => Some(HostKey::New { fingerprints: prints(fingerprints) }),
        HostKeyCheck::Changed { old, new } => Some(HostKey::Changed { old: prints(old), new: prints(new) }),
        HostKeyCheck::Known | HostKeyCheck::Unreachable { .. } => None,
    }
}

impl Inner {
    fn status(&self) -> Option<StatusSink> {
        self.started.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(_, s)| s.clone())
    }

    /// Before signing in to a server whose key isn't known yet this run, its key is checked
    /// (0.10's `checkIdentity`): known, it connects; new or changed, the pane asks you, and
    /// nothing connects until you answer. So ssh never asks the question itself, in the
    /// terminal or of askpass (which would answer no, and the sign-in would fail). A server
    /// that can't be asked connects anyway: ssh says why it can't.
    fn begin(self: &Arc<Self>, review: bool) {
        let Some(Target::Ssh(t)) = self.target.clone() else { return self.connect() };
        if !review && self.verified.lock().unwrap_or_else(|e| e.into_inner()).contains(&self.server) {
            return self.connect();
        }
        if let Some(status) = self.status() {
            status(LinkStatus::Connecting);
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new().name("host-key".into()).spawn(move || match me.keys.check(&me.server, &t, review) {
            Ok(HostKeyCheck::Known) => {
                me.verified.lock().unwrap_or_else(|e| e.into_inner()).insert(me.server.clone());
                me.connect();
            }
            Ok(check) => match host_key_question(check) {
                Some(question) => {
                    if let Some(status) = me.status() {
                        status(LinkStatus::HostKey(question));
                    }
                }
                None => me.connect(),
            },
            Err(_) => me.connect(),
        });
        if spawned.is_err() {
            self.connect();
        }
    }

    /// Your answer to the key question: trusted, the keys you were shown are saved to
    /// known_hosts and it signs in.
    fn answer(self: &Arc<Self>, trust: bool) {
        let Some(status) = self.status() else { return };
        let Some(Target::Ssh(t)) = self.target.clone() else { return };
        if !trust {
            status(LinkStatus::Gone { note: sshee_app::prompts::NOT_CONFIRMED.into() });
            return;
        }
        status(LinkStatus::Connecting);
        let me = self.clone();
        let _ = std::thread::Builder::new().name("host-key".into()).spawn(move || match me.keys.trust(&me.server, &t) {
            Ok(()) => {
                me.verified.lock().unwrap_or_else(|e| e.into_inner()).insert(me.server.clone());
                me.connect();
            }
            Err(e) => status(LinkStatus::Gone { note: e.message }),
        });
    }

    /// Starts the session (again).
    fn connect(&self) {
        let Some((model, status)) = self.started.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        let size = *self.size.lock().unwrap_or_else(|e| e.into_inner());
        let Some(target) = self.target.clone() else {
            status(LinkStatus::Gone { note: "That server isn't in your list anymore.".into() });
            return;
        };
        if self.refused {
            status(LinkStatus::Gone { note: "This container can't be opened: its saved id isn't one.".into() });
            return;
        }
        let saved = self.saved.clone();
        let mut spec = SessionSpec { target, key: self.key.clone(), env: self.env.clone(), run: self.run.clone(), welcome: self.welcome.clone() };
        // On the reader thread (output, a server's Connected) or the session's (the rest): the
        // model locks what it shares.
        let bytes = self.bytes.clone();
        let durable = self.durable.clone();
        let (watch, known, server) = (self.watch.clone(), self.known.clone(), self.signin.clone());
        // The password the session starts with (a saved one, or one that got in before).
        let used: Arc<Mutex<Option<sshee_core::secrets::Secret>>> = Arc::default();
        let started_with = used.clone();
        let was_down = Arc::new(AtomicBool::new(false));
        let sink: EventSink = Arc::new(move |event| match event {
            SessionEvent::Output { data } => {
                bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                watch.lock().unwrap_or_else(|e| e.into_inner()).output(&data);
                model.feed(data.as_bytes());
            }
            SessionEvent::Status { state } => {
                // Kept before the window hears it's connected: what the window then asks
                // (the status bar, a Files tab trying again) signs in with it.
                if let Some(server) = &server {
                    signed_in(&state, server, &watch, &known, &used);
                }
                durable.store(matches!(state, SessionState::Connected { durable: true, .. }), Ordering::Relaxed);
                match &state {
                    SessionState::Connected { durable, .. } => {
                        model.connected(*durable, if *durable { HISTORY_TMUX } else { HISTORY_OWN });
                        // A shell without tmux that came back is a new one: said on screen.
                        if was_down.swap(false, Ordering::Relaxed) && !*durable {
                            model.reconnected_new_shell();
                        }
                    }
                    SessionState::Connecting { .. } => {}
                    _ => {
                        was_down.store(true, Ordering::Relaxed);
                        model.disconnected();
                    }
                }
                status(link_status(&state));
            }
        });
        // The keychain may take a while, or ask to be unlocked: asked here, off the UI thread.
        // A saved password wins over one that got in earlier this run (already in the target).
        let prepare = move || {
            if let (Target::Ssh(target), Some((store, host_id))) = (&mut spec.target, saved) {
                if let Some(password) = store.lock().unwrap_or_else(|e| e.into_inner()).password(&host_id) {
                    target.password = Some(password);
                }
            }
            if let Target::Ssh(target) = &spec.target {
                started_with.lock().unwrap_or_else(|e| e.into_inner()).clone_from(&target.password);
            }
            Ok(spec)
        };
        self.manager.open_with(&self.id, size.cols, size.rows, prepare, sink, Delivery::Direct);
    }
}

/// What is at `path` on `target`: its folder is listed (0.10's `look`).
fn look(target: &Target, env: &SshEnv, path: &str) -> Looked {
    let name = sshee_app::paths::name(path).to_owned();
    let parent = sshee_app::paths::parent(path);
    match sshee_core::files::list(target, env, &parent) {
        Ok(listing) => match listing.entries.iter().find(|e| e.name == name) {
            Some(e) => Looked::Found { dir: listing.path.clone(), name, is_dir: e.kind == sshee_core::files::FileKind::Dir },
            None => Looked::Missing,
        },
        // A folder that's gone means the file is too.
        Err(e) if e.code == sshee_core::ErrorCode::NotFound => Looked::Missing,
        Err(e) => Looked::Failed(e.message),
    }
}

impl TermLink for LiveLink {
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink) {
        *self.0.size.lock().unwrap_or_else(|e| e.into_inner()) = size;
        *self.0.started.lock().unwrap_or_else(|e| e.into_inner()) = Some((model, status));
        self.0.begin(false);
    }

    fn write(&self, bytes: &[u8]) {
        self.0.watch.lock().unwrap_or_else(|e| e.into_inner()).typed(bytes);
        self.0.manager.write(&self.0.id, bytes);
    }

    fn resize(&self, size: Size) {
        *self.0.size.lock().unwrap_or_else(|e| e.into_inner()) = size;
        self.0.manager.resize(&self.0.id, size.cols, size.rows);
    }

    fn reconnect(&self) {
        self.0.manager.reconnect_now(&self.0.id);
    }

    fn close(&self) {
        self.0.manager.close(&self.0.id);
    }

    /// The tab was closed on purpose: its tmux session on the server ends too, on a thread of
    /// its own (it's an ssh round trip), as 0.10's `close_session` does.
    fn end(&self) {
        self.0.manager.close(&self.0.id);
        if let Some(target @ Target::Ssh(_)) = self.0.target.clone() {
            let (env, key) = (self.0.env.clone(), self.0.key.clone());
            let _ = std::thread::Builder::new().name("session-end".into()).spawn(move || {
                let _ = sshee_core::session::end_remote(&target, &env, &key);
            });
        }
    }

    fn trust(&self, yes: bool) {
        self.0.answer(yes);
    }

    fn restart(&self, review: bool) {
        self.0.begin(review);
    }

    fn sleep(&self) {
        self.0.manager.sleep(&self.0.id);
    }

    fn bytes_received(&self) -> u64 {
        self.0.bytes.load(Ordering::Relaxed)
    }

    /// tmux's rows, history, scroll position or folder (only for a shell in tmux), what is at
    /// a path, where a relative path is, which agents the server has: each an ssh round trip
    /// (or a folder listing here), on a thread of its own.
    fn ask(&self, ask: Ask, done: Answer) -> bool {
        let Some(target) = self.0.target.clone() else { return false };
        let ssh = matches!(target, Target::Ssh(_));
        let durable = ssh && self.0.durable.load(Ordering::Relaxed);
        let needs_tmux = matches!(ask, Ask::Rows | Ask::History { .. } | Ask::Scroll { .. } | Ask::Cwd | Ask::PaneInfo);
        let needs_server = matches!(ask, Ask::FindPath { .. } | Ask::Agents);
        if (needs_tmux && !durable) || (needs_server && !ssh) {
            return false;
        }
        let (env, key) = (self.0.env.clone(), self.0.key.clone());
        let work = move || {
            use sshee_core::session;
            let reply = match ask {
                Ask::Rows => session::rows(&target, &env, &key).ok().map(Reply::Rows),
                Ask::History { lines } => session::history(&target, &env, &key, lines).ok().map(|text| Reply::History { text, away_ms: None }),
                Ask::Scroll { position } => session::tmux_scroll(&target, &env, &key, position).ok().map(|s| {
                    Reply::Scroll(sshee_term::scroll::TmuxScroll { in_mode: s.in_mode, position: s.position, history: s.history, height: s.height, limit: s.limit })
                }),
                Ask::Cwd => sshee_core::files::session_cwd(&target, &env, &key).ok().map(Reply::Cwd),
                Ask::Look { path } => Some(Reply::Looked(look(&target, &env, &path))),
                Ask::FindPath { path, cwd } => sshee_core::files::find_path(&target, &env, &path, &cwd).ok().map(Reply::Found),
                Ask::PaneInfo => sshee_core::agents::pane_info(&target, &env, &key).ok().map(|i| Reply::PaneInfo { idle: i.idle, cwd: i.cwd }),
                Ask::Agents => sshee_core::agents::detect(&target, &env).ok().map(Reply::Agents),
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
        assert_eq!(link_status(&SessionState::Asleep), LinkStatus::Asleep);
        assert_eq!(link_status(&SessionState::Connecting { attempt: 3 }), LinkStatus::Rejoining { attempt: 3 });
        assert_eq!(
            link_status(&SessionState::Reconnecting { attempt: 2, retry_in_ms: 4000, reason: "The server stopped responding".into() }),
            LinkStatus::Retrying { attempt: 2, retry_in_ms: 4000, reason: "The server stopped responding".into() }
        );
        assert_eq!(link_status(&SessionState::Disconnected { reason: "The server closed the connection".into() }), LinkStatus::Waiting { note: "The server closed the connection".into() });
        assert_eq!(link_status(&SessionState::Ended { code: Some(0) }), LinkStatus::Ended { code: Some(0) });
        assert!(link_status(&SessionState::Asleep).wants_reconnect());
        assert!(!link_status(&SessionState::Ended { code: Some(0) }).wants_reconnect());
        assert_eq!(link_status(&SessionState::Failed { message: "refused".into() }), LinkStatus::Gone { note: "refused".into() });
    }

    #[test]
    fn the_welcome_is_settings_and_the_first_time_is_once_a_server() {
        let mut settings = Settings::default();
        let w = welcome_of(&settings);
        assert_eq!((w.mode.as_str(), w.parts.len(), w.note.as_str()), ("auto", 4, ""));
        settings.welcome_mode = WelcomeMode::Off;
        settings.welcome_note = "  production, careful ".into();
        let w = welcome_of(&settings);
        assert_eq!((w.mode.as_str(), w.note.as_str()), ("off", "production, careful"));
        // "The first time": SSHeesh's for a server that hasn't had it, its own after.
        settings.welcome_mode = WelcomeMode::First;
        settings.welcomed_hosts = vec!["saved:1".into()];
        let mut backend = LiveBackend::with_env(&[], None, SshEnv::default());
        backend.settings_changed(&settings);
        assert_eq!(backend.welcome.mode, "ssheesh");
        assert!(backend.welcome_first && backend.welcomed == ["saved:1"]);
        let tab = |bench: &mut sshee_app::workbench::Workbench, key: &str| {
            let id = bench.go_to(key);
            bench.tab(id).unwrap().clone()
        };
        let servers = vec![Server::host("saved:1", "web-7731", "deploy@203.0.113.17"), Server::host("saved:2", "db-main", "postgres@203.0.113.31")];
        let mut bench = sshee_app::workbench::Workbench::new(servers.clone());
        let (had, new) = (tab(&mut bench, "host:saved:1"), tab(&mut bench, "host:saved:2"));
        let _ = backend.terminal(Some(&servers[0]), &had);
        assert_eq!(backend.welcomed, ["saved:1"]);
        let _ = backend.terminal(Some(&servers[1]), &new);
        assert_eq!(backend.welcomed, ["saved:1", "saved:2"]);
    }

    /// The password a terminal got in with is kept for its server, and every target to that
    /// server carries it from then on (Files, the edit bridge, the status bar, new terminals);
    /// a session that can't sign in forgets it.
    #[test]
    fn a_server_signed_in_once_keeps_its_password_for_the_run() {
        use sshee_core::secrets::Secret;
        let host = Host {
            id: "saved:1".into(),
            name: "web-7731".into(),
            hostname: "203.0.113.17".into(),
            user: Some("deploy".into()),
            port: None,
            identity_file: None,
            source: HostSource::Saved,
            remembered_password: false,
            identity_agent: None,
        };
        let backend = LiveBackend::with_env(std::slice::from_ref(&host), None, SshEnv::default());
        let password = |b: &LiveBackend| match b.target_of("host:saved:1") {
            Some(Target::Ssh(t)) => t.password.map(|p| p.expose().to_owned()),
            other => panic!("{other:?}"),
        };
        assert_eq!(password(&backend), None);
        let (watch, used) = (Mutex::new(Watch::default()), Mutex::new(None));
        let at = |state| signed_in(&state, "host:saved:1", &watch, &backend.known, &used);
        // Typed at ssh's prompt while connecting, then in.
        at(SessionState::Connecting { attempt: 1 });
        watch.lock().unwrap().output("deploy@203.0.113.17's password: ");
        watch.lock().unwrap().typed(b"made-up\r");
        at(SessionState::Connected { durable: true, tor: false });
        assert_eq!(password(&backend).as_deref(), Some("made-up"));
        // A session that started with one (saved, or kept) keeps that one when nothing is typed.
        *used.lock().unwrap() = Some(Secret::new("saved-one"));
        at(SessionState::Connecting { attempt: 1 });
        at(SessionState::Connected { durable: true, tor: false });
        assert_eq!(password(&backend).as_deref(), Some("saved-one"));
        // Refused: forgotten, so the next terminal asks again rather than failing.
        at(SessionState::Failed { message: "The server didn't accept your login.".into() });
        assert_eq!(password(&backend), None);
        // This computer has none to keep.
        assert!(matches!(backend.target_of("local"), Some(Target::Local)));
    }

    #[test]
    fn tor_as_the_window_takes_it() {
        use sshee_app::tor::Status;
        assert_eq!(tor_status(&TorStatus::Off), Status::Off);
        let held = Some("A VPN can do this.".to_owned());
        assert_eq!(tor_status(&TorStatus::Starting { percent: 15, note: held.clone() }), Status::Starting { percent: 15, note: held });
        assert_eq!(tor_status(&TorStatus::On), Status::On);
        assert_eq!(tor_status(&TorStatus::Failed { message: "no internet".into() }), Status::Failed { message: "no internet".into() });
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
