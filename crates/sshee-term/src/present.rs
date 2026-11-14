//! The presentation pass: what the screen may show of the rows the engine
//! holds. It runs once per group of rows a long line wraps over, before
//! anything is drawn: streamer mode's dots first, then secrets, then
//! highlights, then links, then find matches.
//!
//! A hidden secret or a masked name never leaves this pass: the rows it
//! hands out carry dots in its place, both in the text the screen draws and
//! in the text a copy, a find or a screen reader takes (a secret you clicked
//! to show is shown, as in 0.10). Highlights, links and find work on that
//! same safe text, so none of them can learn or show anything about what's
//! hidden either. In 0.10 the secret was drawn
//! and a cover put over it in the same frame (`markSecrets` in
//! `terminals.ts`), and streamer mode rewrote the output stream
//! (`output.ts`); here both are a pure function of the row text, so
//! switching streamer mode applies at once to what's already on screen.
//!
//! What's hidden is looked for generously, since hiding too much is the
//! safe way to be wrong:
//! - Secrets are looked for in the text as it came and as streamer mode
//!   left it, so a server's name inside a token or a key word (a server
//!   called "api" in `API_KEY=...`) can't stop the token being found.
//! - A group joins rows that only may be one line: tmux leaves no wrap
//!   marks, so a row full to the last column is taken to go on. Each line
//!   the wrap marks show is read on its own as well, and what either reading
//!   finds is hidden. An address at the start of the row after a full one is
//!   dotted, as 0.10 dotted it, where a new line came between them.
//! - A line the screen's top or bottom edge cuts is read with the rest of it
//!   (`Edges`), so a token whose start scrolled off is still known.
//!
//! Highlights, links and find marks only show things, and go line by line
//! as the wrap marks say, as in 0.10: a full row doesn't join the next.
//!
//! What stays as before: the unmasked text is still in the engine's memory,
//! as it was in xterm's buffer (and is in tmux's on the server).
//!
//! The engine's rows come in as `RowText`, which says nothing of colors: the
//! screen layer takes those from its own cells, by column, and its text from
//! the `PresentedRow`.

use crate::find::{find_rows, match_pieces, FindRow};
use crate::highlight::{Highlights, Paint};
use crate::links::{find_links, hidden_target_hint, hyperlink, link_click, LinkClick, LinkKind, OPEN_HINT};
use crate::privacy::Names;
use crate::secrets::{dot_key_line, find_secrets, key_lines, key_rows, secrets_hidden, unfinished_secret, KeyBlock};
use std::cell::Cell;
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::ops::Range;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

/// A link a program marked with OSC 8 over some columns of a row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hyperlink {
    pub cols: Range<u16>,
    pub uri: String,
}

/// One row as the engine holds it, reduced to what this pass reads.
///
/// From alacritty's grid (the engine track builds it): every column left to
/// right, a cell flagged `WIDE_CHAR_SPACER` or `LEADING_WIDE_CHAR_SPACER`
/// skipped, otherwise its `c` (a space when empty) and then its zero-width
/// characters, each pushed with the cell's column. `wrapped` is the row
/// above ending in a cell flagged `WRAPLINE`. `hyperlinks` are the runs of
/// cells with the same `hyperlink()`. Like 0.10's `rowText`, but with every
/// column in it, trailing blanks too.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RowText {
    pub text: String,
    /// The column of each character of `text`, in order.
    pub cols: Vec<u16>,
    /// The row goes on from the row above (a long line wrapped).
    pub wrapped: bool,
    pub hyperlinks: Vec<Hyperlink>,
}

impl RowText {
    /// A row from plain text, each character in the columns its width gives
    /// it, a mark joining the one before in that one's cell. For tests, and
    /// for rows that come as text (tmux's `capture-pane`).
    pub fn plain(text: &str) -> Self {
        let mut cols = Vec::with_capacity(text.len());
        let mut col: u16 = 0;
        for c in text.chars() {
            match (c.width(), cols.last()) {
                (Some(0), Some(&prev)) => cols.push(prev),
                (w, _) => {
                    cols.push(col);
                    col = col.saturating_add(w.unwrap_or(1).max(1) as u16);
                }
            }
        }
        RowText { text: text.to_string(), cols, wrapped: false, hyperlinks: Vec::new() }
    }

    /// The same row, going on from the one above.
    pub fn wrapping(mut self) -> Self {
        self.wrapped = true;
        self
    }

    /// The row without its trailing blanks.
    pub fn trimmed(&self) -> &str {
        self.text.trim_end()
    }

    /// Where each character's cell starts and ends: `(col, end)`. A
    /// character's cell ends where its width says, or where the next one's
    /// starts if that's sooner; a mark joining a character shares its cell.
    /// Columns missing from a malformed row carry on after the last.
    fn cells(&self) -> Vec<(u16, u16)> {
        let chars: Vec<char> = self.text.chars().collect();
        let mut cols: Vec<u16> = Vec::with_capacity(chars.len());
        for (i, c) in chars.iter().enumerate() {
            let col = match self.cols.get(i) {
                Some(&col) => col,
                None => cols.last().map_or(0, |&p| p.saturating_add(c.width().unwrap_or(1).max(1) as u16)),
            };
            cols.push(col);
        }
        let mut out: Vec<(u16, u16)> = Vec::with_capacity(chars.len());
        for i in 0..chars.len() {
            if i > 0 && cols[i] == cols[i - 1] {
                out.push(out[i - 1]);
                continue;
            }
            let mut end = cols[i].saturating_add(chars[i].width().unwrap_or(1).max(1) as u16);
            if let Some(&next) = cols[i + 1..].iter().find(|&&c| c > cols[i]) {
                end = end.min(next);
            }
            out.push((cols[i], end));
        }
        out
    }

    /// The column just past the last character that isn't blank (0 for a blank row).
    pub fn used_cols(&self) -> u16 {
        let cells = self.cells();
        self.text.chars().zip(cells).filter(|(c, _)| !c.is_whitespace()).map(|(_, (_, end))| end).last().unwrap_or(0)
    }
}

/// What a part of a presented row is, for the screen to draw it so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Streamer mode hid a name, a user or an address: the text there is dots.
    Masked,
    /// A covered secret: the text there is dots, drawn as a pill tinted with
    /// the accent. `revealable`: a click shows it (never while streaming, and
    /// never a private key's line).
    Secret { revealable: bool },
    /// Highlighted: an error, a warning, an address, or a rule of yours.
    Paint(Paint),
    /// A link, faintly underlined; the number is its place in `PresentedRow::links`.
    Link(usize),
    /// A find match; `current` for the one the find bar is on.
    Found { current: bool },
}

/// A role over some columns of a row: `cols` is `[start, end)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RowSpan {
    pub cols: Range<u16>,
    pub role: Role,
}

/// A link on screen: what a click opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Link {
    pub kind: LinkKind,
    /// The URL, or the path without its line numbers.
    pub target: String,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    /// A program marked it with OSC 8, rather than its text being a link.
    pub from_program: bool,
    /// Its text on screen isn't where it goes: a program's link over other
    /// words ("docs", or "github.com/acme/app" over another site). Hovering
    /// it names `target`, and a web page opens only once you say yes. In 0.10
    /// xterm asked before opening any program's link.
    pub disguised: bool,
}

impl Link {
    /// What hovering it says.
    pub fn hint(&self) -> String {
        if self.disguised {
            hidden_target_hint(&self.target)
        } else {
            OPEN_HINT.to_string()
        }
    }

    /// What a click on it does: `links::link_click`, but asking first before
    /// a web page its text doesn't show. A path its text doesn't show opens
    /// in the Files pane as it is, where you see where you are.
    pub fn click(&self, open_key: bool, clicks: u32, has_selection: bool) -> LinkClick {
        let click = link_click(open_key, clicks, has_selection);
        if self.disguised && self.kind == LinkKind::Url {
            click.asking_first()
        } else {
            click
        }
    }
}

/// A row as the screen may show it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PresentedRow {
    /// What the row shows, and what copying, finding and reading it aloud
    /// get: every hidden character is a `•`. A mark that joined a hidden
    /// character is left out (the dot stands for both).
    pub text: String,
    /// The column of each character of `text`.
    pub cols: Vec<u16>,
    /// What to draw differently, by columns, in order of where they start.
    /// Spans may overlap (a link inside a highlight); a secret covers all.
    pub spans: Vec<RowSpan>,
    /// The links that start on or run through this row.
    pub links: Vec<Link>,
}

impl PresentedRow {
    /// The characters in some columns, for copying a selection.
    pub fn text_in(&self, cols: Range<u16>) -> String {
        self.text.chars().zip(&self.cols).filter(|(_, c)| cols.contains(c)).map(|(ch, _)| ch).collect()
    }

    /// The role at a column of this kind, for hit tests (a click on a link or a secret).
    pub fn span_at(&self, col: u16, wanted: impl Fn(&Role) -> bool) -> Option<&RowSpan> {
        self.spans.iter().find(|s| s.cols.contains(&col) && wanted(&s.role))
    }
}

/// What a covered secret says when hovered.
pub const SECRET_HINT: &str = "Hidden secret. Click to show it";
/// The same while streaming, when a click shows nothing.
pub const STREAMING_HINT: &str = "Hidden while streaming";

/// The settings the pass follows. Made again when one of them changes;
/// `names` and `highlights` are shared by every terminal. Whether a program
/// that draws its own screen wrote a row belongs to the row (`Screen::own_screen`).
#[derive(Debug, Clone, Default)]
pub struct Look {
    pub streaming: bool,
    pub hide_secrets: bool,
    pub names: Arc<Names>,
    pub highlights: Arc<Highlights>,
}

impl Look {
    /// Secrets are covered by your choice, and always while streaming.
    pub fn secrets_hidden(&self) -> bool {
        secrets_hidden(self.hide_secrets, self.streaming)
    }

    fn same(&self, other: &Look) -> bool {
        self.streaming == other.streaming
            && self.hide_secrets == other.hide_secrets
            && Arc::ptr_eq(&self.names, &other.names)
            && Arc::ptr_eq(&self.highlights, &other.highlights)
    }

    /// One whole text from the terminal shown somewhere else (a tab's
    /// "waiting" note, a tooltip, a notification), as the screen would show
    /// it: names dotted while streaming, secrets while hidden. Its lines are
    /// read together, so a private key's body is known by the BEGIN line
    /// above it. A line cut out of a longer text isn't, as its key may have
    /// begun in a line before it: lines that belong together go to `safe_lines`.
    pub fn safe_text(&self, text: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        self.safe_lines(&lines, false).join("\n")
    }

    /// Lines of the terminal shown somewhere else, made safe together: each
    /// line on its own, but a key's body known by the BEGIN line in an
    /// earlier one, or by `key_open_above` (a key began above the first).
    pub fn safe_lines<S: AsRef<str>>(&self, lines: &[S], key_open_above: bool) -> Vec<String> {
        let mut block = KeyBlock::new(key_open_above);
        lines.iter().map(|line| self.safe_line(line.as_ref(), &mut block)).collect()
    }

    /// What copying goes through (a `select::CopyPresenter`): each line it's
    /// given made safe as `safe_lines` would, with the key block carried from
    /// one line to the next, so a key's body copies as the dots the screen
    /// shows. The lines must come in order, top to bottom, each once, as
    /// `select::copy_text` reads them. `key_open_above`: a key began above
    /// the first line copied (`secrets::key_open_after` over the rows above).
    pub fn copy_presenter(&self, key_open_above: bool) -> impl Fn(&str) -> String + '_ {
        let block = Cell::new(KeyBlock::new(key_open_above));
        move |line: &str| {
            let mut b = block.get();
            let safe = self.safe_line(line, &mut b);
            block.set(b);
            safe
        }
    }

    /// One line, with the key block it's in moved on past it.
    fn safe_line(&self, line: &str, block: &mut KeyBlock) -> String {
        let key = block.line(line);
        if !self.secrets_hidden() {
            return line.to_string();
        }
        if key {
            return dot_key_line(line);
        }
        let (hide, _) = hide_in(self, line, &[], &HashSet::new());
        dotted(line, &hide)
    }

    /// Rows for a find over the whole history (the grid's, or tmux's), made
    /// safe the way the screen's are: a long line is masked as one, so a
    /// name cut by the wrap is hidden too, and each row on its own as well
    /// (tmux's rows say a line went on only because one was full to the
    /// edge), then split back into its rows.
    pub fn safe_rows(&self, rows: &[FindRow]) -> Vec<FindRow> {
        let hidden = self.secrets_hidden();
        let keys: Vec<bool> = if hidden {
            // A row is a key's by its line as the rows join, or by itself: the
            // join may be a guess, and either reading can be the one that knows.
            let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
            let pairs: Vec<(&str, bool)> = rows.iter().map(|r| (r.text.as_str(), r.wrapped)).collect();
            key_rows(&pairs, false).into_iter().zip(key_lines(&texts, false)).map(|(a, b)| a || b).collect()
        } else {
            vec![false; rows.len()]
        };
        let mut out = Vec::with_capacity(rows.len());
        let mut first = 0;
        while first < rows.len() {
            let mut last = first;
            while rows.get(last + 1).is_some_and(|r| r.wrapped) {
                last += 1;
            }
            let group = &rows[first..=last];
            let joined: String = group.iter().map(|r| r.text.as_str()).collect();
            let mut pieces = Vec::with_capacity(group.len());
            let mut at = 0;
            for r in group {
                let n = r.text.chars().count();
                pieces.push(at..at + n);
                at += n;
            }
            let safe: Vec<char> = if hidden {
                let (mut hide, _) = hide_in(self, &joined, &pieces, &HashSet::new());
                // A key's rows are covered whole, every character but spaces.
                for (piece, _) in pieces.iter().zip(&keys[first..=last]).filter(|(_, key)| **key) {
                    for (h, c) in hide[piece.clone()].iter_mut().zip(joined.chars().skip(piece.start)) {
                        if !c.is_whitespace() {
                            *h = Hide::Secret;
                        }
                    }
                }
                dotted(&joined, &hide).chars().collect()
            } else {
                joined.chars().collect()
            };
            for (r, piece) in group.iter().zip(pieces) {
                out.push(FindRow { text: safe[piece].iter().collect(), wrapped: r.wrapped });
            }
            first = last + 1;
        }
        out
    }
}

/// The find bar's part in a pass: what it looks for, and which match it's on
/// (the screen row the match starts on, and its first character there).
#[derive(Debug, Clone, Copy)]
pub struct FindMarks<'a> {
    pub query: &'a str,
    pub current: Option<(usize, usize)>,
}

/// As many rows beyond an edge of the screen as the pass reads of a line the
/// edge cuts. A line longer than that has its rows on screen hidden whole.
pub const CONTEXT_ROWS: usize = 1000;

/// The rest of the lines the screen's edges cut through. Without it, what's
/// hidden in a long line could show in the part of it on screen: a token
/// whose start scrolled above the top, a key in the middle of a long JSON
/// line, a server name cut at the bottom while scrolled up.
#[derive(Debug, Clone, Copy, Default)]
pub struct Edges<'a> {
    /// The rows above the first on screen that its line runs over, oldest
    /// first: taken while a row goes on from the one above it (marked
    /// `wrapped`, or that one is full to the last column, as tmux draws a
    /// long line). At most `CONTEXT_ROWS`; the pass reads no more.
    pub above: &'a [RowText],
    /// The rows below the last on screen that its line runs on into, when
    /// the screen is scrolled up. At most `CONTEXT_ROWS`.
    pub below: &'a [RowText],
    /// The line at the top goes on above `above`: it's longer than
    /// `CONTEXT_ROWS`, or its rows couldn't be read. Its rows on screen are
    /// then hidden whole, as they are when the first row given is `wrapped`.
    pub more_above: bool,
    /// The same at the bottom: the line goes on below `below`.
    pub more_below: bool,
}

/// The rows on screen, top to bottom, and what the pass needs around them.
#[derive(Debug, Clone, Copy)]
pub struct Screen<'a> {
    pub rows: &'a [RowText],
    /// Columns in a row.
    pub cols: u16,
    /// The rest of the lines the top and bottom edges cut.
    pub edges: Edges<'a>,
    /// A private key began above the first row given (the first of
    /// `edges.above`, or of `rows`) and hasn't ended. Worth working out
    /// (`secrets::key_open_after` over up to 80 rows above) only when a row
    /// given `secrets::looks_like_key_body` or `secrets::ends_key`.
    pub key_open_above: bool,
    /// For each row of `rows`, whether a program that draws its own screen
    /// (an agent, vim) was in the foreground when the row was written:
    /// nothing on it is highlighted, as such a program redraws lines in
    /// pieces. The engine marks a row as it writes it, as 0.10 decided for
    /// each piece of output as it came, so the scrollback above an agent
    /// keeps its colours and what the agent wrote never gets any. Rows past
    /// the end of this count as not.
    pub own_screen: &'a [bool],
    pub find: Option<FindMarks<'a>>,
}

impl<'a> Screen<'a> {
    /// The rows on screen and nothing more: no lines cut at the edges, no
    /// key above, no full-screen program, no find.
    pub fn new(rows: &'a [RowText], cols: u16) -> Self {
        Screen { rows, cols, edges: Edges::default(), key_open_above: false, own_screen: &[], find: None }
    }
}

/// A row that may go on in the next one: the next is marked as wrapped, or
/// this one is full to the last column (tmux draws a long line as separate
/// full rows, so the terminal never learns it wrapped).
fn runs_on(rows: &[&RowText], y: usize, cols: u16) -> bool {
    rows.get(y + 1).is_some_and(|next| next.wrapped) || rows[y].used_cols() >= cols
}

/// The groups of rows: `(first, last, is a key's line)`. A key's line
/// stands alone; other rows join while they run on.
fn groups(rows: &[&RowText], cols: u16, keys: &[bool]) -> Vec<(usize, usize, bool)> {
    let mut out = Vec::new();
    let mut r = 0;
    while r < rows.len() {
        let mut end = r;
        if !keys[r] {
            while end + 1 < rows.len() && !keys[end + 1] && runs_on(rows, end, cols) {
                end += 1;
            }
        }
        out.push((r, end, keys[r]));
        r = end + 1;
    }
    out
}

/// The rows the pass reads for a screen: the rows above it that its first
/// line runs over, its own rows, and the rows below that its last runs into.
struct Given<'a> {
    rows: Vec<&'a RowText>,
    /// Where the screen's rows are in `rows`.
    shown: Range<usize>,
    /// The line at the top, or at the bottom, goes on past the rows given.
    more_above: bool,
    more_below: bool,
}

/// Rows `first..=last` of those given, read as one.
#[derive(Debug, Clone, Copy)]
struct Group {
    first: usize,
    last: usize,
    /// A private key's line: every character but spaces hidden.
    key_line: bool,
    /// Cut by an edge with the rest of its line out of reach: hidden whole.
    cut: bool,
}

impl<'a> Given<'a> {
    fn new(screen: &Screen<'a>) -> Self {
        let e = screen.edges;
        let above = &e.above[e.above.len().saturating_sub(CONTEXT_ROWS)..];
        let below = &e.below[..e.below.len().min(CONTEXT_ROWS)];
        let rows: Vec<&RowText> = above.iter().chain(screen.rows).chain(below).collect();
        Given {
            shown: above.len()..above.len() + screen.rows.len(),
            more_above: e.more_above || above.len() < e.above.len() || rows.first().is_some_and(|r| r.wrapped),
            more_below: e.more_below || below.len() < e.below.len(),
            rows,
        }
    }

    /// The groups with a row on screen. `hidden`: secrets are hidden, and
    /// with them a group the edges cut short.
    fn groups(&self, cols: u16, keys: &[bool], hidden: bool) -> Vec<Group> {
        groups(&self.rows, cols, keys)
            .into_iter()
            .filter(|&(first, last, _)| first < self.shown.end && last >= self.shown.start)
            .map(|(first, last, key_line)| {
                let cut = hidden && ((first == 0 && self.more_above) || (last + 1 == self.rows.len() && self.more_below));
                Group { first, last, key_line, cut }
            })
            .collect()
    }

    /// Whether a full-screen program wrote row `i` of those given.
    fn own_screen(&self, screen: &Screen, i: usize) -> bool {
        self.shown.contains(&i) && screen.own_screen.get(i - self.shown.start).copied().unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FindKey {
    query: String,
    /// The current match, if it's in this group: its row in the group and its start there.
    current: Option<(usize, usize)>,
}

/// How many screens of presented rows the cache keeps before it starts over (and never fewer
/// than `CACHE_ROWS_MIN` rows). It holds rows, not groups: a group can be a long line's thousand
/// rows (`CONTEXT_ROWS` above the screen and the screen's own), of which only those on screen are
/// kept. 0.10's `secretIn` kept 2,000 texts; counted in groups here, a line that kept growing
/// would have filled it with a thousand rows a snapshot.
const CACHE_SCREENS: usize = 8;
const CACHE_ROWS_MIN: usize = 512;

/// One terminal's presentation pass, with what it has already worked out.
/// A group of rows with the same text (and find state) as before comes back
/// as the same `Arc`s, so the screen can keep what it built from them; a row
/// that scrolled keeps its shaping.
///
/// The cache keeps a group's 128-bit fingerprint (two hashes with keys
/// picked at random for this run), never its text: what it remembers of a
/// screen is only what it was allowed to show, and of a group only its rows
/// that were on screen.
#[derive(Default)]
pub struct Presenter {
    look: Look,
    /// Secrets you clicked to show, by their text. Ignored while streaming.
    revealed: HashSet<String>,
    cache: HashMap<(u64, u64), Vec<Arc<PresentedRow>>>,
    /// Rows the cache holds, all groups together.
    cached_rows: usize,
    hashers: (RandomState, RandomState),
}

impl Presenter {
    pub fn new(look: Look) -> Self {
        Presenter { look, ..Default::default() }
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    /// New settings. What was worked out under the old ones goes.
    pub fn set_look(&mut self, look: Look) {
        if !self.look.same(&look) {
            self.look = look;
            self.forget();
        }
    }

    /// Drops everything worked out, and the secrets shown: for a terminal
    /// starting over (a new shell), or a secret shown by mistake.
    pub fn hide_revealed(&mut self) {
        self.revealed.clear();
        self.forget();
    }

    fn forget(&mut self) {
        self.cache.clear();
        self.cached_rows = 0;
    }

    /// Rows the cache holds now (for tests and the meter).
    pub fn cached_rows(&self) -> usize {
        self.cached_rows
    }

    /// The rows on screen as they may be shown, one per row of `screen.rows`.
    pub fn present(&mut self, screen: &Screen) -> Vec<Arc<PresentedRow>> {
        let given = Given::new(screen);
        let keys = self.key_rows(&given.rows, screen.key_open_above);
        let mut out = Vec::with_capacity(screen.rows.len());
        let limit = (CACHE_SCREENS * screen.rows.len()).max(CACHE_ROWS_MIN);
        for g in given.groups(screen.cols, &keys, self.look.secrets_hidden()) {
            let find = screen.find.filter(|f| !f.query.is_empty()).map(|f| FindKey {
                query: f.query.to_string(),
                current: f
                    .current
                    .map(|(row, start)| (given.shown.start + row, start))
                    .filter(|(row, _)| (g.first..=g.last).contains(row))
                    .map(|(row, start)| (row - g.first, start)),
            });
            let plain: Vec<bool> = (g.first..=g.last).map(|i| given.own_screen(screen, i)).collect();
            // The group's rows on screen, as indices into the group.
            let on_screen = g.first.max(given.shown.start) - g.first..g.last.min(given.shown.end - 1) + 1 - g.first;
            out.extend(self.group(&given.rows[g.first..=g.last], g, &plain, find, on_screen, limit));
        }
        out
    }

    fn key_rows(&self, rows: &[&RowText], key_open_above: bool) -> Vec<bool> {
        if self.look.secrets_hidden() {
            key_rows(&rows.iter().map(|r| (r.text.as_str(), r.wrapped)).collect::<Vec<_>>(), key_open_above)
        } else {
            vec![false; rows.len()]
        }
    }

    /// The rows `on_screen` of a group (indices into `rows`), presented. The cache keeps only
    /// those, under all of the group's text and where the screen cuts it; it starts over once it
    /// would hold more than `limit` rows.
    fn group(&mut self, rows: &[&RowText], g: Group, plain: &[bool], find: Option<FindKey>, on_screen: Range<usize>, limit: usize) -> Vec<Arc<PresentedRow>> {
        let what = (rows, g.key_line, g.cut, plain, &find, &on_screen);
        let key = (self.hashers.0.hash_one(what), self.hashers.1.hash_one(what));
        if let Some(out) = self.cache.get(&key) {
            return out.clone();
        }
        let prepared = Prepared::new(rows, &self.look, g.key_line || g.cut, &self.revealed);
        let mut all = prepared.present(rows, &self.look, plain, find.as_ref());
        let out: Vec<Arc<PresentedRow>> = all.drain(on_screen).map(Arc::new).collect();
        if self.cached_rows + out.len() > limit {
            self.forget();
        }
        self.cached_rows += out.len();
        self.cache.insert(key, out.clone());
        out
    }

    /// A click at a column of a screen row: if a covered secret is there and
    /// may be shown, it's shown from now on (wherever it appears) and `true`
    /// comes back. Never while streaming, and never a private key's line or
    /// a line hidden whole because an edge cut it.
    pub fn reveal(&mut self, screen: &Screen, row: usize, col: u16) -> bool {
        if self.look.streaming || !self.look.secrets_hidden() {
            return false;
        }
        let given = Given::new(screen);
        let target = given.shown.start + row;
        if !given.shown.contains(&target) {
            return false;
        }
        let keys = self.key_rows(&given.rows, screen.key_open_above);
        let Some(g) = given.groups(screen.cols, &keys, true).into_iter().find(|g| (g.first..=g.last).contains(&target)) else {
            return false;
        };
        if g.key_line || g.cut {
            return false;
        }
        let group = &given.rows[g.first..=g.last];
        let prepared = Prepared::new(group, &self.look, false, &self.revealed);
        let hit = prepared.secrets.iter().find(|(chars, _)| {
            chars.clone().any(|i| prepared.at[i].row == target - g.first && (prepared.at[i].col..prepared.at[i].end).contains(&col))
        });
        let Some((_, text)) = hit else { return false };
        self.revealed.insert(text.clone());
        self.forget();
        true
    }
}

/// Where a character of a group's joined text sits.
#[derive(Debug, Clone, Copy)]
struct At {
    /// The row in the group.
    row: usize,
    col: u16,
    end: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hide {
    No,
    Masked,
    Secret,
}

/// For each byte offset that starts a character, that character's index (and the count at the end).
fn char_index(text: &str) -> Vec<usize> {
    let mut map = vec![0; text.len() + 1];
    let mut n = 0;
    for (i, (b, _)) in text.char_indices().enumerate() {
        map[b] = i;
        n = i + 1;
    }
    map[text.len()] = n;
    map
}

/// `text` with every character `hide` hides turned into a `•`.
fn dotted(text: &str, hide: &[Hide]) -> String {
    text.chars().zip(hide).map(|(c, h)| if *h == Hide::No { c } else { '•' }).collect()
}

/// What streamer mode and the secret finder hide in a text, character by
/// character, and the secrets found (their characters, and their text, kept
/// only for `reveal`). The text is read whole, and each of `pieces`
/// (character ranges) on its own as well: what any reading finds is hidden.
/// A secret you showed (`revealed`) stays shown, but never while streaming.
fn hide_in(look: &Look, text: &str, pieces: &[Range<usize>], revealed: &HashSet<String>) -> (Vec<Hide>, Vec<(Range<usize>, String)>) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let parts: Vec<Range<usize>> = pieces.iter().filter(|p| !p.is_empty() && **p != (0..n)).cloned().collect();
    let part_of = |source: &[char], p: &Range<usize>| -> String { source[p.clone()].iter().collect() };
    let mut hide = vec![Hide::No; n];
    // 1. Streamer mode. Every character comes back as it was or as a dot; if
    // that ever failed to hold, everything is hidden rather than guessed at.
    if look.streaming {
        let mut mark = |offset: usize, original: &[char], masked: String| {
            if masked.chars().count() == original.len() {
                for (k, (a, b)) in original.iter().zip(masked.chars()).enumerate() {
                    if *a != b {
                        hide[offset + k] = Hide::Masked;
                    }
                }
            } else {
                hide[offset..offset + original.len()].fill(Hide::Masked);
            }
        };
        mark(0, &chars, look.names.mask_text(text));
        for p in &parts {
            mark(p.start, &chars[p.clone()], look.names.mask_text(&part_of(&chars, p)));
        }
    }
    // 2. Secrets, in the text as it came (a name dotted inside a token or a
    // key word mustn't hide the token) and as streamer mode left it (a URL
    // whose user it dotted still has its password found).
    let mut secrets: Vec<(Range<usize>, String)> = Vec::new();
    if look.secrets_hidden() {
        let masked: Vec<char> = chars.iter().zip(&hide).map(|(c, h)| if *h == Hide::No { *c } else { '•' }).collect();
        let dotted_some = masked != chars;
        let mut searched: Vec<(usize, String)> = vec![(0, text.to_string())];
        if dotted_some {
            searched.push((0, masked.iter().collect()));
        }
        for p in &parts {
            searched.push((p.start, part_of(&chars, p)));
            if dotted_some {
                searched.push((p.start, part_of(&masked, p)));
            }
        }
        for (offset, t) in &searched {
            let index = char_index(t);
            let found = find_secrets(t);
            // A secret still being printed: hidden from its first certain characters on, and
            // not one a click shows (`secrets::unfinished_secret`). Unless what ends the text
            // is a whole secret already, which the rules below hide (or show, if you did).
            if let Some(s) = unfinished_secret(t).filter(|u| !found.iter().any(|f| f.start < u.end && u.start < f.end)) {
                hide[offset + index[s.start]..offset + index[s.end]].fill(Hide::Secret);
            }
            for s in found {
                let found = &t[s.clone()];
                // One you showed hides again while streaming: then nothing shows.
                if !look.streaming && revealed.contains(found) {
                    continue;
                }
                let range = offset + index[s.start]..offset + index[s.end];
                hide[range.clone()].fill(Hide::Secret);
                if !secrets.iter().any(|(r, _)| *r == range) {
                    secrets.push((range, found.to_string()));
                }
            }
        }
    }
    (hide, secrets)
}

/// Whether a link's text on screen says where it goes: a web link's text is
/// its URL, perhaps without `http://` or `https://` or a `/` at the end; a
/// path's is the path, or its `file://` URL.
fn shows_target(text: &str, uri: &str, kind: LinkKind, target: &str) -> bool {
    if text == uri || text == target {
        return true;
    }
    if kind != LinkKind::Url || text.contains("://") {
        return false;
    }
    let bare = |s: &str| -> String {
        let scheme = if s.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")) {
            8
        } else if s.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("http://")) {
            7
        } else {
            0
        };
        s[scheme..].trim_end_matches('/').to_string()
    };
    bare(uri) == bare(text)
}

/// A group's text with what's hidden in it worked out.
struct Prepared {
    at: Vec<At>,
    /// The joined text with every hidden character a `•`.
    safe: String,
    hide: Vec<Hide>,
    /// The covered secrets: their characters, and their text, kept only for
    /// `reveal` and never handed out.
    secrets: Vec<(Range<usize>, String)>,
    /// Where each row's characters start in the joined text, and one past the last row.
    row_starts: Vec<usize>,
    /// The group's lines as the wrap marks say, `(first row, last row)`:
    /// rows joined only because one was full to the edge are lines apart.
    lines: Vec<(usize, usize)>,
}

impl Prepared {
    /// `whole`: a key's line, or a line an edge cut short, hidden whole.
    fn new(rows: &[&RowText], look: &Look, whole: bool, revealed: &HashSet<String>) -> Self {
        let mut joined = String::new();
        let mut at = Vec::new();
        let mut row_starts = Vec::with_capacity(rows.len() + 1);
        let mut lines: Vec<(usize, usize)> = Vec::new();
        let mut line_start = 0;
        for (row, r) in rows.iter().enumerate() {
            if row > 0 && !r.wrapped {
                lines.push((line_start, row - 1));
                line_start = row;
            }
            row_starts.push(at.len());
            at.extend(r.cells().into_iter().map(|(col, end)| At { row, col, end }));
            joined.push_str(&r.text);
        }
        row_starts.push(at.len());
        if !rows.is_empty() {
            lines.push((line_start, rows.len() - 1));
        }
        let (hide, secrets) = if whole && look.secrets_hidden() {
            (joined.chars().map(|c| if c.is_whitespace() { Hide::No } else { Hide::Secret }).collect(), Vec::new())
        } else {
            let pieces: Vec<Range<usize>> = lines.iter().map(|&(a, b)| row_starts[a]..row_starts[b + 1]).collect();
            hide_in(look, &joined, &pieces, revealed)
        };
        let safe = dotted(&joined, &hide);
        Prepared { at, safe, hide, secrets, row_starts, lines }
    }

    /// `plain`: for each row, whether a full-screen program wrote it, so nothing on it is highlighted.
    fn present(&self, rows: &[&RowText], look: &Look, plain: &[bool], find: Option<&FindKey>) -> Vec<PresentedRow> {
        let mut out: Vec<PresentedRow> = (0..rows.len()).map(|_| PresentedRow::default()).collect();
        let chars: Vec<char> = self.safe.chars().collect();
        // What's hidden, run by run.
        let mut i = 0;
        while i < self.hide.len() {
            let h = self.hide[i];
            let mut j = i + 1;
            while j < self.hide.len() && self.hide[j] == h && self.at[j].row == self.at[i].row {
                j += 1;
            }
            match h {
                Hide::No => {}
                Hide::Masked => self.span(&mut out, i..j, Role::Masked),
                Hide::Secret => {
                    // A key's line can't be shown by a click: 0.10 didn't either.
                    let revealable = !look.streaming && self.secrets.iter().any(|(s, _)| s.contains(&i));
                    self.span(&mut out, i..j, Role::Secret { revealable });
                }
            }
            i = j;
        }
        // 3. Highlights, line by line, but none on what a full-screen program wrote.
        if look.highlights.is_on() {
            for &(a, b) in &self.lines {
                if (a..=b).all(|row| plain[row]) {
                    continue;
                }
                let (start, text) = self.line_text(&chars, a, b);
                let index = char_index(&text);
                for m in look.highlights.find_matches(&text) {
                    for (row, cols) in self.cols_by_row(start + index[m.range.start]..start + index[m.range.end]) {
                        if !plain[row] {
                            out[row].spans.push(RowSpan { cols, role: Role::Paint(m.paint) });
                        }
                    }
                }
            }
        }
        // 4. Links, line by line: none that runs over something hidden, as it
        // couldn't be shown or opened as it is.
        for &(a, b) in &self.lines {
            let (start, text) = self.line_text(&chars, a, b);
            let index = char_index(&text);
            for l in find_links(&text) {
                let range = start + index[l.range.start]..start + index[l.range.end];
                if self.hide[range.clone()].iter().all(|h| *h == Hide::No) {
                    let link = Link { kind: l.kind, target: l.target, line: l.line, end_line: l.end_line, from_program: false, disguised: false };
                    self.link(&mut out, range, link);
                }
            }
        }
        for (uri, range) in self.program_links(rows) {
            let Some((kind, target)) = hyperlink(&uri) else { continue };
            // Where it goes mustn't say what the screen hides, so it opens as it is or not at all.
            if look.safe_text(&uri) != uri || self.hide[range.clone()].iter().any(|h| *h != Hide::No) {
                continue;
            }
            let text: String = chars[range.clone()].iter().collect();
            let disguised = !shows_target(text.trim(), &uri, kind, &target);
            self.link(&mut out, range, Link { kind, target, line: None, end_line: None, from_program: true, disguised });
        }
        // 5. Find, over the rows' safe text, line by line as the wrap marks
        // say (as a find over the history reads them), each line without its
        // trailing blanks.
        if let Some(f) = find {
            let mut find_in: Vec<FindRow> = (0..rows.len())
                .map(|row| FindRow { text: chars[self.row_starts[row]..self.row_starts[row + 1]].iter().collect(), wrapped: row > 0 && rows[row].wrapped })
                .collect();
            for &(_, last) in &self.lines {
                let text = &mut find_in[last].text;
                text.truncate(text.trim_end().len());
            }
            for m in find_rows(&find_in, &f.query) {
                let current = f.current == Some((m.row, m.start));
                for p in match_pieces(&find_in, m) {
                    let a = self.row_starts[p.row] + p.start;
                    self.span(&mut out, a..a + p.length, Role::Found { current });
                }
            }
        }
        // The text, and the spans in order.
        for (row, r) in out.iter_mut().enumerate() {
            let first = self.row_starts[row];
            for (i, &c) in chars.iter().enumerate().take(self.row_starts[row + 1]).skip(first) {
                if self.hide[i] != Hide::No && i > first && self.at[i].col == self.at[i - 1].col {
                    continue;
                }
                r.text.push(c);
                r.cols.push(self.at[i].col);
            }
            r.spans.sort_by_key(|s| s.cols.start);
        }
        out
    }

    /// Where rows `first..=last` start in the joined text, and their safe text.
    fn line_text(&self, chars: &[char], first: usize, last: usize) -> (usize, String) {
        let (a, b) = (self.row_starts[first], self.row_starts[last + 1]);
        (a, chars[a..b].iter().collect())
    }

    /// The OSC 8 links in the group: runs of cells with the same link, as
    /// characters of the joined text. A run that reaches the end of a row
    /// and goes on at the start of the next (a link the terminal wrapped) is
    /// one link.
    fn program_links(&self, rows: &[&RowText]) -> Vec<(String, Range<usize>)> {
        let mut runs: Vec<(String, Range<usize>)> = Vec::new();
        for (row, r) in rows.iter().enumerate() {
            let (first, end) = (self.row_starts[row], self.row_starts[row + 1]);
            for h in &r.hyperlinks {
                let chars: Vec<usize> = (first..end).filter(|&i| h.cols.contains(&self.at[i].col)).collect();
                let (Some(&a), Some(&b)) = (chars.first(), chars.last()) else { continue };
                match runs.last_mut() {
                    Some((uri, run)) if *uri == h.uri && run.end == a && a == first => run.end = b + 1,
                    _ => runs.push((h.uri.clone(), a..b + 1)),
                }
            }
        }
        runs
    }

    /// A role over some characters of the joined text, as a span on each row they're on.
    fn span(&self, out: &mut [PresentedRow], chars: Range<usize>, role: Role) {
        for (row, cols) in self.cols_by_row(chars) {
            out[row].spans.push(RowSpan { cols, role });
        }
    }

    fn link(&self, out: &mut [PresentedRow], chars: Range<usize>, link: Link) {
        for (row, cols) in self.cols_by_row(chars) {
            let r = &mut out[row];
            r.links.push(link.clone());
            r.spans.push(RowSpan { cols, role: Role::Link(r.links.len() - 1) });
        }
    }

    /// Characters of the joined text as column ranges, row by row.
    fn cols_by_row(&self, chars: Range<usize>) -> Vec<(usize, Range<u16>)> {
        let mut out: Vec<(usize, Range<u16>)> = Vec::new();
        for i in chars.filter(|&i| i < self.at.len()) {
            let a = self.at[i];
            match out.last_mut() {
                Some((row, cols)) if *row == a.row => {
                    cols.start = cols.start.min(a.col);
                    cols.end = cols.end.max(a.end);
                }
                _ => out.push((a.row, a.col..a.end)),
            }
        }
        out
    }
}
