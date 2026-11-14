//! A pane's terminal: the engine, fed with output on the session's reader thread and read by
//! the screen layer on the UI thread.
//!
//! The model belongs to the pane, not to a connection: it outlives reconnects and sleep, and the
//! session's output sink holds an `Arc` to it. There is no PTY here; sshee-core owns that and
//! hands over bytes ([`Model::feed`]). What the terminal answers (device attributes, cursor
//! reports, colours) comes back as [`TermEvent::Reply`] for the app to write to the session.
//!
//! Locking follows Alacritty's own event loop: the reader takes alacritty's `FairMutex` for each
//! read it parses; the UI thread takes it too. How long a read takes depends on what its bytes
//! make the terminal do, not on how many there are (8 bytes of REP can write 65,535 cells), so
//! the reader parses a read in slices of bounded work ([`crate::osc::OutputGuard`]) and hands
//! the lock to a waiting UI thread between them: a flood of expensive sequences can't freeze
//! the screen. Events wait in a queue of their own, folded so a flood can't pile them up, and
//! go in under the lock, so they stay in the order they were parsed.
//!
//! ```text
//! reader thread:  bytes ─ guard ─ for each slice: OSC side pass ─ engine.feed ─ events queue ─ (screen's turn)
//!                 ─ waker()
//! UI thread:      due(now)? ─ frame(): changed rows copied out ─ present ─ draw
//! ```

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use alacritty_terminal::sync::FairMutex;
use parking_lot::MutexGuard;

use crate::engine::{
    AlacrittyEngine, AnswerColors, Cursor, Damage, Engine, EngineConfig, Modes, Row, Size, TermEvent, MAX_REPLIES,
};
use crate::gate::{RenderGate, Show};
use crate::input::{is_typing, without_keys};
use crate::osc::{OscEvent, OscScanner, OutputGuard, Progress, SLICE_WORK};
use crate::present::{Look, CONTEXT_ROWS};
use crate::scroll::{local_view, ScrollState, ScrollTo, ScrollView, TmuxScroll, LEAVE_SCROLL};
use crate::select::{CopyPresenter, SelectionSpan};

/// Turns off mouse, focus and paste reports. Whatever asked for them (tmux, an agent, vim) is
/// gone with the connection; left on, every click and drag meanwhile reached the next shell as
/// text ("32;29;29M"). tmux asks again once it's back.
pub const STOP_REPORTS: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1004l\x1b[?2004l";

/// The line shown when a reconnect landed in a new shell (no tmux to keep the old one).
pub const NEW_SHELL_LINE: &str = "\r\n\x1b[2m── reconnected · this is a new shell ──\x1b[0m\r\n";

/// The largest program copy (OSC 52) taken, in bytes once decoded. 0.10 refused base64 of
/// 4,000,000 characters or more, which is this much text.
pub const MAX_COPY: usize = 3_000_000;

/// The most notifications kept between two [`Model::take_events`]: the latest ones.
pub const MAX_NOTIFICATIONS: usize = 8;

/// The most work (see [`crate::osc::SLICE_WORK`]) a program's synchronized update (DEC 2026)
/// may hold back. Its end shows everything it held at once, under the lock; past this much it
/// shows as far as it got and the rest is drawn as it comes. A full redraw of a big screen is a
/// tenth of it.
pub const SYNC_WORK: usize = 16 * SLICE_WORK;

/// Called after output was parsed, from the reader thread: the UI wakes and asks [`Model::due`].
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Called instead of the [`Waker`] when output starts a hold (the gate keeps the old picture for
/// a burst): the UI should ask [`Model::due`] again at this time, and needn't draw anything now.
pub type HoldWaker = Arc<dyn Fn(Instant) + Send + Sync>;

/// One row of the view, copied out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSnapshot {
    /// Row of the view, 0 at the top.
    pub line: usize,
    pub row: Row,
}

/// What the screen layer needs to draw a terminal: the rows that changed since the last frame
/// (or all of them), and the state around them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub size: Size,
    /// Changed rows, top to bottom; every row when `full`.
    pub rows: Vec<RowSnapshot>,
    /// Every row is here: redraw them all (after a scroll, resize, clear, or when asked).
    pub full: bool,
    pub cursor: Cursor,
    pub modes: Modes,
    /// How far the view is scrolled back into the engine's own history.
    pub display_offset: usize,
    pub history: usize,
    /// The selection, in grid coordinates (not damage: draw it on every frame).
    pub selection: Option<SelectionSpan>,
    /// What the scrollbar shows: tmux's history when there is one, else the engine's.
    pub scroll: ScrollView,
    /// When the newest output in this frame was parsed (`None` before any): a key's echo time
    /// is measured up to it.
    pub last_output: Option<Instant>,
    /// What the presentation pass reads beyond the view's edges (`present::Edges`), copied
    /// under the same lock as the rows. Empty in the common case.
    pub edges: EdgeRows,
}

/// The rows beyond the view's top and bottom edges that belong with it: the rest of a line an
/// edge cuts, and whether a private key began above it (see `present::Screen`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EdgeRows {
    /// The rows above the view's first that its line runs over (each wraps into the next, or is
    /// full to the last column as tmux draws a long line), oldest first; at most
    /// [`CONTEXT_ROWS`].
    pub above: Vec<Row>,
    /// When scrolled back: the rows below the view's last that its line runs on into, at most
    /// [`CONTEXT_ROWS`].
    pub below: Vec<Row>,
    /// The line at the top goes on above `above` (it is longer than [`CONTEXT_ROWS`]).
    pub more_above: bool,
    /// The same at the bottom.
    pub more_below: bool,
    /// A private key began above the first row here (`above`'s first, else the view's) and
    /// hasn't ended. Worked out only when a row in view could be part of one
    /// (`secrets::looks_like_key_body` or `secrets::ends_key`), over up to
    /// [`KEY_CONTEXT_ROWS`] rows above.
    pub key_open_above: bool,
}

/// How many rows above the view are read to learn whether a private key is open there.
pub const KEY_CONTEXT_ROWS: usize = 80;

/// When the screen layer should take the next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// Nothing new.
    Idle,
    /// Now.
    Now,
    /// Not before this (a burst is being held back, or a program's synchronized update is open).
    At(Instant),
}

struct Inner<E> {
    engine: E,
    guard: OutputGuard,
    osc: OscScanner,
    found: Vec<OscEvent>,
    scroll: ScrollState,
    gate: RenderGate,
    /// Work parsed while a program's synchronized update stayed open (see [`SYNC_WORK`]).
    sync_work: usize,
    /// When output was last parsed ([`Frame::last_output`]).
    last_output: Option<Instant>,
    /// For each row of the view as last copied out: could it be part of a private key (worth
    /// looking above the view for a BEGIN line)?
    key_hints: Vec<bool>,
}

/// Events waiting for [`Model::take_events`], folded as they come in, so a flood of bells,
/// questions, titles or notifications stays small while nobody takes them (a hidden pane
/// takes them seldom).
#[derive(Debug, Default)]
struct Pending {
    /// The last title.
    title: Option<Option<String>>,
    /// Any number of bells, rung as one.
    bell: bool,
    /// The last copy.
    copy: Option<String>,
    /// Every reply, in the order asked, up to [`MAX_REPLIES`] bytes; past that they are
    /// dropped, as xterm does under a flood of questions.
    replies: Vec<u8>,
    cwd: Option<String>,
    /// The latest [`MAX_NOTIFICATIONS`].
    notifications: VecDeque<Option<String>>,
    progress: Option<Option<Progress>>,
}

impl Pending {
    fn push(&mut self, event: TermEvent) {
        match event {
            TermEvent::Title(title) => self.title = Some(title),
            TermEvent::Bell => self.bell = true,
            TermEvent::Copy(text) => self.copy = Some(text),
            TermEvent::Reply(bytes) => {
                if self.replies.len() + bytes.len() <= MAX_REPLIES {
                    self.replies.extend_from_slice(&bytes);
                }
            }
            TermEvent::Osc(OscEvent::Cwd(path)) => self.cwd = Some(path),
            TermEvent::Osc(OscEvent::Notify(text)) => {
                if self.notifications.len() == MAX_NOTIFICATIONS {
                    self.notifications.pop_front();
                }
                self.notifications.push_back(text);
            }
            TermEvent::Osc(OscEvent::Progress(progress)) => self.progress = Some(progress),
        }
    }

    fn take(&mut self) -> Vec<TermEvent> {
        let p = std::mem::take(self);
        let mut out = Vec::new();
        out.extend(p.title.map(TermEvent::Title));
        if p.bell {
            out.push(TermEvent::Bell);
        }
        out.extend(p.copy.map(TermEvent::Copy));
        if !p.replies.is_empty() {
            out.push(TermEvent::Reply(p.replies));
        }
        out.extend(p.cwd.map(|path| TermEvent::Osc(OscEvent::Cwd(path))));
        out.extend(p.notifications.into_iter().map(|text| TermEvent::Osc(OscEvent::Notify(text))));
        out.extend(p.progress.map(|progress| TermEvent::Osc(OscEvent::Progress(progress))));
        out
    }
}

/// A pane's terminal model. Share it as `Arc<Model>`.
pub struct Model<E: Engine = AlacrittyEngine> {
    inner: FairMutex<Inner<E>>,
    events: Mutex<Pending>,
    dirty: AtomicBool,
    waker: RwLock<Option<Waker>>,
    hold_waker: RwLock<Option<HoldWaker>>,
}

impl Model<AlacrittyEngine> {
    /// A terminal of `size` on alacritty_terminal.
    pub fn new(size: Size, config: &EngineConfig) -> Self {
        Self::from_engine(AlacrittyEngine::new(size, config))
    }
}

impl<E: Engine> Model<E> {
    pub fn from_engine(engine: E) -> Self {
        Model {
            inner: FairMutex::new(Inner {
                engine,
                guard: OutputGuard::new(),
                osc: OscScanner::new(),
                found: Vec::new(),
                scroll: ScrollState::new(),
                gate: RenderGate::new(),
                sync_work: 0,
                last_output: None,
                key_hints: Vec::new(),
            }),
            events: Mutex::new(Pending::default()),
            dirty: AtomicBool::new(true),
            waker: RwLock::new(None),
            hold_waker: RwLock::new(None),
        }
    }

    /// Who to wake after output arrives (the window showing this pane), or nobody.
    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.waker.write().unwrap_or_else(|e| e.into_inner()) = waker;
    }

    /// Who to tell, instead of waking it, that output started a hold and when to look (see
    /// [`HoldWaker`]); without one the waker is called.
    pub fn set_hold_waker(&self, waker: Option<HoldWaker>) {
        *self.hold_waker.write().unwrap_or_else(|e| e.into_inner()) = waker;
    }

    fn wake(&self) {
        let waker = self.waker.read().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(wake) = waker {
            wake();
        }
    }

    /// Parses a program's output. Called on the session's reader thread for each read.
    pub fn feed(&self, bytes: &[u8]) {
        self.feed_at(bytes, Instant::now());
    }

    /// [`Model::feed`] at a time given rather than read from the clock, so a replay on a
    /// virtual clock holds and shows bursts the same on every machine.
    pub fn feed_at(&self, bytes: &[u8], now: Instant) {
        if bytes.is_empty() {
            return;
        }
        let mut inner = self.inner.lock_unfair();
        // While the gate holds a burst, the screen has already been told when to look again
        // (its last `due` said `At`, which only moves later as more output comes): waking it for
        // each read would draw the same picture again at the display's rate. The read that
        // starts a hold tells it when to look (the hold waker), or wakes it to ask.
        let held = inner.gate.is_holding();
        inner.gate.output(now, bytes);
        inner.last_output = Some(now);
        self.parse(&mut inner, bytes, true);
        let due = (!held).then(|| self.due_in(&mut inner, now));
        drop(inner);
        let hold = self.hold_waker.read().unwrap_or_else(|e| e.into_inner()).clone();
        match (due, hold) {
            (None, _) => {}
            (Some(Due::At(at)), Some(hold)) => hold(at),
            (Some(_), _) => self.wake(),
        }
    }

    /// Parses `bytes` through the guard, slice by slice, letting a waiting UI thread have the
    /// terminal between slices. `count_tmux`: line feeds on the bottom row are lines going up
    /// into tmux's history (not for the lines this app writes itself).
    fn parse(&self, inner: &mut MutexGuard<'_, Inner<E>>, bytes: &[u8], count_tmux: bool) {
        let size = inner.engine.size();
        let guarded = inner.guard.pass(bytes, size);
        let mut from = 0;
        for (n, &(end, work)) in guarded.slices.iter().enumerate() {
            if n > 0 {
                // The screen's turn, if it is waiting.
                MutexGuard::bump(inner);
            }
            let state = &mut **inner;
            let slice = &guarded.bytes[from..end];
            from = end;
            state.osc.scan(slice, &mut state.found);
            state.engine.feed(slice);
            if state.engine.sync_deadline().is_some() {
                state.sync_work += work;
                if state.sync_work > SYNC_WORK {
                    state.engine.stop_sync();
                    state.sync_work = 0;
                }
            } else {
                state.sync_work = 0;
            }
            self.settle(state, count_tmux);
            self.dirty.store(true, Ordering::Release);
        }
    }

    /// Takes what parsing found into the events queue, while the terminal is still locked, so
    /// the queue keeps the order things were parsed in; and counts lines going into tmux's
    /// history. Returns whether anything was queued.
    fn settle(&self, inner: &mut Inner<E>, count_tmux: bool) -> bool {
        let mut events = Vec::with_capacity(inner.found.len());
        events.extend(inner.found.drain(..).map(TermEvent::Osc));
        events.extend(take_engine_events(inner));
        let feeds = inner.engine.take_line_feeds_at_bottom();
        if count_tmux && feeds > 0 {
            inner.scroll.lines_scrolled_out(feeds);
        }
        if events.is_empty() {
            return false;
        }
        let mut pending = self.events.lock().unwrap_or_else(|e| e.into_inner());
        for event in events {
            pending.push(event);
        }
        true
    }

    /// What happened since the last call, folded and in this order: the title (the last one;
    /// `None` resets it), a bell (any number rang as one), the last copy, the replies to write
    /// back (all in one, in the order asked; past [`MAX_REPLIES`] bytes the rest were dropped),
    /// the working folder (the last), notifications (the latest [`MAX_NOTIFICATIONS`]),
    /// progress (the last).
    pub fn take_events(&self) -> Vec<TermEvent> {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// Something changed since the last frame.
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    /// When to take the next frame: at once, later (a burst is held back), or not at all.
    pub fn due(&self, now: Instant) -> Due {
        let mut inner = self.inner.lock();
        self.due_in(&mut inner, now)
    }

    fn due_in(&self, inner: &mut Inner<E>, now: Instant) -> Due {
        let sync = inner.engine.sync_deadline();
        // A program's synchronized update whose time is up shows now ([`Model::frame`] ends it).
        if sync.is_some_and(|at| at <= now) {
            return Due::Now;
        }
        if !self.is_dirty() {
            return sync.map_or(Due::Idle, Due::At);
        }
        match inner.gate.show(now) {
            Show::Now => Due::Now,
            Show::At(at) => Due::At(sync.map_or(at, |s| s.min(at))),
        }
    }

    /// The rows that changed since the last frame (all of them when `all`, as for a pane that
    /// comes back into view), with the cursor, modes and scroll position.
    pub fn frame(&self, all: bool) -> Frame {
        self.frame_at(all, Instant::now())
    }

    /// [`Model::frame`] at a time given (see [`Model::feed_at`]).
    pub fn frame_at(&self, all: bool, now: Instant) -> Frame {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let mut late = false;
        if inner.engine.sync_deadline().is_some_and(|at| at <= now) {
            // A program's synchronized update outlived its time: show what it drew so far. Its
            // answers are queued now, before anything parsed after it.
            inner.engine.stop_sync();
            inner.sync_work = 0;
            late = self.settle(inner, true);
        }
        self.dirty.store(false, Ordering::Release);
        let damage = inner.engine.take_damage();
        let size = inner.engine.size();
        let (full, lines) = match damage {
            Damage::Rows(lines) if !all => (false, lines),
            _ => (true, (0..size.rows as usize).collect()),
        };
        let rows: Vec<RowSnapshot> = lines.into_iter().map(|line| RowSnapshot { line, row: inner.engine.row(line) }).collect();
        let display_offset = inner.engine.display_offset();
        let history = inner.engine.history_size();
        inner.key_hints.resize(size.rows as usize, false);
        for r in &rows {
            if let Some(hint) = inner.key_hints.get_mut(r.line) {
                let text = r.row.text();
                *hint = crate::secrets::looks_like_key_body(&text) || crate::secrets::ends_key(&text);
            }
        }
        let key_hint = inner.key_hints.iter().any(|&h| h);
        let edges = edge_rows(&inner.engine, display_offset, size, key_hint);
        let frame = Frame {
            size,
            rows,
            full,
            cursor: inner.engine.cursor(),
            modes: inner.engine.modes(),
            display_offset,
            history,
            selection: inner.engine.selection(),
            scroll: inner.scroll.view(local_view(history, display_offset, size.rows as usize)),
            last_output: inner.last_output,
            edges,
        };
        drop(guard);
        if late {
            self.wake();
        }
        frame
    }

    /// Bytes the user sends (keys, pastes, mouse and focus reports) on their way to the session:
    /// returns what to write. Typing while scrolled back in tmux goes to the prompt (tmux leaves
    /// copy mode first), typing shows the bottom of the history again, and what comes back
    /// shows at once.
    pub fn user_input(&self, bytes: &[u8]) -> Vec<u8> {
        self.user_input_at(bytes, Instant::now())
    }

    /// [`Model::user_input`] at a time given (see [`Model::feed_at`]).
    pub fn user_input_at(&self, bytes: &[u8], now: Instant) -> Vec<u8> {
        let mut inner = self.inner.lock();
        inner.gate.typed(now);
        if !is_typing(bytes) {
            return bytes.to_vec();
        }
        let mut out = Vec::with_capacity(bytes.len() + LEAVE_SCROLL.len());
        if inner.scroll.in_copy_mode() {
            out.extend_from_slice(LEAVE_SCROLL);
            inner.scroll.left_copy_mode();
        }
        out.extend_from_slice(bytes);
        if inner.engine.display_offset() != 0 {
            inner.engine.scroll_to_bottom();
            self.dirty.store(true, Ordering::Release);
        }
        out
    }

    /// A new size. Returns the size the terminal has now, which is what the session's PTY
    /// should be told (sshee-core's `SessionManager::resize`): a size below [`Size::FLOOR`] (a
    /// hidden or minimized pane) is ignored, and the PTY should keep its size too.
    pub fn resize(&self, size: Size) -> Size {
        let now = {
            let mut inner = self.inner.lock();
            inner.engine.resize(size);
            inner.engine.size()
        };
        self.dirty.store(true, Ordering::Release);
        now
    }

    pub fn size(&self) -> Size {
        self.inner.lock().engine.size()
    }

    pub fn modes(&self) -> Modes {
        self.inner.lock().engine.modes()
    }

    pub fn title(&self) -> Option<String> {
        self.inner.lock().engine.title().map(str::to_owned)
    }

    /// Scrolls the engine's own history: positive goes up.
    pub fn scroll_lines(&self, lines: i32) {
        self.inner.lock().engine.scroll_display(lines);
        self.dirty.store(true, Ordering::Release);
    }

    /// What the scrollbar shows.
    pub fn scroll_view(&self) -> ScrollView {
        let inner = self.inner.lock();
        let e = &inner.engine;
        inner.scroll.view(local_view(e.history_size(), e.display_offset(), e.size().rows as usize))
    }

    /// Scrolls so line `top` is first in view. The engine's history scrolls here; for tmux the
    /// answer says which position to ask tmux for (the scrollbar already shows it).
    pub fn scroll_to(&self, top: f64) -> ScrollTo {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let e = &inner.engine;
        let local = local_view(e.history_size(), e.display_offset(), e.size().rows as usize);
        let to = inner.scroll.scroll_to(top, local);
        if let ScrollTo::Local { display_offset } = to {
            let now = inner.engine.display_offset() as i32;
            inner.engine.scroll_display(display_offset as i32 - now);
        }
        self.dirty.store(true, Ordering::Release);
        to
    }

    /// tmux's answer to a scroll request, or `None` when the history is the terminal's own.
    pub fn set_tmux(&self, scroll: Option<TmuxScroll>) {
        self.inner.lock().scroll.set_tmux(scroll);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn tmux(&self) -> Option<TmuxScroll> {
        self.inner.lock().scroll.tmux()
    }

    /// The session connected. A new shell without tmux has its history here; with tmux the
    /// local history is small (tmux keeps it on the server) and `history` says how small.
    pub fn connected(&self, durable: bool, history: usize) {
        let mut inner = self.inner.lock();
        if !durable {
            inner.scroll.set_tmux(None);
        }
        inner.engine.set_history(history);
    }

    /// The connection dropped: the program that asked for mouse, focus and paste reports is
    /// gone, and nothing more of a held-back burst or of its synchronized update is coming. What
    /// the update held shows now, and the reports go off at once: past the gate, which would
    /// hold them as output (and so a key's answer after the reconnect still shows at once).
    pub fn disconnected(&self) {
        let mut inner = self.inner.lock();
        inner.gate.reset();
        inner.engine.stop_sync();
        inner.sync_work = 0;
        self.settle(&mut inner, true);
        self.parse(&mut inner, STOP_REPORTS, false);
        drop(inner);
        self.wake();
    }

    /// A reconnect landed in a new shell: says so on screen (past the gate, as above).
    pub fn reconnected_new_shell(&self) {
        let mut inner = self.inner.lock();
        self.parse(&mut inner, NEW_SHELL_LINE.as_bytes(), false);
        drop(inner);
        self.wake();
    }

    /// Forgets the history and puts the cursor's row at the top (⌘K on a Mac).
    pub fn clear_history(&self) {
        self.inner.lock().engine.clear_history();
        self.dirty.store(true, Ordering::Release);
    }

    /// The colours to answer a program's colour questions with (from the colour scheme).
    pub fn set_answer_colors(&self, colors: AnswerColors) {
        self.inner.lock().engine.set_answer_colors(colors);
    }

    /// The cursor a program gets when it asks for none of its own, from now on (see
    /// [`Engine::set_cursor_default`]). The next frame carries it.
    pub fn set_cursor_default(&self, shape: crate::engine::CursorShape, blinking: bool) {
        self.with_engine(|engine| engine.set_cursor_default(shape, blinking));
    }

    /// Anything else, with the engine locked (fairly): selection, find, reading history rows.
    /// The next frame is taken as if output had come, since a selection isn't damage.
    pub fn with_engine<R>(&self, f: impl FnOnce(&mut E) -> R) -> R {
        let result = f(&mut self.inner.lock().engine);
        self.dirty.store(true, Ordering::Release);
        result
    }

    /// The selected text for a copy, if anything is selected (see [`crate::select::copy_text`]).
    /// The rows are copied out with the terminal locked and `presenter` reads them after, so a
    /// long selection (select all) doesn't hold up output or the screen meanwhile.
    pub fn selected_text(&self, presenter: &dyn CopyPresenter) -> Option<String> {
        let rows = {
            let inner = self.inner.lock();
            crate::select::copy_selection(&inner.engine)
        };
        rows.map(|rows| rows.text(presenter))
    }

    /// The selected text as the screen shows it under `look` (`Look::copy_presenter`): what is
    /// hidden copies as its dots. Whether a private key is open where the copy starts is worked
    /// out here, from the rows above the selection's own first row (a key's BEGIN line can be on
    /// screen above a selection that starts in its body, or above the screen for one that
    /// starts in the history), never from the view's.
    pub fn copy_text(&self, look: &Look) -> Option<String> {
        let (rows, key_open) = {
            let inner = self.inner.lock();
            let rows = crate::select::copy_selection(&inner.engine)?;
            let key_open = look.secrets_hidden() && key_open_above(&inner.engine, rows.top());
            (rows, key_open)
        };
        Some(rows.text(&look.copy_presenter(key_open)))
    }
}

/// Whether a private key began above grid row `first` and hasn't ended, read from up to
/// [`KEY_CONTEXT_ROWS`] rows above it (rows a long line wrapped over read as that one line).
fn key_open_above<E: Engine>(engine: &E, first: i32) -> bool {
    let mut rows: Vec<(String, bool)> =
        (1..=KEY_CONTEXT_ROWS as i32).map_while(|k| engine.grid_row(first - k)).map(|row| (row.text(), row.wraps)).collect();
    rows.reverse();
    let mut lines: Vec<String> = Vec::with_capacity(rows.len());
    let mut joins = false;
    for (text, wraps) in rows {
        match lines.last_mut() {
            Some(last) if joins => last.push_str(&text),
            _ => lines.push(text),
        }
        joins = wraps;
    }
    crate::secrets::key_open_after(&lines)
}

/// Whether a line runs on from `row` into the row below: it wraps there, or it is full to the
/// last column (tmux draws a long line as full rows and the terminal never learns it wrapped).
/// As the presentation pass decides (`present::RowText::used_cols`).
fn runs_on(row: &Row, cols: u16) -> bool {
    row.wraps || row.row_text(false).used_cols() >= cols
}

/// The rows beyond the view's edges that the presentation pass reads with it (see
/// [`EdgeRows`]). `key_hint`: a row in view could be part of a private key.
fn edge_rows<E: Engine>(engine: &E, display_offset: usize, size: Size, key_hint: bool) -> EdgeRows {
    let cols = size.cols;
    let top = -(display_offset as i32);
    let mut edges = EdgeRows::default();
    // A line that wraps over more than CONTEXT_ROWS rows above the view: the pass hides its rows
    // on screen whole whatever those rows hold (`present::Edges::more_above`), so none is copied.
    // Asked without copying a row, so a line that keeps growing (a server printing one long line
    // for minutes) doesn't cost a thousand rows a snapshot under the lock. A line tmux drew as
    // full rows has no wrap marks and is read below as before.
    let long = (1..=CONTEXT_ROWS as i32 + 1).all(|k| engine.row_wraps(top - k) == Some(true));
    if long {
        edges.more_above = true;
    }
    let mut line = top - 1;
    while let Some(row) = if long { None } else { engine.grid_row(line) } {
        if !runs_on(&row, cols) {
            break;
        }
        if edges.above.len() == CONTEXT_ROWS {
            edges.more_above = true;
            break;
        }
        edges.above.push(row);
        line -= 1;
    }
    edges.above.reverse();
    if display_offset > 0 {
        let mut line = top + size.rows as i32 - 1;
        let mut last = engine.grid_row(line);
        while let Some(row) = last.take() {
            if !runs_on(&row, cols) {
                break;
            }
            line += 1;
            let Some(next) = engine.grid_row(line) else { break };
            if edges.below.len() == CONTEXT_ROWS {
                edges.more_below = true;
                break;
            }
            edges.below.push(next.clone());
            last = Some(next);
        }
    }
    if key_hint {
        // Above the first row given.
        edges.key_open_above = key_open_above(engine, top - edges.above.len() as i32);
    }
    edges
}

/// The engine's events, with tmux's scroll reports taken in and program copies cleaned.
fn take_engine_events<E: Engine>(inner: &mut Inner<E>) -> Vec<TermEvent> {
    let mut out = Vec::new();
    for event in inner.engine.take_events() {
        match event {
            TermEvent::Title(Some(title)) => {
                inner.scroll.on_title(&title);
                out.push(TermEvent::Title(Some(title)));
            }
            // What a server copies can come back as a paste anywhere: no keys in it. Too big
            // is refused, as before.
            TermEvent::Copy(text) if text.len() <= MAX_COPY => out.push(TermEvent::Copy(without_keys(&text))),
            TermEvent::Copy(_) => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::GridPoint;
    use crate::osc::{Progress, ProgressState};
    use crate::select::{SelectionKind, Side};
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn model(cols: u16, rows: u16) -> Model {
        Model::new(Size::new(cols, rows), &EngineConfig::default())
    }

    fn texts(frame: &Frame) -> Vec<(usize, String)> {
        frame.rows.iter().map(|r| (r.line, r.row.trimmed_text())).collect()
    }

    #[test]
    fn first_frame_has_every_row_then_only_changes() {
        let m = model(20, 4);
        m.feed(b"one\r\ntwo");
        let f = m.frame(false);
        assert!(f.full);
        assert_eq!(texts(&f), [(0, "one".into()), (1, "two".into()), (2, "".into()), (3, "".into())]);
        assert!(!m.is_dirty());
        // One key's echo changes one row.
        m.feed(b"!");
        let f = m.frame(false);
        assert!(!f.full);
        assert_eq!(texts(&f), [(1, "two!".into())]);
        assert_eq!((f.cursor.line, f.cursor.col), (1, 4));
        // Nothing new: only the cursor's row, which alacritty always counts as changed.
        assert_eq!(texts(&m.frame(false)), [(1, "two!".into())]);
        // A pane shown again asks for all of them.
        assert_eq!(m.frame(true).rows.len(), 4);
    }

    #[test]
    fn rows_carry_colours_styles_and_widths() {
        use crate::engine::{CellFlags, Color};
        let m = model(10, 2);
        m.feed("\x1b[1;31mE\x1b[0m\x1b[38;2;1;2;3;48;5;200mx\x1b[0m漢\x1b[4mu".as_bytes());
        let row = &m.frame(false).rows[0].row;
        assert_eq!(row.cells[0].fg, Color::Indexed(1));
        assert!(row.cells[0].flags.contains(CellFlags::BOLD));
        assert_eq!(row.cells[1].fg, Color::Rgb(crate::engine::Rgb::new(1, 2, 3)));
        assert_eq!(row.cells[1].bg, Color::Indexed(200));
        assert_eq!((row.cells[2].ch, row.cells[2].width), ('漢', 2));
        assert_eq!(row.cells[3].width, 0);
        assert!(row.cells[4].flags.contains(CellFlags::UNDERLINE));
        assert_eq!(row.text(), "Ex漢u     ");
        let tc = row.row_text(false);
        assert_eq!(tc.cols[..4], [0, 1, 2, 4]);
    }

    #[test]
    fn events_from_the_engine_and_the_osc_pass() {
        let m = model(20, 4);
        m.feed(b"\x1b]0;build\x07\x07\x1b]9;Done\x07\x1b]7;file://web-7731/srv\x07\x1b]9;4;1;50\x07\x1b[c\x1b]52;c;aGkbWzIwMX4=\x07");
        let events = m.take_events();
        assert!(events.contains(&TermEvent::Title(Some("build".into()))));
        assert!(events.contains(&TermEvent::Bell));
        assert!(events.contains(&TermEvent::Osc(OscEvent::Notify(Some("Done".into())))));
        assert!(events.contains(&TermEvent::Osc(OscEvent::Cwd("/srv".into()))));
        assert!(events.contains(&TermEvent::Osc(OscEvent::Progress(Some(Progress { state: ProgressState::Normal, value: 50.0 })))));
        assert!(events.contains(&TermEvent::Reply(b"\x1b[?6c".to_vec())));
        // "hi" + ESC[201~ copied by a server: the Esc is taken out.
        assert!(events.contains(&TermEvent::Copy("hi[201~".into())));
        assert!(m.take_events().is_empty());
        assert_eq!(m.title().as_deref(), Some("build"));
    }

    #[test]
    fn da1_is_not_answered_behind_conpty() {
        let m = Model::new(Size::new(20, 4), &EngineConfig { answer_da1: false, ..EngineConfig::default() });
        m.feed(b"\x1b[c\x1b[6n");
        assert_eq!(m.take_events(), [TermEvent::Reply(b"\x1b[1;1R".to_vec())]);
    }

    #[test]
    fn colour_questions_get_the_scheme_or_the_program_colour() {
        let m = model(20, 4);
        m.set_answer_colors(AnswerColors { background: crate::engine::Rgb::new(0x10, 0x20, 0x30), ..AnswerColors::default() });
        m.feed(b"\x1b]11;?\x07");
        assert_eq!(m.take_events(), [TermEvent::Reply(b"\x1b]11;rgb:1010/2020/3030\x07".to_vec())]);
        m.feed(b"\x1b]4;1;rgb:ff/00/00\x07\x1b]4;1;?\x1b\\");
        assert_eq!(m.take_events(), [TermEvent::Reply(b"\x1b]4;1;rgb:ffff/0000/0000\x1b\\".to_vec())]);
    }

    #[test]
    fn copies_too_big_are_refused() {
        let m = model(20, 4);
        let big = "a".repeat(MAX_COPY + 3);
        let encoded = base64_encode(big.as_bytes());
        m.feed(format!("\x1b]52;c;{encoded}\x07").as_bytes());
        assert!(m.take_events().is_empty());
    }

    fn base64_encode(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn tmux_scroll_title_and_typing_while_scrolled_back() {
        let m = model(20, 4);
        m.connected(true, 1000);
        m.feed(b"\x1b]2;sshee-scroll 0 480 4 2000\x07");
        assert_eq!(m.scroll_view(), ScrollView { total: 484, top: 480, rows: 4 });
        // Lines going up into tmux's history while the cursor is on the bottom row.
        m.feed(b"\x1b[4;1H");
        m.feed(b"a\r\nb\r\nc");
        assert_eq!(m.tmux().unwrap().history, 482);
        // Scroll back with the scrollbar: tmux is asked, the scrollbar moves at once.
        assert_eq!(m.scroll_to(382.0), ScrollTo::Tmux { position: 100 });
        assert_eq!(m.scroll_view().top, 382);
        // A key leaves copy mode first; a mouse report doesn't.
        assert_eq!(m.user_input(b"\x1b[<0;1;1M"), b"\x1b[<0;1;1M");
        assert_eq!(m.user_input(b"x"), [LEAVE_SCROLL, b"x"].concat());
        assert_eq!(m.user_input(b"y"), b"y");
        // A new shell without tmux: the history is local again.
        m.connected(false, 10_000);
        assert_eq!(m.tmux(), None);
    }

    #[test]
    fn local_history_scrolls_and_typing_returns_to_the_bottom() {
        let m = model(10, 3);
        for i in 0..20 {
            m.feed(format!("line {i}\r\n").as_bytes());
        }
        m.frame(false);
        assert_eq!(m.scroll_view(), ScrollView { total: 21, top: 18, rows: 3 });
        assert_eq!(m.scroll_to(5.0), ScrollTo::Local { display_offset: 13 });
        let f = m.frame(false);
        assert!(f.full);
        assert_eq!(f.display_offset, 13);
        assert_eq!(f.rows[0].row.trimmed_text(), "line 5");
        assert!(!f.cursor.visible);
        m.user_input(b"q");
        assert_eq!(m.frame(false).display_offset, 0);
    }

    #[test]
    fn disconnect_turns_reports_off() {
        let m = model(60, 4);
        m.feed(b"\x1b[?1000h\x1b[?1006h\x1b[?2004h\x1b[?1004h");
        let on = m.modes();
        assert!(on.bracketed_paste && on.focus_reports && on.mouse != crate::engine::MouseProtocol::Off);
        m.disconnected();
        let off = m.modes();
        assert!(!off.bracketed_paste && !off.focus_reports);
        assert_eq!(off.mouse, crate::engine::MouseProtocol::Off);
        m.reconnected_new_shell();
        let f = m.frame(true);
        assert!(f.rows.iter().any(|r| r.row.text().contains("reconnected · this is a new shell")));
    }

    #[test]
    fn clear_history_keeps_the_prompt_line() {
        let m = model(10, 4);
        m.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\n$ ");
        m.clear_history();
        let f = m.frame(false);
        assert!(f.full);
        assert_eq!(f.history, 0);
        assert_eq!(f.rows[0].row.trimmed_text(), "$");
        assert_eq!((f.cursor.line, f.cursor.col), (0, 2));
    }

    #[test]
    fn waker_is_called_after_output() {
        let m = model(10, 2);
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        m.set_waker(Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })));
        m.feed(b"x");
        m.feed(b"y");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn due_follows_the_gate_and_synchronized_updates() {
        let m = model(20, 4);
        m.frame(false);
        assert_eq!(m.due(Instant::now()), Due::Idle);
        m.feed(b"x");
        assert_eq!(m.due(Instant::now()), Due::Now);
        m.frame(false);
        // A redraw that starts with an erase is held back a moment.
        m.feed(b"\x1b[2J\x1b[Hhalf");
        assert!(matches!(m.due(Instant::now()), Due::At(_)));
        assert_eq!(m.due(Instant::now() + Duration::from_millis(300)), Due::Now);
        m.frame(false);
        // A program's synchronized update that never ends shows after vte's 150 ms.
        m.feed(b"\x1b[?2026hhidden");
        let f = m.frame(false);
        assert!(f.rows.iter().all(|r| !r.row.text().contains("hidden")));
        assert!(matches!(m.due(Instant::now()), Due::At(_)));
        std::thread::sleep(Duration::from_millis(160));
        assert_eq!(m.due(Instant::now()), Due::Now);
        let f = m.frame(false);
        assert!(f.rows.iter().any(|r| r.row.text().contains("hidden")));
    }

    #[test]
    fn due_is_now_once_a_synchronized_update_is_past_its_time() {
        let m = model(20, 4);
        m.frame(false);
        let t0 = Instant::now();
        m.feed(b"\x1b[?2026hpart one");
        std::thread::sleep(Duration::from_millis(100));
        // The rest of the redraw starts with an erase: the gate holds it past the update's time.
        m.feed(b"\x1b[2Jpart two");
        if t0.elapsed() >= Duration::from_millis(140) {
            eprintln!("the machine is too busy to set this up in time; skipped");
            return;
        }
        // Past the update's time: now, not a moment already gone (a timer set for that fires at
        // once and asks again, over and over).
        std::thread::sleep((t0 + Duration::from_millis(160)).saturating_duration_since(Instant::now()));
        assert_eq!(m.due(Instant::now()), Due::Now);
        let f = m.frame(false);
        assert!(f.rows.iter().any(|r| r.row.text().contains("part two")));
    }

    #[test]
    fn tmux_counts_every_line_feed_that_lands_on_the_bottom_row() {
        let m = model(20, 4);
        m.connected(true, 1000);
        m.feed(b"\x1b]2;sshee-scroll 0 480 4 2000\x07");
        // The cursor two rows up; six lines in one read: three go up into tmux's history.
        m.feed(b"\x1b[2;1H");
        m.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf");
        assert_eq!(m.tmux().unwrap().history, 483);
        // A report in the same read counts the lines before it; only the ones after are added.
        m.feed(b"g\r\nh\r\n\x1b]2;sshee-scroll 0 600 4 2000\x07i\r\n");
        assert_eq!(m.tmux().unwrap().history, 601);
        // Inside a synchronized update the lines count when it ends.
        m.feed(b"\x1b[?2026hj\r\nk\r\n");
        assert_eq!(m.tmux().unwrap().history, 601);
        m.feed(b"\x1b[?2026l");
        assert_eq!(m.tmux().unwrap().history, 603);
    }

    #[test]
    fn disconnect_shows_a_held_update_and_keeps_a_keys_answer_due() {
        let m = model(30, 4);
        m.feed(b"\x1b[?1000h\x1b[?2004h\x1b[?2026hhalf drawn");
        m.frame(false);
        m.user_input(b"x");
        m.disconnected();
        // The reports are off at once, and what the update held shows.
        assert_eq!(m.modes().mouse, crate::engine::MouseProtocol::Off);
        assert!(!m.modes().bracketed_paste);
        assert!(m.frame(false).rows.iter().any(|r| r.row.text().contains("half drawn")));
        // The reconnect's redraw starts with an erase: still the key's answer, shown at once.
        m.feed(b"\x1b[2J$ x");
        assert_eq!(m.due(Instant::now()), Due::Now);
    }

    #[test]
    fn answers_a_timed_out_update_held_go_before_later_ones() {
        let m = Arc::new(model(20, 4));
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        m.set_waker(Some(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        })));
        m.feed(b"\x1b[?2026h\x1b[6n");
        assert!(m.take_events().is_empty());
        std::thread::sleep(Duration::from_millis(160));
        let before = woken.load(Ordering::SeqCst);
        m.frame(false);
        assert_eq!(woken.load(Ordering::SeqCst), before + 1, "late answers wake the app");
        m.feed(b"\x1b[c");
        assert_eq!(m.take_events(), [TermEvent::Reply(b"\x1b[1;1R\x1b[?6c".to_vec())]);
    }

    #[test]
    fn floods_of_events_are_folded() {
        let m = model(20, 4);
        m.feed(&b"\x07".repeat(100_000));
        m.feed(&b"\x1b[6n".repeat(20_000));
        let mut notes = Vec::new();
        for i in 0..20 {
            notes.extend_from_slice(format!("\x1b]9;note {i}\x07\x1b]0;title {i}\x07").as_bytes());
        }
        m.feed(&notes);
        let events = m.take_events();
        assert_eq!(events[0], TermEvent::Title(Some("title 19".into())));
        assert_eq!(events[1], TermEvent::Bell);
        let TermEvent::Reply(replies) = &events[2] else { panic!("{:?}", events[2]) };
        assert_eq!(replies.len(), MAX_REPLIES / 6 * 6);
        assert!(replies.chunks(6).all(|r| r == b"\x1b[1;1R"));
        let notified: Vec<_> = events[3..].iter().map(|e| match e {
            TermEvent::Osc(OscEvent::Notify(Some(text))) => text.clone(),
            other => panic!("{other:?}"),
        }).collect();
        assert_eq!(notified, (12..20).map(|i| format!("note {i}")).collect::<Vec<_>>());
        assert!(m.take_events().is_empty());
    }

    #[test]
    fn a_hidden_panes_size_is_ignored() {
        let m = model(80, 24);
        assert_eq!(m.resize(Size::new(0, 0)), Size::new(80, 24));
        assert_eq!(m.resize(Size::new(100, 30)), Size::new(100, 30));
    }

    #[test]
    fn the_clock_can_be_given() {
        let m = model(20, 4);
        let t0 = Instant::now() + Duration::from_secs(3600);
        m.frame_at(false, t0);
        assert_eq!(m.due(t0), Due::Idle);
        // An erase held back by the gate, on the given clock alone.
        m.feed_at(b"\x1b[2J\x1b[Hhalf", t0);
        assert!(matches!(m.due(t0), Due::At(_)));
        assert_eq!(m.due(t0 + Duration::from_millis(300)), Due::Now);
        let f = m.frame_at(false, t0 + Duration::from_millis(300));
        assert_eq!(f.last_output, Some(t0));
        // A key's echo shows at once: the gate heard the key at the time given.
        m.user_input_at(b"x", t0 + Duration::from_secs(1));
        m.feed_at(b"\x1b[2Jx", t0 + Duration::from_secs(1));
        assert_eq!(m.due(t0 + Duration::from_secs(1)), Due::Now);
    }

    #[test]
    fn edges_are_empty_while_nothing_is_cut() {
        let m = model(20, 4);
        m.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        let f = m.frame(false);
        assert_eq!(f.edges, EdgeRows::default());
    }

    #[test]
    fn edges_hold_the_rest_of_a_line_the_top_cuts() {
        let m = model(10, 3);
        // A line of 35 characters wraps over four rows; two more lines push its start off the top.
        m.feed(format!("{}\r\nend\r\n$ ", "a".repeat(35)).as_bytes());
        let f = m.frame(false);
        assert_eq!(f.rows[0].row.trimmed_text(), "aaaaa");
        assert_eq!(f.edges.above.len(), 3);
        assert!(f.edges.above.iter().all(|r| r.wraps && r.trimmed_text() == "aaaaaaaaaa"));
        assert!(!f.edges.more_above && f.edges.below.is_empty());
        // tmux draws a long line as full rows with no wrap marks: those count too.
        let m = model(10, 3);
        m.feed(b"bbbbbbbbbb\r\nbbbbbbbbbb\r\nbb\r\nx\r\ny");
        let f = m.frame(false);
        assert_eq!(f.rows[0].row.trimmed_text(), "bb");
        assert_eq!(f.edges.above.len(), 2);
    }

    #[test]
    fn edges_hold_the_rest_of_a_line_the_bottom_cuts_while_scrolled_back() {
        let m = model(10, 3);
        m.feed(format!("top\r\n{}\r\n1\r\n2\r\n3\r\n4", "c".repeat(45)).as_bytes());
        // Scrolled back so the view ends inside the long line.
        m.with_engine(|e| e.scroll_display(7));
        let f = m.frame(false);
        let shown: Vec<String> = f.rows.iter().map(|r| r.row.trimmed_text()).collect();
        assert_eq!(shown, ["top", "cccccccccc", "cccccccccc"]);
        assert_eq!(f.edges.below.len(), 3);
        assert_eq!(f.edges.below[2].trimmed_text(), "ccccc");
        assert!(!f.edges.more_below);
    }

    #[test]
    fn edges_know_a_key_began_above_the_view() {
        let m = model(70, 3);
        let body = "MIIEowIBAAKCAQEAx7pQ2b3m9wK5fLr8sT1vN4cZ0aY6eU2hJ8dG3kR5qW7tP9";
        m.feed(format!("-----BEGIN RSA PRIVATE KEY-----\r\n{body}\r\n{body}\r\n{body}\r\n{body}").as_bytes());
        let f = m.frame(false);
        assert!(f.edges.key_open_above);
        // Ended, and scrolled out of view: nothing to look for.
        m.feed(b"\r\n-----END RSA PRIVATE KEY-----\r\n$\r\n$\r\n$");
        let f = m.frame(false);
        assert!(!f.edges.key_open_above);
    }

    #[test]
    fn output_the_gate_holds_wakes_the_screen_once_not_per_read() {
        let m = model(40, 10);
        let woken = Arc::new(AtomicUsize::new(0));
        let w = woken.clone();
        m.set_waker(Some(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        })));
        let t0 = Instant::now() + Duration::from_secs(3600);
        m.frame_at(false, t0);
        // A flood: its first big read starts a hold and wakes the screen, which learns when to
        // look (`At`); the reads after it don't wake it again.
        let chunk = "x".repeat(4096);
        for i in 0..50 {
            m.feed_at(chunk.as_bytes(), t0 + Duration::from_millis(i));
        }
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let Due::At(at) = m.due(t0 + Duration::from_millis(50)) else { panic!("held") };
        // The screen looks at that time: the hold is over, it takes the frame.
        assert_eq!(m.due(at), Due::Now);
        m.frame_at(false, at);
        // Output after that wakes it again at once.
        m.feed_at(b"done\r\n", at + Duration::from_millis(1));
        assert_eq!(woken.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_hold_tells_the_screen_when_to_look_instead_of_waking_it() {
        let m = model(40, 10);
        let woken = Arc::new(AtomicUsize::new(0));
        let held = Arc::new(Mutex::new(Vec::new()));
        let (w, h) = (woken.clone(), held.clone());
        m.set_waker(Some(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        })));
        m.set_hold_waker(Some(Arc::new(move |at| h.lock().unwrap().push(at))));
        let t0 = Instant::now() + Duration::from_secs(3600);
        m.frame_at(false, t0);
        let chunk = "y".repeat(4096);
        for i in 0..10 {
            m.feed_at(chunk.as_bytes(), t0 + Duration::from_millis(i));
        }
        // Told once, when the hold began, the time the screen's `due` would say.
        let told = held.lock().unwrap().clone();
        assert_eq!(told.len(), 1);
        assert_eq!(told[0], t0 + crate::gate::BURST_QUIET);
        assert_eq!(woken.load(Ordering::SeqCst), 0);
        // Output the gate shows at once wakes it, as before.
        let at = match m.due(t0 + Duration::from_millis(500)) {
            Due::Now => t0 + Duration::from_millis(500),
            other => panic!("{other:?}"),
        };
        m.frame_at(false, at);
        m.feed_at(b"$ ", at + Duration::from_millis(1));
        assert_eq!(woken.load(Ordering::SeqCst), 1);
    }

    /// A private key's body copies as dots wherever the selection starts: its BEGIN line is
    /// above the selection (on screen or in the history), not necessarily above the view.
    #[test]
    fn a_key_body_copies_as_dots_when_the_selection_starts_below_its_begin_line() {
        let look = Look { hide_secrets: true, ..Look::default() };
        let body = ["MIIEowIBAAKCAQEAx7pQ2b3m9wK5fLr8sT1vN4cZ0aY6eU2hJ8dG3kR5qW7tP9", "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAt", "c3NoLWVkMjU1MTkAAAAg5n0y4dM2rQ8kZ3eTq1wB7xV9cL6pN0aH4sJ2fG8uK1y"];
        let m = model(70, 8);
        m.feed(format!("$ cat id\r\n-----BEGIN OPENSSH PRIVATE KEY-----\r\n{}\r\n{}\r\n{}\r\n", body[0], body[1], body[2]).as_bytes());
        m.frame(false);
        let view_line = |l: usize| GridPoint::from_view(l, 0, 0).line;
        // A drag over the second and third body lines (view rows 3 and 4).
        m.with_engine(|e| {
            e.start_selection(SelectionKind::Simple, GridPoint::new(view_line(3), 0), Side::Left);
            e.update_selection(GridPoint::new(view_line(4), 69), Side::Right);
        });
        let copied = m.copy_text(&look).unwrap();
        assert!(!copied.contains(&body[1][..8]) && !copied.contains(&body[2][..8]), "{copied}");
        assert!(copied.chars().filter(|c| !c.is_whitespace()).all(|c| c == '•'), "{copied}");
        // A double-click on one body line.
        m.with_engine(|e| e.start_selection(SelectionKind::Word, GridPoint::new(view_line(2), 5), Side::Left));
        let copied = m.copy_text(&look).unwrap();
        assert!(!copied.is_empty() && copied.chars().all(|c| c == '•'), "{copied}");
        // Scrolled so the BEGIN line is in the history above the view: still dots.
        m.feed(b"\r\n\r\n\r\n\r\n\r\n");
        let begin = (-20..8).find(|&l| m.with_engine(|e| e.grid_row(l)).is_some_and(|r| r.text().contains("BEGIN"))).unwrap();
        assert!(begin < 0, "the BEGIN line went up into the history");
        m.with_engine(|e| e.start_selection(SelectionKind::Lines, GridPoint::new(begin + 2, 0), Side::Left));
        let copied = m.copy_text(&look).unwrap();
        assert!(!copied.contains(&body[1][..8]), "{copied}");
        // As it is when secrets aren't hidden.
        assert!(m.copy_text(&Look::default()).unwrap().contains(body[1]));
    }

    #[test]
    fn a_line_longer_than_the_context_copies_no_edge_rows() {
        let m = model(20, 4);
        let long = "a".repeat(20 * (CONTEXT_ROWS + 10));
        m.feed(long.as_bytes());
        let f = m.frame(false);
        assert!(f.edges.above.is_empty() && f.edges.more_above, "{} rows above", f.edges.above.len());
        // Every row on screen is hidden: the line's start is out of reach.
        let texts = crate::engine::row_texts(&f.rows.iter().map(|r| r.row.clone()).collect::<Vec<_>>(), false);
        let screen = crate::present::Screen {
            edges: crate::present::Edges { more_above: true, ..Default::default() },
            ..crate::present::Screen::new(&texts, 20)
        };
        let shown = crate::present::Presenter::new(Look { hide_secrets: true, ..Look::default() }).present(&screen);
        assert!(shown.iter().all(|r| !r.text.contains('a')));
        // One a little shorter than the context still has its rows.
        let m = model(20, 4);
        m.feed("b".repeat(20 * (CONTEXT_ROWS - 10)).as_bytes());
        let f = m.frame(false);
        assert!(!f.edges.more_above && f.edges.above.len() > CONTEXT_ROWS - 20);
    }

    #[test]
    fn feeding_from_another_thread() {
        let m = Arc::new(model(40, 10));
        let reader = {
            let m = m.clone();
            std::thread::spawn(move || {
                for i in 0..2000 {
                    m.feed(format!("line {i}\r\n").as_bytes());
                }
            })
        };
        let mut frames = 0;
        while !reader.is_finished() {
            m.frame(false);
            frames += 1;
        }
        reader.join().unwrap();
        let f = m.frame(true);
        assert!(frames > 0);
        assert_eq!(f.rows[8].row.trimmed_text(), "line 1999");
    }
}
