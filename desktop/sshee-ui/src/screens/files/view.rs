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
//! Keys reach the list through the window (`Shell::key`), not through focus: nothing in the
//! list takes the keyboard, so arrows work the moment the tab is on screen, and a text field
//! (a new name, the open file) has the keys only while it is being typed in.

use rune::tok::{Family, TextStyle};
use rune::{col, el, row, spacer, text, text_input, Align, Cx, Drag, El, Phase, Picked, FileDialogOptions, GrowthSpec, Key, KeyEvent, PointerPress, ScrollSpec, Side, SurfaceFit, TextEdit, Tx};
use sshee_app::files::bookmarks;
use sshee_app::files::browser::{key_does, Click, KeyDoes, ListKey, Problem, OPEN_TERMINAL, TRY_AGAIN};
use sshee_app::files::menu::{Action, MenuRow};
use sshee_app::files::ops::{self, Confirm, DropOn};
use sshee_app::files::{bytes, glyph, modified_text, Body, Glyph, Notice, SortKey, Tone};
use sshee_app::paths;
use sshee_app::workbench::{Tab, TabId, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::button::{text_button, toolbar_button, ButtonKind};
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
/// The open file's pane.
const PREVIEW_W: f32 = 480.0;
const MENU_W: f32 = 232.0;

/// What a name is being typed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Naming {
    NewFolder,
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

/// What a yes to the question on screen does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Delete(Vec<String>),
    Discard(After),
}

/// A context menu that is open: where, and for which row (`None`: the folder itself).
pub struct Menu {
    at: [f32; 2],
    clicked: Option<String>,
    rows: Vec<MenuRow>,
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
    /// The bookmarks are open.
    places: bool,
    /// The open picture, decoded by Rune on its own thread.
    picture: Option<(u64, rune::image::Image)>,
    pictures: Option<rune::ImageCache>,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { draft: TextEdit::new(""), draft_of: None, lines: Vec::new(), naming: None, menu: None, confirm: None, dragging: None, places: false, picture: None, pictures: None }
    }
}

fn code_style() -> TextStyle {
    TextStyle::new(12.0, 400.0).family(Family::Named(crate::fonts::TERMINAL_FAMILY)).lh(18.0)
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
        After::Go(to) => super::go(s, tab, &to),
        After::Back => super::back(s, tab),
        After::Forward => super::forward(s, tab),
        After::Up => super::up(s, tab),
    }
    sync(s);
}

/// Does `after`, unless what was typed into the open file would be lost: then it asks first.
fn guarded(s: &mut Shell, tab: TabId, after: After) {
    match dirty(s, tab).map(ops::discard_question) {
        Some(question) => {
            if let Some(state) = s.screens.files.get_mut(&tab) {
                state.ui.confirm = Some((question, Pending::Discard(after)));
            }
        }
        None => run_after(s, tab, after),
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

fn ask_delete(s: &mut Shell, tab: TabId, names: Vec<String>) {
    let on_server = super::on_server(s, tab);
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    let entries: Vec<_> = names.iter().filter_map(|n| state.browser.entry(n)).collect();
    if !entries.is_empty() {
        let question = ops::delete_question(&entries, on_server);
        state.ui.confirm = Some((question, Pending::Delete(names)));
    }
}

fn start_naming(s: &mut Shell, tab: TabId, what: Naming) {
    let Some(ui) = ui(s, tab) else { return };
    let initial = match &what {
        Naming::NewFolder => "New folder".to_owned(),
        Naming::Rename(name) => name.clone(),
    };
    let mut field = TextEdit::new(initial.clone());
    // The name without its ending is selected, so typing replaces just that.
    let stem = match (&what, initial.rfind('.')) {
        (Naming::Rename(_), Some(dot)) if dot > 0 => dot,
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
    tx.open_file(options, move |s: &mut Shell, result, _| match result {
        Ok(Some(path)) => super::upload(s, tab, vec![path]),
        Ok(None) => {}
        Err(e) => super::note(s, Notice::new(Tone::Error, "Couldn't open the file picker").body(e.to_string())),
    });
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
        (Action::CopyPath, Some(_)) => tx.copy(path.unwrap_or_default()),
        (Action::ToggleBookmark, Some(_)) => super::toggle_bookmark(s, tab, &path.unwrap_or_default()),
        (Action::Delete, Some(name)) => ask_delete(s, tab, targets(s, tab, &name)),
        (Action::NewFolder, _) => start_naming(s, tab, Naming::NewFolder),
        (Action::Upload, _) => pick_upload(tab, tx),
        (Action::BookmarkHere, _) => super::toggle_bookmark(s, tab, &dir),
        (Action::CopyFolderPath, _) => tx.copy(dir),
        (Action::ToggleHidden, _) => {
            if let Some(state) = s.screens.files.get_mut(&tab) {
                let show = !state.browser.show_hidden();
                state.browser.set_show_hidden(show);
            }
        }
        (Action::Refresh, _) => super::refresh(s, tab),
        _ => {}
    }
    tx.instant();
}

fn open_menu(s: &mut Shell, tab: TabId, clicked: Option<&str>, at: PointerPress, tx: &mut Tx) {
    if let (Some(name), Some(state)) = (clicked, s.screens.files.get_mut(&tab)) {
        state.browser.context_click(name);
    }
    let rows = super::menu_for(s, tab, clicked);
    if let Some(ui) = ui(s, tab) {
        ui.menu = Some(Menu { at: [at.x, at.y], clicked: clicked.map(str::to_owned), rows });
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
            tx.element_at_in("files-list", x, y, move |s: &mut Shell, picked, tx| {
                let to = over(picked.as_ref()).filter(|o| drop_target(s, tab, &names, *o).is_some());
                if let Some(ui) = ui(s, tab) {
                    ui.dragging = Some((names, to));
                }
                tx.instant();
            });
        }
        Phase::Up => {
            let Some((names, _)) = ui(s, tab).and_then(|ui| ui.dragging.take()) else { return };
            if d.cancelled() {
                return;
            }
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
    if state.ui.naming.is_some() || state.ui.draft.focused || state.ui.confirm.is_some() || state.ui.menu.is_some() || state.replace.is_some() {
        return false;
    }
    let selected = state.browser.selected().to_vec();
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
        // There's no filter box yet: typing finds a name.
        KeyDoes::Filter => return false,
        KeyDoes::Move(_) | KeyDoes::SelectAll | KeyDoes::ClearSelection | KeyDoes::Type(_) => {
            let now = (tx.clock() * 1000.0) as u64;
            if let Some(state) = s.screens.files.get_mut(&tab) {
                let b = &mut state.browser;
                match does {
                    KeyDoes::Move(step) => drop(b.move_by(step)),
                    KeyDoes::SelectAll => b.select_all(),
                    KeyDoes::ClearSelection => b.clear_selection(),
                    KeyDoes::Type(c) => drop(b.type_to_find(c, now)),
                    _ => {}
                }
            }
        }
    }
    follow(s, tab, tx);
    true
}

// ---- drawing

fn glyph_icon(g: Glyph) -> Icon {
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

fn toolbar(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let Some(state) = s.screens.files.get(&id) else { return el() };
    let b = &state.browser;
    let dir = b.dir().to_owned();
    let pinned = bookmarks::is_pinned(&s.screens.bookmarks, b.server(), &dir);
    let mut bar = row().key("files-toolbar").w_full().h(TOOLBAR).items_center().gap(2.0).px(8.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(
        toolbar_button(t, "files-places", Icon::Star, "Bookmarks and places").on_press_tx(move |s: &mut Shell, tx| {
            if let Some(ui) = ui(s, id) {
                ui.places = true;
            }
            tx.instant();
        }),
    );
    bar = bar
        .child(tool(t, "files-back", Icon::ChevronLeft, "Back", b.can_go_back(), |s, tab, _| guarded(s, tab, After::Back), id))
        .child(tool(t, "files-forward", Icon::ChevronRight, "Forward", b.can_go_forward(), |s, tab, _| guarded(s, tab, After::Forward), id))
        .child(tool(t, "files-up", Icon::ArrowUp, "Up one folder", paths::has_parent(&dir), |s, tab, _| guarded(s, tab, After::Up), id));
    // The path: every folder in it goes there.
    let crumbs = paths::crumbs(&dir);
    let last = crumbs.len().saturating_sub(1);
    let mut trail = row().key("files-path").grow(1.0).min_w(0.0).h_full().items_center().ml(6.0).role("navigation").label_static("Folder path");
    for (i, c) in crumbs.into_iter().enumerate() {
        if i > 1 || (i == 1 && !paths::crumbs(&dir)[0].is_root()) {
            trail = trail.child(text("/").font(theme::TEXT).color(t.l2.alpha(0.6)).single_line().a11y_hidden());
        }
        let (ink, font) = if i == last { (t.l1, theme::TEXT_BOLD) } else { (t.l2on, theme::TEXT) };
        let mut crumb = el().key(format!("crumb-{i}")).h(22.0).px(5.0).radius(5.0).items_center().child(text(&c.label).font(font).color(ink).single_line());
        if i != last {
            let to = c.path.clone();
            crumb = crumb.hover(t.hov2).press(t.down).role("button").on_press_tx(move |s: &mut Shell, tx| {
                guarded(s, id, After::Go(to.clone()));
                tx.instant();
            });
        }
        trail = trail.child(crumb);
    }
    let _ = pinned;
    bar = bar
        .child(trail)
        .child(tool(t, "files-refresh", Icon::Refresh, "Refresh", true, |s, tab, _| super::refresh(s, tab), id))
        .child(tool(t, "files-new-folder", Icon::FolderPlus, "New folder", b.listing().is_some(), |s, tab, _| start_naming(s, tab, Naming::NewFolder), id));
    if super::on_server(s, id) {
        bar = bar.child(tool(t, "files-upload", Icon::Upload, "Upload files", b.listing().is_some(), |_, tab, tx| pick_upload(tab, tx), id));
    }
    bar
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
    let (ink, quiet) = if r.selected { (t.on_color, t.on_color.alpha(0.8)) } else { (t.l1, t.l2) };
    let name = e.name.clone();
    let mut line = list_row().role("row").label(&e.name);
    line = if r.selected { line.fill(t.blue) } else { line.hover(t.hov) };
    if let Some((names, to)) = &state.ui.dragging {
        if *to == Some(Over::Row(i)) {
            line = line.border(1.5, t.ring);
        } else if names.contains(&e.name) {
            line = line.opacity(0.45);
        }
    }
    line = line.child(icon(glyph_icon(glyph(e)), theme::ICON, if r.selected { t.on_color } else { t.l2on }));
    line = match &state.ui.naming {
        Some((Naming::Rename(from), edit)) if *from == e.name => line.child(naming_field(t, edit, id)),
        _ => {
            let shown = if e.is_link && !e.name.is_empty() { format!("{} ↗", e.name) } else { e.name.clone() };
            line.child(text(shown).font(theme::TEXT).color(if e.is_hidden() && !r.selected { t.l2on } else { ink }).single_line().truncate().grow(1.0).min_w(0.0))
        }
    };
    let size = if e.is_dir() { "—".to_owned() } else { bytes(e.size) };
    let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name);
    line.child(el().w(SIZE_W).justify_end().child(text(size).font(theme::LIST_NUMBER).color(quiet).single_line()))
        .child(el().w(DATE_W).child(text(modified_text(e.modified, clock.now, clock.utc_offset)).font(theme::LIST_NUMBER).color(quiet).single_line().truncate()))
        .on_press_tx(move |s: &mut Shell, tx| {
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

fn centered(children: Vec<El>) -> El {
    col().w_full().grow(1.0).min_h(0.0).items_center().justify_center().gap(8.0).px(24.0).children(children)
}

fn list(s: &Shell, id: TabId, t: &'static Theme) -> El {
    let Some(state) = s.screens.files.get(&id) else { return el() };
    let b = &state.browser;
    let mut pane = col().key("files-list-pane").grow(1.0).min_w(260.0).h_full();
    let problem = b.problem();
    if let Some(Problem::Page { title, message, open_terminal }) = &problem {
        let mut buttons = row().gap(8.0).mt(6.0);
        if *open_terminal {
            buttons = buttons.child(text_button(t, OPEN_TERMINAL, ButtonKind::Default).key("files-open-terminal").on_press_tx(|s: &mut Shell, tx| s.new_terminal(tx)));
        }
        buttons = buttons.child(text_button(t, TRY_AGAIN, ButtonKind::Plain).key("files-try-again").on_press_tx(move |s: &mut Shell, tx| {
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
    pane = pane.child(
        row()
            .key("files-header")
            .w_full()
            .h(ROW)
            .items_center()
            .gap(8.0)
            .pl(16.0 + theme::ICON + 8.0)
            .pr(16.0)
            .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
            .child(header_cell(t, "sort-name", "Name", on(SortKey::Name), SortKey::Name, id).grow(1.0))
            .child(header_cell(t, "sort-size", "Size", on(SortKey::Size), SortKey::Size, id).w(SIZE_W).justify_end())
            .child(header_cell(t, "sort-modified", "Modified", on(SortKey::Modified), SortKey::Modified, id).w(DATE_W)),
    );
    if let Some(Problem::Note { text: note, open_terminal }) = problem {
        let mut line = row().key("files-note").w_full().min_h(ROW).items_center().gap(8.0).px(16.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(icon(Icon::Warning, 12.0, t.orange)).child(
            text(note).font(theme::NOTE).color(t.l2on).single_line().truncate().grow(1.0).min_w(0.0),
        );
        if open_terminal {
            line = line.child(text_button(t, OPEN_TERMINAL, ButtonKind::Plain).key("files-open-terminal").on_press_tx(|s: &mut Shell, tx| s.new_terminal(tx)));
        }
        pane = pane.child(line.child(text_button(t, TRY_AGAIN, ButtonKind::Plain).key("files-try-again").on_press_tx(move |s: &mut Shell, tx| {
            super::refresh(s, id);
            tx.instant();
        })));
    }
    let mut rows = col().key("files-list").w_full().grow(1.0).min_h(0.0).py(4.0).role("list").label_static("Files").scroll(ScrollSpec { stick_bottom: false }).on_secondary_press(
        move |s: &mut Shell, at, tx| open_menu(s, id, None, at, tx),
    );
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
                    .on_double_press_tx(move |s: &mut Shell, tx| {
                        guarded(s, id, After::Up);
                        tx.instant();
                    }),
            );
        }
        if let Some((Naming::NewFolder, edit)) = &state.ui.naming {
            rows = rows.child(list_row().key("files-new-row").fill(t.hov).child(icon(Icon::Folder, theme::ICON, t.l2on)).child(naming_field(t, edit, id)));
        }
        rows = rows.child(col().key("files-rows").w_full().growth(GrowthSpec::default()).virtual_rows(b.len(), ROW, |_: &Shell, i| format!("row-{i}").into(), move |s: &Shell, i| entry_row(s, id, i, t)));
        if let Some(empty) = b.empty().filter(|_| state.ui.naming.is_none()) {
            let mut said = col().key("files-empty").w_full().items_center().gap(4.0).pt(64.0).child(text(empty.text()).font(theme::TEXT).color(t.l2on).single_line());
            if super::on_server(s, id) && b.filter().is_empty() {
                said = said.child(text("Use Upload in the toolbar to put files here.").font(theme::NOTE).color(t.l2).single_line());
            }
            rows = rows.child(said);
        }
    }
    pane = pane.child(rows);
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
        if to.is_some() { format!("Let go to move {what} here") } else { format!("Drag {what} onto a folder to move it there") }
    });
    if let Some(busy) = moving.as_deref().or(state.busy.as_deref()).or(b.is_loading().then_some("Loading…")) {
        foot = foot.child(text(busy).key("files-busy").font(theme::STATUS).color(t.l2on).single_line());
    }
    pane.child(foot)
}

/// The open file, beside the list.
fn preview(s: &Shell, id: TabId, t: &'static Theme) -> Option<El> {
    let state = s.screens.files.get(&id)?;
    let editor = state.editor.as_ref()?;
    let on_server = super::on_server(s, id);
    let name = editor.name().to_owned();
    let dirty = dirty(s, id).is_some();
    let mut head = row().key("preview-head").w_full().h(TOOLBAR).items_center().gap(8.0).pl(14.0).pr(8.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(
        text(&name).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate().shrink().min_w(0.0),
    );
    if dirty {
        head = head.child(el().key("preview-dirty").size(6.0, 6.0).radius(3.0).fill(t.l2on).label_static("Unsaved changes"));
    }
    head = head.child(spacer());
    if dirty {
        let label = if editor.is_saving() { "Saving…" } else { "Save" };
        head = head.child(text_button(t, label, ButtonKind::Default).key("preview-save").on_press_tx(move |s: &mut Shell, tx| {
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
    let mut pane = col().key("files-preview").w(PREVIEW_W).h_full().border_sides([0.0, 0.0, 0.0, 1.0], t.hair2).child(head);
    let banner = |words: &str| row().w_full().px(14.0).py(7.0).border_sides([0.0, 0.0, 1.0, 0.0], t.hair2).child(text(words).font(theme::NOTE).color(t.l2on).grow(1.0).min_w(0.0));
    // The file changed on the server: nothing was written, and these are the ways on.
    if let Some(conflict) = editor.conflict() {
        let mut answers = row().gap(8.0).flex_wrap();
        for (i, (choice, label)) in conflict.choices().into_iter().enumerate() {
            answers = answers.child(text_button(t, label, if i == 0 { ButtonKind::Destructive } else { ButtonKind::Plain }).key(format!("conflict-{i}")).on_press_tx(move |s: &mut Shell, tx| {
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
    if let Some(why) = editor.read_only() {
        pane = pane.child(banner(why.note()).key("preview-read-only"));
    }
    let body = match editor.body() {
        Body::Loading => centered(vec![text("Loading…").font(theme::NOTE).color(t.l2).single_line()]),
        Body::Text { saved, .. } if editor.can_edit() => {
            let _ = saved;
            col().key("preview-scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(
                text_input(&state.ui.draft, code_style())
                    .key("preview-text")
                    .w_full()
                    .padding(14.0)
                    .color(t.l1)
                    .label(format!("Text of {name}"))
                    .on_edit(move |s: &mut Shell| draft_field(s, id))
                    // Return breaks the line: this is a document, not a form.
                    .on_submit(move |s: &mut Shell, _| draft_field(s, id).insert("\n"))
                    .on_input_key(move |s: &mut Shell, ev, tx| match &ev.key {
                        Key::Command('s') if ev.mods.cmd && !ev.mods.shift && !ev.mods.alt => {
                            save(s, id);
                            true
                        }
                        Key::Escape => {
                            tx.blur();
                            true
                        }
                        _ => false,
                    }),
            )
        }
        Body::Text { .. } => {
            let style = code_style();
            col().key("preview-scroll").w_full().grow(1.0).min_h(0.0).py(10.0).scroll(ScrollSpec { stick_bottom: false }).child(
                col().key("preview-lines").w_full().growth(GrowthSpec::default()).virtual_rows(state.ui.lines.len(), 18.0, |_: &Shell, i| format!("line-{i}").into(), move |s: &Shell, i| {
                    let line = s.screens.files.get(&id).and_then(|st| {
                        let (a, b) = *st.ui.lines.get(i)?;
                        st.editor.as_ref()?.text()?.get(a..b)
                    });
                    el().h(18.0).px(14.0).child(text(line.unwrap_or_default().trim_end_matches('\r')).font(style).color(t.l1).single_line())
                }),
            )
        }
        Body::Image(_) => match &state.ui.picture {
            Some((_, picture)) => {
                // Never bigger than it is: a small icon stays sharp instead of filling the pane.
                let [w, h] = picture.size().map_or([f32::MAX, f32::MAX], |[w, h]| [w as f32, h as f32]);
                el().key("preview-picture").w_full().grow(1.0).min_h(0.0).padding(14.0).items_center().justify_center().child(
                    el().w_full().h_full().max_w(w).max_h(h).pixels_fit(picture.pixels(), SurfaceFit::Fit).label(format!("Picture {name}")),
                )
            }
            None => centered(vec![text("Loading…").font(theme::NOTE).color(t.l2).single_line()]),
        },
        _ => {
            let (title, line) = editor.message(on_server).unwrap_or_default();
            let mut said = vec![text(title).font(theme::TEXT_BOLD).color(t.l1).single_line(), text(line).font(theme::NOTE).color(t.l2).max_w(340.0).align(Align::Center)];
            if on_server {
                let n = name.clone();
                said.push(text_button(t, "Download", ButtonKind::Plain).key("preview-get").mt(6.0).on_press_tx(move |s: &mut Shell, tx| {
                    super::download(s, id, std::slice::from_ref(&n));
                    tx.instant();
                }));
            }
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
    let mut body = row().key("files-body").w_full().grow(1.0).min_h(0.0).child(list(s, id, t));
    if let Some(open) = preview(s, id, t) {
        body = body.child(open);
    }
    col().key("files").w_full().h_full().child(toolbar(s, id, t)).child(body)
}

// ---- over the window

fn close_menu(s: &mut Shell, id: TabId, tx: &mut Tx) {
    if let Some(ui) = ui(s, id) {
        ui.menu = None;
        ui.places = false;
    }
    tx.instant();
}

fn question(t: &'static Theme, q: &Confirm, yes: impl Fn(&mut Shell, &mut Tx) + 'static, no: impl Fn(&mut Shell, &mut Tx) + Clone + 'static) -> El {
    let mut card = popover(t, 400.0).key("files-question").padding(18.0).gap(8.0).child(text(&q.title).font(theme::TEXT_BOLD).color(t.l1));
    if !q.body.is_empty() {
        card = card.child(text(&q.body).font(theme::NOTE).color(t.l2on));
    }
    if !q.lines.is_empty() {
        let mut names = col().w_full().gap(2.0).padding(8.0).radius(theme::RADIUS_ROW).fill(t.code);
        for line in &q.lines {
            names = names.child(text(line).font(theme::NOTE).color(t.l1).single_line().truncate());
        }
        card = card.child(names);
    }
    let cancel = no.clone();
    let kind = if q.danger { ButtonKind::Destructive } else { ButtonKind::Default };
    let card = card
        .child(
            row().w_full().gap(8.0).mt(8.0).child(spacer()).child(text_button(t, "Cancel", ButtonKind::Plain).key("question-no").on_press_tx(cancel)).child(
                text_button(t, q.confirm_label, kind).key("question-yes").default_button().on_press_tx(yes),
            ),
        )
        .modal_tx(false, no);
    el().key("files-question-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).items_center().justify_center().child(card)
}

/// What a Files tab shows over the window: its menu, its question, its bookmarks.
pub fn overlay(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let id = s.bench.active().filter(|tab| tab.kind == TabKind::Files)?.id;
    let state = s.screens.files.get(&id)?;
    if let Some((q, _)) = &state.ui.confirm {
        let answer = move |yes: bool| {
            move |s: &mut Shell, tx: &mut Tx| {
                let Some((_, pending)) = ui(s, id).and_then(|ui| ui.confirm.take()) else { return };
                match pending {
                    Pending::Delete(names) if yes => super::delete(s, id, &names),
                    Pending::Discard(after) if yes => run_after(s, id, after),
                    _ => {}
                }
                tx.instant();
            }
        };
        return Some(question(t, q, answer(true), answer(false)));
    }
    if let Some(replace) = &state.replace {
        let answer = move |yes: bool| {
            move |s: &mut Shell, tx: &mut Tx| {
                super::answer_replace(s, id, yes);
                tx.instant();
            }
        };
        return Some(question(t, &replace.question, answer(true), answer(false)));
    }
    if let Some(open) = &state.ui.menu {
        let mut rows = Vec::new();
        let mut height = 12.0;
        for (i, r) in open.rows.iter().enumerate() {
            match r {
                MenuRow::Separator => {
                    height += 9.0;
                    rows.push(el().key(format!("menu-sep-{i}")).h(1.0).mx(8.0).my(4.0).fill(t.hair));
                }
                MenuRow::Item(item) => {
                    height += 26.0;
                    let (action, clicked) = (item.action, open.clicked.clone());
                    let label = match item.checked {
                        Some(true) => format!("✓ {}", item.label),
                        _ => item.label.clone(),
                    };
                    let draw = if item.destructive { menu_row_destructive } else { menu_row };
                    let line = draw(t, &label, item.hint, RowState::Rest).key(format!("menu-{i}"));
                    rows.push(line.on_press_tx(move |s: &mut Shell, tx| {
                        close_menu(s, id, tx);
                        run_action(s, id, action, clicked.clone(), tx);
                    }));
                }
            }
        }
        // At the pointer, moved in from the window's edges.
        let [_, _, w, h] = cx.layout("window").unwrap_or([0.0, 0.0, 1280.0, 800.0]);
        let x = open.at[0].min(w - MENU_W - 8.0).max(8.0);
        let y = open.at[1].min(h - height - 8.0).max(8.0);
        return Some(popover(t, MENU_W).key("files-menu").absolute(x, y).child(menu(rows)).modal_tx(true, move |s: &mut Shell, tx| close_menu(s, id, tx)));
    }
    if state.ui.places {
        let b = &state.browser;
        let server = b.server().to_owned();
        let on_server = server != "local";
        let mut card = popover(t, 280.0).key("files-places-card").anchored("files-places", Side::Bottom, Align::Start, 6.0).padding(6.0);
        let mut n = 0;
        let mut section = |card: El, title: &str, items: Vec<(String, String)>| {
            if items.is_empty() {
                return card;
            }
            let mut card = card.child(text(title).font(theme::SMALL).color(t.l2).single_line().px(8.0).pt(6.0).pb(2.0));
            for (label, path) in items {
                n += 1;
                let to = path.clone();
                card = card.child(menu_row(t, &label, None, RowState::Rest).key(format!("place-{n}")).help(path).on_press_tx(move |s: &mut Shell, tx| {
                    close_menu(s, id, tx);
                    guarded(s, id, After::Go(to.clone()));
                }));
            }
            card
        };
        let named = |list: &[String]| list.iter().map(|p| (paths::name(p).to_owned(), p.clone())).collect::<Vec<_>>();
        card = section(card, "Places", bookmarks::places(on_server, None).into_iter().map(|p| (p.label.to_owned(), p.path)).collect());
        card = section(card, "Bookmarks", named(bookmarks::pins(&s.screens.bookmarks, &server)));
        card = section(card, "Recent", named(bookmarks::recent(&s.screens.bookmarks, &server)));
        let dir = b.dir().to_owned();
        let pinned = bookmarks::is_pinned(&s.screens.bookmarks, &server, &dir);
        card = card.child(el().h(1.0).mx(8.0).my(4.0).fill(t.hair)).child(
            menu_row(t, if pinned { "Remove bookmark" } else { "Bookmark this folder" }, None, RowState::Rest).key("place-pin").on_press_tx(move |s: &mut Shell, tx| {
                super::toggle_bookmark(s, id, &dir);
                close_menu(s, id, tx);
            }),
        );
        return Some(card.modal_tx(true, move |s: &mut Shell, tx| close_menu(s, id, tx)));
    }
    None
}
