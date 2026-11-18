//! Files as drawn: a toolbar (bookmarks, back, forward, up, the path, refresh, new folder,
//! upload), the list (name, size, modified), the file open beside it, and over the window a
//! menu, a question or the bookmarks.
//!
//! ```text
//! files         column
//! ├── toolbar   40 pt: ★ ‹ › ↑  / home / deploy / shop            ⟳ ＋ ⤒
//! └── row
//!     ├── list      header (Name, Size, Modified), a note, the rows, the status line
//!     └── preview   the open file: its name, Save, the text or the picture
//! ```
//!
//! Nothing here decides a rule: what a click selects, what a key does, what a menu offers and
//! every sentence are sshee-app's (`sshee_app::files`); what the screen does is `super`'s
//! functions. This module keeps only what belongs to drawing: the text being typed, which menu
//! or question is up.
//!
//! Streamer mode: every name and path drawn here (folders in the path, rows, the open file,
//! questions, menus, bookmarks) goes through `Shell::text_shown`, so a user's name in
//! `/home/deploy` is dots while streaming. The open file's own text isn't masked.
//!
//! Keys reach the list through the window (`Shell::key`), not through focus: nothing in the
//! list takes the keyboard, so arrows work the moment the tab is on screen, and a text field
//! (a new name, the open file) has the keys only while it is being typed in.

use rune::tok::TextStyle;
use rune::{col, el, row, spacer, text, text_input, Align, Cx, Drag, El, Phase, Picked, FileDialogOptions, GrowthSpec, Key, KeyEvent, PointerPress, ScrollSpec, Side, SurfaceFit, TextEdit, Tx};
use sshee_app::files::kinds;
use sshee_app::files::layout::{self, Columns, Toolbar};
use sshee_app::files::bookmarks;
use sshee_app::files::browser::{key_does, Click, KeyDoes, ListKey, Problem, OPEN_TERMINAL, TRY_AGAIN};
use sshee_app::files::menu::{Action, MenuRow};
use sshee_app::files::ops::{self, Confirm, DropOn};
use sshee_app::files::{bytes, glyph, modified_text, Body, Glyph, Notice, SortKey, Tone};
use sshee_app::paths;
use sshee_app::workbench::{Tab, TabId, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::button::toolbar_button;
use crate::kit::dialog;
use crate::kit::push::{push_button, Push};
use crate::kit::menu::{menu, menu_row, menu_row_destructive, RowState};
use crate::kit::popover::popover;
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// A row of the list, the header above it and the line under it.
const ROW: f32 = 26.0;
const TOOLBAR: f32 = 40.0;
/// The columns after the name.
const SIZE_W: f32 = 76.0;
const DATE_W: f32 = 132.0;
const MODE_W: f32 = 90.0;
/// The menu's least and most width: it widens to its longest line in between.
const MENU_W: f32 = 232.0;
const MENU_MAX_W: f32 = 360.0;
/// The grip on the open file's left edge, dragged to change its width.
const GRIP_W: f32 = 6.0;

/// What a name is being typed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Naming {
    NewFolder,
    NewFile,
    /// A .tar.gz of these rows.
    Archive(Vec<String>),
    Rename(String),
}

/// What waits behind "Discard your changes?".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    Close,
    Open(String),
    Go(String),
    Back,
    Forward,
    Up,
}

/// Where dragged rows are: over a row of the list, or over "..".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Over {
    Row(usize),
    UpRow,
}

/// How far the pointer travels before a press on a row is a drag.
const DRAG_FROM: f32 = 6.0;

/// How big an SVG is drawn, in points, at twice the pixels: Rune caps it at 4096 px.
const SVG_DRAWN: f32 = 1024.0;

/// What a yes to the question on screen does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// These rows of `dir` on `server`: where they were when the question was asked.
    Delete { server: String, dir: String, names: Vec<String> },
    Discard(After),
}

/// A context menu that is open: where, and for which row (`None`: the folder itself).
pub struct Menu {
    at: [f32; 2],
    clicked: Option<String>,
    rows: Vec<MenuRow>,
    /// A toolbar button it hangs from ("+", "⋯"), instead of the pointer.
    anchor: Option<&'static str>,
}

/// What only the drawing needs to keep for one Files tab.
pub struct Ui {
    /// The open file's text as typed, and the revision of the file it was taken from.
    draft: TextEdit,
    draft_of: Option<u64>,
    /// Where each line of a file too big to edit starts and ends, for showing it by rows.
    lines: Vec<(usize, usize)>,
    naming: Option<(Naming, TextEdit)>,
    menu: Option<Menu>,
    confirm: Option<(Confirm, Pending)>,
    /// Rows being dragged to a folder of this list, and the row under the pointer when they
    /// may be dropped there (`None`: nowhere they could go).
    dragging: Option<(Vec<String>, Option<Over>)>,
    /// Those rows are past the window's edge: letting go downloads them (0.10.13).
    out: bool,
    /// The folder row files from another app are dragged over (`super::dropped` into it).
    drop_row: Option<usize>,
    /// The bookmarks are open.
    places: bool,
    /// Where the last context menu opened: "Open in…" lists the editors there.
    last_menu_at: [f32; 2],
    /// The open picture, decoded by Rune on its own thread.
    picture: Option<(u64, rune::image::Image)>,
    pictures: Option<rune::ImageCache>,
    /// The address bar as a field: a path being typed.
    pub(super) path_edit: Option<TextEdit>,
    /// The filter field's text (the browser filters by it as it's typed).
    pub(super) filter: TextEdit,
    /// Markdown or SVG shown as its source, not drawn.
    pub(super) source: bool,
    /// The row of the list of places the keys are on.
    pub(super) place_at: Option<usize>,
    /// Files from another app are dragged over the list.
    pub(super) drop_over: bool,
    /// The open file's width when its edge was taken, and now, while it's dragged.
    grip: Option<(f64, f64)>,
    /// The pane's width, as last laid out.
    pub(super) width: f32,
    /// How far the path is scrolled sideways by the wheel.
    path_x: f32,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            draft: TextEdit::new(""),
            draft_of: None,
            lines: Vec::new(),
            naming: None,
            menu: None,
            confirm: None,
            dragging: None,
            out: false,
            drop_row: None,
            places: false,
            last_menu_at: [0.0, 0.0],
            picture: None,
            pictures: None,
            path_edit: None,
            filter: TextEdit::new(""),
            source: false,
            place_at: None,
            drop_over: false,
            grip: None,
            width: 0.0,
            path_x: 0.0,
        }
    }
}

impl Ui {
    /// The tab went to another folder: a question, menu or name field about the last one's
    /// rows closes, so it can't act on this one's.
    pub(super) fn folder_changed(&mut self) {
        if matches!(self.confirm, Some((_, Pending::Delete { .. }))) {
            self.confirm = None;
        }
        self.naming = None;
        self.menu = None;
        self.dragging = None;
    }

    /// What a yes to the question on screen would do.
    #[cfg(test)]
    pub(super) fn pending(&self) -> Option<&Pending> {
        self.confirm.as_ref().map(|(_, p)| p)
    }

    /// Types `text` into the open file, as a person would.
    #[cfg(test)]
    pub(super) fn type_draft(&mut self, text: &str) {
        self.draft = TextEdit::new(text);
    }

    /// The open file's text as typed so far.
    pub(crate) fn draft_text(&self) -> &str {
        &self.draft.text
    }
}

/// The open file's text: the interface's code style, on 18-point lines.
fn code_style() -> TextStyle {
    theme::CODE.lh(LINE)
}

/// A line of the open file's text, in points.
const LINE: f32 = 18.0;

/// Where the text starts inside the field (its padding).
const TEXT_PAD: f32 = 14.0;

/// How far below the preview's top the first marked line lands: three lines of what comes
/// before it stay in sight (0.10's).
const MARK_AT: f32 = 3.0 * LINE;

/// The open file's lines a path named (first and last), while they are still those lines:
/// gone once the text is edited, as 0.10 did.
pub(super) fn marked_lines(s: &Shell, tab: TabId) -> Option<(u32, u32)> {
    let state = s.screens.files.get(&tab)?;
    let editor = state.editor.as_ref()?;
    let (path, lines) = state.marked.as_ref()?;
    (*path == editor.path() && editor.text().is_some() && dirty(s, tab).is_none()).then_some(*lines)
}

/// The band behind marked lines: the accent, faintly, with a stronger edge on its left.
fn band(t: &Theme) -> El {
    el().fill(t.blue.alpha(0.10)).border_sides([0.0, 0.0, 0.0, 2.0], t.blue.alpha(0.7)).a11y_hidden()
}

/// The element a preview's marked lines are scrolled to, by what shows the file.
fn marked_key(s: &Shell, tab: TabId) -> Option<String> {
    let state = s.screens.files.get(&tab)?;
    let editor = state.editor.as_ref()?;
    let (first, _) = marked_lines(s, tab)?;
    let markdown = kinds::is_markdown(editor.name()) && !state.ui.source;
    Some(match editor.body() {
        Body::Text { .. } if markdown => format!("md-band-{}", tab.0),
        Body::Text { .. } if editor.can_edit() => format!("lines-band-{}", tab.0),
        Body::Text { .. } => format!("line-{}", first.saturating_sub(1)),
        _ => return None,
    })
}

/// A file opened at some lines scrolls to them once its text is on screen (`files::tick`, at
/// the start of a frame).
pub(super) fn scroll_to_marked(s: &mut Shell, tx: &mut Tx) {
    let due: Vec<TabId> = s.screens.files.iter().filter(|(_, st)| st.scroll_to_marked && st.editor.as_ref().is_some_and(|e| !e.is_loading())).map(|(id, _)| *id).collect();
    for tab in due {
        if let Some(key) = marked_key(s, tab) {
            tx.scroll_key_to(key, MARK_AT);
        }
        if let Some(st) = s.screens.files.get_mut(&tab) {
            st.scroll_to_marked = false;
        }
    }
}

/// After anything that may have changed the open file: the field takes the text the server
/// has (a first read, "load theirs"), and a picture starts decoding.
pub fn sync(s: &mut Shell) {
    let waker = s.waker();
    for state in s.screens.files.values_mut() {
        let ui = &mut state.ui;
        let Some(editor) = &state.editor else {
            if ui.draft_of.take().is_some() {
                ui.draft = TextEdit::new("");
                ui.lines.clear();
                ui.picture = None;
            }
            continue;
        };
        let rev = editor.revision();
        if ui.draft_of == Some(rev) && !matches!((editor.body(), &ui.picture), (Body::Image(_), None)) {
            continue;
        }
        ui.draft_of = Some(rev);
        ui.picture = None;
        ui.lines.clear();
        match editor.body() {
            Body::Text { saved, .. } if editor.can_edit() => ui.draft = TextEdit::new(saved.clone()),
            Body::Text { saved, .. } => {
                ui.draft = TextEdit::new("");
                let mut at = 0;
                ui.lines = saved.split('\n').map(|l| (at, { at += l.len() + 1; at - 1 })).collect();
            }
            Body::Image(bytes) => {
                if let Some(waker) = &waker {
                    let cache = ui.pictures.get_or_insert_with(|| rune::ImageCache::new(waker.clone()));
                    ui.picture = Some((rev, cache.load(bytes.clone())));
                }
            }
            _ => ui.draft = TextEdit::new(""),
        }
        // An SVG is text, and drawn too: as it's saved, on Rune's image thread (its `svg`
        // feature; vendor/rune.patch keeps it from reading this computer's files).
        if let (Body::Text { saved, .. }, true, Some(waker)) = (editor.body(), kinds::is_svg(editor.name()), &waker) {
            let cache = ui.pictures.get_or_insert_with(|| rune::ImageCache::new(waker.clone()));
            ui.picture = Some((rev, cache.svg(saved.as_bytes().to_vec(), SVG_DRAWN, 2.0)));
        }
    }
}

// ---- what the screen does, with the questions that come first

fn dirty(s: &Shell, tab: TabId) -> Option<&str> {
    let state = s.screens.files.get(&tab)?;
    let editor = state.editor.as_ref()?;
    (editor.can_edit() && editor.is_dirty(&state.ui.draft.text)).then(|| editor.name())
}

fn run_after(s: &mut Shell, tab: TabId, after: After) {
    match after {
        After::Close => super::close_file(s, tab),
        After::Open(name) => super::open(s, tab, &name),
        After::Go(to) => super::go_now(s, tab, &to),
        After::Back => super::back_now(s, tab),
        After::Forward => super::forward_now(s, tab),
        After::Up => super::up_now(s, tab),
    }
    sync(s);
}

/// Does `after`, unless what was typed into the open file would be lost: then it asks first.
pub(super) fn guarded(s: &mut Shell, tab: TabId, after: After) {
    match dirty(s, tab).map(ops::discard_question) {
        Some(question) => {
            if let Some(state) = s.screens.files.get_mut(&tab) {
                state.ui.confirm = Some((question, Pending::Discard(after)));
            }
        }
        None => run_after(s, tab, after),
    }
}

/// Yes or no to the question on screen.
pub(super) fn answer_confirm(s: &mut Shell, tab: TabId, yes: bool) {
    let Some((_, pending)) = ui(s, tab).and_then(|ui| ui.confirm.take()) else { return };
    match pending {
        // Only on the server it was asked about; in the folder it was asked in.
        Pending::Delete { server, dir, names } if yes => {
            if s.screens.files.get(&tab).is_some_and(|st| st.browser.server() == server) {
                super::delete(s, tab, &dir, &names);
            }
        }
        Pending::Discard(after) if yes => run_after(s, tab, after),
        _ => {}
    }
}

fn ui(s: &mut Shell, tab: TabId) -> Option<&mut Ui> {
    s.screens.files.get_mut(&tab).map(|st| &mut st.ui)
}

/// The names an action on `clicked` is for: the selection when it's part of it, else itself.
fn targets(s: &Shell, tab: TabId, clicked: &str) -> Vec<String> {
    let Some(state) = s.screens.files.get(&tab) else { return Vec::new() };
    let selected = state.browser.selected();
    if selected.len() > 1 && state.browser.is_selected(clicked) {
        selected.to_vec()
    } else {
        vec![clicked.to_owned()]
    }
}

pub(super) fn ask_delete(s: &mut Shell, tab: TabId, names: Vec<String>) {
    let on_server = super::on_server(s, tab);
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    let entries: Vec<_> = names.iter().filter_map(|n| state.browser.entry(n)).collect();
    if !entries.is_empty() {
        let question = ops::delete_question(&entries, on_server);
        let (server, dir) = (state.browser.server().to_owned(), state.browser.dir().to_owned());
        state.ui.confirm = Some((question, Pending::Delete { server, dir, names }));
    }
}

fn start_naming(s: &mut Shell, tab: TabId, what: Naming) {
    let dir = s.screens.files.get(&tab).map(|st| st.browser.dir().to_owned()).unwrap_or_default();
    let Some(ui) = ui(s, tab) else { return };
    let initial = match &what {
        Naming::NewFolder => ops::NEW_FOLDER.to_owned(),
        Naming::NewFile => ops::NEW_FILE.to_owned(),
        Naming::Archive(names) => ops::archive_name(names, &dir),
        Naming::Rename(name) => name.clone(),
    };
    let mut field = TextEdit::new(initial.clone());
    // The name without its ending is selected, so typing replaces just that.
    let stem = match (&what, initial.rfind('.')) {
        (Naming::Archive(_), _) => initial.len() - ".tar.gz".len(),
        (Naming::Rename(_) | Naming::NewFile, Some(dot)) if dot > 0 => dot,
        _ => initial.len(),
    };
    field.set_selection(0, stem);
    field.focused = true;
    ui.naming = Some((what, field));
}

fn finish_naming(s: &mut Shell, tab: TabId, tx: &mut Tx) {
    let Some((what, field)) = ui(s, tab).and_then(|ui| ui.naming.take()) else { return };
    let name = field.text.trim().to_owned();
    let ok = match &what {
        _ if name.is_empty() => true,
        Naming::NewFolder => super::new_folder(s, tab, &name),
        Naming::NewFile => super::new_file(s, tab, &name),
        Naming::Archive(names) => super::archive(s, tab, names, &name),
        Naming::Rename(from) => name == *from || super::rename(s, tab, from, &name),
    };
    // A name that can't be used stays in the field, under the note that says why.
    if !ok {
        if let Some(ui) = ui(s, tab) {
            ui.naming = Some((what, field));
        }
    } else {
        tx.blur();
    }
    tx.instant();
}

fn pick_upload(tab: TabId, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Upload files".into(), ..Default::default() };
    // Several at once, as 0.10's picker took them.
    tx.open_files(options, move |s: &mut Shell, result, _| match result {
        Ok(paths) if !paths.is_empty() => super::upload(s, tab, paths),
        Ok(_) => {}
        Err(e) => super::note(s, Notice::new(Tone::Error, "Couldn't open the file picker").body(e.to_string())),
    });
}

/// Copies a path, and says so.
fn copied(s: &mut Shell, path: String, tx: &mut Tx) {
    tx.copy(path.clone());
    s.toast(crate::shell::Toast::info("Copied the path").body(path));
}

fn run_action(s: &mut Shell, tab: TabId, action: Action, clicked: Option<String>, tx: &mut Tx) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    let dir = state.browser.dir().to_owned();
    let path = clicked.as_deref().map(|n| state.browser.path_of(n));
    match (action, clicked) {
        (Action::Open | Action::OpenPreview, Some(name)) => guarded(s, tab, After::Open(name)),
        (Action::Download, Some(name)) => super::download(s, tab, &targets(s, tab, &name)),
        (Action::Rename, Some(name)) => start_naming(s, tab, Naming::Rename(name)),
        (Action::Unpack, Some(name)) => super::unpack(s, tab, &name),
        (Action::CopyPath, Some(_)) => copied(s, path.unwrap_or_default(), tx),
        (Action::ToggleBookmark, Some(_)) => super::toggle_bookmark(s, tab, &path.unwrap_or_default()),
        (Action::Delete, Some(name)) => ask_delete(s, tab, targets(s, tab, &name)),
        (Action::NewFolder, _) => start_naming(s, tab, Naming::NewFolder),
        (Action::Upload, _) => pick_upload(tab, tx),
        (Action::BookmarkHere, _) => super::toggle_bookmark(s, tab, &dir),
        (Action::CopyFolderPath, _) => copied(s, dir, tx),
        (Action::ToggleHidden, _) => {
            if let Some(state) = s.screens.files.get_mut(&tab) {
                let show = !state.browser.show_hidden();
                state.browser.set_show_hidden(show);
            }
        }
        (Action::Refresh, _) => super::refresh(s, tab),
        (Action::NewFile, _) => start_naming(s, tab, Naming::NewFile),
        (Action::Archive, Some(name)) => start_naming(s, tab, Naming::Archive(targets(s, tab, &name))),
        // From "+": the selection, else everything shown.
        (Action::Archive, None) => {
            let names = s.screens.files.get(&tab).map(|st| {
                let picked: Vec<String> = st.browser.selected_entries().iter().map(|e| e.name.clone()).collect();
                if picked.is_empty() { st.browser.rows().map(|r| r.entry.name.clone()).collect() } else { picked }
            });
            if let Some(names) = names.filter(|n| !n.is_empty()) {
                start_naming(s, tab, Naming::Archive(names));
            }
        }
        (Action::CopyToServer, Some(name)) => super::open_copy(s, tab, &targets(s, tab, &name)),
        (Action::PasteFiles, _) => tx.read_clipboard(|s: &mut Shell, content, _| {
            super::paste(s, content);
        }),
        (Action::MainTerminalHere, clicked) => super::main_terminal_here(s, tab, &clicked.map_or(dir, |_| path.unwrap_or_default()), tx),
        (Action::TerminalHere, clicked) => super::terminal_here(s, tab, &clicked.map_or(dir, |_| path.unwrap_or_default()), tx),
        (Action::Back, _) => guarded(s, tab, After::Back),
        (Action::Forward, _) => guarded(s, tab, After::Forward),
        (Action::Up, _) => guarded(s, tab, After::Up),
        (Action::OpenInEditor, Some(name)) => super::edit::open_in(s, tab, &name, None),
        (Action::OpenInOther, Some(name)) => {
            // "Open in…": the same place, now listing the editors.
            let rows = sshee_app::files::menu::editor_menu(&s.screens.edits.found);
            if let Some(ui) = ui(s, tab) {
                ui.menu = Some(Menu { at: ui.last_menu_at, clicked: Some(name), rows, anchor: None });
            }
        }
        (Action::OpenWith(i), Some(name)) => {
            let editor = s.screens.edits.found.get(i as usize).cloned();
            super::edit::open_in(s, tab, &name, editor);
        }
        (Action::ChooseApp, Some(name)) => super::edit::choose_app(Some((tab, name)), tx),
        (Action::StopEditing, Some(name)) => {
            if let Some(id) = super::edit::session_of(s, tab, &name) {
                super::edit::stop(s, &id);
            }
        }
        _ => {}
    }
    tx.instant();
}

/// A right-click or a toolbar button in a Files pane beside another makes it the pane you're
/// in, as a click on its page does: what opens takes the keys.
fn into_pane(s: &mut Shell, tab: TabId, tx: &mut Tx) {
    if s.bench.active_id() != Some(tab) {
        s.focus_tab(tab, tx);
    }
}

fn open_menu(s: &mut Shell, tab: TabId, clicked: Option<&str>, at: PointerPress, tx: &mut Tx) {
    into_pane(s, tab, tx);
    if let (Some(name), Some(state)) = (clicked, s.screens.files.get_mut(&tab)) {
        state.browser.context_click(name);
    }
    let rows = super::menu_for(s, tab, clicked);
    if let Some(ui) = ui(s, tab) {
        ui.last_menu_at = [at.x, at.y];
        ui.menu = Some(Menu { at: [at.x, at.y], clicked: clicked.map(str::to_owned), rows, anchor: None });
    }
    tx.instant();
}

/// The row an element under the pointer belongs to.
fn over(picked: Option<&Picked>) -> Option<Over> {
    picked?.path().split('/').find_map(|part| match part {
        "files-up-row" => Some(Over::UpRow),
        _ => part.strip_prefix("row-")?.parse().ok().map(Over::Row),
    })
}

/// The folder `names` would move into when let go over `over`, if they may.
fn drop_target(s: &Shell, tab: TabId, names: &[String], over: Over) -> Option<String> {
    let b = &s.screens.files.get(&tab)?.browser;
    let on = match over {
        Over::Row(i) => DropOn::Row(b.row(i)?.entry),
        Over::UpRow => DropOn::UpRow,
    };
    ops::drop_folder(b.dir(), names, on)
}

/// A press on a row that travels: the row (or the selection it's part of) is dragged, the
/// folder under the pointer lights up, and letting go there moves them into it.
fn drag(s: &mut Shell, tab: TabId, name: &str, d: &mut Drag, tx: &mut Tx) {
    let [x, y] = d.window_point();
    match d.phase {
        Phase::Down => {}
        Phase::Move => {
            if d.dx.hypot(d.dy) < DRAG_FROM {
                return;
            }
            let names = targets(s, tab, name);
            // Nothing of the window under the pointer: it's outside.
            tx.element_at(x, y, move |s: &mut Shell, picked, tx| {
                if let Some(ui) = ui(s, tab) {
                    ui.out = picked.is_none();
                }
                tx.instant();
            });
            tx.element_at_in("files-list", x, y, move |s: &mut Shell, picked, tx| {
                let to = over(picked.as_ref()).filter(|o| drop_target(s, tab, &names, *o).is_some());
                if let Some(ui) = ui(s, tab) {
                    ui.dragging = Some((names, to));
                }
                tx.instant();
            });
        }
        Phase::Up => {
            let Some((names, _)) = ui(s, tab).and_then(|ui| {
                ui.out = false;
                ui.dragging.take()
            }) else {
                return;
            };
            if d.cancelled() {
                return;
            }
            // Let go outside the window: a server's files download there (0.10.13). The
            // pointer is the window's while the button is held, so its place is known.
            let out = names.clone();
            tx.element_at(x, y, move |s: &mut Shell, picked, tx| {
                if picked.is_none() {
                    super::download_here(s, tab, &out);
                }
                tx.instant();
            });
            tx.element_at_in("files-list", x, y, move |s: &mut Shell, picked, tx| {
                if let Some(to) = over(picked.as_ref()).and_then(|o| drop_target(s, tab, &names, o)) {
                    super::move_into(s, tab, &names, &to);
                }
                tx.instant();
            });
        }
    }
    tx.instant();
}

/// The row a move landed on comes into view.
fn follow(s: &mut Shell, tab: TabId, tx: &mut Tx) {
    if let Some(i) = s.screens.files.get_mut(&tab).and_then(|st| st.browser.take_scroll()) {
        tx.scroll_into_view(format!("row-{i}"));
    }
    tx.instant();
}

/// A key while a Files tab is on screen and no text field has it (`Shell::key`). True: taken.
pub fn key(s: &mut Shell, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(tab) = s.bench.active().filter(|t| t.kind == TabKind::Files).map(|t| t.id) else { return false };
    let Some(state) = s.screens.files.get(&tab) else { return false };
    // A name or the file being typed, or a question on screen: the keys are theirs.
    // Save, wherever the keyboard is in the open file (the formatted view too), on any layout.
    let s_key = crate::shell::keys::key_name(ev, tx.physical_key(), tx.key_base()).as_deref() == Some("s");
    if s_key && ev.mods.cmd && !ev.mods.shift && !ev.mods.alt && state.editor.is_some() {
        save(s, tab);
        return true;
    }
    if state.ui.naming.is_some() || state.ui.draft.focused || state.ui.filter.focused || state.ui.path_edit.is_some() || state.ui.confirm.is_some() || state.ui.menu.is_some() || state.replace.is_some() || state.paste.is_some() || state.copy.is_some() {
        return false;
    }
    if state.ui.places {
        return places_key(s, tab, ev, tx);
    }
    if state.peek.is_some() {
        if matches!(ev.key, Key::Escape | Key::Left) {
            if let Some(st) = s.screens.files.get_mut(&tab) {
                st.peek = None;
            }
            tx.instant();
            return true;
        }
        return false;
    }
    let selected = state.browser.selected().to_vec();
    // → on a folder looks inside it.
    if ev.key == Key::Right && !ev.mods.alt {
        if let [name] = selected.as_slice() {
            if state.browser.entry(name).is_some_and(|e| e.is_dir()) {
                let i = state.browser.index_of(name).unwrap_or(0);
                toggle_peek(s, tab, name, None, i, tx);
                return true;
            }
        }
    }
    let key = match &ev.key {
        Key::Up => ListKey::Up,
        Key::Down => ListKey::Down,
        Key::Left => ListKey::Left,
        Key::Right => ListKey::Right,
        Key::Enter => ListKey::Enter,
        Key::Backspace => ListKey::Backspace,
        Key::Delete => ListKey::Delete,
        Key::Escape => ListKey::Escape,
        Key::Function(2) if selected.len() == 1 => {
            start_naming(s, tab, Naming::Rename(selected[0].clone()));
            return true;
        }
        Key::Command(c) => ListKey::Char(*c),
        Key::Text(t) if t.chars().count() == 1 => ListKey::Char(t.chars().next().unwrap_or(' ')),
        _ => return false,
    };
    let Some(does) = key_does(key, ev.mods.cmd, ev.mods.alt, selected.len()) else { return false };
    match does {
        KeyDoes::Back => guarded(s, tab, After::Back),
        KeyDoes::Forward => guarded(s, tab, After::Forward),
        KeyDoes::FolderAbove => guarded(s, tab, After::Up),
        KeyDoes::Open => {
            if let [name] = selected.as_slice() {
                guarded(s, tab, After::Open(name.clone()));
            }
        }
        KeyDoes::Delete => ask_delete(s, tab, selected),
        KeyDoes::Rename => {
            if let [name] = selected.as_slice() {
                start_naming(s, tab, Naming::Rename(name.clone()));
            }
        }
        // The filter field takes the keyboard (it shows even in a narrow pane while it does).
        KeyDoes::Filter => {
            if let Some(ui) = ui(s, tab) {
                ui.filter.focused = true;
                let end = ui.filter.text.len();
                ui.filter.set_selection(0, end);
            }
            tx.instant();
            return true;
        }
        KeyDoes::Move(_) | KeyDoes::SelectAll | KeyDoes::ClearSelection | KeyDoes::Type(_) => {
            let now = (tx.clock() * 1000.0) as u64;
            let mut step_to = None;
            if let Some(state) = s.screens.files.get_mut(&tab) {
                let b = &mut state.browser;
                match does {
                    KeyDoes::Move(step) => {
                        // Photos go by as you move through the list.
                        let at = b.move_by(step);
                        let picture = state.editor.as_ref().is_some_and(|e| kinds::is_picture(e.name()));
                        if let Some(name) = at.and_then(|i| b.row(i)).map(|r| r.entry).filter(|e| picture && !e.is_dir() && kinds::is_picture(&e.name)).map(|e| e.name.clone()) {
                            step_to = Some(name);
                        }
                    }
                    KeyDoes::SelectAll => b.select_all(),
                    KeyDoes::ClearSelection => b.clear_selection(),
                    KeyDoes::Type(c) => drop(b.type_to_find(c, now)),
                    _ => {}
                }
            }
            if let Some(name) = step_to {
                super::open(s, tab, &name);
                sync(s);
            }
        }
    }
    follow(s, tab, tx);
    true
}

// ---- drawing

pub(super) fn glyph_icon(g: Glyph) -> Icon {
    match g {
        Glyph::Folder => Icon::Folder,
        Glyph::BrokenLink => Icon::Link,
        Glyph::Script => Icon::Script,
        Glyph::Code => Icon::Code,
        Glyph::Text => Icon::Doc,
        Glyph::Image => Icon::Image,
        Glyph::Video => Icon::Video,
        Glyph::Archive => Icon::Box,
        Glyph::File => Icon::File,
    }
}

/// A toolbar button that can't be used right now is drawn fainter and does nothing.
fn tool(t: &Theme, key: &'static str, glyph: Icon, name: &'static str, on: bool, run: impl Fn(&mut Shell, TabId, &mut Tx) + 'static, tab: TabId) -> El {
    let b = toolbar_button(t, key, glyph, name);
    if on {
        b.on_press_tx(move |s: &mut Shell, tx| {
            into_pane(s, tab, tx);
            run(s, tab, tx);
            tx.instant();
        })
    } else {
        b.opacity(0.35).disabled(true).focusable(false)
    }
}

/// A text field as the design draws one: a quiet well with the blue ring while it's typed in.
fn field(t: &Theme, edit: El) -> El {
    el().h(22.0).grow(1.0).min_w(0.0).px(6.0).radius(theme::RADIUS_ROW).fill(t.code).items_center().focus_within_ring(2.0, 0.0, t.ring).child(edit.grow(1.0).min_w(0.0))
}

/// A menu of the toolbar ("+", "⋯"), under its button.
fn tool_menu(s: &mut Shell, tab: TabId, anchor: &'static str, more: bool, tx: &mut Tx) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    let b = &state.browser;
    let local = b.server() == "local";
    let selected = b.selected_entries();
    let pins = bookmarks::pins(&s.screens.bookmarks, b.server());
    let cx = sshee_app::files::menu::Context {
        dir: b.dir(),
        clicked: None,
        selected: &selected,
        visible: b.len(),
        on_server: !local,
        on_this_windows: local && cfg!(windows),
        can_copy_out: false,
        pins,
        show_hidden: b.show_hidden(),
        editor: None,
        editing: false,
        can_back: b.can_go_back(),
        can_forward: b.can_go_forward(),
        paste_key: if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" },
    };
    let rows = if more { sshee_app::files::menu::more_menu(&cx) } else { sshee_app::files::menu::new_menu(&cx) };
    if let Some(ui) = ui(s, tab) {
        ui.menu = Some(Menu { at: [0.0, 0.0], clicked: None, rows, anchor: Some(anchor) });
    }
    tx.instant();
}

fn path_field(s: &mut Shell, tab: TabId) -> &mut TextEdit {
    match ui(s, tab).and_then(|ui| ui.path_edit.as_mut()) {
        Some(field) => field,
        None => spare(),
    }
}

fn filter_edit(s: &mut Shell, tab: TabId) -> &mut TextEdit {
    match ui(s, tab) {
        Some(ui) => &mut ui.filter,
        None => spare(),
    }
}

/// The address bar becomes a field with the path in it, all selected.
fn edit_path(s: &mut Shell, tab: TabId, tx: &mut Tx) {
    let Some(dir) = s.screens.files.get(&tab).map(|st| st.browser.dir().to_owned()) else { return };
    if let Some(ui) = ui(s, tab) {
        let mut field = TextEdit::new(dir.clone());
        field.set_selection(0, dir.len());
        field.focused = true;
        ui.path_edit = Some(field);
    }
    tx.instant();
}

/// Enter in the address bar: there (home when it's empty).
fn go_typed(s: &mut Shell, tab: TabId, tx: &mut Tx) {
    let Some(field) = ui(s, tab).and_then(|ui| ui.path_edit.take()) else { return };
    let to = field.text.trim();
    let to = if to.is_empty() { "~" } else { to }.to_owned();
    tx.blur();
    guarded(s, tab, After::Go(to));
    tx.instant();
}

/// The address bar: every folder of the path goes there; the folder you're in, or the empty
/// space after the path, turns it into a field to type or paste a path in.
fn address_bar(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let Some(state) = s.screens.files.get(&id) else { return el() };
    let well = |inner: El| row().key("files-address").grow(1.0).min_w(0.0).h(26.0).ml(4.0).radius(7.0).fill(t.code).items_center().child(inner);
    if let Some(edit) = &state.ui.path_edit {
        let input = text_input(edit, theme::CODE)
            .key("files-path-field")
            .single_line()
            .color(t.l1)
            .label_static("Folder path")
            .on_edit(move |s: &mut Shell| path_field(s, id))
            .on_submit(move |s: &mut Shell, tx| go_typed(s, id, tx))
            .on_input_key(move |s: &mut Shell, ev, tx| {
                if ev.key != Key::Escape {
                    return false;
                }
                if let Some(ui) = ui(s, id) {
                    ui.path_edit = None;
                }
                tx.blur();
                tx.instant();
                true
            });
        return well(row().grow(1.0).min_w(0.0).h_full().px(8.0).gap(6.0).items_center().focus_within_ring(2.0, 0.0, t.ring).radius(7.0).child(icon(Icon::Drive, 13.0, t.l2)).child(input.grow(1.0).min_w(0.0)));
    }
    let dir = state.browser.dir().to_owned();
    let crumbs = paths::crumbs(&dir);
    let last = crumbs.len().saturating_sub(1);
    let mut trail = row().key("files-path").shrink().min_w(0.0).h_full().items_center().pl(3.0).pr(4.0).role("navigation").label_static("Folder path").scroll_x();
    for (i, c) in crumbs.iter().enumerate() {
        let root = i == 0 && c.is_root();
        if i > 0 && !(i == 1 && crumbs[0].is_root()) {
            trail = trail.child(text("/").font(theme::CODE).color(t.l2.alpha(0.6)).single_line().a11y_hidden());
        }
        let (ink, font) = if i == last { (t.l1, TextStyle::new(12.0, 500.0).mono()) } else { (t.l2on, theme::CODE) };
        let mut crumb = el().key(format!("crumb-{i}")).h(22.0).px(4.0).radius(5.0).gap(4.0).items_center().hover(t.hov2).press(t.down).role("button");
        // The start of the path draws as what it is: the disk, or home.
        crumb = match (root, c.label.as_str()) {
            (true, "/") => crumb.label_static("Top of the disk").child(icon(Icon::Drive, 13.0, t.l2on)),
            (true, "~") => crumb.label_static("Home folder").child(icon(Icon::House, 13.0, t.l2on)),
            _ => crumb.child(text(s.text_shown(&c.label).into_owned()).font(font).color(ink).single_line()),
        };
        if root && i != last {
            // The first slash looks like the way to the top, so it is one.
            crumb = crumb.child(text("/").font(theme::CODE).color(t.l2.alpha(0.6)).single_line().a11y_hidden());
        }
        if i == last {
            crumb = crumb.help("Type a path").on_press_tx(move |s: &mut Shell, tx| edit_path(s, id, tx));
        } else {
            let to = c.path.clone();
            crumb = crumb.help(format!("Go to {}", s.text_shown(&c.path))).on_press_tx(move |s: &mut Shell, tx| {
                guarded(s, id, After::Go(to.clone()));
                tx.instant();
            });
        }
        trail = trail.child(crumb);
    }
    // The wheel moves a long path sideways.
    let trail = trail.on_wheel(move |s: &mut Shell, w: rune::WheelEvent, tx| {
        if w.dy == 0.0 || w.dx != 0.0 {
            return false;
        }
        let Some(ui) = ui(s, id) else { return false };
        ui.path_x = (ui.path_x - w.dy).clamp(0.0, 4000.0);
        tx.scroll_to_instant("files-path", ui.path_x, 0.0);
        true
    });
    let space = el().key("files-path-space").grow(1.0).min_w(12.0).h_full().help("Click a folder to go there. Click the empty space to type a path.").on_press_tx(move |s: &mut Shell, tx| edit_path(s, id, tx));
    well(trail).child(space)
}

/// The filter field: only names that hold its text are listed. Esc empties it.
fn filter_field(t: &'static Theme, edit: &TextEdit, id: TabId, width: f32) -> El {
    let input = text_input(edit, theme::NOTE)
        .key("files-filter")
        .single_line()
        .color(t.l1)
        .placeholder("Filter", t.l2)
        .label_static("Filter")
        .on_edit(move |s: &mut Shell| filter_edit(s, id))
        .on_input_change(move |s: &mut Shell, tx| {
            if let Some(st) = s.screens.files.get_mut(&id) {
                let text = st.ui.filter.text.clone();
                st.browser.set_filter(&text);
            }
            tx.instant();
        })
        .on_submit(move |_: &mut Shell, tx| tx.blur())
        .on_input_key(move |s: &mut Shell, ev, tx| {
            if !matches!(ev.key, Key::Escape | Key::Down) {
                return false;
            }
            if let Some(st) = s.screens.files.get_mut(&id) {
                if ev.key == Key::Escape {
                    st.ui.filter = TextEdit::new("");
                    st.browser.set_filter("");
                }
                st.ui.filter.focused = false;
            }
            tx.blur();
            tx.instant();
            true
        });
    row().key("files-filter-box").w(width).h(26.0).ml(4.0).px(7.0).gap(5.0).radius(7.0).fill(t.code).items_center().focus_within_ring(2.0, 0.0, t.ring).child(icon(Icon::Search, 12.0, t.l2)).child(input.grow(1.0).min_w(0.0))
}

fn toolbar(s: &Shell, id: TabId, t: &'static Theme, width: f32) -> El {
    let Some(state) = s.screens.files.get(&id) else { return el() };
    let b = &state.browser;
    let dir = b.dir().to_owned();
    let fits = Toolbar::at(width);
    let lit = |on: bool, e: El| if on { e.fill(t.hov2) } else { e };
    let mut bar = row().key("files-toolbar").w_full().h(TOOLBAR).items_center().gap(2.0).px(8.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(lit(
        state.ui.places,
        toolbar_button(t, "files-places", Icon::Star, "Bookmarks: go to a place, a bookmarked folder or a recent one").on_press_tx(move |s: &mut Shell, tx| {
            into_pane(s, id, tx);
            let home = s.screens.files.get(&id).map(|st| st.browser.server().to_owned());
            if let Some(ui) = ui(s, id) {
                ui.places = !ui.places;
                ui.place_at = None;
            }
            // The Home place shows its real path: ask for it once.
            if let Some(server) = home {
                super::peek_into(s, id, &server, "~");
            }
            tx.instant();
        }),
    ));
    if fits.moves {
        bar = bar.child(tool(t, "files-back", Icon::Back, "Back (Alt+←)", b.can_go_back(), |s, tab, _| guarded(s, tab, After::Back), id));
        if b.can_go_forward() {
            bar = bar.child(tool(t, "files-forward", Icon::ChevronRight, "Forward (Alt+→)", true, |s, tab, _| guarded(s, tab, After::Forward), id));
        }
        bar = bar.child(tool(t, "files-up", Icon::ArrowUp, "Up one folder", paths::has_parent(&dir), |s, tab, _| guarded(s, tab, After::Up), id));
    }
    bar = bar.child(address_bar(s, id, t));
    // The filter shows in a narrow pane too while it's used.
    let filtering = state.ui.filter.focused || !state.ui.filter.text.is_empty();
    if let Some(w) = fits.filter.or(filtering.then_some(120.0)) {
        bar = bar.child(filter_field(t, &state.ui.filter, id, w));
    }
    let listed = b.listing().is_some();
    if fits.upload && super::on_server(s, id) {
        bar = bar.child(tool(t, "files-upload", Icon::Upload, "Upload files", listed, |_, tab, tx| pick_upload(tab, tx), id));
    }
    if fits.refresh {
        bar = bar.child(tool(t, "files-refresh", Icon::Refresh, "Refresh", true, |s, tab, _| super::refresh(s, tab), id));
    }
    let open = |key: &'static str| state.ui.menu.as_ref().is_some_and(|m| m.anchor == Some(key));
    bar.child(lit(open("files-new"), tool(t, "files-new", Icon::Plus, "New file, folder or archive", listed, |s, tab, tx| tool_menu(s, tab, "files-new", false, tx), id)))
        .child(lit(open("files-more"), tool(t, "files-more", Icon::More, "More", true, |s, tab, tx| tool_menu(s, tab, "files-more", true, tx), id)))
}

fn header_cell(t: &Theme, key: &'static str, label: &str, on: Option<bool>, sort: SortKey, id: TabId) -> El {
    let arrow = match on {
        Some(true) => " ↑",
        Some(false) => " ↓",
        None => "",
    };
    row().key(key).h_full().items_center().role("button").child(text(format!("{label}{arrow}")).font(theme::SMALL).color(if on.is_some() { t.l2on } else { t.l2 }).single_line()).on_press_tx(
        move |s: &mut Shell, tx| {
            if let Some(state) = s.screens.files.get_mut(&id) {
                state.browser.sort_by(sort);
            }
            tx.instant();
        },
    )
}

/// The field a name is typed in, in a row's place.
fn naming_field(t: &Theme, field_edit: &TextEdit, id: TabId) -> El {
    field(
        t,
        text_input(field_edit, theme::TEXT)
            .key("files-name")
            .single_line()
            .color(t.l1)
            .label_static("Name")
            .on_edit(move |s: &mut Shell| name_field(s, id))
            .on_submit(move |s: &mut Shell, tx| finish_naming(s, id, tx))
            .on_input_key(move |s: &mut Shell, ev, tx| {
                if ev.key != Key::Escape {
                    return false;
                }
                if let Some(ui) = ui(s, id) {
                    ui.naming = None;
                }
                tx.blur();
                tx.instant();
                true
            }),
    )
}

/// A field's text, for Rune to edit. A field outlives its tab only for the frame the tab
/// closes in; what is typed then goes nowhere.
fn spare() -> &'static mut TextEdit {
    Box::leak(Box::new(TextEdit::new("")))
}

fn name_field(s: &mut Shell, tab: TabId) -> &mut TextEdit {
    match ui(s, tab).and_then(|ui| ui.naming.as_mut()) {
        Some((_, field)) => field,
        None => spare(),
    }
}

fn draft_field(s: &mut Shell, tab: TabId) -> &mut TextEdit {
    match ui(s, tab) {
        Some(ui) => &mut ui.draft,
        None => spare(),
    }
}

fn list_row() -> El {
    row().h(ROW).mx(6.0).px(10.0).gap(8.0).radius(theme::RADIUS_ROW).items_center()
}

/// One file or folder.
fn entry_row(s: &Shell, id: TabId, i: usize, t: &'static Theme) -> El {
    let Some(state) = s.screens.files.get(&id) else { return el() };
    let Some(r) = state.browser.row(i) else { return el() };
    let (e, clock) = (r.entry, state.browser.clock());
    // The selection is blue in the pane you're in, and quiet in the other one.
    let active = s.bench.active_id() == Some(id) && s.window_focused;
    let blue = r.selected && active;
    let (ink, quiet) = if blue { (t.on_color, t.on_color.alpha(0.8)) } else { (t.l1, t.l2) };
    let cols = Columns::at(list_width(s, state));
    let name = e.name.clone();
    let mut line = list_row().role("row").label(s.text_shown(&e.name).into_owned());
    line = match (r.selected, active) {
        (true, true) => line.fill(t.blue),
        (true, false) => line.fill(t.hov2),
        _ => line.hover(t.hov),
    };
    if let Some((names, to)) = &state.ui.dragging {
        if *to == Some(Over::Row(i)) {
            line = line.border(1.5, t.ring);
        } else if names.contains(&e.name) {
            line = line.opacity(0.45);
        }
    }
    line = line.child(icon(glyph_icon(glyph(e)), theme::ICON, if blue { t.on_color } else { t.l2on }));
    // », after a folder's name: what's inside, without going there. It follows the name, so
    // every name sits right by its icon and they all start in one line.
    let peeking = state.peek.as_ref().is_some_and(|(p, _)| *p == state.browser.path_of(&e.name));
    let mut arrow = el().key("peek").size(16.0, 16.0).ml(2.0).radius(4.0).items_center().justify_center();
    if e.is_dir() {
        let n = name.clone();
        arrow = arrow
            .role("button")
            .label(format!("What's in {}", s.text_shown(&e.name)))
            .help("See what's inside without going there")
            .hover(if blue { t.on_color.alpha(0.15) } else { t.hov2 })
            .child(icon(Icon::Peek, 13.0, if blue { t.on_color.alpha(0.8) } else { t.l2 }))
            .on_press_tx(move |s: &mut Shell, tx| toggle_peek(s, id, &n, None, i, tx));
        if peeking {
            arrow = arrow.fill(if blue { t.on_color.alpha(0.15) } else { t.hov2 });
        }
    }
    line = match &state.ui.naming {
        Some((Naming::Rename(from), edit)) if *from == e.name => line.child(naming_field(t, edit, id)),
        _ => {
            let name = s.text_shown(&e.name);
            let shown = if e.is_link && !e.name.is_empty() { format!("{name} ↗") } else { name.into_owned() };
            let mut named = row().grow(1.0).min_w(0.0).items_center().child(text(shown).font(theme::TEXT).color(if e.is_hidden() && !blue { t.l2on } else { ink }).single_line().truncate().shrink().min_w(0.0));
            if e.is_dir() {
                named = named.child(arrow);
            } else if let Some(mark) = super::edit::row_mark(s, state.browser.server(), &state.browser.path_of(&e.name), blue, t) {
                named = named.child(mark);
            }
            line.child(named)
        }
    };
    let size = if e.is_dir() { "—".to_owned() } else { bytes(e.size) };
    let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name.clone());
    line = line.child(el().w(SIZE_W).justify_end().child(text(size).font(theme::LIST_NUMBER).color(quiet).single_line()));
    if cols.mode {
        line = line.child(el().w(MODE_W).child(text(e.mode.clone()).font(TextStyle::new(11.0, 400.0).mono()).color(quiet).single_line().truncate()));
    }
    if cols.modified {
        line = line.child(el().w(DATE_W).child(text(modified_text(e.modified, clock.now, clock.utc_offset)).font(theme::LIST_NUMBER).color(quiet).single_line().truncate()));
    }
    if e.is_dir() {
        // Files from another app let go on a folder go into it (`super::dropped`).
        let into = name.clone();
        line = line
            .on_drop_files(move |s: &mut Shell, files, tx| {
                let folder = s.screens.files.get(&id).map(|st| st.browser.path_of(&into));
                if let Some(ui) = ui(s, id) {
                    ui.drop_row = None;
                }
                super::dropped(s, id, files.to_vec(), folder);
                tx.instant();
            })
            .on_drop_hover(move |s: &mut Shell, over, tx| {
                if let Some(ui) = ui(s, id) {
                    if over {
                        ui.drop_row = Some(i);
                    } else if ui.drop_row == Some(i) {
                        ui.drop_row = None;
                    }
                }
                tx.instant();
            });
        if state.ui.drop_row == Some(i) {
            line = line.border(1.5, t.ring);
        }
        // Resting on a folder lists it ahead, so opening it is instant.
        let n = name;
        line = line.on_hover_change(move |s: &mut Shell, inside, tx| {
            let folder = s.screens.files.get(&id).map(|st| st.browser.path_of(&n)).filter(|_| inside);
            super::point_at(s, id, folder, tx.clock());
            if inside {
                tx.wake_after(std::time::Duration::from_millis(160));
            }
        });
    }
    line.on_press_tx(move |s: &mut Shell, tx| {
        let mods = tx.mods();
        let click = if mods.shift {
            Click::Range
        } else if mods.cmd {
            Click::Toggle
        } else {
            Click::Plain
        };
        // A name being typed is done with, as when its field loses the keyboard in 0.10;
        // and the keys are the list's again, not the open file's.
        finish_naming(s, id, tx);
        tx.blur();
        if let Some(state) = s.screens.files.get_mut(&id) {
            state.browser.click(&n1, click);
            state.ui.filter.focused = false;
        }
        tx.instant();
    })
    .on_double_press_tx(move |s: &mut Shell, tx| {
        guarded(s, id, After::Open(n2.clone()));
        tx.instant();
    })
    .on_secondary_press(move |s: &mut Shell, at, tx| open_menu(s, id, Some(&n3), at, tx))
    .on_drag(move |s: &mut Shell, d, tx| drag(s, id, &n4, d, tx))
}

/// » on folder `name` (row `index`), or → on it: its list opens beside it, or closes.
fn toggle_peek(s: &mut Shell, tab: TabId, name: &str, _at: Option<[f32; 2]>, index: usize, tx: &mut Tx) {
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    let path = state.browser.path_of(name);
    if state.peek.as_ref().is_some_and(|(p, _)| *p == path) {
        state.peek = None;
    } else {
        state.browser.select_only(name);
        state.peek = Some((path.clone(), index));
        let server = state.browser.server().to_owned();
        super::peek_into(s, tab, &server, &path);
    }
    tx.instant();
}

fn centered(children: Vec<El>) -> El {
    col().w_full().grow(1.0).min_h(0.0).items_center().justify_center().gap(8.0).px(24.0).children(children)
}

/// The pane's width, as last laid out.
fn pane_width(state: &super::State) -> f32 {
    if state.ui.width > 0.0 {
        state.ui.width
    } else {
        1000.0
    }
}

/// The width the open file asks for: what its grip is being dragged to, else Settings'.
fn wanted_preview(s: &Shell, state: &super::State) -> f64 {
    state.ui.grip.map_or(s.settings.get().preview_width, |(_, now)| now)
}

/// The list's width: the pane's, less the open file's beside it, worked out the way the page
/// lays them out (not from the last layout, which hasn't seen a file that just opened).
fn list_width(s: &Shell, state: &super::State) -> f32 {
    let w = pane_width(state);
    match state.editor.as_ref().and_then(|_| layout::preview_width(w, wanted_preview(s, state))) {
        Some(p) => w - p,
        None => w,
    }
}

fn list(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let Some(state) = s.screens.files.get(&id) else { return el() };
    let b = &state.browser;
    let cols = Columns::at(list_width(s, state));
    let mut pane = col().key("files-list-pane").grow(1.0).min_w(220.0).h_full();
    let problem = b.problem();
    if let Some(Problem::Page { title, message, open_terminal }) = &problem {
        let mut buttons = row().gap(8.0).mt(6.0);
        if *open_terminal {
            buttons = buttons.child(push_button(t, OPEN_TERMINAL, Push::Quiet, true).key("files-open-terminal").on_press_tx(|s: &mut Shell, tx| s.new_terminal(tx)));
        }
        buttons = buttons.child(push_button(t, TRY_AGAIN, Push::Quiet, true).key("files-try-again").on_press_tx(move |s: &mut Shell, tx| {
            super::refresh(s, id);
            tx.instant();
        }));
        return pane.child(centered(vec![
            text(*title).font(theme::TEXT_BOLD).color(t.l1).single_line(),
            text(*message).font(theme::NOTE).color(t.l2).max_w(360.0).align(Align::Center),
            buttons,
        ]));
    }
    // The columns' names; a click sorts by one, another turns it round.
    let sort = b.sort();
    let on = |key: SortKey| (sort.key == key).then_some(sort.asc);
    let mut header = row()
        .key("files-header")
        .w_full()
        .h(ROW)
        .items_center()
        .gap(8.0)
        .pl(16.0 + theme::ICON + 8.0 + 12.0 + 8.0)
        .pr(16.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        .child(header_cell(t, "sort-name", "Name", on(SortKey::Name), SortKey::Name, id).grow(1.0))
        .child(header_cell(t, "sort-size", "Size", on(SortKey::Size), SortKey::Size, id).w(SIZE_W).justify_end());
    if cols.mode {
        header = header.child(row().w(MODE_W).h_full().items_center().child(text("Permissions").font(theme::SMALL).color(t.l2).single_line()));
    }
    if cols.modified {
        header = header.child(header_cell(t, "sort-modified", "Modified", on(SortKey::Modified), SortKey::Modified, id).w(DATE_W));
    }
    pane = pane.child(header);
    if let Some(Problem::Note { text: note, open_terminal }) = problem {
        let mut line = row().key("files-note").w_full().min_h(ROW).items_center().gap(8.0).px(16.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(icon(Icon::Warning, 12.0, t.orange)).child(
            text(note).font(theme::NOTE).color(t.l2on).single_line().truncate().grow(1.0).min_w(0.0),
        );
        if open_terminal {
            line = line.child(push_button(t, OPEN_TERMINAL, Push::Quiet, true).key("files-open-terminal").on_press_tx(|s: &mut Shell, tx| s.new_terminal(tx)));
        }
        pane = pane.child(line.child(push_button(t, TRY_AGAIN, Push::Quiet, true).key("files-try-again").on_press_tx(move |s: &mut Shell, tx| {
            super::refresh(s, id);
            tx.instant();
        })));
    }
    let mut rows = col()
        .key("files-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .py(4.0)
        .role("list")
        .label_static("Files")
        .scroll(ScrollSpec { stick_bottom: false })
        // A click on the empty space under the rows: nothing is selected.
        .on_press_tx(move |s: &mut Shell, tx| {
            finish_naming(s, id, tx);
            if let Some(state) = s.screens.files.get_mut(&id) {
                state.browser.clear_selection();
                state.peek = None;
            }
            tx.blur();
            tx.instant();
        })
        .on_secondary_press(move |s: &mut Shell, at, tx| open_menu(s, id, None, at, tx))
        // Files from another app: into the folder on screen (the platform's drag and drop
        // feeds this, `super::dropped`).
        .on_drop_files(move |s: &mut Shell, files, tx| {
            if let Some(ui) = ui(s, id) {
                ui.drop_over = false;
            }
            super::dropped(s, id, files.to_vec(), None);
            tx.instant();
        })
        .on_drop_hover(move |s: &mut Shell, over, tx| {
            if let Some(ui) = ui(s, id) {
                ui.drop_over = over;
            }
            tx.instant();
        });
    if state.ui.drop_over {
        rows = rows.border(2.0, t.ring).radius(theme::RADIUS);
    }
    if b.listing().is_none() {
        // Not listed yet: the shape of a list, so nothing jumps when it comes.
        if b.is_loading() {
            for i in 0..9 {
                let w = 120.0 + ((i * 37) % 45) as f32 * 4.0;
                rows = rows.child(list_row().key(format!("ghost-{i}")).child(el().size(theme::ICON, theme::ICON).radius(4.0).fill(t.hov2)).child(el().w(w).h(8.0).radius(4.0).fill(t.hov2)));
            }
        }
    } else {
        if b.has_up_row() {
            let lit = matches!(&state.ui.dragging, Some((_, Some(Over::UpRow))));
            let above = paths::parent(b.dir());
            rows = rows.child(
                list_row()
                    .key("files-up-row")
                    .border(1.5, if lit { t.ring } else { t.ring.alpha(0.0) })
                    .hover(t.hov)
                    .role("row")
                    .label_static("The folder above")
                    .help("Double-click to go to the folder above")
                    .child(icon(Icon::ArrowUp, theme::ICON, t.l2))
                    .child(text("..").font(theme::TEXT).color(t.l2on).single_line())
                    .on_hover_change(move |s: &mut Shell, inside, tx| {
                        super::point_at(s, id, inside.then(|| above.clone()), tx.clock());
                        if inside {
                            tx.wake_after(std::time::Duration::from_millis(160));
                        }
                    })
                    .on_press_tx(move |s: &mut Shell, tx| {
                        if let Some(state) = s.screens.files.get_mut(&id) {
                            state.browser.clear_selection();
                        }
                        tx.instant();
                    })
                    .on_double_press_tx(move |s: &mut Shell, tx| {
                        guarded(s, id, After::Up);
                        tx.instant();
                    }),
            );
        }
        if let Some((what @ (Naming::NewFolder | Naming::NewFile | Naming::Archive(_)), edit)) = &state.ui.naming {
            let glyph = match what {
                Naming::NewFolder => Icon::Folder,
                Naming::NewFile => Icon::Doc,
                _ => Icon::Box,
            };
            rows = rows.child(list_row().key("files-new-row").fill(t.hov).child(icon(glyph, theme::ICON, t.l2on)).child(naming_field(t, edit, id)));
        }
        rows = rows.child(col().key("files-rows").w_full().growth(GrowthSpec::default()).virtual_rows(b.len(), ROW, |_: &Shell, i| format!("row-{i}").into(), move |s: &Shell, i| entry_row(s, id, i, t)));
        if let Some(empty) = b.empty().filter(|_| state.ui.naming.is_none()) {
            let mut said = col().key("files-empty").w_full().items_center().gap(4.0).pt(64.0).child(text(empty.text()).font(theme::TEXT).color(t.l2on).single_line());
            if super::on_server(s, id) && b.filter().is_empty() {
                said = said.child(text(sshee_app::files::browser::DROP_HINT).font(theme::NOTE).color(t.l2).single_line());
            }
            rows = rows.child(said);
        }
    }
    pane = pane.child(rows);
    if let Some(lines) = super::edit::strip(s, id, t) {
        pane = pane.child(lines);
    }
    // The line under the list: how many, how many selected, what runs.
    let mut foot = row().key("files-status").w_full().h(theme::STATUS_HEIGHT).items_center().gap(14.0).px(16.0).border_sides([1.0, 0.0, 0.0, 0.0], t.hair2);
    if let Some(status) = b.status() {
        foot = foot.child(text(status.items).font(theme::STATUS_VALUE).color(t.l2).single_line());
        if let Some(selected) = status.selected {
            foot = foot.child(text(selected).font(theme::STATUS_VALUE).color(t.l2on).single_line());
        }
    }
    foot = foot.child(spacer());
    let moving = state.ui.dragging.as_ref().map(|(names, to)| {
        let what = if let [one] = names.as_slice() { one.clone() } else { format!("{} items", names.len()) };
        if state.ui.out && super::on_server(s, id) {
            format!("Let go to download {what}")
        } else if to.is_some() {
            format!("Let go to move {what} here")
        } else {
            format!("Drag {what} onto a folder to move it there, or out of the window to download it")
        }
    });
    let dropping = state.ui.drop_over.then(|| if super::on_server(s, id) { "Drop to upload here" } else { "Drop to copy here" });
    if let Some(busy) = dropping.or(moving.as_deref()).or(state.busy.as_deref()).or(b.is_loading().then_some("Loading…")) {
        foot = foot.child(text(busy).key("files-busy").font(theme::STATUS).color(t.l2on).single_line().truncate().shrink().min_w(0.0));
    }
    pane.child(foot)
}

/// The grip on the open file's edge: dragged, the file's pane widens or narrows (Settings'
/// `previewWidth`, as in 0.10); a double click puts it back.
fn grip(t: &'static Theme, id: TabId) -> El {
    el().key("preview-grip")
        .w(GRIP_W)
        .h_full()
        .ml(-GRIP_W / 2.0)
        .mr(-GRIP_W / 2.0)
        .hover(t.ring.alpha(0.35))
        .role("separator")
        .label_static("Resize preview")
        .help("Drag to resize the preview. Double-click to reset it.")
        .on_drag(move |s: &mut Shell, d, tx| {
            let wanted = s.settings.get().preview_width;
            match d.phase {
                Phase::Down => {
                    if let Some(ui) = ui(s, id) {
                        ui.grip = Some((wanted, wanted));
                    }
                }
                Phase::Move => {
                    if let Some(ui) = ui(s, id) {
                        if let Some((from, now)) = ui.grip.as_mut() {
                            *now = layout::dragged_width(*from, d.dx);
                        }
                    }
                }
                Phase::Up => {
                    if let Some((_, now)) = ui(s, id).and_then(|ui| ui.grip.take()) {
                        s.change_settings(tx, move |st| st.set(|v| v.preview_width = now));
                    }
                }
            }
            tx.instant();
        })
        .on_double_press_tx(|s: &mut Shell, tx| {
            s.change_settings(tx, |st| st.set(|v| v.preview_width = f64::from(layout::PREVIEW_DEFAULT)));
            tx.instant();
        })
}

/// The open file, beside the list (`width`), or in the whole pane (`None`).
fn preview(s: &Shell, id: TabId, t: &'static Theme, width: Option<f32>) -> Option<El> {
    let state = s.screens.files.get(&id)?;
    let editor = state.editor.as_ref()?;
    let on_server = super::on_server(s, id);
    let name = editor.name().to_owned();
    let dirty = dirty(s, id).is_some();
    let markdown = kinds::is_markdown(&name) && editor.text().is_some();
    let svg = kinds::is_svg(&name) && editor.text().is_some();
    let mut head = row().key("preview-head").w_full().h(TOOLBAR).items_center().gap(8.0).pl(14.0).pr(8.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(
        text(s.text_shown(&name).into_owned()).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate().shrink().min_w(0.0),
    );
    if dirty {
        head = head.child(el().key("preview-dirty").size(6.0, 6.0).radius(3.0).fill(t.blue).label_static("Unsaved changes").help("Unsaved changes"));
    }
    if let Some(size) = editor.size() {
        head = head.child(text(bytes(size)).key("preview-size").font(theme::LIST_NUMBER).color(t.l2).single_line());
    }
    // Opened at some lines: which, in the accent.
    let marked = marked_lines(s, id);
    if let Some((first, last)) = marked {
        head = head.child(text(sshee_app::prompts::lines_note(first, last)).key("preview-lines-note").font(theme::LIST_NUMBER).color(t.blue).single_line());
    }
    head = head.child(spacer());
    if markdown || svg {
        // Formatted (drawn), or as text to edit.
        let picked = usize::from(state.ui.source);
        head = head.child(crate::kit::segmented::segmented(t, "Show the file", &["Preview", "Source"], Some(picked), true, move |s: &mut Shell, i, tx| {
            if let Some(ui) = ui(s, id) {
                ui.source = i == 1;
            }
            tx.instant();
        }).key("preview-mode"));
    }
    if dirty {
        let label = if editor.is_saving() { "Saving…" } else { "Save" };
        head = head.child(push_button(t, label, Push::Quiet, true).key("preview-save").on_press_tx(move |s: &mut Shell, tx| {
            save(s, id);
            tx.instant();
        }));
    }
    if on_server {
        let n = name.clone();
        head = head.child(toolbar_button(t, "preview-download", Icon::Download, "Download").on_press_tx(move |s: &mut Shell, tx| {
            super::download(s, id, std::slice::from_ref(&n));
            tx.instant();
        }));
    }
    head = head.child(toolbar_button(t, "preview-close", Icon::Close, "Close preview").on_press_tx(move |s: &mut Shell, tx| {
        guarded(s, id, After::Close);
        tx.instant();
    }));
    let pane = col().key("files-preview").h_full().border_sides([0.0, 0.0, 0.0, 1.0], t.hair2).child(head);
    let mut pane = match width {
        Some(w) => pane.w(w),
        None => pane.grow(1.0).min_w(0.0),
    };
    let banner = |words: &str| row().w_full().px(14.0).py(7.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(text(words).font(theme::NOTE).color(t.l2on).grow(1.0).min_w(0.0));
    // The file changed on the server: nothing was written, and these are the ways on.
    if let Some(conflict) = editor.conflict() {
        let mut answers = row().gap(8.0).flex_wrap();
        for (i, (choice, label)) in conflict.choices().into_iter().enumerate() {
            answers = answers.child(push_button(t, label, if i == 0 { Push::Caution } else { Push::Quiet }, true).key(format!("conflict-{i}")).on_press_tx(move |s: &mut Shell, tx| {
                let draft = s.screens.files.get(&id).map(|st| st.ui.draft.text.clone()).unwrap_or_default();
                super::resolve(s, id, choice, &draft);
                sync(s);
                tx.instant();
            }));
        }
        pane = pane.child(
            col().key("preview-conflict").w_full().px(14.0).py(10.0).gap(8.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(
                row().gap(7.0).child(icon(Icon::Warning, 13.0, t.red).mt(2.0)).child(text(conflict.message()).font(theme::NOTE).color(t.l1).grow(1.0).min_w(0.0)),
            ).child(answers),
        );
    }
    let open_in = super::edit::chosen(s).map(|e| format!("Open in {}", e.name));
    if let Some(why) = editor.read_only() {
        let mut line = banner(why.note()).key("preview-read-only").items_center().gap(8.0);
        if let (sshee_app::files::ReadOnly::TooBigToEdit, Some(label)) = (why, &open_in) {
            let n = name.clone();
            line = line.child(push_button(t, label, Push::Quiet, true).key("preview-open-in").on_press_tx(move |s: &mut Shell, tx| {
                super::edit::open_in(s, id, &n, None);
                tx.instant();
            }));
        }
        pane = pane.child(line);
    }
    // A picture, or an SVG drawn.
    let picture = || match &state.ui.picture {
        Some((_, picture)) => {
            // Never bigger than it is: a small icon stays sharp instead of filling the pane.
            let [w, h] = picture.size().map_or([f32::MAX, f32::MAX], |[w, h]| [w as f32, h as f32]);
            el().key("preview-picture").w_full().grow(1.0).min_h(0.0).padding(14.0).items_center().justify_center().child(
                el().w_full().h_full().max_w(w).max_h(h).pixels_fit(picture.pixels(), SurfaceFit::Fit).label(format!("Picture {}", s.text_shown(&name))),
            )
        }
        None => centered(vec![crate::icons::logo_loader(20.0), text("Loading…").font(theme::NOTE).color(t.l2).single_line()]),
    };
    let body = match editor.body() {
        Body::Loading => centered(vec![crate::icons::logo_loader(20.0), text("Loading…").font(theme::NOTE).color(t.l2).single_line()]),
        // An SVG, drawn as it's saved; Source edits it.
        Body::Text { .. } if svg && !state.ui.source => picture(),
        // Markdown, formatted: from what's typed, so an edit in Source shows here.
        Body::Text { .. } if markdown && !state.ui.source => {
            let source = if editor.can_edit() { state.ui.draft.text.as_str() } else { editor.text().unwrap_or_default() };
            super::markdown_view::view(s, source, marked, &format!("md-band-{}", id.0), t).key("preview-markdown")
        }
        Body::Text { .. } if editor.can_edit() => col().key("preview-scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(
            // The marked lines' band, behind the text.
            col().w_full().children(marked.map(|(first, last)| {
                band(t)
                    .key(format!("lines-band-{}", id.0))
                    .pin(Some(0.0), Some(TEXT_PAD + (first - 1) as f32 * LINE), Some(0.0), None)
                    .h((last - first + 1) as f32 * LINE)
            })).child(
            text_input(&state.ui.draft, code_style())
                .key("preview-text")
                .w_full()
                .padding(14.0)
                .color(t.l1)
                .label(format!("Text of {}", s.text_shown(&name)))
                .on_edit(move |s: &mut Shell| draft_field(s, id))
                // Return breaks the line: this is a document, not a form.
                .on_submit(move |s: &mut Shell, _| draft_field(s, id).insert("\n"))
                .on_input_key(move |s: &mut Shell, ev, tx| {
                    // Save, on any keyboard layout.
                    let s_key = crate::shell::keys::key_name(ev, tx.physical_key(), tx.key_base()).as_deref() == Some("s");
                    if s_key && ev.mods.cmd && !ev.mods.shift && !ev.mods.alt {
                        save(s, id);
                        return true;
                    }
                    if ev.key == Key::Escape {
                        tx.blur();
                        return true;
                    }
                    false
                }),
            ),
        ),
        Body::Text { .. } => {
            let style = code_style();
            col().key("preview-scroll").w_full().grow(1.0).min_h(0.0).py(10.0).scroll(ScrollSpec { stick_bottom: false }).child(
                col().key("preview-lines").w_full().growth(GrowthSpec::default()).virtual_rows(state.ui.lines.len(), LINE, |_: &Shell, i| format!("line-{i}").into(), move |s: &Shell, i| {
                    let line = s.screens.files.get(&id).and_then(|st| {
                        let (a, b) = *st.ui.lines.get(i)?;
                        st.editor.as_ref()?.text()?.get(a..b)
                    });
                    let row = el().h(LINE).px(14.0).child(text(line.unwrap_or_default().trim_end_matches('\r')).font(style).color(t.l1).single_line());
                    // A marked line: the band behind it.
                    let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
                    match marked_lines(s, id) {
                        Some((first, last)) if (first..=last).contains(&n) => row.fill(t.blue.alpha(0.10)).border_sides([0.0, 0.0, 0.0, 2.0], t.blue.alpha(0.7)),
                        _ => row,
                    }
                }),
            )
        }
        Body::Image(_) => picture(),
        _ => {
            let (title, line) = editor.message(on_server).unwrap_or_default();
            let mut said = vec![text(title).font(theme::TEXT_BOLD).color(t.l1).single_line(), text(line).font(theme::NOTE).color(t.l2).max_w(340.0).align(Align::Center)];
            let mut buttons = row().gap(8.0).mt(6.0);
            if let (false, Some(label)) = (matches!(editor.body(), Body::Failed(_)), &open_in) {
                let n = name.clone();
                buttons = buttons.child(push_button(t, label, Push::Quiet, true).key("preview-open-in").on_press_tx(move |s: &mut Shell, tx| {
                    super::edit::open_in(s, id, &n, None);
                    tx.instant();
                }));
            }
            if on_server {
                let n = name.clone();
                buttons = buttons.child(push_button(t, "Download", Push::Quiet, true).key("preview-get").on_press_tx(move |s: &mut Shell, tx| {
                    super::download(s, id, std::slice::from_ref(&n));
                    tx.instant();
                }));
            }
            said.push(buttons);
            centered(said)
        }
    };
    Some(pane.child(body))
}

fn save(s: &mut Shell, tab: TabId) {
    let Some(draft) = s.screens.files.get(&tab).map(|st| st.ui.draft.text.clone()) else { return };
    super::save(s, tab, &draft);
}

/// A Files tab's page.
pub fn page(s: &Shell, tab: &Tab, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    let id = tab.id;
    let Some(state) = s.screens.files.get(&id) else { return col().key("files").w_full().h_full() };
    let width = pane_width(state);
    let beside = layout::preview_width(width, wanted_preview(s, state));
    let mut body = row().key("files-body").w_full().grow(1.0).min_h(0.0);
    match (preview(s, id, t, beside), beside) {
        // A pane too narrow for both: the open file takes it.
        (Some(open), None) => body = body.child(open),
        (Some(open), Some(_)) => body = body.child(list(s, id, t)).child(grip(t, id)).child(open),
        (None, _) => body = body.child(list(s, id, t)),
    }
    col().key("files").w_full().h_full().child(toolbar(s, id, t, width)).child(body).on_layout(move |s: &mut Shell, rect: [f32; 4]| {
        if let Some(st) = s.screens.files.get_mut(&id) {
            st.ui.width = rect[2];
        }
    })
}

// ---- over the window

fn close_menu(s: &mut Shell, id: TabId, tx: &mut Tx) {
    if let Some(ui) = ui(s, id) {
        ui.menu = None;
        ui.places = false;
    }
    tx.instant();
}

/// A question as streamer mode lets it be drawn: the names in it dotted while streaming.
pub(super) fn shown(s: &Shell, q: &Confirm) -> Confirm {
    let mut q = q.clone();
    q.title = s.text_shown(&q.title).into_owned();
    q.body = s.text_shown(&q.body).into_owned();
    q.lines = q.lines.iter().map(|l| s.text_shown(l).into_owned()).collect();
    q
}

/// A Files question as the kit's confirmation (`kit::dialog`): Cancel and its answer, keyed
/// `files-question` with `question-no` and `question-yes`; Esc cancels. Return answers, unless
/// the answer is a dangerous one (Delete, Discard: they can't be undone), when Return cancels.
pub(super) fn question(t: &'static Theme, q: &Confirm, yes: impl Fn(&mut Shell, &mut Tx) + 'static, no: impl Fn(&mut Shell, &mut Tx) + 'static) -> El {
    question_safe(t, q, q.danger, yes, no)
}

/// The question's card as the kit draws it: Return presses Cancel when `safe`.
fn look(q: &Confirm, safe: bool) -> dialog::Look {
    dialog::Look::yes_no(q.title.clone(), q.body.clone(), q.confirm_label, q.danger, safe).lines(q.lines.clone()).keys("question-no", "question-yes")
}

/// [`question`] whose Return presses Cancel when `safe` (something private on its way out).
fn question_safe(t: &'static Theme, q: &Confirm, safe: bool, yes: impl Fn(&mut Shell, &mut Tx) + 'static, no: impl Fn(&mut Shell, &mut Tx) + 'static) -> El {
    let look = look(q, safe);
    let (yes, no) = (std::rc::Rc::new(yes), std::rc::Rc::new(no));
    dialog::dialog(t, "files-question", &look, move |s: &mut Shell, tx: &mut Tx, i| if i == 1 { yes(s, tx) } else { no(s, tx) })
}

/// What a Files tab on screen shows over the window: its menu, its question, its bookmarks.
/// The tab you're on first; then a Files pane beside it (its toolbar's buttons don't move the
/// keyboard there, so its bookmarks or a question it asked open while you're in another pane).
pub fn overlay(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let active = s.bench.active_id();
    let mut files: Vec<TabId> = s.bench.on_screen().into_iter().filter(|id| s.bench.tab(*id).is_some_and(|tab| tab.kind == TabKind::Files)).collect();
    files.sort_by_key(|id| Some(*id) != active);
    files.into_iter().find_map(|id| tab_overlay(s, id, t, cx))
}

/// [`overlay`] for Files tab `id`: `None` when nothing of it is open over the window.
fn tab_overlay(s: &Shell, id: TabId, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let state = s.screens.files.get(&id)?;
    if let Some(q) = state.ui.confirm.as_ref().map(|(q, _)| shown(s, q)) {
        let answer = move |yes: bool| {
            move |s: &mut Shell, tx: &mut Tx| {
                answer_confirm(s, id, yes);
                tx.instant();
            }
        };
        return Some(question(t, &q, answer(true), answer(false)));
    }
    if let Some(replace) = &state.replace {
        let answer = move |yes: bool| {
            move |s: &mut Shell, tx: &mut Tx| {
                super::answer_replace(s, id, yes);
                tx.instant();
            }
        };
        return Some(question(t, &shown(s, &replace.question), answer(true), answer(false)));
    }
    if let Some(paste) = &state.paste {
        let answer = move |yes: bool| {
            move |s: &mut Shell, tx: &mut Tx| {
                super::answer_paste(s, id, yes);
                tx.instant();
            }
        };
        // Something private on its way to a server: Return cancels.
        return Some(question_safe(t, &paste.question.confirm, paste.question.risky, answer(true), answer(false)));
    }
    if state.copy.is_some() {
        return Some(copy_dialog(s, id, t));
    }
    if let Some(open) = &state.ui.menu {
        let mut rows = Vec::new();
        let mut height = 12.0;
        let mut width = MENU_W;
        for (i, r) in open.rows.iter().enumerate() {
            match r {
                MenuRow::Separator => {
                    height += 9.0;
                    rows.push(el().key(format!("menu-sep-{i}")).h(1.0).mx(8.0).my(4.0).fill(t.hair));
                }
                MenuRow::Item(item) => {
                    height += 26.0;
                    let (action, clicked) = (item.action, open.clicked.clone());
                    let label = s.text_shown(&item.label);
                    let label = match item.checked {
                        Some(true) => format!("✓ {label}"),
                        _ => label.into_owned(),
                    };
                    width = width.max(menu_width(&label, item.hint));
                    let draw = if item.destructive { menu_row_destructive } else { menu_row };
                    let line = draw(t, &label, item.hint, if item.disabled { RowState::Off } else { RowState::Rest }).key(format!("menu-{i}"));
                    rows.push(if item.disabled {
                        line
                    } else {
                        line.on_press_tx(move |s: &mut Shell, tx| {
                            close_menu(s, id, tx);
                            run_action(s, id, action, clicked.clone(), tx);
                        })
                    });
                }
            }
        }
        let card = popover(t, width).key("files-menu").child(menu(rows));
        // Under the toolbar button it came from, or at the pointer, moved in from the window's
        // edges.
        let card = match open.anchor {
            Some(anchor) => card.anchored(anchor, Side::Bottom, if anchor == "files-more" { Align::End } else { Align::Start }, 4.0),
            None => {
                let [_, _, w, h] = cx.layout("window").unwrap_or([0.0, 0.0, 1280.0, 800.0]);
                let x = open.at[0].min(w - width - 8.0).max(8.0);
                let y = open.at[1].min(h - height - 8.0).max(8.0);
                card.absolute(x, y)
            }
        };
        return Some(card.modal_tx(true, move |s: &mut Shell, tx| close_menu(s, id, tx)));
    }
    if state.ui.places {
        return Some(places(s, id, t));
    }
    if state.peek.is_some() {
        return peek(s, id, t);
    }
    None
}

/// The menu's width for a line reading `label`, with `keys` at its end: the words, the row's
/// and the menu's insets and border, the key cap; between [`MENU_W`] and [`MENU_MAX_W`]
/// (longer ones end in "…").
fn menu_width(label: &str, keys: Option<&str>) -> f32 {
    let cap = keys.map_or(0.0, |k| rune::text::advance(k, theme::KEY_CAP).max(8.0) + 10.0 + 2.0 + 8.0);
    (rune::text::advance(label, theme::TEXT) + 16.0 + 2.0 * crate::kit::menu::PAD + 2.0 + cap + 4.0).ceil().clamp(MENU_W, MENU_MAX_W)
}

/// What a row of the list of places is.
#[derive(Debug, Clone, PartialEq)]
enum PlaceKind {
    /// Home, Disk, the Linux folders: hidden from the list, not removed.
    Place(&'static str),
    Pin,
    Recent,
    /// "Bookmark this folder".
    Add,
}

#[derive(Debug, Clone)]
struct PlaceRow {
    kind: PlaceKind,
    label: String,
    path: String,
    title: String,
    glyph: Icon,
}

/// The rows of the list of places, top to bottom, in their sections.
fn place_rows(s: &Shell, tab: TabId) -> Vec<(&'static str, Vec<PlaceRow>)> {
    let Some(state) = s.screens.files.get(&tab) else { return Vec::new() };
    let b = &state.browser;
    let server = b.server();
    let dir = b.dir();
    let hidden = &s.settings.get().hidden_places;
    let home = super::home_of(s, server);
    let glyph = |id: &str| match id {
        "home" => Icon::House,
        "root" => Icon::Drive,
        "/var/log" => Icon::Doc,
        "/etc" => Icon::Gear,
        "/var/www" => Icon::Globe,
        "/opt" => Icon::Box,
        _ => Icon::Folder,
    };
    let mut out = Vec::new();
    let places: Vec<PlaceRow> = bookmarks::shown_places(server != "local", home.as_deref(), hidden)
        .into_iter()
        .map(|p| PlaceRow { glyph: glyph(p.id), kind: PlaceKind::Place(p.id), label: p.label.to_owned(), path: p.path, title: p.title })
        .collect();
    if !places.is_empty() {
        out.push(("Places", places));
    }
    let mut pins: Vec<PlaceRow> =
        bookmarks::pins(&s.screens.bookmarks, server).iter().map(|p| PlaceRow { kind: PlaceKind::Pin, label: paths::name(p).to_owned(), path: p.clone(), title: p.clone(), glyph: Icon::Star }).collect();
    if !bookmarks::is_pinned(&s.screens.bookmarks, server, dir) {
        pins.push(PlaceRow { kind: PlaceKind::Add, label: "Bookmark this folder".into(), path: dir.to_owned(), title: dir.to_owned(), glyph: Icon::Plus });
    }
    out.push(("Bookmarks", pins));
    if !hidden.iter().any(|h| h == bookmarks::RECENT_ID) {
        let recent: Vec<PlaceRow> = bookmarks::recent_shown(&s.screens.bookmarks, server, dir)
            .into_iter()
            .map(|p| PlaceRow { kind: PlaceKind::Recent, label: paths::name(&p).to_owned(), title: p.clone(), path: p, glyph: Icon::Reopen })
            .collect();
        if !recent.is_empty() {
            out.push(("Recent", recent));
        }
    }
    out
}

fn place_flat(s: &Shell, tab: TabId) -> Vec<PlaceRow> {
    place_rows(s, tab).into_iter().flat_map(|(_, rows)| rows).collect()
}

/// A row of the list of places picked: it goes there (or bookmarks the folder, and the list
/// stays open to show it).
fn pick_place(s: &mut Shell, tab: TabId, row: PlaceRow, tx: &mut Tx) {
    if row.kind == PlaceKind::Add {
        super::toggle_bookmark(s, tab, &row.path);
        tx.instant();
        return;
    }
    close_menu(s, tab, tx);
    guarded(s, tab, After::Go(row.path));
}

/// The row's way out of the list: a bookmark or a recent folder removed, a place hidden.
fn take_out(s: &mut Shell, tab: TabId, row: &PlaceRow, tx: &mut Tx) {
    let Some(server) = s.screens.files.get(&tab).map(|st| st.browser.server().to_owned()) else { return };
    match row.kind {
        PlaceKind::Pin => super::remove_bookmark(s, &server, &row.path),
        PlaceKind::Recent => super::forget_recent(s, &server, &row.path),
        PlaceKind::Place(id) => super::toggle_place(s, id, tx),
        PlaceKind::Add => {}
    }
    tx.instant();
}

/// Keys while the list of places is open: arrows and Enter pick, Delete takes a bookmark or a
/// recent folder out, Escape closes it.
fn places_key(s: &mut Shell, tab: TabId, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let rows = place_flat(s, tab);
    let at = s.screens.files.get(&tab).and_then(|st| st.ui.place_at).filter(|i| *i < rows.len());
    let n = rows.len();
    let to = match ev.key {
        Key::Down if n > 0 => Some(at.map_or(0, |i| (i + 1) % n)),
        Key::Up if n > 0 => Some(at.map_or(n - 1, |i| (i + n - 1) % n)),
        Key::Home | Key::PageUp if n > 0 => Some(0),
        Key::End | Key::PageDown if n > 0 => Some(n - 1),
        Key::Enter => {
            if let Some(row) = at.and_then(|i| rows.get(i)).cloned() {
                pick_place(s, tab, row, tx);
            }
            return true;
        }
        Key::Delete | Key::Backspace => {
            if let Some(row) = at.and_then(|i| rows.get(i)).filter(|r| matches!(r.kind, PlaceKind::Pin | PlaceKind::Recent)).cloned() {
                take_out(s, tab, &row, tx);
            }
            return true;
        }
        Key::Escape | Key::Tab => {
            close_menu(s, tab, tx);
            return true;
        }
        _ => return false,
    };
    if let Some(ui) = ui(s, tab) {
        ui.place_at = to;
    }
    tx.instant();
    true
}

/// Bookmarks, places and recent folders, under the toolbar's star.
fn places(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let at = s.screens.files.get(&id).and_then(|st| st.ui.place_at);
    let dir = s.screens.files.get(&id).map(|st| st.browser.dir().to_owned()).unwrap_or_default();
    let mut card = popover(t, 260.0).key("files-places-card").anchored("files-places", Side::Bottom, Align::Start, 6.0).padding(6.0).role("menu").label_static("Bookmarks and places");
    let mut n = 0;
    for (title, rows) in place_rows(s, id) {
        let mut head = row().w_full().px(8.0).pt(6.0).pb(2.0).items_center().child(text(title).font(theme::SMALL).color(t.l2).single_line().grow(1.0));
        if title == "Recent" {
            head = head.child(
                el().key("place-hide-recent").size(18.0, 18.0).radius(4.0).items_center().justify_center().hover(t.hov2).role("button").label_static("Hide recent folders").help("Hide recent folders").child(icon(Icon::EyeOff, 12.0, t.l2)).on_press_tx(
                    |s: &mut Shell, tx| {
                        super::toggle_place(s, bookmarks::RECENT_ID, tx);
                        tx.instant();
                    },
                ),
            );
        }
        card = card.child(head);
        for r in rows {
            let i = n;
            n += 1;
            let here = r.path == dir && r.kind != PlaceKind::Add;
            let state = if at == Some(i) { RowState::Hot } else { RowState::Rest };
            let mut line = row()
                .key(format!("place-{i}"))
                .w_full()
                .h(28.0)
                .px(8.0)
                .gap(8.0)
                .radius(theme::RADIUS_ROW)
                .items_center()
                .role("menuitem")
                .label(s.text_shown(&r.label).into_owned())
                .help(s.text_shown(&r.title).into_owned());
            line = if state == RowState::Hot { line.fill(t.hov2) } else { line.hover(t.hov) };
            let ink = if r.kind == PlaceKind::Add { t.l2on } else { t.l1 };
            line = line
                .child(icon(r.glyph, 14.0, if r.kind == PlaceKind::Pin { t.orange } else { t.l2on }))
                .child(text(s.text_shown(&r.label).into_owned()).font(if here { theme::TEXT_BOLD } else { theme::TEXT }).color(ink).single_line().truncate().shrink().min_w(0.0))
                .child(spacer());
            let (glyph, what) = match r.kind {
                PlaceKind::Pin => (Some(Icon::Close), "Remove bookmark".to_owned()),
                PlaceKind::Recent => (Some(Icon::Close), "Remove from recent".to_owned()),
                PlaceKind::Place(_) => (Some(Icon::EyeOff), format!("Hide {}", r.label)),
                PlaceKind::Add => (None, String::new()),
            };
            if let Some(glyph) = glyph {
                let row_again = r.clone();
                line = line.child(
                    el().key(format!("place-out-{i}")).size(18.0, 18.0).radius(4.0).items_center().justify_center().hover(t.hov2).role("button").label(what.clone()).help(what).child(icon(glyph, 12.0, t.l2)).on_press_tx(
                        move |s: &mut Shell, tx| take_out(s, id, &row_again, tx),
                    ),
                );
            }
            let pick = r.clone();
            line = line.on_hover_change(move |s: &mut Shell, inside, tx| {
                if inside {
                    if let Some(ui) = ui(s, id) {
                        ui.place_at = Some(i);
                    }
                    tx.instant();
                }
            });
            card = card.child(line.on_press_tx(move |s: &mut Shell, tx| pick_place(s, id, pick.clone(), tx)));
        }
    }
    if !s.settings.get().hidden_places.is_empty() {
        card = card.child(el().h(1.0).mx(8.0).my(4.0).fill(t.hair)).child(menu_row(t, "Show everything", None, RowState::Rest).key("place-show-all").on_press_tx(|s: &mut Shell, tx| {
            super::show_every_place(s, tx);
            tx.instant();
        }));
    }
    card.modal_tx(true, move |s: &mut Shell, tx| close_menu(s, id, tx))
}

/// What's in the folder looked into with », beside its row: a click on a folder goes there,
/// on a file opens it there.
fn peek(s: &Shell, id: TabId, t: &'static Theme) -> Option<El> {
    const SHOWN: usize = 200;
    let state = s.screens.files.get(&id)?;
    let (path, index) = state.peek.clone()?;
    let server = state.browser.server();
    let mut card = popover(t, 300.0).key("files-peek").anchored(format!("row-{index}"), Side::Right, Align::Start, 6.0).padding(4.0).role("menu").label(format!("What's in {}", s.text_shown(&path)));
    let close = move |s: &mut Shell, tx: &mut Tx| {
        if let Some(st) = s.screens.files.get_mut(&id) {
            st.peek = None;
        }
        tx.instant();
    };
    let Some(cached) = s.screens.dirs.get(server, &path) else {
        return Some(card.child(row().h(28.0).px(10.0).items_center().child(text("Looking inside…").font(theme::NOTE).color(t.l2).single_line())).modal_tx(true, close));
    };
    let entries = &cached.listing.entries;
    let order = sshee_app::files::browser::sorted(entries, sshee_app::files::Sort::default());
    let shown: Vec<usize> = order.into_iter().filter(|&i| state.browser.show_hidden() || !entries[i].is_hidden()).collect();
    if shown.is_empty() {
        card = card.child(row().h(28.0).px(10.0).items_center().child(text("This folder is empty").font(theme::NOTE).color(t.l2).single_line()));
    }
    let mut list = col().w_full().max_h(380.0).scroll(ScrollSpec { stick_bottom: false });
    for (n, &i) in shown.iter().take(SHOWN).enumerate() {
        let e = &entries[i];
        let full = paths::join(&path, &e.name);
        let dir = e.is_dir();
        let mut line = row().key(format!("peek-{n}")).w_full().h(28.0).px(8.0).gap(8.0).radius(7.0).items_center().hover(t.hov).role("menuitem").help(s.text_shown(&e.name).into_owned());
        line = line.child(icon(glyph_icon(glyph(e)), 14.0, t.l2on)).child(text(s.text_shown(&e.name).into_owned()).font(theme::TEXT).color(if e.is_hidden() { t.l2on } else { t.l1 }).single_line().truncate().shrink().min_w(0.0)).child(spacer());
        if !dir {
            line = line.child(text(bytes(e.size)).font(theme::LIST_NUMBER).color(t.l2).single_line());
        }
        list = list.child(line.on_press_tx(move |s: &mut Shell, tx| {
            if let Some(st) = s.screens.files.get_mut(&id) {
                st.peek = None;
            }
            if dir {
                guarded(s, id, After::Go(full.clone()));
            } else {
                // The file opens, in its folder.
                super::show_path(s, id, &full);
                sync(s);
            }
            tx.instant();
        }));
    }
    if shown.len() > SHOWN {
        let to = path.clone();
        list = list.child(menu_row(t, &format!("And {} more: open the folder", shown.len() - SHOWN), None, RowState::Rest).key("peek-more").on_press_tx(move |s: &mut Shell, tx| {
            if let Some(st) = s.screens.files.get_mut(&id) {
                st.peek = None;
            }
            guarded(s, id, After::Go(to.clone()));
            tx.instant();
        }));
    }
    Some(card.child(list).modal_tx(true, close))
}

/// "Copy to a folder or server": the server to copy to, the folder there, and Copy.
fn copy_dialog(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let Some(copy) = s.screens.files.get(&id).and_then(|st| st.copy.as_ref()) else { return el() };
    let on_server = super::on_server(s, id);
    let names: Vec<String> = copy.paths.iter().map(|p| s.text_shown(paths::name(p)).into_owned()).collect();
    let close = move |s: &mut Shell, tx: &mut Tx| {
        if let Some(st) = s.screens.files.get_mut(&id) {
            st.copy = None;
        }
        tx.instant();
    };
    let mut card = popover(t, 480.0).key("files-copy").padding(18.0).gap(8.0).role("dialog").label_static("Copy to a folder or server").child(
        row().gap(10.0).items_center().child(el().size(30.0, 30.0).radius(15.0).fill(t.blue.alpha(0.15)).items_center().justify_center().child(icon(Icon::Copy, 14.0, t.blue))).child(
            col().grow(1.0).min_w(0.0).gap(2.0).child(text(ops::copy_title(&names, on_server)).font(theme::TITLE).color(t.l1).single_line().truncate()).child(
                text(if names.len() > 1 { names.iter().take(4).cloned().collect::<Vec<_>>().join(", ") + if names.len() > 4 { "…" } else { "" } } else { s.text_shown(&paths::parent(&copy.paths[0])).into_owned() })
                    .font(theme::NOTE)
                    .color(t.l2)
                    .single_line()
                    .truncate(),
            ),
        ),
    );
    let targets = super::copy_targets(s);
    if targets.is_empty() {
        card = card.child(text("There's no server to copy to yet. Add one from the sidebar first.").font(theme::NOTE).color(t.l2on).wrap().w_full());
    } else {
        card = card.child(text("To").font(theme::GROUP_TITLE).color(t.l2on).single_line().mt(6.0));
        let mut chips = row().w_full().gap(6.0).flex_wrap().role("radiogroup").label_static("Server");
        for key in targets {
            let name = s.bench.server(&key).map(|sv| s.server_shown(&key, &sv.name).into_owned()).unwrap_or_else(|| key.clone());
            let on = copy.to.as_deref() == Some(&*key);
            let k = key.clone();
            let mut chip = row().key(format!("copy-to-{}", key.replace(':', "-"))).h(28.0).px(10.0).gap(6.0).radius(8.0).items_center().role("radio").label(name.clone());
            chip = if on { chip.fill(t.blue.alpha(0.14)).border(1.0, t.blue) } else { chip.fill(t.code).border(1.0, t.hair).hover(t.hov) };
            chips = chips.child(chip.child(icon(Icon::Server, 13.0, if on { t.blue } else { t.l2on })).child(text(name).font(theme::CONTROL_TEXT).color(t.l1).single_line().max_w(180.0).truncate()).on_press_tx(move |s: &mut Shell, tx| {
                super::copy_server(s, id, &k);
                tx.instant();
            }));
        }
        card = card.child(chips).child(text("Into this folder").font(theme::GROUP_TITLE).color(t.l2on).single_line().mt(6.0));
        let shown_path = copy.place.as_ref().map(|(p, _)| p.clone()).or_else(|| copy.asked.clone()).unwrap_or_default();
        let above = copy.place.as_ref().map(|(p, _)| p.clone()).filter(|p| paths::has_parent(p)).map(|p| paths::parent(&p));
        let mut up = el().key("copy-up").size(28.0, 28.0).radius(7.0).items_center().justify_center().fill(t.code).role("button").label_static("Folder above").help("Folder above").child(icon(Icon::ArrowUp, 14.0, t.l2on));
        up = match above {
            Some(to) => up.hover(t.hov2).on_press_tx(move |s: &mut Shell, tx| {
                super::copy_go(s, id, &to);
                tx.instant();
            }),
            None => up.opacity(0.4),
        };
        card = card.child(
            row().w_full().gap(6.0).items_center().child(up).child(
                row().grow(1.0).min_w(0.0).h(28.0).px(10.0).radius(7.0).fill(t.code).items_center().child(text(s.text_shown(&shown_path).into_owned()).font(theme::CODE).color(t.l1).single_line().truncate().grow(1.0).min_w(0.0)).child(if copy.asked.is_some() {
                    text("Looking…").font(theme::NOTE).color(t.l2).single_line()
                } else {
                    el()
                }),
            ),
        );
        let mut folders = col().key("copy-folders").w_full().h(168.0).padding(4.0).radius(8.0).fill(t.code).scroll(ScrollSpec { stick_bottom: false });
        match (&copy.error, &copy.place) {
            (Some(error), _) => folders = folders.child(text(s.text_shown(error).into_owned()).font(theme::NOTE).color(t.red).wrap().w_full().padding(6.0)),
            (None, Some((place, dirs))) if dirs.is_empty() && copy.asked.is_none() => {
                let _ = place;
                folders = folders.child(text("No folders inside. The files go right here.").font(theme::NOTE).color(t.l2).single_line().padding(6.0));
            }
            (None, Some((place, dirs))) => {
                for (i, d) in dirs.iter().enumerate() {
                    let to = paths::join(place, d);
                    folders = folders.child(
                        row().key(format!("copy-dir-{i}")).w_full().h(26.0).px(8.0).gap(8.0).radius(6.0).items_center().hover(t.hov).role("button").child(icon(Icon::Folder, 13.0, t.l2on)).child(text(s.text_shown(d).into_owned()).font(theme::TEXT).color(t.l1).single_line().truncate()).on_press_tx(
                            move |s: &mut Shell, tx| {
                                super::copy_go(s, id, &to);
                                tx.instant();
                            },
                        ),
                    );
                }
            }
            _ => {}
        }
        card = card.child(folders);
    }
    let ready = copy.place.is_some() && copy.asked.is_none() && copy.to.is_some();
    let card = card.child(
        row().w_full().gap(8.0).mt(8.0).child(spacer()).child(push_button(t, "Cancel", Push::Quiet, true).key("copy-cancel").on_press_tx(close)).child({
            let b = push_button(t, "Copy here", Push::Primary, ready).key("copy-go");
            if ready {
                b.default_button().on_press_tx(move |s: &mut Shell, tx| {
                    super::copy_here(s, id);
                    tx.instant();
                })
            } else {
                b
            }
        }),
    );
    el().key("files-copy-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.scrim()).items_center().justify_center().child(card.modal_tx(false, close))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_app::files::{Entry, EntryKind};

    /// What can't be undone never goes ahead on Return: Delete and Discard default to Cancel,
    /// while Replace, which loses nothing it wasn't told to, keeps Return for its yes.
    #[test]
    fn return_never_deletes_or_discards() {
        let file = Entry { name: "nginx.conf".into(), kind: EntryKind::File, is_link: false, size: 178, modified: 0, mode: "-rw-r--r--".into() };
        let delete = ops::delete_question(&[&file], true);
        for q in [delete, ops::discard_question("nginx.conf")] {
            let look = look(&q, q.danger);
            assert_eq!(look.default, Some(0), "{}", q.title);
            assert_eq!(look.choices[0].key, "question-no");
        }
        let replace = ops::replace_question("“a” is there.", sshee_app::transfers::Kind::Upload);
        assert_eq!(look(&replace, replace.danger).default, Some(1));
    }
}
