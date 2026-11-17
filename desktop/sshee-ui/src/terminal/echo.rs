//! Settings' "Count how much typing could appear instantly": `sshee_term::echo` run on a
//! pane's screen. Nothing is drawn early and nothing typed is kept: each key is a fingerprint
//! until its echo comes, and only counts leave it.
//!
//! The counts go to [`totals`], this program's sum over every terminal, in memory. 0.10 saved
//! them in batches of 200 keys or more (so they can't tell how long a password typed in
//! between was); 1.0 keeps the counts 0.10 saved (`model::TypingCounts`) and doesn't add to
//! them on disk yet.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use sshee_term::echo::{EchoCount, EchoPlace, EchoScreen, EchoSink, ShadowEcho};
use sshee_term::engine::Row;

use super::pane::TermPane;

/// Counts by where the typing went and what was counted, and how long echoes took (ms).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub counts: HashMap<(EchoPlace, EchoCount), u64>,
    pub echoes: HashMap<EchoPlace, Vec<u64>>,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        for (k, n) in &other.counts {
            *self.counts.entry(*k).or_default() += n;
        }
        for (place, ms) in &other.echoes {
            self.echoes.entry(*place).or_default().extend(ms);
        }
    }

    /// A count, 0 when there's none.
    pub fn count(&self, place: EchoPlace, what: EchoCount) -> u64 {
        self.counts.get(&(place, what)).copied().unwrap_or(0)
    }
}

impl EchoSink for Tally {
    fn count(&mut self, place: EchoPlace, what: EchoCount) {
        *self.counts.entry((place, what)).or_default() += 1;
    }
    fn echo_took(&mut self, place: EchoPlace, ms: u64) {
        self.echoes.entry(place).or_default().push(ms);
    }
}

fn all() -> &'static Mutex<Tally> {
    static ALL: OnceLock<Mutex<Tally>> = OnceLock::new();
    ALL.get_or_init(Default::default)
}

/// Every terminal's counts so far, this run.
pub fn totals() -> Tally {
    all().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A pane's counter.
pub struct Counter {
    shadow: ShadowEcho<Tally>,
    start: Instant,
    /// Rows of history above the screen, at the last frame: a row keeps its number while the
    /// screen scrolls.
    pub(crate) history: usize,
}

impl Counter {
    pub fn new() -> Counter {
        Counter { shadow: ShadowEcho::new(Tally::default()), start: Instant::now(), history: 0 }
    }

    fn ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Hands what was counted so far to [`totals`].
    fn flush(&mut self) {
        let mine = std::mem::take(self.shadow.sink_mut());
        all().lock().unwrap_or_else(|e| e.into_inner()).add(&mine);
    }
}

impl Default for Counter {
    fn default() -> Self {
        Counter::new()
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.flush();
    }
}

/// The screen as the pane last drew it, rows counted from the oldest kept.
struct Screen<'a> {
    rows: &'a [Row],
    history: usize,
    cursor: (usize, usize),
    cols: usize,
}

impl EchoScreen for Screen<'_> {
    fn cols(&self) -> usize {
        self.cols
    }
    fn cursor(&self) -> (usize, usize) {
        self.cursor
    }
    fn cell(&self, row: usize, col: usize) -> (char, &[char]) {
        let ch = row
            .checked_sub(self.history)
            .and_then(|line| self.rows.get(line))
            .and_then(|r| r.cells.get(col))
            .filter(|c| c.width > 0)
            .map_or(' ', |c| c.ch);
        (ch, &[])
    }
}

impl TermPane {
    /// Where typing goes now: a program with a screen of its own, an agent, or a line.
    fn echo_place(&self) -> EchoPlace {
        let agent = self.over.foreground.as_deref().is_some_and(|c| sshee_app::agents::AgentKind::ALL.iter().any(|a| a.command() == c));
        if agent {
            EchoPlace::Agent
        } else if self.view.borrow().modes.alt_screen {
            EchoPlace::Screen
        } else {
            EchoPlace::Line
        }
    }

    fn with_screen(&mut self, f: impl FnOnce(&mut Counter, &Screen)) {
        let Some(mut counter) = self.echo.take() else { return };
        {
            let view = self.view.borrow();
            let screen = Screen {
                rows: view.mirror(),
                history: counter.history,
                cursor: (counter.history + view.cursor.line, usize::from(view.cursor.col)),
                cols: usize::from(view.size.cols),
            };
            f(&mut counter, &screen);
        }
        self.echo = Some(counter);
    }

    /// A key on its way to the session: guessed and counted.
    pub(crate) fn count_key(&mut self, bytes: &[u8]) {
        if self.echo.is_none() || self.view.borrow().display_offset > 0 {
            return;
        }
        let place = self.echo_place();
        let data = String::from_utf8_lossy(bytes).into_owned();
        self.with_screen(|c, screen| {
            let now = c.ms();
            c.shadow.key(screen, &data, place, now);
        });
    }

    /// A frame of output was drawn (`history`: rows above the screen now): the keys on their
    /// way are judged.
    pub(crate) fn count_frame(&mut self, history: usize) {
        if let Some(c) = self.echo.as_mut() {
            if c.history != history {
                c.history = history;
            }
        } else {
            return;
        }
        self.with_screen(|c, screen| {
            let now = c.ms();
            c.shadow.parsed(screen, now);
            c.shadow.tick(screen, now);
            c.flush();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_term::engine::Row;

    #[test]
    fn a_typed_letter_that_echoes_is_counted_and_nothing_typed_is_kept() {
        let mut c = Counter::new();
        let prompt = Row::from_text("$ ", 20);
        let rows = vec![prompt.clone()];
        let screen = Screen { rows: &rows, history: 0, cursor: (0, 2), cols: 20 };
        c.shadow.key(&screen, "l", EchoPlace::Line, 0);
        let echoed = vec![Row::from_text("$ l", 20)];
        let screen = Screen { rows: &echoed, history: 0, cursor: (0, 3), cols: 20 };
        c.shadow.parsed(&screen, 40);
        let tally = c.shadow.sink().clone();
        assert_eq!(tally.count(EchoPlace::Line, EchoCount::Keys), 1);
        assert_eq!(tally.count(EchoPlace::Line, EchoCount::Typed), 1);
        let debug = format!("{:?}", c.shadow);
        assert!(!debug.contains('l') || !debug.contains("\"l\""), "{debug}");
        c.flush();
        assert!(totals().count(EchoPlace::Line, EchoCount::Keys) >= 1);
    }
}
