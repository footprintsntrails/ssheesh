//! The row of tabs in the top bar: every machine's tabs in one row, a tab's menu, dragging a
//! tab to another place, and the list of all tabs when the row can't hold them.
//!
//! ```text
//! tabs   row, takes what the bar has left
//! ├── [sep] [group] tab …      a divider between machines; when crowded, each machine named
//! │                            once before its tabs
//! ├── tab-list                 only when some tabs are out of sight: all of them, in a menu;
//! │                            an orange dot when one of those out of sight waits for you
//! └── new-tab                  "+": its menu (below)
//! ```
//!
//! "+" opens a menu, as 0.10's did (`newTabItems` in `Tabs.tsx`); the new-terminal shortcut
//! still opens a terminal at once. The arrows move one highlight, Return picks, → goes into a
//! machine and ← or Esc comes back out:
//!
//! ```text
//! new-tab-menu
//! ├── nt-terminal  New terminal   ⌃⇧T     on the machine on screen
//! ├── nt-files     Files          ⌃⇧O
//! ├── ──────
//! └── nt-server-N  db-main  ›             every other machine, in the sidebar's order
//!     ├── nt-back      ‹ Back
//!     ├── nt-terminal  New terminal on db-main
//!     └── nt-files     Files on db-main
//! ```
//!
//! Three rules keep the row still under the pointer, as a browser's is:
//! - a tab under the pointer shows its close cross in room it always keeps (the kit); in a
//!   crowded row only the tab you're on has one, and the others' names get its room;
//! - while the pointer is over the row, every tab keeps the width it had when the pointer
//!   came, and the row stays crowded or roomy as it was: a tab renamed by its shell, or one
//!   closed with the cross, doesn't move the next cross away from the pointer;
//! - a dragged tab is lifted and follows the pointer; the tabs it passes step aside, and the
//!   room it would land in is outlined (`tab-drop-slot`). The row's order changes on the drop.
//!   Below the row, over the page, the page shows where it would land (`split::drop_mark`).
//!
//! The title and the row share the bar's middle (`fit`), decided together each frame from one
//! measurement: first the title's address gives way, then the tabs crowd. A crowded row
//! squeezes the other tabs before the one you're on, which keeps its name up to
//! [`ACTIVE_TAB`]. When even crowded tabs don't fit, the row shows a run of them that holds the
//! tab on screen (`layout::in_sight`), and the rest are in the list. "+" is always in the
//! window.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use rune::{Align, Drag, El, Key, KeyEvent, Phase, Side, Tx};
use sshee_app::palette::shortcut_label;
use sshee_app::workbench::shortcuts::Shortcut;
use sshee_app::workbench::layout::{self, Layout};
use sshee_app::workbench::titles::{self, Home};
use sshee_app::model::EnvLabel;
use sshee_app::workbench::{Badge, Open, ServerKey, StripTab, TabId, TabKind};

use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::kit::button::toolbar_button;
use crate::kit::list::{list_row, list_rule, ListRow};
use crate::kit::menu::{menu, menu_row, menu_separator, RowState};
use crate::kit::popover::popover;
use crate::kit::chip::env_chip_width;
use crate::kit::tab::{group_name, tab, Tab};
use crate::kit::{divider, ringed_dot};
use crate::terminal::overlays::Wish;
use crate::theme::{self, Theme};

/// The narrowest a crowded tab gets before tabs start going out of sight (the design's).
pub const MIN_TAB: f32 = 76.0;

/// The widest the tab you're on gets in a crowded row: it keeps its name, up to this, and the
/// others are squeezed first.
pub const ACTIVE_TAB: f32 = 180.0;

/// How much room must come free before the row loosens again (crowded tabs grow back, the
/// title's address comes back): a name a few letters longer doesn't flip it back and forth.
const SLACK: f32 = 16.0;

/// How far the pointer travels before a press on a tab is a drag.
const DRAG_START: f32 = 4.0;

/// The tab menu's, the tab list's and the "+" menu's widths.
const MENU_WIDTH: f32 = 248.0;
const LIST_WIDTH: f32 = 300.0;
const NEW_WIDTH: f32 = 260.0;

/// How the row was when the pointer came over it.
#[derive(Clone, Debug, Default, PartialEq)]
struct Held {
    compact: bool,
    /// The title showed the machine's address.
    address: bool,
    widths: HashMap<TabId, f32>,
}

/// A tab being dragged. It stays at its place in the row until it's dropped: drawn lifted under
/// the pointer, with the tabs it passes stepping aside and the place it would land marked.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dragging {
    id: TabId,
    moved: bool,
    /// How far the pointer went since the press.
    dx: f32,
    dy: f32,
    /// Below the row, over the page: it would land there, not in the row.
    over_page: bool,
    /// Where it would land in the row: before the `slot`th of the other tabs in sight.
    slot: Option<usize>,
    /// The same place in the whole row, counted without it (what `move_tab` takes).
    to: Option<usize>,
}

/// How far each tab is drawn from its place while one is dragged along the row, and the room
/// it would land in (`[x, y, w, h]`, window points).
type DragMarks = (HashMap<TabId, [f32; 2]>, Option<[f32; 4]>);

/// The row's state.
#[derive(Default)]
pub struct Row {
    /// Where each tab was last laid out, in window points.
    rects: HashMap<TabId, [f32; 4]>,
    held: Option<Held>,
    drag: Option<Dragging>,
    /// The tab whose menu is open, and where.
    pub(crate) menu: Option<(TabId, [f32; 2])>,
    /// The highlighted row of the tab's menu, among the rows that can be picked: none until an
    /// arrow lights one, so Return on a menu just opened picks nothing (0.10.5).
    pub(crate) menu_hot: Option<usize>,
    /// The list of all tabs is open.
    pub(crate) list: bool,
    /// The "+" menu is open.
    pub(crate) new_tab: Option<NewTabMenu>,
    /// The title's menu is open (`title.rs`).
    pub(crate) title_menu: Option<super::title::TitleMenu>,
    /// What the view last drew: whether the row was crowded, whether the title showed the
    /// address, the tabs in sight, and the first of them. (The view only reads the window's
    /// state; these are its own notes.)
    compact: Cell<bool>,
    address: Cell<bool>,
    shown: RefCell<Vec<TabId>>,
    first: Cell<usize>,
}

impl Row {
    /// Closes the tab menu and the tab list.
    pub(crate) fn close_overlays(&mut self) {
        self.menu = None;
        self.list = false;
        self.new_tab = None;
        self.title_menu = None;
    }

    /// The row was crowded when it was last drawn.
    #[cfg(test)]
    pub(crate) fn crowded(&self) -> bool {
        self.compact.get()
    }

    /// Forgets tabs that are gone.
    pub(crate) fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.rects.retain(|id, _| open(*id));
        if self.menu.is_some_and(|(id, _)| !open(id)) {
            self.menu = None;
        }
    }
}

/// The "+" menu, open: on its first page, or on one machine's (`on`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NewTabMenu {
    pub(crate) on: Option<ServerKey>,
    /// The highlighted row, among the rows that can be picked.
    pub(crate) hot: Option<usize>,
}

/// A row of the "+" menu.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NewPick {
    Terminal(ServerKey),
    Files(ServerKey),
    /// Another machine: its own page, with these two for it.
    Machine(ServerKey),
    Back,
}

/// What a row of a tab's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabPick {
    Close,
    CloseOthers,
    CloseRight,
    /// Show it beside the tab on screen (or, for the tab on screen, beside its neighbour).
    SideBySide,
    /// Show it under the tab on screen.
    Below,
    /// Take it out of its pair.
    Apart,
    Duplicate,
    /// A new terminal on the tab's machine.
    NewTerminal,
    /// Files on the tab's machine.
    Files,
    /// A terminal's: Files at the folder its shell is in (0.10's "Browse files in this
    /// folder").
    BrowseFiles,
}

impl Shell {
    /// The pointer came over the row, or left it.
    pub(crate) fn row_hover(&mut self, over: bool, tx: &mut Tx) {
        if over {
            let widths = self.row.shown.borrow().iter().filter_map(|id| Some((*id, self.row.rects.get(id)?[2]))).collect();
            self.row.held = Some(Held { compact: self.row.compact.get(), address: self.row.address.get(), widths });
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
                self.row.drag = Some(Dragging { id, moved: false, dx: 0.0, dy: 0.0, over_page: false, slot: None, to: None });
                tx.live_only();
            }
            Phase::Move => {
                let Some(mut drag) = self.row.drag.filter(|g| g.id == id) else { return };
                if !drag.moved && d.dx.abs() < DRAG_START && d.dy.abs() < DRAG_START {
                    tx.live_only();
                    return;
                }
                let at = d.window_point();
                (drag.moved, drag.dx, drag.dy) = (true, d.dx, d.dy);
                // Over the page: it would land on a pane there (`split.rs`), not in the row.
                drag.over_page = self.row.rects.get(&id).is_some_and(|r| at[1] > r[1] + r[3] + 8.0);
                self.sides.drop = if drag.over_page { self.drop_target(id, at) } else { None };
                (drag.slot, drag.to) = if drag.over_page { (None, None) } else { self.row_slot(id, at[0]) };
                self.row.drag = Some(drag);
                // The tabs it passes step aside, and the marks move, on their springs; the
                // dragged tab itself follows the pointer exactly (`strip`).
                tx.animated();
            }
            Phase::Up => {
                let Some(drag) = self.row.drag.take().filter(|g| g.id == id) else { return };
                let drop = self.sides.drop.take();
                tx.instant();
                if d.cancelled() || !drag.moved {
                    return;
                }
                if let Some(at) = drop {
                    return self.drop_on(id, at, tx);
                }
                if let Some(to) = drag.to {
                    self.bench.move_tab(id, to);
                }
                // The tab you dragged is the one you're on, as in a browser; and the row's new
                // order is kept for the next run.
                self.focus_tab(id, tx);
            }
        }
    }

    /// Where tab `id` would land in the row with the pointer at `x`: before which of the other
    /// tabs in sight (by their middles where they were laid out), and that place in the whole
    /// row counted without it.
    fn row_slot(&self, id: TabId, x: f32) -> (Option<usize>, Option<usize>) {
        let others: Vec<(TabId, f32)> = self.row.shown.borrow().iter().filter(|t| **t != id).filter_map(|t| self.row.rects.get(t).map(|r| (*t, r[0] + r[2] / 2.0))).collect();
        let centres: Vec<f32> = others.iter().map(|(_, c)| *c).collect();
        let slot = layout::slot(&centres, x);
        let rest: Vec<TabId> = self.bench.tabs().iter().map(|t| t.id).filter(|t| *t != id).collect();
        let to = match others.get(slot) {
            Some((before, _)) => rest.iter().position(|t| t == before),
            None => others.last().and_then(|(last, _)| rest.iter().position(|t| t == last)).map(|i| i + 1),
        };
        (Some(slot), to)
    }

    /// While a tab is dragged along the row: how far each tab in sight is drawn from its place
    /// (the dragged one under the pointer, the ones it passed stepped aside), and where it
    /// would land, `[x, y, w, h]` in window points.
    fn drag_marks(&self) -> Option<DragMarks> {
        let drag = self.row.drag.filter(|g| g.moved)?;
        let shown = self.row.shown.borrow();
        let mine = *self.row.rects.get(&drag.id)?;
        let mut shifts = HashMap::new();
        shifts.insert(drag.id, [drag.dx, if drag.over_page { drag.dy } else { 0.0 }]);
        let (Some(from), Some(slot)) = (shown.iter().position(|t| *t == drag.id), drag.slot) else { return Some((shifts, None)) };
        let others: Vec<TabId> = shown.iter().copied().filter(|t| *t != drag.id).collect();
        // The room it leaves, and takes: its width and the row's gap.
        let room = mine[2] + 2.0;
        for (i, t) in others.iter().enumerate() {
            let step = if i < from && i >= slot {
                room
            } else if i >= from && i < slot {
                -room
            } else {
                continue;
            };
            shifts.insert(*t, [step, 0.0]);
        }
        let x = if slot <= from {
            others.get(slot).and_then(|t| self.row.rects.get(t)).map_or(mine[0], |r| if slot == from { mine[0] } else { r[0] })
        } else {
            others.get(slot - 1).and_then(|t| self.row.rects.get(t)).map_or(mine[0], |r| r[0] + r[2] - mine[2])
        };
        Some((shifts, Some([x, mine[1], mine[2], mine[3]])))
    }

    pub(crate) fn open_tab_menu(&mut self, id: TabId, at: [f32; 2], tx: &mut Tx) {
        self.row.list = false;
        self.row.menu = Some((id, at));
        self.row.menu_hot = None;
        // The menu takes the keys: arrows, Return, Esc; none reaches the terminal behind it.
        tx.focus("tab-menu", false);
        tx.instant();
    }

    /// A key while a tab's menu is open: arrows move one highlight, Return picks it, Esc
    /// closes; everything else stops here, but streamer mode's shortcut.
    fn tab_menu_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if self.streamer_key(ev, tx) {
            return true;
        }
        let Some((id, _)) = self.row.menu else { return false };
        let picks: Vec<TabPick> = tab_menu_items(self, id).into_iter().flatten().filter(|i| i.on).map(|i| i.pick).collect();
        match ev.key {
            Key::Down | Key::Up => {
                self.row.menu_hot = sshee_app::palette::step(picks.len(), self.row.menu_hot, if ev.key == Key::Down { 1 } else { -1 });
                tx.instant();
            }
            Key::Enter => {
                if let Some(pick) = self.row.menu_hot.and_then(|h| picks.get(h)).copied() {
                    self.tab_pick(id, pick, tx);
                }
            }
            Key::Escape => self.close_tab_overlays(tx),
            _ => {}
        }
        true
    }

    pub(crate) fn toggle_tab_list(&mut self, tx: &mut Tx) {
        self.row.menu = None;
        self.row.new_tab = None;
        self.row.list = !self.row.list;
        tx.instant();
    }

    /// "+": its menu opens, or closes when it is open.
    pub fn toggle_new_tab_menu(&mut self, tx: &mut Tx) {
        let open = self.row.new_tab.is_some();
        self.row.close_overlays();
        if open {
            return self.settle(tx);
        }
        self.row.new_tab = Some(NewTabMenu::default());
        // The menu itself takes the keys: nothing typed reaches the terminal behind it.
        tx.focus("new-tab-menu", false);
        tx.instant();
    }

    /// The machine "+" is for: the one on screen, this computer on the main menu.
    fn new_tab_here(&self) -> ServerKey {
        self.bench.current_key().unwrap_or("local").to_owned()
    }

    /// The "+" menu's rows as it is now, in groups (a rule between two).
    fn new_tab_groups(&self) -> Vec<Vec<NewPick>> {
        let Some(menu) = &self.row.new_tab else { return Vec::new() };
        if let Some(on) = menu.on.clone().filter(|on| self.bench.server(on).is_some()) {
            return vec![vec![NewPick::Back], vec![NewPick::Terminal(on.clone()), NewPick::Files(on)]];
        }
        let here = self.new_tab_here();
        let others: Vec<NewPick> = self.bench.servers().iter().map(|sv| sv.key()).filter(|k| *k != here).map(NewPick::Machine).collect();
        let mut groups = vec![vec![NewPick::Terminal(here.clone()), NewPick::Files(here)]];
        if !others.is_empty() {
            groups.push(others);
        }
        groups
    }

    /// A key while the "+" menu is open. Every key is the menu's: none reaches a terminal.
    fn new_tab_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if self.streamer_key(ev, tx) {
            return true;
        }
        let items = self.new_tab_groups().concat();
        let Some(menu) = self.row.new_tab.as_mut() else { return false };
        let hot = menu.hot.and_then(|h| items.get(h)).cloned();
        let inside = menu.on.is_some();
        match ev.key {
            Key::Down | Key::Up => {
                menu.hot = sshee_app::palette::step(items.len(), menu.hot, if ev.key == Key::Down { 1 } else { -1 });
                tx.instant();
            }
            Key::Enter => {
                if let Some(pick) = hot {
                    self.new_tab_pick(pick, true, tx);
                }
            }
            Key::Right => {
                if let Some(pick @ NewPick::Machine(_)) = hot {
                    self.new_tab_pick(pick, true, tx);
                }
            }
            Key::Left | Key::Backspace | Key::Escape if inside => self.new_tab_pick(NewPick::Back, true, tx),
            Key::Escape => self.close_tab_overlays(tx),
            _ => {}
        }
        true
    }

    /// A row of the "+" menu, picked. `by_key`: from the keyboard, so the page it turns to
    /// starts with a row highlighted (by the pointer, none is until it moves).
    fn new_tab_pick(&mut self, pick: NewPick, by_key: bool, tx: &mut Tx) {
        let came_from = self.row.new_tab.as_ref().and_then(|m| m.on.clone());
        match pick {
            NewPick::Terminal(server) => {
                self.row.close_overlays();
                self.bench.open(TabKind::Terminal, &server);
                self.settle(tx);
            }
            NewPick::Files(server) => {
                self.row.close_overlays();
                self.open_files_on(&server, None, tx);
            }
            NewPick::Machine(server) => {
                // By key, "New terminal on …" is the one Return picks next.
                self.row.new_tab = Some(NewTabMenu { on: Some(server), hot: by_key.then_some(1) });
                tx.instant();
            }
            NewPick::Back => {
                self.row.new_tab = Some(NewTabMenu::default());
                // By key, back on the machine it came from.
                let at = self.new_tab_groups().concat().iter().position(|p| matches!(p, NewPick::Machine(k) if Some(k) == came_from.as_ref()));
                if let Some(menu) = self.row.new_tab.as_mut() {
                    menu.hot = at.filter(|_| by_key);
                }
                tx.instant();
            }
        }
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
            TabPick::SideBySide => self.show_beside(id, layout::Dir::Row, tx),
            TabPick::Below => self.show_beside(id, layout::Dir::Col, tx),
            TabPick::Apart => self.leave_pair(id, tx),
            TabPick::Duplicate => self.duplicate_tab(id, tx),
            TabPick::BrowseFiles => {
                if let Some(pane) = self.panes.get_mut(&id) {
                    pane.browse_files_here(tx);
                }
                self.settle(tx);
            }
            TabPick::NewTerminal | TabPick::Files => {
                let Some(server) = self.bench.tab(id).map(|t| t.server.clone()) else { return };
                if pick == TabPick::Files {
                    self.open_files_on(&server, None, tx);
                } else {
                    // Next to the tab the menu was opened on.
                    self.bench.focus(id);
                    self.bench.open(TabKind::Terminal, &server);
                    self.settle(tx);
                }
            }
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
        // A terminal into a container opens into it again, under its name.
        let title = like.container.as_ref().and(like.title);
        self.bench.open_with(like.kind, &like.server, Open { path: like.path, title, container: like.container, ..Open::default() });
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
                Wish::BrowseFiles(folder) => {
                    self.browse_files(id, folder, tx);
                    changed = true;
                }
                Wish::UploadFolder(folder) => changed |= super::file_drop::folder_found(self, id, folder),
                Wish::NewTerminalWith { command, beside } => {
                    crate::screens::prompts::new_terminal_with(self, id, command, beside, tx);
                    changed = true;
                }
                Wish::CloseTab => {
                    self.close_tab(id, tx);
                    changed = true;
                }
                Wish::AlwaysReconnect => {
                    self.change_settings(tx, |st| st.set(|s| s.reconnect_mode = sshee_app::model::ReconnectMode::Auto));
                    changed = true;
                }
                Wish::Trusted => changed |= crate::screens::prompts::trusted(self, id),
                Wish::ShowPath { dir, name, lines } => {
                    crate::screens::prompts::show_path(self, id, &dir, name.as_deref(), lines, tx);
                    changed = true;
                }
                Wish::Folder(folder) => changed |= self.set_folder(id, &folder),
                // The password typed above the terminal got it in: kept for its server, in the
                // keychain (`terminal/password.rs`).
                Wish::SavePassword(password) => {
                    let Some(server) = self.bench.tab(id).map(|t| t.server.clone()) else { continue };
                    self.backend_mut().servers(sshee_app::servers::Ask::SavePassword { server, password, remember: true });
                }
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
    /// `line` is where in the file. The machine's Files tab shows the folder it is in with it
    /// picked out, and opens it (a folder: goes into it) scrolled to the line, marked, as
    /// 0.10's `openTerminalLink`.
    pub fn open_path(&mut self, from: TabId, path: &str, line: Option<u32>, tx: &mut Tx) {
        let Some(server) = self.bench.tab(from).map(|t| t.server.clone()) else { return };
        let id = self.open_files_on(&server, None, tx);
        crate::screens::files::show_path_at(self, id, path, line.map(|l| (l, l)));
    }

    /// "Browse files in this folder", from terminal `from`: Files on its machine, at the folder
    /// its shell is in (`folder`, from tmux or OSC 7). Not known on a server: the home folder,
    /// and a note that following the terminal needs tmux (0.10's words).
    pub fn browse_files(&mut self, from: TabId, folder: Option<String>, tx: &mut Tx) {
        let Some(server) = self.bench.tab(from).map(|t| t.server.clone()) else { return };
        let local = self.bench.server(&server).is_none_or(|sv| sv.is_local());
        self.open_files_on(&server, Some(folder.as_deref().unwrap_or("~")), tx);
        if folder.is_none() && !local {
            self.toast(super::notes::Toast::info("Opened your home folder").body("Following the terminal's folder needs tmux on the server."));
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
        TabKind::Terminal if tab.container.is_some() => Icon::Cube,
        TabKind::Terminal => Icon::Terminal,
        TabKind::Files => Icon::Folder,
        TabKind::Stats => Icon::Stats,
        TabKind::Containers => Icon::Cube,
    }
}

/// How wide a tab is when nothing is squeezed, in points: for telling whether the row fits.
/// `named`: the tab names its server (Settings → Layout); else its group does. `working`: an
/// agent is at work in it (the quiet ring after its name).
fn natural_width(st: &StripTab, label: &str, server: &str, named: bool, working: bool) -> f32 {
    let mut w = 10.0 + theme::ICON + 7.0 + rune::text::advance(label, theme::TAB_NAME) + 10.0;
    if named {
        w += 7.0 + rune::text::advance(server, theme::TAB_SERVER);
    }
    if let Some(env) = st.env.filter(|_| named) {
        w += 7.0 + env_chip_width(env);
    }
    if let Some(badge) = st.tab.badge() {
        let font = if badge == Badge::NeedsYou { theme::NEEDS } else { theme::TAB_STATE };
        // Asleep is its moon alone (`kit::tab`).
        w += if badge == Badge::Asleep { 7.0 + 11.0 } else { 7.0 + 16.0 + rune::text::advance(badge.label(), font) };
    } else if working {
        w += 7.0 + 10.0;
    }
    // The cross, on every tab (`kit::tab`).
    w += 7.0 + 14.0;
    w + 2.0 + if st.starts_group { 11.0 } else { 0.0 }
}

/// How wide a crowded tab is at its narrowest, and what leads it (the divider and its
/// machine's name, when it starts a group or the run in sight). What it has to say is only
/// its mark there (`kit::tab`), counted at its widest.
fn crowded_width(st: &StripTab, label: &str, server: &str, leads: bool, env: Option<&EnvLabel>, working: bool) -> (f32, f32) {
    let mut own = 8.0 + theme::ICON + 6.0 + rune::text::advance(label, theme::TAB_NAME) + 8.0;
    if st.tab.badge().is_some() {
        own += 6.0 + 14.0;
    } else if working {
        own += 6.0 + 10.0;
    }
    // Only the tab you're on keeps its cross in a crowded row (`kit::tab`).
    if st.active {
        own += crate::kit::tab::close_room();
    }
    let mut lead = if st.starts_group { 11.0 + 2.0 } else { 0.0 };
    if leads {
        lead += 12.0 + rune::text::advance(server, theme::GROUP) + 2.0;
        if let Some(env) = env {
            lead += 7.0 + env_chip_width(env);
        }
    }
    // The tab you're on keeps its name (up to a cap); the others give way first.
    (own.min(if st.active { ACTIVE_TAB } else { MIN_TAB }) + 2.0, lead)
}

/// The tabs the row along the top holds: every machine's, or none while they are under each
/// server in the sidebar (Settings → Layout → Where tabs go). Then the row is only "+", as
/// 0.10's top bar had no strip at all.
fn in_row(s: &Shell) -> Vec<StripTab<'_>> {
    if s.bench.tabs_under_servers() {
        return Vec::new();
    }
    s.bench.strip()
}

/// How the top row shares its room between the title and the tabs, decided once a frame from
/// one measurement (the room the two share), so neither reacts to the other a frame late.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Fit {
    /// The tabs are crowded: squeezed, each machine named once.
    pub(crate) crowded: bool,
    /// The title shows the machine's address.
    pub(crate) address: bool,
    /// The width the row of tabs gets; `None` before the first layout.
    pub(crate) room: Option<f32>,
}

/// Crowded, and the title's address, for tabs `needed` points wide beside a title `title`
/// points wide (and `address` more with its address) in `middle` points. `was`: crowded and
/// with the address last frame; loosening again waits for [`SLACK`] points to spare.
pub(crate) fn decide(middle: f32, title: f32, address: Option<f32>, needed: f32, was: (bool, bool)) -> (bool, bool) {
    let (was_crowded, had_address) = was;
    let fits = |w: f32, loosen: bool| w + if loosen { SLACK } else { 0.0 } <= middle;
    let address = address.is_some_and(|a| fits(needed + title + a, !had_address));
    let crowded = !address && !fits(needed + title, was_crowded);
    (crowded, address)
}

/// Every tab's name and its machine's, as they may be drawn (streamer mode), an agent in front
/// named after it ("Terminal 2 (Claude Code)", as in 0.10).
fn tab_names(s: &Shell, strip: &[StripTab]) -> Vec<(String, String)> {
    strip.iter().map(|st| (s.text_shown(&sshee_app::agents::tab_label(st.tab)).into_owned(), s.server_shown(&st.tab.server, st.server_name).into_owned())).collect()
}

pub(crate) fn working(s: &Shell, tab: &sshee_app::workbench::Tab) -> bool {
    s.screens.agent.working(tab.id)
}

pub(crate) fn connecting(tab: &sshee_app::workbench::Tab) -> bool {
    tab.link == sshee_app::workbench::Link::Connecting
}

pub(crate) fn went_quiet(s: &Shell, tab: &sshee_app::workbench::Tab) -> bool {
    s.screens.agent.agents.attention(tab.id).is_some_and(|a| a.kind == sshee_term::attention::AttentionKind::Done)
}

/// What the program in the tab's terminal says of its progress (OSC 9;4).
pub(crate) fn progress(s: &Shell, tab: &sshee_app::workbench::Tab) -> Option<sshee_term::osc::Progress> {
    s.pane(tab.id).and_then(|p| p.progress())
}

/// A mark after the name (working, connecting, went quiet, a program's progress): the room
/// one takes.
fn marked(s: &Shell, st: &StripTab) -> bool {
    working(s, st.tab) || connecting(st.tab) || went_quiet(s, st.tab) || progress(s, st.tab).is_some()
}

/// How wide the row is with nothing squeezed, "+" included.
fn needed_width(s: &Shell, strip: &[StripTab], names: &[(String, String)]) -> f32 {
    // Settings: every tab names its server (the default), or a server is named once, before
    // its group, as in a crowded row.
    let named = s.settings.more().tabs_name_server;
    strip
        .iter()
        .zip(names)
        .enumerate()
        .map(|(i, (st, (label, server)))| {
            // A group's name before its first tab, where tabs don't name their server.
            let lead = if !named && (i == 0 || st.starts_group) { 12.0 + rune::text::advance(server, theme::GROUP) } else { 0.0 };
            natural_width(st, label, server, named, marked(s, st)) + lead
        })
        .sum::<f32>()
        + theme::CONTROL
        + 2.0
}

/// This frame's [`Fit`]: as the widths say, but not changing under the pointer while what the
/// row held still keeps "+" in the window.
pub(crate) fn fit(s: &Shell) -> Fit {
    let strip = in_row(s);
    let row = &s.row;
    let (title, address) = super::title::widths(s);
    let Some(middle) = s.bar_room else {
        row.compact.set(false);
        row.address.set(address.is_some());
        return Fit { crowded: false, address: address.is_some(), room: None };
    };
    let needed = if strip.is_empty() { 0.0 } else { needed_width(s, &strip, &tab_names(s, &strip)) };
    let held = row.held.as_ref().map(|h| (h.compact, h.address && address.is_some()));
    let keeps = |(crowded, shown): (bool, bool)| crowded || needed + title + if shown { address.unwrap_or(0.0) } else { 0.0 } <= middle;
    let (crowded, shown) = match held.filter(|h| keeps(*h)) {
        Some(h) => h,
        None => decide(middle, title, address, needed, (row.compact.get(), row.address.get())),
    };
    let crowded = crowded && !strip.is_empty();
    row.compact.set(crowded);
    row.address.set(shown);
    let room = middle - title - if shown { address.unwrap_or(0.0) } else { 0.0 };
    Fit { crowded, address: shown, room: Some(room.max(0.0)) }
}

/// The row of tabs, as `fit` shares the bar. `window_active`: the window is in front.
pub(crate) fn strip(s: &Shell, t: &'static Theme, window_active: bool, fit: Fit) -> El {
    let strip = in_row(s);
    let row = &s.row;
    let names = tab_names(s, &strip);
    let working = |st: &StripTab| working(s, st.tab);
    let connecting = |st: &StripTab| connecting(st.tab);
    let went_quiet = |st: &StripTab| went_quiet(s, st.tab);
    let marked = |st: &StripTab| marked(s, st);
    let named = s.settings.more().tabs_name_server;
    let compact = fit.crowded;
    let grouped = compact || !named;
    // While Settings is over the page, no tab is the one on screen.
    let in_settings = s.screens.settings.open;
    let current = s.bench.current_key();
    let active_at = strip.iter().position(|st| st.active);
    // The tabs sharing the page with the one you're on.
    let partners: Vec<TabId> = s.bench.on_screen().into_iter().filter(|id| Some(*id) != s.bench.active_id()).collect();

    // The tabs in sight.
    let env_of = |st: &StripTab| s.bench.server(&st.tab.server).and_then(|sv| sv.env.as_ref());
    let plus = theme::CONTROL + 2.0;
    let widths = |from: usize| -> Vec<f32> {
        strip
            .iter()
            .zip(&names)
            .enumerate()
            .map(|(i, (st, (label, server)))| {
                let (own, lead) = crowded_width(st, label, server, i == from || i == 0 || st.starts_group, env_of(st), marked(st));
                own + lead
            })
            .collect()
    };
    let mut sight = 0..strip.len();
    if let (true, Some(room)) = (compact, fit.room) {
        if widths(0).iter().sum::<f32>() + plus > room {
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
    // The narrowest each tab in sight may be: crowded, its own crowded width; roomy, its
    // natural one.
    let least = |st: &StripTab, label: &str, server: &str| {
        if compact {
            crowded_width(st, label, server, false, None, marked(st)).0 - 2.0
        } else {
            natural_width(st, label, server, !grouped, marked(st)) - 2.0 - if st.starts_group { 11.0 } else { 0.0 }
        }
    };
    // Held still under the pointer, as wide as each tab was; the tab you're on at least wide
    // enough for its cross. In a crowded row only while that still fits: "+" never leaves the
    // window (a roomy row was checked by `fit`).
    let mut held: HashMap<TabId, f32> = HashMap::new();
    if let Some(h) = &row.held {
        for i in sight.clone() {
            let (st, (label, server)) = (&strip[i], &names[i]);
            if let Some(width) = h.widths.get(&st.tab.id) {
                held.insert(st.tab.id, if st.active { width.max(least(st, label, server)) } else { *width });
            }
        }
    }
    if let (true, Some(room)) = (compact, fit.room) {
        let widths = widths(sight.start);
        let mut used = if hidden { 2.0 * plus } else { plus };
        for i in sight.clone() {
            let st = &strip[i];
            let (own, _) = crowded_width(st, &names[i].0, &names[i].1, false, None, marked(st));
            used += held.get(&st.tab.id).map_or(widths[i], |w| widths[i] - own + w + 2.0);
        }
        if used > room {
            held.clear();
        }
    }

    // A tab being dragged along the row: how far each is drawn from its place, worked out from
    // where the tabs were laid out (the order doesn't change until it's dropped).
    let shifts = s.drag_marks().map(|(shifts, _)| shifts).unwrap_or_default();
    let dragged = s.row.drag.filter(|g| g.moved).map(|g| g.id);

    let mut tabs = rune::row()
        .key("tabs")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .items_center()
        .gap(2.0)
        .role("tablist")
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
            tabs = tabs.child(group_name(t, server, env_of(st), current == Some(st.tab.server.as_str())).key(format!("group-{}", id.0)));
        }
        let spec = Tab {
            icon: tab_icon(st.tab),
            name: label,
            server: (!grouped).then_some(server.as_str()),
            env: st.env.filter(|_| !grouped),
            badge: st.tab.badge(),
            working: working(st),
            connecting: connecting(st),
            went_quiet: went_quiet(st),
            progress: progress(s, st.tab),
            active: st.active && !in_settings,
            window_active,
            compact,
        };
        // The whole name, which a crowded tab cuts, what a crowded tab's mark means, and what
        // else a tab can be asked to do.
        let state = st.tab.badge().filter(|b| compact || *b == Badge::Asleep).map(|b| format!(" ({})", b.label())).unwrap_or_default();
        let tip = format!("{label} on {server}{state}. Right-click for more, middle-click to close");
        let mut e = tab(t, &spec, move |s: &mut Shell, tx| s.close_tab(id, tx))
            .key(format!("tab-{}", id.0))
            .on_press_tx(move |s: &mut Shell, tx| s.focus_tab(id, tx))
            .on_middle_press(move |s: &mut Shell, tx| s.close_tab(id, tx))
            .help(tip)
            // Resting on a waiting agent's tab brings up its question (`screens::agent`).
            .on_hover_change(move |s: &mut Shell, over, tx| crate::screens::agent::tab_hover(s, id, over, tx))
            .on_secondary_press(move |s: &mut Shell, press, tx| s.open_tab_menu(id, [press.x, press.y], tx))
            .on_drag(move |s: &mut Shell, d, tx| s.tab_drag(id, d, tx))
            .on_layout_tx(move |s: &mut Shell, rect, tx| {
                s.row.rects.insert(id, rect);
                // Only noted: nothing is drawn from it.
                tx.live_only();
            });
        if partners.contains(&id) {
            // Shown beside the tab you're on: lit a little, as a tab in a window that isn't
            // in front is.
            e = e.fill(t.hov);
        }
        if compact {
            // The others squeeze before the tab you're on does (`crowded_width`).
            e = e.min_w(least(st, label, server));
        }
        if let Some(width) = held.get(&id) {
            // Under the pointer the row keeps still: this tab is as wide as it was.
            e = e.w(*width).min_w(*width);
        }
        if let Some([dx, dy]) = shifts.get(&id).copied() {
            e = e.offset(dx, dy);
        }
        if dragged == Some(id) {
            // Lifted off the row, under the pointer exactly, over everything: the tabs it passes
            // step aside on their springs.
            e = e.offset_motion(rune::Anim::INSTANT).raise().fill(if st.active { t.sel } else { t.pop }).border(0.5, t.rim).shadow(6.0, 18.0, rune::Color::BLACK.alpha(if t.dark { 0.45 } else { 0.2 }));
            if s.sides.drop.is_some() {
                // Over the page, where the mark says it lands: it doesn't hide the mark.
                e = e.layer_opacity(0.85);
            }
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
    // "+": a terminal or Files, here or on another machine. Its shortcut opens a terminal.
    let keys = shortcut_label(Shortcut::NewTerminal, s.options.mac_keys, &s.options.shortcuts);
    let tip = keys.map_or_else(|| "New tab".to_owned(), |k| format!("New tab ({k} for a terminal)"));
    let mut plus = toolbar_button(t, "new-tab", Icon::Plus, "New tab").help(tip).on_press_tx(|s: &mut Shell, tx| s.toggle_new_tab_menu(tx));
    if s.row.new_tab.is_some() {
        // Held down while its menu is open, as a menu's button is.
        plus = plus.fill(t.hov);
    }
    tabs.child(plus)
}

/// What is drawn over the window for the row: a tab's menu, or the list of all tabs.
pub(crate) fn overlays(s: &Shell, t: &'static Theme) -> Vec<El> {
    let mut out = Vec::new();
    if let Some((_, Some([x, y, w, h]))) = s.drag_marks() {
        // Where a tab dragged along the row lands: the room the others leave it, outlined.
        out.push(
            rune::el()
                .key("tab-drop-slot")
                .absolute(x, y)
                .size(w, h)
                .radius(theme::RADIUS)
                .fill(t.ring.alpha(0.10))
                .border(1.5, t.ring.alpha(0.6))
                .a11y_hidden(),
        );
    }
    if let Some((id, at)) = s.row.menu.filter(|(id, _)| s.bench.tab(*id).is_some()) {
        out.push(rune::el().key("tab-menu-at").absolute(at[0], at[1]).size(1.0, 1.0).a11y_hidden());
        out.push(tab_menu(s, t, id));
    }
    if s.row.list {
        out.push(tab_list(s, t));
    }
    if let Some(menu) = &s.row.new_tab {
        out.push(new_tab_menu(s, t, menu));
    }
    out.extend(super::title::title_menu(s, t));
    out
}

/// The "+" menu, under "+".
fn new_tab_menu(s: &Shell, t: &'static Theme, menu: &NewTabMenu) -> El {
    let named = |key: &str| s.bench.server(key).map(|sv| s.server_shown(key, &sv.name).into_owned()).unwrap_or_default();
    let keys = shortcut_label(Shortcut::NewTerminal, s.options.mac_keys, &s.options.shortcuts);
    let files_keys = shortcut_label(Shortcut::OpenFiles, s.options.mac_keys, &s.options.shortcuts);
    let on = menu.on.as_deref().map(named);
    let mut rows = rune::col().w_full().padding(crate::kit::menu::PAD).role("menu");
    let mut n = 0;
    for (g, group) in s.new_tab_groups().into_iter().enumerate() {
        if g > 0 {
            rows = rows.child(list_rule(t).key(format!("nt-rule-{g}")));
        }
        for pick in group {
            let state = if menu.hot == Some(n) { RowState::Hot } else { RowState::Rest };
            let tint = match &pick {
                NewPick::Machine(server) => crate::screens::server_menu::own_color(s, server),
                _ => None,
            };
            let (key, label, glyph, tail, caps) = match &pick {
                NewPick::Terminal(_) => match &on {
                    Some(name) => ("nt-terminal".to_owned(), format!("New terminal on {name}"), Icon::Terminal, "", None),
                    None => ("nt-terminal".to_owned(), "New terminal".to_owned(), Icon::Terminal, "", keys.clone()),
                },
                NewPick::Files(_) => match &on {
                    Some(name) => ("nt-files".to_owned(), format!("Files on {name}"), Icon::Folder, "", None),
                    None => ("nt-files".to_owned(), "Files".to_owned(), Icon::Folder, "", files_keys.clone()),
                },
                NewPick::Machine(server) => {
                    let at = s.bench.servers().iter().position(|sv| sv.key() == *server).unwrap_or_default();
                    let glyph = s.bench.server(server).map_or(Icon::Server, |sv| Icon::for_server(&sv.icon));
                    (format!("nt-server-{at}"), named(server), glyph, "›", None)
                }
                NewPick::Back => ("nt-back".to_owned(), "Back".to_owned(), Icon::Back, "", None),
            };
            let spec = ListRow { icon: Some(glyph), tint, label: &label, tail, keys: caps.as_deref(), state, ..ListRow::default() };
            let i = n;
            let mut row = list_row(t, &spec)
                .key(key)
                .role("menuitem")
                .on_pointer_move(move |s: &mut Shell, _| {
                    if let Some(menu) = s.row.new_tab.as_mut() {
                        menu.hot = Some(i);
                    }
                })
                .on_press_tx(move |s: &mut Shell, tx| s.new_tab_pick(pick.clone(), false, tx));
            if state == RowState::Rest {
                row = row.hover(t.hov2);
            }
            rows = rows.child(row);
            n += 1;
        }
    }
    let label = match &on {
        Some(name) => format!("New tab on {name}"),
        None => "New tab".to_owned(),
    };
    popover(t, NEW_WIDTH)
        .key("new-tab-menu")
        .label(label)
        .anchored_fit("new-tab", Side::Bottom, Align::Start, 6.0)
        .role("menu")
        .focusable(true)
        .no_focus_ring()
        .on_key(|s: &mut Shell, ev, tx| s.new_tab_key(ev, tx))
        // A long list of servers scrolls inside the room the window leaves it.
        .child(crate::kit::scroll_bar::ScrollBars::scroll_bars(rune::col().key("new-tab-rows").w_full().shrink().min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows), t))
        .modal_tx(true, |s: &mut Shell, tx| s.close_tab_overlays(tx))
}

/// A tab's menu: side by side, a new terminal or Files on its machine, duplicate, and the
/// three ways to close. The key caps are the shortcuts as Settings has them.
fn tab_menu(s: &Shell, t: &'static Theme, id: TabId) -> El {
    let mut rows = Vec::new();
    let mut n = 0;
    for item in tab_menu_items(s, id) {
        let Some(item) = item else {
            rows.push(menu_separator(t));
            continue;
        };
        let state = match (item.on, s.row.menu_hot == Some(n)) {
            (false, _) => RowState::Off,
            (true, true) => RowState::Hot,
            (true, false) => RowState::Rest,
        };
        let mut e = menu_row(t, item.label, item.keys.as_deref(), state).key(item.key);
        if item.on {
            let (pick, at) = (item.pick, n);
            e = e.on_pointer_move(move |s: &mut Shell, _| s.row.menu_hot = Some(at)).on_press_tx(move |s: &mut Shell, tx| s.tab_pick(id, pick, tx));
            n += 1;
        }
        rows.push(e);
    }
    popover(t, MENU_WIDTH)
        .key("tab-menu")
        .anchored("tab-menu-at", Side::Bottom, Align::Start, 0.0)
        .role("menu")
        .focusable(true)
        .no_focus_ring()
        .on_key(|s: &mut Shell, ev, tx| s.tab_menu_key(ev, tx))
        .child(menu(rows))
        .modal_tx(true, |s: &mut Shell, tx| s.close_tab_overlays(tx))
}

/// A row of a tab's menu.
struct MenuItem {
    key: &'static str,
    label: &'static str,
    keys: Option<String>,
    pick: TabPick,
    on: bool,
}

/// A tab's menu: side by side, a new terminal or Files on its machine, duplicate, and the
/// three ways to close; `None` is a separator. The key caps are the shortcuts as Settings has
/// them.
fn tab_menu_items(s: &Shell, id: TabId) -> Vec<Option<MenuItem>> {
    let keys = |shortcut: Shortcut| shortcut_label(shortcut, s.options.mac_keys, &s.options.shortcuts);
    let active = s.bench.active_id();
    let paired = s.bench.layout().group_of(id).is_some();
    // Beside the tab on screen; for the tab on screen itself, beside its neighbour.
    let beside = match active {
        // With no other tab, the shortcut opens a terminal beside it.
        Some(a) if a == id => true,
        Some(a) => Layout::can_pair(&s.bench, a, id),
        None => false,
    };
    let row = |key: &'static str, label: &'static str, keys: Option<String>, pick: TabPick, on: bool| Some(MenuItem { key, label, keys, pick, on });
    let side = if paired {
        row("tab-apart", "Back to its own tab", None, TabPick::Apart, true)
    } else if active == Some(id) {
        row("tab-beside", "Show side by side", keys(Shortcut::SideBySide), TabPick::SideBySide, beside)
    } else {
        row("tab-beside", "Show beside this one", None, TabPick::SideBySide, beside)
    };
    // A terminal's folder, in Files (its pane says where it is).
    let browse = s.pane(id).is_some().then(|| row("tab-browse", "Browse files in this folder", None, TabPick::BrowseFiles, true)).flatten();
    let mut items = vec![
        side,
        // Under the tab on screen, for any other tab (0.10's "Show below this one").
        row("tab-below", "Show below this one", None, TabPick::Below, beside && active != Some(id)),
        None,
        row("tab-new-terminal", "New terminal", keys(Shortcut::NewTerminal), TabPick::NewTerminal, true),
        row("tab-files", "Files", keys(Shortcut::OpenFiles), TabPick::Files, true),
    ];
    if browse.is_some() {
        items.push(browse);
    }
    items.extend([
        row("tab-duplicate", "Duplicate tab", None, TabPick::Duplicate, true),
        None,
        row("tab-close", "Close tab", keys(Shortcut::CloseTab), TabPick::Close, true),
        row("tab-close-others", "Close other tabs", None, TabPick::CloseOthers, !layout::others(&s.bench, id).is_empty()),
        // Under each server the list runs down: what's after it is below.
        row("tab-close-right", if s.bench.tabs_under_servers() { "Close tabs below" } else { "Close tabs to the right" }, None, TabPick::CloseRight, !layout::to_the_right(&s.bench, id).is_empty()),
    ]);
    items
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
        let label = s.text_shown(&sshee_app::agents::tab_label(st.tab)).into_owned();
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
        .child(crate::kit::scroll_bar::ScrollBars::scroll_bars(rune::col().key("tab-list-rows").w_full().shrink().min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(menu(rows)), t))
        .modal_tx(true, |s: &mut Shell, tx| s.close_tab_overlays(tx))
}
