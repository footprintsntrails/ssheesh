//! Settings: a page inside the window, in the place of the tab on screen, opened by the gear
//! and by its shortcut (⌘, on a Mac, Ctrl+, elsewhere). A quiet list of sections at the left
//! with a search field over it, the section's rows at the right, as in a Mac settings window.
//! The rows keep to a column about as wide as a Mac's settings window, beside the list, so a
//! control is never far from its name or from the list.
//!
//! ```text
//! settings   row, the page's colour
//! ├── settings-nav    232 pt: [search] then the sections, in three bunches
//! └── settings-main   column
//!     ├── settings-head   the section's name … Reset, Close
//!     └── settings-<id>   the rows under their headings, scrolling; while a search is on,
//!                         settings-results: what it found, live, by section
//! ```
//!
//! What a row is (its name, its help, its control, whether 1.0 acts on it yet) is data:
//! `sshee_app::settings_store::catalog`. This module and `rows` draw that list; `shortcuts`
//! draws the Keyboard section; `apply` is what the window does with the values. A change
//! takes effect at once and is saved by the store on its own thread.

pub mod apply;
mod looks;
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
use crate::kit::form::form_group;
use crate::kit::popup::popup_menu;
use crate::kit::push::{icon_button, push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The list of sections' width.
const NAV_WIDTH: f32 = 232.0;
/// The widest the rows get: past this a row's control drifts too far from its name. About
/// what a Mac settings window gives its rows, so a switch is a short reach from its words.
const CONTENT_WIDTH: f32 = 560.0;
/// The rows' column's padding at each side.
const CONTENT_PAD: f32 = 28.0;
/// The sections in the list, in the bunches it shows them: what you look at and touch, what
/// you share and where it goes, this copy of SSHeesh. A little space between bunches.
const NAV_GROUPS: [&[Section]; 3] = [
    &[Section::Layout, Section::Appearance, Section::Terminal, Section::Keyboard],
    &[Section::Privacy, Section::Files, Section::Agents, Section::Tor],
    &[Section::Updates, Section::About],
];
/// The space between two bunches of sections.
const NAV_GROUP_GAP: f32 = 12.0;

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
    /// Appearance's themes: the one being edited, importing one, the colour being picked.
    pub(crate) looks: looks::Looks,
    /// The winking logo's clock (`crate::looks`).
    pub(crate) wink: crate::looks::Wink,
    /// Whether the system was dark when Settings last drew (which theme is on screen while
    /// following it).
    pub(crate) system_dark: std::cell::Cell<bool>,
    /// The rows' column as last laid out, its padding included; 0 before the first layout.
    rows_width: f32,
    /// Sections visited before this one, and those left by going back: the mouse's side
    /// buttons walk them, as a browser's history.
    back: Vec<Section>,
    ahead: Vec<Section>,
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
            looks: looks::Looks::default(),
            wink: crate::looks::Wink::default(),
            system_dark: std::cell::Cell::new(true),
            rows_width: 0.0,
            back: Vec::new(),
            ahead: Vec::new(),
        }
    }
}

impl State {
    /// How wide a box of rows is: the column less its padding, or the most it can be before
    /// it has been laid out.
    pub(crate) fn row_width(&self) -> f32 {
        if self.rows_width > CONTENT_PAD * 2.0 {
            self.rows_width - CONTENT_PAD * 2.0
        } else {
            CONTENT_WIDTH
        }
    }

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

    /// Shows `section` from its top, the one it leaves kept for going back.
    pub(crate) fn go_to(&mut self, section: Section) {
        if section != self.section {
            self.back.push(self.section);
            self.ahead.clear();
        }
        self.show(section);
    }

    /// The section visited before this one (the mouse's back button). False: none.
    pub(crate) fn go_back(&mut self) -> bool {
        let Some(section) = self.back.pop() else { return false };
        self.ahead.push(self.section);
        self.show(section);
        true
    }

    /// The section gone back from (the mouse's forward button). False: none.
    pub(crate) fn go_forward(&mut self) -> bool {
        let Some(section) = self.ahead.pop() else { return false };
        self.back.push(self.section);
        self.show(section);
        true
    }

    pub(crate) fn show(&mut self, section: Section) {
        self.section = section;
        self.menu = None;
        self.found = None;
        self.keys = shortcuts::Recording::default();
        self.looks = looks::Looks::default();
    }

    /// Settings goes off screen: what was half done in it is dropped.
    pub(crate) fn leave(&mut self) {
        self.open = false;
        self.back.clear();
        self.ahead.clear();
        self.menu = None;
        self.found = None;
        self.keys = shortcuts::Recording::default();
        self.search.clear();
        self.looks = looks::Looks::default();
    }

    /// Something is typed in the search field: the page shows what it finds.
    fn searching(&self) -> bool {
        !self.search.text.trim().is_empty()
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
    // Esc closes the colour picker first, then the theme editor, then Settings (0.10's).
    if ev.key == Key::Escape {
        if s.screens.settings.looks.picking.take().is_some() {
            tx.instant();
        } else if s.screens.settings.looks.editing.is_some() {
            looks::close_editor(s, tx);
        } else {
            s.close_settings(tx);
        }
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

/// One section in the list: its icon and name, in a quiet fill when it is the one on screen.
/// `quiet`: a search is on and found nothing in it.
fn nav_row(t: &Theme, section: Section, on: bool, quiet: bool) -> El {
    // The section you're in is lit as the sidebar's and the menus' picked rows are: a quiet
    // fill of the theme's own, not a fixed blue that ignores it and outshouts the page.
    let ink = if quiet { t.l2 } else { t.l1 };
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
        .child(icon(section_icon(section), theme::ICON, if on { t.l1 } else { t.l2on.alpha(if quiet { 0.6 } else { 1.0 }) }))
        .child(text(section.label()).font(theme::TEXT).color(ink).single_line().truncate().grow(1.0).shrink().a11y_hidden());
    e = if on { e.fill(t.selrail) } else { e.hover(t.hov2).press(t.down) };
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
            .on_input_key(|s: &mut Shell, ev, tx| {
                // Esc empties the field first; a second one leaves Settings (the field has the
                // keyboard as Settings opens).
                if ev.key != Key::Escape {
                    return false;
                }
                if s.screens.settings.search.text.is_empty() {
                    s.close_settings(tx);
                } else {
                    s.screens.settings.search.clear();
                }
                true
            }),
    )
    .w_full();
    // While a search is on, its results fill the page (`results`); the list stays, the
    // sections with nothing found quieter and none of them picked.
    let searching = state.searching();
    let hits: Vec<Section> = if searching { state.results(Platform::current()).into_iter().map(|f| f.section).collect() } else { Vec::new() };
    let mut sections = col().key("nav-sections").w_full().gap(NAV_GROUP_GAP).role("tablist").focus_group().label_static("Settings");
    for bunch in NAV_GROUPS {
        let mut rows = col().w_full().gap(2.0);
        for &section in bunch {
            rows = rows.child(nav_row(t, section, !searching && section == state.section, searching && !hits.contains(&section)));
        }
        sections = sections.child(rows);
    }
    col().key("settings-nav").w(NAV_WIDTH).h_full().padding(10.0).border_sides([0.0, 1.0, 0.0, 0.0], t.hair2).child(search.mb(10.0)).child(sections)
}

/// What a search found, in place of a section's rows: each section's matching rows under the
/// section's name, as live as in their section, and the way to the section itself.
fn results(s: &Shell, t: &'static Theme) -> Vec<El> {
    let state = &s.screens.settings;
    let found = state.results(Platform::current());
    let query = state.search.text.trim();
    if found.is_empty() {
        let none = format!("No setting matches “{query}”.");
        return vec![col()
            .key("settings-none")
            .w_full()
            .pt(48.0)
            .gap(6.0)
            .items_center()
            .child(icon(Icon::Search, 28.0, t.l2))
            .child(text(none).font(theme::TEXT).color(t.l1).single_line().truncate())
            .child(text("Try another word, like “font”, “dark” or “copy”.").font(theme::NOTE).color(t.l2).single_line())];
    }
    let on = keyboard(s);
    let mut out = Vec::new();
    for (n, section) in Section::ALL.into_iter().filter(|section| found.iter().any(|f| f.section == *section)).enumerate() {
        let hits: Vec<&Found> = found.iter().filter(|f| f.section == section).collect();
        let first = hits[0].clone();
        // The section's name: a click goes there, to the first row found in it.
        let title = row()
            .key(format!("found-{n}"))
            .h(26.0)
            .px(8.0)
            .ml(4.0)
            .gap(7.0)
            .radius(theme::RADIUS_ROW)
            .items_center()
            .role("link")
            .label(format!("Show in {}", section.label()))
            .focusable(true)
            .hover(t.hov2)
            .press(t.down)
            .on_press_tx(move |s: &mut Shell, tx| jump(s, &first, tx))
            .child(icon(section_icon(section), 14.0, t.l2on))
            .child(text(section.label()).font(theme::GROUP_TITLE).color(t.l2on).single_line().a11y_hidden())
            .child(icon(Icon::ChevronRight, 12.0, t.l2).a11y_hidden());
        let mut block = col().key(format!("found-in-{}", section.id())).w_full().gap(6.0).child(row().w_full().child(title));
        if section == Section::Keyboard {
            let ids: Vec<&'static str> =
                hits.iter().filter_map(|f| sshee_app::settings_store::keymap::GROUPS.iter().flat_map(|(_, ids)| ids.iter().copied()).find(|id| f.key == format!("shortcut-{id}"))).collect();
            block = block.child(form_group(t, ids.into_iter().map(|id| shortcuts::shortcut_row(s, t, id, on)).collect()));
        } else {
            let rows: Vec<&'static catalog::Row> = hits.iter().filter_map(|f| catalog::row(&f.key)).filter(|r| r.shows(&s.settings, Platform::current())).collect();
            // A heading inside a section stays out: the section's name says where these are.
            block = block.children(rows::draw_flat(s, t, rows));
        }
        out.push(block);
    }
    out
}

/// The section's name, and at its right what applies to the whole section.
fn head(s: &Shell, t: &'static Theme) -> El {
    let state = &s.screens.settings;
    let section = state.section;
    let title = if state.searching() { format!("Results for “{}”", state.search.text.trim()) } else { section.label().to_owned() };
    // As wide as the rows under it, so Reset and Close end where they do.
    let mut head = row()
        .key("settings-head")
        .w_full()
        .max_w(CONTENT_WIDTH + CONTENT_PAD * 2.0)
        .h(52.0)
        .px(CONTENT_PAD)
        .gap(8.0)
        .items_center()
        .child(text(title).key("settings-title").font(theme::PAGE_TITLE).color(t.l1).single_line().truncate().shrink().min_w(0.0).role("heading").level(1))
        .child(rune::spacer());
    if section.has_reset() && !state.searching() {
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
pub fn view(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    let state = &s.screens.settings;
    state.system_dark.set(cx.env().system_dark);
    let (body, scroll_key) = match state.section {
        _ if state.searching() => (results(s, t), "settings-results".to_owned()),
        Section::Keyboard => (shortcuts::view(s, t), "settings-keyboard".to_owned()),
        Section::Appearance if state.looks.editing.is_some() => (looks::editor(s, t), "settings-appearance".to_owned()),
        section => (rows::section(s, t, section), format!("settings-{}", section.id())),
    };
    // Each section under its own key: a section switched to starts at its top.
    let scroll = col()
        .key(scroll_key)
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_margin(24.0)
        .child(
            col()
                .key("settings-rows")
                .w_full()
                .max_w(CONTENT_WIDTH + CONTENT_PAD * 2.0)
                .px(CONTENT_PAD)
                .pt(2.0)
                .pb(CONTENT_PAD)
                .gap(18.0)
                .children(body)
                .on_layout(|s: &mut Shell, rect: [f32; 4]| {
                    s.screens.settings.rows_width = rect[2];
                }),
        );
    let page = row()
        .key("settings")
        .w_full()
        .h_full()
        .role("group")
        .label_static("Settings")
        // The mouse's side buttons go back and forward through the sections visited, as in a
        // browser (Files does the same with folders).
        .pointer_buttons([rune::PointerButton::Back, rune::PointerButton::Forward])
        .on_background_press_tx(|s: &mut Shell, tx| {
            let state = &mut s.screens.settings;
            let moved = match tx.button() {
                rune::PointerButton::Back => state.go_back(),
                rune::PointerButton::Forward => state.go_forward(),
                _ => false,
            };
            if moved {
                tx.instant();
            }
        })
        .child(nav(s, t))
        .child(col().key("settings-main").grow(1.0).min_w(0.0).h_full().child(head(s, t)).child(scroll));
    crate::kit::motion::enters(page, crate::kit::motion::Entrance::Page)
}

/// What Settings draws over the whole window: the open pop-up menu, under its button.
pub fn overlay(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.settings;
    let key = state.menu.filter(|_| state.open)?;
    if let Some(menu) = looks::menu(s, t, key) {
        return Some(menu);
    }
    let row = catalog::row(key)?;
    if let Control::Special(catalog::Special::ExternalEditor) = row.control {
        return Some(editor_menu(s, t, key));
    }
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

/// "Open files in": every editor found, then "Choose an app…", the one in use checked.
fn editor_menu(s: &Shell, t: &'static Theme, key: &'static str) -> El {
    let found: Vec<(String, String)> = s.screens.edits.found.iter().map(|e| (e.id.clone(), sshee_app::files::edit::listed(&e.name))).collect();
    let custom = s.screens.edits.custom.as_ref().map(|e| (e.id.clone(), format!("{} (chosen)", e.name)));
    let all: Vec<(String, String)> = found.into_iter().chain(custom).collect();
    let chosen = crate::screens::files::edit::chosen(s).map(|e| e.id);
    let selected = all.iter().position(|(id, _)| Some(id) == chosen.as_ref());
    let mut labels: Vec<&str> = all.iter().map(|(_, name)| name.as_str()).collect();
    labels.push("Choose an app…");
    let ids: Vec<String> = all.iter().map(|(id, _)| id.clone()).collect();
    popup_menu(
        t,
        &rows::control_key(key),
        &labels,
        selected,
        move |s: &mut Shell, i, tx| {
            s.screens.settings.menu = None;
            match ids.get(i).cloned() {
                Some(id) => s.change_settings(tx, move |st| st.set_more(move |m| m.external_editor = Some(id))),
                None => crate::screens::files::edit::choose_app(None, tx),
            }
            tx.instant();
        },
        |s: &mut Shell, tx| {
            s.screens.settings.menu = None;
            tx.instant();
        },
    )
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
