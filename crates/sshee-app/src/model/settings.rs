//! Preferences: `sshee-settings`, version 4 (`src/lib/settings.ts` in 0.10).
//!
//! 0.10 saves the whole store, so every field below is in it, in this order, plus whatever an
//! older version left that 0.10 no longer reads (0.5's `placesWidth`, for example), kept in
//! `extra`.

use super::json::{string_enum, Json, JsonMap};
use super::{Platform, Store};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The theme SSHeesh comes with ("SSHeesh Default").
pub const DEFAULT_THEME: &str = "ssheesh";
/// The first accent, the >_< logo's green.
pub const DEFAULT_ACCENT: &str = "#4eba65";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// A built-in theme (`ssheesh`, `midnight`, `graphite`, `oled`, `daylight`) or a
    /// `custom-…` id from `custom_themes`.
    pub theme: String,
    /// Switch between `light_theme` and `dark_theme` with the system.
    pub theme_follows_system: bool,
    pub light_theme: String,
    pub dark_theme: String,
    /// Themes made from a built-in one in Settings → Appearance.
    pub custom_themes: Vec<CustomTheme>,
    /// Shortcuts whose keys were changed, by shortcut id; `None` takes the keys away.
    pub shortcuts: IndexMap<String, Option<Keys>>,
    /// Interface size in percent.
    pub ui_zoom: f64,
    /// How much to fade panes not in use, 0 to 50 percent.
    pub dim_inactive: f64,
    pub animations: bool,
    pub logo: Logo,
    /// What hides until the pointer reaches the window's edge.
    pub compact_mode: CompactMode,
    /// What the compact shortcut turns back on (never `Off`).
    pub compact_last: CompactMode,
    /// The first-launch hello has been seen or skipped.
    pub onboarded: bool,
    /// Which hello was seen: a new one shows once to everybody.
    pub onboarding_version: u32,
    /// First-use hints already shown, by id.
    pub hints_seen: Vec<String>,
    /// A Production or Staging colored line along the window's top edge.
    pub env_stripe: bool,
    pub tabs_placement: TabsPlacement,
    /// The version whose "What's new" was seen.
    pub last_seen_version: String,
    pub accent: String,
    /// A terminal color scheme id; `own` uses `terminal_colors`.
    pub terminal_scheme: String,
    pub terminal_colors: Option<TerminalColors>,
    pub terminal_font: String,
    pub terminal_font_size: f64,
    pub terminal_line_height: f64,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    /// Draw terminals with the GPU. The default depends on the system: see [`Settings::defaults`].
    pub gpu_rendering: bool,
    pub copy_on_select: bool,
    /// Count how much typing could appear before the server answers (`sshee-typing-counts`).
    pub count_typing: bool,
    pub terminal_padding: f64,
    pub reconnect_mode: ReconnectMode,
    /// Minutes a terminal can sit unused before it lets go of its connection; 0 never.
    pub idle_sleep: f64,
    pub sidebar_side: Side,
    pub sidebar_visible: bool,
    pub sidebar_width: f64,
    pub sidebar_mode: SidebarMode,
    pub icon_pack: IconPack,
    pub rail_status: RailStatus,
    /// Machines whose container list is open in the sidebar, by target key.
    pub expanded: Vec<String>,
    pub view_mode: ViewMode,
    /// Places taken out of the file manager's bookmarks ("home", "/var/log", "recent").
    pub hidden_places: Vec<String>,
    pub preview_width: f64,
    pub pane_gap: f64,
    pub pane_radius: f64,
    pub show_vitals: bool,
    pub update_mode: UpdateMode,
    /// Hide server names, addresses and users everywhere.
    pub streamer_mode: bool,
    /// Tokens, keys and passwords in terminal output show as dots until clicked.
    pub hide_secrets: bool,
    /// Files lose their metadata before they're uploaded.
    pub clean_uploads: bool,
    pub highlight_errors: bool,
    pub highlight_warnings: bool,
    pub highlight_addresses: bool,
    pub highlight_rules: Vec<HighlightRule>,
    /// A system notification when a terminal needs you.
    pub notify_attention: bool,
    /// Through Tor, ssh keeps hiding the rhythm of typing.
    pub tor_hide_typing: bool,
    pub welcome_mode: WelcomeMode,
    pub welcome_parts: Vec<WelcomePart>,
    pub welcome_note: String,
    /// Servers that have had SSHeesh's welcome once.
    pub welcomed_hosts: Vec<String>,
    /// Windows: the system's own title bar.
    pub native_title_bar: bool,
    /// Fields this version doesn't know, as saved.
    #[serde(flatten)]
    pub extra: JsonMap,
}

impl Settings {
    /// 0.10.14's `DEFAULT_SETTINGS` on `platform`.
    pub fn defaults(platform: Platform) -> Self {
        Settings {
            theme: DEFAULT_THEME.into(),
            theme_follows_system: false,
            light_theme: "daylight".into(),
            dark_theme: DEFAULT_THEME.into(),
            custom_themes: vec![],
            shortcuts: IndexMap::new(),
            ui_zoom: 100.0,
            dim_inactive: 0.0,
            animations: true,
            logo: Logo::Winking,
            compact_mode: CompactMode::Off,
            compact_last: CompactMode::Both,
            onboarded: false,
            onboarding_version: 0,
            hints_seen: vec![],
            env_stripe: false,
            tabs_placement: TabsPlacement::Top,
            last_seen_version: String::new(),
            accent: DEFAULT_ACCENT.into(),
            terminal_scheme: "auto".into(),
            terminal_colors: None,
            terminal_font: "system".into(),
            terminal_font_size: 13.0,
            terminal_line_height: 1.22,
            cursor_style: CursorStyle::Bar,
            cursor_blink: true,
            gpu_rendering: platform.gpu_by_default(),
            copy_on_select: true,
            count_typing: false,
            terminal_padding: 12.0,
            reconnect_mode: ReconnectMode::Ask,
            idle_sleep: 20.0,
            sidebar_side: Side::Left,
            sidebar_visible: true,
            sidebar_width: 232.0,
            sidebar_mode: SidebarMode::Narrow,
            icon_pack: IconPack::Line,
            rail_status: RailStatus::Off,
            expanded: vec![],
            view_mode: ViewMode::Tabs,
            hidden_places: vec![],
            preview_width: 420.0,
            pane_gap: 6.0,
            pane_radius: 11.0,
            show_vitals: true,
            update_mode: UpdateMode::Auto,
            streamer_mode: false,
            hide_secrets: true,
            clean_uploads: true,
            highlight_errors: true,
            highlight_warnings: true,
            highlight_addresses: false,
            highlight_rules: vec![],
            notify_attention: true,
            tor_hide_typing: false,
            welcome_mode: WelcomeMode::Auto,
            welcome_parts: vec![WelcomePart::Logo, WelcomePart::System, WelcomePart::Usage, WelcomePart::Status],
            welcome_note: String::new(),
            welcomed_hosts: vec![],
            native_title_bar: false,
            extra: JsonMap::new(),
        }
    }
}

impl Default for Settings {
    /// The defaults on the system this runs on.
    fn default() -> Self {
        Settings::defaults(Platform::current())
    }
}

impl Store for Settings {
    const KEY: &'static str = "sshee-settings";
    const VERSION: u64 = 4;
    const SHIPPED: &'static [u64] = &[1, 2, 3, 4];

    fn defaults(platform: Platform) -> Self {
        Settings::defaults(platform)
    }

    /// `migrateSettings` in 0.10.14's settings.ts, step by step.
    fn migrate(mut s: JsonMap, from: u64, platform: Platform) -> JsonMap {
        // 2: Macs draw with the GPU, also where it was off by default before.
        if from < 2 && platform == Platform::Mac {
            s.insert("gpuRendering".into(), Json::Bool(true));
        }
        // 3: compact mode picks what hides; load beside server icons is off for everybody.
        if from < 3 {
            let both = s.get("compactMode") == Some(&Json::Bool(true));
            s.insert("compactMode".into(), Json::from(if both { "both" } else { "off" }));
            s.insert("railStatus".into(), Json::from("off"));
        }
        // 4: SSHeesh Default is the new look, for whoever kept the old one as it came
        // (Midnight in blue). A theme or accent picked by hand stays.
        if from < 4 {
            let accent = match s.get("accent") {
                None | Some(Json::Null) => Some("#5b9dff"),
                Some(other) => other.as_str(),
            };
            if s.get("theme").and_then(Json::as_str) == Some("midnight") && accent == Some("#5b9dff") {
                s.insert("theme".into(), Json::from(DEFAULT_THEME));
                s.insert("accent".into(), Json::from(DEFAULT_ACCENT));
                if s.get("darkTheme").and_then(Json::as_str) == Some("midnight") {
                    s.insert("darkTheme".into(), Json::from(DEFAULT_THEME));
                }
            }
        }
        s
    }

    fn complete(&mut self, saved: &JsonMap, platform: Platform) {
        // The one default that depends on the system: `Default` used this machine's.
        if !saved.contains_key("gpuRendering") {
            self.gpu_rendering = platform.gpu_by_default();
        }
    }
}

/// A theme made in Settings → Appearance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTheme {
    /// `custom-` and a random id.
    pub id: String,
    pub name: String,
    /// The built-in theme it started from.
    pub base: String,
    /// Only the colors that were changed, by color key (`window`, `canvas`, …), as `#rrggbb[aa]`.
    pub colors: IndexMap<String, String>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

/// A key and the modifiers held with it, for a changed shortcut.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keys {
    /// As the web view named it: "k", "5", ",", "enter", "f5", "up"…
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctrl: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shift: Option<bool>,
    #[serde(flatten)]
    pub extra: JsonMap,
}

/// Background and text color for the "Your own" terminal scheme.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalColors {
    pub background: String,
    pub foreground: String,
    #[serde(flatten)]
    pub extra: JsonMap,
}

/// A highlight rule of your own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightRule {
    pub id: String,
    /// Plain text (any case), or a JavaScript regular expression written as /like this/.
    pub pattern: String,
    pub color: HighlightColor,
    #[serde(flatten)]
    pub extra: JsonMap,
}

string_enum! {
    /// The >_< mark in the sidebar and empty panes.
    pub enum Logo { Winking = "winking", Still = "still", Hidden = "hidden", }
    default Winking
}

string_enum! {
    /// What compact mode hides.
    pub enum CompactMode { Off = "off", Sidebar = "sidebar", Tabbar = "tabbar", Both = "both", }
    default Off
}

string_enum! {
    /// Tabs under their server in the sidebar, or in a strip at the top.
    pub enum TabsPlacement { Sidebar = "sidebar", Top = "top", }
    default Top
}

string_enum! {
    pub enum CursorStyle { Bar = "bar", Block = "block", Underline = "underline", }
    default Bar
}

string_enum! {
    /// After a dropped connection: reconnect alone, offer a button, or stay quiet.
    pub enum ReconnectMode { Auto = "auto", Ask = "ask", Off = "off", }
    default Ask
}

string_enum! {
    pub enum Side { Left = "left", Right = "right", }
    default Left
}

string_enum! {
    /// Full list, or a rail of server icons.
    pub enum SidebarMode { Wide = "wide", Narrow = "narrow", }
    default Narrow
}

string_enum! {
    /// Server icons in clean lines or in pixels.
    pub enum IconPack { Line = "line", Pixel = "pixel", }
    default Line
}

string_enum! {
    /// How the rail shows a connected server's load.
    pub enum RailStatus { Off = "off", Frame = "frame", Bars = "bars", Pulse = "pulse", Count = "count", }
    default Off
}

string_enum! {
    /// Side by side ("split") or one tab filling the window ("tabs").
    pub enum ViewMode { Split = "split", Tabs = "tabs", }
    default Tabs
}

string_enum! {
    pub enum UpdateMode { Auto = "auto", Notify = "notify", Manual = "manual", }
    default Auto
}

string_enum! {
    pub enum HighlightColor { Red = "red", Yellow = "yellow", Green = "green", Blue = "blue", Magenta = "magenta", Cyan = "cyan", }
    default Red
}

string_enum! {
    /// What a new terminal on a server shows first.
    pub enum WelcomeMode { Auto = "auto", Ssheesh = "ssheesh", First = "first", Server = "server", Off = "off", }
    default Auto
}

string_enum! {
    /// What SSHeesh's welcome shows.
    pub enum WelcomePart { Logo = "logo", System = "system", Usage = "usage", Status = "status", }
    default Logo
}
