//! The 1.0 window: one top row of tabs for every server, a narrow sidebar of icons, the page
//! (a terminal) and a status bar. `state` holds what it shows and does, `view` draws it, `keys`
//! names keys for the shortcut table, `sheet` is the kit's component sheet. How the pieces fit
//! and how to add a screen: desktop/ARCHITECTURE.md.

pub mod keys;
pub mod sheet;
pub mod split;
pub mod state;
pub mod streamer;
pub mod tabs;
pub mod view;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use rune::{App, AppSpec, Titlebar};
use sshee_app::workbench::{Server, Tab, Workbench};

use crate::terminal::{NoLink, TermLink};
use crate::theme;

pub use state::{Backend, Options, Post, Remote, Shell};

/// The window's title (taskbar, window list). Never a server's name: window lists and Rune's
/// traces carry it.
pub const TITLE: &str = "SSHeesh Preview";

/// The window's first size, the design's.
pub const SIZE: (f32, f32) = (1280.0, 800.0);

/// The smallest the window goes: the bar still holds a tab, the terminal a few dozen columns.
pub const MIN_SIZE: (f32, f32) = (560.0, 320.0);

/// What the program hands the window at the start.
pub struct Launch {
    /// The servers, and any tabs to start with.
    pub bench: Workbench,
    pub backend: Box<dyn Backend>,
    pub options: Options,
}

/// The program's own arguments: what Rune didn't take (`--snap`, `--script` and the rest are
/// Rune's), in order.
pub fn extra_args() -> &'static [String] {
    rune::extra_args()
}

/// A backend with no sessions: what the window has before the program's (Rune asks for a
/// state before `App::init`'s).
struct NoBackend;

impl Backend for NoBackend {
    fn terminal(&mut self, _: Option<&Server>, _: &Tab) -> Box<dyn TermLink> {
        Box::new(NoLink)
    }
}

fn blank() -> Shell {
    Shell::new(Workbench::new(vec![]), Box::new(NoBackend), Options::default(), None)
}

/// Opens the window and returns when it closes. Prints why and exits 1 when there's no display.
pub fn run(launch: Launch) {
    crate::fonts::declare();
    let spec = AppSpec::<Shell> {
        fonts: crate::fonts::FONTS,
        families: crate::fonts::SYSTEM_FAMILIES,
        min_size: Some(MIN_SIZE),
        setup: Some(|s, waker| s.set_waker(waker)),
        poll: Some(|s, clock, tx| s.poll(clock, tx)),
        on_key: Some(|s, ev, tx| s.key(ev, tx)),
        ..AppSpec::new(TITLE, SIZE, blank, view::view)
    };
    // The top row is the title bar: macOS keeps its traffic lights over its start, Windows
    // gets Rune's minimize, maximize and close at its end, Linux keeps the system's frame.
    let app = App::new(spec).titlebar(Titlebar::new(theme::BAR_HEIGHT).traffic_lights(theme::LIGHTS_AT.0, theme::LIGHTS_AT.1));
    let handle = app.handle();
    // Sessions' threads and background work reach the state through this, in order.
    let remote = Remote::new(Arc::new(move |f| handle.update_if(move |s: &mut Shell, tx| f(s, tx))));
    let Launch { bench, backend, options } = launch;
    app.on_focus(|s, focused, tx| s.window_focus(focused, tx))
        .on_ime(|s, ev, tx| s.ime(ev, tx))
        .on_close(|s, _| s.close())
        .init(move || Shell::new(bench, backend, options, Some(remote)))
        .run();
}
