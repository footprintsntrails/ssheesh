//! A server you signed in to stays signed in for Files, the edit bridge, the status bar and
//! new terminals, on every system.
//!
//! On macOS and Linux they ride the terminal's connection (ssh's shared connections, see
//! `SshEnv::control_dir`). Windows' OpenSSH can't share one: each of them signs in again, and
//! without a password to give ssh (`BatchMode`) a server that wants one refused them, so Files
//! said "Sign in first" over a terminal that was signed in. So the password that got a
//! terminal in is kept for the rest of the run, in memory only ([`Known`]), and handed to
//! ssh through askpass as a saved one is (`secrets::askpass_main`, which answers only that
//! server's password prompt).
//!
//! Which password got it in: a saved one the session started with, or what you typed at ssh's
//! own password prompt while the terminal was connecting ([`Watch`]). Only that: the keys
//! typed while ssh shows "user@host's password:" (never a key's passphrase, a code, a host
//! key's yes, or anything once the shell is up), and only once the server let the terminal
//! in. A session that fails to sign in forgets it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sshee_core::secrets::{is_password_prompt, Secret};

/// The longest prompt line kept while connecting; ssh's are far shorter.
const LINE_MAX: usize = 512;

/// One terminal's sign-in, from its session's start to the shell being up.
#[derive(Default)]
pub struct Watch {
    /// The session is connecting: its output is ssh's, and so is a prompt.
    connecting: bool,
    /// What ssh printed on the line it is on now, as it came (escapes and all).
    line: String,
    /// Keys typed at the password prompt on screen, not yet sent with Enter.
    typing: String,
    /// The last answer sent to a password prompt.
    answered: Option<Secret>,
}

impl Watch {
    /// A (re)connect starts: anything from the attempt before is forgotten.
    pub fn connecting(&mut self) {
        *self = Watch { connecting: true, ..Watch::default() };
    }

    /// What the session printed.
    pub fn output(&mut self, text: &str) {
        if !self.connecting {
            return;
        }
        // Only the line the cursor is on can be the prompt being answered.
        let rest = match text.rfind(['\n', '\r']) {
            Some(at) => {
                self.line.clear();
                self.typing.clear();
                &text[at + 1..]
            }
            None => text,
        };
        if self.line.len() + rest.len() <= LINE_MAX {
            self.line.push_str(rest);
        }
    }

    /// Whether ssh is asking for a password on the line it is on.
    fn at_password_prompt(&self) -> bool {
        self.connecting && is_password_prompt(&sshee_core::strip_ansi(&self.line))
    }

    /// Keys the terminal sends to the session.
    pub fn typed(&mut self, bytes: &[u8]) {
        if !self.at_password_prompt() {
            return;
        }
        // A key that sends an escape (an arrow, a function key) or a paste's brackets isn't
        // part of a password: such a chunk is left out whole.
        if bytes.contains(&0x1b) {
            return;
        }
        let Ok(text) = std::str::from_utf8(bytes) else { return };
        for c in text.chars() {
            match c {
                '\r' | '\n' => {
                    if !self.typing.is_empty() {
                        self.answered = Some(Secret::new(std::mem::take(&mut self.typing)));
                    }
                }
                // Backspace (DEL or ^H), Ctrl+U, and Ctrl+C, which gives up on the prompt.
                '\x7f' | '\x08' => {
                    self.typing.pop();
                }
                '\x15' | '\x03' => self.typing.clear(),
                c if c.is_control() => {}
                c => self.typing.push(c),
            }
        }
    }

    /// The server let the terminal in: the password typed for it, if one was.
    pub fn connected(&mut self) -> Option<Secret> {
        let answered = self.answered.take();
        *self = Watch::default();
        answered
    }
}

/// The passwords that got a terminal in this run, by the server's key in the sidebar. In
/// memory only: never written, gone when the program quits.
#[derive(Clone, Default)]
pub struct Known(Arc<Mutex<HashMap<String, Secret>>>);

impl Known {
    pub fn get(&self, server: &str) -> Option<Secret> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(server).cloned()
    }

    pub fn keep(&self, server: &str, password: Secret) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(server.to_owned(), password);
    }

    pub fn forget(&self, server: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(server);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_in(steps: &[(&str, &[u8])]) -> Option<String> {
        let mut w = Watch::default();
        w.connecting();
        for (out, keys) in steps {
            w.output(out);
            w.typed(keys);
        }
        w.connected().map(|s| s.expose().to_owned())
    }

    #[test]
    fn the_password_typed_at_ssh_prompt_is_what_got_in() {
        assert_eq!(signed_in(&[("deploy@203.0.113.17's password: ", b"made-up\r")]).as_deref(), Some("made-up"));
        // Typed a key at a time, with a slip mended by Backspace.
        let keys: Vec<(&str, &[u8])> = vec![("(deploy@web-7731) Password: ", b"m"), ("", b"x"), ("", b"\x7f"), ("", b"e"), ("", b"\r")];
        assert_eq!(signed_in(&keys).as_deref(), Some("me"));
        // Windows' console draws the prompt with escapes around it.
        assert_eq!(signed_in(&[("\x1b[?25l\x1b[2J\x1b[m\x1b[Hdeploy@203.0.113.17's password: \x1b[?25h", b"made-up\r")]).as_deref(), Some("made-up"));
    }

    #[test]
    fn a_wrong_try_then_the_right_one_keeps_the_right_one() {
        let steps: Vec<(&str, &[u8])> = vec![
            ("deploy@203.0.113.17's password: ", b"wrong\r"),
            ("\r\nPermission denied, please try again.\r\ndeploy@203.0.113.17's password: ", b"right\r"),
            ("\r\n", b""),
        ];
        assert_eq!(signed_in(&steps).as_deref(), Some("right"));
    }

    #[test]
    fn nothing_but_a_password_prompt_is_kept() {
        // A key's passphrase, a code, the host key's yes: none is a password for the server.
        assert_eq!(signed_in(&[("Enter passphrase for key '/home/me/.ssh/id_ed25519': ", b"phrase\r")]), None);
        assert_eq!(signed_in(&[("(deploy@web-7731) Verification code: ", b"123456\r")]), None);
        assert_eq!(signed_in(&[("Are you sure you want to continue connecting (yes/no/[fingerprint])? ", b"yes\r")]), None);
        // Given up with Ctrl+C, or never sent.
        assert_eq!(signed_in(&[("deploy@203.0.113.17's password: ", b"half\x03")]), None);
        assert_eq!(signed_in(&[("deploy@203.0.113.17's password: ", b"unsent")]), None);
        // An arrow key is no part of it.
        assert_eq!(signed_in(&[("deploy@203.0.113.17's password: ", b"ab"), ("", b"\x1b[D"), ("", b"c\r")]).as_deref(), Some("abc"));
    }

    #[test]
    fn once_the_shell_is_up_nothing_typed_is_kept() {
        let mut w = Watch::default();
        w.connecting();
        w.output("deploy@203.0.113.17's password: ");
        w.typed(b"made-up\r");
        assert!(w.connected().is_some());
        // sudo in the shell asks the same way: the session isn't connecting anymore.
        w.output("[sudo] password for deploy: ");
        w.typed(b"other\r");
        assert!(w.connected().is_none());
        // A reconnect starts afresh: what was typed before it is gone.
        w.output("deploy@203.0.113.17's password: ");
        w.connecting();
        w.typed(b"stale\r");
        assert!(w.connected().is_none());
    }

    #[test]
    fn known_passwords_are_by_server_and_can_be_forgotten() {
        let known = Known::default();
        known.keep("host:saved:1", Secret::new("made-up"));
        assert_eq!(known.clone().get("host:saved:1").map(|s| s.expose().to_owned()).as_deref(), Some("made-up"));
        assert!(known.get("host:saved:2").is_none());
        known.forget("host:saved:1");
        assert!(known.get("host:saved:1").is_none());
    }
}
