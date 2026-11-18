//! Tor as the window shows it: its state in plain words, the round trip through it, and the
//! last lines of its log (0.10's `TorButton.tsx` and `src/lib/tor.ts`).
//!
//! sshee-core's `TorStatus` is mirrored here as [`Status`], so the screen's rules are tested
//! without a network or a window. The sidebar's onion and the status bar read the short
//! [`crate::workbench::Tor`]; the Tor popover reads all of it.

use crate::workbench::Tor;

/// Tor's state, with what sshee-core says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Off,
    /// `percent` of the way; `note` says what holds it up, when Tor knows (a wrong clock, a
    /// network that blocks Tor).
    Starting { percent: u8, note: Option<String> },
    On,
    /// It didn't start. It tries again by itself; meanwhile nothing connects.
    Failed { message: String },
}

impl Status {
    /// What the sidebar and the status bar show.
    pub fn shown(&self, latency_ms: Option<u32>) -> Tor {
        match self {
            Status::Off => Tor::Off,
            Status::Starting { .. } => Tor::Starting,
            Status::On => Tor::On { latency_ms },
            Status::Failed { .. } => Tor::Failed,
        }
    }

    /// Whether the switch is on: starting and failed count, since nothing goes around Tor
    /// then either.
    pub fn switched_on(&self) -> bool {
        *self != Status::Off
    }
}

/// What the Tor popover knows besides the state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detail {
    pub status: Status,
    /// The round trip on the route picked last, once a server has been reached.
    pub latency_ms: Option<u32>,
    /// The last lines of Tor's log, as [`log_tail`] makes them; empty until read.
    pub log: Vec<String>,
    /// Something the switch couldn't do ("couldn't note that for next time").
    pub warning: Option<String>,
}

/// What the window asks the backend about Tor, besides Settings' switch and buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// The last lines of Tor's log, for the popover.
    ReadLog,
    /// The folder with Tor's log, in the system's file manager.
    ShowLog,
}

/// The popover's title.
pub fn headline(status: &Status) -> &'static str {
    match status {
        Status::Off => "Tor is off",
        Status::Starting { .. } => "Tor is starting",
        Status::On => "Tor is on",
        Status::Failed { .. } => "Tor can't start",
    }
}

/// The popover's line under its title, in plain words.
pub fn body(status: &Status, latency_ms: Option<u32>) -> String {
    match status {
        Status::Off => "Servers see your own address. With Tor on they see a Tor address instead, and your internet provider doesn't see which servers you use.".into(),
        Status::Starting { note: Some(note), .. } => note.clone(),
        Status::Starting { .. } => "Connecting to the Tor network. Your servers wait until it's done: nothing connects around Tor.".into(),
        Status::On => match latency_ms {
            Some(ms) => format!("Servers see a Tor address, not yours. A round trip to the server you reached last takes {ms} ms."),
            None => "Servers see a Tor address, not yours. The round trip shows here once a server is reached through Tor.".into(),
        },
        Status::Failed { message } => format!("{} Nothing connects until Tor does; it keeps trying by itself.", sentence(message)),
    }
}

/// A message as a sentence: what sshee-core says sometimes ends without a full stop.
fn sentence(message: &str) -> String {
    let m = message.trim();
    if m.ends_with(['.', '!', '?']) {
        m.to_owned()
    } else {
        format!("{m}.")
    }
}

/// The sidebar onion's tooltip and what a screen reader says of it.
pub fn rail_help(status: &Status) -> String {
    match status {
        Status::Off => "Tor: off".into(),
        Status::Starting { percent, .. } => format!("Tor: starting, {percent}%"),
        Status::On => "Tor: on".into(),
        Status::Failed { .. } => "Tor: can't start".into(),
    }
}

/// How far a start has got, from 0 to 1, for the bar.
pub fn progress(status: &Status) -> Option<f32> {
    match status {
        Status::Starting { percent, .. } => Some(f32::from((*percent).min(100)) / 100.0),
        _ => None,
    }
}

/// How many lines of the log the popover shows.
pub const LOG_LINES: usize = 6;

/// The longest a log line is shown.
const LOG_WIDTH: usize = 160;

/// The last `n` lines of Tor's log, as the popover shows them: without the time (it's UTC in
/// the file, which would read wrong here) and the module, with warnings and errors said so.
/// Tor's own "safe logging" leaves addresses out of the file, and the app adds no server's name.
pub fn log_tail(text: &str, n: usize) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().rev().filter_map(log_line).take(n).collect();
    lines.reverse();
    lines
}

/// One line of the file (`2026-11-14T20:38:37.123Z  INFO tor_dirmgr: Marked consensus usable.`).
fn log_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (first, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim_start()));
    // A line without the time and the level is shown as it is.
    let looks_timed = first.len() >= 19 && first.as_bytes()[4] == b'-' && first.contains('T');
    let (level, rest) = if looks_timed { rest.split_once(char::is_whitespace).unwrap_or((rest, "")) } else { ("", line) };
    let rest = rest.trim_start();
    // The module, up to its colon: "tor_dirmgr: ".
    let message = match rest.split_once(": ") {
        Some((module, message)) if !module.contains(' ') && module.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':')) => message,
        _ => rest,
    };
    let message: String = message.chars().filter(|c| !c.is_control()).collect();
    let message = message.trim();
    // SSHeesh's own note of each state: the popover says the state already.
    if message.is_empty() || message.starts_with("status: ") {
        return None;
    }
    let shown = match level {
        "WARN" => format!("Warning: {message}"),
        "ERROR" => format!("Error: {message}"),
        _ => message.to_owned(),
    };
    Some(if shown.chars().count() > LOG_WIDTH { format!("{}…", shown.chars().take(LOG_WIDTH - 1).collect::<String>()) } else { shown })
}

/// How long Tor gets to start before you're told it hasn't, and why (0.10's `SLOW_START_MS`).
pub const SLOW_START_SECS: f64 = 25.0;

/// A toast's tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Error,
}

/// What a toast about Tor says (0.10's `src/lib/tor.ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub tone: Tone,
    pub title: String,
    pub body: String,
    /// It offers "Show log".
    pub show_log: bool,
}

impl Note {
    fn new(tone: Tone, title: &str, body: impl Into<String>) -> Note {
        Note { tone, title: title.into(), body: body.into(), show_log: false }
    }
}

/// " A terminal on a server without tmux stays as it is until it reconnects.": what a switch
/// leaves on the old way, after a sentence; empty when nothing stayed.
pub fn stayed_note(stayed: usize) -> String {
    match stayed {
        0 => String::new(),
        1 => " A terminal on a server without tmux stays as it is until it reconnects.".into(),
        n => format!(" {n} terminals on servers without tmux stay as they are until they reconnect."),
    }
}

/// Tor was turned off: servers see you again, and what stayed on Tor.
pub fn turned_off(stayed: usize) -> Note {
    Note::new(Tone::Info, "Tor is off", format!("Servers see your own address again.{}", stayed_note(stayed)))
}

/// Tor was turned on: what moves to it now. Nothing when everything did (it says "Tor is on"
/// once it is: [`Follow`]).
pub fn turned_on(stayed: usize) -> Option<Note> {
    (stayed > 0).then(|| Note::new(Tone::Info, "Moving your terminals to Tor", stayed_note(stayed).trim()))
}

/// "New routes" was pressed.
pub fn new_routes(stayed: usize) -> Note {
    Note::new(Tone::Info, "New Tor routes", format!("Terminals reconnect on each server's fastest new route.{}", stayed_note(stayed)))
}

/// What has been said about Tor's state, so each thing is said once (0.10's `follow`): "Tor is
/// on" once it is after you turned it on, and "isn't connected yet" when it failed or took
/// longer than [`SLOW_START_SECS`] to start.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Follow {
    /// Turned on by you: "Tor is on" is said when it is.
    announce_on: bool,
    /// The trouble said last, not said again.
    told: String,
    /// While starting: when it has taken too long, on the window's clock (set at the first
    /// tick after it started).
    slow_at: Option<f64>,
}

impl Follow {
    /// You turned Tor on or off, or started it afresh.
    pub fn switching(&mut self, on: bool) {
        self.announce_on = on;
        if on {
            self.told.clear();
        }
    }

    /// Tor's state changed: what to say.
    pub fn status(&mut self, status: &Status) -> Option<Note> {
        if !matches!(status, Status::Starting { .. }) {
            self.slow_at = None;
        }
        match status {
            Status::On => {
                self.told.clear();
                std::mem::take(&mut self.announce_on).then(|| Note::new(Tone::Success, "Tor is on", "Servers see a Tor address instead of yours, and your internet provider doesn't see which servers you use."))
            }
            Status::Off => {
                self.announce_on = false;
                self.told.clear();
                None
            }
            Status::Failed { message } => self.trouble(message),
            Status::Starting { .. } => None,
        }
    }

    /// Every frame, at `now` on the window's clock: what to say when a start has taken too
    /// long, and when to look again.
    pub fn tick(&mut self, status: &Status, now: f64) -> (Option<Note>, Option<f64>) {
        let Status::Starting { note, .. } = status else {
            self.slow_at = None;
            return (None, None);
        };
        match self.slow_at {
            None => {
                let at = now + SLOW_START_SECS;
                self.slow_at = Some(at);
                (None, Some(at))
            }
            Some(at) if now >= at => {
                self.slow_at = Some(f64::INFINITY);
                (self.trouble(note.as_deref().unwrap_or("It's taking longer than it should.")), None)
            }
            Some(at) => (None, at.is_finite().then_some(at)),
        }
    }

    fn trouble(&mut self, why: &str) -> Option<Note> {
        let why = sentence(why);
        if self.told == why {
            return None;
        }
        self.told.clone_from(&why);
        let mut note = Note::new(Tone::Error, "Tor isn't connected yet", format!("{why} Your servers wait until it is: SSHeesh won't connect them around Tor."));
        note.show_log = true;
        Some(note)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_tor_on_says_so_once_it_is() {
        let mut f = Follow::default();
        // On from the last run: nothing to say.
        assert_eq!(f.status(&Status::On), None);
        f.switching(true);
        assert_eq!(f.status(&Status::Starting { percent: 10, note: None }), None);
        let on = f.status(&Status::On).unwrap();
        assert_eq!((on.tone, on.title.as_str()), (Tone::Success, "Tor is on"));
        assert!(on.body.contains("Tor address instead of yours"));
        // Once.
        assert_eq!(f.status(&Status::On), None);
        // Turned off before it came on: nothing.
        f.switching(true);
        f.status(&Status::Off);
        assert_eq!(f.status(&Status::On), None);
    }

    #[test]
    fn a_start_that_fails_or_takes_too_long_is_said_once_with_its_log() {
        let mut f = Follow::default();
        f.switching(true);
        let starting = Status::Starting { percent: 30, note: Some("Your network may block Tor".into()) };
        assert_eq!(f.status(&starting), None);
        // The first tick starts the wait; nothing before 25 seconds.
        assert_eq!(f.tick(&starting, 100.0), (None, Some(125.0)));
        assert_eq!(f.tick(&starting, 110.0), (None, Some(125.0)));
        let (note, again) = f.tick(&starting, 125.5);
        let note = note.unwrap();
        assert_eq!((note.tone, note.title.as_str(), note.show_log, again), (Tone::Error, "Tor isn't connected yet", true, None));
        assert_eq!(note.body, "Your network may block Tor. Your servers wait until it is: SSHeesh won't connect them around Tor.");
        assert_eq!(f.tick(&starting, 200.0), (None, None), "said once");
        // A failure with the same words isn't said again; other words are.
        assert_eq!(f.status(&Status::Failed { message: "Your network may block Tor.".into() }), None);
        assert!(f.status(&Status::Failed { message: "Tor's clock is wrong".into() }).is_some());
        // Without a note, it says it's slow.
        let mut f = Follow::default();
        let plain = Status::Starting { percent: 5, note: None };
        f.tick(&plain, 0.0);
        assert!(f.tick(&plain, 30.0).0.unwrap().body.starts_with("It's taking longer than it should."));
        // Coming on in time says nothing about trouble, and the wait starts again next time.
        let mut f = Follow::default();
        f.tick(&plain, 0.0);
        f.status(&Status::On);
        assert_eq!(f.tick(&Status::On, 40.0), (None, None));
        assert_eq!(f.tick(&plain, 50.0), (None, Some(75.0)));
    }

    #[test]
    fn switching_says_what_stayed_on_the_old_way() {
        assert_eq!(turned_off(0), Note::new(Tone::Info, "Tor is off", "Servers see your own address again."));
        assert_eq!(turned_off(1).body, "Servers see your own address again. A terminal on a server without tmux stays as it is until it reconnects.");
        assert_eq!(turned_on(0), None);
        let moving = turned_on(2).unwrap();
        assert_eq!((moving.title.as_str(), moving.body.as_str()), ("Moving your terminals to Tor", "2 terminals on servers without tmux stay as they are until they reconnect."));
        assert_eq!(new_routes(0).body, "Terminals reconnect on each server's fastest new route.");
        assert_eq!(new_routes(0).title, "New Tor routes");
    }

    #[test]
    fn the_sidebar_and_the_status_bar_read_the_short_state() {
        assert_eq!(Status::Off.shown(Some(84)), Tor::Off);
        assert_eq!(Status::Starting { percent: 40, note: None }.shown(None), Tor::Starting);
        assert_eq!(Status::On.shown(Some(84)), Tor::On { latency_ms: Some(84) });
        assert_eq!(Status::Failed { message: "x".into() }.shown(None), Tor::Failed);
        assert!(!Status::Off.switched_on() && Status::Failed { message: "x".into() }.switched_on());
    }

    #[test]
    fn every_state_is_said_in_plain_words() {
        assert_eq!(headline(&Status::Off), "Tor is off");
        assert!(body(&Status::Off, None).contains("your own address"));
        let held = Status::Starting { percent: 15, note: Some("A VPN can do this.".into()) };
        assert_eq!(body(&held, None), "A VPN can do this.");
        assert!(body(&Status::Starting { percent: 15, note: None }, None).contains("nothing connects around Tor"));
        assert!(body(&Status::On, Some(84)).contains("84 ms"));
        assert!(body(&Status::On, None).contains("once a server is reached"));
        let failed = body(&Status::Failed { message: "Can't reach the Tor network".into() }, None);
        assert!(failed.starts_with("Can't reach the Tor network. Nothing connects"), "{failed}");
        assert_eq!(rail_help(&Status::Starting { percent: 47, note: None }), "Tor: starting, 47%");
        assert_eq!(progress(&Status::Starting { percent: 47, note: None }), Some(0.47));
        assert_eq!(progress(&Status::On), None);
    }

    #[test]
    fn the_log_shows_its_last_lines_without_time_or_module() {
        let file = "\
2026-11-14T20:38:37.123456Z  INFO ssheesh: SSHeesh 0.10.14 on linux x86_64, this computer's clock says 1794600000 (seconds since 1970)
2026-11-14T20:38:38.000001Z  INFO tor_dirmgr: Marked consensus usable.

2026-11-14T20:38:39.000001Z  WARN tor_chanmgr::mgr: Problem while bootstrapping: timed out
2026-11-14T20:38:40.000001Z ERROR arti_client: \u{1b}[1mgave up\u{1b}[0m
2026-11-14T20:38:41.000001Z  INFO ssheesh: status: Starting { percent: 36, note: None }
plain words without a time
";
        let tail = log_tail(file, 4);
        assert_eq!(tail, ["Marked consensus usable.", "Warning: Problem while bootstrapping: timed out", "Error: [1mgave up[0m", "plain words without a time"]);
        assert_eq!(log_tail(file, 1), ["plain words without a time"]);
        assert!(log_tail("", 8).is_empty());
        let long = format!("2026-11-14T20:38:40.000001Z  INFO tor_proto: {}", "x".repeat(400));
        assert_eq!(log_tail(&long, 1)[0].chars().count(), LOG_WIDTH);
    }
}
