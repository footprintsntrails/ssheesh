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

use crate::shell::Shell;
use crate::theme::Theme;

/// The most notes kept for the screen to show; older ones go first.
const NOTICES_KEPT: usize = 5;

/// An upload found names that are taken and sent nothing: the question to ask, and what to
/// send again on yes ([`answer_replace`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Replace {
    pub question: Confirm,
    paths: Vec<PathBuf>,
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
    /// What only the drawing keeps: the text being typed, the menu or question on screen.
    pub(crate) ui: view::Ui,
}

impl State {
    fn new(server: &str, path: &str) -> State {
        State { browser: Browser::new(server, path), editor: None, tickets: Tickets::default(), busy: None, replace: None, ui: view::Ui::default() }
    }
}

pub fn view(s: &Shell, tab: &Tab, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    view::page(s, tab, t, cx)
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

fn note(s: &mut Shell, notice: Notice) {
    let notices = &mut s.screens.notices;
    notices.push(notice);
    if notices.len() > NOTICES_KEPT {
        notices.remove(0);
    }
}

/// The folder on screen is where the tab is (kept for the next run) and the newest of the
/// machine's recent folders.
fn keep_place(s: &mut Shell, tab: TabId) {
    let Some(state) = s.screens.files.get(&tab).filter(|st| st.browser.listing().is_some()) else { return };
    let (server, dir) = (state.browser.server().to_owned(), state.browser.dir().to_owned());
    bookmarks::visit(&mut s.screens.bookmarks, &server, &dir);
    s.bench.set_path(tab, Some(dir));
}

/// Runs one of the browser's moves, and asks the backend for the listing it wants.
fn load(s: &mut Shell, tab: TabId, step: impl FnOnce(&mut Browser, &DirCache, u64) -> Option<Load>) {
    let now = now_ms(s);
    let screens = &mut s.screens;
    let Some(state) = screens.files.get_mut(&tab) else { return };
    let load = step(&mut state.browser, &screens.dirs, now);
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
    let Some(tab) = s.bench.active().filter(|t| t.kind == TabKind::Files && !s.screens.files.contains_key(&t.id)).cloned() else { return };
    s.screens.files.insert(tab.id, State::new(&tab.server, tab.path.as_deref().unwrap_or("~")));
    load(s, tab.id, |b, cache, now| b.start(cache, now));
}

// ---- going places

/// Shows another folder (a double click on one, the address bar, a bookmark). The open file
/// closes: ask first when its draft is dirty.
pub fn go(s: &mut Shell, tab: TabId, to: &str) {
    if let Some(state) = s.screens.files.get_mut(&tab).filter(|st| st.browser.dir() != to) {
        state.editor = None;
    }
    load(s, tab, |b, cache, now| b.go(to, cache, now));
}

pub fn back(s: &mut Shell, tab: TabId) {
    close_file(s, tab);
    load(s, tab, |b, cache, now| b.back(cache, now));
}

pub fn forward(s: &mut Shell, tab: TabId) {
    close_file(s, tab);
    load(s, tab, |b, cache, now| b.forward(cache, now));
}

/// The folder above (the ".." row, Backspace).
pub fn up(s: &mut Shell, tab: TabId) {
    close_file(s, tab);
    load(s, tab, |b, cache, now| b.up(cache, now));
}

/// Lists the folder again (the button, "Try again").
pub fn refresh(s: &mut Shell, tab: TabId) {
    load(s, tab, |b, cache, now| b.refresh(cache, now));
}

/// A double click or Enter on a row: into a folder, or the file opens beside the list.
pub fn open(s: &mut Shell, tab: TabId, name: &str) {
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    let Some(entry) = state.browser.entry(name) else { return };
    if entry.is_dir() {
        let to = state.browser.path_of(name);
        return go(s, tab, &to);
    }
    let dir = state.browser.dir().to_owned();
    let (editor, want) = Editor::open(&dir, name, &mut state.tickets);
    state.editor = Some(editor);
    state.browser.select_only(name);
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
    let Some(state) = s.screens.files.get_mut(&tab) else { return };
    if names.is_empty() {
        return;
    }
    let shown: Vec<&str> = names.iter().map(String::as_str).collect();
    state.busy = Some(ops::busy(verb, &shown));
    let dir = state.browser.dir().to_owned();
    ask(s, tab, want(dir, names.to_vec()));
}

/// Deletes rows of the folder on screen. Ask first: `ops::delete_question`.
pub fn delete(s: &mut Shell, tab: TabId, names: &[String]) {
    run(s, tab, "Deleting", names, |dir, names| Want::Delete { dir, names });
}

/// Unpacks an archive next to itself.
pub fn unpack(s: &mut Shell, tab: TabId, name: &str) {
    run(s, tab, "Unpacking", &[name.to_owned()], |dir, mut names| Want::Extract { dir, name: names.remove(0) });
}

/// Rows dropped on a folder (`ops::drop_folder` says which, and whether they may).
pub fn move_into(s: &mut Shell, tab: TabId, names: &[String], to: &str) {
    run(s, tab, "Moving", names, |from, names| Want::Move { from, names, to: to.to_owned() });
}

/// "Undo" on the note after a move.
pub fn undo_move(s: &mut Shell, tab: TabId, undo: &Undo) {
    ask(s, tab, Want::Move { from: undo.from.clone(), names: undo.names.clone(), to: undo.to.clone() });
}

// ---- transfers

/// Downloads rows of the folder on screen into this computer's Downloads folder, under one
/// card. Nothing on this computer itself.
pub fn download(s: &mut Shell, tab: TabId, names: &[String]) {
    let Some(state) = s.screens.files.get(&tab).filter(|st| st.browser.server() != "local") else { return };
    let items: Vec<Item> = names.iter().filter_map(|n| state.browser.entry(n)).map(|e| Item { path: state.browser.path_of(&e.name), is_dir: e.is_dir() }).collect();
    if items.is_empty() {
        return;
    }
    let paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
    let id = s.screens.transfers.begin(Kind::Download, files_label(&paths), tab, state.browser.server());
    ask(s, tab, Want::Download { id, items });
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
    let Some(Replace { paths, dir, .. }) = s.screens.files.get_mut(&tab).and_then(|st| st.replace.take()) else { return };
    if yes {
        send(s, tab, paths, dir, true);
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
}

/// What a right click offers: on the row `clicked`, or on the empty space (`None`). Call
/// `Browser::context_click` first, so the selection is what the menu is about.
pub fn menu_for(s: &Shell, tab: TabId, clicked: Option<&str>) -> Vec<MenuRow> {
    let Some(state) = s.screens.files.get(&tab) else { return Vec::new() };
    let browser = &state.browser;
    let local = browser.server() == "local";
    menu::menu(&menu::Context {
        dir: browser.dir(),
        clicked: clicked.and_then(|name| browser.entry(name)),
        selected: &browser.selected_entries(),
        on_server: !local,
        on_this_windows: local && cfg!(windows),
        pins: bookmarks::pins(&s.screens.bookmarks, browser.server()),
        show_hidden: browser.show_hidden(),
    })
}

/// Takes a note off the screen (its cross, its time ran out, its Undo was pressed).
pub fn dismiss_notice(s: &mut Shell, index: usize) {
    if index < s.screens.notices.len() {
        s.screens.notices.remove(index);
    }
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
                    keep_place(s, tab);
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
            for notice in ops::move_notices(&from, &to, &moved, &taken, failure.as_ref()) {
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
                    state.replace = Some(Replace { question: ops::replace_question(&failure.message, card.kind), paths, dir });
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
    use sshee_app::files::{Clock, Entry, EntryKind, Failure, Listing, Stamp, TextFile, Tone};
    use sshee_app::transfers::Progress;
    use sshee_app::workbench::{Server, Workbench};

    use super::*;
    use crate::shell::{Backend, Options};
    use crate::terminal::{NoLink, TermLink};

    const WEB: &str = "host:web";

    /// A backend that only writes down what it was asked.
    struct Asked(Arc<Mutex<Vec<Ask>>>);

    impl Backend for Asked {
        fn terminal(&mut self, _: Option<&Server>, _: &Tab) -> Box<dyn TermLink> {
            Box::new(NoLink)
        }
        fn files(&mut self, ask: Ask) {
            self.0.lock().unwrap().push(ask);
        }
        fn replays(&self) -> bool {
            true
        }
    }

    struct Window {
        s: Shell,
        asked: Arc<Mutex<Vec<Ask>>>,
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
            let asked = Arc::new(Mutex::new(Vec::new()));
            let s = Shell::new(bench, Box::new(Asked(asked.clone())), Options::default(), None);
            Window { s, asked, tab }
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
            self.s.screens.notices.iter().map(|n| n.title.as_str()).collect()
        }
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
    fn a_tab_from_the_last_run_opens_where_it_was() {
        let mut bench = Workbench::new(vec![Server::host("web", "web-7731", "deploy@203.0.113.17")]);
        let tab = bench.open_files(WEB);
        bench.set_path(tab, Some("/srv/www".into()));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let _s = Shell::new(bench, Box::new(Asked(asked.clone())), Options::default(), None);
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
        delete(&mut w.s, w.tab, &["notes.md".into(), "shop".into()]);
        assert_eq!(w.state().busy.as_deref(), Some("Deleting 2 items…"));
        assert_eq!(w.wants(), [Want::Delete { dir: "/home/deploy".into(), names: vec!["notes.md".into(), "shop".into()] }]);
        w.answer(Answer::Deleted { dir: "/home/deploy".into(), deleted: vec!["notes.md".into()], failed: vec![("shop".into(), Failure::new(FailureKind::Failed, "Permission denied"))] });
        assert_eq!((w.state().busy.as_deref(), w.titles()), (None, vec!["Couldn't delete shop"]));
        assert!(matches!(w.wants()[..], [Want::List { .. }]));

        move_into(&mut w.s, w.tab, &["site.zip".into()], "/home/deploy/shop");
        assert_eq!(w.state().busy.as_deref(), Some("Moving site.zip…"));
        assert_eq!(w.wants(), [Want::Move { from: "/home/deploy".into(), names: vec!["site.zip".into()], to: "/home/deploy/shop".into() }]);
        w.answer(Answer::Moved { from: "/home/deploy".into(), to: "/home/deploy/shop".into(), moved: vec!["site.zip".into()], taken: vec![], failure: None });
        let moved = w.s.screens.notices.last().unwrap().clone();
        assert_eq!((moved.tone, moved.title.as_str()), (Tone::Success, "Moved site.zip to shop"));
        w.wants();
        undo_move(&mut w.s, w.tab, moved.undo.as_ref().unwrap());
        assert_eq!(w.wants(), [Want::Move { from: "/home/deploy/shop".into(), names: vec!["site.zip".into()], to: "/home/deploy".into() }]);

        unpack(&mut w.s, w.tab, "site.zip");
        assert_eq!(w.state().busy.as_deref(), Some("Unpacking site.zip…"));
        assert_eq!(w.wants(), [Want::Extract { dir: "/home/deploy".into(), name: "site.zip".into() }]);
        w.answer(Answer::Extracted { dir: "/home/deploy".into(), name: "site.zip".into(), result: Ok("site".into()) });
        assert_eq!(w.titles().last(), Some(&"Unpacked site.zip"));
        assert_eq!(w.state().browser.selected(), ["site"]);
        dismiss_notice(&mut w.s, 0);
        assert_eq!(w.titles().len(), 2);
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
        assert_eq!(labels(menu_for(&w.s, w.tab, Some("shop"))), ["Open", "Download", "Rename", "Copy path", "Add to bookmarks", "---", "Delete"]);
        toggle_bookmark(&mut w.s, w.tab, "/home/deploy/shop");
        assert_eq!(labels(menu_for(&w.s, w.tab, Some("shop")))[4], "Remove bookmark");
        assert_eq!(labels(menu_for(&w.s, w.tab, Some("site.zip")))[3], "Unpack “site.zip”");
        assert_eq!(labels(menu_for(&w.s, w.tab, None))[..2], ["New folder", "Upload files…"]);
        assert!(menu_for(&w.s, TabId(99), None).is_empty());
    }
}
