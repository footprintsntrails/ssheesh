//! The sidebar's servers as 0.10 had them: a mark while one connects or when it's in trouble,
//! the "+" and the chevron on the row under the pointer, every label (Prod, Staging, Dev) in
//! the list, under the icon, in the title, on tabs, the card and a split's header, the page's
//! frame and the line along the top, the shortcuts in the tooltips,
//! and the title's menu with a server's tunnels and recipes.

use rune::Key;
use sshee_app::model::{EnvLabel, IconPack, SidebarMode};
use sshee_app::workbench::{Link, Server, TabKind};

use super::{linux, Window, DB, WEB};
use crate::kit::chip::env_color;

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
fn a_rows_button_shows_its_tooltip_and_not_the_servers_card_too() {
    let mut w = linux();
    w.wide();
    w.rt.help_delay = 0.0;
    let rest = |w: &mut Window| {
        for _ in 0..30 {
            w.rt.frame(1.0 / 60.0);
        }
    };
    w.hover("server-2");
    rest(&mut w);
    assert!(w.rt.find("server-card").is_some(), "the card came up on the row");
    // On the chevron: its tooltip, and the card makes way.
    w.hover("expand-2");
    rest(&mut w);
    assert_eq!(w.rt.help_tooltip().map(|(t, _)| t.to_owned()).as_deref(), Some("Show containers"));
    assert!(w.rt.find("server-card").is_none(), "the card and the chevron's tooltip show together");
    // The "+" too.
    w.hover("new-on-2");
    rest(&mut w);
    assert!(w.rt.help_tooltip().is_some() && w.rt.find("server-card").is_none());
    // Back on the name, the card comes back after a rest.
    let (x, y) = w.rt.find("server-2").unwrap();
    w.rt.pointer_move(x, y);
    w.settle();
    rest(&mut w);
    assert!(w.rt.find("server-card").is_some());
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

/// The colour some text is drawn in, by its words.
fn ink_of(w: &Window, words: &str, under: &str) -> Option<String> {
    w.rt.elements().into_iter().find(|e| !e.exiting && e.text.as_deref() == Some(words) && e.path.contains(under)).and_then(|e| e.text_color).map(|c| c.hex())
}

#[test]
fn staging_and_dev_go_on_tabs_cards_and_the_frame_as_prod_does() {
    let mut w = linux();
    w.rt.update(|s, _| {
        s.bench.set_servers(vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Staging).icon("database"),
            Server::host("lab", "lab-7731", "dev@198.51.100.12").env(EnvLabel::Dev),
        ]);
    });
    w.settle();
    w.rt.update(|s, tx| {
        for server in ["host:lab", "host:db", WEB] {
            let id = s.bench.open(TabKind::Terminal, server);
            s.focus_tab(id, tx);
        }
    });
    w.settle();
    let first_on = |w: &Window, server: &str| w.rt.state.bench.tabs_on(server).next().expect("a tab").id;
    let (web, db, lab) = (first_on(&w, WEB), first_on(&w, "host:db"), first_on(&w, "host:lab"));
    let t = w.rt.state.theme(true);
    let orange = t.orange.hex();
    // Each other machine's first tab carries its label, in its colour; the machine on screen
    // has its own in the title.
    assert!(w.rt.find(&format!("tab-{}/env-staging", db.0)).is_some(), "Staging on db's tab");
    assert!(w.rt.find(&format!("tab-{}/env-dev", lab.0)).is_some(), "Dev on lab's tab");
    assert!(w.rt.find(&format!("tab-{}/prod", web.0)).is_none() && w.rt.find("title/prod").is_some());
    assert_eq!(ink_of(&w, "Staging", &format!("tab-{}", db.0)), Some(env_color(t, &EnvLabel::Staging).hex()));
    assert_eq!(ink_of(&w, "Dev", &format!("tab-{}", lab.0)), Some(env_color(t, &EnvLabel::Dev).hex()));
    // On Staging: its frame is orange, and so is the line along the top when it's asked for.
    w.rt.update(|s, tx| s.focus_tab(db, tx));
    w.settle();
    assert!(w.rt.find("title/env-staging").is_some() && w.rt.find(&format!("tab-{}/env-staging", db.0)).is_none());
    assert!(w.rt.find("env-frame").is_some() && w.rt.find("env-stripe").is_none());
    w.rt.update(|s, tx| s.change_settings(tx, |st| st.set(|v| v.env_stripe = true)));
    w.settle();
    let stripe = w.rt.elements().into_iter().find(|e| e.key.as_deref() == Some("env-stripe")).expect("the line along the top");
    assert_eq!(stripe.fill.hex(), orange);
    // On Dev: framed, but no line along the top (as in 0.10: it warns of Production and
    // Staging only).
    w.rt.update(|s, tx| s.focus_tab(lab, tx));
    w.settle();
    assert!(w.rt.find("title/env-dev").is_some() && w.rt.find("env-frame").is_some() && w.rt.find("env-stripe").is_none());
    // Tabs that don't name their server: the group's name carries the label.
    w.rt.update(|s, tx| s.change_settings(tx, |st| st.set_more(|v| v.tabs_name_server = false)));
    w.settle();
    assert!(w.rt.find(&format!("group-{}/env-staging", db.0)).is_some(), "Staging after db's name");
    assert!(w.rt.find(&format!("group-{}/env-dev", lab.0)).is_some(), "Dev after lab's name");
    // The server's card under the pointer.
    w.wide();
    w.rt.help_delay = 0.0;
    w.hover("server-2");
    for _ in 0..30 {
        w.rt.frame(1.0 / 60.0);
    }
    assert!(w.rt.find("server-card/env-staging").is_some(), "Staging on db's card");
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

/// The servers' names in the sidebar's order.
fn order(w: &mut Window) -> Vec<String> {
    let mut names = Vec::new();
    w.rt.update(|s, _| names = s.bench.servers().iter().map(|s| s.name.clone()).collect());
    names
}

#[test]
fn a_server_held_and_dragged_goes_to_another_place_in_the_list() {
    for wide in [false, true] {
        let mut w = linux();
        w.rt.update(|s, _| {
            s.bench.set_servers(vec![
                Server::host("web", "web-7731", "deploy@203.0.113.17"),
                Server::host("db", "db-main", "postgres@203.0.113.31"),
                Server::host("lab", "lab-7731", "dev@198.51.100.12"),
            ]);
        });
        // The new servers slide in: let them land.
        for _ in 0..40 {
            w.rt.frame(0.05);
        }
        w.settle();
        if wide {
            w.wide();
        }
        assert_eq!(order(&mut w), ["This computer", "web-7731", "db-main", "lab-7731"]);
        // lab-7731, the last, held and dragged above web-7731.
        let (x, y) = w.rt.find("server-3").unwrap();
        let (_, to) = w.rt.find("server-1").unwrap();
        w.rt.pointer_down(x, y);
        w.rt.frame(1.0 / 60.0);
        for step in 1..=6 {
            w.rt.pointer_move(x, y + (to - 6.0 - y) * step as f32 / 6.0);
            w.rt.frame(1.0 / 60.0);
        }
        for _ in 0..20 {
            w.rt.frame(0.05);
        }
        // A line shows where it would go, and the list hasn't changed yet.
        let mark = w.rt.find("server-drop").expect("the mark where it lands");
        assert!(mark.1 < to, "the mark is above web-7731: {mark:?}, {to}");
        assert_eq!(order(&mut w).last().unwrap(), "lab-7731");
        w.rt.pointer_up(x, to - 6.0);
        w.settle();
        assert_eq!(order(&mut w), ["This computer", "lab-7731", "web-7731", "db-main"], "wide: {wide}");
        assert!(w.rt.find("server-drop").is_none());
        // A click is still a click: no move.
        w.click("server-2");
        assert_eq!(order(&mut w), ["This computer", "lab-7731", "web-7731", "db-main"]);
        // Escape mid-drag puts nothing anywhere.
        let (x, y) = w.rt.find("server-0").unwrap();
        w.rt.pointer_down(x, y);
        w.rt.frame(1.0 / 60.0);
        for step in 1..=4 {
            w.rt.pointer_move(x, y + 30.0 * step as f32);
            w.rt.frame(1.0 / 60.0);
        }
        w.key(Key::Escape, "");
        w.rt.pointer_up(x, y + 120.0);
        w.settle();
        assert_eq!(order(&mut w)[0], "This computer", "wide: {wide}");
    }
}
