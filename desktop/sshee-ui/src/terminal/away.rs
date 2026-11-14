//! "While you were away": one line in the output, above the first line a server's shell printed
//! while the connection was down (0.10 showed a card over the terminal; the 1.0 design puts a
//! line where the new output starts).
//!
//! ```text
//! the connection drops     the last finished lines above the cursor are kept (`sshee_term::away`)
//! it comes back (in tmux)  a moment later tmux's history is read and compared: what follows
//!                          those lines is new
//! the line                 sits above the first new line while that is on screen; pinned to
//!                          the top while it is still above (a click scrolls up to it); gone
//!                          once it has been seen and scrolled away, or by its ×
//! ```
//!
//! The line takes room between two rows of the grid without the terminal changing size (the
//! program would redraw): the rows below move down into the pane's bottom margin, and when
//! that isn't enough the rows above move up into the top one. [`Inset`] is that arithmetic,
//! for drawing and for telling which cell a click is on.

use std::rc::Rc;
use std::time::Duration;

use rune::{grid, text, El, GridRow, Tx};
use sshee_term::away::{anchor_lines, build_recap, cursor_line, Recap, RowList, ANCHOR_LINES};
use sshee_term::engine::Engine;
use sshee_term::present::RowText;

use super::metrics::CellMetrics;
use super::overlays::{Ask, Note, Reply};
use super::pane::{TermPane, PADDING};
use super::LinkStatus;
use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// The line's height in points (the design's).
pub const LINE: f32 = 18.0;

/// The least margin left between the text and the pane's edge while the line takes its room.
const MARGIN: f32 = 2.0;

/// How long after the connection is back the history is read: tmux has redrawn by then
/// (0.10's wait).
const SETTLE: f64 = 0.7;

/// How much of tmux's history is read (0.10's).
const HISTORY_LINES: u32 = 4000;

/// What was on screen when the connection went.
#[derive(Clone, Debug, PartialEq)]
struct Mark {
    anchor: Vec<String>,
    current: String,
    /// When it went, in ms (the wall clock; a replay's own clock).
    since: u64,
}

/// Where the session is, as this module cares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Was {
    #[default]
    Other,
    /// Connected, in tmux.
    Durable,
    /// The connection is down and may come back.
    Down,
}

/// The line on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub recap: Recap,
    /// The first rows of the new output as the screen shows them, trimmed: what the line is
    /// found by.
    first: Vec<String>,
    /// Rows the new output takes.
    rows: usize,
    /// The view line the line sits above; `None` while the first new line is above the view
    /// (the line is then pinned to the top).
    pub above: Option<usize>,
    /// The first new line has been on screen.
    seen: bool,
    /// Frames the pane had taken when the line was last placed.
    placed_at: u64,
}

/// A pane's "while you were away".
#[derive(Default)]
pub struct Away {
    was: Was,
    mark: Option<Mark>,
    /// When to read the history (pane clock).
    ask_at: Option<f64>,
    pub(crate) shown: Option<Shown>,
}

/// "41 min", "2 h 5 min", "less than a minute" (0.10's `awayFor`).
pub fn away_for(ms: u64) -> String {
    let min = (ms as f64 / 60_000.0).round() as u64;
    match min {
        0 => "less than a minute".into(),
        1..=59 => format!("{min} min"),
        _ => format!("{} h {} min", min / 60, min % 60),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The line's words: what's plain, and the errors, which are said louder.
/// "While you were away (41 min): 2 new lines, " + "1 error" + ", 3 warnings".
pub fn words(recap: &Recap) -> (String, String, String) {
    let mut lead = format!("While you were away ({}): {}", away_for(recap.until.saturating_sub(recap.since)), plural(recap.lines.len(), "new line"));
    let errors = if recap.errors > 0 { plural(recap.errors, "error") } else { String::new() };
    let mut tail = String::new();
    if recap.warnings > 0 {
        tail = plural(recap.warnings, "warning");
        if !errors.is_empty() {
            tail.insert_str(0, ", ");
        }
    }
    if !errors.is_empty() || !tail.is_empty() {
        lead.push_str(", ");
    }
    (lead, errors, tail)
}

/// The rows a line of `text` takes at `cols` columns.
fn rows_of(text: &str, cols: usize) -> usize {
    text.trim_end().chars().count().div_ceil(cols.max(1)).max(1)
}

/// The first `count` rows the new lines take on a screen `cols` wide, trimmed.
fn first_rows(lines: &[String], cols: usize, count: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        let chars: Vec<char> = line.trim_end().chars().collect();
        if chars.is_empty() {
            out.push(String::new());
        }
        for piece in chars.chunks(cols.max(1)) {
            out.push(piece.iter().collect::<String>().trim_end().to_owned());
        }
        if out.len() >= count {
            break;
        }
    }
    out.truncate(count);
    out
}

/// Where rows `first` start on a screen of `screen` rows (trimmed texts), looking near `near`
/// first: at it, then above it (output moves rows up), then anywhere, lowest first.
fn locate(screen: &[String], first: &[String], near: Option<usize>) -> Option<usize> {
    if first.is_empty() || first.iter().all(|r| r.is_empty()) {
        return None;
    }
    let fits = |at: usize| first.iter().enumerate().all(|(i, want)| screen.get(at + i).is_none_or(|have| have == want)) && screen.get(at) == Some(&first[0]);
    let last = screen.len().saturating_sub(1);
    let near = near.map(|n| n.min(last));
    let above = near.into_iter().flat_map(|n| (0..=n).rev());
    let rest = (0..screen.len()).rev();
    above.chain(rest).find(|at| fits(*at))
}

/// How the grid's rows make room for the line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inset {
    /// The line sits above this view line.
    pub line: usize,
    /// Rows above it move up by this, rows from it on move down by `down`.
    pub up: f32,
    pub down: f32,
}

impl Inset {
    /// Room for the line above view line `line`, in a pane `height` points high holding
    /// `rows` rows of `cell_h`: the rows below move down as far as the bottom margin lets
    /// them, the rows above take the rest.
    pub fn new(line: usize, height: f32, rows: usize, cell_h: f32) -> Inset {
        let slack = (height - 2.0 * PADDING - rows as f32 * cell_h).max(0.0);
        let down = LINE.min(slack + PADDING - MARGIN);
        Inset { line, up: (LINE - down).min(PADDING - MARGIN), down }
    }

    /// Where the line itself is drawn.
    pub fn y(&self, cell_h: f32) -> f32 {
        PADDING - self.up + self.line as f32 * cell_h
    }

    /// The top of view line `line` as drawn.
    pub fn line_y(&self, line: usize, cell_h: f32) -> f32 {
        PADDING + line as f32 * cell_h + if line < self.line { -self.up } else { self.down }
    }

    /// A drawn `y` as it would be without the line: for telling the cell under it. A point on
    /// the line itself counts as the top of the row under it.
    pub fn undo(&self, y: f32, cell_h: f32) -> f32 {
        let top = self.y(cell_h);
        if y < top {
            y + self.up
        } else if y < top + LINE {
            PADDING + self.line as f32 * cell_h
        } else {
            y - self.down
        }
    }
}

impl TermPane {
    /// The room the away line takes in the grid, while it shows.
    pub(crate) fn inset(&self, m: &CellMetrics) -> Option<Inset> {
        let shown = self.over.away.shown.as_ref()?;
        let [_, h] = self.area?;
        Some(Inset::new(shown.above.unwrap_or(0), h, self.size().rows as usize, m.cell_h))
    }

    /// The top of a view line in the pane's points, the away line's room counted.
    pub(crate) fn line_y(&self, m: &CellMetrics, line: usize) -> f32 {
        match self.inset(m) {
            Some(inset) => inset.line_y(line, m.cell_h),
            None => PADDING + line as f32 * m.cell_h,
        }
    }

    /// A pointer's `y` in the pane, as it would be without the away line.
    pub(crate) fn plain_y(&self, m: &CellMetrics, y: f32) -> f32 {
        self.inset(m).map_or(y, |inset| inset.undo(y, m.cell_h))
    }

    fn now_ms(&self) -> u64 {
        if self.replays() {
            (self.clock.max(0.0) * 1000.0) as u64
        } else {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
        }
    }

    /// The screen's rows as they came, trimmed, and the cursor's row.
    fn screen_rows(&self) -> (Vec<RowText>, usize) {
        self.model.with_engine(|engine| {
            let mut wrapped = false;
            let rows = (0..engine.size().rows as usize)
                .map(|line| {
                    let row = engine.row(line);
                    let text = row.row_text(wrapped);
                    wrapped = row.wraps;
                    text
                })
                .collect();
            (rows, engine.cursor().line)
        })
    }

    /// Follows the session's status, reads the history when it's time, and keeps the line
    /// where the new output starts. True when what is drawn changed.
    pub(crate) fn poll_away(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let now = match &self.status {
            LinkStatus::Connected { durable: true } => Was::Durable,
            LinkStatus::Reconnecting { .. } | LinkStatus::Waiting { .. } => Was::Down,
            // Still on its way back.
            LinkStatus::Connecting if self.over.away.was == Was::Down => Was::Down,
            _ => Was::Other,
        };
        let was = std::mem::replace(&mut self.over.away.was, now);
        let mut changed = false;
        match (was, now) {
            (Was::Durable, Was::Down) => {
                let (rows, cursor) = self.screen_rows();
                let src = RowList { cols: self.size().cols as usize, rows: &rows };
                let mark = Mark { anchor: anchor_lines(&src, cursor, ANCHOR_LINES), current: cursor_line(&src, cursor), since: self.now_ms() };
                self.over.away.mark = Some(mark).filter(|m| !m.anchor.is_empty());
                // What was said about the last time away is old news now.
                changed = self.over.away.shown.take().is_some();
            }
            (Was::Down, Was::Durable) if self.over.away.mark.is_some() => {
                self.over.away.ask_at = Some(clock + SETTLE);
                tx.wake_after(Duration::from_secs_f64(SETTLE));
            }
            // A new shell, or none: nothing to compare with.
            (Was::Down, Was::Other) if !matches!(self.status, LinkStatus::Connecting) => self.over.away.mark = None,
            _ => {}
        }
        if let Some(at) = self.over.away.ask_at {
            if clock + 1e-6 >= at {
                self.over.away.ask_at = None;
                let asked = self.ask(Ask::History { lines: HISTORY_LINES }, |reply| {
                    Note::History(match reply {
                        Some(Reply::History { text, away_ms }) => Some((text, away_ms)),
                        _ => None,
                    })
                });
                if !asked {
                    self.over.away.mark = None;
                }
            } else {
                tx.wake_after(Duration::from_secs_f64(at - clock));
            }
        }
        changed | self.place_away()
    }

    /// tmux's history came: what follows the lines kept when the connection went is new.
    pub(crate) fn history_came(&mut self, history: Option<(String, Option<u64>)>) {
        let (Some(mark), Some((text, away_ms))) = (self.over.away.mark.take(), history) else { return };
        let since = mark.since;
        let until = away_ms.map_or_else(|| self.now_ms(), |ms| since + ms);
        let Some(recap) = build_recap(&text, &mark.anchor, &mark.current, since, until) else { return };
        let cols = self.size().cols as usize;
        let rows = recap.lines.iter().map(|l| rows_of(l, cols)).sum();
        let first = first_rows(&recap.lines, cols, 3);
        self.over.away.shown = Some(Shown { recap, first, rows, above: None, seen: false, placed_at: u64::MAX });
        self.place_away();
    }

    /// Finds the first new line on screen again after the screen changed. True when the line
    /// moved, appeared or went.
    fn place_away(&mut self) -> bool {
        let frames = self.frames;
        let Some(shown) = self.over.away.shown.as_ref().filter(|s| s.placed_at != frames) else { return false };
        let (near, seen, rows, first) = (shown.above, shown.seen, shown.rows, shown.first.clone());
        let (screen, cursor) = self.screen_rows();
        let texts: Vec<String> = screen.iter().map(|r| r.trimmed().to_owned()).collect();
        let at = if seen {
            locate(&texts, &first, near)
        } else {
            // The first time: the new output ends at the cursor's row, so it starts `rows`
            // above it; with more new rows than that, it starts above the view.
            cursor.checked_sub(rows).and_then(|start| locate(&texts, &first, Some(start)))
        };
        let Some(shown) = self.over.away.shown.as_mut() else { return false };
        shown.placed_at = frames;
        match at {
            Some(at) => {
                let moved = shown.above != Some(at) || !shown.seen;
                shown.above = Some(at);
                shown.seen = true;
                moved
            }
            // Seen and scrolled away: it has done its job.
            None if shown.seen => {
                self.over.away.shown = None;
                true
            }
            None => std::mem::take(&mut shown.above).is_some(),
        }
    }

    /// A click on the line: the view goes to the first new line, when that is above it.
    pub(crate) fn away_clicked(&mut self, tx: &mut Tx) {
        let Some(shown) = &self.over.away.shown else { return };
        if shown.above.is_none() {
            let (_, cursor) = self.screen_rows();
            let rows = self.size().rows as usize;
            // The first new line lands a third of the way down.
            let up = shown.rows.saturating_sub(cursor) + rows / 3;
            self.scroll_by(up as i32, tx);
            if let Some(shown) = &mut self.over.away.shown {
                shown.seen = true;
                shown.placed_at = u64::MAX;
            }
        }
        self.focus_terminal(tx);
        tx.instant();
    }

    /// The line's ×.
    pub(crate) fn dismiss_away(&mut self, tx: &mut Tx) {
        self.over.away.shown = None;
        self.focus_terminal(tx);
        tx.instant();
    }
}

/// The grid with the away line between its rows: the rows above the line, the line, the rows
/// from it on, each placed by the [`Inset`]. `caret`: the cursor's cell, for screen readers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn split<S: 'static>(
    pane: &TermPane,
    inset: Inset,
    rows: Vec<Rc<GridRow>>,
    cols: u16,
    m: &CellMetrics,
    caret: Option<(u16, usize)>,
    t: &'static Theme,
    get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static,
) -> Vec<El> {
    let Some(shown) = &pane.over.away.shown else { return Vec::new() };
    let width = f32::from(cols) * m.cell_w;
    let at = inset.line.min(rows.len());
    let part = |key: &'static str, from: usize, rows: &[Rc<GridRow>]| {
        let mut g = grid(cols, m.cell_w, m.cell_h, rows.to_vec()).key(key).absolute(PADDING, inset.line_y(from, m.cell_h)).size(width, rows.len() as f32 * m.cell_h);
        if let Some((col, line)) = caret.filter(|(_, line)| (from..from + rows.len()).contains(line)) {
            g = g.grid_caret(col, (line - from) as u16);
        }
        g
    };
    let (lead, errors, tail) = words(&shown.recap);
    let mut line = rune::row()
        .key("away")
        .absolute(PADDING, inset.y(m.cell_h))
        .size(width, LINE)
        .gap(0.0)
        .items_center()
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .role("button")
        .label(format!("{lead}{errors}{tail}"))
        .on_press_tx(move |s: &mut S, tx| {
            if let Some(pane) = get(s) {
                pane.away_clicked(tx);
            }
        })
        .child(text(lead).key("away-text").font(theme::NOTE).color(t.l2).single_line());
    if !errors.is_empty() {
        let mut loud = theme::NOTE;
        loud.weight = 600.0;
        line = line.child(text(errors).key("away-errors").font(loud).color(t.l1).single_line());
    }
    if !tail.is_empty() {
        line = line.child(text(tail).font(theme::NOTE).color(t.l2).single_line());
    }
    line = line.child(rune::el().key("away-rule").grow(1.0).h(1.0).ml(10.0).mr(6.0).fill(t.hair)).child(
        rune::el()
            .key("away-close")
            .size(theme::ICON, theme::ICON)
            .radius(theme::RADIUS_CHIP)
            .hover(t.hov2)
            .press(t.down)
            .role("button")
            .label_static("Dismiss")
            .group_opacity(0.0, 1.0)
            .on_press_tx(move |s: &mut S, tx| {
                if let Some(pane) = get(s) {
                    pane.dismiss_away(tx);
                }
            })
            .child(icon(Icon::Close, theme::ICON, t.l2on)),
    );
    let mut out = Vec::with_capacity(3);
    if at > 0 {
        out.push(part("grid-above", 0, &rows[..at]));
    }
    out.push(line);
    if at < rows.len() {
        out.push(part("grid-below", at, &rows[at..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recap(lines: usize, errors: usize, warnings: usize, minutes: u64) -> Recap {
        Recap { since: 0, until: minutes * 60_000, lines: vec!["x".into(); lines], errors, warnings }
    }

    #[test]
    fn the_line_reads_as_the_design_writes_it() {
        let (lead, errors, tail) = words(&recap(2, 1, 0, 41));
        assert_eq!(format!("{lead}{errors}{tail}"), "While you were away (41 min): 2 new lines, 1 error");
        assert_eq!(errors, "1 error");
        let (lead, errors, tail) = words(&recap(1, 0, 0, 0));
        assert_eq!(format!("{lead}{errors}{tail}"), "While you were away (less than a minute): 1 new line");
        let (lead, errors, tail) = words(&recap(40, 2, 3, 125));
        assert_eq!(format!("{lead}{errors}{tail}"), "While you were away (2 h 5 min): 40 new lines, 2 errors, 3 warnings");
        let (lead, errors, tail) = words(&recap(5, 0, 1, 59));
        assert_eq!(format!("{lead}{errors}{tail}"), "While you were away (59 min): 5 new lines, 1 warning");
    }

    #[test]
    fn the_line_takes_its_room_from_the_margins_not_from_the_program() {
        // 24 rows of 15 points and the two margins fill 384 points exactly: nothing to spare
        // below but the margin, so the rows above give the rest.
        let tight = Inset::new(10, 384.0, 24, 15.0);
        assert_eq!((tight.down, tight.up), (10.0, 8.0));
        assert_eq!(tight.up + tight.down, LINE);
        // The last row stays inside the pane, the first too.
        assert!(tight.line_y(23, 15.0) + 15.0 <= 384.0 - MARGIN);
        assert!(tight.line_y(0, 15.0) >= MARGIN);
        // Room to spare under the last row: nothing above moves.
        let loose = Inset::new(10, 394.0, 24, 15.0);
        assert_eq!((loose.down, loose.up), (LINE, 0.0));
        // The line sits between rows 9 and 10, touching both.
        assert_eq!(tight.y(15.0), tight.line_y(9, 15.0) + 15.0);
        assert_eq!(tight.y(15.0) + LINE, tight.line_y(10, 15.0));
    }

    #[test]
    fn a_click_lands_on_the_cell_it_is_drawn_over() {
        let inset = Inset::new(10, 384.0, 24, 15.0);
        for line in 0..24 {
            let drawn = inset.line_y(line, 15.0) + 7.0;
            let plain = inset.undo(drawn, 15.0);
            assert_eq!(((plain - PADDING) / 15.0).floor() as usize, line, "row {line}");
        }
        // On the line itself: the row under it.
        let on_line = inset.undo(inset.y(15.0) + 9.0, 15.0);
        assert_eq!(((on_line - PADDING) / 15.0).floor() as usize, 10);
    }

    #[test]
    fn the_first_new_line_is_found_again_as_output_moves_it_up() {
        let screen = |rows: &[&str]| rows.iter().map(|r| r.to_string()).collect::<Vec<_>>();
        let first = screen(&["ERROR schema is at 0041", "INFO exiting with code 1"]);
        let now = screen(&["$ logs -f", "INFO starting", "ERROR schema is at 0041", "INFO exiting with code 1", "$"]);
        assert_eq!(locate(&now, &first, Some(2)), Some(2));
        // Two more lines were printed: it's two rows up.
        let later = screen(&["ERROR schema is at 0041", "INFO exiting with code 1", "$ ls", "a b", "$"]);
        assert_eq!(locate(&later, &first, Some(2)), Some(0));
        // The same text further down isn't taken while it's still where it was, or above.
        let twice = screen(&["tick", "tick", "tick", "$"]);
        assert_eq!(locate(&twice, &screen(&["tick"]), Some(1)), Some(1));
        assert_eq!(locate(&twice, &screen(&["tick"]), None), Some(2));
        // Gone, and nothing to look for.
        assert_eq!(locate(&screen(&["$ clear", "$"]), &first, Some(2)), None);
        assert_eq!(locate(&now, &[], Some(2)), None);
        // A match cut by the screen's bottom edge still counts.
        let cut = screen(&["$ logs -f", "ERROR schema is at 0041"]);
        assert_eq!(locate(&cut, &first, None), Some(1));
    }

    #[test]
    fn long_new_lines_are_matched_by_the_rows_they_wrap_into() {
        let lines = vec!["abcdefghij".to_string(), String::new(), "xy".to_string()];
        assert_eq!(first_rows(&lines, 4, 3), ["abcd", "efgh", "ij"]);
        assert_eq!(first_rows(&lines[1..], 4, 3), ["", "xy"]);
        assert_eq!(rows_of("abcdefghij", 4), 3);
        assert_eq!(rows_of("", 4), 1);
        assert_eq!(rows_of("ab  ", 4), 1);
    }
}
