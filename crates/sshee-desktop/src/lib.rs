//! SSHeesh's place on the desktop outside its window, for 1.0: the system notification for a
//! terminal that needs you, whose click comes back to the app, and the tray icon (Windows'
//! notification area, a Linux StatusNotifierItem, the macOS menu bar). No Rune here: the window
//! side (raising the window, switching to the tab, Rune's own tray on Windows and Linux) is
//! `sshee-ui`'s `shell::desktop`; the program wires the two in `desktop/app/src/live.rs`.
//!
//! ```text
//!  window ── Note { tab, title, body } ──▶ Desktop::notify ──▶ the system's notifier
//!                                                                  │ a click
//!  window ◀── Event::OpenTab { tab, token } ◀── Sink ◀─────────────┘
//!  window ── Waiting (tabs that need you) ──▶ Desktop::set_waiting ──▶ the tray's menu,
//!                                                                     withdrawn notifications
//!  tray item picked ──▶ tray::route(id) ──▶ Event::{Show, Hide, OpenTab, Quit} ──▶ Sink
//! ```
//!
//! How each system reports the click:
//! - **Linux**: `org.freedesktop.Notifications` with a `default` action; the server's
//!   `ActionInvoked` (and, before it, `ActivationToken`, which lets a Wayland compositor give
//!   the window focus) reaches the connection that showed it (`linux.rs`).
//! - **Windows**: a toast whose activation is a link, `ssheesh-preview://notification/<tab>`.
//!   Windows starts the program with it; Rune's single-instance pipe hands it to the running
//!   one (`App::on_open_url`), which also lets it take the foreground. This works from the
//!   pop-up and from the notification centre, also after the pop-up is gone (`win.rs`).
//! - **macOS**: `UNUserNotificationCenter`, whose delegate hears the click. macOS allows it
//!   only for a program inside an `.app` bundle; a bare binary (`cargo run`) gets no
//!   notification, and the Dock icon bounces instead (`macos.rs`).
//!
//! The text of a note is already safe to leave the app (secrets dotted, names hidden while
//! streaming: `sshee_term::attention`); each system's markup is escaped here.

pub mod escape;
pub mod fake;
pub mod link;
mod routes;
pub mod tray;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
mod win;

use std::collections::BTreeSet;
use std::sync::Arc;

pub use tray::{TrayHost, Waiting};

/// Who the app is, to the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    /// What notifications and the tray call it: "SSHeesh Preview".
    pub name: String,
    /// A stable id: Windows' AppUserModelID, the tray's id on Linux, the single-instance name.
    pub id: String,
    /// The link scheme a Windows toast's click opens (`ssheesh-preview`).
    pub scheme: String,
}

impl AppInfo {
    /// SSHeesh Preview, the 1.0 window beside the released app: its own names, so it never
    /// shares a notification group, a tray id or a link scheme with 0.10.
    pub fn preview() -> AppInfo {
        AppInfo { name: "SSHeesh Preview".into(), id: "app.ssheesh.preview".into(), scheme: "ssheesh-preview".into() }
    }
}

/// A system notification for one tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The window's id of the tab (`TabId.0`): a click brings it forward.
    pub tab: u64,
    pub title: String,
    pub body: String,
}

/// What the desktop asks of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A notification was clicked, or a tray item named a tab: bring the window forward on
    /// that tab. `token`: the activation token a Wayland compositor wants for focus.
    OpenTab { tab: u64, token: Option<String> },
    /// The tray's icon or its "Show" item: bring the window forward as it is.
    Show { token: Option<String> },
    /// The tray's "Hide": the window goes, the app and its sessions stay.
    Hide,
    /// The tray's "Quit".
    Quit,
}

/// Where events go: called on whatever thread heard them (a D-Bus thread, the main thread on
/// macOS), so it hands them on (the window's `Remote`) and returns.
pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

/// What this system can do here, for Settings and the logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Notifications show, and a click comes back.
    Full,
    /// Notifications show, a click doesn't come back (a Linux server without actions).
    NoClick,
    /// None show: the reason, in a sentence.
    None(&'static str),
}

/// A system's notifications: the platform's, or a test's ([`fake::Recorder`]).
pub trait Notifier {
    /// Shows `note`, replacing the one shown for the same tab.
    fn show(&mut self, note: &Note);
    /// Takes the tab's notification away: you've seen the tab.
    fn withdraw(&mut self, tab: u64);
    fn support(&self) -> Support;
}

/// The desktop side of one running app: its notifications and its tray.
pub struct Desktop {
    app: AppInfo,
    notifier: Box<dyn Notifier>,
    tray: Option<Box<dyn TrayHost>>,
    /// Tabs that need you, as the window last said.
    waiting: Vec<Waiting>,
    /// The window is on screen (the tray offers Hide, else Show).
    visible: bool,
    /// Tabs whose notification may still be up.
    up: BTreeSet<u64>,
}

impl Desktop {
    pub fn new(app: AppInfo, notifier: Box<dyn Notifier>) -> Desktop {
        Desktop { app, notifier, tray: None, waiting: Vec::new(), visible: true, up: BTreeSet::new() }
    }

    /// This system's notifications; clicks go to `sink`. Starts what they need (a D-Bus
    /// connection on a thread, the toast registration) and never blocks on it.
    pub fn system(app: AppInfo, sink: Sink) -> Desktop {
        let notifier = system_notifier(&app, sink);
        Desktop::new(app, notifier)
    }

    pub fn app(&self) -> &AppInfo {
        &self.app
    }

    pub fn support(&self) -> Support {
        self.notifier.support()
    }

    /// Shows a tab's notification.
    pub fn notify(&mut self, note: Note) {
        self.up.insert(note.tab);
        self.notifier.show(&note);
    }

    /// The tabs that need you now: the tray's list, and the notifications of tabs that no
    /// longer wait (you looked) go away.
    pub fn set_waiting(&mut self, waiting: Vec<Waiting>) {
        if waiting == self.waiting {
            return;
        }
        let still: BTreeSet<u64> = waiting.iter().map(|w| w.tab).collect();
        let seen: Vec<u64> = self.waiting.iter().map(|w| w.tab).filter(|tab| !still.contains(tab) && self.up.contains(tab)).collect();
        for tab in seen {
            self.up.remove(&tab);
            self.notifier.withdraw(tab);
        }
        self.waiting = waiting;
        self.refresh_tray();
    }

    /// Whether the window is on screen: the tray's first item says Show or Hide.
    pub fn set_visible(&mut self, visible: bool) {
        if visible != self.visible {
            self.visible = visible;
            self.refresh_tray();
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The tray to keep up to date; it gets the menu at once.
    pub fn set_tray(&mut self, tray: Box<dyn TrayHost>) {
        self.tray = Some(tray);
        self.refresh_tray();
    }

    pub fn has_tray(&self) -> bool {
        self.tray.is_some()
    }

    /// The tray's menu as it is now.
    pub fn menu(&self) -> Vec<tray::Item> {
        tray::menu(&self.app.name, self.visible, &self.waiting)
    }

    fn refresh_tray(&self) {
        if let Some(tray) = &self.tray {
            tray.set_menu(self.menu());
            tray.set_tooltip(tray::tooltip(&self.app.name, &self.waiting));
        }
    }
}

#[cfg(target_os = "linux")]
fn system_notifier(app: &AppInfo, sink: Sink) -> Box<dyn Notifier> {
    Box::new(linux::Freedesktop::start(app, sink))
}

#[cfg(windows)]
fn system_notifier(app: &AppInfo, sink: Sink) -> Box<dyn Notifier> {
    // A click comes back as a link, through the window's own link handler, not the sink.
    let _ = sink;
    Box::new(win::Toasts::start(app))
}

#[cfg(target_os = "macos")]
fn system_notifier(app: &AppInfo, sink: Sink) -> Box<dyn Notifier> {
    macos::notifier(app, sink)
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn system_notifier(app: &AppInfo, sink: Sink) -> Box<dyn Notifier> {
    let _ = (app, sink);
    Box::new(fake::Nothing("This system has no notifications SSHeesh knows how to show."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Recorder, Seen};
    use crate::tray::Item;

    fn waiting(tab: u64, server: &str) -> Waiting {
        Waiting { tab, server: server.into(), what: "Terminal".into() }
    }

    fn desktop() -> (Desktop, Recorder) {
        let recorder = Recorder::default();
        (Desktop::new(AppInfo::preview(), Box::new(recorder.clone())), recorder)
    }

    #[test]
    fn a_note_is_shown_with_its_tab() {
        let (mut d, rec) = desktop();
        d.notify(Note { tab: 7, title: "web-7731 needs you".into(), body: "Allow edits?".into() });
        assert_eq!(rec.seen(), [Seen::Shown(Note { tab: 7, title: "web-7731 needs you".into(), body: "Allow edits?".into() })]);
    }

    #[test]
    fn a_tab_you_looked_at_takes_its_notification_away_once() {
        let (mut d, rec) = desktop();
        d.set_waiting(vec![waiting(7, "web-7731"), waiting(9, "db-0412")]);
        d.notify(Note { tab: 7, title: "a".into(), body: String::new() });
        rec.take();
        // Tab 9 had no notification: nothing to take away. Tab 7 had one.
        d.set_waiting(vec![]);
        assert_eq!(rec.take(), [Seen::Withdrawn(7)]);
        d.set_waiting(vec![waiting(7, "web-7731")]);
        d.set_waiting(vec![]);
        assert_eq!(rec.take(), []);
    }

    #[test]
    fn the_tray_follows_the_waiting_tabs_and_the_window() {
        let (mut d, _) = desktop();
        let tray = fake::TrayLog::default();
        d.set_tray(Box::new(tray.clone()));
        assert_eq!(tray.last_tooltip(), "SSHeesh Preview");
        d.set_waiting(vec![waiting(7, "web-7731")]);
        assert_eq!(tray.last_tooltip(), "SSHeesh Preview: web-7731 needs you");
        assert!(tray.last_menu().contains(&Item::action("tab:7", "web-7731 needs you")));
        d.set_visible(false);
        assert_eq!(tray.last_menu()[0], Item::action("show", "Show SSHeesh Preview"));
        // Nothing changed: the tray isn't told again.
        let told = tray.times();
        d.set_visible(false);
        d.set_waiting(vec![waiting(7, "web-7731")]);
        assert_eq!(tray.times(), told);
    }
}
