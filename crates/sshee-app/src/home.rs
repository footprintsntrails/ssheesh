//! The main menu without a screen: SSHeesh's start page, like a browser's new tab. Your
//! servers as tiles, the agents at work on any of them, the tabs you closed lately, and where
//! the keyboard's highlight goes. A port of 0.10's `src/components/Home.tsx`, plus what a
//! server's card says when the pointer rests on its icon.
//!
//! New in 1.0: the first nine tiles open by their number, and the arrows move a highlight
//! over everything on the page.

use crate::model::EnvLabel;
use crate::workbench::{ServerKey, TabId, TabKind, Tor, Workbench};
use std::collections::HashMap;

/// How many closed tabs the page lists (0.10 shows five of the twenty kept).
pub const CLOSED_SHOWN: usize = 5;

/// A server as a tile.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub server: ServerKey,
    pub name: String,
    /// Its icon's id.
    pub icon: String,
    pub env: Option<EnvLabel>,
    /// Tabs open on it.
    pub open: usize,
    /// Something on it waits for you.
    pub waiting: bool,
    /// The digit that opens it from this page: 1 to 9 for the first nine.
    pub key: Option<char>,
}

/// An agent at work, or a terminal that waits for you.
#[derive(Debug, Clone, PartialEq)]
pub struct Running {
    pub tab: TabId,
    /// The tab's name.
    pub label: String,
    /// The agent in it ("Claude Code"); `None` for a plain terminal that rang.
    pub agent: Option<String>,
    /// Its server's name.
    pub server: String,
    pub prod: bool,
    pub waiting: bool,
}

impl Running {
    /// "Needs you", or "working".
    pub fn state(&self) -> &'static str {
        if self.waiting {
            "Needs you"
        } else {
            "working"
        }
    }
}

/// A tab closed lately.
#[derive(Debug, Clone, PartialEq)]
pub struct Closed {
    /// Its place among the closed tabs, newest first: what [`Workbench::reopen_closed_at`] takes.
    pub index: usize,
    pub label: String,
    pub kind: TabKind,
    /// Its server's name, and for Files the folder it showed.
    pub server: String,
    pub path: Option<String>,
}

/// What the main menu shows now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Home {
    pub tiles: Vec<Tile>,
    /// The ones that wait for you first.
    pub running: Vec<Running>,
    pub closed: Vec<Closed>,
}

/// A place the keyboard's highlight can be.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Spot {
    /// The search field: Return opens the palette.
    #[default]
    Search,
    Tile(usize),
    /// "Add a server", the last tile.
    Add,
    Running(usize),
    Closed(usize),
}

/// An arrow key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Up,
    Down,
    Left,
    Right,
}

impl Home {
    pub fn of(bench: &Workbench) -> Home {
        let tiles = bench
            .servers()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let key = s.key();
                Tile {
                    name: s.name.clone(),
                    icon: s.icon.clone(),
                    env: s.env.clone(),
                    open: bench.tabs_on(&key).count(),
                    waiting: bench.server_needs_you(&key),
                    key: u32::try_from(i + 1).ok().filter(|n| *n <= 9).and_then(|n| char::from_digit(n, 10)),
                    server: key,
                }
            })
            .collect();
        let mut running: Vec<Running> = bench
            .tabs()
            .iter()
            .filter(|t| t.kind == TabKind::Terminal && (t.agent.is_some() || t.needs_you))
            .map(|t| {
                let server = bench.server(&t.server);
                Running {
                    tab: t.id,
                    label: crate::agents::tab_label(t),
                    agent: t.agent.clone(),
                    server: server.map_or("Unknown server", |s| s.name.as_str()).to_owned(),
                    prod: server.is_some_and(|s| s.is_prod()),
                    waiting: t.needs_you,
                }
            })
            .collect();
        // Stable: among the waiting, and among the working, the row's order.
        running.sort_by_key(|r| !r.waiting);
        let closed = bench
            .closed()
            .iter()
            .enumerate()
            // One whose server left the list can't come back.
            .filter_map(|(index, c)| {
                let server = bench.server(&c.server)?;
                let label = c.title.clone().unwrap_or_else(|| c.kind.label().to_owned());
                Some(Closed { index, label, kind: c.kind, server: server.name.clone(), path: c.path.clone() })
            })
            .take(CLOSED_SHOWN)
            .collect();
        Home { tiles, running, closed }
    }

    /// The tile a digit opens.
    pub fn tile_for(&self, digit: char) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.key == Some(digit))
    }

    /// The grid's cells: every tile, then "Add a server".
    fn cells(&self) -> usize {
        self.tiles.len() + 1
    }

    fn cell(&self, i: usize) -> Spot {
        if i < self.tiles.len() {
            Spot::Tile(i)
        } else {
            Spot::Add
        }
    }

    /// The rows under the grid, as one list: the agents, then the closed tabs.
    fn row(&self, i: usize) -> Spot {
        if i < self.running.len() {
            Spot::Running(i)
        } else {
            Spot::Closed(i - self.running.len())
        }
    }

    /// A highlight that still points at something: one left on a row that's gone moves to the
    /// search field.
    pub fn clamp(&self, at: Spot) -> Spot {
        let there = match at {
            Spot::Search | Spot::Add => true,
            Spot::Tile(i) => i < self.tiles.len(),
            Spot::Running(i) => i < self.running.len(),
            Spot::Closed(i) => i < self.closed.len(),
        };
        if there {
            at
        } else {
            Spot::Search
        }
    }

    /// Where an arrow key takes the highlight, with the tiles laid out `columns` to a row. At
    /// an edge it stays.
    pub fn step(&self, at: Spot, arrow: Arrow, columns: usize) -> Spot {
        let columns = columns.max(1);
        let cells = self.cells();
        let rows = self.running.len() + self.closed.len();
        let last_row_start = (cells - 1) / columns * columns;
        match (self.clamp(at), arrow) {
            (Spot::Search, Arrow::Down) => self.cell(0),
            (Spot::Search, _) => Spot::Search,
            (at @ (Spot::Tile(_) | Spot::Add), arrow) => {
                let i = match at {
                    Spot::Tile(i) => i,
                    _ => cells - 1,
                };
                match arrow {
                    Arrow::Left => self.cell(i.saturating_sub(1)),
                    Arrow::Right => self.cell((i + 1).min(cells - 1)),
                    Arrow::Up if i >= columns => self.cell(i - columns),
                    Arrow::Up => Spot::Search,
                    Arrow::Down if i + columns < cells => self.cell(i + columns),
                    // A short last row: down from above it lands on its last cell.
                    Arrow::Down if i < last_row_start => self.cell(cells - 1),
                    Arrow::Down if rows > 0 => self.row(0),
                    Arrow::Down => at,
                }
            }
            (at @ (Spot::Running(_) | Spot::Closed(_)), arrow) => {
                let i = match at {
                    Spot::Running(i) => i,
                    Spot::Closed(i) => self.running.len() + i,
                    _ => 0,
                };
                match arrow {
                    Arrow::Up if i == 0 => self.cell(last_row_start),
                    Arrow::Up => self.row(i - 1),
                    Arrow::Down if i + 1 < rows => self.row(i + 1),
                    _ => at,
                }
            }
        }
    }
}

/// How long ago, for a line that already says what happened: "now", "2 min", "3 h", "2 d".
pub fn ago(seconds: f64) -> String {
    let min = (seconds.max(0.0) / 60.0).floor() as u64;
    match min {
        0 => "now".into(),
        1..=59 => format!("{min} min"),
        60..=1439 => format!("{} h", min / 60),
        _ => format!("{} d", min / 1440),
    }
}

/// Since when each tab has waited for you, on the window's clock (seconds). The workbench
/// knows that a tab waits, not since when; the window notes it here every frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Waits {
    since: HashMap<TabId, f64>,
}

impl Waits {
    /// Tabs that started to wait are noted at `now`; ones that no longer wait are forgotten.
    pub fn note(&mut self, bench: &Workbench, now: f64) {
        self.since.retain(|id, _| bench.tab(*id).is_some_and(|t| t.needs_you));
        for tab in bench.tabs().iter().filter(|t| t.needs_you) {
            self.since.entry(tab.id).or_insert(now);
        }
    }

    /// A tab has in fact waited `seconds` longer than noted (a demo's scene, a restart).
    pub fn backdate(&mut self, tab: TabId, seconds: f64) {
        if let Some(since) = self.since.get_mut(&tab) {
            *since -= seconds;
        }
    }

    /// How long a tab has waited, in seconds.
    pub fn waited(&self, tab: TabId, now: f64) -> Option<f64> {
        self.since.get(&tab).map(|since| (now - since).max(0.0))
    }
}

/// What a server's card says under the pointer: who it is, how it's reached, what's open on
/// it and who waits there.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub name: String,
    pub prod: bool,
    /// "postgres@203.0.113.31, through Tor"; "This computer" has none.
    pub address: Option<String>,
    /// "2 tabs open", "No tabs open".
    pub tabs: String,
    /// "Claude Code needs you, 2 min": one line per tab that waits, the longest wait first.
    pub waiting: Vec<String>,
}

impl Card {
    pub fn of(bench: &Workbench, server: &str, waits: &Waits, now: f64) -> Option<Card> {
        let s = bench.server(server)?;
        let through_tor = !s.is_local() && matches!(bench.tor(), Tor::On { .. } | Tor::Starting);
        let address = (!s.address.is_empty()).then(|| if through_tor { format!("{}, through Tor", s.address) } else { s.address.clone() });
        let open = bench.tabs_on(server).count();
        let tabs = match open {
            0 => "No tabs open".to_owned(),
            1 => "1 tab open".to_owned(),
            n => format!("{n} tabs open"),
        };
        let mut waiting: Vec<(f64, String)> = bench
            .tabs_on(server)
            .filter(|t| t.needs_you)
            .map(|t| {
                let who = t.agent.clone().unwrap_or_else(|| t.label());
                match waits.waited(t.id, now) {
                    Some(secs) => (secs, format!("{who} needs you, {}", ago(secs))),
                    None => (0.0, format!("{who} needs you")),
                }
            })
            .collect();
        waiting.sort_by(|a, b| b.0.total_cmp(&a.0));
        Some(Card { name: s.name.clone(), prod: s.is_prod(), address, tabs, waiting: waiting.into_iter().map(|(_, line)| line).collect() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::{Open, Server};

    const WEB: &str = "host:web";
    const DB: &str = "host:db";

    fn bench() -> Workbench {
        Workbench::new(vec![
            Server::host("web", "web-7731", "deploy@203.0.113.17").env(EnvLabel::Prod).icon("globe"),
            Server::host("staging", "web-staging", "deploy@203.0.113.18").env(EnvLabel::Staging),
            Server::host("db", "db-main", "postgres@203.0.113.31").env(EnvLabel::Prod).icon("database"),
        ])
    }

    fn agent(w: &mut Workbench, server: &str, title: &str, agent: &str) -> TabId {
        w.open_with(TabKind::Terminal, server, Open { title: Some(title.into()), agent: Some(agent.into()), ..Open::default() })
    }

    #[test]
    fn every_machine_is_a_tile_with_its_number() {
        let mut w = bench();
        w.go_to(WEB);
        w.new_terminal();
        let waits = agent(&mut w, DB, "Migration 0042", "Claude Code");
        w.go_to(WEB);
        w.set_needs_you(waits, true);
        let home = Home::of(&w);
        let shown: Vec<(&str, &str, Option<char>, usize, bool)> = home.tiles.iter().map(|t| (t.name.as_str(), t.icon.as_str(), t.key, t.open, t.waiting)).collect();
        assert_eq!(
            shown,
            [
                ("This computer", "laptop", Some('1'), 0, false),
                ("web-7731", "globe", Some('2'), 2, false),
                ("web-staging", "server", Some('3'), 0, false),
                ("db-main", "database", Some('4'), 1, true),
            ]
        );
        assert_eq!(home.tiles[1].env, Some(EnvLabel::Prod));
        assert_eq!(home.tile_for('4').map(|t| t.server.as_str()), Some(DB));
        assert_eq!(home.tile_for('5'), None);
        // Only the first nine have a number.
        let many = Workbench::new((0..12).map(|i| Server::host(&format!("s{i}"), &format!("box-{i}"), "")).collect());
        let keys: Vec<Option<char>> = Home::of(&many).tiles.iter().map(|t| t.key).collect();
        assert_eq!(keys[8], Some('9'));
        assert!(keys[9..].iter().all(Option::is_none));
    }

    #[test]
    fn agents_are_listed_with_the_ones_that_wait_first() {
        let mut w = bench();
        let build = agent(&mut w, WEB, "Build", "Codex");
        let plain = w.go_to("host:staging");
        let migration = agent(&mut w, DB, "Migration 0042", "Claude Code");
        let quiet = w.go_to("local");
        w.show_home();
        w.set_needs_you(migration, true);
        let home = Home::of(&w);
        let rows: Vec<(&str, Option<&str>, &str, &str, bool)> = home.running.iter().map(|r| (r.label.as_str(), r.agent.as_deref(), r.server.as_str(), r.state(), r.prod)).collect();
        assert_eq!(rows, [("Migration 0042", Some("Claude Code"), "db-main", "Needs you", true), ("Build", Some("Codex"), "web-7731", "working", true)]);
        // A plain terminal that rang waits too; one that didn't isn't listed.
        w.set_needs_you(plain, true);
        let home = Home::of(&w);
        assert_eq!(home.running.iter().map(|r| r.tab).collect::<Vec<_>>(), [plain, migration, build]);
        assert_eq!(home.running[0].agent, None);
        assert!(!home.running.iter().any(|r| r.tab == quiet));
    }

    #[test]
    fn closed_tabs_are_listed_newest_first_five_at_most() {
        let mut w = bench();
        w.go_to(WEB);
        let files = w.open_with(TabKind::Files, WEB, Open { path: Some("/srv/shop".into()), ..Open::default() });
        let named = w.open_with(TabKind::Terminal, DB, Open { title: Some("psql shop".into()), ..Open::default() });
        w.close(files);
        w.close(named);
        let home = Home::of(&w);
        let rows: Vec<(usize, &str, &str, Option<&str>)> = home.closed.iter().map(|c| (c.index, c.label.as_str(), c.server.as_str(), c.path.as_deref())).collect();
        assert_eq!(rows, [(0, "psql shop", "db-main", None), (1, "Files", "web-7731", Some("/srv/shop"))]);
        // A click brings that one back, not the newest.
        let back = w.reopen_closed_at(1).unwrap();
        assert_eq!((w.tab(back).unwrap().kind, w.tab(back).unwrap().path.as_deref()), (TabKind::Files, Some("/srv/shop")));
        assert_eq!(Home::of(&w).closed.len(), 1);
        assert_eq!(w.reopen_closed_at(7), None);
        // A server that left the list: its closed tabs aren't offered, and the places of the
        // rest are still the workbench's.
        let gone = w.go_to("host:staging");
        w.close(gone);
        let kept = w.new_terminal();
        w.close(kept);
        w.set_servers(bench().servers().iter().filter(|s| s.key() != "host:staging").cloned().collect());
        let home = Home::of(&w);
        assert_eq!(home.closed.iter().map(|c| c.index).collect::<Vec<_>>(), [0, 2]);
        for _ in 0..9 {
            let t = w.new_terminal();
            w.close(t);
        }
        assert_eq!(Home::of(&w).closed.len(), CLOSED_SHOWN);
    }

    #[test]
    fn arrows_move_over_the_search_field_the_tiles_and_the_rows() {
        let mut w = bench();
        let a = agent(&mut w, DB, "Migration 0042", "Claude Code");
        let closed = w.go_to(WEB);
        w.close(closed);
        w.show_home();
        w.set_needs_you(a, true);
        let home = Home::of(&w);
        // Four tiles and "Add a server", three to a row:  [0 1 2] [3 Add], then one agent, one closed tab.
        let go = |at, arrow| home.step(at, arrow, 3);
        assert_eq!(go(Spot::Search, Arrow::Up), Spot::Search);
        assert_eq!(go(Spot::Search, Arrow::Down), Spot::Tile(0));
        assert_eq!(go(Spot::Tile(0), Arrow::Left), Spot::Tile(0));
        assert_eq!(go(Spot::Tile(0), Arrow::Right), Spot::Tile(1));
        assert_eq!(go(Spot::Tile(2), Arrow::Right), Spot::Tile(3));
        assert_eq!(go(Spot::Tile(3), Arrow::Right), Spot::Add);
        assert_eq!(go(Spot::Add, Arrow::Right), Spot::Add);
        assert_eq!(go(Spot::Add, Arrow::Left), Spot::Tile(3));
        assert_eq!(go(Spot::Tile(1), Arrow::Down), Spot::Add);
        assert_eq!(go(Spot::Tile(0), Arrow::Down), Spot::Tile(3));
        // Nothing under the third column: the last tile.
        assert_eq!(go(Spot::Tile(2), Arrow::Down), Spot::Add);
        assert_eq!(go(Spot::Tile(3), Arrow::Up), Spot::Tile(0));
        assert_eq!(go(Spot::Tile(1), Arrow::Up), Spot::Search);
        assert_eq!(go(Spot::Add, Arrow::Down), Spot::Running(0));
        assert_eq!(go(Spot::Running(0), Arrow::Down), Spot::Closed(0));
        assert_eq!(go(Spot::Closed(0), Arrow::Down), Spot::Closed(0));
        assert_eq!(go(Spot::Closed(0), Arrow::Up), Spot::Running(0));
        assert_eq!(go(Spot::Running(0), Arrow::Up), Spot::Tile(3));
        assert_eq!(go(Spot::Running(0), Arrow::Left), Spot::Running(0));
        // A highlight left on a row that's gone starts again from the search field.
        assert_eq!(home.clamp(Spot::Closed(4)), Spot::Search);
        assert_eq!(go(Spot::Tile(9), Arrow::Down), Spot::Tile(0));
        // One column, and nothing under the tiles.
        let bare = Home::of(&bench());
        assert_eq!(bare.step(Spot::Tile(3), Arrow::Down, 1), Spot::Add);
        assert_eq!(bare.step(Spot::Add, Arrow::Down, 1), Spot::Add);
        assert_eq!(bare.step(Spot::Add, Arrow::Up, 0), Spot::Tile(3));
    }

    #[test]
    fn a_servers_card_says_who_it_is_and_who_waits_there() {
        let mut w = bench();
        let migration = agent(&mut w, DB, "Migration 0042", "Claude Code");
        let shell = w.open(TabKind::Terminal, DB);
        w.go_to(WEB);
        let mut waits = Waits::default();
        waits.note(&w, 10.0);
        let card = Card::of(&w, DB, &waits, 10.0).unwrap();
        assert_eq!((card.name.as_str(), card.prod, card.address.as_deref(), card.tabs.as_str()), ("db-main", true, Some("postgres@203.0.113.31"), "2 tabs open"));
        assert!(card.waiting.is_empty());
        // Two wait, one since two minutes, the other just now: the longest wait first.
        w.set_needs_you(migration, true);
        waits.note(&w, 10.0);
        w.set_needs_you(shell, true);
        waits.note(&w, 130.0);
        w.set_tor(Tor::On { latency_ms: None });
        let card = Card::of(&w, DB, &waits, 135.0).unwrap();
        assert_eq!(card.address.as_deref(), Some("postgres@203.0.113.31, through Tor"));
        assert_eq!(card.waiting, ["Claude Code needs you, 2 min", "Terminal 2 needs you, now"]);
        // Looked at: it no longer waits, and its time is forgotten.
        w.focus(migration);
        waits.note(&w, 140.0);
        assert_eq!(waits.waited(migration, 150.0), None);
        // This computer has no address and never goes through Tor.
        let card = Card::of(&w, "local", &waits, 150.0).unwrap();
        assert_eq!((card.name.as_str(), card.address, card.tabs.as_str()), ("This computer", None, "No tabs open"));
        assert_eq!(Card::of(&w, "host:gone", &waits, 0.0), None);
        // A demo's scene: the wait began before the window did.
        w.go_to(WEB);
        w.set_needs_you(migration, true);
        waits.note(&w, 200.0);
        waits.backdate(migration, 120.0);
        assert_eq!(waits.waited(migration, 200.0), Some(120.0));
    }

    #[test]
    fn how_long_ago_reads_short() {
        assert_eq!(ago(0.0), "now");
        assert_eq!(ago(59.0), "now");
        assert_eq!(ago(125.0), "2 min");
        assert_eq!(ago(3.0 * 3600.0 + 5.0), "3 h");
        assert_eq!(ago(49.0 * 3600.0), "2 d");
        assert_eq!(ago(-5.0), "now");
    }
}
