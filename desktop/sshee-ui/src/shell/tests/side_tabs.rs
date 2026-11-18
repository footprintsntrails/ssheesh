//! Settings → Layout → Where tabs go: "Under each server" (0.10's `SidebarTabs`). Each machine's
//! tabs hang under it in the sidebar, the row along the top keeps only "+", and going through
//! tabs or closing them stays with the machine on screen.

use rune::Key;
use sshee_app::model::{SidebarMode, TabsPlacement};

use super::{linux, Window, DB, WEB};

impl Window {
    fn tabs_placed(&mut self, placement: TabsPlacement) {
        self.rt.update(move |s, tx| s.change_settings(tx, |st| st.set(|x| x.tabs_placement = placement)));
        self.settle();
    }

    fn sidebar_mode(&mut self, mode: SidebarMode) {
        self.rt.update(move |s, tx| s.change_settings(tx, |st| st.set(|x| x.sidebar_mode = mode)));
        // The sidebar widens as it slides: let it land.
        for _ in 0..40 {
            self.rt.frame(0.05);
        }
        self.settle();
    }

    /// The key of the `n`th tab in the row's order.
    fn tab_at(&self, n: usize) -> String {
        format!("tab-{}", self.rt.state.bench.tabs()[n].id.0)
    }

    /// The tab's element is in the sidebar's column.
    fn in_sidebar(&self, key: &str) -> bool {
        let (rail, tab) = (self.rect("rail").unwrap(), self.rect(key).unwrap_or_else(|| panic!("no {key}")));
        tab[0] >= rail[0] && tab[0] + tab[2] <= rail[0] + rail[2] + 0.5 && tab[1] > 40.0
    }

    /// A press on `from`, moved to `to` in a few steps, let go there.
    fn drag_tab(&mut self, from: (f32, f32), to: (f32, f32)) {
        self.rt.pointer_down(from.0, from.1);
        self.rt.frame(1.0 / 60.0);
        for step in 1..=6 {
            let k = step as f32 / 6.0;
            self.rt.pointer_move(from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
            self.rt.frame(1.0 / 60.0);
        }
        self.rt.pointer_up(to.0, to.1);
        self.settle();
    }
}

#[test]
fn tabs_go_under_each_server_and_the_top_row_keeps_only_plus() {
    let mut w = linux();
    w.click("server-1");
    w.click("new-tab");
    w.click("nt-terminal");
    w.click("server-2");
    // Along the top, the first time: nothing hangs under the servers.
    assert!(w.rt.find("tabs-1").is_none());
    assert!(!w.in_sidebar(&w.tab_at(0)));
    w.tabs_placed(TabsPlacement::Sidebar);
    // In the rail of icons, each server's tabs as smaller icons under it; "+" stays on top.
    assert!(w.rt.find("tabs-1").is_some() && w.rt.find("tabs-2").is_some() && w.rt.find("tabs-0").is_none());
    for n in 0..3 {
        assert!(w.in_sidebar(&w.tab_at(n)), "{}", w.tab_at(n));
    }
    assert!(w.rt.find("new-tab").is_some());
    assert!(w.tree().contains("Tabs on web-7731") && w.tree().contains("Tabs on db-main"), "{}", w.tree());
    // In the list, a row each, under its server's name, in the row's order.
    w.sidebar_mode(SidebarMode::Wide);
    let (one, two, db) = (w.rect(&w.tab_at(0)).unwrap(), w.rect(&w.tab_at(1)).unwrap(), w.rect(&w.tab_at(2)).unwrap());
    let (web_row, db_row) = (w.rect("server-1").unwrap(), w.rect("server-2").unwrap());
    assert!(web_row[1] < one[1] && one[1] < two[1] && two[1] < db_row[1] && db_row[1] < db[1], "{one:?} {two:?} {db:?}");
    // Hanging from the server's icon, indented past it.
    assert!(one[0] > web_row[0] + 20.0, "{one:?} {web_row:?}");
    // A click goes there.
    let first = w.tab_at(0);
    w.click(&first);
    assert_eq!(w.row(), "*Terminal@web-7731 Terminal 2@web-7731 Terminal@db-main");
    // Back along the top.
    w.tabs_placed(TabsPlacement::Top);
    assert!(w.rt.find("tabs-1").is_none() && !w.in_sidebar(&w.tab_at(0)));
}

#[test]
fn a_tab_under_its_server_closes_by_middle_click_and_cross_and_its_menu_says_below() {
    let mut w = linux();
    w.tabs_placed(TabsPlacement::Sidebar);
    w.sidebar_mode(SidebarMode::Wide);
    w.click("server-1");
    w.click("new-tab");
    w.click("nt-terminal");
    w.click("new-tab");
    w.click("nt-terminal");
    assert_eq!(w.row(), "Terminal@web-7731 Terminal 2@web-7731 *Terminal 3@web-7731");
    // Its menu is the row's, its last line about the tabs below it.
    let first = w.tab_at(0);
    let (x, y) = w.rt.find(&first).unwrap();
    w.rt.secondary_down(x, y);
    w.rt.secondary_up(x, y);
    w.settle();
    assert!(w.rt.find("tab-menu").is_some());
    assert!(w.tree().contains("Close tabs below") && !w.tree().contains("Close tabs to the right"), "{}", w.tree());
    w.key(Key::Escape, "");
    // A middle click closes one without going to it.
    let second = w.tab_at(1);
    let (x, y) = w.rt.find(&second).unwrap();
    w.rt.button_down(x, y, rune::PointerButton::Middle);
    w.rt.button_up(x, y, rune::PointerButton::Middle);
    w.settle();
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 3@web-7731");
    assert!(w.rt.find(&second).is_none());
    // Its cross, under the pointer, closes it.
    let last = w.tab_at(1);
    let [x, y, tw, th] = w.rect(&last).unwrap();
    w.rt.pointer_move(x + tw - 12.0, y + th / 2.0);
    w.settle();
    w.click(&format!("{last}/close"));
    assert_eq!(w.row(), "*Terminal@web-7731");
}

#[test]
fn under_each_server_ctrl_tab_and_closing_stay_with_the_machine_on_screen() {
    let mut w = linux();
    w.tabs_placed(TabsPlacement::Sidebar);
    w.click("server-1");
    w.click("server-2");
    w.click("server-1");
    w.click("new-tab");
    w.click("nt-terminal");
    assert_eq!(w.row(), "Terminal@web-7731 *Terminal 2@web-7731 Terminal@db-main");
    // Around web-7731's two, never to db-main's.
    w.key(Key::Tab, "c");
    assert_eq!(w.row(), "*Terminal@web-7731 Terminal 2@web-7731 Terminal@db-main");
    w.key(Key::Tab, "c");
    w.key(Key::Tab, "c");
    assert_eq!(w.row(), "*Terminal@web-7731 Terminal 2@web-7731 Terminal@db-main");
    // A machine's last tab closed: the main menu, not the next machine's tab.
    w.click("server-2");
    w.key(Key::Command('w'), "cs");
    assert!(w.rt.state.bench.showing_home());
    let _ = (WEB, DB);
}

#[test]
fn a_tab_that_needs_you_says_so_under_its_server_instead_of_on_it() {
    let mut w = linux();
    w.tabs_placed(TabsPlacement::Sidebar);
    w.sidebar_mode(SidebarMode::Wide);
    w.click("server-1");
    w.click("server-2");
    let id = w.rt.state.bench.tabs()[0].id;
    w.rt.update(move |s, _| s.bench.set_needs_you(id, true));
    w.settle();
    let tab = w.tab_at(0);
    assert!(w.rt.find(&format!("{tab}/needs-you")).is_some());
    assert!(w.tree().contains("Terminal, Needs you"), "{}", w.tree());
    // The server's row doesn't say it twice.
    assert!(w.rt.find("server-1/waits").is_none());
    // Along the top, the server's row says it again.
    w.tabs_placed(TabsPlacement::Top);
    assert!(w.rt.find("server-1/waits").is_some());
}

#[test]
fn a_tab_dragged_down_its_servers_list_moves_there() {
    let mut w = linux();
    w.tabs_placed(TabsPlacement::Sidebar);
    w.sidebar_mode(SidebarMode::Wide);
    w.click("server-1");
    w.click("new-tab");
    w.click("nt-terminal");
    w.click("new-tab");
    w.click("nt-terminal");
    let first = w.tab_at(0);
    let [x, y, _, h] = w.rect(&first).unwrap();
    let [_, y3, _, h3] = w.rect(&w.tab_at(2)).unwrap();
    // Past the last one's middle: it goes last, and you're on it.
    w.drag_tab((x + 30.0, y + h / 2.0), (x + 30.0, y3 + h3 * 0.8));
    assert_eq!(w.row(), "Terminal 2@web-7731 Terminal 3@web-7731 *Terminal@web-7731");
    assert!(!w.rt.state.screens.rail_tree.dragging_tab());
}

#[test]
fn with_the_sidebar_hidden_the_tabs_run_along_the_top_again() {
    let mut w = linux();
    w.tabs_placed(TabsPlacement::Sidebar);
    w.click("server-1");
    assert!(w.in_sidebar(&w.tab_at(0)));
    w.click("sidebar");
    assert!(!w.rt.state.bench.sidebar_shown());
    let tab = w.rect(&w.tab_at(0)).unwrap();
    assert!(tab[1] < 40.0, "in the top row: {tab:?}");
    w.click("sidebar");
    assert!(w.in_sidebar(&w.tab_at(0)));
}
