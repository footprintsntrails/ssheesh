//! The main menu (0.10's Home): SSHeesh's start page, like a browser's new tab. A search
//! field in the middle (it opens the command palette, and so does typing), your servers as
//! tiles with the digit that opens each, the agents at work on any server with the ones that
//! wait for you first, the tabs closed lately, and "Add a server". It shows when no tab is on
//! screen and from the logo.
//!
//! The keyboard: arrows move one highlight over the page, Return opens what it's on, 1 to 9
//! open a tile, any other character starts a search. What the page lists and where an arrow
//! goes are rules of `sshee_app::home`; this file draws them.
//!
//! ```text
//! home-page   scrolls; takes the keyboard while the menu shows
//! └── home-column  at most 720 pt, centred
//!     ├── home-search   the field: a button that opens the palette over itself
//!     ├── home-tiles    tile-0 … tile-N, tile-add (wraps)
//!     ├── home-config   "3 servers in ~/.ssh/config to pick from", when there are
//!     ├── home-running  "Running now": run-0 …
//!     ├── home-closed   "Recently closed": closed-0 …
//!     └── home-hints
//! ```

use indexmap::IndexMap;
use rune::{col, row, text, Cx, El, Key, KeyEvent, ScrollSpec, Tx};
use sshee_app::home::{Arrow, Home, Spot, Waits};
use sshee_app::labels::env_short;
use sshee_app::model::EnvLabel;
use sshee_app::palette::shortcut_label;
use sshee_app::workbench::shortcuts::Shortcut;
use sshee_app::workbench::{ServerKey, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::list::{list_row, ListRow};
use crate::kit::menu::{key_cap, RowState};
use crate::kit::tile::{tile, Tile, TILE_WIDTH};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The gap between two tiles.
const TILE_GAP: f32 = 4.0;
/// The search field's width, the palette's.
pub const SEARCH_WIDTH: f32 = 560.0;

/// The main menu's own state.
pub struct State {
    /// Where the keyboard's highlight is.
    pub spot: Spot,
    /// The highlight shows: a key moved it. A click hides it again.
    pub keys: bool,
    /// How many tiles fit in a row, from layout.
    pub columns: usize,
    /// Since when each tab has waited for you.
    pub waits: Waits,
    /// The window's clock at the last frame, in seconds.
    pub now: f64,
    /// Each server's own colour, by its key (the backend's; a tile is tinted with it).
    pub colors: IndexMap<ServerKey, String>,
    /// Servers in `~/.ssh/config` not said yes or no to yet, and servers 0.10 saved that
    /// aren't here yet: each is a line under the tiles that opens "Add a server".
    pub config_new: usize,
    pub old_new: usize,
    /// The workbench changed off the screen (a server was added): settle at the next frame.
    pub settle_due: bool,
}

impl Default for State {
    fn default() -> Self {
        State { spot: Spot::Search, keys: false, columns: 6, waits: Waits::default(), now: 0.0, colors: IndexMap::new(), config_new: 0, old_new: 0, settle_due: false }
    }
}

/// What every frame does for the screens of this file's family, from `Shell::poll`: notes
/// who started to wait, sends the palette's commands whose terminals have connected, settles
/// after a change that came from another thread, and brings a server's card up when the
/// pointer has rested on its icon. True when the view must run again.
pub fn tick(s: &mut Shell, clock: f64, tx: &mut Tx) -> bool {
    s.screens.home.now = clock;
    s.screens.home.waits.note(&s.bench, clock);
    super::palette::send_pending(s, tx);
    let settled = std::mem::take(&mut s.screens.home.settle_due);
    if settled {
        s.settle(tx);
    }
    super::server_card::tick(s, clock) || settled
}

impl Shell {
    /// What the backend has for the server list beyond the servers themselves: each one's
    /// colour, and how many more the ssh config and 0.10 offer.
    pub fn set_server_looks(&mut self, colors: IndexMap<ServerKey, String>, config_new: usize, old_new: usize) {
        let home = &mut self.screens.home;
        home.colors = colors;
        home.config_new = config_new;
        home.old_new = old_new;
    }

    /// A tab has waited `seconds` longer than the window knows (a demo's scene).
    pub fn backdate_wait(&mut self, tab: sshee_app::workbench::TabId, seconds: f64) {
        let now = self.screens.home.now;
        self.screens.home.waits.note(&self.bench, now);
        self.screens.home.waits.backdate(tab, seconds);
    }

    /// Opens what the highlight is on.
    fn home_open(&mut self, spot: Spot, tx: &mut Tx) {
        let home = Home::of(&self.bench);
        match home.clamp(spot) {
            Spot::Search => self.open_palette("", tx),
            Spot::Tile(i) => {
                let server = home.tiles[i].server.clone();
                self.go_to(&server, tx);
            }
            Spot::Add => self.open_add_server(tx),
            Spot::Running(i) => self.focus_tab(home.running[i].tab, tx),
            Spot::Closed(i) => self.reopen_closed_at(home.closed[i].index, tx),
        }
    }

    /// A click on a tab closed lately: that one comes back.
    pub fn reopen_closed_at(&mut self, index: usize, tx: &mut Tx) {
        self.bench.reopen_closed_at(index);
        self.settle(tx);
    }

    /// A key on the main menu. True: it was the menu's.
    pub fn home_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if !self.bench.showing_home() || self.screens.any_open() {
            return false;
        }
        let arrow = match ev.key {
            Key::Up => Some(Arrow::Up),
            Key::Down => Some(Arrow::Down),
            Key::Left => Some(Arrow::Left),
            Key::Right => Some(Arrow::Right),
            _ => None,
        };
        let plain = !ev.mods.cmd && !ev.mods.ctrl && !ev.mods.alt && !ev.mods.logo;
        if let Some(arrow) = arrow.filter(|_| plain) {
            let home = Home::of(&self.bench);
            let state = &mut self.screens.home;
            // The first arrow shows the highlight where it is; the next ones move it.
            state.spot = if state.keys { home.step(state.spot, arrow, state.columns) } else { home.step(Spot::Search, arrow, state.columns) };
            state.keys = true;
            tx.keep_on_screen(spot_key(state.spot));
            return true;
        }
        match &ev.key {
            Key::Enter if plain => {
                let spot = if self.screens.home.keys { self.screens.home.spot } else { Spot::Search };
                self.home_open(spot, tx);
                true
            }
            Key::Escape if self.screens.home.keys => {
                self.screens.home.keys = false;
                true
            }
            Key::Text(typed) if plain && !typed.trim().is_empty() => {
                let mut chars = typed.chars();
                let digit = chars.next().filter(|c| c.is_ascii_digit() && chars.next().is_none());
                match digit.and_then(|d| Home::of(&self.bench).tile_for(d).map(|t| t.server.clone())) {
                    Some(server) => self.go_to(&server, tx),
                    None => self.open_palette(typed, tx),
                }
                true
            }
            _ => false,
        }
    }
}

/// The element a highlight is on.
fn spot_key(spot: Spot) -> String {
    match spot {
        Spot::Search => "home-search".into(),
        Spot::Tile(i) => format!("tile-{i}"),
        Spot::Add => "tile-add".into(),
        Spot::Running(i) => format!("run-{i}"),
        Spot::Closed(i) => format!("closed-{i}"),
    }
}

/// A section's heading, with a quiet note at its end.
fn heading(t: &Theme, name: &str, note: &str) -> El {
    let mut h = row().w_full().px(8.0).pb(4.0).items_center().child(text(name).font(theme::GROUP).color(t.l2).single_line());
    if !note.is_empty() {
        h = h.child(rune::spacer()).child(text(note).font(theme::SMALL).color(t.l2).single_line());
    }
    h
}

/// A key cap and what it does, for the line of hints.
fn hint(t: &Theme, keys: Option<String>, does: &str) -> Option<El> {
    Some(row().gap(6.0).items_center().child(key_cap(t, &keys?, false)).child(text(does).font(theme::SMALL).color(t.l2).single_line()))
}

pub fn view(s: &Shell, t: &'static Theme, _cx: &mut Cx<Shell>) -> El {
    let home = Home::of(&s.bench);
    let state = &s.screens.home;
    let on = |spot: Spot| state.keys && home.clamp(state.spot) == spot;
    let keys = |shortcut: Shortcut| shortcut_label(shortcut, s.options.mac_keys, &s.options.shortcuts);

    // The search field: the palette's own, at rest.
    let mut search = row()
        .key("home-search")
        .w(SEARCH_WIDTH)
        .max_w(SEARCH_WIDTH)
        .shrink()
        .h(44.0)
        .px(14.0)
        .gap(10.0)
        .radius(theme::RADIUS_POPOVER)
        .fill(t.pop)
        .border(1.0, t.rim)
        .items_center()
        .hover(t.hov)
        .role("button")
        .label_static("Search servers, files, commands and settings")
        .on_press_tx(|s: &mut Shell, tx| s.open_palette("", tx))
        .child(icon(Icon::Search, theme::ICON, t.l2))
        .child(text(sshee_app::palette::placeholder(&s.bench)).font(theme::INPUT).color(t.l2).single_line().truncate().grow(1.0).shrink().a11y_hidden());
    if let Some(keys) = keys(Shortcut::Palette) {
        search = search.child(key_cap(t, &keys, false));
    }
    if on(Spot::Search) {
        search = search.outline(theme::RING, 0.0, t.ring);
    }

    // The servers, and "Add a server" after them.
    let mut tiles = row()
        .key("home-tiles")
        .w_full()
        .flex_wrap()
        .justify_center()
        .gap(TILE_GAP)
        .on_layout(|s: &mut Shell, [_, _, w, _]| s.screens.home.columns = (((w + TILE_GAP) / (TILE_WIDTH + TILE_GAP)).floor() as usize).max(1));
    for (i, server) in home.tiles.iter().enumerate() {
        let key = server.key.map(String::from);
        let color = state.colors.get(&server.server).and_then(|c| rune::color::hex(c));
        let label = server.env.as_ref().map(|env| (env_short(env), *env == EnvLabel::Prod));
        let spec = Tile {
            icon: Some(Icon::for_server(&server.icon)),
            name: &server.name,
            label,
            tint: color,
            key: key.as_deref(),
            waiting: server.waiting,
            open: server.open,
            focused: on(Spot::Tile(i)),
            dashed: false,
        };
        let (to, menu) = (server.server.clone(), server.server.clone());
        tiles = tiles.child(
            tile(t, &spec)
                .key(format!("tile-{i}"))
                .on_press_tx(move |s: &mut Shell, tx| s.go_to(&to, tx))
                .on_secondary_press(move |s: &mut Shell, at, tx| s.open_server_menu(&menu, (at.x, at.y), tx)),
        );
    }
    let add = Tile { icon: Some(Icon::Plus), name: "Add a server", focused: on(Spot::Add), dashed: true, ..Tile::default() };
    tiles = tiles.child(tile(t, &add).key("tile-add").on_press_tx(|s: &mut Shell, tx| s.open_add_server(tx)));

    let mut column = col().key("home-column").w_full().max_w(720.0).px(24.0).pt(56.0).pb(40.0).items_center().child(search).child(tiles.mt(32.0));

    // Servers waiting to be brought in: each line opens "Add a server", where they're listed.
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let offers = [
        ("home-old", state.old_new, format!("{} server{} from SSHeesh 0.10 to bring over", state.old_new, plural(state.old_new))),
        ("home-config", state.config_new, format!("{} server{} in ~/.ssh/config to pick from", state.config_new, plural(state.config_new))),
    ];
    let mut offered = row().key("home-offers").mt(10.0).gap(8.0).flex_wrap().justify_center();
    for (key, n, words) in offers {
        if n > 0 {
            offered = offered.child(
                row()
                    .key(key)
                    .h(26.0)
                    .px(12.0)
                    .radius(13.0)
                    .items_center()
                    .fill(t.hov2)
                    .hover(t.down)
                    .role("button")
                    .on_press_tx(|s: &mut Shell, tx| s.open_add_server(tx))
                    .child(text(words).font(theme::NOTE).color(t.l1).single_line()),
            );
        }
    }
    if state.old_new + state.config_new > 0 {
        column = column.child(offered);
    }

    if !home.running.is_empty() {
        let mut section = col().key("home-running").w_full().mt(32.0).child(heading(t, "Running now", ""));
        for (i, r) in home.running.iter().enumerate() {
            let note = format!("on {}", r.server);
            let spec = ListRow {
                icon: Some(if r.agent.is_some() { Icon::Agent } else { Icon::Terminal }),
                label: &r.label,
                note: &note,
                prod: r.prod,
                needs_you: r.waiting,
                tail: if r.waiting { "" } else { r.state() },
                state: if on(Spot::Running(i)) { RowState::Hot } else { RowState::Rest },
                ..ListRow::default()
            };
            let tab = r.tab;
            section = section.child(list_row(t, &spec).key(format!("run-{i}")).hover(t.hov2).on_press_tx(move |s: &mut Shell, tx| s.focus_tab(tab, tx)));
        }
        column = column.child(section);
    }

    if !home.closed.is_empty() {
        let note = keys(Shortcut::ReopenTab).map(|k| format!("{k} brings back the last one")).unwrap_or_default();
        let mut section = col().key("home-closed").w_full().mt(24.0).child(heading(t, "Recently closed", &note));
        for (i, c) in home.closed.iter().enumerate() {
            let note = match &c.path {
                Some(path) => format!("on {} · {path}", c.server),
                None => format!("on {}", c.server),
            };
            let glyph = match c.kind {
                TabKind::Terminal => Icon::Terminal,
                TabKind::Files => Icon::Folder,
                TabKind::Stats => Icon::Stats,
            };
            let spec = ListRow { icon: Some(glyph), label: &c.label, note: &note, tail: "Reopen", state: if on(Spot::Closed(i)) { RowState::Hot } else { RowState::Rest }, ..ListRow::default() };
            let index = c.index;
            section = section.child(list_row(t, &spec).key(format!("closed-{i}")).hover(t.hov2).on_press_tx(move |s: &mut Shell, tx| s.reopen_closed_at(index, tx)));
        }
        column = column.child(section);
    }

    let hints = [hint(t, keys(Shortcut::NewTerminal), "new terminal"), hint(t, keys(Shortcut::ReopenTab), "reopen closed tab")];
    column = column.child(
        row()
            .key("home-hints")
            .mt(36.0)
            .gap(20.0)
            .items_center()
            .justify_center()
            .flex_wrap()
            .children(hints.into_iter().flatten())
            .child(text("Right-click a server for its tools").font(theme::SMALL).color(t.l2).single_line()),
    );

    col()
        .key("home-page")
        .w_full()
        .h_full()
        .items_center()
        .scroll(ScrollSpec { stick_bottom: false })
        .focusable(true)
        .no_focus_ring()
        .on_key(|s: &mut Shell, ev, tx| s.home_key(ev, tx))
        .label_static("Main menu")
        .child(column)
}
