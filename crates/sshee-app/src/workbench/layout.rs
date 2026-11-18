//! Tabs shown together on the page, split right and down, and where things land in the row of
//! tabs: the rules, without a screen.
//!
//! A port of 0.10's tree of splits (`src/lib/layout.ts`, and `splitPane`, `toOwnTab`,
//! `toggleSplit`, `toggleZoom` in `tabs.ts` and `store.ts`), for tabs along the top: a
//! [`Group`] is a tree whose leaves are tabs, each split a row (side by side) or a column (one
//! above the other) of parts with their share of the room. Every pane stays a tab of its own in
//! the row; leaving a group never closes a tab, closing a tab gives its room back to the panes
//! around it. A tab is in one group at most, and a group holds two tabs or more.
//!
//! - Split right and Split down put a new terminal beside the tab you're on, inside its group
//!   (a split in the same direction stays flat: three columns, not a column inside a column).
//! - The shortcut for side by side shows a group's tabs one at a time and back again
//!   ([`Group::apart`]), the arrangement kept, as 0.10's `toggleSplit` did per machine; a tab
//!   in no group is paired with its neighbour.
//! - "Fill the window" shows one pane of a group alone until it's pressed again, or another
//!   pane of the group becomes the tab you're on ([`Group::zoomed`]).
//! - Panes of different machines may share the page, as 0.10's did when dragged there.
//!
//! The row's own rules are here too, because they are arithmetic on widths: which place a
//! dragged tab lands in ([`slot`]), which tabs are in sight when the row can't hold them all
//! ([`in_sight`]), and what "close the others" and "close to the right" close.

use serde::{Deserialize, Serialize};

use super::{TabId, TabKind, Workbench};

/// The least room a pane keeps along a split, in points (0.10's `minSize: 140`). A split too
/// small for that shares what it has evenly.
pub const MIN_PANE: f32 = 140.0;

/// Which way a split runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    /// Left to right: "Split right".
    Row,
    /// Top to bottom: "Split down".
    Col,
}

/// A place in a group: one tab, or a split of parts.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Pane(TabId),
    /// Parts in order along `dir`, each with its share of the room the dividers leave. The
    /// shares add up to 1; there are two parts or more, and none is a split the same way.
    Split { dir: Dir, parts: Vec<(Node, f32)> },
}

impl Node {
    fn two(dir: Dir, a: TabId, b: TabId) -> Node {
        Node::Split { dir, parts: vec![(Node::Pane(a), 0.5), (Node::Pane(b), 0.5)] }
    }

    /// The tabs in it, in reading order.
    pub fn panes(&self) -> Vec<TabId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<TabId>) {
        match self {
            Node::Pane(id) => out.push(*id),
            Node::Split { parts, .. } => parts.iter().for_each(|(n, _)| n.collect(out)),
        }
    }

    pub fn has(&self, id: TabId) -> bool {
        match self {
            Node::Pane(p) => *p == id,
            Node::Split { parts, .. } => parts.iter().any(|(n, _)| n.has(id)),
        }
    }

    /// The node at `path` (the place of each part on the way down).
    pub fn at(&self, path: &[usize]) -> Option<&Node> {
        match (path.split_first(), self) {
            (None, _) => Some(self),
            (Some((i, rest)), Node::Split { parts, .. }) => parts.get(*i)?.0.at(rest),
            (Some(_), Node::Pane(_)) => None,
        }
    }

    fn at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => match self {
                Node::Split { parts, .. } => parts.get_mut(*i)?.0.at_mut(rest),
                Node::Pane(_) => None,
            },
        }
    }

    /// Every pane `a` becomes `b` and every `b` becomes `a`.
    fn swap(&mut self, a: TabId, b: TabId) {
        match self {
            Node::Pane(id) if *id == a => *id = b,
            Node::Pane(id) if *id == b => *id = a,
            Node::Pane(_) => {}
            Node::Split { parts, .. } => parts.iter_mut().for_each(|(n, _)| n.swap(a, b)),
        }
    }

    /// Puts `new` beside `at`, after it or `before` it, in `dir`. True when `at` was found.
    fn insert(&mut self, at: TabId, new: TabId, dir: Dir, before: bool) -> bool {
        if *self == Node::Pane(at) {
            *self = if before { Node::two(dir, new, at) } else { Node::two(dir, at, new) };
            return true;
        }
        let Node::Split { dir: own, parts } = self else { return false };
        if let Some(i) = parts.iter().position(|(n, _)| *n == Node::Pane(at)) {
            if *own == dir {
                // The same way: one more part, taking half of the room `at` had.
                let half = parts[i].1 / 2.0;
                parts[i].1 = half;
                parts.insert(if before { i } else { i + 1 }, (Node::Pane(new), half));
            } else {
                parts[i].0 = if before { Node::two(dir, new, at) } else { Node::two(dir, at, new) };
            }
            return true;
        }
        parts.iter_mut().any(|(n, _)| n.insert(at, new, dir, before))
    }

    /// Takes `id` out. Returns the pane that is nearest it (the first of the part after it,
    /// else the last of the part before), when it was here. Its room goes to the parts left,
    /// in proportion; a split left with one part becomes that part.
    fn remove(&mut self, id: TabId) -> Option<Option<TabId>> {
        let Node::Split { parts, .. } = self else { return None };
        let found = match parts.iter().position(|(n, _)| *n == Node::Pane(id)) {
            Some(i) => {
                let near = parts.get(i + 1).map(|(n, _)| n.panes()[0]).or_else(|| i.checked_sub(1).and_then(|j| parts[j].0.panes().last().copied()));
                parts.remove(i);
                near
            }
            None => parts.iter_mut().find_map(|(n, _)| n.remove(id))?,
        };
        self.tidy();
        Some(found)
    }

    /// Shares that add up to 1, splits of one part made that part, and a split inside a split
    /// the same way made one.
    fn tidy(&mut self) {
        let Node::Split { dir, parts } = self else { return };
        let dir = *dir;
        let mut flat = Vec::with_capacity(parts.len());
        for (mut node, share) in std::mem::take(parts) {
            node.tidy();
            match node {
                Node::Split { dir: d, parts: inner } if d == dir => flat.extend(inner.into_iter().map(|(n, s)| (n, s * share))),
                node => flat.push((node, share)),
            }
        }
        normalize(&mut flat);
        if flat.len() == 1 {
            *self = flat.pop().unwrap().0;
        } else {
            *parts = flat;
        }
    }
}

/// Shares that are numbers above 0 and add up to 1; with none to go by, even.
fn normalize(parts: &mut [(Node, f32)]) {
    let sane = parts.iter().all(|(_, s)| s.is_finite() && *s > 0.0);
    let sum: f32 = parts.iter().map(|(_, s)| *s).sum();
    let n = parts.len() as f32;
    for (_, s) in parts.iter_mut() {
        *s = if sane && sum > 0.0 { *s / sum } else { 1.0 / n };
    }
}

/// Tabs shown together on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Always a split: a group holds two tabs or more.
    pub root: Node,
    /// The pane filling the window ("Fill the window"), while one does.
    pub zoomed: Option<TabId>,
    /// Shown one at a time for now (the shortcut for side by side); the arrangement is kept.
    pub apart: bool,
}

impl Group {
    pub fn panes(&self) -> Vec<TabId> {
        self.root.panes()
    }
}

/// What the shortcut for side by side did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggled {
    /// The tab on screen and this one are side by side now.
    Paired(TabId),
    /// The group's tabs show one at a time; all stay open, the arrangement is kept.
    Apart,
    /// The group the tab on screen is in shows its panes together again.
    Together,
    /// The tab on screen has no other tab to show beside it: open a terminal and
    /// [`Layout::split`] with it (0.10's "Split right").
    NeedsTab,
    /// Nothing is on screen.
    Nothing,
}

/// Which tabs share the page, and how.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    groups: Vec<Group>,
}

impl Layout {
    pub fn new() -> Layout {
        Layout::default()
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.groups.iter().position(|g| g.root.has(id))
    }

    /// The group `id` is in, shown together or apart.
    pub fn group_of(&self, id: TabId) -> Option<&Group> {
        self.index_of(id).map(|i| &self.groups[i])
    }

    /// The group whose panes share the page while `id` is the tab on screen.
    pub fn shown_with(&self, id: TabId) -> Option<&Group> {
        self.group_of(id).filter(|g| !g.apart)
    }

    /// The tabs drawn while `id` is the tab on screen: its group's panes (only the one filling
    /// the window, while one does), else `id` alone.
    pub fn on_screen(&self, id: TabId) -> Vec<TabId> {
        match self.shown_with(id) {
            Some(Group { zoomed: Some(z), .. }) => vec![*z],
            Some(g) => g.panes(),
            None => vec![id],
        }
    }

    /// Whether `beside` may be shown with `anchor`: two different open tabs.
    pub fn can_pair(bench: &Workbench, anchor: TabId, beside: TabId) -> bool {
        anchor != beside && bench.tab(anchor).is_some() && bench.tab(beside).is_some()
    }

    /// Puts `new` beside `at` (after it, or `before` it) in `dir`, in `at`'s group or a new one
    /// with the two of them. `new` leaves the group it was in; `at`'s group shows together.
    /// False for a tab and itself. (That both are open tabs is the caller's to know:
    /// [`Layout::can_pair`].)
    pub fn split(&mut self, at: TabId, new: TabId, dir: Dir, before: bool) -> bool {
        if at == new {
            return false;
        }
        self.remove(new);
        match self.index_of(at) {
            Some(i) => {
                let g = &mut self.groups[i];
                g.root.insert(at, new, dir, before);
                g.root.tidy();
                g.apart = false;
                g.zoomed = None;
            }
            None => {
                let root = if before { Node::two(dir, new, at) } else { Node::two(dir, at, new) };
                self.groups.push(Group { root, zoomed: None, apart: false });
            }
        }
        true
    }

    /// Takes `id` out of its group (a pane's ×, "Back to its own tab"): it stays open as a tab,
    /// and its room goes to the panes around it. Returns the pane nearest it, the one to show
    /// now; a group left with one tab is no more.
    pub fn remove(&mut self, id: TabId) -> Option<TabId> {
        let i = self.index_of(id)?;
        let g = &mut self.groups[i];
        let near = match &mut g.root {
            Node::Pane(_) => None,
            root => root.remove(id).flatten(),
        };
        if g.zoomed == Some(id) {
            g.zoomed = None;
        }
        if matches!(g.root, Node::Pane(_)) {
            self.groups.remove(i);
        }
        near
    }

    /// Drops the tabs that closed.
    pub fn prune(&mut self, open: impl Fn(TabId) -> bool) {
        let gone: Vec<TabId> = self.groups.iter().flat_map(Group::panes).filter(|id| !open(*id)).collect();
        for id in gone {
            self.remove(id);
        }
    }

    /// "Fill the window": `id` alone on the page, or its group's panes together again. False
    /// when `id` doesn't share the page with others.
    pub fn zoom(&mut self, id: TabId) -> bool {
        let Some(g) = self.index_of(id).map(|i| &mut self.groups[i]).filter(|g| !g.apart) else { return false };
        g.zoomed = if g.zoomed.is_some() { None } else { Some(id) };
        true
    }

    /// `id` became the tab on screen: a group whose other pane filled the window shows all of
    /// them again (as 0.10's `focusing`).
    pub fn landed(&mut self, id: TabId) {
        if let Some(g) = self.index_of(id).map(|i| &mut self.groups[i]) {
            if g.zoomed.is_some_and(|z| z != id) {
                g.zoomed = None;
            }
        }
    }

    /// A divider was dragged. `id` is any pane of the group, `path` the split's place in it,
    /// `i` the divider (between parts `i` and `i + 1`), `at` where the pointer is along the
    /// split as a share of the room its dividers leave (`room`, in points). The two parts
    /// beside the divider share what they had; neither gets less than [`MIN_PANE`].
    pub fn drag(&mut self, id: TabId, path: &[usize], i: usize, at: f32, room: f32) {
        let Some(g) = self.index_of(id).map(|g| &mut self.groups[g]) else { return };
        let Some(Node::Split { parts, .. }) = g.root.at_mut(path) else { return };
        if i + 1 >= parts.len() || !at.is_finite() {
            return;
        }
        let before: f32 = parts[..i].iter().map(|(_, s)| *s).sum();
        let both = parts[i].1 + parts[i + 1].1;
        let least = if room > 0.0 { (MIN_PANE / room).min(both / 2.0) } else { both / 2.0 };
        let first = (at - before).clamp(least, both - least);
        parts[i].1 = first;
        parts[i + 1].1 = both - first;
    }

    /// A divider's double click: the parts of that split share its room evenly.
    pub fn even_out(&mut self, id: TabId, path: &[usize]) {
        let Some(g) = self.index_of(id).map(|g| &mut self.groups[g]) else { return };
        if let Some(Node::Split { parts, .. }) = g.root.at_mut(path) {
            let n = parts.len() as f32;
            parts.iter_mut().for_each(|(_, s)| *s = 1.0 / n);
        }
    }

    /// The tab the shortcut would put beside `id`: the next tab of its machine in the row, else
    /// the one before it. Terminals first: two terminals side by side is what it's mostly for.
    pub fn candidate(bench: &Workbench, id: TabId) -> Option<TabId> {
        let tab = bench.tab(id)?;
        let row: Vec<_> = bench.tabs_on(&tab.server).collect();
        let at = row.iter().position(|t| t.id == id)?;
        let near = row[at + 1..].iter().chain(row[..at].iter().rev());
        let terminal = near.clone().find(|t| t.kind == TabKind::Terminal);
        terminal.or(near.clone().next()).map(|t| t.id)
    }

    /// The shortcut (0.10's "Tabs side by side, or one at a time") for `active`, the tab on
    /// screen: a group shows its panes one at a time and back; a tab in none is paired with
    /// `candidate` ([`Layout::candidate`]).
    pub fn toggle(&mut self, active: Option<TabId>, candidate: Option<TabId>) -> Toggled {
        let Some(id) = active else { return Toggled::Nothing };
        if let Some(i) = self.index_of(id) {
            let g = &mut self.groups[i];
            g.apart = !g.apart;
            g.zoomed = None;
            return if g.apart { Toggled::Apart } else { Toggled::Together };
        }
        match candidate {
            Some(other) if self.split(id, other, Dir::Row, false) => Toggled::Paired(other),
            _ => Toggled::NeedsTab,
        }
    }

    /// A tab (or a pane's header) dropped on pane `target` at `zone` (0.10's `dropAt` and
    /// `movePane`): beside it on that side; on its middle, it takes `target`'s place in its
    /// group (the two swap places when both are in it), or, when `target` is alone, it goes to
    /// its right. False for a tab dropped on itself.
    pub fn place(&mut self, target: TabId, dragged: TabId, zone: Zone) -> bool {
        if target == dragged {
            return false;
        }
        let (dir, before) = match zone {
            Zone::Left => (Dir::Row, true),
            Zone::Right => (Dir::Row, false),
            Zone::Top => (Dir::Col, true),
            Zone::Bottom => (Dir::Col, false),
            Zone::Middle => {
                let Some(i) = self.index_of(target) else { return self.split(target, dragged, Dir::Row, false) };
                if self.groups[i].root.has(dragged) {
                    self.groups[i].root.swap(target, dragged);
                    return true;
                }
                self.remove(dragged);
                let Some(i) = self.index_of(target) else { return self.split(target, dragged, Dir::Row, false) };
                let g = &mut self.groups[i];
                // `dragged` is in no group now: renaming `target`'s leaf puts it there.
                g.root.swap(target, dragged);
                g.zoomed = None;
                g.apart = false;
                return true;
            }
        };
        self.split(target, dragged, dir, before)
    }

    /// Rebuilds the group of `tabs` in one of the preset shapes (0.10's `arrange`): `tabs` in
    /// order, the first the big one of "Main + stack". The group any of them was in is
    /// replaced; fewer than two tabs make no group.
    pub fn arrange(&mut self, tabs: &[TabId], how: Arrangement) {
        for id in tabs {
            self.remove(*id);
        }
        if tabs.len() < 2 {
            return;
        }
        let even = |dir: Dir, ids: &[TabId]| -> Node {
            match ids {
                [one] => Node::Pane(*one),
                _ => Node::Split { dir, parts: ids.iter().map(|id| (Node::Pane(*id), 1.0 / ids.len() as f32)).collect() },
            }
        };
        let root = match how {
            Arrangement::Columns => even(Dir::Row, tabs),
            Arrangement::Rows => even(Dir::Col, tabs),
            Arrangement::Main => Node::Split { dir: Dir::Row, parts: vec![(Node::Pane(tabs[0]), 0.5), (even(Dir::Col, &tabs[1..]), 0.5)] },
            Arrangement::Grid => {
                let cols = (tabs.len() as f32).sqrt().ceil() as usize;
                let rows: Vec<Node> = tabs.chunks(cols).map(|r| even(Dir::Row, r)).collect();
                match rows.len() {
                    1 => rows.into_iter().next().expect("one row"),
                    n => Node::Split { dir: Dir::Col, parts: rows.into_iter().map(|r| (r, 1.0 / n as f32)).collect() },
                }
            }
        };
        self.groups.push(Group { root, zoomed: None, apart: false });
    }

    /// Every group shows its panes one at a time (`true`) or together: Settings' "Tabs on
    /// screen".
    pub fn set_apart(&mut self, apart: bool) {
        for g in &mut self.groups {
            g.apart = apart;
            g.zoomed = None;
        }
    }

    /// The groups as they're kept between runs, each tab by its place in `order` (the row).
    pub fn saved(&self, order: &[TabId]) -> Vec<SavedSplit> {
        fn node(n: &Node, order: &[TabId]) -> Option<SavedNode> {
            Some(match n {
                Node::Pane(id) => SavedNode::Pane { tab: order.iter().position(|t| t == id)? },
                Node::Split { dir, parts } => SavedNode::Split {
                    dir: *dir,
                    parts: parts.iter().map(|(n, s)| Some(SavedPart { node: node(n, order)?, share: *s })).collect::<Option<_>>()?,
                },
            })
        }
        self.groups
            .iter()
            .filter_map(|g| {
                Some(SavedSplit {
                    root: node(&g.root, order)?,
                    zoomed: g.zoomed.and_then(|z| order.iter().position(|t| *t == z)),
                    apart: g.apart,
                })
            })
            .collect()
    }

    /// Brings the groups back: `tab_at(i)` is the tab now at place `i` of the saved row, if it
    /// came back. A tab that didn't, or that a group already holds, is left out; shares that
    /// don't read are made even, and a group of fewer than two tabs isn't one.
    pub fn restore(saved: &[SavedSplit], tab_at: impl Fn(usize) -> Option<TabId>) -> Layout {
        fn node(n: &SavedNode, tab_at: &dyn Fn(usize) -> Option<TabId>, seen: &mut Vec<TabId>) -> Option<Node> {
            match n {
                SavedNode::Pane { tab } => {
                    let id = tab_at(*tab).filter(|id| !seen.contains(id))?;
                    seen.push(id);
                    Some(Node::Pane(id))
                }
                SavedNode::Split { dir, parts } => {
                    let parts: Vec<_> = parts.iter().filter_map(|p| Some((node(&p.node, tab_at, seen)?, p.share))).collect();
                    match parts.len() {
                        0 => None,
                        1 => parts.into_iter().next().map(|(n, _)| n),
                        _ => Some(Node::Split { dir: *dir, parts }),
                    }
                }
            }
        }
        let mut seen = Vec::new();
        let mut groups = Vec::new();
        for s in saved {
            let Some(mut root) = node(&s.root, &tab_at, &mut seen) else { continue };
            root.tidy();
            if matches!(root, Node::Pane(_)) {
                continue;
            }
            let zoomed = s.zoomed.and_then(&tab_at).filter(|z| root.has(*z));
            groups.push(Group { root, zoomed, apart: s.apart });
        }
        Layout { groups }
    }
}

/// Where on a pane a dragged tab lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    /// Its middle: in its place.
    Middle,
}

/// The zone of a pane drawn at `[x, y, w, h]` that the pointer at `(px, py)` is over, or `None`
/// when it's outside: a quarter of the width at either side, 30 % of the height at the top
/// and the bottom, the middle (0.10's `dropAt`).
pub fn zone([x, y, w, h]: [f32; 4], (px, py): (f32, f32)) -> Option<Zone> {
    if w <= 0.0 || h <= 0.0 || px < x || py < y || px > x + w || py > y + h {
        return None;
    }
    let (fx, fy) = ((px - x) / w, (py - y) / h);
    Some(if fx < 0.25 {
        Zone::Left
    } else if fx > 0.75 {
        Zone::Right
    } else if fy < 0.3 {
        Zone::Top
    } else if fy > 0.7 {
        Zone::Bottom
    } else {
        Zone::Middle
    })
}

/// The side of a page drawn at `[x, y, w, h]` that the pointer at `(px, py)` is over, as
/// [`zone`] says, but never the middle: there, the nearest edge. The tab on screen dragged onto
/// its own page splits it with a neighbour, so it always goes to a side.
pub fn side(rect: [f32; 4], (px, py): (f32, f32)) -> Option<Zone> {
    let z = zone(rect, (px, py))?;
    if z != Zone::Middle {
        return Some(z);
    }
    let [x, y, w, h] = rect;
    let (fx, fy) = ((px - x) / w, (py - y) / h);
    let near = [(fx, Zone::Left), (1.0 - fx, Zone::Right), (fy, Zone::Top), (1.0 - fy, Zone::Bottom)];
    near.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, z)| z)
}

/// The preset shapes of Settings' "Arrange open panes" (0.10's `Arrangement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    /// The first big at the left, the others stacked at the right.
    Main,
    Columns,
    Rows,
    Grid,
}

/// The sidebar: icons only, or names this wide.
pub mod sidebar {
    /// The rail of icons' width.
    pub const RAIL: f32 = 52.0;
    /// Dragged narrower than this, the list folds into the rail (0.10's `FOLD_AT`).
    pub const FOLD_AT: f32 = 120.0;
    /// The list's narrowest, widest and first width.
    pub const MIN: f32 = 150.0;
    pub const MAX: f32 = 420.0;
    pub const DEFAULT: f32 = 232.0;

    /// The list's width as kept (anything odd is the default).
    pub fn width(kept: f64) -> f32 {
        let w = kept as f32;
        if w.is_finite() && w > 0.0 {
            w.clamp(MIN, MAX)
        } else {
            DEFAULT
        }
    }

    /// The sidebar's edge dragged to make it `want` points wide: `None` folds it into the rail
    /// of icons, else the list is that wide, within its least and most.
    pub fn dragged(want: f32) -> Option<f32> {
        (want.is_finite() && want >= FOLD_AT).then(|| want.clamp(MIN, MAX))
    }
}

/// A group as it's kept between runs (in `tabs.json`, with the tabs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSplit {
    pub root: SavedNode,
    /// The place in the row of the pane filling the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoomed: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub apart: bool,
}

/// A place in a saved group: a tab by its place in the row, or a split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SavedNode {
    Pane { tab: usize },
    Split { dir: Dir, parts: Vec<SavedPart> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedPart {
    pub node: SavedNode,
    pub share: f32,
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

    /// The tree as text: `a` for TabId(1), `b` for 2, ...; `r(..)` a row, `c(..)` a column.
    fn shape(n: &Node) -> String {
        match n {
            Node::Pane(id) => ((b'a' + id.0 as u8 - 1) as char).to_string(),
            Node::Split { dir, parts } => {
                let inner: Vec<String> = parts.iter().map(|(n, _)| shape(n)).collect();
                format!("{}({})", if *dir == Dir::Row { "r" } else { "c" }, inner.join(" "))
            }
        }
    }

    fn shares(n: &Node) -> Vec<f32> {
        match n {
            Node::Split { parts, .. } => parts.iter().map(|(_, s)| (s * 1000.0).round() / 1000.0).collect(),
            Node::Pane(_) => vec![],
        }
    }

    /// Four terminals on web: a b c d.
    fn four() -> (Workbench, [TabId; 4]) {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.new_terminal();
        let c = w.new_terminal();
        let d = w.new_terminal();
        (w, [a, b, c, d])
    }

    #[test]
    fn splits_in_one_direction_stay_flat_and_across_they_nest() {
        let (w, [a, b, c, d]) = four();
        let mut l = Layout::new();
        assert!(l.split(a, b, Dir::Row, false));
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(a b)");
        // Right of b: a third column, b's room halved.
        assert!(l.split(b, c, Dir::Row, false));
        let g = l.group_of(c).unwrap();
        assert_eq!(shape(&g.root), "r(a b c)");
        assert_eq!(shares(&g.root), [0.5, 0.25, 0.25]);
        // Down from a: a column in a's place, with a's share.
        assert!(l.split(a, d, Dir::Col, false));
        let g = l.group_of(d).unwrap();
        assert_eq!(shape(&g.root), "r(c(a d) b c)");
        assert_eq!(shares(&g.root), [0.5, 0.25, 0.25]);
        assert_eq!(g.panes(), [a, d, b, c]);
        assert_eq!(l.groups().len(), 1);
        // A tab and itself: nothing.
        assert!(!l.split(a, a, Dir::Row, false));
        assert!(Layout::can_pair(&w, a, b) && !Layout::can_pair(&w, a, a) && !Layout::can_pair(&w, a, TabId(99)));
    }

    #[test]
    fn a_split_before_puts_the_new_pane_first() {
        let (_, [a, b, c, _]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, true);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(b a)");
        l.split(a, c, Dir::Col, true);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(b c(c a))");
    }

    #[test]
    fn closing_a_pane_gives_its_room_back_and_names_the_pane_nearest() {
        let (w, [a, b, c, d]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.split(b, c, Dir::Row, false);
        l.split(c, d, Dir::Col, false);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(a b c(c d))");
        // b goes: the part after it is nearest; a and the column grow in proportion.
        assert_eq!(l.remove(b), Some(c));
        let g = l.group_of(a).unwrap();
        assert_eq!(shape(&g.root), "r(a c(c d))");
        assert_eq!(shares(&g.root), [0.667, 0.333]);
        // The last of a column: the one above it.
        assert_eq!(l.remove(d), Some(c));
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(a c)");
        // Two left, one goes: there's no group any more, and both are still tabs.
        assert_eq!(l.remove(a), Some(c));
        assert!(l.groups().is_empty());
        assert_eq!(l.remove(a), None);
        assert_eq!(w.tabs().len(), 4);
    }

    #[test]
    fn a_column_left_with_one_pane_melts_into_its_row() {
        let (_, [a, b, c, d]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.split(b, c, Dir::Col, false);
        l.split(c, d, Dir::Row, false);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(a c(b r(c d)))");
        l.remove(b);
        let g = l.group_of(a).unwrap();
        assert_eq!(shape(&g.root), "r(a c d)");
        assert_eq!(shares(&g.root), [0.5, 0.25, 0.25]);
    }

    #[test]
    fn a_tab_is_in_one_group_at_most() {
        let (_, [a, b, c, d]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.split(c, d, Dir::Row, false);
        assert_eq!(l.groups().len(), 2);
        // b goes beside c: it leaves a's group, which is no more.
        l.split(c, b, Dir::Col, false);
        assert_eq!(l.groups().len(), 1);
        assert_eq!(l.group_of(a), None);
        assert_eq!(shape(&l.group_of(b).unwrap().root), "r(c(c b) d)");
    }

    #[test]
    fn closed_tabs_leave_their_groups() {
        let (mut w, [a, b, c, _]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.split(b, c, Dir::Row, false);
        w.close(b);
        l.prune(|id| w.tab(id).is_some());
        assert_eq!(l.group_of(a).unwrap().panes(), [a, c]);
        w.close(c);
        l.prune(|id| w.tab(id).is_some());
        assert!(l.groups().is_empty());
    }

    #[test]
    fn panes_of_two_machines_may_share_the_page() {
        let mut w = bench();
        let a = w.go_to(WEB);
        let b = w.go_to(DB);
        let mut l = Layout::new();
        assert!(l.split(a, b, Dir::Row, false));
        assert_eq!(l.on_screen(a), [a, b]);
    }

    #[test]
    fn a_divider_moves_as_far_as_the_panes_beside_it_let_it() {
        let (_, [a, b, c, _]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.split(b, c, Dir::Row, false);
        // 1000 points of room: the divider between b and c, to 0.7.
        l.drag(a, &[], 1, 0.7, 1000.0);
        assert_eq!(shares(&l.group_of(a).unwrap().root), [0.5, 0.2, 0.3]);
        // Not closer than 140 points to either neighbour's far side.
        l.drag(a, &[], 1, 0.51, 1000.0);
        assert_eq!(shares(&l.group_of(a).unwrap().root), [0.5, 0.14, 0.36]);
        l.drag(a, &[], 0, 0.99, 1000.0);
        assert_eq!(shares(&l.group_of(a).unwrap().root), [0.5, 0.14, 0.36], "a and b had 0.64: b keeps its 0.14");
        // A split too small for two panes of 140: the two share what they have.
        l.drag(a, &[], 0, 0.0, 200.0);
        assert_eq!(shares(&l.group_of(a).unwrap().root), [0.32, 0.32, 0.36]);
        // Nothing that isn't a number, or a divider that isn't there.
        l.drag(a, &[], 0, f32::NAN, 1000.0);
        l.drag(a, &[], 5, 0.3, 1000.0);
        l.drag(a, &[4], 0, 0.3, 1000.0);
        assert_eq!(shares(&l.group_of(a).unwrap().root), [0.32, 0.32, 0.36]);
        l.even_out(a, &[]);
        assert_eq!(shares(&l.group_of(a).unwrap().root), [0.333, 0.333, 0.333]);
    }

    #[test]
    fn a_nested_divider_is_found_by_its_path() {
        let (_, [a, b, c, _]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.split(b, c, Dir::Col, false);
        l.drag(a, &[1], 0, 0.7, 800.0);
        let g = l.group_of(a).unwrap();
        assert_eq!(shares(g.root.at(&[1]).unwrap()), [0.7, 0.3]);
        assert_eq!(shares(&g.root), [0.5, 0.5]);
    }

    #[test]
    fn a_pane_fills_the_window_until_pressed_again_or_another_is_chosen() {
        let (_, [a, b, c, d]) = four();
        let mut l = Layout::new();
        assert!(!l.zoom(a), "alone, a tab fills the window already");
        l.split(a, b, Dir::Row, false);
        l.split(b, c, Dir::Row, false);
        assert!(l.zoom(b));
        assert_eq!(l.on_screen(a), [b]);
        assert!(l.zoom(b));
        assert_eq!(l.on_screen(a), [a, b, c]);
        l.zoom(b);
        // Landing on b keeps it; on another pane of its group shows them all.
        l.landed(b);
        assert_eq!(l.on_screen(b), [b]);
        l.landed(d);
        assert_eq!(l.on_screen(b), [b]);
        l.landed(c);
        assert_eq!(l.on_screen(b), [a, b, c]);
        // The pane filling the window leaves: the rest show.
        l.zoom(c);
        l.remove(c);
        assert_eq!(l.on_screen(a), [a, b]);
        // A split ends filling the window.
        l.zoom(a);
        l.split(a, d, Dir::Col, false);
        assert_eq!(l.on_screen(a), [a, d, b]);
    }

    #[test]
    fn the_shortcut_pairs_with_the_neighbour_then_shows_one_at_a_time_and_back() {
        let mut w = bench();
        assert_eq!(w.side_by_side(), Toggled::Nothing, "the main menu has nothing to pair");
        let a = w.go_to(WEB);
        // One tab on the machine: a terminal opens beside it, and you stay where you were.
        assert_eq!(w.side_by_side(), Toggled::NeedsTab);
        let b = w.tabs()[1].id;
        assert_eq!((w.active_id(), w.on_screen()), (Some(a), vec![a, b]));
        w.side_by_side();
        w.leave_split(b);
        assert_eq!(w.on_screen(), [a]);
        let files = w.open(TabKind::Files, WEB);
        w.go_to(DB);
        w.focus(a);
        // The next terminal of this machine, not Files in between, and never db's tab.
        assert_eq!(Layout::candidate(&w, a), Some(b));
        assert_eq!(w.side_by_side(), Toggled::Paired(b));
        w.focus(b);
        assert!(w.show_beside(files, Dir::Col));
        assert_eq!(w.on_screen(), [a, b, files]);
        // One at a time: the arrangement is kept, and comes back.
        assert_eq!(w.side_by_side(), Toggled::Apart);
        assert_eq!(w.on_screen(), [b]);
        assert_eq!(w.side_by_side(), Toggled::Together);
        assert_eq!(w.on_screen(), [a, b, files]);
        // Split from a tab of a group shown apart: it shows together again.
        w.side_by_side();
        let c = w.split(Dir::Col).unwrap();
        assert_eq!((w.active_id(), w.on_screen()), (Some(c), vec![a, b, c, files]));
    }

    #[test]
    fn a_tab_dropped_on_a_pane_lands_on_that_side_or_in_its_place() {
        let (w, [a, b, c, d]) = four();
        let mut l = Layout::new();
        // Alone, its middle puts the tab at its right.
        assert!(l.place(a, b, Zone::Middle));
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(a b)");
        assert!(l.place(b, c, Zone::Bottom));
        assert!(l.place(a, d, Zone::Top));
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(c(d a) c(b c))");
        // Two of the group: they swap places.
        l.place(a, c, Zone::Middle);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(c(d c) c(b a))");
        // b moves to the left of d: out of its column, which melts, and into a row with d.
        l.place(d, b, Zone::Left);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(c(r(b d) c) a)");
        assert!(!l.place(a, a, Zone::Left));
        // A tab from outside dropped on a pane's middle takes its place; that one stays a tab.
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.place(b, c, Zone::Middle);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(a c)");
        assert_eq!(l.group_of(b), None);
        assert!(w.tab(b).is_some());
    }

    #[test]
    fn the_zones_of_a_pane_are_its_sides_and_its_middle() {
        let r = [100.0, 50.0, 400.0, 200.0];
        assert_eq!(zone(r, (110.0, 150.0)), Some(Zone::Left));
        assert_eq!(zone(r, (490.0, 150.0)), Some(Zone::Right));
        assert_eq!(zone(r, (300.0, 60.0)), Some(Zone::Top));
        assert_eq!(zone(r, (300.0, 240.0)), Some(Zone::Bottom));
        assert_eq!(zone(r, (300.0, 150.0)), Some(Zone::Middle));
        assert_eq!(zone(r, (50.0, 150.0)), None);
        assert_eq!(zone([0.0, 0.0, 0.0, 10.0], (0.0, 5.0)), None);
        // `side`: the same, with the middle given to its nearest edge.
        assert_eq!(side(r, (110.0, 150.0)), Some(Zone::Left));
        assert_eq!(side(r, (300.0, 240.0)), Some(Zone::Bottom));
        assert_eq!(side(r, (280.0, 150.0)), Some(Zone::Left));
        assert_eq!(side(r, (330.0, 150.0)), Some(Zone::Right));
        assert_eq!(side(r, (300.0, 120.0)), Some(Zone::Top));
        assert_eq!(side(r, (300.0, 180.0)), Some(Zone::Bottom));
        assert_eq!(side(r, (50.0, 150.0)), None);
    }

    #[test]
    fn open_panes_arrange_in_the_preset_shapes() {
        let (_, [a, b, c, d]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.arrange(&[c, a, b, d], Arrangement::Main);
        assert_eq!(l.groups().len(), 1);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(c c(a b d))");
        l.arrange(&[a, b, c, d], Arrangement::Columns);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "r(a b c d)");
        l.arrange(&[a, b, c], Arrangement::Rows);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "c(a b c)");
        assert_eq!(l.group_of(d), None);
        l.arrange(&[a, b, c, d], Arrangement::Grid);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "c(r(a b) r(c d))");
        l.arrange(&[a, b, c], Arrangement::Grid);
        assert_eq!(shape(&l.group_of(a).unwrap().root), "c(r(a b) c)");
        l.arrange(&[a], Arrangement::Grid);
        assert_eq!(l.group_of(a), None, "one tab is no group: it leaves the one it was in");
    }

    #[test]
    fn the_sidebar_folds_into_icons_when_dragged_narrow() {
        assert_eq!(sidebar::dragged(100.0), None);
        assert_eq!(sidebar::dragged(130.0), Some(sidebar::MIN));
        assert_eq!(sidebar::dragged(300.0), Some(300.0));
        assert_eq!(sidebar::dragged(900.0), Some(sidebar::MAX));
        assert_eq!(sidebar::dragged(f32::NAN), None);
        assert_eq!(sidebar::width(232.0), 232.0);
        assert_eq!(sidebar::width(10.0), sidebar::MIN);
        assert_eq!((sidebar::width(f64::NAN), sidebar::width(-3.0)), (sidebar::DEFAULT, sidebar::DEFAULT));
    }

    #[test]
    fn groups_are_kept_by_place_in_the_row_and_come_back() {
        let (_, [a, b, c, d]) = four();
        let mut l = Layout::new();
        l.split(a, b, Dir::Row, false);
        l.split(b, c, Dir::Col, false);
        l.drag(a, &[], 0, 0.3, 1000.0);
        l.zoom(c);
        let order = [d, a, b, c];
        let saved = l.saved(&order);
        let json = serde_json::to_string(&saved).unwrap();
        assert!(json.contains(r#""kind":"split""#) && json.contains(r#""dir":"col""#) && json.contains(r#""zoomed":3"#), "{json}");
        let back: Vec<SavedSplit> = serde_json::from_str(&json).unwrap();
        // The tabs came back with new ids, in the same places.
        let new = [TabId(11), TabId(12), TabId(13), TabId(14)];
        let again = Layout::restore(&back, |i| new.get(i).copied());
        let g = again.group_of(TabId(12)).unwrap();
        assert_eq!(g.panes(), [TabId(12), TabId(13), TabId(14)]);
        assert_eq!(shares(&g.root), [0.3, 0.7]);
        assert_eq!(g.zoomed, Some(TabId(14)));
        // A tab that didn't come back is left out, and its column with it.
        let again = Layout::restore(&back, |i| (i != 3).then(|| new[i]));
        let g = again.group_of(TabId(12)).unwrap();
        assert_eq!((g.panes(), g.zoomed), (vec![TabId(12), TabId(13)], None));
        // Only one came back: no group. Nothing saved: nothing.
        assert!(Layout::restore(&back, |i| (i == 1).then(|| new[1])).groups().is_empty());
        assert!(Layout::restore(&[], |_| None).groups().is_empty());
        // Shares that don't read are even; one tab twice counts once.
        let odd = vec![SavedSplit {
            root: SavedNode::Split {
                dir: Dir::Row,
                parts: vec![
                    SavedPart { node: SavedNode::Pane { tab: 0 }, share: f32::NAN },
                    SavedPart { node: SavedNode::Pane { tab: 1 }, share: 3.0 },
                    SavedPart { node: SavedNode::Pane { tab: 1 }, share: 3.0 },
                ],
            },
            zoomed: None,
            apart: true,
        }];
        let g = Layout::restore(&odd, |i| new.get(i).copied()).groups()[0].clone();
        assert_eq!((g.panes(), shares(&g.root), g.apart), (vec![TabId(11), TabId(12)], vec![0.5, 0.5], true));
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
