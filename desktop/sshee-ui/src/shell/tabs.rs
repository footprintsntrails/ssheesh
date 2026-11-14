//! The row of tabs in the top bar: every machine's tabs in one row, a tab's menu, dragging a
//! tab to another place, and the list of all tabs when the row can't hold them.
//!
//! ```text
//! tabs   row, takes what the bar has left
//! ├── [sep] [group] tab …      a divider between machines; when crowded, each machine named
//! │                            once before its tabs
//! ├── tab-list                 only when some tabs are out of sight: all of them, in a menu;
//! │                            an orange dot when one of those out of sight waits for you
//! └── new-tab                  "+"
//! ```
//!
//! Three rules keep the row still under the pointer, as a browser's is:
//! - a tab under the pointer swaps its icon for the close cross and keeps its width (the kit);
//! - while the pointer is over the row, every tab keeps the width it had when the pointer
//!   came, and the row stays crowded or roomy as it was: a tab renamed by its shell, or one
//!   closed with the cross, doesn't move the next cross away from the pointer;
//! - a tab is dragged by moving it in the row itself (no ghost): it lands where the pointer is.
//!
//! When even crowded tabs don't fit, the row shows a run of them that holds the tab on screen
//! (`layout::in_sight`), and the rest are in the list.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use rune::{Align, Drag, El, Phase, Side, Tx};
use sshee_app::workbench::layout::{self, Layout};
use sshee_app::workbench::titles::{self, Home};
use sshee_app::workbench::{Open, StripTab, TabId, TabKind};

use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::kit::button::toolbar_button;
use crate::kit::menu::{menu, menu_row, menu_separator, RowState};
use crate::kit::popover::popover;
use crate::kit::tab::{group_name, tab, Tab};
use crate::kit::{divider, ringed_dot};
use crate::terminal::overlays::Wish;
use crate::theme::{self, Theme};

/// The narrowest a crowded tab gets before tabs start going out of sight (the design's).
pub const MIN_TAB: f32 = 76.0;

/// How far the pointer travels before a press on a tab is a drag.
const DRAG_START: f32 = 4.0;

/// The tab menu's and the tab list's widths.
const MENU_WIDTH: f32 = 248.0;
const LIST_WIDTH: f32 = 300.0;

/// How the row was when the pointer came over it.
#[derive(Clone, Debug, Default, PartialEq)]
struct Held {
    compact: bool,
    widths: HashMap<TabId, f32>,
}

/// A tab being dragged: where it was, to put it back if the drag is cancelled.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dragging {
    id: TabId,
    from: usize,
    moved: bool,
}

/// The row's state.
#[derive(Default)]
pub struct Row {
    /// Where each tab was last laid out, in window points.
    rects: HashMap<TabId, [f32; 4]>,
    held: Option<Held>,
    drag: Option<Dragging>,
    /// The tab whose menu is open, and where.
    pub(crate) menu: Option<(TabId, [f32; 2])>,
    /// The list of all tabs is open.
    pub(crate) list: bool,
    /// What the view last drew: whether the row was crowded, the tabs in sight, and the first
    /// of them. (The view only reads the window's state; these are its own notes.)
    compact: Cell<bool>,
    shown: RefCell<Vec<TabId>>,
    first: Cell<usize>,
}

impl Row {
    /// Closes the tab menu and the tab list.
    pub(crate) fn close_overlays(&mut self) {
        self.menu = None;
        self.list = false;
    }

    /// Forgets tabs that are gone.
    pub(crate) fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.rects.retain(|id, _| open(*id));
        if self.menu.is_some_and(|(id, _)| !open(id)) {
            self.menu = None;
        }
    }
}

/// What a row of a tab's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabPick {
    Close,
    CloseOthers,
    CloseRight,
    /// Show it beside the tab on screen (or, for the tab on screen, beside its neighbour).
    SideBySide,
    /// Take it out of its pair.
    Apart,
    Duplicate,
}

impl Shell {
    /// The pointer came over the row, or left it.
    pub(crate) fn row_hover(&mut self, over: bool, tx: &mut Tx) {
        if over {
            let widths = self.row.shown.borrow().iter().filter_map(|id| Some((*id, self.row.rects.get(id)?[2]))).collect();
            self.row.held = Some(Held { compact: self.row.compact.get(), widths });
            tx.live_only();
        } else if self.row.drag.is_none() {
            self.row.held = None;
            tx.instant();
        }
    }

    /// A press, a drag and a release on a tab: past a few points it moves the tab to where the
    /// pointer is, among its machine's tabs or another's.
    pub(crate) fn tab_drag(&mut self, id: TabId, d: &Drag, tx: &mut Tx) {
        match d.phase {
            Phase::Down => {
                let from = self.bench.tabs().iter().position(|t| t.id == id).unwrap_or(0);
                self.row.drag = Some(Dragging { id, from, moved: false });
                tx.live_only();
            }
            Phase::Move => {
                let Some(mut drag) = self.row.drag.filter(|g| g.id == id) else { return };
                if !drag.moved && d.dx.abs() < DRAG_START {
                    tx.live_only();
                    return;
                }
                drag.moved = true;
                self.row.drag = Some(drag);
                // The other tabs in sight, by their middles.
                let others: Vec<(TabId, f32)> =
                    self.row.shown.borrow().iter().filter(|t| **t != id).filter_map(|t| self.row.rects.get(t).map(|r| (*t, r[0] + r[2] / 2.0))).collect();
                let centres: Vec<f32> = others.iter().map(|(_, c)| *c).collect();
                let slot = layout::slot(&centres, d.window_point()[0]);
                // Its place in the whole row, counted without the dragged tab.
                let rest: Vec<TabId> = self.bench.tabs().iter().map(|t| t.id).filter(|t| *t != id).collect();
                let to = match others.get(slot) {
                    Some((before, _)) => rest.iter().position(|t| t == before),
                    None => others.last().and_then(|(last, _)| rest.iter().position(|t| t == last)).map(|i| i + 1),
                };
                let now = self.bench.tabs().iter().position(|t| t.id == id);
                match to {
                    Some(to) if Some(to) != now => {
                        self.bench.move_tab(id, to);
                        tx.instant();
                    }
                    _ => tx.live_only(),
                }
            }
            Phase::Up => {
                let Some(drag) = self.row.drag.take().filter(|g| g.id == id) else { return };
                if d.cancelled() && drag.moved {
                    self.bench.move_tab(id, drag.from);
                }
                if drag.moved {
                    // The tab you dragged is the one you're on, as in a browser; and the
                    // row's new order is kept for the next run.
                    self.focus_tab(id, tx);
                }
            }
        }
    }

    pub(crate) fn open_tab_menu(&mut self, id: TabId, at: [f32; 2], tx: &mut Tx) {
        self.row.list = false;
        self.row.menu = Some((id, at));
        tx.instant();
    }

    pub(crate) fn toggle_tab_list(&mut self, tx: &mut Tx) {
        self.row.menu = None;
        self.row.list = !self.row.list;
        tx.instant();
    }

    pub(crate) fn close_tab_overlays(&mut self, tx: &mut Tx) {
        self.row.close_overlays();
        self.settle(tx);
    }

    /// A row of a tab's menu.
    pub fn tab_pick(&mut self, id: TabId, pick: TabPick, tx: &mut Tx) {
        self.row.close_overlays();
        match pick {
            TabPick::Close => self.close_tab(id, tx),
            TabPick::CloseOthers => self.close_tabs(layout::others(&self.bench, id), tx),
            TabPick::CloseRight => self.close_tabs(layout::to_the_right(&self.bench, id), tx),
            TabPick::SideBySide => self.show_beside(id, tx),
            TabPick::Apart => self.leave_pair(id, tx),
            TabPick::Duplicate => self.duplicate_tab(id, tx),
        }
    }

    /// Closes several tabs at once (their sessions end as one closed by hand does).
    pub fn close_tabs(&mut self, ids: Vec<TabId>, tx: &mut Tx) {
        for id in ids {
            self.bench.close(id);
        }
        self.settle(tx);
    }

    /// Another tab like this one, on the same machine, right after it: a new terminal, or the
    /// same folder in Files.
    pub fn duplicate_tab(&mut self, id: TabId, tx: &mut Tx) {
        let Some(like) = self.bench.tab(id).cloned() else { return };
        // A new tab opens next to the tab you were last on at its machine.
        self.bench.focus(id);
        self.bench.open_with(like.kind, &like.server, Open { path: like.path, ..Open::default() });
        self.settle(tx);
    }
}

impl Shell {
    /// What panes asked of the window since the last frame (`terminal::overlays::Wish`). True
    /// when something drawn changed.
    pub(crate) fn grant(&mut self, wishes: Vec<(TabId, Wish)>, tx: &mut Tx) -> bool {
        let mut changed = false;
        for (id, wish) in wishes {
            match wish {
                Wish::NewTerminal => {
                    let Some(server) = self.bench.tab(id).map(|t| t.server.clone()) else { continue };
                    self.bench.focus(id);
                    self.bench.open(TabKind::Terminal, &server);
                    self.settle(tx);
                    changed = true;
                }
                Wish::OpenPath { path, line } => self.open_path(id, &path, line, tx),
                Wish::Folder(folder) => changed |= self.set_folder(id, &folder),
                // Side by side: the side you press is the tab you're on.
                Wish::Pressed => {
                    if self.bench.active_id() != Some(id) && self.tabs_on_screen().contains(&id) {
                        self.bench.focus(id);
                        self.settle(tx);
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    /// A click on a file's path in tab `from`'s terminal: `path` is absolute or starts at `~`,
    /// `line` is where in the file. The Files screen will take this (show the folder, pick the
    /// file out, as 0.10's `openTerminalLink`); until it does, the path is copied and the
    /// terminal says so.
    pub fn open_path(&mut self, from: TabId, path: &str, line: Option<u32>, tx: &mut Tx) {
        let _ = line;
        tx.copy(path.to_owned());
        if let Some(pane) = self.panes.get_mut(&from) {
            pane.say("Copied the path", tx);
        }
    }

    /// A terminal's shell is in `folder` now: its tab is named after it, unless something else
    /// named the tab (an agent's task). True when the name changed. The agents' watch can call
    /// this too, with the folder tmux reports for a shell on a server.
    pub fn set_folder(&mut self, id: TabId, folder: &str) -> bool {
        let Some(tab) = self.bench.tab(id) else { return false };
        if !titles::is_folder_title(tab.title.as_deref()) {
            return false;
        }
        let server = self.bench.server(&tab.server);
        let own_home = std::env::var("HOME").ok();
        let home = match server {
            Some(s) if s.is_local() => Home { path: own_home.as_deref(), user: None },
            Some(s) => Home::of_address(&s.address),
            None => Home::default(),
        };
        let Some(title) = titles::folder_title(folder, home) else { return false };
        if tab.title.as_deref() == Some(title.as_str()) {
            return false;
        }
        self.bench.set_title(id, Some(title));
        self.save_tabs();
        true
    }
}

/// What a tab's icon is: an agent's spark when one runs in it, else its kind's.
pub(crate) fn tab_icon(tab: &sshee_app::workbench::Tab) -> Icon {
    match tab.kind {
        TabKind::Terminal if tab.agent.is_some() => Icon::Agent,
        TabKind::Terminal => Icon::Terminal,
        TabKind::Files => Icon::Folder,
        TabKind::Stats => Icon::Stats,
    }
}

/// How wide a tab is when nothing is squeezed, in points: for telling whether the row fits.
/// `named`: the tab names its server (Settings → Layout); else its group does.
fn natural_width(st: &StripTab, label: &str, server: &str, named: bool) -> f32 {
    let mut w = 10.0 + theme::ICON + 7.0 + rune::text::advance(label, theme::TAB_NAME) + 10.0;
    if named {
        w += 7.0 + rune::text::advance(server, theme::TAB_SERVER);
    }
    if st.prod && named {
        w += 7.0 + 10.0 + rune::text::advance("Prod", theme::CHIP);
    }
    if let Some(badge) = st.tab.badge() {
        w += 7.0 + 14.0 + rune::text::advance(badge.label(), theme::NEEDS);
    }
    if st.active {
        w += 7.0 + 14.0;
    }
    w + 2.0 + if st.starts_group { 11.0 } else { 0.0 }
}

/// How wide a crowded tab is at its narrowest, and what leads it (the divider and its
/// machine's name, when it starts a group or the run in sight).
fn crowded_width(st: &StripTab, label: &str, server: &str, leads: bool, prod: bool) -> (f32, f32) {
    let mut own = 8.0 + theme::ICON + 6.0 + rune::text::advance(label, theme::TAB_NAME) + 8.0;
    if st.tab.badge().is_some() {
        own += 6.0 + 14.0;
    }
    if st.active {
        own += 6.0 + 14.0;
    }
    let mut lead = if st.starts_group { 11.0 + 2.0 } else { 0.0 };
    if leads {
        lead += 12.0 + rune::text::advance(server, theme::GROUP) + 2.0;
        if prod {
            lead += 7.0 + 10.0 + rune::text::advance("Prod", theme::CHIP);
        }
    }
    (own.min(MIN_TAB) + 2.0, lead)
}

/// The row of tabs. `window_active`: the window is in front.
pub(crate) fn strip(s: &Shell, t: &'static Theme, window_active: bool) -> El {
    let strip = s.bench.strip();
    let row = &s.row;
    // Every name as it may be drawn (streamer mode).
    let names: Vec<(String, String)> =
        strip.iter().map(|st| (s.text_shown(&st.tab.label()).into_owned(), s.server_shown(&st.tab.server, st.server_name).into_owned())).collect();
    // Settings: every tab names its server (the default), or a server is named once, before
    // its group, as in a crowded row.
    let named = s.settings.more().tabs_name_server;
    let needed: f32 = strip
        .iter()
        .zip(&names)
        .enumerate()
        .map(|(i, (st, (label, server)))| {
            // A group's name before its first tab, where tabs don't name their server.
            let lead = if !named && (i == 0 || st.starts_group) { 12.0 + rune::text::advance(server, theme::GROUP) } else { 0.0 };
            natural_width(st, label, server, named) + lead
        })
        .sum::<f32>()
        + theme::CONTROL
        + 2.0;
    // Crowded or roomy: as the widths say, but not changing under the pointer.
    let crowded = s.strip_width.is_some_and(|w| needed > w);
    let compact = row.held.as_ref().map_or(crowded, |h| h.compact) && !strip.is_empty();
    row.compact.set(compact);
    let grouped = compact || !named;
    // While Settings is over the page, no tab is the one on screen.
    let in_settings = s.screens.settings.open;
    let current = s.bench.current_key();
    let active_at = strip.iter().position(|st| st.active);
    let partner = s.bench.active_id().and_then(|id| s.layout.partner(id));

    // The tabs in sight.
    let prod_of = |st: &StripTab| s.bench.server(&st.tab.server).is_some_and(|sv| sv.is_prod());
    let mut sight = 0..strip.len();
    if let (true, Some(room)) = (compact, s.strip_width) {
        let widths = |from: usize| -> Vec<f32> {
            strip
                .iter()
                .zip(&names)
                .enumerate()
                .map(|(i, (st, (label, server)))| {
                    let (own, lead) = crowded_width(st, label, server, i == from || i == 0 || st.starts_group, prod_of(st));
                    own + lead
                })
                .collect()
        };
        let all = widths(0);
        let plus = theme::CONTROL + 2.0;
        if all.iter().sum::<f32>() + plus > room {
            // Not even crowded: a run of them, and the list for the rest.
            let first = row.first.get().min(strip.len().saturating_sub(1));
            sight = layout::in_sight(&widths(first), room - 2.0 * plus, first, active_at);
            // The run's first tab is led by its machine's name: fit again with that counted.
            sight = layout::in_sight(&widths(sight.start), room - 2.0 * plus, sight.start, active_at);
        }
    }
    row.first.set(sight.start);
    *row.shown.borrow_mut() = strip[sight.clone()].iter().map(|st| st.tab.id).collect();
    let hidden = sight.len() < strip.len();

    let mut tabs = rune::row()
        .key("tabs")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .items_center()
        .gap(2.0)
        .role("tablist")
        .on_layout(|s: &mut Shell, [_, _, w, _]| s.strip_width = Some(w))
        .on_hover_change(|s: &mut Shell, over, tx| s.row_hover(over, tx));
    for (i, (st, (label, server))) in strip.iter().zip(&names).enumerate() {
        if !sight.contains(&i) {
            continue;
        }
        let id = st.tab.id;
        if st.starts_group && i > sight.start {
            tabs = tabs.child(divider(t).key(format!("sep-{}", id.0)));
        }
        if grouped && (i == sight.start || st.starts_group) {
            // Crowded, or tabs that don't name it: the server is named once, before its tabs.
            tabs = tabs.child(group_name(t, server, prod_of(st), current == Some(st.tab.server.as_str())).key(format!("group-{}", id.0)));
        }
        let spec = Tab {
            icon: tab_icon(st.tab),
            name: label,
            server: (!grouped).then_some(server.as_str()),
            prod: st.prod && !grouped,
            badge: st.tab.badge(),
            active: st.active && !in_settings,
            window_active,
            compact,
        };
        let mut e = tab(t, &spec, move |s: &mut Shell, tx| s.close_tab(id, tx))
            .key(format!("tab-{}", id.0))
            .on_press_tx(move |s: &mut Shell, tx| s.focus_tab(id, tx))
            .on_middle_press(move |s: &mut Shell, tx| s.close_tab(id, tx))
            .on_secondary_press(move |s: &mut Shell, press, tx| s.open_tab_menu(id, [press.x, press.y], tx))
            .on_drag(move |s: &mut Shell, d, tx| s.tab_drag(id, d, tx))
            .on_layout_tx(move |s: &mut Shell, rect, tx| {
                s.row.rects.insert(id, rect);
                // Only noted: nothing is drawn from it.
                tx.live_only();
            });
        if Some(id) == partner {
            // Shown beside the tab you're on: lit a little, as a tab in a window that isn't
            // in front is.
            e = e.fill(t.hov);
        }
        if compact {
            let (own, _) = crowded_width(st, label, server, false, false);
            e = e.min_w(own - 2.0);
        }
        if let Some(width) = row.held.as_ref().and_then(|h| h.widths.get(&id)) {
            // Under the pointer the row keeps still: this tab is as wide as it was.
            e = e.w(*width).min_w(*width);
        }
        tabs = tabs.child(e);
    }
    if hidden {
        let waits = strip.iter().enumerate().any(|(i, st)| !sight.contains(&i) && st.tab.needs_you);
        let mut list = toolbar_button(t, "tab-list", Icon::ChevronDown, "All tabs").on_press_tx(|s: &mut Shell, tx| s.toggle_tab_list(tx));
        if waits {
            // A tab out of sight waits for you.
            list = list.child(ringed_dot(7.0, 2.0, t.orange_dot, t.frame).key("tab-list-waits").absolute(theme::CONTROL - 11.0, 0.0)).label_static("All tabs. One of them needs you");
        }
        tabs = tabs.child(list);
    }
    tabs.child(toolbar_button(t, "new-tab", Icon::Plus, "New terminal").on_press_tx(|s: &mut Shell, tx| s.new_terminal(tx)))
}

/// What is drawn over the window for the row: a tab's menu, or the list of all tabs.
pub(crate) fn overlays(s: &Shell, t: &'static Theme) -> Vec<El> {
    let mut out = Vec::new();
    if let Some((id, at)) = s.row.menu.filter(|(id, _)| s.bench.tab(*id).is_some()) {
        out.push(rune::el().key("tab-menu-at").absolute(at[0], at[1]).size(1.0, 1.0).a11y_hidden());
        out.push(tab_menu(s, t, id));
    }
    if s.row.list {
        out.push(tab_list(s, t));
    }
    out
}

/// A tab's menu: side by side, duplicate, and the three ways to close.
fn tab_menu(s: &Shell, t: &'static Theme, id: TabId) -> El {
    let mac = s.options.mac_keys;
    let active = s.bench.active_id();
    let paired = s.layout.pair_of(id).is_some();
    // Beside the tab on screen; for the tab on screen itself, beside its neighbour.
    let beside = match active {
        Some(a) if a == id => Layout::candidate(&s.bench, id).is_some(),
        Some(a) => Layout::can_pair(&s.bench, a, id),
        None => false,
    };
    let row = |key: &'static str, label: &'static str, keys: Option<&'static str>, pick: TabPick, on: bool| {
        let e = menu_row(t, label, keys, if on { RowState::Rest } else { RowState::Off }).key(key);
        if !on {
            return e;
        }
        e.on_press_tx(move |s: &mut Shell, tx| s.tab_pick(id, pick, tx))
    };
    let side_keys = Some(if mac { "⌘⇧S" } else { "Ctrl+Shift+S" });
    let side = if paired {
        row("tab-apart", "Stop showing side by side", side_keys, TabPick::Apart, true)
    } else {
        row("tab-beside", "Show side by side", side_keys.filter(|_| active == Some(id)), TabPick::SideBySide, beside)
    };
    let rows = [
        side,
        row("tab-duplicate", "Duplicate tab", None, TabPick::Duplicate, true),
        menu_separator(t),
        row("tab-close", "Close tab", Some(if mac { "⌘W" } else { "Ctrl+Shift+W" }), TabPick::Close, true),
        row("tab-close-others", "Close other tabs", None, TabPick::CloseOthers, !layout::others(&s.bench, id).is_empty()),
        row("tab-close-right", "Close tabs to the right", None, TabPick::CloseRight, !layout::to_the_right(&s.bench, id).is_empty()),
    ];
    popover(t, MENU_WIDTH)
        .key("tab-menu")
        .anchored("tab-menu-at", Side::Bottom, Align::Start, 0.0)
        .child(menu(rows))
        .modal_tx(true, |s: &mut Shell, tx| s.close_tab_overlays(tx))
}

/// The list of all tabs, under its button: each by its name and machine, and what it says.
fn tab_list(s: &Shell, t: &'static Theme) -> El {
    let strip = s.bench.strip();
    let mut rows = Vec::with_capacity(strip.len());
    for st in &strip {
        let id = st.tab.id;
        if st.starts_group {
            rows.push(menu_separator(t));
        }
        let label = s.text_shown(&st.tab.label()).into_owned();
        let server = s.server_shown(&st.tab.server, st.server_name).into_owned();
        let said = st.tab.badge().map(|b| b.label());
        let ink = if st.tab.needs_you { t.orange } else { t.l2 };
        let mut row = rune::row()
            .key(format!("list-{}", id.0))
            .h(crate::kit::menu::ROW)
            .w_full()
            .px(8.0)
            .gap(8.0)
            .radius(theme::RADIUS_ROW)
            .items_center()
            .focusable(true)
            .no_focus_ring()
            .role("menuitem")
            .label(match said {
                Some(said) => format!("{label}, {server}, {said}"),
                None => format!("{label}, {server}"),
            })
            .on_press_tx(move |s: &mut Shell, tx| {
                s.row.close_overlays();
                s.focus_tab(id, tx);
            })
            .child(icon(tab_icon(st.tab), theme::ICON, if st.active { t.l1 } else { t.l2 }))
            .child(rune::text(label).font(theme::TAB_NAME).color(t.l1).single_line().truncate().shrink())
            .child(rune::text(server).font(theme::TAB_SERVER).color(t.l2).single_line())
            .child(rune::spacer());
        // The tab you're on is marked as the one selected; the rest grey under the pointer.
        row = if st.active { row.fill(t.hov2) } else { row.hover(t.hov2).press(t.down) };
        if let Some(said) = said {
            row = row.child(rune::text(said).font(theme::NEEDS).color(ink).single_line());
        }
        rows.push(row);
    }
    popover(t, LIST_WIDTH)
        .key("tab-list-menu")
        .anchored_fit("tab-list", Side::Bottom, Align::End, 6.0)
        // A long list scrolls inside the room the window leaves it.
        .child(rune::col().key("tab-list-rows").w_full().shrink().min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(menu(rows)))
        .modal_tx(true, |s: &mut Shell, tx| s.close_tab_overlays(tx))
}
