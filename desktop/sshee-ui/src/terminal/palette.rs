//! The colour scheme: theme "auto" follows the system's light or dark, with 0.10's
//! SSHeesh Night and SSHeesh Day (src/lib/themes.ts).

use rune::Color;
use sshee_term::engine::{AnswerColors, Color as TermColor, Rgb};

/// One scheme's colours, as bytes (what a program asking gets) and as Rune colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    /// The text under a block cursor.
    pub cursor_accent: Rgb,
    pub ansi: [Rgb; 16],
    pub selection: Color,
    pub selection_inactive: Color,
    /// The accent a hidden secret's dots and pill take.
    pub accent: Color,
    bg: Color,
    fg: Color,
    table: Vec<Color>,
}

const fn hex(v: u32) -> Rgb {
    Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// SSHeesh Night.
const NIGHT: [Rgb; 20] = [
    hex(0x0a0b0d), // background
    hex(0xd8dce2), // foreground
    hex(0x5b9dff), // cursor
    hex(0x0a0b0d), // cursor accent
    hex(0x1c1e22), hex(0xff6b6b), hex(0x4eba65), hex(0xf5c451), hex(0x5b9dff), hex(0xc792ea), hex(0x4fd1c5), hex(0xc9cdd3),
    hex(0x5f656e), hex(0xff8a85), hex(0x74c98a), hex(0xffd479), hex(0x86b6ff), hex(0xdbaaf7), hex(0x7be0d6), hex(0xf5f6f8),
];

/// SSHeesh Day.
const DAY: [Rgb; 20] = [
    hex(0xffffff),
    hex(0x1f2328),
    hex(0x2f6fdb),
    hex(0xffffff),
    hex(0x24292f), hex(0xcf222e), hex(0x1a7f37), hex(0x9a6700), hex(0x0969da), hex(0x8250df), hex(0x1b7c83), hex(0x6e7781),
    hex(0x57606a), hex(0xa40e26), hex(0x2da44e), hex(0xbf8700), hex(0x218bff), hex(0xa475f9), hex(0x3192aa), hex(0x8c959f),
];

/// A byte colour as Rune's (linear light, straight alpha).
pub fn rgb(c: Rgb) -> Color {
    let d = |v: u8| rune::color::srgb_decode(f32::from(v) / 255.0);
    Color { r: d(c.r), g: d(c.g), b: d(c.b), a: 1.0 }
}

/// `a` mixed `t` of the way to `b` in sRGB-encoded values, as a browser blends (0.10's dim
/// text was drawn at half opacity over the background).
pub fn mix_srgb(a: Color, b: Color, t: f32) -> Color {
    let e = rune::color::srgb_encode;
    let d = rune::color::srgb_decode;
    let m = |x: f32, y: f32| d(e(x) + (e(y) - e(x)) * t);
    Color { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b), a: a.a + (b.a - a.a) * t }
}

/// Entry `i` of xterm's 256 colours beyond the sixteen: the 6×6×6 cube, then the grey ramp.
pub fn xterm_256(i: u8) -> Rgb {
    match i {
        16..=231 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            Rgb::new(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232);
            Rgb::new(v, v, v)
        }
        _ => Rgb::new(0, 0, 0),
    }
}

impl Palette {
    pub fn new(dark: bool) -> Palette {
        let src = if dark { &NIGHT } else { &DAY };
        let mut ansi = [Rgb::default(); 16];
        ansi.copy_from_slice(&src[4..20]);
        let table = (0..=255u8).map(|i| rgb(if i < 16 { ansi[i as usize] } else { xterm_256(i) })).collect();
        let (selection, selection_inactive) = if dark {
            (rgb(hex(0x5b9dff)).alpha(0.32), Color::WHITE.alpha(0.12))
        } else {
            (rgb(hex(0x2f6fdb)).alpha(0.2), Color::BLACK.alpha(0.08))
        };
        Palette {
            dark,
            background: src[0],
            foreground: src[1],
            cursor: src[2],
            cursor_accent: src[3],
            ansi,
            selection,
            selection_inactive,
            accent: rgb(src[2]),
            bg: rgb(src[0]),
            fg: rgb(src[1]),
            table,
        }
    }

    pub fn bg(&self) -> Color {
        self.bg
    }

    pub fn fg(&self) -> Color {
        self.fg
    }

    /// A program's colour as drawn.
    pub fn color(&self, c: TermColor) -> Color {
        match c {
            TermColor::Foreground => self.fg,
            TermColor::Background => self.bg,
            TermColor::Indexed(i) => self.table[i as usize],
            TermColor::Rgb(v) => rgb(v),
        }
    }

    /// What a program asking for the colours (OSC 4/10/11/12) is told.
    pub fn answer_colors(&self) -> AnswerColors {
        AnswerColors { foreground: self.foreground, background: self.background, cursor: self.cursor, ansi: self.ansi }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_256_colours() {
        assert_eq!(xterm_256(16), Rgb::new(0, 0, 0));
        assert_eq!(xterm_256(21), Rgb::new(0, 0, 255));
        assert_eq!(xterm_256(196), Rgb::new(255, 0, 0));
        assert_eq!(xterm_256(231), Rgb::new(255, 255, 255));
        assert_eq!(xterm_256(232), Rgb::new(8, 8, 8));
        assert_eq!(xterm_256(255), Rgb::new(238, 238, 238));
        let p = Palette::new(true);
        assert_eq!(p.color(TermColor::Indexed(1)), rgb(Rgb::new(0xff, 0x6b, 0x6b)));
        assert_eq!(p.color(TermColor::Indexed(196)), rgb(Rgb::new(255, 0, 0)));
    }

    #[test]
    fn answers_follow_the_scheme() {
        assert_eq!(Palette::new(true).answer_colors().background, Rgb::new(0x0a, 0x0b, 0x0d));
        assert_eq!(Palette::new(false).answer_colors().background, Rgb::new(0xff, 0xff, 0xff));
    }

    #[test]
    fn dim_is_half_way_to_the_background_as_a_browser_blends() {
        let p = Palette::new(true);
        let half = mix_srgb(p.fg(), p.bg(), 0.5);
        let enc = rune::color::srgb_encode(half.r) * 255.0;
        assert!((enc - (0xd8 as f32 + 0x0a as f32) / 2.0).abs() < 0.6, "{enc}");
    }
}
