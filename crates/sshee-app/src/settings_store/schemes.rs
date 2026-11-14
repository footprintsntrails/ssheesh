//! Appearance presets: 0.10's terminal colour schemes, its app themes as far as 1.0 needs them
//! (which are light and which dark), and how a saved choice is read. A port of the data in
//! 0.10's `src/lib/themes.ts`; drawing them is the screen layer's.
//!
//! 1.0 has one look in two appearances (the design's dark and light) where 0.10 had five
//! themes. The settings stay 0.10's: `themeFollowsSystem`, `theme`, `lightTheme`, `darkTheme`.
//! [`Appearance::of`] reads them as "follow the system", "light" or "dark", and
//! [`Appearance::write`] writes a choice back so that 0.10 would show a light or a dark theme
//! for it too.

use crate::model::{CustomTheme, Settings, TerminalColors, DEFAULT_THEME};

/// The built-in light theme of 0.10.
const DAYLIGHT: &str = "daylight";
/// 0.10's built-in themes, with their names.
pub const APP_THEMES: [(&str, &str); 5] =
    [("ssheesh", "SSHeesh Default"), ("midnight", "Midnight"), ("graphite", "Graphite"), ("oled", "OLED"), ("daylight", "Daylight")];
/// 0.10's accent colours, by name.
pub const ACCENTS: [(&str, &str); 7] = [
    ("Green", "#4eba65"),
    ("Blue", "#5b9dff"),
    ("Indigo", "#7c83ff"),
    ("Violet", "#b07cff"),
    ("Mint", "#2bc4b4"),
    ("Silver", "#9aa3ad"),
    ("Terracotta", "#d77757"),
];
/// 0.10's terminal fonts, by id. "system" is SF Mono on a Mac.
pub const TERMINAL_FONTS: [(&str, &str); 5] =
    [("system", "System"), ("jetbrains", "JetBrains Mono"), ("menlo", "Menlo"), ("cascadia", "Cascadia Mono"), ("consolas", "Consolas")];

/// `#rrggbb` or `#rrggbbaa` (the `#` may be missing) as `0xRRGGBBAA`; six digits are solid.
pub fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => u32::from_str_radix(digits, 16).ok().map(|rgb| (rgb << 8) | 0xff),
        8 => u32::from_str_radix(digits, 16).ok(),
        _ => None,
    }
}

/// Whether black text reads better on `rgba` than white (0.10's `isLightColor`, WCAG's
/// relative luminance).
pub fn is_light(rgba: u32) -> bool {
    let lin = |byte: u32| {
        let v = f64::from(byte & 0xff) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * lin(rgba >> 24) + 0.7152 * lin(rgba >> 16) + 0.0722 * lin(rgba >> 8);
    (l + 0.05) / 0.05 > 1.05 / (l + 0.05)
}

/// Whether the 0.10 theme `id` is a light one: Daylight, or a theme of your own whose window
/// is light (its own colour, else its base's). An id nobody knows is the default, dark.
pub fn theme_is_light(id: &str, custom: &[CustomTheme]) -> bool {
    match custom.iter().find(|t| t.id == id) {
        Some(theme) => match theme.colors.get("window").and_then(|c| parse_hex(c)) {
            Some(window) => is_light(window),
            None => theme.base == DAYLIGHT,
        },
        None => id == DAYLIGHT,
    }
}

/// A 0.10 theme's name, for a line that says what was brought over.
pub fn theme_name<'a>(id: &str, custom: &'a [CustomTheme]) -> &'a str {
    if let Some(theme) = custom.iter().find(|t| t.id == id) {
        return &theme.name;
    }
    APP_THEMES.iter().find(|(known, _)| *known == id).map_or(APP_THEMES[0].1, |(_, name)| name)
}

/// How the window picks between its light and its dark look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    /// As the system is set, switching when it does.
    System,
    Light,
    Dark,
}

impl Appearance {
    /// The value Settings' control uses for each.
    pub const CHOICES: [(Appearance, &'static str, &'static str); 3] =
        [(Appearance::System, "system", "Match my system"), (Appearance::Light, "light", "Light"), (Appearance::Dark, "dark", "Dark")];

    pub fn as_str(self) -> &'static str {
        Self::CHOICES.iter().find(|(a, _, _)| *a == self).map_or("system", |(_, value, _)| value)
    }

    pub fn parse(value: &str) -> Option<Appearance> {
        Self::CHOICES.iter().find(|(_, v, _)| *v == value).map(|(a, _, _)| *a)
    }

    /// What 0.10's theme settings come to in 1.0.
    pub fn of(s: &Settings) -> Appearance {
        if s.theme_follows_system {
            Appearance::System
        } else if theme_is_light(&s.theme, &s.custom_themes) {
            Appearance::Light
        } else {
            Appearance::Dark
        }
    }

    /// Whether the window is dark, on a system that is (`system_dark`) or isn't.
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Appearance::System => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }

    /// Writes the choice into 0.10's settings: following the system or not, and for light or
    /// dark, the theme picked for that side in 0.10 (`lightTheme`, `darkTheme`) when it is one
    /// of that kind, else Daylight or the default. So a rollback shows the same choice.
    pub fn write(self, s: &mut Settings) {
        s.theme_follows_system = self == Appearance::System;
        let side = |picked: &str, light: bool, fallback: &str| {
            if theme_is_light(picked, &s.custom_themes) == light && known(picked, &s.custom_themes) { picked.to_owned() } else { fallback.to_owned() }
        };
        match self {
            Appearance::System => {}
            Appearance::Light => s.theme = side(&s.light_theme, true, DAYLIGHT),
            Appearance::Dark => s.theme = side(&s.dark_theme, false, DEFAULT_THEME),
        }
    }
}

fn known(id: &str, custom: &[CustomTheme]) -> bool {
    APP_THEMES.iter().any(|(known, _)| *known == id) || custom.iter().any(|t| t.id == id)
}

/// One terminal scheme's colours, as `0xRRGGBB` (the selection as `0xRRGGBBAA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Colors {
    pub background: u32,
    pub foreground: u32,
    pub cursor: u32,
    /// The text under a block cursor.
    pub cursor_accent: u32,
    pub selection: u32,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then their bright forms.
    pub ansi: [u32; 16],
}

/// A terminal colour scheme of 0.10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalScheme {
    pub id: &'static str,
    pub name: &'static str,
    pub light: bool,
    pub colors: Colors,
}

/// The scheme that follows the app's look.
pub const AUTO: &str = "auto";
/// The scheme with your own background and text colour; the rest follows the app's look.
pub const OWN: &str = "own";

/// 0.10's schemes (well-known open palettes, all MIT-licensed upstream), in its order.
pub const TERMINAL_SCHEMES: [TerminalScheme; 9] = [
    TerminalScheme {
        id: "sshee-night",
        name: "SSHeesh Night",
        light: false,
        colors: Colors {
            background: 0x0a0b0d,
            foreground: 0xd8dce2,
            cursor: 0x5b9dff,
            cursor_accent: 0x0a0b0d,
            selection: 0x5b9dff52,
            ansi: [
                0x1c1e22, 0xff6b6b, 0x4eba65, 0xf5c451, 0x5b9dff, 0xc792ea, 0x4fd1c5, 0xc9cdd3, 0x5f656e, 0xff8a85, 0x74c98a, 0xffd479, 0x86b6ff, 0xdbaaf7,
                0x7be0d6, 0xf5f6f8,
            ],
        },
    },
    TerminalScheme {
        id: "sshee-day",
        name: "SSHeesh Day",
        light: true,
        colors: Colors {
            background: 0xffffff,
            foreground: 0x1f2328,
            cursor: 0x2f6fdb,
            cursor_accent: 0xffffff,
            selection: 0x2f6fdb33,
            ansi: [
                0x24292f, 0xcf222e, 0x1a7f37, 0x9a6700, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781, 0x57606a, 0xa40e26, 0x2da44e, 0xbf8700, 0x218bff, 0xa475f9,
                0x3192aa, 0x8c959f,
            ],
        },
    },
    TerminalScheme {
        id: "tokyo-night",
        name: "Tokyo Night",
        light: false,
        colors: Colors {
            background: 0x1a1b26,
            foreground: 0xc0caf5,
            cursor: 0xc0caf5,
            cursor_accent: 0x1a1b26,
            selection: 0x33467cff,
            ansi: [
                0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7,
                0x7dcfff, 0xc0caf5,
            ],
        },
    },
    TerminalScheme {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        light: false,
        colors: Colors {
            background: 0x1e1e2e,
            foreground: 0xcdd6f4,
            cursor: 0xf5e0dc,
            cursor_accent: 0x1e1e2e,
            selection: 0x585b70ff,
            ansi: [
                0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de, 0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7,
                0x94e2d5, 0xa6adc8,
            ],
        },
    },
    TerminalScheme {
        id: "dracula",
        name: "Dracula",
        light: false,
        colors: Colors {
            background: 0x282a36,
            foreground: 0xf8f8f2,
            cursor: 0xf8f8f2,
            cursor_accent: 0x282a36,
            selection: 0x44475aff,
            ansi: [
                0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, 0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df,
                0xa4ffff, 0xffffff,
            ],
        },
    },
    TerminalScheme {
        id: "nord",
        name: "Nord",
        light: false,
        colors: Colors {
            background: 0x2e3440,
            foreground: 0xd8dee9,
            cursor: 0xd8dee9,
            cursor_accent: 0x2e3440,
            selection: 0x434c5eff,
            ansi: [
                0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0, 0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead,
                0x8fbcbb, 0xeceff4,
            ],
        },
    },
    TerminalScheme {
        id: "one-dark",
        name: "One Dark",
        light: false,
        colors: Colors {
            background: 0x282c34,
            foreground: 0xabb2bf,
            cursor: 0x528bff,
            cursor_accent: 0x282c34,
            selection: 0x3e4451ff,
            ansi: [
                0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, 0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd,
                0x56b6c2, 0xffffff,
            ],
        },
    },
    TerminalScheme {
        id: "gruvbox-dark",
        name: "Gruvbox Dark",
        light: false,
        colors: Colors {
            background: 0x282828,
            foreground: 0xebdbb2,
            cursor: 0xebdbb2,
            cursor_accent: 0x282828,
            selection: 0x504945ff,
            ansi: [
                0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b,
                0x8ec07c, 0xebdbb2,
            ],
        },
    },
    TerminalScheme {
        id: "solarized-dark",
        name: "Solarized Dark",
        light: false,
        colors: Colors {
            background: 0x002b36,
            foreground: 0x839496,
            cursor: 0x93a1a1,
            cursor_accent: 0x002b36,
            selection: 0x073642ff,
            ansi: [
                0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, 0x586e75, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4,
                0x93a1a1, 0xfdf6e3,
            ],
        },
    },
];

/// What a saved `terminalScheme` means for a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    /// The app's own terminal colours, light or dark with the window.
    MatchApp,
    /// One of 0.10's schemes, the same in a light and a dark window.
    Fixed(&'static TerminalScheme),
    /// The app's colours with your own background and text (`0xRRGGBB`).
    Own { background: u32, foreground: u32 },
}

/// Reads a saved scheme id. One nobody knows, or "own" without both colours, follows the app
/// (as 0.10's `resolveScheme` does).
pub fn resolve(id: &str, own: Option<&TerminalColors>) -> Resolved {
    if let Some(scheme) = TERMINAL_SCHEMES.iter().find(|s| s.id == id) {
        return Resolved::Fixed(scheme);
    }
    if id == OWN {
        let colors = own.and_then(|c| Some((parse_hex(&c.background)?, parse_hex(&c.foreground)?)));
        if let Some((background, foreground)) = colors {
            return Resolved::Own { background: background >> 8, foreground: foreground >> 8 };
        }
    }
    Resolved::MatchApp
}

/// The schemes Settings offers, in 0.10's order: `(id, name)`.
pub fn scheme_choices() -> Vec<(&'static str, &'static str)> {
    let mut choices = vec![(AUTO, "Match app")];
    choices.extend(TERMINAL_SCHEMES.iter().map(|s| (s.id, s.name)));
    choices.push((OWN, "Your own"));
    choices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Platform;

    fn settings() -> Settings {
        Settings::defaults(Platform::Linux)
    }

    fn custom(id: &str, base: &str, window: Option<&str>) -> CustomTheme {
        let mut theme = CustomTheme { id: id.into(), name: format!("My {base}"), base: base.into(), ..CustomTheme::default() };
        if let Some(window) = window {
            theme.colors.insert("window".into(), window.into());
        }
        theme
    }

    #[test]
    fn hex_colours_read_with_and_without_alpha() {
        assert_eq!(parse_hex("#1f2937"), Some(0x1f2937ff));
        assert_eq!(parse_hex(" 1F2937 "), Some(0x1f2937ff));
        assert_eq!(parse_hex("#1f293780"), Some(0x1f293780));
        for bad in ["", "#12345", "#12345g", "red", "#1234567", "rgb(1,2,3)"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
        assert!(is_light(0xffffffff) && is_light(0xeceef2ff) && is_light(0x9aa3adff));
        assert!(!is_light(0x000000ff) && !is_light(0x1f1e1dff) && !is_light(0x0071e3ff));
    }

    #[test]
    fn the_0_10_themes_come_to_follow_light_or_dark() {
        let mut s = settings();
        // A fresh 0.10: SSHeesh Default, not following the system.
        assert_eq!(Appearance::of(&s), Appearance::Dark);
        for (theme, want) in [("midnight", Appearance::Dark), ("graphite", Appearance::Dark), ("oled", Appearance::Dark), ("daylight", Appearance::Light)] {
            s.theme = theme.into();
            assert_eq!(Appearance::of(&s), want, "{theme}");
        }
        // A theme nobody knows is the default, as 0.10 shows it.
        s.theme = "custom-gone".into();
        assert_eq!(Appearance::of(&s), Appearance::Dark);
        // Your own themes go by their window: their own colour, else their base's.
        s.custom_themes = vec![custom("custom-a", "midnight", Some("#f4f1e8")), custom("custom-b", "daylight", None), custom("custom-c", "daylight", Some("#101010"))];
        for (theme, want) in [("custom-a", Appearance::Light), ("custom-b", Appearance::Light), ("custom-c", Appearance::Dark)] {
            s.theme = theme.into();
            assert_eq!(Appearance::of(&s), want, "{theme}");
        }
        assert_eq!(theme_name("custom-a", &s.custom_themes), "My midnight");
        assert_eq!(theme_name("oled", &s.custom_themes), "OLED");
        assert_eq!(theme_name("custom-gone", &s.custom_themes), "SSHeesh Default");
        // Following the system wins over whatever theme is picked.
        s.theme_follows_system = true;
        assert_eq!(Appearance::of(&s), Appearance::System);
        assert!(Appearance::System.dark(true) && !Appearance::System.dark(false));
        assert!(!Appearance::Light.dark(true) && Appearance::Dark.dark(false));
    }

    #[test]
    fn a_choice_is_written_so_0_10_would_show_the_same() {
        let mut s = settings();
        Appearance::Light.write(&mut s);
        assert_eq!((s.theme.as_str(), s.theme_follows_system), ("daylight", false));
        assert_eq!(Appearance::of(&s), Appearance::Light);
        // Back to dark: the theme 0.10 had for the dark side.
        s.dark_theme = "graphite".into();
        Appearance::Dark.write(&mut s);
        assert_eq!(s.theme, "graphite");
        // A side's theme that isn't of that kind, or isn't there anymore, gives way.
        s.dark_theme = "daylight".into();
        Appearance::Dark.write(&mut s);
        assert_eq!(s.theme, "ssheesh");
        s.light_theme = "custom-gone".into();
        Appearance::Light.write(&mut s);
        assert_eq!(s.theme, "daylight");
        // Your own light theme for the light side is kept.
        s.custom_themes = vec![custom("custom-a", "midnight", Some("#f4f1e8"))];
        s.light_theme = "custom-a".into();
        Appearance::Light.write(&mut s);
        assert_eq!(s.theme, "custom-a");
        // Following the system leaves the picked theme alone.
        Appearance::System.write(&mut s);
        assert_eq!((s.theme.as_str(), s.theme_follows_system), ("custom-a", true));
        for (a, value, _) in Appearance::CHOICES {
            assert_eq!(Appearance::parse(value), Some(a));
            assert_eq!(a.as_str(), value);
        }
        assert_eq!(Appearance::parse("sepia"), None);
    }

    #[test]
    fn terminal_schemes_are_0_10s() {
        let ids: Vec<&str> = scheme_choices().iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            ["auto", "sshee-night", "sshee-day", "tokyo-night", "catppuccin-mocha", "dracula", "nord", "one-dark", "gruvbox-dark", "solarized-dark", "own"]
        );
        let Resolved::Fixed(dracula) = resolve("dracula", None) else { panic!() };
        assert_eq!((dracula.name, dracula.colors.background, dracula.colors.ansi[1], dracula.colors.ansi[15]), ("Dracula", 0x282a36, 0xff5555, 0xffffff));
        let Resolved::Fixed(day) = resolve("sshee-day", None) else { panic!() };
        assert!(day.light && TERMINAL_SCHEMES.iter().filter(|s| s.light).count() == 1);
        // Each scheme's lightness is its background's.
        for scheme in &TERMINAL_SCHEMES {
            assert_eq!(is_light((scheme.colors.background << 8) | 0xff), scheme.light, "{}", scheme.id);
        }
        assert_eq!(resolve("auto", None), Resolved::MatchApp);
        assert_eq!(resolve("monokai", None), Resolved::MatchApp);
        // Your own: both colours, or the app's.
        let own = TerminalColors { background: "#101418".into(), foreground: "#e6e6e6".into(), ..TerminalColors::default() };
        assert_eq!(resolve("own", Some(&own)), Resolved::Own { background: 0x101418, foreground: 0xe6e6e6 });
        assert_eq!(resolve("own", None), Resolved::MatchApp);
        let half = TerminalColors { background: "#101418".into(), foreground: "white".into(), ..TerminalColors::default() };
        assert_eq!(resolve("own", Some(&half)), Resolved::MatchApp);
    }
}
