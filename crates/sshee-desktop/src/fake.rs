//! Stand-ins for tests and for systems without notifications: a notifier that records what it
//! was asked ([`Recorder`]), one that does nothing ([`Nothing`]), and a tray that records its
//! menus ([`TrayLog`]).

use std::sync::{Arc, Mutex};

use crate::tray::{Item, TrayHost};
use crate::{Note, Notifier, Support};

/// What a [`Recorder`] was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    Shown(Note),
    Withdrawn(u64),
}

/// A notifier that keeps what it was asked; clones share the record.
#[derive(Clone, Default)]
pub struct Recorder {
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Recorder {
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// What it was asked since the last take.
    pub fn take(&self) -> Vec<Seen> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }
}

impl Notifier for Recorder {
    fn show(&mut self, note: &Note) {
        self.seen.lock().unwrap().push(Seen::Shown(note.clone()));
    }

    fn withdraw(&mut self, tab: u64) {
        self.seen.lock().unwrap().push(Seen::Withdrawn(tab));
    }

    fn support(&self) -> Support {
        Support::Full
    }
}

/// A notifier that shows nothing, and says why.
pub struct Nothing(pub &'static str);

impl Notifier for Nothing {
    fn show(&mut self, _: &Note) {}
    fn withdraw(&mut self, _: u64) {}
    fn support(&self) -> Support {
        Support::None(self.0)
    }
}

/// A tray that keeps every menu and tooltip it was given; clones share them.
#[derive(Clone, Default)]
pub struct TrayLog {
    menus: Arc<Mutex<Vec<Vec<Item>>>>,
    tooltips: Arc<Mutex<Vec<String>>>,
}

impl TrayLog {
    pub fn last_menu(&self) -> Vec<Item> {
        self.menus.lock().unwrap().last().cloned().unwrap_or_default()
    }

    pub fn last_tooltip(&self) -> String {
        self.tooltips.lock().unwrap().last().cloned().unwrap_or_default()
    }

    /// How many times the menu was set.
    pub fn times(&self) -> usize {
        self.menus.lock().unwrap().len()
    }
}

impl TrayHost for TrayLog {
    fn set_menu(&self, items: Vec<Item>) {
        self.menus.lock().unwrap().push(items);
    }

    fn set_tooltip(&self, tooltip: String) {
        self.tooltips.lock().unwrap().push(tooltip);
    }
}
