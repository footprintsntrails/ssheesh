//! The main menu (0.10's Home: your servers, what's running now, "Where to?"). Not built yet:
//! this is its place, shown when no tab is on screen.

use rune::{col, text, Cx, El};

use crate::icons::logo;
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The main menu's own state.
#[derive(Default)]
pub struct State {}

pub fn view(s: &Shell, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    let key = if s.options.mac_keys { "⌘T" } else { "Ctrl+Shift+T" };
    col()
        .key("home-page")
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .gap(10.0)
        .child(logo(56.0))
        .child(text("SSHeesh Preview").font(theme::TEXT_BOLD).color(t.l1).single_line())
        .child(text(format!("Pick a server on the left, or press {key} for a terminal on this computer.")).font(theme::NOTE).color(t.l2).single_line())
}
