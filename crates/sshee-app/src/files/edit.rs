//! "Open in your own editor", without a screen: which editor "Open in <name>" means, the files
//! open in an editor elsewhere and what each one's status line says, the questions asked on
//! the way (a big file, a conflict, copies left from an earlier run), and the protocol between
//! the screen and the backend that holds sshee-core's `edit_bridge`.
//!
//! The bridge does the work: it copies a server's file into a private folder, starts the editor
//! on the copy and uploads every save with a guarded save, so a change made on the server in the
//! meantime is never written over without asking. Its rules (sizes, how long copies live) are
//! in the doc comment at the top of `sshee-core/src/edit_bridge.rs`; the screen's part is in
//! desktop/ARCHITECTURE.md, "Open in your own editor".
//!
//! The types mirror the bridge's (`EditEvent`, `Leftover`, `Comparison`): this crate doesn't
//! depend on sshee-core, so the program converts at its edge (`desktop/app/src/edit.rs`). An
//! editor is [`crate::editors::Editor`] itself.

use std::path::PathBuf;

use crate::editors::{self, Editor};
use crate::workbench::{ServerKey, TabId};

use super::format::bytes;
use super::ops::Confirm;
use super::{Failure, Notice, Tone};

/// Below this Files' own editor is the default action; at or above it the open file offers
/// "Open in <name>" instead (`edit_bridge::BUILT_IN_MAX`, the same as [`super::MAX_EDIT`]).
pub const BUILT_IN_MAX: u64 = super::MAX_EDIT;

/// What happened to a session last (`edit_bridge::EditEvent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditEvent {
    /// The copy is ready, and the editor was started on it.
    Opened { local: PathBuf },
    /// A save is on its way to the server.
    Uploading,
    /// The server has the copy's version, since `at` (Unix seconds).
    Uploaded { at: u64 },
    /// The copy was replaced with the server's version.
    Reloaded { at: u64 },
    /// The server can't be reached: the save waits here and goes at the next try.
    Offline { pending: bool, retry_in_ms: u64, message: String },
    /// The file changed on the server since the copy was made; `gone`: it isn't there anymore.
    Conflict { gone: bool },
    /// The save didn't go, for a reason trying again won't fix by itself.
    Failed { message: String },
    /// The copy was cleared to save space; it's on the server.
    Evicted,
    /// The session ended; `kept` is the copy when it has changes the server doesn't.
    Closed { kept: Option<PathBuf> },
}

/// A copy an earlier run left with changes that were never uploaded (`edit_bridge::Leftover`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    pub id: String,
    pub host: ServerKey,
    pub remote_path: String,
    /// The copy on this computer.
    pub local: PathBuf,
    pub size: u64,
}

/// Which side of a conflict a line is from (`edit_bridge::DiffKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Same,
    Mine,
    Theirs,
}

/// The two versions in a conflict, line by line (`edit_bridge::Comparison`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// Either isn't text: nothing to compare line by line.
    pub binary: bool,
    pub lines: Vec<(DiffKind, String)>,
}

/// What the screen asks the backend's bridge. Every one that talks to a server runs on a
/// thread of the backend's and answers with an [`EditReply`].
#[derive(Debug, Clone, PartialEq)]
pub enum EditAsk {
    /// Once, at start: the editors on this computer, the one picked with "Choose an app…"
    /// (`custom`, the setting's path), and the copies an earlier run left (`sweep`).
    Start { custom: Option<PathBuf> },
    /// "Choose an app…": is the picked program an editor that can be started safely?
    ChooseApp { path: PathBuf },
    /// "Open in <editor>" on `path` of `host`, asked from `tab`. `confirmed`: a big file the
    /// question was said yes to.
    Open { tab: TabId, host: ServerKey, path: String, editor: Editor, confirmed: bool },
    RetryNow { id: String },
    /// A server's terminal connected again: its sessions try at once.
    RetryHost { host: ServerKey },
    KeepMine { id: String },
    TakeTheirs { id: String },
    SaveMineAsCopy { id: String },
    Compare { id: String },
    /// "Stop editing".
    Close { id: String },
    /// A server's last tab closed.
    CloseHost { host: ServerKey },
    /// The window closes: done before it goes.
    CloseAll,
    /// The leftovers' prompt was shown: one that then sits a month goes at a later start.
    MarkOffered { ids: Vec<String> },
    /// "Upload" on the leftovers' prompt.
    ResumeLeftover { id: String, host: ServerKey },
    /// "Discard" on the leftovers' prompt.
    DiscardLeftover { id: String },
    /// "Show in folder": the system's file manager on this folder of this computer's.
    ShowFolder { path: PathBuf },
}

/// What `open` did (`edit_bridge::Opened`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    Session { id: String, local: PathBuf },
    /// Over 50 MB: ask, then open with `confirmed`.
    NeedsConfirm { size: u64 },
}

/// The backend's answers, on the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub enum EditReply {
    /// The editors found, and the custom one (`Err`: why it can't be used).
    Editors { found: Vec<Editor>, custom: Option<Result<Editor, String>> },
    /// Leftovers from an earlier run that have unsent changes.
    Leftovers(Vec<Leftover>),
    /// "Choose an app…" checked the pick.
    Chosen(Result<Editor, String>),
    Opened { tab: TabId, host: ServerKey, path: String, editor: String, result: Result<Opened, Failure> },
    /// A session's news, from the bridge's threads.
    Event { id: String, event: EditEvent },
    Compared { id: String, result: Result<Comparison, Failure> },
    /// "Save mine as a copy" made `path` on the server.
    CopySaved { id: String, result: Result<String, Failure> },
    /// "Upload" on a leftover: it is a session now, uploading.
    Resumed { id: String, host: ServerKey, path: String, local: PathBuf },
    /// Something asked of a session failed (keep mine, take theirs, upload a leftover).
    Failed { title: String, failure: Failure },
}

/// One file open in an editor elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub host: ServerKey,
    pub path: String,
    /// The editor's name: "VS Code".
    pub editor: String,
    pub last: EditEvent,
    /// The copy on this computer.
    pub local: PathBuf,
}

/// What a session's status says: the short form on its row, the line under the list, and
/// which button goes with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// On the row, after the name: "Saved 12:41".
    pub mark: String,
    /// The line: "Saved to the server at 12:41".
    pub line: String,
    /// Retry is offered.
    pub retry: bool,
    /// "Resolve…" is offered: the conflict's dialog.
    pub conflict: bool,
    /// The line is an error's: red.
    pub error: bool,
    /// It waits for you (a conflict): the "needs you" orange.
    pub waits: bool,
}

/// `hh:mm` of a Unix time, in this computer's time zone (`utc_offset` seconds east).
pub fn clock_time(unix: u64, utc_offset: i32) -> String {
    let local = unix as i64 + utc_offset as i64;
    let secs = local.rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3_600, secs % 3_600 / 60)
}

impl Session {
    /// The status line, as of now.
    pub fn status(&self, utc_offset: i32) -> Status {
        let plain = |mark: &str, line: String| Status { mark: mark.into(), line, retry: false, conflict: false, error: false, waits: false };
        match &self.last {
            EditEvent::Opened { .. } => plain(&format!("In {}", self.editor), format!("Open in {}", self.editor)),
            EditEvent::Uploading => plain("Uploading…", "Saving to the server…".into()),
            EditEvent::Uploaded { at } => {
                let at = clock_time(*at, utc_offset);
                plain(&format!("Uploaded {at}"), format!("Saved to the server at {at}"))
            }
            EditEvent::Reloaded { at } => {
                let at = clock_time(*at, utc_offset);
                plain(&format!("Reloaded {at}"), format!("Took the server's version at {at}"))
            }
            EditEvent::Offline { pending: true, .. } => Status { retry: true, ..plain("Offline, will retry", "Not on the server yet: waiting for the connection".into()) },
            EditEvent::Offline { .. } => Status { retry: true, ..plain("Offline", "The server can't be reached; nothing is waiting to upload".into()) },
            EditEvent::Conflict { gone: false } => Status { conflict: true, waits: true, ..plain("Conflict", "Changed on the server: choose which version to keep".into()) },
            EditEvent::Conflict { gone: true } => Status { conflict: true, waits: true, ..plain("Conflict", "Deleted on the server: choose what to do with yours".into()) },
            EditEvent::Failed { message } => Status { retry: true, error: true, ..plain("Failed", message.clone()) },
            EditEvent::Evicted => plain("Cleared", "Copy cleared to save space; Open downloads it again".into()),
            EditEvent::Closed { kept: Some(_) } => plain("Kept here", "Stopped editing. The copy has changes the server doesn't, so it was kept".into()),
            EditEvent::Closed { kept: None } => plain("", "Stopped editing".into()),
        }
    }

    /// Still open in the editor: shown on its row.
    pub fn is_open(&self) -> bool {
        !matches!(self.last, EditEvent::Closed { .. })
    }
}

/// A question the screen asks before or while editing elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// A file over 50 MB: download a copy? Yes opens with `confirmed`.
    Big { tab: TabId, host: ServerKey, path: String, editor: Editor, size: u64 },
    /// Before the first "Open in" of a run while Tor or streamer mode is on.
    Warn { tab: TabId, host: ServerKey, path: String, editor: Editor, tor: bool, streaming: bool },
}

impl Question {
    pub fn confirm(&self) -> Confirm {
        match self {
            Question::Big { size, .. } => Confirm {
                title: format!("This file is {}. Download a copy to edit?", bytes(*size)),
                body: "Each save uploads the whole file again, which takes a while on a slow connection.".into(),
                lines: Vec::new(),
                confirm_label: "Download and open",
                danger: false,
            },
            Question::Warn { editor, tor, streaming, .. } => {
                let mut body = Vec::new();
                if *tor {
                    body.push(format!("{}, its extensions and AI helpers connect directly from your address, not through Tor.", editor.name));
                }
                if *streaming {
                    body.push(format!("{} shows the file as it is, names and secrets included: streamer mode can't hide them there.", editor.name));
                }
                Confirm { title: format!("Open in {}?", editor.name), body: format!("{} To keep it inside SSHeesh, use Open preview.", body.join(" ")), lines: Vec::new(), confirm_label: "Open anyway", danger: false }
            }
        }
    }
}

/// Every file open elsewhere, the editors, and what is being asked: window-wide state, kept by
/// the screen and changed only through these methods.
#[derive(Debug, Default)]
pub struct Edits {
    /// The editors found on this computer, VS Code-like ones first.
    pub found: Vec<Editor>,
    /// The one picked with "Choose an app…", when the setting names one.
    pub custom: Option<Editor>,
    /// Open and recently ended sessions, oldest first.
    pub sessions: Vec<Session>,
    /// The session whose conflict dialog is up, and its comparison once asked for.
    pub conflict: Option<String>,
    pub compare: Option<Comparison>,
    /// What is asked before opening.
    pub question: Option<Question>,
    /// Copies left with unsent changes, while their prompt is up.
    pub leftovers: Vec<Leftover>,
    /// Tor's and streamer mode's warning was said yes to in this run.
    pub warned: bool,
    /// Files being opened: their row says "Opening…".
    pub opening: Vec<(ServerKey, String)>,
    /// The servers that had tabs, to tell when one's last tab closes.
    pub hosts: Vec<ServerKey>,
    /// "Choose an app…" from a file's menu: the file to open once the app is checked.
    pub pending: Option<(TabId, ServerKey, String)>,
}

impl Edits {
    /// The editor "Open in" means for the setting `externalEditor` (`setting`): the one it
    /// names, the custom one for `custom:<path>`, else the default (`editors::chosen`'s rule,
    /// over what the backend found).
    pub fn chosen(&self, setting: Option<&str>) -> Option<&Editor> {
        let fallback = || editors::default_id(&self.found).and_then(|id| self.found.iter().find(|e| e.id == id));
        match setting {
            Some(s) if s.starts_with(editors::CUSTOM_PREFIX) => self.custom.as_ref().filter(|c| c.id == s).or_else(fallback),
            Some(id) => self.found.iter().find(|e| e.id == id).or_else(fallback),
            None => fallback(),
        }
    }

    /// The session of `path` on `host` that is still open, if any.
    pub fn open_on(&self, host: &str, path: &str) -> Option<&Session> {
        self.sessions.iter().rev().find(|s| s.host == host && s.path == path && s.is_open())
    }

    /// The open sessions in folder `dir` of `host`, for the lines under its list.
    pub fn in_folder<'a>(&'a self, host: &'a str, dir: &'a str) -> impl Iterator<Item = &'a Session> + 'a {
        self.sessions.iter().filter(move |s| s.host == host && crate::paths::parent(&s.path) == dir && (s.is_open() || matches!(s.last, EditEvent::Closed { kept: Some(_) })))
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn is_opening(&self, host: &str, path: &str) -> bool {
        self.opening.iter().any(|(h, p)| h == host && p == path)
    }

    /// "Open in" on a file: what to send, or the question to ask first. `tor` and `streaming`:
    /// the warning is due once per run while either is on.
    pub fn open(&mut self, tab: TabId, host: &str, path: &str, editor: Editor, tor: bool, streaming: bool) -> Option<EditAsk> {
        if (tor || streaming) && !self.warned {
            self.question = Some(Question::Warn { tab, host: host.into(), path: path.into(), editor, tor, streaming });
            return None;
        }
        self.opening.push((host.into(), path.into()));
        Some(EditAsk::Open { tab, host: host.into(), path: path.into(), editor, confirmed: false })
    }

    /// The answer to the question on screen.
    pub fn answer(&mut self, yes: bool) -> Option<EditAsk> {
        let question = self.question.take()?;
        if !yes {
            return None;
        }
        match question {
            Question::Big { tab, host, path, editor, .. } => {
                self.opening.push((host.clone(), path.clone()));
                Some(EditAsk::Open { tab, host, path, editor, confirmed: true })
            }
            Question::Warn { tab, host, path, editor, .. } => {
                self.warned = true;
                self.open(tab, &host, &path, editor, false, false)
            }
        }
    }

    /// The servers whose last tab closed since the last call: their sessions close.
    pub fn hosts_closed(&mut self, now: Vec<ServerKey>) -> Vec<EditAsk> {
        let gone = self.hosts.iter().filter(|h| !now.contains(h)).map(|host| EditAsk::CloseHost { host: host.clone() }).collect();
        self.hosts = now;
        gone
    }

    /// The conflict's dialog: shown for session `id`, or closed (`None`: decide later).
    pub fn show_conflict(&mut self, id: Option<&str>) {
        self.conflict = id.map(str::to_owned);
        self.compare = None;
    }

    /// The leftovers' prompt was answered: it goes.
    pub fn leftovers_done(&mut self) -> Vec<Leftover> {
        std::mem::take(&mut self.leftovers)
    }

    /// Takes a backend's answer in. Returns a note for Files' notes, when there's one to say.
    pub fn apply(&mut self, reply: EditReply) -> Option<Notice> {
        match reply {
            EditReply::Editors { found, custom } => {
                self.found = found;
                self.custom = custom.and_then(Result::ok);
                None
            }
            EditReply::Leftovers(list) => {
                self.leftovers = list;
                None
            }
            EditReply::Chosen(Err(why)) => Some(error("Can't open files in that app", why)),
            EditReply::Chosen(Ok(editor)) => {
                self.custom = Some(editor);
                None
            }
            EditReply::Opened { tab, host, path, editor, result } => {
                self.opening.retain(|(h, p)| !(*h == host && *p == path));
                match result {
                    Ok(Opened::Session { id, local }) => {
                        self.sessions.retain(|s| s.id != id && !(s.host == host && s.path == path && !s.is_open()));
                        self.sessions.push(Session { id, host, path, editor, last: EditEvent::Opened { local: local.clone() }, local });
                        None
                    }
                    Ok(Opened::NeedsConfirm { size }) => {
                        let found = self.found.iter().chain(self.custom.iter()).find(|e| e.name == editor).cloned();
                        if let Some(editor) = found {
                            self.question = Some(Question::Big { tab, host, path, editor, size });
                        }
                        None
                    }
                    Err(failure) => Some(error(format!("Couldn't open {} in {editor}", crate::paths::name(&path)), failure.message)),
                }
            }
            EditReply::Event { id, event } => {
                let session = self.sessions.iter_mut().find(|s| s.id == id)?;
                if let EditEvent::Opened { local } = &event {
                    session.local = local.clone();
                }
                let conflict = matches!(event, EditEvent::Conflict { .. });
                let kept = match &event {
                    EditEvent::Closed { kept: Some(at) } => Some(at.clone()),
                    _ => None,
                };
                session.last = event;
                if conflict {
                    // The question comes up by itself: nothing more goes up until it's answered.
                    self.conflict = Some(id);
                    self.compare = None;
                } else if self.conflict.as_deref() == Some(&id) {
                    self.conflict = None;
                }
                kept.map(|at| Notice { folder: at.parent().map(PathBuf::from), ..info("Stopped editing; the copy was kept", format!("It has changes the server doesn't: {}", at.display())) })
            }
            EditReply::Compared { id, result } => match result {
                Ok(c) if self.conflict.as_deref() == Some(&id) => {
                    self.compare = Some(c);
                    None
                }
                Ok(_) => None,
                Err(f) => Some(error("Couldn't compare the two versions", f.message)),
            },
            EditReply::CopySaved { result: Ok(path), .. } => Some(info(format!("Saved yours as {}", crate::paths::name(&path)), format!("Beside it on the server: {path}"))),
            EditReply::CopySaved { result: Err(f), .. } => Some(error("Couldn't save yours as a copy", f.message)),
            EditReply::Resumed { id, host, path, local } => {
                self.sessions.retain(|s| s.id != id);
                self.sessions.push(Session { id, host, path, editor: "your editor".into(), last: EditEvent::Uploading, local });
                None
            }
            EditReply::Failed { title, failure } => Some(error(title, failure.message)),
        }
    }
}

fn error(title: impl Into<String>, body: impl Into<String>) -> Notice {
    Notice::new(Tone::Error, title).body(body)
}

fn info(title: impl Into<String>, body: impl Into<String>) -> Notice {
    Notice::new(Tone::Info, title).body(body)
}

/// The leftovers' prompt: "2 files have changes that were never uploaded", and a line per
/// file (its server's name from `server_name`).
pub fn leftovers_prompt(list: &[Leftover], server_name: impl Fn(&str) -> String) -> Confirm {
    let n = list.len();
    let title = if n == 1 { "1 file has changes that were never uploaded".to_owned() } else { format!("{n} files have changes that were never uploaded") };
    let mut lines: Vec<String> = list.iter().take(super::ops::NAMED).map(|l| format!("{} on {} ({})", l.remote_path, server_name(&l.host), bytes(l.size))).collect();
    if n > super::ops::NAMED {
        lines.push(format!("and {} more", n - super::ops::NAMED));
    }
    Confirm {
        title,
        body: "SSHeesh closed before they reached the server. If one also changed there, you'll choose which to keep.".into(),
        lines,
        confirm_label: "Upload",
        danger: false,
    }
}

/// The conflict dialog's words for session `s`, and its four ways on, in order.
pub fn conflict_words(s: &Session) -> (String, String) {
    let name = crate::paths::name(&s.path);
    match s.last {
        EditEvent::Conflict { gone: true } => (format!("{name} isn't on the server anymore"), "It was deleted or moved there since you opened it. Keep mine puts your version back.".into()),
        _ => ("This file changed on the server since you opened it".into(), format!("Your saves of {name} wait on this computer until you choose. Nothing is uploaded meanwhile.")),
    }
}

/// An editor's name in a list: "The default text editor", not "the default text editor".
pub fn listed(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The conflict's choices: what each button says.
pub const KEEP_MINE: &str = "Keep mine";
pub const TAKE_THEIRS: &str = "Take theirs";
pub const SAVE_COPY: &str = "Save mine as a copy";
pub const COMPARE: &str = "Compare";

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(id: &str, name: &str, vscode: bool) -> Editor {
        Editor { id: id.into(), name: name.into(), program: PathBuf::from(format!("/usr/bin/{id}")), args: Vec::new(), vscode_like: vscode }
    }

    fn edits() -> Edits {
        Edits { found: vec![editor("code", "VS Code", true), editor("zed", "Zed", false), editor("system", "the default text editor", false)], ..Edits::default() }
    }

    const WEB: &str = "host:web";

    fn opened(e: &mut Edits, path: &str, id: &str) {
        e.apply(EditReply::Opened { tab: TabId(1), host: WEB.into(), path: path.into(), editor: "VS Code".into(), result: Ok(Opened::Session { id: id.into(), local: PathBuf::from("/tmp/e/1/x") }) });
    }

    #[test]
    fn the_editor_open_in_means_follows_the_setting() {
        let mut e = edits();
        assert_eq!(e.chosen(None).unwrap().name, "VS Code");
        assert_eq!(e.chosen(Some("zed")).unwrap().name, "Zed");
        // Gone since: the default.
        assert_eq!(e.chosen(Some("sublime")).unwrap().name, "VS Code");
        // Picked with "Choose an app…": once the backend checked it.
        assert_eq!(e.chosen(Some("custom:/opt/kate/bin/kate")).unwrap().name, "VS Code");
        e.apply(EditReply::Chosen(Ok(Editor { id: "custom:/opt/kate/bin/kate".into(), ..editor("kate", "kate", false) })));
        assert_eq!(e.chosen(Some("custom:/opt/kate/bin/kate")).unwrap().name, "kate");
        assert!(Edits::default().chosen(None).is_none());
    }

    #[test]
    fn opening_asks_once_when_tor_or_streaming_and_a_big_file_asks_first() {
        let mut e = edits();
        let code = e.found[0].clone();
        assert!(e.open(TabId(1), WEB, "/srv/a.log", code.clone(), true, false).is_none());
        let Some(Question::Warn { tor: true, streaming: false, .. }) = &e.question else { panic!("{:?}", e.question) };
        assert!(e.question.as_ref().unwrap().confirm().body.contains("not through Tor"));
        assert!(matches!(e.answer(true), Some(EditAsk::Open { confirmed: false, .. })));
        assert!(e.is_opening(WEB, "/srv/a.log"));
        // Once said yes, not again in this run.
        assert!(e.open(TabId(1), WEB, "/srv/b.log", code.clone(), true, true).is_some());
        // Over 50 MB: the question names the size; yes opens with `confirmed`.
        e.apply(EditReply::Opened { tab: TabId(1), host: WEB.into(), path: "/srv/b.log".into(), editor: "VS Code".into(), result: Ok(Opened::NeedsConfirm { size: 230 * 1024 * 1024 }) });
        assert!(!e.is_opening(WEB, "/srv/b.log"));
        assert_eq!(e.question.as_ref().unwrap().confirm().title, "This file is 230 MB. Download a copy to edit?");
        assert!(matches!(e.answer(true), Some(EditAsk::Open { confirmed: true, ref path, .. }) if path == "/srv/b.log"));
        // No goes nowhere.
        e.apply(EditReply::Opened { tab: TabId(1), host: WEB.into(), path: "/srv/c".into(), editor: "VS Code".into(), result: Ok(Opened::NeedsConfirm { size: 60 << 20 }) });
        assert_eq!(e.answer(false), None);
        assert!(e.question.is_none());
    }

    #[test]
    fn a_sessions_status_line_follows_its_events() {
        let mut e = edits();
        opened(&mut e, "/srv/app/.env", "s1");
        let status = |e: &Edits| e.session("s1").unwrap().status(3_600);
        assert_eq!(status(&e).line, "Open in VS Code");
        e.apply(EditReply::Event { id: "s1".into(), event: EditEvent::Uploading });
        assert_eq!(status(&e).mark, "Uploading…");
        // 11:41 UTC is 12:41 an hour east.
        e.apply(EditReply::Event { id: "s1".into(), event: EditEvent::Uploaded { at: 1_728_733_260 } });
        assert_eq!((status(&e).mark.as_str(), status(&e).line.as_str()), ("Uploaded 12:41", "Saved to the server at 12:41"));
        e.apply(EditReply::Event { id: "s1".into(), event: EditEvent::Offline { pending: true, retry_in_ms: 4_000, message: "no route".into() } });
        assert!(status(&e).retry && status(&e).mark == "Offline, will retry");
        e.apply(EditReply::Event { id: "s1".into(), event: EditEvent::Failed { message: "Permission denied".into() } });
        assert!(status(&e).retry && status(&e).error && status(&e).line == "Permission denied");
        assert_eq!(e.open_on(WEB, "/srv/app/.env").unwrap().id, "s1");
        assert_eq!(e.in_folder(WEB, "/srv/app").count(), 1);
        // A conflict brings its dialog up; any later news of that session takes it down.
        e.apply(EditReply::Event { id: "s1".into(), event: EditEvent::Conflict { gone: false } });
        assert_eq!(e.conflict.as_deref(), Some("s1"));
        assert!(status(&e).conflict && status(&e).waits && !status(&e).error);
        e.apply(EditReply::Compared { id: "s1".into(), result: Ok(Comparison { binary: false, lines: vec![(DiffKind::Mine, "a".into())] }) });
        assert!(e.compare.is_some());
        e.apply(EditReply::Event { id: "s1".into(), event: EditEvent::Reloaded { at: 1_728_733_260 } });
        assert!(e.conflict.is_none());
        // Closed with changes kept: a note says where, and the row still says so.
        let note = e.apply(EditReply::Event { id: "s1".into(), event: EditEvent::Closed { kept: Some(PathBuf::from("/tmp/e/1/.env")) } }).unwrap();
        assert_eq!(note.folder, Some(PathBuf::from("/tmp/e/1")));
        assert_eq!(note.tone, Tone::Info);
        assert!(e.open_on(WEB, "/srv/app/.env").is_none());
        assert_eq!(e.in_folder(WEB, "/srv/app").count(), 1);
        // Opened again: the old line goes.
        opened(&mut e, "/srv/app/.env", "s2");
        assert_eq!(e.sessions.len(), 1);
        // News of a session nobody knows changes nothing.
        assert!(e.apply(EditReply::Event { id: "nope".into(), event: EditEvent::Uploading }).is_none());
    }

    #[test]
    fn a_servers_last_tab_closing_closes_its_sessions() {
        let mut e = edits();
        assert!(e.hosts_closed(vec![WEB.into(), "local".into()]).is_empty());
        assert_eq!(e.hosts_closed(vec!["local".into()]), [EditAsk::CloseHost { host: WEB.into() }]);
        assert!(e.hosts_closed(vec!["local".into()]).is_empty());
    }

    #[test]
    fn the_leftovers_prompt_names_them() {
        let left = |n: usize| Leftover { id: format!("l{n}"), host: WEB.into(), remote_path: format!("/srv/app/{n}.conf"), local: PathBuf::from("/tmp/e"), size: 2048 };
        let q = leftovers_prompt(&[left(1), left(2)], |_| "web-7731".into());
        assert_eq!(q.title, "2 files have changes that were never uploaded");
        assert_eq!(q.lines, ["/srv/app/1.conf on web-7731 (2.0 KB)", "/srv/app/2.conf on web-7731 (2.0 KB)"]);
        assert_eq!(leftovers_prompt(&[left(1)], |_| "x".into()).title, "1 file has changes that were never uploaded");
        let many: Vec<Leftover> = (0..9).map(left).collect();
        assert_eq!(leftovers_prompt(&many, |_| "x".into()).lines.last().unwrap(), "and 3 more");
    }

    #[test]
    fn a_name_in_a_list_starts_with_a_capital() {
        assert_eq!(listed("the default text editor"), "The default text editor");
        assert_eq!(listed("VS Code"), "VS Code");
        assert_eq!(listed(""), "");
    }

    #[test]
    fn times_read_as_the_clock_here() {
        assert_eq!(clock_time(0, 0), "00:00");
        assert_eq!(clock_time(1_728_733_260, 0), "11:41");
        assert_eq!(clock_time(1_728_733_260, -5 * 3_600), "06:41");
        assert_eq!(clock_time(60, -3_600), "23:01");
    }
}
