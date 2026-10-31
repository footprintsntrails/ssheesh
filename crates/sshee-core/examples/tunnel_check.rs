//! End-to-end check of tunnels against a real ssh server: the web-app scan
//! finds a page by its title, a tunnel and a SOCKS proxy reach it, ports
//! already taken here are refused plainly, and Stop ends the ssh process.
//!
//! cargo run -p sshee-core --example tunnel_check -- <ssh_config> <host> <remote_port> <title> [--wait-for-drop]
//!
//! The server must have a web page with that <title> on <remote_port>. With
//! `--wait-for-drop` it prints DROP-NOW once the tunnel is up and waits for
//! you to cut its connection (say, by ending its sshd process on the
//! server), then checks the tunnel comes back by itself.

use sshee_core::tunnels::{self, Forward, StateSink, TunnelManager, TunnelState};
use sshee_core::{SshEnv, SshTarget, Target};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, cfg, host, port, title, rest @ ..] = &args[..] else {
        panic!("usage: tunnel_check <ssh_config> <host> <remote_port> <title> [--wait-for-drop]");
    };
    let wait_for_drop = rest.iter().any(|a| a == "--wait-for-drop");
    let ssh = SshTarget { destination: host.clone(), extra_args: vec!["-F".into(), cfg.clone()], ..Default::default() };
    let env = SshEnv::default();
    let remote_port: u16 = port.parse().expect("remote port");

    let started = Instant::now();
    let scan = tunnels::scan(&Target::Ssh(ssh.clone()), &env).expect("scan");
    println!("scan: {} ports in {:.1?}, curl={}, root={}", scan.apps.len(), started.elapsed(), scan.curl, scan.as_root);
    for a in &scan.apps {
        let what = a.title.as_deref().or(a.known_as.as_deref()).unwrap_or("");
        println!(
            "  {:>5} {:<16} web={:<5} {:<5} {:<4} via {:<10} {what}",
            a.port,
            a.process.as_deref().unwrap_or("-"),
            a.web,
            a.scheme.as_deref().unwrap_or("-"),
            a.status.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
            a.remote_host
        );
    }
    let app = scan.apps.iter().find(|a| a.port == remote_port).expect("the test page is listed");
    assert_eq!(app.title.as_deref(), Some(title.as_str()), "title read from the page");
    assert!(app.web);
    let first_other = scan.apps.iter().position(|a| !a.web).unwrap_or(scan.apps.len());
    assert!(scan.apps[first_other..].iter().all(|a| !a.web), "web apps come first");

    let manager = TunnelManager::new();
    let local = tunnels::pick_local_port(remote_port, &[]).expect("a free port");
    let (sink, states) = channel();
    let forward = Forward::Local { local_port: local, remote_host: app.remote_host.clone(), remote_port };
    manager.start("web", &ssh, &env, forward.clone(), sink).expect("start");
    wait_for(&states, "on", 20);
    let page = get(local, None);
    assert!(page.contains(title.as_str()), "the page came through the tunnel");
    let pid = ssh_pid(local).expect("the tunnel's ssh process");
    println!("tunnel: localhost:{local} -> {}:{remote_port} on the server, ssh pid {pid}", app.remote_host);

    // Starting it again the same way keeps the same process; its states go
    // to the new listener from then on.
    let (again, states) = channel();
    manager.start("web", &ssh, &env, forward, again).expect("reattach");
    wait_for(&states, "on", 2);
    assert_eq!(ssh_pid(local), Some(pid), "reattached, not restarted");

    let err = manager.start("other", &ssh, &env, Forward::Socks { local_port: local }, channel().0).unwrap_err();
    println!("same port twice: {}", err.message);
    let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let busy = held.local_addr().unwrap().port();
    let err = manager.start("busy", &ssh, &env, Forward::Socks { local_port: busy }, channel().0).unwrap_err();
    assert_eq!(err.message, format!("Port {busy} on this computer is already in use."));
    println!("busy port: {}", err.message);
    drop(held);

    let socks_port = tunnels::pick_local_port(1080, &[local]).expect("a free port");
    let (socks_sink, socks_states) = channel();
    manager.start("socks", &ssh, &env, Forward::Socks { local_port: socks_port }, socks_sink).expect("socks");
    wait_for(&socks_states, "on", 20);
    let page = get(socks_port, Some(remote_port));
    assert!(page.contains(title.as_str()), "the page came through the SOCKS proxy");
    manager.stop("socks");
    println!("socks: localhost:{socks_port} reached the page; stopped");

    let (dead_sink, dead_states) = channel();
    // Port 1 on the server has nothing; the port here mustn't be 1 too, in
    // case the server is this same computer.
    let nobody = Forward::Local { local_port: tunnels::pick_local_port(8001, &[local, socks_port]).unwrap(), remote_host: String::new(), remote_port: 1 };
    let nobody_port = nobody.local_port();
    manager.start("nobody", &ssh, &env, nobody, dead_sink).expect("start");
    wait_for(&dead_states, "on", 20);
    let _ = get_raw(nobody_port);
    let warned = wait_for(&dead_states, "on", 5);
    println!("nothing on the server's port: {warned:?}");
    manager.stop("nobody");

    while let Ok(state) = states.try_recv() {
        println!("  web state meanwhile: {state:?}");
    }
    assert_eq!(ssh_pid(local), Some(pid), "still the same ssh process");
    if wait_for_drop {
        println!("DROP-NOW pid={pid}");
        wait_for(&states, "reconnecting", 60);
        wait_for(&states, "on", 30);
        assert!(get(local, None).contains(title.as_str()), "the page again after reconnecting");
        println!("reconnected by itself");
    }

    manager.stop("web");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(ssh_pid(local), None, "no ssh process left");
    assert!(TcpStream::connect(("127.0.0.1", local)).is_err(), "nothing listens after Stop");
    println!("stopped: ssh process gone, localhost:{local} closed");
    println!("OK");
}

fn channel() -> (StateSink, Receiver<TunnelState>) {
    let (tx, rx) = mpsc::channel();
    let sink: StateSink = Arc::new(move |s| {
        let _ = tx.send(s);
    });
    (sink, rx)
}

/// Waits for a state of `kind` ("on", "reconnecting"…), printing each one.
fn wait_for(states: &Receiver<TunnelState>, kind: &str, secs: u64) -> TunnelState {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let state = states.recv_timeout(left).unwrap_or_else(|_| panic!("no {kind} state within {secs}s"));
        println!("  state: {state:?}");
        let name = serde_json::to_value(&state).unwrap()["kind"].as_str().unwrap().to_string();
        if name == kind {
            return state;
        }
        if let TunnelState::Failed { message } = state {
            panic!("tunnel failed: {message}");
        }
    }
}

/// GET / through localhost:`port`, directly or (with `via_socks`) as a
/// SOCKS5 proxy to the server's own `via_socks` port.
fn get(port: u16, via_socks: Option<u16>) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    if let Some(target) = via_socks {
        s.write_all(&[5, 1, 0]).unwrap();
        let mut hello = [0u8; 2];
        s.read_exact(&mut hello).unwrap();
        assert_eq!(hello, [5, 0]);
        let [hi, lo] = target.to_be_bytes();
        s.write_all(&[5, 1, 0, 1, 127, 0, 0, 1, hi, lo]).unwrap();
        let mut reply = [0u8; 10];
        s.read_exact(&mut reply).unwrap();
        assert_eq!(reply[1], 0, "SOCKS connect succeeded");
    }
    s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
    let mut body = String::new();
    let _ = s.read_to_string(&mut body);
    body
}

/// Opens and reads a connection that may go nowhere.
fn get_raw(port: u16) -> std::io::Result<usize> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut buf = [0u8; 64];
    s.read(&mut buf)
}

/// The ssh process listening on localhost:`port`, found by its command line.
fn ssh_pid(port: u16) -> Option<u32> {
    let out = std::process::Command::new("pgrep").args(["-f", &format!("127\\.0\\.0\\.1:{port}(:| |$)")]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next()?.trim().parse().ok()
}
