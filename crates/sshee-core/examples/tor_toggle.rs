//! Tor turned on, off and on again while it starts, as a quick double click
//! does: it must still come up. Then Start fresh in the middle of a start: it must
//! come up too. Then Start fresh with Tor turned off right after: it must stay off,
//! whatever a start under way was about to say. Prints each status as it changes.
//!
//!   cargo run -p sshee-core --release --example tor_toggle -- [ms between clicks]

use sshee_core::tor::{self, TorStatus};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The last the app was told, which is what its button shows.
static TOLD: Mutex<Option<TorStatus>> = Mutex::new(None);

fn main() {
    if let Some(code) = tor::proxy_main() {
        std::process::exit(code);
    }
    let gap: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(300);
    let started = Instant::now();
    tor::watch(move |s| {
        println!("{:>6} ms  {s:?}", started.elapsed().as_millis());
        *TOLD.lock().unwrap() = Some(s);
    });
    tor::init(std::env::current_exe().unwrap(), vec![]);
    tor::set_on(true, vec![]).unwrap();
    std::thread::sleep(Duration::from_millis(gap));
    tor::set_on(false, vec![]).unwrap();
    std::thread::sleep(Duration::from_millis(gap));
    tor::set_on(true, vec![]).unwrap();
    wait_for_on(started);
    println!("ON after {} ms", started.elapsed().as_millis());
    // Start fresh in the middle of a start, which on Windows still holds Tor's files.
    tor::set_on(false, vec![]).unwrap();
    tor::set_on(true, vec![]).unwrap();
    std::thread::sleep(Duration::from_millis(gap));
    match tor::start_afresh() {
        Ok(()) => println!("Start fresh deleted what Tor keeps"),
        Err(said) => println!("Start fresh says: {said}"),
    }
    wait_for_on(started);
    println!("ON again after Start fresh, {} ms", started.elapsed().as_millis());
    let fresh = std::thread::spawn(tor::start_afresh);
    std::thread::sleep(Duration::from_millis(50));
    tor::set_on(false, vec![]).unwrap();
    let _ = fresh.join().unwrap();
    std::thread::sleep(Duration::from_secs(10));
    let told = TOLD.lock().unwrap().clone();
    if told != Some(TorStatus::Off) {
        println!("FAILED: Tor was turned off, but the app is told {told:?}");
        std::process::exit(1);
    }
    println!("OK: stayed off");
}

fn wait_for_on(started: Instant) {
    let waiting = Instant::now();
    while tor::status() != TorStatus::On {
        if waiting.elapsed() > Duration::from_secs(120) {
            println!("STUCK at {:?}, {} ms in", tor::status(), started.elapsed().as_millis());
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
