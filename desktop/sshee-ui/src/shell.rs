//! The 1.0 window: one top row of tabs for every server, a narrow sidebar of icons, the page
//! (a terminal) and a status bar. `state` holds what it shows and does, `view` draws it, `keys`
//! names keys for the shortcut table, `sheet` is the kit's component sheet. How the pieces fit
//! and how to add a screen: desktop/ARCHITECTURE.md.

pub mod compact;
pub mod desktop;
pub mod file_drop;
pub mod keys;
pub mod notes;
mod pane_marks;
mod place;
pub mod rail;
pub mod rail_load;
pub mod sheet;
pub mod split;
pub mod state;
pub mod streamer;
pub mod tabs;
pub mod title;
pub mod view;
pub mod windows;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use rune::{App, AppSpec, Titlebar, WindowSpec};
use sshee_app::workbench::{Server, Tab, Workbench};

use crate::terminal::{NoLink, TermLink};
use crate::theme::{self, Frame};

pub use desktop::{WindowAsk, WindowControl};
pub use notes::{Confirm, Toast, ToastId};
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

/// The window's app: its spec, its frame and title bar, and Rune's handle to it (for its
/// [`Remote`] and the native window). `options.frame` says who draws the window's buttons.
fn window_app(options: &mut Options) -> App<Shell> {
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
    // Windows with Settings' "Use the Windows title bar": the system's frame, as on Linux.
    let app = if options.frame == crate::theme::Frame::Windows && options.native_title_bar {
        options.frame = crate::theme::Frame::Linux;
        App::new(spec)
    } else {
        App::new(spec).titlebar(Titlebar::new(theme::BAR_HEIGHT).traffic_lights(theme::LIGHTS_AT.0, theme::LIGHTS_AT.1))
    };
    // The native window, for where it is when it closes (`place.rs`).
    let told = app.handle();
    let app = app.on_window(move |w| {
        let w = w.clone();
        told.update(move |s: &mut Shell, _| s.window = Some(w));
    });
    app.on_focus(|s, focused, tx| s.window_focus(focused, tx))
        .on_ime(|s, ev, tx| s.ime(ev, tx))
        .on_close(|s, _| s.close())
        // The tray's Quit ends the app without closing the window first.
        .on_quit(|s, _| s.close())
        // A Windows toast's click, handed over by the program Windows started for it.
        .on_open_url(|s, link, tx| s.open_link(link, tx))
}

/// How background work reaches the window `app` will open: through Rune's handle, in order.
/// A notification's click and the tray bring it forward, hide it, quit (`desktop.rs`).
fn window_remote(app: &App<Shell>) -> Remote {
    let handle = app.handle();
    let window = handle.clone();
    let window: WindowControl = Arc::new(move |ask| match ask {
        WindowAsk::Raise { token: Some(token) } => window.activate(token),
        WindowAsk::Raise { token: None } => window.open(),
        WindowAsk::Hide => window.hide(),
        WindowAsk::Quit => window.quit(),
    });
    Remote::new(Arc::new(move |f| handle.update_if(move |s: &mut Shell, tx| f(s, tx)))).with_window(window)
}

/// Opens the window and returns when it closes. Prints why and exits 1 when there's no display.
pub fn run(launch: Launch) {
    let Launch { bench, backend, mut options } = launch;
    let app = window_app(&mut options);
    let remote = window_remote(&app);
    let place = options.place;
    let app = app.init(move || Shell::new(bench, backend, options, Some(remote)));
    place::run(app, place);
}

/// Opens another window from `from`'s, on `server` (`windows.rs`), in the same app: it starts
/// at the end of this turn. False where no window can open (a headless run).
pub(crate) fn open_window(from: &mut Shell, server: &str) -> bool {
    if rune::is_headless() {
        return false;
    }
    let mut options = Options { frame: Frame::current(), ..from.options.clone() };
    let app = window_app(&mut options);
    let remote = window_remote(&app);
    let no = from.hub().next_window();
    let mut shell = from.window_for(no, server, remote);
    shell.options.frame = options.frame;
    rune::open_window(WindowSpec::from_app(format!("server-{no}"), app.init(move || shell)));
    true
}
