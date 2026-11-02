//! Tor for real, the way the app does it: starts SSHeesh's Tor, then prints
//! the ssh options that send a connection through it (the ProxyCommand is
//! this program, as it's the app in SSHeesh). Keeps Tor on until its input
//! closes, so a script can time ssh logins and round trips meanwhile.
//!
//!   cargo run -p sshee-core --release --example tor_check

use sshee_core::tor::{self, TorStatus};
use std::io::Read;
use std::time::{Duration, Instant};

fn main() {
    if let Some(code) = tor::proxy_main() {
        std::process::exit(code);
    }
    let started = Instant::now();
    tor::init(std::env::current_exe().unwrap(), vec![]);
    tor::set_on(true, vec![]).unwrap();
    while tor::status() != TorStatus::On {
        if let TorStatus::Failed { message } = tor::status() {
            eprintln!("failed: {message}");
        }
        assert!(started.elapsed() < Duration::from_secs(90), "Tor didn't start");
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("READY {}", started.elapsed().as_millis());
    for arg in tor::ssh_options(&tor::proxy_command().unwrap()) {
        println!("{arg}");
    }
    println!("END");
    // With a config file and a host: the arguments SSHeesh itself would use for it.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [config, host] = args.as_slice() {
        let t = sshee_core::SshTarget { destination: host.clone(), extra_args: vec!["-F".into(), config.clone()], ..Default::default() };
        for arg in sshee_core::ssh::common_args(&t, &sshee_core::SshEnv::default()) {
            println!("{arg}");
        }
        println!("END");
    }
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    tor::set_on(false, vec![]).unwrap();
}
