//! End-to-end check of copying between two servers over real ssh: files and
//! folders arrive whole, progress reaches the total, nothing is replaced
//! unless asked, and Stop midway leaves nothing behind.
//!
//! cargo run -p sshee-core --example copy_check -- <ssh_config> <host_a> <host_b>
//!
//! The two hosts may be the same machine.

use sshee_core::files::{self, Transfer};
use sshee_core::{ssh, ErrorCode, SshEnv, SshTarget, Target};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MIB: usize = 1 << 20;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, cfg, a, b] = &args[..] else {
        panic!("usage: copy_check <ssh_config> <host_a> <host_b>");
    };
    let server = |host: &str| SshTarget {
        destination: host.to_string(),
        extra_args: vec!["-F".into(), cfg.clone()],
        ..Default::default()
    };
    let (sa, sb, env) = (server(a), server(b), SshEnv::default());
    let (a, b) = (Target::Ssh(sa.clone()), Target::Ssh(sb.clone()));

    let base = format!("/tmp/sshee-copy-{}", std::process::id());
    let (from, to) = (format!("{base}/from"), format!("{base}/to"));
    let run = |t: &SshTarget, script: &str| String::from_utf8_lossy(&ssh::run_remote(t, &env, script, None).expect(script)).into_owned();
    run(&sa, &format!("mkdir -p {from}/site/css {to} && head -c {} /dev/urandom > {from}/big.bin && echo hi > {from}/site/index.html && echo body > {from}/site/css/a.css", 64 * MIB));
    let paths = vec![format!("{from}/big.bin"), format!("{from}/site")];

    let recorded = || {
        let reports = Arc::new(Mutex::new(Vec::new()));
        let r = reports.clone();
        (Transfer::new(move |p| r.lock().unwrap().push((p.done, p.total))), reports)
    };

    // ---- arrives whole, progress reaches the total
    let (xfer, reports) = recorded();
    files::copy_between(&a, &b, &env, &paths, &to, false, &xfer).expect("copy");
    let last = *reports.lock().unwrap().last().unwrap();
    println!("copy: {} reports, last {last:?}", reports.lock().unwrap().len());
    assert!(last.1 > 0 && last.0 == last.1, "ends at the total");
    let same = run(&sb, &format!("cmp {from}/big.bin {to}/big.bin && cat {to}/site/index.html {to}/site/css/a.css && ls -A {to}"));
    println!("{same}");
    assert_eq!(same.trim(), "hi\nbody\nbig.bin\nsite");

    // ---- nothing replaced unless asked
    let err = files::copy_between(&a, &b, &env, &paths, &to, false, &Transfer::quiet()).expect_err("refuses");
    assert_eq!(err.code, ErrorCode::Exists);
    println!("refused: {}", err.message);
    run(&sa, &format!("echo changed > {from}/site/index.html"));
    files::copy_between(&a, &b, &env, &paths, &to, true, &Transfer::quiet()).expect("replace");
    assert_eq!(run(&sb, &format!("cat {to}/site/index.html")).trim(), "changed");

    // ---- Stop midway leaves nothing behind
    run(&sa, &format!("head -c {} /dev/urandom > {from}/huge.bin", 768 * MIB));
    let xfer = Transfer::new(|_| {});
    let stop = xfer.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        stop.cancel();
    });
    let err = files::copy_between(&a, &b, &env, &[format!("{from}/huge.bin")], &to, false, &xfer).expect_err("stopped");
    assert_eq!(err.code, ErrorCode::Cancelled);
    std::thread::sleep(Duration::from_secs(3));
    let left = run(&sb, &format!("ls -A {to}"));
    println!("after stop: {}", left.trim().replace('\n', " "));
    assert_eq!(left.trim(), "big.bin\nsite", "no half file, no hidden folder");

    run(&sa, &format!("rm -rf {base}"));
    println!("copy_check: all good");
}
