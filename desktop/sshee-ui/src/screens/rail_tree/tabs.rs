//! A server's tabs under it in the sidebar, when Settings → Layout → Where tabs go says "Under
//! each server" (0.10's `SidebarTabs` in `Tabs.tsx`): the first part of the server's tree, above
//! its containers, shown whether or not the chevron opened the containers. Along the top there
//! is then only "+" (`shell::tabs`), as 0.10's top bar had no row of tabs then.
//!
//! ```text
//! tabs-N        under server row N: a line hanging from its icon, then one row per tab
//! └── tab-<id>  its icon, its name ("Needs you" on a line of its own), its state as the row
//!               along the top draws it, its cross under the pointer. In the rail of icons:
//!               a smaller icon with a dot when it waits for you or lost its connection.
//! ```
//!
//! A click goes to it, a middle click or its cross closes it, a right click opens the same menu
//! as a tab along the top. Dragged up or down it moves among its machine's tabs; dragged onto
//! the page it lands there beside the tab on screen, as a tab along the top does (`split.rs`).
//! Each row keeps the key the tab has along the top (`tab-<id>`), so what points at a tab (an
//! agent's question under it) finds it here too.

use std::collections::HashMap;

use rune::{col, el, row, text, Drag, El, Phase, Tx};
use sshee_app::workbench::layout;
use sshee_app::workbench::{Badge, TabId};

use crate::icons::icon;
use crate::kit::tab::{state_mark, Tab};
use crate::kit::{hollow_dot, ringed_dot};
use crate::shell::tabs::{connecting, progress, tab_icon, went_quiet, working};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// A tab's row under a server in the list.
const ROW: f32 = 26.0;

/// A tab's tile under a server's icon in the rail.
const TILE: f32 = 28.0;

/// The room between two rows or tiles.
const GAP: f32 = 1.0;

/// How far the pointer goes before a press becomes a drag.
const DRAG_START: f32 = 4.0;

/// A tab being dragged up or down its machine's list, while it lasts. Its rows stay where they
/// are until it's dropped: the dragged one follows the pointer, the ones it passes step aside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dragging {
    id: TabId,
    moved: bool,
    dx: f32,
    dy: f32,
    /// Over the page: it would land there, beside the tab on screen.
    over_page: bool,
    /// Before which of its machine's other tabs it would land.
    slot: Option<usize>,
}

/// The tabs under the servers: where each was laid out, and a drag.
#[derive(Default)]
pub(crate) struct Tabs {
    rects: HashMap<TabId, [f32; 4]>,
    pub(crate) drag: Option<Dragging>,
}

impl Shell {
    /// A press, a drag and a release on a tab under its server.
    pub(crate) fn side_tab_drag(&mut self, id: TabId, d: &Drag, tx: &mut Tx) {
        match d.phase {
            Phase::Down => {
                self.screens.rail_tree.tabs.drag = Some(Dragging { id, moved: false, dx: 0.0, dy: 0.0, over_page: false, slot: None });
                tx.live_only();
            }
            Phase::Move => {
                let Some(mut drag) = self.screens.rail_tree.tabs.drag.filter(|g| g.id == id) else { return };
                if !drag.moved && d.dx.abs() < DRAG_START && d.dy.abs() < DRAG_START {
                    return tx.live_only();
                }
                let at = d.window_point();
                (drag.moved, drag.dx, drag.dy) = (true, d.dx, d.dy);
                drag.over_page = self.sides.page.is_some_and(|[x, y, w, h]| at[0] >= x && at[0] < x + w && at[1] >= y && at[1] < y + h);
                self.sides.drop = if drag.over_page { self.drop_target(id, at) } else { None };
                drag.slot = if drag.over_page { None } else { self.side_slot(id, at[1]) };
                self.screens.rail_tree.tabs.drag = Some(drag);
                tx.animated();
            }
            Phase::Up => {
                let Some(drag) = self.screens.rail_tree.tabs.drag.take().filter(|g| g.id == id) else { return };
                let drop = self.sides.drop.take();
                tx.instant();
                if d.cancelled() || !drag.moved {
                    return;
                }
                if let Some(at) = drop {
                    return self.drop_on(id, at, tx);
                }
                if let Some(to) = drag.slot.and_then(|slot| self.side_place(id, slot)) {
                    self.bench.move_tab(id, to);
                }
                // The tab you dragged is the one you're on, as along the top; the new order is
                // kept for the next run.
                self.focus_tab(id, tx);
            }
        }
    }

    /// The other tabs of `id`'s machine, in order, with the middles of their rows.
    fn side_siblings(&self, id: TabId) -> Vec<(TabId, f32)> {
        let Some(server) = self.bench.tab(id).map(|t| t.server.clone()) else { return Vec::new() };
        let rects = &self.screens.rail_tree.tabs.rects;
        self.bench.tabs_on(&server).filter(|t| t.id != id).filter_map(|t| rects.get(&t.id).map(|r| (t.id, r[1] + r[3] / 2.0))).collect()
    }

    /// Before which of its machine's other tabs `id` would land with the pointer at `y`.
    fn side_slot(&self, id: TabId, y: f32) -> Option<usize> {
        let middles: Vec<f32> = self.side_siblings(id).iter().map(|(_, m)| *m).collect();
        Some(layout::slot(&middles, y))
    }

    /// Slot `slot` among `id`'s machine's other tabs, as a place in the whole row counted
    /// without it (what `move_tab` takes).
    fn side_place(&self, id: TabId, slot: usize) -> Option<usize> {
        let siblings = self.side_siblings(id);
        let rest: Vec<TabId> = self.bench.tabs().iter().map(|t| t.id).filter(|t| *t != id).collect();
        match siblings.get(slot) {
            Some((before, _)) => rest.iter().position(|t| t == before),
            None => siblings.last().and_then(|(last, _)| rest.iter().position(|t| t == last)).map(|i| i + 1),
        }
    }

    /// While a tab is dragged along its list: how far each of its machine's tabs is drawn from
    /// its place, the dragged one under the pointer.
    fn side_shifts(&self) -> HashMap<TabId, [f32; 2]> {
        let mut shifts = HashMap::new();
        let Some(drag) = self.screens.rail_tree.tabs.drag.filter(|g| g.moved) else { return shifts };
        shifts.insert(drag.id, [if drag.over_page { drag.dx } else { 0.0 }, drag.dy]);
        let (Some(server), Some(slot)) = (self.bench.tab(drag.id).map(|t| t.server.clone()), drag.slot) else { return shifts };
        let Some(mine) = self.screens.rail_tree.tabs.rects.get(&drag.id) else { return shifts };
        let room = mine[3] + GAP;
        let list: Vec<TabId> = self.bench.tabs_on(&server).map(|t| t.id).collect();
        let Some(from) = list.iter().position(|t| *t == drag.id) else { return shifts };
        for (i, t) in list.iter().filter(|t| **t != drag.id).enumerate() {
            if i < from && i >= slot {
                shifts.insert(*t, [0.0, room]);
            } else if i >= from && i < slot {
                shifts.insert(*t, [0.0, -room]);
            }
        }
        shifts
    }
}

impl Tabs {
    /// Forgets the rows of tabs that are gone.
    pub(crate) fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.rects.retain(|id, _| open(*id));
        if self.drag.is_some_and(|g| !open(g.id)) {
            self.drag = None;
        }
    }
}

/// What a tab under its server says to a screen reader and under the pointer.
fn words(name: &str, server: &str, badge: Option<Badge>) -> (String, String) {
    let label = match badge {
        Some(b) => format!("{name}, {}", b.label()),
        None => name.to_owned(),
    };
    let tip = match badge {
        Some(b) => format!("{name} on {server} ({}). Right-click for more, middle-click to close", b.label()),
        None => format!("{name} on {server}. Right-click for more, middle-click to close"),
    };
    (label, tip)
}

/// What every tab under a server does: go there, close, its menu, a drag; its place noted.
fn wired(e: El, id: TabId) -> El {
    e.key(format!("tab-{}", id.0))
        .focusable(true)
        .role("tab")
        .on_press_tx(move |s: &mut Shell, tx| s.focus_tab(id, tx))
        .on_middle_press(move |s: &mut Shell, tx| s.close_tab(id, tx))
        .on_secondary_press(move |s: &mut Shell, at, tx| s.open_tab_menu(id, [at.x, at.y], tx))
        .on_drag(move |s: &mut Shell, d, tx| s.side_tab_drag(id, d, tx))
        // Resting on a waiting agent's tab brings up its question (`screens::agent`).
        .on_hover_change(move |s: &mut Shell, over, tx| crate::screens::agent::tab_hover(s, id, over, tx))
        .on_layout_tx(move |s: &mut Shell, rect, tx| {
            s.screens.rail_tree.tabs.rects.insert(id, rect);
            // Only noted: nothing is drawn from it.
            tx.live_only();
        })
}

/// Drawn where a drag puts it: lifted and following the pointer, or stepped aside.
fn dragged(s: &Shell, t: &Theme, e: El, id: TabId, shifts: &HashMap<TabId, [f32; 2]>) -> El {
    let Some([dx, dy]) = shifts.get(&id).copied() else { return e };
    let e = e.offset(dx, dy);
    if s.screens.rail_tree.tabs.drag.is_some_and(|g| g.id == id) {
        let e = e.offset_motion(rune::Anim::INSTANT).raise().fill(t.pop).border(0.5, t.rim).shadow(4.0, 14.0, rune::Color::BLACK.alpha(if t.dark { 0.4 } else { 0.16 }));
        // Over the page, where the mark says it lands: it doesn't hide the mark.
        return if s.sides.drop.is_some() { e.layer_opacity(0.85) } else { e };
    }
    e
}

/// The tabs under server row `index` in the list, while they go there: `None` when they don't,
/// or it has none. `branch`: where the line they hang from runs, under the server's icon.
pub fn list(s: &Shell, t: &'static Theme, index: usize, server: &str, branch: f32) -> Option<El> {
    if !s.bench.tabs_under_servers() || s.bench.tabs_on(server).next().is_none() {
        return None;
    }
    let server_name = s.bench.server(server).map_or("Unknown server", |sv| sv.name.as_str());
    let server_name = s.server_shown(server, server_name).into_owned();
    let in_settings = s.screens.settings.open;
    let shifts = s.side_shifts();
    let mut out = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .pl(5.0)
        .gap(GAP)
        .role("tablist")
        .label(format!("Tabs on {server_name}"))
        // The line they hang from, starting under the server's icon.
        .border_sides([0.0, 0.0, 0.0, 1.0], t.hair);
    for tab in s.bench.tabs_on(server) {
        let id = tab.id;
        let active = s.bench.active_id() == Some(id) && !in_settings && !s.bench.showing_home();
        let name = s.text_shown(&sshee_app::agents::tab_label(tab)).into_owned();
        // "Needs you" has a line of its own: the name keeps its words.
        let badge = tab.badge();
        let spec = Tab {
            icon: tab_icon(tab),
            name: &name,
            server: None,
            env: None,
            badge: badge.filter(|b| *b != Badge::NeedsYou),
            working: working(s, tab),
            connecting: connecting(tab),
            went_quiet: went_quiet(s, tab),
            progress: progress(s, tab),
            active,
            window_active: s.window_focused,
            compact: true,
        };
        let (label, tip) = words(&name, &server_name, badge);
        let mut words = col().grow(1.0).shrink().min_w(0.0).child(text(name.clone()).key("name").font(theme::NOTE).color(if active || tab.needs_you { t.l1 } else { t.l2on }).single_line().truncate_fade().shrink());
        if tab.needs_you {
            words = words.child(text("Needs you").key("needs-you").font(theme::SMALL).color(t.orange).single_line());
        }
        // Its state and its cross share one place: the state at rest, the cross under the
        // pointer (0.10's), so the name keeps its room.
        let cross = el()
            .key("close")
            .absolute(0.0, 0.0)
            .size(20.0, 20.0)
            .radius(5.0)
            .items_center()
            .justify_center()
            .hover(t.hov)
            .press(t.down)
            .role("button")
            .label(format!("Close {name}"))
            .on_press_tx(move |s: &mut Shell, tx| s.close_tab(id, tx))
            .group_opacity(0.0, 1.0)
            .child(icon(crate::icons::Icon::Close, 13.0, t.l2on));
        let mut end = el().size(20.0, 20.0).items_center().justify_center();
        if let Some(mark) = state_mark(t, &spec, true) {
            end = end.child(mark.group_opacity(1.0, 0.0));
        }
        let mut e = row()
            .w_full()
            .min_h(ROW)
            .py(3.0)
            .pl(6.0)
            .pr(3.0)
            .gap(7.0)
            .items_center()
            .radius(theme::RADIUS_ROW)
            .hover_group()
            .label(label)
            .help(tip)
            .help_side(crate::shell::rail::tip_side(s))
            .child(icon(spec.icon, 14.0, if active { t.l1 } else { t.l2 }))
            .child(words)
            .child(end.child(cross));
        e = if active { e.fill(t.selrail) } else { e.hover(t.hov2).press(t.down) };
        if active {
            // The one you're on marks the line it hangs from, in the accent.
            e = e.child(el().key("on").absolute(-6.5, 5.0).size(2.0, ROW - 10.0).radius(1.0).fill(t.blue).a11y_hidden());
        }
        out = out.child(dragged(s, t, wired(e, id), id, &shifts));
    }
    Some(row().key(format!("tabs-{index}")).w_full().pl(branch).pb(4.0).child(out))
}

/// The tabs under a server's icon in the rail: smaller icons, the one you're on lit, a dot on
/// one that waits for you (orange) or lost its connection (hollow).
pub fn rail(s: &Shell, t: &'static Theme, index: usize, server: &str) -> Option<El> {
    if !s.bench.tabs_under_servers() || s.bench.tabs_on(server).next().is_none() {
        return None;
    }
    let server_name = s.bench.server(server).map_or("Unknown server", |sv| sv.name.as_str());
    let server_name = s.server_shown(server, server_name).into_owned();
    let in_settings = s.screens.settings.open;
    let side = crate::shell::rail::tip_side(s);
    let shifts = s.side_shifts();
    let mut out = col().key(format!("tabs-{index}")).items_center().gap(GAP).pb(4.0).role("tablist").label(format!("Tabs on {server_name}"));
    for tab in s.bench.tabs_on(server) {
        let id = tab.id;
        let active = s.bench.active_id() == Some(id) && !in_settings && !s.bench.showing_home();
        let name = s.text_shown(&sshee_app::agents::tab_label(tab)).into_owned();
        let badge = tab.badge();
        let (label, tip) = words(&name, &server_name, badge);
        let mut e = el()
            .size(TILE, TILE)
            .radius(theme::RADIUS_ROW)
            .items_center()
            .justify_center()
            .label(label)
            .help(tip)
            .help_side(side)
            .child(icon(tab_icon(tab), 14.0, if active { t.l1 } else { t.l2 }));
        e = if active { e.fill(t.selrail) } else { e.hover(t.hov2).press(t.down) };
        let mark = match badge {
            Some(Badge::NeedsYou) => Some(ringed_dot(6.0, 1.5, t.orange_dot, t.frame)),
            Some(Badge::Reconnecting | Badge::Disconnected | Badge::Failed | Badge::Ended) => Some(el().size(9.0, 9.0).radius(4.5).fill(t.frame).items_center().justify_center().a11y_hidden().child(hollow_dot(6.0, 1.2, t.l2))),
            _ => None,
        };
        if let Some(mark) = mark {
            e = e.child(mark.key("tab-dot").absolute(TILE - 9.0, 0.0));
        }
        out = out.child(dragged(s, t, wired(e, id), id, &shifts));
    }
    Some(out)
}
