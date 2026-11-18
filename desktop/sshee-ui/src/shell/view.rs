//! The window's elements (the 1.0 design, research_notes/v1-plan/fresh-apple.html):
//!
//! ```text
//! root   column, the frame's colour
//! ├── bar      44 pt, moves the window: [window buttons' room | logo] [sidebar toggle]
//! │            [bar-middle: title (name, address, Prod), tabs … +] [the sidebar's foot,
//! │            while it is hidden] [room for Windows' buttons]
//! └── body     row
//!     ├── rail     52 pt of icons: [logo] search ─ this computer, servers, + … check, Tor, settings
//!     └── main     column
//!         ├── page     the tab on screen: a terminal pane, or a placeholder; tabs split
//!         │            right and down (`split.rs`)
//!         └── status   24 pt: CPU, Memory, Disk, Uptime, Down, Up … Tor; a click on the
//!                      numbers opens Stats, a right click its menu (vitals-menu: Open full
//!                      stats, Hide vitals)
//! ```
//!
//! The row of tabs is `tabs.rs`; its menu and the list of all tabs are drawn last, over the
//! window. Names are drawn through `Shell::server_shown` (streamer mode, `streamer.rs`).
//!
//! Element keys are fixed words and tab numbers, never a server's name: Rune's traces carry
//! element paths.

use rune::{col, el, row, spacer, Cx, El};
use sshee_app::model::EnvLabel;
use sshee_app::workbench::{TabId, TabKind, Tor};

use super::state::Shell;
use crate::icons::Icon;
use crate::kit::button::toolbar_button;
use crate::kit::status::{status_item, status_tor};
use crate::screens;
use crate::terminal::view::pane_view_with;
use crate::theme::{self, Frame, Theme};

/// The whole window.
pub fn view(s: &Shell, cx: &mut Cx<Shell>) -> El {
    // Dark or light as Settings says, else as the system is.
    // The theme Settings → Appearance picks, dark or light as it says, else as the system is.
    let t = s.theme(cx.env().system_dark);
    if !s.settings.get().animations {
        cx.reduced_motion(true);
    }
    // Settings → Appearance → Interface size (the terminal keeps its own: `apply_settings`).
    cx.zoom(s.ui_zoom());
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
        .child(body)
        // Settings → Layout → A line for Production and Staging.
        .children(env_stripe(s, t));
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
        // The status bar's menu.
        .children(vitals_menu(s, t, cx))
        // Files dropped on a server's terminal: which folder they go to (`file_drop.rs`).
        .children(super::file_drop::dialog(s, t))
        // A confirmation, over all of it (`notes.rs`).
        .children(super::notes::dialog_view(s, t))
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
                    // Beside it: under it, it would cover the sidebar's first button.
                    .help_side(rune::Side::Right)
                    .on_press_tx(|s: &mut Shell, tx| s.show_home(tx))
                    .child(s.app_logo(24.0, t)),
            ),
        ),
    };
    bar = bar.child(
        toolbar_button(t, "sidebar", Icon::Sidebar, if s.bench.sidebar_shown() { "Hide sidebar" } else { "Show sidebar" })
            .on_press_tx(|s: &mut Shell, tx| s.toggle_sidebar(tx)),
    );
    // The machine on screen is the window's title, with its address (`title.rs`); a click
    // opens its tools. Then the tabs: every machine's, in one row (`tabs.rs`). The two share
    // the room between the buttons, measured as one, and decide together who gives way.
    let fit = super::tabs::fit(s);
    let middle = row()
        .key("bar-middle")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .items_center()
        .on_layout(|s: &mut Shell, [_, _, w, _]| s.bar_room = Some(w))
        .child(super::title::title(s, t, active, fit.address))
        .child(super::tabs::strip(s, t, active, fit));
    // Then, while the sidebar is hidden, its foot. What's left of the row moves the window,
    // and Windows' buttons take its end.
    // The AI agents the terminal on screen can start (0.10's header buttons): a pane beside
    // another has its own header, where they are instead.
    let agents = s
        .bench
        .active_id()
        .filter(|id| !s.screens.settings.open && !s.bench.showing_home() && s.bench.layout().shown_with(*id).is_none())
        .and_then(|id| screens::prompts::agent_starters(s, id, theme::CONTROL, t));
    bar.child(middle).children(agents.map(|a| a.ml(6.0))).children(super::title::bar_tools(s, t)).child(el().key("bar-end").w(10.0 + insets.trailing).h_full())
}

/// The page: darker than the frame, its top-left corner rounded where it meets the sidebar.
fn page(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    let rounded = s.bench.sidebar_shown() && !s.sidebar_away();
    // The page's corner is round where it meets the sidebar, and its edge runs down that side.
    let right = s.sidebar_at_right();
    // Settings → Layout → Corner roundness (0.10's `paneRadius`).
    let round = if rounded { s.page_radius() } else { 0.0 };
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
                let pane = col()
                    .key(format!("pane-{}", id.0))
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .child(pane_view_with(pane, cx, move |s: &mut Shell| s.panes.get_mut(&id), |s: &mut Shell, ev, tx| s.shortcut(ev, tx)))
                    // A program's progress, along the top of its terminal (under the top row).
                    .children(super::pane_marks::progress_line(s, id, t));
                // Files dropped on it from another app.
                super::file_drop::terminal(s, t, id, pane)
            }
            // A terminal whose pane isn't made yet draws next frame.
            None if tab.kind == TabKind::Terminal => el().key("pane-pending").w_full().h_full(),
            None if tab.kind == TabKind::Files => screens::files::view(s, tab, t, cx),
            None if tab.kind == TabKind::Containers => screens::containers::view(s, tab, t, cx),
            None => screens::stats::view(s, tab, t, cx),
        },
        None => screens::home::view(s, t, cx),
    };
    // Privacy settings turned on as a precaution: said across the top of the page.
    let content = match screens::settings::notice_bar(s, t) {
        Some(notice) => col().key("page-with-notice").w_full().h_full().child(notice).child(el().w_full().grow(1.0).min_h(0.0).child(content)),
        None => content,
    };
    // A newer test build is ready: said across the top of the page too.
    let content = screens::canary::with_bar(s, t, content);
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
        // A labelled server's page is framed in its colour, as 0.10 framed its panes:
        // Production red, Staging orange, Development green, faintly.
        .children(env_frame(s, t, corners))
}

/// The faint frame round the page of a labelled server (0.10's pane ring), not on the main
/// menu or Settings.
fn env_frame(s: &Shell, t: &'static Theme, corners: [f32; 4]) -> Option<El> {
    if s.screens.settings.open || s.bench.active().is_none() {
        return None;
    }
    let (color, strength) = match s.env_label_on_screen()? {
        EnvLabel::Prod => (t.red, 0.4),
        EnvLabel::Staging => (t.orange, 0.35),
        EnvLabel::Dev => (t.green, 0.3),
        _ => return None,
    };
    Some(el().key("env-frame").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).corners(corners).border(1.0, color.alpha(strength)).a11y_hidden())
}

/// A thin red line along the top of the window while a Production server is on screen, an
/// orange one for Staging (0.10's warning colour), when Settings asks for it.
fn env_stripe(s: &Shell, t: &'static Theme) -> Option<El> {
    let color = match s.env_on_screen()? {
        EnvLabel::Prod => t.red,
        EnvLabel::Staging => t.orange,
        _ => return None,
    };
    Some(el().key("env-stripe").pin(Some(0.0), Some(0.0), Some(0.0), None).h(2.0).fill(color).a11y_hidden())
}

impl Shell {
    /// The status bar's numbers were clicked: the machine on screen's Stats.
    pub fn open_vitals_stats(&mut self, tx: &mut rune::Tx) {
        self.screens.vitals_menu = None;
        if let Some(server) = self.bench.current_key().map(str::to_owned) {
            self.show_tab(TabKind::Stats, &server, None, tx);
        }
    }

    /// "Hide vitals": Settings' "Server vitals" goes off.
    fn hide_vitals(&mut self, tx: &mut rune::Tx) {
        self.screens.vitals_menu = None;
        self.change_settings(tx, |st| st.set(|s| s.show_vitals = false));
        self.settle(tx);
    }
}

/// The status bar's menu, under the pointer that opened it.
fn vitals_menu(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    use crate::kit::menu::{context_menu, menu_row, RowState, ROW};
    let at = s.screens.vitals_menu?;
    let within = cx.layout_size("window").unwrap_or([super::SIZE.0, super::SIZE.1]);
    let rows = [
        menu_row(t, "Open full stats", None, RowState::Rest).key("vitals-stats").on_press_tx(|s: &mut Shell, tx| s.open_vitals_stats(tx)),
        menu_row(t, "Hide vitals", None, RowState::Rest).key("vitals-hide").on_press_tx(|s: &mut Shell, tx| s.hide_vitals(tx)),
    ];
    // Opened from the status bar: it goes up from the pointer, clear of the bar.
    let height = 2.0 * ROW + 2.0 * crate::kit::menu::PAD;
    let at = [at[0], at[1] - height - theme::STATUS_HEIGHT / 2.0 - 2.0];
    Some(context_menu(t, at, within, 180.0, 2.0 * ROW, rows).key("vitals-menu").focusable(true).no_focus_ring().modal_tx(true, |s: &mut Shell, tx| {
        s.screens.vitals_menu = None;
        s.settle(tx);
    }))
}

/// How wide a vital sign is in the status bar (`kit::status::status_item`).
fn status_item_width(label: &str, value: &str, warn: bool) -> f32 {
    if warn {
        12.0 + 5.0 + rune::text::advance(&format!("{label} {value}"), theme::STATUS_WARN)
    } else {
        rune::text::advance(label, theme::STATUS) + 5.0 + rune::text::advance(value, theme::STATUS_VALUE)
    }
}

/// How wide Tor's readout is (`kit::status::status_tor`).
fn status_tor_width(latency_ms: Option<u32>) -> f32 {
    13.0 + 6.0 + rune::text::advance("Tor", theme::STATUS) + latency_ms.map_or(0.0, |ms| 6.0 + rune::text::advance(&format!("{ms} ms"), theme::STATUS_VALUE))
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
        .border_sides([1.0, 0.0, 0.0, 0.0], t.hair2)
        .on_layout(|s: &mut Shell, [_, _, w, _]| s.status_room = Some(w));
    if s.bench.sidebar_shown() && !s.sidebar_away() {
        // The page's edge against the sidebar runs on down the status bar.
        let edge = el().key("status-edge").w(1.0).fill(t.edge);
        bar = bar.child(if s.sidebar_at_right() { edge.pin(None, Some(0.0), Some(0.0), Some(0.0)) } else { edge.pin(Some(0.0), Some(0.0), None, Some(0.0)) });
    }
    let tor = match s.bench.tor() {
        Tor::On { latency_ms } => Some(latency_ms),
        _ => None,
    };
    let not_kept = super::pane_marks::status_not_kept(s, t);
    // Settings' "Server vitals" hides the numbers; Tor's stays. Where the bar is too narrow
    // for all of them, the last go first, and Tor's readout stays whole.
    if let Some(vitals) = s.bench.current_vitals().filter(|_| s.settings.get().show_vitals) {
        let mut room = s.status_room.map_or(f32::INFINITY, |w| w - 2.0 * 18.0);
        if let Some(latency) = tor {
            room -= status_tor_width(latency) + 2.0 * 20.0;
        }
        if s.streamer.on {
            room -= super::streamer::STATUS_WIDTH + 20.0;
        }
        // A click opens the machine's Stats (0.10's "Open full stats"); a right click, the
        // menu with "Hide vitals".
        let mut numbers = row()
            .key("status-vitals")
            .h_full()
            .gap(20.0)
            .px(6.0)
            .ml(-6.0)
            .items_center()
            .hover(t.hov2)
            .press(t.down)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label_static("Open full stats")
            .help("Open full stats. Right-click for more")
            .on_press_tx(|s: &mut Shell, tx| s.open_vitals_stats(tx))
            .on_secondary_press(|s: &mut Shell, p, tx| {
                s.screens.vitals_menu = Some([p.x, p.y]);
                tx.focus("vitals-menu", false);
                tx.instant();
            });
        if not_kept.is_some() {
            room -= super::pane_marks::status_width() + 20.0;
        }
        for (i, v) in vitals.items().iter().enumerate() {
            room -= status_item_width(&v.label, &v.value, v.warn) + if i > 0 { 20.0 } else { 0.0 };
            if room < 0.0 {
                break;
            }
            numbers = numbers.child(status_item(t, &v.label, &v.value, v.warn).key(format!("vital-{i}")));
        }
        bar = bar.child(numbers);
    }
    bar = bar.child(spacer());
    // The terminal on screen's shell isn't in tmux on the server (a pane beside another says
    // it in its own header).
    bar = bar.children(not_kept);
    // Streamer mode, while it's on.
    bar = bar.children(super::streamer::status(s, t));
    if let Some(latency_ms) = tor {
        bar = bar.child(status_tor(t, latency_ms).cursor(rune::Cursor::Pointer).on_press_tx(|s: &mut Shell, tx| s.toggle_tor(tx)));
    }
    bar
}
