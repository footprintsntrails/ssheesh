//! The screens: one module each, owned by whoever builds that screen. The window's shell
//! (`crate::shell`) draws the bar, the sidebar and the status bar and hands the page to the
//! screen of the tab on screen; a terminal tab's page is the terminal pane itself
//! (`crate::terminal`).
//!
//! A screen is `pub fn view(..) -> El` over the window's state, built from the kit
//! (`crate::kit`) and the tokens (`crate::theme`), with its own state in its own struct here in
//! [`States`], so two screens never edit the same lines. How to add one, and the list of those
//! still to build with the files each owns: desktop/ARCHITECTURE.md.

pub mod agent;
pub mod add_server;
pub mod canary;
pub mod checkup;
pub mod containers;
pub mod disk;
pub mod files;
pub mod home;
pub mod palette;
pub mod recipes;
pub mod prompts;
pub mod server_card;
pub mod server_menu;
pub mod settings;
pub mod snippets;
pub mod stats;
pub mod tour;
pub mod transfers;
pub mod welcome;
pub mod whats_new;
pub mod tor;
pub mod tunnels;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod disk_tests;
#[cfg(test)]
mod tools_tests;

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
    pub agent: agent::State,
    pub files: HashMap<TabId, files::State>,
    /// Files, window-wide: folders seen lately, pinned and recent folders, uploads and
    /// downloads in progress (what finished or failed is said in a toast: `notes`).
    pub dirs: sshee_app::dir_cache::DirCache,
    pub bookmarks: sshee_app::model::Bookmarks,
    /// Where Files copied to last ("Copy to another folder or server"): the server and folder
    /// it opens on next time.
    pub copy_last: Option<(String, String)>,
    pub transfers: sshee_app::transfers::Transfers,
    /// Files open in an editor elsewhere, and the editors on this computer.
    pub edits: sshee_app::files::edit::Edits,
    pub stats: HashMap<TabId, stats::State>,
    /// Each Containers tab's last look and what's being done to its containers.
    pub containers: HashMap<TabId, containers::State>,
    /// What the looks at each machine tell beyond its vital signs (the Stats page).
    pub machines: stats::Machines,
    pub settings: settings::State,
    /// Toasts and the open confirmation (`shell::notes`).
    pub notes: crate::shell::notes::Notes,
    /// What the desktop outside the window was last told (`shell::desktop`).
    pub desktop: crate::shell::desktop::State,
    /// A newer test build: the line across the top of the page.
    pub canary: canary::State,
    /// The security check, recipes and snippets; the welcome, the tour (and hints), and
    /// "What's new".
    pub checkup: checkup::State,
    /// "Free up disk space".
    pub disk: disk::State,
    pub recipes: recipes::State,
    pub snippets: snippets::State,
    pub welcome: welcome::State,
    pub tour: tour::State,
    pub whats_new: whats_new::State,
    /// Tor's popover, and what it knows of Tor.
    pub tor: tor::State,
    /// A server's tunnels, and every saved tunnel with its state.
    pub tunnels: tunnels::State,
}

impl States {
    /// The tabs that are gone take their screens' state with them.
    pub fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.files.retain(|id, _| open(*id));
        self.stats.retain(|id, _| open(*id));
        self.containers.retain(|id, _| open(*id));
        self.palette.retain(&open);
        self.agent.retain(&open);
    }

    /// Something is open over the window (the palette, a dialog, a menu): no shortcut fires
    /// behind it and no key reaches a terminal, as behind any dialog in 0.10.
    pub fn any_open(&self) -> bool {
        self.palette.open || self.add_server.open || self.server_menu.open.is_some() || self.agent.holds_keyboard() || self.tor.open || self.tunnels.open.is_some() || self.tools_open() || self.notes.asking()
    }

    /// The security check, recipes, snippets, the welcome, the tour or "What's new" is open.
    fn tools_open(&self) -> bool {
        self.checkup.is_open() || self.disk.is_open() || self.recipes.is_open() || self.snippets.is_open() || self.welcome.open || self.tour.open || !self.whats_new.releases.is_empty()
    }

    /// Closes what is open over the window, before something else opens there: one at a time.
    pub fn close_all(&mut self) {
        self.palette.open = false;
        self.add_server = add_server::State::default();
        self.server_menu.open = None;
        self.server_card.hide();
        self.agent.close();
        self.notes.close();
        self.checkup = checkup::State::default();
        self.disk = disk::State::default();
        // A recipe stepped aside stays aside: its bar brings the steps back.
        if !self.recipes.aside {
            self.recipes.close();
        }
        self.snippets.close();
        self.welcome.open = false;
        self.tour.open = false;
        self.whats_new.releases.clear();
        self.tor.open = false;
        self.tunnels.open = None;
    }
}

/// What is drawn over the window, as last children of its root, in order: a pop-up menu open
/// in Settings, a server's card under the pointer, then whichever of the palette, a server's
/// menu and "Add a server" is open. A screen that opens over the window adds its element here.
pub fn overlays(s: &crate::shell::Shell, t: &'static Theme, cx: &mut Cx<crate::shell::Shell>) -> Vec<El> {
    let mut out: Vec<El> = [settings::overlay(s, t), server_card::view(s, t)].into_iter().flatten().collect();
    // An agent's question under its tab, the list of agents.
    out.extend(agent::overlays(s, t, cx));
    // A recipe stepped aside, and a hint.
    out.extend([recipes::aside(s, t), tour::hint(s, t)].into_iter().flatten());
    out.extend([palette::view(s, t, cx), server_menu::view(s, t, cx), add_server::view(s, t, cx), tor::view(s, t, cx), tunnels::view(s, t)].into_iter().flatten());
    // The security check, recipes, snippets; the welcome, the tour and "What's new".
    out.extend([checkup::view(s, t), disk::view(s, t), recipes::view(s, t), snippets::view(s, t), welcome::view(s, t), tour::view(s, t), whats_new::view(s, t)].into_iter().flatten());
    out
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
