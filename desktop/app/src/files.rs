//! Files on sshee-core: what a Files tab asks for (`sshee_app::files::Ask`), done on a machine
//! (`sshee_core::files`), and answered back to the window (`Remote::files`).
//!
//! Every request runs on a thread of its own, named for what it does, so nothing waits on the
//! UI thread and a slow server holds up only its own answer. A transfer reports through
//! sshee-core's `Transfer` (a few times a second, the core's own pace) and is stopped through
//! it. sshee-app has its own plain types for entries, failures and stamps; the conversions are
//! here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sshee_app::files::protocol::{Answer, Ask, Item, Reply, SaveFailure, Want};
use sshee_app::files::{Clock, Entry, EntryKind, Failure, FailureKind, Listing, Stamp, TextFile, MAX_IMAGE};
use sshee_app::transfers::{Cleaning, Progress, TransferId};
use sshee_core::error::{ErrorCode, OpError};
use sshee_core::files as core;
use sshee_core::ssh::{SshEnv, Target};
use sshee_ui::shell::Remote;

// ---- sshee-core's types as sshee-app's

pub fn entry(e: core::FileEntry) -> Entry {
    let kind = match e.kind {
        core::FileKind::Dir => EntryKind::Dir,
        core::FileKind::File => EntryKind::File,
        core::FileKind::Link => EntryKind::BrokenLink,
        core::FileKind::Other => EntryKind::Other,
    };
    Entry { name: e.name, kind, is_link: e.is_link, size: e.size, modified: e.modified, mode: e.mode }
}

pub fn listing(l: core::DirListing) -> Listing {
    Listing { path: l.path, entries: l.entries.into_iter().map(entry).collect() }
}

pub fn failure(e: OpError) -> Failure {
    let kind = match e.code {
        ErrorCode::Auth => FailureKind::Auth,
        ErrorCode::HostKey => FailureKind::HostKey,
        ErrorCode::Network => FailureKind::Network,
        ErrorCode::NotFound => FailureKind::NotFound,
        ErrorCode::Exists => FailureKind::Exists,
        ErrorCode::TooLarge => FailureKind::TooLarge,
        ErrorCode::Unsupported => FailureKind::Unsupported,
        ErrorCode::Cancelled => FailureKind::Cancelled,
        ErrorCode::Conflict => FailureKind::Conflict,
        ErrorCode::Failed => FailureKind::Failed,
    };
    Failure { kind, message: e.message }
}

pub fn stamp(s: core::Stamp) -> Stamp {
    Stamp { size: s.size, modified: s.modified, sha256: s.sha256 }
}

/// A stamp on its way back to the server with a save.
pub fn core_stamp(s: &Stamp) -> core::Stamp {
    core::Stamp { size: s.size, modified: s.modified, sha256: s.sha256.clone() }
}

pub fn text_file(f: core::TextFile) -> TextFile {
    TextFile { path: f.path, content: f.content, binary: f.binary, size: f.size, lossy: f.lossy, crlf: f.crlf }
}

pub fn save_failure(e: core::SaveError) -> SaveFailure {
    match e {
        core::SaveError::Conflict(c) => SaveFailure::Conflict { now: c.now.map(stamp) },
        core::SaveError::Failed(e) => SaveFailure::Failed(failure(e)),
    }
}

pub fn cleaning(c: sshee_core::scrub::Cleaning) -> Cleaning {
    Cleaning { cleaned: c.cleaned, clean: c.clean, as_is: c.as_is, folders: c.folders, removed: c.removed.iter().map(|w| (*w).to_owned()).collect() }
}

pub fn progress(p: core::Progress, at_ms: u64) -> Progress {
    Progress { done: p.done, total: p.total, waiting: p.waiting, cleaning: p.cleaning.map(cleaning), at_ms }
}

/// Now on this computer, with how far its clock is from UTC: the list's dates are worked out
/// from these, so drawing reads no clock.
pub fn clock() -> Clock {
    let now = chrono::Local::now();
    Clock { now: now.timestamp(), utc_offset: now.offset().local_minus_utc() }
}

/// Where downloads go. Without a Downloads folder set up (a minimal Linux desktop) the system
/// names none, or names the home folder: a server's `.bashrc` would then land live in it, so
/// they go to ~/Downloads instead (as in 0.10).
pub fn downloads_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    dirs::download_dir().filter(|d| *d != home).unwrap_or_else(|| home.join("Downloads"))
}

// ---- the work

/// The transfers that can still be stopped.
type Running = Arc<Mutex<HashMap<TransferId, Arc<core::Transfer>>>>;

/// What one request's thread needs.
#[derive(Clone)]
struct Work {
    env: SshEnv,
    running: Running,
    /// Progress is timed from here, on a clock that only goes forward.
    started: Instant,
    /// Where downloads land.
    downloads: PathBuf,
}

/// Files' side of the live backend.
pub struct Files {
    work: Work,
    remote: Option<Remote>,
}

/// The name of a request's thread: what it does, never a path or a server.
fn thread_name(want: &Want) -> &'static str {
    match want {
        Want::List { .. } => "files-list",
        Want::Read { .. } | Want::ReadImage { .. } => "files-read",
        Want::Save { .. } => "files-save",
        Want::MakeDir { .. } | Want::Rename { .. } | Want::Move { .. } => "files-change",
        Want::Delete { .. } => "files-delete",
        Want::Extract { .. } => "files-unpack",
        Want::Download { .. } => "files-download",
        Want::Upload { .. } => "files-upload",
        Want::CancelTransfer { .. } => "files-stop",
    }
}

impl Files {
    pub fn new(env: SshEnv) -> Files {
        Files { work: Work { env, running: Arc::default(), started: Instant::now(), downloads: downloads_dir() }, remote: None }
    }

    /// How answers reach the window (`Backend::start`).
    pub fn start(&mut self, remote: Remote) {
        self.remote = Some(remote);
    }

    /// Does what a Files tab asks on `target` (`None`: its server left the list) and answers
    /// through the window's `Remote`. Returns at once.
    pub fn ask(&self, ask: Ask, target: Option<Target>) {
        // Stopping is a flag the transfer's own thread sees; its end is that thread's answer.
        if let Want::CancelTransfer { id } = ask.want {
            if let Some(xfer) = self.work.running.lock().unwrap().get(&id) {
                xfer.cancel();
            }
            return;
        }
        let Some(remote) = self.remote.clone() else { return };
        let work = self.work.clone();
        let name = thread_name(&ask.want);
        let lost = ask.clone();
        let spawned = std::thread::Builder::new().name(name.into()).spawn({
            let remote = remote.clone();
            move || {
                let report = {
                    let (remote, tab, server) = (remote.clone(), ask.tab, ask.server.clone());
                    move |answer| remote.files(Reply { tab, server: server.clone(), answer })
                };
                let answer = answer(&ask.want, target.as_ref(), &work, &report);
                remote.files(ask.reply(answer));
            }
        });
        // No thread to do it on: the tab still gets an answer, so nothing waits for good.
        if let Err(e) = spawned {
            let failed = Failure::new(FailureKind::Failed, format!("This computer couldn't start the work: {e}"));
            let answer = answer(&lost.want, None, &self.work, &|_| {}).failed_with(failed);
            remote.files(lost.reply(answer));
        }
    }
}

/// Swaps an answer's failure for another, keeping which request it answers.
trait FailedWith {
    fn failed_with(self, failure: Failure) -> Self;
}

impl FailedWith for Answer {
    fn failed_with(self, failure: Failure) -> Answer {
        match self {
            Answer::Listed { ticket, asked, .. } => Answer::Listed { ticket, asked, result: Err(failure) },
            Answer::Read { ticket, path, .. } => Answer::Read { ticket, path, result: Err(failure) },
            Answer::Image { ticket, path, .. } => Answer::Image { ticket, path, result: Err(failure) },
            Answer::Saved { ticket, path, .. } => Answer::Saved { ticket, path, result: Err(SaveFailure::Failed(failure)) },
            Answer::MadeDir { path, .. } => Answer::MadeDir { path, result: Err(failure) },
            Answer::Renamed { from, to, .. } => Answer::Renamed { from, to, result: Err(failure) },
            Answer::Extracted { dir, name, .. } => Answer::Extracted { dir, name, result: Err(failure) },
            Answer::Moved { from, to, moved, taken, .. } => Answer::Moved { from, to, moved, taken, failure: Some(failure) },
            Answer::Downloaded { id, .. } => Answer::Downloaded { id, result: Err(failure) },
            Answer::Uploaded { id, paths, dir, overwrite, .. } => Answer::Uploaded { id, paths, dir, overwrite, result: Err(failure) },
            other @ (Answer::Deleted { .. } | Answer::Progress { .. }) => other,
        }
    }
}

fn gone() -> OpError {
    OpError::new(ErrorCode::NotFound, "That server isn't in your list anymore.")
}

/// Runs a transfer that can be stopped by its id while it goes, reporting progress as it does.
fn transfer<T>(id: TransferId, work: &Work, report: &(impl Fn(Answer) + Clone + Send + Sync + 'static), run: impl FnOnce(&Arc<core::Transfer>) -> Result<T, OpError>) -> Result<T, Failure> {
    let (started, tell) = (work.started, report.clone());
    let xfer = core::Transfer::new(move |p| tell(Answer::Progress { id, progress: progress(p, started.elapsed().as_millis() as u64) }));
    work.running.lock().unwrap().insert(id, xfer.clone());
    let result = run(&xfer);
    work.running.lock().unwrap().remove(&id);
    result.map_err(failure)
}

/// One request, done: blocks for as long as the machine takes. `report` carries a transfer's
/// progress to the window meanwhile.
fn answer(want: &Want, target: Option<&Target>, work: &Work, report: &(impl Fn(Answer) + Clone + Send + Sync + 'static)) -> Answer {
    let env = &work.env;
    let on = || target.ok_or_else(gone);
    match want {
        Want::List { ticket, path } => {
            let result = on().and_then(|t| core::list(t, env, path)).map(|l| (listing(l), clock())).map_err(failure);
            Answer::Listed { ticket: *ticket, asked: path.clone(), result }
        }
        Want::Read { ticket, path } => {
            let result = on().and_then(|t| core::read_text_stamped(t, env, path)).map(|(f, s)| (text_file(f), stamp(s))).map_err(failure);
            Answer::Read { ticket: *ticket, path: path.clone(), result }
        }
        Want::ReadImage { ticket, path } => {
            let result = on().and_then(|t| core::read_stamped(t, env, path, MAX_IMAGE)).map(|(bytes, _)| bytes).map_err(failure);
            Answer::Image { ticket: *ticket, path: path.clone(), result }
        }
        Want::Save { ticket, path, text, expected } => {
            let expected = expected.as_ref().map(core_stamp);
            let result = match on() {
                Ok(t) => core::save_guarded(t, env, path, text.as_bytes(), expected.as_ref()).map(stamp).map_err(save_failure),
                Err(e) => Err(SaveFailure::Failed(failure(e))),
            };
            Answer::Saved { ticket: *ticket, path: path.clone(), result }
        }
        Want::MakeDir { path } => Answer::MadeDir { path: path.clone(), result: on().and_then(|t| core::make_dir(t, env, path)).map_err(failure) },
        Want::Rename { from, to } => Answer::Renamed { from: from.clone(), to: to.clone(), result: on().and_then(|t| core::rename(t, env, from, to)).map_err(failure) },
        Want::Delete { dir, names } => {
            let (mut deleted, mut failed) = (Vec::new(), Vec::new());
            for name in names {
                match on().and_then(|t| core::delete(t, env, &core::join_remote(dir, name))) {
                    Ok(()) => deleted.push(name.clone()),
                    Err(e) => failed.push((name.clone(), failure(e))),
                }
            }
            Answer::Deleted { dir: dir.clone(), deleted, failed }
        }
        Want::Extract { dir, name } => Answer::Extracted { dir: dir.clone(), name: name.clone(), result: on().and_then(|t| core::extract(t, env, dir, name)).map_err(failure) },
        Want::Move { from, names, to } => {
            let (mut moved, mut taken, mut stopped) = (Vec::new(), Vec::new(), None);
            for name in names {
                match on().and_then(|t| core::rename(t, env, &core::join_remote(from, name), &core::join_remote(to, name))) {
                    Ok(()) => moved.push(name.clone()),
                    // Nothing is replaced: that one stays where it was, the rest go on.
                    Err(e) if e.code == ErrorCode::Exists => taken.push(name.clone()),
                    Err(e) => {
                        stopped = Some(failure(e));
                        break;
                    }
                }
            }
            Answer::Moved { from: from.clone(), to: to.clone(), moved, taken, failure: stopped }
        }
        Want::Download { id, items } => {
            let result = transfer(*id, work, report, |xfer| {
                let t = on()?;
                std::fs::create_dir_all(&work.downloads)?;
                let mut saved = Vec::new();
                for Item { path, is_dir } in items {
                    saved.push(core::download(t, env, path, *is_dir, &work.downloads, xfer)?);
                }
                Ok(saved)
            });
            Answer::Downloaded { id: *id, result }
        }
        Want::Upload { id, paths, dir, overwrite } => {
            let result = transfer(*id, work, report, |xfer| {
                let t = on()?;
                // Everything that would be replaced, asked about once, before anything moves.
                if !overwrite {
                    core::check_upload(t, env, paths, dir)?;
                }
                xfer.check()?;
                xfer.add_total(paths.iter().map(|p| core::upload_size(p)).sum());
                for path in paths {
                    // Cleaned on the way, as 0.10 does unless it's turned off in Settings.
                    core::upload(t, env, path, dir, *overwrite, true, xfer)?;
                }
                Ok(())
            });
            Answer::Uploaded { id: *id, paths: paths.clone(), dir: dir.clone(), overwrite: *overwrite, result }
        }
        // Handled before a thread is made (`Files::ask`); a transfer's end is its own answer.
        Want::CancelTransfer { id } => Answer::Downloaded { id: *id, result: Err(Failure::new(FailureKind::Cancelled, "Cancelled.")) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    use sshee_core::ssh::SshTarget;

    #[test]
    fn entries_and_listings_come_over() {
        let of = |kind, is_link| core::FileEntry { name: "current".into(), kind, is_link, size: 7, modified: 1_700_000_000, mode: "lrwxrwxrwx".into() };
        let e = entry(of(core::FileKind::Dir, true));
        assert_eq!(e, Entry { name: "current".into(), kind: EntryKind::Dir, is_link: true, size: 7, modified: 1_700_000_000, mode: "lrwxrwxrwx".into() });
        assert_eq!(entry(of(core::FileKind::File, false)).kind, EntryKind::File);
        // sshee-core's "link" is one whose target is missing.
        assert_eq!(entry(of(core::FileKind::Link, true)).kind, EntryKind::BrokenLink);
        assert_eq!(entry(of(core::FileKind::Other, false)).kind, EntryKind::Other);
        let l = listing(core::DirListing { path: "/srv".into(), entries: vec![of(core::FileKind::File, false), of(core::FileKind::Dir, false)] });
        assert_eq!((l.path.as_str(), l.entries.len(), l.entries[1].is_dir()), ("/srv", 2, true));
    }

    #[test]
    fn every_kind_of_failure_keeps_its_kind_and_words() {
        let pairs = [
            (ErrorCode::Auth, FailureKind::Auth),
            (ErrorCode::HostKey, FailureKind::HostKey),
            (ErrorCode::Network, FailureKind::Network),
            (ErrorCode::NotFound, FailureKind::NotFound),
            (ErrorCode::Exists, FailureKind::Exists),
            (ErrorCode::TooLarge, FailureKind::TooLarge),
            (ErrorCode::Unsupported, FailureKind::Unsupported),
            (ErrorCode::Cancelled, FailureKind::Cancelled),
            (ErrorCode::Conflict, FailureKind::Conflict),
            (ErrorCode::Failed, FailureKind::Failed),
        ];
        for (code, kind) in pairs {
            assert_eq!(failure(OpError::new(code, "why")), Failure::new(kind, "why"));
        }
        assert!(failure(OpError::new(ErrorCode::Auth, "x")).needs_terminal());
    }

    #[test]
    fn stamps_files_and_conflicts_come_over_and_a_stamp_goes_back_unchanged() {
        let s = core::Stamp { size: 12, modified: 1_700_000_000, sha256: Some("ab".repeat(32)) };
        assert_eq!(core_stamp(&stamp(s.clone())), s);
        let bare = core::Stamp { size: 1, modified: 2, sha256: None };
        assert_eq!(core_stamp(&stamp(bare.clone())), bare);
        let f = text_file(core::TextFile { path: "/a".into(), content: Some("x\n".into()), binary: false, size: 3, lossy: true, crlf: true });
        assert_eq!(f, TextFile { path: "/a".into(), content: Some("x\n".into()), binary: false, size: 3, lossy: true, crlf: true });
        let changed = core::SaveError::Conflict(core::Conflict { path: "/a".into(), now: Some(s.clone()) });
        assert_eq!(save_failure(changed), SaveFailure::Conflict { now: Some(stamp(s)) });
        assert_eq!(save_failure(core::SaveError::Conflict(core::Conflict { path: "/a".into(), now: None })), SaveFailure::Conflict { now: None });
        assert_eq!(save_failure(core::SaveError::Failed(OpError::failed("no"))), SaveFailure::Failed(Failure::new(FailureKind::Failed, "no")));
    }

    #[test]
    fn progress_carries_its_time_and_what_cleaning_did() {
        let p = progress(core::Progress { done: 5, total: 9, waiting: true, cleaning: None }, 1_234);
        assert_eq!(p, Progress { done: 5, total: 9, waiting: true, cleaning: None, at_ms: 1_234 });
        let none = cleaning(sshee_core::scrub::Cleaning::default());
        assert_eq!(none, Cleaning::default());
    }

    #[test]
    fn the_clock_is_now_and_downloads_never_land_in_the_home_folder_itself() {
        let c = clock();
        assert!(c.now > 1_700_000_000 && c.utc_offset.abs() <= 14 * 3_600);
        assert_ne!(Some(downloads_dir()), dirs::home_dir());
    }

    #[test]
    fn a_server_that_left_the_list_answers_every_request_with_that() {
        let work = Work { env: SshEnv::default(), running: Arc::default(), started: Instant::now(), downloads: PathBuf::from("/nowhere") };
        let quiet = |_: Answer| {};
        let Answer::Listed { ticket: 3, result: Err(f), .. } = answer(&Want::List { ticket: 3, path: "~".into() }, None, &work, &quiet) else { panic!() };
        assert_eq!(f, Failure::new(FailureKind::NotFound, "That server isn't in your list anymore."));
        let Answer::Deleted { deleted, failed, .. } = answer(&Want::Delete { dir: "/srv".into(), names: vec!["a".into(), "b".into()] }, None, &work, &quiet) else { panic!() };
        assert!(deleted.is_empty() && failed.len() == 2);
        let Answer::Uploaded { result: Err(f), overwrite: true, .. } = answer(&Want::Upload { id: TransferId(1), paths: vec![], dir: "/srv".into(), overwrite: true }, None, &work, &quiet) else { panic!() };
        assert_eq!(f.kind, FailureKind::NotFound);
        assert!(work.running.lock().unwrap().is_empty(), "a transfer that ended can't be stopped anymore");
        let Answer::Saved { result: Err(SaveFailure::Failed(_)), .. } = answer(&Want::Save { ticket: 1, path: "/a".into(), text: String::new(), expected: None }, None, &work, &quiet) else { panic!() };
    }

    #[test]
    fn a_failure_swapped_in_keeps_which_request_it_answers() {
        let f = Failure::new(FailureKind::Failed, "no thread");
        let listed = Answer::Listed { ticket: 4, asked: "~".into(), result: Err(Failure::new(FailureKind::NotFound, "x")) }.failed_with(f.clone());
        assert_eq!(listed, Answer::Listed { ticket: 4, asked: "~".into(), result: Err(f.clone()) });
        let saved = Answer::Saved { ticket: 5, path: "/a".into(), result: Ok(Stamp { size: 0, modified: 0, sha256: None }) }.failed_with(f.clone());
        assert_eq!(saved, Answer::Saved { ticket: 5, path: "/a".into(), result: Err(SaveFailure::Failed(f)) });
    }

    /// This computer's own files, through the same code a server's go through.
    #[test]
    fn this_computers_folder_lists_saves_guarded_and_changes() {
        let dir = std::env::temp_dir().join(format!("sshee-files-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("shop")).unwrap();
        std::fs::write(dir.join("notes.md"), "one\r\ntwo\r\n").unwrap();
        let at = dir.to_string_lossy().replace('\\', "/");
        let work = Work { env: SshEnv::default(), running: Arc::default(), started: Instant::now(), downloads: dir.join("dl") };
        let quiet = |_: Answer| {};
        let ask = |want: Want| answer(&want, Some(&Target::Local), &work, &quiet);

        let Answer::Listed { result: Ok((l, clock)), .. } = ask(Want::List { ticket: 1, path: at.clone() }) else { panic!() };
        let mut names: Vec<(&str, bool)> = l.entries.iter().map(|e| (e.name.as_str(), e.is_dir())).collect();
        names.sort();
        assert_eq!(names, [("notes.md", false), ("shop", true)]);
        assert!(clock.now > 1_700_000_000);

        let file = format!("{at}/notes.md");
        let Answer::Read { result: Ok((text, stamp)), .. } = ask(Want::Read { ticket: 2, path: file.clone() }) else { panic!() };
        assert_eq!((text.content.as_deref(), text.crlf), (Some("one\ntwo\n"), true));
        // Changed behind the editor's back: the save writes nothing and says what's there.
        std::fs::write(dir.join("notes.md"), "theirs\r\n").unwrap();
        let Answer::Saved { result: Err(SaveFailure::Conflict { now: Some(now) }), .. } = ask(Want::Save { ticket: 3, path: file.clone(), text: "mine\r\n".into(), expected: Some(stamp) }) else { panic!() };
        assert_eq!(std::fs::read_to_string(dir.join("notes.md")).unwrap(), "theirs\r\n");
        // Knowingly over what's there now.
        let Answer::Saved { result: Ok(_), .. } = ask(Want::Save { ticket: 4, path: file.clone(), text: "mine\r\n".into(), expected: Some(now) }) else { panic!() };
        assert_eq!(std::fs::read_to_string(dir.join("notes.md")).unwrap(), "mine\r\n");
        // A copy beside it lands only where there's nothing.
        let Answer::Saved { result: Err(SaveFailure::Conflict { now: Some(_) }), .. } = ask(Want::Save { ticket: 5, path: file.clone(), text: "x".into(), expected: None }) else { panic!() };
        let Answer::Saved { result: Ok(_), .. } = ask(Want::Save { ticket: 6, path: format!("{at}/notes (my copy).md"), text: "x".into(), expected: None }) else { panic!() };

        let Answer::MadeDir { result: Ok(()), .. } = ask(Want::MakeDir { path: format!("{at}/logs") }) else { panic!() };
        let Answer::Renamed { result: Err(taken), .. } = ask(Want::Rename { from: format!("{at}/logs"), to: format!("{at}/shop") }) else { panic!() };
        assert_eq!(taken.kind, FailureKind::Exists);
        std::fs::write(dir.join("shop").join("notes.md"), "already here").unwrap();
        let Answer::Moved { moved, taken, failure: None, .. } = ask(Want::Move { from: at.clone(), names: vec!["notes.md".into(), "logs".into()], to: format!("{at}/shop") }) else { panic!() };
        assert_eq!((moved, taken), (vec!["logs".to_owned()], vec!["notes.md".to_owned()]));
        let Answer::Deleted { deleted, failed, .. } = ask(Want::Delete { dir: at.clone(), names: vec!["notes.md".into(), "shop".into(), "nothing".into()] }) else { panic!() };
        assert_eq!(deleted, ["notes.md", "shop"]);
        assert_eq!(failed.len(), 1);
        assert!(!dir.join("shop").exists());

        // A picture's bytes come whole, and one over the limit is refused by the same call.
        std::fs::write(dir.join("dot.png"), [0x89, b'P', b'N', b'G']).unwrap();
        let Answer::Image { result: Ok(bytes), .. } = ask(Want::ReadImage { ticket: 7, path: format!("{at}/dot.png") }) else { panic!() };
        assert_eq!(bytes, [0x89, b'P', b'N', b'G']);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A throwaway sshd for the live test; killed when dropped.
    struct Sshd(Child);

    impl Drop for Sshd {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn keygen(path: &Path) {
        let _ = std::fs::remove_file(path);
        let ok = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-C", "sshee-files-test", "-f"]).arg(path).status().is_ok_and(|s| s.success());
        assert!(ok, "ssh-keygen");
    }

    /// The whole path against a real sshd: a request in, threads, sshee-core over ssh, answers
    /// out through a `Remote`. Runs only when `SSHEE_LIVE_SSHD` names a folder to keep its
    /// keys and files in (its own host and user keys, its own `authorized_keys`; nothing of
    /// this computer's ssh setup is read or written):
    /// `SSHEE_LIVE_SSHD=/short/dir cargo test -p ssheesh-preview live_sshd`.
    #[test]
    fn live_sshd_lists_reads_saves_uploads_and_downloads() {
        let Some(base) = std::env::var_os("SSHEE_LIVE_SSHD").map(PathBuf::from) else {
            eprintln!("skipped: SSHEE_LIVE_SSHD isn't set");
            return;
        };
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("srv/shop")).unwrap();
        std::fs::create_dir_all(base.join("local")).unwrap();
        keygen(&base.join("host_key"));
        keygen(&base.join("user_key"));
        std::fs::copy(base.join("user_key.pub"), base.join("authorized_keys")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let config = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {b}/host_key\nPidFile {b}/sshd.pid\nAuthorizedKeysFile {b}/authorized_keys\nStrictModes no\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPermitRootLogin prohibit-password\nPermitUserEnvironment no\n",
            b = base.display()
        );
        std::fs::write(base.join("sshd_config"), config).unwrap();
        let sshd = Command::new("/usr/sbin/sshd").arg("-D").arg("-e").arg("-f").arg(base.join("sshd_config")).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        let Ok(child) = sshd else {
            eprintln!("skipped: no sshd here");
            return;
        };
        let _sshd = Sshd(child);
        for _ in 0..100 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
        let opts = [
            ("UserKnownHostsFile", base.join("known_hosts").display().to_string()),
            ("GlobalKnownHostsFile", "/dev/null".into()),
            ("StrictHostKeyChecking", "accept-new".into()),
            ("IdentitiesOnly", "yes".into()),
            ("IdentityAgent", "none".into()),
        ];
        let mut extra_args = vec!["-F".to_owned(), "/dev/null".to_owned()];
        for (key, value) in opts {
            extra_args.extend(["-o".to_owned(), format!("{key}={value}")]);
        }
        let target = Target::Ssh(SshTarget {
            destination: "127.0.0.1".into(),
            user: Some(user),
            port: Some(port),
            identity_file: Some(base.join("user_key").display().to_string()),
            extra_args,
            ..SshTarget::default()
        });

        // Answers arrive as closures for the UI thread; a channel stands in for it. What a
        // closure does to a window is `deliver`'s business (tested in sshee-ui); here the
        // answers themselves are checked, straight from the threads `Files::ask` starts.
        let srv = base.join("srv").display().to_string();
        std::fs::write(base.join("srv/README.md"), "# shop\n").unwrap();
        std::fs::write(base.join("local/upload.txt"), "from this computer\n").unwrap();
        let work = Work { env: SshEnv::default(), running: Arc::default(), started: Instant::now(), downloads: base.join("downloads") };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let report = {
            let seen = seen.clone();
            move |answer: Answer| seen.lock().unwrap().push(answer)
        };
        let on_thread = |want: Want| {
            let (work, target, report) = (work.clone(), target.clone(), report.clone());
            let name = thread_name(&want);
            std::thread::Builder::new().name(name.into()).spawn(move || answer(&want, Some(&target), &work, &report)).unwrap().join().unwrap()
        };

        let Answer::Listed { result, .. } = on_thread(Want::List { ticket: 1, path: srv.clone() }) else { panic!() };
        let (l, _) = result.unwrap_or_else(|f| panic!("listing over ssh: {f:?}"));
        let mut names: Vec<(&str, bool)> = l.entries.iter().map(|e| (e.name.as_str(), e.is_dir())).collect();
        names.sort();
        assert_eq!((l.path.as_str(), names), (srv.as_str(), vec![("README.md", false), ("shop", true)]));

        let readme = format!("{srv}/README.md");
        let Answer::Read { result: Ok((text, stamp)), .. } = on_thread(Want::Read { ticket: 2, path: readme.clone() }) else { panic!() };
        assert_eq!(text.content.as_deref(), Some("# shop\n"));
        let Answer::Saved { result: Ok(saved), .. } = on_thread(Want::Save { ticket: 3, path: readme.clone(), text: "# shop\nopen\n".into(), expected: Some(stamp.clone()) }) else { panic!() };
        assert_eq!(std::fs::read_to_string(base.join("srv/README.md")).unwrap(), "# shop\nopen\n");
        // The stamp from before the save is stale now: a second editor's save is refused.
        let Answer::Saved { result: Err(SaveFailure::Conflict { now: Some(now) }), .. } = on_thread(Want::Save { ticket: 4, path: readme.clone(), text: "other\n".into(), expected: Some(stamp) }) else { panic!() };
        assert_eq!(now.sha256, saved.sha256);

        let Answer::MadeDir { result: Ok(()), .. } = on_thread(Want::MakeDir { path: format!("{srv}/logs") }) else { panic!() };
        let Answer::Moved { moved, taken, failure: None, .. } = on_thread(Want::Move { from: srv.clone(), names: vec!["logs".into()], to: format!("{srv}/shop") }) else { panic!() };
        assert_eq!((moved, taken), (vec!["logs".to_owned()], Vec::<String>::new()));

        let upload = Want::Upload { id: TransferId(1), paths: vec![base.join("local/upload.txt")], dir: srv.clone(), overwrite: false };
        let Answer::Uploaded { result: Ok(()), .. } = on_thread(upload.clone()) else { panic!() };
        assert_eq!(std::fs::read_to_string(base.join("srv/upload.txt")).unwrap(), "from this computer\n");
        // Again without replacing: nothing is sent, and the answer says the name is taken.
        let Answer::Uploaded { result: Err(taken), .. } = on_thread(upload) else { panic!() };
        assert_eq!(taken.kind, FailureKind::Exists);
        assert!(seen.lock().unwrap().iter().any(|a| matches!(a, Answer::Progress { id: TransferId(1), .. })), "the upload reported progress");

        let items = vec![Item { path: readme, is_dir: false }, Item { path: format!("{srv}/shop"), is_dir: true }];
        let Answer::Downloaded { result: Ok(at), .. } = on_thread(Want::Download { id: TransferId(2), items }) else { panic!() };
        assert_eq!(at.len(), 2);
        assert_eq!(std::fs::read_to_string(&at[0]).unwrap(), "# shop\nopen\n");
        assert!(Path::new(&at[1]).join("logs").is_dir());
        assert!(work.running.lock().unwrap().is_empty());

        let Answer::Deleted { deleted, failed, .. } = on_thread(Want::Delete { dir: srv, names: vec!["upload.txt".into(), "shop".into()] }) else { panic!() };
        assert_eq!((deleted.len(), failed.len()), (2, 0));
        assert!(!base.join("srv/shop").exists());
    }
}
