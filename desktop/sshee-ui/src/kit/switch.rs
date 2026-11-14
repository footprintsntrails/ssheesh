//! A switch, as in a Mac settings window: a capsule with a round knob that sits at the right
//! and turns the capsule green when on. It says what it switches through its label (the row
//! beside it shows the same words), and takes Space and Return when it has the keyboard.

use rune::{Color, El};

use crate::theme::{self, Theme};

/// A switch named `name`, on or off. `enabled` false: it shows its value and can't be changed.
/// The caller adds the key and `.on_press_tx(..)`.
pub fn switch(t: &Theme, name: &str, on: bool, enabled: bool) -> El {
    let (w, h) = theme::SWITCH;
    let knob = theme::SWITCH_KNOB;
    let inset = (h - knob) / 2.0;
    let track = if on { t.switch_on } else { t.control };
    let e = rune::el()
        .size(w, h)
        .radius(h / 2.0)
        .fill(track)
        .role("switch")
        .checked(on)
        .label(name)
        .focusable(enabled)
        .disabled(!enabled)
        .child(
            rune::el()
                .key("knob")
                .absolute(if on { w - inset - knob } else { inset }, inset)
                .size(knob, knob)
                .radius(knob / 2.0)
                .fill(t.knob)
                .shadow(1.0, 2.5, Color::BLACK.alpha(0.28))
                .a11y_hidden(),
        );
    if !enabled {
        return e.layer_opacity(Theme::DISABLED);
    }
    // Under the pointer and under a press the track goes a step darker, on or off.
    if on {
        e.hover(t.shade(track, 0.06)).press(t.shade(track, 0.14))
    } else {
        e.hover(t.control_hover).press(t.control_down)
    }
}
