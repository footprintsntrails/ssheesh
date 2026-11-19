//! Label chips: a quiet word in a thin ring beside a name. "Prod" in red is the one the
//! design has; Staging and Dev follow it in 0.10's colours (amber, green), so each label reads
//! the same in the sidebar, the title, the tabs and a split's header.

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
/// Staging and Dev are taken toward the text colour as far as they need to read at 4.5:1 on
/// everything a chip sits on (the light design's green alone is too faint on its bar).
pub fn env_color(t: &Theme, env: &EnvLabel) -> Color {
    match env {
        EnvLabel::Prod => t.red,
        EnvLabel::Staging => legible(t, t.orange),
        EnvLabel::Dev => legible(t, t.green),
        EnvLabel::Other(_) => t.l2,
    }
}

/// What a label's chip is drawn on: the bar along the top (the title, the tabs, the sidebar),
/// the tab you're on, a page (a split's header), a card.
fn grounds(t: &Theme) -> [Color; 4] {
    let solid = |c: Color| if c.a >= 1.0 { c } else { t.frame.alpha(1.0).mix(c.alpha(1.0), c.a) };
    [solid(t.frame), solid(t.sel), solid(t.page), solid(t.pop)]
}

/// WCAG's contrast ratio between two solid colours (a `Color` holds linear values).
fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (a.luminance(), b.luminance());
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `color`, or the first step from it toward the main text colour that reads at 4.5:1 on
/// every [`grounds`].
fn legible(t: &Theme, color: Color) -> Color {
    let grounds = grounds(t);
    (0..=10)
        .map(|step| color.mix(t.l1.alpha(color.a), step as f32 / 10.0))
        .find(|c| grounds.iter().all(|g| contrast(*c, *g) >= 4.5))
        .unwrap_or(t.l1)
}

/// The chip of any label: "Prod" (keyed `prod`, as before), "Staging" or "Dev" (keyed
/// `env-staging`, `env-dev`).
pub fn env_chip(t: &Theme, env: &EnvLabel) -> El {
    env_chip_on(t, env, false)
}

/// [`env_chip`] on a tab: `on_selected`, it sits on the tab you're on, where "Prod" takes
/// [`prod_chip`]'s lighter red.
pub fn env_chip_on(t: &Theme, env: &EnvLabel, on_selected: bool) -> El {
    match env {
        EnvLabel::Prod => prod_chip(t, on_selected),
        _ => chip(t, env_short(env), env_color(t, env)).key(format!("env-{}", env.as_str())),
    }
}

/// How wide [`env_chip`] is, for a row that measures what it holds.
pub fn env_chip_width(env: &EnvLabel) -> f32 {
    10.0 + rune::text::advance(env_short(env), theme::CHIP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::model::Settings;
    use sshee_app::settings_store::looks;

    #[test]
    fn staging_and_dev_read_on_every_built_in_theme_and_prod_keeps_its_red() {
        for resolved in looks::all(&Settings::default()) {
            let t = crate::looks::theme_of(&resolved, !resolved.light, None);
            for env in [EnvLabel::Staging, EnvLabel::Dev] {
                let ink = env_color(t, &env);
                for ground in grounds(t) {
                    let ratio = contrast(ink, ground);
                    assert!(ratio >= 4.5, "{} {env:?}: {} on {} is {ratio:.2}:1", resolved.id, ink.hex(), ground.hex());
                }
            }
            assert_eq!(env_color(t, &EnvLabel::Prod), t.red, "{}", resolved.id);
        }
        // The dark design's own amber and green already read: they're kept as they are.
        let night = Theme::of(true);
        assert_eq!((env_color(night, &EnvLabel::Staging), env_color(night, &EnvLabel::Dev)), (night.orange, night.green));
    }
}
