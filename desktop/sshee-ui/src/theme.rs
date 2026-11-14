//! The 1.0 design's tokens: colours for both appearances, sizes, radii and type, taken from the
//! design's stylesheet (research_notes/v1-plan/fresh-apple.html: `:root` is the dark appearance,
//! `.light` the light one). Every screen and component reads these; none writes a colour or a
//! size of its own.
//!
//! The rules the tokens serve: blue is only the selected item and the keyboard's focus ring,
//! orange only "needs you", red only Production and errors, green only the cursor and Tor.

use std::sync::OnceLock;

use rune::tok::TextStyle;
use rune::Color;

/// The top row: window buttons, the title, the tabs.
pub const BAR_HEIGHT: f32 = 44.0;
/// The sidebar's icons.
pub const RAIL_WIDTH: f32 = 52.0;
/// The status bar under the page.
pub const STATUS_HEIGHT: f32 = 24.0;
/// A toolbar button, and a tab's height.
pub const CONTROL: f32 = 28.0;
/// A sidebar button.
pub const RAIL_BUTTON: f32 = 32.0;
/// Buttons, tabs, sidebar buttons.
pub const RADIUS: f32 = 8.0;
/// Menu rows and fields.
pub const RADIUS_ROW: f32 = 6.0;
/// Label chips and key caps.
pub const RADIUS_CHIP: f32 = 4.0;
/// Popovers, and the page's top-left corner is `PAGE_RADIUS`.
pub const RADIUS_POPOVER: f32 = 12.0;
pub const PAGE_RADIUS: f32 = 10.0;
/// An icon in the top row and in tabs; the sidebar's are `ICON_RAIL`.
pub const ICON: f32 = 16.0;
pub const ICON_RAIL: f32 = 18.0;
/// Every icon's line, at any size (the design's `non-scaling-stroke`).
pub const ICON_LINE: f32 = 1.4;
/// The keyboard focus ring's width.
pub const RING: f32 = 3.0;
/// A control in a form: a push button, a pop-up button, a segmented control, a key cap.
pub const FORM_CONTROL: f32 = 24.0;
/// A switch's track, and the knob inside it.
pub const SWITCH: (f32, f32) = (34.0, 20.0);
pub const SWITCH_KNOB: f32 = 16.0;
/// A slider's track and its knob.
pub const SLIDER_TRACK: f32 = 4.0;
pub const SLIDER_KNOB: f32 = 16.0;
/// A box of grouped rows.
pub const RADIUS_GROUP: f32 = 10.0;
/// On macOS, the room the window's own buttons take at the start of the top row, and where the
/// first of them sits.
pub const LIGHTS_WIDTH: f32 = 78.0;
pub const LIGHTS_AT: (f32, f32) = (14.0, 16.0);

/// The interface's text, 13 pt.
pub const TEXT: TextStyle = TextStyle::new(13.0, 400.0);
/// The window's title, and names in bold.
pub const TEXT_BOLD: TextStyle = TextStyle::new(13.0, 600.0);
/// A tab's name.
pub const TAB_NAME: TextStyle = TextStyle::new(13.0, 500.0);
/// A tab's server, quieter and a point smaller.
pub const TAB_SERVER: TextStyle = TextStyle::new(12.0, 400.0);
/// "Needs you" after a tab's name.
pub const NEEDS: TextStyle = TextStyle::new(12.0, 500.0);
/// "Reconnecting…" and "Disconnected" there.
pub const TAB_STATE: TextStyle = TextStyle::new(12.0, 400.0);
/// The "Prod" chip.
pub const CHIP: TextStyle = TextStyle::new(11.0, 500.0).lh(15.0);
/// The status bar's labels, its values (numbers that change keep their width), and a value
/// past its threshold.
pub const STATUS: TextStyle = TextStyle::new(11.0, 400.0);
pub const STATUS_VALUE: TextStyle = TextStyle::new(11.0, 500.0).features("'tnum' 1");
pub const STATUS_WARN: TextStyle = TextStyle::new(11.0, 600.0).features("'tnum' 1");
/// A key cap in a menu row.
pub const KEY_CAP: TextStyle = TextStyle::new(11.0, 400.0);
/// A server's name before its group of tabs, when the row is crowded.
pub const GROUP: TextStyle = TextStyle::new(11.0, 600.0);
/// Notes under things, 11 pt.
pub const SMALL: TextStyle = TextStyle::new(11.0, 400.0);
/// Sizes and dates in a list: 12 pt, every digit the same width so columns line up.
pub const LIST_NUMBER: TextStyle = TextStyle::new(12.0, 400.0).features("'tnum' 1");
/// Second lines, 12 pt.
pub const NOTE: TextStyle = TextStyle::new(12.0, 400.0);
/// A page's title (Settings' section name).
pub const PAGE_TITLE: TextStyle = TextStyle::new(17.0, 600.0);
/// The small heading over a group of rows.
pub const GROUP_TITLE: TextStyle = TextStyle::new(12.0, 600.0);
/// The word on a control: a segment, a push button, a key cap.
pub const CONTROL_TEXT: TextStyle = TextStyle::new(12.0, 500.0);
/// A number beside a slider: it changes while its width stays.
pub const VALUE: TextStyle = TextStyle::new(12.0, 400.0).features("'tnum' 1");
/// A line of help under a row, wrapping.
pub const HELP: TextStyle = TextStyle::new(11.0, 400.0).lh(15.0);
/// What is typed into the command palette's search field.
pub const INPUT: TextStyle = TextStyle::new(15.0, 400.0);
/// A command in a list row or a field, in the terminal's font.
pub const CODE: TextStyle = TextStyle::new(12.0, 400.0).mono();
/// A dialog's title.
pub const TITLE: TextStyle = TextStyle::new(15.0, 600.0);
/// A text field's and a push button's height; a list row is `ROW`.
pub const FIELD: f32 = 28.0;
pub const ROW: f32 = 30.0;
/// A popover that holds an agent's question, and the list of agents.
pub const CARD_WIDTH: f32 = 480.0;

/// One appearance's colours. The names are the stylesheet's.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// The window: the top row and the sidebar.
    pub frame: Color,
    /// The page: the terminal and the status bar. Darker than the frame.
    pub page: Color,
    /// Popovers and menus.
    pub pop: Color,
    /// The tab you're on.
    pub sel: Color,
    /// Hairlines: dividers, key caps' rings.
    pub hair: Color,
    /// The line where the page meets the frame.
    pub edge: Color,
    /// The status bar's top line.
    pub hair2: Color,
    /// Text: primary, secondary, and secondary on a selected or hovered surface.
    pub l1: Color,
    pub l2: Color,
    pub l2on: Color,
    /// The strongest text (headers in a terminal listing).
    pub strong: Color,
    /// The selected menu item; pressed, `blue_down`.
    pub blue: Color,
    pub blue_down: Color,
    /// The keyboard focus ring.
    pub ring: Color,
    /// "Needs you": the text and, a touch stronger in the light appearance, the dots.
    pub orange: Color,
    pub orange_dot: Color,
    /// Production and errors; `red_on` on the tab you're on.
    pub red: Color,
    pub red_on: Color,
    /// Tor and the cursor.
    pub green: Color,
    /// The current server's tile in the sidebar.
    pub selrail: Color,
    /// The thin ring around popovers.
    pub rim: Color,
    /// Code inside a popover, and a text field's background.
    pub code: Color,
    /// The light line along the top of the tab you're on (dark appearance).
    pub tabhi: Color,
    /// Under the pointer: tabs and sidebar buttons; menu rows and caption buttons, `hov2`.
    pub hov: Color,
    pub hov2: Color,
    /// Under a press.
    pub down: Color,
    /// Window buttons of a window that isn't in front.
    pub off: Color,
    /// A hidden secret's pill in the terminal.
    pub secbg: Color,
    /// White text on blue and on the red close button.
    pub on_color: Color,
    /// Windows' close button under the pointer.
    pub close_hover: Color,
    /// A box of grouped rows on the page (a settings group).
    pub group: Color,
    /// A control at rest: a segmented control's track, a push button, a switch that is off;
    /// under the pointer, `control_hover`; under a press, `control_down`.
    pub control: Color,
    pub control_hover: Color,
    pub control_down: Color,
    /// The picked segment of a segmented control.
    pub raised: Color,
    /// A switch that is on: the system's green, as on a Mac.
    pub switch_on: Color,
    /// A switch's and a slider's knob.
    pub knob: Color,
}

fn c(hex: &str) -> Color {
    rune::color::hex(hex).expect("a colour written in this file")
}

impl Theme {
    /// The appearance for the system's dark or light.
    pub fn of(dark: bool) -> &'static Theme {
        static THEMES: OnceLock<(Theme, Theme)> = OnceLock::new();
        let (night, day) = THEMES.get_or_init(|| (Theme::dark(), Theme::light()));
        if dark {
            night
        } else {
            day
        }
    }

    fn dark() -> Theme {
        let white = c("#ffffff");
        Theme {
            dark: true,
            frame: c("#28282a"),
            page: c("#1c1c1e"),
            pop: c("#2c2c2e"),
            sel: c("#3a3a3c"),
            hair: c("#3a3a3c"),
            edge: c("#111113"),
            hair2: c("#2a2a2c"),
            l1: c("#ebebf0"),
            l2: c("#a1a1a6"),
            l2on: c("#c7c7cc"),
            strong: white,
            blue: c("#0071e3"),
            blue_down: c("#005bbf"),
            ring: c("#0a84ff"),
            orange: c("#ff9f0a"),
            orange_dot: c("#ff9f0a"),
            red: c("#ff6961"),
            red_on: c("#ff8078"),
            green: c("#4eba65"),
            selrail: white.alpha(0.14),
            rim: white.alpha(0.1),
            code: c("#1f1f21"),
            tabhi: white.alpha(0.1),
            hov: white.alpha(0.04),
            hov2: white.alpha(0.09),
            down: white.alpha(0.12),
            off: c("#4c4c4f"),
            secbg: c("#2a2a2d"),
            on_color: white,
            close_hover: c("#c42b1c"),
            group: c("#242426"),
            control: white.alpha(0.1),
            control_hover: white.alpha(0.14),
            control_down: white.alpha(0.19),
            raised: c("#636366"),
            switch_on: c("#30d158"),
            knob: white,
        }
    }

    fn light() -> Theme {
        let black = c("#000000");
        Theme {
            dark: false,
            frame: c("#ececee"),
            page: c("#ffffff"),
            pop: c("#f6f6f8"),
            sel: c("#ffffff"),
            hair: c("#d2d2d7"),
            edge: c("#d2d2d7"),
            hair2: c("#e5e5ea"),
            l1: c("#1d1d1f"),
            l2: c("#636366"),
            l2on: c("#515154"),
            strong: black,
            blue: c("#0071e3"),
            blue_down: c("#005bbf"),
            ring: c("#0071e3"),
            orange: c("#a84a00"),
            orange_dot: c("#d96200"),
            red: c("#c00012"),
            red_on: c("#c00012"),
            green: c("#248a3d"),
            selrail: black.alpha(0.12),
            rim: black.alpha(0.14),
            code: c("#ececee"),
            tabhi: black.alpha(0.0),
            hov: black.alpha(0.05),
            hov2: black.alpha(0.07),
            down: black.alpha(0.1),
            off: c("#c7c7cc"),
            secbg: c("#ececee"),
            on_color: c("#ffffff"),
            close_hover: c("#c42b1c"),
            group: c("#f5f5f7"),
            control: black.alpha(0.07),
            control_hover: black.alpha(0.1),
            control_down: black.alpha(0.14),
            raised: c("#ffffff"),
            switch_on: c("#34c759"),
            knob: c("#ffffff"),
        }
    }

    /// The ring of a label chip and the like: `color` at 45%.
    pub fn outline_of(&self, color: Color) -> Color {
        color.alpha(0.45)
    }

    /// `color` a step darker, `by` of the way to black: a filled control under the pointer or
    /// a press.
    pub fn shade(&self, color: Color, by: f32) -> Color {
        let k = 1.0 - by.clamp(0.0, 1.0);
        Color { r: color.r * k, g: color.g * k, b: color.b * k, a: color.a }
    }

    /// A control that can't be used: this much of itself.
    pub const DISABLED: f32 = 0.42;
    /// A text field's surface: the code colour at night, white by day (the design's `.field`).
    pub fn field(&self) -> Color {
        if self.dark {
            self.code
        } else {
            self.on_color
        }
    }

    /// The veil over the window under a dialog.
    pub fn scrim(&self) -> Color {
        Color::BLACK.alpha(0.25)
    }

    /// Tor's tile when it's on: green at 16%.
    pub fn tor_tile(&self) -> Color {
        self.green.alpha(0.16)
    }
}

/// Who draws the window's buttons, and so how the top row starts and ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// macOS: the traffic lights sit over the start of the top row.
    Mac,
    /// Windows: the logo starts the row; Rune draws minimize, maximize and close at its end.
    Windows,
    /// Linux: the system's own title bar stays above the window (Rune draws no frame there),
    /// so the row has the logo and no window buttons.
    Linux,
}

impl Frame {
    pub fn current() -> Frame {
        if cfg!(target_os = "macos") {
            Frame::Mac
        } else if cfg!(target_os = "windows") {
            Frame::Windows
        } else {
            Frame::Linux
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_appearances_are_the_designs() {
        let (night, day) = (Theme::of(true), Theme::of(false));
        assert!(night.dark && !day.dark);
        assert_eq!(night.frame.hex(), "#28282aff");
        assert_eq!(night.page.hex(), "#1c1c1eff");
        assert_eq!(day.frame.hex(), "#ececeeff");
        assert_eq!(day.page.hex(), "#ffffffff");
        // The page is darker than the frame at night, lighter by day.
        assert!(night.page.r < night.frame.r && day.page.r > day.frame.r);
        // Whole pixels at 1, 1.25, 1.5, 1.75 and 2x, so the terminal's grid starts on a pixel.
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            for points in [BAR_HEIGHT, RAIL_WIDTH, STATUS_HEIGHT] {
                assert_eq!((points * scale).fract(), 0.0, "{points} at {scale}x");
            }
        }
    }
}
