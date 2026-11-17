//! Toasts on Windows, from a thread of its own (WinRT wants COM set up on the thread that calls
//! it; Rune's UI thread is left alone). Named `win`: a `windows` module would shadow the crate.
//!
//! A program that isn't installed from a package shows toasts under an AppUserModelID it
//! registers itself: `HKCU\Software\Classes\AppUserModelId\<id>` with the name Windows shows
//! (what the Windows App SDK does for unpackaged apps). A toast's click is a link,
//! `ssheesh-preview://notification/<tab>` (`activationType="protocol"`), registered under
//! `HKCU\Software\Classes\ssheesh-preview` to start this executable: Windows starts it, Rune's
//! single-instance pipe passes the link to the running program and lets it take the
//! foreground, and its `App::on_open_url` brings the tab forward. That works from the pop-up
//! and from the notification centre, after the pop-up is gone, and even after the app quit
//! (it starts again). Everything goes under the user's own key: no administrator.

use std::sync::mpsc::{channel, Sender};

use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::{escape, link, AppInfo, Note, Notifier, Support};

/// Every SSHeesh toast is in this group: one per tab, by its tag.
const GROUP: &str = "attention";
const TITLE_MAX: usize = 120;
const BODY_MAX: usize = 400;

enum Command {
    Show(Note),
    Withdraw(u64),
}

pub(crate) struct Toasts {
    commands: Option<Sender<Command>>,
}

impl Toasts {
    pub(crate) fn start(app: &AppInfo) -> Toasts {
        let (send, receive) = channel::<Command>();
        let app = app.clone();
        let spawned = std::thread::Builder::new().name("notifications".into()).spawn(move || {
            // SAFETY: once, on this thread of ours, before any WinRT call on it.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if let Err(e) = register(&app) {
                eprintln!("ssheesh: notifications weren't registered: {e}");
            }
            let id = HSTRING::from(app.id.as_str());
            let Ok(notifier) = ToastNotificationManager::CreateToastNotifierWithId(&id) else { return };
            while let Ok(command) = receive.recv() {
                match command {
                    Command::Show(note) => {
                        let shown = (|| {
                            let doc = XmlDocument::new()?;
                            doc.LoadXml(&HSTRING::from(toast_xml(&app.scheme, &note)))?;
                            let toast = ToastNotification::CreateToastNotification(&doc)?;
                            toast.SetTag(&HSTRING::from(tag(note.tab)))?;
                            toast.SetGroup(&HSTRING::from(GROUP))?;
                            notifier.Show(&toast)
                        })();
                        if let Err(e) = shown {
                            eprintln!("ssheesh: a notification wasn't shown: {e}");
                        }
                    }
                    Command::Withdraw(tab) => {
                        if let Ok(history) = ToastNotificationManager::History() {
                            let _ = history.RemoveGroupedTagWithId(&HSTRING::from(tag(tab)), &HSTRING::from(GROUP), &id);
                        }
                    }
                }
            }
        });
        Toasts { commands: spawned.ok().map(|_| send) }
    }
}

impl Notifier for Toasts {
    fn show(&mut self, note: &Note) {
        if let Some(c) = &self.commands {
            let _ = c.send(Command::Show(note.clone()));
        }
    }

    fn withdraw(&mut self, tab: u64) {
        if let Some(c) = &self.commands {
            let _ = c.send(Command::Withdraw(tab));
        }
    }

    fn support(&self) -> Support {
        if self.commands.is_some() {
            Support::Full
        } else {
            Support::None("The notifications' thread couldn't start.")
        }
    }
}

/// A tab's toast's tag (at most 64 characters, as Windows wants).
fn tag(tab: u64) -> String {
    format!("tab-{tab}")
}

/// The toast: its title, its text, and the link its click opens.
pub(crate) fn toast_xml(scheme: &str, note: &Note) -> String {
    format!(
        "<toast launch=\"{}\" activationType=\"protocol\"><visual><binding template=\"ToastGeneric\"><text hint-maxLines=\"1\">{}</text><text>{}</text></binding></visual></toast>",
        escape::xml(&link::for_tab(scheme, note.tab)),
        escape::xml(&escape::clip(&note.title, TITLE_MAX)),
        escape::xml(&escape::clip(&note.body, BODY_MAX)),
    )
}

/// The AppUserModelID's name and the link scheme, under the user's own classes. Written at
/// every start: the executable may have moved.
fn register(app: &AppInfo) -> windows::core::Result<()> {
    let classes = windows_registry::CURRENT_USER.create("Software\\Classes")?;
    let id = classes.create(format!("AppUserModelId\\{}", app.id))?;
    id.set_string("DisplayName", &app.name)?;
    let exe = std::env::current_exe().map_err(|e| windows::core::Error::new(windows::core::HRESULT(-2147467259), e.to_string()))?;
    let exe = exe.to_string_lossy();
    let scheme = classes.create(&app.scheme)?;
    scheme.set_string("", format!("URL:{}", app.name))?;
    scheme.set_string("URL Protocol", "")?;
    classes.create(format!("{}\\DefaultIcon", app.scheme))?.set_string("", format!("\"{exe}\",0"))?;
    // A crafted link with a quote in it can split into more arguments: the app takes a launch
    // with a link as that link alone and ignores the rest (ssheesh-preview's `Args::of_launch`).
    classes.create(format!("{}\\shell\\open\\command", app.scheme))?.set_string("", format!("\"{exe}\" \"%1\""))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_opens_its_tabs_link_and_its_text_stays_text() {
        let note = Note { tab: 42, title: "web-7731 needs you".into(), body: "<b>Allow</b> & go?".into() };
        let xml = toast_xml("ssheesh-preview", &note);
        assert!(xml.starts_with("<toast launch=\"ssheesh-preview://notification/42\" activationType=\"protocol\">"));
        assert!(xml.contains("&lt;b&gt;Allow&lt;/b&gt; &amp; go?"));
    }
}
