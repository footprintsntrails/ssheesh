//! Tor for real, end to end, on the system this runs on: starts SSHeesh's Tor,
//! then has the system's ssh reach a server through it, with the same options
//! the app uses. The ProxyCommand is this program, as it's the app in SSHeesh.
//! Being refused a login proves the connection got all the way there and back,
//! and the mark the ProxyCommand leaves proves it went through Tor: ssh that
//! ignored the option would be refused just the same, straight from your address.
//! Fails if Tor doesn't start, ssh doesn't get there, or ssh goes around Tor.
//!
//!   cargo run -p sshee-core --release --example tor_ssh_check -- test.rebex.net [ssh program…]

use sshee_core::ssh::{common_args, ssh_program, SshEnv, SshTarget};
use sshee_core::tor::{self, TorStatus};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{exit, Command, Stdio};
use std::time::{Duration, Instant};

fn main() {
    let exe = std::env::current_exe().unwrap();
    let mark = exe.with_file_name("went-through-tor");
    if std::env::args().any(|a| a == "--tor-proxy") {
        std::fs::write(&mark, "").unwrap();
    }
    if let Some(code) = tor::proxy_main() {
        exit(code);
    }
    let mut args = std::env::args().skip(1);
    let host = args.next().unwrap_or_else(|| "test.rebex.net".into());
    let mut programs: Vec<OsString> = args.map(OsString::from).collect();
    if programs.is_empty() {
        programs.push(ssh_program());
    }
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
    // A transfer's route, the way a transfer asks for it: the first value ssh hears wins.
    let files = tor::transfer_proxy_command_for(&target).expect("this server should go through Tor");
    let as_transfer = ["-o".to_string(), format!("ProxyCommand={files}")];
    for program in &programs {
        for first in [&[][..], &as_transfer[..]] {
            if !reached(program, &target, &host, &mark, first) {
                println!("FAILED: ssh didn't get through");
                print_log();
                exit(1);
            }
        }
    }
    tor::set_on(false, vec![]).unwrap();
}

fn reached(program: &OsStr, target: &SshTarget, host: &str, mark: &Path, first: &[String]) -> bool {
    let version = Command::new(program).arg("-V").output().expect("couldn't start ssh");
    println!("{}: {}", program.display(), String::from_utf8_lossy(&version.stderr).trim());
    for attempt in 1..=3 {
        let _ = std::fs::remove_file(mark);
        let begun = Instant::now();
        let out = Command::new(program)
            .args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null"])
            .args(first)
            .args(common_args(target, &SshEnv::default()))
            .args(["-T", "--", host, "true"])
            .stdin(Stdio::null())
            .output()
            .expect("couldn't start ssh");
        let said = String::from_utf8_lossy(&out.stderr);
        println!("ssh, try {attempt}, {} ms: {}", begun.elapsed().as_millis(), said.trim());
        if said.contains("Permission denied") {
            if !mark.exists() {
                println!("FAILED: ssh reached the server without going through Tor");
                exit(1);
            }
            println!("OK: reached the server through Tor, and it asked for a login");
            return true;
        }
    }
    false
}

fn print_log() {
    if let Some(log) = tor::log_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        println!("--- Tor's log\n{log}");
    }
}
