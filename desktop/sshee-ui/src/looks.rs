//! What Settings → Appearance makes of the window: the theme on screen (built in or yours),
//! drawn in the accent colour, and the logo (winking, still or hidden). The rules are
//! `sshee_app::settings_store::looks`; this turns a theme's fourteen colours into the design's
//! tokens (`theme::Theme`).
//!
//! SSHeesh Default and Daylight with the default accent are the design's own `Theme::of`, to
//! the digit. Any other theme is the design's appearance of its kind (dark or light) with
//! every colour that theme sets put in its place, and the faint ones (lines, hovers, presses)
//! made from its "Lines and shading" at the design's strengths.
//!
//! Themes are `&'static`: every view holds one. Each different theme is made once and kept
//! for the rest of the run (a few hundred bytes each; the editor makes one per colour tried).

use std::sync::Mutex;

use rune::{Color, El};
use sshee_app::model::{Logo, Settings};
use sshee_app::settings_store::looks::{self, Colors, Resolved};
use sshee_app::settings_store::schemes::{self, Appearance};

use crate::icons::{icon, logo_face, Icon, LogoFace};
use crate::shell::Shell;
use crate::theme::Theme;

/// `0xRRGGBBAA` as a colour.
pub fn color(c: u32) -> Color {
    rune::color::hex(&format!("#{c:08x}")).unwrap_or(Color::BLACK)
}

/// `top` over a solid `bottom` (in linear light, as the GPU blends it).
fn over(top: Color, bottom: Color) -> Color {
    let a = top.a;
    Color { r: top.r * a + bottom.r * (1.0 - a), g: top.g * a + bottom.g * (1.0 - a), b: top.b * a + bottom.b * (1.0 - a), a: 1.0 }
}

/// The design's appearance with a theme's colours and the accent in it.
pub fn build(colors: &Colors, dark: bool, accent: Option<u32>) -> Theme {
    let base = Theme::of(dark);
    let c = |i: usize| color(colors[i]);
    let (window, canvas, raised, ink, fg, fg2, _hints, ok, warn, bad, glass) = (c(0), c(1), c(2), c(3), c(4), c(5), c(6), c(7), c(8), c(9), c(10));
    let solid_canvas = over(canvas, window);
    let faint = |strength: f32| ink.alpha(strength);
    let mut t = Theme {
        frame: window,
        page: canvas,
        pop: glass,
        sel: if dark { over(faint(0.09), window) } else { raised },
        hair: over(faint(if dark { 0.08 } else { 0.11 }), window),
        edge: if dark { over(Color::BLACK.alpha(0.4), solid_canvas) } else { over(faint(0.11), window) },
        hair2: over(faint(if dark { 0.05 } else { 0.1 }), solid_canvas),
        l1: fg,
        l2: fg2,
        // Words on a control (an unpicked segment, a quiet button) read a step brighter than
        // the page's secondary text, as the design's own do: between it and the main text,
        // never the theme's dimmest hint colour.
        l2on: over(fg.alpha(if dark { 0.45 } else { 0.25 }), fg2),
        strong: fg,
        orange: warn,
        orange_dot: warn,
        red: bad,
        red_on: bad,
        green: ok,
        selrail: faint(if dark { 0.14 } else { 0.12 }),
        rim: faint(if dark { 0.1 } else { 0.14 }),
        code: if dark { over(faint(0.015), solid_canvas) } else { window },
        tabhi: faint(if dark { 0.1 } else { 0.0 }),
        hov: faint(if dark { 0.04 } else { 0.05 }),
        hov2: faint(if dark { 0.09 } else { 0.07 }),
        down: faint(if dark { 0.12 } else { 0.1 }),
        secbg: if dark { over(faint(0.06), solid_canvas) } else { window },
        group: over(faint(if dark { 0.03 } else { 0.04 }), solid_canvas),
        control: faint(if dark { 0.1 } else { 0.07 }),
        control_hover: faint(if dark { 0.14 } else { 0.1 }),
        control_down: faint(if dark { 0.19 } else { 0.14 }),
        raised,
        switch_on: ok,
        ..base.clone()
    };
    match accent {
        Some(accent) => with_accent(&mut t, accent),
        // The design's accent words, as they are where they read on this theme's own page
        // and cards, else a shade that does.
        None => t.accent_text = t.legible(base.accent_text),
    }
    t
}

/// The accent where the design has its blue: what is picked, a button Return presses, the
/// keyboard's focus. Text on it is black or white, whichever reads (0.10's `onAccent`).
fn with_accent(t: &mut Theme, accent: u32) {
    let blue = color(accent | 0xff);
    t.blue = blue;
    t.blue_down = t.shade(blue, 0.16);
    t.ring = blue;
    t.accent_text = t.legible(blue);
    t.on_color = if schemes::is_light(accent | 0xff) { Color::BLACK } else { Color::WHITE };
}

/// Themes made so far, by what they were made from.
type Made = Vec<((Colors, bool, Option<u32>), &'static Theme)>;

static MADE: Mutex<Made> = Mutex::new(Vec::new());

/// The theme for `resolved` in a window that is dark or light, in `accent`: the design's own
/// when nothing differs from it.
pub fn theme_of(resolved: &Resolved, dark: bool, accent: Option<u32>) -> &'static Theme {
    if accent.is_none() && resolved.design == Some(dark) {
        return Theme::of(dark);
    }
    let key = (resolved.colors, dark, accent);
    let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, t)) = made.iter().find(|(k, _)| *k == key) {
        return t;
    }
    let base = match resolved.design {
        // The design itself with only the accent changed.
        Some(_) => {
            let mut t = Theme::of(dark).clone();
            if let Some(accent) = accent {
                with_accent(&mut t, accent);
            }
            t
        }
        None => build(&resolved.colors, dark, accent),
    };
    let t: &'static Theme = Box::leak(Box::new(base));
    made.push((key, t));
    t
}

/// The settings' theme in a window on a system that is dark or isn't.
pub fn theme_for(s: &Settings, system_dark: bool) -> &'static Theme {
    let dark = Appearance::of(s).dark(system_dark);
    let resolved = looks::on_screen(s, dark);
    // A light theme picked for the dark side (or the other way round) is drawn as it is.
    theme_of(&resolved, !resolved.light, looks::accent(s))
}

impl Shell {
    /// The theme on screen (Settings → Appearance), on a system that is dark or isn't.
    pub fn theme(&self, system_dark: bool) -> &'static Theme {
        theme_for(self.settings.get(), system_dark)
    }

    /// The logo as Settings → Appearance asks for it, `size` points square: winking now and
    /// then, still, or, hidden, the house the main menu button shows in its place.
    pub fn app_logo(&self, size: f32, t: &Theme) -> El {
        match self.settings.get().logo {
            Logo::Hidden => icon(Icon::House, crate::theme::ICON_RAIL, t.l2),
            _ => logo_face(size, self.screens.settings.wink.face),
        }
    }
}

/// The winking logo's clock: when the next blink comes, and the face it shows now.
#[derive(Debug, Default)]
pub struct Wink {
    pub face: LogoFace,
    /// When the face changes next (window clock).
    next: Option<f64>,
    /// A small pseudo-random sequence, so blinks don't come on a beat.
    seed: u64,
}

impl Wink {
    fn random(&mut self) -> f64 {
        // splitmix64
        self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    }
}

/// Before each frame: the winking logo blinks, or winks with its right eye, every 4.5 to 13.5
/// seconds, as 0.10's did; only with animations on. Returns whether its face changed.
pub(crate) fn tick_logo(s: &mut Shell, clock: f64, tx: &mut rune::Tx) -> bool {
    let winking = s.settings.get().logo == Logo::Winking && s.settings.get().animations;
    let wink = &mut s.screens.settings.wink;
    if !winking {
        let changed = wink.face != LogoFace::Open;
        *wink = Wink { seed: wink.seed, ..Wink::default() };
        return changed;
    }
    let Some(next) = wink.next else {
        let after = 4.5 + wink.random() * 9.0;
        wink.next = Some(clock + after);
        tx.wake_after(std::time::Duration::from_secs_f64(after));
        return false;
    };
    if clock < next {
        return false;
    }
    let roll = wink.random();
    let (face, lasts) = match wink.face {
        LogoFace::Open if roll < 0.4 => (LogoFace::Wink, 0.45),
        LogoFace::Open => (LogoFace::Blink, 0.17),
        _ => (LogoFace::Open, 4.5 + roll * 9.0),
    };
    wink.face = face;
    wink.next = Some(clock + lasts);
    tx.wake_after(std::time::Duration::from_secs_f64(lasts));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::model::Platform;

    #[test]
    fn the_default_themes_are_the_designs_own_and_others_are_theirs() {
        let mut s = Settings::defaults(Platform::Linux);
        assert!(std::ptr::eq(theme_for(&s, true), Theme::of(true)));
        s.theme_follows_system = true;
        assert!(std::ptr::eq(theme_for(&s, false), Theme::of(false)));
        // Midnight: 0.10's window and panes, made once.
        s.theme_follows_system = false;
        s.theme = "midnight".into();
        let night = theme_for(&s, false);
        assert!(night.dark && night.frame.hex() == "#0f1013ff" && night.page.hex() == "#0a0b0dff");
        assert!(std::ptr::eq(night, theme_for(&s, true)));
        // The accent takes the design's blue's place, text on it as it reads best.
        s.accent = "#9aa3ad".into();
        let silver = theme_for(&s, true);
        assert_eq!((silver.blue.hex().as_str(), silver.ring.hex().as_str()), ("#9aa3adff", "#9aa3adff"));
        assert_eq!(silver.on_color, Color::BLACK);
        s.theme = "ssheesh".into();
        let design = theme_for(&s, true);
        assert_eq!(design.frame, Theme::of(true).frame);
        assert_eq!(design.blue.hex(), "#9aa3adff");
    }

    #[test]
    fn words_in_any_accent_read_on_any_theme() {
        let mut s = Settings::defaults(Platform::Linux);
        for theme in ["ssheesh", "midnight"] {
            for accent in ["#ffd60a", "#9aa3ad", "#0a84ff", "#4eba65"] {
                s.theme = theme.into();
                s.accent = accent.into();
                for dark in [true, false] {
                    let t = theme_for(&s, dark);
                    assert_eq!(t.legible(t.accent_text), t.accent_text);
                    for g in [t.page, t.frame].into_iter().filter(|g| g.a >= 1.0) {
                        let ratio = crate::theme::contrast(t.accent_text, g);
                        assert!(ratio >= 4.5, "{theme} {accent} dark={dark}: {ratio:.2}:1 on {}", g.hex());
                    }
                }
            }
        }
    }
}
