//! The component sheet (`ssheesh-preview --sheet`): every component of the kit in its states
//! on one page, in the frame's colours, to compare with the design's states sheet and to see
//! what a change to a token or a component does everywhere at once. Hover, press and the
//! focus ring show under the pointer and the Tab key; nothing here does anything.

use rune::{col, row, text, El};
use sshee_app::workbench::Badge;

use super::state::Shell;
use crate::icons::{icon, logo, Icon};
use crate::kit::button::toolbar_button;
use crate::kit::chip::prod_chip;
use crate::kit::menu::{menu, menu_row, RowState};
use crate::kit::popover::{popover, popover_foot};
use crate::kit::rail::{rail_button, RailState};
use crate::kit::status::{status_item, status_tor};
use crate::kit::tab::{group_name, tab, Tab};
use crate::kit::divider;
use crate::theme::{self, Theme};

/// One labelled row of the sheet.
fn line(t: &Theme, key: &'static str, label: &str, items: Vec<El>) -> El {
    row()
        .key(key)
        .w_full()
        .items_center()
        .gap(14.0)
        .py(10.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair)
        .child(text(label).font(theme::NOTE).color(t.l2).single_line().w(130.0))
        .children(items)
}

fn sample_tab(t: &Theme, key: &'static str, spec: Tab) -> El {
    tab(t, &spec, |_: &mut Shell, _| {}).key(key).on_press_tx(|_: &mut Shell, _| {})
}

pub fn view(t: &'static Theme) -> El {
    let base = Tab { icon: Icon::Folder, name: "Files", server: Some("web-7731"), prod: false, badge: None, active: false, window_active: true, compact: false };
    let term = Tab { icon: Icon::Terminal, name: "~/shop", server: Some("web-staging"), ..base.clone() };
    let tabs = vec![
        sample_tab(t, "t-rest", base.clone()),
        sample_tab(t, "t-on", Tab { active: true, ..base.clone() }),
        sample_tab(t, "t-off", Tab { active: true, window_active: false, ..base.clone() }),
        sample_tab(t, "t-wait", Tab { icon: Icon::Agent, name: "Migration 0042", server: Some("db-main"), prod: true, badge: Some(Badge::NeedsYou), ..base.clone() }),
        sample_tab(t, "t-re", Tab { badge: Some(Badge::Reconnecting), ..term.clone() }),
        sample_tab(t, "t-gone", Tab { badge: Some(Badge::Disconnected), ..term.clone() }),
    ];
    let crowded = vec![
        group_name(t, "web-7731", true, true).key("g-1"),
        sample_tab(t, "c-on", Tab { server: None, compact: true, active: true, ..term.clone() }),
        sample_tab(t, "c-files", Tab { server: None, compact: true, ..base.clone() }),
        divider(t).key("c-sep"),
        group_name(t, "db-main", true, false).key("g-2"),
        sample_tab(t, "c-wait", Tab { icon: Icon::Agent, name: "Migration 0042", server: None, compact: true, badge: Some(Badge::NeedsYou), ..base.clone() }),
    ];
    let rail = vec![
        rail_button(t, Icon::Database, "db-main", RailState::Plain, false).key("r-rest"),
        rail_button(t, Icon::Database, "db-main", RailState::Selected, false).key("r-sel"),
        rail_button(t, Icon::Database, "db-main", RailState::Plain, true).key("r-wait"),
        rail_button(t, Icon::Tor, "Tor is off", RailState::Plain, false).key("r-tor-off"),
        rail_button(t, Icon::Tor, "Tor is starting", RailState::TorStarting, false).key("r-tor-start"),
        rail_button(t, Icon::Tor, "Tor is on", RailState::TorOn, false).key("r-tor-on"),
        rail_button(t, Icon::Tor, "Tor didn't start", RailState::TorFailed, false).key("r-tor-bad"),
    ];
    let buttons = vec![
        toolbar_button(t, "b-side", Icon::Sidebar, "Hide sidebar").on_press_tx(|_: &mut Shell, _| {}),
        toolbar_button(t, "b-plus", Icon::Plus, "New terminal").on_press_tx(|_: &mut Shell, _| {}),
        prod_chip(t, false),
        logo(24.0),
        logo(32.0),
        logo(56.0),
    ];
    let question = popover(t, 340.0)
        .key("pop")
        .child(menu(vec![
            menu_row(t, "Yes", Some("1"), RowState::Rest).key("m-1").on_press_tx(|_: &mut Shell, _| {}),
            menu_row(t, "Yes, and don't ask again for npm test", Some("↩ 2"), RowState::Hot).key("m-2"),
            menu_row(t, "No, and tell Claude what to do…", Some("3"), RowState::Down).key("m-3"),
        ]))
        .child(popover_foot(t, "Press 1, 2 or 3, or click an answer", "Esc to decide later"));
    let status = vec![
        status_item(t, "CPU", "38%", false).key("s-cpu"),
        status_item(t, "Memory", "60%", false).key("s-mem"),
        status_item(t, "Disk", "92%", true).key("s-disk"),
        status_item(t, "Down", "1.6 MB/s", false).key("s-down"),
        status_tor(t, Some(84)),
    ];
    let icons: Vec<El> = Icon::SIGNS.iter().map(|i| icon(*i, theme::ICON_RAIL, t.l2on)).collect();
    let server_icons: Vec<El> = sshee_app::labels::HOST_ICONS.iter().map(|(id, _)| icon(Icon::for_server(id), theme::ICON_RAIL, t.l2on)).collect();
    col()
        .key("sheet")
        .w_full()
        .h_full()
        .fill(t.frame)
        .px(24.0)
        .py(12.0)
        .child(line(t, "l-tabs", "Tab", tabs))
        .child(line(t, "l-crowded", "Crowded row", crowded))
        .child(line(t, "l-rail", "Sidebar button", rail))
        .child(line(t, "l-buttons", "Button, chip, logo", buttons))
        .child(line(t, "l-menu", "Popover and menu", vec![question]))
        .child(line(t, "l-status", "Status bar", status))
        .child(line(t, "l-icons", "Icons", icons))
        .child(line(t, "l-server-icons", "Server icons", server_icons))
}
