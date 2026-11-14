//! Label chips: a quiet word in a thin ring beside a name. "Prod" in red is the one the
//! design has.

use rune::{text, Color, El};

use crate::theme::{self, Theme};

/// A chip saying `label` in `color`, ringed in the same colour at 45%.
pub fn chip(t: &Theme, label: &str, color: Color) -> El {
    rune::el()
        .h(16.0)
        .px(5.0)
        .radius(theme::RADIUS_CHIP)
        .border(1.0, t.outline_of(color))
        .items_center()
        .justify_center()
        .child(text(label).font(theme::CHIP).color(color).single_line())
}

/// "Prod": this server is Production. `on_selected`: it sits on the tab you're on, where the
/// dark appearance's red is a touch lighter.
pub fn prod_chip(t: &Theme, on_selected: bool) -> El {
    chip(t, "Prod", if on_selected { t.red_on } else { t.red }).key("prod")
}
