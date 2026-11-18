//! The file manager (0.10's FilesPane: a server's folders, previews, transfers). The page
//! is drawn by `view`; everything under it is here.
//!
//! The rules are sshee-app's (`sshee_app::files`: [`Browser`], [`Editor`], menus, questions,
//! wording). This module is the seam between them, the window and the backend:
//!
//! - [`State`] is one Files tab's: its folder, the file open beside it, what's running. The
//!   window-wide part (folders seen lately, bookmarks, transfers, notes) is in
//!   `screens::States`.
//! - Every thing the screen does is a function here that takes the window's state and the
//!   tab: [`go`], [`open`], [`save`], [`rename`], [`delete`], [`upload`]... Each changes the
//!   state at once and sends what's needed to the backend (`Shell::files_ask`); none waits.
//! - [`deliver`] takes the backend's answers (through `Remote::files`) back to the tab that
//!   asked, the folder cache and the transfer cards.
//!
//! What the view still decides: when to ask first (`sshee_app::files::ops::delete_question`,
//! `discard_question` when [`Editor::is_dirty`]), and the text being typed, which lives in its
//! text field.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use rune::{Cx, El};

pub mod edit;
mod markdown_view;
mod view;

pub use view::{key, overlay};
use sshee_app::dir_cache::DirCache;
use sshee_app::files::bookmarks;
use sshee_app::files::browser::Load;
use sshee_app::files::editor::{AfterSave, Choice};
use sshee_app::files::menu::{self, MenuRow};
use sshee_app::files::ops::{self, Confirm, Undo};
use sshee_app::files::protocol::{Answer, Ask, Item, Reply, Want};
use sshee_app::files::{Browser, Editor, FailureKind, Notice, Tickets};
use sshee_app::paths;
use sshee_app::transfers::{files_label, Kind, TransferId};
use sshee_app::workbench::{Tab, TabId, TabKind};

use crate::kit::toast::Tone;
use crate::shell::{Shell, Toast};
use crate::theme::Theme;

/// An upload found names that are taken and sent nothing: the question to ask, and what to
/// send again on yes ([`answer_replace`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Replace {
    pub question: Confirm,
    paths: Vec<PathBuf>,
    dir: String,
    /// A copy to a server (its key and the paths on this tab's machine), not an upload.
    copy: Option<(String, Vec<String>)>,
}

/// "Copy to another folder or server": what goes, and where it's going so far.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyTo {
    /// Full paths on the tab's machine, all in one folder.
    pub paths: Vec<String>,
    /// The server picked to copy to.
    pub to: Option<String>,
    /// The folder there being looked at, once listed, with the folders in it.
    pub place: Option<(String, Vec<String>)>,
    /// The folder asked for, while it's being listed.
    pub asked: Option<String>,
    pub error: Option<String>,
}

/// Where "Copy to another folder or server" copied to (0.10's `lastHost`, `lastDir`): the
/// dialog opens on the last server, each server in its last folder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CopyMemory {
    pub last: Option<String>,
    pub dirs: std::collections::HashMap<String, String>,
}

/// Files copied on this computer, waiting for a yes before they go to the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Paste {
    pub question: ops::PasteQuestion,
    paths: Vec<PathBuf>,
    /// The folder the question names: a yes copies there, wherever the tab is by then.
    dir: String,
}

/// One Files tab's own state.
pub struct State {
    /// The folder: rows, selection, history ([`Browser`]'s own methods for what needs no
    /// backend: `click`, `sort_by`, `set_filter`, `move_by`...).
    pub browser: Browser,
    /// The file open beside the list, if any.
    pub editor: Option<Editor>,
    tickets: Tickets,
    /// What runs in this folder right now, for the status line: "Deleting 3 items…".
    pub busy: Option<String>,
    /// Waiting for a yes or no.
    pub replace: Option<Replace>,
    pub paste: Option<Paste>,
    /// "Copy to another folder or server", while it's open.
    pub copy: Option<CopyTo>,
    /// The folder looked into with », and the row it hangs from.
    pub peek: Option<(String, usize)>,
    /// A folder pointed at, and when it's listed ahead if the pointer stays (0.10's 150 ms).
    hover: Option<(String, f64)>,
    /// What only the drawing keeps: the text being typed, the menu or question on screen.
    pub(crate) ui: view::Ui,
    /// A path from a terminal to open once its folder is listed ([`show_path`]).
    reveal: Option<(String, String)>,
    /// When the tab was last on screen or busy (Rune's clock): unused since then.
    seen_at: Option<f64>,
    /// Let go of its server's connection while unused off screen
    /// ([`sshee_app::files::sleeps_now`]); it lists its folder again when it's back on screen.
    asleep: bool,
}

impl State {
    fn new(server: &str, path: &str) -> State {
        State {
            browser: Browser::new(server, path),
            editor: None,
            tickets: Tickets::default(),
            busy: None,
            replace: None,
            paste: None,
            copy: None,
            peek: None,
            hover: None,
            ui: view::Ui::default(),
            reveal: None,
            seen_at: None,
            asleep: false,
        }
    }
}

pub fn view(s: &Shell, tab: &Tab, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    let id = tab.id;
    // The mouse's side buttons go back and forward, as in a browser (0.10.5).
    view::page(s, tab, t, cx).pointer_buttons([rune::PointerButton::Back, rune::PointerButton::Forward]).on_background_press_tx(move |s: &mut Shell, tx| {
        match tx.button() {
            rune::PointerButton::Back => back(s, id),
            rune::PointerButton::Forward => forward(s, id),
            _ => return,
        }
        tx.instant();
    })
}

/// Milliseconds on a clock that only goes forward, for how fresh a cached folder is. It stands
/// still in a demo and in tests, so they draw the same every run. Type-to-find wants a clock
/// that runs there too: give [`Browser::type_to_find`] Rune's.
pub fn now_ms(s: &Shell) -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    if s.replays() {
        return 0;
    }
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Whether a tab's machine is a server: there's something to download from and upload to.
pub fn on_server(s: &Shell, tab: TabId) -> bool {
    s.screens.files.get(&tab).is_some_and(|st| st.browser.server() != "local")
}

fn ask(s: &mut Shell, tab: TabId, want: Want) {
    let Some(server) = s.screens.files.get(&tab).map(|st| st.browser.server().to_owned()) else { return };
    s.files_ask(Ask { tab, server, want });
}

/// A note about what finished or failed: a toast that leaves by itself (`shell::notes`), with
/// Undo for a move and "Show in folder" for a copy kept on this computer.
fn note(s: &mut Shell, notice: Notice) {
    let tone = match notice.tone {
        sshee_app::files::Tone::Info => Tone::Info,
        sshee_app::files::Tone::Success => Tone::Success,
        sshee_app::files::Tone::Error => Tone::Error,
    };
    let mut toast = Toast::new(tone, notice.title);
    toast.body = notice.body;
    if let Some(undo) = notice.undo {
        // Back on the machine it moved on, through the tab that moved it, whatever is on screen.
        toast = toast.action("Undo", move |s| undo_move(s, &undo));
    } else if let Some(path) = notice.folder {
        toast = toast.action("Show in folder", move |s| s.edit_ask(sshee_app::files::edit::EditAsk::ShowFolder { path: path.clone() }));
    }
    s.toast(toast);
}

/// The folder on screen is where the tab is (kept for the next run) and the newest of the
/// machine's recent folders.
fn keep_place(s: &mut Shell, tab: TabId) {
    let Some(state) = s.screens.files.get(&tab).filter(|st| st.browser.listing().is_some()) else { return };
    let (server, dir) = (state.browser.server().to_owned(), state.browser.dir().to_owned());
    if bookmarks::visit(&mut s.screens.bookmarks, &server, &dir) {
        bookmarks_changed(s);
    }
    s.bench.set_path(tab, Some(dir));
}

// ---- bookmarks between runs

/// Files' bookmarks as the backend kept them, once when the window starts.
pub(crate) fn start(s: &mut Shell) {
    if let Some(b) = s.backend_mut().bookmarks() {
        s.screens.bookmarks = b;
    }
}

/// 0.10's bookmarks and recent folders, brought in after this copy's own (a first start).
pub fn seed_bookmarks(s: &mut Shell, from: &sshee_app::model::Bookmarks) {
    if bookmarks::merge(&mut s.screens.bookmarks, from) {
        bookmarks_changed(s);
    }
}

fn bookmarks_changed(s: &mut Shell) {
    let b = s.screens.bookmarks.clone();
    s.backend_mut().bookmarks_changed(&b);
}

/// "Remove bookmark" in the list of places.
pub fn remove_bookmark(s: &mut Shell, server: &str, path: &str) {
    bookmarks::remove_pin(&mut s.screens.bookmarks, server, path);
    bookmarks_changed(s);
}

/// "Remove from recent" in the list of places.
pub fn forget_recent(s: &mut Shell, server: &str, path: &str) {
    bookmarks::forget_recent(&mut s.screens.bookmarks, server, path);
    bookmarks_changed(s);
}

/// A place (or the recent folders) in or out of the list of places: Settings' `hiddenPlaces`.
pub fn toggle_place(s: &mut Shell, id: &str, tx: &mut rune::Tx) {
    let id = id.to_owned();
    s.change_settings(tx, move |st| st.set(|v| bookmarks::toggle_hidden(&mut v.hidden_places, &id)));
}

/// "Show everything": no place hidden.
pub fn show_every_place(s: &mut Shell, tx: &mut rune::Tx) {
    s.change_settings(tx, |st| st.set(|v| v.hidden_places.clear()));
}

/// The machine's home folder's full path, once a "~" listing told it.
pub fn home_of(s: &Shell, server: &str) -> Option<String> {
    s.screens.dirs.get(server, "~").map(|c| c.listing.path.clone())
}

// ---- folders listed ahead

/// The folder `path` of `tab`'s machine into the cache, unless it's there and fresh: a folder
/// looked into, pointed at, or shown in "Copy to…" (`server`: that one's machine).
pub fn peek_into(s: &mut Shell, tab: TabId, server: &str, path: &str) {
    if s.screens.dirs.get(server, path).is_some() && s.screens.dirs.is_fresh(server, path, now_ms(s)) {
        return;
    }
    s.files_ask(Ask { tab, server: server.to_owned(), want: Want::Peek { path: path.to_owned() } });
}

/// The pointer rests on a folder (`Some`) or left it: listed ahead after 150 ms.
pub fn point_at(s: &mut Shell, tab: TabId, folder: Option<String>, clock: f64) {
    if let Some(state) = s.screens.files.get_mut(&tab) {
        state.hover = folder.map(|f| (f, clock + 0.15));
    }
}

/// Every frame (`Shell::poll`): a folder pointed at long enough is listed ahead, and a Files
/// tab unused off screen lets go of its server's connection. True when an open picture was
/// decoded since: drawn again, it knows its size (the footer's pixels, never drawn bigger).
pub(crate) fn tick(s: &mut Shell, clock: f64, tx: &mut rune::Tx) -> bool {
    idle(s, clock, tx);
    // Every tab looks (each keeps whether its picture was decoded), not just until one says yes.
    let mut decoded = false;
    for st in s.screens.files.values_mut() {
        decoded |= st.ui.picture_decoded();
    }
    let due: Vec<(TabId, String, String)> = s
        .screens
        .files
        .iter_mut()
        .filter_map(|(id, st)| match &st.hover {
            Some((path, at)) if *at <= clock => {
                let path = path.clone();
                st.hover = None;
                Some((*id, st.browser.server().to_owned(), path))
            }
            Some((_, at)) => {
                tx.wake_after(std::time::Duration::from_secs_f64((at - clock).max(0.0)));
                None
            }
            None => None,
        })
        .collect();
    for (tab, server, path) in due {
        peek_into(s, tab, &server, &path);
    }
    decoded
}

/// Settings → Terminal → "Disconnect unused terminals", for Files: a tab on a server that has
/// been off screen and idle for that long lets go of its connection, unless something else on
/// the window still uses that server; back on screen, it lists its folder again, which signs
/// in again by itself. Also: the address bar typed in lost the keyboard, so it's the path again.
fn idle(s: &mut Shell, clock: f64, tx: &mut rune::Tx) {
    let minutes = s.settings.idle_sleep_minutes();
    let on_screen = s.tabs_on_screen();
    let mut sleep = Vec::new();
    let mut wake = Vec::new();
    for (id, st) in s.screens.files.iter_mut() {
        if st.ui.path_edit.as_ref().is_some_and(|e| !e.focused) {
            st.ui.path_edit = None;
        }
        let shown = on_screen.contains(id);
        let busy = st.busy.is_some() || st.browser.is_loading() || s.screens.transfers.items().iter().any(|t| t.tab == *id);
        if shown || busy || st.seen_at.is_none() {
            st.seen_at = Some(clock);
            if shown && st.asleep {
                st.asleep = false;
                wake.push(*id);
            }
            continue;
        }
        let idle = clock - st.seen_at.unwrap_or(clock);
        let on_server = st.browser.server() != "local";
        if st.asleep || !on_server {
            continue;
        }
        if sshee_app::files::sleeps_now(on_server, shown, busy, idle, minutes) {
            st.asleep = true;
            sleep.push((*id, st.browser.server().to_owned()));
        } else if let Some(m) = minutes.filter(|m| *m > 0.0) {
            tx.wake_after(std::time::Duration::from_secs_f64((m * 60.0 - idle).max(1.0)));
        }
    }
    for id in wake {
        refresh(s, id);
    }
    for (tab, server) in sleep {
        if !in_use(s, &server, &on_screen) {
            s.files_ask(Ask { tab, server, want: Want::LetGo });
        }
    }
}

/// Whether something on the window still uses `server`'s connection: a tab of it on screen, a
/// terminal connected to it, a Files tab of it that isn't asleep.
fn in_use(s: &Shell, server: &str, on_screen: &[TabId]) -> bool {
    use sshee_app::workbench::Link;
    s.bench.tabs().iter().filter(|t| t.server == server).any(|t| {
        on_screen.contains(&t.id)
            || (t.kind == TabKind::Terminal && matches!(t.link, Link::Connecting | Link::Connected { .. } | Link::Reconnecting))
            || s.screens.files.get(&t.id).is_some_and(|st| !st.asleep)
    })
}

/// Runs one of the browser's moves, and asks the backend for the listing it wants.
fn load(s: &mut Shell, tab: TabId, step: impl FnOnce(&mut Browser, &DirCache, u64) -> Option<Load>) {
    let now = now_ms(s);
    let screens = &mut s.screens;
    let Some(state) = screens.files.get_mut(&tab) else { return };
    let was = state.browser.asked().to_owned();
    let load = step(&mut state.browser, &screens.dirs, now);
    // Another folder: a question about the rows of the last one would act on this one's.
    if state.browser.asked() != was {
        state.ui.folder_changed();
        state.paste = None;
    }
    // A folder known from the cache is on screen already.
    keep_place(s, tab);
    if let Some(Load { ticket, path }) = load {
        ask(s, tab, Want::List { ticket, path });
    }
}

/// Something changed in `dir` on `server`: its cached listing is out of date, and every tab
/// that shows it lists it again.
fn changed(s: &mut Shell, server: &str, dir: &str) {
    s.screens.dirs.changed(server, dir);
    let showing: Vec<TabId> =
        s.screens.files.iter().filter(|(_, st)| st.browser.server() == server && (st.browser.dir() == dir || st.browser.asked() == dir)).map(|(id, _)| *id).collect();
    for tab in showing {
        load(s, tab, |b, cache, now| b.refresh(cache, now));
    }
}

/// The Files tab on screen is shown for the first time: it gets its state, in the folder the
/// tab was last in (the home folder for a new one), and lists it. `Shell::settle` calls this.
pub(crate) fn shown(s: &mut Shell) {
    for id in s.tabs_on_screen() {
        let Some(tab) = s.bench.tab(id).filter(|t| t.kind == TabKind::Files && !s.screens.files.contains_key(&t.id)).cloned() else { continue };
        s.screens.files.insert(tab.id, State::new(&tab.server, tab.path.as_deref().unwrap_or("~")));
        hidden_default(s, tab.id);
        load(s, tab.id, |b, cache, now| b.start(cache, now));
    }
}

/// Whether `tab` shows dot files when nobody chose: yes when the machine is signed in to as
/// root (its address's user, or a home folder of /root), whose files mostly are dot files.
/// "Show hidden files" chosen in this tab holds; chosen in another Files tab on the same
/// machine, it holds here too.
fn hidden_default(s: &mut Shell, tab: TabId) {
    let Some(server) = s.screens.files.get(&tab).filter(|st| st.browser.hidden_choice().is_none()).map(|st| st.browser.server().to_owned()) else { return };
    let chosen = s.screens.files.iter().filter(|(id, st)| **id != tab && st.browser.server() == server).find_map(|(_, st)| st.browser.hidden_choice());
    let user_is_root = s.bench.server(&server).is_some_and(|sv| sv.address.split_once('@').is_some_and(|(user, _)| user == "root"));
    let root = user_is_root || home_of(s, &server).as_deref() == Some("/root");
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    match chosen {
        Some(show) => state.browser.set_show_hidden(show),
        None => state.browser.hidden_by_default(root),
    }
}

/// Shows `path` (a click on it in a terminal): the folder it is in, with it picked out, and
/// then opens it: a file beside the list, a folder in its place. What the open file had
/// typed and not saved is kept: then the path is only picked out.
pub fn show_path(s: &mut Shell, tab: TabId, path: &str) {
    let (dir, name) = (paths::parent(path), paths::name(path).to_owned());
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    if state.editor.as_ref().is_some_and(|e| e.can_edit() && e.is_dirty(state.ui.draft_text())) {
        state.browser.select_when_listed(&name);
        return;
    }
    state.reveal = Some((dir.clone(), name.clone()));
    // A dot file pointed at shows with the other hidden ones (0.10).
    if name.starts_with('.') {
        state.browser.set_show_hidden(true);
    }
    go_now(s, tab, &dir);
    if let Some(state) = s.screens.files.get_mut(&tab) {
        state.browser.select_when_listed(&name);
    }
    reveal(s, tab);
}

/// Opens what [`show_path`] asked for, once the folder it is in is listed.
fn reveal(s: &mut Shell, tab: TabId) {
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    let Some((dir, name)) = state.reveal.clone() else { return };
    let b = &state.browser;
    let there = b.listing().is_some() && !b.is_loading() && (b.asked() == dir || b.dir() == dir);
    if !there {
        return;
    }
    state.reveal = None;
    if state.browser.entry(&name).is_some() {
        open(s, tab, &name);
    }
}

// ---- going places

/// Shows another folder, from anywhere: the palette, a file dropped on a terminal, a path
/// clicked in one. The open file closes, so when it has unsaved changes, "Discard your
/// changes?" comes first and nothing moves until it's answered.
pub fn go(s: &mut Shell, tab: TabId, to: &str) {
    let moves = s.screens.files.get(&tab).is_some_and(|st| st.browser.dir() != to);
    if moves {
        view::guarded(s, tab, view::After::Go(to.to_owned()));
    } else {
        go_now(s, tab, to);
    }
}

/// Back, as [`go`]: asks first when the open file has unsaved changes.
pub fn back(s: &mut Shell, tab: TabId) {
    view::guarded(s, tab, view::After::Back);
}

/// Forward, as [`go`]: asks first when the open file has unsaved changes.
pub fn forward(s: &mut Shell, tab: TabId) {
    view::guarded(s, tab, view::After::Forward);
}

/// The folder above, as [`go`]: asks first when the open file has unsaved changes.
pub fn up(s: &mut Shell, tab: TabId) {
    view::guarded(s, tab, view::After::Up);
}

/// Shows another folder now. The open file closes: only once it was asked about.
fn go_now(s: &mut Shell, tab: TabId, to: &str) {
    if let Some(state) = s.screens.files.get_mut(&tab).filter(|st| st.browser.dir() != to) {
        state.editor = None;
    }
    load(s, tab, |b, cache, now| b.go(to, cache, now));
}

fn back_now(s: &mut Shell, tab: TabId) {
    close_file(s, tab);
    load(s, tab, |b, cache, now| b.back(cache, now));
}

fn forward_now(s: &mut Shell, tab: TabId) {
    close_file(s, tab);
    load(s, tab, |b, cache, now| b.forward(cache, now));
}

fn up_now(s: &mut Shell, tab: TabId) {
    close_file(s, tab);
    load(s, tab, |b, cache, now| b.up(cache, now));
}

/// Lists the folder again (the button, "Try again").
pub fn refresh(s: &mut Shell, tab: TabId) {
    load(s, tab, |b, cache, now| b.refresh(cache, now));
}

/// Terminal `tab` signed in to its server (`Shell::link_status`): every Files tab there that
/// said "Sign in first" tries again by itself, as the connection is there to use now.
pub(crate) fn signed_in(s: &mut Shell, tab: TabId) {
    let Some(server) = s.bench.tab(tab).filter(|t| matches!(t.link, sshee_app::workbench::Link::Connected { .. })).map(|t| t.server.clone()) else { return };
    let waiting: Vec<TabId> = s.screens.files.iter().filter(|(_, st)| st.browser.server() == server && st.browser.error().is_some_and(|e| e.needs_terminal())).map(|(id, _)| *id).collect();
    for id in waiting {
        refresh(s, id);
    }
}

/// A double click or Enter on a row: into a folder, or the file opens beside the list.
pub fn open(s: &mut Shell, tab: TabId, name: &str) {
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    let Some(entry) = state.browser.entry(name) else { return };
    if entry.is_dir() {
        let to = state.browser.path_of(name);
        return go_now(s, tab, &to);
    }
    let dir = state.browser.dir().to_owned();
    let (editor, want) = Editor::open(&dir, name, &mut state.tickets);
    state.editor = Some(editor);
    state.browser.select_only(name);
    if let Some(want) = want {
        ask(s, tab, want);
    }
}

/// A file of the folder `dir`, which isn't the one on screen (picked in the folder looked into
/// with »): it opens beside the list, and the tab stays where it is (0.10).
pub fn open_from(s: &mut Shell, tab: TabId, dir: &str, name: &str) {
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    if dir == state.browser.dir() {
        return open(s, tab, name);
    }
    let (editor, want) = Editor::open(dir, name, &mut state.tickets);
    state.editor = Some(editor);
    if let Some(want) = want {
        ask(s, tab, want);
    }
}

/// Closes the open file. What was typed and not saved is gone: ask first.
pub fn close_file(s: &mut Shell, tab: TabId) {
    if let Some(state) = s.screens.files.get_mut(&tab) {
        state.editor = None;
    }
}

// ---- the open file

/// Saves the field's text over the file that was opened (the button, the save shortcut).
pub fn save(s: &mut Shell, tab: TabId, draft: &str) {
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    let Some(want) = state.editor.as_mut().and_then(|e| e.save(draft, &mut state.tickets)) else { return };
    ask(s, tab, want);
}

/// One of the three ways out when the file changed on the server (`Editor::conflict`).
pub fn resolve(s: &mut Shell, tab: TabId, choice: Choice, draft: &str) {
    let Some(State { browser, editor: Some(editor), tickets, .. }) = s.screens.files.get_mut(&tab) else { return };
    let Some(want) = editor.resolve(choice, draft, |name| browser.entry(name).is_some(), tickets) else { return };
    ask(s, tab, want);
}

// ---- changing what's in the folder

/// "New folder" with the name typed. False when the name can't be used: a note says why, and
/// the name field can stay.
pub fn new_folder(s: &mut Shell, tab: TabId, name: &str) -> bool {
    let Some(state) = s.screens.files.get(&tab) else { return false };
    let name = name.trim();
    if let Err(notice) = ops::check_name(name, state.browser.listing().map(|l| &**l)) {
        note(s, notice);
        return false;
    }
    let path = state.browser.path_of(name);
    ask(s, tab, Want::MakeDir { path });
    true
}

/// A row's new name. The same name or none changes nothing; false when the name can't be
/// used.
pub fn rename(s: &mut Shell, tab: TabId, from: &str, to: &str) -> bool {
    let Some(state) = s.screens.files.get(&tab) else { return false };
    let to = to.trim();
    if to.is_empty() || to == from {
        return true;
    }
    if let Err(notice) = ops::check_name(to, state.browser.listing().map(|l| &**l)) {
        note(s, notice);
        return false;
    }
    let (from, to) = (state.browser.path_of(from), state.browser.path_of(to));
    ask(s, tab, Want::Rename { from, to });
    true
}

fn run(s: &mut Shell, tab: TabId, verb: &str, names: &[String], want: impl FnOnce(String, Vec<String>) -> Want) {
    let Some(dir) = s.screens.files.get(&tab).map(|st| st.browser.dir().to_owned()) else { return };
    run_in(s, tab, &dir, verb, names, want);
}

fn run_in(s: &mut Shell, tab: TabId, dir: &str, verb: &str, names: &[String], want: impl FnOnce(String, Vec<String>) -> Want) {
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    if names.is_empty() {
        return;
    }
    let shown: Vec<&str> = names.iter().map(String::as_str).collect();
    state.busy = Some(ops::busy(verb, &shown));
    ask(s, tab, want(dir.to_owned(), names.to_vec()));
}

/// Deletes rows of `dir` on the tab's server: the folder the question was asked in, whatever
/// the tab shows by the time it's answered. Ask first: `ops::delete_question`.
pub fn delete(s: &mut Shell, tab: TabId, dir: &str, names: &[String]) {
    run_in(s, tab, dir, "Deleting", names, |dir, names| Want::Delete { dir, names });
}

/// Unpacks an archive next to itself.
pub fn unpack(s: &mut Shell, tab: TabId, name: &str) {
    run(s, tab, "Unpacking", &[name.to_owned()], |dir, mut names| Want::Extract { dir, name: names.remove(0) });
}

/// Rows dropped on a folder (`ops::drop_folder` says which, and whether they may).
pub fn move_into(s: &mut Shell, tab: TabId, names: &[String], to: &str) {
    run(s, tab, "Moving", names, |from, names| Want::Move { from, names, to: to.to_owned() });
}

/// "Undo" on the note after a move: on the machine it was made on, answered to the tab that
/// made it (only the folder cache, if that tab closed).
pub fn undo_move(s: &mut Shell, undo: &Undo) {
    s.files_ask(Ask { tab: undo.tab, server: undo.server.clone(), want: Want::Move { from: undo.from.clone(), names: undo.names.clone(), to: undo.to.clone() } });
}

/// "New file" with the name typed. Never over a file: the name is checked here, and the
/// backend makes it only where nothing has that name.
pub fn new_file(s: &mut Shell, tab: TabId, name: &str) -> bool {
    let Some(state) = s.screens.files.get(&tab) else { return false };
    let name = name.trim();
    if let Err(notice) = ops::check_name(name, state.browser.listing().map(|l| &**l)) {
        note(s, notice);
        return false;
    }
    let path = state.browser.path_of(name);
    ask(s, tab, Want::MakeFile { path });
    true
}

/// Packs `names` into a .tar.gz named `typed` (".tar.gz" put on when it lacks one).
pub fn archive(s: &mut Shell, tab: TabId, names: &[String], typed: &str) -> bool {
    let Some(state) = s.screens.files.get_mut(&tab) else { return false };
    let archive = ops::archive_file(typed.trim());
    if let Err(notice) = ops::check_name(&archive, state.browser.listing().map(|l| &**l)) {
        note(s, notice);
        return false;
    }
    state.busy = Some(ops::packing(&archive));
    let dir = state.browser.dir().to_owned();
    ask(s, tab, Want::Archive { dir, names: names.to_vec(), archive });
    true
}

// ---- copies to a folder or a server

/// "Copy to another folder or server…": the dialog, for these rows. It opens on the server you
/// copied to last time, else another server than this one, else this one (as 0.10's did).
pub fn open_copy(s: &mut Shell, tab: TabId, names: &[String]) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    let paths: Vec<String> = names.iter().map(|n| state.browser.path_of(n)).collect();
    if paths.is_empty() {
        return;
    }
    let here = state.browser.server().to_owned();
    let servers = copy_targets(s);
    let last = s.screens.copy_last.last.clone().filter(|k| servers.contains(k));
    // Another server than this one, first: a copy to the same one is to another folder.
    let to = last.or_else(|| servers.iter().find(|k| **k != here).cloned()).or_else(|| servers.first().cloned());
    if let Some(state) = s.screens.files.get_mut(&tab) {
        state.copy = Some(CopyTo { paths, to: None, place: None, asked: None, error: None });
    }
    if let Some(to) = to {
        copy_server(s, tab, &to);
    }
}

/// The servers a copy can go to: every server in the list (the same one too), not this
/// computer, which has Download.
pub fn copy_targets(s: &Shell) -> Vec<String> {
    s.bench.servers().iter().filter(|sv| !sv.is_local()).map(|sv| sv.key()).collect()
}

/// The copy goes to `server`: its folder from last time there, else the one this tab shows
/// (on the same server), else its home.
pub fn copy_server(s: &mut Shell, tab: TabId, server: &str) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    let start = match s.screens.copy_last.dirs.get(server) {
        Some(dir) => dir.clone(),
        None if state.browser.server() == server => state.browser.dir().to_owned(),
        None => "~".to_owned(),
    };
    if let Some(copy) = s.screens.files.get_mut(&tab).and_then(|st| st.copy.as_mut()) {
        copy.to = Some(server.to_owned());
        copy.place = None;
    }
    copy_go(s, tab, &start);
}

/// Looks inside a folder of the server the copy goes to.
pub fn copy_go(s: &mut Shell, tab: TabId, path: &str) {
    let Some(copy) = s.screens.files.get_mut(&tab).and_then(|st| st.copy.as_mut()) else { return };
    let Some(server) = copy.to.clone() else { return };
    copy.asked = Some(path.to_owned());
    copy.error = None;
    // Seen a moment ago: at once, and asked again all the same.
    copy_listed(s, tab, &server, path);
    s.files_ask(Ask { tab, server, want: Want::Peek { path: path.to_owned() } });
}

/// A listing of the copy's server came: the dialog shows it if it waits for it.
fn copy_listed(s: &mut Shell, tab: TabId, server: &str, asked: &str) {
    let Some(cached) = s.screens.dirs.get(server, asked).map(|c| c.listing.clone()) else { return };
    let Some(copy) = s.screens.files.get_mut(&tab).and_then(|st| st.copy.as_mut()) else { return };
    if copy.to.as_deref() != Some(server) || copy.asked.as_deref() != Some(asked) {
        return;
    }
    let mut dirs: Vec<String> = cached.entries.iter().filter(|e| e.is_dir() && !e.is_hidden()).map(|e| e.name.clone()).collect();
    dirs.sort_by(|a, b| sshee_app::files::browser::natural_cmp(a, b));
    copy.place = Some((cached.path.clone(), dirs));
    copy.asked = None;
    // The path field shows where it is now.
    if let Some(state) = s.screens.files.get_mut(&tab) {
        state.ui.copy_typed = rune::TextEdit::new(cached.path.clone());
    }
}

/// Why the copy can't go into the folder the dialog shows (`ops::copy_blocked`).
pub fn copy_blocked(s: &Shell, tab: TabId) -> Option<String> {
    let state = s.screens.files.get(&tab)?;
    let copy = state.copy.as_ref()?;
    let (dest, _) = copy.place.as_ref()?;
    let same = copy.to.as_deref() == Some(state.browser.server());
    ops::copy_blocked(same, &paths::parent(copy.paths.first()?), &copy.paths, dest)
}

/// "Copy here": the copy starts, under a card, into the folder the dialog shows.
pub fn copy_here(s: &mut Shell, tab: TabId) {
    if copy_blocked(s, tab).is_some() {
        return;
    }
    let Some(copy) = s.screens.files.get_mut(&tab).and_then(|st| st.copy.take()) else { return };
    let (Some(to), Some((dir, _))) = (copy.to, copy.place) else { return };
    s.screens.copy_last.last = Some(to.clone());
    s.screens.copy_last.dirs.insert(to.clone(), dir.clone());
    send_copy(s, tab, to, copy.paths, dir, false);
}

fn send_copy(s: &mut Shell, tab: TabId, to: String, paths: Vec<String>, dir: String, overwrite: bool) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    let from = state.browser.server().to_owned();
    let id = s.screens.transfers.begin(Kind::Copy, files_label(&paths), tab, &from);
    ask(s, tab, Want::CopyTo { id, to, paths, dir, overwrite });
}

// ---- files copied on this computer

/// A paste while a Files tab is on screen (`Shell::paste`). True: Files took it. Files copied
/// on this computer go into the folder on screen: to a server only after a question that
/// names them all and what of them looks private.
pub fn paste(s: &mut Shell, content: &rune::ClipboardContent) -> bool {
    let Some(tab) = s.bench.active().filter(|t| t.kind == TabKind::Files).map(|t| t.id) else { return false };
    let Some(state) = s.screens.files.get(&tab) else { return false };
    if state.browser.listing().is_none() {
        return true;
    }
    let paths = ops::copied_files(content.files(), content.text());
    if paths.is_empty() {
        note(s, ops::nothing_to_paste(if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" }));
        return true;
    }
    if !on_server(s, tab) {
        upload(s, tab, paths);
        return true;
    }
    let server = s.bench.server(state.browser.server()).map(|sv| s.server_shown(&sv.key(), &sv.name).into_owned()).unwrap_or_default();
    let files: Vec<(String, bool)> = paths.iter().map(|p| (s.text_shown(&p.to_string_lossy()).into_owned(), p.is_dir())).collect();
    let question = ops::paste_question(&files, &server, &s.text_shown(state.browser.dir()));
    if let Some(state) = s.screens.files.get_mut(&tab) {
        let dir = state.browser.dir().to_owned();
        state.paste = Some(Paste { question, paths, dir });
    }
    true
}

/// The answer to [`State::paste`].
pub fn answer_paste(s: &mut Shell, tab: TabId, yes: bool) {
    let Some(Paste { paths, dir, .. }) = s.screens.files.get_mut(&tab).and_then(|st| st.paste.take()) else { return };
    if yes {
        send(s, tab, paths, dir, false);
    }
}

/// Files dropped on the list from another app (`El::on_drop_files`): into `folder` (a row's
/// folder they landed on) or the folder on screen. The seam the platform's drag and drop
/// feeds; on this computer it's a copy.
pub fn dropped(s: &mut Shell, tab: TabId, paths: Vec<PathBuf>, folder: Option<String>) {
    let Some(dir) = folder.or_else(|| s.screens.files.get(&tab).map(|st| st.browser.dir().to_owned())) else { return };
    send(s, tab, paths, dir, false);
}

// ---- terminals in a folder

fn shell_of(s: &Shell, tab: TabId) -> ops::Shell {
    if !on_server(s, tab) && cfg!(windows) {
        ops::Shell::PowerShell
    } else {
        ops::Shell::Posix
    }
}

/// "Open new terminal here": a new terminal on the tab's machine, in `folder`.
pub fn terminal_here(s: &mut Shell, tab: TabId, folder: &str, tx: &mut rune::Tx) {
    let Some(cmd) = ops::typed_cd(folder, shell_of(s, tab), Some("clear")) else { return note(s, ops::cant_open_terminal()) };
    let Some(server) = s.screens.files.get(&tab).map(|st| st.browser.server().to_owned()) else { return };
    let id = s.bench.open(TabKind::Terminal, &server);
    s.settle(tx);
    s.screens.palette.pending.insert(id, (cmd, true));
}

/// "Open in main terminal": the terminal you already use on this machine goes into `folder`
/// (a new one when there's none).
pub fn main_terminal_here(s: &mut Shell, tab: TabId, folder: &str, tx: &mut rune::Tx) {
    let Some(cmd) = ops::typed_cd(folder, shell_of(s, tab), None) else { return note(s, ops::cant_open_terminal()) };
    let Some(server) = s.screens.files.get(&tab).map(|st| st.browser.server().to_owned()) else { return };
    s.run_on(&server, &cmd, true, tx);
}

// ---- transfers

/// Downloads rows of the folder on screen into this computer's Downloads folder, under one
/// card. Nothing on this computer itself.
pub fn download(s: &mut Shell, tab: TabId, names: &[String]) {
    let Some(dir) = s.screens.files.get(&tab).map(|st| st.browser.dir().to_owned()) else { return };
    download_to(s, tab, &dir, names, None);
}

/// Rows of `dir` downloaded into the folder `into` on this computer (rows dragged out of the
/// window, `view`'s question first), or Downloads. Only while the tab still shows `dir`: the
/// rows are its. Nothing on this computer itself: the files are already here.
pub fn download_to(s: &mut Shell, tab: TabId, dir: &str, names: &[String], into: Option<PathBuf>) {
    let Some(state) = s.screens.files.get(&tab).filter(|st| st.browser.server() != "local" && st.browser.dir() == dir) else { return };
    let items: Vec<Item> = names.iter().filter_map(|n| state.browser.entry(n)).map(|e| Item { path: state.browser.path_of(&e.name), is_dir: e.is_dir() }).collect();
    if items.is_empty() {
        return;
    }
    let paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
    let id = s.screens.transfers.begin(Kind::Download, files_label(&paths), tab, state.browser.server());
    ask(s, tab, match into {
        Some(into) => Want::DownloadInto { id, items, into },
        None => Want::Download { id, items },
    });
}

/// The file open beside the list, downloaded into Downloads. It may be of another folder than
/// the one on screen (opened from the folder looked into with »).
pub fn download_file(s: &mut Shell, tab: TabId, path: &str) {
    let Some(server) = s.screens.files.get(&tab).map(|st| st.browser.server().to_owned()).filter(|sv| sv != "local") else { return };
    let id = s.screens.transfers.begin(Kind::Download, files_label(&[path]), tab, &server);
    ask(s, tab, Want::Download { id, items: vec![Item { path: path.to_owned(), is_dir: false }] });
}

/// Rows dragged onto a folder of another server's Files tab: copied straight across, under a
/// card, as "Copy to another folder or server" does (0.10's `copyToServer`).
pub fn copy_dragged(s: &mut Shell, tab: TabId, names: &[String], to: &str, dir: &str) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    let paths: Vec<String> = names.iter().map(|n| state.browser.path_of(n)).collect();
    if !paths.is_empty() {
        send_copy(s, tab, to.to_owned(), paths, dir.to_owned(), false);
    }
}

fn send(s: &mut Shell, tab: TabId, paths: Vec<PathBuf>, dir: String, overwrite: bool) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    if paths.is_empty() {
        return;
    }
    let server = state.browser.server();
    // On this computer it's a copy from one folder to another.
    let kind = if server == "local" { Kind::Copy } else { Kind::Upload };
    let id = s.screens.transfers.begin(kind, files_label(&paths), tab, server);
    ask(s, tab, Want::Upload { id, paths, dir, overwrite });
}

/// Sends files and folders of this computer into the folder on screen (picked, or dropped on
/// the window). Nothing there is replaced: if a name is taken nothing is sent, and
/// [`State::replace`] holds the question.
pub fn upload(s: &mut Shell, tab: TabId, paths: Vec<PathBuf>) {
    let Some(dir) = s.screens.files.get(&tab).map(|st| st.browser.dir().to_owned()) else { return };
    send(s, tab, paths, dir, false);
}

/// The answer to [`State::replace`]: yes sends the same again, replacing; no leaves it all.
pub fn answer_replace(s: &mut Shell, tab: TabId, yes: bool) {
    let Some(Replace { paths, dir, copy, .. }) = s.screens.files.get_mut(&tab).and_then(|st| st.replace.take()) else { return };
    match (yes, copy) {
        (true, Some((to, from))) => send_copy(s, tab, to, from, dir, true),
        (true, None) => send(s, tab, paths, dir, true),
        _ => {}
    }
}

/// A card's Stop button. The card says "Stopping…" until the backend says it ended.
pub fn stop_transfer(s: &mut Shell, id: TransferId) {
    if !s.screens.transfers.stopping(id) {
        return;
    }
    let Some((tab, server)) = s.screens.transfers.get(id).map(|t| (t.tab, t.server.clone())) else { return };
    s.files_ask(Ask { tab, server, want: Want::CancelTransfer { id } });
}

// ---- bookmarks, menus, notes

/// Pins a folder of the tab's machine, or takes its pin away.
pub fn toggle_bookmark(s: &mut Shell, tab: TabId, path: &str) {
    let Some(server) = s.screens.files.get(&tab).map(|st| st.browser.server().to_owned()) else { return };
    bookmarks::toggle_pin(&mut s.screens.bookmarks, &server, path);
    bookmarks_changed(s);
}

/// What a right click offers: on the row `clicked`, or on the empty space (`None`). Call
/// `Browser::context_click` first, so the selection is what the menu is about.
pub fn menu_for(s: &Shell, tab: TabId, clicked: Option<&str>) -> Vec<MenuRow> {
    let Some(state) = s.screens.files.get(&tab) else { return Vec::new() };
    let browser = &state.browser;
    let local = browser.server() == "local";
    let chosen = edit::chosen(s);
    menu::menu(&menu::Context {
        dir: browser.dir(),
        clicked: clicked.and_then(|name| browser.entry(name)),
        selected: &browser.selected_entries(),
        visible: browser.len(),
        on_server: !local,
        on_this_windows: local && cfg!(windows),
        can_copy_out: !copy_targets(s).is_empty(),
        can_back: browser.can_go_back(),
        can_forward: browser.can_go_forward(),
        paste_key: if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" },
        pins: bookmarks::pins(&s.screens.bookmarks, browser.server()),
        show_hidden: browser.show_hidden(),
        editor: chosen.as_ref().map(|e| e.name.as_str()),
        editing: clicked.is_some_and(|name| edit::session_of(s, tab, name).is_some()),
    })
}

// ---- answers

/// A backend's answer, on the UI thread (`Remote::files`): to the tab that asked, the folder
/// cache and the transfer cards. An answer for a tab that closed still updates the cache and
/// says how a transfer ended.
pub fn deliver(s: &mut Shell, reply: Reply) {
    apply(s, reply);
    // The field takes a file's text when it arrives; a picture starts decoding.
    view::sync(s);
}

fn apply(s: &mut Shell, reply: Reply) {
    let Reply { tab, server, answer } = reply;
    match answer {
        Answer::Listed { ticket, asked, result } => {
            let now = now_ms(s);
            let screens = &mut s.screens;
            let Some(state) = screens.files.get_mut(&tab).filter(|st| st.browser.waits_for(ticket)) else { return };
            match result {
                Ok((listing, clock)) => {
                    let kept = screens.dirs.remember(&server, &asked, listing, clock, now);
                    state.browser.listed(ticket, kept, clock);
                    // A home folder of /root now known: root's dot files show.
                    hidden_default(s, tab);
                    keep_place(s, tab);
                    reveal(s, tab);
                }
                Err(failure) => {
                    state.browser.failed(ticket, failure, &mut screens.dirs);
                }
            }
        }
        Answer::Read { ticket, path, result } => {
            if let Some(editor) = s.screens.files.get_mut(&tab).and_then(|st| st.editor.as_mut()) {
                editor.read(ticket, &path, result);
            }
        }
        Answer::Image { ticket, path, result } => {
            if let Some(editor) = s.screens.files.get_mut(&tab).and_then(|st| st.editor.as_mut()) {
                editor.image(ticket, &path, result);
            }
        }
        Answer::Saved { ticket, path, result } => {
            let Some(State { browser, editor: Some(editor), tickets, .. }) = s.screens.files.get_mut(&tab) else { return };
            match editor.saved(ticket, result, tickets) {
                AfterSave::Stale | AfterSave::Conflict => {}
                AfterSave::Done { notice, copy } => {
                    if let Some(name) = &copy {
                        browser.select_when_listed(name);
                    }
                    note(s, notice);
                    changed(s, &server, &paths::parent(&path));
                }
                AfterSave::Again(want) => ask(s, tab, want),
                AfterSave::Failed(notice) => note(s, notice),
            }
        }
        Answer::Peeked { asked, result } => {
            if let Ok((listing, clock)) = result {
                let now = now_ms(s);
                s.screens.dirs.remember(&server, &asked, listing, clock, now);
                copy_listed(s, tab, &server, &asked);
            } else if let Err(failure) = result {
                if let Some(copy) = s.screens.files.get_mut(&tab).and_then(|st| st.copy.as_mut()).filter(|c| c.to.as_deref() == Some(&*server) && c.asked.as_deref() == Some(&*asked)) {
                    copy.error = Some(failure.message);
                    copy.asked = None;
                }
            }
        }
        Answer::MadeFile { path, result } => match result {
            Ok(()) => {
                let dir = paths::parent(&path);
                if let Some(state) = s.screens.files.get_mut(&tab).filter(|st| st.browser.dir() == dir) {
                    state.browser.select_when_listed(paths::name(&path));
                }
                changed(s, &server, &dir);
            }
            Err(failure) => note(s, ops::made_file_failed(&failure)),
        },
        Answer::Archived { dir, archive, count, result } => {
            if let Some(state) = s.screens.files.get_mut(&tab) {
                state.busy = None;
                if result.is_ok() && state.browser.dir() == dir {
                    state.browser.select_when_listed(&archive);
                }
            }
            note(s, ops::archived(&archive, count, &result));
            if result.is_ok() {
                changed(s, &server, &dir);
            }
        }
        Answer::CopiedTo { id, to, paths: from, dir, overwrite, result } => {
            let Some(card) = s.screens.transfers.end(id) else { return };
            if let (Err(failure), false) = (&result, overwrite) {
                if let Some(state) = s.screens.files.get_mut(&tab).filter(|_| failure.kind == FailureKind::Exists) {
                    state.replace = Some(Replace { question: ops::replace_question(&failure.message, card.kind), paths: Vec::new(), dir, copy: Some((to, from)) });
                    return;
                }
            }
            let name = s.bench.server(&to).map(|sv| sv.name.clone()).unwrap_or_else(|| to.clone());
            note(s, ops::copied_to(&card, &name, &dir, &result));
            if result.is_ok() {
                changed(s, &to, &dir);
            }
        }
        Answer::MadeDir { path, result } => match result {
            Ok(()) => {
                let dir = paths::parent(&path);
                if let Some(state) = s.screens.files.get_mut(&tab).filter(|st| st.browser.dir() == dir) {
                    state.browser.select_when_listed(paths::name(&path));
                }
                changed(s, &server, &dir);
            }
            Err(failure) => note(s, ops::made_dir_failed(&failure)),
        },
        Answer::Renamed { from, to, result } => match result {
            Ok(()) => {
                let dir = paths::parent(&from);
                // The folder that was renamed, and all seen inside it, are under old names.
                s.screens.dirs.forget(&server, &from);
                if let Some(state) = s.screens.files.get_mut(&tab).filter(|st| st.browser.dir() == dir) {
                    state.browser.select_when_listed(paths::name(&to));
                    if let Some(editor) = state.editor.as_mut().filter(|e| e.path() == from) {
                        editor.renamed(paths::name(&to));
                    }
                }
                changed(s, &server, &dir);
            }
            Err(failure) => note(s, ops::rename_failed(&failure)),
        },
        Answer::Deleted { dir, deleted, failed } => {
            for name in &deleted {
                s.screens.dirs.forget(&server, &paths::join(&dir, name));
            }
            if let Some(state) = s.screens.files.get_mut(&tab) {
                state.busy = None;
                state.browser.clear_selection();
                // The open file is gone: so is its editor.
                if state.editor.as_ref().is_some_and(|e| e.dir() == dir && deleted.iter().any(|n| n == e.name())) {
                    state.editor = None;
                }
            }
            for (name, failure) in &failed {
                note(s, ops::delete_failed(name, failure));
            }
            changed(s, &server, &dir);
        }
        Answer::Extracted { dir, name, result } => {
            if let Some(state) = s.screens.files.get_mut(&tab) {
                state.busy = None;
                if let Some(made) = result.as_ref().ok().filter(|_| state.browser.dir() == dir) {
                    state.browser.select_when_listed(made);
                }
            }
            note(s, ops::unpacked(&name, &result));
            if result.is_ok() {
                changed(s, &server, &dir);
            }
        }
        Answer::Moved { from, to, moved, taken, failure } => {
            for name in &moved {
                s.screens.dirs.forget(&server, &paths::join(&from, name));
            }
            if let Some(state) = s.screens.files.get_mut(&tab) {
                state.busy = None;
                state.browser.clear_selection();
            }
            for notice in ops::move_notices(tab, &server, &from, &to, &moved, &taken, failure.as_ref()) {
                note(s, notice);
            }
            changed(s, &server, &from);
            changed(s, &server, &to);
        }
        Answer::Progress { id, progress } => s.screens.transfers.progress(id, &progress),
        Answer::Downloaded { id, result } => {
            if let Some(card) = s.screens.transfers.end(id) {
                note(s, ops::downloaded(&card.label, &result));
            }
        }
        Answer::Uploaded { id, paths, dir, overwrite, result } => {
            let Some(card) = s.screens.transfers.end(id) else { return };
            // Names are taken and nothing was sent: ask, in the tab that sent it.
            if let (Err(failure), false) = (&result, overwrite) {
                if let Some(state) = s.screens.files.get_mut(&tab).filter(|_| failure.kind == FailureKind::Exists) {
                    state.replace = Some(Replace { question: ops::replace_question(&failure.message, card.kind), paths, dir, copy: None });
                    return;
                }
            }
            note(s, ops::uploaded(&card, &dir, &result));
            if result.is_ok() {
                changed(s, &server, &dir);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use sshee_app::files::editor::Body;
    use sshee_app::files::protocol::SaveFailure;
    use sshee_app::files::{Clock, Entry, EntryKind, Failure, Listing, Stamp, TextFile};
    use sshee_app::transfers::Progress;
    use sshee_app::workbench::{Server, Workbench};

    use super::*;
    use crate::shell::{Backend, Options};
    use crate::terminal::{NoLink, TermLink};

    const WEB: &str = "host:web";

    /// A backend that only writes down what it was asked: Files' requests, and the edit
    /// bridge's.
    struct Asked(Arc<Mutex<Vec<Ask>>>, Arc<Mutex<Vec<sshee_app::files::edit::EditAsk>>>);

    impl Backend for Asked {
        fn terminal(&mut self, _: Option<&Server>, _: &Tab) -> Box<dyn TermLink> {
            Box::new(NoLink)
        }
        fn files(&mut self, ask: Ask) {
            self.0.lock().unwrap().push(ask);
        }
        fn edit(&mut self, ask: sshee_app::files::edit::EditAsk) {
            self.1.lock().unwrap().push(ask);
        }
        fn replays(&self) -> bool {
            true
        }
    }

    struct Window {
        s: Shell,
        asked: Arc<Mutex<Vec<Ask>>>,
        edits: Arc<Mutex<Vec<sshee_app::files::edit::EditAsk>>>,
        tab: TabId,
    }

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry { name: name.into(), kind, is_link: false, size: 100, modified: 1_700_000_000, mode: "-rw-r--r--".into() }
    }

    fn home() -> Listing {
        Listing { path: "/home/deploy".into(), entries: vec![entry("shop", EntryKind::Dir), entry("notes.md", EntryKind::File), entry("site.zip", EntryKind::File)] }
    }

    impl Window {
        /// A window whose tab on screen is Files on a server, as after `Shell::open_files`.
        fn open() -> Window {
            let mut bench = Workbench::new(vec![Server::this_computer(), Server::host("web", "web-7731", "deploy@203.0.113.17")]);
            let tab = bench.open_files(WEB);
            let (asked, edits) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
            let s = Shell::new(bench, Box::new(Asked(asked.clone(), edits.clone())), Options::default(), None);
            Window { s, asked, edits, tab }
        }

        /// What the backend was asked since the last look.
        fn wants(&mut self) -> Vec<Want> {
            std::mem::take(&mut *self.asked.lock().unwrap()).into_iter().map(|a| a.want).collect()
        }

        fn answer(&mut self, answer: Answer) {
            deliver(&mut self.s, Reply { tab: self.tab, server: WEB.into(), answer });
        }

        fn state(&self) -> &State {
            &self.s.screens.files[&self.tab]
        }

        /// Answers the listing that is waited for with `listing`.
        fn list(&mut self, listing: Listing) {
            let wants = self.wants();
            let Some(Want::List { ticket, path }) = wants.last().cloned() else { panic!("no listing asked: {wants:?}") };
            self.answer(Answer::Listed { ticket, asked: path, result: Ok((listing, Clock { now: 1_700_000_500, utc_offset: 0 })) });
        }

        /// The home folder on screen.
        fn at_home() -> Window {
            let mut w = Window::open();
            w.list(home());
            w
        }

        fn titles(&self) -> Vec<&str> {
            self.s.screens.notes.titles()
        }
    }

    #[test]
    fn an_unused_files_tab_lets_go_off_screen_and_lists_again_when_back() {
        let mut w = Window::at_home();
        w.s.settings.update(|v| v.idle_sleep = 20.0);
        let mut tx = rune::Tx::default();
        tick(&mut w.s, 0.0, &mut tx);
        // Another tab, on this computer: the Files tab is off screen from here on.
        let other = w.s.bench.open(TabKind::Terminal, "local");
        assert!(!w.s.tabs_on_screen().contains(&w.tab));
        tick(&mut w.s, 10.0, &mut tx);
        tick(&mut w.s, 20.0 * 60.0 - 1.0, &mut tx);
        assert_eq!(w.wants(), [], "not yet");
        tick(&mut w.s, 20.0 * 60.0 + 1.0, &mut tx);
        assert_eq!(w.wants(), [Want::LetGo]);
        tick(&mut w.s, 40.0 * 60.0, &mut tx);
        assert_eq!(w.wants(), [], "once");
        // Back on screen: it lists its folder again, which signs in again.
        w.s.bench.focus(w.tab);
        tick(&mut w.s, 41.0 * 60.0, &mut tx);
        assert!(matches!(w.wants().as_slice(), [Want::List { path, .. }] if path == "/home/deploy"));
        // A terminal connected to the same server keeps the connection: nothing to let go of.
        let term = w.s.bench.open(TabKind::Terminal, WEB);
        w.s.bench.set_link(term, sshee_app::workbench::Link::Connected { durable: true });
        w.s.bench.focus(other);
        tick(&mut w.s, 50.0 * 60.0, &mut tx);
        tick(&mut w.s, 80.0 * 60.0, &mut tx);
        assert_eq!(w.wants(), []);
        // Off, nothing lets go.
        w.s.settings.update(|v| v.idle_sleep = 0.0);
        w.s.bench.close(term);
        tick(&mut w.s, 200.0 * 60.0, &mut tx);
        assert_eq!(w.wants(), []);
    }

    #[test]
    fn a_files_tab_lists_its_folder_when_it_is_first_shown() {
        let mut w = Window::open();
        assert_eq!(w.wants(), [Want::List { ticket: 1, path: "~".into() }]);
        assert!(w.state().browser.is_loading());
        w.asked.lock().unwrap().push(Ask { tab: w.tab, server: WEB.into(), want: Want::List { ticket: 1, path: "~".into() } });
        w.list(home());
        let b = &w.state().browser;
        assert_eq!((b.dir(), b.len(), b.is_loading()), ("/home/deploy", 3, false));
        assert_eq!(b.clock().now, 1_700_000_500);
        // The tab keeps the folder for the next run, and it's the newest recent one.
        assert_eq!(w.s.bench.tab(w.tab).unwrap().path.as_deref(), Some("/home/deploy"));
        assert_eq!(bookmarks::recent(&w.s.screens.bookmarks, WEB), ["/home/deploy"]);
        assert!(w.s.screens.dirs.get(WEB, "~").is_some());
        assert!(on_server(&w.s, w.tab));
        // Shown again (another settle) asks nothing more.
        shown(&mut w.s);
        assert!(w.wants().is_empty());
    }

    #[test]
    fn root_sees_dot_files_unless_it_chose_not_to() {
        // deploy@: hidden as ever.
        let w = Window::at_home();
        assert!(!w.state().browser.show_hidden());
        // A home folder of /root (this computer, or a server whose user isn't in its address).
        let mut w = Window::open();
        w.list(Listing { path: "/root".into(), entries: vec![entry(".bashrc", EntryKind::File), entry("backup.sh", EntryKind::File)] });
        assert!(w.state().browser.show_hidden() && w.state().browser.len() == 2);
        // Turned off here: it stays off, and a second Files tab on the machine starts off too.
        w.s.screens.files.get_mut(&w.tab).unwrap().browser.set_show_hidden(false);
        refresh(&mut w.s, w.tab);
        w.list(Listing { path: "/root".into(), entries: vec![entry(".bashrc", EntryKind::File)] });
        assert!(!w.state().browser.show_hidden());
        let second = w.s.bench.open_files(WEB);
        w.s.bench.focus(second);
        shown(&mut w.s);
        assert!(!w.s.screens.files[&second].browser.show_hidden());
        // root@ in the address: shown from the first moment.
        let mut bench = Workbench::new(vec![Server::host("web", "web-7731", "root@203.0.113.17")]);
        let tab = bench.open_files(WEB);
        let s = Shell::new(bench, Box::new(Asked(Arc::default(), Arc::default())), Options::default(), None);
        assert!(s.screens.files[&tab].browser.show_hidden());
    }

    #[test]
    fn a_tab_from_the_last_run_opens_where_it_was() {
        let mut bench = Workbench::new(vec![Server::host("web", "web-7731", "deploy@203.0.113.17")]);
        let tab = bench.open_files(WEB);
        bench.set_path(tab, Some("/srv/www".into()));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let _s = Shell::new(bench, Box::new(Asked(asked.clone(), Arc::default())), Options::default(), None);
        assert_eq!(asked.lock().unwrap()[0], Ask { tab, server: WEB.into(), want: Want::List { ticket: 1, path: "/srv/www".into() } });
    }

    #[test]
    fn going_into_a_folder_and_back() {
        let mut w = Window::at_home();
        open(&mut w.s, w.tab, "shop");
        assert_eq!(w.wants(), [Want::List { ticket: 2, path: "/home/deploy/shop".into() }]);
        w.asked.lock().unwrap().push(Ask { tab: w.tab, server: WEB.into(), want: Want::List { ticket: 2, path: "/home/deploy/shop".into() } });
        w.list(Listing { path: "/home/deploy/shop".into(), entries: vec![] });
        assert_eq!(w.state().browser.dir(), "/home/deploy/shop");
        assert_eq!(bookmarks::recent(&w.s.screens.bookmarks, WEB), ["/home/deploy/shop", "/home/deploy"]);
        // Back: time stands still in a test, so the folder seen "just now" is shown at once.
        back(&mut w.s, w.tab);
        assert!(w.wants().is_empty());
        assert_eq!(w.state().browser.dir(), "/home/deploy");
        assert_eq!(w.s.bench.tab(w.tab).unwrap().path.as_deref(), Some("/home/deploy"));
        refresh(&mut w.s, w.tab);
        assert!(matches!(w.wants()[..], [Want::List { ticket: 3, .. }]));
    }

    #[test]
    fn an_old_answer_changes_nothing_and_an_error_shows() {
        let mut w = Window::at_home();
        go(&mut w.s, w.tab, "/etc");
        go(&mut w.s, w.tab, "/var");
        let wants = w.wants();
        let [Want::List { ticket: old, .. }, Want::List { ticket: new, .. }] = wants[..] else { panic!("{wants:?}") };
        w.answer(Answer::Listed { ticket: old, asked: "/etc".into(), result: Ok((Listing { path: "/etc".into(), entries: vec![] }, Clock::default())) });
        assert_eq!(w.state().browser.dir(), "/home/deploy");
        assert!(w.s.screens.dirs.get(WEB, "/etc").is_none(), "an answer nobody waits for isn't kept");
        w.answer(Answer::Listed { ticket: new, asked: "/var".into(), result: Err(Failure::new(FailureKind::NotFound, "This folder doesn't exist or you can't open it.")) });
        assert!(w.state().browser.problem().is_some() && w.state().browser.listing().is_none());
        // An answer for a tab that closed is dropped.
        deliver(&mut w.s, Reply { tab: TabId(99), server: WEB.into(), answer: Answer::Listed { ticket: 1, asked: "~".into(), result: Ok((home(), Clock::default())) } });
    }

    /// "Sign in first": once a terminal there is in, the folder is asked for again by itself.
    #[test]
    fn a_folder_that_waited_for_a_sign_in_lists_once_a_terminal_there_is_in() {
        let mut w = Window::open();
        let Some(Want::List { ticket, path }) = w.wants().pop() else { panic!() };
        w.answer(Answer::Listed { ticket, asked: path, result: Err(Failure::new(FailureKind::Auth, "This server needs you to sign in.")) });
        assert!(matches!(w.state().browser.problem(), Some(sshee_app::files::browser::Problem::Page { open_terminal: true, .. })));
        let term = w.s.bench.open(TabKind::Terminal, WEB);
        // Connecting is not in yet.
        w.s.bench.set_link(term, sshee_app::workbench::Link::Connecting);
        signed_in(&mut w.s, term);
        assert!(w.wants().is_empty());
        w.s.bench.set_link(term, sshee_app::workbench::Link::Connected { durable: true });
        signed_in(&mut w.s, term);
        let wants = w.wants();
        let [Want::List { ticket, ref path }] = wants[..] else { panic!("{wants:?}") };
        // A folder that failed for another reason isn't asked again.
        w.answer(Answer::Listed { ticket, asked: path.clone(), result: Err(Failure::new(FailureKind::NotFound, "Gone.")) });
        signed_in(&mut w.s, term);
        assert!(w.wants().is_empty());
    }

    fn stamp(n: u64) -> Stamp {
        Stamp { size: n, modified: n as i64, sha256: None }
    }

    #[test]
    fn a_file_opens_saves_and_a_conflict_ends_as_a_copy_picked_out_in_the_list() {
        let mut w = Window::at_home();
        open(&mut w.s, w.tab, "notes.md");
        let wants = w.wants();
        let [Want::Read { ticket, ref path }] = wants[..] else { panic!("{wants:?}") };
        assert_eq!(w.state().browser.selected(), ["notes.md"]);
        let file = TextFile { path: path.clone(), content: Some("one\n".into()), binary: false, size: 4, lossy: false, crlf: false };
        w.answer(Answer::Read { ticket, path: path.clone(), result: Ok((file, stamp(1))) });
        assert_eq!(w.state().editor.as_ref().unwrap().text(), Some("one\n"));

        save(&mut w.s, w.tab, "mine\n");
        let wants = w.wants();
        let [Want::Save { ticket, ref path, ref expected, .. }] = wants[..] else { panic!("{wants:?}") };
        assert_eq!(expected, &Some(stamp(1)));
        w.answer(Answer::Saved { ticket, path: path.clone(), result: Err(SaveFailure::Conflict { now: Some(stamp(2)) }) });
        assert!(w.state().editor.as_ref().unwrap().conflict().is_some());
        assert!(w.wants().is_empty());

        resolve(&mut w.s, w.tab, Choice::SaveCopy, "mine\n");
        let wants = w.wants();
        let [Want::Save { ticket, ref path, expected: None, .. }] = wants[..] else { panic!("{wants:?}") };
        assert_eq!(path, "/home/deploy/notes (my copy).md");
        w.answer(Answer::Saved { ticket, path: path.clone(), result: Ok(stamp(3)) });
        assert_eq!(w.titles(), ["Saved your version as “notes (my copy).md”"]);
        assert_eq!(w.state().editor.as_ref().unwrap().name(), "notes (my copy).md");
        // The folder is listed again, and the copy is picked out once it's there.
        assert!(matches!(w.wants()[..], [Want::List { .. }]));
        assert_eq!(w.state().browser.selected(), ["notes (my copy).md"]);
    }

    #[test]
    fn a_picture_and_a_file_that_cant_be_shown() {
        let mut w = Window::at_home();
        let mut listing = home();
        listing.entries.push(entry("logo.png", EntryKind::File));
        listing.entries.push(entry("intro.mp4", EntryKind::File));
        refresh(&mut w.s, w.tab);
        w.list(listing);
        open(&mut w.s, w.tab, "logo.png");
        let wants = w.wants();
        let [Want::ReadImage { ticket, ref path }] = wants[..] else { panic!("{wants:?}") };
        w.answer(Answer::Image { ticket, path: path.clone(), result: Ok(vec![0x89, b'P', b'N', b'G']) });
        assert!(matches!(w.state().editor.as_ref().unwrap().body(), Body::Image(bytes) if bytes.len() == 4));
        open(&mut w.s, w.tab, "intro.mp4");
        assert!(w.wants().is_empty());
        assert_eq!(w.state().editor.as_ref().unwrap().body(), &Body::NoPreview);
        // Leaving the folder closes the file.
        up(&mut w.s, w.tab);
        assert!(w.state().editor.is_none());
    }

    #[test]
    fn a_new_folder_and_a_rename_check_the_name_then_list_again() {
        let mut w = Window::at_home();
        assert!(!new_folder(&mut w.s, w.tab, "shop"));
        assert!(!new_folder(&mut w.s, w.tab, "a/b"));
        assert_eq!(w.titles(), ["“shop” already exists here", "That can't be a name"]);
        assert!(w.wants().is_empty());
        assert!(new_folder(&mut w.s, w.tab, " logs "));
        assert_eq!(w.wants(), [Want::MakeDir { path: "/home/deploy/logs".into() }]);
        w.answer(Answer::MadeDir { path: "/home/deploy/logs".into(), result: Ok(()) });
        assert!(matches!(w.wants()[..], [Want::List { .. }]));
        assert_eq!(w.state().browser.selected(), ["logs"]);

        assert!(rename(&mut w.s, w.tab, "notes.md", "notes.md") && rename(&mut w.s, w.tab, "notes.md", ""));
        assert!(!rename(&mut w.s, w.tab, "notes.md", "shop"));
        assert!(rename(&mut w.s, w.tab, "notes.md", "todo.md"));
        assert_eq!(w.wants(), [Want::Rename { from: "/home/deploy/notes.md".into(), to: "/home/deploy/todo.md".into() }]);
        w.answer(Answer::Renamed { from: "/home/deploy/notes.md".into(), to: "/home/deploy/todo.md".into(), result: Err(Failure::new(FailureKind::Exists, "That name is taken.")) });
        assert_eq!(w.titles().last(), Some(&"Couldn't rename"));
        assert!(w.wants().is_empty());
    }

    #[test]
    fn deleting_moving_and_unpacking_say_they_run_and_then_how_they_ended() {
        let mut w = Window::at_home();
        delete(&mut w.s, w.tab, "/home/deploy", &["notes.md".into(), "shop".into()]);
        assert_eq!(w.state().busy.as_deref(), Some("Deleting 2 items…"));
        assert_eq!(w.wants(), [Want::Delete { dir: "/home/deploy".into(), names: vec!["notes.md".into(), "shop".into()] }]);
        w.answer(Answer::Deleted { dir: "/home/deploy".into(), deleted: vec!["notes.md".into()], failed: vec![("shop".into(), Failure::new(FailureKind::Failed, "Permission denied"))] });
        assert_eq!((w.state().busy.as_deref(), w.titles()), (None, vec!["Couldn't delete shop"]));
        assert!(matches!(w.wants()[..], [Want::List { .. }]));

        move_into(&mut w.s, w.tab, &["site.zip".into()], "/home/deploy/shop");
        assert_eq!(w.state().busy.as_deref(), Some("Moving site.zip…"));
        assert_eq!(w.wants(), [Want::Move { from: "/home/deploy".into(), names: vec!["site.zip".into()], to: "/home/deploy/shop".into() }]);
        w.answer(Answer::Moved { from: "/home/deploy".into(), to: "/home/deploy/shop".into(), moved: vec!["site.zip".into()], taken: vec![], failure: None });
        let (moved, toast) = w.s.screens.notes.toasts().last().map(|(id, t)| (id, t.clone())).unwrap();
        assert_eq!((toast.tone, toast.title.as_str(), toast.action.as_ref().map(|(l, _)| l.as_str())), (crate::kit::toast::Tone::Success, "Moved site.zip to shop", Some("Undo")));
        w.wants();
        w.s.act_on_toast(moved);
        assert_eq!(w.wants(), [Want::Move { from: "/home/deploy/shop".into(), names: vec!["site.zip".into()], to: "/home/deploy".into() }]);

        unpack(&mut w.s, w.tab, "site.zip");
        assert_eq!(w.state().busy.as_deref(), Some("Unpacking site.zip…"));
        assert_eq!(w.wants(), [Want::Extract { dir: "/home/deploy".into(), name: "site.zip".into() }]);
        w.answer(Answer::Extracted { dir: "/home/deploy".into(), name: "site.zip".into(), result: Ok("site".into()) });
        assert_eq!(w.titles().last(), Some(&"Unpacked site.zip"));
        assert_eq!(w.state().browser.selected(), ["site"]);
        let first = w.s.screens.notes.toasts().next().unwrap().0;
        w.s.dismiss_toast(first);
        assert_eq!(w.titles().len(), 1);
    }

    #[test]
    fn a_download_has_a_card_until_it_ends_and_stop_reaches_the_backend_once() {
        let mut w = Window::at_home();
        download(&mut w.s, w.tab, &["notes.md".into(), "shop".into(), "nothing".into()]);
        let wants = w.wants();
        let [Want::Download { id, ref items }] = wants[..] else { panic!("{wants:?}") };
        assert_eq!(items, &[Item { path: "/home/deploy/notes.md".into(), is_dir: false }, Item { path: "/home/deploy/shop".into(), is_dir: true }]);
        assert_eq!(w.s.screens.transfers.items()[0].label, "2 items");
        w.answer(Answer::Progress { id, progress: Progress { done: 512, total: 1_024, ..Progress::default() } });
        assert_eq!(w.s.screens.transfers.get(id).unwrap().card().line, "512 B of 1.0 KB · 50%");
        stop_transfer(&mut w.s, id);
        stop_transfer(&mut w.s, id);
        assert_eq!(w.wants(), [Want::CancelTransfer { id }]);
        w.answer(Answer::Downloaded { id, result: Err(Failure::new(FailureKind::Cancelled, "Cancelled.")) });
        assert!(w.s.screens.transfers.items().is_empty());
        assert_eq!(w.titles(), ["Download stopped"]);
    }

    #[test]
    fn an_upload_that_would_replace_asks_first() {
        let mut w = Window::at_home();
        upload(&mut w.s, w.tab, vec![PathBuf::from("/home/dev/notes.md")]);
        let wants = w.wants();
        let [Want::Upload { id, ref paths, ref dir, overwrite: false }] = wants[..] else { panic!("{wants:?}") };
        assert_eq!((w.s.screens.transfers.items()[0].label.as_str(), w.s.screens.transfers.items()[0].kind), ("notes.md", Kind::Upload));
        let taken = Failure::new(FailureKind::Exists, "notes.md is already there.");
        w.answer(Answer::Uploaded { id, paths: paths.clone(), dir: dir.clone(), overwrite: false, result: Err(taken) });
        assert!(w.s.screens.transfers.items().is_empty() && w.titles().is_empty());
        let question = &w.state().replace.as_ref().unwrap().question;
        assert_eq!((question.title.as_str(), question.body.as_str()), ("Replace existing files?", "notes.md is already there. If you cancel, nothing is uploaded."));

        answer_replace(&mut w.s, w.tab, true);
        assert!(w.state().replace.is_none());
        let wants = w.wants();
        let [Want::Upload { id, ref paths, ref dir, overwrite: true }] = wants[..] else { panic!("{wants:?}") };
        w.answer(Answer::Uploaded { id, paths: paths.clone(), dir: dir.clone(), overwrite: true, result: Ok(()) });
        assert_eq!(w.titles(), ["Uploaded notes.md"]);
        assert!(matches!(w.wants()[..], [Want::List { .. }]), "the folder is listed again");
        // No leaves it all.
        upload(&mut w.s, w.tab, vec![PathBuf::from("/home/dev/notes.md")]);
        let wants = w.wants();
        let [Want::Upload { id, ref paths, ref dir, .. }] = wants[..] else { panic!("{wants:?}") };
        w.answer(Answer::Uploaded { id, paths: paths.clone(), dir: dir.clone(), overwrite: false, result: Err(Failure::new(FailureKind::Exists, "x")) });
        answer_replace(&mut w.s, w.tab, false);
        assert!(w.wants().is_empty() && w.state().replace.is_none());
    }

    #[test]
    fn menus_and_bookmarks() {
        let mut w = Window::at_home();
        let labels = |rows: Vec<MenuRow>| -> Vec<String> {
            rows.into_iter().map(|r| match r { MenuRow::Item(i) => i.label, MenuRow::Separator => "---".into() }).collect()
        };
        w.s.screens.files.get_mut(&w.tab).unwrap().browser.context_click("shop");
        assert_eq!(
            labels(menu_for(&w.s, w.tab, Some("shop"))),
            [
                "Open",
                "Download",
                "Copy to another folder or server…",
                "Rename",
                "Archive “shop”",
                "Copy path",
                "Add to bookmarks",
                "Open in main terminal",
                "Open new terminal here",
                "---",
                "Delete"
            ]
        );
        toggle_bookmark(&mut w.s, w.tab, "/home/deploy/shop");
        assert_eq!(labels(menu_for(&w.s, w.tab, Some("shop")))[6], "Remove bookmark");
        assert_eq!(labels(menu_for(&w.s, w.tab, Some("site.zip")))[6], "Unpack “site.zip”");
        assert_eq!(labels(menu_for(&w.s, w.tab, None))[..4], ["New file", "New folder", "Upload files…", "Paste files"]);
        assert!(menu_for(&w.s, TabId(99), None).is_empty());
    }

    #[test]
    fn undo_goes_to_the_tab_and_machine_of_the_move_whatever_is_on_screen() {
        let mut w = Window::at_home();
        move_into(&mut w.s, w.tab, &["site.zip".into()], "/home/deploy/shop");
        w.answer(Answer::Moved { from: "/home/deploy".into(), to: "/home/deploy/shop".into(), moved: vec!["site.zip".into()], taken: vec![], failure: None });
        w.asked.lock().unwrap().clear();
        // Another tab is on screen now, on this computer.
        let other = w.s.bench.open(TabKind::Terminal, "local");
        w.s.bench.focus(other);
        let moved = w.s.screens.notes.toasts().last().map(|(id, _)| id).unwrap();
        w.s.act_on_toast(moved);
        let asked = std::mem::take(&mut *w.asked.lock().unwrap());
        assert_eq!(asked, [Ask { tab: w.tab, server: WEB.into(), want: Want::Move { from: "/home/deploy/shop".into(), names: vec!["site.zip".into()], to: "/home/deploy".into() } }]);
    }

    #[test]
    fn a_new_file_an_archive_and_a_copy_to_another_folder_of_the_same_server() {
        let mut w = Window::at_home();
        assert!(!new_file(&mut w.s, w.tab, "notes.md"), "a file that's there is never emptied");
        assert!(new_file(&mut w.s, w.tab, "todo.txt"));
        assert_eq!(w.wants(), [Want::MakeFile { path: "/home/deploy/todo.txt".into() }]);
        w.answer(Answer::MadeFile { path: "/home/deploy/todo.txt".into(), result: Ok(()) });
        assert_eq!(w.state().browser.selected(), ["todo.txt"]);
        w.wants();

        assert!(archive(&mut w.s, w.tab, &["shop".into(), "notes.md".into()], "backup"));
        assert_eq!(w.state().busy.as_deref(), Some("Packing backup.tar.gz…"));
        assert_eq!(w.wants(), [Want::Archive { dir: "/home/deploy".into(), names: vec!["shop".into(), "notes.md".into()], archive: "backup.tar.gz".into() }]);
        w.answer(Answer::Archived { dir: "/home/deploy".into(), archive: "backup.tar.gz".into(), count: 2, result: Ok(()) });
        assert_eq!(w.titles().last(), Some(&"Made backup.tar.gz"));
        w.wants();

        // The dialog opens on this server, in this folder; the copy goes into a folder in it.
        open_copy(&mut w.s, w.tab, &["notes.md".into()]);
        let asked = std::mem::take(&mut *w.asked.lock().unwrap());
        assert_eq!(asked.last().map(|a| (a.server.as_str(), a.want.clone())), Some((WEB, Want::Peek { path: "/home/deploy".into() })));
        w.answer(Answer::Peeked { asked: "/home/deploy".into(), result: Ok((home(), Clock::default())) });
        assert_eq!(w.state().copy.as_ref().unwrap().place, Some(("/home/deploy".into(), vec!["shop".into()])));
        copy_go(&mut w.s, w.tab, "/home/deploy/shop");
        w.answer(Answer::Peeked { asked: "/home/deploy/shop".into(), result: Ok((Listing { path: "/home/deploy/shop".into(), entries: vec![] }, Clock::default())) });
        w.wants();
        copy_here(&mut w.s, w.tab);
        let wants = w.wants();
        let [Want::CopyTo { id, ref to, ref paths, ref dir, overwrite: false }] = wants[..] else { panic!("{wants:?}") };
        assert_eq!((to.as_str(), paths.as_slice(), dir.as_str()), (WEB, &["/home/deploy/notes.md".to_owned()][..], "/home/deploy/shop"));
        assert!(w.state().copy.is_none());
        // Taken there: asks, and yes sends it again to replace.
        w.answer(Answer::CopiedTo { id, to: WEB.into(), paths: paths.clone(), dir: dir.clone(), overwrite: false, result: Err(Failure::new(FailureKind::Exists, "“notes.md” already there.")) });
        assert!(w.state().replace.is_some());
        answer_replace(&mut w.s, w.tab, true);
        assert!(matches!(w.wants()[..], [Want::CopyTo { overwrite: true, .. }]));
    }

    #[test]
    fn a_copy_refuses_its_own_folder_and_each_server_opens_where_it_copied_last() {
        let mut w = Window::at_home();
        open_copy(&mut w.s, w.tab, &["shop".into()]);
        w.answer(Answer::Peeked { asked: "/home/deploy".into(), result: Ok((home(), Clock::default())) });
        // The folder they're in, and the folder itself: no.
        assert_eq!(copy_blocked(&w.s, w.tab).as_deref(), Some("They're already in this folder. Pick another one."));
        copy_go(&mut w.s, w.tab, "/home/deploy/shop");
        w.answer(Answer::Peeked { asked: "/home/deploy/shop".into(), result: Ok((Listing { path: "/home/deploy/shop".into(), entries: vec![] }, Clock::default())) });
        assert_eq!(copy_blocked(&w.s, w.tab).as_deref(), Some("Can't copy shop into itself. Pick a folder outside it."));
        w.wants();
        copy_here(&mut w.s, w.tab);
        assert_eq!(w.wants(), [], "nothing copies into itself");
        // The path field follows the folder shown.
        assert_eq!(w.state().ui.copy_typed.text, "/home/deploy/shop");
        // Somewhere else on the server: it copies, and that folder is where the server opens next.
        copy_go(&mut w.s, w.tab, "/srv");
        w.answer(Answer::Peeked { asked: "/srv".into(), result: Ok((Listing { path: "/srv".into(), entries: vec![] }, Clock::default())) });
        assert_eq!(copy_blocked(&w.s, w.tab), None);
        copy_here(&mut w.s, w.tab);
        assert!(matches!(&w.wants()[..], [.., Want::CopyTo { dir, .. }] if dir == "/srv"));
        assert_eq!(w.s.screens.copy_last.dirs.get(WEB).map(String::as_str), Some("/srv"));
        open_copy(&mut w.s, w.tab, &["notes.md".into()]);
        assert!(matches!(&w.wants()[..], [.., Want::Peek { path }] if path == "/srv"));
    }

    #[test]
    fn a_file_of_a_folder_looked_into_opens_beside_and_the_tab_stays() {
        let mut w = Window::at_home();
        open_from(&mut w.s, w.tab, "/home/deploy/shop", "package.json");
        assert_eq!(w.state().browser.dir(), "/home/deploy");
        assert_eq!(w.state().editor.as_ref().map(|e| e.path()).as_deref(), Some("/home/deploy/shop/package.json"));
        assert!(matches!(&w.wants()[..], [Want::Read { path, .. }] if path == "/home/deploy/shop/package.json"));
    }

    #[test]
    fn a_dot_file_from_a_terminal_shows_the_hidden_files() {
        let mut w = Window::at_home();
        assert!(!w.state().browser.show_hidden());
        show_path(&mut w.s, w.tab, "/home/deploy/notes.md");
        assert!(!w.state().browser.show_hidden(), "only for a dot file");
        show_path(&mut w.s, w.tab, "/home/deploy/.env");
        assert!(w.state().browser.show_hidden());
    }

    #[test]
    fn files_copied_on_this_computer_are_pasted_after_a_question_that_warns_of_private_ones() {
        let mut w = Window::at_home();
        let content = rune::ClipboardContent::new("id_ed25519").with_files([PathBuf::from("/home/dev/.ssh/id_ed25519"), PathBuf::from("relative.txt")]);
        assert!(paste(&mut w.s, &content));
        let question = &w.state().paste.as_ref().unwrap().question;
        assert!(question.risky && question.confirm.lines == ["/home/dev/.ssh/id_ed25519"]);
        assert!(w.wants().is_empty(), "nothing goes before the yes");
        answer_paste(&mut w.s, w.tab, true);
        assert!(matches!(&w.wants()[..], [Want::Upload { paths, .. }] if paths == &[PathBuf::from("/home/dev/.ssh/id_ed25519")]));
        // Text a server could have written: no files.
        assert!(paste(&mut w.s, &rune::ClipboardContent::new("file:///etc/passwd").with_files([PathBuf::from("/etc/passwd")])));
        assert!(w.state().paste.is_none());
        assert_eq!(w.titles().last(), Some(&"No files to paste"));
    }

    #[test]
    fn a_terminal_in_a_folder_and_one_whose_name_would_type_keys() {
        let mut w = Window::at_home();
        let mut tx = rune::Tx::default();
        terminal_here(&mut w.s, w.tab, "/home/deploy/it's", &mut tx);
        let pending: Vec<String> = w.s.screens.palette.pending.values().map(|(c, _)| c.clone()).collect();
        assert_eq!(pending, ["cd -- '/home/deploy/it'\\''s' && clear"]);
        terminal_here(&mut w.s, w.tab, "/home/deploy/a\nrm -rf ~", &mut tx);
        assert_eq!(w.titles().last(), Some(&"Can't open a terminal there"));
    }

    #[test]
    fn bookmarks_are_kept_by_the_backend_and_0_10s_come_in_after() {
        let mut w = Window::at_home();
        toggle_bookmark(&mut w.s, w.tab, "/srv/www");
        let mut old = sshee_app::model::Bookmarks::default();
        bookmarks::toggle_pin(&mut old, WEB, "/etc");
        seed_bookmarks(&mut w.s, &old);
        assert_eq!(bookmarks::pins(&w.s.screens.bookmarks, WEB), ["/srv/www", "/etc"]);
        remove_bookmark(&mut w.s, WEB, "/srv/www");
        forget_recent(&mut w.s, WEB, "/home/deploy");
        assert_eq!(bookmarks::pins(&w.s.screens.bookmarks, WEB), ["/etc"]);
        assert!(bookmarks::recent(&w.s.screens.bookmarks, WEB).is_empty());
    }

    #[test]
    fn a_path_from_a_terminal_opens_in_its_folder_and_a_folder_opens_itself() {
        let mut w = Window::at_home();
        show_path(&mut w.s, w.tab, "/etc/shop/worker.toml");
        let listing = Listing { path: "/etc/shop".into(), entries: vec![entry("worker.toml", EntryKind::File), entry("old", EntryKind::Dir)] };
        w.list(listing.clone());
        // Listed: picked out and opened beside the list.
        assert_eq!(w.state().browser.dir(), "/etc/shop");
        assert_eq!(w.state().browser.selected(), ["worker.toml"]);
        let wants = w.wants();
        assert!(matches!(&wants[..], [Want::Read { path, .. }] if path == "/etc/shop/worker.toml"), "{wants:?}");
        // A folder's path: the folder it is in is listed again (it is the one on screen), and
        // then it goes into it.
        show_path(&mut w.s, w.tab, "/etc/shop/old");
        w.list(listing);
        let wants = w.wants();
        assert!(matches!(&wants[..], [Want::List { path, .. }] if path == "/etc/shop/old"), "{wants:?}");
        assert!(w.state().editor.is_none(), "a folder opened as a file");
    }

    /// notes.md open beside the list, with "mine" typed and not saved.
    fn with_unsaved_changes() -> Window {
        let mut w = Window::at_home();
        open(&mut w.s, w.tab, "notes.md");
        let wants = w.wants();
        let [Want::Read { ticket, ref path }] = wants[..] else { panic!("{wants:?}") };
        let file = TextFile { path: path.clone(), content: Some("one\n".into()), binary: false, size: 4, lossy: false, crlf: false };
        w.answer(Answer::Read { ticket, path: path.clone(), result: Ok((file, stamp(1))) });
        view::sync(&mut w.s);
        w.s.screens.files.get_mut(&w.tab).unwrap().ui.type_draft("mine\n");
        w
    }

    #[test]
    fn moving_the_tab_from_outside_asks_before_unsaved_changes_are_lost() {
        let mut w = with_unsaved_changes();
        let mut tx = rune::Tx::default();
        // The palette's folder row, a file dropped on a terminal, a clicked path: all of them
        // reach `go` through `open_files_on`.
        assert_eq!(w.s.open_files_on(WEB, Some("/srv/www"), &mut tx), w.tab);
        assert!(w.state().editor.is_some(), "the draft was dropped");
        assert!(w.wants().is_empty(), "the tab moved before asking");
        assert_eq!(w.state().ui.pending(), Some(&view::Pending::Discard(view::After::Go("/srv/www".into()))));
        // The mouse's side buttons and the folder above ask too.
        for (step, after) in [(back as fn(&mut Shell, TabId), view::After::Back), (forward, view::After::Forward), (up, view::After::Up)] {
            step(&mut w.s, w.tab);
            assert!(w.state().editor.is_some() && w.wants().is_empty());
            assert_eq!(w.state().ui.pending(), Some(&view::Pending::Discard(after)));
        }
        // No: the file and what was typed stay, and so does the folder.
        view::answer_confirm(&mut w.s, w.tab, false);
        assert_eq!(w.state().ui.draft_text(), "mine\n");
        assert_eq!(w.state().browser.dir(), "/home/deploy");
        // Yes: then it goes.
        go(&mut w.s, w.tab, "/srv/www");
        view::answer_confirm(&mut w.s, w.tab, true);
        assert!(w.state().editor.is_none());
        assert!(matches!(&w.wants()[..], [Want::List { path, .. }] if path == "/srv/www"));
        // Nothing typed: no question.
        go(&mut w.s, w.tab, "/etc");
        assert!(w.state().ui.pending().is_none());
        assert!(matches!(&w.wants()[..], [Want::List { path, .. }] if path == "/etc"));
    }

    #[test]
    fn a_delete_question_deletes_where_it_was_asked_and_closes_when_the_tab_moves() {
        let mut w = Window::at_home();
        // Asked while another folder is being listed: the rows on screen are still home's.
        go(&mut w.s, w.tab, "/srv/www");
        let wants = w.wants();
        let [Want::List { ticket, .. }] = wants[..] else { panic!("{wants:?}") };
        view::ask_delete(&mut w.s, w.tab, vec!["notes.md".into()]);
        assert!(w.state().ui.pending().is_some());
        w.answer(Answer::Listed {
            ticket,
            asked: "/srv/www".into(),
            result: Ok((Listing { path: "/srv/www".into(), entries: vec![entry("notes.md", EntryKind::File)] }, Clock { now: 1_700_000_500, utc_offset: 0 })),
        });
        assert_eq!(w.state().browser.dir(), "/srv/www");
        view::answer_confirm(&mut w.s, w.tab, true);
        let wants = w.wants();
        assert!(matches!(&wants[..], [Want::Delete { dir, names }] if dir == "/home/deploy" && names == &["notes.md"]), "{wants:?}");
        // A question about one folder's rows closes when the tab goes to another.
        view::ask_delete(&mut w.s, w.tab, vec!["notes.md".into()]);
        assert!(w.state().ui.pending().is_some());
        up(&mut w.s, w.tab);
        assert!(w.state().ui.pending().is_none(), "the question stayed in another folder");
    }

    /// What the edit bridge was asked since the last look.
    fn edit_asks(w: &Window) -> Vec<sshee_app::files::edit::EditAsk> {
        std::mem::take(&mut *w.edits.lock().unwrap())
    }

    #[test]
    fn open_in_an_editor_elsewhere_from_start_to_the_servers_last_tab() {
        use sshee_app::editors::Editor;
        use sshee_app::files::edit::{EditAsk, EditEvent, EditReply, Opened};
        let mut w = Window::at_home();
        // At start: the editors and the leftovers are asked for, nothing custom set.
        assert_eq!(edit_asks(&w), [EditAsk::Start { custom: None }]);
        let code = Editor { id: "vscode".into(), name: "VS Code".into(), program: "/usr/share/code/code".into(), args: vec![], vscode_like: true };
        edit::deliver(&mut w.s, EditReply::Editors { found: vec![code.clone()], custom: None });
        // The menu names it, for a file only.
        let menu: Vec<String> = menu_for(&w.s, w.tab, Some("notes.md")).into_iter().filter_map(|r| match r {
            MenuRow::Item(i) => Some(i.label),
            MenuRow::Separator => None,
        }).collect();
        assert_eq!(menu[..3], ["Open preview", "Open in VS Code", "Open in…"]);
        // Open: no Tor, no streamer mode, so no question.
        edit::open_in(&mut w.s, w.tab, "notes.md", None);
        let asked = edit_asks(&w);
        assert!(matches!(&asked[..], [EditAsk::Open { path, editor, confirmed: false, .. }] if path == "/home/deploy/notes.md" && editor.name == "VS Code"), "{asked:?}");
        assert!(w.s.screens.edits.is_opening(WEB, "/home/deploy/notes.md"));
        edit::deliver(&mut w.s, EditReply::Opened { tab: w.tab, host: WEB.into(), path: "/home/deploy/notes.md".into(), editor: "VS Code".into(), result: Ok(Opened::Session { id: "s1".into(), local: "/tmp/e/s1/notes.md".into() }) });
        // A save that reached the server lists the folder again: its size and time changed.
        w.wants();
        edit::deliver(&mut w.s, EditReply::Event { id: "s1".into(), event: EditEvent::Uploaded { at: 1_700_000_400 } });
        assert!(w.wants().iter().any(|want| matches!(want, Want::List { path, .. } if path == "/home/deploy")));
        assert_eq!(edit::session_of(&w.s, w.tab, "notes.md").as_deref(), Some("s1"));
        // A conflict brings its dialog up by itself.
        edit::deliver(&mut w.s, EditReply::Event { id: "s1".into(), event: EditEvent::Conflict { gone: false } });
        assert_eq!(w.s.screens.edits.conflict.as_deref(), Some("s1"));
        // The server's last tab closes (`Shell::settle` runs this): its sessions end.
        w.s.bench.close(w.tab);
        edit::hosts_changed(&mut w.s);
        assert!(edit_asks(&w).contains(&EditAsk::CloseHost { host: WEB.into() }));
    }

    #[test]
    fn choose_an_app_keeps_it_as_the_setting_and_opens_the_file_it_was_asked_for() {
        use sshee_app::editors::Editor;
        use sshee_app::files::edit::{EditAsk, EditReply};
        let mut w = Window::at_home();
        edit_asks(&w);
        w.s.screens.edits.pending = Some((w.tab, WEB.into(), "/home/deploy/notes.md".into()));
        let kate = Editor { id: "custom:/opt/kate/bin/kate".into(), name: "kate".into(), program: "/opt/kate/bin/kate".into(), args: vec![], vscode_like: false };
        edit::deliver(&mut w.s, EditReply::Chosen(Ok(kate)));
        assert_eq!(w.s.settings.more().external_editor.as_deref(), Some("custom:/opt/kate/bin/kate"));
        assert_eq!(edit::chosen(&w.s).unwrap().name, "kate");
        assert!(matches!(&edit_asks(&w)[..], [EditAsk::Open { editor, .. }] if editor.name == "kate"));
        // A pick that can't be started says why, and changes nothing.
        edit::deliver(&mut w.s, EditReply::Chosen(Err("That isn't a program that can be started.".into())));
        assert_eq!(w.titles().last(), Some(&"Can't open files in that app"));
        // Leftovers at start are marked offered as they're shown.
        let left = sshee_app::files::edit::Leftover { id: "l1".into(), host: WEB.into(), remote_path: "/home/deploy/a".into(), local: "/tmp/e/l1/a".into(), size: 3 };
        edit::deliver(&mut w.s, EditReply::Leftovers(vec![left]));
        assert_eq!(edit_asks(&w), [EditAsk::MarkOffered { ids: vec!["l1".into()] }]);
        assert_eq!(w.s.screens.edits.leftovers.len(), 1);
    }
}
