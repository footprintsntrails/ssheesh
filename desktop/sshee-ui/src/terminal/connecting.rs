//! "Connecting to web-7731…": what a terminal on a server shows while its session starts and
//! nothing is on screen yet, instead of an empty terminal.
//!
//! ```text
//! term-box
//! └── connect-screen centred over the empty terminal: the logo at work, "Connecting to
//!                    web-7731…", and what the session is doing now, when that's known
//! ```
//!
//! It shows only on a terminal that has never printed anything, after a moment (a shared
//! connection is up in a blink and shouldn't flash it), and goes the moment the session
//! prints: ssh's password prompt, a banner, the shell. Once ssh says it's in, it stays a
//! little longer while tmux or the shell starts, then goes even if nothing came. A failure,
//! the server's key question and every other card are the session's own (`session.rs`); a
//! reconnect over text already on screen is the smaller "Reconnecting…" note there.
//!
//! The pane doesn't take the pointer or the keyboard away: the overlay has no handlers, so a
//! click goes to the terminal, and keys typed meanwhile go to ssh as before.

use std::time::Duration;

use rune::{col, text, El, Tx};

use super::pane::TermPane;
use super::LinkStatus;
use crate::theme::{self, Theme};

/// How long a session must be connecting before the overlay shows, in seconds.
pub const SHOW_AFTER: f64 = 0.2;

/// How long it stays after ssh is in while nothing is printed yet (tmux or the shell
/// starting), in seconds.
pub const STARTING_FOR: f64 = 2.0;

/// The second line while tmux starts on the server.
pub const STARTING_TMUX: &str = "Starting tmux";
/// The second line while the shell starts on a server without tmux.
pub const STARTING_SHELL: &str = "Starting the shell";

/// What a link knows its session is doing while it connects ([`super::TermLink::phase`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Looking the server's key up before signing in.
    Key,
    /// The connection goes through Tor, which takes a while.
    Tor,
}

impl Phase {
    /// The quiet second line.
    pub fn words(self) -> &'static str {
        match self {
            Phase::Key => "Checking the server's key",
            Phase::Tor => "Through Tor",
        }
    }

    /// A fixture's `phase` word (`replay.rs`).
    pub fn named(word: &str) -> Option<Phase> {
        match word {
            "key" => Some(Phase::Key),
            "tor" => Some(Phase::Tor),
            _ => None,
        }
    }
}

/// A pane's connecting state.
#[derive(Default)]
pub struct Connecting {
    /// When the session started connecting with nothing on screen (pane clock).
    since: Option<f64>,
    /// When ssh said it's in, after connecting with nothing on screen.
    in_at: Option<f64>,
    /// Whether the last frame drew it.
    drawn: bool,
}

/// What the overlay says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// "Connecting to web-7731…", the name as the window may show it (streamer mode).
    pub title: String,
    /// What the session is doing now, when that's known.
    pub phase: Option<&'static str>,
}

/// "Connecting to web-7731…"; "Connecting…" when the name is hidden or unknown.
pub fn title(name: &str) -> String {
    if name.trim().is_empty() {
        "Connecting…".to_owned()
    } else {
        format!("Connecting to {name}…")
    }
}

impl TermPane {
    /// What the overlay says now, if it shows.
    pub fn connecting(&self) -> Option<Shown> {
        let c = &self.over.connecting;
        let since = c.since?;
        if !self.over.session.on_server || self.last_output.is_some() || self.clock + 1e-6 < since + SHOW_AFTER {
            return None;
        }
        let phase = match self.status {
            LinkStatus::Connecting => self.link.phase().map(Phase::words),
            LinkStatus::Connected { durable } if c.in_at.is_some_and(|at| self.clock + 1e-6 < at + STARTING_FOR) => Some(if durable { STARTING_TMUX } else { STARTING_SHELL }),
            _ => return None,
        };
        Some(Shown { title: title(&self.over.session.server_name), phase })
    }

    /// The overlay's part of every poll: when it starts, when it ends. True when what is drawn
    /// changed.
    pub(crate) fn poll_connecting(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let printed = self.last_output.is_some();
        let c = &mut self.over.connecting;
        match self.status {
            LinkStatus::Connecting if !printed => {
                c.since.get_or_insert(clock);
                c.in_at = None;
            }
            LinkStatus::Connected { .. } if !printed && c.since.is_some() => {
                c.in_at.get_or_insert(clock);
            }
            _ => *c = Connecting { drawn: c.drawn, ..Connecting::default() },
        }
        let due = match (c.since, c.in_at) {
            (Some(_), Some(at)) => Some(at + STARTING_FOR),
            (Some(since), None) => Some(since + SHOW_AFTER),
            _ => None,
        };
        if let Some(due) = due.filter(|d| clock + 1e-6 < *d) {
            tx.wake_after(Duration::from_secs_f64(due - clock));
        }
        let shows = self.connecting().is_some();
        let changed = shows != self.over.connecting.drawn;
        self.over.connecting.drawn = shows;
        changed
    }
}

/// The overlay over `pane`'s terminal, while it shows.
pub fn view(pane: &TermPane, t: &Theme) -> Option<El> {
    let shown = pane.connecting()?;
    let mut words = col().key("connect-screen-words").gap(4.0).items_center().child(text(shown.title).key("connect-screen-title").font(theme::TEXT_BOLD).color(t.l1).single_line());
    if let Some(phase) = shown.phase {
        words = words.child(text(phase).key("connect-screen-phase").font(theme::NOTE).color(t.l2).single_line());
    }
    Some(
        col()
            .key("connect-screen")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .padding(24.0)
            .gap(14.0)
            .role("status")
            .child(crate::icons::logo_loader(40.0))
            .child(words),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_names_the_server_unless_it_is_hidden() {
        assert_eq!(title("web-7731"), "Connecting to web-7731…");
        assert_eq!(title(""), "Connecting…");
        assert_eq!(title("  "), "Connecting…");
    }

    #[test]
    fn phases_read_from_fixtures_and_say_what_happens() {
        assert_eq!(Phase::named("tor"), Some(Phase::Tor));
        assert_eq!(Phase::named("key").map(Phase::words), Some("Checking the server's key"));
        assert_eq!(Phase::named("password"), None);
        assert_eq!(Phase::Tor.words(), "Through Tor");
    }
}
