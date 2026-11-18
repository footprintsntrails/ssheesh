//! A pane's session as you see it: the cards over the text while it comes and goes (drawn by
//! `screens/prompts.rs`), the text dimmed while there is no connection, letting go of an
//! unused connection and waking it with a key, a command typed once a new terminal is
//! connected, a clicked path followed to Files, and the program's progress for its tab.
//!
//! Rules and words are `sshee_app::prompts`; 0.10's are `TerminalPane.tsx` (the cards) and
//! `terminals.ts` (`waking`, `checkIdle`, `pendingCommand`, `onProgress`), `terminalLinks.ts`
//! (finding a clicked path).
//!
//! Waking, as 0.10 does it: a key in a terminal that let go of its connection brings it back
//! and goes nowhere; keys typed before ssh has anything to say go nowhere either (ssh signing
//! in again would echo them: a password typed ahead, say). Once ssh speaks (it asks only for
//! a password or a passphrase there), or the connection is back, keys go through again.

use std::time::Duration;

use rune::Tx;
use sshee_app::agents::AgentKind;
use sshee_app::prompts::{self, HostKey};
use sshee_term::links::{resolve_path, LinkKind};
use sshee_term::osc::Progress;
use sshee_term::present::Link;

use super::overlays::{Ask, Looked, Note, Reply, Wish};
use super::pane::TermPane;
use super::LinkStatus;

/// How long a command for a new terminal waits after it connects (0.10's 150 ms): the shell
/// draws its prompt first.
const RUN_AFTER: f64 = 0.15;

/// What background work brings back to the pane's session (`Note::Session`).
#[derive(Debug)]
pub enum Step {
    /// The shell's folder, as tmux knows it, for the job `ticket`.
    Cwd { ticket: u64, cwd: Option<String> },
    /// What is at the path being followed.
    Looked { ticket: u64, looked: Option<Looked> },
    /// Where the server found a relative path, if it did.
    Found { ticket: u64, path: Option<String> },
    /// Whether the shell waits at its prompt, and its folder: for something to start here.
    PaneInfo { ticket: u64, info: Option<(bool, String)> },
    /// The agents the server has.
    Agents(Option<Vec<String>>),
    /// The shell's folder, as tmux knows it, for what starts here (`Launch`).
    Folder { ticket: u64, cwd: Option<String> },
}

/// A clicked path being followed to Files.
#[derive(Clone, Debug)]
struct Following {
    ticket: u64,
    /// As the output had it.
    target: String,
    relative: bool,
    lines: Option<(u32, u32)>,
    /// The folder it was looked for from.
    cwd: String,
    /// When it started (pane clock), for "Looking for…".
    since: f64,
    said_looking: bool,
    /// The server looked around for it already.
    searched: bool,
}

/// Something to start once the shell's state is known.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Launch {
    /// "New terminal here": a terminal in the same folder.
    Here,
    /// An agent: typed here when the shell waits at its prompt, else in a new terminal beside.
    Agent(&'static str),
    /// "Browse files in this folder": Files there.
    Files,
    /// Files dropped on the terminal: the upload's question starts at this folder.
    Upload,
}

/// A pane's session state.
#[derive(Default)]
pub struct Session {
    /// When the status last changed (pane clock).
    since: f64,
    /// "Back online" shows until then; `true`: the shell in tmux is where it was.
    back: Option<(f64, bool)>,
    /// Woken from asleep and not connected yet: keys go nowhere.
    pub(crate) waking: bool,
    /// When you last typed, looked or got output (pane clock): unused since then.
    last_active: Option<f64>,
    /// Typed once the session connects (`Wish::NewTerminalWith`), and when.
    pending: Option<String>,
    run_at: Option<f64>,
    following: Option<Following>,
    launching: Option<(u64, Launch)>,
    ticket: u64,
    /// What the program says of its progress (OSC 9;4), for its tab.
    pub(crate) progress: Option<Progress>,
    /// The pane's machine is a server, not this computer.
    pub(crate) on_server: bool,
    /// The server's name as the window may show it (streamer mode), for the cards.
    pub(crate) server_name: String,
    /// The agents the server has, once asked.
    pub(crate) agents: Option<Vec<AgentKind>>,
    agents_asked: bool,
    /// The countdown last drawn, to draw again when it changes.
    shown_seconds: Option<u64>,
    /// "Reconnecting…" is due (connecting again for longer than a moment).
    pub(crate) rejoin_shown: bool,
    /// Text was on screen when connecting began: connecting again ("Try again"), not a first
    /// connect, whose password prompt is output too.
    over_text: bool,
}

/// What the pane shows over its text about its session, if anything.
#[derive(Clone, Debug, PartialEq)]
pub enum Card {
    /// The server's key isn't the one remembered: trust it or not.
    HostKey(HostKey),
    /// Dropped, and reconnecting is yours to start (Settings: "Ask").
    Disconnected { reason: String },
    /// Let go of while unused.
    Asleep { minutes: f64 },
    /// Dropped, and coming back by itself: the reason and "Reconnecting in 3s · attempt 2".
    Retrying { reason: String, line: String },
    /// Connecting again, for longer than a moment.
    Rejoining,
    /// Back: "everything's where you left it", or a new shell.
    Back { durable: bool },
    /// It stopped: couldn't connect (why), or the shell ended.
    Stopped { title: &'static str, body: String, again: &'static str, review: bool, failed: bool },
}

impl TermPane {
    /// The session said where it is (`Shell::link_status`).
    pub fn set_status(&mut self, status: LinkStatus) {
        let was = std::mem::replace(&mut self.status, status);
        if was == self.status {
            return;
        }
        let clock = self.clock;
        let s = &mut self.over.session;
        s.since = clock;
        s.shown_seconds = None;
        s.rejoin_shown = false;
        s.over_text = self.last_output.is_some();
        // Waking ends when the session is anywhere but on its way back.
        if !matches!(self.status, LinkStatus::Connecting | LinkStatus::Rejoining { .. }) {
            s.waking = false;
        }
        let down = |st: &LinkStatus| st.offline() || matches!(st, LinkStatus::Asleep | LinkStatus::Rejoining { .. });
        match self.status {
            LinkStatus::Connected { durable } => {
                s.last_active = Some(clock);
                // (A new shell says where it begins on screen: the link's `reconnected_new_shell`.)
                if down(&was) {
                    s.back = Some((clock + prompts::BACK_ONLINE_FOR, durable));
                }
                if self.over.session.pending.is_some() {
                    self.over.session.run_at = Some(clock + RUN_AFTER);
                    self.refit_soon();
                }
            }
            _ => s.back = None,
        }
        // A program's progress belongs to the connection it came on.
        if !matches!(self.status, LinkStatus::Connected { .. }) {
            self.over.session.progress = None;
        }
    }

    /// The card over the text now, if any.
    pub fn card(&self) -> Option<Card> {
        let s = &self.over.session;
        match &self.status {
            LinkStatus::HostKey(question) => Some(Card::HostKey(question.clone())),
            LinkStatus::Waiting { note } if self.options.reconnect_card => Some(Card::Disconnected { reason: note.clone() }),
            LinkStatus::Asleep => Some(Card::Asleep { minutes: self.options.idle_sleep }),
            LinkStatus::Retrying { attempt, retry_in_ms, reason } => {
                let left = prompts::seconds_left(*retry_in_ms, self.clock - s.since);
                Some(Card::Retrying { reason: reason.clone(), line: prompts::reconnecting_line(left, *attempt) })
            }
            LinkStatus::Reconnecting { note } => Some(Card::Retrying { reason: "The connection dropped".into(), line: note_line(note) }),
            LinkStatus::Rejoining { .. } if s.rejoin_shown => Some(Card::Rejoining),
            // Connecting again over what's on screen ("Try again"), as a reconnect does. With
            // nothing on screen it's "Connecting to …" instead (`connecting.rs`).
            LinkStatus::Connecting if s.rejoin_shown && s.over_text => Some(Card::Rejoining),
            LinkStatus::Gone { note } => {
                let (title, body, again) = prompts::stopped(Some(note));
                Some(Card::Stopped { title, body, again, review: prompts::about_identity(note), failed: true })
            }
            LinkStatus::Ended { .. } => {
                let (title, body, again) = prompts::stopped(None);
                Some(Card::Stopped { title, body, again, review: false, failed: false })
            }
            LinkStatus::Connected { .. } => s.back.filter(|(until, _)| self.clock < *until).map(|(_, durable)| Card::Back { durable }),
            _ => None,
        }
    }

    /// How much the text shows (1: all of it): dimmed while there is no connection, less so
    /// asleep, which isn't trouble.
    pub fn text_opacity(&self) -> f32 {
        if self.status.offline() || self.status.stopped() {
            0.4
        } else if self.status == LinkStatus::Asleep {
            0.6
        } else {
            1.0
        }
    }

    /// The session's part of every poll: the countdown, "Reconnecting…" after a moment, "Back
    /// online" going, a command for a new terminal, "Looking for…", and letting go of an
    /// unused connection. True when what is drawn changed.
    pub(crate) fn poll_session(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let mut changed = false;
        // Once the shell has drawn.
        self.ask_agents();
        let since = self.over.session.since;
        match &self.status {
            LinkStatus::Retrying { retry_in_ms, .. } => {
                let left = prompts::seconds_left(*retry_in_ms, clock - since);
                if self.over.session.shown_seconds != Some(left) {
                    self.over.session.shown_seconds = Some(left);
                    changed = true;
                }
                if left > 0 {
                    tx.wake_after(Duration::from_secs_f64(0.25));
                }
            }
            // Connecting counts only over text already on screen: a first connect's password
            // prompt is output too, and "Reconnecting…" mustn't cover it.
            LinkStatus::Rejoining { .. } | LinkStatus::Connecting
                if !self.over.session.rejoin_shown && (self.over.session.over_text || matches!(self.status, LinkStatus::Rejoining { .. })) =>
            {
                let due = since + prompts::REJOIN_DELAY;
                if clock + 1e-6 >= due {
                    self.over.session.rejoin_shown = true;
                    changed = true;
                } else {
                    tx.wake_after(Duration::from_secs_f64(due - clock));
                }
            }
            _ => {}
        }
        if let Some((until, _)) = self.over.session.back {
            if clock + 1e-6 >= until {
                self.over.session.back = None;
                changed = true;
            } else {
                tx.wake_after(Duration::from_secs_f64(until - clock));
            }
        }
        if let Some(at) = self.over.session.run_at {
            if clock + 1e-6 >= at {
                self.over.session.run_at = None;
                if let Some(command) = self.over.session.pending.take() {
                    let out = self.model.user_input_at(format!("{command}\r").as_bytes(), self.now());
                    self.link.write(&out);
                }
            } else {
                tx.wake_after(Duration::from_secs_f64(at - clock));
            }
        }
        if let Some(f) = self.over.session.following.as_mut().filter(|f| !f.said_looking) {
            let due = f.since + prompts::LOOKING_AFTER;
            if clock + 1e-6 >= due {
                f.said_looking = true;
                let note = prompts::looking_for(&f.target);
                self.say_for(&note, 30.0, tx);
                changed = true;
            } else {
                tx.wake_after(Duration::from_secs_f64(due - clock));
            }
        }
        changed |= self.check_idle(clock, tx);
        changed
    }

    /// Lets go of the connection when the pane has been unused for the time Settings says.
    fn check_idle(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let minutes = self.options.idle_sleep;
        let durable = matches!(self.status, LinkStatus::Connected { durable: true });
        let Some(last) = self.over.session.last_active.filter(|_| minutes > 0.0 && durable && self.over.session.on_server) else { return false };
        // Output keeps it awake: the newest output on screen counts as use.
        let idle = clock - last;
        if prompts::sleeps_now(true, idle, minutes) {
            // Asked once; if there was nothing to let go of, the next try is a while off.
            self.over.session.last_active = Some(clock);
            self.link.sleep();
            return false;
        }
        tx.wake_after(Duration::from_secs_f64((minutes * 60.0 - idle).max(1.0)));
        false
    }

    /// The pane was used: a key, output, or it came to the front.
    pub(crate) fn note_active(&mut self) {
        self.over.session.last_active = Some(self.clock);
    }

    /// Output reached the screen: ssh, waking, asks only for a password or a passphrase, and
    /// the keys are for that now.
    pub(crate) fn note_output(&mut self) {
        self.over.session.waking = false;
        self.note_active();
    }

    /// Whether bytes typed now may go to the session. A key in a pane that let go of its
    /// connection brings it back and goes nowhere; so do keys typed while it wakes.
    pub(crate) fn may_send(&mut self, bytes: &[u8]) -> bool {
        if self.status == LinkStatus::Asleep {
            if sshee_term::input::is_typing(bytes) {
                self.wake_up();
            }
            return false;
        }
        if self.over.session.waking {
            return false;
        }
        self.note_active();
        true
    }

    /// Brings back a pane that let go of its connection (a key, its card's Reconnect).
    pub fn wake_up(&mut self) {
        if self.status == LinkStatus::Asleep {
            self.over.session.waking = true;
        }
        self.link.reconnect();
    }

    /// The card's answer to the server's key question.
    pub fn answer_host_key(&mut self, trust: bool, tx: &mut Tx) {
        if !matches!(self.status, LinkStatus::HostKey(_)) {
            return;
        }
        self.link.trust(trust);
        if trust {
            self.over.wish(Wish::Trusted);
        }
        self.focus_terminal(tx);
        tx.instant();
    }

    /// Starts the session again after it stopped ("Try again", "Start again", "Review the
    /// change"), or a server waiting on a key someone else trusted.
    pub fn restart(&mut self) {
        let review = matches!(&self.status, LinkStatus::Gone { note } if prompts::about_identity(note));
        self.over.session.back = None;
        self.link.restart(review);
    }

    /// Connects again now: a click on a tab whose connection is gone, the card's buttons.
    pub fn reconnect(&mut self) {
        match self.status {
            LinkStatus::Asleep => self.wake_up(),
            _ if self.status.stopped() => self.restart(),
            _ => self.link.reconnect(),
        }
    }

    /// A key while a card waits for an answer: Return gives the card's default answer, Escape
    /// the safe one. True: the key was the card's.
    pub(crate) fn card_key(&mut self, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
        use rune::Key;
        match (&self.status, &ev.key) {
            (LinkStatus::HostKey(q), Key::Enter) => {
                // The safe answer is the default when the key changed.
                let trust = !q.changed();
                self.answer_host_key(trust, tx);
                true
            }
            (LinkStatus::HostKey(_), Key::Escape) => {
                self.answer_host_key(false, tx);
                true
            }
            // Nothing connects, nothing to type into.
            (LinkStatus::HostKey(_), _) => true,
            (_, Key::Enter) if self.status.stopped() => {
                self.restart();
                tx.instant();
                true
            }
            _ => false,
        }
    }

    /// A command to type once the session is connected: a new terminal's `cd`, an agent.
    pub fn run_when_connected(&mut self, command: String) {
        self.over.session.pending = Some(command);
        if matches!(self.status, LinkStatus::Connected { .. }) {
            self.over.session.run_at = Some(self.clock + RUN_AFTER);
            self.refit_soon();
        }
    }

    /// The pane's machine is a server (`true`) or this computer, and its name as the window
    /// shows it.
    pub fn set_machine(&mut self, on_server: bool, name: &str) {
        self.over.session.on_server = on_server;
        self.over.session.server_name = name.to_owned();
    }

    /// What the program in the terminal says of its progress (OSC 9;4), for its tab.
    pub fn progress(&self) -> Option<Progress> {
        self.over.session.progress
    }

    fn next_ticket(&mut self) -> u64 {
        self.over.session.ticket += 1;
        self.over.session.ticket
    }

    // ---- a clicked path

    /// A click on a file's path: found on its machine (a relative one from the shell's
    /// folder, which tmux knows when OSC 7 doesn't come through, then where the server finds
    /// it), and shown in Files beside the terminal; or a note that it isn't there.
    pub(crate) fn follow_path(&mut self, link: &Link, tx: &mut Tx) {
        debug_assert_eq!(link.kind, LinkKind::Path);
        let relative = !(link.target.starts_with('/') || link.target.starts_with('~'));
        let lines = link.line.map(|first| (first, link.end_line.unwrap_or(first).max(first)));
        let ticket = self.next_ticket();
        let cwd = self.over.links.cwd.clone().unwrap_or_else(|| "~".into());
        self.over.session.following =
            Some(Following { ticket, target: link.target.clone(), relative, lines, cwd, since: self.clock, said_looking: false, searched: false });
        tx.wake_after(Duration::from_secs_f64(prompts::LOOKING_AFTER));
        // In tmux the shell's folder is tmux's to say.
        if relative && self.over.session.on_server && self.ask(Ask::Cwd, move |reply| Note::Session(Step::Cwd { ticket, cwd: cwd_of(reply) })) {
            return;
        }
        self.look_for_followed();
    }

    /// Looks at the followed path from the folder it is followed from.
    fn look_for_followed(&mut self) {
        let Some(f) = self.over.session.following.clone() else { return };
        let path = resolve_path(&f.target, &f.cwd);
        if path == "~" || path == "/" {
            return self.found_followed(path, None);
        }
        let ticket = f.ticket;
        let asked = self.ask(Ask::Look { path: path.clone() }, move |reply| Note::Session(Step::Looked { ticket, looked: looked_of(reply) }));
        if !asked {
            // Nobody to ask (a replay): Files shows the folder it is in and picks it out.
            let (dir, name) = split(&path);
            self.found_followed(dir, Some(name));
        }
    }

    fn found_followed(&mut self, dir: String, name: Option<String>) {
        let Some(f) = self.over.session.following.take() else { return };
        self.end_looking_note(&f);
        let lines = if name.is_some() { f.lines } else { None };
        self.over.wish(Wish::ShowPath { dir, name, lines });
    }

    /// The "Looking for…" note goes with the search it was for.
    fn end_looking_note(&mut self, f: &Following) {
        if f.said_looking && self.over.links.note.as_ref().is_some_and(|(n, _)| *n == prompts::looking_for(&f.target)) {
            self.over.links.note = None;
        }
    }

    fn followed_failed(&mut self, note: String, tx: &mut Tx) {
        let Some(f) = self.over.session.following.take() else { return };
        self.end_looking_note(&f);
        self.say_for(&note, 4.0, tx);
    }

    /// What background work brought back (`Note::Session`).
    pub(crate) fn session_step(&mut self, step: Step, tx: &mut Tx) {
        let current = |s: &Session, ticket: u64| s.following.as_ref().is_some_and(|f| f.ticket == ticket);
        match step {
            Step::Cwd { ticket, cwd } if current(&self.over.session, ticket) => {
                if let (Some(cwd), Some(f)) = (cwd, self.over.session.following.as_mut()) {
                    f.cwd = cwd;
                }
                self.look_for_followed();
            }
            Step::Looked { ticket, looked } if current(&self.over.session, ticket) => match looked {
                Some(Looked::Found { dir, name, is_dir }) => {
                    if is_dir {
                        self.found_followed(sshee_app::paths::join(&dir, &name), None);
                    } else {
                        self.found_followed(dir, Some(name));
                    }
                }
                Some(Looked::Failed(message)) => {
                    let target = self.over.session.following.as_ref().map(|f| f.target.clone()).unwrap_or_default();
                    self.followed_failed(format!("Can't open {target}: {message}"), tx);
                }
                Some(Looked::Missing) | None => {
                    let Some(f) = self.over.session.following.clone() else { return };
                    // Not in the terminal's folder: a program (an agent, say) may have printed
                    // it from its own, so the server looks around for it.
                    if f.relative && !f.searched && self.over.session.on_server {
                        if let Some(g) = self.over.session.following.as_mut() {
                            g.searched = true;
                        }
                        let (path, cwd) = (f.target.clone(), f.cwd.clone());
                        if self.ask(Ask::FindPath { path, cwd }, move |reply| Note::Session(Step::Found { ticket, path: found_of(reply) })) {
                            return;
                        }
                    }
                    self.followed_failed(prompts::not_there(&f.target, f.relative), tx);
                }
            },
            Step::Found { ticket, path } if current(&self.over.session, ticket) => match path {
                Some(path) => {
                    if let Some(f) = self.over.session.following.as_mut() {
                        f.cwd = sshee_app::paths::parent(&path);
                        f.target = sshee_app::paths::name(&path).to_owned();
                    }
                    self.look_for_followed();
                }
                None => {
                    let Some(f) = self.over.session.following.clone() else { return };
                    self.followed_failed(prompts::not_there(&f.target, f.relative), tx);
                }
            },
            Step::PaneInfo { ticket, info } => self.launch_with(ticket, info, tx),
            Step::Folder { ticket, cwd } => self.launch_with(ticket, cwd.map(|cwd| (false, cwd)), tx),
            Step::Agents(found) => {
                let found = found.unwrap_or_default();
                self.over.session.agents = Some(AgentKind::ALL.into_iter().filter(|a| found.iter().any(|f| f == a.command())).collect());
            }
            _ => {}
        }
        tx.instant();
    }

    // ---- starting something here

    /// "New terminal here": another terminal on this machine, in the folder this shell is in.
    pub fn new_terminal_here(&mut self, tx: &mut Tx) {
        self.start(Launch::Here, tx);
    }

    /// A press on an agent's button: the agent starts in this terminal when its shell waits at
    /// its prompt, else in a new terminal beside it, in the same folder, so whatever runs here
    /// keeps going (0.10's `startAgent`).
    pub fn start_agent(&mut self, agent: AgentKind, tx: &mut Tx) {
        self.start(Launch::Agent(agent.command()), tx);
    }

    /// "Browse files in this folder": Files on this machine, at the folder this shell is in
    /// (tmux says, on a server; else what the shell said, OSC 7).
    pub fn browse_files_here(&mut self, tx: &mut Tx) {
        self.start(Launch::Files, tx);
    }

    /// Files from another app were dropped on this server's terminal: the folder its shell is
    /// in comes back as [`Wish::UploadFolder`], for the upload's question.
    pub fn find_upload_folder(&mut self, tx: &mut Tx) {
        self.start(Launch::Upload, tx);
    }

    fn start(&mut self, launch: Launch, tx: &mut Tx) {
        let ticket = self.next_ticket();
        let folder_only = matches!(launch, Launch::Files | Launch::Upload);
        self.over.session.launching = Some((ticket, launch));
        if self.over.session.on_server && folder_only && self.ask(Ask::Cwd, move |reply| Note::Session(Step::Folder { ticket, cwd: cwd_of(reply) })) {
            return;
        }
        if self.over.session.on_server && !folder_only && self.ask(Ask::PaneInfo, move |reply| Note::Session(Step::PaneInfo { ticket, info: info_of(reply) })) {
            return;
        }
        // No tmux to ask: the folder the shell said (OSC 7), and no telling whether it's busy.
        let info = self.over.links.cwd.clone().map(|cwd| (false, cwd));
        self.launch_with(ticket, info, tx);
    }

    fn launch_with(&mut self, ticket: u64, info: Option<(bool, String)>, tx: &mut Tx) {
        let Some((t, launch)) = self.over.session.launching.take() else { return };
        if t != ticket {
            self.over.session.launching = Some((t, launch));
            return;
        }
        let shell = prompts::Shell::of(self.over.session.on_server, cfg!(windows));
        match (launch, info) {
            (Launch::Files, info) => self.over.wish(Wish::BrowseFiles(info.map(|(_, cwd)| cwd))),
            (Launch::Upload, info) => self.over.wish(Wish::UploadFolder(info.map(|(_, cwd)| cwd))),
            // Waiting at its prompt: typed here. Ctrl+E Ctrl+U first: anything half typed goes.
            (Launch::Agent(command), Some((true, _))) => {
                let out = self.model.user_input_at(format!("\x05\x15{command}\r").as_bytes(), self.now());
                self.link.write(&out);
                self.note_entered();
                self.focus_terminal(tx);
            }
            (launch, Some((_, cwd))) => {
                let next = match launch {
                    Launch::Agent(command) => Some(command),
                    Launch::Here | Launch::Files | Launch::Upload => None,
                };
                match prompts::typed_cd(&cwd, shell, next) {
                    Some(command) => self.over.wish(Wish::NewTerminalWith { command, beside: next.is_some() }),
                    None => self.say_for(prompts::UNTYPABLE_FOLDER, 4.0, tx),
                }
            }
            // Busy, or no way to tell, and no folder known: the agent gets a terminal of its
            // own, where it starts in the shell's first folder.
            (Launch::Agent(command), None) => self.over.wish(Wish::NewTerminalWith { command: command.to_owned(), beside: true }),
            (Launch::Here, None) => self.over.wish(Wish::NewTerminal),
        }
    }

    /// Asks the server once which agents it has, when its terminal is connected and its shell
    /// has drawn: the shell's first screen doesn't wait behind the question (on Windows, a
    /// sign-in of its own).
    pub(crate) fn ask_agents(&mut self) {
        let s = &self.over.session;
        if s.agents_asked || !s.on_server || !matches!(self.status, LinkStatus::Connected { .. }) || self.last_output.is_none() {
            return;
        }
        self.over.session.agents_asked = true;
        if !self.ask(Ask::Agents, |reply| Note::Session(Step::Agents(agents_of(reply)))) {
            self.over.session.agents = Some(Vec::new());
        }
    }
}

/// The old one-word status's line under "The connection dropped".
fn note_line(note: &str) -> String {
    let mut chars = note.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => "Reconnecting…".into(),
    }
}

/// `path` as its folder and its name (`~/x` is in `~`).
fn split(path: &str) -> (String, String) {
    (sshee_app::paths::parent(path), sshee_app::paths::name(path).to_owned())
}

fn cwd_of(reply: Option<Reply>) -> Option<String> {
    match reply {
        Some(Reply::Cwd(cwd)) if !cwd.is_empty() => Some(cwd),
        _ => None,
    }
}

fn looked_of(reply: Option<Reply>) -> Option<Looked> {
    match reply {
        Some(Reply::Looked(l)) => Some(l),
        _ => None,
    }
}

fn found_of(reply: Option<Reply>) -> Option<String> {
    match reply {
        Some(Reply::Found(p)) if !p.is_empty() => Some(p),
        _ => None,
    }
}

fn info_of(reply: Option<Reply>) -> Option<(bool, String)> {
    match reply {
        Some(Reply::PaneInfo { idle, cwd }) => Some((idle, cwd)),
        _ => None,
    }
}

fn agents_of(reply: Option<Reply>) -> Option<Vec<String>> {
    match reply {
        Some(Reply::Agents(list)) => Some(list),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> TermPane {
        let status: crate::terminal::StatusSink = std::sync::Arc::new(|_| {});
        TermPane::new(Box::new(crate::terminal::NoLink), status, crate::terminal::PaneOptions::default())
    }

    fn after_a_while(pane: &mut TermPane) -> Option<Card> {
        let clock = pane.clock + prompts::REJOIN_DELAY + 1.0;
        pane.clock = clock;
        pane.poll_session(clock, &mut Tx::default());
        pane.card()
    }

    /// A first connect prints ssh's password prompt while still connecting: no "Reconnecting…"
    /// over it.
    #[test]
    fn a_first_connects_password_prompt_is_not_covered() {
        let mut pane = pane();
        pane.set_status(LinkStatus::Connecting);
        pane.last_output = Some(std::time::Instant::now());
        assert_eq!(after_a_while(&mut pane), None);
        assert!(!pane.over.session.rejoin_shown);
    }

    /// "Try again" over a terminal with text: connecting again reads as reconnecting.
    #[test]
    fn connecting_again_over_text_says_reconnecting() {
        let mut pane = pane();
        pane.last_output = Some(std::time::Instant::now());
        pane.set_status(LinkStatus::Connecting);
        assert_eq!(after_a_while(&mut pane), Some(Card::Rejoining));
    }

    #[test]
    fn an_old_status_word_reads_as_a_line() {
        assert_eq!(note_line("reconnecting (try 2)"), "Reconnecting (try 2)");
        assert_eq!(note_line(""), "Reconnecting…");
    }

    #[test]
    fn a_path_is_split_into_its_folder_and_name() {
        assert_eq!(split("/var/log/app.log"), ("/var/log".into(), "app.log".into()));
        assert_eq!(split("~/notes.md"), ("~".into(), "notes.md".into()));
    }
}
