//! "Free up disk space" without a screen (0.10's `DiskDialog` in `ServerTools.tsx`): how full
//! each disk is, what takes the space one folder at a time, Docker and the journal, and the
//! cleanups that are safe, each with its command in full.
//!
//! - Looking changes nothing. A cleanup runs only after Run and a yes, and on a server
//!   labelled Production after a second yes: the security check's rule
//!   ([`crate::recipes::checkup::Gate`], [`crate::recipes::checkup::after_yes`]).
//! - After a cleanup that worked the server is looked at again, and what it freed is the drop
//!   in each disk's used space ([`freed`]).
//! - What the window asks its backend ([`Ask`]) and hears back ([`Reply`]); sshee-core's
//!   `disk` types are mirrored here, so this crate needs no ssh.

use crate::recipes::Outcome;
use crate::workbench::vitals::bytes as size;

/// One disk, as `df` sees it (sshee-core's `disk::Volume`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Volume {
    pub mount: String,
    /// Bytes.
    pub size: u64,
    pub used: u64,
}

/// How full a disk looks: 0.10's colours, orange from 80%, red from 90%.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    Fine,
    Warn,
    Full,
}

impl Volume {
    /// Used, from 0 to 1.
    pub fn fraction(&self) -> f64 {
        if self.size == 0 {
            0.0
        } else {
            (self.used as f64 / self.size as f64).clamp(0.0, 1.0)
        }
    }

    pub fn fill(&self) -> Fill {
        match self.fraction() {
            f if f >= 0.9 => Fill::Full,
            f if f >= 0.8 => Fill::Warn,
            _ => Fill::Fine,
        }
    }

    /// "71 GB of 80 GB · 89%".
    pub fn words(&self) -> String {
        format!("{} of {} · {}", size(self.used as f64), size(self.size as f64), percent(self.fraction()))
    }
}

/// 0.892 → "89%".
fn percent(f: f64) -> String {
    format!("{}%", (f.clamp(0.0, 1.0) * 100.0).round())
}

/// One entry inside a folder, for "Biggest here" (sshee-core's `disk::Usage`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    /// Absolute path.
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub is_dir: bool,
}

/// `docker system df`, in bytes (sshee-core's `disk::DockerUsage`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Docker {
    pub images: u64,
    pub images_reclaimable: u64,
    pub containers: u64,
    pub containers_reclaimable: u64,
    pub volumes: u64,
    pub volumes_reclaimable: u64,
    pub build_cache: u64,
    pub build_cache_reclaimable: u64,
}

impl Docker {
    /// The four tiles: name, size, of which unused.
    pub fn rows(&self) -> [(&'static str, u64, u64); 4] {
        [
            ("Images", self.images, self.images_reclaimable),
            ("Containers", self.containers, self.containers_reclaimable),
            ("Volumes", self.volumes, self.volumes_reclaimable),
            ("Build cache", self.build_cache, self.build_cache_reclaimable),
        ]
    }
}

/// One strength of a cleanup, like how old temporary files must be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    /// "Older than a week".
    pub label: String,
    pub command: String,
    pub bytes: Option<u64>,
}

/// A safe cleanup (sshee-core's `disk::Cleanup`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleanup {
    /// Stable: "docker-images", "journal", "apt-cache", "temp", ...
    pub id: String,
    pub title: String,
    /// One plain sentence on what it removes and why that's safe.
    pub detail: String,
    /// The command, for the strength picked first.
    pub command: String,
    pub bytes: Option<u64>,
    /// Strengths to pick from (none, or at least two).
    pub choices: Vec<Choice>,
}

impl Cleanup {
    /// The strength picked when the window opens: the one `command` is.
    pub fn first_pick(&self) -> usize {
        self.choices.iter().position(|c| c.command == self.command).unwrap_or(0)
    }

    /// The command for strength `pick` (the only command without strengths).
    pub fn command_for(&self, pick: usize) -> &str {
        self.choices.get(pick).map_or(self.command.as_str(), |c| c.command.as_str())
    }

    /// Roughly what strength `pick` frees, when known.
    pub fn bytes_for(&self, pick: usize) -> Option<u64> {
        match self.choices.get(pick) {
            Some(c) => c.bytes,
            None => self.bytes,
        }
    }

    /// "frees about 2.6 GB", or "nothing that old"; `None` when it isn't known.
    pub fn frees(&self, pick: usize) -> Option<String> {
        self.bytes_for(pick).map(|b| if b > 0 { format!("frees about {}", size(b as f64)) } else { "nothing that old".to_owned() })
    }

    /// The strengths' labels, when there are some to pick from.
    pub fn labels(&self) -> Vec<&str> {
        if self.choices.len() > 1 {
            self.choices.iter().map(|c| c.label.as_str()).collect()
        } else {
            Vec::new()
        }
    }
}

/// The quick look: disks, Docker, the journal and cleanups (sshee-core's `disk::DiskReport`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub volumes: Vec<Volume>,
    /// `None` without Docker, or when it can't be asked.
    pub docker: Option<Docker>,
    /// The systemd journal's size; `None` without journald.
    pub journal: Option<u64>,
    /// Only cleanups that would free something worth it.
    pub cleanups: Vec<Cleanup>,
    /// False when sudo wanted a password, so some sizes may be missing or low.
    pub as_root: bool,
}

impl Report {
    pub fn cleanup(&self, id: &str) -> Option<&Cleanup> {
        self.cleanups.iter().find(|c| c.id == id)
    }
}

/// What a cleanup freed: how much less each disk uses now than before it ran, added up over
/// the disks there both times. A disk that filled up meanwhile counts as nothing freed.
pub fn freed(before: &[Volume], after: &[Volume]) -> u64 {
    before.iter().filter_map(|b| after.iter().find(|a| a.mount == b.mount).map(|a| b.used.saturating_sub(a.used))).sum()
}

/// Said after a cleanup once the disks were looked at again.
pub fn freed_words(bytes: u64) -> String {
    if bytes == 0 {
        "Done. The disks use about as much as before.".to_owned()
    } else {
        format!("Done. Freed {}.", size(bytes as f64))
    }
}

/// Said while the disks are looked at again after a cleanup.
pub const MEASURING_FREED: &str = "Done. Measuring what it freed…";

/// "/var/lib/docker" → [("/", "/"), ("var", "/var"), ("lib", "/var/lib"), ("docker", "/var/lib/docker")].
pub fn crumbs(path: &str) -> Vec<(String, String)> {
    let mut out = vec![("/".to_owned(), "/".to_owned())];
    let mut at = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        at.push('/');
        at.push_str(part);
        out.push((part.to_owned(), at.clone()));
    }
    out
}

/// A row's bar beside the biggest entry: from 0 to 1, never quite empty.
pub fn share(bytes: u64, top: u64) -> f64 {
    if top == 0 {
        0.01
    } else {
        (bytes as f64 / top as f64).clamp(0.01, 1.0)
    }
}

/// Said under the title while nothing has run.
pub const PROMISE: &str = "nothing is deleted until you run a cleanup";

/// Said while the first look runs.
pub const LOOKING: &str = "Looking at the disks, Docker and logs…";

/// Said while folders are measured.
pub const MEASURING: &str = "Measuring folders. On a big disk this can take a minute…";

/// A folder with nothing in it worth listing.
pub const EMPTY: &str = "Nothing here takes up space.";

/// The journal's line.
pub fn journal_words(bytes: u64) -> String {
    format!("System logs (journal): {}", size(bytes as f64))
}

/// A Docker tile's second line: "14 GB", "14 GB · 2.6 GB unused".
pub fn docker_words(total: u64, unused: u64) -> String {
    if unused > 0 {
        format!("{} · {} unused", size(total as f64), size(unused as f64))
    } else {
        size(total as f64)
    }
}

/// What the window asks the backend. Each is done on a thread of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// The quick look.
    Report { server: String },
    /// The biggest entries directly inside `path`.
    Biggest { server: String, path: String },
    /// Run a cleanup you said yes to (twice on Production): exactly `command`.
    Clean { server: String, id: String, command: String },
}

impl Ask {
    pub fn server(&self) -> &str {
        match self {
            Ask::Report { server } | Ask::Biggest { server, .. } | Ask::Clean { server, .. } => server,
        }
    }
}

/// What the backend answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Report { server: String, result: Result<Report, String> },
    Biggest { server: String, path: String, result: Result<Vec<Usage>, String> },
    Cleaned { server: String, id: String, outcome: Outcome },
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1 << 30;

    fn temp() -> Cleanup {
        let choice = |label: &str, days: u32, bytes: u64| Choice { label: label.into(), command: format!("sudo find /tmp -mmin +{} -delete", days * 1440), bytes: Some(bytes) };
        Cleanup {
            id: "temp".into(),
            title: "Temporary files".into(),
            detail: String::new(),
            command: "sudo find /tmp -mmin +10080 -delete".into(),
            bytes: Some(GB),
            choices: vec![choice("Older than a day", 1, 2 * GB), choice("Older than a week", 7, GB), choice("Older than a month", 30, 0)],
        }
    }

    #[test]
    fn a_disk_reads_as_used_of_total_and_fills_up_in_three_steps() {
        let v = |used: u64| Volume { mount: "/".into(), size: 100 * GB, used: used * GB };
        assert_eq!(v(44).words(), "44 GB of 100 GB · 44%");
        assert_eq!((v(79).fill(), v(80).fill(), v(90).fill()), (Fill::Fine, Fill::Warn, Fill::Full));
        assert_eq!(Volume::default().fraction(), 0.0);
    }

    #[test]
    fn a_cleanup_with_strengths_starts_on_its_own_and_says_what_each_frees() {
        let c = temp();
        assert_eq!(c.first_pick(), 1);
        assert_eq!(c.command_for(0), "sudo find /tmp -mmin +1440 -delete");
        assert_eq!(c.frees(0).as_deref(), Some("frees about 2.0 GB"));
        assert_eq!(c.frees(2).as_deref(), Some("nothing that old"));
        assert_eq!(c.labels(), ["Older than a day", "Older than a week", "Older than a month"]);
        let plain = Cleanup { id: "journal".into(), command: "sudo journalctl --vacuum-size=200M".into(), ..Cleanup::default() };
        assert_eq!((plain.first_pick(), plain.command_for(0), plain.frees(0)), (0, "sudo journalctl --vacuum-size=200M", None));
        assert!(plain.labels().is_empty());
    }

    #[test]
    fn what_a_cleanup_freed_is_the_drop_in_used_space() {
        let before = [Volume { mount: "/".into(), size: 80 * GB, used: 71 * GB }, Volume { mount: "/srv".into(), size: 10 * GB, used: 2 * GB }];
        let after = [Volume { mount: "/".into(), size: 80 * GB, used: 68 * GB }, Volume { mount: "/srv".into(), size: 10 * GB, used: 3 * GB }];
        assert_eq!(freed(&before, &after), 3 * GB);
        assert_eq!(freed_words(3 * GB), "Done. Freed 3.0 GB.");
        assert_eq!(freed_words(0), "Done. The disks use about as much as before.");
        assert_eq!(freed(&before, &[]), 0);
    }

    #[test]
    fn crumbs_walk_down_from_the_root() {
        assert_eq!(crumbs("/"), [("/".to_owned(), "/".to_owned())]);
        let c = crumbs("/var/lib/");
        assert_eq!(c.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(), ["/", "/var", "/var/lib"]);
        assert_eq!(c[2].0, "lib");
        assert_eq!(share(5, 10), 0.5);
        assert_eq!(share(0, 10), 0.01);
        assert_eq!(docker_words(14 * GB, 0), "14 GB");
        assert_eq!(docker_words(14 * GB, GB / 2), "14 GB · 512 MB unused");
    }
}
