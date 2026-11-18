//! The window's "?" marks (`kit::help`): the one card open at a time is `States::tip`, drawn
//! over everything as the window's last child. Settings' rows take theirs from the catalogue
//! (`Row::about`); the other screens' words are here, each where it is used.
//!
//! ```text
//! <key>        the "?": "About …"
//! help-card    its card, under it, while it's open
//! ```

use rune::El;

use crate::kit::help::{help_card, help_mark, Tip};
use crate::shell::Shell;
use crate::theme::Theme;

fn slot(s: &mut Shell) -> &mut Option<Tip> {
    &mut s.screens.tip
}

/// The "?" keyed `key` that explains `about` in `text`.
pub fn tip(s: &Shell, t: &Theme, key: impl Into<String>, about: &str, text: &'static str) -> El {
    let key = key.into();
    let open = s.screens.tip.as_ref().is_some_and(|o| o.key == key);
    help_mark(t, &key, about, text, open, slot)
}

/// The open card, over the whole window.
pub fn card(s: &Shell, t: &Theme) -> Option<El> {
    s.screens.tip.as_ref().map(|tip| help_card(t, tip, slot))
}

impl Shell {
    /// Esc closes a card the pointer opened, wherever focus is (one opened from the keys is
    /// modal and hears Esc itself).
    pub(crate) fn tip_key(&mut self, ev: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
        if ev.key != rune::Key::Escape || self.screens.tip.is_none() {
            return false;
        }
        self.screens.tip = None;
        tx.instant();
        true
    }
}

/// Tunnels: the three kinds, side by side in the form.
pub const TUNNEL_KINDS: &str = "A server's port here: something running on the server, like a database or an admin page, opens on this computer at localhost (this computer's own address), without being opened to the internet. Browse through it: your browser's traffic leaves from the server, as if you sat there. A port here, on the server: the other way round, so the server reaches something running on this computer.";

/// Tunnels: what saving one does.
pub const SAVED_TUNNELS: &str = "A saved tunnel that's on starts again whenever SSHeesh opens, and comes back by itself after a dropped connection. Turn it off to keep it for later without it running. Keep, on a web app above, saves it here too.";

/// Snippets: blanks.
pub const SNIPPET_BLANKS: &str = "A blank is a word in double braces, like {{service}}. Each time the snippet runs, SSHeesh asks what goes there, and {{service:web}} offers web to start with. A blank named {{container}} lets you pick one of the server's running containers.";

/// Containers: what one is.
pub const CONTAINERS: &str = "A container is a program packed with everything it needs, running sealed off from the rest of the server; Docker and Podman are the tools that run them. Here you can see each one, follow its logs or open a shell inside it. Ones started together from a compose file are grouped as a project.";

/// "Free up disk space": Docker's four tiles.
pub const DOCKER_DISK: &str = "Images are the programs Docker downloads, containers are copies of them that ran, volumes hold their saved data, and the build cache speeds up building new images. Unused is what no container needs now. Volumes can hold a database, so SSHeesh never offers to delete them.";

/// Files: the places every Linux server has.
pub const PLACES: &str = "Folders most Linux servers have. Logs (/var/log) is where programs write what happened, Config (/etc) holds the system's settings, Web (/var/www) is where websites usually live, Apps (/opt) holds software added by hand, and Temp (/tmp) is for throwaway files.";

/// Stats: the load average.
pub const LOAD: &str = "Load is how many programs want the processor at once, averaged over the last 1, 5 and 15 minutes. While it stays below the number of cores, the server keeps up; above that, programs wait their turn.";

/// Every one of the words above, for tests.
#[cfg(test)]
pub const ALL: [&str; 7] = [TUNNEL_KINDS, SAVED_TUNNELS, SNIPPET_BLANKS, CONTAINERS, DOCKER_DISK, PLACES, LOAD];

/// What a screen reader hears of the "?" named "About {about}" on screen: its description.
#[cfg(test)]
pub(crate) fn heard(rt: &rune::Runtime<Shell>, about: &str) -> Option<String> {
    let name = format!("About {about}");
    let tree = rt.accessibility_tree();
    tree.nodes.iter().find(|(_, n)| n.label() == Some(name.as_str())).and_then(|(_, n)| n.description().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    #[test]
    fn each_tip_is_a_few_plain_sentences() {
        for words in super::ALL {
            let sentences = words.matches(". ").count() + 1;
            assert!(sentences <= 3 && words.len() <= 400 && words.ends_with('.'), "{sentences} sentences, {} bytes: {words}", words.len());
        }
    }
}
