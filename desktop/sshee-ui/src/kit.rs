//! The component kit: the 1.0 design's controls, one module each, drawn from the tokens in
//! [`crate::theme`]. A component is a function from what it shows to an element; the screen
//! that uses it chains the handlers (`.on_press_tx(..)`), so the kit knows nothing of the
//! app's state.
//!
//! States are the design's states sheet (research_notes/v1-plan/fresh-apple.html, "states"):
//! under the pointer a fainter fill, pressed a stronger one, the keyboard's focus a 3-point
//! blue ring (drawn by Rune for every focusable control: the window's view sets it once with
//! `Cx::focus_ring`), a window that isn't in front quieter. Hover and press are paint only:
//! they never run the view again.
//!
//! What Rune already has is used as it is, not rebuilt here: tooltips (`El::help`), focus
//! rings and Tab order, hover and press tracking, text fields (`rune::text_input`), native
//! pop-up menus (`Tx::popup_menu`). rune-kit (menus, popovers, dialogs, tables and trees in
//! OpenCode's look) isn't a dependency yet: see desktop/ARCHITECTURE.md.

pub mod button;
pub mod chip;
pub mod field;
pub mod form;
pub mod keycap;
pub mod list;
pub mod menu;
pub mod popover;
pub mod popup;
pub mod push;
pub mod rail;
pub mod segmented;
pub mod slider;
pub mod status;
pub mod switch;
pub mod tab;
pub mod tile;

use rune::{canvas, el, Color, El, LineCap, Shape};

use crate::svg::{self, Fit};
use crate::theme::Theme;

/// A round dot, `size` points across.
pub fn dot(size: f32, color: Color) -> El {
    el().size(size, size).radius(size / 2.0).fill(color).a11y_hidden()
}

/// A dot with a ring of the surface it sits on around it, so it reads over an icon's lines:
/// the "needs you" dot on a sidebar icon.
pub fn ringed_dot(size: f32, ring: f32, color: Color, surface: Color) -> El {
    let outer = size + 2.0 * ring;
    el().size(outer, outer).radius(outer / 2.0).fill(surface).items_center().justify_center().a11y_hidden().child(dot(size, color))
}

/// Three quarters of a circle, `size` points across: something is on its way (reconnecting,
/// Tor starting). Still, as the design draws it.
pub fn progress_ring(size: f32, line: f32, color: Color) -> El {
    let r = (size - line) / 2.0;
    let c = size / 2.0;
    let d = format!("M{c} {}A{r} {r} 0 1 1 {} {c}", c - r, c - r);
    let arc = svg::path(&d, Fit { scale: 1.0, dx: 0.0, dy: 0.0 });
    canvas(Shape::new().stroke_path(&arc, line, LineCap::Round, color)).size(size, size).a11y_hidden()
}

/// A hollow circle: a connection that is gone.
pub fn hollow_dot(size: f32, line: f32, color: Color) -> El {
    el().size(size, size).radius(size / 2.0).border(line, color).a11y_hidden()
}

/// The thin upright line between one server's tabs and the next.
pub fn divider(t: &Theme) -> El {
    el().size(1.0, 16.0).mx(5.0).fill(t.hair).a11y_hidden()
}
