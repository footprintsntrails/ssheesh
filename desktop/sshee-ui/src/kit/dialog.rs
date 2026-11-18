//! The confirmation: a question in the middle of the window, over a dimmed page, that waits
//! for an answer (0.10's `ConfirmDialog` of `Overlays.tsx`). "Delete “deploy.sh”?", what it
//! means in a sentence or two, what it's about one per line, and its buttons at the right.
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ Delete “deploy.sh”?                          │  title
//! │ It's deleted on the server right away. …     │  body
//! │ ┌──────────────────────────────────────────┐ │
//! │ │ deploy.sh                                │ │  lines, when there are names to list
//! │ └──────────────────────────────────────────┘ │
//! │                          [Cancel] [Delete]   │  the last one is the answer asked for
//! └──────────────────────────────────────────────┘
//! ```
//!
//! Keys: Return presses the default button, Esc picks the cancel one, Tab moves between the
//! buttons, and nothing behind it takes a key or a click (Rune's `El::modal`). A question about
//! something that can't be undone makes Cancel the default, so a Return meant for the terminal
//! doesn't delete. A button that takes something away has red words on a quiet surface: blue
//! stays for the button Return presses.
//!
//! Only the card is here. Which question is open, and what an answer does, are the window's
//! (`shell::notes` for `Shell::confirm`, or a screen's own state, as Files' questions).

use rune::{col, el, row, spacer, text, El, Tx};

use crate::kit::motion::{enters, Entrance};
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::theme::{self, Theme};

/// The card's width.
pub const WIDTH: f32 = 400.0;

/// One of the dialog's buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    /// Its element's key (tests and scripts press it by this).
    pub key: String,
    /// It takes something away: red words.
    pub destructive: bool,
}

/// What the dialog shows, already as it may be drawn (streamer mode applied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub title: String,
    pub body: String,
    /// What it's about, one per line, under the body.
    pub lines: Vec<String>,
    /// Left to right; the last is the answer asked for ("Delete").
    pub choices: Vec<Choice>,
    /// The one Return presses; `None`: Return does nothing.
    pub default: Option<usize>,
    /// The one Esc picks.
    pub cancel: usize,
}

impl Look {
    /// "Cancel" and `yes`. Return presses `yes`, unless it's `destructive` and `safe_default`
    /// (what can't be undone defaults to Cancel).
    pub fn yes_no(title: impl Into<String>, body: impl Into<String>, yes: &str, destructive: bool, safe_default: bool) -> Look {
        Look {
            title: title.into(),
            body: body.into(),
            lines: Vec::new(),
            choices: vec![
                Choice { label: "Cancel".into(), key: "dialog-no".into(), destructive: false },
                Choice { label: yes.into(), key: "dialog-yes".into(), destructive },
            ],
            default: Some(if destructive && safe_default { 0 } else { 1 }),
            cancel: 0,
        }
    }

    pub fn lines(mut self, lines: Vec<String>) -> Look {
        self.lines = lines;
        self
    }

    /// The buttons' keys, as `<prefix>-no`, `<prefix>-yes` (a screen with tests of its own).
    pub fn keys(mut self, no: &str, yes: &str) -> Look {
        if let [a, b] = &mut self.choices[..] {
            a.key = no.into();
            b.key = yes.into();
        }
        self
    }
}

/// The button's look: blue for the one Return presses (unless it takes something away), red
/// words for one that takes something away, quiet for the rest.
fn kind(choice: &Choice, default: bool) -> Push {
    match (choice.destructive, default) {
        (true, _) => Push::Caution,
        (false, true) => Push::Primary,
        (false, false) => Push::Quiet,
    }
}

/// The card alone, keyed `key`. `pick` gets the index of the button pressed (Esc: the cancel
/// one).
pub fn card<S: 'static>(t: &Theme, key: &str, look: &Look, pick: impl Fn(&mut S, &mut Tx, usize) + Clone + 'static) -> El {
    let mut card = popover(t, WIDTH).key(key).padding(18.0).gap(8.0).label(look.title.clone()).child(text(&look.title).font(theme::TEXT_BOLD).color(t.l1).wrap().w_full());
    if !look.body.is_empty() {
        card = card.child(text(&look.body).font(theme::NOTE).color(t.l2on).wrap().w_full());
    }
    if !look.lines.is_empty() {
        let mut names = col().w_full().gap(2.0).padding(8.0).radius(theme::RADIUS_ROW).fill(t.code);
        for line in &look.lines {
            names = names.child(text(line).font(theme::NOTE).color(t.l1).single_line().truncate());
        }
        card = card.child(names);
    }
    let mut buttons = row().w_full().gap(8.0).mt(8.0).child(spacer());
    for (i, choice) in look.choices.iter().enumerate() {
        let default = look.default == Some(i);
        let pick = pick.clone();
        let mut b = push_button(t, &choice.label, kind(choice, default), true).key(choice.key.clone()).on_press_tx(move |s: &mut S, tx: &mut Tx| pick(s, tx, i));
        if default {
            b = b.default_button();
        }
        buttons = buttons.child(b);
    }
    let cancel = look.cancel;
    enters(card.child(buttons).modal_tx(false, move |s: &mut S, tx: &mut Tx| pick(s, tx, cancel)), Entrance::Card)
}

/// The dialog over the window: the card in the middle of a dimmed layer that takes every
/// click, keyed `<key>-layer`.
pub fn dialog<S: 'static>(t: &Theme, key: &str, look: &Look, pick: impl Fn(&mut S, &mut Tx, usize) + Clone + 'static) -> El {
    let layer = el().key(format!("{key}-layer")).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.scrim()).items_center().justify_center().child(card(t, key, look, pick));
    enters(layer, Entrance::Scrim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_cant_be_undone_defaults_to_cancel() {
        let delete = Look::yes_no("Delete “a”?", "", "Delete", true, true);
        assert_eq!((delete.default, delete.cancel), (Some(0), 0));
        assert_eq!(kind(&delete.choices[1], false), Push::Caution);
        assert_eq!(kind(&delete.choices[0], true), Push::Primary);
        let discard = Look::yes_no("Discard?", "", "Discard", true, false);
        assert_eq!(discard.default, Some(1));
        // A red button never turns blue for being the default.
        assert_eq!(kind(&discard.choices[1], true), Push::Caution);
        let go = Look::yes_no("Download 3 GB?", "", "Download", false, false).keys("q-no", "q-yes");
        assert_eq!((go.default, kind(&go.choices[1], true)), (Some(1), Push::Primary));
        assert_eq!(go.choices.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["q-no", "q-yes"]);
    }
}
