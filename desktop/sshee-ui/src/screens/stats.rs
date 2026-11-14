//! A machine's statistics (0.10's StatsPane: CPU, memory, disks and network over time). Not built yet: this is its tab's page.

use rune::{Cx, El};
use sshee_app::workbench::Tab;

use super::placeholder;
use crate::icons::Icon;
use crate::shell::Shell;
use crate::theme::Theme;

/// One Stats tab's own state.
#[derive(Default)]
pub struct State {}

pub fn view(_s: &Shell, _tab: &Tab, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    placeholder(t, Icon::Stats, "Stats", "This page isn't in the preview yet.")
}
