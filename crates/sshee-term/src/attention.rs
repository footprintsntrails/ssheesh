//! Programs that want you, and long runs that went quiet: what makes a
//! terminal's tab say "waiting", and the system notification about it.
//!
//! A port of the attention parts of 0.10's `src/lib/terminals.ts`: the bell,
//! OSC 9 and OSC 777 (`notice`, around line 580) and "went quiet after a long
//! run" (`noteOutput`, `checkQuiet`). Where they come from is the engine's
//! (alacritty's bell event, the OSC side scanner in `osc.rs`); what to say
//! and whether to say it is here.
//!
//! One thing 0.10 got wrong is put right: its notification text went to the
//! system as the program sent it, with only names dotted while streaming
//! (`terminals.ts:588`, `privacy.ts:98-104`), so a token in an OSC 9 message
//! landed in the notification center. Here the title and body always go
//! through the secret finder, and through streamer masking while streaming:
//! a notification leaves the app (the lock screen, the notification history,
//! a phone mirroring it), where "show secrets" in Settings means nothing.

use crate::present::Look;
use crate::privacy::host_alias;

/// What a terminal's tab says about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttentionKind {
    /// A program is waiting for you: the bell, or a notification it sent.
    Input,
    /// Output stopped after a long run you weren't watching: it may be done.
    Done,
}

/// A terminal wanting you, as the app keeps it for its tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attention {
    pub kind: AttentionKind,
    /// What the program said, as it said it. Shown in the app through
    /// `Attention::shown_message`, never as it is.
    pub message: Option<String>,
    /// When, in ms since the epoch.
    pub at: u64,
}

impl Attention {
    /// The message as the app may show it (a tab's tooltip, the home screen),
    /// by the same rules as the terminal's own text.
    pub fn shown_message(&self, look: &Look) -> Option<String> {
        self.message.as_deref().map(|m| look.safe_text(m))
    }
}

/// A program asking for you, and what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub message: Option<String>,
}

impl Ask {
    fn saying(message: String) -> Ask {
        Ask { message: (!message.is_empty()).then_some(message) }
    }
}

/// The bell: a program wants you and says nothing more.
pub fn bell() -> Ask {
    Ask { message: None }
}

/// OSC 9 (iTerm2 and Windows Terminal): its text is the message. `9;4;…` is
/// a progress report (`osc.rs`), and `9;9;C:\…` and the like are ConEmu's
/// numbered commands (a prompt reporting its folder), not a message for you.
pub fn osc9(data: &str) -> Option<Ask> {
    if data.starts_with("4;") {
        return None;
    }
    let digits = data.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && data.as_bytes().get(digits) == Some(&b';') {
        return None;
    }
    Some(Ask::saying(data.to_string()))
}

/// OSC 777 (`notify;title;body`, from rxvt and others): "title: body".
pub fn osc777(data: &str) -> Option<Ask> {
    let mut parts = data.split(';');
    if parts.next() != Some("notify") {
        return None;
    }
    let said: Vec<&str> = parts.take(2).filter(|p| !p.is_empty()).collect();
    Some(Ask::saying(said.join(": ")))
}

/// Output that stops for this long ends a run; the next output starts a new one.
pub const RUN_GAP_MS: u64 = 4000;
/// Quiet this long after a run...
pub const QUIET_MS: u64 = 6000;
/// ...that went on at least this long means it's probably done.
pub const LONG_RUN_MS: u64 = 15_000;

/// Watches one terminal's output for a long run that stopped while nobody looked.
#[derive(Debug, Clone, Default)]
pub struct QuietWatch {
    busy_since: Option<u64>,
    last_output: u64,
    noted: bool,
}

impl QuietWatch {
    /// Output came at `now`; `watched`: you're looking at this terminal.
    pub fn output(&mut self, now: u64, watched: bool) {
        if watched {
            self.busy_since = None;
        } else if self.busy_since.is_none() || now.saturating_sub(self.last_output) > RUN_GAP_MS {
            self.busy_since = Some(now);
            self.noted = false;
        }
        self.last_output = now;
    }

    /// Called every few seconds: `true` once for a long unwatched run that stopped.
    pub fn check(&mut self, now: u64) -> bool {
        let Some(busy_since) = self.busy_since else { return false };
        if self.noted || now.saturating_sub(self.last_output) < QUIET_MS || self.last_output.saturating_sub(busy_since) < LONG_RUN_MS {
            return false;
        }
        self.noted = true;
        true
    }
}

/// The machine a terminal is on, for a notification's title.
#[derive(Debug, Clone, Copy)]
pub enum Place<'a> {
    Local,
    /// A saved server: where it is in the server list, and its name.
    Host { index: usize, name: &'a str },
    /// A server no longer in the list.
    Unknown,
}

/// "This computer", the server's name, or its alias ("Server 2") while streaming.
pub fn place_name(place: Place, streaming: bool) -> String {
    match place {
        Place::Local => "This computer".to_string(),
        Place::Unknown => "A server".to_string(),
        Place::Host { index, .. } if streaming => host_alias(Some(index)),
        Place::Host { name, .. } => name.to_string(),
    }
}

/// What the system notification says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// What `notice` decided: the tab's new state, and a notification if one is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub attention: Attention,
    pub notification: Option<Notification>,
}

/// Where a terminal stands when something wants attention.
#[derive(Debug, Clone, Copy)]
pub struct NoticeContext<'a> {
    /// You're looking at this terminal right now: its window has the focus
    /// and is visible, and it's the focused pane.
    pub watched: bool,
    /// SSHeesh's window has the focus.
    pub app_focused: bool,
    /// Settings: notify when a terminal needs you.
    pub notify: bool,
    /// What the tab said already.
    pub previous: Option<AttentionKind>,
    /// The machine's name (`place_name`).
    pub place: &'a str,
    pub look: &'a Look,
}

/// A program wants you (`AttentionKind::Input`), or a run went quiet
/// (`Done`). Nothing while you're watching the terminal. A notification
/// only once per wait, and only while SSHeesh is in the background.
pub fn notice(kind: AttentionKind, message: Option<String>, now: u64, cx: &NoticeContext) -> Option<Notice> {
    if cx.watched {
        return None;
    }
    let attention = Attention { kind, message, at: now };
    let due = cx.previous != Some(kind) && !cx.app_focused && cx.notify;
    let notification = due.then(|| notification(&attention, cx.place, cx.look));
    Some(Notice { attention, notification })
}

/// The notification for an attention, made safe to leave the app.
pub fn notification(a: &Attention, place: &str, look: &Look) -> Notification {
    let title = match a.kind {
        AttentionKind::Input => format!("{place} needs you"),
        AttentionKind::Done => format!("{place} went quiet"),
    };
    let body = a.message.clone().unwrap_or_else(|| {
        match a.kind {
            AttentionKind::Input => "A program in the terminal is waiting for you.",
            AttentionKind::Done => "Output stopped after a long run. It may be done.",
        }
        .to_string()
    });
    Notification { title: for_outside(&title, look, 120), body: for_outside(&body, look, 500) }
}

/// Characters that steer how text shows rather than being text: control
/// codes (but a new line), and the marks that reverse its direction (which
/// could make a notification read as something else).
fn steers(c: char) -> bool {
    (c.is_control() && c != '\n') || matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Text that leaves the app: no control codes, names dotted while streaming,
/// secrets dotted always, and at most `limit` characters. Controls go first,
/// so none can split a token the finder would know (a tab becomes a space);
/// lines stay lines, so a private key's body is still known as one; the cut
/// comes last, so it can't cut a token into a piece the finder wouldn't know.
/// Secrets are looked for as the screen looks for them (`Look::safe_text`):
/// in the text as it came too, so a server's name dotted inside a token
/// can't keep the token from being found.
pub fn for_outside(text: &str, look: &Look, limit: usize) -> String {
    let clean: String = text.chars().map(|c| if c == '\t' { ' ' } else { c }).filter(|c| !steers(*c)).collect();
    let outside = Look { hide_secrets: true, ..look.clone() };
    let safe = outside.safe_text(&clean);
    if safe.chars().count() <= limit {
        return safe;
    }
    let mut cut: String = safe.chars().take(limit.saturating_sub(1)).collect();
    cut.push('…');
    cut
}
