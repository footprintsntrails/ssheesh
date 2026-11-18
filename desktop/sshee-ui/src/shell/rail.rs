//! The sidebar, as 0.10 had it: a rail of icons (the design's), or a list with the servers'
//! names, as wide as you drag it. Its edge facing the page drags: narrower than 120 points the
//! list folds into the rail, and the rail dragged out unfolds into the list; a double click on
//! the edge gives the list its first width. Settings keeps both (`sidebarMode`,
//! `sidebarWidth`), and Settings → Layout → Server list picks one. Hidden altogether with the
//! sidebar button, its shortcut and the palette (`Shell::toggle_sidebar`).
//!
//! ```text
//! rail   column, 52 pt (icons) or the list's width (names)
//! ├── home         the logo (on a Mac; elsewhere it is in the bar, above)
//! ├── search       the palette
//! ├── server-N     each machine: icon (and its name, its label, "needs you" in the list); a
//! │                mark while it connects, reconnects or couldn't connect (`health`); its load
//! │                (`load`, `load-line`) as Settings → Layout → Server load picks; in the
//! │                list, under the pointer, expand-N (its containers: `tree-N`) and new-on-N
//! ├── tabs-N       its tabs under it, when Settings → Layout → Where tabs go says so
//! │                (`screens::rail_tree::tabs`)
//! ├── add-server
//! ├── check, tor, settings-gear   at the foot (a row in the list)
//! └── rail-edge    drag to resize, double-click for the first width
//! ```

use rune::{col, el, row, spacer, text, Drag, El, Phase, Side, Tx};
use sshee_app::model::SidebarMode;
use sshee_app::palette::shortcut_label;
use sshee_app::workbench::layout::sidebar;
use sshee_app::workbench::shortcuts::Shortcut;
use sshee_app::workbench::{Server, TabKind, Tor};

use super::rail_load::{self, Load};
use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::kit::chip::{env_chip, env_color};
use crate::kit::rail::{rail_button, RailState};
use crate::kit::ringed_dot;
use crate::screens::rail_tree;
use crate::theme::{self, Frame, Theme};

/// The edge's room to grab.
const EDGE: f32 = 6.0;

/// A row of the list.
const LIST_ROW: f32 = 30.0;

/// A drag of the sidebar's edge, while it lasts.
#[derive(Default)]
pub struct RailDrag {
    /// The width when the drag began.
    from: f32,
    /// What the pointer asks for now: the list this wide, or `None`, the rail.
    now: Option<Option<f32>>,
}

impl Shell {
    /// The sidebar shows the servers' names (else only their icons).
    pub fn sidebar_wide(&self) -> bool {
        match self.rail.now {
            Some(now) => now.is_some(),
            None => self.settings.get().sidebar_mode == SidebarMode::Wide,
        }
    }

    /// The sidebar's width now: the rail's, or the list's (as it's being dragged).
    pub fn sidebar_width(&self) -> f32 {
        match self.rail.now {
            Some(Some(w)) => w,
            Some(None) => sidebar::RAIL,
            None if self.sidebar_wide() => sidebar::width(self.settings.get().sidebar_width),
            None => sidebar::RAIL,
        }
    }

    /// A press, a drag and a release on the sidebar's edge. The width follows the pointer and
    /// is kept once, on release.
    pub(crate) fn rail_drag(&mut self, d: &Drag, tx: &mut Tx) {
        match d.phase {
            Phase::Down if d.clicks >= 2 => {
                self.rail = RailDrag::default();
                self.change_settings(tx, |st| {
                    st.set(|s| {
                        s.sidebar_mode = SidebarMode::Wide;
                        s.sidebar_width = f64::from(sidebar::DEFAULT);
                    })
                });
            }
            Phase::Down => self.rail = RailDrag { from: self.sidebar_width(), now: None },
            Phase::Move => {
                // Toward the page widens it, from either side of the window.
                let dx = if self.sidebar_at_right() { -d.dx } else { d.dx };
                self.rail.now = Some(sidebar::dragged(self.rail.from + dx));
            }
            Phase::Up => {
                let Some(now) = self.rail.now.take().filter(|_| !d.cancelled()) else { return };
                self.change_settings(tx, |st| {
                    st.set(|s| match now {
                        Some(w) => {
                            s.sidebar_mode = SidebarMode::Wide;
                            s.sidebar_width = f64::from(w.round());
                        }
                        None => s.sidebar_mode = SidebarMode::Narrow,
                    })
                });
                // The terminal fits its new room and has the keyboard again.
                self.settle_in_place(tx);
            }
        }
        tx.instant();
    }
}

/// The logo: the main menu.
fn home(s: &Shell, t: &Theme) -> El {
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
        .child(s.app_logo(24.0, t))
}

/// The ink of a list row's icon: quiet, a step brighter on the server on screen.
fn list_ink(t: &Theme, selected: bool) -> rune::Color {
    if selected {
        t.l1
    } else {
        t.l2
    }
}

/// A row of the list: an icon (a drawing of the caller's: a server's own, `list_ink` for a
/// sign) and words. `selected`: the server on screen.
fn list_row(t: &Theme, glyph: El, name: &str, selected: bool) -> El {
    let mut r = row()
        .w_full()
        .h(LIST_ROW)
        .px(8.0)
        .gap(9.0)
        .items_center()
        .radius(theme::RADIUS)
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label(name)
        .child(glyph)
        .child(text(name.to_owned()).key("name").font(theme::TEXT).color(t.l1).single_line().truncate_fade().shrink());
    if selected {
        r = r.fill(t.selrail);
    }
    r
}

/// The sidebar.
pub(crate) fn rail(s: &Shell, t: &'static Theme) -> El {
    let wide = s.sidebar_wide();
    // Tooltips open beside the sidebar, over the page, so they never cover the next button.
    let side = tip_side(s);
    let width = s.sidebar_width();
    let mut rail = col().key("rail").w(width).h_full().gap(3.0).pt(2.0).pb(8.0);
    rail = if wide { rail.px(8.0) } else { rail.items_center() };
    let mac = s.options.frame == Frame::Mac;
    if mac {
        // Elsewhere the logo is in the bar, above this column.
        rail = rail.child(if wide { row().w_full().child(home(s, t).help_side(side)) } else { home(s, t).help_side(side) });
    }
    let search_help = with_keys(s, "Search", Shortcut::Palette);
    rail = rail.child(if wide {
        list_row(t, icon(Icon::Search, theme::ICON_RAIL, list_ink(t, false)), "Search", false)
            .key("search")
            .help(with_keys(s, "Search servers, tabs, files and commands", Shortcut::Palette))
            .help_side(side)
            .on_press_tx(|s: &mut Shell, tx| s.open_palette("", tx))
    } else {
        rail_button(t, Icon::Search, "Search", RailState::Plain, false).key("search").help(search_help).help_side(side).on_press_tx(|s: &mut Shell, tx| s.open_palette("", tx))
    });
    rail = if wide {
        rail.child(
            row().key("rail-head").w_full().h(26.0).mt(6.0).pl(8.0).items_center().child(text("Servers").font(theme::GROUP_TITLE).color(t.l2).single_line()).child(spacer()).child(
                el().key("add-server")
                    .size(22.0, 22.0)
                    .radius(theme::RADIUS_ROW)
                    .items_center()
                    .justify_center()
                    .hover(t.hov2)
                    .press(t.down)
                    .focusable(true)
                    .role("button")
                    .label_static("Add a server")
                    .help("Add a server")
                    .help_side(side)
                    .on_press_tx(|s: &mut Shell, tx| s.open_add_server(tx))
                    .child(icon(Icon::Plus, 14.0, t.l2on)),
            ),
        )
    } else {
        rail.child(el().key("rule").size(20.0, 1.0).mt(5.0).mb(5.0).fill(t.hair))
    };
    let current = s.bench.current_key();
    for (i, server) in s.bench.servers().iter().enumerate() {
        let key = server.key();
        let selected = current == Some(key.as_str());
        let menu = key.clone();
        let b = if wide { list_server(s, t, i, server, selected) } else { rail_server(s, t, server, selected) };
        let b = b
            .key(format!("server-{i}"))
            .on_press_tx(move |s: &mut Shell, tx| {
                s.screens.server_card.hide();
                // Shift+click: in a window of its own, as a link in a browser.
                if tx.mods().shift {
                    s.open_in_new_window(&key, tx);
                } else {
                    s.go_to(&key, tx);
                }
            })
            // Its card (`screens::server_card`) says more than a tooltip; a right click
            // opens its menu (`screens::server_menu`).
            .help("")
            .on_hover_change(move |s: &mut Shell, inside, tx| s.hover_server(i, inside, tx))
            .on_secondary_press(move |s: &mut Shell, at, tx| s.open_server_menu(&menu, (at.x, at.y), tx));
        rail = rail.child(b);
        // Its tabs, when they go under each server; then its containers, when its tree is open.
        if wide {
            rail = rail.children(rail_tree::tabs(s, t, i, &server.key(), TAB_BRANCH));
            rail = rail.children(rail_tree::tree(s, t, i, &server.key(), TREE_INDENT));
        } else {
            rail = rail.children(rail_tree::rail_tabs(s, t, i, &server.key()));
        }
    }
    if !wide {
        rail = rail.child(rail_button(t, Icon::Plus, "Add a server", RailState::Plain, false).key("add-server").help_side(side).on_press_tx(|s: &mut Shell, tx| s.open_add_server(tx)));
    }
    // New hosts in ~/.ssh/config: a quiet note, not a dialog (0.10's `ConfigNotice`).
    let new = s.screens.home.config_new;
    if new > 0 {
        let words = format!("{new} new in ~/.ssh/config · Review");
        let note = if wide {
            list_row(t, icon(Icon::Doc, theme::ICON_RAIL, list_ink(t, false)), &words, false)
        } else {
            rail_button(t, Icon::Doc, &words, RailState::Plain, false).child(
                el().absolute(theme::RAIL_BUTTON - 15.0, 1.0).h(14.0).min_w(14.0).px(3.0).radius(7.0).fill(t.blue).items_center().justify_center().a11y_hidden().child(
                    rune::text(new.min(99).to_string()).font(theme::SMALL).color(t.on_color).single_line(),
                ),
            )
        };
        rail = rail.child(note.key("config-new").help(words.clone()).help_side(side).label(words).on_press_tx(|s: &mut Shell, tx| s.review_config(tx)));
    }
    let tor = match s.bench.tor() {
        Tor::Off => RailState::Plain,
        Tor::Starting => RailState::TorStarting,
        Tor::On { .. } => RailState::TorOn,
        Tor::Failed => RailState::TorFailed,
    };
    let foot = [
        rail_button(t, Icon::Shield, "Security check", if s.screens.checkup.is_open() { RailState::Selected } else { RailState::Plain }, false).key("check").on_press_tx(|s: &mut Shell, tx| s.open_checkup_here(tx)),
        rail_button(t, Icon::Drive, "Free up disk space", if s.screens.disk.is_open() { RailState::Selected } else { RailState::Plain }, false).key("disk").on_press_tx(|s: &mut Shell, tx| s.open_disk_here(tx)),
        rail_button(t, Icon::Tor, &crate::screens::tor::button_help(s), tor, false)
            .key("tor")
            .on_press_tx(|s: &mut Shell, tx| s.tor_button(tx))
            .on_secondary_press(|s: &mut Shell, _, tx| s.toggle_tor(tx)),
        rail_button(t, Icon::Gear, "Settings", if s.screens.settings.open { RailState::Selected } else { RailState::Plain }, false)
            .key("settings-gear")
            .help(with_keys(s, "Settings", Shortcut::Settings))
            .on_press_tx(|s: &mut Shell, tx| s.toggle_settings(tx)),
    ];
    rail = rail.child(spacer());
    // In a row at the foot of the list, its tooltips open above it; in the rail, beside it.
    let foot = foot.map(|b| b.help_side(if wide { Side::Top } else { side }));
    rail = if wide { rail.child(row().key("rail-foot").gap(2.0).children(foot)) } else { rail.children(foot) };
    // The edge facing the page: drag to resize, double-click for the first width.
    let right = s.sidebar_at_right();
    let edge = el()
        .key("rail-edge")
        .w(EDGE)
        .cursor(rune::Cursor::ResizeColumn)
        .role("separator")
        .label_static("Drag to resize the sidebar; double-click for its first width")
        .help(if wide { "Drag to resize. Narrower folds it into icons" } else { "Drag to show the servers' names" })
        .on_drag(|s: &mut Shell, d, tx| s.rail_drag(d, tx));
    let edge = if right { edge.pin(Some(0.0), Some(0.0), None, Some(0.0)) } else { edge.pin(None, Some(0.0), Some(0.0), Some(0.0)) };
    rail.child(edge)
}

/// "Search (Ctrl+Shift+L)": a tooltip with the shortcut that does the same, when it has one.
pub(crate) fn with_keys(s: &Shell, words: &str, shortcut: Shortcut) -> String {
    match shortcut_label(shortcut, s.options.mac_keys, &s.options.shortcuts) {
        Some(keys) => format!("{words} ({keys})"),
        None => words.to_owned(),
    }
}

/// Where a server's containers start in the list: under its name.
const TREE_INDENT: f32 = 40.0;

/// The chevron's room at the start of a server's row in the list.
const CHEVRON: f32 = 14.0;

/// Where the line a server's tabs hang from runs: under the middle of its icon (after the
/// chevron, the row's gap and the icon's own margin).
const TAB_BRANCH: f32 = CHEVRON + 6.0 + 2.0 + theme::ICON_RAIL / 2.0;

/// A server's icon in the rail: its picture or icon (smaller beside its load), its label's
/// line under it, a mark while it connects or when it's in trouble, its load, the orange dot.
fn rail_server(s: &Shell, t: &Theme, server: &Server, selected: bool) -> El {
    let key = server.key();
    let name = s.server_shown(&key, &server.name).into_owned();
    // With its tabs under it, the tab that waits has the dot (0.10's).
    let waiting = s.bench.server_needs_you(&key) && !s.bench.tabs_under_servers();
    let state = if selected { RailState::Selected } else { RailState::Plain };
    let load = rail_load::load(s, &key);
    let style = load.as_ref().map(|(style, _)| style);
    let size = rail_load::icon_size(style);
    let (dx, dy) = rail_load::icon_shift(style);
    // Its picture, else its icon in its own colour (`screens::server_menu::glyph`).
    let drawn = crate::screens::server_menu::glyph(s, server, size, crate::kit::rail::rail_ink(t, state)).mr(-2.0 * dx).mb(-2.0 * dy);
    let mut b = crate::kit::rail::rail_button_with(t, drawn, &name, state, waiting);
    let edge = theme::RAIL_BUTTON;
    if let Some((style, load)) = &load {
        b = b.children(rail_load::on_icon(t, style, load, edge, s.bench.tabs_on(&key).count(), waiting));
    }
    // Production, Staging, Development: a short line under the icon, in the label's colour.
    if let Some(env) = &server.env {
        b = b.child(el().key("env-line").absolute((edge - 14.0) / 2.0, edge - 4.0).size(14.0, 3.0).radius(1.5).fill(env_color(t, env)).a11y_hidden());
    }
    let health = s.bench.health(&key);
    if let Some(mark) = rail_load::health_mark(t, health, 8.0) {
        b = b.child(mark.absolute(edge - 13.0, edge - 13.0));
    }
    b
}

/// A server's row in the list: the chevron to its containers (under the pointer, or while
/// they show), its icon and name, "needs you", its label, the mark of its state, the "+" for a
/// new terminal there (under the pointer); its CPU and RAM under it while it's connected.
fn list_server(s: &Shell, t: &'static Theme, i: usize, server: &Server, selected: bool) -> El {
    let key = server.key();
    let name = s.server_shown(&key, &server.name).into_owned();
    // With its tabs under it, the tab that waits says so (0.10's).
    let waiting = s.bench.server_needs_you(&key) && !s.bench.tabs_under_servers();
    let under = s.screens.server_card.row == Some(i);
    let open = rail_tree::expanded(s, &key);
    let health = s.bench.health(&key);

    let mut line = row().w_full().h(LIST_ROW).pr(4.0).items_center();
    // The chevron: shown under the pointer and while the tree is open; its room is kept.
    let chevron = if under || open {
        let there = key.clone();
        el().key(format!("expand-{i}"))
            .size(CHEVRON, LIST_ROW)
            .items_center()
            .justify_center()
            .focusable(true)
            .role("button")
            .label(if open { format!("Hide containers on {name}") } else { format!("Show containers on {name}") })
            .expanded(open)
            .help(if open { "Hide containers" } else { "Show containers" })
            .on_press_tx(move |s: &mut Shell, tx| s.toggle_containers(&there, tx))
            .child(icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }, 10.0, t.l2))
    } else {
        el().size(CHEVRON, LIST_ROW)
    };
    // Its picture, else its icon in the colour you picked, else quiet, as in the rail.
    let drawn = crate::screens::server_menu::glyph(s, server, theme::ICON_RAIL, list_ink(t, selected));
    line = line
        .child(chevron)
        .child(row().grow(1.0).shrink().min_w(0.0).ml(2.0).gap(9.0).items_center().child(drawn).child(text(name.clone()).key("name").font(theme::TEXT).color(t.l1).single_line().truncate_fade().shrink()))
        .gap(6.0);
    if waiting {
        line = line.child(ringed_dot(8.0, 2.0, t.orange_dot, t.frame).key("waits").label_static("Needs you"));
    }
    if let Some(env) = &server.env {
        line = line.child(env_chip(t, env));
    }
    line = line.children(rail_load::health_mark(t, health, 8.0));
    if under {
        let there = key.clone();
        line = line.child(
            el().key(format!("new-on-{i}"))
                .size(20.0, 20.0)
                .radius(theme::RADIUS_ROW)
                .items_center()
                .justify_center()
                .hover(t.hov)
                .press(t.down)
                .focusable(true)
                .role("button")
                .label(format!("New terminal on {name}"))
                .help(format!("New terminal on {name}"))
                .on_press_tx(move |s: &mut Shell, tx| {
                    s.screens.server_card.hide();
                    s.bench.open(TabKind::Terminal, &there);
                    s.settle(tx);
                })
                .child(icon(Icon::Plus, 13.0, t.l2on)),
        );
    }
    let mut e = col()
        .w_full()
        .radius(theme::RADIUS)
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label(match rail_load::health_label(health) {
            Some(state) => format!("{name}, {}", state.to_lowercase()),
            None => name.clone(),
        })
        .child(line);
    if let Some((_, load)) = rail_load::load(s, &key) {
        e = e.child(load_line(t, &load));
    }
    // The server on screen is lit; with its tabs under it, the tab you're on is instead.
    if selected && !s.bench.tabs_under_servers() {
        e = e.fill(t.selrail);
    }
    e
}

/// CPU and RAM under a server's name, lined up with the name.
fn load_line(t: &Theme, load: &Load) -> El {
    rail_load::under_name(t, load, CHEVRON + 2.0 + theme::ICON_RAIL + 9.0)
}

/// The side a sidebar button's tooltip opens on: away from the window's edge, over the page.
pub(crate) fn tip_side(s: &Shell) -> Side {
    if s.sidebar_at_right() {
        Side::Left
    } else {
        Side::Right
    }
}
