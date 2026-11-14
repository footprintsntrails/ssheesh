//! A push button with a word on it: "Check now", "Choose…". The quiet kind: a soft tile, as
//! the buttons inside a Mac settings window are. Blue is kept for selection and focus.

use rune::{text, El};

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// A button saying `label`. `enabled` false: it is there to be seen, and does nothing. The
/// caller adds the key and `.on_press_tx(..)`.
pub fn push_button(t: &Theme, label: &str, enabled: bool) -> El {
    let e = rune::row()
        .h(theme::FORM_CONTROL)
        .px(10.0)
        .radius(theme::RADIUS_ROW)
        .fill(t.control)
        .items_center()
        .justify_center()
        .role("button")
        .label(label)
        .focusable(enabled)
        .disabled(!enabled)
        .child(text(label).font(theme::TEXT).color(t.l1).single_line().a11y_hidden());
    if enabled {
        e.hover(t.control_hover).press(t.control_down)
    } else {
        e.layer_opacity(Theme::DISABLED)
    }
}

/// A button that stays down: one of several things that can each be on ("Logo", "System").
/// On, it is a raised tile; off, a quiet outline. The caller adds the key and the press.
pub fn toggle_button(t: &Theme, label: &str, on: bool, enabled: bool) -> El {
    let e = rune::row()
        .h(theme::FORM_CONTROL)
        .px(10.0)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .justify_center()
        .role("button")
        .checked(on)
        .label(label)
        .focusable(enabled)
        .disabled(!enabled)
        .child(text(label).font(theme::CONTROL_TEXT).color(if on { t.l1 } else { t.l2 }).single_line().a11y_hidden());
    let e = if on { e.fill(t.raised).border(0.5, t.hair) } else { e.border(1.0, t.hair) };
    match (enabled, on) {
        (false, _) => e.layer_opacity(Theme::DISABLED),
        (true, true) => e,
        (true, false) => e.hover(t.hov2).press(t.down),
    }
}

/// A small square button with one icon, inside a row: "back to its own keys". Named `name`
/// (its tooltip and what a screen reader says). The caller adds the key and the press.
pub fn icon_button(t: &Theme, glyph: Icon, name: &str, enabled: bool) -> El {
    let size = theme::FORM_CONTROL;
    let e = rune::el()
        .size(size, size)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .justify_center()
        .role("button")
        .label(name)
        .help(name)
        .focusable(enabled)
        .disabled(!enabled)
        .child(icon(glyph, 14.0, t.l2on));
    if enabled {
        e.hover(t.hov2).press(t.down)
    } else {
        e.layer_opacity(Theme::DISABLED)
    }
}
