//! "While you were away": when a durable session comes back after a dropped
//! connection, compare the server's scrollback with what was on screen when
//! the connection went, and keep what's new.
//!
//! A port of 0.10's `src/lib/away.ts`. One difference: 0.10 dotted the
//! server's history in streamer mode before comparing, because its screen
//! held dotted text. In 1.0 the grid holds the text as it came (dots are put
//! in only when it's shown), so raw anchors are compared with raw history,
//! and the card shows its lines through `Recap::shown_lines`.

use crate::highlight::count_problems;
use crate::present::{Look, RowText};
use regex::Regex;
use std::sync::LazyLock;

/// Rows of a terminal, oldest first: the scrollback, then the screen. The
/// engine gives them from its grid; `RowList` gives them from a slice.
pub trait RowSource {
    /// Columns in a row.
    fn cols(&self) -> usize;
    /// Row `y`, counted from the oldest row kept, or `None` past the last.
    fn row(&self, y: usize) -> Option<RowText>;
}

/// Rows already at hand.
#[derive(Debug, Clone, Copy)]
pub struct RowList<'a> {
    pub cols: usize,
    pub rows: &'a [RowText],
}

impl RowSource for RowList<'_> {
    fn cols(&self) -> usize {
        self.cols
    }

    fn row(&self, y: usize) -> Option<RowText> {
        self.rows.get(y).cloned()
    }
}

/// How many finished lines above the cursor mark where the screen was.
pub const ANCHOR_LINES: usize = 3;

/// ssh's own goodbye, which isn't in the server's scrollback.
static SSH_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(Connection to .* closed|client_loop: |packet_write_wait|Timeout, server .* not responding|Connection reset by|Broken pipe)").expect("pattern")
});

/// The last few finished lines above the cursor (`cursor_row`, counted as
/// `RowSource` counts): they mark where the screen was. Rows that wrapped are
/// joined back into one line, the way tmux reports its scrollback.
pub fn anchor_lines(src: &impl RowSource, cursor_row: usize, count: usize) -> Vec<String> {
    let full = src.cols().saturating_sub(1);
    let trimmed = |y: usize| src.row(y).map(|r| r.trimmed().to_string()).unwrap_or_default();
    // A row continues the one above when the terminal wrapped it, or when the
    // one above is full to the edge (tmux redraws long lines as separate full rows).
    let continues = |y: usize| y > 0 && (src.row(y).is_some_and(|r| r.wrapped) || src.row(y - 1).is_some_and(|r| r.used_cols() as usize >= full));
    let mut out: Vec<String> = Vec::new();
    let mut y = cursor_row as isize - 1;
    while y >= 0 && out.len() < count {
        let mut text = trimmed(y as usize);
        let mut top = y as usize;
        while continues(top) {
            top -= 1;
            text = src.row(top).map(|r| r.text).unwrap_or_default() + &text;
        }
        y = top as isize - 1;
        if SSH_NOISE.is_match(&text) {
            continue;
        }
        if text.trim().is_empty() {
            if !out.is_empty() {
                break;
            }
            continue;
        }
        out.insert(0, text);
    }
    out
}

/// Lines after the last place the anchor shows up, or `None` if it doesn't.
pub fn lines_after<S: AsRef<str>, T: AsRef<str>>(history: &[S], anchor: &[T]) -> Option<Vec<String>> {
    if anchor.is_empty() || anchor.len() > history.len() {
        return None;
    }
    let want: Vec<&str> = anchor.iter().map(|a| a.as_ref().trim_end()).collect();
    for i in (0..=history.len() - want.len()).rev() {
        if want.iter().enumerate().all(|(k, w)| history[i + k].as_ref().trim_end() == *w) {
            let mut rest: Vec<String> = history[i + want.len()..].iter().map(|s| s.as_ref().to_string()).collect();
            while rest.last().is_some_and(|l| l.trim().is_empty()) {
                rest.pop();
            }
            return Some(rest);
        }
    }
    None
}

/// The line the cursor was on when the connection dropped, usually the prompt.
pub fn cursor_line(src: &impl RowSource, cursor_row: usize) -> String {
    src.row(cursor_row).map(|r| r.trimmed().to_string()).unwrap_or_default()
}

/// What a durable session printed while the connection was down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recap {
    /// When the connection went and when the lines were read, in ms since the epoch.
    pub since: u64,
    pub until: u64,
    /// As they came, never to be shown as they are: the card shows `shown_lines`.
    pub lines: Vec<String>,
    pub errors: usize,
    pub warnings: usize,
}

impl Recap {
    /// The lines as the card may show them, one for each of `lines`: names
    /// dotted while streaming, secrets while hidden. They're made safe
    /// together, as 0.10 masked the card's lines joined, so a private key
    /// printed while you were away is known by its BEGIN line and its body
    /// dotted. One line at a time it wouldn't be.
    pub fn shown_lines(&self, look: &Look) -> Vec<String> {
        look.safe_lines(&self.lines, false)
    }
}

/// The recap from the server's scrollback (`history_text`, as `session::history`
/// reads it), the anchor and cursor line taken when the connection went,
/// and the times. `None` when nothing new came, or the anchor is gone.
pub fn build_recap(history_text: &str, anchor: &[String], current: &str, since: u64, until: u64) -> Option<Recap> {
    let cleaned = history_text.replace('\r', "");
    let history: Vec<&str> = cleaned.split('\n').collect();
    let mut lines = lines_after(&history, anchor)?;
    // That line was already on screen when the connection dropped.
    if !lines.is_empty() && !current.trim().is_empty() && lines[0].trim_end() == current.trim_end() {
        lines.remove(0);
    }
    if lines.is_empty() {
        return None;
    }
    let problems = count_problems(&lines);
    Some(Recap { since, until, lines, errors: problems.errors, warnings: problems.warnings })
}
