//! The grid engine behind a pane's terminal, and the plain types the rest of the crate and the
//! screen layer read from it.
//!
//! A program's output goes in as bytes ([`Engine::feed`]); what comes out is rows of cells
//! ([`Row`]), the cursor, the modes the program turned on, and events (title, bell, a copy, a
//! reply to write back). Nothing here knows about pixels, fonts or a window.
//!
//! The engine is `alacritty_terminal` 0.26 ([`AlacrittyEngine`]) behind the small [`Engine`]
//! trait, so another one (libghostty, say) can take its place later without the screen layer
//! noticing. The row types are deliberately simple and engine-free: a row is a list of cells,
//! each with a character, a width, two colours and style flags.

use std::sync::mpsc;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as AlacCell, Flags};
use alacritty_terminal::term::{ClipboardType, Config, Osc52, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{
    self, ClearMode, Color as AlacColor, CursorShape as AlacShape, CursorStyle, Handler, NamedColor, Processor,
    StdSyncHandler,
};
use bitflags::bitflags;

use crate::osc::OscEvent;

// ---------------------------------------------------------------------------------------------
// Shared types: what a screen layer, the presentation pass, find and the rest read.

/// How big the terminal is, in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl Size {
    /// The smallest terminal the engine is made with: two columns, one row.
    pub const MIN: Size = Size { cols: 2, rows: 1 };

    /// Narrower or lower than this, a pane is hidden, minimized (Windows reports a minimized
    /// window as 0×0) or squeezed to a sliver by a split: a resize to such a size is ignored and
    /// the terminal keeps the size it had. Reflowing to a sliver would take seconds and push the
    /// whole history out (a 200-column line becomes a hundred 2-column rows).
    pub const FLOOR: Size = Size { cols: 20, rows: 2 };

    pub fn new(cols: u16, rows: u16) -> Self {
        Size { cols: cols.max(Self::MIN.cols), rows: rows.max(Self::MIN.rows) }
    }

    /// Big enough to resize to (see [`Size::FLOOR`]).
    pub fn is_usable(self) -> bool {
        self.cols >= Self::FLOOR.cols && self.rows >= Self::FLOOR.rows
    }
}

/// A 24-bit colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }
}

/// A colour as the program asked for it. The screen layer turns it into pixels with the colour
/// scheme; [`Engine::program_color`] has the palette entries a program changed (OSC 4/10/11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    /// The scheme's text colour (`SGR 39`).
    #[default]
    Foreground,
    /// The scheme's background (`SGR 49`).
    Background,
    /// A palette entry: 0 to 15 are the scheme's sixteen colours, 16 to 231 the 6x6x6 colour
    /// cube, 232 to 255 the grey ramp.
    Indexed(u8),
    /// A colour given exactly (`SGR 38;2;r;g;b`).
    Rgb(Rgb),
}

bitflags! {
    /// How a cell's text is styled, and what kind of cell it is.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct CellFlags: u16 {
        const BOLD             = 1 << 0;
        /// Faint text (`SGR 2`): drawn in a colour halfway to the background.
        const DIM              = 1 << 1;
        const ITALIC           = 1 << 2;
        const UNDERLINE        = 1 << 3;
        const DOUBLE_UNDERLINE = 1 << 4;
        const UNDERCURL        = 1 << 5;
        const DOTTED_UNDERLINE = 1 << 6;
        const DASHED_UNDERLINE = 1 << 7;
        /// Foreground and background swap places.
        const INVERSE          = 1 << 8;
        /// Text present but not shown (`SGR 8`).
        const HIDDEN           = 1 << 9;
        const STRIKEOUT        = 1 << 10;
        /// The first half of a character two columns wide.
        const WIDE             = 1 << 11;
        /// Holds nothing of its own (see [`Cell::width`] 0).
        const SPACER           = 1 << 12;
        /// Part of an OSC 8 link (see [`Row::links`]).
        const LINK             = 1 << 13;
    }
}

/// One cell of a row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    /// What the cell shows: a space when empty. A tab shows as a space.
    pub ch: char,
    /// Columns it takes: 1, or 2 for a wide character (emoji, CJK). 0 for a cell that holds
    /// nothing of its own: the right half of a wide character, or the blank left at the end of a
    /// row where a wide character didn't fit. Draw only its background.
    pub width: u8,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    /// Combining marks and other zero-width characters drawn with `ch`, if any.
    pub zerowidth: Option<Box<str>>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            width: 1,
            fg: Color::Foreground,
            bg: Color::Background,
            flags: CellFlags::empty(),
            zerowidth: None,
        }
    }
}

/// An OSC 8 link on a row: columns `start..end` go to `uri`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RowLink {
    pub start: u16,
    pub end: u16,
    pub uri: String,
    /// The program's id for the link: cells with the same id belong to one link, also across rows.
    pub id: String,
}

/// One row of the terminal, copied out of the engine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Row {
    /// Exactly as many cells as the terminal has columns.
    pub cells: Vec<Cell>,
    /// The program's line goes on in the next row: it was wrapped here, not ended.
    pub wraps: bool,
    /// OSC 8 links, left to right.
    pub links: Vec<RowLink>,
}

impl Row {
    /// A row of `cols` empty cells.
    pub fn blank(cols: u16) -> Self {
        Row { cells: vec![Cell::default(); cols as usize], wraps: false, links: Vec::new() }
    }

    /// A row showing `text` in the default colours, one character per cell (wide ones take two),
    /// padded with blanks or cut to `cols`. For tests and for rows that come from elsewhere
    /// (tmux's history read over ssh).
    pub fn from_text(text: &str, cols: u16) -> Self {
        let mut row = Row::blank(cols);
        let mut col = 0usize;
        for ch in text.chars() {
            let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width == 0 {
                if let Some(cell) = col.checked_sub(1).and_then(|c| row.cells.get_mut(c)) {
                    let mut marks = cell.zerowidth.take().map(String::from).unwrap_or_default();
                    marks.push(ch);
                    cell.zerowidth = Some(marks.into());
                }
                continue;
            }
            if col + width > cols as usize {
                break;
            }
            row.cells[col].ch = ch;
            if width == 2 {
                row.cells[col].width = 2;
                row.cells[col].flags |= CellFlags::WIDE;
                row.cells[col + 1].width = 0;
                row.cells[col + 1].flags |= CellFlags::SPACER;
            }
            col += width;
        }
        row
    }

    /// The row's text: one character per cell, blanks included, the halves of wide characters
    /// once, zero-width marks after their character (as `rowText` in 0.10's terminals.ts).
    pub fn text(&self) -> String {
        let mut text = String::with_capacity(self.cells.len());
        for cell in self.cells.iter().filter(|c| c.width != 0) {
            text.push(cell.ch);
            if let Some(marks) = &cell.zerowidth {
                text.push_str(marks);
            }
        }
        text
    }

    /// The row as the presentation pass reads it: the [`Row::text`] with each character's
    /// column, its OSC 8 links, and `wrapped` when it goes on from the row above (which only
    /// the row above knows: pass its [`Row::wraps`]; see [`row_texts`]).
    pub fn row_text(&self, wrapped: bool) -> crate::present::RowText {
        let mut text = String::with_capacity(self.cells.len());
        let mut cols = Vec::with_capacity(self.cells.len());
        for (col, cell) in self.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }
            text.push(cell.ch);
            cols.push(col as u16);
            if let Some(marks) = &cell.zerowidth {
                for ch in marks.chars() {
                    text.push(ch);
                    cols.push(col as u16);
                }
            }
        }
        let hyperlinks = self.links.iter().map(|l| crate::present::Hyperlink { cols: l.start..l.end, uri: l.uri.clone() }).collect();
        crate::present::RowText { text, cols, wrapped, hyperlinks }
    }

    /// The text without the blanks at its end.
    pub fn trimmed_text(&self) -> String {
        let mut text = self.text();
        text.truncate(text.trim_end_matches(' ').len());
        text
    }
}

/// Consecutive rows as the presentation pass reads them, each marked as going on from the one
/// above when that one wraps. `first_wrapped`: the row above the first one (not in `rows`) wraps.
pub fn row_texts(rows: &[Row], first_wrapped: bool) -> Vec<crate::present::RowText> {
    let mut above = first_wrapped;
    rows.iter()
        .map(|row| {
            let text = row.row_text(above);
            above = row.wraps;
            text
        })
        .collect()
}

/// Which cells changed since the screen layer last looked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Damage {
    /// Everything: redraw every row.
    Full,
    /// These rows of the view (0 at the top), in order. Can be empty.
    Rows(Vec<usize>),
}

/// The cursor's look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
    /// An outline, as drawn while the terminal doesn't have the keyboard.
    HollowBlock,
}

/// Where the cursor is and how it looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cursor {
    /// Row in the view (0 at the top). Can be past the bottom while scrolled back.
    pub line: usize,
    pub col: u16,
    pub shape: CursorShape,
    pub blinking: bool,
    /// The program shows it (`?25h`) and it is inside the view.
    pub visible: bool,
}

/// Which mouse events a program asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MouseProtocol {
    /// None: the mouse selects text and scrolls.
    #[default]
    Off,
    /// Presses, releases and the wheel (`?1000h`). (xterm.js also had presses only, `?9h`,
    /// which alacritty ignores.)
    Vt200,
    /// Those plus moves while a button is held (`?1002h`).
    Drag,
    /// Every move too (`?1003h`).
    Any,
}

/// How mouse reports are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MouseEncoding {
    /// `CSI M` and three bytes; columns and rows up to 223.
    #[default]
    Default,
    /// The same with columns past 95 as two UTF-8 bytes (`?1005h`).
    Utf8,
    /// `CSI < b ; x ; y M/m` (`?1006h`).
    Sgr,
}

/// Modes a program turned on that change what keys, the mouse and pastes send, or what the
/// screen shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modes {
    /// Arrows send `ESC O A` instead of `ESC [ A` (`?1h`).
    pub app_cursor: bool,
    /// The keypad sends application codes (`ESC =`).
    pub app_keypad: bool,
    /// The alternate screen (vim, htop, tmux).
    pub alt_screen: bool,
    pub bracketed_paste: bool,
    /// Report focus in and out (`?1004h`).
    pub focus_reports: bool,
    pub mouse: MouseProtocol,
    pub mouse_encoding: MouseEncoding,
    /// The wheel on the alternate screen sends arrows (`?1007h`).
    pub alternate_scroll: bool,
    pub show_cursor: bool,
    pub line_wrap: bool,
    pub insert: bool,
}

/// Something the engine (or the OSC side pass) wants the app to know or do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermEvent {
    /// The window title changed (OSC 0/2); `None` resets it. tmux reports its scroll position here.
    Title(Option<String>),
    /// The bell (BEL).
    Bell,
    /// A program's copy (OSC 52), already cleaned of keys (see `input::without_keys`).
    Copy(String),
    /// Bytes the terminal answers with (device attributes, cursor position, colours): write
    /// them to the session as they are.
    Reply(Vec<u8>),
    /// Working folder, notifications and progress (see [`crate::osc`]).
    Osc(OscEvent),
}

/// What the engine needs to know at the start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    /// Lines kept above the screen: 10,000 for a plain shell, fewer for tmux (whose history is
    /// on the server).
    pub history: usize,
    pub cursor_shape: CursorShape,
    pub cursor_blinking: bool,
    /// Characters a double-click selection stops at. (A tab is one too: alacritty keeps tabs in
    /// cells.)
    pub word_separators: String,
    /// Answer "which terminal are you?" (DA1). Off while Windows' console (ConPTY) sits between
    /// us and ssh: it answers itself, and a second answer lands in the shell as text.
    pub answer_da1: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            history: 10_000,
            cursor_shape: CursorShape::Block,
            cursor_blinking: false,
            word_separators: WORD_SEPARATORS.to_owned(),
            answer_da1: true,
        }
    }
}

/// Where a double-click selection stops: xterm.js's own set, as in 0.10, so `dev@203.0.113.5:8080`
/// and a whole URL are one word each (alacritty's set also stops at `:`, `|`, `<` and `>`). Plus
/// a tab, which alacritty keeps in a cell, and the box-drawing lines of tables and prompts.
pub const WORD_SEPARATORS: &str = " ()[]{}',\"`\t│─";

/// The colours the terminal answers with when a program asks (OSC 4/10/11/12 with `?`), unless
/// the program set its own. The screen layer sets these from the colour scheme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerColors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// The scheme's sixteen colours; the rest of the 256 are computed as xterm does.
    pub ansi: [Rgb; 16],
}

impl Default for AnswerColors {
    /// xterm's defaults, until the scheme says otherwise.
    fn default() -> Self {
        const ANSI: [Rgb; 16] = [
            Rgb::new(0x00, 0x00, 0x00),
            Rgb::new(0xcd, 0x00, 0x00),
            Rgb::new(0x00, 0xcd, 0x00),
            Rgb::new(0xcd, 0xcd, 0x00),
            Rgb::new(0x00, 0x00, 0xee),
            Rgb::new(0xcd, 0x00, 0xcd),
            Rgb::new(0x00, 0xcd, 0xcd),
            Rgb::new(0xe5, 0xe5, 0xe5),
            Rgb::new(0x7f, 0x7f, 0x7f),
            Rgb::new(0xff, 0x00, 0x00),
            Rgb::new(0x00, 0xff, 0x00),
            Rgb::new(0xff, 0xff, 0x00),
            Rgb::new(0x5c, 0x5c, 0xff),
            Rgb::new(0xff, 0x00, 0xff),
            Rgb::new(0x00, 0xff, 0xff),
            Rgb::new(0xff, 0xff, 0xff),
        ];
        AnswerColors { foreground: Rgb::new(0xff, 0xff, 0xff), background: Rgb::new(0, 0, 0), cursor: Rgb::new(0xff, 0xff, 0xff), ansi: ANSI }
    }
}

impl AnswerColors {
    /// Palette entry `index` (0 to 255), or 256/257/258 for foreground, background and cursor.
    pub fn get(&self, index: usize) -> Option<Rgb> {
        match index {
            0..=15 => Some(self.ansi[index]),
            16..=231 => {
                let i = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
                Some(Rgb::new(level(i / 36), level((i / 6) % 6), level(i % 6)))
            }
            232..=255 => {
                let v = (8 + (index - 232) * 10) as u8;
                Some(Rgb::new(v, v, v))
            }
            256 => Some(self.foreground),
            257 => Some(self.background),
            258 => Some(self.cursor),
            _ => None,
        }
    }
}

/// A place in the whole grid: `line` 0 is the top row of the screen (not of the view), lines
/// above it (negative) are history; `col` from 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GridPoint {
    pub line: i32,
    pub col: u16,
}

impl GridPoint {
    pub fn new(line: i32, col: u16) -> Self {
        GridPoint { line, col }
    }

    /// The grid point under row `view_line` of the view while scrolled back `display_offset` lines.
    pub fn from_view(view_line: usize, col: u16, display_offset: usize) -> Self {
        GridPoint { line: view_line as i32 - display_offset as i32, col }
    }

    /// The row of the view this point is on, if it is in view.
    pub fn view_line(self, display_offset: usize, rows: u16) -> Option<usize> {
        let v = self.line + display_offset as i32;
        (v >= 0 && v < rows as i32).then_some(v as usize)
    }
}

// ---------------------------------------------------------------------------------------------
// The trait.

/// A terminal grid fed with a program's output. Implemented for alacritty_terminal; the rest of
/// the crate and the screen layer use only this.
pub trait Engine: Send {
    /// Parses a program's output. Bytes can be cut anywhere, mid-character or mid-sequence.
    fn feed(&mut self, bytes: &[u8]);

    /// A new size. Wrapped lines reflow on the main screen. A size that isn't
    /// [`Size::is_usable`] (a hidden or minimized pane) is ignored: the terminal keeps its size.
    fn resize(&mut self, size: Size);

    fn size(&self) -> Size;

    /// What changed since the last call, and starts collecting afresh.
    fn take_damage(&mut self) -> Damage;

    /// Row `line` of the view (0 at the top), as it shows now.
    fn row(&self, line: usize) -> Row;

    /// Any row of the grid: 0 is the screen's top row, negative ones are history (see [`GridPoint`]).
    /// `None` outside the grid.
    fn grid_row(&self, line: i32) -> Option<Row>;

    /// Whether grid row `line` wraps into the next ([`Row::wraps`]), without copying the row.
    /// `None` outside the grid.
    fn row_wraps(&self, line: i32) -> Option<bool> {
        self.grid_row(line).map(|row| row.wraps)
    }

    /// One cell of the view.
    fn cell(&self, line: usize, col: usize) -> Cell;

    fn modes(&self) -> Modes;

    fn cursor(&self) -> Cursor;

    /// The title the program set last, if any.
    fn title(&self) -> Option<&str>;

    /// Events since the last call, in order. Repeats in a row are folded: bells into one,
    /// titles into the last, replies into one (up to [`MAX_REPLIES`] bytes; past that they are
    /// dropped, as xterm does under a flood of questions).
    fn take_events(&mut self) -> Vec<TermEvent>;

    /// Line feeds made with the cursor on the bottom row (each moves the screen up a line) since
    /// the last call, or since the last title a program set (tmux reports its history there,
    /// counting the lines before it). While tmux runs, each is a line going up into tmux's
    /// history. (0.10 also counted the line feed that brought the cursor down to that row.)
    fn take_line_feeds_at_bottom(&mut self) -> u32 {
        0
    }

    /// Lines of history above the screen.
    fn history_size(&self) -> usize;

    /// How far the view is scrolled back into the history (0 at the bottom).
    fn display_offset(&self) -> usize;

    /// Scrolls the view: positive goes up into the history.
    fn scroll_display(&mut self, lines: i32);

    /// Back to the bottom, where output appears.
    fn scroll_to_bottom(&mut self);

    /// When the program's synchronized update (DEC 2026) times out, if one is open. The owner
    /// calls [`Engine::stop_sync`] then, as alacritty's own event loop does.
    fn sync_deadline(&self) -> Option<Instant>;

    /// Shows what a synchronized update held back so far.
    fn stop_sync(&mut self);

    /// Forgets the history and moves the cursor's row to the top, as ⌘K does in Mac terminals.
    fn clear_history(&mut self);

    /// Back to a fresh terminal (`ESC c`).
    fn reset(&mut self);

    /// Palette entry `index` as the program set it (OSC 4, 10, 11, 12), if it did.
    fn program_color(&self, index: usize) -> Option<Rgb>;

    /// The colours to answer a program's colour questions with.
    fn set_answer_colors(&mut self, colors: AnswerColors);

    /// Characters a double-click selection stops at.
    fn word_separators(&self) -> &str;

    /// Keeps this many lines of history from now on (fewer drops the oldest).
    fn set_history(&mut self, lines: usize);

    /// The cursor a program gets when it asks for none of its own, from now on (Settings'
    /// cursor and blink). A style the program set (DECSCUSR) stays until it gives it up.
    fn set_cursor_default(&mut self, shape: CursorShape, blinking: bool);

    // Selection: see `crate::select` for the meaning of the arguments.

    fn start_selection(&mut self, kind: crate::select::SelectionKind, at: GridPoint, side: crate::select::Side);
    fn update_selection(&mut self, at: GridPoint, side: crate::select::Side);
    fn clear_selection(&mut self);
    /// Selects everything, history included.
    fn select_all(&mut self);
    /// The selected cells, if anything is selected.
    fn selection(&self) -> Option<crate::select::SelectionSpan>;
}

// ---------------------------------------------------------------------------------------------
// alacritty_terminal.

/// Collects what alacritty's `Term` reports while it parses.
pub struct Listener {
    tx: mpsc::Sender<Event>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

struct Dims {
    cols: usize,
    rows: usize,
}

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// alacritty's DA1 answer, dropped when [`EngineConfig::answer_da1`] is off.
const DA1_ANSWER: &str = "\x1b[?6c";

/// The longest title kept, in bytes (OSC 0 and 2; tmux's scroll report is some 40). Longer
/// ones are cut at a character.
pub const MAX_TITLE: usize = 4096;

/// How many titles a program can save (`CSI 22 t`) before saving more does nothing: xterm.js's
/// limit, which 0.10 had. (alacritty keeps 4,096, each a copy of the title.)
pub const TITLE_STACK: usize = 10;

/// The most combining marks one cell keeps (Unicode's stream-safe limit is 30 in a row). More
/// are dropped, so one cell can't grow without end.
pub const MAX_MARKS: usize = 32;

/// The most reply bytes kept between two [`Engine::take_events`] (and in the model's queue).
pub const MAX_REPLIES: usize = 64 * 1024;

/// The engine on alacritty_terminal 0.26 (Apache-2.0).
pub struct AlacrittyEngine {
    term: Term<Listener>,
    parser: Processor<StdSyncHandler>,
    events: mpsc::Receiver<Event>,
    title: Option<String>,
    config: EngineConfig,
    answers: AnswerColors,
    /// Events since the last `take_events`, taken from alacritty as they came.
    held: Vec<TermEvent>,
    /// The start of a character cut off at the end of the last read (see [`Utf8Carry`]).
    carry: Utf8Carry,
    limits: Limits,
}

impl AlacrittyEngine {
    pub fn new(size: Size, config: &EngineConfig) -> Self {
        let (tx, events) = mpsc::channel();
        let size = Size::new(size.cols, size.rows);
        let term = Term::new(alac_config(config), &Dims { cols: size.cols as usize, rows: size.rows as usize }, Listener { tx });
        AlacrittyEngine {
            term,
            parser: Processor::new(),
            events,
            title: None,
            config: config.clone(),
            answers: AnswerColors::default(),
            held: Vec::new(),
            carry: Utf8Carry::default(),
            limits: Limits::default(),
        }
    }

    /// Parses bytes that end at a character's end.
    fn advance(&mut self, bytes: &[u8]) {
        let mut term = Driven { term: &mut self.term, limits: &mut self.limits };
        self.parser.advance(&mut term, bytes);
    }

    /// Keeps an event for [`Engine::take_events`], folded into the one before it when it repeats
    /// that: a flood of bells, titles or questions stays one event.
    fn hold(&mut self, event: TermEvent) {
        match (self.held.last_mut(), event) {
            (Some(TermEvent::Bell), TermEvent::Bell) => {}
            (Some(TermEvent::Title(last)), TermEvent::Title(title)) => *last = title,
            (Some(TermEvent::Reply(last)), TermEvent::Reply(reply)) => {
                if last.len() + reply.len() <= MAX_REPLIES {
                    last.extend_from_slice(&reply);
                }
            }
            (_, TermEvent::Reply(reply)) if reply.len() > MAX_REPLIES => {}
            (_, event) => self.held.push(event),
        }
    }

    /// alacritty's own terminal, for what the trait doesn't cover (search, vi motions).
    pub fn term(&self) -> &Term<Listener> {
        &self.term
    }

    pub fn term_mut(&mut self) -> &mut Term<Listener> {
        &mut self.term
    }

    fn convert_row(&self, line: Line) -> Row {
        let grid = self.term.grid();
        let cols = grid.columns();
        let source = &grid[line];
        let mut row = Row { cells: Vec::with_capacity(cols), wraps: false, links: Vec::new() };
        let mut link: Option<(usize, alacritty_terminal::term::cell::Hyperlink)> = None;
        for col in 0..cols {
            let cell = &source[Column(col)];
            row.cells.push(convert_cell(cell));
            // OSC 8 links: runs of cells with the same link.
            let here = if cell.extra.is_some() { cell.hyperlink() } else { None };
            if link.as_ref().map(|(_, l)| Some(l) != here.as_ref()).unwrap_or(here.is_some()) {
                if let Some((start, l)) = link.take() {
                    row.links.push(RowLink { start: start as u16, end: col as u16, uri: l.uri().to_owned(), id: l.id().to_owned() });
                }
                link = here.map(|l| (col, l));
            }
        }
        if let Some((start, l)) = link {
            row.links.push(RowLink { start: start as u16, end: cols as u16, uri: l.uri().to_owned(), id: l.id().to_owned() });
        }
        row.wraps = source[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
        row
    }

    /// Takes in what alacritty reported as our events (see [`AlacrittyEngine::hold`]), answering
    /// colour questions on the way.
    fn collect(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Title(title) => {
                    self.title = Some(title.clone());
                    self.hold(TermEvent::Title(Some(title)));
                }
                Event::ResetTitle => {
                    self.title = None;
                    self.hold(TermEvent::Title(None));
                }
                Event::Bell => self.hold(TermEvent::Bell),
                // Both the clipboard and the primary selection go to the clipboard, as in 0.10.
                Event::ClipboardStore(ClipboardType::Clipboard | ClipboardType::Selection, text) => self.hold(TermEvent::Copy(text)),
                Event::PtyWrite(text) => {
                    if self.config.answer_da1 || text != DA1_ANSWER {
                        self.hold(TermEvent::Reply(text.into_bytes()));
                    }
                }
                Event::ColorRequest(index, format) => {
                    let color = self.program_color(index).or_else(|| self.answers.get(index));
                    if let Some(c) = color {
                        self.hold(TermEvent::Reply(format(ansi::Rgb { r: c.r, g: c.g, b: c.b }).into_bytes()));
                    }
                }
                // Reading the clipboard is refused (OSC 52 is copy only); the window's size in
                // pixels isn't told, as xterm.js didn't by default; the rest is for alacritty's
                // own window.
                Event::ClipboardLoad(..)
                | Event::TextAreaSizeRequest(_)
                | Event::MouseCursorDirty
                | Event::CursorBlinkingChange
                | Event::Wakeup
                | Event::Exit
                | Event::ChildExit(_) => {}
            }
        }
    }
}

fn alac_config(config: &EngineConfig) -> Config {
    Config {
        scrolling_history: config.history,
        default_cursor_style: CursorStyle { shape: to_alac_shape(config.cursor_shape), blinking: config.cursor_blinking },
        vi_mode_cursor_style: None,
        semantic_escape_chars: config.word_separators.clone(),
        kitty_keyboard: false,
        osc52: Osc52::OnlyCopy,
    }
}

fn to_alac_shape(shape: CursorShape) -> AlacShape {
    match shape {
        CursorShape::Block => AlacShape::Block,
        CursorShape::Underline => AlacShape::Underline,
        CursorShape::Bar => AlacShape::Beam,
        CursorShape::HollowBlock => AlacShape::HollowBlock,
    }
}

fn from_alac_rgb(c: ansi::Rgb) -> Rgb {
    Rgb::new(c.r, c.g, c.b)
}

fn convert_color(color: AlacColor, background: bool) -> Color {
    match color {
        AlacColor::Spec(c) => Color::Rgb(from_alac_rgb(c)),
        AlacColor::Indexed(i) => Color::Indexed(i),
        AlacColor::Named(named) => {
            let n = named as usize;
            if n < 16 {
                return Color::Indexed(n as u8);
            }
            match named {
                NamedColor::DimBlack
                | NamedColor::DimRed
                | NamedColor::DimGreen
                | NamedColor::DimYellow
                | NamedColor::DimBlue
                | NamedColor::DimMagenta
                | NamedColor::DimCyan
                | NamedColor::DimWhite => Color::Indexed((n - NamedColor::DimBlack as usize) as u8),
                NamedColor::Background => Color::Background,
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => Color::Foreground,
                // The cursor's colour never reaches a cell; keep the slot's own default.
                _ if background => Color::Background,
                _ => Color::Foreground,
            }
        }
    }
}

fn convert_cell(cell: &AlacCell) -> Cell {
    let f = cell.flags;
    let mut flags = CellFlags::empty();
    for (from, to) in [
        (Flags::BOLD, CellFlags::BOLD),
        (Flags::DIM, CellFlags::DIM),
        (Flags::ITALIC, CellFlags::ITALIC),
        (Flags::UNDERLINE, CellFlags::UNDERLINE),
        (Flags::DOUBLE_UNDERLINE, CellFlags::DOUBLE_UNDERLINE),
        (Flags::UNDERCURL, CellFlags::UNDERCURL),
        (Flags::DOTTED_UNDERLINE, CellFlags::DOTTED_UNDERLINE),
        (Flags::DASHED_UNDERLINE, CellFlags::DASHED_UNDERLINE),
        (Flags::INVERSE, CellFlags::INVERSE),
        (Flags::HIDDEN, CellFlags::HIDDEN),
        (Flags::STRIKEOUT, CellFlags::STRIKEOUT),
        (Flags::WIDE_CHAR, CellFlags::WIDE),
    ] {
        if f.contains(from) {
            flags |= to;
        }
    }
    let spacer = f.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
    if spacer {
        flags |= CellFlags::SPACER;
    }
    let mut zerowidth = None;
    if cell.extra.is_some() {
        if cell.hyperlink().is_some() {
            flags |= CellFlags::LINK;
        }
        if let Some(marks) = cell.zerowidth().filter(|m| !m.is_empty()) {
            zerowidth = Some(marks.iter().take(MAX_MARKS).collect::<String>().into_boxed_str());
        }
    }
    Cell {
        ch: if spacer || cell.c == '\t' { ' ' } else { cell.c },
        width: if spacer {
            0
        } else if f.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        },
        fg: convert_color(cell.fg, false),
        bg: convert_color(cell.bg, true),
        flags,
        zerowidth,
    }
}

impl Engine for AlacrittyEngine {
    fn feed(&mut self, bytes: &[u8]) {
        // A synchronized update that outlived its time shows now, before anything newer.
        if self.sync_deadline().is_some_and(|at| at <= Instant::now()) {
            self.stop_sync();
        }
        // vte must never see a read that ends inside a character: vte 0.15 drops the byte after
        // a 2-byte character completed by the next read ("приве\xD1" + "\x82 мир" showed
        // "приветмир"). The cut-off start waits here and goes first next time.
        let mut bytes = bytes;
        if let Some(whole) = self.carry.complete(&mut bytes) {
            self.advance(whole.as_slice());
        }
        // Still holding: this read ended inside the same character (nothing of it is left).
        if self.carry.len == 0 {
            let (now, later) = bytes.split_at(bytes.len() - incomplete_tail(bytes));
            self.advance(now);
            self.carry.keep(later);
        }
        // Taken in now, so the title and the answers are current after every feed.
        self.collect();
    }

    fn resize(&mut self, size: Size) {
        if !size.is_usable() {
            return;
        }
        self.term.resize(Dims { cols: size.cols as usize, rows: size.rows as usize });
    }

    fn size(&self) -> Size {
        Size { cols: self.term.columns() as u16, rows: self.term.screen_lines() as u16 }
    }

    fn take_damage(&mut self) -> Damage {
        let rows = self.term.screen_lines();
        let damage = match self.term.damage() {
            TermDamage::Full => Damage::Full,
            TermDamage::Partial(lines) => {
                Damage::Rows(lines.map(|d| d.line).filter(|&l| l < rows).collect())
            }
        };
        self.term.reset_damage();
        damage
    }

    fn row(&self, line: usize) -> Row {
        let offset = self.term.grid().display_offset() as i32;
        let line = (line as i32).min(self.term.screen_lines() as i32 - 1);
        self.convert_row(Line(line - offset))
    }

    fn grid_row(&self, line: i32) -> Option<Row> {
        let top = self.term.topmost_line().0;
        let bottom = self.term.bottommost_line().0;
        (top..=bottom).contains(&line).then(|| self.convert_row(Line(line)))
    }

    fn row_wraps(&self, line: i32) -> Option<bool> {
        let top = self.term.topmost_line().0;
        let bottom = self.term.bottommost_line().0;
        let last = Column(self.term.columns() - 1);
        (top..=bottom).contains(&line).then(|| self.term.grid()[Line(line)][last].flags.contains(Flags::WRAPLINE))
    }

    fn cell(&self, line: usize, col: usize) -> Cell {
        let grid = self.term.grid();
        let offset = grid.display_offset() as i32;
        let line = (line as i32).min(self.term.screen_lines() as i32 - 1) - offset;
        let col = col.min(grid.columns() - 1);
        convert_cell(&grid[Line(line)][Column(col)])
    }

    fn modes(&self) -> Modes {
        let m = *self.term.mode();
        let mouse = if m.contains(TermMode::MOUSE_MOTION) {
            MouseProtocol::Any
        } else if m.contains(TermMode::MOUSE_DRAG) {
            MouseProtocol::Drag
        } else if m.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseProtocol::Vt200
        } else {
            MouseProtocol::Off
        };
        let mouse_encoding = if m.contains(TermMode::SGR_MOUSE) {
            MouseEncoding::Sgr
        } else if m.contains(TermMode::UTF8_MOUSE) {
            MouseEncoding::Utf8
        } else {
            MouseEncoding::Default
        };
        Modes {
            app_cursor: m.contains(TermMode::APP_CURSOR),
            app_keypad: m.contains(TermMode::APP_KEYPAD),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            focus_reports: m.contains(TermMode::FOCUS_IN_OUT),
            mouse,
            mouse_encoding,
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            show_cursor: m.contains(TermMode::SHOW_CURSOR),
            line_wrap: m.contains(TermMode::LINE_WRAP),
            insert: m.contains(TermMode::INSERT),
        }
    }

    fn cursor(&self) -> Cursor {
        let grid = self.term.grid();
        let mut point = grid.cursor.point;
        // On the right half of a wide character the cursor covers the whole character.
        if point.column.0 > 0 && grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
            point.column -= 1;
        }
        let style = self.term.cursor_style();
        let line = point.line.0 + grid.display_offset() as i32;
        let shown = self.term.mode().contains(TermMode::SHOW_CURSOR) && style.shape != AlacShape::Hidden;
        let in_view = line >= 0 && (line as usize) < self.term.screen_lines();
        Cursor {
            line: line.max(0) as usize,
            col: point.column.0 as u16,
            shape: match style.shape {
                AlacShape::Underline => CursorShape::Underline,
                AlacShape::Beam => CursorShape::Bar,
                AlacShape::HollowBlock => CursorShape::HollowBlock,
                AlacShape::Block | AlacShape::Hidden => CursorShape::Block,
            },
            blinking: style.blinking,
            visible: shown && in_view,
        }
    }

    fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    fn take_events(&mut self) -> Vec<TermEvent> {
        self.collect();
        std::mem::take(&mut self.held)
    }

    fn take_line_feeds_at_bottom(&mut self) -> u32 {
        std::mem::take(&mut self.limits.bottom_feeds)
    }

    fn history_size(&self) -> usize {
        self.term.history_size()
    }

    fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn scroll_display(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    fn sync_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    fn stop_sync(&mut self) {
        let mut term = Driven { term: &mut self.term, limits: &mut self.limits };
        self.parser.stop_sync(&mut term);
        self.collect();
    }

    fn clear_history(&mut self) {
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            // The cursor's row goes to the top, the rows above it into the history (gone below).
            let rows = self.term.screen_lines() as i32;
            let up = self.term.grid().cursor.point.line.0.clamp(0, rows - 1) as usize;
            if up > 0 {
                let grid = self.term.grid_mut();
                grid.scroll_up::<ansi::Color>(&(Line(0)..Line(rows)), up);
                grid.cursor.point.line = Line(0);
            }
        }
        self.term.scroll_display(Scroll::Bottom);
        // Damages every row, also when there was no history to forget.
        self.term.clear_screen(ClearMode::Saved);
        self.term.selection = None;
    }

    fn reset(&mut self) {
        Driven { term: &mut self.term, limits: &mut self.limits }.reset_state();
        self.carry = Utf8Carry::default();
        self.collect();
        self.title = None;
    }

    fn program_color(&self, index: usize) -> Option<Rgb> {
        if index < alacritty_terminal::term::color::COUNT {
            self.term.colors()[index].map(from_alac_rgb)
        } else {
            None
        }
    }

    fn set_answer_colors(&mut self, colors: AnswerColors) {
        self.answers = colors;
    }

    fn word_separators(&self) -> &str {
        self.term.semantic_escape_chars()
    }

    fn set_history(&mut self, lines: usize) {
        if lines == self.config.history {
            return;
        }
        self.config.history = lines;
        // Setting options repeats the title as an event though it didn't change: keep what came
        // before, drop the repeat.
        self.collect();
        self.term.set_options(alac_config(&self.config));
        while self.events.try_recv().is_ok() {}
    }

    fn set_cursor_default(&mut self, shape: CursorShape, blinking: bool) {
        if (shape, blinking) == (self.config.cursor_shape, self.config.cursor_blinking) {
            return;
        }
        self.config.cursor_shape = shape;
        self.config.cursor_blinking = blinking;
        // As in `set_history`: setting options repeats the title as an event.
        self.collect();
        self.term.set_options(alac_config(&self.config));
        while self.events.try_recv().is_ok() {}
    }

    fn start_selection(&mut self, kind: crate::select::SelectionKind, at: GridPoint, side: crate::select::Side) {
        crate::select::start(&mut self.term, kind, at, side);
    }

    fn update_selection(&mut self, at: GridPoint, side: crate::select::Side) {
        crate::select::update(&mut self.term, at, side);
    }

    fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    fn select_all(&mut self) {
        crate::select::select_all(&mut self.term);
    }

    fn selection(&self) -> Option<crate::select::SelectionSpan> {
        crate::select::span(&self.term)
    }
}

// ---------------------------------------------------------------------------------------------
// What the parser writes into alacritty's grid, and the limits on it.

/// The first bytes of a character that the end of a read cut off, held for the next read.
#[derive(Clone, Copy, Debug, Default)]
struct Utf8Carry {
    bytes: [u8; 4],
    len: usize,
}

impl Utf8Carry {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Completes the held character with the start of `bytes` (taking those bytes off) and
    /// returns it to parse. `None` when nothing is held, or when `bytes` ran out first (then
    /// the rest waits too). A character the next byte doesn't go on is returned as it is: vte
    /// shows it as a replacement character, as it would have uncut.
    fn complete(&mut self, bytes: &mut &[u8]) -> Option<Utf8Carry> {
        if self.len == 0 {
            return None;
        }
        let need = utf8_len(self.bytes[0]);
        while self.len < need {
            match bytes.first() {
                Some(&b) if is_continuation(b) => {
                    self.bytes[self.len] = b;
                    self.len += 1;
                    *bytes = &bytes[1..];
                }
                Some(_) => break,
                None => return None,
            }
        }
        let whole = *self;
        self.len = 0;
        Some(whole)
    }

    /// Holds `tail` (at most three bytes, from [`incomplete_tail`]).
    fn keep(&mut self, tail: &[u8]) {
        self.bytes[..tail.len()].copy_from_slice(tail);
        self.len = tail.len();
    }
}

/// How many bytes at the end of `bytes` start a character that isn't complete yet: a lead byte
/// (0xC2 to 0xF4) followed by fewer continuation bytes than it needs. At most three.
fn incomplete_tail(bytes: &[u8]) -> usize {
    let n = bytes.len();
    for back in 1..=n.min(3) {
        let b = bytes[n - back];
        if !is_continuation(b) {
            return if utf8_len(b) > back { back } else { 0 };
        }
    }
    0
}

/// Bytes in the UTF-8 character a byte starts (1 for anything that doesn't start one).
fn utf8_len(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

fn is_continuation(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// What [`Driven`] counts and limits, kept between reads.
#[derive(Clone, Copy, Debug, Default)]
struct Limits {
    /// See [`Engine::take_line_feeds_at_bottom`].
    bottom_feeds: u32,
    /// Titles saved with `CSI 22 t` and not taken back yet.
    titles_saved: usize,
}

/// alacritty's `Term` as the parser drives it, with SSHeesh's limits on the way: line feeds on
/// the bottom row counted, titles cut to [`MAX_TITLE`] and at most [`TITLE_STACK`] of them
/// saved, at most [`MAX_MARKS`] combining marks on a cell. Everything else goes straight to
/// `Term`, so every method of the trait is passed on here.
struct Driven<'a> {
    term: &'a mut Term<Listener>,
    limits: &'a mut Limits,
}

/// The cell a zero-width character would join (found as `Term::input` finds it) holds as many
/// marks as it may.
fn marks_full(term: &Term<Listener>) -> bool {
    let grid = term.grid();
    let cursor = &grid.cursor;
    let mut column = cursor.point.column;
    if !cursor.input_needs_wrap {
        column.0 = column.0.saturating_sub(1);
    }
    let row = &grid[cursor.point.line];
    if row[column].flags.contains(Flags::WIDE_CHAR_SPACER) {
        column.0 = column.0.saturating_sub(1);
    }
    row[column].zerowidth().is_some_and(|marks| marks.len() >= MAX_MARKS)
}

/// A title no longer than [`MAX_TITLE`] bytes, cut at a character.
fn cut_title(mut title: String) -> String {
    if title.len() > MAX_TITLE {
        let mut end = MAX_TITLE;
        while !title.is_char_boundary(end) {
            end -= 1;
        }
        title.truncate(end);
    }
    title
}

impl Handler for Driven<'_> {
    fn input(&mut self, c: char) {
        if !c.is_ascii() && unicode_width::UnicodeWidthChar::width(c) == Some(0) && marks_full(self.term) {
            return;
        }
        self.term.input(c);
    }

    fn linefeed(&mut self) {
        let on_bottom = self.term.grid().cursor.point.line.0 == self.term.screen_lines() as i32 - 1;
        self.term.linefeed();
        if on_bottom {
            self.limits.bottom_feeds = self.limits.bottom_feeds.saturating_add(1);
        }
    }

    fn set_title(&mut self, title: Option<String>) {
        if title.is_some() {
            // tmux's report counts the lines before it.
            self.limits.bottom_feeds = 0;
        }
        self.term.set_title(title.map(cut_title));
    }

    fn push_title(&mut self) {
        if self.limits.titles_saved < TITLE_STACK {
            self.limits.titles_saved += 1;
            self.term.push_title();
        }
    }

    fn pop_title(&mut self) {
        self.limits.titles_saved = self.limits.titles_saved.saturating_sub(1);
        self.term.pop_title();
    }

    fn reset_state(&mut self) {
        self.limits.titles_saved = 0;
        self.term.reset_state();
    }

    fn set_cursor_style(&mut self, style: Option<CursorStyle>) {
        self.term.set_cursor_style(style);
    }

    fn set_cursor_shape(&mut self, shape: AlacShape) {
        self.term.set_cursor_shape(shape);
    }

    fn goto(&mut self, line: i32, col: usize) {
        self.term.goto(line, col);
    }

    fn goto_line(&mut self, line: i32) {
        self.term.goto_line(line);
    }

    fn goto_col(&mut self, col: usize) {
        self.term.goto_col(col);
    }

    fn insert_blank(&mut self, count: usize) {
        self.term.insert_blank(count);
    }

    fn move_up(&mut self, rows: usize) {
        self.term.move_up(rows);
    }

    fn move_down(&mut self, rows: usize) {
        self.term.move_down(rows);
    }

    fn identify_terminal(&mut self, intermediate: Option<char>) {
        self.term.identify_terminal(intermediate);
    }

    fn device_status(&mut self, arg: usize) {
        self.term.device_status(arg);
    }

    fn move_forward(&mut self, cols: usize) {
        self.term.move_forward(cols);
    }

    fn move_backward(&mut self, cols: usize) {
        self.term.move_backward(cols);
    }

    fn move_down_and_cr(&mut self, rows: usize) {
        self.term.move_down_and_cr(rows);
    }

    fn move_up_and_cr(&mut self, rows: usize) {
        self.term.move_up_and_cr(rows);
    }

    fn put_tab(&mut self, count: u16) {
        self.term.put_tab(count);
    }

    fn backspace(&mut self) {
        self.term.backspace();
    }

    fn carriage_return(&mut self) {
        self.term.carriage_return();
    }

    fn bell(&mut self) {
        self.term.bell();
    }

    fn substitute(&mut self) {
        self.term.substitute();
    }

    fn newline(&mut self) {
        self.term.newline();
    }

    fn set_horizontal_tabstop(&mut self) {
        self.term.set_horizontal_tabstop();
    }

    fn scroll_up(&mut self, lines: usize) {
        self.term.scroll_up(lines);
    }

    fn scroll_down(&mut self, lines: usize) {
        self.term.scroll_down(lines);
    }

    fn insert_blank_lines(&mut self, count: usize) {
        self.term.insert_blank_lines(count);
    }

    fn delete_lines(&mut self, count: usize) {
        self.term.delete_lines(count);
    }

    fn erase_chars(&mut self, count: usize) {
        self.term.erase_chars(count);
    }

    fn delete_chars(&mut self, count: usize) {
        self.term.delete_chars(count);
    }

    fn move_backward_tabs(&mut self, count: u16) {
        self.term.move_backward_tabs(count);
    }

    fn move_forward_tabs(&mut self, count: u16) {
        self.term.move_forward_tabs(count);
    }

    fn save_cursor_position(&mut self) {
        self.term.save_cursor_position();
    }

    fn restore_cursor_position(&mut self) {
        self.term.restore_cursor_position();
    }

    fn clear_line(&mut self, mode: ansi::LineClearMode) {
        self.term.clear_line(mode);
    }

    fn clear_screen(&mut self, mode: ClearMode) {
        self.term.clear_screen(mode);
    }

    fn clear_tabs(&mut self, mode: ansi::TabulationClearMode) {
        self.term.clear_tabs(mode);
    }

    fn set_tabs(&mut self, interval: u16) {
        self.term.set_tabs(interval);
    }

    fn reverse_index(&mut self) {
        self.term.reverse_index();
    }

    fn terminal_attribute(&mut self, attr: ansi::Attr) {
        self.term.terminal_attribute(attr);
    }

    fn set_mode(&mut self, mode: ansi::Mode) {
        self.term.set_mode(mode);
    }

    fn unset_mode(&mut self, mode: ansi::Mode) {
        self.term.unset_mode(mode);
    }

    fn report_mode(&mut self, mode: ansi::Mode) {
        self.term.report_mode(mode);
    }

    fn set_private_mode(&mut self, mode: ansi::PrivateMode) {
        self.term.set_private_mode(mode);
    }

    fn unset_private_mode(&mut self, mode: ansi::PrivateMode) {
        self.term.unset_private_mode(mode);
    }

    fn report_private_mode(&mut self, mode: ansi::PrivateMode) {
        self.term.report_private_mode(mode);
    }

    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        self.term.set_scrolling_region(top, bottom);
    }

    fn set_keypad_application_mode(&mut self) {
        self.term.set_keypad_application_mode();
    }

    fn unset_keypad_application_mode(&mut self) {
        self.term.unset_keypad_application_mode();
    }

    fn set_active_charset(&mut self, index: ansi::CharsetIndex) {
        self.term.set_active_charset(index);
    }

    fn configure_charset(&mut self, index: ansi::CharsetIndex, charset: ansi::StandardCharset) {
        self.term.configure_charset(index, charset);
    }

    fn set_color(&mut self, index: usize, color: ansi::Rgb) {
        self.term.set_color(index, color);
    }

    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        self.term.dynamic_color_sequence(prefix, index, terminator);
    }

    fn reset_color(&mut self, index: usize) {
        self.term.reset_color(index);
    }

    fn clipboard_store(&mut self, clipboard: u8, base64: &[u8]) {
        self.term.clipboard_store(clipboard, base64);
    }

    fn clipboard_load(&mut self, clipboard: u8, terminator: &str) {
        self.term.clipboard_load(clipboard, terminator);
    }

    fn decaln(&mut self) {
        self.term.decaln();
    }

    fn text_area_size_pixels(&mut self) {
        self.term.text_area_size_pixels();
    }

    fn text_area_size_chars(&mut self) {
        self.term.text_area_size_chars();
    }

    fn set_hyperlink(&mut self, link: Option<ansi::Hyperlink>) {
        self.term.set_hyperlink(link);
    }

    fn set_mouse_cursor_icon(&mut self, icon: ansi::cursor_icon::CursorIcon) {
        self.term.set_mouse_cursor_icon(icon);
    }

    fn report_keyboard_mode(&mut self) {
        self.term.report_keyboard_mode();
    }

    fn push_keyboard_mode(&mut self, mode: ansi::KeyboardModes) {
        self.term.push_keyboard_mode(mode);
    }

    fn pop_keyboard_modes(&mut self, to_pop: u16) {
        self.term.pop_keyboard_modes(to_pop);
    }

    fn set_keyboard_mode(&mut self, mode: ansi::KeyboardModes, behavior: ansi::KeyboardModesApplyBehavior) {
        self.term.set_keyboard_mode(mode, behavior);
    }

    fn set_modify_other_keys(&mut self, mode: ansi::ModifyOtherKeys) {
        self.term.set_modify_other_keys(mode);
    }

    fn report_modify_other_keys(&mut self) {
        self.term.report_modify_other_keys();
    }

    fn set_scp(&mut self, char_path: ansi::ScpCharPath, update_mode: ansi::ScpUpdateMode) {
        self.term.set_scp(char_path, update_mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(cols: u16, rows: u16, output: &str) -> AlacrittyEngine {
        let mut e = AlacrittyEngine::new(Size::new(cols, rows), &EngineConfig::default());
        e.feed(output.as_bytes());
        e
    }

    #[test]
    fn rows_from_text() {
        let row = Row::from_text("a漢b", 6);
        assert_eq!(row.cells.len(), 6);
        assert_eq!((row.cells[1].ch, row.cells[1].width), ('漢', 2));
        assert_eq!(row.cells[2].width, 0);
        assert_eq!(row.text(), "a漢b  ");
        assert_eq!(row.trimmed_text(), "a漢b");
        // Too long: cut at the edge, a wide character that doesn't fit left out.
        assert_eq!(Row::from_text("abc漢", 4).trimmed_text(), "abc");
        // A combining mark stays with its letter.
        let marked = Row::from_text("e\u{301}x", 3);
        assert_eq!(marked.cells[0].zerowidth.as_deref(), Some("\u{301}"));
        assert_eq!(marked.text(), "e\u{301}x ");
    }

    #[test]
    fn row_text_for_the_presentation_pass() {
        let e = engine(12, 3, "\x1b]8;;https://example.org/docs\x1b\\docs\x1b]8;;\x1b\\ 漢 e\u{301}\r\nnext");
        let row = e.row(0);
        assert_eq!(row.links.len(), 1);
        assert_eq!((row.links[0].start, row.links[0].end), (0, 4));
        assert_eq!(row.links[0].uri, "https://example.org/docs");
        assert!(row.cells[0].flags.contains(CellFlags::LINK) && !row.cells[4].flags.contains(CellFlags::LINK));
        let text = row.row_text(false);
        assert_eq!(text.text, "docs 漢 e\u{301}   ");
        assert_eq!(text.cols[..9], [0, 1, 2, 3, 4, 5, 7, 8, 8]);
        assert_eq!(text.hyperlinks, [crate::present::Hyperlink { cols: 0..4, uri: "https://example.org/docs".into() }]);
        // A long line: the second row goes on from the first.
        let e = engine(5, 3, "abcdefgh");
        let texts = row_texts(&[e.row(0), e.row(1), e.row(2)], false);
        assert_eq!(texts.iter().map(|t| t.wrapped).collect::<Vec<_>>(), [false, true, false]);
    }

    #[test]
    fn colours_and_styles() {
        let e = engine(20, 2, "\x1b[31ma\x1b[91mb\x1b[38;5;123mc\x1b[39;48;2;9;8;7md\x1b[0;2;3;7;8;9me\x1b[0;4:3mf\x1b[0;4:2mg\x1b[0m");
        let c: Vec<Cell> = (0..7).map(|i| e.cell(0, i)).collect();
        assert_eq!(c[0].fg, Color::Indexed(1));
        assert_eq!(c[1].fg, Color::Indexed(9));
        assert_eq!(c[2].fg, Color::Indexed(123));
        assert_eq!((c[3].fg, c[3].bg), (Color::Foreground, Color::Rgb(Rgb::new(9, 8, 7))));
        let all = CellFlags::DIM | CellFlags::ITALIC | CellFlags::INVERSE | CellFlags::HIDDEN | CellFlags::STRIKEOUT;
        assert_eq!(c[4].flags, all);
        assert_eq!(c[5].flags, CellFlags::UNDERCURL);
        assert_eq!(c[6].flags, CellFlags::DOUBLE_UNDERLINE);
        assert_eq!(e.cell(0, 9), Cell::default());
    }

    #[test]
    fn damage_is_per_row_until_everything_moves() {
        let mut e = engine(10, 4, "one\r\ntwo");
        assert_eq!(e.take_damage(), Damage::Full);
        e.feed(b"!");
        assert_eq!(e.take_damage(), Damage::Rows(vec![1]));
        e.feed(b"\r\n\r\n\r\nscrolled");
        assert_eq!(e.take_damage(), Damage::Full, "scrolling the screen moves every row");
        e.resize(Size::new(24, 4));
        assert_eq!(e.take_damage(), Damage::Full);
    }

    #[test]
    fn resize_reflows_wrapped_lines() {
        let mut e = engine(10, 3, "0123456789abc");
        assert!(e.row(0).wraps);
        assert_eq!(e.row(1).trimmed_text(), "abc");
        e.resize(Size::new(20, 3));
        assert_eq!(e.size(), Size::new(20, 3));
        assert_eq!(e.row(0).trimmed_text(), "0123456789abc");
        assert!(!e.row(0).wraps);
        // A hidden or minimized pane's size (0×0, or a sliver) is ignored: the size stays.
        e.resize(Size::new(0, 0));
        assert_eq!(e.size(), Size::new(20, 3));
        e.resize(Size::new(19, 40));
        assert_eq!(e.size(), Size::new(20, 3));
        assert_eq!(e.row(0).trimmed_text(), "0123456789abc");
        // Made with one, the engine still starts at two columns and a row.
        assert_eq!(AlacrittyEngine::new(Size::new(0, 0), &EngineConfig::default()).size(), Size::MIN);
    }

    #[test]
    fn history_rows_and_scrolling() {
        let mut e = engine(10, 3, "");
        for i in 0..10 {
            e.feed(format!("line {i}\r\n").as_bytes());
        }
        assert_eq!(e.history_size(), 8);
        assert_eq!(e.grid_row(-1).unwrap().trimmed_text(), "line 7");
        assert_eq!(e.grid_row(-8).unwrap().trimmed_text(), "line 0");
        assert_eq!(e.grid_row(-9), None);
        assert_eq!(e.grid_row(3), None);
        e.scroll_display(2);
        assert_eq!(e.display_offset(), 2);
        assert_eq!(e.row(0).trimmed_text(), "line 6");
        e.scroll_display(100);
        assert_eq!(e.display_offset(), 8);
        e.scroll_to_bottom();
        assert_eq!(e.display_offset(), 0);
        // Less history from now on: the oldest goes.
        e.set_history(5);
        assert_eq!(e.history_size(), 5);
        assert!(e.take_events().is_empty(), "no repeated title");
    }

    #[test]
    fn the_cursor_from_settings_changes_at_once_unless_the_program_set_its_own() {
        let mut e = engine(10, 3, "ab");
        e.set_cursor_default(CursorShape::Bar, true);
        assert_eq!((e.cursor().shape, e.cursor().blinking), (CursorShape::Bar, true));
        assert!(e.take_events().is_empty(), "no repeated title");
        // A program's own cursor (vim's block) stays while Settings changes.
        e.feed(b"\x1b[2 q");
        e.set_cursor_default(CursorShape::Underline, false);
        assert_eq!((e.cursor().shape, e.cursor().blinking), (CursorShape::Block, false));
        // It gives it up (DECSCUSR 0): the cursor is Settings' again.
        e.feed(b"\x1b[0 q");
        assert_eq!((e.cursor().shape, e.cursor().blinking), (CursorShape::Underline, false));
    }

    #[test]
    fn cursor_shape_visibility_and_wide_characters() {
        let mut e = engine(10, 3, "ab");
        assert_eq!(e.cursor(), Cursor { line: 0, col: 2, shape: CursorShape::Block, blinking: false, visible: true });
        e.feed(b"\x1b[5 q");
        assert_eq!((e.cursor().shape, e.cursor().blinking), (CursorShape::Bar, true));
        e.feed(b"\x1b[4 q");
        assert_eq!((e.cursor().shape, e.cursor().blinking), (CursorShape::Underline, false));
        e.feed(b"\x1b[?25l");
        assert!(!e.cursor().visible);
        e.feed(b"\x1b[?25h\r\n\xe6\xbc\xa2\x1b[D");
        // On the right half of 漢 the cursor covers the whole character.
        let mut e2 = engine(10, 3, "漢");
        e2.feed(b"\x1b[1;2H");
        assert_eq!(e2.cursor().col, 0);
    }

    #[test]
    fn modes_programs_turn_on() {
        let mut e = engine(10, 3, "");
        assert_eq!(e.modes(), Modes { show_cursor: true, line_wrap: true, alternate_scroll: true, ..Modes::default() });
        e.feed(b"\x1b[?1049h\x1b[?1h\x1b[?2004h\x1b[?1004h\x1b[?1003h\x1b[?1006h\x1b=");
        let m = e.modes();
        assert!(m.alt_screen && m.app_cursor && m.bracketed_paste && m.focus_reports && m.app_keypad);
        assert_eq!((m.mouse, m.mouse_encoding), (MouseProtocol::Any, MouseEncoding::Sgr));
        e.feed(b"\x1b[?1002h\x1b[?1005h");
        assert_eq!((e.modes().mouse, e.modes().mouse_encoding), (MouseProtocol::Drag, MouseEncoding::Utf8));
        e.feed(b"\x1b[?1000h");
        assert_eq!(e.modes().mouse, MouseProtocol::Vt200);
        e.reset();
        assert_eq!(e.modes().mouse, MouseProtocol::Off);
        assert!(!e.modes().alt_screen);
    }

    #[test]
    fn title_and_program_colours() {
        let mut e = engine(10, 3, "\x1b]2;deploy\x07\x1b]4;3;rgb:12/34/56\x07");
        assert_eq!(e.title(), Some("deploy"));
        assert_eq!(e.take_events(), [TermEvent::Title(Some("deploy".into()))]);
        assert_eq!(e.program_color(3), Some(Rgb::new(0x12, 0x34, 0x56)));
        assert_eq!(e.program_color(4), None);
        assert_eq!(e.program_color(100_000), None);
        e.reset();
        assert_eq!(e.title(), None);
    }

    #[test]
    fn answer_colours_cover_the_palette() {
        let a = AnswerColors::default();
        assert_eq!(a.get(1), Some(Rgb::new(0xcd, 0, 0)));
        assert_eq!(a.get(16), Some(Rgb::new(0, 0, 0)));
        assert_eq!(a.get(21), Some(Rgb::new(0, 0, 0xff)));
        assert_eq!(a.get(196), Some(Rgb::new(0xff, 0, 0)));
        assert_eq!(a.get(231), Some(Rgb::new(0xff, 0xff, 0xff)));
        assert_eq!(a.get(232), Some(Rgb::new(8, 8, 8)));
        assert_eq!(a.get(255), Some(Rgb::new(238, 238, 238)));
        assert_eq!(a.get(257), Some(a.background));
        assert_eq!(a.get(259), None);
    }

    #[test]
    fn grid_points_and_the_view() {
        assert_eq!(GridPoint::from_view(0, 3, 5), GridPoint::new(-5, 3));
        assert_eq!(GridPoint::new(-5, 3).view_line(5, 24), Some(0));
        assert_eq!(GridPoint::new(-6, 3).view_line(5, 24), None);
        assert_eq!(GridPoint::new(19, 0).view_line(5, 24), None);
        assert!(GridPoint::new(-1, 9) < GridPoint::new(0, 0));
    }
}
