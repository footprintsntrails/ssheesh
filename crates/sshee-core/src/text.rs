//! Small text helpers for terminal byte streams.

/// Decodes a byte stream as UTF-8 across chunk boundaries: a multi-byte
/// character split between two reads is held back until it's complete.
#[derive(Default)]
pub struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);
        let mut out = String::with_capacity(buf.len());
        let mut rest: &[u8] = &buf;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&rest[..valid]).unwrap_or_default());
                    match e.error_len() {
                        Some(len) => {
                            out.push('\u{FFFD}');
                            rest = &rest[valid + len..];
                        }
                        None => {
                            self.pending = rest[valid..].to_vec();
                            break;
                        }
                    }
                }
            }
        }
        out
    }
}

/// Our own invisible OSC sequence the remote side prints right before the
/// shell starts. Terminals ignore unknown OSC codes; we strip it anyway.
const MARKER_PREFIX: &str = "\x1b]7777;sshee=";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Durable,
    Plain,
}

#[derive(Default)]
pub struct MarkerScanner {
    carry: String,
}

impl MarkerScanner {
    /// Returns the chunk with markers removed, plus the markers found.
    pub fn scan(&mut self, chunk: &str) -> (String, Vec<Marker>) {
        let mut s = std::mem::take(&mut self.carry);
        s.push_str(chunk);
        let mut out = String::with_capacity(s.len());
        let mut found = Vec::new();
        let mut rest = s.as_str();
        loop {
            if let Some(i) = rest.find(MARKER_PREFIX) {
                out.push_str(&rest[..i]);
                let after = &rest[i + MARKER_PREFIX.len()..];
                if let Some(end) = after.find('\x07') {
                    match &after[..end] {
                        "durable" => found.push(Marker::Durable),
                        "plain" => found.push(Marker::Plain),
                        _ => {}
                    }
                    rest = &after[end + 1..];
                } else if after.len() < 16 {
                    self.carry = rest[i..].to_string();
                    return (out, found);
                } else {
                    out.push_str(&rest[i..i + MARKER_PREFIX.len()]);
                    rest = after;
                }
            } else {
                let keep = partial_prefix_len(rest, MARKER_PREFIX);
                out.push_str(&rest[..rest.len() - keep]);
                self.carry = rest[rest.len() - keep..].to_string();
                return (out, found);
            }
        }
    }
}

fn partial_prefix_len(s: &str, prefix: &str) -> usize {
    let max = prefix.len().saturating_sub(1).min(s.len());
    (1..=max).rev().find(|&k| s.ends_with(&prefix[..k])).unwrap_or(0)
}

/// Removes ANSI escape sequences so error lines can be shown in the UI.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// ssh's own notes about the shared (multiplexed) connection: "Shared
/// connection to x closed.", "mux_client_…", "ControlSocket … disabling
/// multiplexing". They're about ssh's plumbing, not the server.
pub fn is_ssh_mux_noise(line: &str) -> bool {
    let t = line.trim_matches(|c: char| c.is_whitespace() || c.is_control());
    (t.starts_with("Shared connection to ") && t.ends_with("closed."))
        || t.starts_with("mux_client_")
        || (t.starts_with("ControlSocket ") && t.contains("multiplexing"))
}

/// Terminal output without ssh's shared-connection notes.
pub fn strip_ssh_mux_noise(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("Shared connection") && !text.contains("mux_client") && !text.contains("ControlSocket") {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(text.split_inclusive('\n').filter(|l| !is_ssh_mux_noise(l)).collect())
}

pub fn last_meaningful_line(s: &str) -> String {
    strip_ansi(s)
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c.is_control()))
        .rfind(|l| !l.is_empty() && !is_ssh_mux_noise(l))
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_shared_connection_notes() {
        let out = "bye\r\nShared connection to 203.0.113.10 closed.\r\n";
        assert_eq!(strip_ssh_mux_noise(out), "bye\r\n");
        let busy = "mux_client_request_session: session request failed: Session open refused by peer\r\nControlSocket /x/cm/ab already exists, disabling multiplexing\r\nWelcome\r\n";
        assert_eq!(strip_ssh_mux_noise(busy), "Welcome\r\n");
        assert_eq!(strip_ssh_mux_noise("plain output"), "plain output");
        assert_eq!(last_meaningful_line("Permission denied\nShared connection to x closed.\n"), "Permission denied");
    }

    #[test]
    fn utf8_split_across_chunks() {
        let mut d = Utf8Stream::default();
        let bytes = "привет".as_bytes();
        let a = d.push(&bytes[..3]);
        let b = d.push(&bytes[3..]);
        assert_eq!(format!("{a}{b}"), "привет");
    }

    #[test]
    fn utf8_invalid_bytes_are_replaced() {
        let mut d = Utf8Stream::default();
        assert_eq!(d.push(b"a\xffb"), "a\u{FFFD}b");
    }

    #[test]
    fn marker_is_found_and_stripped() {
        let mut m = MarkerScanner::default();
        let (out, found) = m.scan("hello\x1b]7777;sshee=durable\x07world");
        assert_eq!(out, "helloworld");
        assert_eq!(found, vec![Marker::Durable]);
    }

    #[test]
    fn marker_split_across_chunks() {
        let mut m = MarkerScanner::default();
        let (a, f1) = m.scan("abc\x1b]77");
        let (b, f2) = m.scan("77;sshee=plain\x07def");
        assert_eq!(format!("{a}{b}"), "abcdef");
        assert!(f1.is_empty());
        assert_eq!(f2, vec![Marker::Plain]);
    }

    #[test]
    fn other_escapes_pass_through() {
        let mut m = MarkerScanner::default();
        let (a, _) = m.scan("x\x1b[31mred\x1b");
        let (b, _) = m.scan("[0m");
        assert_eq!(format!("{a}{b}"), "x\x1b[31mred\x1b[0m");
    }

    #[test]
    fn last_line_skips_escapes_and_blanks() {
        let s = "foo\r\n\x1b[1mssh: connect to host x port 22: Connection refused\x1b[0m\r\n\r\n";
        assert_eq!(last_meaningful_line(s), "ssh: connect to host x port 22: Connection refused");
    }
}
