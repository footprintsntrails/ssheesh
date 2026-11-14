//! The prototype's meter (`--metrics FILE`, `--hud`): typing latency split into our own time
//! and the wait for the echo, frames by outcome and what they cost, process CPU and memory,
//! floods. Numbers only: never bytes, text, titles, host names or paths.
//!
//! Records, one JSON object per line (`terminal.md` §7 reads them):
//! - `start`: once, when the pane knows its cells and size;
//! - `key`: a key, input method commit or paste whose echo was drawn, in microseconds from the
//!   input: `written` (to the session), `output` (its echo parsed), `snapshot` (the frame that
//!   held it taken), `encoded` (that frame submitted), and `app` = `encoded − (output −
//!   written)`, our share. `src` says where the input time came from: `os` (the event's own
//!   time) or `handler` (when the handler ran: headless, or a platform that doesn't say);
//! - `second`: every second, frames by outcome and their cost, UI-thread and process CPU,
//!   resident memory, bytes parsed, rows built and reused, characters met that the font lacks;
//! - `flood`: output of 1 MiB or more that then went quiet for 500 ms;
//! - `end`: when the window closes (also printed to stderr).
//!
//! A replay runs on Rune's virtual clock: there its latencies are virtual too (`"clock":
//! "virtual"` in `start`), and only frame costs are real.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rune::{FrameEncoded, FrameOutcome, LiveText};

use crate::terminal::rows::RowStats;

/// What the process has used so far, from the program (it knows the platform's calls).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProcessSample {
    /// User and system CPU time of the whole process.
    pub cpu: Option<Duration>,
    /// CPU time of the thread asking (the UI thread: the meter asks from poll).
    pub thread_cpu: Option<Duration>,
    /// Resident memory in bytes (Linux RSS; macOS's footprint; Windows' private bytes).
    pub memory: Option<u64>,
}

/// Samples the process; called on the UI thread once a second.
pub type Sampler = Box<dyn FnMut() -> ProcessSample>;

/// What the program asked the meter for.
pub struct MeterOptions {
    /// Where records go (`--metrics FILE`); `None` with only `--hud`.
    pub out: Option<Box<dyn Write>>,
    /// A record per frame as well (`--metrics-frames`).
    pub per_frame: bool,
    /// The numbers in the title row (`--hud`).
    pub hud: bool,
    pub sampler: Sampler,
    /// `local`, `ssh` or `replay`.
    pub mode: &'static str,
    /// The pane runs on Rune's virtual clock (a replay).
    pub virtual_clock: bool,
}

/// What a measured input was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Key,
    Ime,
    Paste,
}

impl Via {
    fn name(self) -> &'static str {
        match self {
            Via::Key => "key",
            Via::Ime => "ime",
            Via::Paste => "paste",
        }
    }
}

/// What the `start` record says about the pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StartInfo {
    pub scale: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub cell_w_px: f32,
    pub cell_h_px: f32,
    pub cols: u16,
    pub rows: u16,
    pub cell_mode: &'static str,
}

/// The Rune revision the prototype is pinned to (desktop/Cargo.toml).
pub const RUNE_REV: &str = "c8d77a3f";

/// An input waiting for its echo to be drawn.
#[derive(Clone, Copy, Debug)]
struct Mark {
    via: Via,
    input: Instant,
    written: Instant,
    os: bool,
    output: Option<Instant>,
    snapshot: Option<Instant>,
}

/// An input whose echo never came is forgotten after this long.
const MARK_LIMIT: Duration = Duration::from_secs(2);

/// Output this big that then goes quiet is a flood.
const FLOOD_BYTES: u64 = 1 << 20;
const FLOOD_QUIET: Duration = Duration::from_millis(500);

struct Flood {
    start: Instant,
    last: Instant,
    bytes0: u64,
    snapshots0: u64,
    frame_ms: Vec<f64>,
}

/// Counts for one second.
#[derive(Default)]
struct Second {
    drawn: u64,
    unchanged: u64,
    skipped: u64,
    frame_ms: Vec<f64>,
    keys_ms: Vec<f64>,
}

/// The meter. Shared (`Rc<RefCell<..>>`) by the pane, which marks inputs and frames taken,
/// and the window's frame reports.
pub struct Meter {
    out: Option<Box<dyn Write>>,
    per_frame: bool,
    hud: Option<LiveText>,
    sampler: Sampler,
    mode: &'static str,
    virtual_clock: bool,
    started: bool,
    finished: bool,
    marks: Vec<Mark>,
    keys_total: Vec<f64>,
    keys_app: Vec<f64>,
    frames_ms: Vec<f64>,
    drawn: u64,
    first_drawn: Option<Instant>,
    last_drawn: Option<Instant>,
    second: Second,
    /// When the current second began (pane clock), and the process then.
    second_from: Option<f64>,
    sample_from: Option<(Instant, ProcessSample)>,
    bytes_from: u64,
    rows_from: RowStats,
    fallbacks_from: u64,
    flood: Option<Flood>,
    bytes_seen: u64,
    /// The pane's latest `now` (for a virtual clock, frames are stamped with it).
    now: Option<Instant>,
}

pub type SharedMeter = Rc<RefCell<Meter>>;

/// The `p`-th percentile (0–100) of `v`, nearest rank; `None` when empty.
pub fn percentile(v: &[f64], p: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * s.len() as f64).ceil().max(1.0) as usize;
    Some(s[rank.min(s.len()) - 1])
}

fn us(d: Duration) -> u64 {
    d.as_micros().min(u128::from(u64::MAX)) as u64
}

fn since(later: Instant, earlier: Instant) -> Duration {
    later.saturating_duration_since(earlier)
}

/// A number for a record: three decimals, `null` when there is none.
fn num(v: Option<f64>) -> String {
    match v {
        Some(v) if v.is_finite() => format!("{v:.3}"),
        _ => "null".into(),
    }
}

impl Meter {
    pub fn new(o: MeterOptions) -> Self {
        Meter {
            out: o.out,
            per_frame: o.per_frame,
            hud: o.hud.then(|| LiveText::new("keys p50 –  ·  – fps  ·  – MB")),
            sampler: o.sampler,
            mode: o.mode,
            virtual_clock: o.virtual_clock,
            started: false,
            finished: false,
            marks: Vec::new(),
            keys_total: Vec::new(),
            keys_app: Vec::new(),
            frames_ms: Vec::new(),
            drawn: 0,
            first_drawn: None,
            last_drawn: None,
            second: Second::default(),
            second_from: None,
            sample_from: None,
            bytes_from: 0,
            rows_from: RowStats::default(),
            fallbacks_from: 0,
            flood: None,
            bytes_seen: 0,
            now: None,
        }
    }

    pub fn shared(o: MeterOptions) -> SharedMeter {
        Rc::new(RefCell::new(Meter::new(o)))
    }

    /// The title row's numbers (`--hud`).
    pub fn hud(&self) -> Option<&LiveText> {
        self.hud.as_ref()
    }

    fn record(&mut self, line: &str) {
        if let Some(out) = self.out.as_mut() {
            // A full disk or a closed pipe stops the records, never the terminal.
            if out.write_all(line.as_bytes()).and_then(|_| out.write_all(b"\n")).and_then(|_| out.flush()).is_err() {
                self.out = None;
            }
        }
    }

    /// The `start` record, once.
    pub fn start(&mut self, i: StartInfo) {
        if std::mem::replace(&mut self.started, true) {
            return;
        }
        let line = format!(
            "{{\"rec\":\"start\",\"rune\":\"{}\",\"os\":\"{}\",\"mode\":\"{}\",\"clock\":\"{}\",\"scale\":{},\"cell_pt\":[{},{}],\"cell_px\":[{},{}],\"cols\":{},\"rows\":{},\"cell\":\"{}\"}}",
            RUNE_REV,
            std::env::consts::OS,
            self.mode,
            if self.virtual_clock { "virtual" } else { "real" },
            num(Some(f64::from(i.scale))),
            num(Some(f64::from(i.cell_w))),
            num(Some(f64::from(i.cell_h))),
            i.cell_w_px,
            i.cell_h_px,
            i.cols,
            i.rows,
            i.cell_mode,
        );
        self.record(&line);
    }

    /// A typed input went to the session at `written`; `input` is the event's own time when
    /// the platform gave one.
    pub fn input(&mut self, via: Via, input: Option<Instant>, written: Instant) {
        let os = input.is_some();
        self.marks.retain(|m| since(written, m.written) < MARK_LIMIT);
        self.marks.push(Mark { via, input: input.unwrap_or(written).min(written), written, os, output: None, snapshot: None });
    }

    /// The pane took a frame at `now` whose newest output was parsed at `last_output`: the
    /// newest input written before that output has its echo in it. One output answers one
    /// input: the older ones still waiting got no echo of their own (a password typed at a
    /// prompt that echoes nothing) and go untimed. Timed against the same later output, their
    /// count would be the password's length and their differences the gaps between its keys.
    pub fn snapshot(&mut self, last_output: Option<Instant>, now: Instant) {
        self.now = Some(now);
        let Some(out) = last_output else { return };
        let answered = |m: &Mark| m.output.is_none() && m.written <= out;
        let Some(newest) = self.marks.iter().rposition(answered) else { return };
        self.marks[newest].output = Some(out);
        self.marks[newest].snapshot = Some(now);
        let mut i = 0;
        self.marks.retain(|m| {
            let keep = i == newest || !answered(m);
            i += 1;
            keep
        });
    }

    /// A frame report from the window.
    pub fn frame(&mut self, f: &FrameEncoded) {
        let ms = match (f.update_ms(), f.encode_ms()) {
            (Some(u), Some(e)) => Some(u + e),
            (Some(u), None) => Some(u),
            _ => None,
        };
        match f.outcome() {
            FrameOutcome::Drawn => {
                self.second.drawn += 1;
                self.drawn += 1;
                let at = f.at();
                self.first_drawn.get_or_insert(at);
                self.last_drawn = Some(at);
                if let Some(ms) = ms {
                    self.second.frame_ms.push(ms);
                    self.frames_ms.push(ms);
                    if let Some(flood) = self.flood.as_mut() {
                        flood.frame_ms.push(ms);
                    }
                }
                // On a virtual clock the frame is stamped with the pane's clock, as its
                // inputs and outputs are.
                let encoded = if self.virtual_clock { self.now.unwrap_or(at) } else { at };
                self.close_marks(encoded);
            }
            FrameOutcome::Unchanged => self.second.unchanged += 1,
            _ => self.second.skipped += 1,
        }
        if self.per_frame {
            let outcome = match f.outcome() {
                FrameOutcome::Drawn => "drawn",
                FrameOutcome::Unchanged => "unchanged",
                _ => "skipped",
            };
            let line = format!(
                "{{\"rec\":\"frame\",\"seq\":{},\"outcome\":\"{outcome}\",\"update_ms\":{},\"encode_ms\":{}}}",
                f.seq(),
                num(f.update_ms()),
                num(f.encode_ms())
            );
            self.record(&line);
        }
    }

    fn close_marks(&mut self, encoded: Instant) {
        let done: Vec<Mark> = self.marks.iter().filter(|m| m.snapshot.is_some()).copied().collect();
        self.marks.retain(|m| m.snapshot.is_none());
        for m in done {
            let (Some(output), Some(snapshot)) = (m.output, m.snapshot) else { continue };
            let written = since(m.written, m.input);
            let out = since(output, m.input);
            let snap = since(snapshot, m.input);
            let enc = since(encoded.max(snapshot), m.input);
            // Our share: everything but the wait between writing and the echo.
            let app = enc.saturating_sub(since(output, m.written));
            let total_ms = enc.as_secs_f64() * 1e3;
            let app_ms = app.as_secs_f64() * 1e3;
            self.keys_total.push(total_ms);
            self.keys_app.push(app_ms);
            self.second.keys_ms.push(total_ms);
            let line = format!(
                "{{\"rec\":\"key\",\"via\":\"{}\",\"src\":\"{}\",\"written\":{},\"output\":{},\"snapshot\":{},\"encoded\":{},\"app\":{}}}",
                m.via.name(),
                if m.os { "os" } else { "handler" },
                us(written),
                us(out),
                us(snap),
                us(enc),
                us(app)
            );
            self.record(&line);
        }
    }

    /// From the pane's poll: the second's record when one is due, and floods. `bytes` is all
    /// output the session has had, `snapshots` all frames the pane has taken. Returns how long
    /// until the meter needs another poll.
    pub fn poll(&mut self, clock: f64, now: Instant, bytes: u64, snapshots: u64, rows: RowStats, fallbacks: u64) -> Option<Duration> {
        self.now = Some(now);
        let from = *self.second_from.get_or_insert_with(|| {
            self.bytes_from = bytes;
            self.rows_from = rows;
            self.fallbacks_from = fallbacks;
            clock
        });
        if self.sample_from.is_none() {
            self.sample_from = Some((Instant::now(), (self.sampler)()));
        }
        self.flood_step(now, bytes, snapshots);
        let mut wait = Duration::from_secs_f64((from + 1.0 - clock).max(0.0));
        if clock >= from + 1.0 {
            self.second_record(clock, clock - from, bytes, rows, fallbacks);
            self.second_from = Some(clock);
            wait = Duration::from_secs(1);
        }
        if let Some(f) = &self.flood {
            wait = wait.min(FLOOD_QUIET.saturating_sub(since(now, f.last)));
        }
        Some(wait)
    }

    fn flood_step(&mut self, now: Instant, bytes: u64, snapshots: u64) {
        let new = bytes > self.bytes_seen;
        if new {
            match self.flood.as_mut() {
                Some(f) => f.last = now,
                None => self.flood = Some(Flood { start: now, last: now, bytes0: self.bytes_seen, snapshots0: snapshots, frame_ms: Vec::new() }),
            }
            self.bytes_seen = bytes;
            return;
        }
        let quiet = self.flood.as_ref().is_some_and(|f| since(now, f.last) >= FLOOD_QUIET);
        if !quiet {
            return;
        }
        let Some(f) = self.flood.take() else { return };
        let total = bytes - f.bytes0;
        if total >= FLOOD_BYTES {
            let line = format!(
                "{{\"rec\":\"flood\",\"bytes\":{total},\"seconds\":{},\"snapshots\":{},\"frame_p50\":{},\"frame_p99\":{},\"frame_max\":{}}}",
                num(Some(since(f.last, f.start).as_secs_f64())),
                snapshots - f.snapshots0,
                num(percentile(&f.frame_ms, 50.0)),
                num(percentile(&f.frame_ms, 99.0)),
                num(f.frame_ms.iter().copied().reduce(f64::max)),
            );
            self.record(&line);
        }
    }

    fn second_record(&mut self, clock: f64, span: f64, bytes: u64, rows: RowStats, fallbacks: u64) {
        let sample = (self.sampler)();
        let wall = Instant::now();
        let (ui_ms, cpu_pct) = match self.sample_from {
            Some((then, before)) => {
                let secs = since(wall, then).as_secs_f64();
                let ui = sample.thread_cpu.zip(before.thread_cpu).map(|(a, b)| a.saturating_sub(b).as_secs_f64() * 1e3);
                let cpu = sample.cpu.zip(before.cpu).filter(|_| secs > 0.0).map(|(a, b)| a.saturating_sub(b).as_secs_f64() / secs * 100.0);
                (ui, cpu)
            }
            None => (None, None),
        };
        self.sample_from = Some((wall, sample));
        let s = std::mem::take(&mut self.second);
        let fps = if span > 0.0 { s.drawn as f64 / span } else { 0.0 };
        let mb = sample.memory.map(|b| b as f64 / (1024.0 * 1024.0));
        let line = format!(
            "{{\"rec\":\"second\",\"t\":{},\"drawn\":{},\"unchanged\":{},\"skipped\":{},\"frame_p50\":{},\"frame_p99\":{},\"ui_cpu_ms\":{},\"cpu_pct\":{},\"memory_mb\":{},\"bytes\":{},\"rows_built\":{},\"rows_reused\":{},\"fallbacks\":{}}}",
            num(Some(clock)),
            s.drawn,
            s.unchanged,
            s.skipped,
            num(percentile(&s.frame_ms, 50.0)),
            num(percentile(&s.frame_ms, 99.0)),
            num(ui_ms),
            num(cpu_pct),
            num(mb),
            bytes - self.bytes_from,
            rows.built - self.rows_from.built,
            rows.reused - self.rows_from.reused,
            fallbacks - self.fallbacks_from,
        );
        self.bytes_from = bytes;
        self.rows_from = rows;
        self.fallbacks_from = fallbacks;
        self.record(&line);
        if let Some(hud) = &self.hud {
            let mut text = String::new();
            match percentile(&s.keys_ms, 50.0) {
                Some(k) => {
                    let _ = write!(text, "keys p50 {k:.1} ms");
                }
                None => text.push_str("keys p50 –"),
            }
            let _ = write!(text, "  ·  {fps:.0} fps");
            match mb {
                Some(mb) => {
                    let _ = write!(text, "  ·  {mb:.0} MB");
                }
                None => text.push_str("  ·  – MB"),
            }
            hud.set(text);
        }
    }

    /// The `end` record (and the same on stderr). Once; later calls do nothing.
    pub fn finish(&mut self) {
        if std::mem::replace(&mut self.finished, true) || !self.started {
            return;
        }
        let secs = match (self.first_drawn, self.last_drawn) {
            (Some(a), Some(b)) => since(b, a).as_secs_f64(),
            _ => 0.0,
        };
        let fps = if secs > 0.0 { Some(self.drawn.saturating_sub(1) as f64 / secs) } else { None };
        let p = |v: &[f64], q| num(percentile(v, q));
        let line = format!(
            "{{\"rec\":\"end\",\"keys\":{},\"key_p50\":{},\"key_p95\":{},\"key_p99\":{},\"app_p50\":{},\"app_p95\":{},\"app_p99\":{},\"frames\":{},\"frame_p50\":{},\"frame_p99\":{},\"fps\":{}}}",
            self.keys_total.len(),
            p(&self.keys_total, 50.0),
            p(&self.keys_total, 95.0),
            p(&self.keys_total, 99.0),
            p(&self.keys_app, 50.0),
            p(&self.keys_app, 95.0),
            p(&self.keys_app, 99.0),
            self.drawn,
            p(&self.frames_ms, 50.0),
            p(&self.frames_ms, 99.0),
            num(fps),
        );
        self.record(&line);
        if self.out.is_some() || self.hud.is_some() {
            eprintln!("ssheesh-proto: {line}");
        }
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A `Write` the test can read back.
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);

    impl Write for Lines {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Lines {
        fn records(&self) -> Vec<String> {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap().lines().map(str::to_owned).collect()
        }
    }

    fn meter(out: &Lines) -> Meter {
        let mut m = Meter::new(MeterOptions {
            out: Some(Box::new(out.clone())),
            per_frame: false,
            hud: true,
            sampler: Box::new(|| ProcessSample { cpu: Some(Duration::from_millis(5)), thread_cpu: Some(Duration::from_millis(2)), memory: Some(60 << 20) }),
            mode: "replay",
            virtual_clock: false,
        });
        m.start(StartInfo { scale: 1.0, cell_w: 7.0, cell_h: 21.0, cell_w_px: 7.0, cell_h_px: 21.0, cols: 133, rows: 30, cell_mode: "floor" });
        m
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), Some(50.0));
        assert_eq!(percentile(&v, 99.0), Some(99.0));
        assert_eq!(percentile(&v, 100.0), Some(100.0));
        assert_eq!(percentile(&[3.0], 99.0), Some(3.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn a_key_is_split_into_our_time_and_the_wait_for_the_echo() {
        let out = Lines::default();
        let mut m = meter(&out);
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        m.input(Via::Key, Some(t0), ms(1));
        // A frame without the echo yet: nothing closes.
        m.snapshot(None, ms(2));
        // The echo parsed at 31 ms, taken at 33 ms; a frame drawn then reports at 36 ms.
        m.snapshot(Some(ms(31)), ms(33));
        assert!(out.records().iter().all(|r| !r.contains("\"key\"")));
        m.close_marks(ms(36));
        let recs = out.records();
        let key = recs.iter().find(|r| r.contains("\"rec\":\"key\"")).expect("a key record");
        assert!(key.contains("\"src\":\"os\""), "{key}");
        assert!(key.contains("\"written\":1000,\"output\":31000,\"snapshot\":33000,\"encoded\":36000,\"app\":6000"), "{key}");
        // An input whose echo never comes is forgotten.
        m.input(Via::Paste, None, ms(100));
        m.input(Via::Key, None, ms(100) + MARK_LIMIT + Duration::from_millis(1));
        assert_eq!(m.marks.len(), 1);
    }

    /// Keys with no echo of their own (a password at a prompt that echoes nothing), then one
    /// output (the new line after Return): one record at most, so the file says nothing of how
    /// many keys there were or when each was pressed.
    #[test]
    fn keys_without_an_echo_leave_no_records() {
        let out = Lines::default();
        let mut m = meter(&out);
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        for (i, at) in [0, 180, 310, 520, 600].into_iter().enumerate() {
            m.input(Via::Key, Some(ms(at)), ms(at + 1));
            m.snapshot(None, ms(at + 2 + i as u64));
        }
        m.snapshot(Some(ms(900)), ms(902));
        m.close_marks(ms(905));
        let keys: Vec<String> = out.records().into_iter().filter(|r| r.contains("\"rec\":\"key\"")).collect();
        assert!(keys.len() <= 1, "{keys:?}");
        assert!(m.marks.is_empty());
        // A key whose echo came still is timed.
        m.input(Via::Key, Some(ms(1000)), ms(1001));
        m.snapshot(Some(ms(1003)), ms(1004));
        m.close_marks(ms(1006));
        assert_eq!(out.records().iter().filter(|r| r.contains("\"rec\":\"key\"")).count(), keys.len() + 1);
    }

    #[test]
    fn a_second_record_and_a_flood() {
        let out = Lines::default();
        let mut m = meter(&out);
        let t0 = Instant::now();
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        let rows = RowStats { built: 10, reused: 0 };
        m.poll(0.0, at(0.0), 0, 0, rows, 0);
        // 2 MiB arrive, seen by polls 0.2 s apart, then nothing.
        m.poll(0.2, at(0.2), 1 << 20, 3, rows, 0);
        m.poll(0.4, at(0.4), 2 << 20, 6, RowStats { built: 40, reused: 25 }, 1);
        let wait = m.poll(0.6, at(0.6), 2 << 20, 6, rows, 1).unwrap();
        assert!(wait <= Duration::from_millis(300), "{wait:?}");
        m.poll(0.95, at(0.95), 2 << 20, 6, RowStats { built: 40, reused: 25 }, 1);
        m.poll(1.0, at(1.0), 2 << 20, 6, RowStats { built: 40, reused: 25 }, 1);
        let recs = out.records();
        let flood = recs.iter().find(|r| r.contains("\"rec\":\"flood\"")).expect("a flood record");
        assert!(flood.contains("\"bytes\":2097152,\"seconds\":0.200,\"snapshots\":3"), "{flood}");
        let second = recs.iter().find(|r| r.contains("\"rec\":\"second\"")).expect("a second record");
        assert!(second.contains("\"bytes\":2097152,\"rows_built\":30,\"rows_reused\":25,\"fallbacks\":1"), "{second}");
        assert!(second.contains("\"memory_mb\":60.000"), "{second}");
        assert_eq!(&*m.hud().unwrap().get(), "keys p50 –  ·  0 fps  ·  60 MB");
        // Nothing in any record is text from the terminal.
        assert!(recs.iter().all(|r| !r.contains("web-")));
    }

    #[test]
    fn end_once() {
        let out = Lines::default();
        let mut m = meter(&out);
        m.finish();
        m.finish();
        drop(m);
        assert_eq!(out.records().iter().filter(|r| r.contains("\"rec\":\"end\"")).count(), 1);
    }
}
