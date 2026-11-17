//! "What's new" without a screen: the changelog, bundled into the program so it works offline
//! and shows exactly the notes of the versions you skipped (0.10's `src/lib/changelog.ts` and
//! `WhatsNew.tsx`).
//!
//! The notes are the repository's CHANGELOG.md: `## 1.2.3 — title`, then lines. Only lines
//! starting `- ` (a bullet), `### ` (a heading) and plain paragraphs are read; inside a line,
//! `**bold**`, `` `code` `` and `[links](…)` (the link's words only: nothing is opened from
//! here).
//!
//! When it shows by itself (once, after an update): [`due`].

/// The whole changelog, as the program was built with it.
pub const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// One version's notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub title: String,
    /// The lines under the heading.
    pub body: String,
}

/// Every release in a changelog, newest first as written; headings that aren't a version
/// (`## Unreleased`) are skipped.
pub fn parse(text: &str) -> Vec<Release> {
    let mut out = Vec::new();
    for section in text.split("\n## ").skip(1) {
        let (head, body) = section.split_once('\n').unwrap_or((section, ""));
        let (version, title) = match head.split_once(" — ") {
            Some((v, t)) => (v.trim(), t.trim()),
            None => (head.trim(), ""),
        };
        if parts(version).is_some() {
            out.push(Release { version: version.to_owned(), title: title.to_owned(), body: body.trim().to_owned() });
        }
    }
    out
}

/// The releases of the bundled changelog.
pub fn releases() -> Vec<Release> {
    parse(CHANGELOG)
}

/// "1.0.0" → [1, 0, 0]. A suffix (`1.0.0-canary`) is dropped: it is that version.
fn parts(version: &str) -> Option<[u64; 3]> {
    let base = version.trim().split(['-', '+']).next()?;
    let mut it = base.split('.');
    let p = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    it.next().is_none().then_some(p)
}

/// Compares "0.3.2" with "0.3.10" the way people expect. A version that doesn't read sorts
/// first.
pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    parts(a).unwrap_or_default().cmp(&parts(b).unwrap_or_default())
}

/// The releases newer than `since` up to and including `current`, newest first. `since`
/// empty: only `current`'s own (a fresh install has nothing it skipped).
pub fn since<'a>(all: &'a [Release], since: &str, current: &str) -> Vec<&'a Release> {
    let upto = |r: &&Release| compare(&r.version, current).is_le();
    if since.trim().is_empty() || parts(since).is_none() {
        return all.iter().filter(upto).take(1).collect();
    }
    all.iter().filter(upto).filter(|r| compare(&r.version, since).is_gt()).collect()
}

/// The notes of the version running (Settings → About, the palette), else the newest.
pub fn current<'a>(all: &'a [Release], current: &str) -> Vec<&'a Release> {
    let own: Vec<&Release> = all.iter().filter(|r| compare(&r.version, current).is_eq()).collect();
    if own.is_empty() {
        all.iter().take(1).collect()
    } else {
        own
    }
}

/// Whether "What's new" opens by itself at this start, and with what since: after an update
/// (the version seen last is older), never at a first start (the welcome covers that), and
/// never while the welcome is due. Settings keeps `current` as seen either way.
pub fn due(last_seen: &str, current: &str, onboarded: bool, welcome_due: bool) -> bool {
    if welcome_due || (compare(last_seen, current).is_ge() && !last_seen.is_empty()) {
        return false;
    }
    // Nothing on record: an update from before versions were remembered, if you'd used it.
    !last_seen.is_empty() || onboarded
}

/// A block of a release's notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading(String),
    Paragraph(String),
    Bullet(String),
}

/// The blocks of a release's body, in order.
pub fn blocks(body: &str) -> Vec<Block> {
    body.lines()
        .filter_map(|line| {
            if let Some(item) = line.strip_prefix("- ") {
                Some(Block::Bullet(item.trim().to_owned()))
            } else if let Some(h) = line.strip_prefix("### ") {
                Some(Block::Heading(h.trim().to_owned()))
            } else if line.trim().is_empty() {
                None
            } else {
                Some(Block::Paragraph(line.trim().to_owned()))
            }
        })
        .collect()
}

/// A run of a line's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Plain(String),
    Bold(String),
    Code(String),
}

impl Span {
    pub fn text(&self) -> &str {
        match self {
            Span::Plain(t) | Span::Bold(t) | Span::Code(t) => t,
        }
    }
}

/// `**bold**`, `` `code` `` and `[link](…)` inside a line; a link keeps only its words.
pub fn spans(line: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut rest = line;
    let flush = |plain: &mut String, out: &mut Vec<Span>| {
        if !plain.is_empty() {
            out.push(Span::Plain(std::mem::take(plain)));
        }
    };
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**").filter(|e| *e > 0) {
                flush(&mut plain, &mut out);
                out.push(Span::Bold(after[..end].to_owned()));
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`').filter(|e| *e > 0) {
                flush(&mut plain, &mut out);
                out.push(Span::Code(after[..end].to_owned()));
                rest = &after[end + 1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some((words, tail)) = after.split_once("](") {
                if let Some(close) = tail.find(')').filter(|_| !words.contains(']')) {
                    plain.push_str(words);
                    rest = &tail[close + 1..];
                    continue;
                }
            }
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut plain, &mut out);
    out
}

/// A line as plain words: what a screen reader gets, and a test.
pub fn plain(line: &str) -> String {
    spans(line).iter().map(Span::text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\n## 1.0.0 — rebuilt\n\n- **Faster.** Every `tab` opens at once.\n\n## Unreleased\n\n- nope\n\n## 0.10.14 — files land\n\n### On Windows\n- One [link](https://example.com/x) here.\nA paragraph.\n\n## 0.10.2 — older\n\n- old\n";

    #[test]
    fn releases_are_read_newest_first() {
        let all = parse(LOG);
        let v: Vec<(&str, &str)> = all.iter().map(|r| (r.version.as_str(), r.title.as_str())).collect();
        assert_eq!(v, [("1.0.0", "rebuilt"), ("0.10.14", "files land"), ("0.10.2", "older")]);
        assert_eq!(blocks(&all[1].body), [Block::Heading("On Windows".into()), Block::Bullet("One [link](https://example.com/x) here.".into()), Block::Paragraph("A paragraph.".into())]);
    }

    #[test]
    fn versions_compare_as_people_expect() {
        use std::cmp::Ordering::*;
        assert_eq!(compare("0.3.2", "0.3.10"), Less);
        assert_eq!(compare("1.0.0-canary", "1.0.0"), Equal);
        assert_eq!(compare("1.0.0", "0.10.14"), Greater);
        assert_eq!(compare("nonsense", "0.0.1"), Less);
    }

    #[test]
    fn the_notes_since_the_last_version_seen() {
        let all = parse(LOG);
        let v = |r: Vec<&Release>| r.iter().map(|r| r.version.clone()).collect::<Vec<_>>();
        assert_eq!(v(since(&all, "0.10.2", "1.0.0-canary")), ["1.0.0", "0.10.14"]);
        assert_eq!(v(since(&all, "0.10.14", "0.10.14")), Vec::<String>::new());
        assert_eq!(v(since(&all, "", "0.10.14")), ["0.10.14"]);
        assert_eq!(v(current(&all, "1.0.0-canary")), ["1.0.0"]);
        assert_eq!(v(current(&all, "9.9.9")), ["1.0.0"]);
    }

    #[test]
    fn it_opens_by_itself_only_after_an_update() {
        assert!(due("0.10.14", "1.0.0", true, false));
        // A fresh install: the welcome says hello instead.
        assert!(!due("", "1.0.0", false, false));
        // Used before versions were remembered.
        assert!(due("", "1.0.0", true, false));
        assert!(!due("1.0.0", "1.0.0-canary", true, false));
        assert!(!due("0.10.14", "1.0.0", true, true));
    }

    #[test]
    fn a_line_reads_bold_code_and_link_words() {
        assert_eq!(spans("**Faster.** Every `tab` opens."), [Span::Bold("Faster.".into()), Span::Plain(" Every ".into()), Span::Code("tab".into()), Span::Plain(" opens.".into())]);
        assert_eq!(plain("See [the guide](https://example.com/g) now"), "See the guide now");
        assert_eq!(plain("a ** b ` c [d"), "a ** b ` c [d");
    }

    #[test]
    fn the_bundled_changelog_has_1_0s_notes() {
        let all = releases();
        assert!(all.iter().any(|r| r.version == "1.0.0"), "CHANGELOG.md has no 1.0.0 section");
        assert!(all.iter().any(|r| r.version == "0.10.14"));
    }
}
