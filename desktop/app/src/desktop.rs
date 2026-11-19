//! The desktop outside the window, for the live backend: a terminal that needs you while the
//! window is in the background gets a system notification, its click brings the window
//! forward on that tab, and the tray lists the tabs that need you, shows or hides the window
//! and quits. `sshee_desktop` does the notifications and the tray's menu (no Rune); the
//! window's side is `sshee_ui::shell::desktop`. What works where, and why: the doc comment of
//! `crates/sshee-desktop/src/lib.rs`.
//!
//! Nothing starts in a headless run (`--snap`, `--script`): no tray, no notification, no D-Bus.

use std::sync::Arc;

use sshee_app::workbench::{Tab, TabId};
use sshee_desktop::tray::Item;
use sshee_desktop::{AppInfo, Desktop, Event, Note, Sink, TrayHost};
use sshee_ui::shell::desktop::{headless, SystemTray, TrayLine, TrayPick, Waiting};
use sshee_ui::shell::Remote;

/// The live backend's notifications and tray.
pub struct Glue {
    desktop: Desktop,
}

/// What the desktop asks, done to the window through its `Remote`.
fn sink(remote: Remote) -> Sink {
    Arc::new(move |event| match event {
        Event::OpenTab { tab, token } => remote.open_tab(TabId(tab), token),
        Event::Show { token } => remote.show_window(token),
        Event::Hide => remote.hide_window(),
        Event::Quit => remote.quit(),
    })
}

/// Rune's tray as sshee-desktop's tray host.
struct RuneTray(SystemTray);

impl TrayHost for RuneTray {
    fn set_menu(&self, items: Vec<Item>) {
        self.0.set_menu(
            items
                .into_iter()
                .map(|item| match item {
                    Item::Action { id, label, enabled } => TrayLine::Action { id, label, enabled },
                    Item::Separator => TrayLine::Separator,
                })
                .collect(),
        );
    }

    fn set_tooltip(&self, tooltip: String) {
        self.0.set_tooltip(tooltip);
    }
}

/// The tray this system has: the menu bar item on macOS, Rune's elsewhere.
fn tray(app: &AppInfo, sink: Sink) -> Result<Box<dyn TrayHost>, String> {
    #[cfg(target_os = "macos")]
    {
        sshee_desktop::macos::status_item(app, sink).map(|item| Box::new(item) as Box<dyn TrayHost>).ok_or_else(|| "the menu bar item must be made on the main thread".to_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let icon = sshee_desktop::tray::rgba(sshee_desktop::tray::ICON_PNG).ok_or("the tray's icon doesn't read")?;
        let tray = SystemTray::start(&app.id, &app.name, icon, move |pick| {
            let event = match pick {
                TrayPick::Icon { token } => Some(Event::Show { token }),
                TrayPick::Item(id) => sshee_desktop::tray::route(&id, None),
            };
            if let Some(event) = event {
                sink(event);
            }
        })?;
        Ok(Box::new(RuneTray(tray)))
    }
}

impl Glue {
    /// Starts the notifications and the tray; `None` in a headless run.
    pub fn start(remote: Remote) -> Option<Glue> {
        if headless() {
            return None;
        }
        let app = app_info();
        let mut desktop = Desktop::system(app.clone(), sink(remote.clone()));
        match tray(&app, sink(remote)) {
            Ok(tray) => desktop.set_tray(tray),
            Err(e) => eprintln!("ssheesh-preview: no tray icon: {e}"),
        }
        if let sshee_desktop::Support::None(why) = desktop.support() {
            eprintln!("ssheesh-preview: no system notifications: {why}");
        }
        Some(Glue { desktop })
    }

    pub fn notify(&mut self, tab: &Tab, note: &sshee_term::attention::Notification) {
        self.desktop.notify(Note { tab: tab.id.0, title: note.title.clone(), body: note.body.clone() });
    }

    pub fn waiting_changed(&mut self, waiting: &[Waiting]) {
        self.desktop.set_waiting(waiting.iter().map(|w| sshee_desktop::Waiting { tab: w.tab.0, server: w.server.clone(), what: w.what.clone() }).collect());
    }

    pub fn window_shown(&mut self, shown: bool) {
        self.desktop.set_visible(shown);
    }
}

/// The tab a Windows toast's link names (also one that started the app).
pub fn tab_of_link(link: &str) -> Option<TabId> {
    sshee_desktop::link::tab_of(&app_info().scheme, link).map(TabId)
}

/// Whether a launch argument is a notification's link (Windows starts the app with it).
pub fn is_link(arg: &str) -> bool {
    arg.to_ascii_lowercase().starts_with(&format!("{}://", app_info().scheme))
}

/// This copy's single-instance name (Windows: a toast's click starts the program again, and
/// the running one must receive it).
pub fn instance_id() -> String {
    app_info().id
}

/// Who this copy is to the system. The released 1.x is SSHeesh, as 0.10 was: its
/// AppUserModelID is the one 0.10's Start menu shortcut carries (the installer keeps it,
/// `setup::names::APP_ID`), and a toast's link is `ssheesh://`. Test builds stay "SSHeesh
/// Preview", beside it.
pub fn app_info() -> AppInfo {
    if crate::update::build().release {
        AppInfo { name: "SSHeesh".into(), id: crate::setup::names::APP_ID.into(), scheme: "ssheesh".into() }
    } else {
        AppInfo::preview()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_this_copys_notifications() {
        assert_eq!(tab_of_link("ssheesh-preview://notification/12"), Some(TabId(12)));
        assert_eq!(tab_of_link("ssheesh://notification/12"), None);
        assert!(is_link("SSHEESH-PREVIEW://notification/12"));
        assert!(!is_link("--demo") && !is_link("ssheesh://x"));
    }
}
