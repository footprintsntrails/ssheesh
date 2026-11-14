//! What programs tell the terminal through OSC sequences that alacritty_terminal doesn't
//! handle: the working folder (OSC 7), notifications (OSC 9 and OSC 777) and progress (OSC 9;4).
//! And the [`OutputGuard`] every read passes before either parser sees it.
//!
//! A second, tiny `vte::Parser` reads the same bytes the engine parses, before the engine does,
//! and only looks at OSC sequences. The order against the screen's content doesn't matter for
//! these. It is the same parser alacritty uses, so both agree on where a sequence starts and ends.
//!
//! Behaviour is 0.10's (`terminals.ts`, the `registerOscHandler` calls), kept as it was.

use std::borrow::Cow;

use crate::engine::Size;

/// What the side pass found.
#[derive(Clone, Debug, PartialEq)]
pub enum OscEvent {
    /// The shell's working folder (OSC 7, a `file://` URL), as a path.
    Cwd(String),
    /// A program wants you (OSC 9 for iTerm2 and Windows Terminal, OSC 777 `notify` for others).
    /// `None` when it said nothing in words.
    Notify(Option<String>),
    /// A progress report (OSC 9;4, as Windows Terminal popularized); `None` clears it.
    Progress(Option<Progress>),
}

impl Eq for OscEvent {}

/// How far along a program says it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub state: ProgressState,
    /// 0 to 100.
    pub value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProgressState {
    Normal,
    Error,
    Indeterminate,
    Paused,
}

/// Scans bytes for the OSC sequences above. Keeps its place between calls, so a sequence cut
/// between two reads is still found.
///
/// Most output has no OSC at all, so the parser only reads a chunk from its first `ESC ]` on
/// (or all of it while a sequence is open). That gives the same result: in vte an Esc ends any
/// sequence it interrupts, so what came before can't change how an OSC after it is read.
#[derive(Default)]
pub struct OscScanner {
    parser: vte::Parser,
    /// An OSC started and hasn't ended by the end of the last chunk.
    open: bool,
    /// The last chunk ended with an Esc (maybe the first half of `ESC ]`).
    esc_last: bool,
}

const ESC: u8 = 0x1b;

/// Bytes that end an OSC in vte: BEL, CAN, SUB and Esc (the start of `ESC \\` or of anything else).
fn ends_osc(b: u8) -> bool {
    matches!(b, 0x07 | 0x18 | 0x1a | ESC)
}

/// Where the first `ESC ]` starts.
fn first_osc(bytes: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = bytes[from..].iter().position(|&b| b == ESC) {
        let at = from + at;
        if bytes.get(at + 1) == Some(&b']') {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// Whether an OSC is still open after `bytes`, given whether one was open before them.
fn open_after(bytes: &[u8], open_before: bool) -> bool {
    match bytes.windows(2).rposition(|w| w == b"\x1b]") {
        Some(at) => !bytes[at + 2..].iter().copied().any(ends_osc),
        None => open_before && !bytes.iter().copied().any(ends_osc),
    }
}

struct Collect<'a> {
    out: &'a mut Vec<OscEvent>,
}

impl vte::Perform for Collect<'_> {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let Some(event) = osc_event(params) {
            self.out.push(event);
        }
    }
}

impl OscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads `bytes` and adds what it found to `out`.
    pub fn scan(&mut self, bytes: &[u8], out: &mut Vec<OscEvent>) {
        let Some(&first) = bytes.first() else { return };
        let mut collect = Collect { out };
        if self.open {
            self.parser.advance(&mut collect, bytes);
            self.open = open_after(bytes, true);
        } else if self.esc_last && first == b']' {
            // `ESC ]` cut between two reads. A second Esc is harmless: vte ignores it there.
            self.parser.advance(&mut collect, &[ESC]);
            self.parser.advance(&mut collect, bytes);
            self.open = open_after(bytes, true);
        } else if let Some(at) = first_osc(bytes) {
            self.parser.advance(&mut collect, &bytes[at..]);
            self.open = open_after(&bytes[at..], false);
        }
        self.esc_last = bytes.last() == Some(&ESC);
    }
}

/// One OSC sequence, split at `;` by the parser, to the event it means (if one of ours).
pub fn osc_event(params: &[&[u8]]) -> Option<OscEvent> {
    let (&number, rest) = params.split_first()?;
    // Everything after "N;", as 0.10's handlers got it.
    let data = || rest.iter().map(|p| String::from_utf8_lossy(p)).collect::<Vec<_>>().join(";");
    match number {
        b"7" => cwd_from_url(&data()).map(OscEvent::Cwd),
        b"9" => {
            let data = data();
            if data.starts_with("4;") {
                Some(OscEvent::Progress(progress(&data)))
            } else if is_numbered_command(&data) {
                // "9;9;C:\…" and the like are ConEmu's numbered commands (a prompt reporting its
                // folder), not a message for you.
                None
            } else {
                Some(OscEvent::Notify(Some(data).filter(|m| !m.is_empty())))
            }
        }
        b"777" => {
            // "777;notify;title;body": anything after the body is dropped, as before.
            let part = |i: usize| rest.get(i).map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_default();
            if part(0) != "notify" {
                return None;
            }
            let words: Vec<String> = [part(1), part(2)].into_iter().filter(|s| !s.is_empty()).collect();
            Some(OscEvent::Notify(Some(words.join(": ")).filter(|m| !m.is_empty())))
        }
        _ => None,
    }
}

/// Starts with digits and a `;` (`/^\d+;/`).
fn is_numbered_command(data: &str) -> bool {
    let digits = data.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && data.as_bytes().get(digits) == Some(&b';')
}

/// "4;state;value": 0 clears, 1 normal, 2 error, 3 indeterminate, 4 paused. Anything else clears.
fn progress(data: &str) -> Option<Progress> {
    let mut parts = data.split(';');
    parts.next();
    let state = match parts.next().unwrap_or("0") {
        "1" => ProgressState::Normal,
        "2" => ProgressState::Error,
        "3" => ProgressState::Indeterminate,
        "4" => ProgressState::Paused,
        _ => return None,
    };
    let value = parts.next().map(js_number).unwrap_or(0.0);
    let value = if value.is_nan() { 0.0 } else { value.clamp(0.0, 100.0) };
    Some(Progress { state, value: value as f32 })
}

/// JavaScript's `Number(s) || 0` for the decimal numbers a progress report has: blank is 0,
/// anything not a number is 0.
fn js_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    // Rust accepts "inf" and "nan", JavaScript only "Infinity".
    if t.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') {
        return if t == "Infinity" || t == "+Infinity" { f64::INFINITY } else { 0.0 };
    }
    t.parse::<f64>().unwrap_or(0.0)
}

/// The path in an OSC 7 URL (`file://host/path`, percent-encoded), with Windows' drive letter
/// put right (`/C:/Users` → `C:/Users`). `None` when it isn't a URL with a path.
pub fn cwd_from_url(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let valid_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid_scheme {
        return None;
    }
    // The host runs to the first "/"; no path at all is the root, as `new URL` says.
    let path = match rest.find('/') {
        Some(at) => &rest[at..],
        None => "/",
    };
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let path = percent_decode(path)?;
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    Some(if drive { path[1..].to_owned() } else { path })
}

/// `decodeURIComponent`: `None` for a broken `%` escape or bytes that aren't UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// ---------------------------------------------------------------------------------------------
// The guard in front of both parsers.

/// The longest OSC, in bytes after `ESC ]`, by what it is. vte keeps a whole OSC in memory until
/// it ends, in both parsers, and a program can send one that never ends; xterm.js stopped at
/// 10 MB. Titles (OSC 0, 1, 2) a little over [`crate::engine::MAX_TITLE`].
pub const OSC_TITLE: usize = crate::engine::MAX_TITLE + 16;
/// Notifications (OSC 9, OSC 777).
pub const OSC_NOTIFY: usize = 8 * 1024;
/// A program's copy (OSC 52): base64 of [`crate::model::MAX_COPY`] bytes, and its head.
pub const OSC_COPY: usize = crate::model::MAX_COPY / 3 * 4 + 16;
/// Anything else: the working folder, links (OSC 8), colours, sequences nobody reads.
pub const OSC_OTHER: usize = 64 * 1024;

/// Roughly how much parsing one slice of a read may take, in cells written or cleared: a
/// millisecond or so in a release build. See [`OutputGuard`].
pub const SLICE_WORK: usize = 64 * 1024;

/// Bytes put at the end of an OSC cut at its limit, before it is ended, so what it was for fails
/// to read rather than half works: base64 (OSC 52) and UTF-8 (a link) are broken by 0xFF, a
/// `file://` URL (OSC 7) by a `%` with nothing after it. Titles and notifications are kept cut.
const SPOILED: &[u8] = b"\xff%";

/// Ends an OSC early (CAN): vte handles what it has and goes back to plain text.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const BEL: u8 = 0x07;

/// A read after the guard: the bytes to parse, cut into slices that each take a bounded time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guarded<'a> {
    /// The read, or what it became (only when something had to change).
    pub bytes: Cow<'a, [u8]>,
    /// Where each slice ends in `bytes`, and its work (see [`SLICE_WORK`]), in order. The last
    /// ends at the end; none when there is nothing to parse.
    pub slices: Vec<(usize, usize)>,
}

/// Keeps a program's output from making the terminal hold or do unbounded amounts, before
/// either parser sees it. One per terminal; it keeps its place between reads, so a sequence cut
/// between two reads is handled the same.
///
/// - An OSC longer than its limit ([`OSC_TITLE`], [`OSC_NOTIFY`], [`OSC_COPY`], [`OSC_OTHER`])
///   is ended there (with CAN, which vte takes as an end) and the rest of it dropped. Its end
///   (`ESC \\`, BEL) still comes, harmlessly.
/// - REP (`CSI n b`, repeat the last character n times, up to 65,535 times for 8 bytes) is cut
///   to a screen and a row more, keeping where the cursor ends up: anything past a screenful
///   only scrolls the same character through the history.
/// - The read is cut into slices of about [`SLICE_WORK`] each, counting what the bytes make the
///   engine do (an erase of the screen is a screenful of cells), so whoever parses it can let
///   the screen have the terminal in between.
///
/// It follows vte's states closely enough to know where an escape sequence starts and ends,
/// and changes nothing else.
#[derive(Clone, Debug, Default)]
pub struct OutputGuard {
    state: GuardState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GuardState {
    #[default]
    Ground,
    Esc,
    /// After `ESC` and intermediate bytes; `hash` when they were just `#` (`ESC # 8` fills
    /// the screen).
    EscIntermediate {
        hash: bool,
    },
    Csi(Csi),
    Osc(Osc),
    /// Past an OSC's limit: everything up to its end is dropped.
    OscCut,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CsiPhase {
    #[default]
    Entry,
    Param,
    Intermediate,
    /// vte drops the sequence at its end.
    Ignore,
}

/// A CSI as far as it has come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Csi {
    phase: CsiPhase,
    /// A private marker (`?`, `>`) or an intermediate byte came: not plain `CSI n b`.
    marked: bool,
    /// The first parameter, as vte reads it (at most 65,535).
    first: u16,
    /// A `;` or `:` ended the first parameter.
    first_done: bool,
    separators: u8,
}

/// What an OSC is for, by its number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OscKind {
    Title,
    Notify,
    Copy,
    #[default]
    Other,
}

impl OscKind {
    fn of(number: &[u8]) -> Self {
        match number {
            b"0" | b"1" | b"2" => OscKind::Title,
            b"9" | b"777" => OscKind::Notify,
            b"52" => OscKind::Copy,
            _ => OscKind::Other,
        }
    }

    fn limit(self) -> usize {
        match self {
            OscKind::Title => OSC_TITLE,
            OscKind::Notify => OSC_NOTIFY,
            OscKind::Copy => OSC_COPY,
            OscKind::Other => OSC_OTHER,
        }
    }

    /// Cut at a character, without spoiling it: titles and notifications are read cut.
    fn kept_cut(self) -> bool {
        matches!(self, OscKind::Title | OscKind::Notify)
    }
}

/// An OSC as far as it has come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Osc {
    /// Bytes after `ESC ]` so far.
    len: usize,
    /// Its number, while it's being read (the bytes before the first `;`).
    number: [u8; 4],
    number_len: u8,
    /// The number is known (or too long to be one of ours), and so is `kind`.
    number_done: bool,
    kind: OscKind,
}

impl Osc {
    /// Reads the number from the start of the OSC's bytes (`run`, which follows `len` bytes).
    fn read_number(&mut self, run: &[u8]) {
        for &b in run {
            if self.number_done {
                return;
            }
            if b == b';' {
                self.number_done = true;
                self.kind = OscKind::of(&self.number[..self.number_len as usize]);
            } else if (self.number_len as usize) < self.number.len() {
                self.number[self.number_len as usize] = b;
                self.number_len += 1;
            } else {
                self.number_done = true;
                self.kind = OscKind::Other;
            }
        }
    }
}

/// The guard's output: the read as it is until something changes, then a copy.
struct Out<'a> {
    input: &'a [u8],
    owned: Option<Vec<u8>>,
    /// Input before this is decided (kept or dropped).
    from: usize,
}

impl<'a> Out<'a> {
    fn new(input: &'a [u8]) -> Self {
        Out { input, owned: None, from: 0 }
    }

    /// Input up to `to` goes out as it is.
    fn keep_to(&mut self, to: usize) {
        if let Some(owned) = self.owned.as_mut() {
            owned.extend_from_slice(&self.input[self.from..to]);
        }
        self.from = to;
    }

    fn owned(&mut self) -> &mut Vec<u8> {
        let (input, from) = (self.input, self.from);
        self.owned.get_or_insert_with(|| {
            let mut v = Vec::with_capacity(input.len() + 16);
            v.extend_from_slice(&input[..from]);
            v
        })
    }

    /// Input up to `to` is dropped.
    fn drop_to(&mut self, to: usize) {
        if to > self.from {
            self.owned();
            self.from = to;
        }
    }

    fn insert(&mut self, bytes: &[u8]) {
        self.owned().extend_from_slice(bytes);
    }

    /// Length of the output so far (everything decided).
    fn len(&self) -> usize {
        self.owned.as_ref().map_or(self.from, Vec::len)
    }

    fn finish(mut self) -> Cow<'a, [u8]> {
        let end = self.input.len();
        self.keep_to(end);
        match self.owned {
            Some(owned) => Cow::Owned(owned),
            None => Cow::Borrowed(self.input),
        }
    }
}

/// REP's count cut to a screen and a row more, `cols` × (`rows` + 1), keeping the count's
/// remainder by `cols` so the cursor ends where it would have. The screen shows the same.
pub fn rep_limit(count: u16, size: Size) -> u16 {
    let cols = size.cols.max(1) as usize;
    let limit = cols * (size.rows as usize + 1);
    let count = count as usize;
    if count <= limit {
        return count as u16;
    }
    (limit + (count - limit) % cols).min(u16::MAX as usize) as u16
}

impl OutputGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Back to plain text, as after a reset of both parsers.
    pub fn reset(&mut self) {
        self.state = GuardState::Ground;
    }

    /// Guards one read for a terminal of `size`.
    pub fn pass<'a>(&mut self, bytes: &'a [u8], size: Size) -> Guarded<'a> {
        let screen = size.cols as usize * size.rows as usize;
        let mut out = Out::new(bytes);
        let mut slices = Vec::new();
        let mut work = 0usize;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match self.state {
                GuardState::Ground => {
                    // Plain text up to the next Esc (a long run in pieces, for the slices).
                    let room = SLICE_WORK.saturating_sub(work).max(1);
                    let rest = &bytes[i..(i + room).min(bytes.len())];
                    match rest.iter().position(|&c| c == ESC) {
                        Some(at) => {
                            work += at + 1;
                            i += at + 1;
                            self.state = GuardState::Esc;
                        }
                        None => {
                            work += rest.len();
                            i += rest.len();
                        }
                    }
                }
                GuardState::Esc => {
                    i += 1;
                    work += 1;
                    self.state = match b {
                        b'[' => GuardState::Csi(Csi::default()),
                        b']' => GuardState::Osc(Osc::default()),
                        0x20..=0x2f => GuardState::EscIntermediate { hash: b == b'#' },
                        CAN | SUB => GuardState::Ground,
                        ESC => GuardState::Esc,
                        // Executed or ignored: the escape goes on.
                        0x00..=0x1f | 0x7f..=0xff => GuardState::Esc,
                        b'c' => {
                            // A full reset.
                            work += screen;
                            GuardState::Ground
                        }
                        // Dispatched; DCS, SOS, PM and APC strings (`P`, `X`, `^`, `_`) are read
                        // as plain text: vte keeps nothing of them and an Esc ends them too.
                        _ => GuardState::Ground,
                    };
                }
                GuardState::EscIntermediate { hash } => {
                    i += 1;
                    work += 1;
                    self.state = match b {
                        0x20..=0x2f => GuardState::EscIntermediate { hash: false },
                        0x30..=0x7e => {
                            if hash && b == b'8' {
                                // DECALN fills the screen.
                                work += screen;
                            }
                            GuardState::Ground
                        }
                        CAN | SUB => GuardState::Ground,
                        ESC => GuardState::Esc,
                        _ => GuardState::EscIntermediate { hash },
                    };
                }
                GuardState::Csi(mut csi) => {
                    i += 1;
                    work += 1;
                    self.state = GuardState::Csi(csi);
                    match b {
                        CAN | SUB => self.state = GuardState::Ground,
                        ESC => self.state = GuardState::Esc,
                        // Executed in place, or ignored: the sequence goes on.
                        0x00..=0x1f | 0x7f..=0xff => {}
                        0x20..=0x2f => {
                            if csi.phase != CsiPhase::Ignore {
                                csi.marked = true;
                                csi.phase = CsiPhase::Intermediate;
                            }
                            self.state = GuardState::Csi(csi);
                        }
                        0x30..=0x3b => {
                            match csi.phase {
                                CsiPhase::Entry | CsiPhase::Param => {
                                    csi.phase = CsiPhase::Param;
                                    if b.is_ascii_digit() {
                                        if !csi.first_done {
                                            csi.first = csi.first.saturating_mul(10).saturating_add((b - b'0') as u16);
                                        }
                                    } else {
                                        csi.first_done = true;
                                        csi.separators = csi.separators.saturating_add(1);
                                    }
                                }
                                CsiPhase::Intermediate => csi.phase = CsiPhase::Ignore,
                                CsiPhase::Ignore => {}
                            }
                            self.state = GuardState::Csi(csi);
                        }
                        0x3c..=0x3f => {
                            csi.phase = match csi.phase {
                                CsiPhase::Entry => {
                                    csi.marked = true;
                                    CsiPhase::Param
                                }
                                _ => CsiPhase::Ignore,
                            };
                            self.state = GuardState::Csi(csi);
                        }
                        0x40..=0x7e => {
                            self.state = GuardState::Ground;
                            // vte drops a CSI with more than 32 parameters.
                            if csi.phase == CsiPhase::Ignore || csi.separators >= 32 {
                                continue;
                            }
                            let cols = size.cols as usize;
                            work += match b {
                                b'b' if !csi.marked => {
                                    let count = csi.first.max(1);
                                    let cut = rep_limit(count, size);
                                    if cut < count {
                                        // End this one (CAN) and repeat the cut count instead.
                                        out.keep_to(i - 1);
                                        out.drop_to(i);
                                        out.insert(&[CAN]);
                                        out.insert(format!("\x1b[{cut}b").as_bytes());
                                    }
                                    cut as usize
                                }
                                b'J' | b'L' | b'M' | b'S' | b'T' => screen,
                                b'K' | b'@' | b'P' | b'X' => cols,
                                _ => 0,
                            };
                        }
                    }
                }
                GuardState::Osc(mut osc) => {
                    let end = bytes[i..].iter().position(|&c| matches!(c, BEL | CAN | SUB | ESC)).map_or(bytes.len(), |at| i + at);
                    osc.read_number(&bytes[i..end]);
                    let limit = osc.kind.limit();
                    if osc.len + (end - i) > limit {
                        // Past the limit: take what fits, end it, drop the rest.
                        let mut cut = i + limit.saturating_sub(osc.len);
                        if osc.kind.kept_cut() {
                            let floor = cut.saturating_sub(3).max(i);
                            while cut > floor && is_continuation(bytes[cut]) {
                                cut -= 1;
                            }
                        }
                        work += cut - i;
                        out.keep_to(cut);
                        if !osc.kind.kept_cut() {
                            out.insert(SPOILED);
                        }
                        out.insert(&[CAN]);
                        i = cut;
                        self.state = GuardState::OscCut;
                        continue;
                    }
                    osc.len += end - i;
                    work += end - i;
                    i = end;
                    self.state = GuardState::Osc(osc);
                    if let Some(&end_byte) = bytes.get(end) {
                        i += 1;
                        work += 1;
                        self.state = if end_byte == ESC { GuardState::Esc } else { GuardState::Ground };
                    }
                }
                GuardState::OscCut => {
                    let end = bytes[i..].iter().position(|&c| matches!(c, BEL | CAN | SUB | ESC)).map_or(bytes.len(), |at| i + at);
                    out.keep_to(i);
                    out.drop_to(end);
                    i = end;
                    if let Some(&end_byte) = bytes.get(end) {
                        i += 1;
                        work += 1;
                        // The OSC already ended: BEL and CAN go; SUB still shows its mark and
                        // an Esc starts what comes next (`ESC \\` is then a harmless ST).
                        self.state = match end_byte {
                            BEL | CAN => {
                                out.drop_to(i);
                                GuardState::Ground
                            }
                            SUB => GuardState::Ground,
                            _ => GuardState::Esc,
                        };
                    }
                }
            }
            if work >= SLICE_WORK {
                out.keep_to(i.max(out.from));
                slices.push((out.len(), work));
                work = 0;
            }
        }
        let bytes = out.finish();
        if slices.last().map_or(0, |&(end, _)| end) < bytes.len() {
            slices.push((bytes.len(), work));
        }
        Guarded { bytes, slices }
    }
}

fn is_continuation(b: u8) -> bool {
    b & 0xc0 == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(bytes: &[u8]) -> Vec<OscEvent> {
        let mut out = Vec::new();
        OscScanner::new().scan(bytes, &mut out);
        out
    }

    #[test]
    fn working_folder_from_file_url() {
        assert_eq!(scan(b"\x1b]7;file://web-7731/home/dev/my%20site\x07"), [OscEvent::Cwd("/home/dev/my site".into())]);
        // String terminator instead of BEL, and no host.
        assert_eq!(scan(b"\x1b]7;file:///srv/app\x1b\\"), [OscEvent::Cwd("/srv/app".into())]);
        // Windows' drive letter.
        assert_eq!(scan(b"\x1b]7;file://db-main/C:/Users/dev\x07"), [OscEvent::Cwd("C:/Users/dev".into())]);
        // Not a URL, or a broken escape: nothing.
        assert_eq!(scan(b"\x1b]7;/home/dev\x07"), []);
        assert_eq!(scan(b"\x1b]7;file:///bad%zz\x07"), []);
    }

    #[test]
    fn cwd_url_details() {
        assert_eq!(cwd_from_url("file://web-7731").as_deref(), Some("/"));
        assert_eq!(cwd_from_url("file:///tmp/a?x=1#y").as_deref(), Some("/tmp/a"));
        assert_eq!(cwd_from_url("kitty-shell-cwd://web-7731/var/log").as_deref(), Some("/var/log"));
        assert_eq!(cwd_from_url("file:///%D0%B4%D0%BE%D0%BC").as_deref(), Some("/дом"));
        assert_eq!(cwd_from_url("1file:///x"), None);
    }

    #[test]
    fn notifications() {
        assert_eq!(scan(b"\x1b]9;Build finished\x07"), [OscEvent::Notify(Some("Build finished".into()))]);
        // A message with semicolons keeps them.
        assert_eq!(scan(b"\x1b]9;done; 3 warnings\x07"), [OscEvent::Notify(Some("done; 3 warnings".into()))]);
        // ConEmu's numbered commands are not messages.
        assert_eq!(scan(b"\x1b]9;9;C:\\work\x07"), []);
        assert_eq!(scan(b"\x1b]9;\x07"), [OscEvent::Notify(None)]);
        assert_eq!(
            scan(b"\x1b]777;notify;Claude Code;Waiting for input\x07"),
            [OscEvent::Notify(Some("Claude Code: Waiting for input".into()))]
        );
        assert_eq!(scan(b"\x1b]777;notify;;Only body\x07"), [OscEvent::Notify(Some("Only body".into()))]);
        assert_eq!(scan(b"\x1b]777;notify;Title;Body;extra\x07"), [OscEvent::Notify(Some("Title: Body".into()))]);
        assert_eq!(scan(b"\x1b]777;preexec\x07"), []);
    }

    #[test]
    fn progress_reports() {
        let p = |state, value| OscEvent::Progress(Some(Progress { state, value }));
        assert_eq!(scan(b"\x1b]9;4;1;42\x07"), [p(ProgressState::Normal, 42.0)]);
        assert_eq!(scan(b"\x1b]9;4;2;150\x07"), [p(ProgressState::Error, 100.0)]);
        assert_eq!(scan(b"\x1b]9;4;3\x07"), [p(ProgressState::Indeterminate, 0.0)]);
        assert_eq!(scan(b"\x1b]9;4;4;x\x07"), [p(ProgressState::Paused, 0.0)]);
        assert_eq!(scan(b"\x1b]9;4;1;12.5\x07"), [p(ProgressState::Normal, 12.5)]);
        assert_eq!(scan(b"\x1b]9;4;0;0\x07"), [OscEvent::Progress(None)]);
        assert_eq!(scan(b"\x1b]9;4;7;10\x07"), [OscEvent::Progress(None)]);
    }

    #[test]
    fn sequence_cut_between_reads() {
        let mut scanner = OscScanner::new();
        let mut out = Vec::new();
        scanner.scan(b"text \x1b]9;Tests pa", &mut out);
        assert!(out.is_empty());
        scanner.scan(b"ssed\x07 more", &mut out);
        assert_eq!(out, [OscEvent::Notify(Some("Tests passed".into()))]);
    }

    #[test]
    fn cut_in_every_place_still_found() {
        let stream: &[u8] = b"\x1b[31mred\x1b[0m \x1bP1$r\x1b\\ \x1b]9;one\x07 plain \x1b]777;notify;two;x\x1b\\\x1b]7;file:///srv\x1b\\ end\x1b";
        let want = [
            OscEvent::Notify(Some("one".into())),
            OscEvent::Notify(Some("two: x".into())),
            OscEvent::Cwd("/srv".into()),
        ];
        // Whole, then cut in two at every place, then a byte at a time.
        assert_eq!(scan(stream), want);
        for cut in 0..stream.len() {
            let mut scanner = OscScanner::new();
            let mut out = Vec::new();
            scanner.scan(&stream[..cut], &mut out);
            scanner.scan(&stream[cut..], &mut out);
            assert_eq!(out, want, "cut at {cut}");
        }
        let mut scanner = OscScanner::new();
        let mut out = Vec::new();
        for b in stream {
            scanner.scan(std::slice::from_ref(b), &mut out);
        }
        assert_eq!(out, want);
    }

    #[test]
    fn an_osc_ends_at_any_escape_or_cancel() {
        // Ended by CAN: dispatched as it was, as vte does.
        assert_eq!(scan(b"\x1b]9;half\x18\x1b]9;next\x07"), [OscEvent::Notify(Some("half".into())), OscEvent::Notify(Some("next".into()))]);
        // An unfinished OSC across reads, then plain text after it ended.
        let mut scanner = OscScanner::new();
        let mut out = Vec::new();
        scanner.scan(b"\x1b]9;long", &mut out);
        scanner.scan(b" message", &mut out);
        scanner.scan(b"\x07 text without sequences", &mut out);
        scanner.scan(b"more text", &mut out);
        assert_eq!(out, [OscEvent::Notify(Some("long message".into()))]);
    }

    fn guard(chunks: &[&[u8]], size: Size) -> Vec<u8> {
        let mut g = OutputGuard::new();
        let mut out = Vec::new();
        for chunk in chunks {
            let guarded = g.pass(chunk, size);
            assert_eq!(guarded.slices.last().map_or(0, |s| s.0), guarded.bytes.len());
            out.extend_from_slice(&guarded.bytes);
        }
        out
    }

    #[test]
    fn the_guard_leaves_ordinary_output_alone() {
        let stream: &[u8] = "\x1b[1;31mred\x1b[0m привет 漢字 \x1b]0;vim\x07\x1b]8;id=1;https://example.org\x1b\\link\x1b]8;;\x1b\\ \x1b[?2026h\x1b[2J\x1b[H\x1b[5bx\x1b[3b\x1b[?2026l\x1bP1$r\x1b\\\x1b(B\x1b#8\x1b]52;c;aGk=\x07\x1b[?1049h\x1b]9;4;1;50\x1b\\end".as_bytes();
        let size = Size::new(80, 24);
        let guarded = OutputGuard::new().pass(stream, size);
        assert!(matches!(guarded.bytes, Cow::Borrowed(_)));
        assert_eq!(guarded.slices.len(), 1);
        // Cut anywhere, it is still the same bytes.
        for cut in 0..stream.len() {
            assert_eq!(guard(&[&stream[..cut], &stream[cut..]], size), stream, "cut at {cut}");
        }
        let bytes: Vec<&[u8]> = stream.iter().map(std::slice::from_ref).collect();
        assert_eq!(guard(&bytes, size), stream);
    }

    #[test]
    fn a_long_osc_is_ended_at_its_limit() {
        let size = Size::new(80, 24);
        // A title that never ends: cut at the limit, ended with CAN, the rest dropped; the Esc of
        // its real end still goes, and what follows is untouched.
        let mut stream = b"\x1b]2;".to_vec();
        stream.extend(std::iter::repeat_n(b'x', OSC_TITLE * 3));
        stream.extend_from_slice(b"\x1b\\after");
        let out = guard(&stream.chunks(1000).collect::<Vec<_>>(), size);
        let mut want = b"\x1b]2;".to_vec();
        want.extend(std::iter::repeat_n(b'x', OSC_TITLE - 2));
        want.extend_from_slice(b"\x18\x1b\\after");
        assert_eq!(out, want);
        // Cut at a character, never inside one.
        let mut stream = b"\x1b]0;".to_vec();
        while stream.len() < OSC_TITLE + 100 {
            stream.extend_from_slice("ж".as_bytes());
        }
        stream.push(0x07);
        let out = guard(&[&stream], size);
        let body = &out[4..out.len() - 1];
        assert!(std::str::from_utf8(body).is_ok(), "cut inside a character");
        assert_eq!(out.last(), Some(&0x18));
        // A copy too big is spoiled before it ends, so it is never taken; BEL ends it later.
        let mut stream = b"\x1b]52;c;".to_vec();
        stream.extend(std::iter::repeat_n(b'Q', OSC_COPY + 10));
        stream.extend_from_slice(b"\x07z");
        let out = guard(&stream.chunks(1 << 16).collect::<Vec<_>>(), size);
        assert_eq!(out.len(), OSC_COPY + 2 + SPOILED.len() + 1 + 1);
        assert!(out.ends_with(b"\xff%\x18z"));
        // Anything else stops at 64 KiB, also when the number never ends.
        let mut stream = b"\x1b]".to_vec();
        stream.extend(std::iter::repeat_n(b'7', OSC_OTHER + 5));
        stream.extend_from_slice(b"\x1a!");
        let out = guard(&[&stream], size);
        assert_eq!(out.len(), 2 + OSC_OTHER + SPOILED.len() + 1 + 2);
        assert!(out.ends_with(b"\xff%\x18\x1a!"));
    }

    #[test]
    fn rep_is_cut_to_a_screen_and_a_row() {
        let size = Size::new(20, 5);
        assert_eq!(rep_limit(7, size), 7);
        assert_eq!(rep_limit(120, size), 120);
        // 65,535 = 120 + 3270·20 + 15: the cursor ends 15 columns on, as it would have.
        assert_eq!(rep_limit(65535, size), 135);
        assert_eq!(guard(&[b"x\x1b[65535b."], size), b"x\x1b[65535\x18\x1b[135b.");
        // Only plain `CSI n b`, and only when vte would run it.
        for kept in [&b"x\x1b[?65535b"[..], b"x\x1b[65535 b", b"x\x1b[50b", b"x\x1b[b"] {
            assert_eq!(guard(&[kept], size), kept);
        }
        let ignored: Vec<u8> = [&b"x\x1b["[..], &b"1;".repeat(32), b"65535b"].concat();
        assert_eq!(guard(&[&ignored], size), ignored);
        // Cut across reads, and with a control in the middle (run at once by vte).
        assert_eq!(guard(&[b"x\x1b[655", b"\x0735b"], size), b"x\x1b[655\x0735\x18\x1b[135b");
    }

    #[test]
    fn expensive_reads_come_in_slices() {
        let size = Size::new(200, 50);
        // An erase is a screenful of work: a read of nothing but erases is cut into slices.
        let erases = b"x\x1b[2J".repeat(3000);
        let guarded = OutputGuard::new().pass(&erases, size);
        assert!(guarded.slices.len() > 100, "{} slices", guarded.slices.len());
        assert!(guarded.slices.iter().all(|&(_, work)| work < SLICE_WORK + 2 * 10_000));
        assert!(guarded.slices.windows(2).all(|w| w[0].0 < w[1].0));
        // Plain text is one slice per read.
        let text = b"a log line\r\n".repeat(1300);
        assert_eq!(OutputGuard::new().pass(&text, size).slices, [(text.len(), text.len())]);
    }

    #[test]
    fn other_sequences_are_ignored() {
        assert_eq!(scan(b"\x1b]0;title\x07\x1b]52;c;aGVsbG8=\x07\x1b[31mred\x1b[0m\x1b]8;;https://example.org\x07x\x1b]8;;\x07"), []);
    }
}
