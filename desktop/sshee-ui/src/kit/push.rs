//! Buttons a form needs: the push button (a word on a rounded surface, 28 points tall; the one
//! Return presses is blue, one that takes something away is red, the rest are quiet), the
//! toggle button that stays down, the small square icon button inside a row, the check box,
//! and the two-or-three-way choice. Blue stays for the one button Return presses.

use rune::{text, El};

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// What a push button is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Push {
    /// Cancel, Back, anything that isn't the point of the dialog; every button in Settings.
    #[default]
    Quiet,
    /// The one Return presses.
    Primary,
    /// It takes something away.
    Destructive,
    /// It takes something away, offered among choices that don't ("Save over it" beside
    /// "Load what's there", Delete beside Cancel): a quiet surface, its words red.
    Caution,
}

/// A push button saying `label`. `enabled: false` dims it and takes its press away. Under the
/// pointer and under a press it goes a step darker. The caller adds its key and
/// `.on_press_tx(..)`.
pub fn push_button(t: &Theme, label: &str, kind: Push, enabled: bool) -> El {
    let (fill, hover, down, ink) = match kind {
        Push::Quiet => (t.control, t.control_hover, t.control_down, t.l1),
        Push::Primary => (t.blue, t.shade(t.blue, 0.08), t.blue_down, t.on_color),
        Push::Destructive => (t.red, t.shade(t.red, 0.08), t.red.alpha(0.8), if t.dark { t.page } else { t.on_color }),
        Push::Caution => (t.control, t.control_hover, t.control_down, t.red),
    };
    let b = rune::row()
        .h(theme::FIELD)
        .px(12.0)
        .radius(theme::RADIUS)
        .items_center()
        .justify_center()
        .fill(fill)
        .role("button")
        .label(label)
        .child(text(label).font(theme::TAB_NAME).color(ink).single_line().a11y_hidden());
    if enabled {
        b.hover(hover).press(down).focusable(true)
    } else {
        b.disabled(true).layer_opacity(Theme::DISABLED)
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
