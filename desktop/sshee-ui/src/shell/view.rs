//! The window's elements (the 1.0 design, research_notes/v1-plan/fresh-apple.html):
//!
//! ```text
//! root   column, the frame's colour
//! ├── bar      44 pt, moves the window: [window buttons' room | logo] [sidebar toggle]
//! │            [title: name, address, Prod] [tabs … +] [streamer mode] [the sidebar's foot,
//! │            while it is hidden] [room for Windows' buttons]
//! └── body     row
//!     ├── rail     52 pt of icons: [logo] search ─ this computer, servers, + … check, Tor, settings
//!     └── main     column
//!         ├── page     the tab on screen: a terminal pane, or a placeholder; tabs split
//!         │            right and down (`split.rs`)
//!         └── status   24 pt: CPU, Memory, Disk, Uptime, Down, Up … Tor
//! ```
//!
//! The row of tabs is `tabs.rs`; its menu and the list of all tabs are drawn last, over the
//! window. Names are drawn through `Shell::server_shown` (streamer mode, `streamer.rs`).
//!
//! Element keys are fixed words and tab numbers, never a server's name: Rune's traces carry
//! element paths.

use rune::{col, el, row, spacer, Cx, El};
use sshee_app::workbench::{TabId, TabKind, Tor};

use super::state::Shell;
use crate::icons::{logo, Icon};
use crate::kit::button::toolbar_button;
use crate::kit::status::{status_item, status_tor};
use crate::screens;
use crate::terminal::view::pane_view_with;
use crate::theme::{self, Frame, Theme};

/// The whole window.
pub fn view(s: &Shell, cx: &mut Cx<Shell>) -> El {
    // Dark or light as Settings says, else as the system is.
    let t = Theme::of(s.dark(cx.env().system_dark));
    if !s.settings.get().animations {
        cx.reduced_motion(true);
    }
    // The keyboard's focus ring, for every control that takes focus.
    cx.focus_ring(theme::RING, 0.0, t.ring);
    // Windows' minimize, maximize and close, which Rune draws over the end of the bar.
    cx.caption_color(t.l2on);
    if s.options.sheet {
        return super::sheet::view(t);
    }
    let mut body = row().key("body").w_full().grow(1.0).min_h(0.0);
    let mut main = col().key("main").grow(1.0).min_w(0.0).h_full().child(page(s, t, cx));
    // Compact mode "Both" hides the status bar too.
    if !(s.bar_away() && s.sidebar_away()) {
        main = main.child(status_bar(s, t));
    }
    // The sidebar at the side Settings puts it; in compact mode it waits at the edge.
    let shown = s.bench.sidebar_shown();
    let docked = shown && !s.sidebar_away();
    body = match (docked, s.sidebar_at_right()) {
        (true, false) => body.child(super::rail::rail(s, t)).child(main),
        (true, true) => body.child(main).child(super::rail::rail(s, t)),
        (false, _) => body.child(main),
    };
    let away_bar = s.bar_away();
    let mut window = col()
        .key("window")
        .w_full()
        .h_full()
        .fill(t.frame)
        .on_paste_content(|s: &mut Shell, content, tx| s.paste(content, tx))
        .child(if away_bar { super::compact::top_band(s, t) } else { bar(s, t, cx) })
        .child(body);
    if shown && s.sidebar_away() {
        window = window.child(super::compact::side_band(s));
    }
    let out_bar = (away_bar && s.compact.top).then(|| bar(s, t, cx));
    let out_rail = (shown && s.sidebar_away() && s.compact.side).then(|| super::rail::rail(s, t));
    window
        .children(super::compact::out(s, t, out_bar, out_rail))
        // Over the window: uploads and downloads in progress, then a Files tab's menu or question.
        .children(screens::transfers::view(s, t))
        .children(screens::files::overlay(s, t, cx))
        // Files open in an editor elsewhere: a question, a conflict, copies left unsent.
        .children(screens::files::edit::overlay(s, t))
        // Over the window: a tab's menu, the list of all tabs.
        .children(super::tabs::overlays(s, t))
        // Over everything: the palette, a server's card and menu, "Add a server".
        .children(screens::overlays(s, t, cx))
        // A tab being dragged over the page: where it would land.
        .children(super::split::drop_mark(s, t))
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
    // The machine on screen is the window's title, with its address (`title.rs`); a click
    // opens its tools.
    bar = bar.child(super::title::title(s, t, active));

    // The tabs: every machine's, in one row (`tabs.rs`); then streamer mode's switch, and
    // while the sidebar is hidden, its foot.
    let tabs = super::tabs::strip(s, t, active);
    // What's left of the row moves the window, and Windows' buttons take its end.
    bar.child(tabs).child(super::streamer::eye(s, t)).children(super::title::bar_tools(s, t)).child(el().key("bar-end").w(10.0 + insets.trailing).h_full())
}

/// The page: darker than the frame, its top-left corner rounded where it meets the sidebar.
fn page(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    let rounded = s.bench.sidebar_shown() && !s.sidebar_away();
    // The page's corner is round where it meets the sidebar, and its edge runs down that side.
    let right = s.sidebar_at_right();
    let round = if rounded { theme::PAGE_RADIUS } else { 0.0 };
    let corners = if right { [0.0, round, 0.0, 0.0] } else { [round, 0.0, 0.0, 0.0] };
    let side = if rounded { 1.0 } else { 0.0 };
    let edges = if right { [1.0, side, 0.0, 0.0] } else { [1.0, 0.0, 0.0, side] };
    let content = match s.bench.active() {
        // Settings takes the page while it is open.
        _ if s.screens.settings.open => screens::settings::view(s, t, cx),
        // Sharing the page with other tabs: all of them, with dividers (`split.rs`).
        Some(tab) if s.bench.layout().shown_with(tab.id).is_some() => super::split::page(s, t, cx).unwrap_or_else(el),
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
    // Privacy settings turned on as a precaution: said across the top of the page.
    let content = match screens::settings::notice_bar(s, t) {
        Some(notice) => col().key("page-with-notice").w_full().h_full().child(notice).child(el().w_full().grow(1.0).min_h(0.0).child(content)),
        None => content,
    };
    el().key("page")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        // A dragged tab lands on the page by where it is (`split.rs`).
        .on_layout_tx(|s: &mut Shell, rect, tx| {
            s.sides.page = Some(rect);
            tx.live_only();
        })
        .fill(t.page)
        .corners(corners)
        .clip_corners(corners)
        .child(content)
        // The line where the page meets the frame, drawn over the page's content: as padding
        // it would push the terminal's grid off whole pixels at 1.25x and 1.75x.
        .child(el().key("page-edge").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).corners(corners).border_sides(edges, t.edge))
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
    if s.bench.sidebar_shown() && !s.sidebar_away() {
        // The page's edge against the sidebar runs on down the status bar.
        let edge = el().key("status-edge").w(1.0).fill(t.edge);
        bar = bar.child(if s.sidebar_at_right() { edge.pin(None, Some(0.0), Some(0.0), Some(0.0)) } else { edge.pin(Some(0.0), Some(0.0), None, Some(0.0)) });
    }
    // Settings' "Server vitals" hides the numbers; Tor's stays.
    if let Some(vitals) = s.bench.current_vitals().filter(|_| s.settings.get().show_vitals) {
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
