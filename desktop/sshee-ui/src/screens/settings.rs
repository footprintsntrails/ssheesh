//! Settings: a page inside the window, in the place of the tab on screen, opened by the gear
//! and by its shortcut (⌘, on a Mac, Ctrl+, elsewhere). A quiet list of sections at the left
//! with a search field over it, the section's rows at the right, as in a Mac settings window.
//!
//! ```text
//! settings   row, the page's colour
//! ├── settings-nav    232 pt: [search] then the sections, or what the search found
//! └── settings-main   column
//!     ├── settings-head   the section's name … Reset, Done
//!     └── settings-<id>   the rows, scrolling
//! ```
//!
//! What a row is (its name, its help, its control, whether 1.0 acts on it yet) is data:
//! `sshee_app::settings_store::catalog`. This module and `rows` draw that list; `shortcuts`
//! draws the Keyboard section; `apply` is what the window does with the values. A change
//! takes effect at once and is saved by the store on its own thread.

pub mod apply;
mod rows;
mod shortcuts;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use rune::{col, row, text, Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx};
use sshee_app::model::Platform;
use sshee_app::settings_store::catalog::{self, Control, Found, Section};
use sshee_app::settings_store::keymap::Keyboard;
use sshee_app::settings_store::SettingsStore;

use crate::icons::{icon, Icon};
use crate::kit::field::{input, search_field};
use crate::kit::popup::popup_menu;
use crate::kit::push::{icon_button, push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The list of sections' width.
const NAV_WIDTH: f32 = 232.0;
/// The widest the rows get: past this a row's control drifts too far from its name.
const CONTENT_WIDTH: f32 = 640.0;

/// What Settings asks of Tor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TorCommand {
    On,
    Off,
    /// Other routes through the network, when typing feels slow.
    NewRoutes,
    /// Forget what Tor keeps about the network and start over.
    StartAfresh,
}

/// Settings' own state: where it is, and what is being typed or recorded in it. The values
/// themselves are the store's (`Shell::settings`).
pub struct State {
    /// Shown, over the page of the tab on screen.
    pub open: bool,
    pub section: Section,
    /// The search field.
    pub search: TextEdit,
    /// The pop-up menu that is open, by its row's key.
    pub menu: Option<&'static str>,
    /// Text rows while they are typed in, by row key.
    pub texts: HashMap<&'static str, TextEdit>,
    /// A new word to highlight, and the colour picked for it (its place among the six).
    pub word: TextEdit,
    pub word_color: usize,
    /// The Keyboard section: the shortcut being recorded.
    pub keys: shortcuts::Recording,
    /// The row a search led to, pointed out until Settings goes elsewhere.
    pub found: Option<String>,
    pub(crate) applied: apply::Applied,
}

impl Default for State {
    fn default() -> Self {
        State {
            open: false,
            section: Section::Layout,
            search: TextEdit::default(),
            menu: None,
            texts: HashMap::new(),
            word: TextEdit::default(),
            // Magenta, as 0.10 starts a new rule.
            word_color: 4,
            keys: shortcuts::Recording::default(),
            found: None,
            applied: apply::Applied::default(),
        }
    }
}

impl State {
    /// Takes the store's text values into the fields that show them (when Settings opens).
    pub(crate) fn sync(&mut self, store: &SettingsStore) {
        for row in catalog::rows() {
            if let Control::Text { get, .. } = row.control {
                let now = get(store);
                let field = self.texts.entry(row.key).or_default();
                if field.text != now {
                    field.replace_text(now);
                }
            }
        }
    }

    /// Shows `section` from its top.
    pub(crate) fn go_to(&mut self, section: Section) {
        self.section = section;
        self.menu = None;
        self.found = None;
        self.keys = shortcuts::Recording::default();
    }

    /// Settings goes off screen: what was half done in it is dropped.
    pub(crate) fn leave(&mut self) {
        self.open = false;
        self.menu = None;
        self.found = None;
        self.keys = shortcuts::Recording::default();
        self.search.clear();
    }

    /// What the search field found.
    fn results(&self, platform: Platform) -> Vec<Found> {
        catalog::search(&self.search.text, platform)
    }
}

/// The system whose key names Settings shows.
pub(crate) fn keyboard(s: &Shell) -> Keyboard {
    if s.options.mac_keys {
        Keyboard::Mac
    } else if cfg!(windows) {
        Keyboard::Windows
    } else {
        Keyboard::Other
    }
}

/// A key the window's shortcuts didn't take, while Settings is open and nothing in it has
/// the keyboard. Esc goes back to the tab; everything else stops here, so no key reaches a
/// terminal nobody sees. False when Settings isn't open.
pub fn key(s: &mut Shell, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if !s.screens.settings.open {
        return false;
    }
    if ev.key == Key::Escape {
        s.close_settings(tx);
    }
    true
}

/// A search result was picked: its section, scrolled to its row.
fn jump(s: &mut Shell, found: &Found, tx: &mut Tx) {
    let state = &mut s.screens.settings;
    state.go_to(found.section);
    state.search.clear();
    state.found = Some(found.key.clone());
    tx.scroll_into_view(found.key.clone());
}

fn section_icon(section: Section) -> Icon {
    match section {
        Section::Layout => Icon::Sidebar,
        Section::Appearance => Icon::Contrast,
        Section::Terminal => Icon::Terminal,
        Section::Keyboard => Icon::Keyboard,
        Section::Privacy => Icon::Lock,
        Section::Files => Icon::Folder,
        Section::Agents => Icon::Agent,
        Section::Tor => Icon::Tor,
        Section::Updates => Icon::Refresh,
        Section::About => Icon::Info,
    }
}

/// One section in the list: its icon and name, filled blue when it is the one on screen.
fn nav_row(t: &Theme, section: Section, on: bool) -> El {
    let ink = if on { t.on_color } else { t.l1 };
    let mut e = row()
        .key(format!("nav-{}", section.id()))
        .h(theme::CONTROL)
        .w_full()
        .px(8.0)
        .gap(8.0)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .role("tab")
        .checked(on)
        .label(section.label())
        .focusable(true)
        .focus_selected(on)
        .on_press_tx(move |s: &mut Shell, tx| {
            s.screens.settings.go_to(section);
            tx.instant();
        })
        .child(icon(section_icon(section), theme::ICON, if on { t.on_color } else { t.l2 }))
        .child(text(section.label()).font(theme::TEXT).color(ink).single_line().truncate().grow(1.0).shrink().a11y_hidden());
    e = if on { e.fill(t.blue) } else { e.hover(t.hov2).press(t.down) };
    e
}

/// The list at the left: the search field, then the sections or what the search found.
fn nav(s: &Shell, t: &'static Theme) -> El {
    let state = &s.screens.settings;
    let search = search_field(
        t,
        input(t, &state.search, "Search")
            .key("settings-search")
            .on_edit(|s: &mut Shell| &mut s.screens.settings.search)
            .on_submit(|s: &mut Shell, tx| {
                // Return goes to the first thing found.
                if let Some(first) = s.screens.settings.results(Platform::current()).into_iter().next() {
                    jump(s, &first, tx);
                }
            })
            .on_input_key(|s: &mut Shell, ev, _| {
                // Esc empties the field first; a second one leaves Settings.
                let typed = !s.screens.settings.search.text.is_empty();
                if ev.key == Key::Escape && typed {
                    s.screens.settings.search.clear();
                }
                ev.key == Key::Escape && typed
            }),
    )
    .w_full();
    let mut list = col().key("settings-nav").w(NAV_WIDTH).h_full().padding(10.0).gap(2.0).border_sides([0.0, 1.0, 0.0, 0.0], t.hair2).child(search.mb(8.0));
    if state.search.text.trim().is_empty() {
        let mut sections = col().key("nav-sections").w_full().gap(2.0).role("tablist").focus_group().label_static("Settings");
        for section in Section::ALL {
            sections = sections.child(nav_row(t, section, section == state.section));
        }
        return list.child(sections);
    }
    let results = state.results(Platform::current());
    if results.is_empty() {
        let none = format!("No setting matches “{}”.", state.search.text.trim());
        return list.child(text(none).key("nav-none").font(theme::NOTE).color(t.l2).wrap().px(8.0).py(6.0));
    }
    let mut found = col().key("nav-results").w_full().gap(2.0).grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false });
    for (i, hit) in results.into_iter().enumerate() {
        let (label, section) = (hit.label, hit.section);
        found = found.child(
            col()
                .key(format!("found-{i}"))
                .w_full()
                .px(8.0)
                .py(5.0)
                .gap(1.0)
                .radius(theme::RADIUS_ROW)
                .role("button")
                .label(format!("{label}, {}", section.label()))
                .focusable(true)
                .hover(t.hov2)
                .press(t.down)
                .on_press_tx(move |s: &mut Shell, tx| jump(s, &hit, tx))
                .child(text(label).font(theme::TEXT).color(t.l1).wrap().a11y_hidden())
                .child(text(section.label()).font(theme::SMALL).color(t.l2).single_line().a11y_hidden()),
        );
    }
    list = list.child(found);
    list
}

/// The section's name, and at its right what applies to the whole section.
fn head(s: &Shell, t: &'static Theme) -> El {
    let section = s.screens.settings.section;
    let mut head = row()
        .key("settings-head")
        .w_full()
        .h(52.0)
        .px(28.0)
        .gap(8.0)
        .items_center()
        .child(text(section.label()).key("settings-title").font(theme::PAGE_TITLE).color(t.l1).single_line().role("heading").level(1))
        .child(rune::spacer());
    if section.has_reset() {
        head = head.child(push_button(t, "Reset", Push::Quiet, true).key("settings-reset").help("Put this section back as it comes").on_press_tx(move |s: &mut Shell, tx| {
            s.screens.settings.keys = shortcuts::Recording::default();
            let keys = catalog::reset_keys(section);
            s.change_settings(tx, |st| {
                st.reset(&keys);
            });
            s.screens.settings.sync(&s.settings);
            // The sidebar is the window's to show: it follows what Reset put back.
            if s.settings.get().sidebar_visible != s.bench.sidebar_shown() {
                s.toggle_sidebar(tx);
            }
        }));
    }
    head.child(icon_button(t, Icon::Close, "Close settings", true).key("settings-close").on_press_tx(|s: &mut Shell, tx| s.close_settings(tx)))
}

/// The page.
pub fn view(s: &Shell, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    let state = &s.screens.settings;
    let body = match state.section {
        Section::Keyboard => shortcuts::view(s, t),
        section => rows::section(s, t, section),
    };
    // Each section under its own key: a section switched to starts at its top.
    let scroll = col()
        .key(format!("settings-{}", state.section.id()))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_margin(24.0)
        .child(col().key("settings-rows").w_full().max_w(CONTENT_WIDTH + 56.0).px(28.0).pb(28.0).gap(10.0).children(body));
    row()
        .key("settings")
        .w_full()
        .h_full()
        .role("group")
        .label_static("Settings")
        .child(nav(s, t))
        .child(col().key("settings-main").grow(1.0).min_w(0.0).h_full().child(head(s, t)).child(scroll))
}

/// What Settings draws over the whole window: the open pop-up menu, under its button.
pub fn overlay(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.settings;
    let key = state.menu.filter(|_| state.open)?;
    let row = catalog::row(key)?;
    let Control::Choice { options, get, set, .. } = row.control else { return None };
    let labels = row.choice_labels(keyboard(s));
    let now = get(&s.settings);
    let selected = options.iter().position(|(value, _)| *value == now);
    Some(popup_menu(
        t,
        &rows::control_key(key),
        &labels,
        selected,
        move |s: &mut Shell, i, tx| {
            s.screens.settings.menu = None;
            s.change_settings(tx, |st| set(st, options[i].0));
            tx.instant();
        },
        |s: &mut Shell, tx| {
            s.screens.settings.menu = None;
            tx.instant();
        },
    ))
}

/// The line across the top of the page while privacy settings are on as a precaution: what
/// was turned on and why, a way to look, and "OK" to stop being told. `None` when there is
/// nothing to say.
pub fn notice_bar(s: &Shell, t: &'static Theme) -> Option<El> {
    let notice = s.settings.notice()?;
    Some(
        row()
            .key("privacy-notice")
            .w_full()
            .min_h(36.0)
            .px(14.0)
            .py(7.0)
            .gap(10.0)
            .items_center()
            .fill(t.group)
            .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
            .role("alert")
            .child(icon(Icon::Lock, 12.0, t.l2on))
            .child(text(notice.headline()).key("privacy-notice-text").font(theme::NOTE).color(t.l1).wrap().grow(1.0).shrink())
            .child(push_button(t, "Review", Push::Quiet, true).key("privacy-notice-review").on_press_tx(|s: &mut Shell, tx| s.open_settings_at(Section::Privacy, tx)))
            .child(push_button(t, "OK", Push::Quiet, true).key("privacy-notice-ok").on_press_tx(|s: &mut Shell, tx| {
                s.settings.dismiss_notice();
                tx.instant();
            })),
    )
}
