//! A segmented control: a few short words side by side in one track, the picked one raised.
//! For a choice among two to five where seeing them all at once helps. Left and Right move
//! the choice with the keyboard, as in a Mac settings window.

use rune::{text, Color, El, Tx};

use crate::theme::{self, Theme};

/// The control named `name` with one segment per label, `selected` raised. `enabled` false:
/// it shows the choice and can't be changed. `on_pick` gets the segment's place.
pub fn segmented<S: 'static>(
    t: &Theme,
    name: &str,
    labels: &[&str],
    selected: Option<usize>,
    enabled: bool,
    on_pick: impl Fn(&mut S, usize, &mut Tx) + Clone + 'static,
) -> El {
    let height = theme::FORM_CONTROL;
    let mut track = rune::row().h(height).padding(2.0).radius(7.0).fill(t.control).items_center().role("radiogroup").focus_group().label(name);
    for (i, label) in labels.iter().enumerate() {
        let on = selected == Some(i);
        let mut segment = rune::row()
            .key(format!("seg-{i}"))
            .h(height - 4.0)
            .px(10.0)
            .radius(5.0)
            .items_center()
            .justify_center()
            .role("radio")
            .checked(on)
            .label(*label)
            .position_in_set(i + 1, labels.len())
            .focusable(enabled)
            .focus_selected(on)
            .disabled(!enabled)
            .child(text(*label).font(theme::CONTROL_TEXT).color(if on { t.l1 } else { t.l2on }).single_line().a11y_hidden());
        if on {
            // Raised: a lighter tile with a soft shadow under it.
            segment = segment.fill(t.raised).shadow(1.0, 2.0, Color::BLACK.alpha(if t.dark { 0.3 } else { 0.14 }));
        } else if enabled {
            segment = segment.hover(t.hov2).press(t.down);
        }
        if enabled {
            let pick = on_pick.clone();
            segment = segment.on_press_tx(move |s: &mut S, tx| pick(s, i, tx));
        }
        track = track.child(segment);
    }
    if !enabled {
        track = track.layer_opacity(Theme::DISABLED);
    }
    track
}
