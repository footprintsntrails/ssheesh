//! Find in a terminal: the bar at the page's top right, as in a browser, and the search of the
//! whole history behind it (0.10's `openFind`, `setFindQuery`, `stepFind` and `FindBar`).
//!
//! The history searched is tmux's on a server that has it (read over ssh, as rows), else the
//! terminal's own grid. Either way the rows go through `Look::safe_rows` before anything looks
//! in them, on the thread that read them: a hidden secret, or a name hidden by streamer mode,
//! is dots there, so it is never found, counted or scrolled to. What is marked on screen is
//! marked by the presentation pass (`PaneView::set_find`), which reads the same safe text.
//!
//! ```text
//! typing ─ search ─ rows fresh? ─ yes ─ thread: find_rows ─ Found ─ poll: nearest match, scroll, mark
//!                        └ no ─ thread: read + safe_rows ─ FindRows ─ poll: keep, then as above
//! ```

use std::sync::Arc;
use std::time::Instant;

use rune::{text, text_input, El, Key, KeyEvent, TextEdit, Tx};
use sshee_term::find::{find_rows, nearest_match, rows_from_capture, takes_find_key, FindMatch, FindRow, FIND_LIMIT};

use super::overlays::{Ask, Note, Overlays, Reply};
use super::pane::TermPane;
use super::LinkStatus;
use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// How long rows read for a find stay good while output keeps coming (0.10's
/// `FIND_ROWS_FOR_MS`): stepping through matches scrolls tmux, which counts as output, and
/// must not ask the server for its history at every step.
const ROWS_FOR: f64 = 3.0;

/// The find field's width, in points (0.10's).
const FIELD: f32 = 176.0;

/// What to do with the matches of a search once they come.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Then {
    /// Start from the match nearest at or above this place (rows up from the bottom).
    Nearest(usize),
    /// Step `by` from the match at this place.
    Step(usize, i32),
}

/// What the presentation pass marks: the text looked for, and the match the bar is on (the
/// rows up from the history's bottom its first row is, and its first character there).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindMark {
    pub query: String,
    pub current: Option<(usize, usize)>,
}

/// A pane's find.
pub struct Find {
    pub(crate) open: bool,
    /// The field. Its text is the query.
    pub(crate) edit: TextEdit,
    /// The query the matches are of.
    query: String,
    rows: Option<Arc<Vec<FindRow>>>,
    /// When the rows were read (pane clock), and the newest output then.
    rows_at: f64,
    rows_output: Option<Instant>,
    loading: bool,
    matches: Vec<FindMatch>,
    /// Rows in the history searched, to count a match's place from the bottom.
    total: usize,
    /// The match the bar is on, in `matches`.
    at: Option<usize>,
    busy: bool,
    error: bool,
    /// Where the view was when the bar opened (rows up from the bottom): a search starts there.
    anchor: usize,
    /// The newest search: older ones' answers are dropped.
    ticket: u64,
    /// Goes up when the bar closes: rows read for an earlier bar aren't kept.
    epoch: u64,
    then: Then,
}

impl Default for Find {
    fn default() -> Self {
        Find {
            open: false,
            edit: TextEdit::new(""),
            query: String::new(),
            rows: None,
            rows_at: 0.0,
            rows_output: None,
            loading: false,
            matches: Vec::new(),
            total: 0,
            at: None,
            busy: false,
            error: false,
            anchor: 0,
            ticket: 0,
            epoch: 0,
            then: Then::Nearest(0),
        }
    }
}

impl Find {
    /// The find field has the keyboard: keys are its, not the program's.
    pub(crate) fn has_keyboard(&self) -> bool {
        self.open && self.edit.focused
    }

    /// What the bar says beside the field (0.10's wording).
    pub(crate) fn count(&self) -> String {
        if self.error {
            "Couldn't search".into()
        } else if self.busy {
            "…".into()
        } else if self.query.is_empty() {
            String::new()
        } else if self.matches.is_empty() {
            "No results".into()
        } else {
            let n = self.matches.len();
            let of = if n >= FIND_LIMIT { format!("{FIND_LIMIT}+") } else { n.to_string() };
            format!("{} of {of}", self.at.map_or(0, |i| i + 1))
        }
    }

    /// Looked, and it isn't there.
    fn nothing(&self) -> bool {
        !self.query.is_empty() && !self.busy && !self.error && self.matches.is_empty()
    }

    /// The rows read are still what the history holds: nothing was printed since, or they
    /// were read a moment ago.
    fn fresh(&self, clock: f64, last_output: Option<Instant>) -> bool {
        self.rows.is_some() && (last_output <= self.rows_output || clock - self.rows_at < ROWS_FOR)
    }
}

impl TermPane {
    /// The history is tmux's. Not just a title saying so (any program can print that): the
    /// session says it runs in tmux too.
    pub(crate) fn finds_in_tmux(&self) -> bool {
        matches!(self.status, LinkStatus::Connected { durable: true }) && self.model.tmux().is_some()
    }

    /// Whether Ctrl+F is for the find bar here (`sshee_term::find::takes_find_key`). Under
    /// tmux the terminal can't see which program is in front (tmux draws for all of them), so
    /// until something says (`set_foreground`), Ctrl+F stays the program's: it is vim's and
    /// less's "page down". Ctrl+Shift+F and ⌘F always open the bar.
    pub fn takes_find_key(&self) -> bool {
        let tmux = self.finds_in_tmux();
        if tmux && self.over.foreground.is_none() {
            return false;
        }
        takes_find_key(self.over.foreground.as_deref(), tmux, self.view.borrow().modes.alt_screen)
    }

    pub fn find_is_open(&self) -> bool {
        self.over.find.open
    }

    /// Rows up from the bottom of the history that the bottom of the view is.
    fn view_bottom(&self) -> usize {
        let v = self.model.scroll_view();
        v.total.saturating_sub(v.top + v.rows)
    }

    /// Opens the find bar, or puts the keyboard back on it with its text selected.
    pub fn open_find(&mut self, tx: &mut Tx) {
        let bottom = self.view_bottom();
        let f = &mut self.over.find;
        if !f.open {
            f.anchor = bottom;
            f.rows = None;
            f.open = true;
        }
        let end = f.edit.text.len();
        f.edit.set_selection(0, end);
        f.edit.focused = true;
        tx.focus("find-input", false);
        tx.instant();
    }

    /// Closes the bar. What was typed goes too: it could be a name, with streamer mode on by
    /// the next time.
    pub fn close_find(&mut self, tx: &mut Tx) {
        let f = &mut self.over.find;
        if !f.open {
            return;
        }
        let (ticket, epoch) = (f.ticket + 1, f.epoch + 1);
        *f = Find { ticket, epoch, ..Find::default() };
        self.view.borrow_mut().set_find(None);
        self.focus_terminal(tx);
        tx.instant();
    }

    /// The look changed (streamer mode): rows made safe under the old one may show what the
    /// new one hides, so they go, and an open find looks again.
    pub(crate) fn find_look_changed(&mut self, tx: &mut Tx) {
        let f = &mut self.over.find;
        f.rows = None;
        f.epoch += 1;
        f.loading = false;
        if f.open {
            let anchor = f.anchor;
            self.search(Then::Nearest(anchor), tx);
        }
    }

    /// The field's text changed.
    pub(crate) fn find_typed(&mut self, tx: &mut Tx) {
        let f = &mut self.over.find;
        if f.edit.text == f.query {
            return;
        }
        f.query = f.edit.text.clone();
        let anchor = f.anchor;
        self.search(Then::Nearest(anchor), tx);
    }

    /// A key in the field, before the field takes it: Return goes to the older match (most of
    /// what you're after is further up), Shift+Return to the newer, Escape closes.
    pub(crate) fn find_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        match ev.key {
            Key::Escape => self.close_find(tx),
            Key::Enter => self.find_step(if ev.mods.shift { 1 } else { -1 }, tx),
            _ => return false,
        }
        true
    }

    /// Older (`-1`) or newer (`1`).
    pub fn find_step(&mut self, by: i32, tx: &mut Tx) {
        let fresh = self.over.find.fresh(self.clock, self.last_output);
        let f = &mut self.over.find;
        if f.query.is_empty() {
            return;
        }
        if fresh && !f.busy {
            let n = f.matches.len() as i32;
            if n > 0 {
                f.at = Some((f.at.map_or(0, |i| i as i32) + by).rem_euclid(n) as usize);
            }
            self.go_to_match(tx);
            return;
        }
        // New output moved the rows: from the bottom, the current match is where it was.
        let place = f.at.and_then(|i| f.matches.get(i)).map_or(f.anchor, |m| f.total.saturating_sub(1 + m.row));
        self.search(Then::Step(place, by), tx);
    }

    /// Looks for the query, in rows read again if the ones at hand are stale.
    fn search(&mut self, then: Then, tx: &mut Tx) {
        let fresh = self.over.find.fresh(self.clock, self.last_output);
        let f = &mut self.over.find;
        f.ticket += 1;
        f.then = then;
        if f.query.is_empty() {
            f.matches.clear();
            f.at = None;
            f.busy = false;
            f.error = false;
            self.view.borrow_mut().set_find(None);
            tx.instant();
            return;
        }
        f.busy = true;
        match f.rows.clone().filter(|_| fresh) {
            Some(rows) => self.match_rows(rows),
            None => self.load_find_rows(),
        }
        tx.instant();
    }

    /// Searches `rows` off the UI thread.
    fn match_rows(&mut self, rows: Arc<Vec<FindRow>>) {
        let (ticket, query) = (self.over.find.ticket, self.over.find.query.clone());
        let post = self.over.post.clone();
        Overlays::work(self.replays(), move || post.post(Note::Found { ticket, matches: find_rows(&rows, &query) }));
    }

    /// Reads the rows to search: tmux's from the server, else the terminal's own. One read at
    /// a time, however fast you type: on Windows each may be a whole new login.
    fn load_find_rows(&mut self) {
        if self.over.find.loading {
            return;
        }
        self.over.find.loading = true;
        let epoch = self.over.find.epoch;
        let look = self.view.borrow().look().clone();
        let cols = self.size().cols as usize;
        if self.finds_in_tmux() {
            let safe = look.clone();
            let asked = self.ask(Ask::Rows, move |reply| {
                let rows = match reply {
                    Some(Reply::Rows(text)) => Some(safe.safe_rows(&rows_from_capture(&text, cols))),
                    _ => None,
                };
                Note::FindRows { epoch, rows }
            });
            if asked {
                return;
            }
        }
        let (model, post) = (self.model.clone(), self.over.post.clone());
        Overlays::work(self.replays(), move || {
            let rows = model.with_engine(|engine| {
                use sshee_term::engine::Engine;
                let history = engine.history_size() as i32;
                let screen = i32::from(engine.size().rows);
                let mut wrapped = false;
                let mut rows = Vec::with_capacity((history + screen) as usize);
                for line in -history..screen {
                    let Some(row) = engine.grid_row(line) else { continue };
                    rows.push(FindRow { text: row.trimmed_text(), wrapped });
                    wrapped = row.wraps;
                }
                rows
            });
            post.post(Note::FindRows { epoch, rows: Some(look.safe_rows(&rows)) });
        });
    }

    /// The rows came (or couldn't be read).
    pub(crate) fn find_rows_came(&mut self, epoch: u64, rows: Option<Vec<FindRow>>, tx: &mut Tx) {
        let (clock, last_output) = (self.clock, self.last_output);
        let f = &mut self.over.find;
        // The bar was closed meanwhile: the history isn't kept.
        if epoch != f.epoch {
            return;
        }
        f.loading = false;
        match rows {
            Some(rows) => {
                let rows = Arc::new(rows);
                f.rows = Some(rows.clone());
                f.rows_at = clock;
                f.rows_output = last_output;
                if f.busy {
                    self.match_rows(rows);
                }
            }
            None => {
                f.busy = false;
                f.error = true;
                f.matches.clear();
                f.at = None;
                self.view.borrow_mut().set_find(None);
            }
        }
        tx.instant();
    }

    /// A search's matches came.
    pub(crate) fn find_matches_came(&mut self, ticket: u64, matches: Vec<FindMatch>, tx: &mut Tx) {
        let f = &mut self.over.find;
        // You've typed on, or closed the bar.
        if ticket != f.ticket || !f.open {
            return;
        }
        f.total = f.rows.as_ref().map_or(0, |r| r.len());
        f.matches = matches;
        f.busy = false;
        f.error = false;
        let n = f.matches.len() as i32;
        f.at = match f.then {
            Then::Nearest(place) => nearest_match(&f.matches, f.total, place),
            Then::Step(place, by) => nearest_match(&f.matches, f.total, place).map(|i| (i as i32 + by).rem_euclid(n) as usize),
        };
        self.go_to_match(tx);
    }

    /// Marks the matches on screen and brings the current one into view: a third of the way
    /// down, unless it's in view already.
    fn go_to_match(&mut self, tx: &mut Tx) {
        let f = &self.over.find;
        let current = f.at.and_then(|i| f.matches.get(i)).map(|m| (f.total.saturating_sub(1 + m.row), m.start));
        self.view.borrow_mut().set_find(Some(FindMark { query: f.query.clone(), current }));
        tx.instant();
        let Some((from_bottom, _)) = current else { return };
        let v = self.model.scroll_view();
        let bottom = v.total.saturating_sub(v.top + v.rows);
        if from_bottom >= bottom && from_bottom < bottom + v.rows {
            return;
        }
        let wanted = from_bottom.saturating_sub(v.rows.saturating_sub(1 + v.rows / 3));
        let top = v.total.saturating_sub(v.rows + wanted);
        self.scroll_to(top as f64, tx);
    }
}

/// A `TextEdit` for a pane that is gone (its tab closed between the frame that drew its find
/// field and an edit the field had queued). The edit lands here and is never read: a few
/// hundred bytes, left behind in a case that takes a closed tab and a keystroke in one frame.
fn nowhere() -> &'static mut TextEdit {
    Box::leak(Box::new(TextEdit::new("")))
}

/// A small icon button of the bar.
fn bar_button(t: &Theme, key: &'static str, glyph: Icon, name: &'static str, enabled: bool) -> El {
    let e = rune::el()
        .key(key)
        .size(24.0, 24.0)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .justify_center()
        .role("button")
        .label_static(name)
        .help(name)
        .child(icon(glyph, theme::ICON, if enabled { t.l2on } else { t.off }));
    if enabled {
        e.hover(t.hov2).press(t.down).focusable(true)
    } else {
        e
    }
}

/// The find bar, while it's open.
pub(crate) fn bar<S: 'static>(pane: &TermPane, t: &'static Theme, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> Option<El> {
    let f = &pane.over.find;
    if !f.open {
        return None;
    }
    let any = !f.matches.is_empty();
    let field = text_input(&f.edit, theme::TEXT)
        .single_line()
        .key("find-input")
        .w(FIELD)
        .color(if f.nothing() { t.red } else { t.l1 })
        .placeholder("Find in the terminal", t.l2)
        .label_static("Find in the terminal")
        .on_edit(rune::edit_access(move |s: &mut S| match get(s) {
            Some(pane) => &mut pane.over.find.edit,
            None => nowhere(),
        }))
        .on_input_change(move |s: &mut S, tx| {
            if let Some(pane) = get(s) {
                pane.find_typed(tx);
            }
        })
        .on_input_key(move |s: &mut S, ev, tx| get(s).is_some_and(|pane| pane.find_key(ev, tx)));
    let count = text(f.count()).key("find-count").font(theme::SMALL.features("'tnum' 1")).color(t.l2).single_line();
    let step = move |by: i32| {
        move |s: &mut S, tx: &mut Tx| {
            if let Some(pane) = get(s) {
                pane.find_step(by, tx);
            }
        }
    };
    let mut older = bar_button(t, "find-older", Icon::ChevronUp, "Older match (Enter)", any);
    let mut newer = bar_button(t, "find-newer", Icon::ChevronDown, "Newer match (Shift+Enter)", any);
    if any {
        older = older.on_press_tx(step(-1));
        newer = newer.on_press_tx(step(1));
    }
    let close = bar_button(t, "find-close", Icon::Close, "Close (Esc)", true).on_press_tx(move |s: &mut S, tx| {
        if let Some(pane) = get(s) {
            pane.close_find(tx);
        }
    });
    Some(
        rune::row()
            .key("find")
            .pin(None, Some(8.0), Some(20.0), None)
            .h(36.0)
            .pl(12.0)
            .pr(6.0)
            .gap(2.0)
            .items_center()
            .radius(theme::RADIUS_POPOVER)
            .fill(t.pop)
            .border(1.0, t.rim)
            .shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.3))
            .cursor(rune::Cursor::Default)
            .role("search")
            .label_static("Find")
            .child(field)
            .child(rune::el().key("find-count-box").min_w(52.0).pr(4.0).items_center().justify_end().child(count))
            .child(older)
            .child(newer)
            .child(close),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_says_what_0_10_said() {
        let mut f = Find::default();
        assert_eq!(f.count(), "");
        f.query = "error".into();
        f.busy = true;
        assert_eq!(f.count(), "…");
        f.busy = false;
        assert_eq!(f.count(), "No results");
        assert!(f.nothing());
        f.matches = vec![FindMatch { row: 0, start: 0, length: 5 }; 12];
        f.at = Some(2);
        assert_eq!(f.count(), "3 of 12");
        f.matches = vec![FindMatch { row: 0, start: 0, length: 5 }; FIND_LIMIT];
        assert_eq!(f.count(), "3 of 10000+");
        f.error = true;
        assert_eq!(f.count(), "Couldn't search");
        assert!(!f.nothing());
    }

    #[test]
    fn rows_are_read_again_only_after_new_output_and_a_moment() {
        let mut f = Find::default();
        let t0 = Instant::now();
        assert!(!f.fresh(0.0, None), "no rows yet");
        f.rows = Some(Arc::new(Vec::new()));
        f.rows_at = 10.0;
        f.rows_output = Some(t0);
        assert!(f.fresh(100.0, Some(t0)), "nothing printed since");
        let later = t0 + std::time::Duration::from_millis(5);
        assert!(f.fresh(12.0, Some(later)), "printed since, but read a moment ago");
        assert!(!f.fresh(13.5, Some(later)));
    }
}
