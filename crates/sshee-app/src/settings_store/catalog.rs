//! Everything the Settings page holds, as data: its sections, each row's name, its one line
//! of help, the control it has and the settings it owns, in 0.10's words
//! (`src/components/SettingsDialog.tsx`). The screen draws this list; nothing about a setting
//! is written twice.
//!
//! Two rules it keeps, each with a test:
//!
//! - **Nothing is forgotten.** Every one of 0.10's sixty settings is owned by a row here or
//!   named in [`KEPT_UNSEEN`] (what 0.10 itself saved without showing).
//! - **Nothing pretends.** A row whose feature 1.0 doesn't have yet says so in one line
//!   ([`Status::Later`]) and is drawn disabled, rather than left out or left clickable.

use std::sync::OnceLock;

use super::keymap::{self, Keyboard};
use super::schemes::{Appearance, TERMINAL_FONTS};
use super::SettingsStore;
use crate::model::{
    CompactMode, CursorStyle, IconPack, Logo, Platform, RailStatus, ReconnectMode, Side, SidebarMode, TabsPlacement, UpdateMode, ViewMode, WelcomeMode,
    WelcomePart,
};

/// A page of Settings, in the order the list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Layout,
    Appearance,
    Terminal,
    Keyboard,
    Privacy,
    Files,
    Agents,
    Tor,
    Updates,
    About,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::Layout,
        Section::Appearance,
        Section::Terminal,
        Section::Keyboard,
        Section::Privacy,
        Section::Files,
        Section::Agents,
        Section::Tor,
        Section::Updates,
        Section::About,
    ];

    /// A fixed word for element keys and for opening Settings at a section.
    pub fn id(self) -> &'static str {
        match self {
            Section::Layout => "layout",
            Section::Appearance => "appearance",
            Section::Terminal => "terminal",
            Section::Keyboard => "keyboard",
            Section::Privacy => "privacy",
            Section::Files => "files",
            Section::Agents => "agents",
            Section::Tor => "tor",
            Section::Updates => "updates",
            Section::About => "about",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Layout => "Layout",
            Section::Appearance => "Appearance",
            Section::Terminal => "Terminal",
            Section::Keyboard => "Keyboard",
            Section::Privacy => "Privacy",
            Section::Files => "Files",
            Section::Agents => "Agents and notifications",
            Section::Tor => "Tor",
            Section::Updates => "Updates",
            Section::About => "About",
        }
    }

    pub fn from_id(id: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The section after this one (`step` 1) or before it (-1), stopping at the ends.
    pub fn step(self, step: i32) -> Section {
        let at = Section::ALL.iter().position(|s| *s == self).unwrap_or(0) as i32;
        Section::ALL[(at + step).clamp(0, Section::ALL.len() as i32 - 1) as usize]
    }

    /// Whether the section has a Reset, as in 0.10: where settings are a matter of taste.
    /// Privacy, Tor, Updates and About have none: a privacy switch shouldn't flip in a sweep.
    pub fn has_reset(self) -> bool {
        matches!(self, Section::Layout | Section::Appearance | Section::Terminal | Section::Keyboard | Section::Agents)
    }
}

/// Whether 1.0 acts on a row yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// It takes effect at once.
    Works,
    /// Kept as 0.10 left it and shown, but 1.0 doesn't act on it yet: drawn disabled, with
    /// this one line under the row.
    Later(&'static str),
}

/// What a row can read and change. Setters go through the store, so listeners hear and the
/// file is written.
pub type Get<T> = fn(&SettingsStore) -> T;
pub type Set<T> = fn(&mut SettingsStore, T);

/// How a choice among a few is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceLook {
    /// Side by side, all visible: two to five short words.
    Segmented,
    /// A pop-up button: longer names, or more of them.
    Popup,
}

/// Something only the window can do or show: the screen draws these by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    /// The sidebar is shown (the window keeps this with its tabs).
    SidebarShown,
    /// 0.10's theme, by name, and that themes of your own are kept.
    Theme,
    /// 0.10's accent colours, and one of your own.
    Accent,
    /// While following the system: the theme for each side.
    LightDark,
    /// The terminal colour schemes, as small pictures.
    Schemes,
    /// A terminal in the colours, size and cursor chosen.
    TerminalPreview,
    /// Words of your own to highlight.
    HighlightRules,
    /// Every shortcut and its keys.
    Shortcuts,
    /// Tor's switch, with its state under it.
    TorSwitch,
    /// What SSHeesh connects to.
    Network,
    /// The version, and checking for a new one.
    UpdateCard,
    /// The editor Files' "Open in <editor>" starts: the ones found, and "Choose an app…".
    ExternalEditor,
    /// The logo, the version and the links.
    AboutCard,
    /// Tips: on, off, shown again.
    Tips,
}

/// A row's control.
#[derive(Debug, Clone, Copy)]
pub enum Control {
    Switch { get: Get<bool>, set: Set<bool> },
    /// One of `options`, each `(value, label)`.
    Choice { look: ChoiceLook, options: &'static [(&'static str, &'static str)], get: Get<String>, set: fn(&mut SettingsStore, &str) },
    /// A number between `min` and `max` in steps of `step`, shown through `show`.
    Slider { min: f64, max: f64, step: f64, get: Get<f64>, set: Set<f64>, show: fn(f64) -> String },
    /// A line of text, at most `max` characters.
    Text { placeholder: &'static str, max: usize, get: Get<String>, set: fn(&mut SettingsStore, &str) },
    /// Several of `options` at once, each `(value, label)`; `toggle` flips one.
    Toggles { options: &'static [(&'static str, &'static str)], get: Get<Vec<String>>, toggle: fn(&mut SettingsStore, &str) },
    /// Buttons that do something once.
    Buttons(&'static [(&'static str, Action)]),
    Special(Special),
}

/// What a button does. The window decides how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing yet: its row says what comes later.
    Later,
    /// Opens a page in the browser.
    Open(&'static str),
    /// Opens "Add a server", where the ssh config's hosts and 0.10's servers are offered.
    AddServer,
    /// "What's new", the tour, the welcome again.
    WhatsNew,
    Tour,
    Welcome,
}

/// A row's one line of help.
#[derive(Debug, Clone, Copy)]
pub enum Hint {
    None,
    Fixed(&'static str),
    /// It depends on what is chosen, or on the system's keys.
    By(fn(&SettingsStore, Keyboard) -> String),
}

/// What a row's "?" explains: what the thing is, why you'd want it, what happens when it's
/// on, in a few plain sentences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct About {
    /// What it is about, for its name: "About Tor".
    pub name: &'static str,
    pub text: &'static str,
}

/// One row of a section.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// A fixed word: the row's element key, and where search scrolls to.
    pub key: &'static str,
    pub section: Section,
    /// The small heading it sits under, if any.
    pub group: Option<&'static str>,
    pub label: &'static str,
    pub hint: Hint,
    /// What a newcomer may not know, behind a quiet "?" after the name.
    pub about: Option<About>,
    pub control: Control,
    pub status: Status,
    /// The 0.10 settings it owns, by their names in the store.
    pub saves: &'static [&'static str],
    /// The settings 1.0 added that it owns, by their names in the file's `more`.
    pub adds: &'static [&'static str],
    /// A choice's labels on a Mac, where they differ (a key's name), in the options' order.
    pub mac_labels: Option<&'static [&'static str]>,
    /// Other ways people might say it, for search.
    pub words: &'static str,
    /// Only on this system.
    pub only: Option<Platform>,
    /// Shown only while this holds (a row that depends on another's choice).
    pub when: Option<Get<bool>>,
}

impl Row {
    const fn new(key: &'static str, section: Section, label: &'static str, control: Control) -> Row {
        Row {
            key,
            section,
            group: None,
            label,
            hint: Hint::None,
            about: None,
            control,
            status: Status::Works,
            saves: &[],
            adds: &[],
            mac_labels: None,
            words: "",
            only: None,
            when: None,
        }
    }

    const fn hint(mut self, hint: &'static str) -> Row {
        self.hint = Hint::Fixed(hint);
        self
    }

    const fn hint_by(mut self, hint: fn(&SettingsStore, Keyboard) -> String) -> Row {
        self.hint = Hint::By(hint);
        self
    }

    const fn about(mut self, name: &'static str, text: &'static str) -> Row {
        self.about = Some(About { name, text });
        self
    }

    const fn later(mut self, why: &'static str) -> Row {
        self.status = Status::Later(why);
        self
    }

    const fn saves(mut self, keys: &'static [&'static str]) -> Row {
        self.saves = keys;
        self
    }

    const fn adds(mut self, keys: &'static [&'static str]) -> Row {
        self.adds = keys;
        self
    }

    const fn on_mac(mut self, labels: &'static [&'static str]) -> Row {
        self.mac_labels = Some(labels);
        self
    }

    const fn words(mut self, words: &'static str) -> Row {
        self.words = words;
        self
    }

    const fn group(mut self, group: &'static str) -> Row {
        self.group = Some(group);
        self
    }

    const fn only(mut self, platform: Platform) -> Row {
        self.only = Some(platform);
        self
    }

    const fn when(mut self, when: Get<bool>) -> Row {
        self.when = Some(when);
        self
    }

    /// Whether 1.0 acts on it.
    pub fn works(&self) -> bool {
        self.status == Status::Works
    }

    /// The line under the row: why it is disabled, else its help.
    pub fn help(&self, store: &SettingsStore, keyboard: Keyboard) -> Option<String> {
        match self.hint {
            Hint::None => None,
            Hint::Fixed(text) => Some(text.to_owned()),
            Hint::By(f) => Some(f(store, keyboard)),
        }
    }

    /// A choice's labels as `keyboard`'s system names them, in the options' order. Empty for
    /// a row that isn't a choice.
    pub fn choice_labels(&self, keyboard: Keyboard) -> Vec<&'static str> {
        let Control::Choice { options, .. } = self.control else { return vec![] };
        match self.mac_labels.filter(|labels| keyboard.mac() && labels.len() == options.len()) {
            Some(labels) => labels.to_vec(),
            None => options.iter().map(|(_, label)| *label).collect(),
        }
    }

    /// Whether the row shows on `platform` with the settings as they are.
    pub fn shows(&self, store: &SettingsStore, platform: Platform) -> bool {
        self.only.is_none_or(|only| only == platform) && self.when.is_none_or(|when| when(store))
    }
}

fn px(v: f64) -> String {
    format!("{v} px")
}

fn percent(v: f64) -> String {
    format!("{v}%")
}

/// SSHeesh's welcome is shown at all.
fn welcome_is_ours(store: &SettingsStore) -> bool {
    !matches!(store.get().welcome_mode, WelcomeMode::Off | WelcomeMode::Server)
}

// What the "?" after a row's name says (`Row::about`).

const ABOUT_RECONNECT: &str = "Your terminal is a program running on the server. If the server has tmux, a program that keeps terminals running while nobody is connected, SSHeesh takes you back into the same one and nothing is lost. Without tmux, a dropped connection ends what was running, and you start a fresh terminal.";
const ABOUT_IDLE: &str = "An idle terminal still holds a login on the server. With tmux on the server, a program that keeps terminals running while nobody is connected, SSHeesh can let that login go: your programs carry on there, and any key takes you back to them. Terminals on servers without tmux stay connected.";
const ABOUT_COUNT_TYPING: &str = "On a slow connection, each letter you type travels to the server and back before it shows. This counts how often SSHeesh could have shown it at once instead, to learn whether that's worth building. It counts keys, never which ones.";
const ABOUT_WELCOME: &str = "Many servers greet you with a message when you log in (often called the MOTD): notices from whoever runs it, updates waiting. SSHeesh's own welcome is a short look at the server instead: its system, memory and disk.";
const ABOUT_PROGRAM_CLIPBOARD: &str = "Programs on the server can ask to put text on this computer's clipboard, as tmux and coding agents do when you copy inside them. Off, only what you copy yourself gets there.";
const ABOUT_SSH_CONFIG: &str = "~/.ssh/config is a file on this computer where ssh keeps shortcuts to servers: a short name, the address, the user and the key to sign in with. SSHeesh reads it to list those servers, and never writes to it.";
const ABOUT_TOR: &str = "Tor is a free network run by volunteers: each connection hops through three of its computers, so none of them sees both you and the server, and it's slower than going direct. Servers on your own network still connect directly. One reached through another server (a jump host) doesn't connect, and an ssh you type in This computer's terminal goes as typed.";
const ABOUT_TOR_TYPING: &str = "Even encrypted, the gaps between your keystrokes can hint at what you type, a password included. Sending keys on an even beat leaves the network nothing to time.";

const REPO: &str = "https://github.com/footprintsntrails/ssheesh";

/// 0.10's settings that no row shows: what it saved for itself (how far the hello got, which
/// tips were seen, which places were taken out, sizes dragged by hand). They are kept and
/// brought over all the same.
/// `gpuRendering` too: 1.0 draws the whole window with the GPU, so its switch had nothing to do.
pub const KEPT_UNSEEN: [&str; 10] =
    ["onboarded", "onboardingVersion", "lastSeenVersion", "expanded", "hiddenPlaces", "previewWidth", "compactLast", "sidebarWidth", "welcomedHosts", "gpuRendering"];

/// Every row, section by section in the list's order.
pub fn rows() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(build)
}

/// The rows of one section that show on `platform` now.
pub fn section_rows(section: Section, store: &SettingsStore, platform: Platform) -> Vec<&'static Row> {
    rows().iter().filter(|r| r.section == section && r.shows(store, platform)).collect()
}

/// The row with this key.
pub fn row(key: &str) -> Option<&'static Row> {
    rows().iter().find(|r| r.key == key)
}

/// What a Reset never takes: what you made yourself (themes of your own, your own terminal
/// colours), as 0.10's Reset kept them.
pub const KEPT_ON_RESET: [&str; 2] = ["customThemes", "terminalColors"];

/// The settings a section's Reset puts back: every row's, as 0.10's Reset did, but for what
/// you made yourself ([`KEPT_ON_RESET`]).
pub fn reset_keys(section: Section) -> Vec<&'static str> {
    rows()
        .iter()
        .filter(|r| r.section == section)
        .flat_map(|r| r.saves.iter().chain(r.adds).copied())
        .filter(|k| !KEPT_ON_RESET.contains(k))
        .collect()
}

/// Something search found: a row, or one shortcut in the Keyboard section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub section: Section,
    pub label: &'static str,
    /// The element to scroll to.
    pub key: String,
}

/// The rows and shortcuts matching `query`: every word of it must be in the row's name or its
/// other words, in any case (0.10's rule). An empty query finds nothing.
pub fn search(query: &str, platform: Platform) -> Vec<Found> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return vec![];
    }
    let hit = |label: &str, more: &str| {
        let hay = format!("{label} {more}").to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    };
    let mut found: Vec<Found> = rows()
        .iter()
        .filter(|r| r.only.is_none_or(|only| only == platform) && !r.label.is_empty() && hit(r.label, r.words))
        .map(|r| Found { section: r.section, label: r.label, key: r.key.to_owned() })
        .collect();
    let mut named = vec![];
    for (_, ids) in keymap::GROUPS {
        for id in ids {
            let Some(entry) = keymap::entry(id) else { continue };
            if hit(entry.name, "shortcut keys hotkey keybinding") && !named.contains(&entry.name) {
                named.push(entry.name);
                found.push(Found { section: Section::Keyboard, label: entry.name, key: format!("shortcut-{id}") });
            }
        }
    }
    found
}

fn build() -> Vec<Row> {
    use Control::{Buttons, Choice, Slider, Switch, Text, Toggles};
    use Section::{About, Agents, Appearance as Look, Files, Keyboard as Keys, Layout, Privacy, Terminal, Tor, Updates};
    const SEG: ChoiceLook = ChoiceLook::Segmented;
    const POP: ChoiceLook = ChoiceLook::Popup;
    vec![
        // ------------------------------------------------------------------ Layout
        Row::new("layout-sidebar-shown", Layout, "Show sidebar", Control::Special(Special::SidebarShown))
            .group("Sidebar")
            .hint_by(|st, on| format!("Show or hide with {}.", keymap::shortcut_label("sidebar", on, &st.get().shortcuts)))
            .saves(&["sidebarVisible"])
            .words("hide show server list"),
        Row::new(
            "layout-sidebar-side",
            Layout,
            "Side",
            Choice {
                look: SEG,
                options: &[("left", "Left"), ("right", "Right")],
                get: |st| st.get().sidebar_side.as_str().to_owned(),
                set: |st, v| st.set(|s| s.sidebar_side = Side::from(v)),
            },
        )
        .group("Sidebar")
        .saves(&["sidebarSide"])
        .words("left right server list"),
        Row::new(
            "layout-server-list",
            Layout,
            "Server list",
            Choice {
                look: SEG,
                options: &[("wide", "Names"), ("narrow", "Icons")],
                get: |st| st.get().sidebar_mode.as_str().to_owned(),
                set: |st, v| st.set(|s| s.sidebar_mode = SidebarMode::from(v)),
            },
        )
        .group("Sidebar")
        .hint("Names, or just icons to save room.")
        .saves(&["sidebarMode"])
        .words("sidebar narrow icons names rail"),
        Row::new(
            "layout-server-icons",
            Layout,
            "Server icons",
            Choice {
                look: SEG,
                options: &[("line", "Lines"), ("pixel", "Pixels")],
                get: |st| st.get().icon_pack.as_str().to_owned(),
                set: |st, v| st.set(|s| s.icon_pack = IconPack::from(v)),
            },
        )
        .group("Sidebar")
        .hint("Clean lines, or pixels like the >_< logo.")
        .saves(&["iconPack"])
        .words("icon pack pixel line style look"),
        Row::new(
            "layout-view",
            Layout,
            "Tabs on screen",
            Choice {
                look: SEG,
                options: &[("tabs", "One at a time"), ("split", "Side by side")],
                get: |st| st.get().view_mode.as_str().to_owned(),
                set: |st, v| st.set(|s| s.view_mode = ViewMode::from(v)),
            },
        )
        .group("Tabs")
        .hint("Side by side, a server's tabs share its page. Drag a tab onto the page to split it.")
        .saves(&["viewMode"])
        .words("split panes side by side"),
        Row::new(
            "layout-tab-server",
            Layout,
            "Name the server on every tab",
            Switch { get: |st| st.more().tabs_name_server, set: |st, v| st.set_more(|m| m.tabs_name_server = v) },
        )
        .group("Tabs")
        .hint("Off, a server is named once, before its group of tabs.")
        .adds(&["tabsNameServer"])
        .words("tab title host name group"),
        Row::new(
            "layout-tabs",
            Layout,
            "Where tabs go",
            Choice {
                look: SEG,
                options: &[("sidebar", "Under each server"), ("top", "Along the top")],
                get: |st| st.get().tabs_placement.as_str().to_owned(),
                set: |st, v| st.set(|s| s.tabs_placement = TabsPlacement::from(v)),
            },
        )
        .group("Tabs")
        .hint("Under each server in the sidebar, or in a strip along the top.")
        .later("Tabs run along the top. Tabs under each server aren't built yet.")
        .saves(&["tabsPlacement"])
        .words("tab strip bar sidebar top"),
        Row::new(
            "layout-compact",
            Layout,
            "Compact mode",
            Choice {
                look: POP,
                options: &[("off", "Off"), ("sidebar", "Sidebar"), ("tabbar", "Tab bar"), ("both", "Both")],
                get: |st| st.get().compact_mode.as_str().to_owned(),
                set: |st, v| st.set(|s| s.compact_mode = CompactMode::from(v)),
            },
        )
        .group("Window")
        .hint("What hides until you point at the window's edge. Both hides the vitals too.")
        .saves(&["compactMode"])
        .words("zen focus hide toolbar tab bar sidebar minimal"),
        Row::new(
            "layout-vitals",
            Layout,
            "Server vitals",
            Switch { get: |st| st.get().show_vitals, set: |st, v| st.set(|s| s.show_vitals = v) },
        )
        .group("Window")
        .hint_by(|st, on| {
            format!(
                "CPU, memory, network and disk under your tabs. {} shows and hides it.",
                keymap::shortcut_label("vitals", on, &st.get().shortcuts)
            )
        })
        .saves(&["showVitals"])
        .words("stats cpu memory bar metrics status"),
        Row::new(
            "layout-env-line",
            Layout,
            "A line for Production and Staging",
            Switch { get: |st| st.get().env_stripe, set: |st, v| st.set(|s| s.env_stripe = v) },
        )
        .group("Window")
        .hint("A thin red or orange line along the top while a Production or Staging server is on screen.")
        .saves(&["envStripe"])
        .words("prod staging stripe label red"),
        Row::new(
            "layout-title-bar",
            Layout,
            "Use the Windows title bar",
            Switch { get: |st| st.get().native_title_bar, set: |st, v| st.set(|s| s.native_title_bar = v) },
        )
        .group("Window")
        .hint("For when dragging, snapping or the window buttons misbehave. Takes effect after a restart.")
        .saves(&["nativeTitleBar"])
        .words("title bar window frame decorations native snap drag minimize maximize close buttons")
        .only(Platform::Windows),
        Row::new(
            "layout-gap",
            Layout,
            "Space between panes",
            Slider { min: 0.0, max: 16.0, step: 1.0, get: |st| st.get().pane_gap, set: |st, v| st.set(|s| s.pane_gap = v), show: px },
        )
        .group("Window")
        .saves(&["paneGap"])
        .words("gap spacing"),
        Row::new(
            "layout-radius",
            Layout,
            "Corner roundness",
            Slider { min: 0.0, max: 18.0, step: 1.0, get: |st| st.get().pane_radius, set: |st, v| st.set(|s| s.pane_radius = v), show: px },
        )
        .group("Window")
        .hint("The page's corner where it meets the sidebar.")
        .saves(&["paneRadius"])
        .words("radius rounded corners"),
        Row::new("layout-ssh-config", Layout, "Servers from ~/.ssh/config", Buttons(&[("Choose…", Action::AddServer)]))
            .group("Servers")
            .hint("Pick which show up in the sidebar. Your config file isn't changed.")
            .about("~/.ssh/config", ABOUT_SSH_CONFIG)
            .words("hidden hosts import config bring over 0.10"),
        Row::new(
            "layout-server-load",
            Layout,
            "Server load",
            Choice {
                look: POP,
                options: &[("off", "Off"), ("frame", "Frame"), ("bars", "Bars"), ("pulse", "Pulse"), ("count", "Count")],
                get: |st| st.get().rail_status.as_str().to_owned(),
                set: |st, v| st.set(|s| s.rail_status = RailStatus::from(v)),
            },
        )
        .group("Servers")
        .hint("How busy each connected server is: on its icon, and as CPU and RAM under its name in the list.")
        .saves(&["railStatus"])
        .words("cpu memory busy meter stats"),
        // ------------------------------------------------------------------ Appearance
        Row::new(
            "look-appearance",
            Look,
            "Appearance",
            Choice {
                look: SEG,
                options: &[("system", "Match my system"), ("light", "Light"), ("dark", "Dark")],
                get: |st| Appearance::of(st.get()).as_str().to_owned(),
                set: |st, v| {
                    if let Some(appearance) = Appearance::parse(v) {
                        st.update(|s| appearance.write(s));
                    }
                },
            },
        )
        .hint("Match my system switches when your system does.")
        .saves(&["themeFollowsSystem", "theme", "lightTheme", "darkTheme"])
        .words("auto dark mode light mode night day schedule theme colors look"),
        Row::new("look-theme", Look, "Theme", Control::Special(Special::Theme))
            .hint("Arrow keys flip through them. Duplicate one to change its colors.")
            .saves(&["customThemes"])
            .words("midnight graphite oled daylight custom own edit editor hex palette transparency import export share"),
        Row::new("look-light-dark", Look, "Light and dark", Control::Special(Special::LightDark))
            .hint("The theme by day and the one by night.")
            .words("theme light dark day night system")
            .when(|st| st.get().theme_follows_system),
        Row::new("look-accent", Look, "Accent color", Control::Special(Special::Accent))
            .hint("What is picked, the button Return presses, and the focus ring.")
            .saves(&["accent"])
            .words("highlight color blue green"),
        Row::new(
            "look-size",
            Look,
            "Interface size",
            // A choice rather than 0.10's slider: the window zooms with each step, and a knob
            // being dragged would move under the pointer.
            Choice {
                look: POP,
                options: &[
                    ("85", "85%"),
                    ("90", "90%"),
                    ("95", "95%"),
                    ("100", "100%"),
                    ("105", "105%"),
                    ("110", "110%"),
                    ("115", "115%"),
                    ("120", "120%"),
                    ("125", "125%"),
                    ("130", "130%"),
                ],
                get: |st| format!("{}", (st.get().ui_zoom / 5.0).round() * 5.0),
                set: |st, v| {
                    if let Ok(percent) = v.parse::<f64>() {
                        st.update(|s| s.ui_zoom = percent);
                    }
                },
            },
        )
        .group("Interface")
        .hint("Everything but the terminal's text, which is sized under Terminal.")
        .saves(&["uiZoom"])
        .words("zoom scale bigger smaller ui text"),
        Row::new(
            "look-fade",
            Look,
            "Fade panes you're not using",
            Slider {
                min: 0.0,
                max: 50.0,
                step: 5.0,
                get: |st| st.get().dim_inactive,
                set: |st, v| st.set(|s| s.dim_inactive = v),
                show: |v| if v > 0.0 { percent(v) } else { "Off".into() },
            },
        )
        .group("Interface")
        .hint("With tabs side by side, the one you're not typing in fades.")
        .saves(&["dimInactive"])
        .words("dim inactive focus opacity"),
        Row::new("look-animations", Look, "Animations", Switch { get: |st| st.get().animations, set: |st, v| st.set(|s| s.animations = v) })
            .group("Interface")
            .saves(&["animations"])
            .words("motion reduce effects"),
        Row::new(
            "look-logo",
            Look,
            "Logo",
            Choice {
                look: SEG,
                options: &[("winking", "Winking"), ("still", "Still"), ("hidden", "Hidden")],
                get: |st| st.get().logo.as_str().to_owned(),
                set: |st, v| st.set(|s| s.logo = Logo::from(v)),
            },
        )
        .group("Interface")
        .hint("On the main menu's button. Hidden, the button shows a house.")
        .saves(&["logo"])
        .words("icon wink animated mascot face"),
        // ------------------------------------------------------------------ Terminal
        Row::new("term-preview", Terminal, "", Control::Special(Special::TerminalPreview)),
        Row::new(
            "term-font",
            Terminal,
            "Font",
            Choice { look: POP, options: &TERMINAL_FONTS, get: |st| st.get().terminal_font.clone(), set: |st, v| st.set(|s| s.terminal_font = v.to_owned()) },
        )
        .group("Text and cursor")
        .hint("A font this computer lacks shows as JetBrains Mono.")
        .on_mac(&["SF Mono", "JetBrains Mono", "Menlo", "Cascadia Mono", "Consolas"])
        .saves(&["terminalFont"])
        .words("typeface mono jetbrains menlo cascadia sf"),
        Row::new(
            "term-font-size",
            Terminal,
            "Font size",
            Slider { min: 10.0, max: 20.0, step: 1.0, get: |st| st.get().terminal_font_size, set: |st, v| st.set(|s| s.terminal_font_size = v), show: px },
        )
        .group("Text and cursor")
        .saves(&["terminalFontSize"])
        .words("text bigger smaller"),
        Row::new(
            "term-line-height",
            Terminal,
            "Line height",
            Slider {
                min: 1.0,
                max: 1.6,
                step: 0.02,
                get: |st| st.get().terminal_line_height,
                set: |st, v| st.set(|s| s.terminal_line_height = v),
                show: |v| format!("{v:.2}"),
            },
        )
        .group("Text and cursor")
        .saves(&["terminalLineHeight"])
        .words("spacing lines"),
        Row::new(
            "term-padding",
            Terminal,
            "Padding",
            Slider { min: 0.0, max: 28.0, step: 1.0, get: |st| st.get().terminal_padding, set: |st, v| st.set(|s| s.terminal_padding = v), show: px },
        )
        .group("Text and cursor")
        .hint("The space between the terminal's edge and its text.")
        .saves(&["terminalPadding"])
        .words("margin space terminal edge"),
        Row::new(
            "term-cursor",
            Terminal,
            "Cursor",
            Choice {
                look: SEG,
                options: &[("bar", "Bar"), ("block", "Block"), ("underline", "Line")],
                get: |st| st.get().cursor_style.as_str().to_owned(),
                set: |st, v| st.set(|s| s.cursor_style = CursorStyle::from(v)),
            },
        )
        .group("Text and cursor")
        .saves(&["cursorStyle"])
        .words("bar block underline caret"),
        Row::new("term-blink", Terminal, "Blinking cursor", Switch { get: |st| st.get().cursor_blink, set: |st, v| st.set(|s| s.cursor_blink = v) })
            .group("Text and cursor")
            .saves(&["cursorBlink"])
            .words("blink caret"),
        Row::new("term-scheme", Terminal, "Color scheme", Control::Special(Special::Schemes))
            .saves(&["terminalScheme", "terminalColors"])
            .words("terminal theme dracula nord tokyo catppuccin solarized gruvbox colors"),
        Row::new(
            "term-reconnect",
            Terminal,
            "When the connection drops",
            Choice {
                look: SEG,
                options: &[("auto", "Reconnect"), ("ask", "Ask me"), ("off", "Off")],
                get: |st| st.get().reconnect_mode.as_str().to_owned(),
                set: |st, v| st.set(|s| s.reconnect_mode = ReconnectMode::from(v)),
            },
        )
        .group("Connection")
        .hint_by(|st, _| {
            let text = match st.get().reconnect_mode {
                ReconnectMode::Auto => "Reconnects on its own and lands you back in the same session.",
                ReconnectMode::Ask => "The tab says Disconnected and waits for you: a key or a click on it reconnects.",
                _ => "Stays quiet: the tab says Disconnected until a key or a click on it reconnects.",
            };
            text.into()
        })
        .about("reconnecting", ABOUT_RECONNECT)
        .saves(&["reconnectMode"])
        .words("reconnect disconnect auto ask network"),
        Row::new(
            "term-idle",
            Terminal,
            "Disconnect unused terminals",
            Choice {
                look: POP,
                options: &[("0", "Off"), ("15", "15 min"), ("20", "20 min"), ("30", "30 min"), ("60", "1 h")],
                get: |st| format!("{}", st.get().idle_sleep),
                set: |st, v| {
                    if let Ok(minutes) = v.parse::<f64>() {
                        st.update(|s| s.idle_sleep = minutes);
                    }
                },
            },
        )
        .group("Connection")
        .hint_by(|st, _| {
            let minutes = st.get().idle_sleep;
            if minutes > 0.0 {
                let after = if minutes < 60.0 { format!("{minutes} min") } else { "an hour".into() };
                format!("A terminal unused for {after} lets go of its connection where tmux keeps the shell. Any key reconnects.")
            } else {
                "Terminals stay connected for as long as they're open.".into()
            }
        })
        .about("disconnecting unused terminals", ABOUT_IDLE)
        .saves(&["idleSleep"])
        .words("idle afk sleep disconnect unused away tmux timeout"),
        Row::new(
            "term-count-typing",
            Terminal,
            "Count how much typing could appear instantly",
            Switch { get: |st| st.get().count_typing, set: |st, v| st.set(|s| s.count_typing = v) },
        )
        .group("Connection")
        .hint("A test for a possible feature. It keeps counts only, never what you type.")
        .about("counting typing", ABOUT_COUNT_TYPING)
        .saves(&["countTyping"])
        .words("local echo predict latency lag slow tor delay test measure statistics"),
        Row::new(
            "term-welcome",
            Terminal,
            "Show",
            Choice {
                look: POP,
                options: &[
                    ("auto", "The server's own welcome, or SSHeesh's if it has none"),
                    ("ssheesh", "SSHeesh's, every time"),
                    ("first", "SSHeesh's the first time, then the server's"),
                    ("server", "The server's own, every time"),
                    ("off", "Nothing"),
                ],
                get: |st| st.get().welcome_mode.as_str().to_owned(),
                set: |st, v| st.set(|s| s.welcome_mode = WelcomeMode::from(v)),
            },
        )
        .group("Welcome")
        .hint("What a new terminal shows first. The server's own notices always show.")
        .about("the welcome", ABOUT_WELCOME)
        .saves(&["welcomeMode"])
        .words("motd welcome banner logo login message greeting"),
        Row::new(
            "term-welcome-parts",
            Terminal,
            "SSHeesh's welcome shows",
            Toggles {
                options: &[("logo", "Logo"), ("system", "System"), ("usage", "Memory and disk"), ("status", "Containers and updates")],
                get: |st| st.get().welcome_parts.iter().map(|p| p.as_str().to_owned()).collect(),
                toggle: |st, part| {
                    let part = WelcomePart::from(part);
                    st.update(|s| match s.welcome_parts.iter().position(|p| *p == part) {
                        Some(at) => drop(s.welcome_parts.remove(at)),
                        None => s.welcome_parts.push(part),
                    });
                },
            },
        )
        .group("Welcome")
        .hint("Read from the server each time, without waiting on the network.")
        .saves(&["welcomeParts"])
        .words("motd welcome banner logo system memory disk containers updates")
        .when(welcome_is_ours),
        Row::new(
            "term-welcome-note",
            Terminal,
            "Your own line",
            Text { placeholder: "Nothing", max: 120, get: |st| st.get().welcome_note.clone(), set: |st, v| st.set(|s| s.welcome_note = v.to_owned()) },
        )
        .group("Welcome")
        .hint("Under SSHeesh's welcome, like a note to yourself: “production, careful”.")
        .saves(&["welcomeNote"])
        .words("motd welcome note message")
        .when(welcome_is_ours),
        // Copying: 0.10's `copyOnSelect` as a choice of two, and whether a program may copy.
        Row::new(
            "term-copy",
            Terminal,
            "Copy from the terminal",
            Choice {
                look: POP,
                options: &[("select", "When I select text"), ("keys", "Only with Ctrl+Shift+C")],
                get: |st| if st.copy_on_select() { "select" } else { "keys" }.to_owned(),
                set: |st, v| st.set(|s| s.copy_on_select = v == "select"),
            },
        )
        .on_mac(&["When I select text", "Only with ⌘C"])
        .group("Copying")
        .saves(&["copyOnSelect"])
        .words("clipboard selection mouse copy on select ctrl+shift+c"),
        Row::new(
            "term-program-clipboard",
            Terminal,
            "Programs can copy to my clipboard",
            Switch { get: |st| st.program_clipboard(), set: |st, v| st.set_more(|m| m.program_clipboard = v) },
        )
        .group("Copying")
        .hint_by(|_, on| {
            let keys = if on.mac() { "⌘C" } else { "Ctrl+Shift+C" };
            format!("Agents like OpenCode copy what you select inside them. If that fights with SSHeesh's own copying, choose 'Only with {keys}' above.")
        })
        .about("programs copying", ABOUT_PROGRAM_CLIPBOARD)
        .adds(&["programClipboard"])
        .words("clipboard osc 52 tmux agent opencode claude copy program"),
        Row::new("term-hl-errors", Terminal, "Errors", Switch { get: |st| st.get().highlight_errors, set: |st, v| st.set(|s| s.highlight_errors = v) })
            .group("Highlighting")
            .hint("ERROR, FATAL, failed:, Permission denied, command not found…")
            .saves(&["highlightErrors"])
            .words("highlight red error output logs"),
        Row::new(
            "term-hl-warnings",
            Terminal,
            "Warnings",
            Switch { get: |st| st.get().highlight_warnings, set: |st, v| st.set(|s| s.highlight_warnings = v) },
        )
        .group("Highlighting")
        .hint("WARN, warning:, deprecated…")
        .saves(&["highlightWarnings"])
        .words("highlight yellow warn output logs"),
        Row::new(
            "term-hl-addresses",
            Terminal,
            "Addresses",
            Switch { get: |st| st.get().highlight_addresses, set: |st, v| st.set(|s| s.highlight_addresses = v) },
        )
        .group("Highlighting")
        .hint("IP addresses and web links.")
        .saves(&["highlightAddresses"])
        .words("highlight ip links urls"),
        Row::new("term-hl-words", Terminal, "Your own words", Control::Special(Special::HighlightRules))
            .group("Highlighting")
            .hint("Plain text matches in any case. Write /like this/ for a regular expression.")
            .saves(&["highlightRules"])
            .words("highlight custom rules regex keywords"),
        // ------------------------------------------------------------------ Keyboard
        Row::new("keys-shortcuts", Keys, "Keyboard shortcuts", Control::Special(Special::Shortcuts))
            .saves(&["shortcuts"])
            .words("keys hotkeys keybindings bindings combos remap"),
        // ------------------------------------------------------------------ Privacy
        Row::new("privacy-streamer", Privacy, "Streamer mode", Switch { get: |st| st.get().streamer_mode, set: |st, v| st.set(|s| s.streamer_mode = v) })
            .hint_by(|st, on| {
                format!(
                    "Hides server names, addresses and user names everywhere, terminal output included. {} turns it on and off.",
                    keymap::shortcut_label("streamer", on, &st.get().shortcuts)
                )
            })
            .saves(&["streamerMode"])
            .words("hide names ip addresses screen share recording obs twitch"),
        Row::new("privacy-secrets", Privacy, "Hide secrets", Switch { get: |st| st.get().hide_secrets, set: |st, v| st.set(|s| s.hide_secrets = v) })
            .hint("Tokens, keys and passwords in the output show as dots until clicked. Always on in streamer mode.")
            .saves(&["hideSecrets"])
            .words("tokens keys passwords api redact mask privacy streamer env"),
        Row::new(
            "files-clean",
            Privacy,
            "Clean files before uploading",
            Switch { get: |st| st.get().clean_uploads, set: |st, v| st.set(|s| s.clean_uploads = v) },
        )
        .hint("Photos lose place and camera, documents their author and company. Only the server's copy is cleaned; files inside folders go as they are.")
        .saves(&["cleanUploads"])
        .words("metadata exif gps location camera author strip remove photos pdf word upload"),
        Row::new("privacy-network", Privacy, "What SSHeesh connects to", Control::Special(Special::Network)).words("network telemetry tracking internet"),
        // ------------------------------------------------------------------ Files
        Row::new("files-editor", Files, "Open files in", Control::Special(Special::ExternalEditor))
            .hint("“Open in” edits a copy of a server's file here; every save goes back.")
            .adds(&["externalEditor"])
            .words("editor external vs code vscode zed sublime open in app"),
        // ------------------------------------------------------------------ Agents and notifications
        Row::new(
            "agents-notify",
            Agents,
            "Tell me when a terminal needs me",
            Switch { get: |st| st.get().notify_attention, set: |st, v| st.set(|s| s.notify_attention = v) },
        )
        .hint("When a program rings the bell or asks for you, as coding agents do. In the background, a system notification says so too.")
        .saves(&["notifyAttention"])
        .words("notification bell alert agent claude codex done attention"),
        // ------------------------------------------------------------------ Tor
        Row::new("tor-switch", Tor, "Connect through Tor", Control::Special(Special::TorSwitch))
            .hint("Servers see a Tor address, not yours, and your provider can't see which servers you use. If Tor can't carry a connection, it isn't made.")
            .about("Tor", ABOUT_TOR)
            .words("onion anonymous proxy hide ip address"),
        Row::new(
            "tor-typing",
            Tor,
            "Hide my typing rhythm",
            Switch { get: |st| st.get().tor_hide_typing, set: |st, v| st.set(|s| s.tor_hide_typing = v) },
        )
        .hint("Sends keys on a steady beat, so the network can't time your typing. Keys take about twice as long through Tor. For terminals opened from now on.")
        .about("hiding your typing rhythm", ABOUT_TOR_TYPING)
        .saves(&["torHideTyping"])
        .words("keystroke timing obfuscation rhythm password"),
        // ------------------------------------------------------------------ Updates
        Row::new("updates-card", Updates, "", Control::Special(Special::UpdateCard)),
        Row::new(
            "updates-canary",
            Updates,
            "Check for test builds",
            Switch { get: |st| st.canary_updates(), set: |st, v| st.set_more(|m| m.canary_updates = v) },
        )
        .hint("At start and every few hours; asks before restarting. Builds made on your own computer never look.")
        .adds(&["canaryUpdates"])
        .words("canary test build preview update nightly"),
        Row::new(
            "updates-mode",
            Updates,
            "Install updates",
            Choice {
                look: SEG,
                options: &[("auto", "Automatically"), ("notify", "Ask me"), ("manual", "Never check")],
                get: |st| st.get().update_mode.as_str().to_owned(),
                set: |st, v| st.set(|s| s.update_mode = UpdateMode::from(v)),
            },
        )
        .hint("At start and every few hours. Restarting reconnects your terminals.")
        .saves(&["updateMode"])
        .words("auto update version check"),
        // ------------------------------------------------------------------ About
        Row::new("about-card", About, "", Control::Special(Special::AboutCard)),
        Row::new(
            "about-links",
            About,
            "On the web",
            Buttons(&[
                ("GitHub", Action::Open(REPO)),
                ("Request a feature", Action::Open("https://github.com/footprintsntrails/ssheesh/issues/new/choose")),
                ("All releases", Action::Open("https://github.com/footprintsntrails/ssheesh/releases")),
            ]),
        )
        .words("github source code issues bug report releases"),
        Row::new(
            "about-tour",
            About,
            "Getting to know SSHeesh",
            Buttons(&[("What's new", Action::WhatsNew), ("Take the tour", Action::Tour), ("Show the welcome again", Action::Welcome)]),
        )
        .words("tour hello onboarding release notes changelog"),
        Row::new("about-tips", About, "Tips", Control::Special(Special::Tips))
            .hint("Small notes the first time you meet a feature, one at a time.")
            .saves(&["hintsSeen"])
            .words("hints help notes"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Json, Settings, Store};

    fn store() -> SettingsStore {
        SettingsStore::in_memory(Settings::defaults(Platform::Linux))
    }

    #[test]
    fn every_0_10_setting_has_a_row_or_is_kept_unseen() {
        let Json::Object(state) = Settings::defaults(Platform::Linux).to_state() else { panic!() };
        let owned: Vec<&str> = rows().iter().flat_map(|r| r.saves.iter().copied()).collect();
        for key in state.keys() {
            let rows = owned.iter().filter(|k| **k == key).count();
            let unseen = KEPT_UNSEEN.contains(&key.as_str());
            assert!(rows + usize::from(unseen) == 1, "{key}: {rows} row(s), kept unseen: {unseen}");
        }
        // And no row owns a setting that isn't one.
        for key in owned.iter().chain(&KEPT_UNSEEN) {
            assert!(state.contains_key(*key), "{key} isn't a setting of 0.10");
        }
        assert_eq!(owned.len() + KEPT_UNSEEN.len(), 60);
        // What 1.0 added has a row each, too.
        let Json::Object(more) = Json::of(&crate::settings_store::Additions::default()) else { panic!() };
        let added: Vec<&str> = rows().iter().flat_map(|r| r.adds.iter().copied()).collect();
        for key in more.keys() {
            assert_eq!(added.iter().filter(|k| **k == key).count(), 1, "{key}");
        }
        assert_eq!(added.len(), more.len());
    }

    #[test]
    fn a_rows_about_is_a_few_plain_sentences_where_a_newcomer_would_stop() {
        let with: Vec<&str> = rows().iter().filter(|r| r.about.is_some()).map(|r| r.key).collect();
        assert_eq!(with, ["layout-ssh-config", "term-reconnect", "term-idle", "term-count-typing", "term-welcome", "term-program-clipboard", "tor-switch", "tor-typing"]);
        for r in rows() {
            let Some(About { text: about, .. }) = r.about else { continue };
            let sentences = about.matches(". ").count() + 1;
            assert!(sentences <= 3 && about.len() <= 360 && about.ends_with('.'), "{}: {sentences} sentences, {} bytes", r.key, about.len());
            // It explains; it doesn't repeat the line under the name.
            assert!(r.help(&store(), Keyboard::Other).is_none_or(|h| h != about), "{}", r.key);
        }
        // Tor's row is shorter for it: what Tor is and where it doesn't reach went behind the "?".
        assert!(row("tor-switch").unwrap().help(&store(), Keyboard::Other).unwrap().len() < 160);
        assert!(row("tor-switch").unwrap().about.unwrap().text.contains("jump host"));
    }

    #[test]
    fn copying_is_a_choice_of_two_and_a_switch_as_each_system_names_the_keys() {
        let mut st = store();
        let copy = row("term-copy").unwrap();
        let program = row("term-program-clipboard").unwrap();
        assert_eq!((copy.group, program.group), (Some("Copying"), Some("Copying")));
        assert!(copy.works() && program.works());
        assert_eq!(copy.choice_labels(Keyboard::Other), ["When I select text", "Only with Ctrl+Shift+C"]);
        assert_eq!(copy.choice_labels(Keyboard::Windows), ["When I select text", "Only with Ctrl+Shift+C"]);
        assert_eq!(copy.choice_labels(Keyboard::Mac), ["When I select text", "Only with ⌘C"]);
        // The first choice is 0.10's default; the second is its `copyOnSelect` off.
        let Control::Choice { get, set, .. } = copy.control else { panic!() };
        assert_eq!(get(&st), "select");
        set(&mut st, "keys");
        assert!(!st.get().copy_on_select && !st.copy_on_select());
        assert_eq!(get(&st), "keys");
        set(&mut st, "select");
        assert!(st.copy_on_select());
        // The switch is 1.0's own key, on by default.
        let Control::Switch { get, set } = program.control else { panic!() };
        assert!(get(&st));
        set(&mut st, false);
        assert!(!st.program_clipboard() && !st.more().program_clipboard);
        assert_eq!(
            program.help(&st, Keyboard::Other).as_deref(),
            Some("Agents like OpenCode copy what you select inside them. If that fights with SSHeesh's own copying, choose 'Only with Ctrl+Shift+C' above.")
        );
        assert!(program.help(&st, Keyboard::Mac).unwrap().ends_with("choose 'Only with ⌘C' above."));
        // The two sit together, the choice first, and Reset puts both back.
        let keys: Vec<&str> = section_rows(Section::Terminal, &st, Platform::Linux).iter().map(|r| r.key).collect();
        let at = keys.iter().position(|k| *k == "term-copy").unwrap();
        assert_eq!(keys[at + 1], "term-program-clipboard");
        st.reset(&reset_keys(Section::Terminal));
        assert!(st.copy_on_select() && st.program_clipboard());
        // A row that isn't a choice has no labels.
        assert!(program.choice_labels(Keyboard::Mac).is_empty());
    }

    #[test]
    fn rows_have_one_key_each_and_every_section_has_rows() {
        let mut keys: Vec<&str> = rows().iter().map(|r| r.key).collect();
        let all = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), all, "a row key is used twice");
        for key in &keys {
            assert!(key.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'), "{key} isn't a fixed plain word");
        }
        let st = store();
        for section in Section::ALL {
            assert!(!section_rows(section, &st, Platform::Linux).is_empty(), "{section:?} is empty");
            assert_eq!(Section::from_id(section.id()), Some(section));
        }
        assert_eq!(Section::ALL.map(Section::label), ["Layout", "Appearance", "Terminal", "Keyboard", "Privacy", "Files", "Agents and notifications", "Tor", "Updates", "About"]);
        assert_eq!((Section::Layout.step(-1), Section::Layout.step(1), Section::About.step(1)), (Section::Layout, Section::Appearance, Section::About));
    }

    #[test]
    fn a_row_that_comes_later_says_so_in_one_line() {
        let later: Vec<&Row> = rows().iter().filter(|r| !r.works()).collect();
        // Only what belongs to features still being built.
        assert!(!later.is_empty() && later.len() <= 8, "{}", later.len());
        for row in later {
            let Status::Later(why) = row.status else { unreachable!() };
            assert!(!why.is_empty() && !why.contains('\n') && why.ends_with('.'), "{}: {why:?}", row.key);
            assert!(why.len() < 130, "{}: too long for one line", row.key);
        }
        // What the window and the terminal honour today works.
        for key in [
            "look-appearance",
            "term-font-size",
            "term-line-height",
            "term-cursor",
            "term-blink",
            "term-scheme",
            "layout-sidebar-side",
            "layout-sidebar-shown",
            "layout-tab-server",
            "privacy-streamer",
            "privacy-secrets",
            "keys-shortcuts",
            "term-padding",
            "term-idle",
            "look-size",
            "look-fade",
            "layout-radius",
            "layout-env-line",
            "files-clean",
            "agents-notify",
            "term-font",
            "look-theme",
            "look-accent",
            "look-logo",
            "layout-title-bar",
            "layout-server-load",
            "layout-server-icons",
        ] {
            assert!(row(key).is_some_and(Row::works), "{key}");
        }
    }

    #[test]
    fn controls_read_and_write_the_store() {
        let mut st = store();
        let flip = |st: &mut SettingsStore, key: &str| {
            let Control::Switch { get, set } = row(key).unwrap().control else { panic!("{key} isn't a switch") };
            let before = get(st);
            set(st, !before);
            assert_eq!(get(st), !before, "{key}");
        };
        // Every switch flips what it reads.
        let switches: Vec<&str> = rows().iter().filter(|r| matches!(r.control, Control::Switch { .. })).map(|r| r.key).collect();
        assert!(switches.len() >= 14, "{switches:?}");
        for key in switches {
            flip(&mut st, key);
        }
        assert!(st.get().streamer_mode && !st.get().hide_secrets && !st.more().tabs_name_server && !st.program_clipboard());
        // Every choice takes each of its options and reads it back; its value now is one of them.
        for row in rows() {
            let Control::Choice { options, get, set, .. } = row.control else { continue };
            assert!(options.iter().any(|(value, _)| *value == get(&st)), "{}: {} isn't an option", row.key, get(&st));
            for (value, _) in options {
                set(&mut st, value);
                assert_eq!(get(&st), *value, "{}", row.key);
            }
        }
        assert_eq!(st.get().cursor_style, CursorStyle::Underline);
        assert_eq!(st.get().idle_sleep, 60.0);
        // Every slider starts inside its range and takes both ends.
        let fresh = store();
        for row in rows() {
            let Control::Slider { min, max, step, get, set, show } = row.control else { continue };
            assert!((min..=max).contains(&get(&fresh)) && step > 0.0, "{}", row.key);
            for v in [min, max] {
                set(&mut st, v);
                assert_eq!(get(&st), v, "{}", row.key);
                assert!(!show(v).is_empty());
            }
        }
        assert_eq!((st.get().terminal_font_size, st.get().terminal_line_height), (20.0, 1.6));
        // The welcome's parts flip one at a time, and its note is plain text.
        let Control::Toggles { get, toggle, .. } = row("term-welcome-parts").unwrap().control else { panic!() };
        toggle(&mut st, "logo");
        assert_eq!(get(&st), ["system", "usage", "status"]);
        toggle(&mut st, "logo");
        assert_eq!(get(&st), ["system", "usage", "status", "logo"]);
        let Control::Text { get, set, max, .. } = row("term-welcome-note").unwrap().control else { panic!() };
        set(&mut st, "production, careful");
        assert_eq!((get(&st).as_str(), max), ("production, careful", 120));
    }

    #[test]
    fn appearance_is_one_choice_over_0_10s_theme_settings() {
        let mut st = store();
        let Control::Choice { get, set, .. } = row("look-appearance").unwrap().control else { panic!() };
        assert_eq!(get(&st), "dark");
        set(&mut st, "light");
        assert_eq!((get(&st).as_str(), st.get().theme.as_str()), ("light", "daylight"));
        set(&mut st, "system");
        assert!(st.get().theme_follows_system);
        set(&mut st, "sepia");
        assert_eq!(get(&st), "system");
    }

    #[test]
    fn rows_show_by_system_and_by_what_is_chosen() {
        let mut st = store();
        let keys = |st: &SettingsStore, platform| section_rows(Section::Layout, st, platform).iter().map(|r| r.key).collect::<Vec<_>>();
        assert!(keys(&st, Platform::Windows).contains(&"layout-title-bar"));
        assert!(!keys(&st, Platform::Linux).contains(&"layout-title-bar") && !keys(&st, Platform::Mac).contains(&"layout-title-bar"));
        let welcome = |st: &SettingsStore| section_rows(Section::Terminal, st, Platform::Linux).iter().filter(|r| r.group == Some("Welcome")).count();
        assert_eq!(welcome(&st), 3);
        st.update(|s| s.welcome_mode = WelcomeMode::Off);
        assert_eq!(welcome(&st), 1);
        // Help that depends on the choice, and on the system's keys.
        let help = |st: &SettingsStore, key: &str, on| row(key).unwrap().help(st, on).unwrap();
        assert!(help(&st, "layout-sidebar-shown", Keyboard::Other).contains("Ctrl+Shift+B"));
        assert!(help(&st, "layout-sidebar-shown", Keyboard::Mac).contains("⌘B"));
        assert!(help(&st, "privacy-streamer", Keyboard::Mac).contains("⇧⌘H turns it on and off."));
        assert!(help(&st, "term-idle", Keyboard::Other).contains("for 20 min lets go"));
        st.update(|s| s.idle_sleep = 60.0);
        assert!(help(&st, "term-idle", Keyboard::Other).contains("for an hour lets go"));
        st.update(|s| s.idle_sleep = 0.0);
        assert_eq!(help(&st, "term-idle", Keyboard::Other), "Terminals stay connected for as long as they're open.");
        assert_eq!(row("look-animations").unwrap().help(&st, Keyboard::Other), None);
    }

    #[test]
    fn search_finds_rows_by_their_name_and_by_other_words() {
        let find = |q: &str| search(q, Platform::Linux).into_iter().map(|f| (f.section, f.label)).collect::<Vec<_>>();
        assert_eq!(find(""), []);
        assert_eq!(find("   "), []);
        assert_eq!(find("font size"), [(Section::Terminal, "Font size")]);
        assert!(find("FONT").contains(&(Section::Terminal, "Font")));
        // Other ways people say it.
        assert_eq!(find("twitch"), [(Section::Privacy, "Streamer mode")]);
        assert!(find("dark mode").contains(&(Section::Appearance, "Appearance")));
        assert_eq!(find("exif"), [(Section::Privacy, "Clean files before uploading")]);
        assert_eq!(find("onion"), [(Section::Tor, "Connect through Tor")]);
        // Every word must be there.
        assert_eq!(find("font zebra"), []);
        // Shortcuts are found by name, once (the command bar has two keys), and lead to their row.
        let keys = search("command bar", Platform::Linux);
        assert_eq!(keys.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["shortcut-palette"]);
        assert!(search("shortcut", Platform::Linux).len() > 15);
        assert!(search("streamer", Platform::Linux).iter().any(|f| f.key == "shortcut-streamer"));
        // A Windows-only row isn't found elsewhere.
        assert_eq!(find("title bar"), []);
        assert_eq!(search("title bar", Platform::Windows).len(), 1);
    }

    #[test]
    fn reset_puts_back_every_row_of_a_section_but_what_you_made() {
        let terminal = reset_keys(Section::Terminal);
        for key in ["terminalFontSize", "terminalLineHeight", "cursorStyle", "cursorBlink", "terminalScheme", "copyOnSelect", "highlightErrors", "welcomeMode", "idleSleep", "countTyping"] {
            assert!(terminal.contains(&key), "{key}");
        }
        // Every row, as 0.10's Reset: the ones another part of the app honours too.
        for key in ["terminalFont", "countTyping", "terminalPadding", "idleSleep"] {
            assert!(terminal.contains(&key), "{key}");
        }
        // What you made stays.
        assert!(!terminal.contains(&"terminalColors") && !reset_keys(Section::Appearance).contains(&"customThemes"));
        assert!(reset_keys(Section::Appearance).contains(&"theme") && reset_keys(Section::Appearance).contains(&"accent"));
        assert_eq!(reset_keys(Section::Keyboard), ["shortcuts"]);
        assert!(reset_keys(Section::Layout).contains(&"sidebarSide") && !reset_keys(Section::Layout).contains(&"streamerMode"));
        assert!(reset_keys(Section::Layout).contains(&"tabsNameServer") && terminal.contains(&"programClipboard"));
        assert!(Section::Terminal.has_reset() && !Section::Privacy.has_reset() && !Section::Tor.has_reset());
        let mut st = store();
        st.update(|s| {
            s.terminal_font_size = 18.0;
            s.terminal_font = "menlo".into();
            s.streamer_mode = true;
        });
        st.reset(&terminal);
        assert_eq!((st.get().terminal_font_size, st.get().terminal_font.as_str(), st.get().streamer_mode), (13.0, "system", true));
    }
}
