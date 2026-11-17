//! The Linux notifier against a notification server on a real session bus: the made-up one in
//! `fake_notifications.py`, which "clicks" every notification it's shown. Ignored by default
//! (it needs that server running); how to run it is in the script's doc comment.
#![cfg(target_os = "linux")]

use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

use sshee_desktop::{AppInfo, Desktop, Event, Note, Support};

#[test]
#[ignore = "needs a session bus with fake_notifications.py on it"]
fn a_click_on_a_shown_notification_comes_back_with_its_tab_and_token() {
    let (send, clicks) = channel();
    let mut desktop = Desktop::system(AppInfo::preview(), Arc::new(move |event| send.send(event).unwrap()));
    desktop.notify(Note { tab: 42, title: "web-7731 needs you".into(), body: "Allow <b>edits</b>?".into() });
    let event = clicks.recv_timeout(Duration::from_secs(10)).expect("no click came back");
    assert_eq!(event, Event::OpenTab { tab: 42, token: Some("fake-activation-token".into()) });
    assert_eq!(desktop.support(), Support::Full);
    // A second note for the same tab replaces the first, and is clicked the same way.
    desktop.notify(Note { tab: 42, title: "web-7731 needs you".into(), body: "Again".into() });
    assert_eq!(clicks.recv_timeout(Duration::from_secs(10)).unwrap(), Event::OpenTab { tab: 42, token: Some("fake-activation-token".into()) });
}
