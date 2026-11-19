//! "Type the password here once and SSHeesh remembers it": a quiet strip above a terminal
//! whose session waits at ssh's password prompt while it connects, for a server with no
//! saved password (one from `~/.ssh/config`, one added without a password).
//!
//! ```text
//! term-box
//! ├── pw-strip   above the terminal, never over the prompt: the words, pw-strip-field (a
//! │              secure field), pw-strip-save
//! └── term
//! ```
//!
//! It shows only while the line the cursor is on is ssh's password prompt
//! (`sshee_app::servers::asks_for_password`: never a key's passphrase, a host key's question
//! or a code) and the session is still connecting: once the shell is up, a prompt there is a
//! program's (sudo's), never offered. And only while the session's link says ssh asks for
//! this server's own password ([`TermLink::password_prompt`]): worded exactly as ssh words
//! it for the configured user and host, with no jump host on the way (whose prompt would be
//! saved as this server's), and nothing but ssh's own lines printed before it this attempt
//! (a server that signed in by key runs login scripts that could print a fake one). Typing
//! into the terminal works as ever; the strip just goes when the prompt does.
//!
//! Save sends the password and Return to the waiting prompt (nothing is echoed, logged or
//! drawn) and keeps it, in memory only, until the server lets the terminal in: then the window
//! asks the backend to keep it in the keychain (`Wish::SavePassword`, `Ask::SavePassword`).
//! One the server refuses, ssh asks again (a new prompt on a new line, as the link counts
//! them): it is dropped, never saved, and the strip says so. The field is a secure one (no
//! screen reader, trace or tree holds it), its text wiped from memory when the password is
//! sent or the prompt goes; so is the copy written to the session.

use rune::{row, text, text_input, El, Key, TextEdit, Tx};
use sshee_app::servers::{asks_for_password, Password};

use super::overlays::Wish;
use super::pane::TermPane;
use super::LinkStatus;
use crate::icons::{icon, Icon};
use crate::kit::field::field;
use crate::kit::push::{push_button, Push};
use crate::theme::{self, Theme};

/// What the strip says.
pub const OFFER: &str = "Type the password here once and SSHeesh remembers it";
/// What it says after the server refused the password typed there.
pub const REFUSED: &str = "That password didn't work. Type it again";

/// A pane's strip.
pub struct Strip {
    /// The server has a saved password: nothing to offer.
    saved: bool,
    pub(crate) field: TextEdit,
    /// Drawn this frame.
    shown: bool,
    /// The password sent to the prompt, kept until the server lets the terminal in.
    sent: Option<Password>,
    /// Which prompt it answered ([`TermLink::password_prompt`]): a later one is ssh asking
    /// again, the server having refused it.
    ///
    /// [`TermLink::password_prompt`]: super::TermLink::password_prompt
    answered: u32,
    /// The server refused the one sent here, and ssh asks again.
    refused: bool,
}

impl Default for Strip {
    fn default() -> Strip {
        Strip { saved: false, field: TextEdit::default().secure(), shown: false, sent: None, answered: 0, refused: false }
    }
}

/// What was typed in a secure `field`, moved (never copied) into a [`Password`], which wipes
/// it from memory when it goes; the field starts empty again. Rune's `TextEdit` wipes its own
/// text, and its undo steps', when it's dropped: emptying a field is replacing it.
pub(crate) fn take_password(field: &mut TextEdit) -> Password {
    let mut old = std::mem::replace(field, TextEdit::default().secure());
    Password::new(std::mem::take(&mut old.text))
}

impl TermPane {
    /// Whether the pane's server has a saved password (`Face::saved_password`): then ssh gets
    /// it by itself, and nothing is offered.
    pub fn set_saved_password(&mut self, saved: bool) {
        self.over.password.saved = saved;
    }

    /// ssh asks for this server's own password on the line the cursor is on, while the
    /// session connects to a server: which such prompt it is this attempt (the link's count).
    pub(crate) fn password_prompt(&self) -> Option<u32> {
        if !self.over.session.on_server || !matches!(self.status, LinkStatus::Connecting | LinkStatus::Rejoining { .. }) {
            return None;
        }
        if !self.view.borrow().cursor_row_text().is_some_and(|line| asks_for_password(&line)) {
            return None;
        }
        self.link.password_prompt()
    }

    /// Whether the strip shows.
    pub fn offers_password(&self) -> bool {
        self.over.password.shown
    }

    /// The strip's part of every poll: whether it shows, and a password sent from it saved
    /// once the server let the terminal in. True when what is drawn changed.
    pub(crate) fn poll_password(&mut self) -> bool {
        let prompt = self.password_prompt();
        let at_prompt = prompt.is_some();
        let p = &mut self.over.password;
        let mut save = None;
        if p.sent.is_some() {
            match self.status {
                LinkStatus::Connected { .. } => save = p.sent.take(),
                // ssh asks again, on a new line after the answer ("Permission denied" between):
                // the server refused it. Dropped, never saved. The same prompt drawn again (a
                // console's repaint) is no new one.
                LinkStatus::Connecting | LinkStatus::Rejoining { .. } if prompt.is_some_and(|n| n > p.answered) => {
                    p.sent = None;
                    p.refused = true;
                }
                LinkStatus::Connecting | LinkStatus::Rejoining { .. } => {}
                // It stopped, or dropped: nothing was proven.
                _ => p.sent = None,
            }
        }
        let shows = at_prompt && !p.saved && p.sent.is_none();
        if !shows && p.shown {
            // Answered in the terminal, or the session moved on: what was typed here goes
            // (the old field wipes it as it's dropped).
            p.field = TextEdit::default().secure();
            p.refused = false;
        }
        let changed = shows != p.shown;
        p.shown = shows;
        if let Some(password) = save {
            self.over.wish(Wish::SavePassword(password));
        }
        changed
    }

    /// The strip's Save, and Return in its field: the password answers the prompt, and the
    /// terminal has the keyboard again.
    pub(crate) fn send_password(&mut self, tx: &mut Tx) {
        let Some(prompt) = self.password_prompt() else { return };
        if !self.over.password.shown || self.over.password.field.text.is_empty() {
            return;
        }
        let p = &mut self.over.password;
        let password = take_password(&mut p.field);
        p.answered = prompt;
        p.refused = false;
        p.shown = false;
        // Written straight from the password (no copy here), then Return.
        self.link.write(password.expose().as_bytes());
        self.link.write(b"\r");
        self.over.password.sent = Some(password);
        self.focus_terminal(tx);
        tx.instant();
    }
}

/// The strip above `pane`'s terminal, while ssh waits for the password there.
pub(crate) fn strip<S: 'static>(pane: &TermPane, t: &'static Theme, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> Option<El> {
    let p = &pane.over.password;
    if !p.shown {
        return None;
    }
    let send = move |s: &mut S, tx: &mut Tx| {
        if let Some(pane) = get(s) {
            pane.send_password(tx);
        }
    };
    let secret = text_input(&p.field, theme::TEXT)
        .secure()
        .single_line()
        .key("pw-strip-field")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .color(t.l1)
        .placeholder("Password", t.l2)
        .caret(t.ring, t.ring.alpha(0.35))
        .label_static("Password")
        .on_edit(rune::edit_access(move |s: &mut S| match get(s) {
            Some(pane) => &mut pane.over.password.field,
            None => super::find::nowhere(),
        }))
        .on_submit(send)
        // Esc: back to the terminal, to type it there.
        .on_input_key(move |s: &mut S, ev, tx| {
            let back = ev.key == Key::Escape;
            if let Some(pane) = get(s).filter(|_| back) {
                pane.focus_terminal(tx);
            }
            back
        });
    let words = if p.refused { REFUSED } else { OFFER };
    Some(
        row()
            .key("pw-strip")
            .w_full()
            .h(44.0)
            .px(12.0)
            .gap(10.0)
            .items_center()
            .fill(t.group)
            .border_sides([0.0, 0.0, 1.0, 0.0], t.hair)
            .cursor(rune::Cursor::Default)
            .role("group")
            .label_static("Save the server's password")
            .child(icon(Icon::Key, 14.0, if p.refused { t.red } else { t.l2 }))
            .child(text(words).key("pw-strip-words").font(theme::SMALL).color(if p.refused { t.red } else { t.l2 }).single_line().truncate().grow(1.0).shrink().min_w(0.0))
            .child(field(t, secret, p.refused).w(220.0).shrink().min_w(120.0))
            .child(push_button(t, "Save", Push::Primary, true).key("pw-strip-save").on_press_tx(send)),
    )
}
