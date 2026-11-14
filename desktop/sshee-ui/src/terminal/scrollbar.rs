//! Scrolling a terminal's history: the scroll bar over its right edge, Shift+Page Up and Page
//! Down, and "Jump to the bottom" (0.10's `TerminalScrollbar`, `scrollTo` and `askTmux`).
//!
//! On a server with tmux the history is tmux's: the bar shows what tmux reports (in the
//! window title, and in its answer to a scroll), and scrolling asks tmux, one request at a time
//! (`sshee_term::scroll::TmuxAsks`), while the bar already shows where it's going. Without tmux
//! the history is the terminal's own. The model (`sshee_term::scroll`) knows which.

use std::time::Instant;

use rune::{text, Drag, El, Phase, Tx};
use sshee_term::scroll::{thumb, top_for_offset, ScrollTo, ScrollView, TmuxAsks, TmuxScroll, MIN_THUMB};

use super::overlays::{Ask, Note, Reply};
use super::pane::TermPane;
use crate::icons::{icon, Icon};
use crate::theme::{self, Theme};

/// The bar's room at the pane's right edge, and how far it stays from the top and bottom.
const TRACK: f32 = 14.0;
const INSET: f32 = 8.0;

/// The thumb at rest and under the pointer, in points across.
const THUMB: f32 = 6.0;
const THUMB_HOT: f32 = 8.0;

/// A pane's scrolling state.
#[derive(Default)]
pub struct Scroll {
    asks: TmuxAsks,
    /// A drag of the thumb is on.
    dragging: bool,
    /// While scrolled up: how long the history was when the view left the bottom, and the
    /// newest output then. Longer or newer since means there is something new below.
    left_bottom: Option<(usize, Option<Instant>)>,
    /// Something was printed below while the view is scrolled up.
    pub(crate) new_below: bool,
}

/// The thumb's length and place on a track `track` points long.
fn thumb_of(view: &ScrollView, track: f32) -> (f32, f32) {
    let t = thumb(view, f64::from(track), MIN_THUMB.min(f64::from(track)));
    (t.size as f32, t.offset as f32)
}

impl TermPane {
    /// What the bar shows: lines in all, the first in view, lines on screen.
    pub fn scroll_view(&self) -> ScrollView {
        self.view.borrow().scroll
    }

    /// The track's length: the pane's height less the bar's insets.
    fn track(&self) -> Option<f32> {
        self.area.map(|[_, h]| h - 2.0 * INSET).filter(|h| *h > 0.0)
    }

    /// Scrolls so line `top` is the first in view. tmux's history moves when tmux answers; the
    /// bar shows it at once.
    pub fn scroll_to(&mut self, top: f64, tx: &mut Tx) {
        if let ScrollTo::Tmux { position } = self.model.scroll_to(top.max(0.0)) {
            self.ask_tmux(Some(position));
        }
        self.pull(tx);
        tx.instant();
    }

    /// Scrolls by `lines`: up into the history when positive (Shift+Page Up).
    pub fn scroll_by(&mut self, lines: i32, tx: &mut Tx) {
        let v = self.model.scroll_view();
        self.scroll_to(v.top as f64 - f64::from(lines), tx);
    }

    /// Back to where output appears.
    pub fn scroll_to_bottom(&mut self, tx: &mut Tx) {
        let v = self.model.scroll_view();
        self.scroll_to(v.total as f64, tx);
    }

    /// Wants tmux at `position` (`None`: only its report). One request is out at a time; while
    /// it is, only the latest wish waits.
    fn ask_tmux(&mut self, position: Option<u32>) {
        if let Some(now) = self.over.scroll.asks.ask(position) {
            self.send_tmux_ask(now);
        }
    }

    fn send_tmux_ask(&mut self, position: Option<u32>) {
        let asked = self.ask(Ask::Scroll { position }, |reply| {
            Note::Scrolled(match reply {
                Some(Reply::Scroll(report)) => Some(report),
                _ => None,
            })
        });
        if !asked {
            // No tmux to ask: nothing is out.
            self.over.scroll.asks.answered();
        }
    }

    /// tmux answered a scroll (or couldn't): its report is shown unless a newer wish waits
    /// (mid-drag the bar already shows that one), and the wish that waited goes out.
    pub(crate) fn tmux_answered(&mut self, report: Option<TmuxScroll>, tx: &mut Tx) {
        let (show, next) = self.over.scroll.asks.answered();
        if let (true, Some(report), true) = (show, report, self.model.tmux().is_some()) {
            self.model.set_tmux(Some(report));
            self.pull(tx);
        }
        if let Some(next) = next {
            self.send_tmux_ask(next);
        }
    }

    /// Notes whether something new was printed below a view that is scrolled up. True when
    /// that changed.
    pub(crate) fn poll_scroll(&mut self) -> bool {
        let v = self.scroll_view();
        let up = v.top + v.rows < v.total;
        let s = &mut self.over.scroll;
        let before = s.new_below;
        if !up {
            s.left_bottom = None;
            s.new_below = false;
        } else {
            let (total, output) = *s.left_bottom.get_or_insert((v.total, self.last_output));
            s.new_below |= v.total > total || self.last_output > output;
        }
        s.new_below != before
    }

    /// A press or a drag on the bar: the thumb is dragged; a press above or below it moves a
    /// screen that way.
    pub(crate) fn scrollbar_drag(&mut self, d: &mut Drag, tx: &mut Tx) {
        let Some(track) = self.track() else { return };
        let v = self.model.scroll_view();
        let (size, offset) = thumb_of(&v, track);
        match d.phase {
            Phase::Down => {
                if d.y >= offset && d.y <= offset + size {
                    // Where on the thumb it was grabbed.
                    d.data = d.y - offset;
                    self.over.scroll.dragging = true;
                } else {
                    d.data = -1.0;
                    let page = v.rows.saturating_sub(1).max(1) as f64;
                    self.scroll_to(v.top as f64 + if d.y < offset { -page } else { page }, tx);
                }
            }
            Phase::Move if d.data >= 0.0 => {
                let top = top_for_offset(&v, f64::from(track), f64::from(size), f64::from(d.y - d.data));
                self.scroll_to(top as f64, tx);
            }
            Phase::Up => {
                self.over.scroll.dragging = false;
                // The keyboard goes back to the terminal.
                self.focus_terminal(tx);
                tx.instant();
            }
            Phase::Move => {}
        }
    }

    /// The pointer came over the bar: tmux says how much history there is, for an exact bar.
    pub(crate) fn scrollbar_hover(&mut self, over: bool) {
        if over && self.model.tmux().is_some() {
            self.ask_tmux(None);
        }
    }
}

/// The scroll bar, once there is history to scroll through.
pub(crate) fn bar<S: 'static>(pane: &TermPane, t: &'static Theme, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> Option<El> {
    let v = pane.scroll_view();
    let track = pane.track()?;
    if v.total <= v.rows {
        return None;
    }
    let (size, offset) = thumb_of(&v, track);
    let dragging = pane.over.scroll.dragging;
    // Two thumbs, one over the other: the quiet one at rest, the stronger one while the
    // pointer is over the bar or the thumb is held. Paint only: no view runs for a hover.
    let part = |key: &'static str, across: f32, ink: rune::Color| {
        rune::el().key(key).absolute(TRACK - 3.0 - across, offset).size(across, size).radius(across / 2.0).fill(ink).a11y_hidden()
    };
    let rest = part("thumb", THUMB, t.l1.alpha(0.16));
    let hot = part("thumb-hot", THUMB_HOT, t.l1.alpha(if dragging { 0.36 } else { 0.28 }));
    let (rest, hot) = if dragging { (rest.opacity(0.0), hot) } else { (rest.group_opacity(1.0, 0.0), hot.group_opacity(0.0, 1.0)) };
    Some(
        rune::el()
            .key("scroll")
            .pin(None, Some(INSET), Some(0.0), Some(INSET))
            .w(TRACK)
            .cursor(rune::Cursor::Default)
            .hover_group()
            .role("scrollbar")
            .label_static("Scroll back")
            .help("Drag to scroll back. Shift+Page Up and Page Down work too.")
            .on_drag(move |s: &mut S, d, tx| {
                if let Some(pane) = get(s) {
                    pane.scrollbar_drag(d, tx);
                }
            })
            .on_hover_change(move |s: &mut S, over, tx| {
                if let Some(pane) = get(s) {
                    pane.scrollbar_hover(over);
                }
                tx.live_only();
            })
            .child(rest)
            .child(hot),
    )
}

/// "Jump to the bottom": while the view is scrolled up and something new was printed below.
pub(crate) fn to_bottom<S: 'static>(pane: &TermPane, t: &'static Theme, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> Option<El> {
    if !pane.over.scroll.new_below {
        return None;
    }
    Some(
        rune::row()
            .key("to-bottom")
            .pin(None, None, Some(24.0), Some(14.0))
            .h(theme::CONTROL)
            .pl(8.0)
            .pr(12.0)
            .gap(6.0)
            .items_center()
            .radius(theme::RADIUS)
            .fill(t.pop)
            .border(1.0, t.rim)
            .shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.3))
            .cursor(rune::Cursor::Default)
            .hover(t.hov2)
            .press(t.down)
            .focusable(true)
            .role("button")
            .label_static("Jump to the bottom")
            .on_press_tx(move |s: &mut S, tx| {
                if let Some(pane) = get(s) {
                    pane.scroll_to_bottom(tx);
                    pane.focus_terminal(tx);
                }
            })
            .child(icon(Icon::ChevronDown, theme::ICON, t.l2on))
            .child(text("Jump to the bottom").font(theme::NOTE).color(t.l1).single_line()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_is_the_screens_share_of_the_history_and_never_too_short_to_grab() {
        // 24 rows of 240: a tenth of the track, at the bottom.
        let v = ScrollView { total: 240, top: 216, rows: 24 };
        assert_eq!(thumb_of(&v, 500.0), (50.0, 450.0));
        // A long history: the shortest thumb, and still at the right place.
        let v = ScrollView { total: 100_000, top: 0, rows: 24 };
        assert_eq!(thumb_of(&v, 500.0), (MIN_THUMB as f32, 0.0));
        // A pane too short for even that: the thumb is the track.
        assert_eq!(thumb_of(&v, 10.0).0, 10.0);
    }
}
