//! The sidebar's buttons: a 32-point tile with one icon. The current server's has a soft
//! fill; an orange dot means an agent there waits for you; Tor's shows its state.

use rune::El;

use crate::icons::{icon, Icon};
use crate::kit::{progress_ring, ringed_dot};
use crate::theme::{self, Theme};

/// What a sidebar button shows beyond its icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RailState {
    #[default]
    Plain,
    /// The server on screen.
    Selected,
    /// Tor is on: green on a green-tinted tile.
    TorOn,
    /// Tor is starting: green, inside a ring on its way round.
    TorStarting,
    /// Tor didn't start: a small red square.
    TorFailed,
}

/// A sidebar button named `name` (its tooltip and what a screen reader says). `waiting`: the
/// orange "needs you" dot. The caller adds the key and `.on_press_tx(..)`.
pub fn rail_button(t: &Theme, glyph: Icon, name: &str, state: RailState, waiting: bool) -> El {
    let size = theme::RAIL_BUTTON;
    let ink = match state {
        RailState::Plain | RailState::TorFailed => t.l2,
        RailState::Selected => t.l1,
        RailState::TorOn | RailState::TorStarting => t.green,
    };
    let mut b = rune::el()
        .size(size, size)
        .radius(theme::RADIUS)
        .items_center()
        .justify_center()
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label(name)
        .help(name);
    b = match state {
        RailState::Selected => b.fill(t.selrail),
        RailState::TorOn => b.fill(t.tor_tile()),
        _ => b,
    };
    b = b.child(icon(glyph, theme::ICON_RAIL, ink));
    if state == RailState::TorStarting {
        b = b.child(progress_ring(size - 6.0, 1.5, t.green).absolute(3.0, 3.0));
    }
    if state == RailState::TorFailed {
        // A square, not a dot: it isn't "needs you".
        b = b.child(rune::el().absolute(size - 13.0, 2.0).size(11.0, 11.0).radius(4.0).fill(t.frame).items_center().justify_center().child(rune::el().size(7.0, 7.0).radius(2.0).fill(t.red)));
    }
    if waiting {
        b = b.child(ringed_dot(8.0, 2.0, t.orange_dot, t.frame).key("waits").absolute(size - 13.0, 1.0));
    }
    b
}
