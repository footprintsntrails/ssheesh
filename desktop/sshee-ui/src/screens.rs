//! The screens: one module each, owned by whoever builds that screen. The window's shell
//! (`crate::shell`) draws the bar, the sidebar and the status bar and hands the page to the
//! screen of the tab on screen; a terminal tab's page is the terminal pane itself
//! (`crate::terminal`).
//!
//! A screen is `pub fn view(..) -> El` over the window's state, built from the kit
//! (`crate::kit`) and the tokens (`crate::theme`), with its own state in its own struct here in
//! [`States`], so two screens never edit the same lines. How to add one, and the list of those
//! still to build with the files each owns: desktop/ARCHITECTURE.md.

pub mod add_server;
pub mod files;
pub mod home;
pub mod palette;
pub mod server_card;
pub mod server_menu;
pub mod settings;
pub mod stats;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use rune::{col, text, Cx, El};
use sshee_app::workbench::TabId;

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// Every screen's own state, in the window's state (`Shell::screens`). A tab's is made when
/// the tab is first shown and dropped when it closes.
#[derive(Default)]
pub struct States {
    pub home: home::State,
    pub palette: palette::State,
    pub add_server: add_server::State,
    pub server_card: server_card::State,
    pub server_menu: server_menu::State,
    pub files: HashMap<TabId, files::State>,
    pub stats: HashMap<TabId, stats::State>,
    pub settings: settings::State,
}

impl States {
    /// The tabs that are gone take their screens' state with them.
    pub fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.files.retain(|id, _| open(*id));
        self.stats.retain(|id, _| open(*id));
        self.palette.retain(&open);
    }

    /// Something is open over the window (the palette, a dialog, a menu): no shortcut fires
    /// behind it and no key reaches a terminal, as behind any dialog in 0.10.
    pub fn any_open(&self) -> bool {
        self.palette.open || self.add_server.open || self.server_menu.open.is_some()
    }

    /// Closes what is open over the window, before something else opens there: one at a time.
    pub fn close_all(&mut self) {
        self.palette.open = false;
        self.add_server = add_server::State::default();
        self.server_menu.open = None;
        self.server_card.hide();
    }
}

/// What is drawn over the window, as last children of its root, in order: a pop-up menu open
/// in Settings, a server's card under the pointer, then whichever of the palette, a server's
/// menu and "Add a server" is open. A screen that opens over the window adds its element here.
pub fn overlays(s: &crate::shell::Shell, t: &'static Theme, cx: &mut Cx<crate::shell::Shell>) -> Vec<El> {
    [settings::overlay(s, t), server_card::view(s, t), palette::view(s, t, cx), server_menu::view(s, t, cx), add_server::view(s, t)]
        .into_iter()
        .flatten()
        .collect()
}

/// A page with nothing to show yet: an icon, a name and a line under it, in the middle.
pub fn placeholder(t: &Theme, glyph: Icon, name: &str, note: &str) -> El {
    col()
        .key("placeholder")
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .gap(8.0)
        .child(icon(glyph, 28.0, t.l2))
        .child(text(name).font(theme::TEXT_BOLD).color(t.l1).single_line())
        .child(text(note).font(theme::NOTE).color(t.l2).single_line())
}
