//! The toolbar button: a square icon button in the top row (the sidebar toggle, "+").

use rune::El;

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
