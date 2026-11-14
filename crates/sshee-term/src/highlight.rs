//! Highlighting in terminal output: errors in red, warnings in yellow,
//! addresses underlined, and whatever rules you add. Colors are the terminal
//! scheme's own, so they fit every theme.
//!
//! A port of 0.10's `src/lib/highlight.ts`. In 0.10 a match became SGR codes
//! put into the output stream (`output.ts`); in 1.0 it's a span the
//! presentation pass hands the screen, and nothing in the stream changes.
//! The built-in patterns read `\b`, `\w` and `\d` the ASCII way JavaScript
//! does, and their look-arounds are checked by hand. Plain-text rules of
//! yours are a literal match in any case.
//!
//! Your own `/.../` rules were written as JavaScript regexes, and they run
//! over whatever a server prints, inside the pass that draws the screen. A
//! backtracking engine (JavaScript's, or `regress`, which reads its syntax)
//! can take time growing exponentially with a line for a plausible rule
//! like `/(\w+\s?)+:/`. So a rule is checked by `regress`, as JavaScript
//! would check it, and then put into the `regex` crate's syntax, which runs
//! in time linear in the text and finds the same matches. Only a rule that
//! needs what that crate lacks (a back-reference, a look-around, the `u` or
//! `v` flag) runs on `regress`, and then only if it repeats nothing that
//! itself repeats without end, and only over lines of up to `JS_TEXT_LIMIT`
//! bytes.
//!
//! Positions are byte offsets into the text given.

use crate::privacy::ipv4_at;
use crate::secrets::char_len;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::ops::Range;
use std::sync::LazyLock;

/// The scheme colors a highlight can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HighlightColor {
    Red,
    Yellow,
    Green,
    Blue,
    Magenta,
    Cyan,
}

pub const HIGHLIGHT_COLORS: [HighlightColor; 6] =
    [HighlightColor::Red, HighlightColor::Yellow, HighlightColor::Green, HighlightColor::Blue, HighlightColor::Magenta, HighlightColor::Cyan];

impl HighlightColor {
    /// The SGR color parameter 0.10 wrote for it.
    pub fn sgr(self) -> &'static str {
        match self {
            HighlightColor::Red => "31",
            HighlightColor::Yellow => "33",
            HighlightColor::Green => "32",
            HighlightColor::Blue => "34",
            HighlightColor::Magenta => "35",
            HighlightColor::Cyan => "36",
        }
    }
}

/// How a match is drawn: in a scheme color, and for errors bold, for addresses underlined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Paint {
    pub color: HighlightColor,
    pub bold: bool,
    pub underline: bool,
}

impl Paint {
    const fn plain(color: HighlightColor) -> Paint {
        Paint { color, bold: false, underline: false }
    }

    /// The SGR parameters 0.10 put before the match, `1;31` for an error.
    pub fn sgr(&self) -> String {
        let mut parts = Vec::new();
        if self.bold {
            parts.push("1");
        }
        if self.underline {
            parts.push("4");
        }
        parts.push(self.color.sgr());
        parts.join(";")
    }
}

const ERROR_PAINT: Paint = Paint { color: HighlightColor::Red, bold: true, underline: false };
const WARNING_PAINT: Paint = Paint::plain(HighlightColor::Yellow);
const ADDRESS_PAINT: Paint = Paint { color: HighlightColor::Cyan, bold: false, underline: true };

/// One of your rules, as Settings keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightRule {
    pub id: String,
    /// Plain text (any case), or a regular expression written as /like this/.
    pub pattern: String,
    pub color: HighlightColor,
}

/// A highlighted part of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub range: Range<usize>,
    pub paint: Paint,
}

/// Errors. The second group is `\b(?:[Ee]rror|[Ff]atal|[Ff]ailed|[Pp]anic)(?=[:[])`
/// in 0.10: its look-ahead is checked in `errors_from`.
static ERRORS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?-u:\b)(?:ERROR|ERR!|FATAL|FAILED|FAILURE|FAIL|PANIC|CRITICAL)(?-u:\b)",
        r"|(?-u:\b)([Ee]rror|[Ff]atal|[Ff]ailed|[Pp]anic)",
        r"|Permission denied|No such file or directory|command not found|Connection refused|Connection timed out|Segmentation fault",
        r"|Traceback \(most recent call last\)",
    ))
    .expect("pattern")
});

/// Warnings, the same way.
static WARNINGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)(?:WARNING|WARN|DEPRECATED)(?-u:\b)|(?-u:\b)([Ww]arning|[Dd]eprecated)").expect("pattern"));

/// Web links; IP addresses are found by `ipv4_at`.
static WEB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?-u:\b)https?://[^\s"'<>]+"#).expect("pattern"));

/// The first match at or after `at` of a pattern whose group 1, when it took
/// part, must be followed by `:` or `[`. A match that isn't is tried again a
/// character later; no other alternative can start where group 1 did.
fn with_look_ahead(re: &Regex, text: &str, mut at: usize) -> Option<Range<usize>> {
    while let Some(caps) = re.captures_at(text, at) {
        let m = caps.get(0)?;
        if caps.get(1).is_none() || text[m.end()..].starts_with([':', '[']) {
            return Some(m.range());
        }
        at = m.start() + char_len(text, m.start());
    }
    None
}

fn errors_from(text: &str, at: usize) -> Option<Range<usize>> {
    with_look_ahead(&ERRORS, text, at)
}

fn warnings_from(text: &str, at: usize) -> Option<Range<usize>> {
    with_look_ahead(&WARNINGS, text, at)
}

/// `\bhttps?:\/\/[^\s"'<>]+|(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?(?![\w.])`:
/// whichever starts first (they can't start at the same place).
fn addresses_from(text: &str, at: usize) -> Option<Range<usize>> {
    let web = WEB.find_at(text, at).map(|m| m.range());
    let limit = web.as_ref().map_or(text.len(), |w| w.start);
    let b = text.as_bytes();
    let ip = (at..limit).find_map(|i| if b[i].is_ascii_digit() { ipv4_at(text, i, true).map(|end| i..end) } else { None });
    ip.or(web)
}

/// The longest text, in bytes, a rule run by `regress` is run over. A
/// longer line goes without that rule's colors.
pub const JS_TEXT_LIMIT: usize = 1000;

/// A rule ready to run.
#[derive(Debug)]
enum Matcher {
    Errors,
    Warnings,
    Addresses,
    /// Plain text, in any case.
    Plain(Regex),
    /// A JavaScript regex put into the `regex` crate's syntax; `sticky` for
    /// its `y` flag: matches must follow one another from the start.
    Linear { re: Regex, sticky: bool },
    /// A JavaScript regex only a backtracking engine runs (see the module notes).
    Js { re: regress::Regex, sticky: bool },
}

impl Matcher {
    fn next(&self, text: &str, at: usize) -> Option<Range<usize>> {
        match self {
            Matcher::Errors => errors_from(text, at),
            Matcher::Warnings => warnings_from(text, at),
            Matcher::Addresses => addresses_from(text, at),
            Matcher::Plain(re) => re.find_at(text, at).map(|m| m.range()),
            Matcher::Linear { re, sticky } => {
                let m = re.find_at(text, at)?.range();
                (!*sticky || m.start == at).then_some(m)
            }
            Matcher::Js { re, sticky } => {
                if text.len() > JS_TEXT_LIMIT {
                    return None;
                }
                let m = re.find_from(text, at).next()?.range();
                (!*sticky || m.start == at).then_some(m)
            }
        }
    }
}

/// A rule of yours that compiles.
#[derive(Debug)]
pub struct CompiledRule(Matcher);

/// Turns a rule into something that finds matches, or `None` when its
/// pattern doesn't compile (an empty one, a broken regex, unknown flags), or
/// would only run on a backtracking engine and repeats a group that itself
/// repeats without end (`/(?<=x)(a+)+b/`), which can stall the terminal.
pub fn compile_rule(pattern: &str) -> Option<CompiledRule> {
    static SLASHED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/([^\n\r\u{2028}\u{2029}]+)/([a-z]*)$").expect("pattern"));
    let p = pattern.trim();
    if p.is_empty() {
        return None;
    }
    let Some(caps) = SLASHED.captures(p) else {
        let re = RegexBuilder::new(&regex::escape(p)).case_insensitive(true).build().ok()?;
        return Some(CompiledRule(Matcher::Plain(re)));
    };
    let flags = &caps[2];
    // JavaScript refuses a flag it doesn't know or one given twice, and `u` with `v`.
    let mut seen = String::new();
    for f in flags.chars() {
        if !"dgimsuvy".contains(f) || seen.contains(f) {
            return None;
        }
        seen.push(f);
    }
    if seen.contains('u') && seen.contains('v') {
        return None;
    }
    let engine_flags: String = seen.chars().filter(|f| "imsuv".contains(*f)).collect();
    // JavaScript's rules first: a pattern 0.10 refused is refused here too.
    let re = regress::Regex::with_flags(&caps[1], engine_flags.as_str()).ok()?;
    let sticky = seen.contains('y');
    if let Some(re) = linear(&caps[1], &seen) {
        return Some(CompiledRule(Matcher::Linear { re, sticky }));
    }
    if nests_repeats(&caps[1]) {
        return None;
    }
    Some(CompiledRule(Matcher::Js { re, sticky }))
}

/// A JavaScript regex (already checked) in the `regex` crate's syntax,
/// compiled, if it uses nothing that crate lacks and reads the same there.
/// Without the `u` or `v` flag only, as those change too much of the syntax.
fn linear(pattern: &str, flags: &str) -> Option<Regex> {
    if flags.contains('u') || flags.contains('v') {
        return None;
    }
    let translated = translate(pattern, flags.contains('s'))?;
    RegexBuilder::new(&translated).case_insensitive(flags.contains('i')).multi_line(flags.contains('m')).build().ok()
}

/// What JavaScript's `\s` takes: white space and line ends, as a class's insides.
const JS_SPACE: &str = r"\x{9}-\x{D} \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

/// A character as the `regex` crate reads it as itself: ASCII punctuation
/// and controls by their code (`\<` means something there, `\/` doesn't in
/// JavaScript), letters, digits and the rest as they are.
fn lit(c: char) -> String {
    if c.is_ascii_alphanumeric() || !c.is_ascii() {
        c.to_string()
    } else {
        format!("\\x{{{:X}}}", c as u32)
    }
}

/// `\d`, `\w`, `\s` and their opposites, as JavaScript reads them without
/// the `u` flag (`\d` and `\w` in ASCII): a class, and whether it's negated.
fn class_escape(c: char) -> Option<(&'static str, bool)> {
    match c {
        'd' => Some(("0-9", false)),
        'D' => Some(("0-9", true)),
        'w' => Some(("0-9A-Za-z_", false)),
        'W' => Some(("0-9A-Za-z_", true)),
        's' => Some((JS_SPACE, false)),
        'S' => Some((JS_SPACE, true)),
        _ => None,
    }
}

fn class_text(set: &str, negated: bool) -> String {
    format!("[{}{set}]", if negated { "^" } else { "" })
}

/// The character an escape stands for (`i` is just after its `\`), and how
/// many pattern characters it took after the `\`. `None` for one the
/// `regex` crate can't be given the same: a back-reference, an octal
/// escape, half of a surrogate pair, a `\c` JavaScript reads as itself.
fn char_escape(p: &[char], i: usize, in_class: bool) -> Option<(char, usize)> {
    let hex = |from: usize, n: usize| -> Option<u32> {
        let digits = p.get(from..from + n)?;
        digits.iter().all(char::is_ascii_hexdigit).then(|| digits.iter().fold(0, |v, d| v * 16 + d.to_digit(16).unwrap_or(0)))
    };
    Some(match *p.get(i)? {
        't' => ('\t', 1),
        'n' => ('\n', 1),
        'v' => ('\x0B', 1),
        'f' => ('\x0C', 1),
        'r' => ('\r', 1),
        'b' if in_class => ('\x08', 1),
        '0' if !p.get(i + 1).is_some_and(char::is_ascii_digit) => ('\0', 1),
        '0'..='9' | 'k' => return None,
        'c' => match p.get(i + 1) {
            Some(l) if l.is_ascii_alphabetic() => (char::from(*l as u8 % 32), 2),
            _ => return None,
        },
        'x' => match hex(i + 1, 2) {
            Some(v) => (char::from_u32(v)?, 3),
            None => ('x', 1),
        },
        'u' => match hex(i + 1, 4) {
            Some(v) => (char::from_u32(v)?, 5),
            None => ('u', 1),
        },
        // Anything else stands for itself: `\.`, `\/`, and in JavaScript `\a` or `\-` too.
        c => (c, 1),
    })
}

/// Where a `{n}`, `{n,}` or `{n,m}` starting at `at` ends (just past its
/// `}`), if one does. JavaScript reads any other `{` as itself.
fn braces(p: &[char], at: usize) -> Option<usize> {
    let mut i = at + 1;
    let digits = |i: usize| p[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    let n = digits(i);
    if n == 0 {
        return None;
    }
    i += n;
    if p.get(i) == Some(&',') {
        i += 1;
        i += digits(i);
    }
    (p.get(i) == Some(&'}')).then_some(i + 1)
}

/// A JavaScript regex without the `u` or `v` flag in the `regex` crate's
/// syntax, or `None` if it uses what that crate lacks or reads differently.
/// `dot_all`: the `s` flag. `i` and `m` are the builder's.
fn translate(pattern: &str, dot_all: bool) -> Option<String> {
    let p: Vec<char> = pattern.chars().collect();
    // Without `u`, JavaScript reads a character past U+FFFF as two halves.
    if p.iter().any(|&c| c as u32 > 0xFFFF) {
        return None;
    }
    let mut out = String::new();
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        i += 1;
        match c {
            '\\' => {
                let e = *p.get(i)?;
                if let Some((set, negated)) = class_escape(e) {
                    out.push_str(&class_text(set, negated));
                    i += 1;
                } else if e == 'b' || e == 'B' {
                    out.push_str(if e == 'b' { r"(?-u:\b)" } else { r"(?-u:\B)" });
                    i += 1;
                } else {
                    let (ch, n) = char_escape(&p, i, false)?;
                    out.push_str(&lit(ch));
                    i += n;
                }
            }
            '[' => i = class(&p, i, &mut out)?,
            '(' if p.get(i) == Some(&'?') => match p.get(i + 1) {
                Some(':') => {
                    out.push_str("(?:");
                    i += 2;
                }
                Some('<') if !matches!(p.get(i + 2), Some('=' | '!')) => {
                    let close = p[i + 2..].iter().position(|&c| c == '>')? + i + 2;
                    let name: String = p[i + 2..close].iter().collect();
                    let ok = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                    if !ok {
                        return None;
                    }
                    out.push_str(&format!("(?<{name}>"));
                    i = close + 1;
                }
                // A look-around, or flags set inside the pattern.
                _ => return None,
            },
            '(' | ')' | '|' | '*' | '+' | '?' | '^' | '$' => out.push(c),
            '{' => match braces(&p, i - 1) {
                Some(end) => {
                    out.extend(&p[i - 1..end]);
                    i = end;
                }
                None => out.push_str(&lit('{')),
            },
            '.' if dot_all => out.push_str(r"(?s:.)"),
            '.' => out.push_str(r"[^\n\r\x{2028}\x{2029}]"),
            _ => out.push_str(&lit(c)),
        }
    }
    Some(out)
}

/// A character class whose `[` is just before `i`, put into `out`; where
/// the pattern goes on after its `]`.
fn class(p: &[char], mut i: usize, out: &mut String) -> Option<usize> {
    let negated = p.get(i) == Some(&'^');
    if negated {
        i += 1;
    }
    if p.get(i) == Some(&']') {
        // In JavaScript `[^]` takes anything, and `[]` nothing (left to `regress`).
        if !negated {
            return None;
        }
        out.push_str(r"(?s:.)");
        return Some(i + 1);
    }
    /// What a class holds at one place: a character, or a class escape (which can't end a range).
    enum Item {
        Char(char),
        Set(String),
    }
    let item = |i: usize| -> Option<(Item, usize)> {
        if p.get(i)? != &'\\' {
            return Some((Item::Char(p[i]), i + 1));
        }
        let e = *p.get(i + 1)?;
        if let Some((set, negated)) = class_escape(e) {
            return Some((Item::Set(class_text(set, negated)), i + 2));
        }
        let (ch, n) = char_escape(p, i + 1, true)?;
        Some((Item::Char(ch), i + 1 + n))
    };
    let mut set = String::new();
    while p.get(i)? != &']' {
        let (first, next) = item(i)?;
        match first {
            Item::Set(s) => set.push_str(&s),
            Item::Char(a) => {
                // A range, unless its end is a class escape: then JavaScript reads the `-` as itself.
                if p.get(next) == Some(&'-') && p.get(next + 1).is_some_and(|&c| c != ']') {
                    if let (Item::Char(b), after) = item(next + 1)? {
                        if a > b {
                            return None;
                        }
                        set.push_str(&format!("{}-{}", lit(a), lit(b)));
                        i = after;
                        continue;
                    }
                }
                set.push_str(&lit(a));
            }
        }
        i = next;
    }
    out.push_str(&class_text(&set, negated));
    Some(i + 1)
}

/// A repeat at `i`: whether it lets what it follows come more than once,
/// whether without end, and how many pattern characters it takes (0: none).
fn repeat_at(p: &[char], i: usize) -> (bool, bool, usize) {
    let lazy = |len: usize| len + usize::from(p.get(i + len) == Some(&'?'));
    match p.get(i) {
        Some('*' | '+') => (true, true, lazy(1)),
        Some('?') => (false, false, lazy(1)),
        Some('{') => match braces(p, i) {
            Some(end) => {
                let inside: String = p[i + 1..end - 1].iter().collect();
                let mut parts = inside.split(',');
                let least: u64 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let most = parts.next();
                let unbounded = most == Some("");
                let many = unbounded || least > 1 || most.and_then(|m| m.parse::<u64>().ok()).is_some_and(|m| m > 1);
                (many, unbounded, lazy(end - i))
            }
            None => (false, false, 0),
        },
        _ => (false, false, 0),
    }
}

/// Whether a group that holds a repeat without end (`+`, `*`, `{n,}`) is
/// itself repeated, as in `(\w+\s?)+`: a backtracking engine can take time
/// growing exponentially with the text to find that such a pattern doesn't
/// match. Read roughly, the way the pattern is written; JavaScript has
/// already said it's a valid regex.
fn nests_repeats(pattern: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    // For each group open, whether it holds a repeat without end so far.
    let mut open: Vec<bool> = vec![false];
    let mut i = 0;
    while i < p.len() {
        let mut closed = None;
        match p[i] {
            '\\' => i += 2,
            '[' => {
                i += 1;
                if p.get(i) == Some(&'^') {
                    i += 1;
                }
                // `[]` and `[^]` close at once in JavaScript.
                while i < p.len() && p[i] != ']' {
                    i += if p[i] == '\\' { 2 } else { 1 };
                }
                i += 1;
            }
            '(' => {
                open.push(false);
                i += 1;
                if p.get(i) == Some(&'?') {
                    // `(?:`, `(?=`, `(?!`, `(?<=`, `(?<!`, `(?<name>`.
                    i += 1;
                    if p.get(i) == Some(&'<') && !matches!(p.get(i + 1), Some('=' | '!')) {
                        while i < p.len() && p[i] != '>' {
                            i += 1;
                        }
                    } else if p.get(i) == Some(&'<') {
                        i += 1;
                    }
                    i += 1;
                }
                continue;
            }
            ')' => {
                closed = open.pop();
                if open.is_empty() {
                    open.push(false);
                }
                i += 1;
            }
            _ => i += 1,
        }
        let (many, unbounded, len) = repeat_at(&p, i);
        if many && closed == Some(true) {
            return true;
        }
        i += len;
        if closed == Some(true) || unbounded {
            if let Some(held) = open.last_mut() {
                *held = true;
            }
        }
    }
    false
}

/// The rules in effect, made again only when a highlight setting changes.
#[derive(Debug, Default)]
pub struct Highlights {
    rules: Vec<(Matcher, Paint)>,
}

impl Highlights {
    /// From Settings: the built-in kinds that are on, and your own rules,
    /// which go first so they win where they overlap a built-in one. A rule
    /// that doesn't compile is left out; `broken_rules` names them.
    pub fn new(errors: bool, warnings: bool, addresses: bool, custom: &[HighlightRule]) -> Self {
        let mut rules: Vec<(Matcher, Paint)> = custom.iter().filter_map(|r| Some((compile_rule(&r.pattern)?.0, Paint::plain(r.color)))).collect();
        if errors {
            rules.push((Matcher::Errors, ERROR_PAINT));
        }
        if warnings {
            rules.push((Matcher::Warnings, WARNING_PAINT));
        }
        if addresses {
            rules.push((Matcher::Addresses, ADDRESS_PAINT));
        }
        Highlights { rules }
    }

    /// Whether anything gets highlighted at all.
    pub fn is_on(&self) -> bool {
        !self.rules.is_empty()
    }

    /// Non-overlapping matches in `text`, in order. Where two overlap the
    /// earlier one is kept; on a tie, the first rule's.
    pub fn find_matches(&self, text: &str) -> Vec<Match> {
        let mut found: Vec<Match> = Vec::new();
        for (rule, paint) in &self.rules {
            let mut at = 0;
            while at <= text.len() {
                let Some(m) = rule.next(text, at) else { break };
                if m.is_empty() {
                    at = m.start + char_len(text, m.start);
                    continue;
                }
                at = m.end;
                found.push(Match { range: m, paint: *paint });
                // As many as 0.10 took: the rest of the line goes plain.
                if found.len() > 400 {
                    break;
                }
            }
        }
        found.sort_by_key(|m| m.range.start);
        let mut out: Vec<Match> = Vec::with_capacity(found.len());
        for m in found {
            if out.last().is_none_or(|last| m.range.start >= last.range.end) {
                out.push(m);
            }
        }
        out
    }
}

/// Your rules that don't compile, or that `compile_rule` refuses to run, by
/// id, so Settings can say so rather than drop them quietly.
pub fn broken_rules(custom: &[HighlightRule]) -> Vec<&str> {
    custom.iter().filter(|r| compile_rule(&r.pattern).is_none()).map(|r| r.id.as_str()).collect()
}

/// How many lines hold an error, and how many (of the rest) a warning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Problems {
    pub errors: usize,
    pub warnings: usize,
}

/// Errors and warnings in some lines, by the built-in rules, whatever the settings say.
pub fn count_problems<S: AsRef<str>>(lines: &[S]) -> Problems {
    let mut p = Problems::default();
    for line in lines {
        match line_tone(line.as_ref()) {
            Some(Tone::Error) => p.errors += 1,
            Some(Tone::Warning) => p.warnings += 1,
            None => {}
        }
    }
    p
}

/// What a line would be colored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Error,
    Warning,
}

/// `Error` or `Warning` when a line would be colored as one.
pub fn line_tone(line: &str) -> Option<Tone> {
    if errors_from(line, 0).is_some() {
        Some(Tone::Error)
    } else if warnings_from(line, 0).is_some() {
        Some(Tone::Warning)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_one(rule: &str) -> bool {
        matches!(compile_rule(rule), Some(CompiledRule(Matcher::Linear { .. })))
    }

    #[test]
    fn ordinary_rules_run_on_the_linear_engine() {
        for rule in [
            r"/took \d+(\.\d+)?s/",
            r"/\b[A-Z]{2,}\b/i",
            r"/(?:GET|POST) \/\S*/",
            r"/\[[^\]]*\]/",
            r"/(?<n>\d{1,3})(\.\d{1,3}){3}/",
            r"/[^]/s",
            r"/(\w+\s?)+:/",
            r"/a{x}/",
            r"/ab/y",
            r"/\cJ/",
        ] {
            assert!(linear_one(rule), "{rule}");
        }
        // What the `regex` crate lacks, or reads differently, goes to `regress`.
        for rule in [r"/(?<=x)y/", r"/(a)\1/", r"/a/u", r"/[]/", r"/\c1/"] {
            assert!(!linear_one(rule), "{rule}");
        }
    }

    #[test]
    fn finds_repeats_nested_in_repeats() {
        for nested in [r"(a+)+", r"(\w+\s?)+:", r"((ab)*c){2,}", r"(?:x*y)*", r"(?<n>a+){3}"] {
            assert!(nests_repeats(nested), "{nested}");
        }
        for fine in [r"(a+)?", r"(ab)+", r"a+b+", r"[(a+)]+", r"\(a+\)+", r"(?<=a+)b", r"(a{2})+"] {
            assert!(!nests_repeats(fine), "{fine}");
        }
    }
}
