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
    /// Can't be picked now (Copy with nothing selected): quieter, and deaf to the pointer.
    Off,
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
    row_in(t, label, keys, state, t.l1)
}

/// A row that deletes or discards: its words are red while it rests.
pub fn menu_row_destructive(t: &Theme, label: &str, keys: Option<&str>, state: RowState) -> El {
    row_in(t, label, keys, state, t.red)
}

fn row_in(t: &Theme, label: &str, keys: Option<&str>, state: RowState, rest_ink: rune::Color) -> El {
    let on_color = matches!(state, RowState::Hot | RowState::Down);
    let mut row = rune::row().h(ROW).w_full().px(8.0).gap(8.0).radius(theme::RADIUS_ROW).items_center().no_focus_ring().role("menuitem");
    row = match state {
        RowState::Rest => row.focusable(true).hover(t.hov2).press(t.down),
        RowState::Hot => row.focusable(true).fill(t.blue),
        RowState::Down => row.focusable(true).fill(t.blue_down),
        RowState::Off => row,
    };
    let ink = match state {
        RowState::Off => t.l2,
        _ if on_color => t.on_color,
        _ => rest_ink,
    };
    row = row.child(text(label).font(theme::TEXT).color(ink).single_line().truncate().grow(1.0).shrink());
    if let Some(keys) = keys {
        row = row.child(key_cap(t, keys, on_color));
    }
    row
}

/// A row's height, a separator's with its room, and the container's padding: a menu of `rows`
/// rows and `seps` separators is `rows * ROW + seps * SEPARATOR + 2 * PAD` points high.
pub const ROW: f32 = 26.0;
pub const SEPARATOR: f32 = 9.0;
pub const PAD: f32 = 6.0;

/// A hairline between groups of rows.
pub fn menu_separator(t: &Theme) -> El {
    rune::el().w_full().h(SEPARATOR).px(8.0).items_center().a11y_hidden().child(rune::el().w_full().h(1.0).fill(t.hair))
}

/// A context menu: `rows` on a popover `width` points wide whose top-left is at `at`, moved
/// as far as it must to stay inside `within` (the size of what it is drawn in). `height` is
/// the rows' and separators' own (see [`ROW`]). The caller keys it and makes it modal
/// (`El::modal_tx`: Escape and a click outside close it).
pub fn context_menu(t: &Theme, at: [f32; 2], within: [f32; 2], width: f32, height: f32, rows: impl IntoIterator<Item = El>) -> El {
    let height = height + 2.0 * PAD;
    let x = at[0].min(within[0] - width - 8.0).max(8.0);
    let y = at[1].min(within[1] - height - 8.0).max(8.0);
    crate::kit::popover::popover(t, width).absolute(x, y).cursor(rune::Cursor::Default).child(menu(rows))
}

/// The rows' container.
pub fn menu(rows: impl IntoIterator<Item = El>) -> El {
    rune::col().w_full().padding(PAD).role("menu").children(rows)
}
