//! A slider: a thin track with a round knob, and the value in words at its right. Dragging
//! anywhere on the track moves the knob there; the arrow keys move it a step, Page Up and
//! Page Down ten.

use rune::{text, Align, Color, El, Phase, Tx};

use crate::theme::{self, Theme};

/// The track's width in points.
const TRACK: f32 = 150.0;
/// Room for the value: "100%", "1.22", "13 px".
const VALUE: f32 = 44.0;

/// What a slider moves over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl Range {
    /// `value` on the nearest step, inside the range. Steps count from `min`, and the result
    /// is rounded to the step's own digits, so 1.22 stays 1.22 and not 1.2200000000000002.
    pub fn snap(&self, value: f64) -> f64 {
        if self.step.is_nan() || self.step <= 0.0 || self.max.is_nan() || self.max <= self.min {
            return self.min;
        }
        let steps = ((value.clamp(self.min, self.max) - self.min) / self.step).round();
        let digits = (0..6).find(|d| (self.step * 10f64.powi(*d)).fract().abs() < 1e-9).unwrap_or(6);
        let scale = 10f64.powi(digits);
        (((self.min + steps * self.step) * scale).round() / scale).clamp(self.min, self.max)
    }

    /// Where `value` is along the track, 0 to 1.
    pub fn fraction(&self, value: f64) -> f64 {
        if self.max > self.min {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The value at `fraction` of the track, on a step.
    pub fn at(&self, fraction: f64) -> f64 {
        self.snap(self.min + fraction.clamp(0.0, 1.0) * (self.max - self.min))
    }

    /// `value` moved by `steps` of the range's step.
    pub fn stepped(&self, value: f64, steps: i32) -> f64 {
        self.snap(value + f64::from(steps) * self.step)
    }
}

/// The slider named `name` at `value`, saying `shown` beside it. `enabled` false: it shows
/// its value and can't be moved. `on_change` gets each new value while the knob is dragged.
pub fn slider<S: 'static>(
    t: &Theme,
    name: &str,
    range: Range,
    value: f64,
    shown: &str,
    enabled: bool,
    on_change: impl Fn(&mut S, f64, &mut Tx) + Clone + 'static,
) -> El {
    let knob = theme::SLIDER_KNOB;
    let line = theme::SLIDER_TRACK;
    let travel = TRACK - knob;
    let at = range.fraction(value) as f32 * travel;
    let mut track = rune::el()
        .key("track")
        .size(TRACK, knob)
        .role("slider")
        .label(name)
        .a11y_value(shown)
        .range_value(value, range.min, range.max)
        .focusable(enabled)
        .disabled(!enabled)
        .child(rune::el().absolute(0.0, (knob - line) / 2.0).size(TRACK, line).radius(line / 2.0).fill(t.control).a11y_hidden())
        // What is behind the knob is filled in the text's own tone: blue stays for selection.
        .child(rune::el().absolute(0.0, (knob - line) / 2.0).size(at + knob / 2.0, line).radius(line / 2.0).fill(t.l2).a11y_hidden())
        .child(
            rune::el()
                .key("knob")
                .absolute(at, 0.0)
                .size(knob, knob)
                .radius(knob / 2.0)
                .fill(t.knob)
                .border(0.5, Color::BLACK.alpha(0.12))
                .shadow(1.0, 2.5, Color::BLACK.alpha(0.28))
                .a11y_hidden(),
        );
    if enabled {
        let (drag, step) = (on_change.clone(), on_change);
        track = track
            .on_drag(move |s: &mut S, d, tx| {
                match d.phase {
                    // Where it was, to put back if the drag is called off.
                    Phase::Down => d.data = value as f32,
                    Phase::Up if d.cancelled() => return drag(s, range.snap(f64::from(d.data)), tx),
                    _ => {}
                }
                let fraction = f64::from(((d.x - knob / 2.0) / travel).clamp(0.0, 1.0));
                drag(s, range.at(fraction), tx);
            })
            .on_step(move |s: &mut S, steps, tx| step(s, range.stepped(value, steps), tx));
    }
    let mut row = rune::row()
        .items_center()
        .gap(10.0)
        .child(track)
        .child(text(shown).key("value").font(theme::VALUE).color(t.l2on).single_line().align(Align::End).w(VALUE).a11y_hidden());
    if !enabled {
        row = row.layer_opacity(Theme::DISABLED);
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_land_on_steps_inside_the_range() {
        let size = Range { min: 10.0, max: 20.0, step: 1.0 };
        assert_eq!(size.snap(13.4), 13.0);
        assert_eq!(size.snap(13.5), 14.0);
        assert_eq!(size.snap(3.0), 10.0);
        assert_eq!(size.snap(99.0), 20.0);
        assert_eq!((size.at(0.0), size.at(0.5), size.at(1.0), size.at(7.0)), (10.0, 15.0, 20.0, 20.0));
        assert_eq!((size.fraction(10.0), size.fraction(15.0), size.fraction(25.0)), (0.0, 0.5, 1.0));
        // Line height: steps of 0.02 from 1, with no stray digits.
        let height = Range { min: 1.0, max: 1.6, step: 0.02 };
        assert_eq!(height.snap(1.22), 1.22);
        assert_eq!(height.snap(1.2249), 1.22);
        assert_eq!(height.stepped(1.22, 1), 1.24);
        assert_eq!(height.stepped(1.22, -1), 1.2);
        assert_eq!(height.stepped(1.6, 10), 1.6);
        assert_eq!(format!("{}", height.stepped(1.1, 1)), "1.12");
        // The interface size: steps of 5 from 85.
        let zoom = Range { min: 85.0, max: 130.0, step: 5.0 };
        assert_eq!((zoom.snap(101.0), zoom.snap(103.0), zoom.stepped(100.0, -10)), (100.0, 105.0, 85.0));
        // A range that makes no sense stays at its start.
        assert_eq!(Range { min: 1.0, max: 1.0, step: 0.0 }.snap(5.0), 1.0);
        assert_eq!(Range { min: 1.0, max: 1.0, step: 0.0 }.fraction(5.0), 0.0);
    }
}
