//! The sidebar's servers as 0.10 had them: a mark while one connects or when it's in trouble,
//! the "+" and the chevron on the row under the pointer, every label (Prod, Staging, Dev) in
//! the list, under the icon, in the title and a split's header, the shortcuts in the tooltips,
//! and the title's menu with a server's tunnels and recipes.

use rune::Key;
use sshee_app::model::{EnvLabel, IconPack, SidebarMode};
use sshee_app::workbench::{Link, Server, TabKind};

use super::{linux, Window, DB, WEB};

impl Window {
    fn wide(&mut self) {
        self.rt.update(|s, tx| s.change_settings(tx, |st| st.set(|x| x.sidebar_mode = SidebarMode::Wide)));
        // The sidebar widens as it slides: let it land.
        for _ in 0..40 {
            self.rt.frame(0.05);
        }
        self.settle();
    }

    fn hover(&mut self, key: &str) {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.pointer_move(x, y);
        self.settle();
    }

    /// What the tooltip of `key` says.
    fn tip_words(&mut self, key: &str) -> String {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.help_delay = 0.0;
        self.rt.pointer_move(x, y);
        for _ in 0..20 {
            self.rt.frame(0.05);
        }
        let words = self.rt.help_tooltip().map(|(t, _)| t.to_owned()).unwrap_or_else(|| panic!("{key} shows no tooltip"));
        self.rt.pointer_move(700.0, 400.0);
        self.settle();
        words
    }

    /// Every tab on `server` gets this link.
    fn link_all(&mut self, server: &'static str, link: Link) {
        self.rt.update(move |s, _| {
            let ids: Vec<_> = s.bench.tabs_on(server).map(|t| t.id).collect();
            for id in ids {
                s.bench.set_link(id, link);
            }
        });
        self.settle();
    }
}

#[test]
fn a_servers_icon_is_marked_while_it_connects_and_when_it_is_in_trouble() {
    let mut w = linux();
    w.click("server-1");
    // Connected: the usual case, nothing marked.
    assert!(w.rt.find("health").is_none());
    for (link, said) in [(Link::Connecting, "Connecting"), (Link::Reconnecting, "Connection lost, reconnecting"), (Link::Failed, "Couldn't connect")] {
        w.link_all(WEB, link);
        assert!(w.rt.find("health").is_some(), "{said}");
        assert!(w.tree().contains(said), "{said} isn't in: {}", w.tree());
    }
    // The list says it after the name.
    w.wide();
    assert!(w.tree().contains("web-7731, couldn't connect"), "{}", w.tree());
    w.link_all(WEB, Link::Connected { durable: true });
    assert!(w.rt.find("health").is_none());
}

#[test]
fn the_row_under_the_pointer_offers_a_new_terminal_there_and_its_containers() {
    let mut w = linux();
    w.wide();
    assert!(w.rt.find("new-on-2").is_none() && w.rt.find("expand-2").is_none(), "only under the pointer");
    w.hover("server-2");
    assert!(w.rt.find("new-on-2").is_some() && w.rt.find("expand-2").is_some());
    assert!(w.tree().contains("New terminal on db-main") && w.tree().contains("Show containers on db-main"), "{}", w.tree());
    w.hover("new-on-2");
    w.click("new-on-2");
    assert_eq!(w.row(), "*Terminal@db-main");
    w.hover("new-on-2");
    w.click("new-on-2");
    assert_eq!(w.row(), "Terminal@db-main *Terminal 2@db-main");
    // Gone when the pointer leaves the row.
    w.rt.pointer_move(700.0, 400.0);
    w.settle();
    assert!(w.rt.find("new-on-2").is_none());
}

#[test]
fn staging_and_dev_are_labelled_like_prod() {
    let mut w = linux();
    w.rt.update(|s, _| {
        s.bench.set_servers(vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Staging).icon("database"),
            Server::host("lab", "lab-7731", "dev@198.51.100.12").env(EnvLabel::Dev),
        ]);
    });
    w.settle();
    // Under each icon in the rail, a short line in its label's colour.
    let lines = (1..=3).filter(|i| w.rt.find(&format!("server-{i}/env-line")).is_some()).count();
    assert_eq!(lines, 3);
    // In the list, the chips.
    w.wide();
    for chip in ["prod", "env-staging", "env-dev"] {
        assert!(w.rt.find(chip).is_some(), "{chip}");
    }
    assert!(w.tree().contains("Staging") && w.tree().contains("Dev"), "{}", w.tree());
    // In the title, the label of the server on screen.
    w.click("server-2");
    let title = w.rect("title").unwrap();
    let (x, y) = w.rt.find("title/env-staging").expect("Staging in the title");
    assert!(x > title[0] && x < title[0] + title[2] && y > title[1] && y < title[1] + title[3], "{title:?} {x},{y}");
    // In a split's header, beside another server's tab.
    w.rt.update(|s, tx| {
        let db = s.bench.active_id().unwrap();
        let lab = s.bench.open(TabKind::Terminal, "host:lab");
        s.bench.pair(db, lab);
        s.settle(tx);
    });
    w.settle();
    assert!(w.rt.find("env-dev").is_some(), "Dev in the split's header");
}

#[test]
fn search_and_settings_say_their_shortcuts() {
    let mut w = linux();
    let search = w.tip_words("search");
    assert!(search.starts_with("Search (") && search.ends_with(')'), "{search}");
    let settings = w.tip_words("settings-gear");
    assert!(settings.starts_with("Settings (") && settings.ends_with(')'), "{settings}");
    // The list's search says what it finds, and the same keys.
    w.wide();
    let wide = w.tip_words("search");
    assert!(wide.starts_with("Search servers, tabs, files and commands (") && wide.ends_with(&search["Search ".len()..]), "{wide}");
}

#[test]
fn the_title_menu_opens_a_servers_tunnels_and_recipes() {
    let mut w = linux();
    w.click("server-1");
    w.click("title");
    assert!(w.tree().contains("Web apps and tunnels") && w.tree().contains("Recipes"), "{}", w.tree());
    w.click("tm-tunnels");
    assert_eq!(w.rt.state.screens.tunnels.open.as_deref(), Some(WEB));
    w.key(Key::Escape, "");
    w.rt.update(|s, tx| s.close_tunnels(tx));
    w.settle();
    w.click("title");
    w.click("tm-recipes");
    assert_eq!(w.rt.state.screens.recipes.open.as_deref(), Some(WEB));
    w.rt.update(|s, tx| s.close_recipes(tx));
    w.settle();
    // This computer has neither.
    w.click("server-0");
    w.click("title");
    assert!(w.rt.find("title-menu").is_some() && w.rt.find("tm-tunnels").is_none() && w.rt.find("tm-recipes").is_none());
    let _ = DB;
}

#[test]
fn server_icons_can_be_pixels() {
    let mut w = linux();
    let before = w.rect("server-1").unwrap();
    w.rt.update(|s, tx| s.change_settings(tx, |st| st.set(|x| x.icon_pack = IconPack::Pixel)));
    w.settle();
    // Drawn in the same place, as pixels: the same buttons, each with its drawing.
    assert_eq!(w.rect("server-1").unwrap(), before);
    assert!(w.tree().contains("web-7731") && w.tree().contains("This computer"));
}
