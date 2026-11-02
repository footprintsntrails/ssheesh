//! Tor for real, end to end, on the system this runs on: starts SSHeesh's Tor,
//! then has the system's ssh reach a server through it, with the same options
//! the app uses. The ProxyCommand is this program, as it's the app in SSHeesh.
//! Being refused a login proves the connection got all the way there and back.
//! Fails if Tor doesn't start, or ssh doesn't get there.
//!
//!   cargo run -p sshee-core --release --example tor_ssh_check -- test.rebex.net

use sshee_core::ssh::{common_args, ssh_program, SshEnv, SshTarget};
use sshee_core::tor::{self, TorStatus};
use std::process::{exit, Command, Stdio};
use std::time::{Duration, Instant};

fn main() {
    if let Some(code) = tor::proxy_main() {
        exit(code);
    }
    let host = std::env::args().nth(1).unwrap_or_else(|| "test.rebex.net".into());
    let exe = std::env::current_exe().unwrap();
    println!("this program: {}", exe.display());
    let started = Instant::now();
    tor::init(exe, vec![]);
    tor::set_on(true, vec![]).unwrap();
    while tor::status() != TorStatus::On {
        if started.elapsed() > Duration::from_secs(180) {
            println!("FAILED: Tor didn't start: {:?}", tor::status());
            print_log();
            exit(1);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("Tor is on after {} ms", started.elapsed().as_millis());

    let target = SshTarget { destination: host.clone(), user: Some("ssheesh-check".into()), ..Default::default() };
    assert!(tor::carries(&target), "this server should go through Tor");
    let known = std::env::temp_dir().join("ssheesh-check-known-hosts");
    let _ = std::fs::remove_file(&known);
    for attempt in 1..=3 {
        let begun = Instant::now();
        let out = Command::new(ssh_program())
            .args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no"])
            .arg("-o")
            .arg(format!("UserKnownHostsFile=\"{}\"", known.display()))
            .args(common_args(&target, &SshEnv::default()))
            .args(["-T", "--", &host, "true"])
            .stdin(Stdio::null())
            .output()
            .expect("couldn't start ssh");
        let said = String::from_utf8_lossy(&out.stderr);
        println!("ssh, try {attempt}, {} ms: {}", begun.elapsed().as_millis(), said.trim());
        if said.contains("Permission denied") {
            println!("OK: reached the server through Tor, and it asked for a login");
            tor::set_on(false, vec![]).unwrap();
            exit(0);
        }
    }
    println!("FAILED: ssh didn't get through");
    print_log();
    exit(1);
}

fn print_log() {
    if let Some(log) = tor::log_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        println!("--- Tor's log\n{log}");
    }
}
