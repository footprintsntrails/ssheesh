//! The screens: one module each, owned by whoever builds that screen. The window's shell
//! (`crate::shell`) draws the bar, the sidebar and the status bar and hands the page to the
//! screen of the tab on screen; a terminal tab's page is the terminal pane itself
//! (`crate::terminal`).
//!
//! A screen is `pub fn view(..) -> El` over the window's state, built from the kit
//! (`crate::kit`) and the tokens (`crate::theme`), with its own state in its own struct here in
//! [`States`], so two screens never edit the same lines. How to add one, and the list of those
//! still to build with the files each owns: desktop/ARCHITECTURE.md.

pub mod files;
pub mod home;
pub mod stats;

use std::collections::HashMap;

use rune::{col, text, El};
use sshee_app::workbench::TabId;

use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// Every screen's own state, in the window's state (`Shell::screens`). A tab's is made when
/// the tab is first shown and dropped when it closes.
#[derive(Default)]
pub struct States {
    pub home: home::State,
    pub files: HashMap<TabId, files::State>,
    pub stats: HashMap<TabId, stats::State>,
}

impl States {
    /// The tabs that are gone take their screens' state with them.
    pub fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.files.retain(|id, _| open(*id));
        self.stats.retain(|id, _| open(*id));
    }
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
