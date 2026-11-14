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
use sshee_app::model::{CursorStyle, HighlightColor, HighlightRule, Side};
use sshee_app::settings_store::catalog::Section;
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

impl Shell {
    /// Hands the settings to everything that honours them. Called at the start and after every
    /// change. What terminals show (streamer mode, secrets, highlighting) reaches them with the
    /// next [`Shell::push_look`]: at once from [`Shell::change_settings`], else at the next poll.
    pub fn apply_settings(&mut self) {
        let s = self.settings.get();
        self.options.shortcuts = s.shortcuts.clone();
        let mut pane = self.options.pane.clone();
        pane.font_size = (s.terminal_font_size as f32).clamp(6.0, 40.0);
        pane.line_height = (s.terminal_line_height as f32).clamp(1.0, 2.0);
        pane.cursor_shape = match s.cursor_style {
            CursorStyle::Block => CursorShape::Block,
            CursorStyle::Underline => CursorShape::Underline,
            _ => CursorShape::Bar,
        };
        pane.blink = s.cursor_blink;
        pane.scheme = match schemes::resolve(&s.terminal_scheme, s.terminal_colors.as_ref()) {
            Resolved::MatchApp => Scheme::Fresh,
            Resolved::Fixed(scheme) => Scheme::Fixed(scheme.colors),
            Resolved::Own { background, foreground } => Scheme::Own { background, foreground },
        };
        pane.dark = match Appearance::of(s) {
            Appearance::System => None,
            Appearance::Light => Some(false),
            Appearance::Dark => Some(true),
        };
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
        // Settings → Terminal → Copying, for every terminal open now and opened later.
        let (copy_on_select, program_clipboard) = (self.settings.copy_on_select(), self.settings.program_clipboard());
        self.options.pane = pane;
        self.set_copy_modes(copy_on_select, program_clipboard);
        let options = self.options.pane.clone();
        for pane in self.panes.values_mut() {
            pane.set_options(options.clone());
        }
        self.follow_streamer(streamer);
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

    /// The sidebar is at the window's right.
    pub fn sidebar_at_right(&self) -> bool {
        self.settings.get().sidebar_side == Side::Right
    }

    /// Opens Settings over the page, at the section it was last on. Nothing has the keyboard
    /// until a control is clicked or Tab is pressed: no terminal types behind it.
    pub fn open_settings(&mut self, tx: &mut Tx) {
        // The palette, a menu or a dialog it was opened from goes.
        self.screens.close_all();
        let state = &mut self.screens.settings;
        state.open = true;
        state.sync(&self.settings);
        self.settle_in_place(tx);
        tx.blur();
        tx.instant();
    }

    /// Opens Settings at `section` (a search result elsewhere, "Turn it off in Updates").
    pub fn open_settings_at(&mut self, section: Section, tx: &mut Tx) {
        self.screens.settings.go_to(section);
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
