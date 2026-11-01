//! End-to-end check that transfers live through a dropped connection: they
//! wait, go on once the server is back (a file from where it stopped), and
//! everything arrives whole. The drop is `<drop>`, a shell command run once
//! some of the data has moved, for example one that ends the test server's
//! connections.
//!
//! cargo run -p sshee-core --example resume_check -- <ssh_config> <host> <control_dir|none> <drop>

use sshee_core::files::{self, Progress, Transfer};
use sshee_core::{ssh, SshEnv, SshTarget, Target};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const MIB: u64 = 1 << 20;

/// A transfer that runs `drop` once, when a third of it has moved, and keeps every report.
fn dropping(drop: &str) -> (Arc<Transfer>, Arc<Mutex<Vec<Progress>>>) {
    let reports: Arc<Mutex<Vec<Progress>>> = Arc::default();
    let (r, fired, drop) = (reports.clone(), Arc::new(AtomicBool::new(false)), drop.to_string());
    let xfer = Transfer::new(move |p| {
        r.lock().unwrap().push(p);
        if p.total > 0 && p.done > p.total / 3 && !fired.swap(true, Ordering::Relaxed) {
            println!("  dropping the connection at {} of {} MiB", p.done / MIB, p.total / MIB);
            let _ = Command::new("sh").arg("-c").arg(&drop).status();
        }
    });
    (xfer, reports)
}

/// Checks the reports: it waited, came back, and (for a file) went on from
/// where it was. A folder's total is an estimate, so only a file's count
/// ends exactly on it.
fn check_reports(reports: &[Progress], picks_up: bool) {
    let first_wait = reports.iter().position(|p| p.waiting).expect("it waited for the server");
    let at_drop = reports[first_wait].done;
    assert!(reports[first_wait..].iter().any(|p| !p.waiting), "it came back");
    // Where the next attempt began: the least it counted from then on.
    let from = reports[first_wait..].iter().map(|p| p.done).min().unwrap();
    let last = reports.last().unwrap();
    println!("  waited at {} MiB, went on from {} MiB, ended at {} of {} MiB", at_drop / MIB, from / MIB, last.done / MIB, last.total / MIB);
    assert!(!last.waiting);
    if picks_up {
        assert_eq!(last.done, last.total);
        assert!(from > 0 && from + 8 * MIB >= at_drop, "picked up where it stopped");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, cfg, host, control, drop] = &args[..] else {
        panic!("usage: resume_check <ssh_config> <host> <control_dir|none> <drop>");
    };
    let st = SshTarget { destination: host.clone(), extra_args: vec!["-F".into(), cfg.clone()], ..Default::default() };
    let target = Target::Ssh(st.clone());
    let env = if control == "none" { SshEnv::default() } else { SshEnv::with_control_dir(PathBuf::from(control)) };
    let run = |script: &str| String::from_utf8_lossy(&ssh::run_remote(&st, &env, script, None).expect(script)).trim().to_string();

    let base = format!("/tmp/sshee-resume-{}", std::process::id());
    run(&format!("mkdir -p {base}/site && head -c {} /dev/urandom > {base}/big.bin && head -c {} /dev/urandom > {base}/site/a.bin && echo hi > {base}/site/b.txt", 192 * MIB, 96 * MIB));
    let sum = |path: &str| run(&format!("sha256sum {path} | cut -d' ' -f1"));
    let local = std::env::temp_dir().join(format!("sshee-resume-local-{}", std::process::id()));
    std::fs::create_dir_all(&local).unwrap();
    let local_sum = |path: &PathBuf| {
        let out = Command::new("sha256sum").arg(path).output().unwrap();
        String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap().to_string()
    };

    println!("download a file:");
    let (xfer, reports) = dropping(drop);
    let got = PathBuf::from(files::download(&target, &env, &format!("{base}/big.bin"), false, &local, &xfer).expect("download"));
    check_reports(&reports.lock().unwrap(), true);
    assert_eq!(local_sum(&got), sum(&format!("{base}/big.bin")), "the file arrived whole");

    println!("upload a file:");
    let (xfer, reports) = dropping(drop);
    xfer_total(&xfer, &got);
    run(&format!("mkdir -p {base}/up"));
    files::upload(&target, &env, &got, &format!("{base}/up"), false, &xfer).expect("upload");
    check_reports(&reports.lock().unwrap(), true);
    assert_eq!(sum(&format!("{base}/up/big.bin")), local_sum(&got), "the upload arrived whole");
    assert_eq!(run(&format!("ls -A {base}/up")), "big.bin", "nothing staged is left behind");

    println!("download a folder:");
    let (xfer, reports) = dropping(drop);
    let site = PathBuf::from(files::download(&target, &env, &format!("{base}/site"), true, &local, &xfer).expect("download folder"));
    check_reports(&reports.lock().unwrap(), false);
    assert_eq!(local_sum(&site.join("a.bin")), sum(&format!("{base}/site/a.bin")));
    assert_eq!(std::fs::read_to_string(site.join("b.txt")).unwrap(), "hi\n");
    let leftovers: Vec<_> = std::fs::read_dir(&local).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".sshee-part")).collect();
    assert!(leftovers.is_empty(), "no scratch left in Downloads");

    run(&format!("rm -rf {base}"));
    std::fs::remove_dir_all(&local).ok();
    println!("resume_check: all good");
}

/// Sets an upload's total, as the app does before it starts.
fn xfer_total(xfer: &Transfer, path: &Path) {
    xfer.add_total(files::upload_size(path));
}
