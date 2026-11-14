//! Push buttons: a word on a rounded surface, 28 points tall. The one Return presses is blue;
//! one that takes something away is red; the rest are quiet. Also the check box and the
//! two-or-three-way choice a form needs.

use rune::{text, El};

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// What a push button is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Push {
    /// Cancel, Back, anything that isn't the point of the dialog.
    #[default]
    Quiet,
    /// The one Return presses.
    Primary,
    /// It takes something away.
    Destructive,
}

/// A push button saying `label`. `enabled: false` greys it and takes its press away. The
/// caller adds its key and `.on_press_tx(..)`.
pub fn push_button(t: &Theme, label: &str, kind: Push, enabled: bool) -> El {
    let (fill, down, ink) = match kind {
        Push::Quiet => (t.hov2, t.down, t.l1),
        Push::Primary => (t.blue, t.blue_down, t.on_color),
        Push::Destructive => (t.red, t.red.alpha(0.8), if t.dark { t.page } else { t.on_color }),
    };
    let mut b = rune::row().h(theme::FIELD).px(12.0).radius(theme::RADIUS).items_center().justify_center().fill(fill).role("button").label(label);
    b = if enabled { b.press(down).focusable(true) } else { b.disabled(true).opacity(0.45) };
    b.child(text(label).font(theme::TAB_NAME).color(ink).single_line().a11y_hidden())
}

/// A check box with its words. The caller adds its key and `.on_press_tx(..)`.
pub fn check_box(t: &Theme, label: &str, on: bool) -> El {
    let mut mark = rune::el().size(16.0, 16.0).radius(4.0).items_center().justify_center();
    mark = if on { mark.fill(t.blue).child(icon(Icon::Check, 14.0, t.on_color)) } else { mark.fill(t.field()).border(1.0, t.hair) };
    rune::row()
        .h(22.0)
        .gap(8.0)
        .items_center()
        .radius(theme::RADIUS_CHIP)
        .focusable(true)
        .role("checkbox")
        .checked(on)
        .label(label)
        .child(mark)
        .child(text(label).font(theme::TEXT).color(t.l1).single_line().a11y_hidden())
}

/// One of a few choices side by side ("SSH key | Password"): the chosen one is raised. The
/// caller adds each one's key and `.on_press_tx(..)`.
pub fn choice(t: &Theme, label: &str, chosen: bool) -> El {
    let mut b = rune::row().h(24.0).px(10.0).radius(theme::RADIUS_ROW).items_center().justify_center().focusable(true).role("radio").checked(chosen).label(label);
    b = if chosen { b.fill(t.sel).border(0.5, t.rim) } else { b.hover(t.hov2).press(t.down) };
    b.child(text(label).font(if chosen { theme::TAB_NAME } else { theme::TEXT }).color(if chosen { t.l1 } else { t.l2 }).single_line().a11y_hidden())
}

/// The trough a row of [`choice`]s sits in.
pub fn choices(t: &Theme, items: impl IntoIterator<Item = El>) -> El {
    rune::row().padding(2.0).gap(2.0).radius(theme::RADIUS).fill(t.hov2).role("radiogroup").children(items)
}
