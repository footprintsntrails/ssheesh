//! "Open in your own editor" on sshee-core: the app's one `EditBridge`, what the window asks of
//! it (`sshee_app::files::edit::EditAsk`) and its news back to the window through `Remote`.
//!
//! The bridge's calls that talk to a server block (a download, a guarded save), so each runs on
//! a thread of its own, as Files' requests do; its sessions' own threads send their events
//! through the sink, which posts them to the window. A server is reached with the same `Target`
//! and `SshEnv` as Files, saved password and Tor included, the password read on that thread.
//! Finding the editors reads the disk (and on Windows the registry): on a thread too.

use std::path::Path;
use std::sync::Arc;

use sshee_app::editors::{self, Editor, Os, Places};
use sshee_app::files::edit::{Comparison, DiffKind, EditAsk, EditEvent, EditReply, Leftover, Opened};
use sshee_core::edit_bridge::{self as core, EditBridge, EditorCommand, OpenRequest};
use sshee_core::error::OpError;
use sshee_core::ssh::{SshEnv, Target};
use sshee_ui::shell::Remote;

use crate::files::{failure, Prepare};

/// A server's event as the window has it.
pub fn event(e: core::EditEvent) -> EditEvent {
    match e {
        core::EditEvent::Opened { local } => EditEvent::Opened { local },
        core::EditEvent::Uploading => EditEvent::Uploading,
        core::EditEvent::Uploaded { at } => EditEvent::Uploaded { at },
        core::EditEvent::Reloaded { at } => EditEvent::Reloaded { at },
        core::EditEvent::Offline { pending, retry_in_ms, message } => EditEvent::Offline { pending, retry_in_ms, message },
        core::EditEvent::Conflict { now } => EditEvent::Conflict { gone: now.is_none() },
        core::EditEvent::Failed { message } => EditEvent::Failed { message },
        core::EditEvent::Evicted => EditEvent::Evicted,
        core::EditEvent::Closed { kept } => EditEvent::Closed { kept },
    }
}

pub fn leftover(l: core::Leftover) -> Leftover {
    Leftover { id: l.id, host: l.host, remote_path: l.remote_path, local: l.local, size: l.size }
}

pub fn comparison(c: core::Comparison) -> Comparison {
    let kind = |k: core::DiffKind| match k {
        core::DiffKind::Same => DiffKind::Same,
        core::DiffKind::Mine => DiffKind::Mine,
        core::DiffKind::Theirs => DiffKind::Theirs,
    };
    Comparison { binary: c.binary, lines: c.lines.into_iter().map(|l| (kind(l.kind), l.text)).collect() }
}

/// How the bridge starts an editor: its program by its full path, and its arguments.
pub fn command(e: &Editor) -> EditorCommand {
    EditorCommand { program: e.program.clone(), args: e.args.clone() }
}

/// Where a server is, for the bridge: its target (`None`: it left the list) and what to do on
/// the worker thread before using it (read its saved password).
pub type Reach = (Option<Target>, Prepare);

/// The app's edit bridge.
pub struct Edit {
    env: SshEnv,
    bridge: Option<Arc<EditBridge>>,
    /// Why there's no bridge, said when "Open in" is used.
    problem: Option<String>,
    remote: Option<Remote>,
}

/// Runs `work` on a thread named `name`; a failure to start one is said as `failed`.
fn spawn(remote: &Remote, name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        remote.edit(EditReply::Failed { title: "Couldn't start the work".into(), failure: failure(OpError::failed(e.to_string())) });
    }
}

impl Edit {
    pub fn new(env: SshEnv) -> Edit {
        Edit { env, bridge: None, problem: None, remote: None }
    }

    /// Makes the bridge (`<the copy's folder>/edit`), its news going to the window.
    pub fn start(&mut self, remote: Remote) {
        let sink_remote = remote.clone();
        let sink: core::EditSink = Arc::new(move |id: &str, e| sink_remote.edit(EditReply::Event { id: id.to_owned(), event: event(e) }));
        match EditBridge::for_app(sink) {
            Ok(bridge) => self.bridge = Some(Arc::new(bridge)),
            Err(e) => self.problem = Some(e.message),
        }
        self.remote = Some(remote);
    }

    /// Does what the window asks; `reach` says where a server is. Returns at once, except
    /// for `CloseAll`, which the window waits for as it goes.
    pub fn ask(&self, ask: EditAsk, reach: impl Fn(&str) -> Reach) {
        let Some(remote) = self.remote.clone() else { return };
        let Some(bridge) = self.bridge.clone() else {
            if let EditAsk::Open { tab, host, path, editor, .. } = ask {
                let why = self.problem.clone().unwrap_or_else(|| "The folder for copies can't be used.".into());
                remote.edit(EditReply::Opened { tab, host, path, editor: editor.name, result: Err(failure(OpError::failed(why))) });
            }
            return;
        };
        let env = self.env.clone();
        let server = move |(target, prepare): Reach| -> Option<Arc<dyn core::Remote>> {
            let mut target = target?;
            prepare(&mut target);
            Some(Arc::new(core::Server { target, env }))
        };
        let gone = || failure(OpError::new(sshee_core::error::ErrorCode::NotFound, "That server isn't in your list anymore."));
        match ask {
            EditAsk::Start { custom } => spawn(&remote.clone(), "edit-start", move || {
                let (found, places) = (editors::installed(), Places::here());
                let custom = custom.map(|path| editors::choose_app(Os::current(), &places, &path));
                remote.edit(EditReply::Editors { found, custom });
                let left: Vec<Leftover> = bridge.sweep().into_iter().map(leftover).collect();
                remote.edit(EditReply::Leftovers(left));
            }),
            EditAsk::ChooseApp { path } => spawn(&remote.clone(), "edit-choose", move || {
                remote.edit(EditReply::Chosen(editors::choose_app(Os::current(), &Places::here(), &path)));
            }),
            EditAsk::Open { tab, host, path, editor, confirmed } => {
                let reach = reach(&host);
                spawn(&remote.clone(), "edit-open", move || {
                    let name = editor.name.clone();
                    let result = match server(reach) {
                        None => Err(gone()),
                        Some(remote) => bridge.open(OpenRequest { host: host.clone(), path: path.clone(), remote, editor: Some(command(&editor)), confirmed }).map_err(failure).map(|o| match o {
                            core::Opened::Session(info) => Opened::Session { id: info.id, local: info.local },
                            core::Opened::NeedsConfirm { size } => Opened::NeedsConfirm { size },
                        }),
                    };
                    remote.edit(EditReply::Opened { tab, host, path, editor: name, result });
                })
            }
            EditAsk::RetryNow { id } => bridge.retry_now(&id),
            EditAsk::RetryHost { host } => bridge.retry_host(&host),
            EditAsk::KeepMine { id } => spawn(&remote.clone(), "edit-keep", move || {
                if let Err(e) = bridge.keep_mine(&id) {
                    remote.edit(EditReply::Failed { title: "Couldn't upload yours".into(), failure: failure(e) });
                }
            }),
            EditAsk::TakeTheirs { id } => spawn(&remote.clone(), "edit-theirs", move || {
                if let Err(e) = bridge.take_theirs(&id) {
                    remote.edit(EditReply::Failed { title: "Couldn't take the server's version".into(), failure: failure(e) });
                }
            }),
            // Yours goes beside it on the server under a new name; then the copy here takes
            // theirs, so the session goes on with the file as it is there.
            EditAsk::SaveMineAsCopy { id } => spawn(&remote.clone(), "edit-copy", move || {
                let result = bridge.save_mine_as_copy(&id).map_err(failure);
                let saved = result.is_ok();
                remote.edit(EditReply::CopySaved { id: id.clone(), result });
                if saved {
                    if let Err(e) = bridge.take_theirs(&id) {
                        remote.edit(EditReply::Failed { title: "Couldn't take the server's version".into(), failure: failure(e) });
                    }
                }
            }),
            EditAsk::Compare { id } => spawn(&remote.clone(), "edit-compare", move || {
                let result = bridge.compare(&id).map(comparison).map_err(failure);
                remote.edit(EditReply::Compared { id, result });
            }),
            EditAsk::Close { id } => spawn(&remote, "edit-close", move || {
                bridge.close(&id);
            }),
            EditAsk::CloseHost { host } => spawn(&remote, "edit-close", move || {
                bridge.close_host(&host);
            }),
            // The window is going: the last saves go up now, and copies with changes the
            // server doesn't have stay for the next start.
            EditAsk::CloseAll => {
                bridge.close_all();
            }
            EditAsk::MarkOffered { ids } => spawn(&remote, "edit-offered", move || bridge.mark_offered(&ids)),
            EditAsk::ResumeLeftover { id, host } => {
                let reach = reach(&host);
                spawn(&remote.clone(), "edit-resume", move || {
                    let result = match server(reach) {
                        None => Err(OpError::new(sshee_core::error::ErrorCode::NotFound, "That server isn't in your list anymore.")),
                        Some(r) => bridge.resume_leftover(&id, r, None),
                    };
                    match result {
                        Ok(info) => remote.edit(EditReply::Resumed { id: info.id, host: info.host, path: info.remote_path, local: info.local }),
                        Err(e) => remote.edit(EditReply::Failed { title: "Couldn't upload a file left from before".into(), failure: failure(e) }),
                    }
                })
            }
            EditAsk::DiscardLeftover { id } => spawn(&remote.clone(), "edit-discard", move || {
                if let Err(e) = bridge.discard_leftover(&id) {
                    remote.edit(EditReply::Failed { title: "Couldn't delete a file left from before".into(), failure: failure(e) });
                }
            }),
            EditAsk::ShowFolder { path } => show_folder(&remote, &path),
        }
    }
}

/// The system's file manager on `path`, a folder of this computer's (an edit copy's): started
/// by its full program name with the folder as one argument, never through a shell.
fn show_folder(remote: &Remote, path: &Path) {
    if !path.is_dir() {
        return;
    }
    let program = if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else if cfg!(windows) {
        "explorer.exe"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(path).spawn() {
        remote.edit(EditReply::Failed { title: "Couldn't show the folder".into(), failure: failure(OpError::failed(format!("{}: {e}", path.display()))) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    #[test]
    fn events_and_comparisons_come_over() {
        assert_eq!(event(core::EditEvent::Conflict { now: None }), EditEvent::Conflict { gone: true });
        assert_eq!(event(core::EditEvent::Uploaded { at: 7 }), EditEvent::Uploaded { at: 7 });
        let c = core::compare_bytes(b"a\nmine\n", b"a\ntheirs\n");
        let c = comparison(c);
        assert!(!c.binary);
        assert!(c.lines.contains(&(DiffKind::Same, "a".into())) && c.lines.iter().any(|(k, _)| *k == DiffKind::Mine) && c.lines.iter().any(|(k, _)| *k == DiffKind::Theirs));
        let e = Editor { id: "zed".into(), name: "Zed".into(), program: "/usr/bin/zed".into(), args: vec!["{file}".into()], vscode_like: false };
        assert_eq!(command(&e), EditorCommand { program: "/usr/bin/zed".into(), args: vec!["{file}".into()] });
    }

    /// The whole path on this computer: a file opened (no editor started), a save to the copy
    /// uploaded, and the session closed with nothing left behind.
    #[test]
    fn a_file_of_this_computers_opens_uploads_its_saves_and_closes() {
        let dir = std::env::temp_dir().join(format!("sshee-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("server")).unwrap();
        let file = dir.join("server").join("notes.md");
        std::fs::write(&file, "one\n").unwrap();
        let events = Arc::new(Mutex::new(Vec::<(String, core::EditEvent)>::new()));
        let seen = events.clone();
        let sink: core::EditSink = Arc::new(move |id: &str, e| seen.lock().unwrap().push((id.to_owned(), e)));
        let bridge = EditBridge::new(core::BridgeConfig::in_dir(dir.join("edit")), sink).unwrap();
        let remote: Arc<dyn core::Remote> = Arc::new(core::Server { target: Target::Local, env: SshEnv::default() });
        let opened = bridge.open(OpenRequest { host: "local".into(), path: file.display().to_string(), remote, editor: None, confirmed: false }).unwrap();
        let core::Opened::Session(info) = opened else { panic!("asked to confirm a small file") };
        std::fs::write(&info.local, "two\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::fs::read_to_string(&file).unwrap() != "two\n" {
            assert!(Instant::now() < deadline, "the save never reached the file: {:?}", events.lock().unwrap());
            std::thread::sleep(Duration::from_millis(50));
        }
        let closed = bridge.close(&info.id).unwrap();
        assert!(closed.kept.is_none());
        assert!(events.lock().unwrap().iter().any(|(_, e)| matches!(e, core::EditEvent::Uploaded { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
