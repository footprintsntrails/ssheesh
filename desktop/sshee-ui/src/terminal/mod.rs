//! A terminal pane on Rune: sshee-term's model drawn as a `rune::grid`, fed by a session the
//! pane reaches only through [`TermLink`] (so screens can be tested with a fake one).
//!
//! ```text
//! reader thread:  sink(Output) ─ model.feed ─ Coalesce::wake ─ (Rune wakes the window)
//! UI thread:      poll: rearm ─ model.due ─ frame ─ PaneView::intake ─ events (replies → link)
//!                 view: metrics at this scale ─ presentation pass ─ rows (cached) ─ grid + cursor
//!                 handlers: keys → model.user_input → link.write; drag → selection; click → reveal
//! ```

pub mod coverage;
pub mod glyphs;
pub mod ime;
pub mod keys;
pub mod metrics;
pub mod palette;
pub mod pane;
pub mod rows;
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
    /// Reconnecting, or asleep: it comes back.
    Waiting { note: String },
    /// Failed or ended: it doesn't.
    Gone { note: String },
}

impl LinkStatus {
    /// The short note beside the session's name.
    pub fn note(&self) -> &str {
        match self {
            LinkStatus::Idle => "no session yet",
            LinkStatus::Connecting => "connecting",
            LinkStatus::Connected { .. } => "",
            LinkStatus::Waiting { note } | LinkStatus::Gone { note } => note,
        }
    }

    /// The session is gone or waiting: a key should bring it back.
    pub fn wants_reconnect(&self) -> bool {
        matches!(self, LinkStatus::Waiting { .. })
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
    /// The window closes.
    fn close(&self);
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
    /// The pane copied `text` to the clipboard. A replay records it (its fixtures are made
    /// up); a live session ignores it.
    fn note_copy(&self, text: &str) {
        let _ = text;
    }
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
