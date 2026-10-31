//! How long opening a folder takes: a new `ssh` per command (what every
//! file operation did before 0.5) against a shell that stays signed in.
//!
//! cargo run -p sshee-core --example speed_check -- <ssh_config> <host> <control_dir|none>
//!
//! With `none`, ssh runs without connection sharing, as it does on Windows.

use sshee_core::ssh::remote_command;
use sshee_core::{files, SshEnv, SshTarget, Target};
use std::path::PathBuf;
use std::time::Instant;

const ROUNDS: u32 = 10;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, cfg, host, control] = &args[..] else {
        panic!("usage: speed_check <ssh_config> <host> <control_dir|none>");
    };
    let t = SshTarget { destination: host.clone(), extra_args: vec!["-F".into(), cfg.clone()], ..Default::default() };
    let env = if control == "none" { SshEnv::default() } else { SshEnv::with_control_dir(PathBuf::from(control)) };
    let target = Target::Ssh(t.clone());

    let started = Instant::now();
    for _ in 0..ROUNDS {
        let out = remote_command(&t, &env, "cd / && find . -mindepth 1 -maxdepth 1 -printf '%f\\0'").output().expect("ssh");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let fresh = started.elapsed() / ROUNDS;

    let started = Instant::now();
    files::list(&target, &env, "/").expect("first listing");
    let first = started.elapsed();
    let started = Instant::now();
    let mut count = 0;
    for _ in 0..ROUNDS {
        count = files::list(&target, &env, "/").expect("listing").entries.len();
    }
    let pooled = started.elapsed() / ROUNDS;

    println!("new ssh each time: {fresh:?} per folder");
    println!("signed-in shell:   {pooled:?} per folder ({count} entries; the first one, which signs in, took {first:?})");
}
