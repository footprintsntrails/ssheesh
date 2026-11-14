//! The popover surface: a raised panel with a thin ring and a soft shadow, for an agent's
//! question, a server's card, a menu.
//!
//! Only the surface is here. Placing one under its anchor and closing it on a click outside
//! is Rune's (`El::modal`, `Cx::window_layout` for the anchor's rectangle); the first screen
//! that opens one wires that.

use rune::{text, Color, El};

use crate::theme::{self, Theme};

/// The surface, `width` points wide, its children in a column.
pub fn popover(t: &Theme, width: f32) -> El {
    rune::col()
        .w(width)
        .radius(theme::RADIUS_POPOVER)
        .fill(t.pop)
        .border(1.0, t.rim)
        .shadow(16.0, 40.0, Color::BLACK.alpha(0.3))
        .role("dialog")
}

/// A popover's foot: a quiet hint at the left ("Press 1, 2 or 3, or click an answer") and
/// another at the right ("Esc to decide later"), over a hairline.
pub fn popover_foot(t: &Theme, left: &str, right: &str) -> El {
    rune::row()
        .w_full()
        .h(34.0)
        .px(14.0)
        .gap(8.0)
        .items_center()
        .border_sides([1.0, 0.0, 0.0, 0.0], t.hair)
        .child(text(left).font(theme::SMALL).color(t.l2).single_line().truncate().shrink())
        .child(rune::spacer())
        .child(text(right).font(theme::SMALL).color(t.l2).single_line())
}
