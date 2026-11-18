//! The security check, its fixes and what recipes need to know, on sshee-core; and the
//! snippets file. Everything that talks to a server or reads a file runs on a thread of its
//! own and answers through `Remote::recipes`.
//!
//! - **The check** is `sshee_core::checkup::run`: one read-only round trip.
//! - **A fix** is `sshee_core::fixes::run_unattended`, only ever after the window's yes (two
//!   on Production): it runs without a terminal, or says it needs you.
//! - **Facts**: the server's kernel (a full look of its vital signs), how a fresh `ssh` typed
//!   here reaches it, the keys in `~/.ssh`, and ssh's options through Tor while it's on.
//! - **Snippets** are `snippets.json` in this copy's own folder, written whole or not at all,
//!   on one writer thread so saves land in order.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use sshee_app::model::Snippets;
use sshee_app::recipes::checkup::{Finding, Report, Verdict};
use sshee_app::recipes::{Ask, Facts, LocalKey, Outcome, Reply, SshHost};
use sshee_core::checkup::{self, Checkup};
use sshee_core::fixes::{self, RunOutcome};
use sshee_core::ssh::{SshEnv, Target};
use sshee_core::{Host, HostSource};
use sshee_ui::shell::Remote;

const SNIPPETS: &str = "snippets.json";

/// sshee-core's check as the window reads it.
pub fn report(c: Checkup) -> Report {
    let findings = c
        .checks
        .into_iter()
        .map(|c| Finding {
            id: c.id,
            title: c.title,
            verdict: match c.verdict {
                checkup::Verdict::Good => Verdict::Good,
                checkup::Verdict::Warn => Verdict::Warn,
                checkup::Verdict::Bad => Verdict::Bad,
                checkup::Verdict::Unknown => Verdict::Unknown,
            },
            detail: c.detail,
            fix: c.fix,
            fix_note: c.fix_note,
        })
        .collect();
    Report { findings, as_root: c.as_root }
}

pub fn outcome(o: RunOutcome) -> Outcome {
    match o {
        RunOutcome::Done { output } => Outcome::Done { output },
        RunOutcome::Failed { output, code } => Outcome::Failed { output, code },
        RunOutcome::NeedsYou { ask } => Outcome::NeedsYou { ask },
    }
}

/// How a fresh `ssh` typed on this computer reaches `host`.
pub fn ssh_host(host: &Host) -> SshHost {
    SshHost {
        name: host.name.clone(),
        hostname: host.hostname.clone(),
        port: host.port,
        user: host.user.clone(),
        identity_file: host.identity_file.clone(),
        from_config: matches!(host.source, HostSource::SshConfig),
    }
}

/// `~/…` on this computer.
fn expand(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// A key's public half, read from `<path>.pub` (at most a few KB).
pub fn read_key(path: &str, home: Option<&Path>) -> Option<String> {
    let file = expand(&format!("{path}.pub"), home);
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > 16 * 1024 {
        return None;
    }
    std::fs::read_to_string(file).ok()
}

/// What the window asked, done on a thread of its own.
/// `prepare` reads the server's saved password into its target, there: the keychain may wait.
pub fn answer(ask: Ask, target: Option<Target>, prepare: crate::files::Prepare, host: Option<Host>, env: SshEnv, remote: Remote) {
    let gone = || "That server isn't in your list anymore.".to_owned();
    let work = move || {
        // Only what talks to the server needs its password.
        let target = match (&ask, target) {
            (Ask::Check { .. } | Ask::Fix { .. } | Ask::Facts { .. }, Some(mut t)) => {
                prepare(&mut t);
                Some(t)
            }
            (_, t) => t,
        };
        let reply = match ask {
            Ask::Check { server } => {
                let result = match &target {
                    Some(t) => checkup::run(t, &env).map(report).map_err(|e| e.message),
                    None => Err(gone()),
                };
                Reply::Checked { server, result }
            }
            Ask::Fix { server, check, command } => {
                let outcome = match &target {
                    Some(t) => fixes::run_unattended(t, &env, &command).map(outcome).unwrap_or_else(|e| Outcome::Failed { output: e.message, code: None }),
                    None => Outcome::Failed { output: gone(), code: None },
                };
                Reply::Fixed { server, check, outcome }
            }
            Ask::Facts { server } => {
                let kernel = target.as_ref().filter(|t| matches!(t, Target::Ssh(_))).and_then(|t| sshee_core::metrics::fetch(t, &env, true).ok()).map(|m| m.kernel).filter(|k| !k.is_empty());
                let keys = sshee_core::keys::discover().keys.into_iter().map(|k| LocalKey { path: k.path, name: k.name, kind: k.kind }).collect();
                let facts = Facts { kernel, host: host.as_ref().map(ssh_host), keys, windows: cfg!(windows), tor: sshee_core::tor::typed_ssh_options() };
                Reply::Facts { server, facts }
            }
            Ask::ReadKey { server, path } => {
                let line = read_key(&path, dirs::home_dir().as_deref());
                Reply::Key { server, path, line }
            }
        };
        remote.recipes(reply);
    };
    if let Err(e) = std::thread::Builder::new().name("recipes".into()).spawn(work) {
        eprintln!("ssheesh-preview: the security check couldn't start: {e}");
    }
}

/// The snippets kept in `dir`, if there's a file and it reads. One that doesn't read is moved
/// aside to a `.damaged` name (the next save would write over it), and the window starts with
/// the starters.
pub fn load_snippets(dir: &Path) -> Option<Snippets> {
    sshee_app::settings_store::read_json_or_set_aside(&dir.join(SNIPPETS))
}

/// Whether this copy keeps snippets of its own yet: a file that reads. One that doesn't is
/// moved aside, so 0.10's are brought over instead.
pub fn has_snippets(dir: &Path) -> bool {
    load_snippets(dir).is_some()
}

/// One thread that writes the snippets file, in the order saves came.
pub struct SnippetWriter {
    tx: Option<mpsc::Sender<Snippets>>,
}

impl SnippetWriter {
    pub fn new(dir: Option<PathBuf>) -> SnippetWriter {
        let Some(dir) = dir else { return SnippetWriter { tx: None } };
        let (tx, rx) = mpsc::channel::<Snippets>();
        let spawned = std::thread::Builder::new().name("snippets".into()).spawn(move || {
            while let Ok(mut list) = rx.recv() {
                // Only the latest of a burst matters.
                while let Ok(newer) = rx.try_recv() {
                    list = newer;
                }
                if let Err(e) = save_snippets(&dir, &list) {
                    eprintln!("ssheesh-preview: the snippets weren't saved: {e}");
                }
            }
        });
        SnippetWriter { tx: spawned.ok().map(|_| tx) }
    }

    pub fn save(&self, list: &Snippets) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(list.clone());
        }
    }
}

/// Writes the list to `dir`, private to you: commands may hold secrets.
pub fn save_snippets(dir: &Path, list: &Snippets) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(list).map_err(std::io::Error::other)?;
    sshee_app::settings_store::write_atomic(&dir.join(SNIPPETS), &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sshee-preview-recipes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn snippets_are_saved_and_read_back() {
        let dir = folder("snippets");
        assert!(load_snippets(&dir).is_none() && !has_snippets(&dir));
        let mut list = Snippets::default();
        sshee_app::snippets::save(&mut list, None, "Tail the log", "tail -f /var/log/shop.log", Some("host:web"));
        save_snippets(&dir, &list).unwrap();
        assert_eq!(load_snippets(&dir), Some(list));
        std::fs::write(dir.join(SNIPPETS), "{ not json").unwrap();
        assert!(load_snippets(&dir).is_none() && !has_snippets(&dir));
        // Moved aside, so a save doesn't write over it.
        save_snippets(&dir, &Snippets::default()).unwrap();
        let aside = dir.join(SNIPPETS.replace(".json", ".damaged.json"));
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_is_read_from_its_pub_file_under_home() {
        let home = folder("home");
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh").join("id_ed25519.pub"), "ssh-ed25519 AAAA me@laptop-7731\n").unwrap();
        assert_eq!(read_key("~/.ssh/id_ed25519", Some(&home)).as_deref(), Some("ssh-ed25519 AAAA me@laptop-7731\n"));
        assert_eq!(read_key("~/.ssh/missing", Some(&home)), None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_core_check_reads_as_the_window_has_it() {
        let c = Checkup {
            checks: vec![checkup::Check {
                id: "firewall".into(),
                title: "Firewall".into(),
                verdict: checkup::Verdict::Bad,
                detail: "No firewall is on.".into(),
                fix: Some("sudo ufw enable".into()),
                fix_note: None,
            }],
            as_root: true,
        };
        let r = report(c);
        assert_eq!((r.findings[0].verdict, r.findings[0].fix.as_deref(), r.as_root), (Verdict::Bad, Some("sudo ufw enable"), true));
        assert_eq!(outcome(RunOutcome::NeedsYou { ask: "type y".into() }), Outcome::NeedsYou { ask: "type y".into() });
    }
}
