//! The status bar's numbers: one thread that looks at the machine on screen every two
//! seconds (sshee-core's `metrics::fetch`, as 0.10's `src/lib/metrics.ts` did) and hands the
//! window its vital signs.
//!
//! The window says which machine to watch, and only while a terminal there is connected: a
//! look reuses that connection, and never signs in or asks about a host key on its own. A
//! look that fails waits longer before the next (up to a minute); a machine that can't be
//! asked at all (macOS and Windows have no `/proc`) isn't asked again.

use std::collections::HashSet;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use sshee_app::workbench::vitals::{Disk, Reading, Vitals};
use sshee_core::metrics::{self, Metrics};
use sshee_core::ssh::{SshEnv, Target};
use sshee_core::ErrorCode;
use sshee_ui::shell::Remote;

/// Between two looks.
const POLL: Duration = Duration::from_secs(2);
/// The longest wait between looks while they keep failing.
const MAX_WAIT: Duration = Duration::from_secs(60);
/// The disks change slowly, and asking starts a program on the server: this often.
const FULL_EVERY: Duration = Duration::from_secs(30);

/// A machine to watch: its key in the sidebar, and where to ask.
pub type Watched = (String, Target);

/// The thread's handle.
pub struct Watcher {
    tx: mpsc::Sender<Option<Watched>>,
}

/// The counters the status bar reads, from sshee-core's look.
fn reading(m: &Metrics) -> Reading {
    Reading {
        at_ms: m.at_ms,
        cpu_total: m.cpu_total,
        cpu_idle: m.cpu_idle,
        mem_total: m.mem_total,
        mem_available: m.mem_available,
        uptime_secs: m.uptime_secs,
        net_rx: m.net_rx,
        net_tx: m.net_tx,
        disks: m.disks.iter().map(|d| Disk { mount: d.mount.clone(), size: d.size, used: d.used }).collect(),
    }
}

/// How long to wait after `failures` looks failed in a row: twice as long each time.
fn wait_after(failures: u32) -> Duration {
    POLL.saturating_mul(1u32.checked_shl(failures.min(16)).unwrap_or(u32::MAX)).min(MAX_WAIT)
}

impl Watcher {
    /// Starts the thread. It ends when the `Watcher` is dropped.
    pub fn start(env: SshEnv, remote: Remote) -> Watcher {
        let (tx, rx) = mpsc::channel::<Option<Watched>>();
        let spawned = std::thread::Builder::new().name("vitals".into()).spawn(move || {
            let mut watched: Option<Watched> = None;
            let mut before: Option<Reading> = None;
            let mut full_at: Option<Instant> = None;
            let mut failures = 0u32;
            let mut cannot: HashSet<String> = HashSet::new();
            // A machine just named is looked at at once.
            let mut wait = Duration::ZERO;
            loop {
                match rx.recv_timeout(if watched.is_some() { wait } else { Duration::from_secs(3600) }) {
                    Ok(next) => {
                        // The window may name the same machine again (a second tab connected).
                        if next.as_ref().map(|w| &w.0) != watched.as_ref().map(|w| &w.0) {
                            (before, full_at, failures) = (None, None, 0);
                            wait = Duration::ZERO;
                        }
                        watched = next;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                let Some((key, target)) = &watched else { continue };
                if cannot.contains(key) {
                    wait = MAX_WAIT;
                    continue;
                }
                let full = full_at.is_none_or(|at| at.elapsed() >= FULL_EVERY);
                match metrics::fetch(target, &env, full) {
                    Ok(m) => {
                        let mut now = reading(&m);
                        // A quick look leaves the disks out: the last full look's stay.
                        if now.disks.is_empty() {
                            now.disks = before.as_ref().map(|b| b.disks.clone()).unwrap_or_default();
                        }
                        if full {
                            full_at = Some(Instant::now());
                        }
                        remote.set_vitals(key.clone(), Some(Vitals::between(before.as_ref(), &now)));
                        before = Some(now);
                        failures = 0;
                    }
                    Err(e) if e.code == ErrorCode::Unsupported => {
                        cannot.insert(key.clone());
                        remote.set_vitals(key.clone(), None);
                    }
                    Err(_) => failures += 1,
                }
                wait = wait_after(failures);
            }
        });
        if let Err(e) = spawned {
            eprintln!("ssheesh-preview: the status bar's thread didn't start: {e}");
        }
        Watcher { tx }
    }

    /// Watch this machine from now on; `None`: nothing (the main menu, or no terminal there
    /// is connected).
    pub fn watch(&self, machine: Option<Watched>) {
        let _ = self.tx.send(machine);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_looks_wait_longer_up_to_a_minute() {
        assert_eq!(wait_after(0), Duration::from_secs(2));
        assert_eq!(wait_after(1), Duration::from_secs(4));
        assert_eq!(wait_after(3), Duration::from_secs(16));
        assert_eq!(wait_after(5), Duration::from_secs(60));
        assert_eq!(wait_after(40), Duration::from_secs(60));
    }

    #[test]
    fn a_look_carries_over_what_the_bar_reads() {
        let m = Metrics {
            at_ms: 7,
            cpu_total: 100,
            cpu_idle: 60,
            mem_total: 1000,
            mem_available: 400,
            uptime_secs: 90,
            net_rx: 5,
            net_tx: 6,
            disks: vec![metrics::Disk { mount: "/".into(), size: 10, used: 4 }],
            ..Metrics::default()
        };
        let r = reading(&m);
        assert_eq!((r.at_ms, r.cpu_total, r.cpu_idle, r.net_rx, r.net_tx, r.uptime_secs), (7, 100, 60, 5, 6, 90));
        assert_eq!(r.disks, [Disk { mount: "/".into(), size: 10, used: 4 }]);
        assert_eq!(Vitals::between(None, &r).memory, 0.6);
    }
}
