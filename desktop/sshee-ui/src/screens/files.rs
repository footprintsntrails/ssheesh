//! The file manager (0.10's FilesPane: a server's folders, previews, transfers). Not built yet: this is its tab's page.

use rune::{Cx, El};
use sshee_app::workbench::Tab;

use super::placeholder;
use crate::icons::Icon;
use crate::shell::Shell;
use crate::theme::Theme;

/// One Files tab's own state.
#[derive(Default)]
pub struct State {}

pub fn view(_s: &Shell, _tab: &Tab, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    placeholder(t, Icon::Folder, "Files", "This page isn't in the preview yet.")
}
