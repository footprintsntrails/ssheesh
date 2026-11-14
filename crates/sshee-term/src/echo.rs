//! Stage zero of showing typed text before the server answers: nothing is
//! shown. For every key this works out whether it could have been drawn at
//! once and whether that guess would have been right, from the screen's cells
//! and not the bytes that drew them (those get rewritten on the way, by
//! tmux). All it does with the answer is count. Over Tor a key takes 60-400 ms
//! to come back, and the counts say whether drawing keys early would be worth
//! building at all.
//!
//! A port of 0.10's `src/lib/shadowEcho.ts`. It reads the terminal through
//! `EchoScreen` (read-only by its type) and tells `EchoSink`, and it uses
//! nothing else: it has no way to draw, send or save anything. Time comes in
//! as an argument, in ms, so it never reads a clock.
//!
//! Typed text isn't kept. To compare a key with its cell, a fingerprint of
//! the key is held until its answer comes, or for about 3 seconds if none
//! does. Fingerprints are a keyed hash (SipHash, the standard library's,
//! with keys picked at random for each terminal), so one that got out says
//! nothing of the key: trying every letter needs the keys too. That's no
//! lock against someone reading this program's memory, who could find the
//! keys there (and would see the key go past anyway). It means no typed text
//! can end up in the counts, on disk or in a log by accident: `Debug` shows
//! no fingerprint, nor how many keys are on their way (at a password prompt
//! that's the password's length).
//!
//! The rule for "could have been drawn" is the best a client can do alone:
//! the echo of an earlier key landed where it should, on this line, since the
//! last Enter or other control key. That's mosh's rule, and it has a hole: a
//! password prompt can appear on a row that already echoed something, and
//! keys typed then count as drawable. They come out here as `Wrong`,
//! `Dropped` or `Silent`, which is how the hole gets measured. Real
//! characters on screen would also need the server to announce its prompts.

use regex::Regex;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::LazyLock;

/// What the guesses read of a terminal. Rows are counted from the oldest
/// row kept, so a row keeps its number while the screen scrolls (xterm's
/// `baseY + cursorY`; alacritty's `history_size() + cursor line`).
pub trait EchoScreen {
    fn cols(&self) -> usize;
    /// The cursor's row and column.
    fn cursor(&self) -> (usize, usize);
    /// What a cell holds: its character and the marks joined to it. An empty
    /// cell, the second half of a wide character, and a cell past the edge
    /// are a space.
    fn cell(&self, row: usize, col: usize) -> (char, &[char]);
}

/// Where a key went: a line being typed, an agent, or a program with a screen of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EchoPlace {
    Line,
    Agent,
    Screen,
}

/// What gets counted.
/// - `Keys`: every key, Enter and arrows too; not pastes, clicks or the terminal's own replies.
/// - `Typed`: characters and Backspace, what could be drawn at all.
/// - `AtOnce`: of those, could have been drawn at once. Each ends as `Right`; `Wrong`; `Dropped`, on
///   its way when something else proved the guesses wrong; `Silent`, nothing came back; or
///   `Unjudged`, when an Enter or a new size came first and there's no telling.
/// - `AtPrompt`: of `AtOnce`, on a row that looked like a shell prompt. A rough stand-in for a prompt the server announces.
/// - `Waited`: had to wait for its echo: the first on a line, or the gate was shut.
/// - `HeldSilent`: of `Waited`, nothing ever came back. What the gate protected: password prompts, and vim's commands too.
/// - `MidLine`, `Unfit`: not guessed, because of text to the right of the cursor; or a wide character, or the last column.
/// - `PasswordOpen`: the gate opened on a row that reads like a password prompt. Should stay at zero.
///
/// Keys typed on such a row aren't counted as such: saved, that would be a password's length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EchoCount {
    Keys,
    Typed,
    AtOnce,
    AtPrompt,
    Right,
    Wrong,
    Dropped,
    Silent,
    Unjudged,
    Waited,
    HeldSilent,
    MidLine,
    Unfit,
    PasswordOpen,
}

/// Where the counts go.
pub trait EchoSink {
    fn count(&mut self, place: EchoPlace, what: EchoCount);
    /// How long a key's echo took.
    fn echo_took(&mut self, place: EchoPlace, ms: u64);
}

/// How long a key's answer is waited for. Nothing is drawn, so this only decides when a key counts as `Silent`.
pub const GIVE_UP_MS: u64 = 3000;
/// A flood of keys (a stuck key, say) isn't typing.
const MOST_ON_THE_WAY: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    None,
    Break,
    Other,
    Soft,
    Erase,
    Char,
}

/// The terminal's own answers to a server's questions, and focus reports. They aren't keys.
static NOT_KEYS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\x1b(?:[\]P_^X][^\n\r\u{2028}\u{2029}]|\[[?>=]|\[[0-9]*(?:;[0-9]+)*[Rn]$|\[[IO]$)").expect("pattern"));
/// Mouse reports and pastes: a click may switch tmux's pane.
static BREAKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\x1b\[(?:<|M|200~)").expect("pattern"));
/// Left, Right, Home and End, in both ways of writing them; Tab; Ctrl+A, E, K, U and W. They edit the line but don't end it.
static SOFT_KEYS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\x1b[\[O][DCHF]|[\t\x01\x05\x0b\x15\x17])$").expect("pattern"));

fn key_kind(data: &str) -> KeyKind {
    if NOT_KEYS.is_match(data) {
        return KeyKind::None;
    }
    if BREAKS.is_match(data) {
        return KeyKind::Break;
    }
    if SOFT_KEYS.is_match(data) {
        return KeyKind::Soft;
    }
    if data == "\x7f" || data == "\x08" {
        return KeyKind::Erase;
    }
    let mut chars = data.chars();
    let first = chars.next().unwrap_or('\0');
    if first == '\x1b' || first < ' ' {
        return KeyKind::Other;
    }
    // Text that came in one piece: an input method, or a paste with no marks around it.
    if chars.next().is_none() {
        KeyKind::Char
    } else {
        KeyKind::Break
    }
}

/// Takes one cell, by what people type: Latin, Greek, Cyrillic, their
/// punctuation, currency signs and №. Everything else (wide characters,
/// emoji, marks joining the one before) isn't guessed: how wide those are
/// is the terminal's to say once they're on screen.
fn one_cell(c: char) -> bool {
    matches!(c as u32, 0x20..=0x7e | 0xa0..=0x2ff | 0x370..=0x52f | 0x2010..=0x2027 | 0x2030..=0x205e | 0x20a0..=0x20bf | 0x2116)
}

/// A shell's prompt ends like this. Only a rough guess, for counting: `read -ep '$ '` looks the same.
static PROMPT_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[$#%>❯➜➤λ»›→] ?$").expect("pattern"));
/// Python's and the like: the end looks right, and the line is no shell's.
static NOT_SHELL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|\s)(?:>>>|\.\.\.) ?$").expect("pattern"));
/// A prompt that asks for something secret, in the languages seen most. Also only for counting: nothing here can be sure.
static ASKS_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)pass(?:word|phrase|code|wd)|парол|кодов|passwort|kennwort|contrase|mot de passe|(?-u:\b)pin(?-u:\b)|(?-u:\b)otp(?-u:\b)|密码|パスワード").expect("pattern")
});

/// What kind of row a key is typed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowLook {
    Prompt,
    Password,
    Other,
}

/// What the row reads like, by its text left of the cursor.
pub fn row_look(left: &str) -> RowLook {
    if ASKS_SECRET.is_match(left) {
        RowLook::Password
    } else if PROMPT_END.is_match(left) && !NOT_SHELL.is_match(left) {
        RowLook::Prompt
    } else {
        RowLook::Other
    }
}

/// A key on its way: where it should land and what should be there then.
/// Holds no text, and isn't `Debug`: its fingerprints stay out of logs.
#[derive(Clone, Copy)]
struct Guess {
    /// The cell it changes, and where the cursor is once its answer is in.
    col: usize,
    cursor: usize,
    /// Fingerprints of what the cell should hold after it, and what it holds till then.
    want: u64,
    was: u64,
    erase: bool,
    /// It was drawable when typed.
    shown: bool,
    /// A key that isn't typing came after it, so a later screen says nothing against it.
    overtaken: bool,
    place: EchoPlace,
    at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Right,
    Wrong,
    Silent,
    Unjudged,
}

impl Verdict {
    fn count(self) -> EchoCount {
        match self {
            Verdict::Right => EchoCount::Right,
            Verdict::Wrong => EchoCount::Wrong,
            Verdict::Silent => EchoCount::Silent,
            Verdict::Unjudged => EchoCount::Unjudged,
        }
    }
}

/// One terminal's counter.
pub struct ShadowEcho<K: EchoSink> {
    sink: K,
    /// An echo proved this line takes keys, and nothing has happened since to cast doubt.
    open: bool,
    pending: Vec<Guess>,
    /// The row and column the next key would land on, while keys are on their way.
    row: usize,
    next: usize,
    /// Columns the cursor can be in while they are.
    lo: usize,
    hi: usize,
    /// Where the proved typing starts, and a fingerprint of the row left of it.
    span_start: usize,
    before: u64,
    /// A key that isn't typing was sent and no output has come since, so the cursor's place is old news.
    awaiting: bool,
    look: RowLook,
    look_row: Option<usize>,
    /// The fingerprints' keys, picked at random for this terminal.
    keys: RandomState,
    /// The fingerprint of an empty cell.
    blank: u64,
}

/// Only what says nothing of the keys typed: never a fingerprint, nor how
/// many keys are on their way, only where they went.
impl<K: EchoSink + fmt::Debug> fmt::Debug for ShadowEcho<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut places: Vec<EchoPlace> = Vec::new();
        for g in &self.pending {
            if !places.contains(&g.place) {
                places.push(g.place);
            }
        }
        f.debug_struct("ShadowEcho")
            .field("sink", &self.sink)
            .field("open", &self.open)
            .field("awaiting", &self.awaiting)
            .field("look", &self.look)
            .field("on_the_way_to", &places)
            .finish_non_exhaustive()
    }
}

impl<K: EchoSink> ShadowEcho<K> {
    pub fn new(sink: K) -> Self {
        let keys = RandomState::new();
        ShadowEcho {
            sink,
            open: false,
            pending: Vec::new(),
            row: 0,
            next: 0,
            lo: 0,
            hi: 0,
            span_start: 0,
            before: 0,
            awaiting: false,
            look: RowLook::Other,
            look_row: None,
            blank: cell_print(&keys, ' ', &[]),
            keys,
        }
    }

    pub fn sink(&self) -> &K {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut K {
        &mut self.sink
    }

    /// Something the terminal is sending, before it goes; `place` is where it goes, as far as anyone knows.
    pub fn key(&mut self, screen: &impl EchoScreen, data: &str, place: EchoPlace, now: u64) {
        let kind = key_kind(data);
        if kind == KeyKind::None {
            return;
        }
        self.settle(screen, now);
        if kind == KeyKind::Break {
            return self.boundary(true);
        }
        self.sink.count(place, EchoCount::Keys);
        if kind == KeyKind::Other || kind == KeyKind::Soft {
            return self.boundary(kind == KeyKind::Other);
        }
        self.sink.count(place, EchoCount::Typed);
        if kind == KeyKind::Erase {
            self.guess_erase(screen, place, now);
        } else {
            self.guess_char(screen, data.chars().next().unwrap_or(' '), place, now);
        }
    }

    /// Output reached the terminal's grid.
    pub fn parsed(&mut self, screen: &impl EchoScreen, now: u64) {
        self.settle(screen, now);
        self.awaiting = false;
    }

    /// Every so often: keys that got no answer are given up on, and their fingerprints go.
    pub fn tick(&mut self, screen: &impl EchoScreen, now: u64) {
        self.settle(screen, now);
    }

    /// What's on screen stopped meaning what it did: a new size, the connection, the other screen.
    pub fn reset(&mut self) {
        self.drop_pending(EchoCount::Unjudged);
        self.boundary(true);
    }

    /// Keys on their way (tests: a key that got no answer must not stay).
    pub fn on_the_way(&self) -> usize {
        self.pending.len()
    }

    fn guess_char(&mut self, screen: &impl EchoScreen, code: char, place: EchoPlace, now: u64) {
        // An Enter's or a Tab's answer is on its way: nobody knows yet where this lands.
        if self.awaiting || self.pending.first().is_some_and(|g| g.overtaken) {
            return self.sink.count(place, EchoCount::Waited);
        }
        if self.pending.is_empty() {
            let (row, col) = screen.cursor();
            self.row = row;
            self.next = col;
            self.lo = col;
            self.hi = col;
        }
        if self.look_row != Some(self.row) {
            self.look_row = Some(self.row);
            self.look = row_look(&self.left_of(screen, self.next));
        }
        if self.pending.len() >= MOST_ON_THE_WAY || !one_cell(code) || self.next >= screen.cols().saturating_sub(1) {
            self.sink.count(place, EchoCount::Unfit);
            return self.boundary(false);
        }
        // Cells a Backspace on its way is about to clear don't count.
        if self.has_text_from(screen, self.hi) {
            self.sink.count(place, EchoCount::MidLine);
            return self.boundary(false);
        }
        let shown = self.open;
        self.sink.count(place, if shown { EchoCount::AtOnce } else { EchoCount::Waited });
        if shown && self.look == RowLook::Prompt {
            self.sink.count(place, EchoCount::AtPrompt);
        }
        let col = self.next;
        self.next = col + 1;
        self.hi = self.hi.max(self.next);
        let want = cell_print(&self.keys, code, &[]);
        self.pending.push(Guess { col, cursor: col + 1, want, was: self.blank, erase: false, shown, overtaken: false, place, at: now });
    }

    fn guess_erase(&mut self, screen: &impl EchoScreen, place: EchoPlace, now: u64) {
        // Only over what was proved. Over a key still on its way, what's on
        // screen now looks just like the Backspace having arrived.
        if !self.open || self.next <= self.span_start || self.pending.last().is_some_and(|g| !g.erase) {
            self.sink.count(place, EchoCount::Waited);
            return self.boundary(false);
        }
        self.sink.count(place, EchoCount::AtOnce);
        if self.look == RowLook::Prompt {
            self.sink.count(place, EchoCount::AtPrompt);
        }
        let col = self.next - 1;
        self.next = col;
        self.lo = self.lo.min(col);
        let was = self.cell_print(screen, self.row, col);
        self.pending.push(Guess { col, cursor: col, want: self.blank, was, erase: true, shown: true, overtaken: false, place, at: now });
    }

    /// The line isn't what was being guessed any more. What's on its way can still come true, but nothing new is guessed from it.
    fn boundary(&mut self, forget_row: bool) {
        self.open = false;
        self.awaiting = true;
        if forget_row {
            self.look_row = None;
        }
        for g in &mut self.pending {
            g.overtaken = true;
        }
    }

    /// Compares what's on screen with what's on its way, and watches the proved line.
    fn settle(&mut self, screen: &impl EchoScreen, now: u64) {
        let cur = screen.cursor();
        if !self.pending.is_empty() {
            self.judge(screen, cur, now);
        }
        if self.open {
            self.watch(screen, cur);
        }
    }

    fn judge(&mut self, screen: &impl EchoScreen, cur: (usize, usize), now: u64) {
        if cur.0 != self.row {
            // The line is left behind. Its cells still say what landed before that.
            for g in std::mem::take(&mut self.pending) {
                let landed = g.want != self.blank && self.cell_print(screen, self.row, g.col) == g.want;
                let verdict = if landed {
                    Verdict::Right
                } else if g.overtaken {
                    Verdict::Unjudged
                } else {
                    Verdict::Wrong
                };
                self.settled(screen, &g, verdict, cur, now);
            }
            return;
        }
        // Answers come in order: the newest key whose answer is on screen, and all before it, are in.
        let up_to = (0..self.pending.len()).rev().find(|&k| {
            let g = self.pending[k];
            g.cursor == cur.1 && self.cell_print(screen, self.row, g.col) == g.want
        });
        let landed: Vec<Guess> = self.pending.drain(..up_to.map_or(0, |k| k + 1)).collect();
        for (i, g) in landed.iter().enumerate() {
            let redone = landed[i + 1..].iter().any(|later| later.col == g.col);
            let verdict = if redone || self.cell_print(screen, self.row, g.col) == g.want {
                Verdict::Right
            } else if g.overtaken {
                Verdict::Unjudged
            } else {
                Verdict::Wrong
            };
            self.settled(screen, g, verdict, cur, now);
        }
        while let Some(&g) = self.pending.first() {
            let late = now.saturating_sub(g.at) >= GIVE_UP_MS;
            if g.overtaken {
                if !late {
                    return;
                }
                self.pending.remove(0);
                self.settled(screen, &g, Verdict::Unjudged, cur, now);
                continue;
            }
            let here = self.cell_print(screen, self.row, g.col);
            // Something other than what was guessed, or the cursor went past a cell left as it was.
            if here != g.want && (here != g.was || (!g.erase && cur.1 >= g.cursor)) {
                self.pending.remove(0);
                self.settled(screen, &g, Verdict::Wrong, cur, now);
                self.open = false;
                self.look_row = None;
                return self.drop_pending(EchoCount::Dropped);
            }
            if !late {
                return;
            }
            self.pending.remove(0);
            self.settled(screen, &g, Verdict::Silent, cur, now);
            // The line stopped taking keys. Later ones run out on their own time.
            self.open = false;
            self.look_row = None;
        }
    }

    fn settled(&mut self, screen: &impl EchoScreen, g: &Guess, verdict: Verdict, cur: (usize, usize), now: u64) {
        if verdict == Verdict::Right {
            if g.erase || g.want != self.blank {
                self.sink.echo_took(g.place, now.saturating_sub(g.at));
            }
            if g.shown {
                self.sink.count(g.place, EchoCount::Right);
            } else {
                self.prove(screen, g, cur);
            }
        } else if g.shown {
            self.sink.count(g.place, verdict.count());
        } else if verdict == Verdict::Silent {
            self.sink.count(g.place, EchoCount::HeldSilent);
        }
    }

    /// A key that had to wait came back where it should: the line takes keys. A blank proves nothing, since it looks the same before and after.
    fn prove(&mut self, screen: &impl EchoScreen, g: &Guess, cur: (usize, usize)) {
        if self.open || g.overtaken || g.erase || g.want == self.blank || cur.0 != self.row || cur.1 <= g.col {
            return;
        }
        self.open = true;
        self.span_start = g.col;
        self.before = self.row_print(screen, self.row, g.col);
        if self.look == RowLook::Password {
            self.sink.count(g.place, EchoCount::PasswordOpen);
        }
    }

    /// Shuts the gate if the cursor isn't where the keys say, or the text before them changed: something other than the line editor is writing here.
    fn watch(&mut self, screen: &impl EchoScreen, cur: (usize, usize)) {
        let on_way = !self.pending.is_empty();
        let in_place = cur.0 == self.row && if on_way { cur.1 >= self.lo && cur.1 <= self.hi } else { cur.1 == self.next };
        if !in_place || self.row_print(screen, self.row, self.span_start) != self.before {
            self.open = false;
            self.look_row = None;
            self.drop_pending(EchoCount::Dropped);
        } else if !on_way {
            self.lo = self.next;
            self.hi = self.next;
        }
    }

    /// Forgets what's on its way, counting what was shown as `how` (`Dropped` or `Unjudged`).
    fn drop_pending(&mut self, how: EchoCount) {
        for g in std::mem::take(&mut self.pending) {
            if g.shown {
                self.sink.count(g.place, if g.overtaken { EchoCount::Unjudged } else { how });
            }
        }
    }

    fn has_text_from(&self, screen: &impl EchoScreen, col: usize) -> bool {
        (col..screen.cols()).any(|x| {
            let (c, marks) = screen.cell(self.row, x);
            !c.is_whitespace() || marks.iter().any(|m| !m.is_whitespace())
        })
    }

    /// The row's text left of a column. Read for a moment to tell what kind of row it is, never kept.
    fn left_of(&self, screen: &impl EchoScreen, col: usize) -> String {
        let mut text = String::new();
        for x in 0..col {
            let (c, marks) = screen.cell(self.row, x);
            text.push(c);
            text.extend(marks);
        }
        text
    }

    fn cell_print(&self, screen: &impl EchoScreen, row: usize, col: usize) -> u64 {
        let (c, marks) = screen.cell(row, col);
        cell_print(&self.keys, c, marks)
    }

    fn row_print(&self, screen: &impl EchoScreen, row: usize, end: usize) -> u64 {
        let mut h = self.keys.build_hasher();
        for x in 0..end {
            let (c, marks) = screen.cell(row, x);
            (c, marks).hash(&mut h);
        }
        h.finish()
    }
}

/// A cell's fingerprint under `keys`: its character and the marks joined to
/// it. An empty cell and a space are the same thing to look at (the screen
/// gives a space for both), and a key typed is a cell with no marks.
fn cell_print(keys: &RandomState, c: char, marks: &[char]) -> u64 {
    keys.hash_one((c, marks))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen of plain rows with a cursor, enough for what needs no terminal.
    struct Rows {
        rows: Vec<Vec<char>>,
        cursor: (usize, usize),
    }

    impl EchoScreen for Rows {
        fn cols(&self) -> usize {
            80
        }
        fn cursor(&self) -> (usize, usize) {
            self.cursor
        }
        fn cell(&self, row: usize, col: usize) -> (char, &[char]) {
            (self.rows.get(row).and_then(|r| r.get(col)).copied().unwrap_or(' '), &[])
        }
    }

    #[derive(Debug, Default)]
    struct Nothing;
    impl EchoSink for Nothing {
        fn count(&mut self, _: EchoPlace, _: EchoCount) {}
        fn echo_took(&mut self, _: EchoPlace, _: u64) {}
    }

    fn password_prompt() -> Rows {
        let prompt: Vec<char> = "Password: ".chars().collect();
        let at = prompt.len();
        Rows { rows: vec![prompt], cursor: (0, at) }
    }

    #[test]
    fn forgets_the_keys_that_got_no_answer() {
        let screen = password_prompt();
        let mut echo = ShadowEcho::new(Nothing);
        for c in "hunter2-пароль".chars() {
            echo.key(&screen, &c.to_string(), EchoPlace::Line, 1000);
        }
        assert!(echo.on_the_way() > 0);
        echo.tick(&screen, 1000 + GIVE_UP_MS);
        assert_eq!(echo.on_the_way(), 0);
    }

    #[test]
    fn holds_no_typed_text_while_keys_are_on_their_way() {
        let screen = password_prompt();
        let mut echo = ShadowEcho::new(Nothing);
        for c in "hunter2".chars() {
            echo.key(&screen, &c.to_string(), EchoPlace::Line, 1000);
        }
        assert!(echo.on_the_way() > 0);
        // Only the names of places and kinds of row: nothing of the keys.
        let state = format!("{echo:?}");
        assert!(!state.contains("hunter") && !state.contains("unter2"), "{state}");
        // No fingerprint either, in any base: with the keys it hashes under,
        // one would give its key back after a hundred tries.
        let mut prints = vec![echo.blank, echo.before];
        prints.extend(echo.pending.iter().flat_map(|g| [g.want, g.was]));
        for p in prints {
            for shown in [p.to_string(), format!("{p:x}"), format!("{p:X}"), (p as u32).to_string(), format!("{:x}", p as u32)] {
                assert!(!state.contains(&shown), "{shown} in {state}");
            }
        }
        // Nor how many keys are on their way: at this prompt, a password's length.
        assert!(!state.contains("pending") && !state.contains("next"), "{state}");
    }

    #[test]
    fn a_fingerprint_cant_be_turned_back_into_its_key_without_the_keys() {
        let screen = password_prompt();
        let mut echo = ShadowEcho::new(Nothing);
        echo.key(&screen, "q", EchoPlace::Line, 0);
        let want = echo.pending[0].want;
        // What anyone could compute without this terminal's keys matches no letter.
        let unkeyed = RandomState::new();
        assert!(!(' '..='~').any(|c| cell_print(&unkeyed, c, &[]) == want));
        assert_eq!(cell_print(&echo.keys, 'q', &[]), want);
    }

    #[test]
    fn gets_a_different_fingerprint_each_time_so_none_can_be_looked_up() {
        let screen = password_prompt();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..5 {
            let mut echo = ShadowEcho::new(Nothing);
            echo.key(&screen, "a", EchoPlace::Line, 0);
            seen.insert(echo.pending[0].want);
        }
        assert!(seen.len() > 1);
    }
}
