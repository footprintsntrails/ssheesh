//! Two tabs side by side on the page (0.10's "pair"), as a browser shows two pages: the tab
//! you're on and the one shown beside it, a divider between them that can be dragged, and over
//! each a thin header with its name and a × that takes it out of the pair without closing it.
//!
//! ```text
//! side   row, the page
//! ├── side-N     column, the left tab's share of the width
//! │   ├── side-head-N   its name, and × ("Stop showing side by side")
//! │   └── pane-N        the terminal (or the tab's page)
//! ├── side-divider      drag to give one side more room
//! └── side-M     the right tab, the rest of the width
//! ```
//!
//! Which tabs pair, and how far the divider goes, are `sshee_app::workbench::layout`'s rules.
//! Each side stays a tab of its own in the row; a press on a side makes it the tab you're on
//! (the keyboard follows).

use rune::{Cx, Drag, El, Phase, Tx};
use sshee_app::workbench::layout::{Pair, Toggled};
use sshee_app::workbench::{Tab, TabId, TabKind};

use super::state::Shell;
use super::tabs::tab_icon;
use crate::icons::{icon, Icon};
use crate::screens;
use crate::terminal::view::pane_view_with;
use crate::theme::{self, Theme};

/// The divider's room between the two sides, and the line in its middle.
const DIVIDER: f32 = 7.0;

/// A side's header.
const HEAD: f32 = 28.0;

/// Side by side's state beyond the pairs themselves.
#[derive(Default)]
pub struct Sides {
    /// Where the pair is drawn, `[x, y, w, h]` in window points: a drag of the divider is
    /// measured against it.
    rect: Option<[f32; 4]>,
}

impl Shell {
    /// The pair on screen: the tab you're on and the one beside it.
    pub fn pair_on_screen(&self) -> Option<Pair> {
        self.layout.pair_of(self.bench.active_id()?)
    }

    /// The tabs whose pages are drawn now: the one you're on, and the one beside it.
    pub(crate) fn tabs_on_screen(&self) -> Vec<TabId> {
        match (self.pair_on_screen(), self.bench.active_id()) {
            (Some(pair), _) => vec![pair.left, pair.right],
            (None, Some(id)) => vec![id],
            (None, None) => Vec::new(),
        }
    }

    /// The shortcut: the tab you're on beside its neighbour, or one at a time again. With no
    /// other tab on this machine, a new terminal opens beside it (0.10's "Split right").
    pub fn toggle_side_by_side(&mut self, tx: &mut Tx) {
        let Some(id) = self.bench.active_id() else { return };
        if self.layout.toggle(&self.bench) == Toggled::NeedsTab {
            let other = self.bench.new_terminal();
            // The keyboard stays where you were: the new terminal is the one beside.
            self.bench.focus(id);
            self.layout.pair(&self.bench, id, other);
        }
        self.settle(tx);
    }

    /// A tab's menu: show it beside the tab on screen; for the tab on screen, beside its
    /// neighbour.
    pub fn show_beside(&mut self, id: TabId, tx: &mut Tx) {
        match self.bench.active_id() {
            Some(active) if active == id => return self.toggle_side_by_side(tx),
            Some(active) => {
                self.layout.pair(&self.bench, active, id);
            }
            None => {}
        }
        self.settle(tx);
    }

    /// A side's ×, or a tab's menu: out of the pair, still open. The other side fills the page.
    pub fn leave_pair(&mut self, id: TabId, tx: &mut Tx) {
        let was_on_screen = self.tabs_on_screen().contains(&id);
        if let Some(other) = self.layout.leave(id) {
            if was_on_screen {
                self.bench.focus(other);
            }
        }
        self.settle(tx);
    }

    /// A drag of the divider: the left side is as wide as the pointer says.
    pub(crate) fn divider_drag(&mut self, d: &Drag, tx: &mut Tx) {
        let (Some([x, _, w, _]), Some(pair)) = (self.sides.rect, self.pair_on_screen()) else { return };
        if d.phase == Phase::Move && w > 0.0 {
            self.layout.set_share(pair.left, (d.window_point()[0] - x) / w);
        }
        if d.phase == Phase::Up {
            // The keyboard goes back to the terminal you were in.
            self.settle(tx);
        }
        tx.instant();
    }
}

/// One side: its header and its tab's page.
fn side(s: &Shell, tab: &Tab, t: &'static Theme, cx: &mut Cx<Shell>, active: bool) -> El {
    let id = tab.id;
    let label = s.text_shown(&tab.label()).into_owned();
    let head = rune::row()
        .key(format!("side-head-{}", id.0))
        .w_full()
        .h(HEAD)
        .pl(12.0)
        .pr(4.0)
        .gap(7.0)
        .items_center()
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        .child(icon(tab_icon(tab), theme::ICON, if active { t.l1 } else { t.l2 }))
        .child(rune::text(label).font(theme::TAB_NAME).color(if active { t.l1 } else { t.l2 }).single_line().truncate_fade().shrink())
        .child(rune::spacer())
        .child(
            rune::el()
                .key(format!("side-close-{}", id.0))
                .size(22.0, 22.0)
                .radius(theme::RADIUS_ROW)
                .items_center()
                .justify_center()
                .hover(t.hov2)
                .press(t.down)
                .focusable(true)
                .role("button")
                .label_static("Stop showing side by side")
                .help("Stop showing side by side (the tab stays open)")
                .on_press_tx(move |s: &mut Shell, tx| s.leave_pair(id, tx))
                .child(icon(Icon::Close, 14.0, t.l2on)),
        );
    let content = match s.pane(id) {
        Some(pane) => rune::col().key(format!("pane-{}", id.0)).w_full().grow(1.0).min_h(0.0).child(pane_view_with(
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
        .key(format!("side-{}", id.0))
        .h_full()
        .min_w(0.0)
        // A press on a page that isn't a terminal makes it the tab you're on too (a terminal
        // says so itself: `Wish::Pressed`).
        .on_background_press_tx(move |s: &mut Shell, tx| s.focus_tab(id, tx))
        .child(head)
        .child(content)
}

/// The page when the tab you're on is shown beside another: `None` otherwise.
pub(crate) fn page(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let pair = s.pair_on_screen()?;
    let (left, right) = (s.bench.tab(pair.left)?, s.bench.tab(pair.right)?);
    let active = s.bench.active_id();
    let divider = rune::el()
        .key("side-divider")
        .w(DIVIDER)
        .h_full()
        .justify_center()
        .cursor(rune::Cursor::ResizeColumn)
        .hover_group()
        .role("separator")
        .label_static("Drag to resize")
        .on_drag(|s: &mut Shell, d, tx| s.divider_drag(d, tx))
        .child(rune::el().w(1.0).h_full().fill(t.edge));
    Some(
        rune::row()
            .key("side")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .on_layout_tx(|s: &mut Shell, rect, tx| {
                s.sides.rect = Some(rect);
                tx.live_only();
            })
            // The left side's share of what the divider leaves.
            .child(side(s, left, t, cx, active == Some(left.id)).w_frac(pair.share).shrink())
            .child(divider)
            .child(side(s, right, t, cx, active == Some(right.id)).grow(1.0)),
    )
}
