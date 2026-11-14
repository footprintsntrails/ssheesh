//! Selecting text with the mouse, and the text a copy gets.
//!
//! The selection itself is alacritty's (simple, block, by word, by line), kept inside the
//! engine so it moves with the text when output scrolls. What this adds is the copied text: it
//! is read row by row like xterm.js did (blanks at a line's end dropped, wrapped rows joined
//! without a line break) and goes through a [`CopyPresenter`] first, so streamer mode can turn
//! names into dots in what you copy, exactly as on screen.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side as AlacSide};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::Term;

use crate::engine::{Engine, GridPoint, Row};

/// How a selection grows as the mouse moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionKind {
    /// Cell by cell: click and drag.
    Simple,
    /// A rectangle: Alt and drag.
    Block,
    /// Whole words: a double-click.
    Word,
    /// Whole lines, wrapped parts included: a triple-click.
    Lines,
}

/// Which half of a cell the pointer is on. A drag that ends on a cell's left half leaves that
/// cell out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

/// The selected cells: from `start` to `end`, both included, in reading order; or the rectangle
/// they span when `block`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SelectionSpan {
    pub start: GridPoint,
    pub end: GridPoint,
    pub block: bool,
}

impl SelectionSpan {
    /// The selected columns of grid line `line`, as `start..end`, for a terminal `cols` wide.
    pub fn columns_on(&self, line: i32, cols: u16) -> Option<std::ops::Range<u16>> {
        if line < self.start.line || line > self.end.line {
            return None;
        }
        if self.block {
            let (a, b) = (self.start.col.min(self.end.col), self.start.col.max(self.end.col));
            return Some(a..(b + 1).min(cols));
        }
        let from = if line == self.start.line { self.start.col } else { 0 };
        let to = if line == self.end.line { self.end.col + 1 } else { cols };
        (from < to.min(cols)).then(|| from..to.min(cols))
    }

    pub fn contains(&self, at: GridPoint) -> bool {
        self.columns_on(at.line, u16::MAX).is_some_and(|r| r.contains(&at.col))
    }
}

/// Turns the text of one line (rows joined across wraps) into what may leave the terminal.
///
/// The answer must have exactly as many characters as the line, one for each (streamer mode
/// puts a dot in place of each character of a name); a presenter that changes the length is
/// applied to the copied piece alone instead.
pub trait CopyPresenter {
    fn present_line(&self, line: &str) -> String;
}

impl<F: Fn(&str) -> String> CopyPresenter for F {
    fn present_line(&self, line: &str) -> String {
        self(line)
    }
}

/// Copies text as it is.
pub struct AsIs;

impl CopyPresenter for AsIs {
    fn present_line(&self, line: &str) -> String {
        line.to_owned()
    }
}

/// The selected text, if anything is selected. Lines end in `\n`; 0.10 used `\r\n` on Windows
/// (xterm.js does), so the caller there swaps them before copying.
///
/// The presenter runs with the engine locked here; [`crate::model::Model::selected_text`]
/// copies the rows out first and runs it after.
pub fn selected_text(engine: &dyn Engine, presenter: &dyn CopyPresenter) -> Option<String> {
    copy_selection(engine).map(|rows| rows.text(presenter))
}

/// The rows a copy of the selection needs, if anything is selected (see [`copy_rows`]).
pub fn copy_selection(engine: &dyn Engine) -> Option<CopiedRows> {
    engine.selection().map(|span| copy_rows(engine, span))
}

/// The text of `span`, read as xterm.js read a selection: wrapped rows joined without a line
/// break, a rectangle row by row, and the blanks after a row's last character dropped where a
/// line ends. (xterm.js dropped only cells nothing was ever written to; alacritty can't tell
/// those from written spaces, so blanks at a line's end go and blanks inside it stay. A space
/// in the last column of a row that wraps is inside the line.)
pub fn copy_text(engine: &dyn Engine, span: SelectionSpan, presenter: &dyn CopyPresenter) -> String {
    copy_rows(engine, span).text(presenter)
}

/// Rows of context the presenter gets around a selection, above and below, when its first or
/// last line goes on past it: enough for a name cut by the selection's edge to be recognised,
/// not ten thousand rows of a minified file for a double-click.
pub const CONTEXT_ROWS: i32 = 4;

/// The rows a copy reads, taken out of the engine (do this with it locked, it is quick), so the
/// text can be made after letting go of it ([`CopiedRows::text`]).
pub fn copy_rows(engine: &dyn Engine, span: SelectionSpan) -> CopiedRows {
    let cols = engine.size().cols;
    // Widen to whole lines, up to CONTEXT_ROWS: up while the row above wraps into this one,
    // down while this one wraps.
    let mut top = span.start.line;
    while span.start.line - top < CONTEXT_ROWS && engine.row_wraps(top - 1) == Some(true) {
        top -= 1;
    }
    let mut bottom = span.end.line;
    while bottom - span.end.line < CONTEXT_ROWS && engine.row_wraps(bottom) == Some(true) && engine.row_wraps(bottom + 1).is_some() {
        bottom += 1;
    }
    let rows = (top..=bottom)
        .map(|line| CopyRow::new(&engine.grid_row(line).unwrap_or_else(|| Row::blank(cols))))
        .collect();
    CopiedRows { span, cols, top, rows }
}

/// Rows of the grid around a selection, copied out of the engine (see [`copy_rows`]).
#[derive(Clone, Debug)]
pub struct CopiedRows {
    span: SelectionSpan,
    cols: u16,
    /// The grid line of `rows[0]`.
    top: i32,
    rows: Vec<CopyRow>,
}

/// One row as a copy reads it.
#[derive(Clone, Debug)]
struct CopyRow {
    /// What its cells show, left to right: a character and its marks each; nothing for the right
    /// half of a wide character.
    text: String,
    /// Characters of `text` before each cell, and the whole count last: cell `c` shows
    /// characters `starts[c]..starts[c + 1]`.
    starts: Vec<u32>,
    /// Each cell's [`crate::engine::Cell::width`].
    widths: Vec<u8>,
    /// The column after the last one that shows something (0 for a blank row).
    content_end: u16,
    wraps: bool,
}

impl CopyRow {
    fn new(row: &Row) -> Self {
        let mut text = String::with_capacity(row.cells.len());
        let mut starts = Vec::with_capacity(row.cells.len() + 1);
        let mut widths = Vec::with_capacity(row.cells.len());
        let mut chars = 0u32;
        let mut content_end = 0;
        for (col, cell) in row.cells.iter().enumerate() {
            starts.push(chars);
            widths.push(cell.width);
            if cell.width == 0 {
                continue;
            }
            text.push(cell.ch);
            chars += 1;
            if let Some(marks) = &cell.zerowidth {
                text.push_str(marks);
                chars += marks.chars().count() as u32;
            }
            if cell.ch != ' ' || cell.zerowidth.is_some() {
                content_end = col + 1;
            }
        }
        starts.push(chars);
        CopyRow { text, starts, widths, content_end: content_end as u16, wraps: row.wraps }
    }

    fn chars(&self) -> usize {
        self.starts.last().copied().unwrap_or(0) as usize
    }
}

impl CopiedRows {
    /// The grid line of the first row the presenter reads (the selection's first row, or above
    /// it when its line goes on from there).
    pub fn top(&self) -> i32 {
        self.top
    }

    /// The copy: each line goes through `presenter` whole (rows joined across wraps, within
    /// the context taken), so streamer mode turns names into dots in what you copy exactly as
    /// on screen, also a name the selection cuts.
    pub fn text(&self, presenter: &dyn CopyPresenter) -> String {
        let rows = &self.rows;
        // What each row shows after the presenter read its line; `None` when the presenter
        // changed the line's length (then the copied piece goes through it instead).
        let mut shown: Vec<Option<String>> = vec![None; rows.len()];
        let mut begin = 0;
        while begin < rows.len() {
            let mut end = begin;
            while end + 1 < rows.len() && rows[end].wraps {
                end += 1;
            }
            let line: String = rows[begin..=end].iter().map(|r| r.text.as_str()).collect();
            let presented = presenter.present_line(&line);
            if presented.chars().count() == line.chars().count() {
                let mut chars = presented.chars();
                for (r, row) in rows.iter().enumerate().take(end + 1).skip(begin) {
                    shown[r] = Some(chars.by_ref().take(row.chars()).collect());
                }
            }
            begin = end + 1;
        }

        let span = self.span;
        // Each line of the copy, and whether the presenter still has to read it.
        let mut lines: Vec<(String, bool)> = Vec::new();
        let mut joins_previous = false;
        for line in span.start.line..=span.end.line {
            let index = (line - self.top) as usize;
            let row = &rows[index];
            let Some(range) = span.columns_on(line, self.cols) else {
                // A line with nothing selected (a simple selection ending left of column 0).
                joins_previous = false;
                continue;
            };
            let mut start = range.start as usize;
            // Starting on the right half of a wide character takes the whole character.
            if start > 0 && row.widths[start] == 0 && row.widths[start - 1] == 2 {
                start -= 1;
            }
            // Blanks go only where the line ends: a row that wraps goes on in the next.
            let end = if row.wraps && !span.block { range.end as usize } else { (range.end as usize).min(row.content_end as usize) };
            let end = end.max(start);
            let text = shown[index].as_deref().unwrap_or(&row.text);
            let piece = char_range(text, row.starts[start], row.starts[end]).replace('\u{a0}', " ");
            let raw = shown[index].is_none();
            match lines.last_mut() {
                Some((last, last_raw)) if joins_previous && !span.block => {
                    last.push_str(&piece);
                    *last_raw |= raw;
                }
                _ => lines.push((piece, raw)),
            }
            joins_previous = row.wraps;
        }
        let lines: Vec<String> = lines.into_iter().map(|(text, raw)| if raw { presenter.present_line(&text) } else { text }).collect();
        lines.join("\n")
    }
}

/// Characters `from..to` of `text`.
fn char_range(text: &str, from: u32, to: u32) -> &str {
    let mut at = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len()));
    let a = at.by_ref().nth(from as usize).unwrap_or(text.len());
    let b = if to > from { at.nth((to - from - 1) as usize).unwrap_or(text.len()) } else { a };
    &text[a..b]
}

// ---------------------------------------------------------------------------------------------
// alacritty's selection, used by `AlacrittyEngine`.

fn point<T>(term: &Term<T>, at: GridPoint) -> Point {
    let line = at.line.clamp(term.topmost_line().0, term.bottommost_line().0);
    let col = (at.col as usize).min(term.columns() - 1);
    Point::new(Line(line), Column(col))
}

fn side(side: Side) -> AlacSide {
    match side {
        Side::Left => AlacSide::Left,
        Side::Right => AlacSide::Right,
    }
}

pub(crate) fn start<T>(term: &mut Term<T>, kind: SelectionKind, at: GridPoint, s: Side) {
    let ty = match kind {
        SelectionKind::Simple => SelectionType::Simple,
        SelectionKind::Block => SelectionType::Block,
        SelectionKind::Word => SelectionType::Semantic,
        SelectionKind::Lines => SelectionType::Lines,
    };
    let at = point(term, at);
    term.selection = Some(Selection::new(ty, at, side(s)));
}

pub(crate) fn update<T>(term: &mut Term<T>, at: GridPoint, s: Side) {
    let at = point(term, at);
    if let Some(selection) = term.selection.as_mut() {
        selection.update(at, side(s));
    }
}

pub(crate) fn select_all<T>(term: &mut Term<T>) {
    let mut selection = Selection::new(SelectionType::Simple, Point::new(term.topmost_line(), Column(0)), AlacSide::Left);
    selection.update(Point::new(term.bottommost_line(), term.last_column()), AlacSide::Right);
    term.selection = Some(selection);
}

pub(crate) fn span<T>(term: &Term<T>) -> Option<SelectionSpan> {
    let range = term.selection.as_ref()?.to_range(term)?;
    let at = |p: Point| GridPoint::new(p.line.0, p.column.0 as u16);
    Some(SelectionSpan { start: at(range.start), end: at(range.end), block: range.is_block })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AlacrittyEngine, EngineConfig, Size};

    fn engine(cols: u16, rows: u16, output: &str) -> AlacrittyEngine {
        let mut e = AlacrittyEngine::new(Size::new(cols, rows), &EngineConfig::default());
        e.feed(output.as_bytes());
        e
    }

    fn select(e: &mut AlacrittyEngine, kind: SelectionKind, from: (i32, u16), to: (i32, u16)) -> String {
        e.start_selection(kind, GridPoint::new(from.0, from.1), Side::Left);
        e.update_selection(GridPoint::new(to.0, to.1), Side::Right);
        selected_text(e, &AsIs).unwrap()
    }

    #[test]
    fn simple_selection_drops_trailing_blanks_and_keeps_line_breaks() {
        let mut e = engine(20, 4, "first line   \r\nsecond\r\nthird");
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 0), (2, 19)), "first line\nsecond\nthird");
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 6), (1, 2)), "line\nsec");
    }

    #[test]
    fn wrapped_rows_join_without_a_break() {
        let mut e = engine(10, 4, "0123456789abcdefghij-tail\r\nnext");
        assert!(e.row(0).wraps && e.row(1).wraps && !e.row(2).wraps);
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 0), (3, 9)), "0123456789abcdefghij-tail\nnext");
    }

    #[test]
    fn word_and_line_selection() {
        let mut e = engine(30, 3, "ssh dev@web-7731 -p 22\r\n");
        e.start_selection(SelectionKind::Word, GridPoint::new(0, 1), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "ssh");
        e.start_selection(SelectionKind::Lines, GridPoint::new(0, 5), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "ssh dev@web-7731 -p 22");
    }

    #[test]
    fn block_selection_is_row_by_row() {
        let mut e = engine(10, 3, "abcdef\r\nghijkl\r\nmnopqr");
        assert_eq!(select(&mut e, SelectionKind::Block, (0, 1), (2, 3)), "bcd\nhij\nnop");
    }

    #[test]
    fn wide_characters_copy_once() {
        let mut e = engine(10, 2, "a漢字b");
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 0), (0, 9)), "a漢字b");
        // Starting on the right half of 漢 still takes it.
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 2), (0, 3)), "漢字");
    }

    #[test]
    fn presenter_sees_the_whole_line() {
        // A presenter that hides "web-7731" one dot per character, as streamer mode does.
        let hide = |line: &str| line.replace("web-7731", "••••••••");
        let mut e = engine(12, 3, "ssh web-7731 now");
        // The selection starts in the middle of the name, on the second row of the wrap.
        e.start_selection(SelectionKind::Simple, GridPoint::new(0, 8), Side::Left);
        e.update_selection(GridPoint::new(1, 3), Side::Right);
        assert_eq!(selected_text(&e, &hide).unwrap(), "•••• now");
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "7731 now");
    }

    #[test]
    fn presenter_that_changes_length_still_hides() {
        let mut e = engine(20, 2, "token abc123");
        let shorten = |line: &str| line.replace("abc123", "*");
        e.select_all();
        // The copied piece goes through it instead, so nothing raw slips through.
        let text = selected_text(&e, &shorten).unwrap();
        assert!(!text.contains("abc123"));
    }

    #[test]
    fn selection_follows_scrolling_output() {
        let mut e = engine(10, 3, "one\r\ntwo\r\nthree");
        e.start_selection(SelectionKind::Lines, GridPoint::new(0, 0), Side::Left);
        e.feed(b"\r\nfour\r\nfive");
        // "one" scrolled up into the history and the selection went with it.
        let span = e.selection().unwrap();
        assert_eq!(span.start.line, -2);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "one");
    }

    #[test]
    fn a_space_where_a_row_wraps_is_kept() {
        // "abcdefghi " fills the first row and wraps: the space is part of the line.
        let mut e = engine(10, 3, "abcdefghi jkl");
        assert!(e.row(0).wraps);
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 0), (1, 9)), "abcdefghi jkl");
        e.start_selection(SelectionKind::Lines, GridPoint::new(0, 3), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "abcdefghi jkl");
        // A command wrapped at the space between two paths stays two paths.
        let mut e = engine(20, 3, "rm -r /srv/app/cache /srv/app/tmp");
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 0), (1, 19)), "rm -r /srv/app/cache /srv/app/tmp");
        // Spaces inside a row stay, blanks after its last character go.
        let mut e = engine(20, 2, "ab cd");
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 0), (0, 2)), "ab ");
        assert_eq!(select(&mut e, SelectionKind::Simple, (0, 3), (0, 19)), "cd");
    }

    #[test]
    fn double_click_takes_a_host_and_port_or_a_url_whole() {
        let mut e = engine(60, 2, "ssh dev@203.0.113.5:8080 see https://example.org/docs|less");
        e.start_selection(SelectionKind::Word, GridPoint::new(0, 10), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "dev@203.0.113.5:8080");
        e.start_selection(SelectionKind::Word, GridPoint::new(0, 35), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "https://example.org/docs|less");
        // Brackets, quotes and box-drawing lines still end a word.
        let mut e = engine(40, 2, "(web-7731) 'db-main' │x│");
        e.start_selection(SelectionKind::Word, GridPoint::new(0, 3), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "web-7731");
        e.start_selection(SelectionKind::Word, GridPoint::new(0, 14), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "db-main");
        e.start_selection(SelectionKind::Word, GridPoint::new(0, 22), Side::Left);
        assert_eq!(selected_text(&e, &AsIs).unwrap(), "x");
    }

    #[test]
    fn the_presenter_gets_a_few_rows_around_a_selection_in_a_huge_line() {
        use std::cell::Cell as Count;
        // One line of 200 rows; a word in the middle of it.
        let line: String = (0..400).map(|i| format!("word{i:04} ")).collect();
        let mut e = engine(20, 4, "");
        e.set_history(1000);
        e.feed(line.as_bytes());
        let rows = e.history_size() as i32 + 4;
        let middle = -(rows / 2);
        let seen = Count::new(0usize);
        let counting = |text: &str| {
            seen.set(seen.get().max(text.chars().count()));
            text.to_owned()
        };
        e.start_selection(SelectionKind::Word, GridPoint::new(middle, 2), Side::Left);
        let word = selected_text(&e, &counting).unwrap();
        assert!(word.starts_with("word") && word.len() == 8, "{word:?}");
        // The word's rows (it can start on the row above) and the context around them.
        assert!(seen.get() <= 20 * (2 * CONTEXT_ROWS as usize + 2), "the presenter read {} characters", seen.get());
        // The rows are copied out first; the text is made from them after.
        let copied = copy_selection(&e).unwrap();
        e.feed(b"\x1b[2J\x1b[3J");
        assert_eq!(copied.text(&AsIs), word);
    }

    #[test]
    fn span_columns() {
        let span = SelectionSpan { start: GridPoint::new(0, 4), end: GridPoint::new(2, 1), block: false };
        assert_eq!(span.columns_on(0, 10), Some(4..10));
        assert_eq!(span.columns_on(1, 10), Some(0..10));
        assert_eq!(span.columns_on(2, 10), Some(0..2));
        assert_eq!(span.columns_on(3, 10), None);
        assert!(span.contains(GridPoint::new(1, 7)));
        assert!(!span.contains(GridPoint::new(0, 3)));
        let block = SelectionSpan { start: GridPoint::new(0, 4), end: GridPoint::new(2, 1), block: true };
        assert_eq!(block.columns_on(1, 10), Some(1..5));
    }
}
