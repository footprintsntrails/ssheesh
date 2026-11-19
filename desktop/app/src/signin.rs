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
//! in. A session that fails to sign in forgets it. And only at a prompt askpass would itself
//! answer for this server's own account (`secrets::answers`): a jump host's "jump@bastion's
//! password:", or a fake prompt a server prints before it signs you in, is never kept.
//!
//! The same watch tells the window when it may offer to save the password (the strip above
//! the terminal, [`Watch::password_prompt`]), more strictly still: never through a jump host
//! (ProxyJump, ProxyCommand), only at a prompt worded exactly as ssh words this server's own
//! ("user@host's password:", "(user@host) Password:", an older ssh's bare "Password:"), and
//! never once anything but ssh's own lines was printed this attempt. A server that signed in
//! by key runs your login scripts before the shell is up, and one of them could print a
//! prompt that looks like ssh's; what they print first gives them away. (One that prints
//! nothing but a perfect copy of ssh's prompt can't be told from ssh by its text; a server
//! that shows a banner before you sign in isn't offered the strip, for the same reason.)

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use sshee_core::secrets::{answers, PasswordFor, Secret};

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
    /// Whose password a prompt must ask for to be watched: the server's own account. None
    /// watches nothing.
    to: Option<PasswordFor>,
    /// Output that isn't ssh's own (a login script's, a banner's) was printed this attempt:
    /// nothing is offered to the window until the next.
    other: bool,
    /// The prompts for this server's password this attempt, each on a line of its own: one
    /// after an answer means the server refused it.
    prompts: u32,
    /// The line the cursor is on is the one counted last ([`Watch::prompts`]): drawn again
    /// on the same line (a console's repaint) it isn't a new one.
    counted: bool,
}

impl Watch {
    /// A (re)connect to the account `to` starts: anything from the attempt before is
    /// forgotten. Without `to` nothing typed is kept.
    pub fn connecting(&mut self, to: Option<PasswordFor>) {
        *self = Watch { connecting: true, to, ..Watch::default() };
    }

    /// What the session printed.
    pub fn output(&mut self, text: &str) {
        if !self.connecting {
            return;
        }
        // Only the line the cursor is on can be the prompt being answered. Every line that
        // ends is looked at: anything but ssh's own lines means the sign-in is past.
        let mut rest = text;
        while let Some(at) = rest.find(['\n', '\r']) {
            self.push(&rest[..at]);
            if !self.is_ssh_line() {
                self.other = true;
            }
            if rest.as_bytes()[at] == b'\n' {
                self.counted = false;
            }
            self.line.clear();
            self.typing.clear();
            rest = &rest[at + 1..];
        }
        self.push(rest);
        if !self.counted && self.asks_strictly() {
            self.prompts += 1;
            self.counted = true;
        }
    }

    /// More of the line the cursor is on. A line far longer than ssh's is no prompt of its.
    fn push(&mut self, text: &str) {
        if self.line.len() + text.len() <= LINE_MAX {
            self.line.push_str(text);
        } else if !text.trim().is_empty() {
            self.other = true;
        }
    }

    /// The line the cursor is on is ssh's prompt for this server's own password, worded
    /// exactly as ssh words it, with no hop on the way that could word one the same.
    fn asks_strictly(&self) -> bool {
        let Some(to) = self.to.as_ref().filter(|to| self.connecting && !to.hops) else { return false };
        let p = sshee_core::strip_ansi(&self.line).trim().to_lowercase();
        let who = to.who.to_lowercase();
        p == format!("{who}'s password:") || p == format!("({who}) password:") || (p == "password:" && !to.labels_questions)
    }

    /// The line the cursor is on, as it ends, is one ssh itself prints while it signs in:
    /// nothing, this server's password prompt (answered, or asked again), "Permission
    /// denied", or one of its warnings (a host added to known_hosts, "** WARNING: connection
    /// is not using a post-quantum key exchange", ...).
    fn is_ssh_line(&self) -> bool {
        let t = sshee_core::strip_ansi(&self.line).trim().to_lowercase();
        t.is_empty() || self.asks_strictly() || t.starts_with("permission denied") || t.starts_with("warning:") || t.starts_with("** ")
    }

    /// For the window's offer to save the password: ssh asks, on the line it is on now, for
    /// this server's own password, and nothing but ssh's own lines came before it this
    /// attempt. Which prompt it is, counting from 1: a higher one after an answer was sent
    /// means the server refused that answer. `None` otherwise.
    pub fn password_prompt(&self) -> Option<u32> {
        (!self.other && self.asks_strictly()).then_some(self.prompts)
    }

    /// Whether ssh is asking for this server's own password on the line it is on: a prompt
    /// askpass would answer for it, never a hop's or one that names another account.
    fn at_password_prompt(&self) -> bool {
        let Some(to) = self.to.as_ref().filter(|_| self.connecting) else { return false };
        answers(&sshee_core::strip_ansi(&self.line), to)
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
///
/// And the servers that refused a password one of the app's own checks sent (its tmux, a
/// folder, its agents): those checks stop sending one there until a terminal signs in again.
/// Without a shared connection (Windows, Tor) each check signs in on its own, and a stale
/// saved password would be a failed login every few seconds: enough for fail2ban.
#[derive(Clone, Default)]
pub struct Known {
    passwords: Arc<Mutex<HashMap<String, Secret>>>,
    refused: Arc<Mutex<HashSet<String>>>,
}

impl Known {
    pub fn get(&self, server: &str) -> Option<Secret> {
        self.passwords.lock().unwrap_or_else(|e| e.into_inner()).get(server).cloned()
    }

    pub fn keep(&self, server: &str, password: Secret) {
        self.passwords.lock().unwrap_or_else(|e| e.into_inner()).insert(server.to_owned(), password);
    }

    pub fn forget(&self, server: &str) {
        self.passwords.lock().unwrap_or_else(|e| e.into_inner()).remove(server);
    }

    /// A check's password was refused by `server`: the checks there go without one.
    pub fn refuse_for_checks(&self, server: &str) {
        self.refused.lock().unwrap_or_else(|e| e.into_inner()).insert(server.to_owned());
    }

    /// Whether the checks on `server` go without a password.
    pub fn refused_for_checks(&self, server: &str) -> bool {
        self.refused.lock().unwrap_or_else(|e| e.into_inner()).contains(server)
    }

    /// A terminal signed in to `server`: its checks may send a password again.
    pub fn signed_in_again(&self, server: &str) {
        self.refused.lock().unwrap_or_else(|e| e.into_inner()).remove(server);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Option<PasswordFor> {
        Some(PasswordFor { who: "deploy@203.0.113.17".into(), hops: false, labels_questions: true })
    }

    fn signed_in(steps: &[(&str, &[u8])]) -> Option<String> {
        signed_in_to(target(), steps)
    }

    fn signed_in_to(to: Option<PasswordFor>, steps: &[(&str, &[u8])]) -> Option<String> {
        let mut w = Watch::default();
        w.connecting(to);
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
        let keys: Vec<(&str, &[u8])> = vec![("(deploy@203.0.113.17) Password: ", b"m"), ("", b"x"), ("", b"\x7f"), ("", b"e"), ("", b"\r")];
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
        w.connecting(target());
        w.output("deploy@203.0.113.17's password: ");
        w.typed(b"made-up\r");
        assert!(w.connected().is_some());
        // sudo in the shell asks the same way: the session isn't connecting anymore.
        w.output("[sudo] password for deploy: ");
        w.typed(b"other\r");
        assert!(w.connected().is_none());
        // A reconnect starts afresh: what was typed before it is gone.
        w.output("deploy@203.0.113.17's password: ");
        w.connecting(target());
        w.typed(b"stale\r");
        assert!(w.connected().is_none());
    }

    #[test]
    fn a_jump_hosts_password_is_never_kept_as_the_servers() {
        // Through ProxyJump with a current ssh: the bastion asks first, labelled with its own
        // account. Only the target's own prompt is kept.
        let behind = Some(PasswordFor { who: "deploy@203.0.113.17".into(), hops: true, labels_questions: true });
        assert_eq!(signed_in_to(behind.clone(), &[("(jump@198.51.100.4) Password: ", b"bastion-pw\r")]), None);
        assert_eq!(signed_in_to(behind.clone(), &[("jump@198.51.100.4's password: ", b"bastion-pw\r")]), None);
        let both: Vec<(&str, &[u8])> = vec![("(jump@198.51.100.4) Password: ", b"bastion-pw\r"), ("\r\n(deploy@203.0.113.17) Password: ", b"target-pw\r")];
        assert_eq!(signed_in_to(behind.clone(), &both).as_deref(), Some("target-pw"));
        assert_eq!(signed_in_to(behind, &[("deploy@203.0.113.17's password: ", b"target-pw\r")]).as_deref(), Some("target-pw"));
        // Direct, the same: a prompt naming another account isn't this server's password.
        assert_eq!(signed_in(&[("jump@198.51.100.4's password: ", b"other\r")]), None);
        assert_eq!(signed_in(&[("(jump@198.51.100.4) Password: ", b"other\r")]), None);
        // An older ssh through a hop can't tell who asks: nothing is kept.
        let old = Some(PasswordFor { who: "deploy@203.0.113.17".into(), hops: true, labels_questions: false });
        assert_eq!(signed_in_to(old, &[("deploy@203.0.113.17's password: ", b"either\r")]), None);
        // No account to check against: nothing is kept.
        assert_eq!(signed_in_to(None, &[("deploy@203.0.113.17's password: ", b"made-up\r")]), None);
    }

    fn offered(to: Option<PasswordFor>, output: &[&str]) -> Option<u32> {
        let mut w = Watch::default();
        w.connecting(to);
        for out in output {
            w.output(out);
        }
        w.password_prompt()
    }

    #[test]
    fn the_strip_is_offered_at_ssh_own_prompt_for_this_server() {
        assert_eq!(offered(target(), &["deploy@203.0.113.17's password: "]), Some(1));
        assert_eq!(offered(target(), &["(deploy@203.0.113.17) Password: "]), Some(1));
        // ssh's own notes before it, a byte at a time, or drawn with a console's escapes.
        assert_eq!(offered(target(), &["Warning: Permanently added '203.0.113.17' (ED25519) to the list of known hosts.\r\n", "deploy@203.0.113.17's ", "password: "]), Some(1));
        assert_eq!(offered(target(), &["\x1b[?25l\x1b[2J\x1b[m\x1b[Hdeploy@203.0.113.17's password: \x1b[?25h"]), Some(1));
        // An older ssh's bare prompt names nobody: only without labels to look for.
        let old = Some(PasswordFor { who: "deploy@203.0.113.17".into(), hops: false, labels_questions: false });
        assert_eq!(offered(old, &["Password: "]), Some(1));
        assert_eq!(offered(target(), &["Password: "]), None);
        // Anything worded otherwise, or for another account, or not connecting.
        assert_eq!(offered(target(), &["Enter your password: "]), None);
        assert_eq!(offered(target(), &["root@203.0.113.17's password: "]), None);
        assert_eq!(offered(target(), &["deploy@203.0.113.17's password: please"]), None);
        assert_eq!(offered(None, &["deploy@203.0.113.17's password: "]), None);
        let mut w = Watch::default();
        w.output("deploy@203.0.113.17's password: ");
        assert_eq!(w.password_prompt(), None);
    }

    #[test]
    fn a_refused_answer_is_a_new_prompt_on_a_new_line() {
        let mut w = Watch::default();
        w.connecting(target());
        w.output("deploy@203.0.113.17's password: ");
        assert_eq!(w.password_prompt(), Some(1));
        // A console repainting the same line: the same prompt.
        w.output("\rdeploy@203.0.113.17's password: ");
        w.output("\x1b[?25h");
        assert_eq!(w.password_prompt(), Some(1));
        w.typed(b"wrong\r");
        w.output("\r\nPermission denied, please try again.\r\n");
        assert_eq!(w.password_prompt(), None);
        w.output("deploy@203.0.113.17's password: ");
        assert_eq!(w.password_prompt(), Some(2));
        // A reconnect counts afresh.
        w.connecting(target());
        w.output("deploy@203.0.113.17's password: ");
        assert_eq!(w.password_prompt(), Some(1));
    }

    #[test]
    fn a_fake_prompt_after_a_key_login_is_never_offered() {
        // Signed in by key: the login scripts run before the shell is up, and print.
        assert_eq!(offered(target(), &["Welcome to web-7731\r\n", "deploy@203.0.113.17's password: "]), None);
        assert_eq!(offered(target(), &["\x1b[1mLast login: Tue Nov 17 09:12:03 2026\x1b[0m\r\ndeploy@203.0.113.17's password: "]), None);
        // After an answer that got in, the same: what follows is the server's.
        assert_eq!(offered(target(), &["deploy@203.0.113.17's password: ", "\r\n", "motd\r\n", "deploy@203.0.113.17's password: "]), None);
        // A line far longer than ssh's.
        let long = format!("{}\r\ndeploy@203.0.113.17's password: ", "x".repeat(LINE_MAX + 1));
        assert_eq!(offered(target(), &[&long]), None);
    }

    #[test]
    fn nothing_is_offered_through_a_jump_host() {
        // The bastion's prompt, and even one that names the server: through a hop, never.
        let behind = Some(PasswordFor { who: "deploy@203.0.113.17".into(), hops: true, labels_questions: true });
        assert_eq!(offered(behind.clone(), &["(jump@198.51.100.4) Password: "]), None);
        assert_eq!(offered(behind.clone(), &["jump@198.51.100.4's password: "]), None);
        assert_eq!(offered(behind, &["(jump@198.51.100.4) Password: ", "\r\n(deploy@203.0.113.17) Password: "]), None);
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
