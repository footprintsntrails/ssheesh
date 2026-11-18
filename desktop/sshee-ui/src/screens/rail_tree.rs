//! A server's containers under its name in the sidebar's list (0.10's container tree in
//! `Sidebar.tsx`): the chevron that shows on the row opens it, grouped by Compose project as on
//! the Containers tab. A click on a running container opens a shell in it; a right click opens
//! its menu: Open shell, Follow logs, Restart and Stop (or Start), Copy name.
//!
//! With Settings → Layout → Where tabs go on "Under each server", the server's tabs come first
//! in its tree, open or not (`tabs.rs`).
//!
//! ```text
//! expand-N        the chevron on server row N (kept open in Settings' `expanded`, as 0.10 did)
//! tabs-N          under the row: its tabs, when they go under each server (`tabs.rs`)
//! tree-N          under the row: tree-note (looking, a problem, none yet) · ct-N-<i> rows
//! ct-menu         a container's menu: ctm-shell, ctm-logs, ctm-restart, ctm-stop | ctm-start,
//!                 ctm-copy
//! ```
//!
//! The data and every rule are the Containers tab's (`sshee_app::containers::Board`), asked of
//! the same backend (`Backend::containers`). An open tree has a board of its own, under an id no
//! tab has ([`TREE_IDS`] and up), so the answers come back through `Remote::containers` like a
//! tab's; it looks every 10 seconds while it's open and the list is shown, and never otherwise.

use indexmap::IndexMap;
use rune::{col, el, row, text, El, Key, KeyEvent, Tx};
use sshee_app::containers::{self as rules, Action, Answer, Ask, Board, Problem, Reply};
use sshee_app::model::ContainerRun;
use sshee_app::workbench::{Open, ServerKey, TabId, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::list::{list_row, list_rule, ListRow};
use crate::kit::menu::RowState;
use crate::kit::popover::popover;
use crate::shell::{Shell, Toast};
use crate::theme::{self, Theme};

mod tabs;
pub use tabs::{list as tabs, rail as rail_tabs};

/// The ids of trees' boards count down from here: no tab ever has one.
pub const TREE_IDS: u64 = u64::MAX / 2;

/// The menu's width.
const MENU_WIDTH: f32 = 220.0;

/// One open tree.
struct Tree {
    id: TabId,
    board: Board,
    /// When the next look is due, on the window's clock; `None`: now.
    due: Option<f64>,
    /// When the look on its way was first seen waiting.
    asked: Option<f64>,
}

/// A container's menu, open at the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub server: ServerKey,
    /// The container, by id.
    pub id: String,
    pub at: (f32, f32),
    pub hot: Option<usize>,
}

/// What the menu can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Shell,
    Logs,
    Act(Action),
    CopyName,
}

/// The trees open now, and the menu.
#[derive(Default)]
pub struct State {
    trees: IndexMap<ServerKey, Tree>,
    made: u64,
    pub menu: Option<Menu>,
    /// The servers' tabs, when they go under each server.
    tabs: tabs::Tabs,
}

impl State {
    /// The board behind a reply, when the reply is a tree's.
    fn by_id(&mut self, id: TabId) -> Option<(&ServerKey, &mut Tree)> {
        self.trees.iter_mut().find(|(_, t)| t.id == id)
    }

    /// Forgets what it knew of tabs that are gone.
    pub(crate) fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.tabs.retain(open);
    }

    /// A tab under its server is being dragged.
    #[cfg(test)]
    pub(crate) fn dragging_tab(&self) -> bool {
        self.tabs.drag.is_some()
    }

    /// A server's board, while its tree is open.
    pub fn board(&self, server: &str) -> Option<&Board> {
        self.trees.get(server).map(|t| &t.board)
    }
}

/// The tree under this server is open (Settings keeps which, as 0.10 did).
pub fn expanded(s: &Shell, server: &str) -> bool {
    s.settings.get().expanded.iter().any(|k| k == server)
}

/// The trees drawn now: the list is shown, wide, and these are open.
fn shown(s: &Shell) -> Vec<ServerKey> {
    if !s.bench.sidebar_shown() || !s.sidebar_wide() {
        return Vec::new();
    }
    s.bench.servers().iter().map(|sv| sv.key()).filter(|k| expanded(s, k)).collect()
}

fn ask(s: &mut Shell, server: &str, want: rules::Want) {
    let Some(id) = s.screens.rail_tree.trees.get(server).map(|t| t.id) else { return };
    s.backend_mut().containers(Ask { tab: id, server: server.to_owned(), want });
}

/// Looks again now.
fn refresh(s: &mut Shell, server: &str) {
    let Some(tree) = s.screens.rail_tree.trees.get_mut(server) else { return };
    let want = tree.board.list();
    tree.due = None;
    tree.asked = None;
    ask(s, server, want);
}

impl Shell {
    /// The chevron on a server's row: its containers under it, or not.
    pub fn toggle_containers(&mut self, server: &str, tx: &mut Tx) {
        let open = expanded(self, server);
        let server = server.to_owned();
        self.change_settings(tx, |st| {
            st.set(|s| {
                if open {
                    s.expanded.retain(|k| *k != server);
                } else {
                    s.expanded.push(server.clone());
                }
            })
        });
        tx.instant();
    }

    /// A right click on a container: its menu at the pointer.
    pub fn open_container_menu(&mut self, server: &str, id: &str, at: (f32, f32), tx: &mut Tx) {
        self.screens.close_all();
        self.screens.rail_tree.menu = Some(Menu { server: server.to_owned(), id: id.to_owned(), at, hot: None });
        tx.focus("ct-menu", false);
        tx.instant();
    }

    pub fn close_container_menu(&mut self, tx: &mut Tx) {
        self.screens.rail_tree.menu = None;
        self.settle(tx);
    }

    /// A terminal into a container: a shell there, or its logs as they come.
    fn container_terminal(&mut self, server: &str, id: &str, run: ContainerRun, tx: &mut Tx) {
        let Some(inside) = self.screens.rail_tree.board(server).and_then(|b| b.container(id)).map(|c| rules::inside(c, run)) else { return };
        self.bench.open_with(TabKind::Terminal, server, Open { title: Some(rules::tab_title(&inside)), container: Some(inside), ..Open::default() });
        self.settle(tx);
    }

    /// A click on a container: a shell in it; a stopped one says how to start it.
    fn container_pressed(&mut self, server: &str, id: &str, tx: &mut Tx) {
        let Some(c) = self.screens.rail_tree.board(server).and_then(|b| b.container(id)) else { return };
        if c.running {
            return self.container_terminal(server, id, ContainerRun::Shell, tx);
        }
        let name = self.text_shown(&c.name).into_owned();
        self.toast(Toast::info(format!("{name} is stopped")).body("Right-click it to start it or read its logs."));
    }

    fn container_menu_items(&self) -> Vec<Vec<Item>> {
        let Some(menu) = &self.screens.rail_tree.menu else { return Vec::new() };
        let Some(c) = self.screens.rail_tree.board(&menu.server).and_then(|b| b.container(&menu.id)) else { return Vec::new() };
        let acts = if c.running { vec![Item::Act(Action::Restart), Item::Act(Action::Stop)] } else { vec![Item::Act(Action::Start)] };
        let open = if c.running { vec![Item::Shell, Item::Logs] } else { vec![Item::Logs] };
        vec![open, acts, vec![Item::CopyName]]
    }

    fn container_menu_pick(&mut self, item: Item, tx: &mut Tx) {
        let Some(menu) = self.screens.rail_tree.menu.take() else { return };
        match item {
            Item::Shell => self.container_terminal(&menu.server, &menu.id, ContainerRun::Shell, tx),
            Item::Logs => self.container_terminal(&menu.server, &menu.id, ContainerRun::Logs, tx),
            Item::Act(action) => {
                let want = self.screens.rail_tree.trees.get_mut(&menu.server).and_then(|t| t.board.act(&menu.id, action));
                if let Some(want) = want {
                    ask(self, &menu.server, want);
                }
                self.settle(tx);
            }
            Item::CopyName => {
                if let Some(name) = self.screens.rail_tree.board(&menu.server).and_then(|b| b.container(&menu.id)).map(|c| c.name.clone()) {
                    tx.copy(name);
                    self.toast(Toast::info("Copied the name"));
                }
                self.settle(tx);
            }
        }
    }

    fn container_menu_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let items = self.container_menu_items().concat();
        let Some(menu) = self.screens.rail_tree.menu.as_mut() else { return false };
        match ev.key {
            Key::Down | Key::Up => {
                menu.hot = sshee_app::palette::step(items.len(), menu.hot, if ev.key == Key::Down { 1 } else { -1 });
                tx.instant();
            }
            Key::Enter => {
                if let Some(item) = menu.hot.and_then(|h| items.get(h)).copied() {
                    self.container_menu_pick(item, tx);
                }
            }
            Key::Escape => self.close_container_menu(tx),
            _ => return false,
        }
        true
    }
}

/// Every frame: an open tree with no look yet, or whose next look is due, asks for one; one
/// unanswered too long gives up and says so. Trees no longer shown are dropped. True when
/// something changed on screen.
pub(crate) fn tick(s: &mut Shell, clock: f64, tx: &mut Tx) -> bool {
    let shown = shown(s);
    let st = &mut s.screens.rail_tree;
    st.trees.retain(|k, _| shown.contains(k));
    let mut changed = false;
    for server in shown {
        let st = &mut s.screens.rail_tree;
        if !st.trees.contains_key(&server) {
            st.made += 1;
            let id = TabId(TREE_IDS - st.made);
            st.trees.insert(server.clone(), Tree { id, board: Board::default(), due: None, asked: None });
        }
        let Some(tree) = st.trees.get_mut(&server) else { continue };
        if tree.board.is_loading() {
            let Some(asked) = tree.asked else {
                tree.asked = Some(clock);
                tx.wake_at(clock + rules::LOOK_TIMEOUT_SECS);
                continue;
            };
            if clock >= asked + rules::LOOK_TIMEOUT_SECS && tree.board.no_answer() {
                let due = clock + tree.board.next_look_in();
                (tree.asked, tree.due) = (None, Some(due));
                tx.wake_at(due);
                changed = true;
            }
        } else if tree.due.is_none_or(|due| clock >= due) {
            changed |= tree.board.is_first_look();
            refresh(s, &server);
            if let Some(tree) = s.screens.rail_tree.trees.get_mut(&server).filter(|t| t.board.is_loading()) {
                tree.asked = Some(clock);
                tx.wake_at(clock + rules::LOOK_TIMEOUT_SECS);
            }
        }
    }
    changed
}

/// The backend's answer, when it's a tree's: true when it was (it's taken care of).
pub fn deliver(s: &mut Shell, reply: &Reply, tx: &mut Tx) -> bool {
    let Some((server, tree)) = s.screens.rail_tree.by_id(reply.tab) else { return false };
    let server = server.clone();
    match &reply.answer {
        Answer::Listed { ticket, result } => {
            if tree.board.listed(*ticket, result.clone()) {
                tree.asked = None;
                let due = tx.clock() + tree.board.next_look_in();
                tree.due = Some(due);
                tx.wake_at(due);
            }
        }
        Answer::Acted { id, name, action, result } => {
            tree.board.acted(id, name, *action, result.clone());
            // How it went, in a toast: the list has no room for a note.
            let note = tree.board.note().cloned();
            tree.board.dismiss_note();
            if let Some(note) = note {
                let words = s.text_shown(&note.text).into_owned();
                s.toast(if note.ok { Toast::success(words) } else { Toast::error(words) });
            }
            if !s.screens.rail_tree.trees.get(&server).is_some_and(|t| t.board.is_loading()) {
                refresh(s, &server);
            }
        }
    }
    true
}

/// A quiet line in the tree: looking, what's wrong, nothing yet.
fn note(t: &Theme, words: &str, indent: f32) -> El {
    text(words.to_owned()).key("tree-note").font(theme::SMALL).color(t.l2).w_full().pl(indent).pr(8.0).py(4.0).wrap()
}

/// The tree under server row `index`, when it's open.
pub fn tree(s: &Shell, t: &'static Theme, index: usize, server: &str, indent: f32) -> Option<El> {
    if !expanded(s, server) {
        return None;
    }
    let mut out = col().key(format!("tree-{index}")).w_full().pb(2.0).role("group").label_static("Containers");
    let Some(board) = s.screens.rail_tree.board(server) else { return Some(out.child(note(t, "Looking for containers…", indent))) };
    if board.is_first_look() {
        return Some(out.child(note(t, "Looking for containers…", indent)));
    }
    match board.problem() {
        Some(Problem::Page { message, command, .. }) => {
            out = out.child(note(t, &s.text_shown(&message), indent));
            if let Some(command) = command {
                out = out.child(copy_chip(t, &command, indent));
            }
            return Some(out);
        }
        Some(Problem::Note { text, .. }) => out = out.child(note(t, &s.text_shown(&text), indent)),
        None => {}
    }
    if let Some((title, line)) = board.empty() {
        return Some(out.child(note(t, &format!("{title}. {line}"), indent)));
    }
    let mut n = 0;
    for group in board.groups() {
        let inner = if group.project.is_some() { indent + 12.0 } else { indent };
        if let Some(project) = &group.project {
            out = out.child(
                row()
                    .key(format!("ct-project-{index}-{n}"))
                    .w_full()
                    .h(22.0)
                    .pl(indent)
                    .gap(6.0)
                    .items_center()
                    .help("Docker Compose project")
                    // Its name alone, as the Containers tab heads a project: a folder here would
                    // read as the Files tab beside it.
                    .child(text(s.text_shown(project).into_owned()).font(theme::SMALL).color(t.l2).single_line().truncate().shrink()),
            );
        }
        for r in &group.rows {
            let Some(c) = board.container(&r.id) else { continue };
            let (key, id) = (server.to_owned(), c.id.clone());
            let (menu_key, menu_id) = (key.clone(), id.clone());
            let name = s.text_shown(&c.name).into_owned();
            let ink = if c.running { t.l1 } else { t.l2 };
            let mut line = row()
                .key(format!("ct-{index}-{n}"))
                .w_full()
                .h(26.0)
                .pl(inner)
                .pr(8.0)
                .gap(7.0)
                .items_center()
                .radius(theme::RADIUS_ROW)
                .hover(t.hov2)
                .press(t.down)
                .focusable(true)
                .role("button")
                .label(if c.running { name.clone() } else { format!("{name}, stopped") })
                .help(s.text_shown(&format!("{}\n{}", c.image, c.status)).into_owned())
                .on_press_tx(move |s: &mut Shell, tx| s.container_pressed(&key, &id, tx))
                .on_secondary_press(move |s: &mut Shell, at, tx| s.open_container_menu(&menu_key, &menu_id, (at.x, at.y), tx))
                .child(icon(Icon::Cube, 13.0, if c.running { t.l2on } else { t.l2 }))
                .child(text(name).font(theme::NOTE).color(ink).single_line().truncate().grow(1.0).shrink().min_w(0.0));
            if let Some(doing) = r.busy {
                line = line.child(text(doing.doing()).font(theme::SMALL).color(t.l2).single_line());
            } else if !c.running {
                line = line.child(text("stopped").font(theme::SMALL).color(t.l2).single_line());
            }
            out = out.child(line);
            n += 1;
        }
    }
    Some(out)
}

/// A command to copy, in the terminal's font: a click copies it.
fn copy_chip(t: &Theme, command: &str, indent: f32) -> El {
    let words = command.to_owned();
    el().w_full().pl(indent).pr(8.0).pb(4.0).child(
        row()
            .key("tree-copy")
            .px(6.0)
            .py(3.0)
            .gap(5.0)
            .items_center()
            .radius(theme::RADIUS_CHIP)
            .fill(t.hov2)
            .hover(t.hov)
            .focusable(true)
            .role("button")
            .label(format!("Copy {command}"))
            .help("Copy")
            .on_press_tx(move |s: &mut Shell, tx| {
                tx.copy(words.clone());
                s.toast(Toast::info("Copied").body(words.clone()));
            })
            .child(text(command.to_owned()).font(theme::CODE).color(t.l1).wrap().shrink().min_w(0.0))
            .child(icon(Icon::Copy, 11.0, t.l2)),
    )
}

/// A container's menu, when it's open: a last child of the window's root.
pub fn menu(s: &Shell, t: &'static Theme) -> Option<El> {
    let menu = s.screens.rail_tree.menu.as_ref()?;
    let groups = s.container_menu_items();
    if groups.is_empty() {
        return None;
    }
    let mut rows = col().w_full().padding(crate::kit::menu::PAD);
    let mut n = 0;
    for (g, group) in groups.into_iter().enumerate() {
        if g > 0 {
            rows = rows.child(list_rule(t).key(format!("ctm-rule-{g}")));
        }
        for item in group {
            let state = if menu.hot == Some(n) { RowState::Hot } else { RowState::Rest };
            let (key, label, glyph) = match item {
                Item::Shell => ("ctm-shell", "Open shell", Some(Icon::Terminal)),
                Item::Logs => ("ctm-logs", "Follow logs", Some(Icon::Doc)),
                Item::Act(Action::Restart) => ("ctm-restart", "Restart", Some(Icon::Refresh)),
                Item::Act(Action::Stop) => ("ctm-stop", "Stop", None),
                Item::Act(Action::Start) => ("ctm-start", "Start", None),
                Item::CopyName => ("ctm-copy", "Copy name", Some(Icon::Copy)),
            };
            let i = n;
            let mut line = list_row(t, &ListRow { icon: glyph, label, state, ..ListRow::default() })
                .key(key)
                .role("menuitem")
                .on_pointer_move(move |s: &mut Shell, _| {
                    if let Some(m) = s.screens.rail_tree.menu.as_mut() {
                        m.hot = Some(i);
                    }
                })
                .on_press_tx(move |s: &mut Shell, tx| s.container_menu_pick(item, tx));
            if state == RowState::Rest {
                line = line.hover(t.hov2);
            }
            rows = rows.child(line);
            n += 1;
        }
    }
    let name = s.screens.rail_tree.board(&menu.server).and_then(|b| b.container(&menu.id)).map(|c| s.text_shown(&c.name).into_owned()).unwrap_or_default();
    Some(
        popover(t, MENU_WIDTH)
            .key("ct-menu")
            .label(format!("{name} menu"))
            .absolute(menu.at.0.max(8.0), menu.at.1.max(8.0))
            .role("menu")
            .focusable(true)
            .no_focus_ring()
            .on_key(|s: &mut Shell, ev, tx| s.container_menu_key(ev, tx))
            .child(rows)
            .modal_tx(true, |s: &mut Shell, tx| s.close_container_menu(tx)),
    )
}
