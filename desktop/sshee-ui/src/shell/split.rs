//! Tabs sharing the page, split right and down (0.10's tree of splits): every pane a tab of
//! its own, a divider between two that can be dragged (double-click: even shares), and over
//! each a thin header with its name and its buttons. The pane you're in has the keyboard, its
//! name and icon in full ink and its buttons out; the others are quieter until the pointer
//! comes.
//!
//! ```text
//! split          row or column, the group's root (`split-1-0` is the part at 1, then 0)
//! ├── side-N     column: one pane, its share of the room the dividers leave
//! │   ├── side-head-N   icon, name, [Split right] [Split down] [Fill the window] [×]
//! │   └── pane-N        the terminal (or the tab's page)
//! ├── divider-0  drag to give one side more room (`divider-1-0`: inside part 1)
//! └── split-2    a split the other way, of more panes
//! ```
//!
//! What splits, where a closed pane's room goes, how far a divider moves and what is kept
//! between runs are `sshee_app::workbench::layout`'s rules.

use std::collections::HashMap;

use rune::{Cx, Drag, El, Phase, Tx};
use sshee_app::workbench::layout::{self, Dir, Group, Node, Zone};
use sshee_app::workbench::shortcuts::Shortcut;
use sshee_app::workbench::{Tab, TabId, TabKind};

use super::state::Shell;
use super::tabs::tab_icon;
use crate::icons::{icon, Icon};
use crate::kit::chip::prod_chip;
use crate::screens;
use crate::terminal::view::pane_view_with;
use crate::theme::{self, Theme};

/// How far the pointer travels before a press on a pane's header is a drag.
const DRAG_START: f32 = 6.0;

/// A pane's header.
const HEAD: f32 = 28.0;

/// A header's buttons.
const HEAD_BUTTON: f32 = 22.0;

/// The split view's state beyond the groups themselves.
#[derive(Default)]
pub struct Sides {
    /// Where each split of the group on screen is drawn, `[x, y, w, h]` in window points, by
    /// its path: a drag of its dividers is measured against it.
    rects: HashMap<Vec<usize>, [f32; 4]>,
    /// Where each pane on screen is drawn, and the page: a dragged tab lands by them.
    panes: HashMap<TabId, [f32; 4]>,
    pub(crate) page: Option<[f32; 4]>,
    /// Where the tab or pane being dragged would land now.
    pub(crate) drop: Option<(TabId, Zone)>,
    /// A pane's header being dragged, once it has moved.
    moving: Option<TabId>,
}

/// The room between two panes: Settings' "Space between panes", with a line in its middle and
/// enough of it to grab.
pub(crate) fn divider_room(s: &Shell) -> f32 {
    (s.settings.get().pane_gap as f32).clamp(0.0, 16.0).max(4.0) + 1.0
}

/// A split's element key, by its path in the group.
fn path_key(word: &str, path: &[usize]) -> String {
    path.iter().fold(word.to_owned(), |k, i| format!("{k}-{i}"))
}

impl Shell {
    /// The group whose panes share the page now (none while one fills the window).
    pub fn split_on_screen(&self) -> Option<&Group> {
        self.bench.split_on_screen()
    }

    /// The tabs whose pages are drawn now: the one you're on, and those sharing the page.
    pub(crate) fn tabs_on_screen(&self) -> Vec<TabId> {
        self.bench.on_screen()
    }

    /// Split right or down: a new terminal beside the tab on screen, with the keyboard.
    pub fn split_pane(&mut self, dir: Dir, tx: &mut Tx) {
        if self.bench.split(dir).is_some() {
            self.settle(tx);
        }
    }

    /// A pane's own Split right or Split down: from that pane.
    pub fn split_from(&mut self, id: TabId, dir: Dir, tx: &mut Tx) {
        self.bench.focus(id);
        self.split_pane(dir, tx);
    }

    /// The shortcut: the tab you're on beside its neighbour; its group one at a time and back.
    /// With no other tab on this machine, a new terminal opens beside it (0.10's "Split right").
    pub fn toggle_side_by_side(&mut self, tx: &mut Tx) {
        self.bench.side_by_side();
        self.settle(tx);
    }

    /// A tab's menu: show it beside (`Dir::Row`) or below (`Dir::Col`) the tab on screen; for
    /// the tab on screen, the shortcut.
    pub fn show_beside(&mut self, id: TabId, dir: Dir, tx: &mut Tx) {
        self.bench.show_beside(id, dir);
        self.settle(tx);
    }

    /// Settings' "Arrange open panes", and the palette's.
    pub fn arrange(&mut self, how: layout::Arrangement, tx: &mut Tx) {
        self.bench.arrange(how);
        self.settle(tx);
    }

    /// Where a tab or pane `dragged` would land with the pointer at `at` (window points): a
    /// side or the middle of a pane on screen, not its own.
    pub(crate) fn drop_target(&self, dragged: TabId, at: [f32; 2]) -> Option<(TabId, Zone)> {
        let p = (at[0], at[1]);
        if self.split_on_screen().is_some() {
            return self.tabs_on_screen().into_iter().filter(|id| *id != dragged).find_map(|id| Some((id, layout::zone(*self.sides.panes.get(&id)?, p)?)));
        }
        let id = self.bench.active_id().filter(|id| *id != dragged)?;
        Some((id, layout::zone(self.sides.page?, p)?))
    }

    /// A tab or pane dropped on a pane: it lands there, and you're on it.
    pub(crate) fn drop_on(&mut self, dragged: TabId, (target, zone): (TabId, Zone), tx: &mut Tx) {
        self.bench.place(target, dragged, zone);
        self.settle(tx);
    }

    /// A press, a drag and a release on a pane's header: dropped on another pane it lands
    /// there, dropped on the row of tabs it goes back to being a tab of its own; a double click
    /// fills the window with it, and back.
    pub(crate) fn pane_drag(&mut self, id: TabId, d: &Drag, tx: &mut Tx) {
        match d.phase {
            Phase::Down if d.clicks >= 2 => self.toggle_zoom(id, tx),
            Phase::Down => {
                self.sides.moving = None;
                if self.bench.active_id() != Some(id) {
                    self.focus_tab(id, tx);
                }
            }
            Phase::Move => {
                if self.sides.moving.is_none() && d.dx.hypot(d.dy) < DRAG_START {
                    return tx.live_only();
                }
                self.sides.moving = Some(id);
                self.sides.drop = self.drop_target(id, d.window_point());
                tx.instant();
            }
            Phase::Up => {
                let (moved, drop) = (self.sides.moving.take(), self.sides.drop.take());
                if moved.is_none() || d.cancelled() {
                    return tx.instant();
                }
                match drop {
                    Some(at) => self.drop_on(id, at, tx),
                    // Over the row of tabs: a tab of its own again.
                    None if d.window_point()[1] < theme::BAR_HEIGHT => self.leave_pair(id, tx),
                    None => self.settle(tx),
                }
            }
        }
    }

    /// A pane's ×, or a tab's menu: off the shared page, still open as a tab. The pane nearest
    /// it takes its room (and, if it was the one you were in, the keyboard).
    pub fn leave_pair(&mut self, id: TabId, tx: &mut Tx) {
        self.bench.leave_split(id);
        self.settle(tx);
    }

    /// "Fill the window" for pane `id`, or all the panes again.
    pub fn toggle_zoom(&mut self, id: TabId, tx: &mut Tx) {
        self.bench.focus(id);
        self.bench.zoom();
        self.settle(tx);
    }

    /// The shortcut for "Fill the window": the tab you're on.
    pub fn zoom_active(&mut self, tx: &mut Tx) {
        if self.bench.zoom() {
            self.settle(tx);
        }
    }

    /// A drag of divider `i` of the split at `path`: the panes beside it are as wide (or tall)
    /// as the pointer says.
    pub(crate) fn divider_drag(&mut self, path: &[usize], i: usize, d: &Drag, tx: &mut Tx) {
        if d.phase == Phase::Down && d.clicks >= 2 {
            self.bench.even_out(path);
        }
        let parts = match self.split_on_screen().and_then(|g| g.root.at(path)) {
            Some(Node::Split { dir, parts }) => Some((*dir, parts.len())),
            _ => None,
        };
        if let (Phase::Move, Some((dir, n)), Some([x, y, w, h])) = (d.phase, parts, self.sides.rects.get(path).copied()) {
            let (start, extent, at) = match dir {
                Dir::Row => (x, w, d.window_point()[0]),
                Dir::Col => (y, h, d.window_point()[1]),
            };
            let gap = divider_room(self);
            let room = extent - gap * (n - 1) as f32;
            if room > 0.0 {
                // Where the pointer is, as a share of the room: the divider's middle under it.
                let share = (at - start - gap * i as f32 - gap / 2.0) / room;
                self.bench.drag_divider(path, i, share, room);
            }
        }
        if d.phase == Phase::Up {
            // The keyboard goes back to the terminal you were in.
            self.settle(tx);
        }
        tx.instant();
    }
}

/// A header's button. It shows on the pane you're in, and on another while the pointer is over
/// that pane's header.
fn head_button(t: &Theme, key: String, glyph: Icon, name: String, lit: bool) -> El {
    let e = rune::el()
        .key(key)
        .size(HEAD_BUTTON, HEAD_BUTTON)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .justify_center()
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label(name.clone())
        .help(name)
        .child(icon(glyph, 14.0, t.l2on));
    if lit {
        e
    } else {
        e.group_opacity(0.0, 1.0)
    }
}

/// What a button's tooltip says, with its shortcut when it has one.
fn with_keys(s: &Shell, words: &str, shortcut: Shortcut) -> String {
    match sshee_app::palette::shortcut_label(shortcut, s.options.mac_keys, &s.options.shortcuts) {
        Some(keys) if !keys.is_empty() => format!("{words} ({keys})"),
        _ => words.to_owned(),
    }
}

/// One pane: its header and its tab's page. `mixed`: the page holds more than one machine's
/// tabs, so each pane says whose it is. `alone`: it fills the window (`count` panes in all).
fn side(s: &Shell, tab: &Tab, t: &'static Theme, cx: &mut Cx<Shell>, mixed: bool, zoomed: Option<usize>) -> El {
    let id = tab.id;
    let active = s.bench.active_id() == Some(id);
    let ink = if active { t.l1 } else { t.l2 };
    let label = s.text_shown(&tab.label()).into_owned();
    let mut head = rune::row()
        .key(format!("side-head-{}", id.0))
        .w_full()
        .h(HEAD)
        .pl(12.0)
        .pr(4.0)
        .gap(7.0)
        .items_center()
        .hover_group()
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        // Dragged onto another pane it moves there, onto the row of tabs it's a tab of its own;
        // a double click fills the window with it, and back (0.10).
        .cursor(rune::Cursor::Grab)
        .help("Drag onto another pane to move it, onto the tabs to show it alone. Double-click to fill the window")
        .on_drag(move |s: &mut Shell, d, tx| s.pane_drag(id, d, tx))
        .child(icon(tab_icon(tab), theme::ICON, ink))
        .child(rune::text(label).key("side-name").font(theme::TAB_NAME).color(ink).single_line().truncate_fade().shrink());
    if mixed {
        let server = s.bench.server(&tab.server);
        let name = server.map_or("Unknown server", |sv| sv.name.as_str());
        head = head.child(rune::text(s.server_shown(&tab.server, name).into_owned()).key("side-server").font(theme::TAB_SERVER).color(t.l2).single_line());
        if server.is_some_and(|sv| sv.is_prod()) {
            head = head.child(prod_chip(t, false));
        }
    }
    head = head.child(rune::spacer());
    let n = id.0;
    match zoomed {
        // Filling the window: the way back says how many panes there are.
        Some(count) => {
            head = head.child(
                rune::row()
                    .key(format!("side-all-{n}"))
                    .h(HEAD_BUTTON)
                    .px(8.0)
                    .gap(5.0)
                    .items_center()
                    .radius(theme::RADIUS_ROW)
                    .hover(t.hov2)
                    .press(t.down)
                    .focusable(true)
                    .role("button")
                    .help(with_keys(s, "Show all the panes again", Shortcut::Zoom))
                    .on_press_tx(move |s: &mut Shell, tx| s.toggle_zoom(id, tx))
                    .child(icon(Icon::Shrink, 14.0, t.l2on))
                    .child(rune::text(format!("Show all {count} panes")).font(theme::TAB_SERVER).color(t.l2on).single_line()),
            );
        }
        None => {
            // Close together, so a narrow pane (140 points) still holds them all.
            let buttons = rune::row()
                .key("side-buttons")
                .items_center()
                .child(
                    head_button(t, format!("side-right-{n}"), Icon::SideBySide, with_keys(s, "Split right", Shortcut::SplitRight), active)
                        .on_press_tx(move |s: &mut Shell, tx| s.split_from(id, Dir::Row, tx)),
                )
                .child(
                    head_button(t, format!("side-down-{n}"), Icon::SplitDown, with_keys(s, "Split down", Shortcut::SplitDown), active)
                        .on_press_tx(move |s: &mut Shell, tx| s.split_from(id, Dir::Col, tx)),
                )
                .child(
                    head_button(t, format!("side-zoom-{n}"), Icon::Expand, with_keys(s, "Fill the window", Shortcut::Zoom), active)
                        .on_press_tx(move |s: &mut Shell, tx| s.toggle_zoom(id, tx)),
                )
                .child(
                    head_button(t, format!("side-close-{n}"), Icon::Close, "Back to its own tab (it stays open)".to_owned(), active)
                        .on_press_tx(move |s: &mut Shell, tx| s.leave_pair(id, tx)),
                );
            head = head.child(buttons);
        }
    }
    let content = match s.pane(id) {
        Some(pane) => rune::col().key(format!("pane-{n}")).w_full().grow(1.0).min_h(0.0).child(pane_view_with(
            pane,
            cx,
            move |s: &mut Shell| s.panes.get_mut(&id),
            |s: &mut Shell, ev, tx| s.shortcut(ev, tx),
        )),
        None if tab.kind == TabKind::Terminal => rune::el().key("pane-pending").w_full().grow(1.0),
        None if tab.kind == TabKind::Files => rune::col().w_full().grow(1.0).min_h(0.0).child(screens::files::view(s, tab, t, cx)),
        None => rune::col().w_full().grow(1.0).min_h(0.0).child(screens::stats::view(s, tab, t, cx)),
    };
    rune::col()
        .key(format!("side-{n}"))
        .on_layout_tx(move |s: &mut Shell, rect, tx| {
            s.sides.panes.insert(id, rect);
            tx.live_only();
        })
        .layer_opacity(if s.sides.moving == Some(id) { 0.45 } else { 1.0 })
        .role("group")
        .label(if active { "Pane you're in" } else { "Pane" })
        // A press on a page that isn't a terminal makes it the tab you're on too (a terminal
        // says so itself: `Wish::Pressed`).
        .on_background_press_tx(move |s: &mut Shell, tx| {
            if s.bench.active_id() != Some(id) {
                s.focus_tab(id, tx)
            }
        })
        .child(head)
        .child(content)
}

/// A split and what is in it; `path` is its place in the group.
fn node(s: &Shell, n: &Node, path: &mut Vec<usize>, t: &'static Theme, cx: &mut Cx<Shell>, mixed: bool) -> El {
    let Node::Split { dir, parts } = n else {
        let Node::Pane(id) = n else { unreachable!() };
        return match s.bench.tab(*id) {
            Some(tab) => side(s, tab, t, cx, mixed, None),
            None => rune::el(),
        };
    };
    let dir = *dir;
    let key = path_key("split", path);
    let at = path.clone();
    let mut e = match dir {
        Dir::Row => rune::row(),
        Dir::Col => rune::col(),
    }
    .key(key)
    .on_layout_tx(move |s: &mut Shell, rect, tx| {
        s.sides.rects.insert(at.clone(), rect);
        tx.live_only();
    });
    let gap = divider_room(s);
    for (i, (child, share)) in parts.iter().enumerate() {
        if i > 0 {
            e = e.child(divider(t, dir, path, i - 1, gap));
        }
        path.push(i);
        let c = node(s, child, path, t, cx, mixed).grow(*share);
        path.pop();
        e = e.child(match dir {
            Dir::Row => c.min_w(0.0).h_full(),
            Dir::Col => c.min_h(0.0).w_full(),
        });
    }
    e
}

/// The divider after part `i` of the split at `path`.
fn divider(t: &Theme, dir: Dir, path: &[usize], i: usize, room: f32) -> El {
    let at = path.to_vec();
    let line = rune::el().fill(t.edge);
    let (e, line) = match dir {
        Dir::Row => (rune::row().w(room).h_full().justify_center().cursor(rune::Cursor::ResizeColumn), line.w(1.0).h_full()),
        Dir::Col => (rune::col().h(room).w_full().justify_center().cursor(rune::Cursor::ResizeRow), line.h(1.0).w_full()),
    };
    e.key(format!("{}-{i}", path_key("divider", path)))
        .role("separator")
        .label_static("Drag to resize; double-click to share the room evenly")
        .help("Drag to resize. Double-click to share the room evenly")
        .on_drag(move |s: &mut Shell, d, tx| s.divider_drag(&at, i, d, tx))
        .child(line)
}

/// The page when the tab you're on shares it with others: `None` otherwise.
pub(crate) fn page(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let active = s.bench.active_id()?;
    let group = s.bench.layout().shown_with(active)?;
    let panes = group.panes();
    let mut servers: Vec<&str> = panes.iter().filter_map(|id| s.bench.tab(*id)).map(|t| t.server.as_str()).collect();
    servers.sort_unstable();
    servers.dedup();
    let mixed = servers.len() > 1;
    let root = match group.zoomed.and_then(|z| s.bench.tab(z)) {
        // One pane fills the window, with the way back in its header.
        Some(tab) => side(s, tab, t, cx, mixed, Some(panes.len())),
        None => node(s, &group.root, &mut Vec::new(), t, cx, mixed),
    };
    Some(rune::col().key("side").w_full().grow(1.0).min_h(0.0).child(root.w_full().grow(1.0).min_h(0.0)))
}

/// Where a dragged tab or pane would land, drawn over the page: the half (or the whole) of the
/// pane it would take, and what happens there.
pub(crate) fn drop_mark(s: &Shell, t: &Theme) -> Option<El> {
    let (target, zone) = s.sides.drop?;
    let [x, y, w, h] = if s.split_on_screen().is_some() { *s.sides.panes.get(&target)? } else { s.sides.page? };
    let inset = 6.0;
    let (x, y, w, h) = (x + inset, y + inset, w - 2.0 * inset, h - 2.0 * inset);
    let [x, y, w, h] = match zone {
        Zone::Left => [x, y, w / 2.0, h],
        Zone::Right => [x + w / 2.0, y, w / 2.0, h],
        Zone::Top => [x, y, w, h / 2.0],
        Zone::Bottom => [x, y + h / 2.0, w, h / 2.0],
        Zone::Middle => [x, y, w, h],
    };
    let swap = zone == Zone::Middle && s.bench.layout().shown_with(target).is_some();
    let words = if swap { "Swap places" } else { "Move here" };
    Some(
        rune::el()
            .key("drop-mark")
            .absolute(x, y)
            .size(w.max(0.0), h.max(0.0))
            .radius(theme::RADIUS)
            .fill(t.ring.alpha(0.14))
            .border(2.0, t.ring.alpha(0.7))
            .items_center()
            .justify_center()
            .child(rune::el().px(12.0).py(4.0).radius(theme::RADIUS_POPOVER).fill(t.pop).border(0.5, t.rim).child(rune::text(words).font(theme::TAB_NAME).color(t.l1).single_line())),
    )
}
