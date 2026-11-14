//! A pop-up button and its menu: the value it has now with two small arrows at its end, and
//! under a press the list of what it could be, the current one checked. For a choice among
//! more than a few, or among names too long to sit side by side.
//!
//! The menu is drawn here rather than by the system: Linux has no native pop-up menu in Rune,
//! and one look on every system is easier to test. It is a popover under its button; a click
//! outside or Esc closes it, the arrows walk it and Return picks.

use rune::{text, Align, El, Side, Tx};

use crate::icons::{icon, Icon};
use crate::kit::popover::popover;
use crate::theme::{self, Theme};

/// The widest a pop-up button's value is drawn; longer ones end in an ellipsis.
const VALUE_MAX: f32 = 260.0;

/// The button named `name`, showing `value`. `enabled` false: it shows its value and opens
/// nothing. The caller adds the key (the menu's anchor) and `.on_press_tx(..)` to open it.
pub fn popup_button(t: &Theme, name: &str, value: &str, enabled: bool) -> El {
    let e = rune::row()
        .h(theme::FORM_CONTROL)
        .pl(9.0)
        .pr(5.0)
        .gap(4.0)
        .radius(theme::RADIUS_ROW)
        .fill(t.control)
        .items_center()
        .role("button")
        .label(name)
        .a11y_value(value)
        .focusable(enabled)
        .disabled(!enabled)
        .child(text(value).font(theme::TEXT).color(t.l1).single_line().truncate().max_w(VALUE_MAX).a11y_hidden())
        .child(icon(Icon::UpDown, 14.0, t.l2on));
    if enabled {
        e.hover(t.control_hover).press(t.control_down)
    } else {
        e.layer_opacity(Theme::DISABLED)
    }
}

/// The menu under the button keyed `anchor`: one row per label, `selected` checked. `on_pick`
/// gets the row's place; `on_dismiss` runs on Esc and on a click outside. Draw it as a late
/// child of the window's root, so nothing clips it.
pub fn popup_menu<S: 'static>(
    t: &Theme,
    anchor: &str,
    labels: &[&str],
    selected: Option<usize>,
    on_pick: impl Fn(&mut S, usize, &mut Tx) + Clone + 'static,
    on_dismiss: impl Fn(&mut S, &mut Tx) + 'static,
) -> El {
    let widest = labels.iter().map(|l| rune::text::advance(l, theme::TEXT)).fold(0.0_f32, f32::max);
    // The check's place, the label, and the rows' and the menu's own padding.
    let width = (theme::ICON + 6.0 + widest + 2.0 * 8.0 + 2.0 * 6.0).ceil().clamp(120.0, 460.0);
    let mut rows = rune::col().w_full().padding(6.0).role("menu").focus_group();
    for (i, label) in labels.iter().enumerate() {
        let on = selected == Some(i);
        let pick = on_pick.clone();
        let mark = if on { icon(Icon::Check, theme::ICON, t.l1) } else { rune::el().size(theme::ICON, theme::ICON) };
        let mut row = rune::row()
            .key(format!("item-{i}"))
            .h(26.0)
            .w_full()
            .px(8.0)
            .gap(6.0)
            .radius(theme::RADIUS_ROW)
            .items_center()
            .role("menuitemradio")
            .checked(on)
            .label(*label)
            .focusable(true)
            .focus_selected(on)
            .hover(t.hov2)
            .press(t.down)
            .on_press_tx(move |s: &mut S, tx| pick(s, i, tx))
            .child(mark)
            .child(text(*label).font(theme::TEXT).color(t.l1).single_line().truncate().grow(1.0).shrink().a11y_hidden());
        if on {
            row = row.autofocus();
        }
        rows = rows.child(row);
    }
    popover(t, width).key("popup-menu").anchored(anchor, Side::Bottom, Align::Start, 4.0).modal_tx(true, on_dismiss).instant_changes().child(rows)
}
