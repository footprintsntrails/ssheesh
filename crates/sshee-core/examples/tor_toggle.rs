//! Tor turned on, off and on again while it starts, as a quick double click
//! does: it must still come up. Prints each status as it changes.
//!
//!   cargo run -p sshee-core --release --example tor_toggle -- [ms between clicks]

use sshee_core::tor::{self, TorStatus};
use std::time::{Duration, Instant};

fn main() {
    if let Some(code) = tor::proxy_main() {
        std::process::exit(code);
    }
    let gap: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(300);
    let started = Instant::now();
    tor::watch(move |s| println!("{:>6} ms  {s:?}", started.elapsed().as_millis()));
    tor::init(std::env::current_exe().unwrap(), vec![]);
    tor::set_on(true, vec![]).unwrap();
    std::thread::sleep(Duration::from_millis(gap));
    tor::set_on(false, vec![]).unwrap();
    std::thread::sleep(Duration::from_millis(gap));
    tor::set_on(true, vec![]).unwrap();
    while tor::status() != TorStatus::On {
        if started.elapsed() > Duration::from_secs(90) {
            println!("STUCK at {:?}", tor::status());
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("ON after {} ms", started.elapsed().as_millis());
    tor::set_on(false, vec![]).unwrap();
}
