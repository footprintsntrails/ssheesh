//! "Open in your own editor", on screen: the seam between Files and the backend's edit bridge,
//! and what it draws. The rules (which editor, what a status says, the questions) are
//! `sshee_app::files::edit`'s; the work is the bridge's, on the backend's threads
//! (`Backend::edit`), and its news comes back through `Remote::edit` to [`deliver`].
//!
//! What it draws:
//! - after a file's name in the list, a quiet mark while it is open elsewhere ("Saved 12:41");
//! - under the list, a line for each file of the folder that is open elsewhere, with Retry,
//!   "Resolve…" and "Stop editing";
//! - over the window: the question before a big file or the first open with Tor or streamer
//!   mode on, the conflict's dialog (with its comparison), and at start the prompt for copies
//!   an earlier run left with changes that never reached the server.
//!
//! Names and paths go through `Shell::text_shown`, for streamer mode.

use std::path::PathBuf;

use rune::{col, el, row, spacer, text, El, FileDialogOptions, ScrollSpec, Tx};
use sshee_app::editors::Editor;
use sshee_app::files::edit::{self, leftovers_prompt, DiffKind, EditAsk, EditEvent, EditReply, Question, Session};
use sshee_app::files::{Notice, Tone};
use sshee_app::paths;
use sshee_app::workbench::{Link, TabId, Tor};

use super::view::{question, shown};
use crate::icons::{icon, Icon};
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// Your lines in the comparison: a touch heavier than the server's.
const CODE_MINE: rune::tok::TextStyle = rune::tok::TextStyle::new(12.0, 500.0).mono();

/// The longest the comparison gets before it scrolls.
const COMPARE_H: f32 = 320.0;

fn ask(s: &mut Shell, ask: EditAsk) {
    s.edit_ask(ask);
}

/// Once, when the window starts: the editors, the one picked with "Choose an app…", and the
/// copies an earlier run left.
pub fn start(s: &mut Shell) {
    let custom = setting(s).and_then(|v| v.strip_prefix(sshee_app::editors::CUSTOM_PREFIX).map(PathBuf::from));
    s.screens.edits.hosts = hosts_with_tabs(s);
    ask(s, EditAsk::Start { custom });
}

/// The setting `externalEditor`.
pub fn setting(s: &Shell) -> Option<String> {
    s.settings.more().external_editor.clone()
}

/// The editor "Open in" means now.
pub fn chosen(s: &Shell) -> Option<Editor> {
    s.screens.edits.chosen(setting(s).as_deref()).cloned()
}

/// "Open in <editor>" on `name` in tab `tab`'s folder; `editor`: one picked in "Open in…".
pub fn open_in(s: &mut Shell, tab: TabId, name: &str, editor: Option<Editor>) {
    let Some(state) = s.screens.files.get(&tab) else { return };
    let (host, path) = (state.browser.server().to_owned(), state.browser.path_of(name));
    let Some(editor) = editor.or_else(|| chosen(s)) else {
        super::note(s, Notice::new(Tone::Error, "No editor was found on this computer").body("Pick one with “Open in…” → “Choose an app…”."));
        return;
    };
    let tor = matches!(s.bench.tor(), Tor::On { .. } | Tor::Starting);
    let streaming = s.streaming();
    if let Some(want) = s.screens.edits.open(tab, &host, &path, editor, tor, streaming) {
        ask(s, want);
    }
}

/// "Choose an app…": the system's picker; the program picked is checked by the backend, kept
/// as the setting, and then (from a file's menu) opens that file.
pub fn choose_app(tab: Option<(TabId, String)>, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Choose an editor".into(), ..Default::default() };
    tx.open_file(options, move |s: &mut Shell, result, _| match result {
        Ok(Some(path)) => {
            s.screens.edits.pending = tab.clone().and_then(|(tab, name)| s.screens.files.get(&tab).map(|st| (tab, st.browser.server().to_owned(), st.browser.path_of(&name))));
            ask(s, EditAsk::ChooseApp { path });
        }
        Ok(None) => {}
        Err(e) => super::note(s, Notice::new(Tone::Error, "Couldn't open the file picker").body(e.to_string())),
    });
}

/// "Stop editing": the session ends; a copy with unsent changes is kept, and a note says where.
pub fn stop(s: &mut Shell, id: &str) {
    ask(s, EditAsk::Close { id: id.to_owned() });
}

/// The clicked file's open session, for "Stop editing" in its menu.
pub fn session_of(s: &Shell, tab: TabId, name: &str) -> Option<String> {
    let state = s.screens.files.get(&tab)?;
    s.screens.edits.open_on(state.browser.server(), &state.browser.path_of(name)).map(|e| e.id.clone())
}

/// The window's tabs changed: the sessions of a server whose last tab closed end.
pub fn hosts_changed(s: &mut Shell) {
    let now = hosts_with_tabs(s);
    for want in s.screens.edits.hosts_closed(now) {
        ask(s, want);
    }
}

fn hosts_with_tabs(s: &Shell) -> Vec<String> {
    let mut hosts: Vec<String> = s.bench.tabs().iter().map(|t| t.server.clone()).collect();
    hosts.sort();
    hosts.dedup();
    hosts
}

/// A terminal of `host` connected again: its sessions waiting for the connection try at once.
pub fn reconnected(s: &mut Shell, tab: TabId) {
    let Some(host) = s.bench.tab(tab).filter(|t| matches!(t.link, Link::Connected { .. })).map(|t| t.server.clone()) else { return };
    if s.screens.edits.sessions.iter().any(|e| e.host == host && e.is_open()) {
        ask(s, EditAsk::RetryHost { host });
    }
}

/// The window closes: every session ends first (copies with unsent changes stay).
pub fn close_all(s: &mut Shell) {
    if s.screens.edits.sessions.iter().any(Session::is_open) {
        ask(s, EditAsk::CloseAll);
    }
}

/// The backend's answer (`Remote::edit`).
pub fn deliver(s: &mut Shell, reply: EditReply) {
    let chosen = match &reply {
        EditReply::Chosen(Ok(editor)) => Some(editor.clone()),
        _ => None,
    };
    if let EditReply::Leftovers(list) = &reply {
        // Shown now: one that then sits a month is deleted at a later start.
        if !list.is_empty() {
            let ids = list.iter().map(|l| l.id.clone()).collect();
            ask(s, EditAsk::MarkOffered { ids });
        }
    }
    let landed = match &reply {
        EditReply::Event { id, event: EditEvent::Uploaded { .. } } => s.screens.edits.session(id).map(|e| (e.host.clone(), paths::parent(&e.path))),
        EditReply::CopySaved { id, result: Ok(_) } => s.screens.edits.session(id).map(|e| (e.host.clone(), paths::parent(&e.path))),
        _ => None,
    };
    if let Some(notice) = s.screens.edits.apply(reply) {
        super::note(s, notice);
    }
    if let Some((host, dir)) = landed {
        super::changed(s, &host, &dir);
    }
    if let Some(editor) = chosen {
        let id = editor.id.clone();
        s.settings.set_more(|m| m.external_editor = Some(id));
        if let Some((tab, host, path)) = s.screens.edits.pending.take() {
            if s.screens.files.get(&tab).is_some_and(|st| st.browser.server() == host) {
                open_in(s, tab, paths::name(&path), Some(editor));
            }
        }
    }
}

// ---- drawing

/// The quiet mark after a file's name while it is open elsewhere, or being opened.
pub fn row_mark(s: &Shell, host: &str, path: &str, selected: bool, t: &Theme) -> Option<El> {
    let edits = &s.screens.edits;
    let (mark, error, waits) = match edits.open_on(host, path) {
        Some(session) => {
            let st = session.status(utc_offset(s));
            (st.mark, st.error, st.waits)
        }
        None if edits.is_opening(host, path) => ("Opening…".to_owned(), false, false),
        None => return None,
    };
    let ink = match (selected, error, waits) {
        (true, ..) => t.on_color.alpha(0.85),
        (false, true, _) => t.red,
        (false, _, true) => t.orange,
        _ => t.l2,
    };
    Some(text(mark).key("edit-mark").font(theme::SMALL).color(ink).single_line().ml(8.0))
}

fn utc_offset(s: &Shell) -> i32 {
    s.screens.files.values().find_map(|st| st.browser.listing().map(|_| st.browser.clock().utc_offset)).unwrap_or(0)
}

/// Under the list: a line for each file of this folder open elsewhere.
pub fn strip(s: &Shell, tab: TabId, t: &'static Theme) -> Option<El> {
    let state = s.screens.files.get(&tab)?;
    let (host, dir) = (state.browser.server(), state.browser.dir());
    let sessions: Vec<&Session> = s.screens.edits.in_folder(host, dir).collect();
    if sessions.is_empty() {
        return None;
    }
    let offset = utc_offset(s);
    let mut lines = col().key("files-editing").w_full().border_sides([1.0, 0.0, 0.0, 0.0], t.hair2).py(3.0);
    for (i, e) in sessions.iter().enumerate() {
        let status = e.status(offset);
        let name = s.text_shown(paths::name(&e.path)).into_owned();
        let mut line = row()
            .key(format!("editing-{i}"))
            .w_full()
            .h(30.0)
            .px(16.0)
            .gap(8.0)
            .items_center()
            .role("status")
            .child(match (status.error || status.waits, state.browser.entry(paths::name(&e.path))) {
                (true, _) => icon(Icon::Warning, 13.0, if status.error { t.red } else { t.orange }),
                (false, Some(entry)) => icon(super::view::glyph_icon(sshee_app::files::glyph(entry)), 13.0, t.l2),
                (false, None) => icon(Icon::File, 13.0, t.l2),
            })
            .child(text(name).font(theme::TAB_NAME).color(t.l1).single_line().truncate().shrink().min_w(0.0))
            .child(text(s.text_shown(&status.line).into_owned()).font(theme::NOTE).color(if status.error { t.red } else if status.waits { t.orange } else { t.l2 }).single_line().truncate().grow(1.0).min_w(0.0));
        let id = e.id.clone();
        if status.retry {
            let id = id.clone();
            line = line.child(push_button(t, "Retry", Push::Quiet, true).key(format!("editing-retry-{i}")).on_press_tx(move |s: &mut Shell, tx| {
                ask(s, EditAsk::RetryNow { id: id.clone() });
                tx.instant();
            }));
        }
        if status.conflict {
            let id = id.clone();
            line = line.child(push_button(t, "Resolve…", Push::Quiet, true).key(format!("editing-resolve-{i}")).on_press_tx(move |s: &mut Shell, tx| {
                s.screens.edits.show_conflict(Some(&id));
                tx.instant();
            }));
        }
        if e.is_open() {
            line = line.child(push_button(t, "Stop editing", Push::Quiet, true).key(format!("editing-stop-{i}")).on_press_tx(move |s: &mut Shell, tx| {
                stop(s, &id);
                tx.instant();
            }));
        } else {
            let folder = e.local.parent().map(PathBuf::from);
            line = line.child(push_button(t, "Show in folder", Push::Quiet, true).key(format!("editing-folder-{i}")).on_press_tx(move |s: &mut Shell, tx| {
                if let Some(path) = folder.clone() {
                    ask(s, EditAsk::ShowFolder { path });
                }
                tx.instant();
            }));
        }
        lines = lines.child(line);
    }
    Some(lines)
}

/// The window-wide overlays: a question before opening, the conflict's dialog, the leftovers'
/// prompt. At most one, in that order.
pub fn overlay(s: &Shell, t: &'static Theme) -> Option<El> {
    let edits = &s.screens.edits;
    if let Some(q) = &edits.question {
        let answer = move |yes: bool| {
            move |s: &mut Shell, tx: &mut Tx| {
                if let Some(want) = s.screens.edits.answer(yes) {
                    ask(s, want);
                }
                tx.instant();
            }
        };
        let key = match q {
            Question::Big { .. } => "edit-big",
            Question::Warn { .. } => "edit-warn",
        };
        return Some(question(t, &shown(s, &q.confirm()), answer(true), answer(false)).key(key));
    }
    if let Some(session) = edits.conflict.as_deref().and_then(|id| edits.session(id)) {
        return Some(conflict(s, session, t));
    }
    if !edits.leftovers.is_empty() {
        return Some(leftovers(s, t));
    }
    None
}

fn layer(t: &Theme, card: El) -> El {
    let card = crate::kit::motion::enters(card, crate::kit::motion::Entrance::Card);
    crate::kit::motion::enters(el().key("edit-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.scrim()).items_center().justify_center().child(card), crate::kit::motion::Entrance::Scrim)
}

fn conflict(s: &Shell, session: &Session, t: &'static Theme) -> El {
    let (title, body) = edit::conflict_words(session);
    let id = session.id.clone();
    let act = |want: fn(String) -> EditAsk| {
        let id = id.clone();
        move |s: &mut Shell, tx: &mut Tx| {
            ask(s, want(id.clone()));
            s.screens.edits.show_conflict(None);
            tx.instant();
        }
    };
    let mut card = popover(t, 560.0)
        .key("edit-conflict")
        .padding(18.0)
        .gap(8.0)
        .child(row().gap(8.0).items_center().child(icon(Icon::Warning, 15.0, t.orange)).child(text(s.text_shown(&title).into_owned()).font(theme::TEXT_BOLD).color(t.l1).wrap().grow(1.0).min_w(0.0)))
        .child(text(s.text_shown(&body).into_owned()).font(theme::NOTE).color(t.l2on).wrap().w_full());
    if let Some(c) = &s.screens.edits.compare {
        card = card.child(comparison(s, c, t));
    }
    let compare_id = id.clone();
    let comparing = s.screens.edits.compare.is_some();
    let buttons = row()
        .w_full()
        .gap(8.0)
        .mt(8.0)
        .flex_wrap()
        .child(push_button(t, if comparing { "Hide changes" } else { edit::COMPARE }, Push::Quiet, true).key("conflict-compare").on_press_tx(move |s: &mut Shell, tx| {
            if s.screens.edits.compare.take().is_none() {
                ask(s, EditAsk::Compare { id: compare_id.clone() });
            }
            tx.instant();
        }))
        .child(spacer())
        // Each of the first two throws a version away; the copy keeps both.
        .child(push_button(t, edit::KEEP_MINE, Push::Caution, true).key("conflict-mine").on_press_tx(act(|id| EditAsk::KeepMine { id })))
        .child(push_button(t, edit::TAKE_THEIRS, Push::Caution, true).key("conflict-theirs").on_press_tx(act(|id| EditAsk::TakeTheirs { id })))
        .child(push_button(t, edit::SAVE_COPY, Push::Quiet, true).key("conflict-copy").autofocus().on_press_tx(act(|id| EditAsk::SaveMineAsCopy { id })));
    let card = card
        .child(buttons)
        .modal_tx(false, |s: &mut Shell, tx| {
            s.screens.edits.show_conflict(None);
            tx.instant();
        });
    layer(t, card)
}

/// The two versions, line by line: what only yours has, what only theirs has, the rest quiet.
fn comparison(s: &Shell, c: &edit::Comparison, t: &'static Theme) -> El {
    if c.binary {
        return text("These files aren't text: there's nothing to compare line by line.").key("compare-binary").font(theme::NOTE).color(t.l2).wrap().w_full();
    }
    let mut lines = col().key("compare-lines").w_full().py(4.0);
    for (i, (kind, line)) in c.lines.iter().enumerate() {
        let (side, fill, ink) = match kind {
            DiffKind::Same => ("", None, t.l2),
            DiffKind::Mine => ("yours", Some(t.hov2), t.l1),
            DiffKind::Theirs => ("server", Some(t.code), t.l1),
        };
        let mut r = row().key(format!("compare-{i}")).w_full().h(18.0).px(8.0).gap(10.0).items_center();
        if let Some(fill) = fill {
            r = r.fill(fill);
        }
        lines = lines.child(
            r.child(text(side).font(theme::SMALL).color(t.l2on).single_line().w(44.0))
                .child(text(s.text_shown(line).into_owned()).font(if *kind == DiffKind::Mine { CODE_MINE } else { theme::CODE }).color(ink).single_line().truncate().grow(1.0).min_w(0.0)),
        );
    }
    col()
        .key("compare")
        .w_full()
        .max_h(COMPARE_H)
        .radius(theme::RADIUS_ROW)
        .border(1.0, t.hair)
        .scroll(ScrollSpec { stick_bottom: false })
        .role("table")
        .label_static("Yours and the server's, line by line")
        .child(lines)
}

fn leftovers(s: &Shell, t: &'static Theme) -> El {
    let list = &s.screens.edits.leftovers;
    let q = leftovers_prompt(list, |host| s.bench.server(host).map_or_else(|| host.to_owned(), |sv| s.server_shown(host, &sv.name).into_owned()));
    let q = shown(s, &q);
    // One file's own folder; several, the folder that holds them all.
    let folder = match list.as_slice() {
        [one] => one.local.parent().map(PathBuf::from),
        many => many.first().and_then(|l| l.local.parent()).and_then(|p| p.parent()).map(PathBuf::from),
    };
    let mut names = col().w_full().gap(2.0).padding(8.0).radius(theme::RADIUS_ROW).fill(t.code);
    for line in &q.lines {
        names = names.child(text(line).font(theme::NOTE).color(t.l1).single_line().truncate());
    }
    let card = popover(t, 460.0)
        .key("edit-leftovers")
        .padding(18.0)
        .gap(8.0)
        .child(text(&q.title).font(theme::TEXT_BOLD).color(t.l1).wrap().w_full())
        .child(text(&q.body).font(theme::NOTE).color(t.l2on).wrap().w_full())
        .child(names)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .mt(8.0)
                .child(push_button(t, "Show in folder", Push::Quiet, folder.is_some()).key("leftovers-folder").on_press_tx(move |s: &mut Shell, tx| {
                    if let Some(path) = folder.clone() {
                        ask(s, EditAsk::ShowFolder { path });
                    }
                    tx.instant();
                }))
                .child(spacer())
                .child(push_button(t, "Discard", Push::Caution, true).key("leftovers-discard").on_press_tx(|s: &mut Shell, tx| {
                    for l in s.screens.edits.leftovers_done() {
                        ask(s, EditAsk::DiscardLeftover { id: l.id });
                    }
                    tx.instant();
                }))
                .child(push_button(t, "Upload", Push::Primary, true).key("leftovers-upload").default_button().on_press_tx(|s: &mut Shell, tx| {
                    for l in s.screens.edits.leftovers_done() {
                        ask(s, EditAsk::ResumeLeftover { id: l.id, host: l.host });
                    }
                    tx.instant();
                })),
        )
        .modal_tx(false, |s: &mut Shell, tx| {
            s.screens.edits.leftovers_done();
            tx.instant();
        });
    layer(t, card)
}
