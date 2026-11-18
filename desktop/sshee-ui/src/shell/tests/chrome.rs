//! The window's chrome: the title and the row of tabs sharing the top bar, crowded tabs and
//! what they say, the status bar at a narrow width, and the sidebar's tooltips. Same harness
//! as the tests above.

use sshee_app::model::SidebarMode;
use sshee_app::workbench::{Link, TabId, Tor, Vitals};

use super::*;
use crate::theme;

impl Window {
    /// Every tab drawn in the row, with where it is.
    fn drawn_tabs(&self) -> Vec<(TabId, [f32; 4])> {
        self.rt.state.bench.tabs().iter().filter_map(|t| Some((t.id, self.rect(&format!("tab-{}", t.id.0))?))).collect()
    }

    /// "+" is whole in the window and clear of streamer mode's eye, every tab is before it, and
    /// a few more frames move none of it: the title and the row agree.
    fn plus_in_window(&mut self, width: f32, what: &str) {
        let plus = self.rect("new-tab").unwrap_or_else(|| panic!("{what}: no +"));
        let eye = self.rect("streamer").expect("the eye");
        assert!(plus[0] + plus[2] <= eye[0] + 0.5 && eye[0] + eye[2] <= width + 0.5, "{what}: + at {plus:?}, the eye at {eye:?}");
        for (id, r) in self.drawn_tabs() {
            assert!(r[0] + r[2] <= plus[0] + 0.5, "{what}: tab {} at {r:?} runs onto + at {plus:?}", id.0);
        }
        let address = self.rt.find("title-address").is_some();
        for _ in 0..3 {
            self.rt.frame(1.0 / 60.0);
        }
        assert_eq!(self.rect("new-tab"), Some(plus), "{what}: + moved a frame later");
        assert_eq!(self.rt.find("title-address").is_some(), address, "{what}: the title's address came or went a frame later");
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.rt.set_viewport((width, height), 1.0);
        self.settle();
    }
}

/// How wide a crowded tab named `name` is with its whole name and its cross (`kit::tab`: its
/// padding, icon, name, then the cross, which reaches 5 points into the end padding).
fn with_cross(name: &str) -> f32 {
    8.0 + theme::ICON + 6.0 + rune::text::advance(name, theme::TAB_NAME) + 6.0 + 14.0 + 8.0
}

#[test]
fn plus_stays_in_the_window_as_tabs_open_and_split() {
    for width in [1280.0, 900.0] {
        let mut w = linux();
        w.resize(width, 800.0);
        w.click("server-1");
        // Split right, then down, from the keyboard (the hunt's five tabs, here a few more).
        w.key(Key::Command('d'), "cs");
        w.plus_in_window(width, "split right");
        w.key(Key::Command('e'), "cs");
        w.plus_in_window(width, "split down");
        for n in 0..12 {
            w.rt.update(|s, tx| s.new_terminal(tx));
            w.settle();
            w.plus_in_window(width, &format!("{width} wide, {} more tabs", n + 1));
        }
        // And back: tabs close one by one, and the address comes back once there's room.
        for _ in 0..13 {
            let id = w.rt.state.bench.active_id().unwrap();
            w.rt.update(|s, tx| s.close_tab(id, tx));
            w.settle();
            w.plus_in_window(width, "closing");
        }
        assert!(w.rt.find("title-address").is_some(), "{width}: the address didn't come back with one tab left");
    }
}

#[test]
fn a_crowded_tab_says_its_state_with_a_mark_and_keeps_its_cross_inside() {
    let mut w = linux();
    w.resize(900.0, 600.0);
    w.click("server-1");
    for _ in 0..7 {
        w.rt.update(|s, tx| s.new_terminal(tx));
    }
    w.settle();
    assert!(w.rt.state.row.crowded(), "the row isn't crowded");
    let ids: Vec<TabId> = w.rt.state.bench.tabs().iter().map(|t| t.id).collect();
    let active = w.rt.state.bench.active_id().unwrap();
    // The tab you're on, and two others, lose their connection in three ways.
    w.rt.update(|s, _| {
        s.bench.set_link(active, Link::Reconnecting);
        s.bench.set_link(ids[ids.len() - 2], Link::Failed);
        s.bench.set_link(ids[ids.len() - 3], Link::Disconnected);
    });
    w.settle();
    w.plus_in_window(900.0, "reconnecting");
    // The words are only said to a screen reader; the cross is inside its tab.
    let tree = w.tree();
    assert!(tree.contains("Reconnecting…") && tree.contains("Couldn't connect") && tree.contains("Disconnected"), "{tree}");
    let plus = w.rect("new-tab").unwrap();
    for (id, r) in w.drawn_tabs() {
        let (x, y) = w.rt.find(&format!("tab-{}/close", id.0)).unwrap_or_else(|| panic!("tab {} has no cross", id.0));
        assert!(x > r[0] && x < r[0] + r[2] && y > r[1] && y < r[1] + r[3], "tab {}'s cross at {x},{y} is outside {r:?}", id.0);
        assert!(x < plus[0], "tab {}'s cross is on +", id.0);
    }
    // The tab you're on keeps its name and its cross; the others gave way.
    let mine = w.rect(&format!("tab-{}", active.0)).unwrap();
    let name = w.rt.state.bench.tab(active).unwrap().label();
    // Its mark (the ring, 10 points) comes before the cross.
    assert!(mine[2] + 2.5 >= with_cross(&name) + 6.0 + 10.0, "the tab you're on ({name}) is {} wide, not {}", mine[2], with_cross(&name) + 16.0);
}

#[test]
fn the_tab_you_are_on_keeps_its_name_in_a_crowded_row_even_under_the_pointer() {
    let mut w = linux();
    w.click("server-1");
    for _ in 0..9 {
        w.rt.update(|s, tx| s.new_terminal(tx));
    }
    let first = w.rt.state.bench.tabs()[0].id;
    w.rt.update(|s, _| {
        s.bench.set_title(first, Some("~/shop".into()));
    });
    w.settle();
    assert!(w.rt.state.row.crowded(), "the row isn't crowded");
    let active = w.rt.state.bench.active_id().unwrap();
    let mine = w.rect(&format!("tab-{}", active.0)).unwrap();
    assert!(mine[2] + 2.5 >= with_cross("Terminal 10"), "the tab you're on is {} wide", mine[2]);
    // The pointer comes over the row and clicks the first tab: it gets room for its cross,
    // and "+" stays in the window.
    let r = w.rect(&format!("tab-{}", first.0)).unwrap();
    w.rt.pointer_move(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    w.settle();
    w.click(&format!("tab-{}", first.0));
    assert_eq!(w.rt.state.bench.active_id(), Some(first));
    let now = w.rect(&format!("tab-{}", first.0)).unwrap();
    assert!(now[2] + 2.5 >= with_cross("~/shop"), "the tab clicked is {} wide", now[2]);
    let plus = w.rect("new-tab").unwrap();
    assert!(plus[0] + plus[2] <= 1280.0);
}

#[test]
fn a_failed_sign_in_is_named_on_its_tab_as_on_its_card() {
    let mut w = linux();
    w.click("server-1");
    let status = w.session(0).lock().unwrap().status.clone().unwrap();
    status(LinkStatus::Gone { note: "The server refused the connection".into() });
    w.settle();
    let tab = w.rt.state.bench.tabs()[0].clone();
    assert_eq!(tab.badge(), Some(Badge::Failed));
    assert!(w.tree().contains("Couldn't connect"), "{}", w.tree());
    // A shell that exited says so.
    status(LinkStatus::Ended { code: Some(0) });
    w.settle();
    assert_eq!(w.rt.state.bench.tabs()[0].badge(), Some(Badge::Ended));
}

#[test]
fn the_status_bar_drops_vitals_before_tor() {
    let mut w = linux();
    w.resize(900.0, 600.0);
    w.click("server-1");
    w.rt.update(|s, tx| {
        s.bench.set_tor(Tor::On { latency_ms: Some(84) });
        s.bench.set_vitals(WEB, Some(Vitals { cpu: Some(0.38), memory: 0.6, disk: Some((0.44, None)), uptime_secs: 3_200_000, down: Some(1_600_000.0), up: Some(315_000.0) }));
        s.change_settings(tx, |st| {
            st.set(|x| {
                x.sidebar_mode = SidebarMode::Wide;
                x.sidebar_width = 240.0;
            })
        });
    });
    w.settle();
    let tor = w.rect("tor-time").expect("Tor's readout");
    assert!(tor[0] + tor[2] <= 900.0, "Tor's readout is cut at {tor:?}");
    assert!(w.rt.find("vital-0").is_some(), "every vital went");
    let shown = (0..8).filter(|i| w.rt.find(&format!("vital-{i}")).is_some()).count();
    // Wider, they all come back.
    w.resize(1600.0, 800.0);
    let wide = (0..8).filter(|i| w.rt.find(&format!("vital-{i}")).is_some()).count();
    assert!(shown < wide, "{shown} at 900, {wide} at 1600");
    for i in 0..wide {
        let r = w.rect(&format!("vital-{i}")).unwrap();
        assert!(r[0] + r[2] <= w.rect("tor-time").unwrap()[0]);
    }
}

#[test]
fn the_vitals_open_stats_and_their_menu_hides_them() {
    let mut w = linux();
    w.click("server-1");
    w.rt.update(|s, _| s.bench.set_vitals(WEB, Some(Vitals { cpu: Some(0.38), memory: 0.6, disk: Some((0.44, None)), uptime_secs: 3_200_000, down: None, up: None })));
    w.settle();
    // A click on the numbers: the server's Stats, with its other tools under what it is.
    w.click("status-vitals");
    assert_eq!(w.rt.state.bench.active().map(|t| t.kind), Some(TabKind::Stats));
    assert!(w.rt.find("stats-tunnels").is_some() && w.rt.find("stats-recipes").is_some());
    w.click("stats-tunnels");
    assert!(w.rt.find("tunnels-dialog").is_some());
    w.rt.update(|s, tx| s.close_tunnels(tx));
    w.settle();
    w.click("stats-recipes");
    assert!(w.rt.state.screens.recipes.is_open());
    w.rt.update(|s, _| s.screens.close_all());
    w.settle();
    // A right click: the menu, whose "Hide vitals" turns Settings' switch off.
    let (x, y) = w.rt.find("status-vitals").unwrap();
    w.rt.secondary_down(x, y);
    w.rt.secondary_up(x, y);
    w.settle();
    assert!(w.rt.find("vitals-menu").is_some());
    w.click("vitals-hide");
    assert!(!w.rt.state.settings.get().show_vitals);
    assert!(w.rt.find("vitals-menu").is_none() && w.rt.find("status-vitals").is_none());
}

impl Window {
    /// The tooltip over `key`, once the pointer has rested on it.
    fn tip_of(&mut self, key: &str) -> [f32; 4] {
        let (x, y) = self.rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        self.rt.help_delay = 0.0;
        self.rt.pointer_move(x, y);
        for _ in 0..20 {
            self.rt.frame(0.05);
        }
        let tip = self.rt.help_tooltip().map(|(_, r)| r).unwrap_or_else(|| panic!("{key} at {x},{y} shows no tooltip; under it {:?}", self.rt.element_at(x, y).map(|p| p.path().to_owned())));
        self.rt.pointer_move(700.0, 400.0);
        self.settle();
        tip
    }
}

#[test]
fn the_sidebar_tooltips_open_beside_it_and_cover_no_other_button() {
    let mut w = Window::open(Frame::Mac, true);
    w.resize(900.0, 600.0);
    let buttons = ["home", "search", "add-server", "check", "tor", "settings-gear"];
    for key in buttons {
        let tip = w.tip_of(key);
        let b = w.rect(key).unwrap();
        assert!(tip[0] >= b[0] + b[2], "{key}'s tooltip at {tip:?} isn't beside it {b:?}");
        for other in buttons.iter().filter(|o| **o != key) {
            let r = w.rect(other).unwrap();
            let apart = tip[0] >= r[0] + r[2] || r[0] >= tip[0] + tip[2] || tip[1] >= r[1] + r[3] || r[1] >= tip[1] + tip[3];
            assert!(apart, "{key}'s tooltip {tip:?} covers {other} {r:?}");
        }
    }
    // The sidebar at the right: they open to its left.
    w.rt.update(|s, tx| s.change_settings(tx, |st| st.set(|x| x.sidebar_side = sshee_app::model::Side::Right)));
    // The sidebar slides across: let it land.
    for _ in 0..40 {
        w.rt.frame(0.05);
    }
    let b = w.rect("settings-gear").unwrap();
    let tip = w.tip_of("settings-gear");
    assert!(tip[0] + tip[2] <= b[0], "the tooltip at {tip:?} isn't left of the button {b:?}");
}

#[test]
fn the_main_menu_tooltip_in_the_bar_covers_no_sidebar_button() {
    let mut w = linux();
    w.resize(900.0, 600.0);
    let tip = w.tip_of("home");
    let search = w.rect("search").unwrap();
    assert!(tip[1] >= search[1] + search[3] || tip[1] + tip[3] <= search[1] || tip[0] >= search[0] + search[2], "{tip:?} covers search {search:?}");
}

#[test]
fn the_address_gives_way_before_the_tabs_crowd_and_neither_flips_back_at_the_edge() {
    use crate::shell::tabs::decide;
    // 1000 points between a 100-point title (150 more with its address) and the tabs.
    assert_eq!(decide(1000.0, 100.0, Some(150.0), 700.0, (false, true)), (false, true));
    // A tab more: the address goes first.
    assert_eq!(decide(1000.0, 100.0, Some(150.0), 760.0, (false, true)), (false, false));
    // A name a little shorter doesn't bring it straight back.
    assert_eq!(decide(1000.0, 100.0, Some(150.0), 740.0, (false, false)), (false, false));
    assert_eq!(decide(1000.0, 100.0, Some(150.0), 700.0, (false, false)), (false, true));
    // Then the tabs crowd, and loosen only with room to spare.
    assert_eq!(decide(1000.0, 100.0, Some(150.0), 950.0, (false, false)), (true, false));
    assert_eq!(decide(1000.0, 100.0, Some(150.0), 890.0, (true, false)), (true, false));
    assert_eq!(decide(1000.0, 100.0, Some(150.0), 880.0, (true, false)), (false, false));
    // This computer has no address.
    assert_eq!(decide(1000.0, 100.0, None, 700.0, (false, false)), (false, false));
}

#[test]
fn the_plus_menu_shows_the_files_shortcut() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    // The key caps are drawn text in each row, as Settings has them.
    let caps = |row: &str| w.rt.elements().into_iter().filter(|e| e.path.contains(row)).filter_map(|e| e.text).collect::<Vec<_>>();
    assert!(caps("nt-terminal").iter().any(|t| t == "Ctrl+Shift+T"), "{:?}", caps("nt-terminal"));
    assert!(caps("nt-files").iter().any(|t| t == "Ctrl+Shift+O"), "{:?}", caps("nt-files"));
}
