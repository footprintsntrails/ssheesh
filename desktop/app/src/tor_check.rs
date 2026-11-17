//! `ssheesh-preview --tor-check`: starts this copy's Tor without a window, says each step on
//! the terminal, and exits 0 once Tor is on (1 if it isn't within three minutes). The same
//! start the window makes (`sshee_core::tor`, this copy's folder, this executable as ssh's
//! helper), for finding out on a computer where Tor won't start, and for the test that needs
//! the network (`tests/tor.rs`, `SSHEE_TOR_NETWORK=1`).

use std::sync::mpsc;
use std::time::{Duration, Instant};

use sshee_core::tor::{self, TorStatus};

const WAIT: Duration = Duration::from_secs(180);

pub fn run() -> i32 {
    let (tx, rx) = mpsc::channel();
    tor::watch(move |status| {
        let _ = tx.send(status);
    });
    println!("Tor's folder: {}", tor::log_path().and_then(|p| p.parent().map(|d| d.display().to_string())).unwrap_or_else(|| "none (no home folder)".into()));
    tor::init(std::env::current_exe().unwrap_or_default(), vec![]);
    if !tor::is_on() {
        if let Err(e) = tor::set_on(true, vec![]) {
            println!("Tor: {e}");
        }
    }
    let deadline = Instant::now() + WAIT;
    let mut last = tor::status();
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(TorStatus::On) => {
                println!("Tor is on");
                return 0;
            }
            Ok(status) => {
                println!("{status:?}");
                last = status;
            }
            Err(_) => {
                println!("Tor didn't come on within {} s. Last: {last:?}. Its log: {}", WAIT.as_secs(), tor::log_path().map(|p| p.display().to_string()).unwrap_or_default());
                return 1;
            }
        }
    }
}
