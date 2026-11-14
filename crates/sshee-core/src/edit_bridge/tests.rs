//! The bridge against a pretend server that can go offline, change its file
//! behind the bridge's back and count saves, with an edit folder of its own
//! in the system's temp folder.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;

const GB: u64 = 1024 * MB;

// ---------------------------------------------------------------- a pretend server

#[derive(Default)]
struct FakeServer {
    files: Mutex<HashMap<String, (Vec<u8>, i64)>>,
    offline: AtomicBool,
    denied: AtomicBool,
    saves: AtomicUsize,
    clock: AtomicUsize,
}

fn stamp_of(bytes: &[u8], modified: i64) -> Stamp {
    Stamp { size: bytes.len() as u64, modified, sha256: Some(sha256_hex(bytes)) }
}

impl FakeServer {
    fn with(path: &str, content: &[u8]) -> Arc<FakeServer> {
        let server = FakeServer::default();
        server.put(path, content);
        Arc::new(server)
    }

    /// Someone changes the file on the server.
    fn put(&self, path: &str, content: &[u8]) {
        let t = 1_700_000_000 + self.clock.fetch_add(1, Ordering::SeqCst) as i64;
        lock(&self.files).insert(path.into(), (content.to_vec(), t));
    }

    fn get(&self, path: &str) -> Option<Vec<u8>> {
        lock(&self.files).get(path).map(|(b, _)| b.clone())
    }

    fn down(&self) -> Result<(), OpError> {
        if self.offline.load(Ordering::SeqCst) {
            return Err(OpError::new(ErrorCode::Network, "Can't reach the server: connection timed out"));
        }
        Ok(())
    }
}

impl Remote for FakeServer {
    fn size(&self, path: &str) -> Result<u64, OpError> {
        self.down()?;
        lock(&self.files).get(path).map(|(b, _)| b.len() as u64).ok_or_else(|| OpError::new(ErrorCode::NotFound, "No such file"))
    }

    fn read(&self, path: &str, limit: u64) -> Result<(Vec<u8>, Stamp), OpError> {
        self.down()?;
        let files = lock(&self.files);
        let (bytes, t) = files.get(path).ok_or_else(|| OpError::new(ErrorCode::NotFound, "No such file"))?;
        if bytes.len() as u64 > limit {
            return Err(OpError::new(ErrorCode::TooLarge, "too large"));
        }
        Ok((bytes.clone(), stamp_of(bytes, *t)))
    }

    fn save(&self, path: &str, bytes: &[u8], expected: Option<&Stamp>) -> Result<Stamp, SaveError> {
        self.down()?;
        if self.denied.load(Ordering::SeqCst) {
            return Err(SaveError::Failed(OpError::failed("Permission denied")));
        }
        let mut files = lock(&self.files);
        let now = files.get(path).map(|(b, t)| stamp_of(b, *t));
        let ok = match (expected, &now) {
            (Some(e), Some(n)) => e.matches(n),
            (None, None) => true,
            _ => false,
        };
        if !ok {
            return Err(SaveError::Conflict(files::Conflict { path: path.into(), now }));
        }
        self.saves.fetch_add(1, Ordering::SeqCst);
        let t = 1_700_000_000 + self.clock.fetch_add(1, Ordering::SeqCst) as i64;
        files.insert(path.into(), (bytes.to_vec(), t));
        Ok(stamp_of(bytes, t))
    }
}

// ---------------------------------------------------------------- a bridge in a temp folder

/// A folder of its own in the temp folder, gone at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Scratch {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("sshee-edit-{what}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Rig {
    bridge: EditBridge,
    events: Receiver<(String, EditEvent)>,
    scratch: Scratch,
}

fn plenty(_: &Path) -> io::Result<u64> {
    Ok(500 * GB)
}

fn quick() -> Timing {
    Timing {
        debounce: Duration::from_millis(60),
        poll: Duration::from_millis(250),
        idle_close: Duration::from_secs(3600),
        leftover_age: Duration::from_secs(30 * 24 * 3600),
        backoff: |_| Duration::from_millis(150),
    }
}

fn rig_with(what: &str, change: impl FnOnce(&mut BridgeConfig)) -> Rig {
    let scratch = Scratch::new(what);
    let mut config = BridgeConfig::in_dir(scratch.0.join("app").join("edit"));
    config.timing = quick();
    config.free_space = plenty;
    change(&mut config);
    let (tx, events) = channel();
    let tx = Mutex::new(tx);
    let sink: EditSink = Arc::new(move |id, e| {
        let _ = lock(&tx).send((id.to_string(), e));
    });
    Rig { bridge: EditBridge::new(config, sink).unwrap(), events, scratch }
}

fn rig(what: &str) -> Rig {
    rig_with(what, |_| {})
}

const PATH: &str = "/home/deploy/shop/app.conf";

fn request(server: &Arc<FakeServer>, path: &str) -> OpenRequest {
    OpenRequest { host: "web-7731".into(), path: path.into(), remote: server.clone(), editor: None, confirmed: false }
}

impl Rig {
    fn open(&self, server: &Arc<FakeServer>) -> SessionInfo {
        match self.bridge.open(request(server, PATH)).unwrap() {
            Opened::Session(info) => info,
            other => panic!("expected a session, got {other:?}"),
        }
    }

    /// Waits for an event `ok` accepts, failing after a few seconds.
    fn wait(&self, what: &str, ok: impl Fn(&EditEvent) -> bool) -> EditEvent {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut seen = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.events.recv_timeout(left) {
                Ok((_, e)) if ok(&e) => return e,
                Ok((_, e)) => seen.push(e),
                Err(_) => break,
            }
        }
        panic!("timed out waiting for {what}; saw {seen:?}");
    }

    /// Fails if an upload starts within `ms`.
    fn no_upload_for(&self, ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if let Ok((_, e)) = self.events.recv_timeout(left) {
                assert!(!matches!(e, EditEvent::Uploading | EditEvent::Uploaded { .. }), "unexpected {e:?}");
            }
        }
    }

    fn session_dirs(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.bridge.root()).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

/// An editor's save the way VS Code and many others do it: a new file
/// beside the old one, renamed over it.
fn editor_save(local: &Path, content: &[u8]) {
    let temp = local.with_file_name(format!(".{}.tmp-save", local.file_name().unwrap().to_string_lossy()));
    std::fs::write(&temp, content).unwrap();
    std::fs::rename(&temp, local).unwrap();
}

// ---------------------------------------------------------------- names

#[test]
fn hostile_server_names_become_one_plain_visible_name() {
    let cases = [
        ("/etc/nginx/nginx.conf", "nginx.conf"),
        ("/srv/../../etc/passwd", "passwd"),
        ("~/shop/.env", "_.env"),
        ("/home/deploy/.bashrc", "_.bashrc"),
        ("/tmp/-rf", "_-rf"),
        ("/tmp/..", "file"),
        ("/tmp/...", "file"),
        ("/tmp/", "tmp"),
        ("", "file"),
        ("/tmp/a\\..\\..\\AppData\\Startup\\evil.bat", "a_.._.._AppData_Startup_evil.bat"),
        ("/tmp/CON", "_CON"),
        ("/tmp/nul.txt", "_nul.txt"),
        ("/tmp/com1.log", "_com1.log"),
        ("/tmp/notes. ", "notes"),
        ("/tmp/x & calc.exe", "x & calc.exe"),
        ("/tmp/line\nbreak.txt", "line_break.txt"),
        ("/tmp/evil\u{202e}txt.exe", "evil_txt.exe"),
        ("/tmp/c:stream.txt", "c_stream.txt"),
    ];
    for (path, want) in cases {
        assert_eq!(local_file_name(path), want, "{path:?}");
    }
    let long = format!("/var/log/{}.conf", "a".repeat(300));
    let name = local_file_name(&long);
    assert_eq!(name.chars().count(), 100);
    assert!(name.ends_with(".conf"), "the extension is kept: {name}");
    for (path, _) in cases {
        let name = local_file_name(path);
        assert!(!name.starts_with('.') && !name.contains(['/', '\\']) && name != MARKER && name != LOCK);
    }
}

#[test]
fn sizes_decide_how_a_file_opens() {
    let l = Limits::default();
    assert_eq!(l.rule(10 * 1024), SizeRule::BuiltIn);
    assert_eq!(l.rule(BUILT_IN_MAX), SizeRule::Open);
    assert_eq!(l.rule(50 * MB), SizeRule::Open);
    assert_eq!(l.rule(50 * MB + 1), SizeRule::Ask);
    assert_eq!(l.rule(GB), SizeRule::Ask);
    assert_eq!(l.rule(GB + 1), SizeRule::TooLarge);
}

#[test]
fn copy_names_say_whose_and_when() {
    // 2026-11-14 12:41 UTC.
    assert_eq!(copy_name("app.conf", 1_794_660_060, 1), "app.conf.mine-20261114-1241");
    assert_eq!(copy_name("app.conf", 1_794_660_060, 2), "app.conf.mine-20261114-1241-2");
    assert_eq!(copy_name("x", 951_782_400, 1), "x.mine-20000229-0000");
}

// ---------------------------------------------------------------- opening and uploading

#[test]
fn a_save_in_the_editor_goes_to_the_server_and_the_next_one_too() {
    let rig = rig("save");
    let server = FakeServer::with(PATH, b"listen 80\n");
    let info = rig.open(&server);
    assert_eq!(info.local.file_name().unwrap(), "app.conf");
    assert_eq!(std::fs::read(&info.local).unwrap(), b"listen 80\n");

    editor_save(&info.local, b"listen 8080\n");
    rig.wait("the first upload", |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(server.get(PATH).unwrap(), b"listen 8080\n");

    // The stamp moved on with the upload, so the next save isn't a conflict.
    std::fs::write(&info.local, b"listen 8081\n").unwrap();
    rig.wait("the second upload", |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(server.get(PATH).unwrap(), b"listen 8081\n");
    assert_eq!(server.saves.load(Ordering::SeqCst), 2);
    assert!(!rig.bridge.status(&info.id).unwrap().pending);
}

#[test]
fn a_burst_of_writes_is_one_upload_and_a_save_without_change_is_none() {
    let rig = rig("burst");
    let server = FakeServer::with(PATH, b"a\n");
    let info = rig.open(&server);
    for i in 0..5 {
        std::fs::write(&info.local, format!("a\n{i}\n")).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    rig.wait("the upload", |e| matches!(e, EditEvent::Uploaded { .. }));
    rig.no_upload_for(500);
    assert_eq!(server.saves.load(Ordering::SeqCst), 1);
    assert_eq!(server.get(PATH).unwrap(), b"a\n4\n");
    // Touched, same bytes: nothing to send.
    std::fs::write(&info.local, b"a\n4\n").unwrap();
    rig.no_upload_for(600);
    assert_eq!(server.saves.load(Ordering::SeqCst), 1);
}

#[test]
fn a_copy_emptied_and_then_written_never_sends_the_empty_one() {
    // A plain write empties the file first; an editor on a busy machine may pause between
    // the two for longer than the quiet wait. The server must only ever see the text.
    let rig = rig_with("emptied", |c| c.timing.debounce = Duration::from_millis(300));
    let server = FakeServer::with(PATH, b"listen 80;\n");
    let info = rig.open(&server);
    std::fs::write(&info.local, b"").unwrap();
    std::thread::sleep(Duration::from_millis(420));
    std::fs::write(&info.local, b"listen 8080;\n").unwrap();
    rig.wait("the upload", |e| matches!(e, EditEvent::Uploaded { .. }));
    rig.no_upload_for(900);
    assert_eq!(server.get(PATH).unwrap(), b"listen 8080;\n");
    assert_eq!(server.saves.load(Ordering::SeqCst), 1, "the empty copy went up first");
}

#[test]
fn other_files_in_the_folder_and_files_elsewhere_are_never_uploaded() {
    let rig = rig("others");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    let dir = info.local.parent().unwrap();
    // vim's swap file, a backup, an editor's half-written temp file.
    std::fs::write(dir.join(".app.conf.swp"), b"swap").unwrap();
    std::fs::write(dir.join("app.conf~"), b"backup").unwrap();
    std::fs::write(dir.join("4913"), b"").unwrap();
    // "Save as" somewhere else.
    std::fs::write(rig.scratch.0.join("app.conf"), b"saved elsewhere").unwrap();
    rig.no_upload_for(700);
    assert_eq!(server.saves.load(Ordering::SeqCst), 0);
    assert_eq!(server.get(PATH).unwrap(), b"x\n");
    assert_eq!(lock(&server.files).len(), 1, "nothing else appeared on the server");
}

#[cfg(unix)]
#[test]
fn only_you_can_read_the_copies() {
    use std::os::unix::fs::PermissionsExt;
    let rig = rig("modes");
    let server = FakeServer::with(PATH, b"secret=1\n");
    let info = rig.open(&server);
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(rig.bridge.root()), 0o700);
    assert_eq!(mode(info.local.parent().unwrap()), 0o700);
    assert_eq!(mode(&info.local), 0o600);
    assert_eq!(mode(&info.local.parent().unwrap().join(MARKER)), 0o600);
}

#[cfg(unix)]
#[test]
fn an_edit_folder_others_could_open_is_closed_to_them_and_a_link_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("root");
    let root = scratch.0.join("edit");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    let sink: EditSink = Arc::new(|_, _| {});
    let _bridge = EditBridge::new(BridgeConfig::in_dir(root.clone()), sink.clone()).unwrap();
    assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);

    let elsewhere = scratch.0.join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let linked = scratch.0.join("linked-edit");
    std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
    assert!(EditBridge::new(BridgeConfig::in_dir(linked), sink).is_err());
}

// ---------------------------------------------------------------- links and swaps

#[cfg(unix)]
#[test]
fn a_copy_replaced_with_a_link_is_never_uploaded() {
    let rig = rig("link");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    let secret = rig.scratch.0.join("id_ed25519");
    std::fs::write(&secret, b"-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();
    std::fs::remove_file(&info.local).unwrap();
    std::os::unix::fs::symlink(&secret, &info.local).unwrap();
    rig.wait("the refusal", |e| matches!(e, EditEvent::Failed { message } if message.contains("link")));
    assert_eq!(server.get(PATH).unwrap(), b"x\n");
    assert_eq!(server.saves.load(Ordering::SeqCst), 0);

    // Closing keeps it (it's not known to be on the server) and deletes nothing it points at.
    let closed = rig.bridge.close(&info.id).unwrap();
    assert!(closed.kept.is_some());
    rig.bridge.discard_leftover(&info.id).unwrap();
    assert!(secret.exists(), "the link's target is untouched");
    assert!(rig.session_dirs().is_empty());
}

#[cfg(unix)]
#[test]
fn a_copy_hard_linked_to_another_file_is_never_uploaded() {
    let rig = rig("hardlink");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    let secret = rig.scratch.0.join("token");
    std::fs::write(&secret, b"hunter2\n").unwrap();
    std::fs::remove_file(&info.local).unwrap();
    std::fs::hard_link(&secret, &info.local).unwrap();
    rig.wait("the refusal", |e| matches!(e, EditEvent::Failed { message } if message.contains("linked")));
    assert_eq!(server.saves.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn deleting_a_folder_never_follows_a_link_out_of_it() {
    let rig = rig("nofollow");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    let outside = rig.scratch.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
    let dir = info.local.parent().unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("to-outside")).unwrap();
    std::os::unix::fs::symlink(outside.join("keep.txt"), dir.join("to-file")).unwrap();
    std::fs::create_dir(dir.join("backup")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("backup").join("deeper")).unwrap();

    let closed = rig.bridge.close(&info.id).unwrap();
    assert_eq!(closed.kept, None);
    assert!(rig.session_dirs().is_empty());
    assert_eq!(std::fs::read(outside.join("keep.txt")).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn a_session_folder_swapped_for_a_link_is_not_deleted_through() {
    let rig = rig("swap");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    let dir = info.local.parent().unwrap().to_path_buf();
    let outside = rig.scratch.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("app.conf"), b"x\n").unwrap();
    std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
    // Swap the session folder for a link to another folder.
    let moved = rig.scratch.0.join("moved");
    std::fs::rename(&dir, &moved).unwrap();
    std::os::unix::fs::symlink(&outside, &dir).unwrap();

    let _ = rig.bridge.discard(&info.id);
    assert_eq!(std::fs::read(outside.join("keep.txt")).unwrap(), b"keep");
    assert_eq!(std::fs::read(outside.join("app.conf")).unwrap(), b"x\n");
    // At most the link itself went.
    assert!(std::fs::symlink_metadata(&dir).map(|m| m.file_type().is_symlink()).unwrap_or(true));
}

#[cfg(unix)]
#[test]
fn an_edit_folder_swapped_under_the_bridge_is_left_alone() {
    let rig = rig("rootswap");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    let root = rig.bridge.root().to_path_buf();
    // Another folder, with a session folder of the same name, takes the edit folder's place.
    let decoy = rig.scratch.0.join("decoy");
    std::fs::create_dir_all(decoy.join(&info.id)).unwrap();
    std::fs::write(decoy.join(&info.id).join("app.conf"), b"x\n").unwrap();
    std::fs::rename(&root, rig.scratch.0.join("real-edit")).unwrap();
    std::fs::rename(&decoy, &root).unwrap();

    let _ = rig.bridge.discard(&info.id);
    assert!(root.join(&info.id).join("app.conf").exists(), "nothing deleted in the folder that was swapped in");
    assert!(rig.bridge.sweep().is_empty());
    assert!(root.join(&info.id).join("app.conf").exists());
}

// ---------------------------------------------------------------- conflicts

#[test]
fn a_file_changed_on_the_server_stops_uploads_and_asks() {
    let rig = rig("conflict");
    let server = FakeServer::with(PATH, b"one\ntwo\n");
    let info = rig.open(&server);
    server.put(PATH, b"one\ntwo\nthree (theirs)\n");
    std::fs::write(&info.local, b"one\nTWO (mine)\n").unwrap();
    rig.wait("the conflict", |e| matches!(e, EditEvent::Conflict { now: Some(_) }));
    assert_eq!(server.get(PATH).unwrap(), b"one\ntwo\nthree (theirs)\n", "theirs isn't written over");

    // More saves wait for the choice.
    std::fs::write(&info.local, b"one\nTWO (mine, again)\n").unwrap();
    rig.no_upload_for(600);
    let status = rig.bridge.status(&info.id).unwrap();
    assert!(status.conflict && status.pending);

    let cmp = rig.bridge.compare(&info.id).unwrap();
    assert!(!cmp.binary);
    assert!(cmp.lines.contains(&DiffLine { kind: DiffKind::Mine, text: "TWO (mine, again)".into() }));
    assert!(cmp.lines.contains(&DiffLine { kind: DiffKind::Theirs, text: "three (theirs)".into() }));
    assert_eq!(cmp.lines[0], DiffLine { kind: DiffKind::Same, text: "one".into() });

    let copy = rig.bridge.save_mine_as_copy(&info.id).unwrap();
    assert!(copy.starts_with("/home/deploy/shop/app.conf.mine-"), "{copy}");
    assert_eq!(server.get(&copy).unwrap(), b"one\nTWO (mine, again)\n");
    let second = rig.bridge.save_mine_as_copy(&info.id).unwrap();
    assert_ne!(copy, second, "a second copy never writes over the first");

    rig.bridge.keep_mine(&info.id).unwrap();
    assert_eq!(server.get(PATH).unwrap(), b"one\nTWO (mine, again)\n");
    let status = rig.bridge.status(&info.id).unwrap();
    assert!(!status.conflict && !status.pending);
    // And uploads go on as before.
    std::fs::write(&info.local, b"after\n").unwrap();
    rig.wait("an upload after keeping mine", |e| matches!(e, EditEvent::Uploading));
    rig.wait("its end", |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(server.get(PATH).unwrap(), b"after\n");
}

#[test]
fn take_theirs_reloads_the_copy_and_uploads_nothing() {
    let rig = rig("theirs");
    let server = FakeServer::with(PATH, b"old\n");
    let info = rig.open(&server);
    server.put(PATH, b"theirs\n");
    std::fs::write(&info.local, b"mine\n").unwrap();
    rig.wait("the conflict", |e| matches!(e, EditEvent::Conflict { .. }));
    rig.bridge.take_theirs(&info.id).unwrap();
    assert_eq!(std::fs::read(&info.local).unwrap(), b"theirs\n");
    rig.no_upload_for(600);
    assert_eq!(server.saves.load(Ordering::SeqCst), 0);
    let closed = rig.bridge.close(&info.id).unwrap();
    assert_eq!(closed.kept, None, "the copy is the server's, so it goes");
}

#[test]
fn a_file_deleted_on_the_server_is_a_conflict_and_keep_mine_puts_it_back() {
    let rig = rig("deleted");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    lock(&server.files).remove(PATH);
    std::fs::write(&info.local, b"y\n").unwrap();
    rig.wait("the conflict", |e| matches!(e, EditEvent::Conflict { now: None }));
    rig.bridge.keep_mine(&info.id).unwrap();
    assert_eq!(server.get(PATH).unwrap(), b"y\n");
}

// ---------------------------------------------------------------- offline

#[test]
fn saves_wait_while_the_server_is_down_and_the_latest_goes_when_it_is_back() {
    let rig = rig("offline");
    let server = FakeServer::with(PATH, b"v0\n");
    let info = rig.open(&server);
    server.offline.store(true, Ordering::SeqCst);
    std::fs::write(&info.local, b"v1\n").unwrap();
    rig.wait("offline", |e| matches!(e, EditEvent::Offline { pending: true, .. }));
    std::fs::write(&info.local, b"v2\n").unwrap();
    rig.wait("still offline", |e| matches!(e, EditEvent::Offline { .. }));
    assert!(rig.bridge.status(&info.id).unwrap().pending);

    server.offline.store(false, Ordering::SeqCst);
    rig.bridge.retry_host("web-7731");
    rig.wait("the upload once back", |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(server.get(PATH).unwrap(), b"v2\n");
    assert_eq!(server.saves.load(Ordering::SeqCst), 1);
}

#[test]
fn a_refused_save_waits_for_the_next_save_or_a_retry() {
    let rig = rig("denied");
    let server = FakeServer::with(PATH, b"v0\n");
    let info = rig.open(&server);
    server.denied.store(true, Ordering::SeqCst);
    std::fs::write(&info.local, b"v1\n").unwrap();
    rig.wait("the refusal", |e| matches!(e, EditEvent::Failed { message } if message.contains("Permission denied")));
    rig.no_upload_for(500);
    server.denied.store(false, Ordering::SeqCst);
    rig.bridge.retry_now(&info.id);
    rig.wait("the upload on retry", |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(server.get(PATH).unwrap(), b"v1\n");
}

#[test]
fn closing_while_offline_keeps_the_copy_and_the_next_start_offers_it() {
    let rig = rig("leftover");
    let server = FakeServer::with(PATH, b"v0\n");
    let info = rig.open(&server);
    server.offline.store(true, Ordering::SeqCst);
    std::fs::write(&info.local, b"unsent\n").unwrap();
    rig.wait("offline", |e| matches!(e, EditEvent::Offline { .. }));
    let closed = rig.bridge.close(&info.id).unwrap();
    assert_eq!(closed.kept.as_deref(), Some(info.local.as_path()));
    assert_eq!(std::fs::read(&info.local).unwrap(), b"unsent\n");

    // The next start: the same folder, a new bridge.
    let root = rig.bridge.root().to_path_buf();
    drop(rig.bridge);
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let mut config = BridgeConfig::in_dir(root);
    config.timing = quick();
    config.free_space = plenty;
    let bridge = EditBridge::new(config, Arc::new(move |id, e| drop(lock(&tx).send((id.to_string(), e))))).unwrap();
    let rig = Rig { bridge, events: rx, scratch: rig.scratch };
    let leftovers = rig.bridge.sweep();
    assert_eq!(leftovers.len(), 1);
    assert_eq!(leftovers[0].remote_path, PATH);
    assert_eq!(leftovers[0].host, "web-7731");
    assert!(!leftovers[0].offered_before);

    server.offline.store(false, Ordering::SeqCst);
    rig.bridge.resume_leftover(&leftovers[0].id, server.clone(), None).unwrap();
    rig.wait("the leftover's upload", |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(server.get(PATH).unwrap(), b"unsent\n");
    assert_eq!(rig.bridge.close(&leftovers[0].id).unwrap().kept, None);
    assert!(rig.session_dirs().is_empty());
}

// ---------------------------------------------------------------- sizes and space

#[test]
fn big_files_ask_first_huge_ones_are_refused() {
    let rig = rig_with("sizes", |c| c.limits = Limits { ask_above: 100, max: 1000, keep_free: 0, folder_cap: 1 << 40 });
    let server = FakeServer::with(PATH, &[b'x'; 500]);
    match rig.bridge.open(request(&server, PATH)).unwrap() {
        Opened::NeedsConfirm { size } => assert_eq!(size, 500),
        other => panic!("{other:?}"),
    }
    assert!(rig.session_dirs().is_empty(), "nothing downloaded before you say yes");
    let mut yes = request(&server, PATH);
    yes.confirmed = true;
    assert!(matches!(rig.bridge.open(yes).unwrap(), Opened::Session(_)));

    server.put("/srv/huge.img", &[0; 1001]);
    let err = rig.bridge.open(request(&server, "/srv/huge.img")).unwrap_err();
    assert_eq!(err.code, ErrorCode::TooLarge);
    assert!(err.message.contains("download it instead"), "{}", err.message);
}

#[test]
fn a_copy_that_would_leave_the_disk_nearly_full_is_refused() {
    fn tight(_: &Path) -> io::Result<u64> {
        Ok(1024 * MB + 50)
    }
    let rig = rig_with("space", |c| c.free_space = tight);
    let server = FakeServer::with(PATH, &[b'x'; 100]);
    let err = rig.bridge.open(request(&server, PATH)).unwrap_err();
    assert_eq!(err.code, ErrorCode::TooLarge);
    assert!(err.message.contains("free space"), "{}", err.message);
    assert!(rig.session_dirs().is_empty());
    server.put(PATH, &[b'x'; 40]);
    assert!(matches!(rig.bridge.open(request(&server, PATH)).unwrap(), Opened::Session(_)));
}

#[test]
fn a_failed_download_leaves_no_folder() {
    let rig = rig("failed-open");
    let server = FakeServer::with(PATH, b"x\n");
    assert!(rig.bridge.open(request(&server, "/home/deploy/missing.conf")).is_err());
    assert!(rig.session_dirs().is_empty());
}

#[test]
fn the_real_disk_has_some_free_space() {
    assert!(folder::free_space(&std::env::temp_dir()).unwrap() > 0);
}

// ---------------------------------------------------------------- lifetime

#[test]
fn closing_deletes_an_uploaded_copy() {
    let rig = rig("close");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    std::fs::write(&info.local, b"y\n").unwrap();
    rig.wait("the upload", |e| matches!(e, EditEvent::Uploaded { .. }));
    let closed = rig.bridge.close(&info.id).unwrap();
    assert_eq!(closed.kept, None);
    rig.wait("closed", |e| matches!(e, EditEvent::Closed { kept: None }));
    assert!(rig.session_dirs().is_empty());
    assert!(rig.bridge.status(&info.id).is_none());
}

#[test]
fn closing_uploads_a_save_the_watcher_had_not_sent_yet() {
    let rig = rig_with("close-flush", |c| c.timing.debounce = Duration::from_secs(30));
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    std::fs::write(&info.local, b"last-second\n").unwrap();
    let closed = rig.bridge.close(&info.id).unwrap();
    assert_eq!(closed.kept, None);
    assert_eq!(server.get(PATH).unwrap(), b"last-second\n");
}

#[test]
fn closing_a_server_or_quitting_ends_its_sessions() {
    let rig = rig("close-host");
    let server = FakeServer::with(PATH, b"x\n");
    server.put("/etc/hosts", b"127.0.0.1 localhost\n");
    let a = rig.open(&server);
    let mut other = request(&server, "/etc/hosts");
    other.host = "db-12".into();
    let Opened::Session(b) = rig.bridge.open(other).unwrap() else { panic!() };
    let closed = rig.bridge.close_host("web-7731");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].id, a.id);
    assert_eq!(rig.bridge.sessions().len(), 1);
    let all = rig.bridge.close_all();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, b.id);
    assert!(rig.session_dirs().is_empty());
}

#[test]
fn opening_an_open_file_again_reuses_its_session() {
    let rig = rig("reopen");
    let server = FakeServer::with(PATH, b"x\n");
    let a = rig.open(&server);
    let b = rig.open(&server);
    assert_eq!(a.id, b.id);
    assert_eq!(rig.session_dirs().len(), 1);
}

#[test]
fn a_session_idle_for_a_day_closes_on_its_own() {
    let rig = rig_with("idle", |c| c.timing.idle_close = Duration::from_millis(400));
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    rig.wait("closed for idling", |e| matches!(e, EditEvent::Closed { kept: None }));
    assert!(rig.bridge.status(&info.id).is_none());
    assert!(rig.session_dirs().is_empty());
}

#[test]
fn a_session_with_unsent_changes_never_closes_for_idling() {
    let rig = rig_with("idle-pending", |c| c.timing.idle_close = Duration::from_millis(300));
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    server.offline.store(true, Ordering::SeqCst);
    std::fs::write(&info.local, b"unsent\n").unwrap();
    rig.wait("offline", |e| matches!(e, EditEvent::Offline { .. }));
    std::thread::sleep(Duration::from_millis(900));
    assert!(rig.bridge.status(&info.id).is_some());
    assert_eq!(std::fs::read(&info.local).unwrap(), b"unsent\n");
}

// ---------------------------------------------------------------- the start-up sweep

/// A finished session's folder as an earlier run left it.
fn plant(root: &Path, content: &[u8], uploaded: &[u8], offered: Option<u64>, age_days: u64) -> String {
    let id = new_id();
    let dir = root.join(&id);
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("app.conf"), content).unwrap();
    let marker = Marker { host: "web-7731".into(), remote: PATH.into(), name: "app.conf".into(), base: stamp_of(uploaded, 1), created: 1, offered };
    std::fs::write(dir.join(MARKER), serde_json::to_vec(&marker).unwrap()).unwrap();
    let when = SystemTime::now() - Duration::from_secs(age_days * 86400);
    File::options().write(true).open(dir.join("app.conf")).unwrap().set_modified(when).unwrap();
    id
}

#[test]
fn the_sweep_deletes_uploaded_copies_and_offers_unsent_ones() {
    let rig = rig("sweep");
    let root = rig.bridge.root().to_path_buf();
    let clean = plant(&root, b"same\n", b"same\n", None, 1);
    let unsent = plant(&root, b"mine\n", b"theirs\n", None, 1);
    let old_unoffered = plant(&root, b"mine\n", b"theirs\n", None, 40);
    let old_offered = plant(&root, b"mine\n", b"theirs\n", Some(1_000), 40);
    let young_offered = plant(&root, b"mine\n", b"theirs\n", Some(1_000), 3);
    // Not SSHeesh's: left alone.
    std::fs::create_dir(root.join("not-a-session")).unwrap();
    std::fs::write(root.join("notes.txt"), b"mine").unwrap();
    // Half made by a crash: nothing to upload.
    let half = new_id();
    std::fs::create_dir(root.join(&half)).unwrap();
    std::fs::write(root.join(&half).join(LOCK), b"").unwrap();

    let found = rig.bridge.sweep();
    let mut ids: Vec<&str> = found.iter().map(|l| l.id.as_str()).collect();
    ids.sort();
    let mut want = vec![unsent.as_str(), old_unoffered.as_str(), young_offered.as_str()];
    want.sort();
    assert_eq!(ids, want);
    let dirs = rig.session_dirs();
    assert!(!dirs.contains(&clean) && !dirs.contains(&old_offered) && !dirs.contains(&half));
    assert!(dirs.contains(&"not-a-session".to_string()) && dirs.contains(&"notes.txt".to_string()));
    assert!(found.iter().find(|l| l.id == young_offered).unwrap().offered_before);

    // Offered now: a later start deletes the old one, not the young one.
    rig.bridge.mark_offered(&found.iter().map(|l| l.id.clone()).collect::<Vec<_>>());
    let marker: Marker = serde_json::from_slice(&std::fs::read(root.join(&old_unoffered).join(MARKER)).unwrap()).unwrap();
    assert!(marker.offered.is_some());
    let mut config = BridgeConfig::in_dir(root.clone());
    config.free_space = plenty;
    // A later start: offers recorded "before" it.
    std::thread::sleep(Duration::from_millis(1100));
    let later = EditBridge::new(config, Arc::new(|_, _| {})).unwrap();
    let found: Vec<String> = later.sweep().into_iter().map(|l| l.id).collect();
    assert!(found.contains(&unsent) && found.contains(&young_offered));
    assert!(!found.contains(&old_unoffered));
    assert!(!root.join(&old_unoffered).exists());

    later.discard_leftover(&unsent).unwrap();
    assert!(!root.join(&unsent).exists());
}

#[test]
fn the_sweep_leaves_a_session_another_running_copy_has_open() {
    let rig = rig("sweep-open");
    let server = FakeServer::with(PATH, b"x\n");
    let info = rig.open(&server);
    let mut config = BridgeConfig::in_dir(rig.bridge.root().to_path_buf());
    config.free_space = plenty;
    let other = EditBridge::new(config, Arc::new(|_, _| {})).unwrap();
    assert!(other.sweep().is_empty());
    assert!(info.local.exists());
    assert!(other.discard_leftover(&info.id).is_err());
    assert!(info.local.exists());
}

// ---------------------------------------------------------------- the 2 GB cap

#[test]
fn past_the_cap_uploaded_copies_go_oldest_first_and_unsent_ones_stay() {
    let rig = rig_with("cap", |c| c.limits.folder_cap = 27_000);
    let root = rig.bridge.root().to_path_buf();
    let oldest_clean = plant(&root, &[b'a'; 10_000], &[b'a'; 10_000], None, 5);
    let unsent = plant(&root, &[b'b'; 10_000], b"other", None, 9);
    let server = FakeServer::with(PATH, &[b'c'; 6_000]);
    // An open session with nothing to upload, older than the planted clean one.
    let open = rig.open(&server);
    File::options().write(true).open(&open.local).unwrap().set_modified(SystemTime::now() - Duration::from_secs(6 * 86400)).unwrap();

    // About 26,700 bytes, with room wanted for 1,000 more, is past 27,000:
    // the open session's copy (the oldest uploaded one) goes first, which is
    // enough.
    rig.bridge.enforce_cap(1_000);
    rig.wait("the open copy cleared", |e| matches!(e, EditEvent::Evicted));
    assert!(!open.local.exists());
    assert!(root.join(&unsent).join("app.conf").exists(), "unsent changes are never deleted");
    assert!(root.join(&oldest_clean).exists(), "one copy was enough to get under the cap");

    // Its session still works: opening again downloads a fresh copy.
    server.put(PATH, b"fresh\n");
    let again = rig.open(&server);
    assert_eq!(again.id, open.id);
    assert_eq!(std::fs::read(&open.local).unwrap(), b"fresh\n");

    rig.bridge.enforce_cap(10_000);
    assert!(!root.join(&oldest_clean).exists());
    assert!(root.join(&unsent).join("app.conf").exists());
}

#[test]
fn an_editor_saving_a_cleared_copy_again_still_uploads() {
    let rig = rig_with("cap-save", |c| c.limits.folder_cap = 10);
    let server = FakeServer::with(PATH, b"0123456789abcdef\n");
    let info = rig.open(&server);
    rig.bridge.enforce_cap(0);
    rig.wait("cleared", |e| matches!(e, EditEvent::Evicted));
    // The editor still had it open, and saves.
    std::fs::write(&info.local, b"edited after clearing\n").unwrap();
    rig.wait("the upload", |e| matches!(e, EditEvent::Uploaded { .. }));
    assert_eq!(server.get(PATH).unwrap(), b"edited after clearing\n");
}

// ---------------------------------------------------------------- editors

#[test]
fn an_editor_is_started_only_by_full_path() {
    let err = EditorCommand::new("code", &[]).check().unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    let missing = std::env::temp_dir().join("sshee-no-such-editor.exe");
    assert_eq!(EditorCommand::new(&missing, &[]).check().unwrap_err().code, ErrorCode::NotFound);
    if cfg!(windows) {
        let scratch = Scratch::new("cmd");
        let shim = scratch.0.join("code.cmd");
        std::fs::write(&shim, b"@echo off").unwrap();
        assert_eq!(EditorCommand::new(&shim, &[]).check().unwrap_err().code, ErrorCode::Unsupported);
    }
}

#[cfg(unix)]
#[test]
fn the_file_is_one_argument_whatever_its_name() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("argv");
    let out = scratch.0.join("argv.txt");
    let editor = scratch.0.join("editor.sh");
    std::fs::write(&editor, format!("#!/bin/sh\nfor a in \"$@\"; do printf '[%s]\\n' \"$a\"; done > '{}'\n", out.display())).unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
    let file = scratch.0.join("x & touch pwned; $(id) `id` -rf.conf");

    let wait_for = |p: &Path| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !p.exists() || std::fs::read(p).unwrap().is_empty() {
            assert!(Instant::now() < deadline, "the editor didn't run");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(50));
        std::fs::read_to_string(p).unwrap()
    };
    // A script written a moment ago can't be started while another test's fork still holds
    // it open for writing ("Text file busy"): that clears in a moment.
    let launch = |args: &[&str]| {
        for _ in 0..100 {
            match EditorCommand::new(&editor, args).launch(&file) {
                Ok(_) => return,
                Err(e) if e.message.contains("busy") || e.message.contains("os error 26") => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("{e:?}"),
            }
        }
        panic!("the script stayed busy");
    };
    launch(&["--new-window"]);
    assert_eq!(wait_for(&out), format!("[--new-window]\n[{}]\n", file.display()));
    std::fs::remove_file(&out).unwrap();

    launch(&["-a", FILE_ARG, "--wait"]);
    assert_eq!(wait_for(&out), format!("[-a]\n[{}]\n[--wait]\n", file.display()));
    assert!(!scratch.0.join("pwned").exists());
    assert!(EditorCommand::new(&editor, &[]).command(Path::new("relative.conf")).is_err());
}

#[cfg(unix)]
#[test]
fn opening_starts_the_editor_on_the_copy() {
    use std::os::unix::fs::PermissionsExt;
    let rig = rig("launch");
    let out = rig.scratch.0.join("opened.txt");
    let editor = rig.scratch.0.join("editor.sh");
    std::fs::write(&editor, format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", out.display())).unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
    let server = FakeServer::with(PATH, b"x\n");
    let mut req = request(&server, PATH);
    req.editor = Some(EditorCommand::new(&editor, &[]));
    let Opened::Session(info) = rig.bridge.open(req).unwrap() else { panic!() };
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::fs::read(&out).map_or(true, |b| b.is_empty()) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(PathBuf::from(std::fs::read_to_string(&out).unwrap()), info.local);
}

// ---------------------------------------------------------------- compare

#[test]
fn the_diff_lists_lines_in_order() {
    let lines = line_diff("a\nB\nc\nd\n", "a\nb\nc\n");
    let kinds: Vec<(DiffKind, &str)> = lines.iter().map(|l| (l.kind, l.text.as_str())).collect();
    assert_eq!(kinds, vec![(DiffKind::Same, "a"), (DiffKind::Theirs, "b"), (DiffKind::Mine, "B"), (DiffKind::Same, "c"), (DiffKind::Mine, "d")]);
    assert!(compare_bytes(b"\x00\x01", b"text").binary);
}
