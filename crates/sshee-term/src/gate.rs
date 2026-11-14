//! When the screen layer should take a new picture of the terminal.
//!
//! A program redrawing its screen (an agent erasing everything and printing it again, say)
//! reaches us in several pieces over the network. Drawn as they come, the screen shows an empty
//! or half-drawn picture: it flickers and tears. 0.10's `FrameGate` (output.ts) held such bursts
//! back with DEC 2026 marks written into xterm.js's input. Here nothing is rewritten: the
//! [`RenderGate`] only says whether to snapshot the rows now or wait, with the same timings.
//!
//! What it doesn't do any more: a program's own DEC 2026 hold (tmux sends one around every
//! update) is kept by the parser itself, which buffers until the program ends it or 150 ms pass
//! (vte's limit; 0.10 waited up to a second).
//!
//! Time is passed in, never read here, so it's all testable.

use std::time::{Duration, Instant};

/// Output this long is a screenful or more on its way.
pub const BURST_SIZE: usize = 2048;
/// A burst is over when nothing has come for this long: a little over the 16 ms a steady
/// stream arrives in.
pub const BURST_QUIET: Duration = Duration::from_millis(24);
/// The longest the old picture stays while output keeps coming, so a flood still moves.
pub const BURST_MAX: Duration = Duration::from_millis(64);
/// The same after an erase: better a moment's wait than an empty screen. The pieces of a
/// redraw can be a round trip to the server apart.
pub const ERASED_QUIET: Duration = Duration::from_millis(80);
pub const ERASED_MAX: Duration = Duration::from_millis(200);
/// What you type answers at once: an erase this soon after a key is a prompt redrawing.
pub const TYPED: Duration = Duration::from_millis(150);
/// The longest a key's answer can take, through Tor at its worst. The first output in this
/// time is the answer.
pub const ANSWER: Duration = Duration::from_millis(3000);

/// Decides when a terminal's screen is taken a picture of. One per terminal.
#[derive(Clone, Debug, Default)]
pub struct RenderGate {
    /// Holding the picture for a burst.
    holding: bool,
    /// The burst began by erasing the screen.
    erased: bool,
    /// Show by then even if output is still coming.
    quiet_until: Option<Instant>,
    longest_until: Option<Instant>,
    typed_at: Option<Instant>,
    answer_due: bool,
    /// The end of the last output when it was the start of an erase (`ESC`, `ESC [`,
    /// `ESC [ 2`…), joined to the next output's start: an erase cut between two reads still
    /// counts (0.10's FrameGate held such a cut mark back the same way).
    tail: [u8; 3],
    tail_len: u8,
}

/// What the gate says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// Take the picture now.
    Now,
    /// Not yet: ask again at this time (or when more output comes).
    At(Instant),
}

impl RenderGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// A key was sent: what comes back shows at once.
    pub fn typed(&mut self, now: Instant) {
        self.typed_at = Some(now);
        self.answer_due = true;
    }

    /// Output arrived (call before or after the engine parsed it, with the same bytes).
    pub fn output(&mut self, now: Instant, bytes: &[u8]) {
        let since_key = self.typed_at.map(|at| now.saturating_duration_since(at));
        let answer = since_key.is_some_and(|d| d <= TYPED || (self.answer_due && d < ANSWER));
        let (length, erases) = measure(bytes);
        let erases = erases || self.erase_across(bytes);
        self.keep_tail(bytes);
        if length > 0 {
            self.answer_due = false;
        }
        let erases = erases && !answer;
        if !self.holding && (erases || length >= BURST_SIZE) {
            self.holding = true;
            self.longest_until = Some(now + if erases { ERASED_MAX } else { BURST_MAX });
        } else if self.holding && erases && !self.erased {
            // A flood that turns into a redraw gets the longer wait.
            self.longest_until = Some(now + ERASED_MAX);
        }
        self.erased |= erases;
        if self.holding {
            self.quiet_until = Some(now + if self.erased { ERASED_QUIET } else { BURST_QUIET });
        }
    }

    /// Whether to take the picture now. Ends a hold whose time is up.
    pub fn show(&mut self, now: Instant) -> Show {
        if !self.holding {
            return Show::Now;
        }
        let until = self.deadline().unwrap_or(now);
        if now >= until {
            self.release();
            Show::Now
        } else {
            Show::At(until)
        }
    }

    /// While holding, when the hold ends at the latest (unless more output moves it).
    pub fn deadline(&self) -> Option<Instant> {
        if !self.holding {
            return None;
        }
        match (self.quiet_until, self.longest_until) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub fn is_holding(&self) -> bool {
        self.holding
    }

    /// The burst is over: show what's there.
    pub fn release(&mut self) {
        self.holding = false;
        self.erased = false;
        self.quiet_until = None;
        self.longest_until = None;
    }

    /// The connection dropped: nothing more of this burst is coming. (A key's answer still
    /// counts as one: the key that wakes a sleeping terminal is answered by the reconnect.)
    pub fn reset(&mut self) {
        self.release();
        self.tail_len = 0;
    }

    /// The last output's tail and this output's start make an erase.
    fn erase_across(&self, bytes: &[u8]) -> bool {
        let tail = self.tail_len as usize;
        if tail == 0 {
            return false;
        }
        let mut joined = [0u8; 7];
        joined[..tail].copy_from_slice(&self.tail[..tail]);
        let n = bytes.len().min(4);
        joined[tail..tail + n].copy_from_slice(&bytes[..n]);
        is_erase(&joined[..tail + n])
    }

    /// Keeps the end of what came (with the last tail, when this output is short) if it is the
    /// start of an erase.
    fn keep_tail(&mut self, bytes: &[u8]) {
        let mut last = [0u8; 6];
        let mut len = 0;
        if bytes.len() < 3 {
            let tail = self.tail_len as usize;
            last[..tail].copy_from_slice(&self.tail[..tail]);
            len = tail;
        }
        let take = bytes.len().min(3);
        last[len..len + take].copy_from_slice(&bytes[bytes.len() - take..]);
        len += take;
        let last = &last[len.saturating_sub(3)..len];
        self.tail_len = 0;
        if let Some(at) = last.iter().rposition(|&b| b == 0x1b) {
            let start = &last[at..];
            if matches!(start, [0x1b] | [0x1b, b'['] | [0x1b, b'[', b'0' | b'2' | b'3']) {
                self.tail[..start.len()].copy_from_slice(start);
                self.tail_len = start.len() as u8;
            }
        }
    }
}

const SYNC_ON: &[u8] = b"\x1b[?2026h";
const SYNC_OFF: &[u8] = b"\x1b[?2026l";

/// The output's length without synchronized-output marks (0.10 took them out before measuring),
/// and whether it erases the screen. One pass over its Esc characters.
fn measure(bytes: &[u8]) -> (usize, bool) {
    let mut marks = 0;
    let mut erases = false;
    let mut from = 0;
    while let Some(at) = bytes[from..].iter().position(|&b| b == 0x1b) {
        let tail = &bytes[from + at..];
        if tail.starts_with(SYNC_ON) || tail.starts_with(SYNC_OFF) {
            marks += 1;
        } else if is_erase(tail) {
            erases = true;
        }
        from += at + 1;
    }
    (bytes.len().saturating_sub(marks * SYNC_ON.len()), erases)
}

/// `ESC [ J`, `ESC [ 0 J`, `ESC [ 2 J` or `ESC [ 3 J` at the start.
fn is_erase(tail: &[u8]) -> bool {
    tail.starts_with(b"\x1b[J") || (tail.len() >= 4 && tail.starts_with(b"\x1b[") && matches!(tail[2], b'0' | b'2' | b'3') && tail[3] == b'J')
}

#[cfg(test)]
fn content_len(bytes: &[u8]) -> usize {
    measure(bytes).0
}

/// Erasing the screen (`ESC [ J`, `ESC [ 0 J`, `ESC [ 2 J`, `ESC [ 3 J`): what draws it again is
/// right behind.
pub fn erases_screen(bytes: &[u8]) -> bool {
    measure(bytes).1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn small_output_shows_at_once() {
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        g.output(t0, b"hello\r\n");
        assert_eq!(g.show(t0), Show::Now);
        assert_eq!(g.deadline(), None);
    }

    #[test]
    fn a_flood_is_held_until_quiet_or_the_longest_wait() {
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        let chunk = vec![b'x'; BURST_SIZE];
        g.output(t0, &chunk);
        assert_eq!(g.show(t0 + ms(10)), Show::At(t0 + BURST_QUIET));
        // Output keeps coming every 16 ms: quiet never arrives, the longest wait does.
        for i in 1..=3 {
            g.output(t0 + ms(16 * i), &chunk);
        }
        assert_eq!(g.show(t0 + ms(60)), Show::At(t0 + BURST_MAX));
        assert_eq!(g.show(t0 + BURST_MAX), Show::Now);
        assert!(!g.is_holding());
    }

    #[test]
    fn a_burst_ends_when_output_stops() {
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        g.output(t0, &vec![b'x'; 4000]);
        g.output(t0 + ms(5), b"tail");
        assert_eq!(g.show(t0 + ms(20)), Show::At(t0 + ms(5) + BURST_QUIET));
        assert_eq!(g.show(t0 + ms(29)), Show::Now);
    }

    #[test]
    fn an_erase_waits_longer() {
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        g.output(t0, b"\x1b[H\x1b[2J");
        assert_eq!(g.show(t0 + ms(50)), Show::At(t0 + ERASED_QUIET));
        g.output(t0 + ms(70), b"half the screen");
        assert_eq!(g.show(t0 + ms(140)), Show::At(t0 + ms(70) + ERASED_QUIET));
        assert_eq!(g.show(t0 + ms(150)), Show::Now);
    }

    #[test]
    fn a_flood_turning_into_a_redraw_gets_the_longer_wait() {
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        g.output(t0, &vec![b'x'; 3000]);
        g.output(t0 + ms(10), b"\x1b[J");
        for i in 1..12 {
            g.output(t0 + ms(10 + 16 * i), b"more");
        }
        assert_eq!(g.deadline(), Some(t0 + ms(10) + ERASED_MAX));
    }

    #[test]
    fn a_keys_answer_is_never_held_for_an_erase() {
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        g.typed(t0);
        // A prompt redrawing itself right after a key.
        g.output(t0 + ms(40), b"\x1b[2J$ ");
        assert_eq!(g.show(t0 + ms(40)), Show::Now);
        // Through a slow link: the first output within 3 s is the answer too.
        g.typed(t0 + ms(1000));
        g.output(t0 + ms(2500), b"\x1b[J");
        assert_eq!(g.show(t0 + ms(2500)), Show::Now);
        // But only the first: the next erase is held.
        g.output(t0 + ms(2600), b"\x1b[J");
        assert!(matches!(g.show(t0 + ms(2600)), Show::At(_)));
    }

    #[test]
    fn reset_lets_go() {
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        g.output(t0, b"\x1b[2J");
        g.reset();
        assert_eq!(g.show(t0), Show::Now);
    }

    #[test]
    fn an_erase_cut_between_reads_still_waits() {
        // Cut after Esc, after `ESC [`, after `ESC [ 2`, and a byte at a time.
        for (a, b) in [(&b"prompt\x1b"[..], &b"[2J redraw"[..]), (b"prompt\x1b[", b"2J redraw"), (b"prompt\x1b[2", b"J redraw"), (b"x\x1b[", b"J")] {
            let t0 = Instant::now();
            let mut g = RenderGate::new();
            g.output(t0, a);
            assert_eq!(g.show(t0), Show::Now);
            g.output(t0 + ms(5), b);
            assert_eq!(g.show(t0 + ms(5)), Show::At(t0 + ms(5) + ERASED_QUIET), "{a:?} + {b:?}");
        }
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        for &byte in b"\x1b[3J" {
            g.output(t0, &[byte]);
        }
        assert!(g.is_holding());
        // Not an erase: `ESC [ 1 J`, `ESC [ K`, and an Esc that isn't at the end.
        for (a, b) in [(&b"x\x1b[1"[..], &b"J"[..]), (b"x\x1b[", b"K"), (b"\x1b[0m done", b"J")] {
            let mut g = RenderGate::new();
            g.output(t0, a);
            g.output(t0, b);
            assert!(!g.is_holding(), "{a:?} + {b:?}");
        }
        // The connection dropped in between: the next connection's output starts afresh.
        let mut g = RenderGate::new();
        g.output(t0, b"\x1b[");
        g.reset();
        g.output(t0, b"2J");
        assert!(!g.is_holding());
    }

    #[test]
    fn erase_detection_and_sync_marks() {
        assert!(erases_screen(b"x\x1b[J"));
        assert!(erases_screen(b"\x1b[0J"));
        assert!(erases_screen(b"\x1b[3J"));
        assert!(!erases_screen(b"\x1b[1J"));
        assert!(!erases_screen(b"\x1b[K"));
        assert!(!erases_screen(b"\x1b[2"));
        assert_eq!(content_len(b"\x1b[?2026habc\x1b[?2026l"), 3);
        // tmux's update marks alone aren't output: a pending answer stays pending.
        let t0 = Instant::now();
        let mut g = RenderGate::new();
        g.typed(t0);
        g.output(t0 + ms(500), b"\x1b[?2026h\x1b[?2026l");
        g.output(t0 + ms(900), b"\x1b[2J");
        assert_eq!(g.show(t0 + ms(900)), Show::Now);
    }
}
