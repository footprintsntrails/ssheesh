//! Files dropped on a terminal from another app (0.10's `useFileDrop`; Files takes its own,
//! `screens::files::dropped`). On this computer's terminal their paths are typed, quoted for
//! its shell, as any terminal app does. On a server's terminal a question asks where they go
//! (0.10's `UploadDialog`), starting at the folder the shell is in (tmux says; else what the
//! shell said, OSC 7; else home); the upload then runs through Files on that server, which
//! opens at that folder with the transfer's card. The pane lights up while files are over it.
//!
//! ```text
//! upload-layer      the dimmed window, taking every click
//! └── upload        Upload “notes.md” to web-7731?   (the names under it when there are more)
//!     ├── upload-folder   Into this folder: [the folder]   "Finding the terminal's folder…"
//!     └── upload-cancel, upload-go
//! ```
//!
//! Rune hears drops on macOS, Windows and X11; winit 0.30 reports none on Wayland.

use std::path::PathBuf;

use rune::{col, el, row, spacer, text, ClipboardContent, El, TextEdit, Tx};
use sshee_app::dropped::{self, ShellKind};
use sshee_app::workbench::TabId;

use super::notes::Toast;
use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::kit::field::{code_input, field};
use crate::kit::motion::{enters, Entrance};
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::theme::{self, Theme};

/// The question's width (0.10's).
const WIDTH: f32 = 460.0;

/// Files dropped on a server's terminal, waiting for a folder to go to.
pub(crate) struct UploadAsk {
    /// The terminal they were dropped on, and its server.
    tab: TabId,
    server: String,
    paths: Vec<PathBuf>,
    /// The folder, as typed; it starts at the terminal's.
    pub(crate) folder: TextEdit,
    /// The terminal's folder is being asked for: the field waits.
    finding: bool,
}

/// A terminal pane's element, taking files dropped on it.
pub(crate) fn terminal(s: &Shell, t: &Theme, id: TabId, pane: El) -> El {
    let pane = pane
        .on_drop_files(move |s: &mut Shell, files, tx| {
            s.drop_over = None;
            dropped_on_terminal(s, id, files, tx);
            tx.instant();
        })
        .on_drop_hover(move |s: &mut Shell, over, tx| {
            if over {
                s.drop_over = Some(id);
            } else if s.drop_over == Some(id) {
                s.drop_over = None;
            }
            tx.instant();
        });
    if s.drop_over == Some(id) {
        pane.border(2.0, t.ring)
    } else {
        pane
    }
}

fn dropped_on_terminal(s: &mut Shell, id: TabId, files: &[std::path::PathBuf], tx: &mut Tx) {
    let Some(tab) = s.bench.tab(id) else { return };
    let server = tab.server.clone();
    let local = s.bench.server(&server).is_none_or(|sv| sv.is_local());
    if local {
        let paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        match dropped::typed_paths(&paths, ShellKind::here()) {
            Some(text) => {
                if let Some(pane) = s.panes.get_mut(&id) {
                    pane.paste(&ClipboardContent::new(text), tx);
                }
            }
            None => {
                let (title, body) = dropped::CANT_TYPE;
                s.toast(Toast::error(title).body(body));
            }
        }
        return;
    }
    // A server's terminal: where they go is asked, starting at the folder its shell is in.
    if files.is_empty() {
        return;
    }
    s.upload = Some(UploadAsk { tab: id, server, paths: files.to_vec(), folder: TextEdit::new(String::new()), finding: true });
    match s.panes.get_mut(&id) {
        Some(pane) => pane.find_upload_folder(tx),
        None => {
            folder_found(s, id, None);
        }
    }
    tx.focus("upload-folder", false);
}

/// The folder the terminal's shell is in came back (`Wish::UploadFolder`): the question starts
/// there, or at home when nobody can tell. True when the question changed.
pub(crate) fn folder_found(s: &mut Shell, id: TabId, folder: Option<String>) -> bool {
    let Some(ask) = s.upload.as_mut().filter(|a| a.tab == id && a.finding) else { return false };
    ask.finding = false;
    // The server's own `~` is found when the upload lists it.
    ask.folder = TextEdit::new(dropped::start_folder(folder));
    true
}

/// Upload: Files on the server opens at the folder and the files go there, with a card for
/// their progress.
fn send(s: &mut Shell, tx: &mut Tx) {
    let Some(folder) = s.upload.as_ref().filter(|a| !a.finding).and_then(|a| dropped::folder(&a.folder.text).map(str::to_owned)) else { return };
    let Some(ask) = s.upload.take() else { return };
    let files_tab = s.open_files_on(&ask.server, Some(&folder), tx);
    crate::screens::files::dropped(s, files_tab, ask.paths, Some(folder));
    tx.instant();
}

fn cancel(s: &mut Shell, tx: &mut Tx) {
    if s.upload.take().is_some() {
        s.settle(tx);
    }
}

/// The folder field's text, for Rune to edit; the window's spare one when the question closed
/// under an edit already on its way (never read).
fn folder_field(s: &mut Shell) -> &mut TextEdit {
    match s.upload.as_mut() {
        Some(ask) => &mut ask.folder,
        None => &mut s.upload_spare,
    }
}

/// The question, while files dropped on a server's terminal wait for a folder.
pub(crate) fn dialog(s: &Shell, t: &'static Theme) -> Option<El> {
    let ask = s.upload.as_ref()?;
    let names: Vec<String> = ask.paths.iter().map(|p| s.text_shown(dropped::local_name(&p.to_string_lossy())).into_owned()).collect();
    let server = s.bench.server(&ask.server).map(|sv| s.server_shown(&ask.server, &sv.name).into_owned()).unwrap_or_default();
    let title = dropped::upload_title(&names, &server);
    let mut words = col().grow(1.0).min_w(0.0).gap(4.0).child(text(&title).key("upload-title").font(theme::TITLE).color(t.l1).wrap().w_full());
    if let Some(line) = dropped::names_line(&names) {
        words = words.child(text(line).font(theme::NOTE).color(t.l2).single_line().truncate());
    }
    let head = row()
        .w_full()
        .gap(12.0)
        .items_start()
        .child(el().size(32.0, 32.0).radius(16.0).fill(t.blue.alpha(0.15)).items_center().justify_center().child(icon(Icon::Upload, 15.0, t.blue)))
        .child(words);
    let hint = if ask.finding { "Finding the terminal's folder…" } else { "/path/on/the/server" };
    let input = code_input(t, &ask.folder, hint)
        .key("upload-folder")
        .label_static("Into this folder")
        .on_edit(folder_field)
        .on_submit(send);
    let mut folder = field(t, input, false);
    if ask.finding {
        folder = folder.opacity(0.6).child(crate::kit::progress_ring(13.0, 1.5, t.l2));
    }
    let ready = !ask.finding && dropped::folder(&ask.folder.text).is_some();
    let go = push_button(t, "Upload", Push::Primary, ready).key("upload-go");
    let go = if ready { go.default_button().on_press_tx(send) } else { go };
    let card = popover(t, WIDTH)
        .key("upload")
        .padding(20.0)
        .gap(6.0)
        .role("dialog")
        .label(title.clone())
        .child(head)
        .child(text("Into this folder").font(theme::GROUP_TITLE).color(t.l2on).single_line().mt(10.0))
        .child(folder)
        .child(text("The folder this terminal is in. Change it if you like.").font(theme::SMALL).color(t.l2).wrap().w_full())
        .child(row().w_full().gap(8.0).mt(12.0).child(spacer()).child(push_button(t, "Cancel", Push::Quiet, true).key("upload-cancel").on_press_tx(cancel)).child(go))
        .modal_tx(false, cancel);
    let layer = el().key("upload-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.scrim()).items_center().justify_center().child(enters(card, Entrance::Card));
    Some(enters(layer, Entrance::Scrim))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An edit that lands after the question closed goes to the window's one spare field,
    /// not to a new one each time (which was never freed).
    #[test]
    fn an_edit_after_the_question_closed_goes_to_the_one_spare_field() {
        let mut s = super::super::blank();
        let spare: *const TextEdit = &s.upload_spare;
        assert!(std::ptr::eq(folder_field(&mut s), spare));
        assert!(std::ptr::eq(folder_field(&mut s), spare));
    }
}
