//! Where a terminal is scrolled to, for its scrollbar.
//!
//! In a server terminal the history lives in tmux, which reports its scroll position through the
//! window title (`sshee-scroll 1 100 480 24 2000`, see `SCROLL_TITLE` in sshee-core's ssh.rs). A
//! plain shell's history is the engine's own. Ported from 0.10's scroll.ts and the scrollbar
//! parts of terminals.ts.

/// tmux's report: in copy mode or not, lines scrolled up, lines of history, the pane's height,
/// the history's limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TmuxScroll {
    pub in_mode: bool,
    pub position: u32,
    pub history: u32,
    pub height: u32,
    pub limit: u32,
}

/// tmux leaves scrolling back on this key (bound in sshee-core's `durable_command`). Typing
/// while scrolled back sends it first, so the keys go to the prompt.
pub const LEAVE_SCROLL: &[u8] = b"\x1b[9006~";

/// Reads tmux's report from a title. Out of copy mode tmux leaves the position out.
///
/// Any program can set a title, so the numbers are taken only when they could be tmux's: a
/// pane at least a row high and no higher than a terminal can be. The sums made from them
/// never overflow either way.
pub fn parse_scroll_title(title: &str) -> Option<TmuxScroll> {
    let mut words = title.split_whitespace();
    if words.next()? != "sshee-scroll" {
        return None;
    }
    let n: Vec<u32> = words.map(|w| w.parse::<u32>().ok().filter(|_| w.bytes().all(|b| b.is_ascii_digit()))).collect::<Option<_>>()?;
    let report = match n[..] {
        [mode, position, history, height, limit] => TmuxScroll { in_mode: mode == 1, position, history, height, limit },
        [mode, history, height, limit] => TmuxScroll { in_mode: mode == 1, position: 0, history, height, limit },
        _ => return None,
    };
    (1..=u16::MAX as u32).contains(&report.height).then_some(report)
}

/// What a scrollbar shows: lines in all, the first one in view, lines on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScrollView {
    pub total: usize,
    pub top: usize,
    pub rows: usize,
}

/// tmux's history as a scrollbar sees it.
pub fn tmux_view(s: &TmuxScroll) -> ScrollView {
    ScrollView {
        total: (s.history as usize).saturating_add(s.height as usize),
        top: (s.history - s.position.min(s.history)) as usize,
        rows: s.height as usize,
    }
}

/// The engine's own history: `history` lines above the screen, the view `display_offset` lines
/// up from the bottom, `rows` on screen.
pub fn local_view(history: usize, display_offset: usize, rows: usize) -> ScrollView {
    ScrollView { total: history + rows, top: history.saturating_sub(display_offset), rows }
}

/// JavaScript's `Math.round`: halves go up, also below zero.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// The tmux position (lines scrolled up) that puts line `top` first in view.
pub fn tmux_position(s: &TmuxScroll, top: f64) -> u32 {
    js_round(s.history as f64 - top).clamp(0.0, s.history as f64) as u32
}

/// A scrollbar thumb: its length and where it starts, in the track's units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thumb {
    pub size: f64,
    pub offset: f64,
}

/// The shortest a thumb gets, so it can still be grabbed (pixels).
pub const MIN_THUMB: f64 = 24.0;

/// Thumb size and place on a `track` this long; never shorter than `min`.
pub fn thumb(view: &ScrollView, track: f64, min: f64) -> Thumb {
    if view.total <= view.rows || track <= 0.0 {
        return Thumb { size: track, offset: 0.0 };
    }
    let size = min.max(track * view.rows as f64 / view.total as f64);
    let room = track - size;
    let scrollable = (view.total - view.rows) as f64;
    Thumb { size, offset: room * (view.top as f64 / scrollable).clamp(0.0, 1.0) }
}

/// The first line in view for a thumb `size` long dragged to `offset`.
pub fn top_for_offset(view: &ScrollView, track: f64, size: f64, offset: f64) -> usize {
    let room = track - size;
    if room <= 0.0 {
        return 0;
    }
    js_round(offset.clamp(0.0, room) / room * view.total.saturating_sub(view.rows) as f64) as usize
}

/// What to do to scroll somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollTo {
    /// Scroll the engine's own history so `display_offset` is this.
    Local { display_offset: usize },
    /// Ask tmux to scroll to this position (sshee-core's `tmux_scroll`); the scrollbar already shows it.
    Tmux { position: u32 },
}

/// Which history a terminal scrolls through, tmux's or its own, and where tmux is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrollState {
    tmux: Option<TmuxScroll>,
}

impl ScrollState {
    pub fn new() -> Self {
        Self::default()
    }

    /// tmux's last report, when the history is tmux's.
    pub fn tmux(&self) -> Option<TmuxScroll> {
        self.tmux
    }

    /// A new report, or `None` when the history is the terminal's own again (a new shell
    /// without tmux after a reconnect).
    pub fn set_tmux(&mut self, s: Option<TmuxScroll>) {
        self.tmux = s;
    }

    /// The window title changed: true when it was tmux's report (taken in).
    pub fn on_title(&mut self, title: &str) -> bool {
        match parse_scroll_title(title) {
            Some(s) => {
                self.tmux = Some(s);
                true
            }
            None => false,
        }
    }

    /// tmux reports only when it redraws, so lines going up into its history meanwhile are
    /// counted here: `lines` line feeds made on the bottom row.
    pub fn lines_scrolled_out(&mut self, lines: u32) {
        if let Some(t) = self.tmux.as_mut() {
            if !t.in_mode {
                t.history = t.history.saturating_add(lines).min(t.limit.max(t.history));
            }
        }
    }

    /// Typing while scrolled back went to the prompt: tmux left copy mode.
    pub fn left_copy_mode(&mut self) {
        if let Some(t) = self.tmux.as_mut() {
            t.in_mode = false;
            t.position = 0;
        }
    }

    /// Scrolled back in tmux, where a typed key should send [`LEAVE_SCROLL`] first.
    pub fn in_copy_mode(&self) -> bool {
        self.tmux.is_some_and(|t| t.in_mode)
    }

    /// What the scrollbar shows: tmux's history when there is one, else `local`.
    pub fn view(&self, local: ScrollView) -> ScrollView {
        self.tmux.as_ref().map(tmux_view).unwrap_or(local)
    }

    /// Scrolls so line `top` is first in view. For tmux the scrollbar moves at once and the
    /// caller asks tmux (see [`TmuxAsks`]).
    pub fn scroll_to(&mut self, top: f64, local: ScrollView) -> ScrollTo {
        match self.tmux.as_mut() {
            None => {
                let history = local.total.saturating_sub(local.rows);
                let top = js_round(top).max(0.0) as usize;
                ScrollTo::Local { display_offset: history.saturating_sub(top.min(history)) }
            }
            Some(t) => {
                let position = tmux_position(t, top);
                t.in_mode = position > 0;
                t.position = position;
                ScrollTo::Tmux { position }
            }
        }
    }
}

/// One tmux scroll at a time: while one is on its way only the latest wish waits, so a drag of
/// the scrollbar doesn't queue up a request per pixel. `None` asks where tmux is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TmuxAsks {
    want: Option<Option<u32>>,
    busy: bool,
}

impl TmuxAsks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wants tmux at `position` (or its report). Returns what to send now, if nothing is out.
    pub fn ask(&mut self, position: Option<u32>) -> Option<Option<u32>> {
        self.want = Some(position);
        if self.busy {
            return None;
        }
        self.busy = true;
        self.want.take()
    }

    /// The one out came back. Returns whether its answer should be shown (not when a newer wish
    /// is waiting, which the scrollbar already shows) and what to send next, if anything.
    pub fn answered(&mut self) -> (bool, Option<Option<u32>>) {
        let show = self.want.is_none();
        let next = self.want.take();
        if next.is_none() {
            self.busy = false;
        }
        (show, next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmux(in_mode: bool, position: u32, history: u32, height: u32, limit: u32) -> TmuxScroll {
        TmuxScroll { in_mode, position, history, height, limit }
    }

    #[test]
    fn reads_tmux_titles() {
        assert_eq!(parse_scroll_title("sshee-scroll 1 100 480 24 2000"), Some(tmux(true, 100, 480, 24, 2000)));
        // Out of copy mode the position is empty.
        assert_eq!(parse_scroll_title("  sshee-scroll 0 480 24 2000 "), Some(tmux(false, 0, 480, 24, 2000)));
        assert_eq!(parse_scroll_title("sshee-scroll 1 2 3"), None);
        assert_eq!(parse_scroll_title("sshee-scroll 1 -2 3 4"), None);
        assert_eq!(parse_scroll_title("sshee-scroll 1 2.5 3 4"), None);
        assert_eq!(parse_scroll_title("vim README.md"), None);
        assert_eq!(parse_scroll_title(""), None);
        // Not a pane tmux could have: no rows, or more than a terminal can show.
        assert_eq!(parse_scroll_title("sshee-scroll 0 480 0 2000"), None);
        assert_eq!(parse_scroll_title("sshee-scroll 0 4294967295 4294967295 1"), None);
    }

    #[test]
    fn numbers_from_any_program_never_overflow() {
        // A title any program can print: the largest numbers there are.
        let s = parse_scroll_title("sshee-scroll 0 4294967295 4 4294967295").unwrap();
        let view = tmux_view(&s);
        assert_eq!(view, ScrollView { total: 4_294_967_299, top: 4_294_967_295, rows: 4 });
        assert!(view.top <= view.total);
        let mut st = ScrollState::new();
        st.set_tmux(Some(s));
        st.lines_scrolled_out(u32::MAX);
        assert_eq!(st.tmux().unwrap().history, u32::MAX);
        let max = tmux(true, u32::MAX, u32::MAX, u16::MAX as u32, u32::MAX);
        assert_eq!(tmux_view(&max).top, 0);
        assert_eq!(tmux_position(&max, 0.0), u32::MAX);
    }

    #[test]
    fn views() {
        assert_eq!(tmux_view(&tmux(true, 100, 480, 24, 2000)), ScrollView { total: 504, top: 380, rows: 24 });
        // A position past the history stops at its top.
        assert_eq!(tmux_view(&tmux(true, 900, 480, 24, 2000)).top, 0);
        assert_eq!(local_view(1000, 0, 40), ScrollView { total: 1040, top: 1000, rows: 40 });
        assert_eq!(local_view(1000, 30, 40).top, 970);
    }

    #[test]
    fn positions_and_thumbs() {
        let s = tmux(false, 0, 480, 24, 2000);
        assert_eq!(tmux_position(&s, 380.0), 100);
        assert_eq!(tmux_position(&s, -5.0), 480);
        assert_eq!(tmux_position(&s, 900.0), 0);
        assert_eq!(tmux_position(&s, 379.5), 101);

        let view = ScrollView { total: 1000, top: 0, rows: 100 };
        assert_eq!(thumb(&view, 500.0, MIN_THUMB), Thumb { size: 50.0, offset: 0.0 });
        let bottom = ScrollView { top: 900, ..view };
        assert_eq!(thumb(&bottom, 500.0, MIN_THUMB), Thumb { size: 50.0, offset: 450.0 });
        // A long history keeps the thumb grabbable.
        let long = ScrollView { total: 100_000, top: 0, rows: 40 };
        assert_eq!(thumb(&long, 500.0, MIN_THUMB).size, MIN_THUMB);
        // Nothing to scroll: the thumb fills the track.
        assert_eq!(thumb(&ScrollView { total: 20, top: 0, rows: 40 }, 300.0, MIN_THUMB), Thumb { size: 300.0, offset: 0.0 });

        assert_eq!(top_for_offset(&view, 500.0, 50.0, 225.0), 450);
        assert_eq!(top_for_offset(&view, 500.0, 50.0, 9999.0), 900);
        assert_eq!(top_for_offset(&view, 500.0, 500.0, 10.0), 0);
    }

    #[test]
    fn scroll_state_follows_tmux() {
        let mut st = ScrollState::new();
        let local = local_view(200, 0, 24);
        assert_eq!(st.view(local), local);
        assert_eq!(st.scroll_to(150.0, local), ScrollTo::Local { display_offset: 50 });
        assert_eq!(st.scroll_to(-3.0, local), ScrollTo::Local { display_offset: 200 });

        assert!(st.on_title("sshee-scroll 0 480 24 2000"));
        assert!(!st.on_title("htop"));
        assert_eq!(st.view(local), ScrollView { total: 504, top: 480, rows: 24 });
        // Output scrolling lines into tmux's history meanwhile.
        st.lines_scrolled_out(3);
        assert_eq!(st.tmux().unwrap().history, 483);
        assert_eq!(st.scroll_to(383.0, local), ScrollTo::Tmux { position: 100 });
        assert!(st.in_copy_mode());
        // Not counted in copy mode.
        st.lines_scrolled_out(3);
        assert_eq!(st.tmux().unwrap().history, 483);
        st.left_copy_mode();
        assert!(!st.in_copy_mode());
        assert_eq!(st.tmux().unwrap().position, 0);
        // Never past the limit.
        st.set_tmux(Some(tmux(false, 0, 1999, 24, 2000)));
        st.lines_scrolled_out(5);
        assert_eq!(st.tmux().unwrap().history, 2000);
        st.set_tmux(None);
        assert_eq!(st.view(local), local);
    }

    #[test]
    fn one_tmux_ask_at_a_time() {
        let mut asks = TmuxAsks::new();
        assert_eq!(asks.ask(Some(10)), Some(Some(10)));
        // While that one is out, wishes replace each other.
        assert_eq!(asks.ask(Some(20)), None);
        assert_eq!(asks.ask(Some(30)), None);
        // The answer to 10 isn't shown: 30 is what the scrollbar shows. 30 goes next.
        assert_eq!(asks.answered(), (false, Some(Some(30))));
        assert_eq!(asks.answered(), (true, None));
        // Idle again.
        assert_eq!(asks.ask(None), Some(None));
        assert_eq!(asks.answered(), (true, None));
    }
}
