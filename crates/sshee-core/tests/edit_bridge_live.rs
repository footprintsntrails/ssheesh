//! "Open in <editor>" against a real ssh server: a throwaway sshd on
//! 127.0.0.1 with keys of its own, started and stopped here.
//!
//! SSHEE_LIVE_SSHD=1 cargo test -p sshee-core --test edit_bridge_live -- --nocapture
//!
//! Skipped unless SSHEE_LIVE_SSHD is set. Everything lives in a short folder
//! under /tmp (also HOME for this test, so the ssh client reads none of your
//! files): the server's host key, an authorized_keys of its own (never your
//! account's), the client key, known_hosts and the "server's" files. Only the
//! sshd started here is stopped, with its connections.
//!
//! Checks: a file opened through ssh, an editor's save (write a new file,
//! rename it over the copy, as VS Code does) uploaded; a change made on the
//! server meanwhile stops the upload as a conflict; with the server down the
//! save waits, and goes once the server is back.

#![cfg(unix)]

use sshee_core::edit_bridge::{BridgeConfig, EditBridge, EditEvent, EditSink, OpenRequest, Opened, Server, Timing};
use sshee_core::{SshEnv, SshTarget, Target};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Sshd {
    dir: PathBuf,
    config: PathBuf,
    port: u16,
    child: Option<Child>,
}

impl Sshd {
    fn program() -> Option<PathBuf> {
        ["/usr/sbin/sshd", "/usr/bin/sshd", "/usr/local/sbin/sshd", "/opt/homebrew/sbin/sshd"].iter().map(PathBuf::from).find(|p| p.exists())
    }

    fn new(dir: &Path) -> Sshd {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        for key in ["host_ed25519", "client_ed25519"] {
            let ok = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-C", "edit-bridge-test", "-f"]).arg(dir.join(key)).status().unwrap();
            assert!(ok.success());
        }
        std::fs::copy(dir.join("client_ed25519.pub"), dir.join("authorized_keys")).unwrap();
        let config = dir.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host_ed25519\nAuthorizedKeysFile {d}/authorized_keys\n\
                 PubkeyAuthentication yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\n\
                 PermitRootLogin prohibit-password\nStrictModes no\nUsePAM no\nPidFile none\n",
                d = dir.display()
            ),
        )
        .unwrap();
        let mut sshd = Sshd { dir: dir.to_path_buf(), config, port, child: None };
        sshd.start();
        sshd
    }

    fn start(&mut self) {
        let child = Command::new(Self::program().unwrap()).arg("-D").arg("-e").arg("-f").arg(&self.config).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        self.child = Some(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(("127.0.0.1", self.port)).is_err() {
            assert!(Instant::now() < deadline, "sshd didn't start");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Stops this sshd and every connection it holds: what a server going
    /// away looks like to the client.
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = Command::new("pkill").arg("-P").arg(child.id().to_string()).status();
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn client_config(&self) -> PathBuf {
        let user = String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string();
        let path = self.dir.join("ssh_config");
        std::fs::write(
            &path,
            format!(
                "Host web-7731\n  HostName 127.0.0.1\n  Port {}\n  User {user}\n  IdentityFile {d}/client_ed25519\n  IdentitiesOnly yes\n\
                 UserKnownHostsFile {d}/known_hosts\n  StrictHostKeyChecking accept-new\n  BatchMode yes\n",
                self.port,
                d = self.dir.display()
            ),
        )
        .unwrap();
        path
    }
}

impl Drop for Sshd {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wait(rx: &Receiver<EditEvent>, what: &str, secs: u64, ok: impl Fn(&EditEvent) -> bool) -> EditEvent {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(e) if ok(&e) => {
                println!("  {what}: {e:?}");
                return e;
            }
            Ok(e) => seen.push(e),
            Err(_) => break,
        }
    }
    panic!("timed out waiting for {what}; saw {seen:?}");
}

/// An editor's save: a new file beside the copy, renamed over it.
fn editor_save(local: &Path, content: &[u8]) {
    let temp = local.with_file_name(".app.conf.vscode-save");
    std::fs::write(&temp, content).unwrap();
    std::fs::rename(&temp, local).unwrap();
}

#[test]
fn open_save_conflict_and_offline_against_a_real_sshd() {
    if std::env::var_os("SSHEE_LIVE_SSHD").is_none() {
        eprintln!("skipped: set SSHEE_LIVE_SSHD=1 to run against a throwaway sshd");
        return;
    }
    if Sshd::program().is_none() {
        eprintln!("skipped: no sshd on this computer");
        return;
    }
    let dir = PathBuf::from(format!("/tmp/sxe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The client reads nothing of yours (and the app's folder is this one).
    std::env::set_var("HOME", &dir);
    let mut sshd = Sshd::new(&dir);
    let server_files = dir.join("srv").join("shop");
    std::fs::create_dir_all(&server_files).unwrap();
    let remote_path = server_files.join("app.conf");
    std::fs::write(&remote_path, b"listen 80\n").unwrap();
    let remote_path = remote_path.to_string_lossy().into_owned();

    let ssh = SshTarget { destination: "web-7731".into(), extra_args: vec!["-F".into(), sshd.client_config().to_string_lossy().into_owned()], ..Default::default() };
    let server = Arc::new(Server { target: Target::Ssh(ssh), env: SshEnv::with_control_dir(dir.join("cm")) });

    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let sink: EditSink = Arc::new(move |_, e| {
        let _ = tx.lock().unwrap().send(e);
    });
    let mut config = BridgeConfig::in_dir(dir.join("app").join("edit"));
    config.timing = Timing { debounce: Duration::from_millis(150), poll: Duration::from_millis(500), backoff: |_| Duration::from_millis(500), ..Timing::default() };
    let bridge = EditBridge::new(config, sink).unwrap();

    // ---- open and upload an editor's save
    let request = || OpenRequest { host: "web-7731".into(), path: remote_path.clone(), remote: server.clone(), editor: None, confirmed: false };
    let Opened::Session(info) = bridge.open(request()).unwrap() else { panic!("asked to confirm a small file") };
    println!("opened {} as {}", remote_path, info.local.display());
    assert_eq!(std::fs::read(&info.local).unwrap(), b"listen 80\n");
    editor_save(&info.local, b"listen 8080\n");
    wait(&rx, "upload", 30, |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(std::fs::read(&remote_path).unwrap(), b"listen 8080\n");
    editor_save(&info.local, b"listen 8081\n");
    wait(&rx, "second upload", 30, |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(std::fs::read(&remote_path).unwrap(), b"listen 8081\n");

    // ---- a change on the server meanwhile: a conflict, theirs untouched
    std::fs::write(&remote_path, b"listen 9090 # changed on the server\n").unwrap();
    editor_save(&info.local, b"listen 8082\n");
    wait(&rx, "conflict", 30, |e| matches!(e, EditEvent::Conflict { now: Some(_) }));
    assert_eq!(std::fs::read(&remote_path).unwrap(), b"listen 9090 # changed on the server\n");
    let cmp = bridge.compare(&info.id).unwrap();
    assert!(cmp.theirs.contains("9090") && cmp.mine.contains("8082"));
    let copy = bridge.save_mine_as_copy(&info.id).unwrap();
    assert_eq!(std::fs::read(&copy).unwrap(), b"listen 8082\n");
    println!("  saved mine as {copy}");
    bridge.keep_mine(&info.id).unwrap();
    assert_eq!(std::fs::read(&remote_path).unwrap(), b"listen 8082\n");

    // ---- the server goes away: the save waits, and goes when it's back
    sshd.stop();
    editor_save(&info.local, b"listen 8083 # saved while offline\n");
    wait(&rx, "offline", 60, |e| matches!(e, EditEvent::Offline { pending: true, .. }));
    assert_eq!(std::fs::read(&remote_path).unwrap(), b"listen 8082\n");
    sshd.start();
    bridge.retry_host("web-7731");
    wait(&rx, "upload after reconnect", 60, |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(std::fs::read(&remote_path).unwrap(), b"listen 8083 # saved while offline\n");

    // ---- closing deletes the copy, all of it being on the server
    let closed = bridge.close(&info.id).unwrap();
    assert_eq!(closed.kept, None);
    assert!(!info.local.exists());
    drop(bridge);
    sshd.stop();
    let _ = std::fs::remove_dir_all(&dir);
    println!("ALL OK");
}
