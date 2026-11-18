//! A server's menu: a right click on its icon in the sidebar or on its tile on the main menu
//! (0.10's server menu in `Sidebar.tsx`, with its look picker and its "Remove?" question).
//! New terminal, Files, the two side by side, Stats, Containers and its tunnels; its address
//! to copy; a name of your own; its icon, colour or picture; its Production, Staging or
//! Development label; forgetting its saved password; and taking it out of the list, asked by
//! name first.
//!
//! One popover with three pages, at the point that was clicked:
//!
//! ```text
//! server-menu    modal: Esc and a click outside close it
//!   Menu     sm-0 … rows; arrows move one highlight, Return picks
//!   Rename   sm-name (a field), sm-cancel, sm-save
//!   Look     sm-auto, sm-check · icon-<id> × 30 · color-0 … 5, more-0 … 11, sm-hex ·
//!            sm-picture, sm-no-picture · sm-error · sm-done
//! ```
//!
//! Removing a server and forgetting its password ask first, in the window's own question in
//! the middle (`Shell::confirm`): "Remove web-7731?", dialog-no (Return), dialog-yes.
//!
//! Nothing here saves: each change is asked of the backend (`sshee_app::servers::Ask`), which
//! keeps it and sends the list back. A change that fails says so: on the page while the menu
//! is open, else in a toast. This computer has no name, look or label to change.
//!
//! Every server's face (its look, its picture ready to draw, whether a password is saved) is
//! kept here too ([`State::faces`]): the sidebar draws its icon in its colour, or its picture
//! (never in streamer mode), with [`glyph`]. The first time a terminal on a server connects,
//! SSHeesh looks at what runs there to guess its icon ([`linked`], `sshee_app::roles`).

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use rune::{col, row, text, Color, Cx, El, FileDialogOptions, Key, KeyEvent, SurfaceFit, TextEdit, Tx};
use sshee_app::labels::{env_name, env_order, Face, HOST_COLORS, HOST_ICONS, MORE_COLORS};
use sshee_app::model::EnvLabel;
use sshee_app::roles::{automatic_words, Watch};
use sshee_app::servers::{host_id, removal, Ask};
use sshee_app::workbench::{Link, Server, ServerKey, TabId, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::field::{code_input, field, input};
use crate::kit::list::{list_row, list_rule, ListRow};
use crate::kit::menu::RowState;
use crate::kit::popover::popover;
use crate::kit::push::{icon_button, push_button, Push};
use crate::shell::{Confirm, Shell, Toast};
use crate::theme::{self, Theme};

/// Which page the popover shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Menu,
    Rename,
    Look,
}

/// The menu, open on one server.
pub struct Open {
    pub server: ServerKey,
    /// Where it was asked for, in window points.
    pub at: (f32, f32),
    pub page: Page,
    /// The highlighted row of the Menu page, among the rows that can be picked.
    pub hot: Option<usize>,
}

/// The menu's own state, and every server's face.
#[derive(Default)]
pub struct State {
    pub open: Option<Open>,
    /// The Rename page's field.
    pub name: TextEdit,
    /// The Look page's "your own colour" field.
    pub hex: TextEdit,
    /// What went wrong with the last change, on the page.
    pub error: Option<String>,
    /// Every server's look, picture and saved password, by key (`Shell::set_server_faces`).
    pub faces: IndexMap<ServerKey, Face>,
    /// The pictures, ready for Rune, by key.
    pictures: HashMap<ServerKey, rune::Pixels>,
    /// When to look at a server to guess its icon.
    watch: Watch,
    /// Servers a "Check the server now" couldn't read.
    unchecked: HashSet<ServerKey>,
}

/// A row of the Menu page.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
    NewTerminal,
    Files,
    SideBySide,
    Stats,
    Containers,
    Tunnels,
    CopyAddress,
    Rename,
    Look,
    Env(EnvLabel),
    ForgetPassword,
    Remove,
}

/// A server from `~/.ssh/config`: it can be hidden, not removed.
fn from_config(server: &Server) -> bool {
    server.key().starts_with("host:cfg:")
}

/// The rows for a server, in groups (a rule is drawn between two groups). Its address isn't
/// offered in streamer mode; "Forget saved password" only when one is saved.
fn groups(server: &Server, saved_password: bool, streaming: bool) -> Vec<Vec<Item>> {
    let mut groups = vec![vec![Item::NewTerminal, Item::Files, Item::SideBySide, Item::Stats, Item::Containers]];
    if !server.is_local() {
        groups[0].push(Item::Tunnels);
        let mut own = vec![Item::Rename, Item::Look];
        if !streaming && !server.address.is_empty() {
            own.insert(0, Item::CopyAddress);
        }
        groups.push(own);
        groups.push(env_order().into_iter().map(Item::Env).collect());
        let mut leave = vec![Item::Remove];
        if saved_password {
            leave.insert(0, Item::ForgetPassword);
        }
        groups.push(leave);
    }
    groups
}

impl Shell {
    /// A right click on a server: its menu, at the pointer.
    pub fn open_server_menu(&mut self, server: &str, at: (f32, f32), tx: &mut Tx) {
        if self.bench.server(server).is_none() {
            return;
        }
        self.screens.close_all();
        self.screens.server_menu.open = Some(Open { server: server.to_owned(), at, page: Page::Menu, hot: None });
        self.screens.server_menu.error = None;
        // The popover itself takes the keys: its rows aren't Tab stops.
        tx.focus("server-menu", false);
        tx.instant();
    }

    /// Closes it; the terminal on screen has the keyboard again.
    pub fn close_server_menu(&mut self, tx: &mut Tx) {
        self.screens.server_menu.open = None;
        self.screens.server_menu.error = None;
        self.settle(tx);
    }

    /// Every server's look, picture and saved password, from the backend with each new list.
    pub fn set_server_faces(&mut self, faces: IndexMap<ServerKey, Face>) {
        let state = &mut self.screens.server_menu;
        let mut pictures = HashMap::new();
        for (key, face) in &faces {
            let Some(picture) = &face.picture else { continue };
            // The same picture as before keeps its pixels.
            let same = state.faces.get(key).and_then(|f| f.picture.as_ref()) == Some(picture);
            let pixels = match state.pictures.remove(key).filter(|_| same) {
                Some(pixels) => pixels,
                None => {
                    let pixels = rune::Pixels::new();
                    pixels.set(picture.width, picture.height, picture.rgba.to_vec());
                    pixels
                }
            };
            pictures.insert(key.clone(), pixels);
        }
        state.pictures = pictures;
        state.faces = faces;
    }

    /// The server the menu is open on, while it's still in the list.
    fn menu_server(&self) -> Option<&Server> {
        self.bench.server(&self.screens.server_menu.open.as_ref()?.server)
    }

    fn menu_items(&self) -> Option<Vec<Item>> {
        let server = self.menu_server()?;
        let saved = self.screens.server_menu.faces.get(&server.key()).is_some_and(|f| f.saved_password);
        Some(groups(server, saved, self.streaming()).concat())
    }

    /// A key on the Menu page: arrows move the highlight, Return picks.
    fn server_menu_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let Some(items) = self.menu_items() else { return false };
        let Some(open) = self.screens.server_menu.open.as_mut().filter(|o| o.page == Page::Menu) else { return false };
        match ev.key {
            Key::Down | Key::Up => {
                open.hot = sshee_app::palette::step(items.len(), open.hot, if ev.key == Key::Down { 1 } else { -1 });
                true
            }
            Key::Enter => {
                if let Some(item) = open.hot.and_then(|hot| items.get(hot)).cloned() {
                    self.server_menu_pick(item, tx);
                }
                true
            }
            _ => false,
        }
    }

    /// "Terminal and files side by side": a terminal of the server's (the one you were on there,
    /// else a new one) with its Files beside it, and you're in the terminal.
    pub fn terminal_and_files(&mut self, server: &str, tx: &mut Tx) {
        let last = self.bench.go_to(server);
        let terminal = match self.bench.tab(last).map(|t| t.kind) {
            Some(TabKind::Terminal) => last,
            _ => self.bench.open(TabKind::Terminal, server),
        };
        let files = self.bench.open_files(server);
        if !self.bench.together(terminal, files) {
            self.bench.pair(terminal, files);
        }
        self.bench.focus(terminal);
        self.settle(tx);
    }

    fn server_menu_pick(&mut self, item: Item, tx: &mut Tx) {
        let Some(server) = self.menu_server().cloned() else { return self.close_server_menu(tx) };
        let key = server.key();
        match item {
            Item::NewTerminal => {
                self.screens.server_menu.open = None;
                self.bench.open(TabKind::Terminal, &key);
                self.settle(tx);
            }
            Item::Files | Item::Stats | Item::Containers => {
                self.screens.server_menu.open = None;
                let kind = match item {
                    Item::Files => TabKind::Files,
                    Item::Stats => TabKind::Stats,
                    _ => TabKind::Containers,
                };
                self.show_tab(kind, &key, None, tx);
            }
            Item::SideBySide => {
                self.screens.server_menu.open = None;
                self.terminal_and_files(&key, tx);
            }
            Item::Tunnels => {
                self.screens.server_menu.open = None;
                self.open_tunnels(&key, tx);
            }
            Item::CopyAddress => {
                tx.copy(server.address.clone());
                self.toast(Toast::info("Copied the address"));
                self.close_server_menu(tx);
            }
            Item::Rename => {
                // Streamer mode: the field starts empty rather than show the name.
                let start = if self.streaming() { String::new() } else { server.name.clone() };
                let menu = &mut self.screens.server_menu;
                if let Some(open) = &mut menu.open {
                    open.page = Page::Rename;
                }
                menu.name = TextEdit::new(start);
                // The whole name is selected: typing replaces it.
                menu.name.anchor = 0;
                menu.name.focus = menu.name.text.len();
                menu.name.focused = true;
                tx.instant();
            }
            Item::Remove => {
                // Asked in the middle of the window, by the name it shows (streamer mode's
                // "Server 2"); Return is Cancel: taking a server away is never the easy key.
                let (question, meaning, button) = removal(&self.server_shown(&key, &server.name), from_config(&server));
                let mut ask = Confirm::new(question, meaning, button);
                if !from_config(&server) {
                    ask = ask.destructive(false);
                }
                self.close_server_menu(tx);
                self.confirm(ask, move |s, _| {
                    s.backend_mut().servers(Ask::Remove { server: key });
                });
            }
            Item::Look => {
                let streaming = self.streaming();
                let menu = &mut self.screens.server_menu;
                if let Some(open) = &mut menu.open {
                    open.page = Page::Look;
                }
                let color = menu.faces.get(&key).filter(|f| f.look.own_color).map(|f| f.look.color.clone()).unwrap_or_default();
                menu.hex = TextEdit::new(if streaming { String::new() } else { color });
                tx.instant();
            }
            Item::Env(env) => {
                // A click on the label it has takes it off.
                let env = (server.env.as_ref() != Some(&env)).then_some(env);
                self.backend_mut().servers(Ask::SetEnv { server: key, env });
                self.close_server_menu(tx);
            }
            Item::ForgetPassword => {
                let name = self.server_shown(&key, &server.name).into_owned();
                let ask = Confirm::new(format!("Forget the saved password for {name}?"), "You'll type it again the next time you connect.", "Forget").destructive(false);
                self.close_server_menu(tx);
                self.confirm(ask, move |s, _| {
                    s.backend_mut().servers(Ask::ForgetPassword { server: key });
                });
            }
        }
    }

    /// The Rename page's Return and its button.
    fn server_menu_rename(&mut self, tx: &mut Tx) {
        let Some(open) = &self.screens.server_menu.open else { return };
        let name = self.screens.server_menu.name.text.trim().to_owned();
        // In streamer mode the field started empty: empty there changes nothing.
        if !(name.is_empty() && self.streaming()) {
            let ask = Ask::Rename { server: open.server.clone(), name };
            self.backend_mut().servers(ask);
        }
        self.close_server_menu(tx);
    }

    fn server_menu_look(&mut self, ask: impl FnOnce(ServerKey) -> Ask) {
        if let Some(server) = self.screens.server_menu.open.as_ref().map(|o| o.server.clone()) {
            self.screens.server_menu.error = None;
            self.backend_mut().servers(ask(server));
        }
    }

    /// The colour typed into the Look page's field, on Return.
    fn server_menu_hex(&mut self) {
        let typed = self.screens.server_menu.hex.text.clone();
        match sshee_app::labels::parse_color(&typed) {
            Some(color) => self.server_menu_look(|server| Ask::SetColor { server, color }),
            None => self.screens.server_menu.error = Some("That isn't a color. Type it as #2389e2.".into()),
        }
    }

    /// "Check the server now", beside "Automatic".
    fn server_menu_check(&mut self) {
        let Some(server) = self.screens.server_menu.open.as_ref().map(|o| o.server.clone()) else { return };
        let menu = &mut self.screens.server_menu;
        menu.unchecked.remove(&server);
        if menu.watch.check_now(&server) {
            self.backend_mut().servers(Ask::Detect { server, first: false });
        }
    }

    /// A change from the menu didn't work: on its page while it's open on that server, else a
    /// toast.
    pub(crate) fn server_menu_refused(&mut self, server: &str, doing: &str, why: &str) {
        let menu = &mut self.screens.server_menu;
        if menu.open.as_ref().is_some_and(|o| o.server == server && o.page != Page::Menu) {
            menu.error = Some(format!("{doing}. {why}"));
        } else {
            self.toast(Toast::error(doing).body(why));
        }
    }

    /// A look at a server is over (`Answer::Detected`).
    pub(crate) fn server_detected(&mut self, server: &str, guessed: bool) {
        let menu = &mut self.screens.server_menu;
        menu.watch.done(server);
        if guessed {
            menu.unchecked.remove(server);
        } else {
            menu.unchecked.insert(server.to_owned());
        }
    }
}

/// A terminal's connection changed: the first time one on a server connects, SSHeesh looks at
/// what runs there to guess its icon, unless you picked one or it guessed before.
pub fn linked(s: &mut Shell, tab: TabId) {
    let Some(t) = s.bench.tab(tab).filter(|t| matches!(t.link, Link::Connected { .. })) else { return };
    let server = t.server.clone();
    let Some(face) = s.screens.server_menu.faces.get(&server) else { return };
    let wants = !face.look.picked && face.look.guessed.is_none() && host_id(&server).is_some();
    if s.screens.server_menu.watch.connected(&server, wants) {
        s.backend_mut().servers(Ask::Detect { server, first: true });
    }
}

/// A server's icon `size` points square, as the sidebar, the main menu's tiles and the menus
/// draw it: its picture (never in streamer mode), else its icon in the colour you picked for
/// it, else in `ink`, the quiet grey of where it's drawn (0.10's `HostIcon`, without the
/// colour 0.10 made up from its name).
pub fn glyph(s: &Shell, server: &Server, size: f32, ink: Color) -> El {
    glyph_of(s, &server.key(), &server.icon, size, ink)
}

/// [`glyph`] for the server keyed `key`, whose icon's id is `icon_id`.
pub fn glyph_of(s: &Shell, key: &str, icon_id: &str, size: f32, ink: Color) -> El {
    if let (Some(pixels), false) = (s.screens.server_menu.pictures.get(key), s.streaming()) {
        return rune::el().size(size, size).radius(size * 0.22).clip_rounded(size * 0.22).pixels_fit(pixels, SurfaceFit::Fill).a11y_hidden();
    }
    icon(Icon::for_server(icon_id), size, own_color(s, key).unwrap_or(ink))
}

/// The colour you picked for a server, if you picked one.
pub fn own_color(s: &Shell, key: &str) -> Option<Color> {
    s.screens.server_menu.faces.get(key).filter(|f| f.look.own_color).and_then(|f| rune::color::hex(&f.look.color))
}

/// "Choose a picture…": the system's picker, then the backend makes the file a small square icon.
fn pick_picture(tx: &mut Tx) {
    let options = FileDialogOptions { title: "Choose a picture".into(), ..Default::default() }.filter("Pictures", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "ico"]);
    tx.open_file(options, |s: &mut Shell, result, _| match result {
        Ok(Some(path)) => s.server_menu_look(|server| Ask::SetImage { server, path }),
        Ok(None) => {}
        Err(e) => s.screens.server_menu.error = Some(format!("Couldn't open the file picker. {e}")),
    });
}

/// The Menu page's rows.
fn menu_page(t: &Theme, server: &Server, saved_password: bool, streaming: bool, hot: Option<usize>) -> (El, f32) {
    let mut page = col().w_full().padding(6.0);
    let mut height = 12.0;
    let mut n = 0;
    for (g, group) in groups(server, saved_password, streaming).into_iter().enumerate() {
        if g > 0 {
            page = page.child(list_rule(t).key(format!("sm-rule-{g}")));
            height += 11.0;
        }
        for item in group {
            let state = if hot == Some(n) { RowState::Hot } else { RowState::Rest };
            let spec = match &item {
                Item::NewTerminal => ListRow { icon: Some(Icon::Terminal), label: "New terminal", state, ..ListRow::default() },
                Item::Files => ListRow { icon: Some(Icon::Folder), label: "Files", state, ..ListRow::default() },
                Item::SideBySide => ListRow { icon: Some(Icon::Sidebar), label: "Terminal and files side by side", state, ..ListRow::default() },
                Item::Stats => ListRow { icon: Some(Icon::Stats), label: "Stats", state, ..ListRow::default() },
                Item::Containers => ListRow { icon: Some(Icon::Cube), label: "Containers", state, ..ListRow::default() },
                Item::Tunnels => ListRow { icon: Some(Icon::Link), label: "Tunnels…", state, ..ListRow::default() },
                Item::CopyAddress => ListRow { icon: Some(Icon::Copy), label: "Copy address", state, ..ListRow::default() },
                Item::Rename => ListRow { icon: Some(Icon::Pencil), label: "Rename…", state, ..ListRow::default() },
                Item::Look => ListRow { icon: Some(Icon::Swatch), label: "Icon and color…", state, ..ListRow::default() },
                Item::Env(env) => ListRow { label: env_name(env), checked: Some(server.env.as_ref() == Some(env)), state, ..ListRow::default() },
                Item::ForgetPassword => ListRow { icon: Some(Icon::Key), label: "Forget saved password", state, ..ListRow::default() },
                Item::Remove => ListRow { icon: Some(Icon::Bin), label: if from_config(server) { "Hide from list…" } else { "Remove server…" }, destructive: true, state, ..ListRow::default() },
            };
            let i = n;
            page = page.child(
                list_row(t, &spec)
                    .key(format!("sm-{n}"))
                    .role("menuitem")
                    .on_pointer_move(move |s: &mut Shell, _| {
                        if let Some(open) = &mut s.screens.server_menu.open {
                            open.hot = Some(i);
                        }
                    })
                    .on_press_tx(move |s: &mut Shell, tx| s.server_menu_pick(item.clone(), tx)),
            );
            height += theme::ROW;
            n += 1;
        }
    }
    (page, height)
}

/// A page's title line.
fn title(t: &Theme, words: &str) -> El {
    text(words).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate()
}

/// The red line of a change that didn't work.
fn error_line(t: &Theme, why: &str) -> El {
    row().key("sm-error").w_full().gap(6.0).items_start().role("alert").child(icon(Icon::Warning, 13.0, t.red).mt(1.0)).child(text(why).font(theme::SMALL).color(t.red).grow(1.0).shrink().min_w(0.0).wrap())
}

fn rename_page(t: &Theme, shown: &str, streaming: bool, name: &TextEdit) -> El {
    let name = input(t, name, if streaming { "New name" } else { shown })
        .key("sm-name")
        .on_edit(|s: &mut Shell| &mut s.screens.server_menu.name)
        .on_submit(|s: &mut Shell, tx| s.server_menu_rename(tx));
    col()
        .w_full()
        .padding(14.0)
        .gap(10.0)
        .child(title(t, &format!("Rename {shown}")))
        .child(field(t, name, false))
        .child(text(if streaming { "Only its name in SSHeesh changes." } else { "Only its name in SSHeesh changes. Leave it empty for the name it came with." }).font(theme::SMALL).color(t.l2).w_full().wrap())
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(push_button(t, "Cancel", Push::Quiet, true).key("sm-cancel").on_press_tx(|s: &mut Shell, tx| s.close_server_menu(tx)))
                .child(push_button(t, "Rename", Push::Primary, true).key("sm-save").on_press_tx(|s: &mut Shell, tx| s.server_menu_rename(tx))),
        )
}

/// A round colour to pick, ringed when it's the one in use.
fn swatch(t: &Theme, key: String, hex: &'static str, name: String, chosen: bool) -> El {
    let color = rune::color::hex(hex).unwrap_or(t.l2);
    let mut b = rune::el()
        .key(key)
        .size(22.0, 22.0)
        .radius(11.0)
        .items_center()
        .justify_center()
        .focusable(true)
        .role("radio")
        .checked(chosen)
        .label(name)
        .on_press_tx(move |s: &mut Shell, _| s.server_menu_look(|server| Ask::SetColor { server, color: hex.to_owned() }))
        .child(rune::el().size(16.0, 16.0).radius(8.0).fill(color));
    if chosen {
        b = b.border(1.5, t.l1);
    }
    b
}

/// The Look page (0.10's `LookPopover`): "Automatic", thirty icons, the six colours and any
/// other, and a picture of your own.
fn look_page(s: &Shell, t: &Theme, server: &Server, shown: &str) -> El {
    let key = server.key();
    let state = &s.screens.server_menu;
    let face = state.faces.get(&key).cloned().unwrap_or_default();
    let look = &face.look;
    let has_picture = look.image.is_some();
    // With "Automatic" or a picture, no icon or colour is marked as picked.
    let picked = look.picked && !has_picture;
    // The icons in the colour you picked; without one, quiet.
    let ink = rune::color::hex(&look.color).filter(|_| look.own_color).unwrap_or(t.l2);

    let words = automatic_words(look.guessed.as_deref(), state.watch.is_looking(&key), state.unchecked.contains(&key));
    let auto_icon = look.guessed.as_deref().unwrap_or("server");
    let mut auto = row()
        .key("sm-auto")
        .h(32.0)
        .px(8.0)
        .gap(8.0)
        .grow(1.0)
        .min_w(0.0)
        .radius(theme::RADIUS)
        .items_center()
        .focusable(true)
        .role("radio")
        .checked(!look.picked)
        .label(words.clone())
        .on_press_tx(|s: &mut Shell, _| s.server_menu_look(|server| Ask::Automatic { server }))
        .child(icon(Icon::for_server(auto_icon), 16.0, ink))
        .child(text(words).font(theme::TEXT).color(t.l1).single_line().truncate().grow(1.0).shrink().min_w(0.0).a11y_hidden());
    auto = if look.picked { auto.hover(t.hov2).press(t.down) } else { auto.fill(t.selrail).child(icon(Icon::Check, 13.0, t.l1)) };
    let check = icon_button(t, Icon::Refresh, if look.guessed.is_some() { "Check the server again" } else { "Check the server now" }, !state.watch.is_looking(&key))
        .key("sm-check")
        .on_press_tx(|s: &mut Shell, _| s.server_menu_check());

    let mut icons = row().w_full().flex_wrap().gap(4.0).role("radiogroup").label_static("Icon");
    for (id, name) in HOST_ICONS {
        let chosen = picked && look.icon == id;
        let mut b = rune::el()
            .key(format!("icon-{id}"))
            .size(32.0, 32.0)
            .radius(theme::RADIUS)
            .items_center()
            .justify_center()
            .focusable(true)
            .role("radio")
            .checked(chosen)
            .label_static(name)
            .help(name)
            .on_press_tx(move |s: &mut Shell, _| s.server_menu_look(|server| Ask::SetIcon { server, icon: id.to_owned() }))
            .child(icon(Icon::for_server(id), theme::ICON_RAIL, ink));
        b = if chosen { b.fill(t.selrail) } else { b.hover(t.hov2).press(t.down) };
        icons = icons.child(b);
    }
    let own = look.color.as_str();
    let is = |hex: &str| picked && look.own_color && own.eq_ignore_ascii_case(hex);
    let colors = row()
        .w_full()
        .flex_wrap()
        .gap(4.0)
        .role("radiogroup")
        .label_static("Color")
        .children(HOST_COLORS.into_iter().enumerate().map(|(i, hex)| swatch(t, format!("color-{i}"), hex, format!("Color {}", i + 1), is(hex))))
        .children(MORE_COLORS.into_iter().enumerate().map(|(i, hex)| swatch(t, format!("more-{i}"), hex, format!("More color {}", i + 1), is(hex))));
    let hex = code_input(t, &state.hex, "#2389e2")
        .key("sm-hex")
        .label_static("Your own color")
        .on_edit(|s: &mut Shell| &mut s.screens.server_menu.hex)
        .on_submit(|s: &mut Shell, _| s.server_menu_hex());
    let own_color = row()
        .w_full()
        .gap(8.0)
        .items_center()
        .child(text("Your own").font(theme::SMALL).color(t.l2).single_line())
        .child(field(t, hex, false).grow(1.0).min_w(0.0))
        .child(push_button(t, "Use", Push::Quiet, true).key("sm-hex-use").on_press_tx(|s: &mut Shell, _| s.server_menu_hex()));

    let mut picture = row()
        .w_full()
        .gap(4.0)
        .items_center()
        .child(push_button(t, if has_picture { "Pick another picture…" } else { "Use a picture…" }, Push::Quiet, true).key("sm-picture").on_press_tx(|_: &mut Shell, tx| pick_picture(tx)));
    if has_picture {
        picture = picture.child(push_button(t, "Remove the picture", Push::Quiet, true).key("sm-no-picture").on_press_tx(|s: &mut Shell, _| s.server_menu_look(|server| Ask::ClearImage { server })));
    }

    let mut page = col()
        .w_full()
        .padding(14.0)
        .gap(10.0)
        .child(title(t, &format!("Icon and color for {shown}")))
        .child(row().w_full().gap(4.0).items_center().child(auto).child(check))
        .child(icons)
        .child(colors)
        .child(own_color)
        .child(picture);
    if let Some(why) = &state.error {
        page = page.child(error_line(t, why));
    }
    page.child(row().w_full().justify_end().child(push_button(t, "Done", Push::Primary, true).key("sm-done").default_button().on_press_tx(|s: &mut Shell, tx| s.close_server_menu(tx))))
}

/// The menu, when it's open: a last child of the window's root.
pub fn view(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let open = s.screens.server_menu.open.as_ref()?;
    let server = s.bench.server(&open.server)?;
    // Its name in titles and questions: "Server 2" in streamer mode.
    let shown = s.server_shown(&open.server, &server.name).into_owned();
    let (page, width, height) = match open.page {
        Page::Menu => {
            let saved = s.screens.server_menu.faces.get(&open.server).is_some_and(|f| f.saved_password);
            let (page, height) = menu_page(t, server, saved, s.streaming(), open.hot);
            (page, 268.0, height)
        }
        Page::Rename => (rename_page(t, &shown, s.streaming(), &s.screens.server_menu.name), 300.0, 170.0),
        Page::Look => (look_page(s, t, server, &shown), 296.0, 470.0),
    };
    // At the pointer, pulled in from the window's far edges.
    let [w, h] = cx.layout_size("window").unwrap_or([crate::shell::SIZE.0, crate::shell::SIZE.1]);
    let x = open.at.0.min(w - width - 8.0).max(8.0);
    let y = open.at.1.min(h - height - 8.0).max(8.0);
    let mut surface = popover(t, width)
        .key("server-menu")
        .label(format!("{shown} menu"))
        .absolute(x, y)
        .modal_tx(true, |s: &mut Shell, tx| s.close_server_menu(tx))
        .child(page);
    if open.page == Page::Menu {
        surface = surface.role("menu").focusable(true).no_focus_ring().on_key(|s: &mut Shell, ev, tx| s.server_menu_key(ev, tx));
    }
    Some(surface)
}
