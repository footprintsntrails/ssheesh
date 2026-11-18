//! The welcome's pictures: the window itself, drawn headless by the preview with `--demo` and
//! cut down by scripts/welcome-pictures.sh, built into the program. Three small pictures of
//! the window in each appearance, and for the terminal's choices a few frames each, in the
//! dark and the light appearance, played as a short loop.
//!
//! A loop is its frames stacked, each fading in at its turn on Rune's render clock (a
//! [`Cycle`]): no timer of ours, and Reduce Motion (the system's, or Settings → Appearance →
//! Animations off) holds the frame that says it best. What the terminal doesn't draw itself
//! (the keys pressed, "Copied") is drawn over the frames as a quiet note.

use rune::image::Image;
use rune::{el, row, text, Color, Cycle, El, ImageCache, SurfaceFit, Waker};

use crate::icons::{icon, Icon};
use crate::kit::menu::key_cap;
use crate::theme::{self, Theme};

/// A terminal picture's size on screen, in points; its file is twice that in pixels.
pub const TERMINAL_SIZE: [f32; 2] = [272.0, 153.0];

macro_rules! shots {
    ($($name:literal),* $(,)?) => {
        [$(($name, include_bytes!(concat!("../../../pictures/welcome/", $name, ".png")) as &[u8])),*]
    };
}

/// Every picture, by name: `look-*` in neither appearance, the others in both.
const SHOTS: [(&str, &[u8]); 15] = shots![
    "look-system",
    "look-light",
    "look-dark",
    "dark-copy-picked",
    "dark-drop-on",
    "dark-drop-retrying",
    "dark-drop-rejoining",
    "dark-drop-ask",
    "dark-drop-off",
    "light-copy-picked",
    "light-drop-on",
    "light-drop-retrying",
    "light-drop-rejoining",
    "light-drop-ask",
    "light-drop-off",
];

/// The pictures, decoding (or decoded) on Rune's image threads.
pub struct Pictures {
    shots: Vec<(&'static str, Image)>,
    /// Kept for as long as the pictures are.
    _cache: ImageCache,
}

impl Pictures {
    /// Starts decoding every picture; each draws once it's ready, the window woken by `waker`.
    pub fn load(waker: Waker) -> Pictures {
        let cache = ImageCache::new(waker);
        let shots = SHOTS.iter().map(|(name, bytes)| (*name, cache.load(*bytes))).collect();
        Pictures { shots, _cache: cache }
    }

    /// The picture `name` filling its box (an empty box while it decodes).
    fn shot(&self, name: &str) -> El {
        let e = el();
        match self.shots.iter().find(|(n, _)| *n == name) {
            Some((_, image)) => e.pixels_fit(image.pixels(), SurfaceFit::Fill),
            None => e,
        }
    }

    /// The window in the appearance `value` ("system", "light", "dark").
    pub fn look(&self, value: &str) -> El {
        self.shot(&format!("look-{value}"))
    }

    /// What the terminal choice `key` does when it's `value`, as a loop in `t`'s appearance.
    pub fn terminal(&self, t: &Theme, key: &str, value: &str, mac: bool) -> El {
        let side = if t.dark { "dark" } else { "light" };
        let frame = |name: &str| self.shot(&format!("{side}-{name}"));
        match (key, value) {
            ("term-copy", "select") => frames(vec![frame("drop-on"), frame("copy-picked").child(notes(t, None))], 1),
            ("term-copy", _) => {
                let keys = if mac { "⌘ C" } else { "Ctrl Shift C" };
                frames(vec![frame("drop-on"), frame("copy-picked"), frame("copy-picked").child(notes(t, Some(keys)))], 2)
            }
            (_, "auto") => frames(vec![frame("drop-on"), frame("drop-retrying"), frame("drop-rejoining")], 2),
            (_, "ask") => frames(vec![frame("drop-on"), frame("drop-ask")], 1),
            _ => frames(vec![frame("drop-on"), frame("drop-off")], 1),
        }
    }
}

/// At the picture's foot: the keys pressed (when some are), then "Copied".
fn notes(t: &Theme, keys: Option<&str>) -> El {
    let mut notes = row().gap(6.0).items_center();
    if let Some(keys) = keys {
        notes = notes.child(row().gap(3.0).children(keys.split(' ').map(|k| key_cap(t, k, false).fill(t.pop))));
    }
    let copied = row()
        .h(22.0)
        .px(8.0)
        .gap(5.0)
        .items_center()
        .radius(theme::RADIUS_ROW)
        .fill(t.pop)
        .border(1.0, t.rim)
        .shadow(2.0, 8.0, Color::BLACK.alpha(if t.dark { 0.35 } else { 0.12 }))
        .child(icon(Icon::Copy, 14.0, t.l2on))
        .child(text("Copied").font(theme::SMALL).color(t.l1).single_line());
    notes.child(copied).pin(None, None, Some(10.0), Some(10.0))
}

/// How long a frame stays, in seconds.
const FRAME: f32 = 1.5;

/// When each frame after the first is shown in a loop of two, three or four: it fades in at its
/// turn and stays until the next is in; the last fades out into the first.
static TWO: [&[(f32, f32)]; 1] = [&[(0.0, 0.0), (0.5, 0.0), (0.54, 1.0), (0.96, 1.0), (1.0, 0.0)]];
static THREE: [&[(f32, f32)]; 2] = [
    &[(0.0, 0.0), (0.333, 0.0), (0.373, 1.0), (0.707, 1.0), (0.71, 0.0), (1.0, 0.0)],
    &[(0.0, 0.0), (0.667, 0.0), (0.707, 1.0), (0.96, 1.0), (1.0, 0.0)],
];

/// `layers` played in turn, the first underneath them all; with Reduce Motion, `still` stays.
fn frames(layers: Vec<El>, still: usize) -> El {
    let stops: &[&[(f32, f32)]] = if layers.len() == 2 { &TWO } else { &THREE };
    let period = FRAME * layers.len() as f32;
    let mut stack = el();
    for (i, layer) in layers.into_iter().enumerate() {
        let layer = layer.pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
        stack = stack.child(match i.checked_sub(1).and_then(|k| stops.get(k)) {
            Some(stops) => layer.cycle(Cycle { period, phase: 0.0, stops, still: if i == still { 1.0 } else { 0.0 }, fps: 30.0 }),
            None => layer,
        });
    }
    stack
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picture_is_a_small_png() {
        let mut total = 0;
        for (name, bytes) in SHOTS {
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{name}");
            total += bytes.len();
        }
        // The program carries them: a modest cost.
        assert!(total < 1_500_000, "{total} bytes");
    }

    #[test]
    fn a_loop_shows_one_frame_at_a_time_and_ends_on_the_first() {
        for stops in TWO.iter().chain(THREE.iter()) {
            assert_eq!(stops.first(), Some(&(0.0, 0.0)));
            assert_eq!(stops.last(), Some(&(1.0, 0.0)));
            assert!(stops.windows(2).all(|w| w[0].0 < w[1].0));
        }
    }
}
