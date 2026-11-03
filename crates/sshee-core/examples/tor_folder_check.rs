//! Folders sent through real Tor arrive whole, however they travelled: starts
//! SSHeesh's Tor and downloads each folder as the app does, with the ProxyCommand
//! being this program. Each result is compared with a copy of the folder on this
//! computer, the bar has to end at the folder's size, and Stop midway has to leave
//! nothing behind. Then the copy is uploaded beside the folder, under a new name,
//! and fetched back to compare. Fails if any of that doesn't hold.
//!
//! A file instead of a folder is downloaded CHECK_TIMES times (1 unless set), on
//! newly picked routes each time, and says how often it moved off a weak route.
//!
//! Set HOME to a folder of its own, so Tor's state isn't yours, and keep the
//! server's address in the ssh config, not on the command line:
//!
//!   HOME=/tmp/tor-check cargo run -p sshee-core --release --example tor_folder_check -- \
//!       <ssh_config> <host> <folder on the server> <same folder here> [<folder> <same here>…]

use sshee_core::files::{self, Transfer};
use sshee_core::tor::{self, TorStatus};
use sshee_core::{ErrorCode, SshEnv, SshTarget, Target};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

fn main() {
    if let Some(code) = tor::proxy_main() {
        exit(code);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [cfg, host, folders @ ..] = &args[..] else { panic!("usage: tor_folder_check <ssh_config> <host> <remote> <local> ...") };
    assert!(!folders.is_empty() && folders.len() % 2 == 0, "each folder needs its copy here");

    tor::init(std::env::current_exe().unwrap(), vec![]);
    tor::set_on(true, vec![]).unwrap();
    let started = Instant::now();
    while tor::status() != TorStatus::On {
        assert!(started.elapsed() < Duration::from_secs(180), "Tor didn't start: {:?}", tor::status());
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("Tor is on after {} ms", started.elapsed().as_millis());

    let t = SshTarget { destination: host.clone(), extra_args: vec!["-F".into(), cfg.clone()], ..Default::default() };
    assert!(tor::carries(&t), "this server should go through Tor");
    let target = Target::Ssh(t);
    // With CHECK_SHARING=<folder>, ssh shares connections as the app does on a Mac or Linux.
    let env = std::env::var_os("CHECK_SHARING").map_or_else(SshEnv::default, |dir| SshEnv::with_control_dir(PathBuf::from(dir)));
    let dest = std::env::temp_dir().join(format!("sshee-folder-check-{}", std::process::id()));
    std::fs::create_dir_all(&dest).unwrap();

    for pair in folders.chunks(2) {
        let (remote, local) = (&pair[0], Path::new(&pair[1]));
        if local.is_file() {
            check_file(&target, &env, remote, local, &dest);
            continue;
        }
        let last: Arc<Mutex<(u64, u64)>> = Arc::default();
        let l = last.clone();
        let xfer = Transfer::new(move |p| *l.lock().unwrap() = (p.done, p.total));
        let begun = Instant::now();
        let got = files::download(&target, &env, remote, true, &dest, &xfer).expect("download");
        let secs = begun.elapsed().as_secs_f64();
        let (done, total) = *last.lock().unwrap();
        let size = tree_size(local);
        println!("{remote}: {:.1} s, {:.2} MB/s as files, bar at {done} of {total}, files {size} bytes", secs, size as f64 / secs / 1e6);
        let same = Command::new("diff").args(["-r", "--"]).arg(local).arg(&got).status().expect("diff");
        assert!(same.success(), "{remote} didn't arrive as it is");
        // tar's own stream is a little more than the files in it.
        assert!(done >= size && done <= size + size / 10 + 1_000_000, "the bar counts what tar got: {done} against {size}");
        std::fs::remove_dir_all(&got).unwrap();

        let xfer = cancelled_early();
        let err = files::download(&target, &env, remote, true, &dest, &xfer).expect_err("stopped");
        assert_eq!(err.code, ErrorCode::Cancelled);
        let left: Vec<PathBuf> = std::fs::read_dir(&dest).unwrap().flatten().map(|e| e.path()).collect();
        assert!(left.is_empty(), "Stop left {left:?} behind");
        println!("{remote}: Stop left nothing behind");

        // Beside the folder, under a folder of its own: its name stays the copy's.
        let beside = format!("{}-upload", remote.trim_end_matches('/'));
        files::make_dir(&target, &env, &beside).expect("mkdir");
        let last: Arc<Mutex<(u64, u64)>> = Arc::default();
        let l = last.clone();
        let xfer = Transfer::new(move |p| *l.lock().unwrap() = (p.done, p.total));
        xfer.add_total(files::upload_size(local));
        let begun = Instant::now();
        files::upload(&target, &env, local, &beside, false, false, &xfer).expect("upload");
        let secs = begun.elapsed().as_secs_f64();
        let (done, total) = *last.lock().unwrap();
        println!("{remote}: uploaded in {secs:.1} s, {:.2} MB/s as files, bar at {done} of {total}", size as f64 / secs / 1e6);
        assert_eq!((done, total), (size, size), "the bar ends at the folder's size");
        let name = local.file_name().unwrap().to_string_lossy();
        let back = files::download(&target, &env, &format!("{beside}/{name}"), true, &dest, &Transfer::quiet()).expect("fetching it back");
        let same = Command::new("diff").args(["-r", "--"]).arg(local).arg(&back).status().expect("diff");
        assert!(same.success(), "{remote} didn't upload as it is");
        std::fs::remove_dir_all(&back).unwrap();
        files::delete(&target, &env, &beside).expect("delete");
        println!("{remote}: uploaded whole");
    }
    std::fs::remove_dir_all(&dest).unwrap();
    tor::set_on(false, vec![]).unwrap();
    println!("ALL OK");
}

fn check_file(target: &Target, env: &SshEnv, remote: &str, local: &Path, dest: &Path) {
    let times: u32 = std::env::var("CHECK_TIMES").map_or(1, |n| n.parse().unwrap());
    let size = local.metadata().unwrap().len();
    for time in 0..times {
        tor::new_routes();
        let moves_before = moves_so_far();
        let begun = Instant::now();
        let got = files::download(target, env, remote, false, dest, &Transfer::quiet()).expect("download");
        let secs = begun.elapsed().as_secs_f64();
        println!("{remote} #{time}: {secs:.1} s, {:.2} MB/s, moved {} times", size as f64 / secs / 1e6, moves_so_far() - moves_before);
        let same = Command::new("cmp").arg("--").arg(local).arg(&got).status().expect("cmp");
        assert!(same.success(), "{remote} didn't arrive as it is");
        std::fs::remove_file(&got).unwrap();
    }
}

/// Moves off weak routes so far, as Tor's log has them.
fn moves_so_far() -> usize {
    let log = tor::log_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    log.lines().filter(|l| l.contains("moved off a weak route")).count()
}

fn tree_size(path: &Path) -> u64 {
    let meta = path.symlink_metadata().unwrap();
    if meta.is_dir() {
        std::fs::read_dir(path).unwrap().flatten().map(|e| tree_size(&e.path())).sum()
    } else {
        meta.len()
    }
}

/// A transfer that stops itself once the first bytes have moved.
fn cancelled_early() -> Arc<Transfer> {
    let slot: Arc<OnceLock<Weak<Transfer>>> = Arc::default();
    let s = slot.clone();
    let xfer = Transfer::new(move |p| {
        if p.done > 0 {
            if let Some(x) = s.get().and_then(Weak::upgrade) {
                x.cancel();
            }
        }
    });
    slot.set(Arc::downgrade(&xfer)).ok();
    xfer
}
