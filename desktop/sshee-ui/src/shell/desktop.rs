//! The window as the desktop outside it reaches it: brought forward by a notification's click
//! or the tray, hidden and quit from the tray, and telling the tray which tabs need you. The
//! notifications and the tray's menu are `crates/sshee-desktop`'s (no Rune); the program
//! (`desktop/app/src/desktop.rs`) joins the two. Here are only what needs Rune or the window's
//! state:
//!
//! - [`WindowAsk`], done by Rune's `AppHandle` from any thread ([`Remote::window`]).
//! - [`Shell::open_from_outside`]: the window comes forward on a tab, whatever was open over
//!   it closes, and the tab gets the keyboard. A Windows toast's click arrives as a link
//!   (`App::on_open_url`, [`Shell::open_link`]); the backend says which tab it names.
//! - [`Shell::notify_system`]: the one way to a system notification, so Settings' "Tell me
//!   when a terminal needs me" is honoured for every kind of note.
//! - [`Waiting`]: the tabs that need you, each time the list changes
//!   (`Backend::waiting_changed`), with names as the window shows them (streamer mode).
//! - [`SystemTray`]: Rune's tray (Windows' notification area, a Linux StatusNotifierItem),
//!   for the program to put the menu in. macOS has none in Rune: sshee-desktop draws the menu
//!   bar item itself.

use std::sync::Arc;

use rune::tray::{Tray, TrayEvent, TrayIcon, TrayItem, TraySpec};
use rune::Tx;
use sshee_app::workbench::{Tab, TabId};

use super::state::{Remote, Shell};

/// What can be asked of the window from outside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowAsk {
    /// Show it, restore it if minimized, and bring it in front with the keyboard. `token`:
    /// the activation token a Wayland compositor wants before it gives focus.
    Raise { token: Option<String> },
    /// It goes from the screen; the app and its sessions stay (the tray brings it back).
    Hide,
    /// The app ends, as when its last window closes.
    Quit,
}

/// Does a [`WindowAsk`]: Rune's `AppHandle` in the program, a record in tests.
pub type WindowControl = Arc<dyn Fn(WindowAsk) + Send + Sync>;

/// A tab that needs you, as the tray lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub tab: TabId,
    /// Its server's name as the window shows it (an alias while streaming).
    pub server: String,
    /// The tab's name ("Terminal 2", "Claude Code").
    pub what: String,
}

/// What the window last told the backend.
pub struct State {
    told: Vec<Waiting>,
    shown: bool,
}

impl Default for State {
    fn default() -> State {
        State { told: Vec::new(), shown: true }
    }
}

impl Remote {
    /// The window's own requests (`shell::run` gives Rune's).
    pub fn with_window(mut self, window: WindowControl) -> Remote {
        self.window = Some(window);
        self
    }

    /// Asks it of the window; nothing where there's no window to ask (a test, a headless run).
    /// The backend's: of the window in front.
    pub fn window(&self, ask: WindowAsk) {
        if let Some(window) = &self.window {
            window(ask);
        } else if let Some(door) = self.hub().and_then(|hub| hub.front_door()) {
            door.window(ask);
        }
    }

    /// The backend's: each open window, the one in front last (so it stays in front).
    fn each_window(&self) -> Vec<Remote> {
        match self.hub() {
            None => vec![self.clone()],
            Some(hub) => {
                let front = hub.front();
                let mut doors = hub.doors();
                doors.sort_by_key(|(n, _)| *n == front);
                doors.into_iter().map(|(_, door)| door).collect()
            }
        }
    }

    /// A notification's click, or a tab picked from the tray: the window that has `tab` comes
    /// forward on it.
    pub fn open_tab(&self, tab: TabId, token: Option<String>) {
        let window = match self.hub() {
            None => Some(self.clone()),
            Some(hub) => hub.door(tab.window()),
        };
        let Some(window) = window else { return };
        window.window(WindowAsk::Raise { token });
        window.update_tx(move |s, tx| {
            s.window_shown(true);
            s.open_from_outside(tab, tx);
        });
    }

    /// The tray's icon or its Show: the windows come forward as they are.
    pub fn show_window(&self, token: Option<String>) {
        let windows = self.each_window();
        let last = windows.len().saturating_sub(1);
        for (i, window) in windows.into_iter().enumerate() {
            // A Wayland compositor's token is good once: for the window that ends in front.
            window.window(WindowAsk::Raise { token: token.clone().filter(|_| i == last) });
            window.update(|s| s.window_shown(true));
        }
    }

    /// The tray's Hide: every window.
    pub fn hide_window(&self) {
        for window in self.each_window() {
            window.window(WindowAsk::Hide);
            window.update(|s| s.window_shown(false));
        }
    }

    /// The tray's Quit.
    pub fn quit(&self) {
        self.window(WindowAsk::Quit);
    }
}

impl Shell {
    /// The window came forward for `tab` (a notification's click, the tray): what was open
    /// over it closes, Settings too, and the tab is on screen with the keyboard. A tab that
    /// closed since leaves the window as it was.
    pub fn open_from_outside(&mut self, tab: TabId, tx: &mut Tx) {
        if self.bench.tab(tab).is_none() {
            return;
        }
        self.screens.close_all();
        self.focus_tab(tab, tx);
    }

    /// A link the system opened with this app (`App::on_open_url`): a Windows toast's click.
    /// The window comes forward, on the tab the link names when the backend knows it.
    pub fn open_link(&mut self, link: &str, tx: &mut Tx) {
        let tab = self.backend_mut().tab_of_link(link);
        // Another window's tab: that window comes forward on it.
        if let Some(tab) = tab.filter(|t| t.window() != self.window_no()) {
            Remote::routed(self.hub().clone()).open_tab(tab, None);
            return;
        }
        if let Some(remote) = &self.remote {
            remote.window(WindowAsk::Raise { token: None });
        }
        if let Some(tab) = tab {
            self.open_from_outside(tab, tx);
        }
    }

    /// A system notification for `tab`, unless Settings says not to be told.
    pub(crate) fn notify_system(&mut self, tab: &Tab, note: &sshee_term::attention::Notification) {
        if self.settings.notify_attention() {
            self.backend_mut().notify(tab, note);
        }
    }

    /// The window was shown or hidden (the tray says Hide or Show), or came to the front.
    pub fn window_shown(&mut self, shown: bool) {
        if shown != self.screens.desktop.shown {
            self.screens.desktop.shown = shown;
            self.backend_mut().window_shown(shown);
        }
    }

    /// The tabs that need you, in the row's order.
    pub fn waiting(&self) -> Vec<Waiting> {
        self.bench
            .tabs()
            .iter()
            .filter(|t| t.needs_you)
            .map(|t| {
                let name = self.bench.server(&t.server).map_or("Unknown server", |sv| sv.name.as_str());
                Waiting { tab: t.id, server: self.server_shown(&t.server, name).into_owned(), what: self.text_shown(&t.label()).into_owned() }
            })
            .collect()
    }

    /// Tells the backend the tabs that need you when they changed (after every change and
    /// every frame's poll; comparing is cheap).
    pub(crate) fn sync_waiting(&mut self) {
        let now = self.waiting();
        if now != self.screens.desktop.told {
            self.backend_mut().waiting_changed(&now);
            self.screens.desktop.told = now;
        }
    }
}

/// Whether this run draws no window (Rune's `--snap`, `--script`, `--tree`, ...): no tray,
/// no notifications, no single instance.
pub fn headless() -> bool {
    rune::is_headless()
}

/// Makes this process the app's only one (Windows, Linux), before the window opens: false when
/// another already runs and got this launch's links (a toast's click), so return from `main`.
/// Elsewhere, true.
pub fn single_instance(id: &str) -> bool {
    rune::single_instance(id)
}

/// A line of the tray's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayLine {
    Action { id: String, label: String, enabled: bool },
    Separator,
}

/// What was clicked in the tray.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayPick {
    /// The icon itself (its primary button). `token`: Wayland's activation token.
    Icon { token: Option<String> },
    /// A menu item, by its id.
    Item(String),
}

/// Rune's tray icon: in Windows' notification area, or a Linux StatusNotifierItem (shown
/// where a tray host runs: KDE, most bars; GNOME needs an extension). Its menu and clicks
/// are handled on a thread of its own. Dropping it takes the icon away.
pub struct SystemTray {
    tray: Tray,
}

impl SystemTray {
    /// Puts the icon up: `rgba` is `(width, height, pixels)`. An error where Rune has no tray
    /// (macOS) or it can't start.
    pub fn start(id: &str, title: &str, (width, height, rgba): (u32, u32, Vec<u8>), on_pick: impl Fn(TrayPick) + Send + Sync + 'static) -> Result<SystemTray, String> {
        let spec = TraySpec::new(id, title, TrayIcon::rgba(width, height, rgba)).tooltip(title);
        let tray = Tray::new(spec, move |event| match event {
            TrayEvent::Activate { token } => on_pick(TrayPick::Icon { token }),
            TrayEvent::Menu(id) => on_pick(TrayPick::Item(id)),
            _ => {}
        })
        .map_err(|e| e.to_string())?;
        Ok(SystemTray { tray })
    }

    pub fn set_menu(&self, lines: Vec<TrayLine>) {
        self.tray.set_menu(
            lines
                .into_iter()
                .map(|line| match line {
                    TrayLine::Action { id, label, enabled } => TrayItem::action(id, label).enabled(enabled),
                    TrayLine::Separator => TrayItem::Separator,
                })
                .collect(),
        );
    }

    pub fn set_tooltip(&self, tooltip: String) {
        self.tray.set_tooltip(tooltip);
    }
}
