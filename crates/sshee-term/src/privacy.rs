//! Streamer mode for terminal text: server names, users and IP addresses
//! hidden behind dots of the same length, so columns stay where they were.
//!
//! A port of the text half of 0.10's `src/lib/privacy.ts` (`maskText` and
//! the word list behind it). In 0.10 this rewrote the output stream on its
//! way to xterm; in 1.0 the presentation pass (`present.rs`) runs it over
//! the rows on screen instead, and copy, find, links and notifications read
//! what it made. Labels outside the terminal (`hostAlias`, `useNames`) stay
//! with the app.

use crate::secrets::{char_before, char_len, dot_out, is_word, merge, Span};
use aho_corasick::{AhoCorasick, MatchKind};
use regex::{Regex, RegexBuilder};
use std::sync::LazyLock;

/// Names everyone has. Hiding them protects nothing and makes output hard to read.
const COMMON: [&str; 19] = [
    "root", "admin", "administrator", "user", "ubuntu", "debian", "fedora", "centos", "rocky", "alma", "pi", "ec2-user", "localhost", "docker",
    "git", "www-data", "nobody", "home", "server",
];

/// What a saved server is called, where it is and who you are on it.
#[derive(Debug, Clone, Copy, Default)]
pub struct HostNames<'a> {
    pub name: &'a str,
    pub hostname: &'a str,
    pub user: Option<&'a str>,
}

/// A name plus its short form: "web-1.example.com" also shows up as "web-1".
pub fn variants(name: &str) -> Vec<String> {
    let n = name.trim();
    if n.is_empty() {
        return Vec::new();
    }
    let short = if is_ip(n) { "" } else { n.split('.').next().unwrap_or("") };
    if !short.is_empty() && short != n {
        vec![n.to_string(), short.to_string()]
    } else {
        vec![n.to_string()]
    }
}

fn is_ip(s: &str) -> bool {
    static V4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{1,3}(\.[0-9]{1,3}){3}$").expect("pattern"));
    V4.is_match(s) || s.contains(':')
}

fn hideable(word: &str) -> bool {
    word.chars().count() >= 3 && !COMMON.contains(&word.to_lowercase().as_str())
}

/// "Server 2": the name streamer mode shows for the server at `index` in
/// the server list (none: "Server ?"). Stable while the list stays the same.
pub fn host_alias(index: Option<usize>) -> String {
    match index {
        Some(i) => format!("Server {}", i + 1),
        None => "Server ?".to_string(),
    }
}

/// Everything that must not appear on screen while streaming, longest first,
/// and what finds any of it.
///
/// 0.10 put every word into one JavaScript regex. Here names in ASCII (most
/// of them) go to an Aho-Corasick automaton, which finds the longest at each
/// place, in any ASCII case, as that regex did, and takes any number of names
/// in time and memory that grow with their length. Names with other letters
/// (a server called "сервер") go to case-blind regexes, as many as it takes
/// to keep each under the `regex` crate's size limit: a long server list
/// (thousands of hosts, each with its name, address, short name and user) is
/// more than one pattern may hold, and must not stop the app.
#[derive(Debug, Clone, Default)]
pub struct Names {
    words: Vec<String>,
    /// The names in ASCII, in any ASCII case.
    ascii: Option<AhoCorasick>,
    /// The others, in any case.
    others: Vec<Regex>,
    /// Names no regex would take even alone (none that a server or a user
    /// could be called): found as they are, in any ASCII case.
    exact: Option<AhoCorasick>,
}

/// How big one regex of names may get. Past this the list is split.
const NAMES_SIZE_LIMIT: usize = 32 << 20;

impl Names {
    /// From the saved servers (their names, addresses and users, each with
    /// its short form) and names learned at run time: hostnames from stats,
    /// this computer's user and name.
    pub fn new<'a>(hosts: impl IntoIterator<Item = HostNames<'a>>, learned: impl IntoIterator<Item = &'a str>) -> Self {
        let mut all: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut add = |w: String| {
            if hideable(&w) && seen.insert(w.clone()) {
                all.push(w);
            }
        };
        for h in hosts {
            for w in variants(h.name).into_iter().chain(variants(h.hostname)) {
                add(w);
            }
            add(h.user.unwrap_or("").to_string());
        }
        for name in learned {
            for w in variants(name) {
                add(w);
            }
        }
        // Longest first, so "web-1.example.com" goes as a whole before "web-1" can.
        all.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
        let (ascii_words, other_words): (Vec<&String>, Vec<&String>) = all.iter().partition(|w| w.is_ascii());
        let mut leftover: Vec<&String> = Vec::new();
        let ascii = automaton(&ascii_words);
        if ascii.is_none() {
            leftover.extend(&ascii_words);
        }
        leftover.extend(&other_words);
        let mut others = Vec::new();
        let mut alone = Vec::new();
        patterns(&leftover, &mut others, &mut alone);
        let exact = automaton(&alone);
        Names { words: all, ascii, others, exact }
    }

    /// The words hidden, longest first.
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// Server names, users and IP addresses replaced by dots, one `•` per
    /// character: the text keeps its length in characters, and every
    /// character is either as it was or a `•`.
    pub fn mask_text(&self, text: &str) -> String {
        let mut found: Vec<Span> = Vec::new();
        for ac in [&self.ascii, &self.exact].into_iter().flatten() {
            found.extend(ac.find_iter(text).map(|m| m.range()));
        }
        for re in &self.others {
            found.extend(re.find_iter(text).map(|m| m.range()));
        }
        let out = dot_out(text, &merge(found));
        mask_ipv6(&mask_ipv4(&out))
    }
}

/// The longest name at each place, in any ASCII case; `None` for no names,
/// or if the automaton can't be built.
fn automaton(words: &[&String]) -> Option<AhoCorasick> {
    if words.is_empty() {
        return None;
    }
    AhoCorasick::builder().ascii_case_insensitive(true).match_kind(MatchKind::LeftmostLongest).build(words).ok()
}

/// Case-blind regexes for `words` (longest first), as few as fit under
/// `NAMES_SIZE_LIMIT`: a list too big for one is split in two, and so on.
/// A word no regex takes even alone goes to `alone`.
fn patterns<'w>(words: &[&'w String], out: &mut Vec<Regex>, alone: &mut Vec<&'w String>) {
    if words.is_empty() {
        return;
    }
    let alternatives: Vec<String> = words.iter().map(|w| regex::escape(w)).collect();
    let limit = if words.len() == 1 { usize::MAX } else { NAMES_SIZE_LIMIT };
    match RegexBuilder::new(&alternatives.join("|")).case_insensitive(true).size_limit(limit).dfa_size_limit(NAMES_SIZE_LIMIT).build() {
        Ok(re) => out.push(re),
        Err(_) if words.len() == 1 => alone.push(words[0]),
        Err(_) => {
            let (a, b) = words.split_at(words.len() / 2);
            patterns(a, out, alone);
            patterns(b, out, alone);
        }
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// How many ASCII digits start at byte `at`.
fn digit_run(b: &[u8], at: usize) -> usize {
    b.get(at..).map_or(0, |rest| rest.iter().take_while(|c| c.is_ascii_digit()).count())
}

/// An IPv4 address starting at byte `at`, standing alone, and where it ends:
/// `(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?(?![\w.])` as JavaScript reads
/// it, the port only when `port`. A port that is followed by more of a word
/// is left off, as the regex backtracks to the address alone.
pub(crate) fn ipv4_at(text: &str, at: usize, port: bool) -> Option<usize> {
    let b = text.as_bytes();
    if at > 0 && (is_word_byte(b[at - 1]) || b[at - 1] == b'.') {
        return None;
    }
    let mut i = at;
    for group in 0..4 {
        // More than three digits: no way to split them that a `.` or the look-ahead accepts.
        let n = digit_run(b, i);
        if n == 0 || n > 3 {
            return None;
        }
        i += n;
        if group < 3 {
            if b.get(i) != Some(&b'.') {
                return None;
            }
            i += 1;
        }
    }
    let alone = |end: usize| !b.get(end).is_some_and(|&c| is_word_byte(c) || c == b'.');
    if port && b.get(i) == Some(&b':') {
        let n = digit_run(b, i + 1);
        if n > 0 && alone(i + 1 + n) {
            return Some(i + 1 + n);
        }
    }
    alone(i).then_some(i)
}

fn mask_ipv4(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut at, mut copied) = (0, 0);
    while at < b.len() {
        if b[at].is_ascii_digit() {
            if let Some(end) = ipv4_at(text, at, false) {
                out.push_str(&text[copied..at]);
                out.extend(text[at..end].chars().map(|c| if c.is_ascii_digit() { '•' } else { c }));
                copied = end;
                at = end;
                continue;
            }
        }
        at += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// Colon groups; checked in `looks_like_ipv6` so clock times like 12:34:56 stay readable.
static IPV6: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:[0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}").expect("pattern"));

fn looks_like_ipv6(m: &str) -> bool {
    static HEX2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9a-fA-F]{2,}").expect("pattern"));
    if m.len() < 6 {
        return false;
    }
    if m.contains("::") {
        return HEX2.is_match(m);
    }
    m.split(':').count() >= 5 && m.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'))
}

/// IPv6 addresses with their hex digits dotted. The look-behind `(?<![\w:[;])`
/// and look-ahead `(?![\w:])` of 0.10's pattern are checked here; when the
/// longest match fails the look-ahead, no shorter one at that place passes it
/// either (it would end inside a run of hex digits or before a `:`).
fn mask_ipv6(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut at, mut copied) = (0, 0);
    while let Some(m) = IPV6.find_at(text, at) {
        let before_ok = !char_before(text, m.start()).is_some_and(|c| is_word(c) || matches!(c, ':' | '[' | ';'));
        let after_ok = !text[m.end()..].chars().next().is_some_and(|c| is_word(c) || c == ':');
        if !(before_ok && after_ok) {
            at = m.start() + char_len(text, m.start());
            continue;
        }
        if looks_like_ipv6(m.as_str()) {
            out.push_str(&text[copied..m.start()]);
            out.extend(m.as_str().chars().map(|c| if c.is_ascii_hexdigit() { '•' } else { c }));
            copied = m.end();
        }
        at = m.end();
    }
    out.push_str(&text[copied..]);
    out
}
