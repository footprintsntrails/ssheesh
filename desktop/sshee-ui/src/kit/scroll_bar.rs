//! Scroll bars for every scroll viewport: `viewport.scroll_bars(t)`, the last call on a viewport
//! made with [`El::scroll`], [`El::scroll_x`] or [`El::scroll_xy`] (virtual rows included).
//!
//! The bars look like the terminal's (`terminal/scrollbar.rs`), so the app reads as one system:
//! a thin rounded thumb, 6 points across and 8 under the pointer, in the text colour at 16%
//! (28% under the pointer, 36% held). They are overlays, as on macOS and in a browser: they take
//! no room from the content, show while it scrolls and while the pointer is over the area, and
//! fade a second after both stop (or stay, when the system says to always show scroll bars).
//! A vertical bar runs down the right edge, a horizontal one along the bottom, each only while
//! the content overflows that way; with both, the vertical one stops short of the corner.
//! Dragging the thumb scrolls; pressing the track above or below it pages toward the press, again
//! and again while held (or jumps there, under the system's "jump to the spot" preference and
//! Option). Under Reduce Motion the bars show and hide without a fade.
//!
//! Everything runs through live values that the viewport's `on_scroll_offset` sets in the frame
//! that scrolls, so scrolling never runs the view. A viewport's bars keep their state between
//! views in a table keyed by the viewport's key.
//!
//! Adapted from rune-kit's `scroll_bar` (https://github.com/anomalyco/rune, MIT, at the rev
//! `desktop/Cargo.toml` pins): its wrapping of the viewport, its visibility machine and its
//! press handling, cut down to this one look and given a horizontal bar beside the vertical one.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::Location;
use std::rc::Rc;
use std::time::Duration;

use rune::{el, Color, El, Live, Phase, PointerEvent, ScrollPos, Spring};

use crate::theme::Theme;

mod visibility;

use visibility::{BarState, Event as BarEvent, Follow, Opacity};

/// The strip along the edge that takes presses (and widens the thumb under the pointer).
pub const STRIP: f32 = 12.0;
/// The thumb's gap from the viewport's edge.
const EDGE: f32 = 3.0;
/// The thumb across, at rest and under the pointer: the terminal's.
const THUMB: f32 = 6.0;
const THUMB_HOT: f32 = 8.0;
/// The track's gap from the viewport's ends.
const INSET: f32 = 4.0;
/// The shortest the thumb gets, however long the content: the terminal's.
pub const MIN_THUMB: f32 = 24.0;
/// How long a bar stays up after the last scroll, a release, or the pointer leaving.
const HOLD: Duration = Duration::from_millis(1000);
const ENTER: Spring = Spring::new(0.14, 0.0);
const LEAVE: Spring = Spring::new(0.28, 0.0);
/// The thumb widening and changing colour under the pointer, and gliding to a new length.
const WIDEN: Spring = Spring::new(0.12, 0.0);
/// How long a held track press waits before paging again, then how often it pages.
const PAGE_DELAY: Duration = Duration::from_millis(300);
const PAGE_EVERY: Duration = Duration::from_millis(60);
/// A page sideways: as Page Down's is of the height.
const PAGE: f32 = 0.9;
/// The most bars kept between views; past it they're all dropped, and each comes back on its
/// viewport's next commit.
const KEPT: usize = 256;

thread_local! {
    static CALM: Cell<bool> = const { Cell::new(false) };
    static BARS: RefCell<HashMap<(u64, String, bool), Rc<Bar>>> = RefCell::new(HashMap::new());
}

/// Whether the window reduces motion (the system's Reduce Motion, or Settings → Appearance →
/// Animations off): the bars then show and hide at once. The window's view says so each time it
/// runs.
pub fn set_calm(calm: bool) {
    CALM.set(calm);
}

/// Gives a scroll viewport its scroll bars. See the module's notes.
pub trait ScrollBars {
    /// `self`, a scroll viewport, wrapped in an area that draws its bars over it. The area takes
    /// the viewport's size and place in its parent, and keeps its key, so `Tx::scroll_*` and
    /// tests find it as before. Call it last, after the viewport's children.
    #[track_caller]
    fn scroll_bars(self, t: &Theme) -> El;
}

impl ScrollBars for El {
    #[track_caller]
    fn scroll_bars(self, t: &Theme) -> El {
        let site = Location::caller();
        let (area, key, outer, viewport) = self.wrap_scroll_area((site.file(), site.line(), site.column()));
        attach(Look::of(t), &area, &key, outer, viewport)
    }
}

/// How a bar's thumb looks, and whether it fades.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    rest: Color,
    hot: Color,
    held: Color,
    calm: bool,
}

impl Look {
    fn of(t: &Theme) -> Look {
        Look { rest: t.l1.alpha(0.16), hot: t.l1.alpha(0.28), held: t.l1.alpha(0.36), calm: CALM.get() }
    }

    /// The spring a change of `normal` runs on: none under Reduce Motion.
    fn spring(&self, normal: Spring) -> Spring {
        if self.calm { Spring::NONE } else { normal }
    }
}

/// Which way a bar runs: down the right edge (`y`) or along the bottom (`x`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Axis {
    x: bool,
}

impl Axis {
    const Y: Axis = Axis { x: false };
    const X: Axis = Axis { x: true };

    /// The index of the axis the thumb runs along in a point or size: 0 sideways, 1 down.
    fn along(self) -> usize {
        usize::from(!self.x)
    }

    /// `[x, y, w, h]` for a rect laid out down a vertical bar (`across` from its left, `along`
    /// from its top), turned to run sideways on a horizontal one.
    fn turned(self, [across, along, thick, long]: [f32; 4]) -> [f32; 4] {
        if self.x { [along, across, long, thick] } else { [across, along, thick, long] }
    }
}

/// The track's length on `axis` for a viewport at `at`, and whether a bar shows on it at all. A
/// viewport that overflows both ways leaves the corner to the horizontal bar.
fn track_len(at: ScrollPos, axis: Axis) -> (f32, bool) {
    let a = axis.along();
    let corner = if !axis.x && at.max[0] > 0.5 { STRIP } else { 0.0 };
    let len = at.viewport[a] - 2.0 * INSET - corner;
    (len, at.max[a] > 0.5 && len >= MIN_THUMB)
}

/// The thumb's start and length on a track `track` points long, for a viewport at `at`: the
/// share of the content in view, never shorter than [`MIN_THUMB`] (or the track).
fn thumb_along(at: ScrollPos, track: f32, axis: Axis) -> (f32, f32) {
    let a = axis.along();
    let (shown, max) = (at.viewport[a], at.max[a]);
    let track = track.max(0.0);
    let len = (track * shown / (shown + max).max(1.0)).max(MIN_THUMB).min(track);
    let top = if max > 0.0 { (track - len) * (at.offset[a] / max).clamp(0.0, 1.0) } else { 0.0 };
    (top, len)
}

/// The content offset that puts the thumb's start at `top` on the track, clamped to the ends.
fn offset_along(at: ScrollPos, track: f32, top: f32, axis: Axis) -> f32 {
    let a = axis.along();
    let travel = track - thumb_along(at, track, axis).1;
    if travel <= 0.0 {
        return at.offset[a];
    }
    (top / travel * at.max[a]).clamp(0.0, at.max[a])
}

/// The strip's rect over a viewport at `at`, `len` long, in the viewport's points.
fn track_rect(at: ScrollPos, axis: Axis, len: f32) -> [f32; 4] {
    let across = at.viewport[1 - axis.along()];
    axis.turned([across - STRIP, INSET, STRIP, len])
}

/// Where a thumb at `top`, `len` long, runs on the track: as wide as it gets, against the edge.
fn slot_rect(axis: Axis, top: f32, len: f32) -> [f32; 4] {
    axis.turned([STRIP - EDGE - THUMB_HOT, top, THUMB_HOT, len])
}

/// The thumb `across` points wide and `len` long, in its slot, against the edge.
fn thumb_rect(axis: Axis, across: f32, len: f32) -> [f32; 4] {
    axis.turned([THUMB_HOT - across, 0.0, across, len])
}

/// What a press on the bar holds: the thumb, caught `grab` points from its start, or the track,
/// paging toward the pointer (-1 back, 1 on).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Press {
    Thumb { grab: f32 },
    Pages { toward: i32 },
}

/// A press held from its press to its release, which can be lost (the bar unmounts, or a modal
/// takes the pointer) so no `Phase::Up` comes. The runtime hands every move of a held pointer to
/// the drag and to the area's pointer handler, in either order, so one move the drag hasn't heard
/// yet is normal; a second at another point means the drag is gone (rune-kit's `Grip`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grip {
    press: Press,
    /// The last point the drag heard, and a pointer move at another point it hasn't heard yet.
    heard: [f32; 2],
    unheard: Option<[f32; 2]>,
}

impl Grip {
    fn new(press: Press, at: [f32; 2]) -> Grip {
        Grip { press, heard: at, unheard: None }
    }

    /// The drag's own move to `at`.
    fn moved(self, at: [f32; 2]) -> Grip {
        Grip { heard: at, unheard: None, ..self }
    }

    /// The area's pointer handler saw the pointer at `at`: the grip as it now stands, or `None`
    /// when that shows its release was lost.
    fn pointer(self, at: [f32; 2]) -> Option<Grip> {
        if self.heard == at {
            return Some(self);
        }
        match self.unheard {
            Some(u) if u != at => None,
            _ => Some(Grip { unheard: Some(at), ..self }),
        }
    }
}

/// One bar: where its viewport stands, its visibility, the pointer and a press, and the live
/// rects and opacity that follow them.
struct Bar {
    axis: Axis,
    pos: Cell<ScrollPos>,
    state: Cell<BarState>,
    /// A press on the thumb or track, until its release.
    press: Cell<Option<Grip>>,
    /// The pointer is over the area.
    near: Cell<bool>,
    /// The pointer is over the strip.
    over: Cell<bool>,
    look: Cell<Option<Look>>,
    last_now: Cell<f64>,
    track: Live<[f32; 4]>,
    slot: Live<[f32; 4]>,
    thumb: Live<[f32; 4]>,
    fill: Live<Color>,
    opacity: Live<f32>,
}

fn kept(area: &str, axis: Axis) -> Rc<Bar> {
    let id = (rune::current_runtime_id(), area.to_owned(), axis.x);
    BARS.with(|bars| {
        let mut bars = bars.borrow_mut();
        if bars.len() >= KEPT && !bars.contains_key(&id) {
            bars.clear();
        }
        bars.entry(id)
            .or_insert_with(|| {
                Rc::new(Bar {
                    axis,
                    pos: Cell::new(ScrollPos::default()),
                    state: Cell::new(BarState::Uninit),
                    press: Cell::new(None),
                    near: Cell::new(false),
                    over: Cell::new(false),
                    look: Cell::new(None),
                    last_now: Cell::new(0.0),
                    track: Live::new([0.0; 4]),
                    slot: Live::new([0.0; 4]),
                    thumb: Live::new([0.0; 4]),
                    fill: Live::new(Color::TRANSPARENT),
                    opacity: Live::new(1.0),
                })
            })
            .clone()
    })
}

impl Bar {
    fn look(&self) -> Look {
        self.look.get().unwrap_or(Look { rest: Color::TRANSPARENT, hot: Color::TRANSPARENT, held: Color::TRANSPARENT, calm: false })
    }

    fn pressed(&self) -> bool {
        self.press.get().is_some()
    }

    fn wide(&self) -> bool {
        self.over.get() || self.pressed()
    }

    fn thumb_fill(&self) -> Color {
        let look = self.look();
        if self.pressed() {
            look.held
        } else if self.over.get() {
            look.hot
        } else {
            look.rest
        }
    }

    fn thumb_len(&self) -> f32 {
        self.thumb.get()[if self.axis.x { 2 } else { 3 }]
    }

    fn thumb_now(&self, len: f32) -> [f32; 4] {
        thumb_rect(self.axis, if self.wide() { THUMB_HOT } else { THUMB }, len)
    }

    /// Widens or narrows the thumb, and changes its colour, after the pointer or a press changed.
    fn widen(&self) {
        self.thumb.set(self.thumb_now(self.thumb_len()));
        self.fill.set(self.thumb_fill());
    }

    /// Takes the look a view built the bar with; a new one lands at once.
    fn restyle(&self, look: Look) {
        if self.look.replace(Some(look)) == Some(look) {
            return;
        }
        self.fill.snap(self.thumb_fill());
        let at = self.pos.get();
        let (len, shows) = track_len(at, self.axis);
        let (top, h) = thumb_along(at, len, self.axis);
        let h = if shows { h } else { 0.0 };
        self.track.snap(track_rect(at, self.axis, if shows { len } else { 0.0 }));
        self.slot.snap(slot_rect(self.axis, top, h));
        self.thumb.snap(self.thumb_now(h));
    }

    /// Steps what the bar shows and carries out the opacity change; says whether the thumb
    /// glides to its new length, and whether a glide it was in was cut.
    fn visibility(&self, event: BarEvent) -> (bool, bool) {
        let mut state = self.state.get();
        let effects = visibility::step(&mut state, event);
        self.state.set(state);
        let (mut glides, mut cut) = (false, false);
        for effect in effects {
            match effect {
                visibility::Effect::Glide { glides: g, cut: c } => (glides, cut) = (g, c),
                visibility::Effect::Opacity(Opacity::Set(o)) => {
                    self.opacity.set(o);
                }
                visibility::Effect::Opacity(Opacity::Snap(o)) => {
                    self.opacity.snap(o);
                }
                visibility::Effect::Opacity(Opacity::Pulse { hold }) => {
                    self.opacity.pulse(1.0, 0.0, hold);
                }
            }
        }
        (glides, cut)
    }

    /// The pointer or a press holds an overlay bar up.
    fn hold(&self) {
        self.visibility(BarEvent::Hold);
    }

    /// Once neither holds it, an overlay bar fades out after [`HOLD`].
    fn let_go(&self) {
        self.visibility(BarEvent::LetGo { pressed: self.pressed(), revealed: self.near.get(), now: self.last_now.get(), hold: HOLD });
    }

    /// A press ended (or was lost): the thumb narrows and an overlay bar may fade.
    fn released(&self) {
        self.widen();
        self.let_go();
    }

    /// The pointer at `p` in the area (its points), or gone from it.
    fn point(&self, p: Option<PointerEvent>) {
        if let (Some(p), Some(grip)) = (p, self.press.get()) {
            match grip.pointer([p.window_x, p.window_y]) {
                Some(grip) => self.press.set(Some(grip)),
                None => {
                    self.press.set(None);
                    self.released();
                }
            }
        }
        let at = self.pos.get();
        let (len, shows) = track_len(at, self.axis);
        let a = self.axis.along();
        let near = shows && p.is_some();
        let over = near
            && p.is_some_and(|p| {
                let (across, along) = if self.axis.x { (p.y, p.x) } else { (p.x, p.y) };
                let edge = at.viewport[1 - a];
                (edge - STRIP..=edge).contains(&across) && (INSET..=INSET + len).contains(&along)
            });
        if self.over.replace(over) != over {
            self.widen();
        }
        let was = self.near.replace(near);
        match (was, near) {
            (false, true) => self.hold(),
            (true, false) => self.let_go(),
            _ => {}
        }
    }

    /// Follows the viewport to `at`, from its `on_scroll_offset`, at the size it's drawn at.
    fn follow(&self, mut at: ScrollPos, overlay: bool) {
        at.viewport = at.drawn;
        let prev = self.pos.replace(at);
        let (scrolled, instant, now) = rune::scroll_watch_state();
        self.last_now.set(now);
        let (len, shows) = track_len(at, self.axis);
        let a = self.axis.along();
        self.track.set(track_rect(at, self.axis, if shows { len } else { 0.0 }));
        let (top, h) = thumb_along(at, len, self.axis);
        let h = if shows { h } else { 0.0 };
        let resized = (prev.viewport[a] - at.viewport[a]).abs() > 0.5;
        let lengthened = !resized && (prev.max[a] - at.max[a]).abs() > 0.5;
        let extent_changed = (prev.max[a] - at.max[a]).abs() > 0.5 || resized;
        let (glides, cut) = self.visibility(BarEvent::Follow(Follow {
            shows,
            overlay,
            scrolled,
            instant,
            resized,
            lengthened,
            extent_changed,
            pressed: self.pressed(),
            revealed: self.near.get(),
            now,
            hold: HOLD,
        }));
        let to = |live: &Live<[f32; 4]>, rect| if glides { live.set(rect) } else { live.snap(rect) };
        to(&self.slot, slot_rect(self.axis, top, h));
        // A scroll leaves the thumb's width alone, so it doesn't cut a widening short.
        if cut || self.thumb_len() != h {
            to(&self.thumb, self.thumb_now(h));
        }
    }

    /// The thumb or track pressed, dragged or released; `target` is the viewport's path, and
    /// `jumps` says a track press jumps to the spot rather than paging toward it.
    fn drag(&self, target: &str, jumps: bool, d: &mut rune::Drag, tx: &mut rune::Tx) {
        tx.live_only();
        let at = self.pos.get();
        let (len, shows) = track_len(at, self.axis);
        if !shows {
            if self.press.take().is_some() {
                self.widen();
            }
            return;
        }
        let (top, h) = thumb_along(at, len, self.axis);
        let y = if self.axis.x { d.x } else { d.y };
        let press = match d.phase {
            Phase::Down => {
                let on_thumb = (top..=top + h).contains(&y);
                // A track press pages toward itself, or with Option (or the preference) jumps
                // there. A press on the thumb holds it where it was caught; a jump centres it.
                let press = if !on_thumb && jumps == tx.mods().alt {
                    Press::Pages { toward: if y < top { -1 } else { 1 } }
                } else {
                    Press::Thumb { grab: if on_thumb { y - top } else { h / 2.0 } }
                };
                self.press.set(Some(Grip::new(press, d.window_point())));
                self.hold();
                self.widen();
                if let Press::Pages { toward } = press {
                    self.page(target, toward, tx);
                    tx.drag_repeat_after(PAGE_DELAY);
                    return;
                }
                press
            }
            Phase::Up => {
                if self.press.take().is_some() {
                    self.released();
                }
                return;
            }
            Phase::Move => match self.press.get() {
                Some(grip) => {
                    self.press.set(Some(grip.moved(d.window_point())));
                    grip.press
                }
                None => return,
            },
        };
        match press {
            Press::Thumb { grab } => {
                let to = offset_along(at, len, y - grab, self.axis);
                if self.axis.x {
                    tx.scroll_to_instant(target, to, at.offset[1]);
                } else {
                    tx.scroll_to_instant(target, at.offset[0], to);
                }
            }
            // Paging goes on while held, until the thumb reaches the pointer.
            Press::Pages { toward } => {
                if tx.drag_repeat() {
                    tx.drag_repeat_after(PAGE_EVERY);
                    if (toward < 0 && y < top) || (toward > 0 && y > top + h) {
                        self.page(target, toward, tx);
                    }
                }
            }
        }
    }

    /// A page toward `toward`: Page Up or Down's, or as much sideways.
    fn page(&self, target: &str, toward: i32, tx: &mut rune::Tx) {
        let at = self.pos.get();
        if self.axis.x {
            let x = (at.offset[0] + toward as f32 * PAGE * at.viewport[0]).clamp(0.0, at.max[0]);
            tx.scroll_to_instant(target, x, at.offset[1]);
        } else {
            tx.scroll_page(target, toward);
        }
    }

    /// The bar for the view: a strip over the viewport's edge with the thumb in it, a scroll bar
    /// for assistive technology while there's somewhere to scroll.
    fn el(self: &Rc<Bar>, target: &str, jumps: bool) -> El {
        let look = self.look();
        let drag = self.clone();
        let target = target.to_owned();
        let thumb = el()
            .key("thumb")
            .absolute(0.0, 0.0)
            .live_rect(self.thumb.smooth(look.spring(WIDEN)))
            .radius(THUMB_HOT / 2.0)
            .live_fill(self.fill.smooth(look.spring(WIDEN)));
        let slot = el().key("thumb-slot").absolute(0.0, 0.0).live_rect(self.slot.smooth(look.spring(WIDEN))).child(thumb);
        let track = el()
            .key(if self.axis.x { "scroll-bar-x" } else { "scroll-bar" })
            .absolute(0.0, 0.0)
            .live_rect(&self.track)
            .live_opacity(self.opacity.smooth(look.spring(ENTER)).rest(look.spring(LEAVE)))
            .cursor(rune::Cursor::Default)
            .on_drag_any(move |d, tx| drag.drag(&target, jumps, d, tx))
            .child(slot);
        let at = self.pos.get();
        let a = self.axis.along();
        if track_len(at, self.axis).1 {
            track
                .role("scrollbar")
                .label_static(if self.axis.x { "Scroll sideways" } else { "Scroll" })
                .range_value(f64::from(at.offset[a]), 0.0, f64::from(at.max[a]))
        } else {
            track.a11y_hidden()
        }
    }
}

fn attach(look: Look, area: &str, key: &str, outer: El, viewport: El) -> El {
    let overlay = rune::system_scroll_bars().is_overlay(rune::system_scroller_overlay());
    let jumps = rune::system_scroller_jumps();
    let target = format!("{area}/{key}");
    let (down, across) = (kept(area, Axis::Y), kept(area, Axis::X));
    down.restyle(look);
    across.restyle(look);
    let (watch_down, watch_across) = (down.clone(), across.clone());
    let (moved_down, moved_across) = (down.clone(), across.clone());
    let (left_down, left_across) = (down.clone(), across.clone());
    let bars = [down.el(&target, jumps), across.el(&target, jumps)];
    outer
        .on_pointer_move_any(move |p, tx| {
            tx.live_only();
            moved_down.point(Some(p));
            moved_across.point(Some(p));
        })
        .on_hover_change_any(move |inside, tx| {
            tx.live_only();
            if !inside {
                left_down.point(None);
                left_across.point(None);
            }
        })
        .child(viewport.chain_scroll_offset(move |at| {
            watch_down.follow(at, overlay);
            watch_across.follow(at, overlay);
        }))
        .children(bars)
}

#[cfg(test)]
mod tests;
