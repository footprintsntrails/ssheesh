//! Where the main window was (0.10's window-state): its size, place and whether it was
//! maximized are handed to the backend as it closes (`Backend::window_placed`), and it opens
//! there at the next start (`Options::place`), on a screen that is still there
//! (`sshee_app::windows`). Wayland doesn't tell a window where it is: nothing is kept there.

use rune::{App, WindowOptions, WindowSpec};
use sshee_app::windows::{Placement, Screen, Seen};

use super::state::Shell;

/// Runs `app`, opening where `saved` says when it says anything and this is a real window.
pub(crate) fn run(app: App<Shell>, saved: Option<Placement>) {
    let Some(saved) = saved.filter(|_| !rune::is_headless()) else { return app.run() };
    rune::run_windows(move |displays| {
        let screens: Vec<Screen> = displays.iter().map(|d| Screen { position: d.position, size: d.size, scale: d.scale, primary: d.primary }).collect();
        let at = saved.restore(&screens);
        let app = app.window_options(WindowOptions { position: at.position, physical_size: at.size, ..WindowOptions::default() }).maximized(at.maximized);
        [WindowSpec::from_app("main", app)]
    });
}

/// The window as it is now, to keep for the next start; `None` where the system doesn't say.
pub(crate) fn now(s: &Shell) -> Option<Placement> {
    let p = s.window.as_ref()?.placement()?;
    let seen = Seen {
        position: Some(p.position),
        size: p.content_size,
        scale: p.scale,
        maximized: p.maximized,
        minimized: p.minimized,
        full_screen: p.full_screen,
        restore: p.restore,
    };
    Some(Placement::of(&seen, s.options.place.as_ref()))
}
