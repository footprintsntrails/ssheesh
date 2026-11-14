//! Paths and links in a terminal's output that open with a click: web links,
//! file paths (`/etc/hosts`, `~/notes.md`, `src/main.rs:12`), and the links a
//! program marks itself with OSC 8.
//!
//! A port of `findLinks` and `resolvePath` from 0.10's `src/lib/paths.ts`,
//! and of the click rule in `src/lib/terminals.ts` (its link provider). What
//! a click does with a link (the browser, the Files pane) stays with the app,
//! as `terminalLinks.ts` did.
//!
//! Positions are byte offsets into the text given.

use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

/// What a link opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkKind {
    /// A web page, in the browser.
    Url,
    /// A file or folder on the terminal's machine, in the Files pane.
    Path,
}

/// A link found in a line of text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextLink {
    /// Where it sits in the text.
    pub range: Range<usize>,
    pub kind: LinkKind,
    /// The URL, or the path without a trailing ":line:column" or line range.
    pub target: String,
    /// "src/main.rs:12" → 12; the first of a range.
    pub line: Option<u32>,
    /// "notes.md:242-296" (or "#L242-L296") → 296.
    pub end_line: Option<u32>,
}

/// Characters that end a link: spaces, quotes, brackets and separators.
fn stops(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | ';' | ',')
}

/// Punctuation that ends a sentence rather than the path.
fn without_trailing(s: &str) -> &str {
    s.trim_end_matches(['.', ':', '!', '?'])
}

/// Finds web links and file paths in a line of terminal output. A path is
/// one that starts at a folder (`/etc/hosts`, `~/notes.md`, `./run.sh`,
/// `../x`) or a relative one with a folder and a file name in it
/// (`src/main.rs:12`). Bare words with a slash ("and/or", "TCP/IP") don't
/// count, nor do lone file names: too many of those aren't files.
pub fn find_links(text: &str) -> Vec<TextLink> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if stops(c) {
            chars.next();
            continue;
        }
        let mut j = text.len();
        while let Some(&(k, c)) = chars.peek() {
            if stops(c) {
                j = k;
                break;
            }
            chars.next();
        }
        if let Some(mut link) = classify(&text[i..j]) {
            link.range = i + link.range.start..i + link.range.end;
            out.push(link);
        }
    }
    out
}

/// ":12", ":12:7" (a column), ":12-40", and GitHub's "#L12" and "#L12-L40".
static POSITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.+?)(?::([0-9]+)(?::[0-9]+|[-–]([0-9]+))?|#L([0-9]+)(?:-L?([0-9]+))?)$").expect("pattern"));

fn number(s: &str) -> u32 {
    s.parse().unwrap_or(u32::MAX)
}

fn classify(word: &str) -> Option<TextLink> {
    let lower = word.get(..8).unwrap_or(word).to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let url = without_trailing(word);
        // JavaScript counted UTF-16 units; a link this short is ASCII anyway.
        return (url.chars().count() > 8).then(|| TextLink { range: 0..url.len(), kind: LinkKind::Url, target: url.to_string(), line: None, end_line: None });
    }
    // "--config=/etc/x.conf", "path=/srv": the path is after the last "=".
    let mut start = word.rfind('=').map_or(0, |eq| eq + 1);
    if word[start..].starts_with("file://") {
        start += "file://".len();
    }
    let trimmed = without_trailing(&word[start..]);
    let mut token = trimmed;
    let mut line = None;
    let mut end_line = None;
    let pos = POSITION.captures(trimmed);
    if let Some(caps) = &pos {
        let first = number(caps.get(2).or(caps.get(4)).map_or("", |m| m.as_str()));
        line = Some(first);
        // A range that runs backwards is just its first line.
        end_line = caps.get(3).or(caps.get(5)).map(|m| number(m.as_str())).filter(|&last| last > first);
        token = caps.get(1).map_or("", |m| m.as_str());
    }
    if !is_path(token) {
        return None;
    }
    let end = start + if pos.is_some() { trimmed.len() } else { token.len() };
    Some(TextLink { range: start..end, kind: LinkKind::Path, target: token.to_string(), line, end_line })
}

fn is_path(s: &str) -> bool {
    static RELATIVE_DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9A-Za-z_.-]+(?:/[0-9A-Za-z_.-]+)*/$").expect("pattern"));
    static RELATIVE_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9A-Za-z_@.+-]+(?:/[0-9A-Za-z_@.+-]+)*$").expect("pattern"));
    static EXTENSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[A-Za-z0-9]{1,8}$").expect("pattern"));
    static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").expect("pattern"));
    if !s.chars().any(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    let rest = s.strip_prefix('~').or_else(|| s.strip_prefix("..")).or_else(|| s.strip_prefix('.')).unwrap_or(s);
    if rest.starts_with('/') || s.starts_with('/') {
        return !s.starts_with("//");
    }
    // Relative: a folder and then a file with an extension, or a folder ending in "/".
    if !s.contains('/') || s.starts_with('@') || SCHEME.is_match(s) {
        return false;
    }
    let last = &s[s.rfind('/').map_or(0, |i| i + 1)..];
    if s.ends_with('/') {
        RELATIVE_DIR.is_match(s)
    } else {
        RELATIVE_FILE.is_match(s) && EXTENSION.is_match(last)
    }
}

/// A path from the terminal, made absolute against the terminal's folder.
pub fn resolve_path(path: &str, cwd: &str) -> String {
    if path.starts_with('/') || path == "~" || path.starts_with("~/") {
        return normalize(path);
    }
    let base = cwd.trim_end_matches('/');
    let base = if base.is_empty() { "/" } else { base };
    normalize(&format!("{base}/{path}"))
}

/// Folds "." and ".." and repeated slashes away.
fn normalize(path: &str) -> String {
    let home = path == "~" || path.starts_with("~/");
    let mut parts: Vec<&str> = Vec::new();
    for part in (if home { &path[1..] } else { path }).split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let trail = parts.join("/");
    match (home, trail.is_empty()) {
        (true, true) => "~".to_string(),
        (true, false) => format!("~/{trail}"),
        (false, _) => format!("/{trail}"),
    }
}

/// What an OSC 8 link a program printed opens, if it's one we open: web
/// pages, and `file://` paths (on the terminal's machine, so in Files).
/// Anything else (`ssh:`, `mailto:`, `javascript:`) stays plain text.
pub fn hyperlink(uri: &str) -> Option<(LinkKind, String)> {
    let lower = uri.get(..8).unwrap_or(uri).to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some((LinkKind::Url, uri.to_string()));
    }
    let rest = uri.strip_prefix("file://")?;
    // file://host/path: the host is the machine the program runs on.
    let path = &rest[rest.find('/')?..];
    Some((LinkKind::Path, percent_decoded(path)))
}

/// `%20` and the like turned back into the characters they stand for.
fn percent_decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |k: usize| b.get(k).and_then(|c| (*c as char).to_digit(16));
        match (b[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What hovering a link says.
pub const OPEN_HINT: &str = "Click to open";

/// What hovering a link says whose text isn't where it goes (a program's
/// OSC 8 link over other words): the place itself, so nobody opens it blind.
pub fn hidden_target_hint(target: &str) -> String {
    format!("Goes to {target}. Click to open")
}

/// What to ask before opening a web link whose text isn't where it goes. In
/// 0.10 xterm asked this for every program's link (its default handler).
pub fn ask_to_open(target: &str) -> String {
    format!("Open {target}?\n\nThe program that printed this link showed other text in its place.")
}

/// A plain click opens a link this long after, unless it turns out to be a double-click.
pub const CLICK_WAIT_MS: u64 = 260;

/// What a click on a link does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkClick {
    /// Ctrl+click (⌘-click on a Mac): open now.
    OpenNow,
    /// A plain click: open after `CLICK_WAIT_MS`, if no selection was made meanwhile.
    OpenAfterWait,
    /// As `OpenNow`, but ask first (`ask_to_open`) and open only on yes.
    AskNow,
    /// As `OpenAfterWait`, but ask first (`ask_to_open`) and open only on yes.
    AskAfterWait,
    /// Part of a double-click, or a selection is being made: not a click on the link.
    Nothing,
}

impl LinkClick {
    /// The same click on a link that must be asked about before it opens.
    pub fn asking_first(self) -> LinkClick {
        match self {
            LinkClick::OpenNow => LinkClick::AskNow,
            LinkClick::OpenAfterWait => LinkClick::AskAfterWait,
            other => other,
        }
    }
}

/// The click rule: Ctrl+click (⌘-click) opens at once; a plain click waits a
/// beat, so a double-click that selects part of a path, or a drag across it,
/// doesn't open anything. `open_key` is Ctrl, or ⌘ on a Mac. A link on screen
/// knows whether it must be asked about first: use `present::Link::click`.
pub fn link_click(open_key: bool, clicks: u32, has_selection: bool) -> LinkClick {
    if open_key {
        LinkClick::OpenNow
    } else if clicks > 1 || has_selection {
        LinkClick::Nothing
    } else {
        LinkClick::OpenAfterWait
    }
}
