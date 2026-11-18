//! "Free up disk space" on sshee-core: `disk::report` (the disks, Docker, the journal and the
//! cleanups, one read-only round trip), `disk::biggest` (a folder's biggest entries, `du` on
//! the server) and a cleanup you said yes to (`fixes::run_unattended`, as the security
//! check's fixes run). Each runs on a thread of its own with the server's saved password read
//! there, and answers through `Remote::disk`.

use sshee_app::disk::{Ask, Choice, Cleanup, Docker, Reply, Report, Usage, Volume};
use sshee_core::disk::{self, DiskReport};
use sshee_core::fixes;
use sshee_core::ssh::{SshEnv, Target};
use sshee_ui::shell::Remote;

/// sshee-core's look as the window reads it.
pub fn report(r: DiskReport) -> Report {
    Report {
        volumes: r.volumes.into_iter().map(|v| Volume { mount: v.mount, size: v.size, used: v.used }).collect(),
        docker: r.docker.map(|d| Docker {
            images: d.images,
            images_reclaimable: d.images_reclaimable,
            containers: d.containers,
            containers_reclaimable: d.containers_reclaimable,
            volumes: d.volumes,
            volumes_reclaimable: d.volumes_reclaimable,
            build_cache: d.build_cache,
            build_cache_reclaimable: d.build_cache_reclaimable,
        }),
        journal: r.journal,
        cleanups: r
            .cleanups
            .into_iter()
            .map(|c| Cleanup {
                id: c.id,
                title: c.title,
                detail: c.detail,
                command: c.command,
                bytes: c.bytes,
                choices: c.choices.into_iter().map(|x| Choice { label: x.label, command: x.command, bytes: x.bytes }).collect(),
            })
            .collect(),
        as_root: r.as_root,
    }
}

fn usage(u: disk::Usage) -> Usage {
    Usage { path: u.path, name: u.name, bytes: u.bytes, is_dir: u.is_dir }
}

/// What the window asked, done on a thread of its own.
pub fn answer(ask: Ask, target: Option<Target>, prepare: crate::files::Prepare, env: SshEnv, remote: Remote) {
    let work = move || {
        let target = target.map(|mut t| {
            prepare(&mut t);
            t
        });
        let gone = || "That server isn't in your list anymore.".to_owned();
        let reply = match ask {
            Ask::Report { server } => {
                let result = target.as_ref().ok_or_else(gone).and_then(|t| disk::report(t, &env).map(report).map_err(|e| e.message));
                Reply::Report { server, result }
            }
            Ask::Biggest { server, path } => {
                let result = target.as_ref().ok_or_else(gone).and_then(|t| disk::biggest(t, &env, &path).map(|l| l.into_iter().map(usage).collect()).map_err(|e| e.message));
                Reply::Biggest { server, path, result }
            }
            Ask::Clean { server, id, command } => {
                let outcome = match &target {
                    Some(t) => fixes::run_unattended(t, &env, &command).map(crate::recipes::outcome).unwrap_or_else(|e| sshee_app::recipes::Outcome::Failed { output: e.message, code: None }),
                    None => sshee_app::recipes::Outcome::Failed { output: gone(), code: None },
                };
                Reply::Cleaned { server, id, outcome }
            }
        };
        remote.disk(reply);
    };
    if let Err(e) = std::thread::Builder::new().name("disk".into()).spawn(work) {
        eprintln!("ssheesh-preview: the disk look couldn't start: {e}");
    }
}
