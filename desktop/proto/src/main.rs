//! SSHeesh 1.0's terminal prototype (Phase 1): one window, one pane.
//!
//! The window and its pane are sshee-ui's; this program picks the pane's session (sshee-core,
//! `live.rs`, or a fixture played on Rune's clock, `replay.rs`) and the meter's options from
//! its arguments (research_notes/v1-plan/prototype-design.md).

mod args;
mod live;
mod meter;
mod replay;

use std::fs::File;
use std::io::BufWriter;

use sshee_core::AppIdentity;
use sshee_ui::meter::MeterOptions;
use sshee_ui::terminal::TermLink;

use args::{Args, Session};

/// The prototype's own folder and keychain name, so it never touches 0.10's `~/.sshee`
/// (shared connections, host keys, Tor) or its saved passwords.
const FOLDER: &str = ".sshee-proto";
const KEYCHAIN_SERVICE: &str = "io.github.footprintsntrails.ssheesh.proto";

fn fail(message: &str) -> ! {
    eprintln!("ssheesh-proto: {message}");
    std::process::exit(2);
}

fn create(path: &std::path::Path) -> File {
    File::create(path).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())))
}

fn main() {
    // First, while this is the only thread: Rune must not see its trace and log switches.
    let scrubbed = sshee_ui::hygiene::scrub_env();
    // The shells the terminal starts get the environment this was started with, less Rune's
    // own two switches.
    let _ = sshee_core::session::set_child_env(sshee_core::session::ChildEnv { set: scrubbed.removed, remove: scrubbed.kept });
    // Before anything asks sshee-core where this copy keeps its files.
    let _ = sshee_core::app_identity::set(AppIdentity::in_home(FOLDER, KEYCHAIN_SERVICE));
    let args = match Args::parse(sshee_ui::proto::extra_args()) {
        Ok(args) => args,
        Err(e) => fail(&format!("{e}\n\n{}", args::USAGE)),
    };
    let mut pane = sshee_ui::terminal::PaneOptions::default();
    if let Some(mode) = args.cell {
        pane.cell_mode = mode;
    }
    let label = args.label();
    let mode = args.session.mode();
    let (link, virtual_clock): (Box<dyn TermLink>, bool) = match args.session {
        Session::Replay { fixture, out, seed } => {
            let fixture = replay::Fixture::load(&fixture, seed).unwrap_or_else(|e| fail(&e));
            let out = out.as_deref().map(create);
            (Box::new(replay::ReplayLink::new(fixture, out)), true)
        }
        session => (Box::new(live::LiveLink::new(session)), false),
    };
    let meter = (args.metrics.is_some() || args.hud).then(|| MeterOptions {
        out: args.metrics.as_deref().map(|p| Box::new(BufWriter::new(create(p))) as Box<dyn std::io::Write>),
        per_frame: args.metrics_frames,
        hud: args.hud,
        sampler: Box::new(meter::sample),
        mode,
        virtual_clock,
    });
    sshee_ui::proto::run(sshee_ui::proto::Options { label, link, pane, meter, virtual_clock });
}
