//! What the window does with its settings: every change goes through
//! [`Shell::change_settings`], which hands the new values to whatever honours them, at once.
//!
//! Honoured today: the appearance (follow the system, light, dark), the terminal's text size,
//! line height, cursor, blink, colours and copy on select, streamer mode and secret hiding,
//! highlighting, the sidebar's side, whether tabs name their server, the status bar's vital
//! signs, the shortcuts, and through the backend what only the program can do (reconnecting,
//! the welcome, Tor's typing rhythm). The rest is kept and shown; `settings_store::catalog`
//! says which.

use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rune::Tx;
use sshee_app::model::{CursorStyle, HighlightColor, HighlightRule, Side};
use sshee_app::settings_store::catalog::Section;
use sshee_app::settings_store::schemes::{self, Appearance, Resolved};
use sshee_app::settings_store::{Seed, SettingsStore};
use sshee_term::engine::CursorShape;
use sshee_term::highlight::{self, Highlights};
use sshee_term::privacy::{host_alias, HostNames, Names};

use crate::shell::Shell;
use crate::terminal::palette::Scheme;

/// What the look in force was made from, so it is made again only when that changes: a list
/// of thousands of servers isn't compiled into a matcher on every click.
#[derive(Default)]
pub struct Applied {
    /// The servers the hidden names were made of, hashed; `None` while streamer mode is off.
    names: Option<u64>,
    highlights: Option<(bool, bool, bool, Vec<HighlightRule>)>,
}

/// `user@host:port` as a server's card shows it, in its parts.
fn address_parts(address: &str) -> (Option<&str>, &str) {
    let (user, host) = match address.rsplit_once('@') {
        Some((user, host)) => (Some(user).filter(|u| !u.is_empty()), host),
        None => (None, address),
    };
    // A port after the last colon, unless the colons are an IPv6 address's own.
    let host = match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') && !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    (user, host.trim_matches(['[', ']']))
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
    /// Hands the settings to everything that honours them. Called at the start, after every
    /// change, and when the list of servers changes (streamer mode hides their names).
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
        pane.copy_on_select = self.settings.copy_on_select();
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

        // The names streamer mode hides, made again only when the servers changed.
        let applied = &mut self.screens.settings.applied;
        let names_for = s.streamer_mode.then(|| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for server in self.bench.servers() {
                (&server.name, &server.address).hash(&mut hasher);
            }
            hasher.finish()
        });
        if names_for != applied.names {
            applied.names = names_for;
            let hosts = self.bench.servers().iter().filter(|server| !server.is_local()).map(|server| {
                let (user, hostname) = address_parts(&server.address);
                HostNames { name: &server.name, hostname, user }
            });
            self.look.names = Arc::new(if names_for.is_some() { Names::new(hosts, []) } else { Names::default() });
        }
        let highlights_for = (s.highlight_errors, s.highlight_warnings, s.highlight_addresses, s.highlight_rules.clone());
        if applied.highlights.as_ref() != Some(&highlights_for) {
            let rules: Vec<highlight::HighlightRule> = s
                .highlight_rules
                .iter()
                .map(|r| highlight::HighlightRule { id: r.id.clone(), pattern: r.pattern.clone(), color: term_color(&r.color) })
                .collect();
            self.look.highlights = Arc::new(Highlights::new(s.highlight_errors, s.highlight_warnings, s.highlight_addresses, &rules));
            applied.highlights = Some(highlights_for);
        }
        self.look.streaming = s.streamer_mode;
        self.look.hide_secrets = s.hide_secrets;

        self.options.pane = pane;
        let (options, look) = (self.options.pane.clone(), self.look.clone());
        for pane in self.panes.values_mut() {
            pane.set_options(options.clone());
            pane.set_look(look.clone());
        }
        // The copy is the settings' own, so the backend can't hold a borrow of the window.
        let settings = self.settings.get().clone();
        self.backend().settings_changed(&settings);
    }

    /// Changes settings and applies them at once: what every control in Settings does. The
    /// change draws as the handler's own does (a switch's knob slides); `tx` is here so a
    /// caller needn't change when applying a setting one day asks something of the window.
    pub fn change_settings(&mut self, _tx: &mut Tx, change: impl FnOnce(&mut SettingsStore)) {
        let before = self.settings.revision();
        change(&mut self.settings);
        if self.settings.revision() != before {
            self.apply_settings();
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

    /// Streamer mode is on: names of servers, addresses and users are hidden.
    pub fn streaming(&self) -> bool {
        self.settings.get().streamer_mode
    }

    /// The sidebar is at the window's right.
    pub fn sidebar_at_right(&self) -> bool {
        self.settings.get().sidebar_side == Side::Right
    }

    /// A server's name as the window shows it: its own, or "Server 2" while streaming (its
    /// place among the servers, as 0.10 counted). This computer keeps its name.
    pub fn shown_server<'a>(&self, key: &str, name: &'a str) -> Cow<'a, str> {
        if !self.streaming() {
            return Cow::Borrowed(name);
        }
        let mut servers = self.bench.servers().iter().filter(|s| !s.is_local());
        match self.bench.server(key) {
            Some(server) if server.is_local() => Cow::Borrowed(name),
            _ => Cow::Owned(host_alias(servers.position(|s| s.key() == key))),
        }
    }

    /// A tab's own name as the window shows it: while streaming, with the names of servers
    /// and users and any address in it as dots.
    pub fn shown_title<'a>(&self, title: &'a str) -> Cow<'a, str> {
        if self.streaming() {
            Cow::Owned(self.look.safe_text(title))
        } else {
            Cow::Borrowed(title)
        }
    }

    /// Opens Settings over the page, at the section it was last on. Nothing has the keyboard
    /// until a control is clicked or Tab is pressed: no terminal types behind it.
    pub fn open_settings(&mut self, tx: &mut Tx) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_gives_its_user_and_its_host() {
        assert_eq!(address_parts("deploy@203.0.113.17"), (Some("deploy"), "203.0.113.17"));
        assert_eq!(address_parts("postgres@203.0.113.31:2222"), (Some("postgres"), "203.0.113.31"));
        assert_eq!(address_parts("backups"), (None, "backups"));
        assert_eq!(address_parts("web-7731.example.net:22"), (None, "web-7731.example.net"));
        // An IPv6 address's colons aren't a port.
        assert_eq!(address_parts("ops@2001:db8::17"), (Some("ops"), "2001:db8::17"));
        assert_eq!(address_parts(""), (None, ""));
        assert_eq!(address_parts("@host"), (None, "host"));
    }
}
