//! The program's windows, without a screen: where the main window was when it closed, and
//! where it opens at the next start (0.10's window-state plugin: size, place, maximized).
//!
//! Saved in the copy's own folder as `window.json`. A place is kept only while a screen is
//! still there to show it: a window saved on a monitor that's gone opens on one that is, at a
//! size that fits it. Positions and sizes are the desktop's physical pixels, as the system
//! gives them; a size is the content's, without the frame.

use serde::{Deserialize, Serialize};

/// The file, in the copy's own folder.
pub const FILE: &str = "window.json";

/// How much of the window's top edge must be on a screen for the saved place to be kept, in
/// points: enough to grab it and move it.
const GRAB: (f64, f64) = (120.0, 32.0);

/// Smaller than this (in points) was never a real window: the size isn't used.
const SMALLEST: (f64, f64) = (200.0, 120.0);

/// Where the main window was, as it is kept.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    /// The frame's top-left. `None` where the system doesn't say (Wayland).
    #[serde(default)]
    pub position: Option<(i32, i32)>,
    /// The content's size.
    pub size: (u32, u32),
    /// Physical pixels per point on its screen, when it was saved.
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub maximized: bool,
}

fn one() -> f64 {
    1.0
}

/// The window as the system sees it now.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Seen {
    pub position: Option<(i32, i32)>,
    pub size: (u32, u32),
    pub scale: f64,
    pub maximized: bool,
    pub minimized: bool,
    pub full_screen: bool,
    /// While it's maximized or minimized, the frame restoring it returns to, where the system
    /// keeps one (Windows): its top-left and content size.
    pub restore: Option<((i32, i32), (u32, u32))>,
}

/// A screen, as the system lists it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub scale: f64,
    pub primary: bool,
}

/// What the window opens with.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Restore {
    /// `None`: where the system puts a new window.
    pub position: Option<(i32, i32)>,
    /// `None`: the design's size.
    pub size: Option<(u32, u32)>,
    pub maximized: bool,
}

impl Placement {
    /// What to keep of the window as it is now. A maximized, minimized or full-screen window
    /// keeps the frame it returns to: the system's record of it where there is one, else the
    /// one kept `before`, so un-maximizing after the next start doesn't leave a window the size
    /// of the screen. Full screen is kept as maximized.
    pub fn of(now: &Seen, before: Option<&Placement>) -> Placement {
        let scale = if now.scale > 0.0 { now.scale } else { 1.0 };
        let normal = !(now.maximized || now.minimized || now.full_screen);
        // A minimized window opens as it was before it went down.
        let maximized = if now.minimized && !now.maximized { before.is_some_and(|b| b.maximized) } else { now.maximized || now.full_screen };
        if normal {
            return Placement { position: now.position, size: now.size, scale, maximized: false };
        }
        if let Some((position, size)) = now.restore {
            return Placement { position: Some(position), size, scale, maximized };
        }
        match before {
            Some(b) => Placement { maximized, ..*b },
            None => Placement { position: now.position, size: now.size, scale, maximized },
        }
    }

    /// Where the window opens, on `screens` as they are now. The size fits the screen it
    /// opens on (in points, if that screen's scale differs); the place is kept only if enough
    /// of the window's top edge is on a screen to grab it, and is moved so the whole window is
    /// on that screen. With no screens listed (a headless run) nothing is restored.
    pub fn restore(&self, screens: &[Screen]) -> Restore {
        let (w, h) = self.size;
        let saved_scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        if screens.is_empty() || (w as f64) < SMALLEST.0 * saved_scale || (h as f64) < SMALLEST.1 * saved_scale {
            return Restore { maximized: self.maximized, ..Restore::default() };
        }
        let home = self.position.and_then(|at| grabbed(at, (w, h), saved_scale, screens));
        let screen = home.or_else(|| screens.iter().find(|s| s.primary)).unwrap_or(&screens[0]);
        // The same size in points on a screen of another scale.
        let k = if screen.scale > 0.0 { screen.scale / saved_scale } else { 1.0 };
        let size = ((w as f64 * k).round().min(screen.size.0 as f64) as u32, (h as f64 * k).round().min(screen.size.1 as f64) as u32);
        let position = home.and(self.position).map(|(x, y)| {
            let (sx, sy) = screen.position;
            let right = sx + screen.size.0 as i32 - size.0 as i32;
            let bottom = sy + screen.size.1 as i32 - size.1 as i32;
            (x.clamp(sx, right.max(sx)), y.clamp(sy, bottom.max(sy)))
        });
        Restore { position, size: Some(size), maximized: self.maximized }
    }

    /// The file's text.
    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// A file's text, or `None` when it can't be read as one.
    pub fn from_text(text: &str) -> Option<Placement> {
        serde_json::from_str(text).ok()
    }
}

/// The screen the window's top edge is on, if enough of it is to grab it.
fn grabbed(at: (i32, i32), size: (u32, u32), scale: f64, screens: &[Screen]) -> Option<&Screen> {
    let top = (at.0 as i64, at.1 as i64, at.0 as i64 + size.0 as i64, at.1 as i64 + (GRAB.1 * scale) as i64);
    screens
        .iter()
        .map(|s| {
            let (sx, sy) = (s.position.0 as i64, s.position.1 as i64);
            let (ex, ey) = (sx + s.size.0 as i64, sy + s.size.1 as i64);
            let w = (top.2.min(ex) - top.0.max(sx)).max(0);
            let h = (top.3.min(ey) - top.1.max(sy)).max(0);
            (s, w, h)
        })
        .filter(|(s, w, h)| *w as f64 >= GRAB.0 * s.scale.max(1.0) && *h > 0)
        .max_by_key(|(_, w, h)| w * h)
        .map(|(s, _, _)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Screen = Screen { position: (0, 0), size: (2560, 1440), scale: 1.0, primary: true };
    const RETINA: Screen = Screen { position: (2560, 0), size: (3024, 1964), scale: 2.0, primary: false };

    fn seen(position: (i32, i32), size: (u32, u32)) -> Seen {
        Seen { position: Some(position), size, scale: 1.0, ..Seen::default() }
    }

    #[test]
    fn a_normal_window_is_kept_as_it_is() {
        let p = Placement::of(&seen((100, 80), (1400, 900)), None);
        assert_eq!(p, Placement { position: Some((100, 80)), size: (1400, 900), scale: 1.0, maximized: false });
        assert_eq!(p.restore(&[WIDE]), Restore { position: Some((100, 80)), size: Some((1400, 900)), maximized: false });
        assert_eq!(Placement::from_text(&p.to_text()), Some(p));
    }

    #[test]
    fn a_maximized_window_keeps_the_frame_it_returns_to() {
        let before = Placement { position: Some((100, 80)), size: (1400, 900), scale: 1.0, maximized: false };
        let full = Seen { maximized: true, ..seen((0, 0), (2560, 1400)) };
        // macOS and Linux keep no record: the frame saved before.
        assert_eq!(Placement::of(&full, Some(&before)), Placement { maximized: true, ..before });
        // Windows says where it goes back to.
        let windows = Seen { restore: Some(((50, 40), (1200, 800))), ..full };
        assert_eq!(Placement::of(&windows, Some(&before)), Placement { position: Some((50, 40)), size: (1200, 800), scale: 1.0, maximized: true });
        // Maximized from the first run on: the screen's size, and maximized.
        assert!(Placement::of(&full, None).maximized);
        let restored = Placement { maximized: true, ..before }.restore(&[WIDE]);
        assert_eq!(restored, Restore { position: Some((100, 80)), size: Some((1400, 900)), maximized: true });
    }

    #[test]
    fn minimized_and_full_screen() {
        let before = Placement { position: Some((10, 10)), size: (1000, 700), scale: 1.0, maximized: true };
        let down = Seen { minimized: true, ..seen((-32000, -32000), (0, 0)) };
        assert_eq!(Placement::of(&down, Some(&before)), before, "it opens as it was before it went down");
        let full = Seen { full_screen: true, ..seen((0, 0), (2560, 1440)) };
        assert_eq!(Placement::of(&full, Some(&Placement { maximized: false, ..before })), before, "full screen comes back maximized");
    }

    #[test]
    fn a_window_on_a_screen_that_is_gone_opens_on_one_that_is() {
        // Saved on the right-hand monitor, now unplugged.
        let p = Placement { position: Some((3000, 200)), size: (1600, 1000), scale: 1.0, maximized: false };
        assert_eq!(p.restore(&[WIDE]), Restore { position: None, size: Some((1600, 1000)), maximized: false });
        // Its title bar hidden above the screen's top: also placed anew.
        let p = Placement { position: Some((100, -500)), size: (1600, 400), scale: 1.0, maximized: false };
        assert_eq!(p.restore(&[WIDE]).position, None);
        // A sliver on screen isn't enough to grab it.
        let p = Placement { position: Some((2500, 100)), size: (1200, 800), scale: 1.0, maximized: false };
        assert_eq!(p.restore(&[WIDE]).position, None);
    }

    #[test]
    fn it_fits_the_screen_it_opens_on() {
        // Bigger than a smaller screen it now opens on: as big as that screen, moved onto it.
        let p = Placement { position: Some((300, 200)), size: (2400, 1400), scale: 1.0, maximized: false };
        let laptop = Screen { position: (0, 0), size: (1920, 1080), scale: 1.0, primary: true };
        assert_eq!(p.restore(&[laptop]), Restore { position: Some((0, 0)), size: Some((1920, 1080)), maximized: false });
        // Hanging off the right edge: moved back in whole.
        let p = Placement { position: Some((2000, 100)), size: (1000, 700), scale: 1.0, maximized: false };
        assert_eq!(p.restore(&[WIDE]).position, Some((1560, 100)));
    }

    #[test]
    fn the_size_in_points_follows_a_screen_of_another_scale() {
        // Saved at 1x, the place is on the 2x screen now: twice the pixels, the same points.
        let p = Placement { position: Some((2700, 100)), size: (1200, 800), scale: 1.0, maximized: false };
        assert_eq!(p.restore(&[WIDE, RETINA]), Restore { position: Some((2700, 100)), size: Some((2400, 1600)), maximized: false });
        // A place nobody knows (Wayland): the size alone, on the main screen.
        let p = Placement { position: None, size: (1300, 850), scale: 1.0, maximized: false };
        assert_eq!(p.restore(&[RETINA, WIDE]), Restore { position: None, size: Some((1300, 850)), maximized: false });
    }

    #[test]
    fn nonsense_isnt_restored() {
        let tiny = Placement { position: Some((0, 0)), size: (10, 10), scale: 1.0, maximized: false };
        assert_eq!(tiny.restore(&[WIDE]), Restore::default());
        assert_eq!(Placement::from_text("{\"size\": \"big\"}"), None);
        assert_eq!(Placement::from_text("{\"size\": [900, 600]}").map(|p| (p.scale, p.position)), Some((1.0, None)));
        // A headless run lists no screens.
        let p = Placement { position: Some((0, 0)), size: (900, 600), scale: 1.0, maximized: true };
        assert_eq!(p.restore(&[]), Restore { maximized: true, ..Restore::default() });
    }
}
