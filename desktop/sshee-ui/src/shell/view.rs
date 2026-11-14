//! The window's elements (the 1.0 design, research_notes/v1-plan/fresh-apple.html):
//!
//! ```text
//! root   column, the frame's colour
//! ├── bar      44 pt, moves the window: [window buttons' room | logo] [sidebar toggle]
//! │            [title, Prod] [tabs … +] [streamer mode] [room for Windows' buttons]
//! └── body     row
//!     ├── rail     52 pt of icons: [logo] search ─ this computer, servers, + … check, Tor, settings
//!     └── main     column
//!         ├── page     the tab on screen: a terminal pane, or a placeholder; two side by
//!         │            side (`split.rs`)
//!         └── status   24 pt: CPU, Memory, Disk, Uptime, Down, Up … Tor
//! ```
//!
//! The row of tabs is `tabs.rs`; its menu and the list of all tabs are drawn last, over the
//! window. Names are drawn through `Shell::server_shown` (streamer mode, `streamer.rs`).
//!
//! Element keys are fixed words and tab numbers, never a server's name: Rune's traces carry
//! element paths.

use rune::{col, el, row, spacer, text, Cx, El};
use sshee_app::workbench::{TabId, TabKind, Tor};

use super::state::Shell;
use crate::icons::{logo, Icon};
use crate::kit::button::toolbar_button;
use crate::kit::chip::prod_chip;
use crate::kit::rail::{rail_button, RailState};
use crate::kit::status::{status_item, status_tor};
use crate::screens;
use crate::terminal::view::pane_view_with;
use crate::theme::{self, Frame, Theme};

/// The whole window.
pub fn view(s: &Shell, cx: &mut Cx<Shell>) -> El {
    let t = Theme::of(cx.env().system_dark);
    // The keyboard's focus ring, for every control that takes focus.
    cx.focus_ring(theme::RING, 0.0, t.ring);
    // Windows' minimize, maximize and close, which Rune draws over the end of the bar.
    cx.caption_color(t.l2on);
    if s.options.sheet {
        return super::sheet::view(t);
    }
    let mut body = row().key("body").w_full().grow(1.0).min_h(0.0);
    if s.bench.sidebar_shown() {
        body = body.child(rail(s, t));
    }
    col()
        .key("window")
        .w_full()
        .h_full()
        .fill(t.frame)
        .on_paste_content(|s: &mut Shell, content, tx| s.paste(content, tx))
        .child(bar(s, t, cx))
        .child(body.child(col().key("main").grow(1.0).min_w(0.0).h_full().child(page(s, t, cx)).child(status_bar(s, t))))
        // Over the window: uploads and downloads in progress, then a Files tab's menu or question.
        .children(screens::transfers::view(s, t))
        .children(screens::files::overlay(s, t, cx))
        // Over the window: a tab's menu, the list of all tabs.
        .children(super::tabs::overlays(s, t))
        // Over everything: the palette, a server's card and menu, "Add a server".
        .children(screens::overlays(s, t, cx))
}

/// The top row.
fn bar(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    let insets = cx.chrome_insets();
    let active = s.window_focused;
    let mut bar = row().key("bar").w_full().h(theme::BAR_HEIGHT).items_center().window_drag();
    bar = match s.options.frame {
        // The traffic lights sit over the row's start; in full screen they're gone.
        Frame::Mac => bar.child(el().key("lights").w(if insets.leading > 0.0 { insets.leading.max(theme::LIGHTS_WIDTH) } else { 12.0 }).h_full()),
        // The logo opens the main menu, in the sidebar's column.
        Frame::Windows | Frame::Linux => bar.child(
            el().key("bar-logo").w(theme::RAIL_WIDTH).h_full().items_center().justify_center().child(
                el().key("home")
                    .size(theme::RAIL_BUTTON, theme::RAIL_BUTTON)
                    .radius(theme::RADIUS)
                    .items_center()
                    .justify_center()
                    .hover(t.hov2)
                    .press(t.down)
                    .focusable(true)
                    .role("button")
                    .label_static("Main menu")
                    .help("Main menu")
                    .on_press_tx(|s: &mut Shell, tx| s.show_home(tx))
                    .child(logo(24.0)),
            ),
        ),
    };
    bar = bar.child(
        toolbar_button(t, "sidebar", Icon::Sidebar, if s.bench.sidebar_shown() { "Hide sidebar" } else { "Show sidebar" })
            .on_press_tx(|s: &mut Shell, tx| s.toggle_sidebar(tx)),
    );
    // The machine on screen is the window's title (its alias in streamer mode).
    let (title, prod) = s.bench.window_title();
    let title = s.server_shown(s.bench.current_key().unwrap_or("local"), title);
    // A click on it opens the command palette under it, as a browser's address bar.
    let mut name = row()
        .key("title")
        .h(theme::CONTROL)
        .items_center()
        .gap(8.0)
        .ml(2.0)
        .mr(10.0)
        .px(6.0)
        .radius(theme::RADIUS)
        .hover(t.hov2)
        .press(t.down)
        .role("button")
        .help(s.text_shown(&sshee_app::palette::placeholder(&s.bench)).into_owned())
        .on_press_tx(|s: &mut Shell, tx| s.open_palette("", tx))
        .child(text(title).key("title-text").font(theme::TEXT_BOLD).color(if active { t.l1 } else { t.l2 }).single_line());
    if prod {
        name = name.child(prod_chip(t, false));
    }
    bar = bar.child(name);

    // The tabs: every machine's, in one row (`tabs.rs`); then streamer mode's switch.
    let tabs = super::tabs::strip(s, t, active);
    // What's left of the row moves the window, and Windows' buttons take its end.
    bar.child(tabs).child(super::streamer::eye(s, t)).child(el().key("bar-end").w(10.0 + insets.trailing).h_full())
}

/// The sidebar: icons only.
fn rail(s: &Shell, t: &'static Theme) -> El {
    let mut rail = col().key("rail").w(theme::RAIL_WIDTH).h_full().items_center().gap(3.0).pt(2.0).pb(8.0);
    if s.options.frame == Frame::Mac {
        // Elsewhere the logo is in the bar, above this column.
        rail = rail.child(
            el().key("home")
                .size(theme::RAIL_BUTTON, theme::RAIL_BUTTON)
                .radius(theme::RADIUS)
                .items_center()
                .justify_center()
                .hover(t.hov2)
                .press(t.down)
                .focusable(true)
                .role("button")
                .label_static("Main menu")
                .help("Main menu")
                .on_press_tx(|s: &mut Shell, tx| s.show_home(tx))
                .child(logo(24.0)),
        );
    }
    rail = rail
        .child(rail_button(t, Icon::Search, "Search", RailState::Plain, false).key("search").on_press_tx(|s: &mut Shell, tx| s.open_palette("", tx)))
        .child(el().key("rule").size(20.0, 1.0).mt(5.0).mb(5.0).fill(t.hair));
    let current = s.bench.current_key();
    for (i, server) in s.bench.servers().iter().enumerate() {
        let key = server.key();
        let state = if current == Some(key.as_str()) { RailState::Selected } else { RailState::Plain };
        let waiting = s.bench.server_needs_you(&key);
        let menu = key.clone();
        rail = rail.child(
            rail_button(t, Icon::for_server(&server.icon), &s.server_shown(&key, &server.name), state, waiting)
                .key(format!("server-{i}"))
                .on_press_tx(move |s: &mut Shell, tx| {
                    s.screens.server_card.hide();
                    s.go_to(&key, tx)
                })
                // Its card (`screens::server_card`) says more than a tooltip; a right click
                // opens its menu (`screens::server_menu`).
                .help("")
                .on_hover_change(move |s: &mut Shell, inside, tx| s.hover_server(i, inside, tx))
                .on_secondary_press(move |s: &mut Shell, at, tx| s.open_server_menu(&menu, (at.x, at.y), tx)),
        );
    }
    let tor = match s.bench.tor() {
        Tor::Off => RailState::Plain,
        Tor::Starting => RailState::TorStarting,
        Tor::On { .. } => RailState::TorOn,
        Tor::Failed => RailState::TorFailed,
    };
    rail.child(rail_button(t, Icon::Plus, "Add a server", RailState::Plain, false).key("add-server").on_press_tx(|s: &mut Shell, tx| s.open_add_server(tx)))
        .child(spacer())
        .child(rail_button(t, Icon::Shield, "Security check", RailState::Plain, false).key("check"))
        .child(rail_button(t, Icon::Tor, "Tor", tor, false).key("tor"))
        .child(rail_button(t, Icon::Gear, "Settings", RailState::Plain, false).key("settings"))
}

/// The page: darker than the frame, its top-left corner rounded where it meets the sidebar.
fn page(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    let rounded = s.bench.sidebar_shown();
    let corners = [if rounded { theme::PAGE_RADIUS } else { 0.0 }, 0.0, 0.0, 0.0];
    let content = match s.bench.active() {
        // Shown beside another tab: both, with a divider (`split.rs`).
        Some(_) if s.pair_on_screen().is_some() => super::split::page(s, t, cx).unwrap_or_else(el),
        Some(tab) => match s.pane(tab.id) {
            // Each tab's pane under its own key: a tab switched to is a new element, so its
            // first layout is told to it (and opens its session).
            Some(pane) => {
                let id: TabId = tab.id;
                col().key(format!("pane-{}", id.0)).w_full().grow(1.0).min_h(0.0).child(pane_view_with(
                    pane,
                    cx,
                    move |s: &mut Shell| s.panes.get_mut(&id),
                    |s: &mut Shell, ev, tx| s.shortcut(ev, tx),
                ))
            }
            // A terminal whose pane isn't made yet draws next frame.
            None if tab.kind == TabKind::Terminal => el().key("pane-pending").w_full().h_full(),
            None if tab.kind == TabKind::Files => screens::files::view(s, tab, t, cx),
            None => screens::stats::view(s, tab, t, cx),
        },
        None => screens::home::view(s, t, cx),
    };
    el().key("page")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(t.page)
        .corners(corners)
        .clip_corners(corners)
        .child(content)
        // The line where the page meets the frame, drawn over the page's content: as padding
        // it would push the terminal's grid off whole pixels at 1.25x and 1.75x.
        .child(el().key("page-edge").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).corners(corners).border_sides([1.0, 0.0, 0.0, if rounded { 1.0 } else { 0.0 }], t.edge))
}

/// The status bar: the machine on screen's vital signs, and Tor at the right while it's on.
fn status_bar(s: &Shell, t: &'static Theme) -> El {
    let mut bar = row()
        .key("status")
        .w_full()
        .h(theme::STATUS_HEIGHT)
        .items_center()
        .gap(20.0)
        .px(18.0)
        .fill(t.page)
        .border_sides([1.0, 0.0, 0.0, 0.0], t.hair2);
    if s.bench.sidebar_shown() {
        // The page's edge against the sidebar runs on down the status bar.
        bar = bar.child(el().key("status-edge").absolute(0.0, 0.0).w(1.0).h_full().fill(t.edge));
    }
    if let Some(vitals) = s.bench.current_vitals() {
        for (i, v) in vitals.items().iter().enumerate() {
            bar = bar.child(status_item(t, &v.label, &v.value, v.warn).key(format!("vital-{i}")));
        }
    }
    bar = bar.child(spacer());
    if let Tor::On { latency_ms } = s.bench.tor() {
        bar = bar.child(status_tor(t, latency_ms));
    }
    bar
}
