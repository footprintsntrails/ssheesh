//! Streamer mode: one switch (the eye in the top row, and 0.10's shortcut) that hides server
//! names, addresses, users and secrets everywhere the window draws them, at once: the title,
//! the tabs, tooltips, what screen readers are told, and every terminal's text, without
//! clearing a screen.
//!
//! Outside the terminals a server is "Server 2" (`sshee_term::privacy::host_alias`, as 0.10's
//! `hostAlias`) and any other text (a tab named after a folder) has the names in it dotted.
//! Inside them the presentation pass does the hiding (`Look::streaming`), so a find, a copy
//! and a link's hint see dots too.
//!
//! Every screen draws names through [`Shell::server_shown`] and [`Shell::text_shown`], so the
//! switch stays one.

use std::borrow::Cow;
use std::sync::Arc;

use rune::{El, Tx};
use sshee_app::workbench::Server;
use sshee_term::present::Look;
use sshee_term::privacy::{host_alias, HostNames, Names};

use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// Streamer mode's state.
#[derive(Default)]
pub struct Streamer {
    pub(crate) on: bool,
    /// Everything to hide, made from the server list when the mode was switched on or the
    /// list changed since.
    names: Arc<Names>,
    /// The list the names were made from, once they were made.
    made_from: Option<Vec<(String, String)>>,
}

/// The host of `deploy@203.0.113.17:2222`, and its user.
fn parts(address: &str) -> (&str, Option<&str>) {
    let (user, host) = match address.split_once('@') {
        Some((user, host)) => (Some(user).filter(|u| !u.is_empty()), host),
        None => (None, address),
    };
    // A port after the last colon, unless the colons are an IPv6 address's.
    let host = match host.rsplit_once(':') {
        Some((h, port)) if !h.contains(':') && port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => host,
    };
    (host.trim_matches(['[', ']']), user)
}

/// Everything streamer mode hides for these servers, and this computer's user.
fn names_of(servers: &[Server], local: &[String]) -> Names {
    let hosts = servers.iter().filter(|s| !s.is_local()).map(|s| {
        let (hostname, user) = parts(&s.address);
        HostNames { name: &s.name, hostname, user }
    });
    Names::new(hosts, local.iter().map(String::as_str))
}

/// This computer's user, as its environment names it: a prompt shows it.
fn local_names() -> Vec<String> {
    ["USER", "LOGNAME", "USERNAME"].iter().filter_map(|k| std::env::var(k).ok()).filter(|v| !v.is_empty()).collect()
}

impl Shell {
    /// Streamer mode is on.
    pub fn streaming(&self) -> bool {
        self.streamer.on
    }

    /// A server's name as the window may draw it: "Server 2" while streaming. This computer is
    /// "This computer" either way.
    pub fn server_shown<'a>(&self, key: &str, name: &'a str) -> Cow<'a, str> {
        if !self.streamer.on || key == "local" {
            return Cow::Borrowed(name);
        }
        let index = self.bench.servers().iter().filter(|s| !s.is_local()).position(|s| s.key() == key);
        Cow::Owned(host_alias(index))
    }

    /// Any other text the window draws outside a terminal (a tab's title, a note): names,
    /// users and addresses in it are dots while streaming.
    pub fn text_shown<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.streamer.on {
            Cow::Owned(self.streamer.names.mask_text(text))
        } else {
            Cow::Borrowed(text)
        }
    }

    /// What every terminal's presentation pass follows now.
    pub(crate) fn look(&self) -> Look {
        Look { streaming: self.streamer.on, hide_secrets: true, names: self.streamer.names.clone(), ..Look::default() }
    }

    /// The names to hide, made again when the server list changed.
    fn refresh_names(&mut self) -> bool {
        let list: Vec<(String, String)> = self.bench.servers().iter().map(|s| (s.name.clone(), s.address.clone())).collect();
        if self.streamer.made_from.as_ref() == Some(&list) {
            return false;
        }
        self.streamer.names = Arc::new(names_of(self.bench.servers(), &local_names()));
        self.streamer.made_from = Some(list);
        true
    }

    /// The eye, and its shortcut: everything hidden, or shown again. Applies to what is
    /// already on screen.
    pub fn toggle_streamer(&mut self, tx: &mut Tx) {
        self.streamer.on = !self.streamer.on;
        self.refresh_names();
        let look = self.look();
        let on = self.streamer.on;
        for pane in self.panes.values_mut() {
            // What was typed into a find bar could be a name: it goes with the bar.
            if on {
                pane.close_find(tx);
            }
            pane.set_look(look.clone(), tx);
        }
        self.row.close_overlays();
        self.settle(tx);
    }

    /// Before each frame while streaming: a server added or renamed meanwhile is hidden too.
    pub(crate) fn keep_streaming(&mut self, tx: &mut Tx) -> bool {
        if !self.streamer.on || !self.refresh_names() {
            return false;
        }
        let look = self.look();
        for pane in self.panes.values_mut() {
            pane.set_look(look.clone(), tx);
        }
        true
    }
}

/// The eye in the top row. Struck through and lit while streamer mode is on.
pub(crate) fn eye(s: &Shell, t: &'static Theme) -> El {
    let on = s.streamer.on;
    let keys = if s.options.mac_keys { "⌘⇧H" } else { "Ctrl+Shift+H" };
    let name = if on { format!("Streamer mode is on: names and secrets are hidden ({keys})") } else { format!("Streamer mode: hide names and secrets ({keys})") };
    let mut e = rune::el()
        .key("streamer")
        .size(theme::CONTROL, theme::CONTROL)
        .radius(theme::RADIUS)
        .items_center()
        .justify_center()
        .press(t.down)
        .focusable(true)
        .role("switch")
        .label_static("Streamer mode")
        .help(name)
        .on_press_tx(|s: &mut Shell, tx| s.toggle_streamer(tx))
        .child(icon(if on { Icon::EyeOff } else { Icon::Eye }, theme::ICON, if on { t.l1 } else { t.l2 }));
    e = if on { e.fill(t.selrail) } else { e.hover(t.hov2) };
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_gives_its_host_and_user() {
        assert_eq!(parts("deploy@203.0.113.17"), ("203.0.113.17", Some("deploy")));
        assert_eq!(parts("deploy@web-7731.example:2222"), ("web-7731.example", Some("deploy")));
        assert_eq!(parts("203.0.113.17"), ("203.0.113.17", None));
        assert_eq!(parts("root@[2001:db8::7]"), ("2001:db8::7", Some("root")));
        assert_eq!(parts("@web"), ("web", None));
        assert_eq!(parts(""), ("", None));
    }

    #[test]
    fn a_servers_name_host_and_user_are_all_hidden() {
        let servers = [Server::this_computer(), Server::host("web", "web-7731", "deploy@shop.example:2222")];
        let names = names_of(&servers, &["mira".to_owned()]);
        let masked = names.mask_text("deploy@web-7731 ~ ssh shop.example as mira from 203.0.113.9");
        for hidden in ["deploy", "web-7731", "shop.example", "mira", "203.0.113.9"] {
            assert!(!masked.contains(hidden), "{hidden} shows in {masked:?}");
        }
        assert!(masked.contains("ssh") && masked.contains("from"));
        // "This computer" is what the app calls it, not a name to hide.
        assert_eq!(names.mask_text("This computer"), "This computer");
    }
}
