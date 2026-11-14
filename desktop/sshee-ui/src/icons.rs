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
//!   symbol for a server without a look of its own, nor for a Stats tab);
//! - the rest of 0.10's thirty server icons (`HOST_ICONS` in src/lib/labels.ts: cloud, house,
//!   network, storage and so on) and the few signs menus and forms need (a check mark, a
//!   pencil, a bin) are drawn here too, on the same 16-unit grid with the same round-ended
//!   line, from plain shapes: none is copied from an icon set.
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
    /// Files: a file of no special kind, one of text, of code, a script, a picture, a video,
    /// a link that leads nowhere.
    File,
    Doc,
    Code,
    Script,
    Image,
    Video,
    Link,
    /// Files' toolbar: forward (back is [`Icon::Back`]), the folder above, list again,
    /// bookmarks, a new folder.
    ChevronRight,
    ArrowUp,
    Refresh,
    Star,
    FolderPlus,
    /// A download and an upload.
    Download,
    Upload,
    /// Up and down: the find bar's older and newer match, a list that opens.
    ChevronUp,
    ChevronDown,
    /// Streamer mode: names and secrets are shown, or (the eye struck through) hidden.
    Eye,
    EyeOff,
    /// Two tabs side by side.
    SideBySide,
    // ---- the rest of the server icons, by what the server is for
    Cloud,
    /// Home, and the main menu in a list.
    House,
    /// Network: three boxes joined.
    Router,
    /// Storage: a disk drive.
    Drive,
    /// Compute: a chip.
    Cpu,
    /// VPN: a padlock on the icon grid ([`Icon::Lock`] is the small one over a secret).
    Padlock,
    Rocket,
    Bot,
    /// AI: a four-pointed spark (an agent at work is [`Icon::Agent`]).
    Sparkle,
    Git,
    Mail,
    Chat,
    Shop,
    /// Passwords, and signing in with a key.
    Key,
    /// Monitoring.
    Chart,
    Game,
    Film,
    Music,
    Camera,
    /// Smart home.
    Wifi,
    /// Workstation.
    Desktop,
    /// Containers.
    Cube,
    // ---- signs for menus and forms
    /// A row that is on.
    Check,
    /// Rename.
    Pencil,
    /// Remove.
    Bin,
    /// Bring a closed tab back.
    Reopen,
    /// Icon and colour.
    Swatch,
    /// Back to the page before.
    Back,
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

/// A sheet with its corner folded, which every kind of file shares.
const SHEET: Piece = Path("M4.4 2.2h4.8l3.4 3.4v7.2a1 1 0 0 1-1 1H4.4a1 1 0 0 1-1-1V3.2a1 1 0 0 1 1-1zM9.2 2.2v3.4h3.4");

/// A picture's and a video's frame.
const FRAME: Piece = Rect(2.2, 3.0, 11.6, 10.0, 1.6);

/// An eye's outline, for streamer mode's two states.
const EYE: Piece = Path("M1.6 8c1.5-2.8 3.7-4.2 6.4-4.2S12.9 5.2 14.4 8c-1.5 2.8-3.7 4.2-6.4 4.2S3.1 10.8 1.6 8z");

impl Icon {
    /// Every icon, for tests and the component sheet.
    pub const ALL: [Icon; 66] = [
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
        Icon::File,
        Icon::Doc,
        Icon::Code,
        Icon::Script,
        Icon::Image,
        Icon::Video,
        Icon::Link,
        Icon::ChevronRight,
        Icon::ArrowUp,
        Icon::Refresh,
        Icon::Star,
        Icon::FolderPlus,
        Icon::Download,
        Icon::Upload,
        Icon::ChevronUp,
        Icon::ChevronDown,
        Icon::Eye,
        Icon::EyeOff,
        Icon::SideBySide,
        Icon::Cloud,
        Icon::House,
        Icon::Router,
        Icon::Drive,
        Icon::Cpu,
        Icon::Padlock,
        Icon::Rocket,
        Icon::Bot,
        Icon::Sparkle,
        Icon::Git,
        Icon::Mail,
        Icon::Chat,
        Icon::Shop,
        Icon::Key,
        Icon::Chart,
        Icon::Game,
        Icon::Film,
        Icon::Music,
        Icon::Camera,
        Icon::Wifi,
        Icon::Desktop,
        Icon::Cube,
        Icon::Check,
        Icon::Pencil,
        Icon::Bin,
        Icon::Reopen,
        Icon::Swatch,
        Icon::Back,
    ];

    /// The signs that aren't a server's icon, for the component sheet.
    pub const SIGNS: [Icon; 36] = [
        Icon::ChevronUp,
        Icon::ChevronDown,
        Icon::Eye,
        Icon::EyeOff,
        Icon::SideBySide,
        Icon::Sidebar,
        Icon::Folder,
        Icon::Agent,
        Icon::Close,
        Icon::Plus,
        Icon::Search,
        Icon::Tor,
        Icon::Lock,
        Icon::Warning,
        Icon::Gear,
        Icon::Stats,
        Icon::Check,
        Icon::Pencil,
        Icon::Bin,
        Icon::Reopen,
        Icon::Swatch,
        Icon::Back,
        Icon::File,
        Icon::Doc,
        Icon::Code,
        Icon::Script,
        Icon::Image,
        Icon::Video,
        Icon::Link,
        Icon::ChevronRight,
        Icon::ArrowUp,
        Icon::Refresh,
        Icon::Star,
        Icon::FolderPlus,
        Icon::Download,
        Icon::Upload,
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
            Icon::File => (16.0, &[SHEET]),
            Icon::Doc => (16.0, &[SHEET, Path("M5.6 8.6h4.8M5.6 11h3.2")]),
            Icon::Code => (16.0, &[SHEET, Path("M6.8 8.2 5.4 9.8l1.4 1.6M9.2 8.2l1.4 1.6-1.4 1.6")]),
            Icon::Script => (16.0, &[SHEET, Path("M5.6 8.4 7.2 9.8l-1.6 1.4M8.6 11.4h1.8")]),
            Icon::Image => (16.0, &[FRAME, Oval(5.6, 6.4, 1.1, 1.1), Path("M2.6 11.6l3.2-3.2 2.4 2.4 2-2 3.2 3.2")]),
            Icon::Video => (16.0, &[FRAME, Path("M6.8 6v4l3.4-2z")]),
            Icon::Link => (
                16.0,
                &[Path("M6.8 9.2a2.6 2.6 0 0 0 3.7 0l2-2a2.6 2.6 0 0 0-3.7-3.7l-.8.8M9.2 6.8a2.6 2.6 0 0 0-3.7 0l-2 2a2.6 2.6 0 0 0 3.7 3.7l.8-.8")],
            ),
            Icon::ChevronRight => (16.0, &[Path("M6.2 3.6 10.6 8l-4.4 4.4")]),
            Icon::ArrowUp => (16.0, &[Path("M8 13V3.4M4 7.2l4-4 4 4")]),
            Icon::Refresh => (16.0, &[Path("M13 8a5 5 0 1 1-1.5-3.6M12.6 1.8v2.8H9.8")]),
            Icon::Star => (16.0, &[Path("M8 2.2l1.8 3.7 4 .6-2.9 2.8.7 4L8 11.4l-3.6 1.9.7-4L2.2 6.5l4-.6z")]),
            Icon::FolderPlus => (
                16.0,
                &[
                    Path("M1.8 4.6a1.6 1.6 0 0 1 1.6-1.6h2.7l1.5 1.7h5a1.6 1.6 0 0 1 1.6 1.6v5a1.6 1.6 0 0 1-1.6 1.6H3.4a1.6 1.6 0 0 1-1.6-1.6z"),
                    Path("M8 7.4v3.6M6.2 9.2h3.6"),
                ],
            ),
            Icon::Download => (16.0, &[Path("M8 2.6v7.6M4.8 7.2 8 10.4l3.2-3.2M3 13.2h10")]),
            Icon::Upload => (16.0, &[Path("M8 10.4V2.8M4.8 6 8 2.8 11.2 6M3 13.2h10")]),
            Icon::ChevronUp => (16.0, &[Path("M4.2 9.8 8 6l3.8 3.8")]),
            Icon::ChevronDown => (16.0, &[Path("M4.2 6.2 8 10l3.8-3.8")]),
            Icon::Eye => (16.0, &[EYE, Oval(8.0, 8.0, 1.9, 1.9)]),
            Icon::EyeOff => (16.0, &[EYE, Oval(8.0, 8.0, 1.9, 1.9), Path("M3.2 12.8 12.8 3.2")]),
            Icon::SideBySide => (16.0, &[WINDOW, Path("M8 2.8v10.4")]),
            Icon::Cloud => (16.0, &[Path("M4.8 12.6h6.6a2.8 2.8 0 0 0 .5-5.56 4.2 4.2 0 0 0-8.1 1.1A2.3 2.3 0 0 0 4.8 12.6z")]),
            Icon::House => (16.0, &[Path("M2.4 7.6 8 2.8l5.6 4.8M3.8 6.6v6.6h8.4V6.6M6.7 13.2V9.6h2.6v3.6")]),
            Icon::Router => (16.0, &[Rect(5.8, 2.2, 4.4, 3.6, 1.0), Rect(1.8, 10.2, 4.4, 3.6, 1.0), Rect(9.8, 10.2, 4.4, 3.6, 1.0), Path("M8 5.8V8M4 10.2V8h8v2.2")]),
            Icon::Drive => (16.0, &[Rect(2.0, 8.4, 12.0, 4.8, 1.4), Path("M2.6 8.6 4.6 3.4h6.8l2 5.2M10.9 10.8h.2")]),
            Icon::Cpu => (
                16.0,
                &[Rect(4.0, 4.0, 8.0, 8.0, 1.4), Rect(6.5, 6.5, 3.0, 3.0, 0.6), Path("M6.4 1.9V4M9.6 1.9V4M6.4 12v2.1M9.6 12v2.1M1.9 6.4H4M1.9 9.6H4M12 6.4h2.1M12 9.6h2.1")],
            ),
            Icon::Padlock => (16.0, &[Rect(3.4, 7.2, 9.2, 6.4, 1.4), Path("M5.4 7.2V5.2a2.6 2.6 0 0 1 5.2 0v2M8 9.8v1.4")]),
            Icon::Rocket => (
                16.0,
                &[Path("M8 1.8c2 1.6 3 3.8 3 6.4l-1 3H6l-1-3c0-2.6 1-4.8 3-6.4zM5.2 9l-1.9 1.9.3 2.3 2.1-1.1M10.8 9l1.9 1.9-.3 2.3-2.1-1.1M8 12.6v1.6"), Oval(8.0, 6.4, 1.2, 1.2)],
            ),
            Icon::Bot => (16.0, &[Rect(3.0, 5.4, 10.0, 7.6, 2.0), Path("M8 5.4V3M6 8.4v1.4M10 8.4v1.4M1.6 8.2v2.2M14.4 8.2v2.2"), Oval(8.0, 2.4, 0.5, 0.5)]),
            Icon::Sparkle => (16.0, &[Path("M8 2c.5 3.2 2.8 5.5 6 6-3.2.5-5.5 2.8-6 6-.5-3.2-2.8-5.5-6-6 3.2-.5 5.5-2.8 6-6z")]),
            Icon::Git => (16.0, &[Oval(4.6, 3.8, 1.6, 1.6), Oval(4.6, 12.2, 1.6, 1.6), Oval(11.4, 5.6, 1.6, 1.6), Path("M4.6 5.4v5.2M11.4 7.2c0 2.2-2.2 3.2-6.8 3.4")]),
            Icon::Mail => (16.0, &[Rect(1.8, 3.4, 12.4, 9.2, 1.6), Path("M2.6 4.8 8 8.8l5.4-4")]),
            Icon::Chat => (
                16.0,
                &[Path("M3.4 2.8h9.2a1.6 1.6 0 0 1 1.6 1.6v5.4a1.6 1.6 0 0 1-1.6 1.6H7.2l-3 2.4v-2.4h-.8a1.6 1.6 0 0 1-1.6-1.6V4.4a1.6 1.6 0 0 1 1.6-1.6z")],
            ),
            Icon::Shop => (16.0, &[Path("M2.2 6.4 3.6 2.8h8.8l1.4 3.6zM3.4 6.4v6.8h9.2V6.4M6.6 13.2V9.6h2.8v3.6")]),
            Icon::Key => (16.0, &[Oval(5.2, 10.8, 2.6, 2.6), Path("M7.1 8.9 13 3M11.2 4.8l1.8 1.8M9.3 6.7l1.5 1.5")]),
            Icon::Chart => (16.0, &[Path("M2.2 2.4v11.4h11.6M4.8 10.6l2.6-3.2 2.2 1.8 3.4-4.4")]),
            Icon::Game => (
                16.0,
                &[Path("M5.2 5h5.6A3.2 3.2 0 0 1 14 8.2v.6a2.6 2.6 0 0 1-4.6 1.6l-.2-.2H6.8l-.2.2A2.6 2.6 0 0 1 2 8.8v-.6A3.2 3.2 0 0 1 5.2 5zM5.2 7v2M4.2 8h2M10.7 8h.2")],
            ),
            Icon::Film => (16.0, &[Rect(2.2, 3.0, 11.6, 10.0, 1.6), Path("M5.2 3v10M10.8 3v10M2.2 6.4h3M2.2 9.6h3M10.8 6.4h3M10.8 9.6h3")]),
            Icon::Music => (16.0, &[Oval(4.8, 11.6, 1.9, 1.9), Oval(11.3, 10.4, 1.9, 1.9), Path("M6.7 11.6V4l6.5-1.4v7.8")]),
            Icon::Camera => (16.0, &[Rect(1.8, 4.6, 12.4, 8.6, 1.6), Oval(8.0, 8.9, 2.4, 2.4), Path("M5.6 4.6l.9-1.7h3l.9 1.7")]),
            Icon::Wifi => (16.0, &[Path("M2.2 6.6a8.4 8.4 0 0 1 11.6 0M4.3 8.9a5.3 5.3 0 0 1 7.4 0M6.4 11.1a2.3 2.3 0 0 1 3.2 0M8 13.3v.1")]),
            Icon::Desktop => (16.0, &[Rect(1.8, 2.8, 12.4, 8.2, 1.4), Path("M5.8 13.6h4.4M8 11v2.6")]),
            Icon::Cube => (16.0, &[Path("M8 1.9l5.4 3v6.2L8 14.1l-5.4-3V4.9zM2.6 4.9 8 8l5.4-3.1M8 8v6.1")]),
            Icon::Check => (16.0, &[Path("M3.4 8.4l3 3 6.2-6.6")]),
            Icon::Pencil => (16.0, &[Path("M10.6 2.9l2.5 2.5-7.6 7.6-3 .5.5-3zM9.2 4.3l2.5 2.5")]),
            Icon::Bin => (
                16.0,
                &[Path("M2.8 4.4h10.4M6.2 4.4V2.9h3.6v1.5M4.2 4.4l.6 8.2a1.2 1.2 0 0 0 1.2 1.1h4a1.2 1.2 0 0 0 1.2-1.1l.6-8.2M6.7 7v4M9.3 7v4")],
            ),
            Icon::Reopen => (16.0, &[Path("M2.8 3v3.6h3.6M3 6.6a5.3 5.3 0 1 1 1.3 5.2")]),
            Icon::Swatch => (16.0, &[Oval(8.0, 8.0, 6.0, 6.0), Oval(8.0, 8.0, 2.2, 2.2)]),
            Icon::Back => (16.0, &[Path("M9.8 3.6 5.4 8l4.4 4.4")]),
        }
    }

    /// The icon for a server's look in 0.10, by its id there (`HOST_ICONS` in
    /// src/lib/labels.ts: thirty of them). An id this version doesn't know shows the plain
    /// server.
    pub fn for_server(id: &str) -> Icon {
        match id {
            "laptop" | "local" => Icon::Laptop,
            "cloud" => Icon::Cloud,
            "globe" => Icon::Globe,
            "database" => Icon::Database,
            "box" => Icon::Cube,
            "home" => Icon::House,
            "router" => Icon::Router,
            "drive" => Icon::Drive,
            "cpu" => Icon::Cpu,
            "shield" => Icon::Shield,
            "vpn" => Icon::Padlock,
            "rocket" => Icon::Rocket,
            "flask" => Icon::Flask,
            "bot" => Icon::Bot,
            "ai" => Icon::Sparkle,
            "terminal" => Icon::Terminal,
            "git" => Icon::Git,
            "mail" => Icon::Mail,
            "chat" => Icon::Chat,
            "shop" => Icon::Shop,
            "vault" => Icon::Key,
            "chart" => Icon::Chart,
            "backup" => Icon::Box,
            "game" => Icon::Game,
            "film" => Icon::Film,
            "music" => Icon::Music,
            "camera" => Icon::Camera,
            "iot" => Icon::Wifi,
            "desktop" => Icon::Desktop,
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
        assert_eq!(Icon::for_server("rocket"), Icon::Rocket);
        assert_eq!(Icon::for_server("something-new"), Icon::Server);
        assert_eq!(Icon::for_server(""), Icon::Server);
    }

    #[test]
    fn each_of_0_10s_thirty_server_icons_has_a_drawing_of_its_own() {
        let ids = sshee_app::labels::HOST_ICONS.map(|(id, _)| id);
        let drawn: std::collections::HashSet<Icon> = ids.iter().map(|id| Icon::for_server(id)).collect();
        assert_eq!(drawn.len(), 30);
        // Only the plain server shows the plain server.
        assert_eq!(ids.iter().filter(|id| Icon::for_server(id) == Icon::Server).count(), 1);
        // Every icon is on the sheet once: as a server's, or as a sign.
        assert_eq!(drawn.len() + Icon::SIGNS.len(), Icon::ALL.len());
        assert!(Icon::SIGNS.iter().all(|sign| !drawn.contains(sign)));
    }
}
