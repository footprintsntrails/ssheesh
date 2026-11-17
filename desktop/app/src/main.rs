//! SSHeesh Preview: the 1.0 window on Rune, a program of its own beside the released app.
//!
//! It is its own copy (`AppIdentity::preview`): its shared connections, host keys and Tor
//! live in `~/.sshee-preview` and its passwords under its own keychain name, so it never
//! touches 0.10's. The window, its state and everything drawn are sshee-ui's; this program
//! decides which copy it is, answers when ssh starts it as a helper, and gives the window its
//! backend: sshee-core (`live.rs`), or made-up data (`demo.rs`, `--demo`).
//! How the pieces fit: desktop/ARCHITECTURE.md.

// Hides the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod args;
mod containers;
mod demo;
mod desktop;
mod edit;
mod files;
mod live;
mod recipes;
mod servers;
mod settings;
mod signin;
mod tabs;
mod update;
mod tor_check;
mod tunnels;
mod vitals;

use sshee_app::workbench::{SavedTabs, Server, Workbench};
use sshee_core::AppIdentity;
use sshee_ui::shell::{self, Backend, Launch, Options, Remote};
use sshee_ui::terminal::TermLink;

use args::Args;

fn fail(message: &str) -> ! {
    eprintln!("ssheesh-preview: {message}");
    std::process::exit(2);
}

/// The live backend, which also keeps the tabs between runs.
struct Live {
    inner: live::LiveBackend,
    /// Test builds looking for newer ones (`update`).
    update: update::Updater,
}

impl Backend for Live {
    fn start(&mut self, remote: Remote) {
        // At 1.0's first start here, the settings start from 0.10's, in its web view's
        // storage: reading it copies a database, so it happens on a thread of its own, and
        // the window takes them when done. A copy with its own settings file ignores the
        // answer. 0.10's servers aren't copied: "Add a server" offers to bring them over.
        let seeder = remote.clone();
        let spawned = settings::first_start().then(|| std::thread::Builder::new().name("import".into()).spawn(move || {
            let (found, imported) = servers::import_of_0_10();
            let seed = settings::seed(&found, imported.as_ref(), servers::ran_0_10_here());
            // 0.10's snippets, unless this copy keeps its own already.
            let snippets = imported.map(|i| i.state.snippets).filter(|_| !sshee_core::app_identity::current().dir().is_some_and(recipes::has_snippets));
            seeder.update(move |s| {
                s.seed_settings(seed);
                if let Some(list) = snippets {
                    s.seed_snippets(list);
                }
            });
        }));
        if let Some(Err(e)) = spawned {
            eprintln!("ssheesh-preview: 0.10's settings weren't read: {e}");
            // Nothing will answer: the settings stay private, and say why.
            remote.update(|s| s.seed_settings(sshee_app::settings_store::Seed::Doubt("the settings of SSHeesh 0.10 couldn't be looked for".into())));
        }
        self.update.start(remote.clone());
        self.inner.start(remote);
    }

    fn restart_to_update(&mut self) -> Result<(), String> {
        self.update.restart()
    }

    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        self.inner.servers(ask);
    }

    fn settings(&mut self) -> sshee_app::settings_store::SettingsStore {
        let mut store = self.inner.settings();
        self.update.listen(&mut store);
        store
    }

    fn settings_changed(&mut self, settings: &sshee_app::model::Settings) {
        self.inner.settings_changed(settings);
    }

    fn tor(&mut self, command: sshee_ui::screens::settings::TorCommand) {
        self.inner.tor(command);
    }

    fn tor_ask(&mut self, ask: sshee_app::tor::Ask) {
        self.inner.tor_ask(ask);
    }

    fn tunnels(&mut self, ask: sshee_app::tunnels::Ask) {
        self.inner.tunnels(ask);
    }

    fn terminal(&mut self, server: Option<&Server>, tab: &sshee_app::workbench::Tab) -> Box<dyn TermLink> {
        self.inner.terminal(server, tab)
    }

    fn watch(&mut self, server: Option<&Server>) {
        self.inner.watch(server);
    }

    fn agents_on(&mut self, servers: &[Server]) {
        self.inner.agents_on(servers);
    }

    fn files(&mut self, ask: sshee_app::files::Ask) {
        self.inner.files(ask);
    }

    fn edit(&mut self, ask: sshee_app::files::edit::EditAsk) {
        self.inner.edit(ask);
    }

    fn notify(&mut self, tab: &sshee_app::workbench::Tab, note: &sshee_term::attention::Notification) {
        self.inner.notify(tab, note);
    }

    fn waiting_changed(&mut self, waiting: &[shell::desktop::Waiting]) {
        self.inner.waiting_changed(waiting);
    }

    fn window_shown(&mut self, shown: bool) {
        self.inner.window_shown(shown);
    }

    fn tab_of_link(&self, link: &str) -> Option<sshee_app::workbench::TabId> {
        self.inner.tab_of_link(link)
    }

    fn recipes(&mut self, ask: sshee_app::recipes::Ask) {
        self.inner.recipes(ask);
    }

    fn snippets(&mut self) -> Option<sshee_app::model::Snippets> {
        self.inner.snippets()
    }

    fn snippets_changed(&mut self, list: &sshee_app::model::Snippets) {
        self.inner.snippets_changed(list);
    }

    fn welcome(&mut self) -> bool {
        self.inner.welcome()
    }

    fn version(&mut self) -> Option<String> {
        self.inner.version()
    }

    fn tabs_changed(&mut self, saved: &SavedTabs) {
        if let Some(dir) = sshee_core::app_identity::current().dir() {
            if let Err(e) = tabs::save(dir, saved) {
                eprintln!("ssheesh-preview: the tabs weren't saved: {e}");
            }
        }
    }
}

fn main() {
    // Which copy this is, before anything asks sshee-core where its files are: the helper
    // roles below find the app's Tor door and passwords through it.
    // SSHEE_PREVIEW_DIR puts the copy's folder elsewhere: the window tests give each run its
    // own, since Windows finds the home folder without asking HOME. Child processes (the Tor
    // and askpass helpers) inherit it, so they find the same folder.
    let identity = match std::env::var_os("SSHEE_PREVIEW_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => AppIdentity::in_dir(dir.into(), AppIdentity::preview().keychain_service()),
        None => AppIdentity::preview(),
    };
    let _ = sshee_core::app_identity::set(identity);
    // ssh starts this executable to carry a connection through Tor. First: askpass's
    // variables reach it too.
    if let Some(code) = sshee_core::tor::proxy_main() {
        std::process::exit(code);
    }
    // ssh starts this executable to ask for a saved password: answer and exit.
    if let Some(code) = sshee_core::secrets::askpass_main() {
        std::process::exit(code);
    }
    // A test build's update started this, the build from before, to start the new one and
    // put this one back if it doesn't start.
    if let Some(code) = update::watch_main() {
        std::process::exit(code);
    }
    // What the update before this start said (taken out of the environment while this is
    // still the only thread).
    let arrival = update::arrival();
    // `--tor-check`: this copy's Tor started without a window, each step said (`tor_check.rs`).
    if std::env::args().nth(1).as_deref() == Some("--tor-check") {
        std::process::exit(tor_check::run());
    }
    // While this is still the only thread: Rune must not see its trace and log switches.
    let scrubbed = sshee_ui::hygiene::scrub_env();
    // The shells the terminals start get the environment this was started with, less Rune's
    // own two switches.
    let _ = sshee_core::session::set_child_env(sshee_core::session::ChildEnv { set: scrubbed.removed, remove: scrubbed.kept });

    let args = match Args::parse(shell::extra_args()) {
        Ok(args) => args,
        Err(e) => fail(&format!("{e}\n\n{}", args::USAGE)),
    };
    // Windows starts the program again for a notification's click, with its link: the running
    // copy takes it (and the foreground) and this one ends. Only the real window claims it:
    // a demo or a headless run beside it stays a program of its own.
    if cfg!(windows) && !args.demo && !args.sheet && !shell::desktop::headless() && !shell::desktop::single_instance(&desktop::instance_id()) {
        return;
    }
    let options = Options { sheet: args.sheet, build: update::build(), ..Options::default() };
    let launch = if args.demo {
        Launch { bench: demo::bench(), backend: Box::new(demo::DemoBackend::new().with_leftovers(args.demo_leftovers).with_card(args.demo_card.clone()).with_first_run(args.demo_welcome, args.demo_whats_new).with_update(args.demo_update)), options }
    } else {
        // This copy's own list (`~/.sshee-preview/hosts.json`, `looks.json`) and the ssh
        // config: a couple of small files, read before the window opens, so the sidebar is
        // there in the first frame. 0.10's list is never this copy's: "Add a server" offers to
        // bring it over. Passwords are kept under this copy's own keychain name.
        let places = servers::Places::of(sshee_core::app_identity::current(), args.ssh_config.clone());
        let mut store = servers::ServerStore::open(places, std::sync::Arc::new(sshee_core::secrets::PasswordStore::default()));
        let shown = store.shown();
        let mut bench = Workbench::new(shown.servers);
        // The tabs of the last run, each terminal with the key of its shell on the server.
        if let Some(saved) = sshee_core::app_identity::current().dir().and_then(tabs::load) {
            bench.restore(&saved);
        }
        let inner = live::LiveBackend::new(&shown.hosts, args.ssh_config).with_store(std::sync::Arc::new(std::sync::Mutex::new(store)));
        // Settings' "Use the Windows title bar" is read before the window opens: it decides
        // the window's frame.
        let native_title_bar = sshee_core::app_identity::current().dir().is_some_and(sshee_app::settings_store::native_title_bar);
        Launch { bench, backend: Box::new(Live { inner, update: update::Updater::new(arrival) }), options: Options { native_title_bar, ..options } }
    };
    shell::run(launch);
}
