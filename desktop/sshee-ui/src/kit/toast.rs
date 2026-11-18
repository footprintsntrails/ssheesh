//! The toast: a short note in the window's bottom right corner that leaves by itself ("Copied
//! the path", "Downloaded site.zip", "Couldn't rename"), 0.10's toasts of `Overlays.tsx`.
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ ⚠ Couldn't rename          [Undo]  × │  title; an error has the warning mark
//! │   Permission denied                  │  body, at most three lines
//! └──────────────────────────────────────┘
//! ```
//!
//! Only the card is here. The queue, how long each stays and the press handlers are the
//! window's (`shell::notes`): the kit knows nothing of the app's state.

use std::time::Duration;

use rune::{col, el, row, text, El, LiveRegion, Tx};

use crate::icons::{icon, Icon};
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::theme::{self, Theme};

/// The card's width: the transfer cards' it stacks with.
pub const WIDTH: f32 = 300.0;

/// What a toast is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Something to know ("Copied the path").
    #[default]
    Info,
    /// Something finished ("Downloaded site.zip").
    Success,
    /// Something failed: it stays longer, and has the warning mark.
    Error,
}

impl Tone {
    /// How long a toast stays: long enough to read, longer for an error or one with a button
    /// to press (0.10's times).
    pub fn stays(self, has_action: bool) -> Duration {
        match (self, has_action) {
            (Tone::Error, _) => Duration::from_millis(7000),
            (_, true) => Duration::from_millis(9000),
            _ => Duration::from_millis(3800),
        }
    }
}

/// What the card shows, already as it may be drawn (streamer mode applied).
pub struct Look<'a> {
    pub tone: Tone,
    pub title: &'a str,
    pub body: Option<&'a str>,
    /// The button beside the title ("Undo", "Show in folder").
    pub action: Option<&'a str>,
}

/// The small cross that dismisses a card.
fn cross(t: &Theme) -> El {
    el().size(20.0, 20.0)
        .mr(-4.0)
        .radius(5.0)
        .items_center()
        .justify_center()
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label_static("Dismiss")
        .help("Dismiss")
        .child(icon(Icon::Close, 14.0, t.l2on))
}

/// One toast's card, keyed `key` (its parts `<key>-action`, `<key>-close`). `act` runs on its
/// button, `close` on its cross.
pub fn toast<S: 'static>(t: &Theme, key: &str, look: &Look, act: impl Fn(&mut S, &mut Tx) + 'static, close: impl Fn(&mut S, &mut Tx) + 'static) -> El {
    let mut head = row().w_full().gap(6.0).items_center();
    if look.tone == Tone::Error {
        head = head.child(icon(Icon::Warning, 14.0, t.red));
    }
    head = head.child(text(look.title).font(theme::TAB_NAME).color(t.l1).single_line().truncate().grow(1.0).min_w(0.0));
    if let Some(label) = look.action {
        head = head.child(push_button(t, label, Push::Quiet, true).key(format!("{key}-action")).on_press_tx(act));
    }
    head = head.child(cross(t).key(format!("{key}-close")).on_press_tx(close));
    let mut card = crate::kit::motion::enters(popover(t, WIDTH), crate::kit::motion::Entrance::Toast).key(key).role("status").live_region(LiveRegion::Polite).px(12.0).py(9.0).gap(3.0).child(head);
    if let Some(body) = look.body.filter(|b| !b.is_empty()) {
        // Under the title's words, not under the mark.
        let indent = if look.tone == Tone::Error { 20.0 } else { 0.0 };
        card = card.child(col().w_full().pl(indent).child(text(body).font(theme::NOTE).color(t.l2).max_lines(3).w_full()));
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_and_actions_stay_longer() {
        assert!(Tone::Error.stays(false) > Tone::Info.stays(false));
        assert!(Tone::Success.stays(true) > Tone::Success.stays(false));
        assert_eq!(Tone::Info.stays(false), Tone::Success.stays(false));
    }
}
