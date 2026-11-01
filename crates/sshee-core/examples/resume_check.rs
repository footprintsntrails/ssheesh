//! End-to-end check that transfers live through a dropped connection: they
//! wait, go on once the server is back (a file from where it stopped), and
//! everything arrives whole, a cleaned photo too. The drop is `<drop>`, a shell command run once
//! some of the data has moved, for example one that ends the test server's
//! connections.
//!
//! cargo run -p sshee-core --example resume_check -- <ssh_config> <host> <control_dir|none> <drop>

use sshee_core::files::{self, Progress, Transfer};
use sshee_core::{scrub, ssh, SshEnv, SshTarget, Target};
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
        r.lock().unwrap().push(p.clone());
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
    files::upload(&target, &env, &got, &format!("{base}/up"), false, false, &xfer).expect("upload");
    check_reports(&reports.lock().unwrap(), true);
    assert_eq!(sum(&format!("{base}/up/big.bin")), local_sum(&got), "the upload arrived whole");
    assert_eq!(run(&format!("ls -A {base}/up")), "big.bin", "nothing staged is left behind");

    println!("upload a photo, cleaned:");
    let photo = local.join("photo.jpg");
    write_photo(&photo, 96 * MIB as usize);
    let clean = scrub::clean_copy(&photo).expect("clean").1.expect("something to clean");
    let (xfer, reports) = dropping(drop);
    xfer_total(&xfer, &photo);
    run(&format!("mkdir -p {base}/photos"));
    files::upload(&target, &env, &photo, &format!("{base}/photos"), false, true, &xfer).expect("upload photo");
    let reports = reports.lock().unwrap();
    check_reports(&reports, true);
    assert_eq!(sum(&format!("{base}/photos/photo.jpg")), local_sum(&clean.path().to_path_buf()), "the cleaned copy arrived whole");
    let cleaning = reports.last().unwrap().cleaning.clone().expect("it says what it cleaned");
    assert_eq!(cleaning.removed, ["location", "camera"]);
    assert_eq!(run(&format!("grep -c Nikon {base}/photos/photo.jpg || true")), "0", "no camera on the server");
    std::mem::drop(reports);
    forget_photo(clean, &photo);

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

/// A JPEG-shaped file of about `size` bytes, with the camera and a GPS
/// position in its EXIF.
fn write_photo(path: &Path, size: usize) {
    // EXIF, big-endian: Make, and a GPS directory with a latitude reference.
    let mut tiff = b"MM\0*\0\0\0\x08\0\x02".to_vec();
    tiff.extend_from_slice(&[0x01, 0x0F, 0, 2, 0, 0, 0, 4, b'N', b'i', b'k', 0]);
    tiff.extend_from_slice(&[0x88, 0x25, 0, 4, 0, 0, 0, 1, 0, 0, 0, 38]);
    tiff.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 1, 0, 2, 0, 0, 0, 2, b'N', 0, 0, 0, 0, 0, 0, 0]);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    app1.extend_from_slice(b"Nikon D850 body serial 1234");
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
    jpeg.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    jpeg.extend_from_slice(&app1);
    jpeg.extend_from_slice(&[0xFF, 0xDA, 0, 8, 1, 1, 0, 0, 63, 0]);
    // The picture: noise without 0xFF, so nothing in it reads as a marker.
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    jpeg.extend((0..size).map(|_| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x as u8).min(0xFE)
    }));
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    std::fs::write(path, jpeg).unwrap();
}

/// Once sent, the cleaned copy is gone; the photo itself is as it was.
fn forget_photo(clean: scrub::CleanCopy, photo: &Path) {
    let at = clean.path().to_path_buf();
    std::mem::drop(clean);
    assert!(!at.exists());
    assert!(std::fs::read(photo).unwrap().windows(5).any(|w| w == b"Nikon"));
}

/// Sets an upload's total, as the app does before it starts.
fn xfer_total(xfer: &Transfer, path: &Path) {
    xfer.add_total(files::upload_size(path));
}
