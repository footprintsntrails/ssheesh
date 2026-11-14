//! A terminal's own menu, on the secondary button: copy, paste, select all, find, clear, and a
//! new terminal on the same machine.
//!
//! It is drawn here, with the kit's menu rows, on every system: Rune's native pop-up menus
//! exist on macOS and Windows only, and one menu that looks the same everywhere is less to
//! test than two.
//!
//! Who gets the secondary button: a program that asked for the mouse does, as before, unless
//! the terminal runs in tmux. tmux always asks for the mouse (it scrolls its history with it),
//! so there the menu opens, and Shift sends the click to the program instead.

use rune::{El, Mods, Tx};
use sshee_term::engine::{Engine, Modes};
use sshee_term::input::{mouse_goes_to_program, Platform};

use super::overlays::Wish;
use super::pane::TermPane;
use crate::kit::menu::{context_menu, menu_row, menu_separator, RowState, ROW, SEPARATOR};
use crate::theme::Theme;

/// The menu's width, in points: room for "New terminal here" and the longest key cap.
const WIDTH: f32 = 248.0;

/// A pane's menu: where it is open, in the pane's points.
#[derive(Default)]
pub struct Menu {
    pub(crate) at: Option<[f32; 2]>,
}

/// What a row of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Copy,
    Paste,
    SelectAll,
    Find,
    Clear,
    NewTerminal,
}

/// The keys a row's cap shows, as the terminal reads them on `platform` (`sshee_term::input`).
pub(crate) fn keys_of(pick: Pick, platform: Platform) -> Option<&'static str> {
    let mac = platform == Platform::Mac;
    match pick {
        Pick::Copy => Some(if mac { "⌘C" } else { "Ctrl+Shift+C" }),
        Pick::Paste => Some(match platform {
            Platform::Mac => "⌘V",
            Platform::Windows => "Ctrl+V",
            Platform::Other => "Ctrl+Shift+V",
        }),
        Pick::SelectAll => mac.then_some("⌘A"),
        Pick::Find => Some(if mac { "⌘F" } else { "Ctrl+Shift+F" }),
        Pick::Clear => mac.then_some("⌘K"),
        Pick::NewTerminal => Some(if mac { "⌘T" } else { "Ctrl+Shift+T" }),
    }
}

/// Whether the secondary button opens the menu (see the module's docs), or is the program's.
pub(crate) fn opens_menu(modes: &Modes, mods: Mods, platform: Platform, in_tmux: bool) -> bool {
    if in_tmux {
        !mods.shift
    } else {
        !mouse_goes_to_program(modes, mods.shift, platform)
    }
}

impl TermPane {
    /// The secondary button at a point of the pane: opens the menu there when it is the
    /// menu's. False: the click is the program's.
    pub(crate) fn menu_pressed(&mut self, x: f32, y: f32, tx: &mut Tx) -> bool {
        let modes = self.view.borrow().modes;
        if !opens_menu(&modes, tx.mods(), self.platform, self.finds_in_tmux()) {
            return false;
        }
        self.over.menu.at = Some([x, y]);
        tx.instant();
        true
    }

    pub(crate) fn close_menu(&mut self, tx: &mut Tx) {
        if self.over.menu.at.take().is_some() {
            self.focus_terminal(tx);
            tx.instant();
        }
    }

    /// A row was picked.
    pub(crate) fn pick(&mut self, pick: Pick, tx: &mut Tx) {
        self.close_menu(tx);
        match pick {
            Pick::Copy => self.copy(tx),
            Pick::Paste => tx.request_paste(),
            Pick::SelectAll => {
                self.model.with_engine(|e| e.select_all());
                self.pull(tx);
            }
            Pick::Find => self.open_find(tx),
            Pick::Clear => {
                self.model.clear_history();
                self.pull(tx);
            }
            Pick::NewTerminal => self.over.wish(Wish::NewTerminal),
        }
    }
}

/// The menu, while it's open.
pub(crate) fn view<S: 'static>(pane: &TermPane, t: &'static Theme, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> Option<El> {
    let at = pane.over.menu.at?;
    let [w, h] = pane.area?;
    let selected = pane.view.borrow().selection.is_some();
    let row = move |key: &'static str, label: &'static str, pick: Pick, on: bool| {
        let e = menu_row(t, label, keys_of(pick, pane.platform), if on { RowState::Rest } else { RowState::Off }).key(key);
        if !on {
            return e;
        }
        e.on_press_tx(move |s: &mut S, tx| {
            if let Some(pane) = get(s) {
                pane.pick(pick, tx);
            }
        })
    };
    let rows = [
        row("menu-copy", "Copy", Pick::Copy, selected),
        row("menu-paste", "Paste", Pick::Paste, true),
        row("menu-select-all", "Select all", Pick::SelectAll, true),
        menu_separator(t),
        row("menu-find", "Find", Pick::Find, true),
        row("menu-clear", "Clear", Pick::Clear, true),
        menu_separator(t),
        row("menu-new", "New terminal here", Pick::NewTerminal, true),
    ];
    let height = 6.0 * ROW + 2.0 * SEPARATOR;
    Some(context_menu(t, at, [w, h], WIDTH, height, rows).key("menu").modal_tx(true, move |s: &mut S, tx| {
        if let Some(pane) = get(s) {
            pane.close_menu(tx);
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_term::engine::MouseProtocol;

    #[test]
    fn the_caps_are_the_keys_the_terminal_reads() {
        assert_eq!(keys_of(Pick::Copy, Platform::Mac), Some("⌘C"));
        assert_eq!(keys_of(Pick::Copy, Platform::Other), Some("Ctrl+Shift+C"));
        assert_eq!(keys_of(Pick::Paste, Platform::Windows), Some("Ctrl+V"));
        assert_eq!(keys_of(Pick::Paste, Platform::Other), Some("Ctrl+Shift+V"));
        assert_eq!(keys_of(Pick::Clear, Platform::Other), None);
        assert_eq!(keys_of(Pick::Clear, Platform::Mac), Some("⌘K"));
        assert_eq!(keys_of(Pick::Find, Platform::Windows), Some("Ctrl+Shift+F"));
    }

    #[test]
    fn a_program_that_wants_the_mouse_keeps_the_secondary_button_unless_it_is_tmux() {
        let plain = Modes::default();
        let wants = Modes { mouse: MouseProtocol::Vt200, ..Modes::default() };
        let shift = Mods { shift: true, ..Mods::default() };
        // A shell: the menu. A program with the mouse (vim, mc): the program, and Shift for
        // the menu, as Shift is for selecting there.
        assert!(opens_menu(&plain, Mods::default(), Platform::Other, false));
        assert!(!opens_menu(&wants, Mods::default(), Platform::Other, false));
        assert!(opens_menu(&wants, shift, Platform::Other, false));
        // tmux has the mouse always: the menu, and Shift for the program.
        assert!(opens_menu(&wants, Mods::default(), Platform::Other, true));
        assert!(!opens_menu(&wants, shift, Platform::Other, true));
    }
}
