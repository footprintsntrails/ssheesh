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
    /// A file of another folder (picked in », the folder looked into) opens beside the list;
    /// the tab stays where it is.
    OpenFrom { dir: String, name: String },
    Go(String),
    Back,
    Forward,
    Up,
}

/// What dragged rows are over in a Files tab: a row of its list, "..", a folder of its path,
/// or the list itself (the folder it shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Over {
    Row(usize),
    UpRow,
    Crumb(usize),
    List,
}

/// Where dragged rows would land if let go now (0.10's `dropFor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landing {
    /// Into the folder `to` on their own machine, shown by Files tab `tab` (theirs, or another
    /// on the same machine), which lights up `over`.
    Move { tab: TabId, over: Over, to: String },
    /// Copied into the folder `to` of another server, shown by Files tab `tab`.
    Copy { tab: TabId, over: Over, server: String, to: String },
    /// Past the window's edge: a server's files download to this computer.
    Out,
}

impl Landing {
    /// What lights up in Files tab `tab`.
    fn lights(&self, tab: TabId) -> Option<Over> {
        match self {
            Landing::Move { tab: t, over, .. } | Landing::Copy { tab: t, over, .. } if *t == tab => Some(*over),
            _ => None,
        }
    }
}

/// Rows being dragged out of their list.
pub struct Dragging {
    names: Vec<String>,
    /// Where letting go now puts them; `None`: nowhere.
    landing: Option<Landing>,
    /// The pointer, in the window's points: the card flies with it.
    at: [f32; 2],
    /// The row the drag started on is a folder (the card shows one).
    folder: bool,
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
    /// Rows of `dir` on `server` dragged out of the window: downloaded into `into` (the folder
    /// under the pointer when they were let go), or Downloads.
    DownloadOut { server: String, dir: String, names: Vec<String>, into: Option<std::path::PathBuf> },
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
    /// Rows of this list being dragged: onto a folder they move there, onto another server's
    /// Files they're copied there, past the window's edge they download (0.10.13).
    dragging: Option<Dragging>,
    /// The folder row files from another app are dragged over (`super::dropped` into it).
    drop_row: Option<usize>,
    /// The bookmarks are open.
    places: bool,
    /// Their right-click menu, where it opened and the row it opened on.
    place_menu: Option<([f32; 2], Option<PlaceRow>)>,
    /// Where the last context menu opened: "Open in…" lists the editors there.
    last_menu_at: [f32; 2],
    /// The open picture, decoded by Rune on its own thread.
    picture: Option<(u64, rune::image::Image)>,
    /// The squares under a picture's see-through parts, at the size the picture is drawn.
    checker: Option<([u32; 2], rune::image::Image)>,
    /// The open picture was decoded when last looked.
    decoded: bool,
    pictures: Option<rune::ImageCache>,
    /// The address bar as a field: a path being typed.
    pub(super) path_edit: Option<TextEdit>,
    /// The filter field's text (the browser filters by it as it's typed).
    pub(super) filter: TextEdit,
    /// Markdown or SVG shown as its source, not drawn.
    pub(super) source: bool,
    /// The row of the list of places the keys are on.
    pub(super) place_at: Option<usize>,
    /// The row of the folder looked into with » that the keys are on.
    pub(super) peek_at: Option<usize>,
    /// "Copy to another folder or server": the folder there, as typed.
    pub(super) copy_typed: TextEdit,
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
            drop_row: None,
            places: false,
            place_menu: None,
            last_menu_at: [0.0, 0.0],
            picture: None,
            checker: None,
            decoded: false,
            pictures: None,
            path_edit: None,
            filter: TextEdit::new(""),
            source: false,
            place_at: None,
            peek_at: None,
            copy_typed: TextEdit::new(""),
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
        self.path_edit = None;
    }

    /// The address bar as a field, while it has the keyboard. A click anywhere else takes the
    /// keyboard from it (Rune clears `focused`), and from then on the path shows as folders
    /// again, as a browser's address bar does when it loses focus.
    pub(super) fn typing_path(&self) -> Option<&TextEdit> {
        self.path_edit.as_ref().filter(|e| e.focused)
    }

    /// The open picture was decoded since the last look: it's drawn again, now with its size.
    pub(super) fn picture_decoded(&mut self) -> bool {
        let ready = self.picture.as_ref().is_some_and(|(_, p)| p.is_ready());
        let news = ready && !self.decoded;
        self.decoded = ready;
        news
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
        After::OpenFrom { dir, name } => super::open_from(s, tab, &dir, &name),
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
        Pending::DownloadOut { server, dir, names, into } if yes && s.screens.files.get(&tab).is_some_and(|st| st.browser.server() == server) => {
            super::download_to(s, tab, &dir, &names, into);
        }
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

/// The Files tab and what of it an element under the pointer belongs to.
fn over(picked: &Picked) -> Option<(TabId, Over)> {
    let parts: Vec<&str> = picked.path().split('/').collect();
    let tab = parts.iter().find_map(|p| p.strip_prefix("files-tab-")?.parse().ok()).map(TabId)?;
    let over = parts.iter().rev().find_map(|part| match *part {
        "files-up-row" => Some(Over::UpRow),
        "files-list" => Some(Over::List),
        _ => part.strip_prefix("row-").and_then(|i| i.parse().ok()).map(Over::Row).or_else(|| part.strip_prefix("crumb-").and_then(|i| i.parse().ok()).map(Over::Crumb)),
    })?;
    Some((tab, over))
}

/// Where rows `names` of Files tab `from` would land if let go at a point: past the window's
/// edge (`outside`), or over `picked` in the page. Onto a folder of their own machine they
/// move, in any Files tab; onto a folder of another server's Files they're copied; past the
/// edge a server's files download.
fn landing(s: &Shell, from: TabId, names: &[String], outside: bool, picked: Option<&Picked>) -> Option<Landing> {
    let source = &s.screens.files.get(&from)?.browser;
    if outside {
        return (source.server() != "local").then_some(Landing::Out);
    }
    let (tab, over) = over(picked?)?;
    let b = &s.screens.files.get(&tab)?.browser;
    let to = match over {
        // Rows of their own list: a folder that isn't one of them (`ops::drop_folder`).
        Over::Row(i) if tab == from => ops::drop_folder(b.dir(), names, DropOn::Row(b.row(i)?.entry))?,
        Over::UpRow if tab == from => ops::drop_folder(b.dir(), names, DropOn::UpRow)?,
        Over::Row(i) => Some(b.row(i)?.entry).filter(|e| e.is_dir()).map(|e| b.path_of(&e.name))?,
        Over::UpRow => paths::has_parent(b.dir()).then(|| paths::parent(b.dir()))?,
        Over::Crumb(i) => paths::crumbs(b.dir()).into_iter().nth(i)?.path,
        Over::List => b.dir().to_owned(),
    };
    if b.server() == source.server() {
        let dragged: Vec<String> = names.iter().map(|n| source.path_of(n)).collect();
        ops::may_move_into(source.dir(), &dragged, &to).then_some(Landing::Move { tab, over, to })
    } else {
        // To a server; this computer's own Files take nothing from one.
        (b.server() != "local").then(|| Landing::Copy { tab, over, server: b.server().to_owned(), to })
    }
}

/// Where a drag from any Files tab would land in Files tab `tab`, lit there.
fn lit(s: &Shell, tab: TabId) -> Option<Over> {
    s.screens.files.values().find_map(|st| st.ui.dragging.as_ref()?.landing.as_ref()?.lights(tab))
}

/// What's under window point (`x`, `y`) for dragged rows, handed to `then` with whether it's
/// past the window's edge. Each Files tab on screen is looked into on its own: a server's
/// coloured frame, cards and notes drawn over the page don't hide the folder under them.
fn pick_under(s: &Shell, tx: &mut Tx, x: f32, y: f32, then: impl FnOnce(&mut Shell, bool, Option<Picked>) + 'static) {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    let outside = Rc::new(Cell::new(false));
    let found: Rc<RefCell<Option<Picked>>> = Rc::default();
    let seen = outside.clone();
    tx.element_at(x, y, move |_: &mut Shell, picked, _| seen.set(picked.is_none()));
    for id in s.bench.on_screen().into_iter().filter(|id| s.screens.files.contains_key(id)) {
        let found = found.clone();
        tx.element_at_in(format!("files-tab-{}", id.0), x, y, move |_: &mut Shell, picked, _| {
            if let Some(picked) = picked {
                found.borrow_mut().get_or_insert(picked);
            }
        });
    }
    // Read after the ones above: what they found.
    tx.element_at(x, y, move |s: &mut Shell, _, tx| {
        then(s, outside.get(), found.take());
        tx.instant();
    });
}

/// A press on a row that travels: the row (or the selection it's part of) is dragged, a card
/// with it flies with the pointer, the folder under it lights up, and letting go there moves
/// them into it (or copies them, to another server's Files). Past the window's edge, a
/// server's files download, after a question naming where.
fn drag(s: &mut Shell, tab: TabId, name: &str, d: &mut Drag, tx: &mut Tx) {
    let [x, y] = d.window_point();
    match d.phase {
        Phase::Down => {}
        Phase::Move => {
            if d.dx.hypot(d.dy) < DRAG_FROM && ui(s, tab).is_some_and(|ui| ui.dragging.is_none()) {
                return;
            }
            let names = targets(s, tab, name);
            let folder = s.screens.files.get(&tab).and_then(|st| st.browser.entry(name)).is_some_and(|e| e.is_dir());
            pick_under(s, tx, x, y, move |s, outside, picked| {
                let landing = landing(s, tab, &names, outside, picked.as_ref());
                if let Some(ui) = ui(s, tab) {
                    ui.dragging = Some(Dragging { names, landing, at: [x, y], folder });
                }
            });
        }
        Phase::Up => {
            let Some(dragging) = ui(s, tab).and_then(|ui| ui.dragging.take()) else { return };
            if d.cancelled() {
                tx.instant();
                return;
            }
            let names = dragging.names;
            pick_under(s, tx, x, y, move |s, outside, picked| match landing(s, tab, &names, outside, picked.as_ref()) {
                Some(Landing::Move { to, .. }) => super::move_into(s, tab, &names, &to),
                Some(Landing::Copy { server, to, .. }) => super::copy_dragged(s, tab, &names, &server, &to),
                Some(Landing::Out) => ask_download_out(s, tab, names),
                None => {}
            });
        }
    }
    tx.instant();
}

/// Rows let go outside the window (0.10.14). Where the system can say which folder is under
/// the pointer (asked at once: the pointer moves on while the question is up) they go there,
/// else into Downloads; it asks first, since letting go out there is easy to do by accident.
fn ask_download_out(s: &mut Shell, tab: TabId, names: Vec<String>) {
    let into = s.backend_mut().folder_under_pointer();
    let Some(state) = s.screens.files.get(&tab).filter(|st| st.browser.server() != "local") else { return };
    let (server, dir) = (state.browser.server().to_owned(), state.browser.dir().to_owned());
    let name = server_name(s, &server);
    let shown: Vec<&str> = names.iter().map(String::as_str).collect();
    let where_to = into.as_ref().map(|p| p.to_string_lossy().into_owned());
    let question = ops::download_out_question(&shown, &name, where_to.as_deref());
    if let Some(ui) = ui(s, tab) {
        ui.confirm = Some((question, Pending::DownloadOut { server, dir, names, into }));
    }
}

/// A server's name as the window shows it (dots while streaming).
fn server_name(s: &Shell, key: &str) -> String {
    s.bench.server(key).map(|sv| s.server_shown(key, &sv.name).into_owned()).unwrap_or_else(|| key.to_owned())
}

/// What letting go does now, for the card that flies with the pointer and the line under the
/// list (0.10's wording).
fn landing_text(s: &Shell, d: &Dragging, from_server: bool) -> String {
    match &d.landing {
        Some(Landing::Move { to, .. }) => {
            let folder = match paths::name(to) {
                "" => "/",
                name => name,
            };
            format!("Move to {}", s.text_shown(folder))
        }
        Some(Landing::Copy { server, .. }) => format!("Copy to {}", server_name(s, server)),
        Some(Landing::Out) => "Let go to download".to_owned(),
        None if from_server => "To a folder, or out to download".to_owned(),
        None => "To a folder to move it".to_owned(),
    }
}

/// The card that flies with the pointer while rows are dragged: a folder or a file, how many,
/// and what letting go would do. It lets the pointer through, so what's under it still counts.
fn drag_card(s: &Shell, id: TabId, t: &'static Theme) -> Option<El> {
    let state = s.screens.files.get(&id)?;
    let d = state.ui.dragging.as_ref()?;
    if d.landing == Some(Landing::Out) {
        return Some(out_hint(s, d, t));
    }
    let what = if let [one] = d.names.as_slice() { s.text_shown(one).into_owned() } else { format!("{} items", d.names.len()) };
    let ok = d.landing.is_some();
    let glyph = if d.folder { Icon::Folder } else { Icon::Doc };
    let mut badge = el().size(30.0, 30.0).radius(8.0).fill(t.blue.alpha(0.14)).items_center().justify_center().child(icon(glyph, 16.0, t.blue));
    if d.names.len() > 1 {
        badge = badge.child(el().pin(None, Some(-5.0), Some(-5.0), None).h(16.0).min_w(16.0).px(4.0).radius(8.0).fill(t.blue).items_center().justify_center().child(text(d.names.len().to_string()).font(theme::SMALL).color(t.on_color).single_line()));
    }
    let card = popover(t, 230.0)
        .key("files-drag-card")
        .absolute(d.at[0] + 12.0, d.at[1] + 14.0)
        .padding(8.0)
        .child(
            row().w_full().gap(10.0).items_center().child(badge).child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(text(what).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate())
                    .child(text(landing_text(s, d, super::on_server(s, id))).key("files-drag-does").font(theme::NOTE).color(if ok { t.blue } else { t.l2 }).single_line().truncate()),
            ),
        )
        .inert();
    Some(card)
}

/// Past the window's edge the card can't follow: the window says what letting go does, and
/// where it lands.
fn out_hint(s: &Shell, d: &Dragging, t: &'static Theme) -> El {
    let what = if let [one] = d.names.as_slice() { s.text_shown(one).into_owned() } else { format!("{} items", d.names.len()) };
    let lands = if cfg!(windows) { "Over the desktop or a folder's window it lands there; anywhere else, in Downloads." } else { "It lands in your Downloads folder." };
    let card = popover(t, 320.0)
        .key("files-drag-out")
        .padding(20.0)
        .gap(6.0)
        .items_center()
        .child(el().size(54.0, 54.0).radius(14.0).fill(t.blue.alpha(0.14)).items_center().justify_center().child(icon(Icon::Download, 26.0, t.blue)))
        .child(text(format!("Let go to download {what}")).font(theme::TITLE).color(t.l1).single_line().truncate().max_w(280.0))
        .child(text(lands).font(theme::NOTE).color(t.l2).wrap().align(Align::Center).max_w(280.0));
    el().key("files-drag-out-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.blue.alpha(0.06)).border(2.0, t.blue.alpha(0.6)).items_center().justify_center().child(card).inert()
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
    if state.ui.naming.is_some() || state.ui.draft.focused || state.ui.filter.focused || state.ui.typing_path().is_some() || state.ui.confirm.is_some() || state.ui.menu.is_some() || state.replace.is_some() || state.paste.is_some() || state.copy.is_some() {
        return false;
    }
    if state.ui.places {
        return places_key(s, tab, ev, tx);
    }
    if state.peek.is_some() {
        return peek_key(s, tab, ev, tx);
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
    if let Some(edit) = state.ui.typing_path() {
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
        // Rows dragged over a folder of the path move there.
        if lit(s, id) == Some(Over::Crumb(i)) {
            crumb = crumb.fill(t.blue.alpha(0.15)).border(1.5, t.blue.alpha(0.7));
        }
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
        // The disk or home at the start always goes there, even when you're there already: it
        // never turns the bar into a field (that's the folder you're in, or the space after).
        if i == last && !root {
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
    // Dragged rows: the folder they'd land in lights up (0.10's accent wash and ring), and the
    // rows on their way dim.
    if lit(s, id) == Some(Over::Row(i)) {
        line = line.fill(t.blue.alpha(0.15)).border(1.5, t.blue.alpha(0.7));
    } else if state.ui.dragging.as_ref().is_some_and(|d| d.names.contains(&e.name)) {
        line = line.opacity(0.45);
    }
    line = line.child(icon(glyph_icon(glyph(e)), theme::ICON, if blue { t.on_color } else { t.l2on }));
    // », after a folder's name: what's inside, without going there. It follows the name, so
    // every name sits right by its icon and they all start in one line.
    let peeking = state.peek.as_ref().is_some_and(|(p, _)| *p == state.browser.path_of(&e.name));
    let mut arrow = el().key(peek_arrow(id, i)).size(16.0, 16.0).ml(2.0).radius(4.0).items_center().justify_center();
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
        state.ui.peek_at = None;
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
    if state.ui.drop_over || lit(s, id) == Some(Over::List) {
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
            let lit = lit(s, id) == Some(Over::UpRow);
            let above = paths::parent(b.dir());
            rows = rows.child(
                list_row()
                    .key("files-up-row")
                    .border(1.5, if lit { t.blue.alpha(0.7) } else { t.ring.alpha(0.0) })
                    .fill(if lit { t.blue.alpha(0.15) } else { t.blue.alpha(0.0) })
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
    let moving = state.ui.dragging.as_ref().map(|d| landing_text(s, d, super::on_server(s, id)));
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
        let n = editor.path();
        head = head.child(toolbar_button(t, "preview-download", Icon::Download, "Download").on_press_tx(move |s: &mut Shell, tx| {
            super::download_file(s, id, &n);
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
    // A picture, or an SVG drawn: on squares where it's see-through, with its size in pixels
    // and bytes under it (0.10's MediaPreview).
    let picture = || match &state.ui.picture {
        Some((_, picture)) => {
            let natural = picture.size();
            // Never bigger than it is: a small icon stays sharp instead of filling the pane.
            let [w, h] = natural.map_or([f32::MAX, f32::MAX], |[w, h]| [w as f32, h as f32]);
            let mut drawn = el().key("preview-picture-box").max_w(w).max_h(h).label(format!("Picture {}", s.text_shown(&name)));
            drawn = match natural {
                Some([nw, nh]) if nw > 0 && nh > 0 => drawn.aspect_fit(nw as f32 / nh as f32).on_layout(move |s: &mut Shell, rect: [f32; 4]| checker_at(s, id, t, rect)),
                _ => drawn.w_full().h_full(),
            };
            if let Some((_, squares)) = &state.ui.checker {
                drawn = drawn.pixels_fit(squares.pixels(), SurfaceFit::Fill);
            }
            let drawn = drawn.child(el().w_full().h_full().pixels_fit(picture.pixels(), SurfaceFit::Fit));
            // A drawing has no size of its own: only its bytes.
            let size = editor.size().map(bytes).unwrap_or_default();
            let said = match natural {
                Some([w, h]) if !svg => format!("{w} × {h} · {size}"),
                _ => size,
            };
            col().key("preview-picture").w_full().grow(1.0).min_h(0.0).child(row().w_full().grow(1.0).min_h(0.0).padding(14.0).items_center().justify_center().child(drawn)).child(
                row().key("preview-picture-foot").w_full().h(24.0).px(12.0).items_center().border_sides([1.0, 0.0, 0.0, 0.0], t.hair2).child(text(said).font(theme::LIST_NUMBER).color(t.l2).single_line()),
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
                let n = editor.path();
                buttons = buttons.child(push_button(t, "Download", Push::Quiet, true).key("preview-get").on_press_tx(move |s: &mut Shell, tx| {
                    super::download_file(s, id, &n);
                    tx.instant();
                }));
            }
            said.push(buttons);
            centered(said)
        }
    };
    Some(pane.child(body))
}

/// Points of one square under a see-through picture.
const SQUARE: u32 = 8;

/// The squares under a picture, made for the box it's drawn in once its size is known (and
/// again when it changes): one pixel a point, so they stay square at any size. Theme-coloured.
fn checker_at(s: &mut Shell, id: TabId, t: &'static Theme, rect: [f32; 4]) {
    let size = [rect[2].round().max(1.0) as u32, rect[3].round().max(1.0) as u32];
    let Some(ui) = ui(s, id) else { return };
    if ui.checker.as_ref().is_some_and(|(at, _)| *at == size) || size[0] > 4096 || size[1] > 4096 {
        return;
    }
    let Some(cache) = ui.pictures.as_ref() else { return };
    ui.checker = Some((size, cache.rgba(size[0], size[1], checker_pixels(t, size))));
}

/// Straight-alpha RGBA: every other square the interface's text colour, faintly.
fn checker_pixels(t: &Theme, [w, h]: [u32; 2]) -> Vec<u8> {
    let ink = t.l1;
    let dark = [(ink.r * 255.0) as u8, (ink.g * 255.0) as u8, (ink.b * 255.0) as u8, (0.07 * 255.0) as u8];
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            out.extend_from_slice(if (x / SQUARE + y / SQUARE).is_multiple_of(2) { &[0, 0, 0, 0] } else { &dark });
        }
    }
    out
}

fn save(s: &mut Shell, tab: TabId) {
    let Some(draft) = s.screens.files.get(&tab).map(|st| st.ui.draft.text.clone()) else { return };
    super::save(s, tab, &draft);
}

/// A Files tab's page.
pub fn page(s: &Shell, tab: &Tab, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    let id = tab.id;
    // Keyed by the tab: rows dragged over it know whose they are (`over`).
    let key = format!("files-tab-{}", id.0);
    let Some(state) = s.screens.files.get(&id) else { return col().key(key).w_full().h_full() };
    let width = pane_width(state);
    let beside = layout::preview_width(width, wanted_preview(s, state));
    let mut body = row().key("files-body").w_full().grow(1.0).min_h(0.0);
    match (preview(s, id, t, beside), beside) {
        // A pane too narrow for both: the open file takes it.
        (Some(open), None) => body = body.child(open),
        (Some(open), Some(_)) => body = body.child(list(s, id, t)).child(grip(t, id)).child(open),
        (None, _) => body = body.child(list(s, id, t)),
    }
    col().key(key).w_full().h_full().child(toolbar(s, id, t, width)).child(body).on_layout(move |s: &mut Shell, rect: [f32; 4]| {
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
        ui.place_menu = None;
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
    if let Some(card) = drag_card(s, id, t) {
        return Some(card);
    }
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
        let list = places(s, id, t);
        return Some(match place_menu(s, id, t, cx) {
            Some(menu) => el().key("files-places-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(list).child(menu),
            None => list,
        });
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
        let mut head = row().w_full().px(8.0).pt(6.0).pb(2.0).gap(2.0).items_center().child(text(title).font(theme::SMALL).color(t.l2).single_line());
        if title == "Places" {
            // The "?" is a hover's and a click's here: the list's own keys walk its rows.
            head = head.child(crate::screens::help::tip(s, t, "places-about", "places", crate::screens::help::PLACES).my(-2.0));
        }
        head = head.child(spacer());
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
            let menu_row = r.clone();
            card = card.child(line.on_press_tx(move |s: &mut Shell, tx| pick_place(s, id, pick.clone(), tx)).on_secondary_press(move |s: &mut Shell, at, tx| {
                if let Some(ui) = ui(s, id) {
                    ui.place_menu = Some(([at.x, at.y], Some(menu_row.clone())));
                }
                tx.instant();
            }));
        }
    }
    if !s.settings.get().hidden_places.is_empty() {
        card = card.child(el().h(1.0).mx(8.0).my(4.0).fill(t.hair)).child(menu_row(t, "Show everything", None, RowState::Rest).key("place-show-all").on_press_tx(|s: &mut Shell, tx| {
            super::show_every_place(s, tx);
            tx.instant();
        }));
    }
    // A right-click on the list: what it shows, ticked (0.10's PlacesPopover).
    card.on_secondary_press(move |s: &mut Shell, at, tx| {
        if let Some(ui) = ui(s, id) {
            ui.place_menu = Some(([at.x, at.y], None));
        }
        tx.instant();
    })
    .modal_tx(true, move |s: &mut Shell, tx| close_menu(s, id, tx))
}

/// A row of the right-click menu over the list of places: a tick when it's shown.
fn check_row(t: &Theme, key: String, label: &str, checked: bool) -> El {
    row()
        .key(key)
        .w_full()
        .h(crate::kit::menu::ROW)
        .px(8.0)
        .gap(8.0)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .hover(t.hov2)
        .press(t.down)
        .role("menuitemcheckbox")
        .checked(checked)
        .label(label.to_owned())
        .child(el().size(14.0, 14.0).items_center().justify_center().child(if checked { icon(Icon::Check, 12.0, t.l1) } else { el() }))
        .child(text(label).font(theme::TEXT).color(t.l1).single_line().truncate().grow(1.0).shrink())
}

/// The right-click menu over the list of places (0.10's `PlacesPopover`): the row's own way
/// out, then a tick for each place and for recent folders, to show or hide them there.
fn place_menu(s: &Shell, id: TabId, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let state = s.screens.files.get(&id)?;
    let (at, clicked) = state.ui.place_menu.clone()?;
    let hidden = &s.settings.get().hidden_places;
    let server = state.browser.server();
    let home = super::home_of(s, server);
    let close = move |s: &mut Shell, tx: &mut Tx| {
        if let Some(ui) = ui(s, id) {
            ui.place_menu = None;
        }
        tx.instant();
    };
    let mut rows = Vec::new();
    let (mut n_rows, mut n_seps) = (0.0, 0.0);
    if let Some(r) = clicked.filter(|r| r.kind != PlaceKind::Add) {
        let what = match r.kind {
            PlaceKind::Pin => "Remove bookmark".to_owned(),
            PlaceKind::Recent => "Remove from recent".to_owned(),
            _ => format!("Hide {}", r.label),
        };
        rows.push(menu_row(t, &s.text_shown(&what), None, RowState::Rest).key("place-menu-out").on_press_tx(move |s: &mut Shell, tx| {
            close(s, tx);
            take_out(s, id, &r, tx);
        }));
        rows.push(crate::kit::menu::menu_separator(t));
        n_rows += 1.0;
        n_seps += 1.0;
    }
    for p in bookmarks::places(server != "local", home.as_deref()) {
        let shown = !hidden.iter().any(|h| h == p.id);
        let place = p.id;
        rows.push(check_row(t, format!("place-menu-{}", p.id.replace('/', "-")), p.label, shown).on_press_tx(move |s: &mut Shell, tx| {
            super::toggle_place(s, place, tx);
            tx.instant();
        }));
        n_rows += 1.0;
    }
    rows.push(crate::kit::menu::menu_separator(t));
    let recent = !hidden.iter().any(|h| h == bookmarks::RECENT_ID);
    rows.push(check_row(t, "place-menu-recent".into(), "Recent folders", recent).on_press_tx(|s: &mut Shell, tx| {
        super::toggle_place(s, bookmarks::RECENT_ID, tx);
        tx.instant();
    }));
    n_rows += 1.0;
    n_seps += 1.0;
    if !hidden.is_empty() {
        rows.push(crate::kit::menu::menu_separator(t));
        rows.push(menu_row(t, "Show everything", None, RowState::Rest).key("place-menu-all").on_press_tx(|s: &mut Shell, tx| {
            super::show_every_place(s, tx);
            tx.instant();
        }));
        n_rows += 1.0;
        n_seps += 1.0;
    }
    let [_, _, w, h] = cx.layout("window").unwrap_or([0.0, 0.0, 1280.0, 800.0]);
    let height = n_rows * crate::kit::menu::ROW + n_seps * crate::kit::menu::SEPARATOR;
    Some(crate::kit::menu::context_menu(t, at, [w, h], 220.0, height, rows).key("files-place-menu").modal_tx(true, close))
}

/// The key of the » on row `i` of Files tab `id`, which the folder looked into hangs from.
fn peek_arrow(id: TabId, i: usize) -> String {
    format!("peek-arrow-{}-{i}", id.0)
}

/// The most rows the folder looked into with » lists: enough to see what's there; a folder of
/// 10,000 files is better opened.
const PEEK_SHOWN: usize = 200;

/// The folder looked into with », and its rows as listed, at most
/// [`PEEK_SHOWN`], with how many there are in all. `None` until it's listed.
fn peek_rows(s: &Shell, id: TabId) -> Option<(String, Vec<sshee_app::files::Entry>, usize)> {
    let state = s.screens.files.get(&id)?;
    let (path, _) = state.peek.clone()?;
    let cached = s.screens.dirs.get(state.browser.server(), &path)?;
    let entries = &cached.listing.entries;
    let order = sshee_app::files::browser::sorted(entries, sshee_app::files::Sort::default());
    let shown: Vec<usize> = order.into_iter().filter(|&i| state.browser.show_hidden() || !entries[i].is_hidden()).collect();
    let all = shown.len();
    let rows = shown.into_iter().take(PEEK_SHOWN).map(|i| entries[i].clone()).collect();
    Some((path, rows, all))
}

/// A row of the folder looked into picked: a folder goes there, a file opens beside the list
/// while the tab stays where it is (0.10).
fn pick_peek(s: &mut Shell, id: TabId, dir: &str, name: &str, is_dir: bool) {
    if let Some(st) = s.screens.files.get_mut(&id) {
        st.peek = None;
        st.ui.peek_at = None;
    }
    let full = paths::join(dir, name);
    if is_dir {
        guarded(s, id, After::Go(full));
    } else {
        guarded(s, id, After::OpenFrom { dir: dir.to_owned(), name: name.to_owned() });
    }
}

/// Keys while a folder is looked into (0.10's `FolderPeek`): ↑ and ↓ move, Return opens, Tab,
/// Esc and ← close it.
fn peek_key(s: &mut Shell, tab: TabId, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let close = |s: &mut Shell, tx: &mut Tx| {
        if let Some(st) = s.screens.files.get_mut(&tab) {
            st.peek = None;
            st.ui.peek_at = None;
        }
        tx.instant();
        true
    };
    if matches!(ev.key, Key::Escape | Key::Left | Key::Tab) {
        return close(s, tx);
    }
    let Some((dir, rows, _)) = peek_rows(s, tab) else { return false };
    let n = rows.len();
    let at = s.screens.files.get(&tab).and_then(|st| st.ui.peek_at).filter(|i| *i < n);
    let to = match ev.key {
        Key::Down if n > 0 => at.map_or(0, |i| (i + 1) % n),
        Key::Up if n > 0 => at.map_or(n - 1, |i| (i + n - 1) % n),
        Key::Enter => {
            if let Some(e) = at.and_then(|i| rows.get(i)).cloned() {
                pick_peek(s, tab, &dir, &e.name, e.is_dir());
                sync(s);
            }
            tx.instant();
            return true;
        }
        _ => return false,
    };
    if let Some(ui) = ui(s, tab) {
        ui.peek_at = Some(to);
    }
    tx.scroll_into_view(format!("peek-{to}"));
    tx.instant();
    true
}

/// What's in the folder looked into with », beside its row: a click on a folder goes there,
/// on a file opens it beside the list.
fn peek(s: &Shell, id: TabId, t: &'static Theme) -> Option<El> {
    let state = s.screens.files.get(&id)?;
    let (path, index) = state.peek.clone()?;
    // Beside the » it hangs from (the row is the pane's width: beside it is off the window).
    let mut card = popover(t, 300.0).key("files-peek").anchored(peek_arrow(id, index), Side::Right, Align::Start, 6.0).padding(4.0).role("menu").label(format!("What's in {}", s.text_shown(&path)));
    let close = move |s: &mut Shell, tx: &mut Tx| {
        if let Some(st) = s.screens.files.get_mut(&id) {
            st.peek = None;
            st.ui.peek_at = None;
        }
        tx.instant();
    };
    let Some((dir, rows, all)) = peek_rows(s, id) else {
        return Some(card.child(row().h(28.0).px(10.0).items_center().child(text("Looking inside…").font(theme::NOTE).color(t.l2).single_line())).modal_tx(true, close));
    };
    if rows.is_empty() {
        card = card.child(row().h(28.0).px(10.0).items_center().child(text("This folder is empty").font(theme::NOTE).color(t.l2).single_line()));
    }
    let at = state.ui.peek_at;
    let mut list = col().w_full().max_h(380.0).scroll(ScrollSpec { stick_bottom: false });
    for (n, e) in rows.into_iter().enumerate() {
        let (name, is_dir) = (e.name.clone(), e.is_dir());
        let mut line = row().key(format!("peek-{n}")).w_full().h(28.0).px(8.0).gap(8.0).radius(7.0).items_center().role("menuitem").help(s.text_shown(&name).into_owned());
        line = if at == Some(n) { line.fill(t.hov2) } else { line.hover(t.hov) };
        line = line.child(icon(glyph_icon(glyph(&e)), 14.0, t.l2on)).child(text(s.text_shown(&name).into_owned()).font(theme::TEXT).color(if e.is_hidden() { t.l2on } else { t.l1 }).single_line().truncate().shrink().min_w(0.0)).child(spacer());
        if !is_dir {
            line = line.child(text(bytes(e.size)).font(theme::LIST_NUMBER).color(t.l2).single_line());
        }
        let dir = dir.clone();
        list = list.child(
            line.on_hover_change(move |s: &mut Shell, inside, tx| {
                if inside {
                    if let Some(ui) = ui(s, id) {
                        ui.peek_at = Some(n);
                    }
                    tx.instant();
                }
            })
            .on_press_tx(move |s: &mut Shell, tx| {
                pick_peek(s, id, &dir, &name, is_dir);
                sync(s);
                tx.instant();
            }),
        );
    }
    if all > PEEK_SHOWN {
        let to = path.clone();
        list = list.child(menu_row(t, &format!("And {} more: open the folder", all - PEEK_SHOWN), None, RowState::Rest).key("peek-more").on_press_tx(move |s: &mut Shell, tx| {
            if let Some(st) = s.screens.files.get_mut(&id) {
                st.peek = None;
            }
            guarded(s, id, After::Go(to.clone()));
            tx.instant();
        }));
    }
    Some(card.child(list).modal_tx(true, close))
}

/// Return in the copy's path field: a path typed there is looked into first; once the dialog
/// shows it, Return copies (0.10).
fn copy_typed_go(s: &mut Shell, id: TabId, tx: &mut Tx) {
    let Some(state) = s.screens.files.get(&id) else { return };
    let typed = state.ui.copy_typed.text.trim().to_owned();
    let place = state.copy.as_ref().and_then(|c| c.place.as_ref()).map(|(p, _)| p.clone());
    if !typed.is_empty() && Some(&typed) != place.as_ref() {
        super::copy_go(s, id, &typed);
    } else if copy_ready(s, id) {
        super::copy_here(s, id);
    }
    tx.instant();
}

/// "Copy here" can be pressed: a server and a folder there, listed, the path field showing it,
/// and nothing against copying into it.
fn copy_ready(s: &Shell, id: TabId) -> bool {
    let Some(state) = s.screens.files.get(&id) else { return false };
    let Some(copy) = &state.copy else { return false };
    let shown = copy.place.as_ref().is_some_and(|(p, _)| state.ui.copy_typed.text.trim() == p);
    shown && copy.asked.is_none() && copy.to.is_some() && super::copy_blocked(s, id).is_none()
}

fn copy_typed(s: &mut Shell, tab: TabId) -> &mut TextEdit {
    match ui(s, tab) {
        Some(ui) => &mut ui.copy_typed,
        None => spare(),
    }
}

/// "Copy to a folder or server": the server to copy to, the folder there, and Copy.
fn copy_dialog(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let Some(state) = s.screens.files.get(&id) else { return el() };
    let Some(copy) = state.copy.as_ref() else { return el() };
    let on_server = super::on_server(s, id);
    let here = state.browser.server().to_owned();
    let names: Vec<String> = copy.paths.iter().map(|p| s.text_shown(paths::name(p)).into_owned()).collect();
    let close = move |s: &mut Shell, tx: &mut Tx| {
        if let Some(st) = s.screens.files.get_mut(&id) {
            st.copy = None;
        }
        tx.instant();
    };
    // "From web-7731", and what goes when it's more than one (0.10).
    let mut from = format!("From {}", server_name(s, &here));
    if names.len() > 1 {
        from.push_str(&format!(": {}", names.iter().take(4).cloned().collect::<Vec<_>>().join(", ")));
        if names.len() > 4 {
            from.push_str(&format!(" and {} more", names.len() - 4));
        }
    }
    let mut card = popover(t, 480.0).key("files-copy").padding(18.0).gap(8.0).role("dialog").label_static("Copy to a folder or server").child(
        row().gap(10.0).items_center().child(el().size(30.0, 30.0).radius(15.0).fill(t.blue.alpha(0.15)).items_center().justify_center().child(icon(Icon::Copy, 14.0, t.blue))).child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(ops::copy_title(&names, on_server)).font(theme::TITLE).color(t.l1).single_line().truncate())
                .child(text(from).key("copy-from").font(theme::NOTE).color(t.l2).single_line().truncate()),
        ),
    );
    let targets = super::copy_targets(s);
    if targets.is_empty() {
        card = card.child(text("There's no server to copy to yet. Add one from the sidebar first.").font(theme::NOTE).color(t.l2on).wrap().w_full());
    } else {
        card = card.child(text("To").font(theme::GROUP_TITLE).color(t.l2on).single_line().mt(6.0));
        let mut chips = row().w_full().gap(6.0).flex_wrap().role("radiogroup").label_static("Server");
        for key in targets {
            let name = server_name(s, &key);
            let on = copy.to.as_deref() == Some(&*key);
            let k = key.clone();
            let mut chip = row().key(format!("copy-to-{}", key.replace(':', "-"))).h(28.0).px(10.0).gap(6.0).radius(8.0).items_center().role("radio").label(name.clone());
            chip = if on { chip.fill(t.blue.alpha(0.14)).border(1.0, t.blue) } else { chip.fill(t.code).border(1.0, t.hair).hover(t.hov) };
            chip = chip.child(icon(Icon::Server, 13.0, if on { t.blue } else { t.l2on })).child(text(name).font(theme::CONTROL_TEXT).color(t.l1).single_line().max_w(180.0).truncate());
            // The server the files are on: a copy there goes to another folder.
            if key == here {
                chip = chip.child(text("this one").font(theme::NOTE).color(t.l2).single_line());
            }
            chips = chips.child(chip.on_press_tx(move |s: &mut Shell, tx| {
                super::copy_server(s, id, &k);
                tx.instant();
            }));
        }
        card = card.child(chips).child(text("Into this folder").font(theme::GROUP_TITLE).color(t.l2on).single_line().mt(6.0));
        let above = copy.place.as_ref().map(|(p, _)| p.clone()).filter(|p| paths::has_parent(p)).map(|p| paths::parent(&p));
        let mut up = el().key("copy-up").size(28.0, 28.0).radius(7.0).items_center().justify_center().fill(t.code).role("button").label_static("Folder above").help("Folder above").child(icon(Icon::ArrowUp, 14.0, t.l2on));
        up = match above {
            Some(to) => up.hover(t.hov2).on_press_tx(move |s: &mut Shell, tx| {
                super::copy_go(s, id, &to);
                tx.instant();
            }),
            None => up.opacity(0.4),
        };
        // The folder there, as a path that can be typed or pasted; Return goes there. While
        // streaming it's only shown, dotted, as 0.10 did.
        let input = if s.streaming() {
            text(s.text_shown(&state.ui.copy_typed.text).into_owned()).key("copy-path").font(theme::CODE).color(t.l1).single_line().truncate()
        } else {
            text_input(&state.ui.copy_typed, theme::CODE)
                .key("copy-path")
                .single_line()
                .color(t.l1)
                .placeholder("/path/on/the/server", t.l2)
                .label_static("Folder on the server")
                .on_edit(move |s: &mut Shell| copy_typed(s, id))
                .on_submit(move |s: &mut Shell, tx| copy_typed_go(s, id, tx))
        };
        let mut field = row().grow(1.0).min_w(0.0).h(28.0).px(10.0).gap(6.0).radius(7.0).fill(t.code).items_center().focus_within_ring(2.0, 0.0, t.ring).child(input.grow(1.0).min_w(0.0));
        if copy.asked.is_some() {
            field = field.child(text("Looking…").font(theme::NOTE).color(t.l2).single_line());
        }
        card = card.child(row().w_full().gap(6.0).items_center().child(up).child(field));
        if let Some(why) = super::copy_blocked(s, id).filter(|_| copy.error.is_none()) {
            card = card.child(text(s.text_shown(&why).into_owned()).key("copy-blocked").font(theme::NOTE).color(t.l2).wrap().w_full());
        }
        let mut folders = col().key("copy-folders").w_full().h(168.0).padding(4.0).radius(8.0).fill(t.code).scroll(ScrollSpec { stick_bottom: false });
        match (&copy.error, &copy.place) {
            (Some(error), _) => folders = folders.child(text(s.text_shown(error).into_owned()).font(theme::NOTE).color(t.red).wrap().w_full().padding(6.0)),
            (None, Some((_, dirs))) if dirs.is_empty() && copy.asked.is_none() => {
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
    let ready = copy_ready(s, id);
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
