//! What the window does with its settings: every change goes through
//! [`Shell::change_settings`], which hands the new values to whatever honours them, at once.
//!
//! Honoured today: the appearance (follow the system, light, dark), the terminal's text size,
//! line height, cursor, blink, colours and copy on select, streamer mode and secret hiding,
//! highlighting, the sidebar's side, whether tabs name their server, the status bar's vital
//! signs, the shortcuts, and through the backend what only the program can do (reconnecting,
//! the welcome, Tor's typing rhythm). The rest is kept and shown; `settings_store::catalog`
//! says which.

use std::sync::Arc;

use rune::Tx;
use sshee_app::model::{CursorStyle, EnvLabel, HighlightColor, HighlightRule, Side};
use sshee_app::settings_store::catalog::Section;
use sshee_app::settings_store::looks;
use sshee_app::settings_store::schemes::{self, Appearance, Resolved};
use sshee_app::settings_store::{Seed, SettingsStore};
use sshee_term::engine::CursorShape;
use sshee_term::highlight::{self, Highlights};

use crate::shell::Shell;
use crate::terminal::palette::Scheme;

/// What the highlighting in force was made from, so it is made again only when that changes:
/// rules are compiled into matchers.
#[derive(Default)]
pub struct Applied {
    highlights: Option<(bool, bool, bool, Vec<HighlightRule>)>,
}

fn term_color(color: &HighlightColor) -> highlight::HighlightColor {
    match color {
        HighlightColor::Yellow => highlight::HighlightColor::Yellow,
        HighlightColor::Green => highlight::HighlightColor::Green,
        HighlightColor::Blue => highlight::HighlightColor::Blue,
        HighlightColor::Magenta => highlight::HighlightColor::Magenta,
        HighlightColor::Cyan => highlight::HighlightColor::Cyan,
        // Red, and a colour a newer version wrote.
        _ => highlight::HighlightColor::Red,
    }
}

/// "Match app": the design's terminal colours, on the panes of the theme for each side when
/// one isn't the design's own (0.10's "auto" scheme took the theme's pane colour).
fn app_scheme(s: &sshee_app::model::Settings) -> Scheme {
    let pane = |dark: bool| {
        let theme = looks::on_screen(s, dark);
        (theme.design.is_some(), looks::over(theme.colors[1], theme.colors[0] | 0xff) >> 8)
    };
    match (pane(true), pane(false)) {
        ((true, _), (true, _)) => Scheme::Fresh,
        ((_, night), (_, day)) => Scheme::App { night, day },
    }
}

/// Interface size as a zoom, within the row's range.
fn ui_zoom(store: &SettingsStore) -> f32 {
    (store.get().ui_zoom as f32 / 100.0).clamp(0.85, 1.3)
}

impl Shell {
    /// Hands the settings to everything that honours them. Called at the start and after every
    /// change. What terminals show (streamer mode, secrets, highlighting) reaches them with the
    /// next [`Shell::push_look`]: at once from [`Shell::change_settings`], else at the next poll.
    pub fn apply_settings(&mut self) {
        let s = self.settings.get();
        self.options.shortcuts = s.shortcuts.clone();
        let mut pane = self.options.pane.clone();
        // Interface size zooms everything but the terminal's text and padding, which keep
        // their own sizes (Terminal → Font size, Padding).
        let zoom = ui_zoom(&self.settings);
        pane.font_size = (s.terminal_font_size as f32).clamp(6.0, 40.0) / zoom;
        pane.padding = (s.terminal_padding as f32).clamp(0.0, 28.0) / zoom;
        pane.family = schemes::terminal_family(&s.terminal_font, self.options.platform);
        pane.line_height = (s.terminal_line_height as f32).clamp(1.0, 2.0);
        pane.cursor_shape = match s.cursor_style {
            CursorStyle::Block => CursorShape::Block,
            CursorStyle::Underline => CursorShape::Underline,
            _ => CursorShape::Bar,
        };
        pane.blink = s.cursor_blink;
        // A terminal's session: letting go of an unused connection, the card of a dropped one,
        // and the typing counter (`terminal/session.rs`, `terminal/echo.rs`).
        pane.idle_sleep = s.idle_sleep;
        pane.reconnect_card = s.reconnect_mode == sshee_app::model::ReconnectMode::Ask;
        pane.count_typing = s.count_typing;
        pane.scheme = match schemes::resolve(&s.terminal_scheme, s.terminal_colors.as_ref()) {
            Resolved::MatchApp => app_scheme(s),
            Resolved::Fixed(scheme) => Scheme::Fixed(scheme.colors),
            Resolved::Own { background, foreground } => Scheme::Own { background, foreground },
        };
        pane.dark = match Appearance::of(s) {
            Appearance::System => None,
            Appearance::Light => Some(false),
            Appearance::Dark => Some(true),
        };
        // What sits over a terminal (find, the menu, the away line) in the theme on screen.
        pane.themes = [crate::looks::theme_for(s, false), crate::looks::theme_for(s, true)];
        let highlights_for = (s.highlight_errors, s.highlight_warnings, s.highlight_addresses, s.highlight_rules.clone());
        if self.screens.settings.applied.highlights.as_ref() != Some(&highlights_for) {
            let rules: Vec<highlight::HighlightRule> = s
                .highlight_rules
                .iter()
                .map(|r| highlight::HighlightRule { id: r.id.clone(), pattern: r.pattern.clone(), color: term_color(&r.color) })
                .collect();
            self.highlights = Arc::new(Highlights::new(s.highlight_errors, s.highlight_warnings, s.highlight_addresses, &rules));
            self.screens.settings.applied.highlights = Some(highlights_for);
        }
        let streamer = s.streamer_mode;
        // Agents and notifications: whether a terminal that needs you says so at all.
        self.screens.agent.notify = s.notify_attention;
        // Settings → Terminal → Copying, for every terminal open now and opened later.
        let (copy_on_select, program_clipboard) = (self.settings.copy_on_select(), self.settings.program_clipboard());
        self.options.pane = pane;
        self.set_copy_modes(copy_on_select, program_clipboard);
        let options = self.options.pane.clone();
        for pane in self.panes.values_mut() {
            pane.set_options(options.clone());
        }
        self.follow_streamer(streamer);
        // "Tabs on screen": side by side, or one at a time.
        let together = self.settings.get().view_mode == sshee_app::model::ViewMode::Split;
        self.bench.set_side_by_side(together);
        // The copy is the settings' own, so the backend can't hold a borrow of the window.
        let settings = self.settings.get().clone();
        self.backend_mut().settings_changed(&settings);
    }

    /// Changes settings and applies them at once: what every control in Settings does.
    pub fn change_settings(&mut self, tx: &mut Tx, change: impl FnOnce(&mut SettingsStore)) {
        let before = self.settings.revision();
        change(&mut self.settings);
        if self.settings.revision() != before {
            self.apply_settings();
            self.push_look(tx);
        }
    }

    /// What the first start begins from (see `SettingsStore::seed`): told by the program once
    /// it has looked for 0.10's settings, from its own thread through `Remote`.
    pub fn seed_settings(&mut self, seed: Seed) {
        if self.settings.seed(seed) {
            // The sidebar as 0.10 left it.
            if self.settings.get().sidebar_visible != self.bench.sidebar_shown() {
                self.bench.toggle_sidebar();
            }
            self.apply_settings();
        }
    }

    /// Whether the window draws dark, on a system that is (`system_dark`) or isn't.
    pub fn dark(&self, system_dark: bool) -> bool {
        Appearance::of(self.settings.get()).dark(system_dark)
    }

    /// Settings → Appearance → Interface size, as a zoom (1 is 100%).
    pub fn ui_zoom(&self) -> f32 {
        ui_zoom(&self.settings)
    }

    /// Settings → Layout → Corner roundness: the page's corner where it meets the sidebar.
    pub fn page_radius(&self) -> f32 {
        (self.settings.get().pane_radius as f32).clamp(0.0, 18.0)
    }

    /// Settings → Appearance → Fade panes you're not using: how opaque the side you're not
    /// on is drawn, side by side (1: not faded).
    pub fn unused_opacity(&self) -> f32 {
        1.0 - (self.settings.get().dim_inactive as f32).clamp(0.0, 50.0) / 100.0
    }

    /// The label of the server on screen, when Settings → Layout → "A line for Production and
    /// Staging" is on and it has one.
    pub fn env_on_screen(&self) -> Option<EnvLabel> {
        if !self.settings.get().env_stripe {
            return None;
        }
        self.env_label_on_screen()
    }

    /// The label (Production, Staging, Development) of the server on screen, if it has one.
    pub fn env_label_on_screen(&self) -> Option<EnvLabel> {
        self.bench.current_key().and_then(|key| self.bench.server(key)).and_then(|server| server.env.clone())
    }

    /// The sidebar is at the window's right.
    pub fn sidebar_at_right(&self) -> bool {
        self.settings.get().sidebar_side == Side::Right
    }

    /// Opens Settings over the page, at the section it was last on, with the keyboard in its
    /// search field: what's typed looks for a setting, and no terminal types behind it.
    pub fn open_settings(&mut self, tx: &mut Tx) {
        // The palette, a menu or a dialog it was opened from goes.
        self.screens.close_all();
        let state = &mut self.screens.settings;
        state.open = true;
        state.sync(&self.settings);
        self.settle_in_place(tx);
        tx.blur();
        tx.focus("settings-search", true);
        tx.instant();
    }

    /// Opens Settings at `section` (a search result elsewhere, "Turn it off in Updates").
    pub fn open_settings_at(&mut self, section: Section, tx: &mut Tx) {
        // Opened afresh, Settings starts its history at this section.
        let state = &mut self.screens.settings;
        if state.open {
            state.go_to(section);
        } else {
            state.show(section);
        }
        self.open_settings(tx);
    }

    /// Back to the tab that was on screen.
    pub fn close_settings(&mut self, tx: &mut Tx) {
        self.screens.settings.leave();
        self.settle(tx);
    }

    /// The gear and its shortcut: Settings, or back from it.
    pub fn toggle_settings(&mut self, tx: &mut Tx) {
        if self.screens.settings.open {
            self.close_settings(tx);
        } else {
            self.open_settings(tx);
        }
    }
}
