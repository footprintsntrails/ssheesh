//! The toolbar button: a square icon button in the top row (the sidebar toggle, "+").

use rune::{text, El};

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// A 28-point icon button named `name` (its tooltip and what a screen reader says). The
/// caller adds `.on_press_tx(..)`.
pub fn toolbar_button(t: &Theme, key: &'static str, glyph: Icon, name: &'static str) -> El {
    rune::el()
        .key(key)
        .size(theme::CONTROL, theme::CONTROL)
        .radius(theme::RADIUS)
        .items_center()
        .justify_center()
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label_static(name)
        .help(name)
        .child(icon(glyph, theme::ICON, t.l2))
}

/// What a button with words is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonKind {
    /// One of several things to do.
    #[default]
    Plain,
    /// The one Return picks: blue, as the selected answer of a menu.
    Default,
    /// Deletes or discards: its words are red.
    Destructive,
}

/// A button that says what it does ("Save", "Try again", "Delete"). The caller adds the key
/// and `.on_press_tx(..)`.
pub fn text_button(t: &Theme, label: &str, kind: ButtonKind) -> El {
    let b = rune::el().h(24.0).px(10.0).radius(theme::RADIUS_ROW).items_center().justify_center().focusable(true).role("button").label(label);
    let (b, ink) = match kind {
        ButtonKind::Plain => (b.fill(t.hov2).hover(t.hov2).press(t.down), t.l1),
        ButtonKind::Default => (b.fill(t.blue).press(t.blue_down), t.on_color),
        ButtonKind::Destructive => (b.fill(t.hov2).hover(t.hov2).press(t.down), t.red),
    };
    b.child(text(label).font(theme::TAB_NAME).color(ink).single_line())
}
