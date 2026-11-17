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

#[cfg(test)]
mod tests {
    use super::*;

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
