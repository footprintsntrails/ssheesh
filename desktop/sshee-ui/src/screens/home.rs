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
//!     ├── home-config   "3 servers in ~/.ssh/config to pick from" in a soft pill, when there are;
//!     │                 home-first on a first start, when there's nothing else to show
//!     ├── home-running  "Running now": run-0 …, rows on the page lit only under the pointer
//!     ├── home-closed   "Recently closed": closed-0 …, "Reopen" showing on the one you point at
//!     └── home-hints    at the foot of the page
//! ```
//!
//! One strong thing, the search, as a browser's new tab is its address bar; the servers in
//! their own colours and pictures under it; the lists under small quiet headings, as Settings
//! names its groups, their rows bare until you point at one. The logo stays the menu's button:
//! it isn't drawn again here.

use indexmap::IndexMap;
use rune::{col, row, text, Cx, El, Key, KeyEvent, ScrollSpec, Tx};
use sshee_app::home::{Arrow, Home, Spot, Waits};
use sshee_app::labels::env_short;
use sshee_app::palette::shortcut_label;
use sshee_app::workbench::shortcuts::Shortcut;
use sshee_app::workbench::{ServerKey, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::list::{list_row, ListRow};
use crate::kit::menu::{key_cap, RowState};
use crate::kit::tile::{tile, Tile, TILE_ICON, TILE_WIDTH};
use crate::shell::Shell;
use crate::kit::scroll_bar::ScrollBars;
use crate::theme::{self, Theme};

/// The gap between two tiles.
const TILE_GAP: f32 = 4.0;
/// A list row's height on the page, and how far in its icon and its list's heading sit.
const PAGE_ROW: f32 = 34.0;
const LIST_INSET: f32 = 10.0;
/// A server's picture or icon on its tile.
const TILE_GLYPH: f32 = 26.0;
/// The page's column at its widest.
const COLUMN_WIDTH: f32 = 720.0;
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
    /// The page's height as last laid out (0 before): the column starts lower on a tall one.
    pub height: f32,
}

impl Default for State {
    fn default() -> Self {
        State { spot: Spot::Search, keys: false, columns: 6, waits: Waits::default(), now: 0.0, colors: IndexMap::new(), config_new: 0, old_new: 0, settle_due: false, height: 0.0 }
    }
}

/// Where the column starts: a little over a tenth of the way down, as a browser's new tab
/// puts its field, never tight under the bar nor halfway down.
fn top(height: f32) -> f32 {
    (height * 0.13).clamp(56.0, 140.0).round()
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

/// A list's heading: its name small and quiet, as Settings names its groups, in line with the
/// rows' icons under it; a quiet note at its end.
fn heading(t: &Theme, name: &str, note: &str) -> El {
    let mut h = row().w_full().px(LIST_INSET).pb(2.0).items_center().child(text(name).font(theme::GROUP_TITLE).color(t.l2).single_line().role("heading").level(2));
    if !note.is_empty() {
        h = h.child(rune::spacer()).child(text(note).font(theme::SMALL).color(t.l2).single_line());
    }
    h
}

/// A list under its heading: its rows straight on the page, each lit only under the pointer
/// or the keyboard, so that a list of one doesn't read as a second search field. As wide as
/// the search over it: the page keeps one measure.
fn list(t: &Theme, key: &str, name: &str, note: &str, rows: Vec<El>) -> El {
    col().key(key.to_owned()).w_full().max_w(SEARCH_WIDTH).gap(4.0).child(heading(t, name, note)).child(col().w_full().gap(1.0).children(rows))
}

/// A list's row: the kit's, a little taller on a page than in a menu, its icon in line with
/// the heading. `action` ("Reopen") shows at its end only under the pointer or the keyboard:
/// five of them at rest would be five times the same word.
fn page_row(t: &Theme, spec: &ListRow, action: Option<&str>) -> El {
    let lit = spec.state != RowState::Rest;
    let mut row = list_row(t, spec).h(PAGE_ROW).px(LIST_INSET).radius(theme::RADIUS).hover(t.hov2).hover_group();
    if let Some(action) = action {
        let ink = if lit { t.on_color } else { t.l2 };
        row = row.child(text(action).key("tail").font(theme::NOTE).color(ink).single_line().group_opacity(if lit { 1.0 } else { 0.0 }, 1.0));
    }
    row
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

    // The search field: the palette's own, at rest. The page's one strong thing, as a
    // browser's new tab is its address bar.
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
        .shadow(2.0, 10.0, rune::Color::BLACK.alpha(if t.dark { 0.28 } else { 0.06 }))
        .items_center()
        .hover(t.hov)
        .role("button")
        .label_static("Search servers, files, commands and settings")
        .on_press_tx(|s: &mut Shell, tx| s.open_palette("", tx))
        .child(icon(Icon::Search, theme::ICON, t.l2))
        .child(text(s.text_shown(&sshee_app::palette::placeholder(&s.bench)).into_owned()).font(theme::INPUT).color(t.l2).single_line().truncate().grow(1.0).shrink().a11y_hidden());
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
        // Streamer mode: "Server 2", as everywhere else.
        let name = s.server_shown(&server.server, &server.name);
        // Its picture, else its icon in the colour you picked, washed over its square; else quiet.
        let color = crate::screens::server_menu::own_color(s, &server.server);
        let label = server.env.as_ref().map(|env| (env_short(env), crate::kit::chip::env_color(t, env)));
        let spec = Tile {
            icon: Some(Icon::for_server(&server.icon)),
            // A picture fills the square, as an app's icon does; an icon sits in it.
            glyph: Some(crate::screens::server_menu::glyph_of(
                s,
                &server.server,
                &server.icon,
                if crate::screens::server_menu::has_picture(s, &server.server) { TILE_ICON } else { TILE_GLYPH },
                t.l2on,
            )),
            name: &name,
            label,
            tint: color,
            // The digit that opens it shows once the keyboard is in use here; the hints under
            // the page say the digits are there.
            key: key.as_deref().filter(|_| state.keys),
            waiting: server.waiting,
            open: server.open,
            focused: on(Spot::Tile(i)),
            dashed: false,
        };
        let (to, menu) = (server.server.clone(), server.server.clone());
        tiles = tiles.child(
            tile(t, spec)
                .key(format!("tile-{i}"))
                .on_press_tx(move |s: &mut Shell, tx| if tx.mods().shift { s.open_in_new_window(&to, tx) } else { s.go_to(&to, tx) })
                .on_secondary_press(move |s: &mut Shell, at, tx| s.open_server_menu(&menu, (at.x, at.y), tx)),
        );
    }
    let add = Tile { icon: Some(Icon::Plus), name: "Add a server", focused: on(Spot::Add), dashed: true, ..Tile::default() };
    tiles = tiles.child(tile(t, add).key("tile-add").on_press_tx(|s: &mut Shell, tx| s.open_add_server(tx)));

    let mut column = col().key("home-column").w_full().max_w(COLUMN_WIDTH).px(24.0).pt(top(state.height)).min_h(state.height).pb(24.0).items_center().child(search).child(tiles.mt(36.0));

    // Servers waiting to be brought in: each line opens "Add a server", where they're listed.
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let offers = [
        ("home-old", state.old_new, format!("{} server{} from SSHeesh 0.10 to bring over", state.old_new, plural(state.old_new))),
        ("home-config", state.config_new, format!("{} server{} in ~/.ssh/config to pick from", state.config_new, plural(state.config_new))),
    ];
    // Each a soft pill, so it reads as something to click rather than a stray line of text.
    let mut offered = row().key("home-offers").mt(16.0).gap(8.0).flex_wrap().justify_center();
    for (key, n, words) in offers {
        if n > 0 {
            offered = offered.child(
                row()
                    .key(key)
                    .h(28.0)
                    .pl(10.0)
                    .pr(12.0)
                    .gap(6.0)
                    .radius(14.0)
                    .items_center()
                    .fill(t.hov)
                    .hover(t.hov2)
                    .press(t.down)
                    .role("button")
                    .on_press_tx(|s: &mut Shell, tx| s.open_add_server(tx))
                    .child(icon(Icon::Plus, 12.0, t.l2on))
                    .child(text(words).font(theme::NOTE).color(t.l2on).single_line()),
            );
        }
    }
    if state.old_new + state.config_new > 0 {
        column = column.child(offered);
    } else if home.tiles.len() <= 1 && home.running.is_empty() && home.closed.is_empty() {
        // A first start: only this computer so far. One line on what the page is for.
        column = column.child(
            text("Servers you add show up here, each one click or one digit away.").key("home-first").font(theme::NOTE).color(t.l2).single_line().mt(16.0),
        );
    }

    if !home.running.is_empty() {
        let mut rows = Vec::new();
        for (i, r) in home.running.iter().enumerate() {
            // Streamer mode: the server as "Server 2", the tab's name with names dotted.
            let server = s.bench.tab(r.tab).map_or_else(|| s.text_shown(&r.server).into_owned(), |tab| s.server_shown(&tab.server, &r.server).into_owned());
            let note = format!("on {server}");
            let label = s.text_shown(&r.label);
            let spec = ListRow {
                icon: Some(if r.agent.is_some() { Icon::Agent } else { Icon::Terminal }),
                label: &label,
                note: &note,
                prod: r.prod,
                needs_you: r.waiting,
                tail: if r.waiting { "" } else { r.state() },
                state: if on(Spot::Running(i)) { RowState::Hot } else { RowState::Rest },
                ..ListRow::default()
            };
            let tab = r.tab;
            rows.push(page_row(t, &spec, None).key(format!("run-{i}")).on_press_tx(move |s: &mut Shell, tx| s.focus_tab(tab, tx)));
        }
        column = column.child(list(t, "home-running", "Running now", "", rows).mt(40.0));
    }

    if !home.closed.is_empty() {
        let note = keys(Shortcut::ReopenTab).map(|k| format!("{k} brings back the last one")).unwrap_or_default();
        let mut rows = Vec::new();
        for (i, c) in home.closed.iter().enumerate() {
            let server = s.bench.closed().get(c.index).map_or_else(|| s.text_shown(&c.server).into_owned(), |closed| s.server_shown(&closed.server, &c.server).into_owned());
            let note = match &c.path {
                Some(path) => format!("on {server} · {}", s.text_shown(path)),
                None => format!("on {server}"),
            };
            let label = s.text_shown(&c.label);
            let glyph = match c.kind {
                TabKind::Terminal => Icon::Terminal,
                TabKind::Files => Icon::Folder,
                TabKind::Stats => Icon::Stats,
                TabKind::Containers => Icon::Cube,
            };
            let spec = ListRow { icon: Some(glyph), label: &label, note: &note, state: if on(Spot::Closed(i)) { RowState::Hot } else { RowState::Rest }, ..ListRow::default() };
            let index = c.index;
            rows.push(page_row(t, &spec, Some("Reopen")).key(format!("closed-{i}")).on_press_tx(move |s: &mut Shell, tx| s.reopen_closed_at(index, tx)));
        }
        let gap = if home.running.is_empty() { 40.0 } else { 28.0 };
        column = column.child(list(t, "home-closed", "Recently closed", &note, rows).mt(gap));
    }

    // A few keys worth knowing, only the ones that do something here: the digits and the
    // right-click once there are servers. The closed tabs' key is in their list's heading.
    let numbered = home.tiles.iter().filter(|tile| tile.key.is_some()).count();
    let servers = home.tiles.len() > 1;
    let digits = (numbered > 1).then(|| format!("1–{}", numbered.min(9)));
    let hints = [hint(t, keys(Shortcut::NewTerminal), "new terminal"), hint(t, digits, "open a server")];
    let mut line = row().key("home-hints").mt(40.0).gap(20.0).items_center().justify_center().flex_wrap().children(hints.into_iter().flatten());
    if servers {
        line = line.child(text("Right-click a server for its tools").font(theme::SMALL).color(t.l2).single_line());
    }
    // At the foot of the page, out of the way of what you came for, as a status line is.
    column = column.child(rune::spacer()).child(line);

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
        .on_layout(|s: &mut Shell, [_, _, _, h]| s.screens.home.height = h)
        .child(column)
        .scroll_bars(t)
}
