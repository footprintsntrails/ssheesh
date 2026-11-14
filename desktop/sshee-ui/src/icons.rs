//! The icon set: line drawings on one grid with one line weight, drawn as vector shapes (no
//! font, no image files), and the app's logo.
//!
//! The drawings are the 1.0 design's own symbols (research_notes/v1-plan/fresh-apple.html),
//! written for it on a 16-unit grid, with these exceptions:
//! - the onion (Tor) is 0.10's own drawing (`Onion` in src/lib/labels.ts), on a 256-unit grid;
//! - the gear is Lucide's "settings" icon (ISC licence, see desktop/THIRD-PARTY.md), on a
//!   24-unit grid, as the design uses it;
//! - the logo is 0.10's (`Logo()` in src/components/ui.tsx): a terminal window making a `>_<`
//!   face;
//! - the server and the stats line are drawn here in the same manner (the design has no
//!   symbol for a server without a look of its own, nor for a Stats tab).
//!
//! An icon's line stays [`theme::ICON_LINE`] points whatever its size, as the design's
//! `non-scaling-stroke` does.

use rune::{canvas, el, Color, El, LineCap, Shape};

use crate::svg::{self, Fit};
use crate::theme;

/// Every icon the window draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// The sidebar's toggle.
    Sidebar,
    Globe,
    Terminal,
    Folder,
    /// An AI agent: an eight-pointed spark.
    Agent,
    Close,
    Plus,
    Search,
    /// This computer.
    Laptop,
    Flask,
    /// An archive box (backups, storage).
    Box,
    Database,
    /// Security check.
    Shield,
    /// Tor's onion.
    Tor,
    /// A hidden secret.
    Lock,
    Warning,
    /// Settings.
    Gear,
    /// A server without a look of its own.
    Server,
    /// A Stats tab.
    Stats,
    /// Appearance: a disc, half of it shaded.
    Contrast,
    Keyboard,
    /// Updates: two arrows going round.
    Refresh,
    /// Back to what it was: an arrow turning back.
    Undo,
    /// About.
    Info,
    /// A pop-up button's two arrows.
    UpDown,
    /// The picked item in a menu.
    Check,
}

/// A piece of a drawing, in the drawing's own units.
#[derive(Clone, Copy)]
enum Piece {
    /// SVG path data.
    Path(&'static str),
    /// A rectangle `x y width height` with corners of radius `r`.
    Rect(f32, f32, f32, f32, f32),
    /// An ellipse `cx cy rx ry`.
    Oval(f32, f32, f32, f32),
}

use Piece::{Oval, Path, Rect};

/// The window frame the sidebar toggle and the terminal share.
const WINDOW: Piece = Rect(1.8, 2.8, 12.4, 10.4, 2.4);

impl Icon {
    /// Every icon, for tests and the component sheet.
    pub const ALL: [Icon; 26] = [
        Icon::Sidebar,
        Icon::Globe,
        Icon::Terminal,
        Icon::Folder,
        Icon::Agent,
        Icon::Close,
        Icon::Plus,
        Icon::Search,
        Icon::Laptop,
        Icon::Flask,
        Icon::Box,
        Icon::Database,
        Icon::Shield,
        Icon::Tor,
        Icon::Lock,
        Icon::Warning,
        Icon::Gear,
        Icon::Server,
        Icon::Stats,
        Icon::Contrast,
        Icon::Keyboard,
        Icon::Refresh,
        Icon::Undo,
        Icon::Info,
        Icon::UpDown,
        Icon::Check,
    ];

    /// The drawing: its grid's size and its pieces.
    fn drawing(self) -> (f32, &'static [Piece]) {
        match self {
            Icon::Sidebar => (16.0, &[WINDOW, Path("M6 2.8v10.4")]),
            Icon::Globe => (16.0, &[Oval(8.0, 8.0, 6.0, 6.0), Path("M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12")]),
            Icon::Terminal => (16.0, &[WINDOW, Path("M4.8 6.4 6.8 8l-2 1.6M8.6 9.8h2.6")]),
            Icon::Folder => (
                16.0,
                &[Path("M1.8 4.6a1.6 1.6 0 0 1 1.6-1.6h2.7l1.5 1.7h5a1.6 1.6 0 0 1 1.6 1.6v5a1.6 1.6 0 0 1-1.6 1.6H3.4a1.6 1.6 0 0 1-1.6-1.6z")],
            ),
            Icon::Agent => (16.0, &[Path("M8 2v12M2 8h12M3.8 3.8l8.4 8.4M12.2 3.8l-8.4 8.4")]),
            Icon::Close => (16.0, &[Path("M4.5 4.5l7 7M11.5 4.5l-7 7")]),
            Icon::Plus => (16.0, &[Path("M8 3v10M3 8h10")]),
            Icon::Search => (16.0, &[Oval(7.2, 7.2, 4.4, 4.4), Path("M10.6 10.6 13.6 13.6")]),
            Icon::Laptop => (16.0, &[Rect(3.0, 3.4, 10.0, 7.0, 1.2), Path("M1.6 12.6h12.8")]),
            Icon::Flask => (16.0, &[Path("M6.2 2.2h3.6M6.8 2.2v4L3.4 12a1.2 1.2 0 0 0 1 1.8h7.2a1.2 1.2 0 0 0 1-1.8L9.2 6.2v-4M5 10h6")]),
            Icon::Box => (16.0, &[Rect(2.0, 2.8, 12.0, 3.2, 1.0), Path("M3 6v6a1.2 1.2 0 0 0 1.2 1.2h7.6A1.2 1.2 0 0 0 13 12V6M6.4 8.8h3.2")]),
            Icon::Database => (16.0, &[Oval(8.0, 4.0, 5.0, 2.0), Path("M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4M3 8c0 1.1 2.2 2 5 2s5-.9 5-2")]),
            Icon::Shield => (16.0, &[Path("M8 1.8 13 3.6v4c0 3-2 5.4-5 6.6-3-1.2-5-3.6-5-6.6v-4z")]),
            Icon::Tor => (
                256.0,
                &[
                    Path("M128 72C96 84 48 112 48 160C48 200 84 228 128 228C172 228 208 200 208 160C208 112 160 84 128 72Z"),
                    Path("M128 72C104 104 96 132 96 162C96 194 108 216 128 228M128 72C152 104 160 132 160 162C160 194 148 216 128 228"),
                    Path("M128 72C124 56 112 44 96 36M128 72C134 54 148 42 164 36"),
                ],
            ),
            Icon::Lock => (10.0, &[Rect(1.6, 4.4, 6.8, 4.4, 1.0), Path("M3.2 4.4V3a1.8 1.8 0 0 1 3.6 0v1.4")]),
            Icon::Warning => (16.0, &[Path("M8 2.2 14.2 13H1.8zM8 6.6v3M8 11.3v.2")]),
            // Lucide's "settings" (ISC): https://lucide.dev/icons/settings
            Icon::Gear => (
                24.0,
                &[
                    Path("M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"),
                    Oval(12.0, 12.0, 3.0, 3.0),
                ],
            ),
            Icon::Server => (16.0, &[Rect(2.2, 2.6, 11.6, 4.6, 1.4), Rect(2.2, 8.8, 11.6, 4.6, 1.4), Path("M4.7 4.9h.2M4.7 11.1h.2")]),
            Icon::Stats => (16.0, &[Path("M1.8 8.6h2.4l1.8-4.6 3.4 8.6 1.9-4h2.9")]),
            Icon::Contrast => (16.0, &[Oval(8.0, 8.0, 6.0, 6.0), Path("M8 2v12M8 5h5.1M8 8h6M8 11h5.1")]),
            Icon::Keyboard => (16.0, &[Rect(1.6, 4.0, 12.8, 8.0, 1.8), Path("M4.3 6.9h.2M6.8 6.9h.2M9.2 6.9h.2M11.6 6.9h.2M5.4 9.5h5.2")]),
            Icon::Refresh => (16.0, &[Path("M13.2 8a5.2 5.2 0 0 1-9.3 3.2M2.8 8a5.2 5.2 0 0 1 9.3-3.2M12.6 2.2v2.9H9.7M3.4 13.8v-2.9h2.9")]),
            Icon::Undo => (16.0, &[Path("M3 2.8v3.4h3.4M3.2 6.2a5.2 5.2 0 1 1-.3 3.4")]),
            Icon::Info => (16.0, &[Oval(8.0, 8.0, 6.0, 6.0), Path("M8 7.5v3.4M8 5v.2")]),
            Icon::UpDown => (16.0, &[Path("M5.2 6.3 8 3.6l2.8 2.7M5.2 9.7 8 12.4l2.8-2.7")]),
            Icon::Check => (16.0, &[Path("M3.4 8.4 6.5 11.4 12.6 4.8")]),
        }
    }

    /// The icon for a server's look in 0.10 (`HOST_ICONS` in src/lib/labels.ts has thirty; the
    /// 1.0 design has drawn these so far, and the rest show the plain server).
    pub fn for_server(id: &str) -> Icon {
        match id {
            "laptop" | "local" | "desktop" | "terminal" => Icon::Laptop,
            "globe" => Icon::Globe,
            "database" => Icon::Database,
            "flask" => Icon::Flask,
            "backup" | "drive" | "box" => Icon::Box,
            "shield" | "vpn" | "vault" => Icon::Shield,
            _ => Icon::Server,
        }
    }
}

/// A piece as path data.
fn data(piece: Piece) -> std::borrow::Cow<'static, str> {
    match piece {
        Path(d) => d.into(),
        Rect(x, y, w, h, r) => format!(
            "M{} {y}h{}a{r} {r} 0 0 1 {r} {r}v{}a{r} {r} 0 0 1-{r} {r}h-{}a{r} {r} 0 0 1-{r}-{r}v-{}a{r} {r} 0 0 1 {r}-{r}z",
            x + r,
            w - 2.0 * r,
            h - 2.0 * r,
            w - 2.0 * r,
            h - 2.0 * r
        )
        .into(),
        Oval(cx, cy, rx, ry) => format!("M{} {cy}a{rx} {ry} 0 1 0 {} 0a{rx} {ry} 0 1 0-{} 0z", cx - rx, 2.0 * rx, 2.0 * rx).into(),
    }
}

/// The icon's lines at `size` points, in `color`.
pub fn shape(icon: Icon, size: f32, color: Color) -> Shape {
    let (grid, pieces) = icon.drawing();
    let fit = Fit::square(0.0, 0.0, grid, size);
    pieces.iter().fold(Shape::new(), |s, piece| s.stroke_path(&svg::path(&data(*piece), fit), theme::ICON_LINE, LineCap::Round, color))
}

/// The icon as an element `size` points square. It's a picture beside a name or inside a
/// named button, so screen readers skip it.
pub fn icon(icon: Icon, size: f32, color: Color) -> El {
    canvas(shape(icon, size, color)).size(size, size).a11y_hidden()
}

/// The polygon a line of `width` through three points covers, with square ends and a sharp
/// corner (SVG's `stroke-linecap: square`, `stroke-linejoin: miter`), which Rune's strokes
/// don't draw.
fn chevron(points: [[f32; 2]; 3], width: f32) -> Vec<[f32; 2]> {
    let h = width / 2.0;
    let unit = |a: [f32; 2], b: [f32; 2]| {
        let (x, y) = (b[0] - a[0], b[1] - a[1]);
        let len = x.hypot(y).max(f32::EPSILON);
        [x / len, y / len]
    };
    let [a, b, c] = points;
    let (u, v) = (unit(a, b), unit(b, c));
    let (nu, nv) = ([-u[1], u[0]], [-v[1], v[0]]);
    // Square ends reach half the width past the points.
    let (a, c) = ([a[0] - u[0] * h, a[1] - u[1] * h], [c[0] + v[0] * h, c[1] + v[1] * h]);
    // The corner's two points: along the normals' sum, far enough to meet both edges.
    let k = h / (1.0 + nu[0] * nv[0] + nu[1] * nv[1]).max(0.05);
    let m = [(nu[0] + nv[0]) * k, (nu[1] + nv[1]) * k];
    vec![
        [a[0] + nu[0] * h, a[1] + nu[1] * h],
        [b[0] + m[0], b[1] + m[1]],
        [c[0] + nv[0] * h, c[1] + nv[1] * h],
        [c[0] - nv[0] * h, c[1] - nv[1] * h],
        [b[0] - m[0], b[1] - m[1]],
        [a[0] - nu[0] * h, a[1] - nu[1] * h],
    ]
}

/// The app's mark, `size` points square: 0.10's logo, shape for shape and colour for colour
/// (its drawing is on a 184-unit square from (8, 8)). The same in both appearances.
pub fn logo(size: f32) -> El {
    let fit = Fit::square(8.0, 8.0, 184.0, size);
    let at = |p: [f32; 2]| [p[0] * fit.scale + fit.dx, p[1] * fit.scale + fit.dy];
    let hex = |h: &str| rune::color::hex(h).expect("a colour written here");
    let (body, band, dot, eye, mouth) = (hex("#232428"), hex("#2f3136"), hex("#5c5f67"), hex("#f2f0ea"), hex("#4eba65"));
    let rect = |x: f32, y: f32, w: f32, h: f32| vec![at([x, y]), at([x + w, y]), at([x + w, y + h]), at([x, y + h])];
    let mut face = Shape::new()
        // The title band: the window's top 38 units (its round corners are the element's clip).
        .fill(rect(8.0, 8.0, 184.0, 38.0), band);
    for cx in [35.0, 52.0, 69.0] {
        face = face.fill_path(&svg::path(&data(Oval(cx, 27.0, 5.5, 5.5)), fit), dot);
    }
    face = face
        .fill(chevron([[32.0, 86.0], [61.0, 112.0], [32.0, 138.0]], 20.0).into_iter().map(at).collect::<Vec<_>>(), eye)
        .fill(chevron([[168.0, 86.0], [139.0, 112.0], [168.0, 138.0]], 20.0).into_iter().map(at).collect::<Vec<_>>(), eye)
        .fill(rect(76.0, 140.0, 48.0, 20.0), mouth);
    let radius = 44.0 * fit.scale;
    el().size(size, size)
        .radius(radius)
        .fill(body)
        .clip_rounded(radius)
        .a11y_hidden()
        .child(canvas(face).absolute(0.0, 0.0).size(size, size))
        // The faint ring that keeps the dark mark's edge on a dark frame.
        .child(el().absolute(0.0, 0.0).size(size, size).radius(radius).border(1.0, Color::WHITE.alpha(0.16)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drawing_reads_whole_and_stays_on_its_grid() {
        for icon in Icon::ALL {
            let (grid, pieces) = icon.drawing();
            assert!(!pieces.is_empty(), "{icon:?}");
            for piece in pieces {
                let d = data(*piece);
                assert!(svg::reads_whole(&d), "{icon:?}: {d}");
            }
            assert!(!shape(icon, 16.0, Color::WHITE).is_empty(), "{icon:?}");
            assert!([10.0, 16.0, 24.0, 256.0].contains(&grid), "{icon:?}");
        }
    }

    #[test]
    fn a_rounded_rectangle_and_an_oval_close_on_themselves() {
        assert_eq!(data(Rect(1.0, 2.0, 10.0, 6.0, 2.0)), "M3 2h6a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2h-6a2 2 0 0 1-2-2v-2a2 2 0 0 1 2-2z");
        assert_eq!(data(Oval(8.0, 4.0, 5.0, 2.0)), "M3 4a5 2 0 1 0 10 0a5 2 0 1 0-10 0z");
    }

    #[test]
    fn the_logos_eye_is_a_sharp_chevron_with_square_ends() {
        // A right angle through the origin, 2 wide: the corner's outer point is a full
        // half-width out on both axes.
        let p = chevron([[-10.0, 0.0], [0.0, 0.0], [0.0, 10.0]], 2.0);
        let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        assert!(close(p[0], [-11.0, 1.0]), "{p:?}");
        assert!(close(p[1], [-1.0, 1.0]), "{p:?}");
        assert!(close(p[2], [-1.0, 11.0]), "{p:?}");
        assert!(close(p[4], [1.0, -1.0]), "{p:?}");
    }

    #[test]
    fn a_servers_look_maps_to_a_drawn_icon() {
        assert_eq!(Icon::for_server("globe"), Icon::Globe);
        assert_eq!(Icon::for_server("database"), Icon::Database);
        assert_eq!(Icon::for_server("laptop"), Icon::Laptop);
        assert_eq!(Icon::for_server("backup"), Icon::Box);
        assert_eq!(Icon::for_server("rocket"), Icon::Server);
        assert_eq!(Icon::for_server(""), Icon::Server);
    }
}
