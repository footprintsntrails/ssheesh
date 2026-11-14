//! Finding text in a terminal: in its rows as a terminal keeps them, where a
//! long line runs over several rows, and a match may too.
//!
//! A port of 0.10's `src/lib/termFind.ts`, plus the rule for who gets
//! Ctrl+F (`takesFindKey` in `terminals.ts`). Rows come from the grid, with
//! its exact wrap marks, or from tmux's `capture-pane` (`rows_from_capture`).
//! Give it rows the presentation pass made safe (`present::Look::safe_rows`),
//! so a hidden secret or name can't be found.
//!
//! Positions here count characters, as the rows' cells do (0.10 counted
//! UTF-16 units, the same for everything but emoji and rarer symbols).

use regex::RegexBuilder;
use unicode_width::UnicodeWidthStr;

/// One row of a terminal. `wrapped`: it goes on from the row before.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FindRow {
    pub text: String,
    pub wrapped: bool,
}

impl FindRow {
    pub fn new(text: impl Into<String>, wrapped: bool) -> Self {
        FindRow { text: text.into(), wrapped }
    }
}

/// Where a match starts, and how many characters it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FindMatch {
    pub row: usize,
    pub start: usize,
    pub length: usize,
}

/// A part of a match on one row.
pub type FindPiece = FindMatch;

/// More than anyone goes through one by one.
pub const FIND_LIMIT: usize = 10_000;

/// The rows tmux printed (`capture-pane -p`). It says nothing about wrapping
/// there, but a row that fills the width goes on in the next one. Width is
/// counted in cells, so a row of wide characters counts too (0.10 counted
/// characters and missed those).
pub fn rows_from_capture(text: &str, cols: usize) -> Vec<FindRow> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| FindRow { text: line.to_string(), wrapped: i > 0 && lines[i - 1].width() >= cols })
        .collect()
}

/// Every match of `query`, case aside, in order; at most `FIND_LIMIT`.
pub fn find_rows(rows: &[FindRow], query: &str) -> Vec<FindMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let Ok(pattern) = RegexBuilder::new(&regex::escape(query)).case_insensitive(true).build() else {
        return Vec::new();
    };
    let mut matches = Vec::new();
    let mut first = 0;
    while first < rows.len() && matches.len() < FIND_LIMIT {
        let mut last = first;
        while rows.get(last + 1).is_some_and(|r| r.wrapped) {
            last += 1;
        }
        let line: String = rows[first..=last].iter().map(|r| r.text.as_str()).collect();
        let mut row = first;
        let mut row_start = 0;
        // Byte offsets to characters, counted as the matches go along.
        let (mut byte, mut chr) = (0, 0);
        for m in pattern.find_iter(&line) {
            if matches.len() >= FIND_LIMIT {
                break;
            }
            chr += line[byte..m.start()].chars().count();
            byte = m.start();
            while row < last && chr >= row_start + rows[row].text.chars().count() {
                row_start += rows[row].text.chars().count();
                row += 1;
            }
            matches.push(FindMatch { row, start: chr - row_start, length: m.as_str().chars().count() });
        }
        first = last + 1;
    }
    matches
}

/// A match, row by row.
pub fn match_pieces(rows: &[FindRow], m: FindMatch) -> Vec<FindPiece> {
    let mut pieces = Vec::new();
    let FindMatch { mut row, mut start, mut length } = m;
    while length > 0 {
        let Some(r) = rows.get(row) else { break };
        let here = length.min(r.text.chars().count().saturating_sub(start));
        if here > 0 {
            pieces.push(FindPiece { row, start, length: here });
        }
        length -= here;
        row += 1;
        start = 0;
        if !rows.get(row).is_some_and(|r| r.wrapped) {
            break;
        }
    }
    pieces
}

/// The match to start from when a search begins: the nearest at or above a
/// place in the history (`anchor`, rows up from the bottom), as what you're
/// after is mostly recent. Rows are counted from the bottom as new output
/// pushes everything up. `None` when there are no matches.
pub fn nearest_match(matches: &[FindMatch], total: usize, anchor: usize) -> Option<usize> {
    let from_bottom = |m: &FindMatch| total as i64 - 1 - m.row as i64;
    matches.iter().rposition(|m| from_bottom(m) >= anchor as i64).or(matches.len().checked_sub(1))
}

/// Programs that read Ctrl+F themselves, besides those drawing their own screen; ssh is another server's shell.
pub const KEEP_CTRL_F: [&str; 19] =
    ["emacs", "hx", "helix", "micro", "joe", "ne", "ranger", "nnn", "lf", "yazi", "vifm", "tig", "mutt", "neomutt", "weechat", "irssi", "ssh", "tmux", "screen"];

/// Programs that draw their own screen, besides the AI agents (the app knows those).
pub const FULL_SCREEN: [&str; 13] = ["vim", "nvim", "vi", "nano", "less", "man", "htop", "top", "btop", "mc", "lazygit", "k9s", "watch"];

/// Whether Ctrl+F is for the find bar. A program with a screen of its own
/// that reads this key keeps it: vim, less and the like, known by name
/// (`command`, the foreground program) under tmux, and by the terminal's
/// second screen without it. Agents have no use for it, and do have a lot
/// of output to look through. `finds_in_tmux`: the history is tmux's.
pub fn takes_find_key(command: Option<&str>, finds_in_tmux: bool, alt_screen: bool) -> bool {
    if let Some(c) = command.map(str::to_lowercase) {
        if FULL_SCREEN.contains(&c.as_str()) || KEEP_CTRL_F.contains(&c.as_str()) {
            return false;
        }
    }
    finds_in_tmux || !alt_screen
}
