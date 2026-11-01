//! End-to-end check of uploads and downloads against a real ssh server:
//! progress adds up to the size, and Cancel stops a transfer midway without
//! leaving half a file or folder behind on either side.
//!
//! cargo run -p sshee-core --example transfer_check -- <ssh_config> <host> <control_dir|none>
//!
//! With `none`, ssh runs without connection sharing, as it does on Windows.

use sshee_core::files::{self, Transfer};
use sshee_core::{ErrorCode, SshEnv, SshTarget, Target};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

const MIB: u64 = 1 << 20;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, cfg, host, control] = &args[..] else {
        panic!("usage: transfer_check <ssh_config> <host> <control_dir|none>");
    };
    let target = Target::Ssh(SshTarget {
        destination: host.clone(),
        extra_args: vec!["-F".into(), cfg.clone()],
        ..Default::default()
    });
    let env = if control == "none" { SshEnv::default() } else { SshEnv::with_control_dir(PathBuf::from(control)) };

    let base = format!("/tmp/sshee-xfer-{}", std::process::id());
    files::make_dir(&target, &env, &base).expect("mkdir");
    // Apart from `base`, in case the server is this same machine.
    let local = std::env::temp_dir().join(format!("sshee-xfer-local-{}", std::process::id()));
    let folder = local.join("site");
    std::fs::create_dir_all(&folder).unwrap();
    let big = local.join("big.bin");
    std::fs::write(&big, vec![7u8; (256 * MIB) as usize]).unwrap();
    for i in 0..4 {
        std::fs::write(folder.join(format!("part{i}.bin")), vec![i as u8; (64 * MIB) as usize]).unwrap();
    }
    std::fs::write(folder.join("index.html"), "hi").unwrap();

    // ---- progress adds up
    let (xfer, reports) = recorded();
    xfer.add_total(files::upload_size(&big));
    let t0 = Instant::now();
    files::upload(&target, &env, &big, &base, false, false, &xfer).expect("upload file");
    let last = *reports.lock().unwrap().last().unwrap();
    println!("upload file: {} reports in {:?}, last {last:?}", reports.lock().unwrap().len(), t0.elapsed());
    assert_eq!(last, (256 * MIB, 256 * MIB));

    let (xfer, reports) = recorded();
    xfer.add_total(files::upload_size(&folder));
    files::upload(&target, &env, &folder, &base, false, false, &xfer).expect("upload folder");
    let last = *reports.lock().unwrap().last().unwrap();
    println!("upload folder: {} reports, last {last:?}", reports.lock().unwrap().len());
    assert_eq!(last, (256 * MIB + 2, 256 * MIB + 2));
    let done: Vec<_> = reports.lock().unwrap().iter().map(|r| r.0).collect();
    assert!(done.windows(2).all(|w| w[0] <= w[1]), "progress never goes back");

    let downloads = local.join("downloads");
    let (xfer, reports) = recorded();
    let got = files::download(&target, &env, &format!("{base}/big.bin"), false, &downloads, &xfer).expect("download file");
    let last = *reports.lock().unwrap().last().unwrap();
    println!("download file: {got}, last {last:?}");
    assert_eq!(std::fs::metadata(&got).unwrap().len(), 256 * MIB);
    assert_eq!(last.1, 256 * MIB);

    let (xfer, reports) = recorded();
    let got = files::download(&target, &env, &format!("{base}/site"), true, &downloads, &xfer).expect("download folder");
    let last = *reports.lock().unwrap().last().unwrap();
    println!("download folder: {got}, last {last:?}");
    assert_eq!(std::fs::read_to_string(Path::new(&got).join("index.html")).unwrap(), "hi");
    assert!(last.1 >= 256 * MIB, "folder total comes from du");

    // ---- Cancel midway
    files::make_dir(&target, &env, &format!("{base}/c")).expect("mkdir c");
    let dir_c = format!("{base}/c");
    let xfer = cancelled_early();
    let err = files::upload(&target, &env, &big, &dir_c, false, false, &xfer).expect_err("cancelled file upload");
    assert_eq!(err.code, ErrorCode::Cancelled);
    let xfer = cancelled_early();
    let err = files::upload(&target, &env, &folder, &dir_c, false, false, &xfer).expect_err("cancelled folder upload");
    assert_eq!(err.code, ErrorCode::Cancelled);
    // The server side tidies up on its own within a moment.
    std::thread::sleep(Duration::from_secs(3));
    let left = files::list(&target, &env, &dir_c).expect("list c");
    let names: Vec<_> = left.entries.iter().map(|e| e.name.clone()).collect();
    println!("cancel upload: {dir_c} has {names:?}");
    assert!(names.is_empty(), "nothing left behind on the server");

    let xfer = cancelled_early();
    let dl = local.join("cancelled");
    let err = files::download(&target, &env, &format!("{base}/big.bin"), false, &dl, &xfer).expect_err("cancelled download");
    assert_eq!(err.code, ErrorCode::Cancelled);
    let xfer = cancelled_early();
    let err = files::download(&target, &env, &format!("{base}/site"), true, &dl, &xfer).expect_err("cancelled folder download");
    assert_eq!(err.code, ErrorCode::Cancelled);
    let left: Vec<_> = std::fs::read_dir(&dl).unwrap().flatten().map(|e| e.file_name()).collect();
    println!("cancel download: {} has {left:?}", dl.display());
    assert!(left.is_empty(), "nothing left behind on this computer");

    // Replacing still works, and a replaced file is whole.
    let xfer = Transfer::quiet();
    files::upload(&target, &env, &big, &base, true, false, &xfer).expect("replace file");
    let listing = files::list(&target, &env, &base).expect("list base");
    let size = listing.entries.iter().find(|e| e.name == "big.bin").map(|e| e.size);
    assert_eq!(size, Some(256 * MIB));
    assert!(!listing.entries.iter().any(|e| e.name.ends_with(".sshee-part")));

    files::delete(&target, &env, &base).expect("delete");
    std::fs::remove_dir_all(&local).ok();
    println!("ALL OK");
}

type Reports = Arc<Mutex<Vec<(u64, u64)>>>;

/// A transfer that keeps every progress report.
fn recorded() -> (Arc<Transfer>, Reports) {
    let reports: Reports = Arc::default();
    let r = reports.clone();
    (Transfer::new(move |p| r.lock().unwrap().push((p.done, p.total))), reports)
}

/// A transfer that cancels itself once the first bytes have moved.
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
