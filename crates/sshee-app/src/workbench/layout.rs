//! Two tabs side by side, and where things land in the row of tabs: the rules, without a
//! screen.
//!
//! Side by side is 0.10's "pair" (`placeBeside`, `toOwnTab` and `toggleSplit` in
//! `src/lib/tabs.ts`), as a browser shows two pages next to each other: two tabs of one machine
//! share the page, a divider between them, and each stays a tab of its own in the row. Leaving
//! the pair never closes a tab. 0.10's tree of splits (`layout.ts`: any number of panes, in rows
//! and columns) isn't here: one pair per machine is what the 1.0 design keeps.
//!
//! The row's own rules are here too, because they are arithmetic on widths: which place a
//! dragged tab lands in ([`slot`]), which tabs are in sight when the row can't hold them all
//! ([`in_sight`]), and what "close the others" and "close to the right" close.

use super::{TabId, TabKind, Workbench};

/// The least of the page either tab of a pair keeps, as a share of its width.
pub const MIN_SHARE: f32 = 0.2;

/// Two tabs shown next to each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    pub left: TabId,
    pub right: TabId,
    /// The left tab's share of the page's width, between [`MIN_SHARE`] and its complement.
    pub share: f32,
}

impl Pair {
    pub fn has(&self, id: TabId) -> bool {
        self.left == id || self.right == id
    }

    /// The other tab of the pair.
    pub fn other(&self, id: TabId) -> Option<TabId> {
        match id {
            _ if id == self.left => Some(self.right),
            _ if id == self.right => Some(self.left),
            _ => None,
        }
    }
}

/// What the shortcut for side by side did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggled {
    /// The tab on screen and this one are a pair now.
    Paired(TabId),
    /// The pair is no more; both tabs stay open.
    Apart,
    /// The machine on screen has no other tab to show beside this one: open a terminal there
    /// and call [`Layout::pair`] with it (0.10's "Split right").
    NeedsTab,
    /// Nothing is on screen to pair.
    Nothing,
}

/// Which tabs are shown side by side. A tab is in one pair at most.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pairs: Vec<Pair>,
}

impl Layout {
    pub fn new() -> Layout {
        Layout::default()
    }

    /// The pair `id` is in.
    pub fn pair_of(&self, id: TabId) -> Option<Pair> {
        self.pairs.iter().copied().find(|p| p.has(id))
    }

    /// The tab shown beside `id`.
    pub fn partner(&self, id: TabId) -> Option<TabId> {
        self.pair_of(id).and_then(|p| p.other(id))
    }

    /// Whether `beside` may be shown next to `anchor`: two different open tabs of one machine.
    pub fn can_pair(bench: &Workbench, anchor: TabId, beside: TabId) -> bool {
        match (bench.tab(anchor), bench.tab(beside)) {
            (Some(a), Some(b)) => a.id != b.id && a.server == b.server,
            _ => false,
        }
    }

    /// Shows `beside` to the right of `anchor`, half the page each. Pairs either was in are
    /// undone first. False when they can't be a pair ([`Layout::can_pair`]).
    pub fn pair(&mut self, bench: &Workbench, anchor: TabId, beside: TabId) -> bool {
        if !Layout::can_pair(bench, anchor, beside) {
            return false;
        }
        self.pairs.retain(|p| !p.has(anchor) && !p.has(beside));
        self.pairs.push(Pair { left: anchor, right: beside, share: 0.5 });
        true
    }

    /// Takes `id` out of its pair (a pane's ×): the pair is no more, and both stay open as tabs.
    /// Returns the tab that was beside it, which is the one to show now.
    pub fn leave(&mut self, id: TabId) -> Option<TabId> {
        let other = self.partner(id)?;
        self.pairs.retain(|p| !p.has(id));
        Some(other)
    }

    /// The divider was dragged: the left tab of `id`'s pair takes `share` of the width, as far
    /// as [`MIN_SHARE`] lets either side shrink.
    pub fn set_share(&mut self, id: TabId, share: f32) {
        if let Some(p) = self.pairs.iter_mut().find(|p| p.has(id)) {
            p.share = if share.is_finite() { share.clamp(MIN_SHARE, 1.0 - MIN_SHARE) } else { 0.5 };
        }
    }

    /// Drops the pairs with a tab that closed, or whose tabs are no longer on one machine.
    pub fn prune(&mut self, bench: &Workbench) {
        self.pairs.retain(|p| Layout::can_pair(bench, p.left, p.right));
    }

    /// The tab the shortcut would put beside the tab on screen: the next tab of its machine in
    /// the row, else the one before it. Terminals first: two terminals side by side is what
    /// the pair is mostly for.
    pub fn candidate(bench: &Workbench, id: TabId) -> Option<TabId> {
        let tab = bench.tab(id)?;
        let row: Vec<_> = bench.tabs_on(&tab.server).collect();
        let at = row.iter().position(|t| t.id == id)?;
        let near = row[at + 1..].iter().chain(row[..at].iter().rev());
        let terminal = near.clone().find(|t| t.kind == TabKind::Terminal);
        terminal.or(near.clone().next()).map(|t| t.id)
    }

    /// The shortcut (0.10's "Tabs side by side, or one at a time"): apart when the tab on
    /// screen is in a pair, else paired with its neighbour.
    pub fn toggle(&mut self, bench: &Workbench) -> Toggled {
        let Some(id) = bench.active_id() else { return Toggled::Nothing };
        if self.leave(id).is_some() {
            return Toggled::Apart;
        }
        match Layout::candidate(bench, id) {
            Some(other) if self.pair(bench, id, other) => Toggled::Paired(other),
            _ => Toggled::NeedsTab,
        }
    }
}

/// The tabs "Close other tabs" closes: every tab of the row but `id`.
pub fn others(bench: &Workbench, id: TabId) -> Vec<TabId> {
    bench.tabs().iter().map(|t| t.id).filter(|t| *t != id).collect()
}

/// The tabs "Close tabs to the right" closes: those after `id` in the row.
pub fn to_the_right(bench: &Workbench, id: TabId) -> Vec<TabId> {
    let after = bench.tabs().iter().skip_while(|t| t.id != id).skip(1);
    after.map(|t| t.id).collect()
}

/// The place a dragged tab lands in: `centres` are the middles of the other tabs along the
/// row, in order, and `x` is the pointer. It goes before the first tab whose middle is past the
/// pointer, else last (0.10's `reorder` in `Tabs.tsx`).
pub fn slot(centres: &[f32], x: f32) -> usize {
    centres.iter().position(|c| x < *c).unwrap_or(centres.len())
}

/// Which tabs are drawn when the row can't hold them all: `widths` are the tabs' widths as
/// drawn when crowded (each with what leads it: a divider, its machine's name), `room` is the
/// row's width for tabs, `first` is where the tabs in sight started last time, and `active` is
/// the tab on screen. The tabs in sight are a run of the row that holds the tab on screen and
/// moves as little as it can: the row doesn't jump when you click a tab that's already there.
/// At least one tab is always in sight.
pub fn in_sight(widths: &[f32], room: f32, first: usize, active: Option<usize>) -> std::ops::Range<usize> {
    let n = widths.len();
    if n == 0 {
        return 0..0;
    }
    // As many tabs as fit from `from` on.
    let run = |from: usize| {
        let mut used = 0.0;
        let mut end = from;
        while end < n && (end == from || used + widths[end] <= room) {
            used += widths[end];
            end += 1;
        }
        from..end
    };
    let mut from = first.min(n - 1);
    if let Some(active) = active.filter(|a| *a < n) {
        if active < from {
            from = active;
        }
        // Past the end of the run: start as late as still shows it.
        while !run(from).contains(&active) {
            from += 1;
        }
    }
    // Room left at the end after tabs closed: earlier tabs come back into sight.
    while from > 0 && run(from - 1).end == n && run(from).end == n {
        from -= 1;
    }
    run(from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::Server;

    const WEB: &str = "host:web";
    const DB: &str = "host:db";

    fn bench() -> Workbench {
        Workbench::new(vec![Server::host("web", "web-7731", "deploy@203.0.113.17"), Server::host("db", "db-main", "postgres@203.0.113.31")])
    }

    #[test]
    fn two_tabs_of_one_machine_pair_up_and_come_apart_without_closing() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let c = w.go_to(DB);
        let mut l = Layout::new();
        assert!(l.pair(&w, a, b));
        assert_eq!(l.pair_of(a), Some(Pair { left: a, right: b, share: 0.5 }));
        assert_eq!((l.partner(a), l.partner(b), l.partner(c)), (Some(b), Some(a), None));
        // Another machine's tab, the tab itself, a tab that isn't there: no pair.
        assert!(!l.pair(&w, a, c));
        assert!(!l.pair(&w, a, a));
        assert!(!l.pair(&w, a, TabId(99)));
        assert_eq!(l.partner(a), Some(b), "a refused pair must leave the old one alone");
        // The pane's ×: the pair is gone, the other tab is the one to show, both are open.
        assert_eq!(l.leave(b), Some(a));
        assert_eq!((l.pair_of(a), l.pair_of(b)), (None, None));
        assert_eq!(l.leave(b), None);
        assert_eq!(w.tabs().len(), 3);
    }

    #[test]
    fn a_tab_is_in_one_pair_at_most() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let c = w.new_terminal();
        let mut l = Layout::new();
        assert!(l.pair(&w, a, b));
        // c beside a: a's old pair goes, b is alone again.
        assert!(l.pair(&w, a, c));
        assert_eq!((l.partner(a), l.partner(b), l.partner(c)), (Some(c), None, Some(a)));
    }

    #[test]
    fn closing_a_tab_ends_its_pair() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let mut l = Layout::new();
        l.pair(&w, a, b);
        w.close(b);
        l.prune(&w);
        assert_eq!(l.pair_of(a), None);
    }

    #[test]
    fn the_divider_stops_before_either_side_is_too_narrow() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let mut l = Layout::new();
        l.pair(&w, a, b);
        l.set_share(b, 0.7);
        assert_eq!(l.pair_of(a).unwrap().share, 0.7);
        l.set_share(a, 0.01);
        assert_eq!(l.pair_of(a).unwrap().share, MIN_SHARE);
        l.set_share(a, 3.0);
        assert_eq!(l.pair_of(a).unwrap().share, 1.0 - MIN_SHARE);
        l.set_share(a, f32::NAN);
        assert_eq!(l.pair_of(a).unwrap().share, 0.5);
    }

    #[test]
    fn the_shortcut_pairs_with_the_neighbour_and_takes_it_apart_again() {
        let mut w = bench();
        let mut l = Layout::new();
        assert_eq!(l.toggle(&w), Toggled::Nothing, "the main menu has nothing to pair");
        let a = w.go_to(WEB);
        // One tab on the machine: the window opens a terminal for it.
        assert_eq!(l.toggle(&w), Toggled::NeedsTab);
        let files = w.open(TabKind::Files, WEB);
        let b = w.open(TabKind::Terminal, WEB);
        w.go_to(DB);
        w.focus(a);
        // The next terminal of this machine, not Files in between, and never db's tab.
        assert_eq!(Layout::candidate(&w, a), Some(b));
        assert_eq!(l.toggle(&w), Toggled::Paired(b));
        assert_eq!(l.toggle(&w), Toggled::Apart);
        assert_eq!(l.pair_of(a), None);
        // From the last tab of the machine: the one before it. With no other terminal, Files.
        w.focus(b);
        assert_eq!(Layout::candidate(&w, b), Some(a));
        w.close(a);
        assert_eq!(Layout::candidate(&w, b), Some(files));
    }

    #[test]
    fn close_others_and_close_to_the_right() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let c = w.go_to(DB);
        assert_eq!(others(&w, b), [a, c]);
        assert_eq!(to_the_right(&w, a), [b, c]);
        assert_eq!(to_the_right(&w, c), []);
        assert_eq!(to_the_right(&w, TabId(99)), []);
    }

    #[test]
    fn a_dragged_tab_lands_before_the_first_tab_whose_middle_is_past_the_pointer() {
        let centres = [50.0, 150.0, 250.0];
        assert_eq!(slot(&centres, 10.0), 0);
        assert_eq!(slot(&centres, 50.0), 1);
        assert_eq!(slot(&centres, 149.0), 1);
        assert_eq!(slot(&centres, 251.0), 3);
        assert_eq!(slot(&[], 5.0), 0);
    }

    #[test]
    fn the_tabs_in_sight_hold_the_tab_on_screen_and_move_as_little_as_they_can() {
        let w = [100.0; 10];
        // Room for four.
        assert_eq!(in_sight(&w, 400.0, 0, Some(0)), 0..4);
        assert_eq!(in_sight(&w, 400.0, 0, Some(3)), 0..4, "a tab in sight doesn't move the row");
        assert_eq!(in_sight(&w, 400.0, 0, Some(4)), 1..5, "the next one moves it by one");
        assert_eq!(in_sight(&w, 400.0, 1, Some(9)), 6..10);
        assert_eq!(in_sight(&w, 400.0, 6, Some(2)), 2..6);
        assert_eq!(in_sight(&w, 400.0, 6, None), 6..10);
        // Tabs closed at the end: the earlier ones come back rather than leave room unused.
        assert_eq!(in_sight(&w[..7], 400.0, 6, Some(6)), 3..7);
        // Everything fits, whatever was remembered.
        assert_eq!(in_sight(&w[..3], 400.0, 9, Some(1)), 0..3);
        // A tab wider than the row is still drawn; nothing at all is nothing.
        assert_eq!(in_sight(&[900.0, 50.0], 400.0, 0, Some(0)), 0..1);
        assert_eq!(in_sight(&[], 400.0, 3, Some(1)), 0..0);
        // Uneven widths.
        assert_eq!(in_sight(&[200.0, 50.0, 50.0, 300.0, 80.0], 400.0, 0, Some(3)), 1..4);
    }
}
