//! Which tab a notification server's own number stands for, and what its signals mean: the
//! bookkeeping of `linux.rs`, without D-Bus, so it's tested on every system.
//!
//! A freedesktop server numbers each notification. Showing a tab's notification again replaces
//! the one it had (`replaces_id`), so a tab has one at a time. A click is `ActionInvoked(id,
//! "default")`; servers that support it send `ActivationToken(id, token)` just before, for a
//! Wayland compositor to let the window take focus.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::HashMap;

use crate::Event;

/// The actions a notification offers: "default" is a click on the notification itself. Some
/// servers draw it as a button too, named this.
pub const ACTIONS: [&str; 2] = ["default", "Show"];

/// `NotificationClosed`'s reasons (the spec's numbers).
pub const EXPIRED: u32 = 1;

#[derive(Default)]
pub struct Routes {
    by_id: HashMap<u32, u64>,
    by_tab: HashMap<u64, u32>,
    tokens: HashMap<u32, String>,
}

impl Routes {
    /// The server's number of `tab`'s notification, to replace it; 0 for a new one.
    pub fn replaces(&self, tab: u64) -> u32 {
        self.by_tab.get(&tab).copied().unwrap_or(0)
    }

    /// The server showed `tab`'s notification as `id`.
    pub fn shown(&mut self, tab: u64, id: u32) {
        if let Some(old) = self.by_tab.insert(tab, id).filter(|old| *old != id) {
            self.by_id.remove(&old);
            self.tokens.remove(&old);
        }
        self.by_id.insert(id, tab);
    }

    pub fn token(&mut self, id: u32, token: String) {
        if self.by_id.contains_key(&id) && !token.is_empty() {
            self.tokens.insert(id, token);
        }
    }

    /// An action of notification `id`: the click brings its tab forward. Another app's
    /// notification (the signal goes to everyone listening) is none of ours.
    pub fn invoked(&mut self, id: u32, action: &str) -> Option<Event> {
        let tab = *self.by_id.get(&id)?;
        let token = self.tokens.remove(&id);
        ACTIONS.contains(&action).then_some(Event::OpenTab { tab, token })
    }

    /// The server closed notification `id`. One that only timed out may still sit in the
    /// server's list and be clicked there, so it stays known.
    pub fn closed(&mut self, id: u32, reason: u32) {
        if reason == EXPIRED {
            return;
        }
        if let Some(tab) = self.by_id.remove(&id) {
            self.by_tab.remove(&tab);
        }
        self.tokens.remove(&id);
    }

    /// `tab` was seen: the number of its notification to close, if it has one.
    pub fn withdraw(&mut self, tab: u64) -> Option<u32> {
        let id = self.by_tab.remove(&tab)?;
        self.by_id.remove(&id);
        self.tokens.remove(&id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_brings_its_tab_with_the_token_sent_before_it() {
        let mut r = Routes::default();
        assert_eq!(r.replaces(7), 0);
        r.shown(7, 31);
        r.token(31, "wayland-token".into());
        assert_eq!(r.invoked(31, "default"), Some(Event::OpenTab { tab: 7, token: Some("wayland-token".into()) }));
        // The token is used once.
        assert_eq!(r.invoked(31, "Show"), Some(Event::OpenTab { tab: 7, token: None }));
    }

    #[test]
    fn other_apps_notifications_and_other_actions_are_not_ours() {
        let mut r = Routes::default();
        r.shown(7, 31);
        r.token(99, "theirs".into());
        assert_eq!(r.invoked(99, "default"), None);
        assert_eq!(r.invoked(31, "reply"), None);
    }

    #[test]
    fn a_tab_has_one_notification_at_a_time() {
        let mut r = Routes::default();
        r.shown(7, 31);
        assert_eq!(r.replaces(7), 31);
        // A server that doesn't replace gives a new number: the old one is forgotten.
        r.shown(7, 32);
        assert_eq!(r.invoked(31, "default"), None);
        assert_eq!(r.invoked(32, "default"), Some(Event::OpenTab { tab: 7, token: None }));
    }

    #[test]
    fn a_dismissed_one_is_forgotten_a_timed_out_one_is_not() {
        let mut r = Routes::default();
        r.shown(7, 31);
        r.closed(31, EXPIRED);
        assert_eq!(r.invoked(31, "default"), Some(Event::OpenTab { tab: 7, token: None }));
        r.closed(31, 2);
        assert_eq!((r.invoked(31, "default"), r.replaces(7)), (None, 0));
    }

    #[test]
    fn a_seen_tab_closes_its_notification() {
        let mut r = Routes::default();
        r.shown(7, 31);
        assert_eq!(r.withdraw(7), Some(31));
        assert_eq!(r.withdraw(7), None);
        assert_eq!(r.invoked(31, "default"), None);
    }
}
