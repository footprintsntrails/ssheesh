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
pub mod dialog;
pub mod field;
pub mod form;
pub mod graph;
pub mod help;
pub mod keycap;
pub mod list;
pub mod menu;
pub mod motion;
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
pub mod toast;

use rune::{el, Color, El};

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

/// The spinner's three quarters of a circle on a 16-unit square, its line 2 units wide.
static RING: rune::Icon = rune::Icon {
    name: "sshee-progress-ring",
    view: 16.0,
    body: r#"<path d="M8 1A7 7 0 1 1 1 8" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>"#,
};

/// [`RING`] with a line half as wide, for a ring around a whole sidebar button.
static RING_FINE: rune::Icon = rune::Icon {
    name: "sshee-progress-ring-fine",
    view: 16.0,
    body: r#"<path d="M8 0.5A7.5 7.5 0 1 1 0.5 8" stroke="currentColor" stroke-width="1" stroke-linecap="round" fill="none"/>"#,
};

/// How long the ring takes to go round once, in seconds.
const RING_TURN: f64 = 0.9;

/// Three quarters of a circle, `size` points across, going round: something is on its way
/// (reconnecting, Tor starting, a tab connecting). The turn is Rune's loop on the render clock,
/// so it asks for frames only while it's on screen, and none from the view. Under Reduce Motion
/// (the system's, or Settings → Appearance → Animations off) Rune holds every loop still: the
/// ring rests where it starts, as the design draws it. `line` picks the thinner drawing for a
/// big ring (a line under a tenth of its size).
pub fn progress_ring(size: f32, line: f32, color: Color) -> El {
    let drawing = if line / size < 0.09 { &RING_FINE } else { &RING };
    rune::icon(drawing, size).color(color).looping(rune::LoopSpec::rotation(RING_TURN)).a11y_hidden()
}

/// How far along something is, `fraction` of the way (0 to 1): a full circle in `track`, `size`
/// points across with a `line`-point line, and over it an arc from twelve o'clock round
/// clockwise, in `color`. A program's progress on its tab (OSC 9;4).
pub fn progress_arc(size: f32, line: f32, fraction: f32, color: Color, track: Color) -> El {
    let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let fit = crate::svg::Fit::square(0.0, 0.0, 16.0, size);
    // The circle's radius, on a 16-unit grid, with the line inside the square.
    let r = 8.0 - 8.0 * line / size;
    let circle = format!("M{} 8a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0z", 8.0 - r, 2.0 * r, -2.0 * r);
    let mut shape = rune::Shape::new().stroke_path(&crate::svg::path(&circle, fit), line, rune::LineCap::Round, track);
    if fraction >= 0.999 {
        shape = shape.stroke_path(&crate::svg::path(&circle, fit), line, rune::LineCap::Round, color);
    } else if fraction > 0.0 {
        let angle = fraction * std::f32::consts::TAU;
        let (x, y) = (8.0 + r * angle.sin(), 8.0 - r * angle.cos());
        let arc = format!("M8 {}A{r} {r} 0 {} 1 {x} {y}", 8.0 - r, u8::from(fraction > 0.5));
        shape = shape.stroke_path(&crate::svg::path(&arc, fit), line, rune::LineCap::Round, color);
    }
    rune::canvas(shape).size(size, size).a11y_hidden()
}

/// A hollow circle: a connection that is gone.
pub fn hollow_dot(size: f32, line: f32, color: Color) -> El {
    el().size(size, size).radius(size / 2.0).border(line, color).a11y_hidden()
}

/// The thin upright line between one server's tabs and the next.
pub fn divider(t: &Theme) -> El {
    el().size(1.0, 16.0).mx(5.0).fill(t.hair).a11y_hidden()
}
