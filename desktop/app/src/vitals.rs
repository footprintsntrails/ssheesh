//! The status bar's numbers, the Stats page's and the sidebar card's: one thread that looks at
//! the machine on screen every two seconds (sshee-core's `metrics::fetch`, as 0.10's
//! `src/lib/metrics.ts` did) and at the other machines with a connected terminal every ten,
//! and hands the window their vital signs and the facts of the same look.
//!
//! The window says which machine is on screen, and only while a terminal there is connected,
//! and which others have one connected: a look reuses that connection, and never signs in or
//! asks about a host key on its own. A look that fails waits longer before the next (up to a
//! minute); a machine that can't be asked at all (macOS and Windows have no `/proc`) isn't
//! asked again.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use sshee_app::stats::{self, Facts};
use sshee_app::workbench::vitals::{Disk, Reading, Vitals};
use sshee_core::metrics::{self, Metrics};
use sshee_core::ssh::{SshEnv, Target};
use sshee_core::ErrorCode;
use sshee_ui::shell::Remote;

/// Between two looks at the machine on screen.
const POLL: Duration = Duration::from_secs(2);
/// Between two looks at another machine with a connected terminal (its card, its graphs).
const AWAY: Duration = Duration::from_secs(10);
/// The longest wait between looks while they keep failing.
const MAX_WAIT: Duration = Duration::from_secs(60);
/// The disks change slowly, and asking starts a program on the server: this often.
const FULL_EVERY: Duration = Duration::from_secs(30);

/// A machine to watch: its key in the sidebar, and where to ask.
pub type Watched = (String, Target);

enum Say {
    /// The machine on screen.
    Watch(Option<Watched>),
    /// The others with a connected terminal.
    Also(Vec<Watched>),
}

/// The thread's handle.
pub struct Watcher {
    tx: mpsc::Sender<Say>,
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

/// The Stats page's facts: this look's numbers, and the slow ones from the last full look.
fn facts(m: &Metrics, slow: &Metrics) -> Facts {
    Facts {
        hostname: slow.hostname.clone(),
        kernel: slow.kernel.clone(),
        cores: slow.cores,
        load: m.load,
        mem_total: m.mem_total,
        mem_available: m.mem_available,
        swap_total: m.swap_total,
        swap_free: m.swap_free,
        net_rx: m.net_rx,
        net_tx: m.net_tx,
        month: slow.month.as_ref().map(|t| stats::Month { month: t.month.clone(), rx: t.rx, tx: t.tx }),
        disks: slow.disks.iter().map(|d| stats::Disk { mount: d.mount.clone(), size: d.size, used: d.used }).collect(),
        uptime_secs: m.uptime_secs,
    }
}

/// How long to wait after `failures` looks failed in a row: twice as long each time.
fn wait_after(every: Duration, failures: u32) -> Duration {
    every.saturating_mul(1u32.checked_shl(failures.min(16)).unwrap_or(u32::MAX)).min(MAX_WAIT)
}

/// What the thread keeps of one machine between looks.
struct Machine {
    target: Target,
    before: Option<Reading>,
    /// The last full look, for the slow facts a quick one leaves out, and when it was.
    slow: Option<(Metrics, Instant)>,
    failures: u32,
    next: Instant,
}

impl Machine {
    fn new(target: Target) -> Machine {
        Machine { target, before: None, slow: None, failures: 0, next: Instant::now() }
    }

    /// One look; what the window hears of it.
    fn look(&mut self, key: &str, env: &SshEnv, remote: &Remote) -> Result<(), ErrorCode> {
        let full = self.slow.as_ref().is_none_or(|(_, at)| at.elapsed() >= FULL_EVERY);
        let m = metrics::fetch(&self.target, env, full).map_err(|e| e.code)?;
        if full {
            self.slow = Some((m.clone(), Instant::now()));
        }
        let mut now = reading(&m);
        // A quick look leaves the disks out: the last full look's stay.
        if now.disks.is_empty() {
            now.disks = self.before.as_ref().map(|b| b.disks.clone()).unwrap_or_default();
        }
        remote.set_vitals(key.to_owned(), Some(Vitals::between(self.before.as_ref(), &now)));
        if let Some((slow, _)) = &self.slow {
            remote.set_facts(key.to_owned(), Ok(facts(&m, slow)));
        }
        self.before = Some(now);
        Ok(())
    }
}

impl Watcher {
    /// Starts the thread. It ends when the `Watcher` is dropped.
    pub fn start(env: SshEnv, remote: Remote) -> Watcher {
        let (tx, rx) = mpsc::channel::<Say>();
        let spawned = std::thread::Builder::new().name("vitals".into()).spawn(move || {
            let mut on_screen: Option<String> = None;
            let mut also: HashSet<String> = HashSet::new();
            let mut machines: HashMap<String, Machine> = HashMap::new();
            let mut cannot: HashSet<String> = HashSet::new();
            loop {
                let wait = machines.values().map(|m| m.next.saturating_duration_since(Instant::now())).min().unwrap_or(Duration::from_secs(3600));
                match rx.recv_timeout(wait) {
                    Ok(Say::Watch(next)) => {
                        let key = next.as_ref().map(|w| w.0.clone());
                        // The window may name the same machine again (a second tab connected).
                        if key != on_screen {
                            if let Some((key, target)) = next {
                                // A machine just named is looked at at once; one looked at
                                // already keeps what it had, so its CPU shows at once.
                                let m = machines.entry(key.clone()).or_insert_with(|| Machine::new(target.clone()));
                                // Signed in as its terminal is now (a password typed since).
                                m.target = target;
                                m.next = Instant::now();
                                on_screen = Some(key);
                            } else {
                                on_screen = None;
                            }
                        }
                    }
                    Ok(Say::Also(list)) => {
                        also = list.iter().map(|w| w.0.clone()).collect();
                        for (key, target) in list {
                            machines.entry(key).and_modify(|m| m.target = target.clone()).or_insert_with(|| Machine::new(target));
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                // Only machines still connected are kept.
                machines.retain(|k, _| on_screen.as_ref() == Some(k) || also.contains(k));
                let due: Vec<String> = machines.iter().filter(|(k, m)| m.next <= Instant::now() && !cannot.contains(*k)).map(|(k, _)| k.clone()).collect();
                for key in due {
                    let Some(m) = machines.get_mut(&key) else { continue };
                    let every = if on_screen.as_ref() == Some(&key) { POLL } else { AWAY };
                    match m.look(&key, &env, &remote) {
                        Ok(()) => m.failures = 0,
                        Err(ErrorCode::Unsupported) => {
                            cannot.insert(key.clone());
                            remote.set_vitals(key.clone(), None);
                            remote.set_facts(key.clone(), Err("Stats work on Linux machines for now.".into()));
                        }
                        Err(_) => m.failures += 1,
                    }
                    m.next = Instant::now() + wait_after(every, m.failures);
                }
                // A machine that can't be asked waits for good.
                for (k, m) in machines.iter_mut() {
                    if cannot.contains(k) {
                        m.next = Instant::now() + Duration::from_secs(3600);
                    }
                }
            }
        });
        if let Err(e) = spawned {
            eprintln!("ssheesh-preview: the status bar's thread didn't start: {e}");
        }
        Watcher { tx }
    }

    /// Watch this machine closely from now on; `None`: none (the main menu, or no terminal
    /// there is connected).
    pub fn watch(&self, machine: Option<Watched>) {
        let _ = self.tx.send(Say::Watch(machine));
    }

    /// The other machines with a connected terminal: looked at now and then, for their card.
    pub fn also(&self, machines: Vec<Watched>) {
        let _ = self.tx.send(Say::Also(machines));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_looks_wait_longer_up_to_a_minute() {
        assert_eq!(wait_after(POLL, 0), Duration::from_secs(2));
        assert_eq!(wait_after(POLL, 1), Duration::from_secs(4));
        assert_eq!(wait_after(POLL, 3), Duration::from_secs(16));
        assert_eq!(wait_after(POLL, 5), Duration::from_secs(60));
        assert_eq!(wait_after(POLL, 40), Duration::from_secs(60));
        assert_eq!(wait_after(AWAY, 0), Duration::from_secs(10));
        assert_eq!(wait_after(AWAY, 3), Duration::from_secs(60));
    }

    #[test]
    fn a_look_carries_over_what_the_bar_and_the_page_read() {
        let m = Metrics {
            at_ms: 7,
            cpu_total: 100,
            cpu_idle: 60,
            mem_total: 1000,
            mem_available: 400,
            uptime_secs: 90,
            net_rx: 5,
            net_tx: 6,
            load: [0.5, 0.4, 0.3],
            disks: vec![metrics::Disk { mount: "/".into(), size: 10, used: 4 }],
            ..Metrics::default()
        };
        let r = reading(&m);
        assert_eq!((r.at_ms, r.cpu_total, r.cpu_idle, r.net_rx, r.net_tx, r.uptime_secs), (7, 100, 60, 5, 6, 90));
        assert_eq!(r.disks, [Disk { mount: "/".into(), size: 10, used: 4 }]);
        assert_eq!(Vitals::between(None, &r).memory, 0.6);
        // A quick look's numbers, a full look's name, kernel, cores and disks.
        let slow = Metrics { hostname: "web-7731".into(), kernel: "Linux 6.8.0".into(), cores: 4, month: Some(metrics::MonthTraffic { month: "2026-10".into(), rx: 1, tx: 2 }), ..m.clone() };
        let quick = Metrics { disks: vec![], load: [0.9, 0.5, 0.4], ..m };
        let f = facts(&quick, &slow);
        assert_eq!((f.hostname.as_str(), f.kernel.as_str(), f.cores, f.load), ("web-7731", "Linux 6.8.0", 4, [0.9, 0.5, 0.4]));
        assert_eq!(f.disks, [stats::Disk { mount: "/".into(), size: 10, used: 4 }]);
        assert_eq!(f.month.unwrap().month, "2026-10");
    }
}
