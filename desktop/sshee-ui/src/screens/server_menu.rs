//! A server's menu: a right click on its icon in the sidebar or on its tile on the main menu
//! (0.10's server menu in `Sidebar.tsx`, with its look picker and its "Remove?" question).
//! New terminal and Files; a name of your own; its icon and colour; its Production, Staging
//! or Development label; and taking it out of the list, asked by name first.
//!
//! One popover with four pages, at the point that was clicked:
//!
//! ```text
//! server-menu    modal: Esc and a click outside close it
//!   Menu     sm-0 … rows; arrows move one highlight, Return picks
//!   Rename   sm-name (a field), sm-cancel, sm-save
//!   Look     icon-<id> × 30, color-0 … 5, sm-done
//!   Remove   "Remove web-7731?", sm-cancel (Return), sm-remove
//! ```
//!
//! Nothing here saves: each change is asked of the backend (`sshee_app::servers::Ask`), which
//! keeps it and sends the list back. This computer has no name, look or label to change.

use rune::{col, row, text, Cx, El, Key, KeyEvent, TextEdit, Tx};
use sshee_app::labels::{env_name, env_order, HOST_COLORS, HOST_ICONS};
use sshee_app::model::EnvLabel;
use sshee_app::servers::{removal, Ask};
use sshee_app::workbench::{Server, ServerKey, TabKind};

use crate::icons::{icon, Icon};
use crate::kit::field::{field, input};
use crate::kit::list::{list_row, list_rule, ListRow};
use crate::kit::menu::RowState;
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// Which page the popover shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Menu,
    Rename,
    Look,
    Remove,
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

/// The menu's own state.
#[derive(Default)]
pub struct State {
    pub open: Option<Open>,
    /// The Rename page's field.
    pub name: TextEdit,
}

/// A row of the Menu page.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
    NewTerminal,
    Files,
    Rename,
    Look,
    Env(EnvLabel),
    Remove,
}

/// A server from `~/.ssh/config`: it can be hidden, not removed.
fn from_config(server: &Server) -> bool {
    server.key().starts_with("host:cfg:")
}

/// The rows for a server, in groups (a rule is drawn between two groups).
fn groups(server: &Server) -> Vec<Vec<Item>> {
    let mut groups = vec![vec![Item::NewTerminal, Item::Files]];
    if !server.is_local() {
        groups.push(vec![Item::Rename, Item::Look]);
        groups.push(env_order().into_iter().map(Item::Env).collect());
        groups.push(vec![Item::Remove]);
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
        // The popover itself takes the keys: its rows aren't Tab stops.
        tx.focus("server-menu", false);
        tx.instant();
    }

    /// Closes it; the terminal on screen has the keyboard again.
    pub fn close_server_menu(&mut self, tx: &mut Tx) {
        self.screens.server_menu.open = None;
        self.settle(tx);
    }

    /// The server the menu is open on, while it's still in the list.
    fn menu_server(&self) -> Option<&Server> {
        self.bench.server(&self.screens.server_menu.open.as_ref()?.server)
    }

    /// A key on the Menu page: arrows move the highlight, Return picks.
    fn server_menu_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let Some(items) = self.menu_server().map(|sv| groups(sv).concat()) else { return false };
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

    fn server_menu_pick(&mut self, item: Item, tx: &mut Tx) {
        let Some(server) = self.menu_server().cloned() else { return self.close_server_menu(tx) };
        let key = server.key();
        match item {
            Item::NewTerminal => {
                self.screens.server_menu.open = None;
                self.bench.open(TabKind::Terminal, &key);
                self.settle(tx);
            }
            Item::Files => {
                self.screens.server_menu.open = None;
                self.show_tab(TabKind::Files, &key, None, tx);
            }
            Item::Rename => {
                let menu = &mut self.screens.server_menu;
                if let Some(open) = &mut menu.open {
                    open.page = Page::Rename;
                }
                menu.name = TextEdit::new(server.name.clone());
                // The whole name is selected: typing replaces it.
                menu.name.anchor = 0;
                menu.name.focus = server.name.len();
                menu.name.focused = true;
                tx.instant();
            }
            Item::Look | Item::Remove => {
                if let Some(open) = &mut self.screens.server_menu.open {
                    open.page = if item == Item::Look { Page::Look } else { Page::Remove };
                }
                tx.instant();
            }
            Item::Env(env) => {
                // A click on the label it has takes it off.
                let env = (server.env.as_ref() != Some(&env)).then_some(env);
                self.backend_mut().servers(Ask::SetEnv { server: key, env });
                self.close_server_menu(tx);
            }
        }
    }

    /// The Rename page's Return and its button.
    fn server_menu_rename(&mut self, tx: &mut Tx) {
        let Some(open) = &self.screens.server_menu.open else { return };
        let ask = Ask::Rename { server: open.server.clone(), name: self.screens.server_menu.name.text.trim().to_owned() };
        self.backend_mut().servers(ask);
        self.close_server_menu(tx);
    }

    fn server_menu_look(&mut self, ask: impl FnOnce(ServerKey) -> Ask) {
        if let Some(server) = self.screens.server_menu.open.as_ref().map(|o| o.server.clone()) {
            self.backend_mut().servers(ask(server));
        }
    }

    fn server_menu_remove(&mut self, tx: &mut Tx) {
        if let Some(server) = self.screens.server_menu.open.as_ref().map(|o| o.server.clone()) {
            self.backend_mut().servers(Ask::Remove { server });
        }
        self.close_server_menu(tx);
    }
}

/// The Menu page's rows.
fn menu_page(t: &Theme, server: &Server, hot: Option<usize>) -> (El, f32) {
    let mut page = col().w_full().padding(6.0);
    let mut height = 12.0;
    let mut n = 0;
    for (g, group) in groups(server).into_iter().enumerate() {
        if g > 0 {
            page = page.child(list_rule(t).key(format!("sm-rule-{g}")));
            height += 11.0;
        }
        for item in group {
            let state = if hot == Some(n) { RowState::Hot } else { RowState::Rest };
            let spec = match &item {
                Item::NewTerminal => ListRow { icon: Some(Icon::Terminal), label: "New terminal", state, ..ListRow::default() },
                Item::Files => ListRow { icon: Some(Icon::Folder), label: "Files", state, ..ListRow::default() },
                Item::Rename => ListRow { icon: Some(Icon::Pencil), label: "Rename…", state, ..ListRow::default() },
                Item::Look => ListRow { icon: Some(Icon::Swatch), label: "Icon and colour…", state, ..ListRow::default() },
                Item::Env(env) => ListRow { label: env_name(env), checked: Some(server.env.as_ref() == Some(env)), state, ..ListRow::default() },
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

fn rename_page(t: &Theme, server: &Server, name: &TextEdit) -> El {
    let name = input(t, name, &server.name)
        .key("sm-name")
        .on_edit(|s: &mut Shell| &mut s.screens.server_menu.name)
        .on_submit(|s: &mut Shell, tx| s.server_menu_rename(tx));
    col()
        .w_full()
        .padding(14.0)
        .gap(10.0)
        .child(title(t, &format!("Rename {}", server.name)))
        .child(field(t, name, false))
        .child(text("Only its name in SSHeesh changes. Leave it empty for the name it came with.").font(theme::SMALL).color(t.l2).w_full().wrap())
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(push_button(t, "Cancel", Push::Quiet, true).key("sm-cancel").on_press_tx(|s: &mut Shell, tx| s.close_server_menu(tx)))
                .child(push_button(t, "Rename", Push::Primary, true).key("sm-save").on_press_tx(|s: &mut Shell, tx| s.server_menu_rename(tx))),
        )
}

fn look_page(s: &Shell, t: &Theme, server: &Server) -> El {
    let mut icons = row().w_full().flex_wrap().gap(4.0).role("radiogroup").label_static("Icon");
    for (id, name) in HOST_ICONS {
        let chosen = server.icon == id;
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
            .child(icon(Icon::for_server(id), theme::ICON_RAIL, if chosen { t.l1 } else { t.l2 }));
        b = if chosen { b.fill(t.selrail) } else { b.hover(t.hov2).press(t.down) };
        icons = icons.child(b);
    }
    let own = s.screens.home.colors.get(&server.key()).map(String::as_str).unwrap_or("");
    let mut colors = row().gap(8.0).items_center().role("radiogroup").label_static("Colour");
    for (i, hex) in HOST_COLORS.into_iter().enumerate() {
        let chosen = own.eq_ignore_ascii_case(hex);
        let color = rune::color::hex(hex).unwrap_or(t.l2);
        let mut swatch = rune::el()
            .key(format!("color-{i}"))
            .size(22.0, 22.0)
            .radius(11.0)
            .items_center()
            .justify_center()
            .focusable(true)
            .role("radio")
            .checked(chosen)
            .label(format!("Colour {}", i + 1))
            .on_press_tx(move |s: &mut Shell, _| s.server_menu_look(|server| Ask::SetColor { server, color: hex.to_owned() }))
            .child(rune::el().size(16.0, 16.0).radius(8.0).fill(color));
        if chosen {
            swatch = swatch.border(1.5, t.l1);
        }
        colors = colors.child(swatch);
    }
    col()
        .w_full()
        .padding(14.0)
        .gap(10.0)
        .child(title(t, &format!("Icon and colour for {}", server.name)))
        .child(icons)
        .child(colors)
        .child(row().w_full().justify_end().child(push_button(t, "Done", Push::Primary, true).key("sm-done").default_button().on_press_tx(|s: &mut Shell, tx| s.close_server_menu(tx))))
}

fn remove_page(t: &Theme, server: &Server) -> El {
    let hide = from_config(server);
    let (question, meaning, button) = removal(&server.name, hide);
    let confirm = push_button(t, button, if hide { Push::Primary } else { Push::Destructive }, true).key("sm-remove").on_press_tx(|s: &mut Shell, tx| s.server_menu_remove(tx));
    col()
        .w_full()
        .padding(14.0)
        .gap(8.0)
        .child(title(t, &question))
        .child(text(meaning).font(theme::NOTE).color(t.l2).w_full().wrap())
        .child(
            row()
                .w_full()
                .mt(6.0)
                .gap(8.0)
                .justify_end()
                // Return is Cancel: taking a server away is never the easy key.
                .child(push_button(t, "Cancel", Push::Quiet, true).key("sm-cancel").autofocus().default_button().on_press_tx(|s: &mut Shell, tx| s.close_server_menu(tx)))
                .child(confirm),
        )
}

/// The menu, when it's open: a last child of the window's root.
pub fn view(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    let open = s.screens.server_menu.open.as_ref()?;
    let server = s.bench.server(&open.server)?;
    let (page, width, height) = match open.page {
        Page::Menu => {
            let (page, height) = menu_page(t, server, open.hot);
            (page, 232.0, height)
        }
        Page::Rename => (rename_page(t, server, &s.screens.server_menu.name), 300.0, 170.0),
        Page::Look => (look_page(s, t, server), 252.0, 330.0),
        Page::Remove => (remove_page(t, server), 320.0, 140.0),
    };
    // At the pointer, pulled in from the window's far edges.
    let [w, h] = cx.layout_size("window").unwrap_or([crate::shell::SIZE.0, crate::shell::SIZE.1]);
    let x = open.at.0.min(w - width - 8.0).max(8.0);
    let y = open.at.1.min(h - height - 8.0).max(8.0);
    let mut surface = popover(t, width)
        .key("server-menu")
        .label(format!("{} menu", server.name))
        .absolute(x, y)
        .modal_tx(true, |s: &mut Shell, tx| s.close_server_menu(tx))
        .child(page);
    if open.page == Page::Menu {
        surface = surface.role("menu").focusable(true).no_focus_ring().on_key(|s: &mut Shell, ev, tx| s.server_menu_key(ev, tx));
    }
    Some(surface)
}
