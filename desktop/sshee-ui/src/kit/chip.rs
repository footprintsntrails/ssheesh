//! Label chips: a quiet word in a thin ring beside a name. "Prod" in red is the one the
//! design has; Staging and Dev follow it in 0.10's colours (amber, green), so each label reads
//! the same in the sidebar, the title and a split's header.

use rune::{text, Color, El};
use sshee_app::labels::env_short;
use sshee_app::model::EnvLabel;

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

/// A label's colour: Production red, Staging amber, Development green; one of your own grey.
pub fn env_color(t: &Theme, env: &EnvLabel) -> Color {
    match env {
        EnvLabel::Prod => t.red,
        EnvLabel::Staging => t.orange,
        EnvLabel::Dev => t.green,
        EnvLabel::Other(_) => t.l2,
    }
}

/// The chip of any label: "Prod" (keyed `prod`, as before), "Staging" or "Dev" (keyed
/// `env-staging`, `env-dev`).
pub fn env_chip(t: &Theme, env: &EnvLabel) -> El {
    match env {
        EnvLabel::Prod => prod_chip(t, false),
        _ => chip(t, env_short(env), env_color(t, env)).key(format!("env-{}", env.as_str())),
    }
}

/// How wide [`env_chip`] is, for a row that measures what it holds.
pub fn env_chip_width(env: &EnvLabel) -> f32 {
    10.0 + rune::text::advance(env_short(env), theme::CHIP)
}
