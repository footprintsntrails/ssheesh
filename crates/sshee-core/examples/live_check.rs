//! End-to-end check against a real ssh server: metrics, files, and a
//! terminal session that survives the connection being cut.
//!
//! cargo run -p sshee-core --example live_check -- <ssh_config> <host> <sshd_pid> <control_dir>
//!
//! `sshd_pid` is the listening sshd; killing its children cuts every live
//! connection, which is what a network drop looks like to the client.

use sshee_core::session::EventSink;
use sshee_core::{containers, files, metrics, session, SessionEvent, SessionManager, SessionSpec, SessionState, SshEnv, SshTarget, Target};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, cfg, host, sshd_pid, control] = &args[..] else {
        panic!("usage: live_check <ssh_config> <host> <sshd_pid> <control_dir>");
    };
    let target = Target::Ssh(SshTarget {
        destination: host.clone(),
        extra_args: vec!["-F".into(), cfg.clone()],
        ..Default::default()
    });
    let env = SshEnv::with_control_dir(PathBuf::from(control));

    // ---- metrics
    let m = metrics::fetch(&target, &env).expect("metrics");
    println!(
        "metrics: host={} kernel={} cores={} load={:?} mem={} MiB avail={} MiB rx={} tx={} ifaces={:?} disks={:?} month={:?}",
        m.hostname,
        m.kernel,
        m.cores,
        m.load,
        m.mem_total >> 20,
        m.mem_available >> 20,
        m.net_rx,
        m.net_tx,
        m.net_ifaces,
        m.disks.iter().map(|d| (&d.mount, d.used >> 30, d.size >> 30)).collect::<Vec<_>>(),
        m.month
    );
    assert!(m.cpu_total > 0 && m.mem_total > 0);

    // ---- files
    let home = files::list(&target, &env, "~").expect("list ~");
    println!("files: ~ = {} ({} entries)", home.path, home.entries.len());
    let etc = files::list(&target, &env, "/etc").expect("list /etc");
    assert!(etc.entries.iter().any(|e| e.name == "hostname"));
    let hostname = files::read_text(&target, &env, "/etc/hostname").expect("read");
    println!("files: /etc/hostname = {:?}", hostname.content.as_deref().map(str::trim));

    let base = format!("/tmp/sshee-live-{}", std::process::id());
    files::make_dir(&target, &env, &format!("{base}/sub dir")).expect("mkdir");
    files::write_text(&target, &env, &format!("{base}/sub dir/it's.txt"), "привет\n").expect("write");
    let back = files::read_text(&target, &env, &format!("{base}/sub dir/it's.txt")).expect("read back");
    assert_eq!(back.content.as_deref(), Some("привет\n"));
    files::rename(&target, &env, &format!("{base}/sub dir/it's.txt"), &format!("{base}/sub dir/renamed.txt")).expect("rename");
    let dup = files::rename(&target, &env, &format!("{base}/sub dir"), &format!("{base}/sub dir"));
    assert!(dup.is_err(), "renaming onto an existing name must fail");

    let dest = std::env::temp_dir().join(format!("sshee-dl-{}", std::process::id()));
    std::fs::create_dir_all(&dest).unwrap();
    let f = files::download(&target, &env, &format!("{base}/sub dir/renamed.txt"), false, &dest).expect("download file");
    let d = files::download(&target, &env, &format!("{base}/sub dir"), true, &dest).expect("download dir");
    let d2 = files::download(&target, &env, &format!("{base}/sub dir"), true, &dest).expect("download dir again");
    println!("files: downloaded {f}, {d}, {d2}");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "привет\n");
    assert!(d2.ends_with("sub dir (1)"));
    files::upload(&target, &env, &PathBuf::from(&d), &base, false).expect_err("upload must not overwrite");
    files::upload(&target, &env, &PathBuf::from(&d2), &base, false).expect("upload dir");
    files::upload(&target, &env, &PathBuf::from(&f), &base, false).expect("upload file");
    let after = files::list(&target, &env, &base).expect("list after upload");
    let names: Vec<_> = after.entries.iter().map(|e| e.name.as_str()).collect();
    println!("files: {base} now has {names:?}");
    assert!(names.contains(&"sub dir (1)") && names.contains(&"renamed.txt"));
    files::delete(&target, &env, &base).expect("delete");
    std::fs::remove_dir_all(&dest).ok();

    // ---- durable session across a dropped connection
    let manager = SessionManager::new();
    let (tx, rx) = channel();
    let sink: EventSink = Arc::new(move |e| {
        let _ = tx.send(e);
    });
    let key = format!("livecheck{}", std::process::id());
    manager.open("p1", SessionSpec { target: target.clone(), key: key.clone(), cols: 100, rows: 30, env: env.clone(), run: None }, sink);
    let mut log = Log { rx, output: String::new() };

    log.wait("connected (durable)", 20, |s| matches!(s, SessionState::Connected { durable: true }));
    manager.write("p1", b"export SSHEE_MARK=alive-123; cd /var/log\r");
    std::thread::sleep(Duration::from_millis(800));
    let cwd = files::session_cwd(&target, &env, &key).expect("cwd");
    println!("session: cwd from tmux = {cwd}");
    assert_eq!(cwd, "/var/log");

    println!("session: cutting every connection (pkill -P {sshd_pid})");
    let t0 = Instant::now();
    std::process::Command::new("pkill").args(["-P", sshd_pid]).status().unwrap();
    let state = log.wait("reconnecting", 30, |s| matches!(s, SessionState::Reconnecting { .. }));
    println!("session: noticed the drop after {:?}: {state:?}", t0.elapsed());
    log.wait("connected again (durable)", 30, |s| matches!(s, SessionState::Connected { durable: true }));
    println!("session: back after {:?}", t0.elapsed());
    manager.write("p1", b"echo mark=$SSHEE_MARK\r");
    log.wait_output("mark=alive-123", 10);
    println!("session: shell state survived the drop");

    // ---- manual reconnect: wait for the user, then pick the session back up
    manager.set_auto_reconnect(false);
    std::process::Command::new("pkill").args(["-P", sshd_pid]).status().unwrap();
    log.wait("disconnected (manual mode)", 30, |s| matches!(s, SessionState::Disconnected { .. }));
    log.expect_quiet(3);
    println!("session: manual mode waited without reconnecting");
    manager.reconnect_now("p1");
    log.wait("connected after Reconnect", 30, |s| matches!(s, SessionState::Connected { durable: true }));
    println!("session: Reconnect brought it back");

    // Turning automatic reconnecting on resumes a session that's waiting.
    std::process::Command::new("pkill").args(["-P", sshd_pid]).status().unwrap();
    log.wait("disconnected again", 30, |s| matches!(s, SessionState::Disconnected { .. }));
    manager.set_auto_reconnect(true);
    log.wait("resumed by turning auto on", 30, |s| matches!(s, SessionState::Connected { durable: true }));
    manager.write("p1", b"echo again=$SSHEE_MARK\r");
    log.wait_output("again=alive-123", 10);
    println!("session: switching to automatic resumed it, shell state intact");

    // ---- containers: list, act, and a durable shell inside one
    let list = containers::list(&target, &env).expect("containers");
    println!(
        "containers: engines={:?} {:?} problems={:?}",
        list.engines,
        list.containers.iter().map(|c| (&c.name, c.running, &c.project)).collect::<Vec<_>>(),
        list.problems
    );
    if let Some(c) = list.containers.first().cloned() {
        containers::act(&target, &env, c.engine, &c.id, containers::Action::Restart).expect("restart");
        let (tx2, rx2) = channel();
        let sink2: EventSink = Arc::new(move |e| {
            let _ = tx2.send(e);
        });
        let key2 = format!("{key}c");
        let run = Some(containers::argv(c.engine, &c.id, containers::Run::Shell));
        manager.open("p2", SessionSpec { target: target.clone(), key: key2.clone(), cols: 100, rows: 30, env: env.clone(), run }, sink2);
        let mut log2 = Log { rx: rx2, output: String::new() };
        log2.wait("container shell (durable)", 20, |s| matches!(s, SessionState::Connected { durable: true }));
        manager.write("p2", b"echo inside-container=$((6*7))\r");
        log2.wait_output("inside-container=42", 10);
        println!("containers: shell inside {} works and is durable", c.name);
        manager.close("p2");
        session::end_remote(&target, &env, &key2).expect("end container session");
    }

    manager.close("p1");
    session::end_remote(&target, &env, &key).expect("end remote");
    println!("ALL OK");
}

struct Log {
    rx: Receiver<SessionEvent>,
    output: String,
}

impl Log {
    fn wait(&mut self, what: &str, secs: u64, ok: impl Fn(&SessionState) -> bool) -> SessionState {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(left) {
                Ok(SessionEvent::Status { state }) => {
                    if ok(&state) {
                        return state;
                    }
                    if matches!(state, SessionState::Failed { .. } | SessionState::Ended { .. }) {
                        panic!("waiting for {what}, got {state:?}\n--- output ---\n{}", self.output);
                    }
                }
                Ok(SessionEvent::Output { data }) => self.output.push_str(&data),
                Err(_) => break,
            }
        }
        panic!("timed out waiting for {what}\n--- output ---\n{}", self.output);
    }

    /// Fails if the session tries to connect on its own within `secs`.
    fn expect_quiet(&mut self, secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(left) {
                Ok(SessionEvent::Status { state }) => panic!("expected no reconnect, got {state:?}"),
                Ok(SessionEvent::Output { data }) => self.output.push_str(&data),
                Err(_) => break,
            }
        }
    }

    fn wait_output(&mut self, needle: &str, secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while !self.output.contains(needle) {
            let left = deadline.checked_duration_since(Instant::now()).unwrap_or_else(|| {
                panic!("timed out waiting for {needle:?}\n--- output ---\n{}", self.output)
            });
            if let Ok(SessionEvent::Output { data }) = self.rx.recv_timeout(left) {
                self.output.push_str(&data);
            }
        }
    }
}
