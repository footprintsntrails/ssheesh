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
mod tabs;
mod vitals;

use sshee_app::model::Labels;
use sshee_app::workbench::{SavedTabs, Server, Workbench};
use sshee_core::AppIdentity;
use sshee_ui::shell::{self, Backend, Launch, Options, Remote};
use sshee_ui::terminal::TermLink;

use args::Args;

fn fail(message: &str) -> ! {
    eprintln!("ssheesh-preview: {message}");
    std::process::exit(2);
}

/// The live backend, which also brings 0.10's labels over once the window is up.
struct Live {
    inner: live::LiveBackend,
    hosts: Vec<sshee_core::Host>,
}

impl Backend for Live {
    fn start(&mut self, remote: Remote) {
        self.inner.start(remote.clone());
        // Production labels and icons are in 0.10's web view storage: reading it copies a
        // database, so it happens on a thread of its own and the sidebar is labelled when done.
        let hosts = self.hosts.clone();
        let spawned = std::thread::Builder::new().name("labels".into()).spawn(move || {
            if let Some(labels) = servers::labels_of_0_10() {
                remote.set_servers(servers::labelled(&hosts, &labels));
            }
        });
        if let Err(e) = spawned {
            eprintln!("ssheesh-preview: labels weren't read: {e}");
        }
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
        Launch { bench: demo::bench(), backend: Box::new(demo::DemoBackend), options }
    } else {
        // The server list is a couple of small files: read before the window opens, so the
        // sidebar is there in the first frame.
        let hosts = servers::list(args.ssh_config.as_deref());
        let mut bench = Workbench::new(servers::labelled(&hosts, &Labels::default()));
        // The tabs of the last run, each terminal with the key of its shell on the server.
        if let Some(saved) = sshee_core::app_identity::current().dir().and_then(tabs::load) {
            bench.restore(&saved);
        }
        let inner = live::LiveBackend::new(&hosts, args.ssh_config);
        Launch { bench, backend: Box::new(Live { inner, hosts }), options }
    };
    shell::run(launch);
}
