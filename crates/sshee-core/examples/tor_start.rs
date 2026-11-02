//! Starts SSHeesh's Tor and prints every status until it's on or `secs` pass.
//!
//!   cargo run -p sshee-core --release --example tor_start -- [secs]

use sshee_core::tor::{self, TorStatus};
use std::time::{Duration, Instant};

fn main() {
    if let Some(code) = tor::proxy_main() {
        std::process::exit(code);
    }
    let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(60);
    let started = Instant::now();
    tor::watch(move |s| println!("{:>6} ms  {s:?}", started.elapsed().as_millis()));
    tor::init(std::env::current_exe().unwrap(), vec![]);
    tor::set_on(true, vec![]).unwrap();
    while tor::status() != TorStatus::On && started.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("END {:?} after {} ms", tor::status(), started.elapsed().as_millis());
    tor::set_on(false, vec![]).unwrap();
}
