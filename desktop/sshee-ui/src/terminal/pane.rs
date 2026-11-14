//! A pane's state on the UI thread: its model and link, what the view last drew, and what
//! poll and the handlers do with them.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rune::{ClipboardContent, Drag, Ime, Phase, PhysicalInput, PhysicalKeyCode, PointerButton, PointerEvent, Tx, WheelEvent};
use sshee_term::engine::{CursorShape, EngineConfig, MouseProtocol, Size, TermEvent};
use sshee_term::input::{
    self, focus_report, mouse_goes_to_program, paste, pasted_text, terminal_copy_key, terminal_key, typed_text, CopyKey, KeyAction,
    KeyOptions, MouseAction, MouseButton, MouseEvent, MouseReporter, Platform, WheelDelta, WheelLines,
};
use sshee_term::model::{Due, Model};
use sshee_term::osc::OscEvent;
use sshee_term::present::Look;
use sshee_term::select::{SelectionKind, Side};

use super::ime::ImeState;
use super::keys::{guarded_key, term_key, typed_run};
use super::metrics::{CellMetrics, CellMode};
use super::palette::{Palette, Scheme};
use super::rows::PaneView;
use super::wake::Coalesce;
use super::{LinkStatus, StatusSink, TermLink};
use crate::meter::{SharedMeter, StartInfo, Via};
use sshee_term::engine::{Engine, GridPoint};

/// The space between the pane's edge and its cells, in points (0.10's padding). Whole pixels at
/// 1, 1.25, 1.5, 1.75 and 2x.
pub const PADDING: f32 = 12.0;

/// How long a new size waits for the next before the terminal takes it (a window being dragged
/// bigger reflows once, at the end).
pub const RESIZE_DEBOUNCE: f64 = 0.09;

/// How long a selection rests before copy-on-select copies it (0.10's settle).
pub const COPY_SETTLE: f64 = 0.25;

/// How long the cursor stays lit after a key before it blinks again.
pub const BLINK_REST: f64 = 0.6;

/// Half a blink: lit this long, then dark this long (xterm.js's 600 ms).
pub const BLINK_HALF: f64 = 0.6;

/// The input method's anchor (where its candidate window opens) follows the cursor while a key
/// was typed this recently, while composing, or once output has been quiet this long; during a
/// flood it stays put, so the window doesn't send a new cursor rectangle every frame (Rune #225,
/// a redraw loop with Wayland input methods).
pub const ANCHOR_TYPING: f64 = 0.5;
pub const ANCHOR_QUIET: Duration = Duration::from_millis(100);

/// 0.10's terminal settings, the ones the prototype has.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneOptions {
    pub font_size: f32,
    pub line_height: f32,
    pub cell_mode: CellMode,
    pub cursor_shape: CursorShape,
    pub blink: bool,
    pub copy_on_select: bool,
    /// Lines of history kept before a session says how many (tmux keeps its own).
    pub history: usize,
    /// Answer a program's "which terminal are you?" (DA1). Off where Windows' console (ConPTY)
    /// sits between the pane and the program: it answers itself, and a second answer would
    /// reach the shell as typed text ("6c" at the prompt, stray keys in vim), as 0.10 found.
    /// Every session on Windows goes through ConPTY today; later the link says.
    pub answer_da1: bool,
    /// The colours: 0.10's, or the 1.0 design's.
    pub scheme: Scheme,
}

impl Default for PaneOptions {
    fn default() -> Self {
        PaneOptions::for_platform(Platform::current())
    }
}

impl PaneOptions {
    /// 0.10's defaults on `platform`.
    pub fn for_platform(platform: Platform) -> Self {
        PaneOptions {
            font_size: 13.0,
            line_height: 1.22,
            cell_mode: CellMode::Floor,
            cursor_shape: CursorShape::Bar,
            blink: true,
            copy_on_select: true,
            history: 10_000,
            answer_da1: platform != Platform::Windows,
            scheme: Scheme::Classic,
        }
    }
}

/// A mouse gesture in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Gesture {
    #[default]
    None,
    /// Pressed on a secret a click may show, at (line, col): shown on release.
    Reveal(usize, u16),
    /// Selecting here.
    Select { moved: bool },
    /// Reporting this button's press, moves and release to the program.
    Report(MouseButton),
    /// A press the terminal ignores (the middle button when the program doesn't want the
    /// mouse): its moves and release too.
    Ignore,
}

/// One terminal pane.
pub struct TermPane {
    pub model: Arc<Model>,
    link: Box<dyn TermLink>,
    status_sink: StatusSink,
    opened: bool,
    wake: Option<Arc<Coalesce>>,
    pub(crate) view: RefCell<PaneView>,
    /// The cell size the view last drew with (the view knows the scale).
    pub(crate) metrics: Cell<Option<CellMetrics>>,
    pub options: PaneOptions,
    platform: Platform,
    /// The terminal area's size in points (padding included), from layout.
    area: Option<[f32; 2]>,
    /// The size the area settled on and when the terminal takes it (each new size restarts
    /// the wait).
    resize_due: Option<f64>,
    pending: Option<Size>,
    link_size: Option<Size>,
    /// The window has the keyboard.
    pub(crate) focused: bool,
    copy_due: Option<f64>,
    gesture: Gesture,
    reporter: MouseReporter,
    wheel: WheelLines,
    wheel_reports: WheelLines,
    /// The session's status, as its link last told it.
    pub status: LinkStatus,
    /// The pointer is over a secret a click shows.
    pub(crate) hover_secret: bool,
    /// The cursor rests lit until this clock (after a key), then blinks.
    pub(crate) blink_rest: Option<f64>,
    /// The blink's state: dark for this half beat, and when the next half begins.
    blink_dark: bool,
    blink_next: Option<f64>,
    clock: f64,
    /// Clock to `Instant` for the model: `None` on the real clock (a live session, whose output
    /// is stamped by the reader thread); a replay runs on Rune's virtual clock from here.
    virtual_base: Option<Instant>,
    /// Frames taken from the model, for the meter.
    pub frames: u64,
    /// The input method's composition and the #602 guard.
    pub(crate) ime: ImeState,
    /// Where the input method's anchor is drawn (view line, column): the cursor's cell, as
    /// the #225 rule lets it follow.
    pub(crate) anchor: Option<(usize, u16)>,
    /// When the user last typed (pane clock).
    typed_at: Option<f64>,
    /// When the newest output on screen was parsed.
    last_output: Option<Instant>,
    meter: Option<SharedMeter>,
    /// A program rang the bell or sent a notification since the window last asked.
    wants_you: bool,
    /// What the last notification said (OSC 9, OSC 777), until the window takes it.
    said: Option<String>,
    /// The shell's working folder, as it last reported it (OSC 7).
    cwd: Option<String>,
}

impl TermPane {
    /// A pane with `link` as its session, which tells its status to `status`. Nothing starts
    /// until the pane is laid out and knows its size.
    pub fn new(link: Box<dyn TermLink>, status: StatusSink, options: PaneOptions) -> Self {
        let size = Size::new(80, 24);
        let config = EngineConfig {
            history: options.history,
            cursor_shape: options.cursor_shape,
            cursor_blinking: options.blink,
            answer_da1: options.answer_da1,
            ..EngineConfig::default()
        };
        let model = Arc::new(Model::new(size, &config));
        model.set_answer_colors(Palette::of(options.scheme, true).answer_colors());
        let mut view = PaneView::new(size, Look { hide_secrets: true, ..Look::default() });
        view.set_scheme(options.scheme);
        TermPane {
            model,
            link,
            status_sink: status,
            opened: false,
            wake: None,
            view: RefCell::new(view),
            metrics: Cell::new(None),
            options,
            platform: Platform::current(),
            area: None,
            resize_due: None,
            pending: None,
            link_size: None,
            focused: true,
            copy_due: None,
            gesture: Gesture::None,
            reporter: MouseReporter::new(),
            wheel: WheelLines::history(),
            wheel_reports: WheelLines::new(),
            status: LinkStatus::Idle,
            hover_secret: false,
            blink_rest: None,
            blink_dark: false,
            blink_next: None,
            clock: 0.0,
            virtual_base: None,
            frames: 0,
            ime: ImeState::default(),
            anchor: None,
            typed_at: None,
            last_output: None,
            meter: None,
            wants_you: false,
            said: None,
            cwd: None,
        }
    }

    /// Measure this pane (`--metrics`, `--hud`).
    pub fn set_meter(&mut self, meter: SharedMeter) {
        self.meter = Some(meter);
    }

    /// The platform's key rules (tests pick one).
    pub fn set_platform(&mut self, platform: Platform) {
        self.platform = platform;
    }

    /// Run the model on Rune's clock (headless replays): times are `base + clock`.
    pub fn use_virtual_clock(&mut self, base: Instant) {
        self.virtual_base = Some(base);
    }

    pub(crate) fn now(&self) -> Instant {
        match self.virtual_base {
            Some(base) => base + Duration::from_secs_f64(self.clock.max(0.0)),
            None => Instant::now(),
        }
    }

    /// Rune's waker, from `AppSpec::setup`: output wakes the window at most once a frame.
    pub fn set_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
        let coalesce = Arc::new(Coalesce::new(waker));
        let c = coalesce.clone();
        self.model.set_waker(Some(Arc::new(move || c.wake())));
        self.wake = Some(coalesce);
    }

    /// Output started a burst the gate holds until `at` (`Model::set_hold_waker`): poll then.
    /// Nothing to draw before it, so the window isn't woken now (that frame would draw the same
    /// picture again).
    pub fn wake_at(&mut self, at: Instant, tx: &mut Tx) {
        self.clock = tx.clock();
        tx.wake_after(at.saturating_duration_since(self.now()));
    }

    /// Asks for a poll soon, past the output flag: the view found the cell size changed.
    pub(crate) fn refit_soon(&self) {
        if let Some(w) = &self.wake {
            w.wake_now();
        }
    }

    /// The size the terminal has now.
    pub fn size(&self) -> Size {
        self.view.borrow().size
    }

    /// The columns and rows the laid-out area holds at the current cell size.
    fn wanted(&self) -> Option<Size> {
        let [w, h] = self.area?;
        let m = self.metrics.get()?;
        let (cols, rows) = m.fit(w - 2.0 * PADDING, h - 2.0 * PADDING);
        Some(Size::new(cols, rows))
    }

    /// The terminal area was laid out (`on_layout_tx`). The first layout opens the session at
    /// once; later ones resize after `RESIZE_DEBOUNCE`.
    pub fn laid_out(&mut self, w: f32, h: f32, tx: &mut Tx) {
        self.area = Some([w, h]);
        self.clock = tx.clock();
        self.check_size(tx);
    }

    fn check_size(&mut self, tx: &mut Tx) {
        let Some(want) = self.wanted() else { return };
        if !self.opened {
            self.apply_size(want);
            self.opened = true;
            self.link.open(self.model.clone(), self.size(), self.status_sink.clone());
            self.link_size = Some(self.size());
            if let (Some(meter), Some(m)) = (&self.meter, self.metrics.get()) {
                let size = self.size();
                let cell_mode = match self.options.cell_mode {
                    CellMode::Floor => "floor",
                    CellMode::Round => "round",
                    CellMode::Exact => "exact",
                };
                meter.borrow_mut().start(StartInfo {
                    scale: m.scale,
                    cell_w: m.cell_w,
                    cell_h: m.cell_h,
                    cell_w_px: m.cell_w_px,
                    cell_h_px: m.cell_h_px,
                    cols: size.cols,
                    rows: size.rows,
                    cell_mode,
                });
            }
            return;
        }
        // Too small for the terminal to take (a tiling window manager squeezing the pane): it
        // keeps its size, so there is nothing to wait for (`Model::resize` would ignore it, and
        // asking again every 90 ms would redraw everything for nothing).
        if !want.is_usable() {
            self.pending = None;
            self.resize_due = None;
            return;
        }
        // Already waiting for this size, or nothing to wait for.
        if self.pending == Some(want) || (self.pending.is_none() && Some(want) == self.link_size) {
            return;
        }
        self.pending = Some(want);
        self.resize_due = Some(self.clock + RESIZE_DEBOUNCE);
        tx.wake_after(Duration::from_secs_f64(RESIZE_DEBOUNCE));
    }

    /// Gives the terminal the size `want`; true when its size changed.
    fn apply_size(&mut self, want: Size) -> bool {
        let before = self.model.size();
        let now = self.model.resize(want);
        if self.opened && now == before && self.view.borrow().size == now {
            return false;
        }
        // Rows of the old size are gone: the next frame has every row.
        let frame = self.model.frame_at(true, self.now());
        self.view.borrow_mut().intake(frame);
        if self.opened && Some(now) != self.link_size {
            self.link.resize(now);
            self.link_size = Some(now);
        }
        true
    }

    /// Whether the session has started (the pane was laid out).
    pub fn is_open(&self) -> bool {
        self.opened
    }

    /// A new size is waiting out its debounce.
    pub fn waiting_to_resize(&self) -> bool {
        self.resize_due.is_some()
    }

    /// The start of every frame (`AppSpec::poll`). True when the view must run again.
    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        self.clock = clock;
        if let Some(w) = &self.wake {
            w.rearm();
        }
        let mut changed = false;
        // Output a replay has due by now.
        if self.opened {
            let now = self.now();
            if let Some(next) = self.link.tick(clock, now) {
                tx.wake_after(Duration::from_secs_f64((next - clock).max(0.0)));
            }
        }
        // A size the area settled on.
        if self.opened {
            self.check_size(tx);
        }
        if let Some(due) = self.resize_due {
            if clock + 1e-6 >= due {
                self.resize_due = None;
                if let Some(want) = self.pending.take() {
                    if Some(want) != self.link_size || want != self.size() {
                        changed |= self.apply_size(want);
                    }
                }
            } else {
                tx.wake_after(Duration::from_secs_f64(due - clock));
            }
        }
        // What the program asked of the terminal.
        for event in self.model.take_events() {
            match event {
                TermEvent::Reply(bytes) => self.link.write(&bytes),
                TermEvent::Copy(text) => {
                    self.link.note_copy(&text);
                    tx.copy(text);
                }
                // A program wants you: the bell, or a notification it sent (OSC 9, OSC 777).
                TermEvent::Bell => self.wants_you = true,
                TermEvent::Osc(OscEvent::Notify(said)) => {
                    self.wants_you = true;
                    self.said = said;
                }
                TermEvent::Osc(OscEvent::Cwd(folder)) => self.cwd = Some(folder),
                _ => {}
            }
        }
        // A selection that rested long enough.
        if let Some(due) = self.copy_due {
            if clock + 1e-6 >= due {
                self.copy_due = None;
                self.copy(tx);
            } else {
                tx.wake_after(Duration::from_secs_f64(due - clock));
            }
        }
        changed |= self.pull(tx);
        changed |= self.blink(clock, tx);
        changed |= self.place_anchor(clock, tx);
        if let Some(meter) = &self.meter {
            let view = self.view.borrow();
            let wait = meter.borrow_mut().poll(clock, self.now(), self.link.bytes_received(), self.frames, view.stats, view.fallbacks());
            if let Some(wait) = wait {
                tx.wake_after(wait);
            }
        }
        if changed {
            tx.instant();
        }
        changed
    }

    /// Moves the input method's anchor to the cursor's cell when the #225 rule lets it (see
    /// `ANCHOR_TYPING`). True when it moved.
    fn place_anchor(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let cursor = {
            let v = self.view.borrow();
            let last = (v.size.rows.max(1) - 1) as usize;
            (v.cursor.line.min(last), v.cursor.col.min(v.size.cols.saturating_sub(1)))
        };
        if self.anchor == Some(cursor) {
            return false;
        }
        let typing = self.typed_at.is_some_and(|t| clock - t < ANCHOR_TYPING);
        let quiet_for = self.last_output.map(|at| self.now().saturating_duration_since(at));
        let quiet = quiet_for.is_none_or(|q| q >= ANCHOR_QUIET);
        if self.anchor.is_none() || typing || self.ime.composing() || quiet {
            self.anchor = Some(cursor);
            return true;
        }
        tx.wake_after(ANCHOR_QUIET.saturating_sub(quiet_for.unwrap_or_default()));
        false
    }

    /// Whether the cursor blinks now: the setting, the program's cursor style, and the window
    /// has the keyboard.
    fn blinks(&self) -> bool {
        self.options.blink && self.focused && self.view.borrow().cursor.blinking
    }

    /// Whether the cursor is drawn this frame (lit, or not blinking).
    pub(crate) fn cursor_lit(&self) -> bool {
        !(self.blinks() && self.blink_rest.is_none() && self.blink_dark)
    }

    /// Steps the blink: lit while resting after a key, then a half beat lit, a half beat dark.
    /// An idle blinking terminal draws a frame each half beat and nothing between them.
    fn blink(&mut self, clock: f64, tx: &mut Tx) -> bool {
        if !self.blinks() {
            let changed = self.blink_dark;
            self.blink_dark = false;
            self.blink_next = None;
            return changed;
        }
        if let Some(rest) = self.blink_rest {
            if clock + 1e-6 < rest {
                tx.wake_after(Duration::from_secs_f64(rest - clock));
                return false;
            }
            // Lit for a whole half beat after the rest.
            self.blink_rest = None;
            self.blink_dark = false;
            self.blink_next = Some(clock + BLINK_HALF);
        }
        let next = *self.blink_next.get_or_insert(clock + BLINK_HALF);
        let mut changed = false;
        if clock + 1e-6 >= next {
            self.blink_dark = !self.blink_dark;
            // From the beat, not from now, so a late frame doesn't stretch the rhythm (but
            // never into the past after a long sleep).
            let mut n = next + BLINK_HALF;
            if n <= clock {
                n = clock + BLINK_HALF;
            }
            self.blink_next = Some(n);
            changed = true;
        }
        tx.wake_after(Duration::from_secs_f64((self.blink_next.unwrap_or(clock) - clock).max(0.0)));
        changed
    }

    /// Takes a frame from the model if one is due. True when one was taken.
    fn pull(&mut self, tx: &mut Tx) -> bool {
        let now = self.now();
        match self.model.due(now) {
            Due::Now => {
                let frame = self.model.frame_at(false, now);
                if frame.last_output.is_some() {
                    self.last_output = frame.last_output;
                }
                if let Some(meter) = &self.meter {
                    meter.borrow_mut().snapshot(frame.last_output, now);
                }
                self.view.borrow_mut().intake(frame);
                self.frames += 1;
                true
            }
            Due::At(at) => {
                tx.wake_after(at.saturating_duration_since(now));
                false
            }
            Due::Idle => false,
        }
    }

    /// Bytes the user sends: through the model (which leaves tmux's copy mode, scrolls to the
    /// bottom and lets the echo show at once) to the session. `via`: a typed input the meter
    /// times until its echo is drawn (`None` for reports).
    fn send(&mut self, bytes: &[u8], via: Option<Via>, tx: &mut Tx) {
        if bytes.is_empty() {
            return;
        }
        let now = self.now();
        let out = self.model.user_input_at(bytes, now);
        self.link.write(&out);
        if input::is_typing(bytes) {
            self.typed_at = Some(tx.clock());
            self.rest_cursor(tx);
            if let (Some(meter), Some(via)) = (&self.meter, via.filter(|_| echoes(bytes))) {
                // On a virtual clock the event's own (wall) time means nothing.
                let input = if self.virtual_base.is_some() { None } else { tx.input_time() };
                meter.borrow_mut().input(via, input, if self.virtual_base.is_some() { now } else { Instant::now() });
            }
        }
    }

    fn rest_cursor(&mut self, tx: &mut Tx) {
        if self.options.blink {
            self.blink_rest = Some(tx.clock() + BLINK_REST);
            self.blink_dark = false;
            tx.wake_after(Duration::from_secs_f64(BLINK_REST));
        }
    }

    /// A key, from the terminal's `on_key` or the window's (nothing focused). True: taken.
    ///
    /// Its frame usually changes nothing on screen (the echo comes from the session a moment
    /// later, and wakes the window): with Rune patch 2 (`desktop/vendor/rune.patch`) such a frame
    /// is built and draws nothing. Not `Tx::live_only`: in Rune a live-only event marks live
    /// values pending, so frames keep coming to latch them and none goes undrawn (measured:
    /// 30–44 drawn frames a second typing 8 keys a second, against 8 without it).
    pub fn key(&mut self, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
        self.clock = tx.clock();
        if self.status.wants_reconnect() {
            self.link.reconnect();
        }
        if let Some(text) = typed_run(ev) {
            let modes = self.view.borrow().modes;
            let text = typed_text(text, modes.bracketed_paste);
            self.send(text.as_bytes(), Some(Via::Key), tx);
            return true;
        }
        let Some(k) = term_key(ev, tx.physical_key(), tx.key_base()) else { return false };
        let has_selection = self.view.borrow().selection.is_some();
        match terminal_copy_key(&k, self.platform) {
            Some(CopyKey::CtrlShiftC) => {
                self.copy(tx);
                return true;
            }
            Some(CopyKey::CtrlC) if has_selection => {
                self.copy(tx);
                self.model.with_engine(|e| e.clear_selection());
                self.pull(tx);
                return true;
            }
            _ => {}
        }
        if self.platform == Platform::Mac && ev.is_command('c') {
            self.copy(tx);
            return true;
        }
        if input::is_terminal_paste(&k, self.platform) {
            tx.request_paste();
            return true;
        }
        if input::is_mac_clear(&k, self.platform) {
            self.model.clear_history();
            self.pull(tx);
            return true;
        }
        let modes = self.view.borrow().modes;
        let rows = self.size().rows as i32;
        match terminal_key(&k, &KeyOptions::new(self.platform, &modes)) {
            KeyAction::Send(s) => self.send(s.as_bytes(), Some(Via::Key), tx),
            KeyAction::PageUp => {
                self.model.scroll_lines(rows);
                self.pull(tx);
            }
            KeyAction::PageDown => {
                self.model.scroll_lines(-rows);
                self.pull(tx);
            }
            KeyAction::SelectAll => {
                self.model.with_engine(|e| e.select_all());
                self.pull(tx);
            }
            KeyAction::Nothing => return false,
        }
        true
    }

    /// Keys taken before Rune's own handling: Linux's Ctrl+V, which Rune would make a paste,
    /// and Return and Escape while Rune's composition state is stale (#602, `ime.rs`).
    pub fn physical(&mut self, input: &PhysicalInput, tx: &mut Tx) -> bool {
        self.clock = tx.clock();
        if let PhysicalInput::Key { key, mods, pressed: true, .. } = *input {
            // By the layout's letter, not the key's place: Dvorak's V is where QWERTY has the
            // period, and its K where QWERTY has V (Ctrl+K must stay ^K). `key_base` reaches
            // this handler through Rune's patch (desktop/vendor/rune.patch).
            if self.platform == Platform::Other && is_linux_ctrl_v(key, mods, tx.key_base()) {
                self.send(b"\x16", Some(Via::Key), tx);
                return true;
            }
        }
        if self.ime.guards_keys() {
            if let Some(k) = guarded_key(input) {
                let modes = self.view.borrow().modes;
                if let KeyAction::Send(s) = terminal_key(&k, &KeyOptions::new(self.platform, &modes)) {
                    self.send(s.as_bytes(), Some(Via::Key), tx);
                }
                return true;
            }
        }
        false
    }

    /// The input method (`App::on_ime`): a composition is drawn over the cursor and sends
    /// nothing; a commit is typed. True: taken.
    ///
    /// A `Preedit` that changes nothing (a Wayland input method answers each cursor rectangle
    /// with an empty one, Rune #225) changes nothing here either, and with Rune patch 2 its
    /// frame draws nothing; patch 1 stops the rectangles that started it.
    pub fn ime(&mut self, ev: &Ime, tx: &mut Tx) -> bool {
        self.clock = tx.clock();
        match ev {
            Ime::Preedit(text, cursor, clauses) => {
                let clauses = clauses.iter().map(|c| (c.range(), c.is_target())).collect();
                if self.ime.set_preedit(text, *cursor, clauses) {
                    // While composing the anchor follows the cursor; and the cursor rests lit.
                    self.place_anchor(self.clock, tx);
                    self.rest_cursor(tx);
                    tx.instant();
                }
                true
            }
            Ime::Commit(text) => {
                if let Some(text) = self.ime.commit(text) {
                    self.type_text(&text, Via::Ime, tx);
                }
                true
            }
            Ime::Disabled => {
                if let Some(text) = self.ime.disabled() {
                    self.type_text(&text, Via::Ime, tx);
                }
                true
            }
            Ime::Enabled => true,
            // Reconversion of committed text: a terminal has none to give back.
            Ime::Select(_) => false,
        }
    }

    /// A paste (⌘V, Ctrl+Shift+V, Windows' Ctrl+V).
    pub fn paste(&mut self, content: &ClipboardContent, tx: &mut Tx) {
        let modes = self.view.borrow().modes;
        let text = pasted_text(content.text(), "", content.html().unwrap_or(""));
        if text.is_empty() {
            // A picture alone: an empty bracketed paste, so an agent reads the clipboard itself.
            if content.image().is_some() && modes.bracketed_paste {
                self.send(paste("", true).as_bytes(), Some(Via::Paste), tx);
            }
            return;
        }
        self.send(paste(&text, modes.bracketed_paste).as_bytes(), Some(Via::Paste), tx);
    }

    /// Typed text that came whole (an input method's commit): a line break in it makes it a
    /// paste.
    fn type_text(&mut self, text: &str, via: Via, tx: &mut Tx) {
        let modes = self.view.borrow().modes;
        self.send(typed_text(text, modes.bracketed_paste).as_bytes(), Some(via), tx);
    }

    /// The window gained or lost the keyboard. (A composition open then is finished by Rune,
    /// which commits it: `ime`.)
    pub fn focus(&mut self, focused: bool, tx: &mut Tx) {
        self.focused = focused;
        let modes = self.view.borrow().modes;
        if let Some(report) = focus_report(focused, &modes) {
            let out = self.model.user_input_at(report, self.now());
            self.link.write(&out);
        }
        let _ = tx;
    }

    /// Copies the selection as shown: hidden secrets copy as their dots (a private key's body
    /// too, wherever the selection starts in it: `Model::copy_text`).
    fn copy(&mut self, tx: &mut Tx) {
        let view = self.view.borrow();
        if let Some(text) = self.model.copy_text(view.look()).filter(|t| !t.is_empty()) {
            self.link.note_copy(&text);
            tx.copy(text);
        }
    }

    /// The cell under a point of the terminal area, clamped to the screen, and which half of
    /// it the point is on.
    pub(crate) fn cell_at(&self, x: f32, y: f32) -> Option<(usize, u16, Side)> {
        let m = self.metrics.get()?;
        let size = self.size();
        let fx = ((x - PADDING) / m.cell_w).max(0.0);
        let fy = ((y - PADDING) / m.cell_h).max(0.0);
        let col = (fx.floor() as u32).min(u32::from(size.cols.saturating_sub(1))) as u16;
        let line = (fy.floor() as usize).min(size.rows.saturating_sub(1) as usize);
        let side = if fx.fract() < 0.5 || fx >= f32::from(size.cols) { Side::Left } else { Side::Right };
        Some((line, col, side))
    }

    fn mouse_report(&mut self, line: usize, col: u16, action: MouseAction, mods: rune::Mods, tx: &mut Tx) {
        let modes = self.view.borrow().modes;
        let ev = MouseEvent { col, row: line as u16, action, shift: mods.shift, alt: mods.alt, ctrl: mods.ctrl };
        if let Some(bytes) = self.reporter.report(ev, &modes, self.size()) {
            self.send(&bytes, None, tx);
        }
    }

    /// The button driving a drag, as a report names it.
    fn button(tx: &Tx) -> MouseButton {
        match tx.button() {
            PointerButton::Middle => MouseButton::Middle,
            PointerButton::Secondary => MouseButton::Right,
            _ => MouseButton::Left,
        }
    }

    /// A press, drag and release of the primary or middle button over the terminal.
    pub fn drag(&mut self, d: &Drag, tx: &mut Tx) {
        self.clock = tx.clock();
        let Some((line, col, side)) = self.cell_at(d.x, d.y) else { return };
        let mods = tx.mods();
        let button = Self::button(tx);
        match d.phase {
            Phase::Down => {
                let modes = self.view.borrow().modes;
                if button != MouseButton::Left {
                    // The middle button is the program's when it wants the mouse; here it
                    // does nothing (pasting the primary selection is for later).
                    self.gesture = if mouse_goes_to_program(&modes, mods.shift, self.platform) {
                        self.mouse_report(line, col, MouseAction::Press(button), mods, tx);
                        Gesture::Report(button)
                    } else {
                        Gesture::Ignore
                    };
                    return;
                }
                if self.view.borrow_mut().revealable_at(line, col) {
                    self.gesture = Gesture::Reveal(line, col);
                    return;
                }
                if mouse_goes_to_program(&modes, mods.shift, self.platform) {
                    self.gesture = Gesture::Report(MouseButton::Left);
                    self.mouse_report(line, col, MouseAction::Press(MouseButton::Left), mods, tx);
                    return;
                }
                let kind = match (d.clicks, mods.alt) {
                    (_, true) => SelectionKind::Block,
                    (2, _) => SelectionKind::Word,
                    (n, _) if n >= 3 => SelectionKind::Lines,
                    _ => SelectionKind::Simple,
                };
                let offset = self.view.borrow().display_offset;
                let at = GridPoint::from_view(line, col, offset);
                self.model.with_engine(|e| e.start_selection(kind, at, side));
                self.gesture = Gesture::Select { moved: d.clicks > 1 };
                self.copy_due = None;
                self.pull(tx);
            }
            Phase::Move => match self.gesture {
                Gesture::Select { .. } => {
                    let offset = self.view.borrow().display_offset;
                    let at = GridPoint::from_view(line, col, offset);
                    self.model.with_engine(|e| e.update_selection(at, side));
                    self.gesture = Gesture::Select { moved: true };
                    self.pull(tx);
                }
                Gesture::Report(b) => {
                    let modes = self.view.borrow().modes;
                    if matches!(modes.mouse, MouseProtocol::Drag | MouseProtocol::Any) {
                        self.mouse_report(line, col, MouseAction::Move(b), mods, tx);
                    }
                }
                _ => {}
            },
            Phase::Up => {
                match std::mem::take(&mut self.gesture) {
                    Gesture::Reveal(l, c) if !d.cancelled() && d.dx.abs() < 4.0 && d.dy.abs() < 4.0 => {
                        if self.view.borrow_mut().reveal(l, c) {
                            tx.instant();
                        }
                    }
                    Gesture::Select { moved: false } => {
                        // A click without a drag: no selection.
                        self.model.with_engine(|e| e.clear_selection());
                        self.pull(tx);
                    }
                    Gesture::Select { moved: true } if self.options.copy_on_select && !d.cancelled() => {
                        self.copy_due = Some(self.clock + COPY_SETTLE);
                        tx.wake_after(Duration::from_secs_f64(COPY_SETTLE));
                    }
                    Gesture::Report(b) => self.mouse_report(line, col, MouseAction::Release(b), mods, tx),
                    _ => {}
                }
            }
        }
    }

    /// The secondary button over the terminal. Rune gives its press only, so a program that
    /// wants the mouse gets the press and the release together (a right-button drag isn't
    /// reported). The terminal's own menu is for later.
    pub fn secondary(&mut self, p: &PointerEvent, tx: &mut Tx) {
        self.clock = tx.clock();
        let Some((line, col, _)) = self.cell_at(p.x, p.y) else { return };
        let mods = tx.mods();
        let modes = self.view.borrow().modes;
        if mouse_goes_to_program(&modes, mods.shift, self.platform) {
            self.mouse_report(line, col, MouseAction::Press(MouseButton::Right), mods, tx);
            self.mouse_report(line, col, MouseAction::Release(MouseButton::Right), mods, tx);
        }
    }

    /// The pointer moved over the terminal (no button): the hand over a secret a click shows,
    /// and hover reports for a program that asked for every move.
    pub fn pointer_moved(&mut self, p: &PointerEvent, tx: &mut Tx) {
        let Some((line, col, _)) = self.cell_at(p.x, p.y) else { return };
        let over = self.view.borrow_mut().revealable_at(line, col);
        let modes = self.view.borrow().modes;
        if modes.mouse == MouseProtocol::Any {
            self.mouse_report(line, col, MouseAction::Move(MouseButton::None), tx.mods(), tx);
        }
        if over != self.hover_secret {
            self.hover_secret = over;
        } else {
            tx.live_only();
        }
    }

    /// The wheel or a trackpad over the terminal. True: taken.
    pub fn wheel(&mut self, w: &WheelEvent, tx: &mut Tx) -> bool {
        let Some(m) = self.metrics.get() else { return false };
        // Rune's dy > 0 moves content down: the view goes up into the history.
        let dy = if w.dy == 0.0 && w.mods.shift { w.dx } else { w.dy };
        let delta = wheel_delta(dy, w.precise, self.platform);
        let fast = w.mods.alt || w.mods.ctrl;
        let size = self.size();
        let modes = self.view.borrow().modes;
        let program = mouse_goes_to_program(&modes, w.mods.shift, self.platform);
        if program {
            let lines = self.wheel_reports.lines(delta, f64::from(m.cell_h), size.rows, fast);
            let Some((line, col, _)) = self.cell_at(w.x, w.y) else { return true };
            let action = if lines < 0 { MouseAction::WheelUp } else { MouseAction::WheelDown };
            for _ in 0..lines.unsigned_abs() {
                self.mouse_report(line, col, action, w.mods, tx);
            }
            return true;
        }
        let lines = self.wheel.lines(delta, f64::from(m.cell_h), size.rows, fast);
        if lines == 0 {
            return true;
        }
        if modes.alt_screen && modes.alternate_scroll {
            let keys = input::alternate_scroll(lines < 0, &modes);
            let bytes: Vec<u8> = keys.iter().copied().cycle().take(keys.len() * lines.unsigned_abs() as usize).collect();
            self.send(&bytes, None, tx);
        } else {
            self.model.scroll_lines(-lines);
            self.pull(tx);
        }
        true
    }

    /// Whether a program in the terminal asked for you (the bell, a notification) since this
    /// was last called. The window marks the tab unless you're looking at it.
    pub fn take_wants_you(&mut self) -> bool {
        std::mem::take(&mut self.wants_you)
    }

    /// What the notification that asked for you said, if it said anything. Taken once.
    pub fn take_said(&mut self) -> Option<String> {
        self.said.take()
    }

    /// The folder the shell said it's in (OSC 7), when it has.
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// When a key was last typed here, on the pane's clock.
    pub fn typed_at(&self) -> Option<f64> {
        self.typed_at
    }

    /// Starts the session of a tab that hasn't been on screen, at `size` (its first layout
    /// gives it its real one): an agent that waits there is read without going to it.
    pub fn open_unseen(&mut self, size: Size) {
        if !self.opened {
            self.apply_size(size);
            self.opened = true;
            self.link.open(self.model.clone(), self.size(), self.status_sink.clone());
            self.link_size = Some(self.size());
        }
    }

    /// A key pressed for you from outside the terminal (an answer picked on an agent's card):
    /// encoded and sent as that key typed here would be, so the program gets what the
    /// keyboard would have sent. By what the key is alone: the event being handled is a
    /// click, or another key. True when something was sent.
    pub fn type_key(&mut self, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
        self.clock = tx.clock();
        let modes = self.view.borrow().modes;
        if let Some(text) = typed_run(ev) {
            self.send(typed_text(text, modes.bracketed_paste).as_bytes(), Some(Via::Key), tx);
            return true;
        }
        let Some(k) = term_key(ev, None, None) else { return false };
        match terminal_key(&k, &KeyOptions::new(self.platform, &modes)) {
            KeyAction::Send(s) => {
                self.send(s.as_bytes(), Some(Via::Key), tx);
                true
            }
            _ => false,
        }
    }

    /// Connect again now: a click on a tab whose connection is gone.
    pub fn reconnect(&mut self) {
        self.link.reconnect();
    }

    /// The tab was closed on purpose: its session ends, on the server too.
    pub fn end(&mut self) {
        self.link.end();
    }

    /// The window closes.
    pub fn close(&mut self) {
        self.link.close();
        if let Some(meter) = &self.meter {
            meter.borrow_mut().finish();
        }
    }
}

/// Input a shell echoes at once: text, Return, Backspace, Tab. Only these are timed: an arrow
/// or a chord may answer with nothing, and the next echo would be counted as its.
fn echoes(bytes: &[u8]) -> bool {
    matches!(bytes, b"\r" | b"\x7f" | b"\x08" | b"\t") || bytes.iter().all(|&b| b >= 0x20 && b != 0x7f)
        || bytes.starts_with(b"\x1b[200~")
}

/// Points Rune moves content by for one notch of a wheel, off Windows (40, or 120 on Linux).
fn line_points() -> f32 {
    if cfg!(target_os = "linux") {
        120.0
    } else {
        40.0
    }
}

/// A wheel event's vertical points (Rune's: positive moves content down) as the terminal's
/// delta (positive goes down the history). A trackpad's are pixels. So are a Windows wheel's:
/// Rune turns a notch into Chromium's pixels there (the system's lines per notch × 100/3, or a
/// page), which is what xterm.js got in 0.10's WebView2, about 4.8 rows a notch at 3 lines.
/// Elsewhere a notch is one line.
pub(crate) fn wheel_delta(dy: f32, precise: bool, platform: Platform) -> WheelDelta {
    if precise || platform == Platform::Windows {
        WheelDelta::Pixels(f64::from(-dy))
    } else {
        WheelDelta::Lines(f64::from(-dy) / f64::from(line_points()))
    }
}

/// Linux's Ctrl+V (^V to the program; Rune would make it a paste): the key whose letter on the
/// layout is "v", or the key in V's place when the layout types a letter beyond ASCII there
/// (Russian's "м", where Ctrl+V is the habit) or doesn't say what it types.
pub(crate) fn is_linux_ctrl_v(key: PhysicalKeyCode, mods: rune::Mods, base: Option<char>) -> bool {
    if !mods.ctrl || mods.shift || mods.alt || mods.logo {
        return false;
    }
    match base {
        Some(c) if c.eq_ignore_ascii_case(&'v') => true,
        Some(c) => key == PhysicalKeyCode::KeyV && !c.is_ascii(),
        None => key == PhysicalKeyCode::KeyV,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool) -> rune::Mods {
        rune::Mods { ctrl, shift, ..rune::Mods::default() }
    }

    /// Linux's Ctrl+V goes by the layout's letter: Dvorak's V is QWERTY's period key and its K
    /// is QWERTY's V; Russian's V place types "м".
    #[test]
    fn ctrl_v_is_the_key_that_types_v() {
        use PhysicalKeyCode::{KeyK, KeyV, Period};
        assert!(is_linux_ctrl_v(KeyV, mods(true, false), Some('v')));
        assert!(is_linux_ctrl_v(Period, mods(true, false), Some('v')), "Dvorak's V");
        assert!(!is_linux_ctrl_v(KeyV, mods(true, false), Some('k')), "Dvorak's K is ^K");
        assert!(is_linux_ctrl_v(KeyV, mods(true, false), Some('м')), "Russian");
        assert!(is_linux_ctrl_v(KeyV, mods(true, false), None), "no layout said");
        assert!(!is_linux_ctrl_v(KeyK, mods(true, false), Some('k')));
        assert!(!is_linux_ctrl_v(KeyV, mods(true, true), Some('v')), "Ctrl+Shift+V pastes");
        assert!(!is_linux_ctrl_v(KeyV, mods(false, false), Some('v')));
    }

    /// One notch of a Windows wheel at 3 lines a notch: Rune says 100 points (Chromium's
    /// pixels), which the history scrolls by as 0.10 did, about 4.8 rows of 21 points.
    #[test]
    fn a_windows_wheel_notch_scrolls_as_in_0_10() {
        assert_eq!(wheel_delta(-100.0, false, Platform::Windows), WheelDelta::Pixels(100.0));
        let mut history = WheelLines::history();
        let rows: Vec<i32> = (0..5).map(|_| history.lines(wheel_delta(-100.0, false, Platform::Windows), 21.0, 30, false)).collect();
        assert_eq!(rows, [4, 5, 5, 5, 4]);
        // "One line a notch" in Windows' settings is 33⅓ points: 1.6 rows, not 0.83.
        let mut one = WheelLines::history();
        assert_eq!((0..3).map(|_| one.lines(wheel_delta(-100.0 / 3.0, false, Platform::Windows), 21.0, 30, false)).sum::<i32>(), 4);
        // A trackpad's pixels anywhere, and a notch as one line elsewhere.
        assert_eq!(wheel_delta(-63.0, true, Platform::Other), WheelDelta::Pixels(63.0));
        assert_eq!(wheel_delta(-line_points(), false, Platform::Other), WheelDelta::Lines(1.0));
    }
}
