//! Files dropped on a terminal from another app (0.10's `useFileDrop`; Files takes its own,
//! `screens::files::dropped`). On this computer's terminal their paths are typed, quoted for
//! its shell, as any terminal app does. On a server's terminal they upload through Files on
//! that server, into the folder the shell is in when it said so (OSC 7); else Files opens there
//! and asks for them again. 0.10's question about which folder (`UploadDialog`) isn't built
//! yet. The pane lights up while files are over it.
//!
//! Rune hears drops on macOS, Windows and X11; winit 0.30 reports none on Wayland.

use rune::{ClipboardContent, El, Tx};
use sshee_app::dropped::{self, ShellKind};
use sshee_app::workbench::TabId;

use super::notes::Toast;
use super::state::Shell;
use crate::theme::Theme;

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
    // A server's terminal: Files on that server takes them, into the folder the shell said
    // it's in when it did; else Files opens and asks for them there.
    let folder = s.panes.get(&id).and_then(|p| p.cwd()).filter(|f| f.starts_with('/')).map(str::to_owned);
    let files_tab = s.open_files_on(&server, folder.as_deref(), tx);
    if folder.is_some() {
        crate::screens::files::dropped(s, files_tab, files.to_vec(), folder);
        return;
    }
    let names: Vec<String> = files.iter().map(|p| dropped::local_name(&p.to_string_lossy()).to_owned()).collect();
    let what = if let [one] = names.as_slice() { format!("“{one}”") } else { format!("{} items", names.len()) };
    s.toast(Toast::info(format!("Drop {what} on a folder here to upload")).body("Files on this server opened to take them."));
}
