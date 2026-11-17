//! Themes, themes of your own and the accent colour, as rules: a port of 0.10's
//! `src/lib/themes.ts`, `src/lib/color.ts` and the theme editor's rules (`ThemeEditor.tsx`,
//! `ColorPicker.tsx`). Drawing them is the screen layer's (`sshee-ui`'s `looks`).
//!
//! A theme is fourteen colours by 0.10's keys (`window`, `canvas`, `raised`, `ink`, `fg`…).
//! Midnight, Graphite and OLED are 0.10's to the digit. SSHeesh Default and Daylight are the
//! 1.0 design's dark and light, written in the same fourteen keys, so the default look is the
//! design's and a theme of your own started from them changes only what you change. A theme
//! of your own keeps only the colours you changed, over its base's (0.10's `CustomTheme`), and
//! goes between computers as 0.10's text: `{ "ssheesh": "theme", "version": 1, … }`.
//!
//! Colours are `0xRRGGBBAA`, as [`parse_hex`] reads them.

use indexmap::IndexMap;

use super::schemes::{is_light, parse_hex, APP_THEMES};
use super::SettingsStore;
use crate::model::{CustomTheme, Json, JsonMap, Settings, DEFAULT_ACCENT, DEFAULT_THEME};

/// One colour a theme has, as the editor lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorKey {
    /// Its key in a theme (0.10's `ThemeColorKey`).
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    /// It may be see-through.
    pub alpha: bool,
}

const fn key(key: &'static str, group: &'static str, label: &'static str, hint: &'static str, alpha: bool) -> ColorKey {
    ColorKey { key, group, label, hint, alpha }
}

/// Every colour of a theme, in the editor's order. What each paints in 1.0 is in its hint.
pub const COLOR_KEYS: [ColorKey; 14] = [
    key("window", "Window and panels", "Window", "Behind everything: the sidebar, the top row and the edges.", false),
    key("canvas", "Window and panels", "Panes", "Terminals (with the Match app color scheme), Settings and the main menu. See-through panes take on the window's color.", true),
    key("raised", "Window and panels", "Raised controls", "The picked option in segmented controls, and the tab you're on in a light theme.", false),
    key("ink", "Window and panels", "Lines and shading", "Borders, dividers and hover tints, drawn faintly. Picks white or black to suit the window until you change it.", false),
    key("fg", "Text", "Text", "Names, titles and everything you read first.", false),
    key("fg2", "Text", "Secondary text", "Details next to the main text, and help under a setting.", false),
    key("fg3", "Text", "Hints", "Small print on what's picked or under the pointer.", false),
    key("ok", "Status colors", "Good", "Connected, healthy, done, and a switch that is on.", false),
    key("warn", "Status colors", "Needs attention", "A terminal waiting for you, a disk nearly full.", false),
    key("bad", "Status colors", "Problem", "Errors, lost connections and Production servers.", false),
    key("glass", "Glass", "Menus and dialogs", "Menus, dialogs, the command bar and notices. Solid unless you make it see-through.", true),
    key("fileCode", "Files", "Code", "Scripts, configs and .env files in the file manager.", true),
    key("fileImage", "Files", "Images", "Pictures, icons and videos.", true),
    key("fileArchive", "Files", "Archives", "Zip, tar and other packed files.", true),
];

/// The place of `key` among [`COLOR_KEYS`].
pub fn key_index(key: &str) -> Option<usize> {
    COLOR_KEYS.iter().position(|k| k.key == key)
}

/// A theme's fourteen colours, in [`COLOR_KEYS`]' order.
pub type Colors = [u32; 14];

const fn rgb(v: u32) -> u32 {
    (v << 8) | 0xff
}

const DARK_STATUS: [u32; 3] = [rgb(0x4eba65), rgb(0xf5b841), rgb(0xff6159)];
const DARK_FILES: [u32; 3] = [0xc792eacc, 0xf5c451cc, 0xff9f6bcc];
const LIGHT_FILES: [u32; 3] = [rgb(0x8250df), rgb(0xa16207), rgb(0xc2410c)];

/// The colours of a built-in theme: Midnight, Graphite and OLED as 0.10 had them; SSHeesh
/// Default and Daylight as the 1.0 design's dark and light (`sshee-ui`'s `theme.rs`). An id
/// nobody knows is `None`.
pub fn built_in(id: &str) -> Option<Colors> {
    let c = |window, canvas, raised, ink, fg, fg2, fg3, status: [u32; 3], glass, files: [u32; 3]| {
        [rgb(window), rgb(canvas), rgb(raised), rgb(ink), rgb(fg), rgb(fg2), rgb(fg3), status[0], status[1], status[2], rgb(glass), files[0], files[1], files[2]]
    };
    Some(match id {
        // The 1.0 design's dark: its frame, page, picked segment, text and status colours.
        "ssheesh" => c(0x28282a, 0x1c1c1e, 0x636366, 0xffffff, 0xebebf0, 0xa1a1a6, 0xc7c7cc, [rgb(0x4eba65), rgb(0xff9f0a), rgb(0xff6961)], 0x2c2c2e, DARK_FILES),
        "midnight" => c(0x0f1013, 0x0a0b0d, 0x1a1c20, 0xffffff, 0xeceef1, 0xa2a8b1, 0x8a919b, DARK_STATUS, 0x1f2126, DARK_FILES),
        "graphite" => c(0x161616, 0x101010, 0x222222, 0xffffff, 0xededed, 0xa6a6a6, 0x8e8e8e, DARK_STATUS, 0x282828, DARK_FILES),
        "oled" => c(0x000000, 0x000000, 0x111111, 0xffffff, 0xf2f2f2, 0xa0a0a0, 0x8a8a8a, DARK_STATUS, 0x161616, DARK_FILES),
        // The 1.0 design's light.
        "daylight" => c(0xececee, 0xffffff, 0xffffff, 0x000000, 0x1d1d1f, 0x636366, 0x515154, [rgb(0x248a3d), rgb(0xa84a00), rgb(0xc00012)], 0xf6f6f8, LIGHT_FILES),
        _ => return None,
    })
}

/// Whether `id` is one of the built-in themes.
pub fn is_built_in(id: &str) -> bool {
    built_in(id).is_some()
}

/// A theme as it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub id: String,
    pub name: String,
    pub colors: Colors,
    /// Its window is light.
    pub light: bool,
    /// It is exactly the 1.0 design's look, dark (`Some(true)`) or light: SSHeesh Default or
    /// Daylight with nothing changed. The screen then draws the design's own tokens.
    pub design: Option<bool>,
}

/// Every colour of a theme of your own: its own on top of its base's. "Lines and shading"
/// follows the window (white on dark, black on light) until set (0.10's `themeColors`).
pub fn custom_colors(t: &CustomTheme) -> Colors {
    let mut colors = built_in(&t.base).or_else(|| built_in(DEFAULT_THEME)).expect("the default theme is built in");
    for (i, k) in COLOR_KEYS.iter().enumerate() {
        if let Some(c) = t.colors.get(k.key).and_then(|v| parse_hex(v)) {
            colors[i] = if k.alpha { c } else { c | 0xff };
        }
    }
    if t.colors.get("ink").and_then(|v| parse_hex(v)).is_none() {
        colors[3] = if is_light(colors[0]) { rgb(0x000000) } else { rgb(0xffffff) };
    }
    colors
}

/// The theme `id` (built in or yours) as it is drawn; an id nobody knows is the default.
pub fn resolve(id: &str, custom: &[CustomTheme]) -> Resolved {
    if let Some(t) = custom.iter().find(|t| t.id == id) {
        let colors = custom_colors(t);
        let design = (custom_colors(&CustomTheme { colors: IndexMap::new(), ..t.clone() }) == colors && matches!(t.base.as_str(), "ssheesh" | "daylight"))
            .then(|| t.base == DEFAULT_THEME);
        return Resolved { id: t.id.clone(), name: t.name.clone(), colors, light: is_light(colors[0]), design };
    }
    let id = if is_built_in(id) { id } else { DEFAULT_THEME };
    let colors = built_in(id).expect("a built-in theme");
    let design = match id {
        "ssheesh" => Some(true),
        "daylight" => Some(false),
        _ => None,
    };
    let name = APP_THEMES.iter().find(|(known, _)| *known == id).map_or("SSHeesh Default", |(_, name)| name);
    Resolved { id: id.to_owned(), name: name.to_owned(), colors, light: is_light(colors[0]), design }
}

/// The theme on screen for a window that is dark (`dark`) or light: while Settings follows
/// the system, the one picked for that side (0.10's `lightTheme`, `darkTheme`), else `theme`.
pub fn on_screen(s: &Settings, dark: bool) -> Resolved {
    let id = match (s.theme_follows_system, dark) {
        (true, true) => &s.dark_theme,
        (true, false) => &s.light_theme,
        (false, _) => &s.theme,
    };
    resolve(id, &s.custom_themes)
}

/// Every theme the picker shows: the built-in ones, then yours.
pub fn all(s: &Settings) -> Vec<Resolved> {
    APP_THEMES.iter().map(|(id, _)| resolve(id, &s.custom_themes)).chain(s.custom_themes.iter().map(|t| resolve(&t.id, &s.custom_themes))).collect()
}

// ---------------------------------------------------------------- the accent

/// The accents to pick from (0.10's, none that reads as a status). The first is the default:
/// 0.10's green value, drawn as the 1.0 design's blue, the colour of what is picked and of
/// the keyboard's focus.
pub const ACCENTS: [(&str, &str); 7] = [
    ("Default", DEFAULT_ACCENT),
    ("Blue", "#5b9dff"),
    ("Indigo", "#7c83ff"),
    ("Violet", "#b07cff"),
    ("Mint", "#2bc4b4"),
    ("Silver", "#9aa3ad"),
    ("Terracotta", "#d77757"),
];

/// The 1.0 design's blue: the default accent's colour.
pub const DESIGN_BLUE: u32 = 0x0071e3ff;

/// The accent as it is drawn: `None` for the default (the design's own blue).
pub fn accent(s: &Settings) -> Option<u32> {
    let value = s.accent.trim();
    if value.eq_ignore_ascii_case(DEFAULT_ACCENT) {
        return None;
    }
    parse_hex(value).map(|c| c | 0xff)
}

/// Whether the accent is one of the presets (else it's your own colour).
pub fn accent_is_preset(value: &str) -> bool {
    ACCENTS.iter().any(|(_, v)| v.eq_ignore_ascii_case(value.trim()))
}

/// What a swatch for `value` shows: the default's design blue, else the colour itself.
pub fn accent_swatch(value: &str) -> u32 {
    if value.trim().eq_ignore_ascii_case(DEFAULT_ACCENT) {
        DESIGN_BLUE
    } else {
        parse_hex(value).map_or(DESIGN_BLUE, |c| c | 0xff)
    }
}

// ---------------------------------------------------------------- colour

/// `#rrggbb`, or `#rrggbbaa` when not solid.
pub fn to_hex(c: u32) -> String {
    if c & 0xff == 0xff {
        format!("#{:06x}", c >> 8)
    } else {
        format!("#{c:08x}")
    }
}

fn channels(c: u32) -> [f64; 4] {
    [(c >> 24) & 0xff, (c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| f64::from(v) / 255.0)
}

fn pack([r, g, b, a]: [f64; 4]) -> u32 {
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(r) << 24) | (byte(g) << 16) | (byte(b) << 8) | byte(a)
}

/// `top` laid over `bottom` (which is solid).
pub fn over(top: u32, bottom: u32) -> u32 {
    let (t, b) = (channels(top), channels(bottom));
    let a = t[3];
    pack([t[0] * a + b[0] * (1.0 - a), t[1] * a + b[1] * (1.0 - a), t[2] * a + b[2] * (1.0 - a), 1.0])
}

/// WCAG's contrast ratio between two solid colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    let lum = |c: u32| {
        let lin = |v: f64| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        let [r, g, b, _] = channels(c);
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Hue (0 to 360), saturation and value (0 to 1): what the colour picker moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub h: f64,
    pub s: f64,
    pub v: f64,
}

pub fn to_hsv(c: u32) -> Hsv {
    let [r, g, b, _] = channels(c);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    Hsv { h, s: if max == 0.0 { 0.0 } else { d / max }, v: max }
}

/// The colour at `hsv`, `alpha` solid (0 to 1).
pub fn from_hsv(hsv: Hsv, alpha: f64) -> u32 {
    let (h, s, v) = (hsv.h.rem_euclid(360.0), hsv.s.clamp(0.0, 1.0), hsv.v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    pack([r + m, g + m, b + m, alpha])
}

/// How solid a colour is, 0 to 1.
pub fn alpha_of(c: u32) -> f64 {
    f64::from(c & 0xff) / 255.0
}

// ---------------------------------------------------------------- readability

/// A text colour that may be hard to read on a background: below 4.5:1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weak {
    pub text: usize,
    pub on: usize,
    pub ratio: f64,
}

/// What each background is called in a warning (0.10's `ON`).
pub fn background_name(on: usize) -> &'static str {
    match COLOR_KEYS[on].key {
        "window" => "the window",
        "canvas" => "panes",
        "raised" => "raised controls",
        _ => "menus and dialogs",
    }
}

/// Each text colour on each background it sits on, seen over the window, below 4.5:1 (0.10's
/// `weakContrast`).
pub fn weak_contrast(colors: &Colors) -> Vec<Weak> {
    let window = colors[0] | 0xff;
    let mut out = vec![];
    for text in [4, 5, 6] {
        for on in [0, 1, 2, 10] {
            let ratio = contrast(over(colors[text], window), over(colors[on], window));
            if ratio < 4.5 {
                out.push(Weak { text, on, ratio });
            }
        }
    }
    out
}

// ---------------------------------------------------------------- your own themes

/// "My Midnight", or "My Midnight 2" when that's taken.
pub fn free_name(name: &str, list: &[CustomTheme]) -> String {
    if !list.iter().any(|t| t.name == name) {
        return name.to_owned();
    }
    (2..).map(|n| format!("{name} {n}")).find(|n| !list.iter().any(|t| &t.name == n)).unwrap_or_default()
}

/// An id no theme has: `custom-` and a number (0.10 made a random one; either reads).
pub fn new_id(list: &[CustomTheme]) -> String {
    (1..).map(|n| format!("custom-{n}")).find(|id| !list.iter().any(|t| &t.id == id)).unwrap_or_default()
}

/// Picking a theme: while following the system it is also the one for its side (0.10's
/// `choose`).
fn choose(s: &mut Settings, id: &str) {
    s.theme = id.to_owned();
    if s.theme_follows_system {
        let light = resolve(id, &s.custom_themes).light;
        if light {
            s.light_theme = id.to_owned();
        } else {
            s.dark_theme = id.to_owned();
        }
    }
}

/// Settings → Appearance → Theme: shows `id`.
pub fn pick(st: &mut SettingsStore, id: &str) {
    let id = id.to_owned();
    st.set(move |s| choose(s, &id));
}

/// "Duplicate and edit": a copy of the theme on screen (`on_screen`), in use at once so edits
/// show live. Returns its id.
pub fn duplicate(st: &mut SettingsStore, on_screen: &str) -> String {
    let s = st.get();
    let mine = s.custom_themes.iter().find(|t| t.id == on_screen);
    let base = mine.map_or_else(|| if is_built_in(on_screen) { on_screen.to_owned() } else { DEFAULT_THEME.to_owned() }, |t| t.base.clone());
    let name = match mine {
        Some(t) => format!("{} copy", t.name),
        None => format!("My {}", resolve(on_screen, &s.custom_themes).name),
    };
    let id = new_id(&s.custom_themes);
    let theme = CustomTheme {
        id: id.clone(),
        name: free_name(&name, &s.custom_themes),
        base,
        colors: mine.map(|t| t.colors.clone()).unwrap_or_default(),
        extra: JsonMap::new(),
    };
    st.set(|s| {
        s.custom_themes.push(theme);
        choose(s, &id);
    });
    id
}

/// A new colour for `key` of your theme `id`, or `None` for its base's again. Picking the
/// colour it would have anyway keeps no entry, so its Reset shows as done.
pub fn set_color(st: &mut SettingsStore, id: &str, key: &str, color: Option<u32>) {
    let Some(at) = key_index(key) else { return };
    st.set(|s| {
        let Some(t) = s.custom_themes.iter_mut().find(|t| t.id == id) else { return };
        t.colors.shift_remove(key);
        let fallback = custom_colors(t)[at];
        if let Some(c) = color.map(|c| if COLOR_KEYS[at].alpha { c } else { c | 0xff }).filter(|c| *c != fallback) {
            t.colors.insert(key.to_owned(), to_hex(c));
        }
    });
}

/// A new name for your theme `id`, at most 40 characters; an empty one is "My <base>".
pub fn rename(st: &mut SettingsStore, id: &str, name: &str) {
    st.set(|s| {
        let Some(t) = s.custom_themes.iter_mut().find(|t| t.id == id) else { return };
        let name: String = name.chars().take(40).collect();
        t.name = if name.trim().is_empty() { format!("My {}", resolve(&t.base, &[]).name) } else { name };
    });
}

/// "Reset all colors": the base's again.
pub fn reset_colors(st: &mut SettingsStore, id: &str) {
    st.set(|s| {
        if let Some(t) = s.custom_themes.iter_mut().find(|t| t.id == id) {
            t.colors.clear();
        }
    });
}

/// Deletes your theme `id`; whatever used it goes back to its base (0.10's `remove`).
pub fn delete(st: &mut SettingsStore, id: &str) {
    st.set(|s| {
        let Some(at) = s.custom_themes.iter().position(|t| t.id == id) else { return };
        let gone = s.custom_themes.remove(at);
        let base_light = resolve(&gone.base, &[]).light;
        let swap = |v: &mut String, fallback: &str| {
            if v == id {
                *v = fallback.to_owned();
            }
        };
        swap(&mut s.theme, &gone.base);
        swap(&mut s.light_theme, if base_light { &gone.base } else { "daylight" });
        swap(&mut s.dark_theme, if base_light { DEFAULT_THEME } else { &gone.base });
    });
}

/// A theme as text to share: paste it into Import on any computer (0.10's `themeToJson`).
pub fn to_json(t: &CustomTheme) -> String {
    let colors = custom_colors(t);
    let mut map = JsonMap::new();
    for (i, k) in COLOR_KEYS.iter().enumerate() {
        // Left out, "Lines and shading" keeps following the window.
        if k.key == "ink" && !t.colors.contains_key("ink") {
            continue;
        }
        map.insert(k.key.to_owned(), Json::String(to_hex(colors[i])));
    }
    let mut out = JsonMap::new();
    out.insert("ssheesh".into(), Json::from("theme"));
    out.insert("version".into(), Json::from(1u64));
    out.insert("name".into(), Json::from(t.name.as_str()));
    out.insert("base".into(), Json::from(t.base.as_str()));
    out.insert("colors".into(), Json::Object(map));
    Json::Object(out).to_text()
}

/// Reads a theme someone shared, strict about what it knows and quiet about the rest, keeping
/// only colours that differ from the base (0.10's `themeFromJson`, its words). `Err` says
/// what is wrong, in a line for under the field.
pub fn from_json(text: &str) -> Result<(String, String, IndexMap<String, String>), String> {
    let Ok(data) = Json::parse(text) else { return Err("That isn't a theme. Copy all of it, from the first { to the last }.".into()) };
    let Json::Object(o) = data else { return Err("That isn't a theme. It should start with { and end with }.".into()) };
    match o.get("ssheesh") {
        None => {}
        Some(Json::String(s)) if s == "theme" => {}
        Some(_) => return Err("That's something else from SSHeesh, not a theme.".into()),
    }
    let name = match o.get("name") {
        None => String::new(),
        Some(Json::String(s)) => s.clone(),
        Some(_) => return Err("The theme's name should be text in quotes.".into()),
    };
    let base = match o.get("base") {
        None => None,
        Some(Json::String(s)) if is_built_in(s) => Some(s.clone()),
        Some(_) => return Err(format!("\"base\" should be one of {}.", APP_THEMES.map(|(id, _)| id).join(", "))),
    };
    let Some(Json::Object(given)) = o.get("colors") else { return Err("The theme has no colors.".into()) };
    let mut colors: IndexMap<String, String> = IndexMap::new();
    for k in &COLOR_KEYS {
        let Some(v) = given.get(k.key) else { continue };
        let parsed = match v {
            Json::String(s) if s.trim_start().starts_with('#') => parse_hex(s),
            _ => None,
        };
        let Some(c) = parsed else {
            return Err(format!("\"{}\" ({}) should be a color like #1f2937, not {}.", k.key, k.label, v.to_text()));
        };
        if c & 0xff != 0xff && !k.alpha {
            return Err(format!("\"{}\" ({}) can't be see-through. Use six digits after the #.", k.key, k.label));
        }
        colors.insert(k.key.to_owned(), to_hex(c));
    }
    if colors.is_empty() {
        return Err("None of the theme's colors are ones SSHeesh knows.".into());
    }
    let window_light = colors.get("window").and_then(|w| parse_hex(w)).is_some_and(is_light);
    let base = base.unwrap_or_else(|| if window_light { "daylight".into() } else { "midnight".into() });
    let own = built_in(&base).expect("a built-in base");
    colors.retain(|key, value| key == "ink" || key_index(key).is_none_or(|i| to_hex(own[i]) != *value));
    let name: String = name.trim().chars().take(40).collect();
    Ok((if name.is_empty() { "Imported theme".into() } else { name }, base, colors))
}

/// Import: adds a theme someone shared and shows it. Returns its id, or what is wrong.
pub fn import(st: &mut SettingsStore, text: &str) -> Result<String, String> {
    let (name, base, colors) = from_json(text)?;
    let s = st.get();
    let id = new_id(&s.custom_themes);
    let theme = CustomTheme { id: id.clone(), name: free_name(&name, &s.custom_themes), base, colors, extra: JsonMap::new() };
    st.set(|s| {
        s.custom_themes.push(theme);
        choose(s, &id);
    });
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Platform;

    fn store() -> SettingsStore {
        SettingsStore::in_memory(Settings::defaults(Platform::Linux))
    }

    #[test]
    fn built_in_themes_are_0_10s_and_the_defaults_are_the_designs() {
        assert_eq!(APP_THEMES.len(), 5);
        for (id, _) in APP_THEMES {
            assert!(built_in(id).is_some(), "{id}");
        }
        assert_eq!(to_hex(built_in("midnight").unwrap()[0]), "#0f1013");
        assert_eq!(to_hex(built_in("oled").unwrap()[1]), "#000000");
        assert_eq!(to_hex(built_in("graphite").unwrap()[10]), "#282828");
        assert_eq!(resolve("ssheesh", &[]).design, Some(true));
        assert_eq!(resolve("daylight", &[]).design, Some(false));
        assert_eq!(resolve("midnight", &[]).design, None);
        assert!(resolve("daylight", &[]).light && !resolve("oled", &[]).light);
        // An id nobody knows is the default.
        assert_eq!(resolve("sepia", &[]).id, "ssheesh");
    }

    #[test]
    fn a_theme_of_your_own_is_its_base_and_what_you_changed() {
        let mut st = store();
        let id = duplicate(&mut st, "midnight");
        assert_eq!(st.get().theme, id);
        let t = &st.get().custom_themes[0];
        assert_eq!((t.name.as_str(), t.base.as_str()), ("My Midnight", "midnight"));
        // A second copy has a name of its own.
        let second = duplicate(&mut st, "midnight");
        assert_eq!(st.get().custom_themes[1].name, "My Midnight 2");
        assert_ne!(id, second);
        // A colour changed, then put back to the base's: no entry is kept.
        set_color(&mut st, &id, "window", Some(0x223344ff));
        assert_eq!(st.get().custom_themes[0].colors["window"], "#223344");
        assert_eq!(resolve(&id, &st.get().custom_themes).colors[0], 0x223344ff);
        set_color(&mut st, &id, "window", Some(0x0f1013ff));
        assert!(st.get().custom_themes[0].colors.is_empty());
        // A solid key never keeps an alpha; a see-through one does.
        set_color(&mut st, &id, "fg", Some(0x11223344));
        set_color(&mut st, &id, "glass", Some(0x11223344));
        assert_eq!((st.get().custom_themes[0].colors["fg"].as_str(), st.get().custom_themes[0].colors["glass"].as_str()), ("#112233", "#11223344"));
        // A light window turns lines black until they are set.
        set_color(&mut st, &id, "window", Some(0xf0f0f0ff));
        assert_eq!(resolve(&id, &st.get().custom_themes).colors[3], 0x000000ff);
        assert!(resolve(&id, &st.get().custom_themes).light);
        rename(&mut st, &id, "  ");
        assert_eq!(st.get().custom_themes[0].name, "My Midnight");
        reset_colors(&mut st, &id);
        assert!(st.get().custom_themes[0].colors.is_empty());
        // Deleting it puts what used it back on its base.
        delete(&mut st, &id);
        assert_eq!(st.get().custom_themes.len(), 1);
        pick(&mut st, &second);
        delete(&mut st, &second);
        assert_eq!(st.get().theme, "midnight");
    }

    #[test]
    fn a_copy_of_the_default_is_still_the_design_until_a_colour_changes() {
        let mut st = store();
        let id = duplicate(&mut st, "ssheesh");
        assert_eq!(resolve(&id, &st.get().custom_themes).design, Some(true));
        set_color(&mut st, &id, "bad", Some(0xff0000ff));
        assert_eq!(resolve(&id, &st.get().custom_themes).design, None);
    }

    #[test]
    fn picking_a_theme_while_following_the_system_sets_its_side() {
        let mut st = store();
        st.set(|s| s.theme_follows_system = true);
        pick(&mut st, "oled");
        assert_eq!((st.get().dark_theme.as_str(), st.get().theme.as_str()), ("oled", "oled"));
        assert_eq!(on_screen(st.get(), true).id, "oled");
        assert_eq!(on_screen(st.get(), false).id, st.get().light_theme);
        st.set(|s| s.theme_follows_system = false);
        assert_eq!(on_screen(st.get(), false).id, "oled");
        assert_eq!(all(st.get()).len(), 5);
    }

    #[test]
    fn a_shared_theme_reads_back_and_bad_text_says_what_is_wrong() {
        let mut st = store();
        let id = duplicate(&mut st, "graphite");
        set_color(&mut st, &id, "window", Some(0x123456ff));
        set_color(&mut st, &id, "glass", Some(0x2828287f));
        let text = to_json(&st.get().custom_themes[0]);
        assert!(text.contains("\"ssheesh\": \"theme\"") || text.contains("\"ssheesh\":\"theme\""), "{text}");
        let (name, base, colors) = from_json(&text).unwrap();
        assert_eq!((name.as_str(), base.as_str()), ("My Graphite", "graphite"));
        // Only what differs from the base comes back.
        assert_eq!(colors.keys().collect::<Vec<_>>(), ["window", "glass"]);
        let added = import(&mut st, &text).unwrap();
        assert_eq!(st.get().theme, added);
        assert_eq!(st.get().custom_themes.last().unwrap().name, "My Graphite 2");
        for (bad, says) in [
            ("nope", "That isn't a theme. Copy all of it"),
            ("[1]", "It should start with {"),
            (r##"{"ssheesh":"snippet"}"##, "something else from SSHeesh"),
            (r##"{"colors":{}}"##, "None of the theme's colors"),
            (r##"{"base":"sepia","colors":{"fg":"#fff"}}"##, "\"base\" should be one of ssheesh, midnight"),
            (r##"{"colors":{"fg":"red"}}"##, "\"fg\" (Text) should be a color like #1f2937, not \"red\"."),
            (r##"{"colors":{"fg":"#11223344"}}"##, "\"fg\" (Text) can't be see-through."),
        ] {
            let why = from_json(bad).unwrap_err();
            assert!(why.contains(says), "{bad}: {why}");
        }
        // No base said: by its window, light or dark.
        assert_eq!(from_json(r##"{"colors":{"window":"#fafafa"}}"##).unwrap().1, "daylight");
    }

    #[test]
    fn hard_to_read_text_is_pointed_out() {
        assert!(weak_contrast(&built_in("midnight").unwrap()).is_empty());
        assert!(weak_contrast(&built_in("ssheesh").unwrap()).iter().all(|w| w.on == 2), "only on the picked segment's grey");
        let mut grey = built_in("graphite").unwrap();
        grey[4] = 0x333333ff;
        let weak = weak_contrast(&grey);
        assert!(weak.iter().any(|w| w.text == 4 && w.on == 0 && w.ratio < 4.5));
        assert_eq!(background_name(weak[0].on), "the window");
    }

    #[test]
    fn colours_go_round_hsv_and_hex() {
        for c in [0x4eba65ff, 0x000000ff, 0xffffffff, 0x7c83ff80, 0xd77757ff] {
            let back = from_hsv(to_hsv(c), alpha_of(c));
            assert_eq!(back, c, "{c:08x}");
        }
        assert_eq!(to_hex(0x4eba65ff), "#4eba65");
        assert_eq!(to_hex(0x4eba6580), "#4eba6580");
        assert!((contrast(0x000000ff, 0xffffffff) - 21.0).abs() < 1e-9);
        assert_eq!(over(0xffffff80, 0x000000ff), 0x808080ff);
    }

    #[test]
    fn the_default_accent_is_the_designs_blue() {
        let mut s = Settings::defaults(Platform::Linux);
        assert_eq!(accent(&s), None);
        assert_eq!(accent_swatch(DEFAULT_ACCENT), DESIGN_BLUE);
        s.accent = "#7c83ff".into();
        assert_eq!(accent(&s), Some(0x7c83ffff));
        assert!(accent_is_preset("#7C83FF") && !accent_is_preset("#123456"));
        s.accent = "not a colour".into();
        assert_eq!(accent(&s), None);
    }
}
