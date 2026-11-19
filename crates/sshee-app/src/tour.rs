//! The hello, the tour and the hints, without a screen (0.10's `Welcome.tsx`, `Tour.tsx`,
//! `Hints.tsx`).
//!
//! - **The welcome** shows once ([`welcome_due`]), to everyone: a new install, a 0.10 user and
//!   whoever saw an earlier 1.0 welcome. A few short pages ([`PAGES`]): your servers (1.0 keeps
//!   a list of its own, so 0.10's are offered to bring over, and the ssh config's to pick
//!   from), then how the window is laid out, the sidebar, how it looks and the terminal, each
//!   big choice with a picture of what it changes, then three things people miss. A choice is
//!   a row of Settings ([`Page::rows`]), changed through that row as Settings does; what's on
//!   when it opens stays picked, so clicking through changes nothing. Skipping, at any page,
//!   decides nothing for you.
//! - **The tour** is the simple one (plan decision Р7): six cards, a title and a line each,
//!   with the shortcut that does it. 0.10's animated scenes come later.
//! - **Hints** are one small note at a time about something on screen, the first time it's
//!   worth saying: none in the first minute, 90 seconds apart, three a run at most, never
//!   over a dialog, and never one you've found yourself ([`next_hint`]). They never take the
//!   keyboard.

use crate::model::Settings;
use crate::servers::{Source, Sources};
use crate::settings_store::catalog::{self, Control, Row, Special};
use crate::settings_store::schemes::APP_THEMES;
use crate::settings_store::{looks, SettingsStore};
use crate::workbench::shortcuts::Shortcut;

/// The welcome's version: 0.10's was 2, 1.0's first (your servers only) 3, and 4 the one with
/// light or dark and the terminal but not the layout; so everyone sees this one once,
/// whichever they saw before.
pub const ONBOARDING_VERSION: u32 = 5;

/// Whether the welcome opens at this start.
pub fn welcome_due(settings: &Settings) -> bool {
    settings.onboarding_version < ONBOARDING_VERSION
}

/// The settings once the welcome is done or skipped.
pub fn welcomed(settings: &mut Settings) {
    settings.onboarded = true;
    settings.onboarding_version = ONBOARDING_VERSION;
}

/// A page of the welcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Bring 0.10's servers over, pick from the ssh config, or add one.
    Servers,
    /// Where tabs go, two at once, compact mode, and the window's bars and lines.
    Layout,
    /// The sidebar's side, its list and icons, how busy a server looks, and the logo.
    Sidebar,
    /// Light or dark, the dark theme, the accent, the size, and motion.
    Look,
    /// Copying, and what a dropped connection does.
    Terminal,
    /// Three things people miss.
    Tips,
}

/// The welcome's pages, in order.
pub const PAGES: [Page; 6] = [Page::Servers, Page::Layout, Page::Sidebar, Page::Look, Page::Terminal, Page::Tips];

impl Page {
    /// A fixed word for its element's key.
    pub fn id(self) -> &'static str {
        match self {
            Page::Servers => "servers",
            Page::Layout => "layout",
            Page::Sidebar => "sidebar",
            Page::Look => "look",
            Page::Terminal => "terminal",
            Page::Tips => "tips",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Servers => "Your servers",
            Page::Layout => "Layout",
            Page::Sidebar => "Sidebar",
            Page::Look => "How it looks",
            Page::Terminal => "In the terminal",
            Page::Tips => "Three things people miss",
        }
    }

    /// The settings it offers, by their rows' keys in Settings (`settings_store::catalog`), in
    /// the order shown. Left to Settings: the sliders (the space between panes, their corners,
    /// fading the pane not in use), hiding the sidebar (its shortcut does that), and Windows'
    /// own title bar (for when the window misbehaves).
    pub fn rows(self) -> &'static [&'static str] {
        match self {
            Page::Layout => &["layout-tabs", "layout-view", "layout-compact", "layout-tab-server", "layout-vitals", "layout-env-line"],
            Page::Sidebar => &["layout-sidebar-side", "layout-server-list", "layout-server-icons", "layout-server-load", "look-logo"],
            Page::Look => &["look-appearance", "look-theme", "look-accent", "look-size", "look-animations"],
            Page::Terminal => &["term-copy", "term-reconnect"],
            Page::Servers | Page::Tips => &[],
        }
    }

    /// The rows it shows: those 1.0 acts on that the welcome can offer ([`options`]).
    pub fn choices(self) -> Vec<&'static Row> {
        self.rows().iter().filter_map(|key| catalog::row(key)).filter(|r| r.works() && !options(r.key).is_empty()).collect()
    }
}

/// The dark themes, for the welcome's "Dark theme": light has one, Daylight, and "Light" is it.
pub fn dark_themes() -> impl Iterator<Item = (&'static str, &'static str)> {
    APP_THEMES.into_iter().filter(|(id, _)| !looks::resolve(id, &[]).light)
}

/// What the row `key` can be in the welcome, each `(value, label)`: a choice's options, a
/// switch's on and off, the dark themes, the accents. Empty: not something it offers.
pub fn options(key: &str) -> Vec<(&'static str, &'static str)> {
    match catalog::row(key).map(|r| r.control) {
        Some(Control::Choice { options, .. }) => options.to_vec(),
        Some(Control::Switch { .. }) => vec![("on", "On"), ("off", "Off")],
        Some(Control::Special(Special::Theme)) => dark_themes().collect(),
        // Settings keeps them as (name, value).
        Some(Control::Special(Special::Accent)) => looks::ACCENTS.iter().map(|&(name, value)| (value, name)).collect(),
        _ => vec![],
    }
}

/// The page after `at` going `by`, held at the ends.
pub fn page(at: usize, by: i32) -> usize {
    (at as i64 + by as i64).clamp(0, PAGES.len() as i64 - 1) as usize
}

/// The value the row `key` has now, as [`options`] names it. The dark theme is the one drawn
/// whenever the window is dark: while following the system, the one for that side.
pub fn chosen(store: &SettingsStore, key: &str) -> Option<String> {
    let s = store.get();
    match catalog::row(key)?.control {
        Control::Choice { get, .. } => Some(get(store)),
        Control::Switch { get, .. } => Some(if get(store) { "on" } else { "off" }.to_owned()),
        Control::Special(Special::Theme) => Some(if s.theme_follows_system { s.dark_theme.clone() } else { s.theme.clone() }),
        Control::Special(Special::Accent) => {
            let preset = looks::ACCENTS.iter().find(|(_, v)| v.eq_ignore_ascii_case(s.accent.trim()));
            Some(preset.map_or_else(|| s.accent.clone(), |(_, v)| (*v).to_owned()))
        }
        _ => None,
    }
}

/// Picks `value` for the row `key` through the row itself, as Settings does: a theme as its
/// picture in Settings picks it, an accent as its swatch does. False: no such row, or no such
/// value of it.
pub fn pick(store: &mut SettingsStore, key: &str, value: &str) -> bool {
    let Some(control) = catalog::row(key).map(|r| r.control) else { return false };
    if !options(key).iter().any(|(v, _)| *v == value) {
        return false;
    }
    match control {
        Control::Choice { set, .. } => set(store, value),
        Control::Switch { set, .. } => set(store, value == "on"),
        Control::Special(Special::Theme) => looks::pick(store, value),
        Control::Special(Special::Accent) => store.set(|s| s.accent = value.to_owned()),
        _ => return false,
    }
    true
}

/// The line under the copying choice, for the value picked (`mac`: ⌘C, else Ctrl+Shift+C).
pub fn copy_line(value: &str, mac: bool) -> String {
    let keys = if mac { "⌘C" } else { "Ctrl+Shift+C" };
    match value {
        "select" => "Select text and it's copied.".into(),
        _ => format!("Selecting only selects; {keys} copies."),
    }
}

/// Something people miss, on the welcome's last page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tip {
    /// A fixed word for its element's key.
    pub id: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    /// The shortcut that does it.
    pub keys: Shortcut,
}

/// The welcome's last page. Streamer mode isn't one: it's for the few who stream.
pub const TIPS: [Tip; 3] = [
    Tip { id: "palette", title: "Find anything", body: "Servers, folders, commands and settings, from one bar.", keys: Shortcut::Palette },
    Tip { id: "files", title: "Files on the server", body: "Browse and edit them, and drag files in or out.", keys: Shortcut::OpenFiles },
    Tip { id: "agents", title: "Every agent, in one list", body: "Claude Code, Codex and the rest, on every server, the ones waiting for you first.", keys: Shortcut::Agents },
];

/// What the welcome says over your servers, by how many the list has (this computer not
/// counted) and what can be brought in: from 0.10, from `~/.ssh/config`, and from XPipe and
/// other SSH apps (`from_apps`, all of theirs together).
pub fn welcome_line(servers: usize, from_old: usize, from_config: usize, from_apps: usize) -> String {
    let s = |n: usize| if n == 1 { "" } else { "s" };
    match (servers, from_old, from_config, from_apps) {
        (0, 0, 0, 0) => "Add your first one: paste its address, or a whole ssh command, and SSHeesh fills in the rest.".into(),
        (0, old, _, _) if old > 0 => format!("SSHeesh 1.0 keeps its own list. Bring your {old} server{} over from 0.10, or add one.", s(old)),
        (0, _, config, _) if config > 0 => format!("Found {config} in your SSH config. Pick the ones you want; the file isn't changed."),
        (0, _, _, apps) => format!("Found {apps} server{} in your other SSH apps. Bring over the ones you want; nothing there is changed.", s(apps)),
        (n, _, _, _) => format!("Your {n} server{} {} here.", s(n), if n == 1 { "is" } else { "are" }),
    }
}

/// Where else servers were found, beyond 0.10 and `~/.ssh/config` (the welcome offers those
/// by their own counts): XPipe, then the other SSH apps, each with how many it has that the
/// list hasn't. None found, none listed.
pub fn found_elsewhere(sources: &Sources) -> Vec<(Source, usize)> {
    let xpipe = (!sources.xpipe.is_empty()).then(|| (Source::Xpipe, sources.xpipe.len()));
    let apps = sources.apps.iter().filter(|o| !o.servers.is_empty()).map(|o| (Source::App(o.app), o.servers.len()));
    xpipe.into_iter().chain(apps).collect()
}

/// The welcome's row for a source found elsewhere: "Bring over 12 servers from XPipe".
pub fn bring_over(source: Source, n: usize) -> String {
    let from = match source {
        Source::Xpipe => "XPipe",
        Source::App(app) => app.name(),
        Source::Old => "SSHeesh 0.10",
        Source::SshConfig => "~/.ssh/config",
    };
    format!("Bring over {n} server{} from {from}", if n == 1 { "" } else { "s" })
}

/// One card of the tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// A fixed word for its element's key.
    pub id: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    /// One quieter line for those who want more.
    pub detail: &'static str,
    /// The shortcut that does it, if one does.
    pub keys: Option<Shortcut>,
}

/// The tour, in order. Streamer mode isn't one of its cards: it's for the few who stream, and
/// it lives in Settings → Privacy, the palette and its shortcut.
pub const STEPS: [Step; 5] = [
    Step {
        id: "find",
        title: "Find anything",
        body: "Servers, folders, commands and settings, from one bar.",
        detail: "Commands say which server they run on before they run.",
        keys: Some(Shortcut::Palette),
    },
    Step {
        id: "tabs",
        title: "Every server keeps its tabs",
        body: "Each server has its own tabs along the top. The sidebar says when an agent on any server needs you.",
        detail: "“Needs you” means an agent is waiting for an answer.",
        keys: None,
    },
    Step {
        id: "durable",
        title: "Lost Wi-Fi? Work keeps running",
        body: "Your session stays alive on the server and picks up where it was.",
        detail: "Uses tmux on the server.",
        keys: None,
    },
    Step {
        id: "check",
        title: "Check a server's safety",
        body: "The shield runs a security check: logins, open ports, the firewall and updates, in plain words.",
        detail: "Nothing changes until you run a fix, and Production asks twice.",
        keys: None,
    },
    Step {
        id: "split",
        title: "Two tabs at once",
        body: "Show two tabs of a server side by side, and go back to one at a time.",
        detail: "Right-click a tab for more.",
        keys: Some(Shortcut::SideBySide),
    },
];

/// The card after `at` going `by` steps, held at the ends.
pub fn step(at: usize, by: i32) -> usize {
    (at as i64 + by as i64).clamp(0, STEPS.len() as i64 - 1) as usize
}

/// A hint: a note beside something on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    /// 0.10's id, kept in Settings' `hintsSeen` once shown.
    pub id: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    /// What it's about, as the window has it on screen.
    pub about: About,
}

/// What a hint is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum About {
    /// A server's tabs, when it has two or more.
    Tabs,
    /// A terminal that's connected.
    Terminal,
    /// An agent working in a tab.
    Agent,
    /// The security check, on a server you've had open a while.
    Check,
}

/// What the window looks like now, as far as hints care.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Moment {
    /// Seconds since the window started.
    pub since_start: f64,
    /// Seconds since the last hint showed (`None`: none yet this run).
    pub since_last: Option<f64>,
    pub shown_this_run: usize,
    /// A dialog, the palette or Settings is over the window.
    pub busy: bool,
    pub tabs_on_server: usize,
    pub side_by_side: bool,
    pub terminal_connected: bool,
    pub agent_working: bool,
    pub on_a_server: bool,
}

/// The hints, in the order they're worth knowing.
pub const HINTS: [Hint; 4] = [
    Hint { id: "tabs", title: "Each server has its own tabs", body: "Right-click a tab for more, or show two side by side.", about: About::Tabs },
    Hint { id: "peek", title: "Click a path", body: "Ctrl-click a path or a link in the output to open it.", about: About::Terminal },
    Hint { id: "agents", title: "Agents show up by name", body: "On every server, so you see them working while you're elsewhere. “Needs you” means one is waiting for an answer.", about: About::Agent },
    Hint { id: "checkup", title: "Is this server safe?", body: "The shield in the sidebar checks logins, open ports, the firewall and updates. Nothing changes until you say so.", about: About::Check },
];

/// None this soon after the start, this long apart, and this many a run at most.
pub const QUIET_START: f64 = 60.0;
pub const APART: f64 = 90.0;
pub const PER_RUN: usize = 3;

/// The hint to show now, if any: the first not seen whose moment has come.
pub fn next_hint(seen: &[String], now: &Moment) -> Option<&'static Hint> {
    let quiet = now.since_start < QUIET_START || now.since_last.is_some_and(|t| t < APART) || now.shown_this_run >= PER_RUN || now.busy;
    if quiet || seen.iter().any(|s| s == "*") {
        return None;
    }
    HINTS.iter().find(|h| {
        if seen.iter().any(|s| s == h.id) {
            return false;
        }
        match h.about {
            // Found it yourself: two side by side.
            About::Tabs => now.tabs_on_server >= 2 && !now.side_by_side,
            About::Terminal => now.terminal_connected,
            About::Agent => now.agent_working,
            About::Check => now.on_a_server && now.terminal_connected,
        }
    })
}

/// "Got it": this one is seen. "No more tips": every one is.
pub fn mark_seen(settings: &mut Settings, id: &str) {
    if !settings.hints_seen.iter().any(|s| s == id) {
        settings.hints_seen.push(id.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_welcome_shows_once_and_to_0_10_users_too() {
        let mut s = Settings::default();
        assert!(welcome_due(&s));
        // A 0.10 user who saw 0.10's hello sees 1.0's: 1.0 keeps its own list of servers.
        s.onboarded = true;
        s.onboarding_version = 2;
        assert!(welcome_due(&s));
        welcomed(&mut s);
        assert!(!welcome_due(&s) && s.onboarded);
    }

    #[test]
    fn whoever_saw_the_first_1_0_welcome_sees_this_one_once() {
        let mut s = Settings { onboarded: true, onboarding_version: 3, ..Settings::default() };
        assert!(welcome_due(&s));
        // And whoever saw the one without the layout (canary's 4).
        assert!(welcome_due(&Settings { onboarded: true, onboarding_version: 4, ..Settings::default() }));
        welcomed(&mut s);
        assert!(!welcome_due(&s));
        // A newer version's welcome seen, then back on this one: it doesn't show again.
        s.onboarding_version = ONBOARDING_VERSION + 1;
        assert!(!welcome_due(&s));
    }

    #[test]
    fn every_choice_is_a_working_row_of_settings() {
        for page in PAGES {
            let choices = page.choices();
            assert_eq!(choices.len(), page.rows().len(), "{page:?}: a row is missing, disabled or not offered");
            for row in choices {
                let options = options(row.key);
                assert!((2..=10).contains(&options.len()), "{}: {} to pick from", row.key, options.len());
                // Every value it offers is one the row takes.
                let mut store = SettingsStore::in_memory(Settings::default());
                for (value, _) in &options {
                    assert!(pick(&mut store, row.key, value), "{}: {value}", row.key);
                    assert_eq!(chosen(&store, row.key).as_deref(), Some(*value), "{}", row.key);
                }
            }
        }
        assert_eq!(PAGES.map(Page::id), ["servers", "layout", "sidebar", "look", "terminal", "tips"]);
        assert_eq!(page(0, -1), 0);
        assert_eq!(page(PAGES.len() - 1, 1), PAGES.len() - 1);
    }

    #[test]
    fn the_layout_and_the_look_are_all_offered_but_the_fine_tuning() {
        use crate::settings_store::catalog::Section;
        let offered: Vec<&str> = PAGES.iter().flat_map(|p| p.rows().iter().copied()).collect();
        // Left to Settings: sliders, hiding the sidebar (a shortcut), Windows' title bar.
        let left = ["layout-gap", "layout-radius", "look-fade", "layout-sidebar-shown", "layout-title-bar", "layout-ssh-config", "look-light-dark"];
        for r in catalog::rows().iter().filter(|r| matches!(r.section, Section::Layout | Section::Appearance)) {
            assert!(offered.contains(&r.key) != left.contains(&r.key), "{}: offered, or named as left to Settings", r.key);
        }
    }

    #[test]
    fn a_choice_changes_the_setting_as_settings_does_and_nothing_else() {
        use crate::model::{CompactMode, IconPack, RailStatus, ReconnectMode, Side, SidebarMode, TabsPlacement, ViewMode};
        let mut store = SettingsStore::in_memory(Settings::default());
        let before = store.get().clone();
        // What's on stays picked: the defaults, and nothing changes by looking.
        for (key, value) in [
            ("term-copy", "select"),
            ("term-reconnect", "ask"),
            ("layout-sidebar-side", "left"),
            ("layout-tabs", "top"),
            ("layout-view", "tabs"),
            ("layout-compact", "off"),
            ("layout-vitals", "on"),
            ("layout-env-line", "off"),
            ("layout-server-list", "narrow"),
            ("layout-server-icons", "line"),
            ("look-theme", "ssheesh"),
            ("look-accent", "#4eba65"),
            ("look-size", "100"),
            ("look-animations", "on"),
        ] {
            assert_eq!(chosen(&store, key).as_deref(), Some(value), "{key}");
        }
        assert_eq!(store.get(), &before);
        assert!(pick(&mut store, "term-copy", "keys"));
        assert!(!store.get().copy_on_select);
        assert!(pick(&mut store, "term-reconnect", "auto"));
        assert_eq!(store.get().reconnect_mode, ReconnectMode::Auto);
        assert!(pick(&mut store, "look-appearance", "light"));
        assert!(!store.get().theme_follows_system && store.get().theme == "daylight");
        assert!(pick(&mut store, "layout-sidebar-side", "right"));
        assert_eq!(store.get().sidebar_side, Side::Right);
        // The layout, each through its row.
        assert!(pick(&mut store, "layout-tabs", "sidebar") && pick(&mut store, "layout-view", "split") && pick(&mut store, "layout-compact", "both"));
        let s = store.get();
        assert!(s.tabs_placement == TabsPlacement::Sidebar && s.view_mode == ViewMode::Split && s.compact_mode == CompactMode::Both);
        assert!(pick(&mut store, "layout-vitals", "off") && pick(&mut store, "layout-env-line", "on") && pick(&mut store, "layout-tab-server", "off"));
        let s = store.get();
        assert!(!s.show_vitals && s.env_stripe && !store.more().tabs_name_server);
        assert!(pick(&mut store, "layout-server-list", "wide") && pick(&mut store, "layout-server-icons", "pixel") && pick(&mut store, "layout-server-load", "bars"));
        let s = store.get();
        assert!(s.sidebar_mode == SidebarMode::Wide && s.icon_pack == IconPack::Pixel && s.rail_status == RailStatus::Bars);
        // The look: a dark theme from light goes dark in it, as its picture in Settings does.
        assert!(pick(&mut store, "look-theme", "midnight"));
        assert_eq!(crate::settings_store::schemes::Appearance::of(store.get()), crate::settings_store::schemes::Appearance::Dark);
        assert_eq!(store.get().theme, "midnight");
        assert!(pick(&mut store, "look-accent", "#7c83ff"));
        assert!(pick(&mut store, "look-size", "110"));
        assert!(pick(&mut store, "look-animations", "off"));
        let s = store.get();
        assert!(s.accent == "#7c83ff" && s.ui_zoom == 110.0 && !s.animations);
        // Following the system, the dark theme is the dark side's, whatever the system is now.
        assert!(pick(&mut store, "look-appearance", "system") && pick(&mut store, "look-theme", "oled"));
        assert_eq!(store.get().dark_theme, "oled");
        assert_eq!(chosen(&store, "look-theme").as_deref(), Some("oled"));
        // Nothing it doesn't know: no such value, no such row, a row it doesn't offer.
        let now = store.get().clone();
        assert!(!pick(&mut store, "term-copy", "always"));
        assert!(!pick(&mut store, "no-such-row", "x"));
        assert!(!pick(&mut store, "term-font-size", "14"));
        assert!(!pick(&mut store, "look-theme", "daylight"));
        assert!(!pick(&mut store, "look-accent", "#123456"));
        assert!(!pick(&mut store, "layout-gap", "4"));
        assert_eq!(store.get(), &now);
        assert_eq!(chosen(&store, "layout-gap"), None);
    }

    #[test]
    fn the_last_page_names_no_streamer_mode() {
        assert!(TIPS.iter().all(|t| !t.title.contains("Streamer") && !t.body.contains("Streamer")));
        assert!(copy_line("select", false).starts_with("Select text"));
        assert_eq!(copy_line("keys", true), "Selecting only selects; ⌘C copies.");
    }

    #[test]
    fn the_welcome_line_says_where_servers_come_from() {
        assert!(welcome_line(0, 0, 0, 0).starts_with("Add your first one"));
        assert_eq!(welcome_line(0, 3, 2, 5), "SSHeesh 1.0 keeps its own list. Bring your 3 servers over from 0.10, or add one.");
        assert_eq!(welcome_line(0, 1, 0, 0), "SSHeesh 1.0 keeps its own list. Bring your 1 server over from 0.10, or add one.");
        assert!(welcome_line(0, 0, 4, 2).starts_with("Found 4 in your SSH config"));
        assert_eq!(welcome_line(0, 0, 0, 12), "Found 12 servers in your other SSH apps. Bring over the ones you want; nothing there is changed.");
        assert_eq!(welcome_line(1, 0, 0, 0), "Your 1 server is here.");
        assert_eq!(welcome_line(5, 2, 0, 3), "Your 5 servers are here.");
    }

    #[test]
    fn only_the_apps_that_have_servers_are_offered_by_name() {
        use crate::servers::{App, AppOffer, Candidate};
        let c = |id: &str| Candidate { id: id.into(), name: id.into(), address: id.into(), identity_file: None, had_password: false, picked: true };
        let offer = |app, servers: Vec<Candidate>| AppOffer { app, servers, here: 0, through_jump: 0, ppk_keys: 0 };
        let mut sources = Sources {
            old: vec![c("old")],
            xpipe: vec![c("a"), c("b")],
            apps: vec![offer(App::Putty, vec![c("p")]), offer(App::WinScp, vec![]), offer(App::Remmina, vec![c("r1"), c("r2"), c("r3")])],
            ..Sources::default()
        };
        assert_eq!(found_elsewhere(&sources), [(Source::Xpipe, 2), (Source::App(App::Putty), 1), (Source::App(App::Remmina), 3)]);
        assert_eq!(bring_over(Source::Xpipe, 2), "Bring over 2 servers from XPipe");
        assert_eq!(bring_over(Source::App(App::Putty), 1), "Bring over 1 server from PuTTY");
        sources.xpipe.clear();
        sources.apps.clear();
        assert!(found_elsewhere(&sources).is_empty());
    }

    #[test]
    fn the_tour_holds_at_its_ends() {
        assert_eq!(step(0, -1), 0);
        assert_eq!(step(0, 1), 1);
        assert_eq!(step(STEPS.len() - 1, 1), STEPS.len() - 1);
        let ids: Vec<_> = STEPS.iter().map(|s| s.id).collect();
        let mut unique = ids.clone();
        unique.dedup();
        assert_eq!(ids, unique);
    }

    #[test]
    fn hints_wait_their_turn_and_never_repeat() {
        let base = Moment { since_start: 120.0, tabs_on_server: 2, terminal_connected: true, on_a_server: true, ..Moment::default() };
        assert_eq!(next_hint(&[], &base).map(|h| h.id), Some("tabs"));
        // Not in the first minute, not right after another, not over a dialog, not past three.
        assert!(next_hint(&[], &Moment { since_start: 30.0, ..base }).is_none());
        assert!(next_hint(&[], &Moment { since_last: Some(20.0), ..base }).is_none());
        assert!(next_hint(&[], &Moment { busy: true, ..base }).is_none());
        assert!(next_hint(&[], &Moment { shown_this_run: 3, ..base }).is_none());
        // Found it yourself: two side by side already.
        assert_eq!(next_hint(&[], &Moment { side_by_side: true, ..base }).map(|h| h.id), Some("peek"));
        let seen = vec!["tabs".to_owned(), "peek".to_owned()];
        assert_eq!(next_hint(&seen, &base).map(|h| h.id), Some("checkup"));
        assert!(next_hint(&["*".to_owned()], &base).is_none());
        let mut s = Settings::default();
        mark_seen(&mut s, "tabs");
        mark_seen(&mut s, "tabs");
        assert_eq!(s.hints_seen, ["tabs"]);
    }
}
