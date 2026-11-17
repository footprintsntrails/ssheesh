//! The top row's title and, while the sidebar is hidden, the sidebar's foot in the top row.
//!
//! ```text
//! title           the machine on screen: its name, its address after it in a quieter tone
//! │               (neither in streamer mode: "Server 2"), "Prod"; a click opens its tools
//! └── title-menu  New terminal, Files, Copy address (0.10's AddressChip)
//! bar-tools       only while the sidebar is hidden: main menu (a Mac's logo lives in the
//!                 sidebar), security check, Tor, settings (0.10's top bar did the same)
//! ```
//!
//! On the main menu the title is the app's name and a click opens the command palette, as
//! before. Tunnels and Recipes join the menu when their screens land.

use rune::{row, text, Align, El, Key, KeyEvent, Side, Tx};
use sshee_app::palette::shortcut_label;
use sshee_app::settings_store::catalog::Section;
use sshee_app::workbench::shortcuts::Shortcut;
use sshee_app::workbench::{ServerKey, TabKind, Tor};

use super::state::Shell;
use crate::icons::{icon, Icon};
use crate::kit::chip::prod_chip;
use crate::kit::list::{list_row, list_rule, ListRow};
use crate::kit::menu::RowState;
use crate::kit::popover::popover;
use crate::kit::rail::RailState;
use crate::theme::{self, Frame, Theme};

const MENU_WIDTH: f32 = 248.0;

/// The title's menu, open on a machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TitleMenu {
    pub(crate) server: ServerKey,
    pub(crate) hot: Option<usize>,
}

/// A row of the title's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    NewTerminal,
    Files,
    CopyAddress,
}

/// A button of the sidebar's foot, drawn in the top row while the sidebar is hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FootTool {
    Home,
    Check,
    Tor,
    Settings,
}

impl Shell {
    /// The title's menu: rows that can be picked for `server`.
    fn title_tools(&self, server: &str) -> Vec<Vec<Tool>> {
        let mut groups = vec![vec![Tool::NewTerminal, Tool::Files]];
        // An address to copy: a server's, and not while streaming.
        if self.bench.server(server).is_some_and(|sv| !sv.is_local() && !sv.address.is_empty()) && !self.streaming() {
            groups.push(vec![Tool::CopyAddress]);
        }
        groups
    }

    /// A click on the title: the machine's tools, or on the main menu the palette.
    pub fn title_pressed(&mut self, tx: &mut Tx) {
        let Some(server) = self.bench.current_key().map(str::to_owned) else { return self.open_palette("", tx) };
        let open = self.row.title_menu.is_some();
        self.row.close_overlays();
        if open {
            return self.settle(tx);
        }
        self.row.title_menu = Some(TitleMenu { server, hot: None });
        tx.focus("title-menu", false);
        tx.instant();
    }

    fn title_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if self.streamer_key(ev, tx) {
            return true;
        }
        let Some(menu) = self.row.title_menu.clone() else { return false };
        let items = self.title_tools(&menu.server).concat();
        match ev.key {
            Key::Down | Key::Up => {
                let hot = sshee_app::palette::step(items.len(), menu.hot, if ev.key == Key::Down { 1 } else { -1 });
                if let Some(m) = self.row.title_menu.as_mut() {
                    m.hot = hot;
                }
                tx.instant();
            }
            Key::Enter => {
                if let Some(tool) = menu.hot.and_then(|h| items.get(h)).copied() {
                    self.title_pick(tool, tx);
                }
            }
            Key::Escape => self.close_tab_overlays(tx),
            _ => {}
        }
        true
    }

    fn title_pick(&mut self, tool: Tool, tx: &mut Tx) {
        let Some(menu) = self.row.title_menu.take() else { return };
        self.row.close_overlays();
        match tool {
            Tool::NewTerminal => {
                self.bench.open(TabKind::Terminal, &menu.server);
                self.settle(tx);
            }
            Tool::Files => {
                self.open_files_on(&menu.server, None, tx);
            }
            Tool::CopyAddress => {
                if let Some(address) = self.bench.server(&menu.server).map(|sv| sv.address.clone()) {
                    tx.copy(address);
                }
                self.settle(tx);
            }
        }
    }

    /// A button of the sidebar's foot, from the top row. Settings opens or closes; Tor opens
    /// its page in Settings until it has a screen of its own. The security check has no
    /// screen yet: the button is drawn, as the sidebar's is, and does what the sidebar's does
    /// once that lands.
    pub fn foot_tool(&mut self, tool: FootTool, tx: &mut Tx) {
        match tool {
            FootTool::Home => self.show_home(tx),
            FootTool::Settings => self.toggle_settings(tx),
            FootTool::Tor => self.open_settings_at(Section::Tor, tx),
            FootTool::Check => {}
        }
    }
}

/// The title: the machine on screen, its address, and "Prod".
pub(crate) fn title(s: &Shell, t: &'static Theme, active: bool) -> El {
    let (name, prod) = s.bench.window_title();
    let key = s.bench.current_key();
    let name = s.server_shown(key.unwrap_or("local"), name);
    // Its address after its name, quieter; none in streamer mode, for this computer, or while
    // the row of tabs is crowded (the tabs need the room more).
    let address = key.and_then(|k| s.bench.server(k)).filter(|sv| !sv.is_local() && !s.streaming() && !s.row.crowded()).map(|sv| sv.address.clone()).filter(|a| !a.is_empty());
    let help = match key {
        Some(_) => "New terminal, Files, copy the address".to_owned(),
        None => s.text_shown(&sshee_app::palette::placeholder(&s.bench)).into_owned(),
    };
    let label = match &address {
        Some(address) => format!("{name}, {address}"),
        None => name.clone().into_owned(),
    };
    let mut e = row()
        .key("title")
        .h(theme::CONTROL)
        .min_w(0.0)
        .shrink()
        .items_center()
        .gap(8.0)
        .ml(2.0)
        .mr(10.0)
        .px(6.0)
        .radius(theme::RADIUS)
        .press(t.down)
        .role("button")
        .label(label)
        .help(help)
        .on_press_tx(|s: &mut Shell, tx| s.title_pressed(tx))
        .child(text(name).key("title-text").font(theme::TEXT_BOLD).color(if active { t.l1 } else { t.l2 }).single_line());
    e = if s.row.title_menu.is_some() { e.fill(t.hov) } else { e.hover(t.hov2) };
    if let Some(address) = address {
        e = e.child(text(address).key("title-address").font(theme::TAB_SERVER).color(t.l2).single_line().truncate().shrink().min_w(0.0));
    }
    if prod {
        e = e.child(prod_chip(t, false));
    }
    e
}

/// The title's menu, under the title.
pub(crate) fn title_menu(s: &Shell, t: &'static Theme) -> Option<El> {
    let menu = s.row.title_menu.as_ref()?;
    let server = s.bench.server(&menu.server)?;
    let keys = shortcut_label(Shortcut::NewTerminal, s.options.mac_keys, &s.options.shortcuts);
    let mut rows = rune::col().w_full().padding(crate::kit::menu::PAD).role("menu");
    let mut n = 0;
    for (g, group) in s.title_tools(&menu.server).into_iter().enumerate() {
        if g > 0 {
            rows = rows.child(list_rule(t).key(format!("tm-rule-{g}")));
        }
        for tool in group {
            let state = if menu.hot == Some(n) { RowState::Hot } else { RowState::Rest };
            let (key, label, glyph, caps) = match tool {
                Tool::NewTerminal => ("tm-terminal", "New terminal", Icon::Terminal, keys.as_deref()),
                Tool::Files => ("tm-files", "Files", Icon::Folder, None),
                Tool::CopyAddress => ("tm-copy", "Copy address", Icon::Copy, None),
            };
            let spec = ListRow { icon: Some(glyph), label, keys: caps, state, ..ListRow::default() };
            let i = n;
            let mut row = list_row(t, &spec)
                .key(key)
                .role("menuitem")
                .on_pointer_move(move |s: &mut Shell, _| {
                    if let Some(m) = s.row.title_menu.as_mut() {
                        m.hot = Some(i);
                    }
                })
                .on_press_tx(move |s: &mut Shell, tx| s.title_pick(tool, tx));
            if state == RowState::Rest {
                row = row.hover(t.hov2);
            }
            rows = rows.child(row);
            n += 1;
        }
    }
    Some(
        popover(t, MENU_WIDTH)
            .key("title-menu")
            .label(format!("{} tools", s.server_shown(&menu.server, &server.name)))
            .anchored("title", Side::Bottom, Align::Start, 6.0)
            .role("menu")
            .focusable(true)
            .no_focus_ring()
            .on_key(|s: &mut Shell, ev, tx| s.title_key(ev, tx))
            .child(rows)
            .modal_tx(true, |s: &mut Shell, tx| s.close_tab_overlays(tx)),
    )
}

/// The sidebar's foot in the top row, while the sidebar is hidden: the main menu (where the
/// sidebar had it), the security check, Tor and Settings.
pub(crate) fn bar_tools(s: &Shell, t: &'static Theme) -> Option<El> {
    if s.bench.sidebar_shown() {
        return None;
    }
    let button = |key: &'static str, glyph: Icon, name: &'static str, tool: FootTool, lit: bool, ink: rune::Color| {
        let mut e = rune::el()
            .key(key)
            .size(theme::CONTROL, theme::CONTROL)
            .radius(theme::RADIUS)
            .items_center()
            .justify_center()
            .press(t.down)
            .focusable(true)
            .role("button")
            .label_static(name)
            .help(name)
            .on_press_tx(move |s: &mut Shell, tx| s.foot_tool(tool, tx))
            .child(icon(glyph, theme::ICON, ink));
        e = if lit { e.fill(t.selrail) } else { e.hover(t.hov2) };
        e
    };
    let mut tools = row().key("bar-tools").gap(2.0).items_center();
    // A Mac keeps the logo in the sidebar: with it hidden, the main menu is here.
    if s.options.frame == Frame::Mac {
        tools = tools.child(button("bar-home", Icon::House, "Main menu", FootTool::Home, s.bench.showing_home(), t.l2));
    }
    let tor = match s.bench.tor() {
        Tor::Off => RailState::Plain,
        Tor::Starting => RailState::TorStarting,
        Tor::On { .. } => RailState::TorOn,
        Tor::Failed => RailState::TorFailed,
    };
    let tor_ink = match tor {
        RailState::TorOn => t.green,
        RailState::TorFailed => t.red,
        _ => t.l2,
    };
    Some(
        tools
            .child(button("bar-check", Icon::Shield, "Security check", FootTool::Check, false, t.l2))
            .child(button("bar-tor", Icon::Tor, "Tor", FootTool::Tor, false, tor_ink))
            .child(button("bar-settings", Icon::Gear, "Settings", FootTool::Settings, s.screens.settings.open, t.l2)),
    )
}
