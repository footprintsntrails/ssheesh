//! The demo's "Free up disk space": 0.10's made-up disks, folders and cleanups. A cleanup
//! "runs" at once and takes what it frees off the root disk, so the next look says what it
//! freed. Nothing connects.

use std::collections::{HashMap, HashSet};

use sshee_app::disk::{Ask, Choice, Cleanup, Docker, Reply, Report, Usage, Volume};
use sshee_app::recipes::Outcome;

const GB: f64 = (1u64 << 30) as f64;

fn gb(n: f64) -> u64 {
    (n * GB) as u64
}

const TEMP: &str = "sudo find /tmp /var/tmp -xdev -type f ! -path '*/systemd-private-*' ! -name '.X*-lock' -mmin +{m} -amin +{m} -delete";

fn temp(minutes: u32) -> String {
    TEMP.replace("{m}", &minutes.to_string())
}

/// Every cleanup the made-up servers offer, before any ran.
fn cleanups() -> Vec<Cleanup> {
    let plain = |id: &str, title: &str, detail: &str, command: &str, bytes: f64| Cleanup { id: id.into(), title: title.into(), detail: detail.into(), command: command.into(), bytes: Some(gb(bytes)), choices: Vec::new() };
    let choice = |label: &str, minutes: u32, bytes: f64| Choice { label: label.into(), command: temp(minutes), bytes: Some(gb(bytes)) };
    vec![
        plain("docker-images", "Unused Docker images", "Removes the 7 images no container uses; they download again when needed.", "docker image prune -a -f", 2.6),
        plain("docker-build-cache", "Docker build cache", "Leftovers from building images. The next build just takes a bit longer.", "docker builder prune -a -f", 3.7),
        plain("journal", "Old system logs", "Trims the systemd journal to the newest 200 MB.", "sudo journalctl --vacuum-size=200M", 1.4),
        plain("apt-cache", "Downloaded packages", "Installers apt keeps after installing. Nothing installed is removed.", "sudo apt-get clean", 0.6),
        Cleanup {
            id: "temp".into(),
            title: "Temporary files".into(),
            detail: "Deletes files in /tmp and /var/tmp that nothing has opened or changed for as long as you pick. Programs leave these behind and often forget them.".into(),
            command: temp(10_080),
            bytes: Some(gb(0.9)),
            choices: vec![choice("Older than a day", 1_440, 2.1), choice("Older than a week", 10_080, 0.9), choice("Older than a month", 43_200, 0.3)],
        },
    ]
}

/// What each made-up server has cleaned so far, and how much that freed.
#[derive(Default)]
pub struct DemoDisk {
    cleaned: HashMap<String, (HashSet<String>, u64)>,
}

impl DemoDisk {
    fn report(&self, server: &str) -> Report {
        let (done, freed) = self.cleaned.get(server).cloned().unwrap_or_default();
        Report {
            volumes: vec![Volume { mount: "/".into(), size: gb(80.0), used: gb(71.4).saturating_sub(freed) }, Volume { mount: "/mnt/backups".into(), size: gb(500.0), used: gb(212.0) }],
            docker: Some(Docker {
                images: gb(14.2),
                images_reclaimable: if done.contains("docker-images") { 0 } else { gb(2.6) },
                containers: gb(1.1),
                containers_reclaimable: gb(0.4),
                volumes: gb(6.3),
                volumes_reclaimable: 0,
                build_cache: if done.contains("docker-build-cache") { 0 } else { gb(3.7) },
                build_cache_reclaimable: if done.contains("docker-build-cache") { 0 } else { gb(3.7) },
            }),
            journal: Some(if done.contains("journal") { gb(0.2) } else { gb(1.6) }),
            cleanups: cleanups().into_iter().filter(|c| !done.contains(&c.id)).collect(),
            as_root: true,
        }
    }

    pub fn answer(&mut self, ask: Ask) -> Reply {
        match ask {
            Ask::Report { server } => {
                let report = self.report(&server);
                Reply::Report { server, result: Ok(report) }
            }
            Ask::Biggest { server, path } => Reply::Biggest { result: Ok(biggest(&path)), server, path },
            Ask::Clean { server, id, command } => {
                let frees = cleanups().into_iter().find(|c| c.id == id).and_then(|c| if c.choices.is_empty() { c.bytes } else { c.choices.into_iter().find(|x| x.command == command).and_then(|x| x.bytes) }).unwrap_or(0);
                let entry = self.cleaned.entry(server.clone()).or_default();
                entry.0.insert(id.clone());
                entry.1 += frees;
                let output = if id.starts_with("docker") { format!("Total reclaimed space: {:.1}GB", frees as f64 / GB) } else { String::new() };
                Reply::Cleaned { server, id, outcome: Outcome::Done { output } }
            }
        }
    }
}

/// The biggest entries in a made-up folder, biggest first.
fn biggest(path: &str) -> Vec<Usage> {
    let rows: &[(&str, f64, bool)] = match path {
        "/" => &[("var", 38.2, true), ("home", 14.1, true), ("usr", 9.3, true), ("opt", 4.8, true), ("snap", 2.2, true), ("swapfile", 2.0, false), ("root", 1.4, true), ("boot", 0.3, true)],
        "/var" => &[("lib", 29.4, true), ("log", 6.1, true), ("cache", 1.9, true), ("backups", 0.6, true), ("www", 0.2, true)],
        "/var/lib" => &[("docker", 24.8, true), ("postgresql", 3.9, true), ("apt", 0.4, true), ("snapd", 0.3, true)],
        _ => &[("shop", 3.1, true), ("releases", 1.2, true), ("dump-2026-10.sql.gz", 0.8, false), ("cache", 0.2, true)],
    };
    rows.iter()
        .map(|(name, size, dir)| Usage { path: if path == "/" { format!("/{name}") } else { format!("{}/{name}", path.trim_end_matches('/')) }, name: (*name).into(), bytes: gb(*size), is_dir: *dir })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cleanup_frees_what_it_said_and_leaves_the_list() {
        let mut d = DemoDisk::default();
        let Reply::Report { result: Ok(before), .. } = d.answer(Ask::Report { server: "host:demo-web".into() }) else { panic!() };
        assert_eq!(before.cleanups.len(), 5);
        d.answer(Ask::Clean { server: "host:demo-web".into(), id: "temp".into(), command: temp(1_440) });
        let Reply::Report { result: Ok(after), .. } = d.answer(Ask::Report { server: "host:demo-web".into() }) else { panic!() };
        assert_eq!(sshee_app::disk::freed(&before.volumes, &after.volumes), gb(2.1));
        assert!(after.cleanup("temp").is_none());
        assert_eq!(biggest("/var")[0].path, "/var/lib");
    }
}
