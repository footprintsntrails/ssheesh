//! A terminal pane on Rune: sshee-term's model drawn as a `rune::grid`, fed by a session the
//! pane reaches only through [`TermLink`] (so screens can be tested with a fake one).
//!
//! ```text
//! reader thread:  sink(Output) ─ model.feed ─ Coalesce::wake ─ (Rune wakes the window)
//! UI thread:      poll: rearm ─ model.due ─ frame ─ PaneView::intake ─ events (replies → link)
//!                 view: metrics at this scale ─ presentation pass ─ rows (cached) ─ grid + cursor
//!                 handlers: keys → model.user_input → link.write; drag → selection; click → reveal
//! ```

pub mod away;
pub mod connecting;
pub mod coverage;
pub mod echo;
pub mod find;
pub mod glyphs;
pub mod ime;
pub mod keys;
pub mod links;
pub mod menu;
pub mod metrics;
pub mod overlays;
pub mod palette;
pub mod password;
pub mod pane;
pub mod replay;
pub mod rows;
pub mod scrollbar;
pub mod session;
pub mod view;
pub mod wake;

use std::sync::Arc;
use std::time::Instant;

use sshee_term::engine::Size;
use sshee_term::model::Model;

pub use pane::{PaneOptions, TermPane};

/// Where the session is, as the title row shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LinkStatus {
    /// No session yet.
    #[default]
    Idle,
    Connecting,
    /// `durable`: the shell lives in tmux on the server.
    Connected { durable: bool },
    /// The connection dropped and is being brought back by itself.
    Reconnecting { note: String },
    /// Disconnected or asleep: a key or a click brings it back.
    Waiting { note: String },
    /// Failed or ended: it doesn't. `note` says why (sshee-core's words for a failure).
    Gone { note: String },
    /// The connection dropped and comes back by itself: try `attempt` in `retry_in_ms`,
    /// because of `reason` (a live session's [`LinkStatus::Reconnecting`], with its countdown).
    Retrying { attempt: u32, retry_in_ms: u64, reason: String },
    /// Let go of while unused (Settings' "Disconnect unused terminals"): the shell keeps
    /// running in tmux on the server, and any key brings it back.
    Asleep,
    /// Connecting again after a drop or a sleep, try `attempt`.
    Rejoining { attempt: u32 },
    /// The shell exited (`code`): a key doesn't bring it back, "Start again" does.
    Ended { code: Option<u32> },
    /// The server's key isn't the one remembered: nothing signs in until you answer
    /// (`TermLink::trust`). Askpass is never asked this.
    HostKey(sshee_app::prompts::HostKey),
}

impl LinkStatus {
    /// The short note beside the session's name.
    pub fn note(&self) -> &str {
        match self {
            LinkStatus::Idle => "no session yet",
            LinkStatus::Connecting => "connecting",
            LinkStatus::Connected { .. } => "",
            LinkStatus::Reconnecting { note } | LinkStatus::Waiting { note } | LinkStatus::Gone { note } => note,
            LinkStatus::Retrying { .. } | LinkStatus::Rejoining { .. } => "reconnecting",
            LinkStatus::Asleep => "asleep",
            LinkStatus::Ended { .. } => "ended",
            LinkStatus::HostKey(_) => "waiting for your answer",
        }
    }

    /// The session waits to come back: a key should bring it back now.
    pub fn wants_reconnect(&self) -> bool {
        matches!(self, LinkStatus::Reconnecting { .. } | LinkStatus::Waiting { .. } | LinkStatus::Retrying { .. } | LinkStatus::Asleep)
    }

    /// No connection now, after there was one: the text dims (0.10's `offline`).
    pub fn offline(&self) -> bool {
        matches!(self, LinkStatus::Reconnecting { .. } | LinkStatus::Waiting { .. } | LinkStatus::Retrying { .. } | LinkStatus::Rejoining { .. })
    }

    /// It stopped, and only a press starts it again.
    pub fn stopped(&self) -> bool {
        matches!(self, LinkStatus::Gone { .. } | LinkStatus::Ended { .. })
    }
}

/// Where a session tells its status, from any thread.
pub type StatusSink = Arc<dyn Fn(LinkStatus) + Send + Sync>;

/// A pane's session, as the pane sees it: sshee-core's sessions in the program, a replay or a
/// fake in tests. Called on the UI thread.
pub trait TermLink {
    /// Starts the session at `size`. Its output goes into `model` (parsed on the session's own
    /// thread, which wakes the window through the model's waker), its status to `status`.
    fn open(&mut self, model: Arc<Model>, size: Size, status: StatusSink);
    /// Keys, pastes, reports and the terminal's answers, in order.
    fn write(&self, bytes: &[u8]);
    /// The terminal's new size (the PTY, ssh's window, tmux).
    fn resize(&self, size: Size);
    /// Connect again now (a key in a pane that dropped or sleeps).
    fn reconnect(&self);
    /// The window closes: the connection goes, and a shell in tmux on the server stays to
    /// come back to.
    fn close(&self);
    /// The tab was closed on purpose: the connection goes and so does its shell on the server.
    /// A link with nothing on a server just closes.
    fn end(&self) {
        self.close();
    }
    /// The start of each frame, on the pane's clock (`clock` seconds, `now` as an `Instant`):
    /// a replay feeds the output that is due. Returns the clock at which it has more, so the
    /// window wakes then. A live session's output comes on its own thread: nothing to do.
    fn tick(&mut self, clock: f64, now: Instant) -> Option<f64> {
        let _ = (clock, now);
        None
    }
    /// All the output the session has had so far, in bytes (for the meter).
    fn bytes_received(&self) -> u64 {
        0
    }
    /// What the session is doing while it connects, when the link knows: the quiet second
    /// line under "Connecting to web-7731…" (`connecting.rs`).
    fn phase(&self) -> Option<connecting::Phase> {
        None
    }
    /// ssh asks, on the line it is on now, for this server's own password, as the link read
    /// the session's output since this connection attempt began: which such prompt it is,
    /// counting from 1 (a higher one after an answer means the server refused it). `None`
    /// otherwise: no prompt, a jump host on the way, a prompt worded otherwise or naming
    /// another account, or other output already printed (a login script's). The window
    /// offers to save the password only then (`password.rs`); a link that can't tell says
    /// `None`, and nothing is offered.
    fn password_prompt(&self) -> Option<u32> {
        None
    }
    /// The pane copied `text` to the clipboard. A replay records it (its fixtures are made
    /// up); a live session ignores it.
    fn note_copy(&self, text: &str) {
        let _ = text;
    }
    /// Asks the server for something of this terminal's tmux session, when it has one: its
    /// rows for a find, its history for "while you were away", or a scroll. The answer comes
    /// through `done` from any thread, `None` when it couldn't be had. Returns false, and
    /// never calls `done`, when the history is the terminal's own (no tmux): the pane then
    /// reads its own grid.
    fn ask(&self, ask: overlays::Ask, done: overlays::Answer) -> bool {
        let _ = (ask, done);
        false
    }
    /// Your answer to the server's key question ([`LinkStatus::HostKey`]): `true` saves the
    /// key you were shown and signs in, `false` doesn't connect.
    fn trust(&self, yes: bool) {
        let _ = yes;
    }
    /// Starts the session again after it stopped ("Try again", "Start again"); `review`:
    /// look at the server's key again first, as when ssh said it changed.
    fn restart(&self, review: bool) {
        let _ = review;
        self.reconnect();
    }
    /// Lets go of the connection of a terminal nobody used for a while: the shell keeps
    /// running in tmux on the server, and the session waits, [`LinkStatus::Asleep`], for
    /// [`TermLink::reconnect`]. A link with nothing on a server does nothing.
    fn sleep(&self) {}
}

/// A link to nothing: what a pane has before the program gives it one.
pub struct NoLink;

impl TermLink for NoLink {
    fn open(&mut self, _: Arc<Model>, _: Size, _: StatusSink) {}
    fn write(&self, _: &[u8]) {}
    fn resize(&self, _: Size) {}
    fn reconnect(&self) {}
    fn close(&self) {}
}
