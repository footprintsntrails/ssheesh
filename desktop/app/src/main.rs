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

mod args;
mod demo;
mod live;
mod servers;
mod settings;
mod tabs;
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
            seeder.update(move |s| s.seed_settings(seed));
        }));
        if let Some(Err(e)) = spawned {
            eprintln!("ssheesh-preview: 0.10's settings weren't read: {e}");
            // Nothing will answer: the settings stay private, and say why.
            remote.update(|s| s.seed_settings(sshee_app::settings_store::Seed::Doubt("the settings of SSHeesh 0.10 couldn't be looked for".into())));
        }
        self.inner.start(remote);
    }

    fn servers(&mut self, ask: sshee_app::servers::Ask) {
        self.inner.servers(ask);
    }

    fn settings(&mut self) -> sshee_app::settings_store::SettingsStore {
        self.inner.settings()
    }

    fn settings_changed(&mut self, settings: &sshee_app::model::Settings) {
        self.inner.settings_changed(settings);
    }

    fn tor(&mut self, command: sshee_ui::screens::settings::TorCommand) {
        self.inner.tor(command);
    }

    fn terminal(&mut self, server: Option<&Server>, tab: &sshee_app::workbench::Tab) -> Box<dyn TermLink> {
        self.inner.terminal(server, tab)
    }

    fn watch(&mut self, server: Option<&Server>) {
        self.inner.watch(server);
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
    let _ = sshee_core::app_identity::set(AppIdentity::preview());
    // ssh starts this executable to carry a connection through Tor. First: askpass's
    // variables reach it too.
    if let Some(code) = sshee_core::tor::proxy_main() {
        std::process::exit(code);
    }
    // ssh starts this executable to ask for a saved password: answer and exit.
    if let Some(code) = sshee_core::secrets::askpass_main() {
        std::process::exit(code);
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
    let options = Options { sheet: args.sheet, ..Options::default() };
    let launch = if args.demo {
        Launch { bench: demo::bench(), backend: Box::new(demo::DemoBackend::new()), options }
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
        Launch { bench, backend: Box::new(Live { inner }), options }
    };
    shell::run(launch);
}
