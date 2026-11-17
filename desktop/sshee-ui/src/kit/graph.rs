//! Numbers over time and how full something is: the Stats page's graphs and meters, and the
//! meters on a server's card in the sidebar.
//!
//! Calm on purpose: one hairline of ink per series in the page's quiet greys, a faint wash
//! under the first, a hairline baseline, no axes or grid. The newest look is at the right
//! edge and the line grows in from the right as looks come, as Activity Monitor draws it, so
//! a page opened a moment ago doesn't stretch two looks across the whole width. Blue stays the
//! selection's: a meter is grey ink, and only past its threshold the warning's orange (the
//! status bar's rule).

use rune::{canvas, el, Color, El, Shape};

use crate::theme::Theme;

/// One line of a graph: its values (oldest first) and its ink.
pub struct Series<'a> {
    pub values: &'a [f64],
    pub ink: Color,
    /// A faint wash under the line.
    pub wash: bool,
}

/// A graph `height` points tall that fills its width. `slots`: how many looks fit across it
/// (the history's length); `max`: the value at the top.
pub fn graph(t: &Theme, series: &[Series], slots: usize, max: f64, height: f32) -> El {
    let slots = slots.max(2);
    // The view box: one unit per look across, one per point down; strokes stay in points.
    let (w, h) = ((slots - 1) as f32, height);
    let max = if max.is_finite() && max > 0.0 { max } else { 1.0 };
    // Room for half the line at the top and bottom, so a flat line at 0 or 100% isn't cut.
    let inset = 1.0;
    let y = |v: f64| h - inset - ((v / max).clamp(0.0, 1.0) as f32) * (h - 2.0 * inset);
    let mut shape = Shape::new().line([0.0, h - 0.5], [w, h - 0.5], 1.0, rune::LineCap::Butt, t.hair2);
    for s in series {
        let n = s.values.len().min(slots);
        if n < 2 {
            continue;
        }
        let first = slots - n;
        let points: Vec<[f32; 2]> = s.values[s.values.len() - n..].iter().enumerate().map(|(i, v)| [(first + i) as f32, y(*v)]).collect();
        if s.wash {
            let mut area = points.clone();
            area.push([w, h]);
            area.push([first as f32, h]);
            shape = shape.fill(area, s.ink.alpha(if t.dark { 0.14 } else { 0.1 }));
        }
        shape = shape.stroke(points, 1.5, s.ink);
    }
    canvas(shape.view_box(w, h)).w_full().h(height).a11y_hidden()
}

/// How full something is: a rounded track `height` points tall with the used part in ink, in
/// the warning's orange once `warn`.
pub fn meter(t: &Theme, value: f64, warn: bool, height: f32) -> El {
    let v = value.clamp(0.0, 1.0) as f32;
    let ink = if warn { t.orange_dot } else { t.l2on };
    el().w_full()
        .h(height)
        .radius(height / 2.0)
        .fill(t.control)
        .clip_corners([height / 2.0; 4])
        .a11y_hidden()
        .child(el().w_frac(v.max(0.01)).h_full().radius(height / 2.0).fill(ink))
}
