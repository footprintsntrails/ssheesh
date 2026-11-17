//! The demo's made-up containers, programs and machine facts, so `--demo` shows every state
//! of the Containers and Stats pages: web-7731 runs a Compose project and a stopped job,
//! web-staging has one that keeps restarting, backups has no Docker, db-main's user isn't
//! allowed to use it, and this computer has Podman with nothing in it. Buttons change the
//! made-up state at once. Every name is made up.

use std::collections::HashMap;

use sshee_app::containers::{Action, Answer, Ask, Container, ContainerList, Want};
use sshee_app::model::Engine;
use sshee_app::stats::{Disk, Facts, Month, Process};
use sshee_app::workbench::vitals::Vitals;

use super::{BACKUPS, DB, STAGING, WEB};

fn c(id: &str, name: &str, image: &str, state: &str, status: &str, ports: &str, project: Option<&str>) -> Container {
    Container {
        engine: Engine::Docker,
        id: id.into(),
        name: name.into(),
        image: image.into(),
        running: state == "running",
        state: state.into(),
        status: status.into(),
        ports: ports.into(),
        project: project.map(str::to_owned),
    }
}

/// Each machine's containers, as a look would find them.
pub struct DemoContainers {
    lists: HashMap<String, ContainerList>,
}

impl DemoContainers {
    pub fn new() -> DemoContainers {
        let docker = |containers: Vec<Container>| ContainerList { engines: vec![Engine::Docker], containers, problems: vec![] };
        let mut lists = HashMap::new();
        lists.insert(
            WEB.to_owned(),
            docker(vec![
                c("a1b2c3d4e5f6", "shop-web-1", "nginx:1.27-alpine", "running", "Up 3 days", "0.0.0.0:443->443/tcp, [::]:443->443/tcp, 0.0.0.0:80->80/tcp", Some("shop")),
                c("b2c3d4e5f6a1", "shop-api-1", "registry.example/shop/api:2.14.0", "running", "Up 3 days (healthy)", "127.0.0.1:8080->8080/tcp", Some("shop")),
                c("c3d4e5f6a1b2", "shop-worker-1", "registry.example/shop/worker:2.14.0", "running", "Up 41 minutes", "", Some("shop")),
                c("d4e5f6a1b2c3", "shop-db-1", "postgres:17", "running", "Up 3 days (healthy)", "5432/tcp", Some("shop")),
                c("e5f6a1b2c3d4", "shop-cache-1", "redis:8-alpine", "running", "Up 3 days", "6379/tcp", Some("shop")),
                c("f6a1b2c3d4e5", "nightly-backup", "restic/restic:0.18", "exited", "Exited (0) 6 hours ago", "", None),
                c("a1b2c3d4e5f7", "image-resizer", "registry.example/tools/resizer:1.3", "exited", "Exited (137) 2 days ago", "", None),
            ]),
        );
        lists.insert(
            STAGING.to_owned(),
            docker(vec![
                c("0a1b2c3d4e5f", "shop-web-1", "nginx:1.27-alpine", "running", "Up 2 hours", "0.0.0.0:8443->443/tcp", Some("shop")),
                c("1a2b3c4d5e6f", "shop-api-1", "registry.example/shop/api:2.15.0-rc1", "restarting", "Restarting (1) 8 seconds ago", "", Some("shop")),
            ]),
        );
        lists.insert(BACKUPS.to_owned(), ContainerList::default());
        lists.insert(
            DB.to_owned(),
            ContainerList {
                engines: vec![Engine::Docker],
                containers: vec![],
                problems: vec!["Docker is installed, but this user can't use it. Add the user to the docker group (sudo usermod -aG docker $USER), then sign in again.".into()],
            },
        );
        lists.insert("local".to_owned(), ContainerList { engines: vec![Engine::Podman], containers: vec![], problems: vec![] });
        DemoContainers { lists }
    }

    /// What a look or a button gets back.
    pub fn answer(&mut self, ask: &Ask) -> Answer {
        match &ask.want {
            Want::List { ticket } => Answer::Listed { ticket: *ticket, result: Ok(self.lists.get(&ask.server).cloned().unwrap_or_default()) },
            Want::Act { id, name, action, .. } => {
                if let Some(c) = self.lists.get_mut(&ask.server).and_then(|l| l.containers.iter_mut().find(|c| c.id == *id)) {
                    match action {
                        Action::Start | Action::Restart => {
                            (c.running, c.state, c.status) = (true, "running".into(), "Up Less than a second".into());
                        }
                        Action::Stop => {
                            (c.running, c.state, c.status) = (false, "exited".into(), "Exited (0) Less than a second ago".into());
                        }
                    }
                }
                Answer::Acted { id: id.clone(), name: name.clone(), action: *action, result: Ok(()) }
            }
        }
    }
}

/// The programs on a machine: a web server's on web-7731, a database's on db-main.
pub fn programs(server: &str) -> Vec<Process> {
    let p = |pid: u32, cpu: f64, mb: u64, command: &str| Process { pid, cpu, memory: mb << 20, command: command.into() };
    if server == DB {
        return vec![p(812, 9.4, 2_310, "postgres"), p(1907, 2.2, 640, "postgres"), p(77, 0.8, 92, "pgbouncer"), p(1, 0.0, 12, "systemd")];
    }
    vec![
        p(2231, 14.2, 412, "node"),
        p(1874, 6.8, 2_048, "postgres"),
        p(2210, 3.1, 96, "nginx"),
        p(2402, 2.4, 188, "redis-server"),
        p(911, 0.6, 54, "dockerd"),
        p(1, 0.0, 12, "systemd"),
        p(3120, 0.0, 8, "sshd"),
    ]
}

/// The facts of a look at each machine that has its vital signs in the scene.
pub fn facts() -> Vec<(&'static str, Facts)> {
    const GB: u64 = 1 << 30;
    vec![
        (
            WEB,
            Facts {
                hostname: "web-7731".into(),
                kernel: "Linux 6.8.0-45-generic".into(),
                cores: 8,
                load: [1.12, 0.94, 0.88],
                mem_total: 16 * GB,
                mem_available: 16 * GB * 2 / 5,
                swap_total: 2 * GB,
                swap_free: 2 * GB - (180 << 20),
                net_rx: 812 * GB,
                net_tx: 203 * GB,
                month: Some(Month { month: "2026-11".into(), rx: 96 * GB, tx: 31 * GB }),
                disks: vec![Disk { mount: "/".into(), size: 160 * GB, used: 70 * GB }, Disk { mount: "/srv/shop".into(), size: 500 * GB, used: 211 * GB }],
                uptime_secs: 37 * 86_400 + 5 * 3_600,
            },
        ),
        (
            DB,
            Facts {
                hostname: "db-main".into(),
                kernel: "Linux 6.1.0-26-amd64".into(),
                cores: 16,
                load: [0.31, 0.42, 0.40],
                mem_total: 64 * GB,
                mem_available: 64 * GB * 29 / 100,
                swap_total: 0,
                swap_free: 0,
                net_rx: 3_100 * GB,
                net_tx: 2_600 * GB,
                month: None,
                disks: vec![Disk { mount: "/".into(), size: 40 * GB, used: 11 * GB }, Disk { mount: "/var/lib/postgresql".into(), size: 2_000 * GB, used: 1_840 * GB }],
                uptime_secs: 212 * 86_400 + 9 * 3_600,
            },
        ),
    ]
}

/// Five made-up minutes of a machine's looks, ending at `last`: a calm line with a few busy
/// moments, the same every run.
pub fn history(last: &Vitals, seed: u32) -> Vec<Vitals> {
    let n = sshee_app::workbench::vitals::HISTORY - 1;
    (0..n)
        .map(|i| {
            let x = i as f64 + seed as f64 * 17.0;
            let wave = (x / 9.0).sin() * 0.5 + (x / 3.7).sin() * 0.25 + (x / 23.0).cos() * 0.25;
            let burst = if (i + seed as usize * 13) % 47 < 4 { 0.28 } else { 0.0 };
            let cpu = (last.cpu.unwrap_or(0.2) * 0.8 + wave * 0.1 + burst).clamp(0.02, 0.98);
            let net = (1.0 + wave * 0.6 + burst * 3.0).max(0.05);
            Vitals {
                cpu: Some(cpu),
                memory: (last.memory - 0.03 + (x / 31.0).sin() * 0.02).clamp(0.0, 1.0),
                down: last.down.map(|d| d * net),
                up: last.up.map(|u| u * (net * 0.8 + 0.2)),
                ..last.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::containers::{Board, Problem};
    use sshee_app::workbench::TabId;

    fn look(d: &mut DemoContainers, server: &str) -> Board {
        let mut b = Board::default();
        let want = b.list();
        let Answer::Listed { ticket, result } = d.answer(&Ask { tab: TabId(1), server: server.into(), want }) else { unreachable!() };
        b.listed(ticket, result);
        b
    }

    #[test]
    fn every_machine_shows_another_state() {
        let mut d = DemoContainers::new();
        let web = look(&mut d, WEB);
        assert_eq!(web.summary().as_deref(), Some("7 containers, 5 running"));
        assert_eq!(web.groups().iter().map(|g| g.project.as_deref()).collect::<Vec<_>>(), [Some("shop"), None]);
        assert!(matches!(look(&mut d, BACKUPS).problem(), Some(Problem::Page { ref title, .. }) if title == "No Docker here"));
        assert!(matches!(look(&mut d, DB).problem(), Some(Problem::Page { command: Some(ref c), .. }) if c.contains("usermod")));
        assert_eq!(look(&mut d, "local").empty().map(|e| e.0), Some("No containers yet"));
        // A stop takes at once.
        let ask = Ask { tab: TabId(1), server: WEB.into(), want: Want::Act { engine: Engine::Docker, id: "c3d4e5f6a1b2".into(), name: "shop-worker-1".into(), action: Action::Stop } };
        assert!(matches!(d.answer(&ask), Answer::Acted { result: Ok(()), .. }));
        assert_eq!(look(&mut d, WEB).summary().as_deref(), Some("7 containers, 4 running"));
    }

    #[test]
    fn the_made_up_history_is_calm_and_ends_where_the_scene_is() {
        let last = Vitals { cpu: Some(0.38), memory: 0.6, down: Some(1000.0), up: Some(100.0), ..Vitals::default() };
        let h = history(&last, 1);
        assert_eq!(h.len(), sshee_app::workbench::vitals::HISTORY - 1);
        assert!(h.iter().all(|v| v.cpu.is_some_and(|c| (0.0..=1.0).contains(&c)) && (0.0..=1.0).contains(&v.memory)));
        assert_eq!(history(&last, 1), h, "the same every run");
        assert!(programs(WEB).len() > sshee_app::stats::TOP && programs(DB)[0].command == "postgres");
    }
}
