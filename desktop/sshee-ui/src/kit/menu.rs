//! Menu rows with key caps: the answers under an agent's question, a command palette's
//! results, a context menu. One row is highlighted at most, and it moves with the arrows or
//! the pointer, as in a Mac menu.

use rune::{text, El};

use crate::theme::{self, Theme};

/// How a row is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowState {
    /// Nothing special: under the pointer it greys.
    #[default]
    Rest,
    /// The one Return picks: blue.
    Hot,
    /// Being pressed, or picked a moment ago: the darker blue.
    Down,
}

/// A key cap: the key that picks a row, in a thin ring. `on_color`: on a blue row.
pub fn key_cap(t: &Theme, keys: &str, on_color: bool) -> El {
    let (ink, ring) = if on_color { (t.on_color, t.on_color.alpha(0.45)) } else { (t.l2on, t.hair) };
    rune::el()
        .h(18.0)
        .min_w(18.0)
        .px(5.0)
        .radius(theme::RADIUS_CHIP)
        .border(1.0, ring)
        .items_center()
        .justify_center()
        .a11y_hidden()
        .child(text(keys).font(theme::KEY_CAP).color(ink).single_line())
}

/// One row: `label`, and the key that picks it at the right. The caller adds the key and
/// `.on_press_tx(..)`.
pub fn menu_row(t: &Theme, label: &str, keys: Option<&str>, state: RowState) -> El {
    let on_color = state != RowState::Rest;
    let mut row = rune::row().h(26.0).w_full().px(8.0).gap(8.0).radius(theme::RADIUS_ROW).items_center().focusable(true).no_focus_ring().role("menuitem");
    row = match state {
        RowState::Rest => row.hover(t.hov2).press(t.down),
        RowState::Hot => row.fill(t.blue),
        RowState::Down => row.fill(t.blue_down),
    };
    row = row.child(text(label).font(theme::TEXT).color(if on_color { t.on_color } else { t.l1 }).single_line().truncate().grow(1.0).shrink());
    if let Some(keys) = keys {
        row = row.child(key_cap(t, keys, on_color));
    }
    row
}

/// The rows' container.
pub fn menu(rows: impl IntoIterator<Item = El>) -> El {
    rune::col().w_full().padding(6.0).role("menu").children(rows)
}
