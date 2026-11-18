//! The command palette (0.10's command bar), in the 1.0 design: a search field over a list,
//! opened by its shortcut (⌘L on a Mac, Ctrl+Shift+L and Ctrl+Shift+K elsewhere), by a click
//! on the server's name in the top row, by the sidebar's search button, and from the main
//! menu's field. It hangs under the title like an address bar; on the main menu it opens over
//! the field in the middle.
//!
//! What it lists, how typing narrows it and which row Return picks are rules of
//! `sshee_app::palette`. The one this file must keep: a row that runs a command on a server
//! is never highlighted by itself, so Return on a fresh list runs nothing; you move to that
//! row (an arrow, or the pointer) and press Return or click it.
//!
//! ```text
//! palette      the popover; modal: Esc and a click outside close it
//! ├── palette-in     [search] [scope chip] the field [esc]
//! ├── palette-list   headings and rows: pal-0 … (scrolls)
//! └── palette-foot   a hint, or why a pick did nothing
//! ```
//!
//! The field keeps the keyboard; arrows, Return and Esc are taken before it edits. Closing
//! gives the keyboard back to the terminal (`Shell::settle`).

use std::collections::HashMap;

use rune::{col, row, text, text_input, Align, ClipboardContent, Cx, El, Key, KeyEvent, Mods, ScrollSpec, Side, TextEdit, Tx};
use sshee_app::palette::{self, Action, Context, Glyph, Group, Row};
use sshee_app::workbench::shortcuts::{match_shortcut, Pressed, Shortcut};
use sshee_app::workbench::{Link, Open, ServerKey, TabId, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::chip::chip;
use crate::kit::list::{list_heading, list_row, ListRow};
use crate::kit::menu::{key_cap, RowState};
use crate::kit::popover::{popover, popover_foot};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The palette's width, the design's.
pub const WIDTH: f32 = 560.0;

/// The palette's own state.
#[derive(Default)]
pub struct State {
    pub open: bool,
    /// What is typed.
    pub query: TextEdit,
    /// The highlighted row, by its place in the list for what is typed.
    pub hot: Option<usize>,
    /// Why the last pick did nothing, under the list.
    pub note: Option<String>,
    /// Commands for terminals that haven't connected yet: typed (and run, when `true`) once
    /// they have.
    pub(crate) pending: HashMap<TabId, (String, bool)>,
}


impl State {
    /// A tab closed: nothing waits to be typed into it.
    pub fn retain(&mut self, open: impl Fn(TabId) -> bool) {
        self.pending.retain(|id, _| open(*id));
    }
}

/// The picture for a row.
fn glyph(g: &Glyph) -> Icon {
    match g {
        Glyph::Folder => Icon::Folder,
        Glyph::Terminal => Icon::Terminal,
        Glyph::Agent => Icon::Agent,
        Glyph::Stats => Icon::Stats,
        Glyph::Containers => Icon::Cube,
        Glyph::Server(id) => Icon::for_server(id),
        Glyph::Plus => Icon::Plus,
        Glyph::Close => Icon::Close,
        Glyph::Sidebar => Icon::Sidebar,
        Glyph::Home => Icon::House,
        Glyph::Reopen => Icon::Reopen,
        Glyph::Gear => Icon::Gear,
        Glyph::Shield => Icon::Shield,
        Glyph::Drive => Icon::Drive,
        Glyph::Recipe => Icon::Doc,
        Glyph::Link => Icon::Link,
        Glyph::News => Icon::Sparkle,
        Glyph::Help => Icon::Info,
        Glyph::SplitRight => Icon::SideBySide,
        Glyph::SplitDown => Icon::SplitDown,
        Glyph::Zoom => Icon::Expand,
        Glyph::Compact => Icon::Sidebar,
        Glyph::Find => Icon::Search,
        Glyph::Eye(true) => Icon::EyeOff,
        Glyph::Eye(false) => Icon::Eye,
        Glyph::Tor => Icon::Tor,
        Glyph::Star => Icon::Star,
        Glyph::Refresh => Icon::Refresh,
    }
}

/// Types the commands whose terminals have connected (every frame, from `home::tick`).
pub(crate) fn send_pending(s: &mut Shell, tx: &mut Tx) {
    if s.screens.palette.pending.is_empty() {
        return;
    }
    let ready: Vec<TabId> = s.screens.palette.pending.keys().copied().filter(|id| s.bench.tab(*id).is_some_and(|t| matches!(t.link, Link::Connected { .. }))).collect();
    for id in ready {
        if let Some((command, enter)) = s.screens.palette.pending.remove(&id) {
            s.type_into(id, &command, enter, tx);
        }
    }
}

impl Shell {
    /// The rows for what is typed now.
    fn palette_rows(&self) -> Vec<Row> {
        // Yours, as the snippets screen keeps them: those for every server and this one's own.
        let snippets = crate::screens::snippets::offered(self);
        let now = palette::Now { streaming: self.streaming(), compact: self.settings.get().compact_mode != sshee_app::model::CompactMode::Off, ..palette::Now::default() };
        let ctx = Context { bench: &self.bench, snippets: &snippets, mac: self.options.mac_keys, shortcuts: &self.options.shortcuts, bookmarks: &self.screens.bookmarks, now };
        palette::rows(&ctx, &palette::parse(&self.screens.palette.query.text))
    }

    /// Opens the palette with `typed` in its field (the main menu passes the key that
    /// opened it). The first row that runs nothing on a server is highlighted.
    pub fn open_palette(&mut self, typed: &str, tx: &mut Tx) {
        self.screens.close_all();
        let state = &mut self.screens.palette;
        state.open = true;
        state.query = TextEdit::new(typed);
        state.query.set_caret(typed.len());
        state.query.focused = true;
        state.note = None;
        self.screens.palette.hot = palette::first_choice(&self.palette_rows());
        tx.instant();
    }

    /// The shortcut: opens it, or closes it when it's open.
    pub fn toggle_palette(&mut self, tx: &mut Tx) {
        if self.screens.palette.open {
            self.close_palette(tx);
        } else {
            self.open_palette("", tx);
        }
    }

    /// Closes it; the terminal on screen has the keyboard again.
    pub fn close_palette(&mut self, tx: &mut Tx) {
        let state = &mut self.screens.palette;
        state.open = false;
        state.query = TextEdit::default();
        state.hot = None;
        state.note = None;
        self.settle(tx);
    }

    /// The text changed: the highlight starts again from the first row that runs nothing.
    fn palette_typed(&mut self, tx: &mut Tx) {
        self.screens.palette.hot = palette::first_choice(&self.palette_rows());
        self.screens.palette.note = None;
        tx.scroll_to_y("palette-list", 0.0);
    }

    /// A key in the field, before the field edits. True: it was the palette's.
    fn palette_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        match ev.key {
            Key::Down | Key::Up => {
                let len = self.palette_rows().len();
                let state = &mut self.screens.palette;
                state.hot = palette::step(len, state.hot, if ev.key == Key::Down { 1 } else { -1 });
                if let Some(hot) = state.hot {
                    tx.scroll_into_view_in("palette-list", format!("pal-{hot}"));
                }
                true
            }
            Key::Enter => {
                if let Some(hot) = self.screens.palette.hot {
                    self.palette_pick(hot, tx);
                }
                true
            }
            Key::Escape => {
                self.close_palette(tx);
                true
            }
            // Its own shortcut closes it; no other shortcut works behind it, as behind any
            // dialog in 0.10.
            _ => {
                let Some(name) = crate::shell::keys::key_name(ev, tx.physical_key(), tx.key_base()) else { return false };
                let pressed = Pressed { key: &name, ctrl: ev.mods.ctrl, meta: ev.mods.logo, alt: ev.mods.alt, shift: ev.mods.shift };
                match match_shortcut(&pressed, self.options.mac_keys, &self.options.shortcuts) {
                    Some(Shortcut::Palette | Shortcut::PaletteAlt) => {
                        self.close_palette(tx);
                        true
                    }
                    // Streamer mode, though, works over anything: names must hide at once.
                    Some(Shortcut::Streamer) => {
                        if !tx.key_repeat() {
                            self.toggle_streamer(tx);
                        }
                        true
                    }
                    // In a text field Ctrl+Shift+Z is redo, as in 0.10, not "Reopen closed tab".
                    Some(Shortcut::ReopenTab) => false,
                    Some(_) => true,
                    None => false,
                }
            }
        }
    }

    /// Return on a row, or a click on it.
    fn palette_pick(&mut self, at: usize, tx: &mut Tx) {
        let Some(row) = self.palette_rows().into_iter().nth(at) else { return };
        self.close_palette(tx);
        match row.action {
            Action::Files { server, path } => {
                self.open_files_on(&server, path.as_deref(), tx);
            }
            Action::Stats { server } => self.show_tab(TabKind::Stats, &server, None, tx),
            Action::Containers { server } => self.open_containers(&server, tx),
            Action::Tab(id) => self.focus_tab(id, tx),
            Action::Server(server) => self.go_to(&server, tx),
            Action::Run { server, command, enter } => self.run_on(&server, &command, enter, tx),
            Action::FillSnippet { server, id } => self.fill_snippet(&id, &server, tx),
            Action::NewTerminal => self.new_terminal(tx),
            Action::CloseTab => self.close_active(tx),
            Action::ReopenTab => self.reopen_closed(tx),
            Action::ToggleSidebar => self.toggle_sidebar(tx),
            Action::SplitRight => self.split_pane(sshee_app::workbench::layout::Dir::Row, tx),
            Action::SplitDown => self.split_pane(sshee_app::workbench::layout::Dir::Col, tx),
            Action::Zoom => self.zoom_active(tx),
            Action::MainMenu => self.show_home(tx),
            Action::AddServer => self.open_add_server(tx),
            // At the section named, else where Settings was last.
            Action::Settings(Some(id)) => match sshee_app::settings_store::catalog::Section::from_id(id) {
                Some(section) => self.open_settings_at(section, tx),
                None => self.open_settings(tx),
            },
            Action::Settings(None) => self.open_settings(tx),
            Action::Checkup { server } => self.open_checkup(&server, tx),
            Action::Disk { server } => self.open_disk(&server, tx),
            Action::Recipes { server, recipe } => self.open_recipes(&server, recipe, tx),
            Action::Tunnels { server } => self.open_tunnels(&server, tx),
            Action::Snippets => self.open_snippets(tx),
            Action::NewSnippet(command) => self.new_snippet(&command, tx),
            Action::WhatsNew => self.open_whats_new(tx),
            Action::Tour => self.open_tour(0, tx),
            Action::SettingsRow { section, key } => {
                if let Some(section) = sshee_app::settings_store::catalog::Section::from_id(section) {
                    self.open_settings_at(section, tx);
                    self.screens.settings.found = Some(key.clone());
                    tx.scroll_into_view(key);
                }
            }
            Action::SideBySide => self.toggle_side_by_side(tx),
            Action::Arrange(how) => self.arrange(how, tx),
            Action::Compact => self.toggle_compact(tx),
            Action::Find => {
                if let Some(pane) = self.active_pane() {
                    pane.open_find(tx);
                }
            }
            Action::Streamer => self.toggle_streamer(tx),
            Action::Tor => self.switch_tor(tx),
            Action::Agents => crate::screens::agent::toggle_list(self, tx),
            Action::Workspace(server) => self.open_workspace(&server, tx),
            Action::CheckUpdates => self.open_settings_at(sshee_app::settings_store::catalog::Section::Updates, tx),
        }
    }

    /// "Workspace for web-7731" (0.10's `openWorkspace`): the machine's terminal with its files
    /// beside it, each the one it has or a new one; the keyboard in the terminal.
    pub fn open_workspace(&mut self, server: &str, tx: &mut Tx) {
        let term = self.bench.tabs_on(server).find(|t| t.kind == TabKind::Terminal).map(|t| t.id);
        let term = term.unwrap_or_else(|| self.bench.open(TabKind::Terminal, server));
        let files = self.bench.open_files(server);
        if !self.bench.together(term, files) {
            self.bench.pair(term, files);
        }
        self.bench.focus(term);
        self.settle(tx);
    }

    /// Goes to a machine's Files or Stats tab (at `path`, when given), or opens one.
    pub fn show_tab(&mut self, kind: TabKind, server: &str, path: Option<String>, tx: &mut Tx) {
        let there = self.bench.tabs_on(server).find(|t| t.kind == kind && (path.is_none() || t.path == path)).map(|t| t.id);
        match there {
            Some(id) => {
                self.bench.focus(id);
            }
            None => {
                self.bench.open_with(kind, server, Open { path, ..Open::default() });
            }
        }
        self.settle(tx);
    }

    /// Types `command` into a terminal on `server`, and with `enter` runs it: the terminal on
    /// screen when it's that machine's, else its first terminal, else a new one. A terminal
    /// that isn't connected yet gets it once it is.
    pub fn run_on(&mut self, server: &ServerKey, command: &str, enter: bool, tx: &mut Tx) {
        let terminal = |t: &&sshee_app::workbench::Tab| t.kind == TabKind::Terminal && t.server == *server;
        let id = match self.bench.active().filter(terminal).or_else(|| self.bench.tabs().iter().find(terminal)).map(|t| t.id) {
            Some(id) => {
                self.bench.focus(id);
                id
            }
            None => self.bench.open(TabKind::Terminal, server),
        };
        self.settle(tx);
        if self.bench.tab(id).is_some_and(|t| matches!(t.link, Link::Connected { .. })) {
            self.type_into(id, command, enter, tx);
        } else {
            self.screens.palette.pending.insert(id, (command.to_owned(), enter));
        }
    }

    /// The command goes in as a paste, so a shell doesn't run each line of a long one as it
    /// arrives (0.10's `send`); Return after it runs it.
    fn type_into(&mut self, id: TabId, command: &str, enter: bool, tx: &mut Tx) {
        let Some(pane) = self.panes.get_mut(&id) else { return };
        pane.paste(&ClipboardContent::new(command), tx);
        if enter {
            pane.key(&KeyEvent { key: Key::Enter, mods: Mods::default() }, tx);
        }
    }
}

/// The palette, when it's open: a last child of the window's root.
pub fn view(s: &Shell, t: &'static Theme, _cx: &mut Cx<Shell>) -> Option<El> {
    let state = &s.screens.palette;
    if !state.open {
        return None;
    }
    let query = palette::parse(&state.query.text);
    let rows = s.palette_rows();
    // Streamer mode: servers are "Server 2", and names in any other words are dotted.
    let server_name = s.bench.current_server().map(|sv| s.server_shown(&sv.key(), &sv.name).into_owned());
    let server = server_name.as_deref();
    let on_prod = s.bench.current_server().is_some_and(|sv| sv.is_prod());

    let mut field = row().key("palette-in").w_full().h(44.0).px(14.0).gap(10.0).items_center().border_sides([0.0, 0.0, 1.0, 0.0], t.hair).child(icon(Icon::Search, theme::ICON, t.l2));
    if let Some(scope) = query.scope {
        field = field.child(chip(t, scope.name(), t.l2on).key("palette-scope"));
    }
    let hint = s.text_shown(&palette::placeholder(&s.bench)).into_owned();
    let mut input = text_input(&state.query, theme::INPUT)
        .single_line()
        .key("palette-field")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .color(t.l1)
        .placeholder(hint.clone(), t.l2)
        .caret(t.ring, t.ring.alpha(0.35))
        .label(hint)
        .on_edit(|s: &mut Shell| &mut s.screens.palette.query)
        .on_input_key(|s: &mut Shell, ev, tx| s.palette_key(ev, tx))
        .on_input_change(|s: &mut Shell, tx| s.palette_typed(tx));
    if let Some(hot) = state.hot {
        input = input.active_descendant(format!("pal-{hot}"));
    }
    field = field.child(input).child(key_cap(t, "esc", false));

    let mut list = col().key("palette-list").w_full().max_h(420.0).shrink().min_h(0.0).px(6.0).pb(6.0).role("listbox").scroll(ScrollSpec { stick_bottom: false });
    let mut group: Option<Group> = None;
    for (i, r) in rows.iter().enumerate() {
        if group != Some(r.group) {
            group = Some(r.group);
            let mut heading = list_heading(t, &palette::heading(r.group, server)).key(format!("pal-group-{i}"));
            if r.group == Group::OnServer && on_prod {
                heading = heading.gap(7.0).items_center().child(chip(t, "Prod", t.red));
            }
            list = list.child(heading);
        }
        let hot = state.hot == Some(i);
        let (label, note) = match &r.action {
            Action::Server(key) if s.streaming() => (s.server_shown(key, &r.label), std::borrow::Cow::Borrowed("")),
            _ => (s.text_shown(&r.label), s.text_shown(&r.note)),
        };
        // A server in the colour you picked for it.
        let tint = match &r.action {
            Action::Server(key) => crate::screens::server_menu::own_color(s, key),
            _ => None,
        };
        let spec = ListRow {
            icon: Some(glyph(&r.glyph)),
            tint,
            label: &label,
            mono: r.mono,
            note: &note,
            // A command says "Prod" when it's the one Return would run; its group's heading
            // says it for the rest.
            prod: r.prod && (hot || !r.runs()),
            needs_you: r.needs_you,
            // The row Return picks shows the Return key; the others, their own shortcut.
            keys: if hot { Some("↩") } else { r.keys.as_deref() },
            state: if hot { RowState::Hot } else { RowState::Rest },
            ..ListRow::default()
        };
        list = list.child(
            list_row(t, &spec)
                .key(format!("pal-{i}"))
                // The pointer moves the same highlight the arrows do: one, never two.
                .on_pointer_move(move |s: &mut Shell, _| s.screens.palette.hot = Some(i))
                .on_press_tx(move |s: &mut Shell, tx| s.palette_pick(i, tx)),
        );
    }
    if rows.is_empty() {
        list = list.child(row().key("palette-empty").w_full().h(64.0).items_center().justify_center().child(text("Nothing found.").font(theme::TEXT).color(t.l2).single_line()));
    }

    // Under the list: why a pick did nothing, how to run a command no row is chosen for, or
    // how to narrow the list. How to close it is the "esc" cap in the field, said once.
    let running = state.hot.and_then(|hot| rows.get(hot)).is_some_and(Row::runs);
    let foot = match (&state.note, state.hot, server) {
        (Some(note), _, _) => popover_foot(t, note, ""),
        (None, Some(_), Some(server)) if running => popover_foot(t, &format!("↩ types it into the terminal on {server}{}", if on_prod { ", a Production server" } else { "" }), ""),
        (None, None, Some(server)) if !rows.is_empty() => popover_foot(t, &format!("Nothing runs by itself: ↓ to choose, then ↩ runs it on {server}"), ""),
        _ => popover_foot(t, "srv:  cmd:  file:  set:  in front narrows it down", ""),
    };

    let surface = popover(t, WIDTH)
        .key("palette")
        .label_static("Command bar")
        .max_h(520.0)
        .modal_tx(true, |s: &mut Shell, tx| s.close_palette(tx))
        .child(field)
        .child(list)
        .child(foot.key("palette-foot"));
    Some(if s.bench.showing_home() {
        // Over the main menu's own field, so the field you clicked is the one you type in.
        surface.anchored("home-search", Side::Bottom, Align::Center, -44.0)
    } else {
        surface.anchored("title", Side::Bottom, Align::Start, 8.0)
    })
}
