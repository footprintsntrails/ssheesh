//! Tokens, keys and passwords in terminal output, found so they can be
//! covered before they're drawn. Two kinds: the well-known shapes of tokens
//! (GitHub, GitLab, OpenAI, Anthropic, Stripe, Slack, AWS, Google, Telegram
//! bots, JWTs...) and values given to names that say they're secret
//! (DB_PASSWORD=..., api_key: ..., a password in a URL). Private key blocks
//! go by whole lines. The name next to a value stays readable.
//!
//! A port of 0.10's `src/lib/secrets.ts`, with the same results. JavaScript
//! reads `\b`, `\w` and `\d` as ASCII, so the patterns here spell that out;
//! the look-behind the URL pattern had is checked by hand, since the `regex`
//! crate has none (and runs in time linear in the text, which a long wrapped
//! line needs).
//!
//! Positions are byte offsets into the text given.

use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

/// Where a secret sits in a line: a byte range.
pub type Span = Range<usize>;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("built-in pattern")
}

/// Tokens recognizable by their shape alone: the whole match is the secret.
static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    re(&[
        r"(?-u:\b)gh[pousr]_[A-Za-z0-9]{36,}", // GitHub
        r"(?-u:\b)github_pat_[A-Za-z0-9_]{60,}",
        r"(?-u:\b)glpat-[A-Za-z0-9_-]{20,}",            // GitLab
        r"(?-u:\b)sk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}", // Anthropic, OpenAI
        r"(?-u:\b)[sr]k_(?:live|test)_[A-Za-z0-9]{16,}", // Stripe
        r"(?-u:\b)xox[abposr]-[A-Za-z0-9-]{10,}",        // Slack
        r"(?-u:\b)AKIA[0-9A-Z]{16}(?-u:\b)",             // AWS access key
        r"(?-u:\b)AIza[0-9A-Za-z_-]{35}",                // Google
        r"(?-u:\b)hf_[A-Za-z0-9]{30,}",                  // Hugging Face
        r"(?-u:\b)npm_[A-Za-z0-9]{36}",
        r"(?-u:\b)do[por]_v1_[a-f0-9]{64}",         // DigitalOcean
        r"(?-u:\b)[0-9]{8,10}:AA[A-Za-z0-9_-]{33}", // Telegram bot
        r"(?-u:\b)eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}", // JWT
    ]
    .join("|"))
});

/// A value after `KEY=`, `key: ` or `"key": `: quoted (spaces and all) or up to a space.
const VALUE: &str = r#"\s*[=:]\s*(?:"([^"\n]{4,})"|'([^'\n]{4,})'|([^\s"',;]{4,}))"#;

/// A name ending in what it holds: DB_PASSWORD, api_key, GITHUB_TOKEN, client-secret.
/// The names are matched in any case, by ASCII rules as JavaScript's `i` does.
static NAMED: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r#"(?-u:\b)[0-9A-Za-z_.-]*?(?i-u:password|passwd|passphrase|pwd|secret|token|api[_-]?key|access[_-]?key|secret[_-]?key|private[_-]?key|auth[_-]?key|credentials?)["']?{VALUE}"#
    ))
});

/// A password in a URL: scheme://user:password@host. Streamer mode may have
/// dotted out part of it (a server's user called "postgres"), so • counts too.
/// In 0.10 this began with the look-behind `(?<![\w+.•-])`: see `url_starts_here`.
static URL_PASSWORD: LazyLock<Regex> = LazyLock::new(|| re(r"[a-zA-Z•][a-zA-Z0-9+.•-]*://[^\s:/@]*:([^\s@/]{3,})@"));

/// Authorization headers.
static AUTH_HEADER: LazyLock<Regex> = LazyLock::new(|| re(r"(?-u:\b)(?:Bearer|Basic)\s+([A-Za-z0-9._~+/-]{16,}=*)"));

/// Values that are settings, not secrets: `PASSWORD=true`, `token: null`, `${TOKEN}`.
/// A variable is `${...}`, or `$` and a name in one case (`$DB_PASS`, `$db_pass`):
/// `$uperS3cret9` is a password that starts with a dollar sign.
static NOT_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(?:(?i-u:true|false|yes|no|on|off|null|none|nil|undefined|required|optional|disabled|enabled|x+)|\*+|•+|<[^>]*>|\$\{[^}]+\}|\$[A-Z_][A-Z0-9_]*|\$[a-z_][a-z0-9_]*|\{\{.*\}\}|%[0-9A-Za-z_]+%)$")
});

/// What JavaScript's `\w` takes: ASCII letters, digits and `_`.
pub(crate) fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The character just before byte `at`, if any.
pub(crate) fn char_before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

/// Bytes in the character at `at` (1 at the end, so a loop always moves on).
pub(crate) fn char_len(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(1, char::len_utf8)
}

/// The look-behind `(?<![\w+.•-])`: no part of a longer word just before.
fn url_starts_here(text: &str, at: usize) -> bool {
    !char_before(text, at).is_some_and(|c| is_word(c) || matches!(c, '+' | '.' | '•' | '-'))
}

/// The value in a match: the first of groups 1 to 3 that took part, unless it's a setting.
fn value(caps: &regex::Captures) -> Option<Span> {
    let m = (1..=3).find_map(|g| caps.get(g))?;
    (!NOT_SECRET.is_match(m.as_str())).then(|| m.range())
}

/// Where the secrets in one line of text are, sorted and merged.
pub fn find_secrets(text: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = TOKENS.find_iter(text).map(|m| m.range()).collect();
    spans.extend(NAMED.captures_iter(text).filter_map(|c| value(&c)));
    // A match that fails the look-behind goes on from its end. JavaScript tries
    // again a character later, but no match it could find starts inside this
    // one: every place in the scheme has a scheme character before it, `://`
    // can't start a scheme, and neither the user (no `:` or `/`) nor the
    // password (no `/` or `@`) can hold the `://` another match would need.
    // Trying each character again took time growing with the square of a
    // long scheme-like run, which anything printed on a server can make.
    let mut at = 0;
    while let Some(caps) = URL_PASSWORD.captures_at(text, at) {
        let whole = caps.get(0).expect("group 0");
        if url_starts_here(text, whole.start()) {
            spans.extend(value(&caps));
        }
        at = whole.end();
    }
    spans.extend(AUTH_HEADER.captures_iter(text).filter_map(|c| value(&c)));
    merge(spans)
}

/// The start of a token whose shape is already certain, at the end of a text: its fixed
/// prefix and whatever of its body came so far. `TOKENS` needs the whole token (36 characters
/// after `ghp_`), so output that stops in the middle of one (a slow link, a read boundary, an
/// agent streaming its reply) would show the part that came until the rest arrives.
static UNFINISHED_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        "(?:{})\\z",
        [
            r"(?-u:\b)gh[pousr]_[A-Za-z0-9]*",
            r"(?-u:\b)github_pat_[A-Za-z0-9_]*",
            r"(?-u:\b)glpat-[A-Za-z0-9_-]*",
            r"(?-u:\b)sk-(?:ant-|proj-)?[A-Za-z0-9_-]*",
            r"(?-u:\b)[sr]k_(?:live|test)_[A-Za-z0-9]*",
            r"(?-u:\b)xox[abposr]-[A-Za-z0-9-]*",
            r"(?-u:\b)AKIA[0-9A-Z]*",
            r"(?-u:\b)AIza[0-9A-Za-z_-]*",
            r"(?-u:\b)hf_[A-Za-z0-9]*",
            r"(?-u:\b)npm_[A-Za-z0-9]*",
            r"(?-u:\b)do[por]_v1_[a-f0-9]*",
            r"(?-u:\b)[0-9]{8,10}:AA[A-Za-z0-9_-]*",
            r"(?-u:\b)eyJ[A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]*){0,2}",
        ]
        .join("|")
    ))
});

/// A secret's name with its value begun and not ended, at the end of a text: an open quote
/// (`"token": "gh…`, which `NAMED` takes only once the quote closes), or fewer than the four
/// characters `NAMED` wants unquoted; and an Authorization header's value so far.
static UNFINISHED_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r#"(?:(?-u:\b)[0-9A-Za-z_.-]*?(?i-u:password|passwd|passphrase|pwd|secret|token|api[_-]?key|access[_-]?key|secret[_-]?key|private[_-]?key|auth[_-]?key|credentials?)["']?\s*[=:]\s*(?:"([^"\n]+)|'([^'\n]+)|([^\s"',;]{1,3}))|(?-u:\b)Bearer\s+([A-Za-z0-9._~+/-]+=*))\z"#,
    )
});

/// What of a text's end is a secret still being printed (see `UNFINISHED_TOKEN` and
/// `UNFINISHED_VALUE`), as a byte range: trailing blanks aside, the text ends inside it. Hidden
/// until the rest comes, when `find_secrets` takes over; a line that stops for good with such an
/// end ("hf_cache", say) stays dotted there, the safe way to be wrong. Not a secret a click
/// shows: what's hidden isn't all of it yet.
pub fn unfinished_secret(text: &str) -> Option<Span> {
    let text = text.trim_end();
    if let Some(m) = UNFINISHED_TOKEN.find(text) {
        return Some(m.range());
    }
    let caps = UNFINISHED_VALUE.captures(text)?;
    let m = (1..=4).find_map(|g| caps.get(g))?;
    // A setting, not a secret (`PASSWORD=no`); an unquoted value this short is all there is.
    let quoted = caps.get(1).is_some() || caps.get(2).is_some();
    (quoted || !NOT_SECRET.is_match(m.as_str())).then(|| m.range())
}

/// Sorted by start, with overlapping and touching spans made one.
pub(crate) fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    if spans.len() < 2 {
        return spans;
    }
    spans.sort_by_key(|s| s.start);
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for s in spans {
        match merged.last_mut() {
            Some(last) if s.start <= last.end => last.end = last.end.max(s.end),
            _ => merged.push(s),
        }
    }
    merged
}

static KEY_BEGIN: LazyLock<Regex> = LazyLock::new(|| re(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----"));
static KEY_END: LazyLock<Regex> = LazyLock::new(|| re(r"-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----"));
/// A line that looks like a key's body on its own: 16 or more base64 characters and nothing else.
static KEY_BODY: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*[A-Za-z0-9+/]{16,}={0,2}\s*$"));
/// A line inside a key block that is part of its body: base64 and nothing
/// else, however short. A key's last line is often short (`Qm7vT2xLp9A=`),
/// and so is the end of a line a narrow pane wrapped.
static KEY_PART: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*[A-Za-z0-9+/]+={0,2}\s*$"));
/// A header inside a key block, like `Proc-Type: 4,ENCRYPTED`.
static KEY_HEADER: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z-]+: "));

/// A line that could be part of a key's body, so it's worth looking above
/// the screen for its BEGIN. Only lines of 16 base64 characters or more
/// count here, so ordinary short words don't send anyone looking.
pub fn looks_like_key_body(line: &str) -> bool {
    KEY_BODY.is_match(line)
}

/// A key's END line: a key may have begun above it, so it's worth looking
/// above the screen too (the short last line of a body has nothing else to
/// give it away).
pub fn ends_key(line: &str) -> bool {
    KEY_END.is_match(line)
}

/// Where a private key block stands as lines are read one after another:
/// whether one began and hasn't ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyBlock {
    open: bool,
}

impl KeyBlock {
    /// Before the first line; `open` says a key began above it.
    pub fn new(open: bool) -> Self {
        KeyBlock { open }
    }

    /// A key began and hasn't ended.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Reads the next line: whether it's the body of a private key (or a
    /// header or a blank line inside its block), so it's covered whole.
    pub fn line(&mut self, line: &str) -> bool {
        if KEY_BEGIN.is_match(line) {
            self.open = !KEY_END.is_match(line);
            return false;
        }
        if self.open && KEY_END.is_match(line) {
            self.open = false;
            return false;
        }
        self.open && (KEY_PART.is_match(line) || KEY_HEADER.is_match(line) || line.trim().is_empty())
    }
}

/// For each line, whether it's the body of a private key: the lines between
/// BEGIN and END. `in_key` says a key began above the first line.
pub fn key_lines<S: AsRef<str>>(lines: &[S], in_key: bool) -> Vec<bool> {
    let mut block = KeyBlock::new(in_key);
    lines.iter().map(|line| block.line(line.as_ref())).collect()
}

/// `key_lines` over a terminal's rows: each row's text, and whether it goes
/// on from the row above (the terminal wrapped a long line there). The rows
/// a line wrapped over are read as that one line, so every row of a key's
/// body line is covered, and a BEGIN a narrow pane cut in two is still known.
pub fn key_rows<S: AsRef<str>>(rows: &[(S, bool)], in_key: bool) -> Vec<bool> {
    let mut block = KeyBlock::new(in_key);
    let mut out = Vec::with_capacity(rows.len());
    let mut first = 0;
    while first < rows.len() {
        let mut last = first;
        while rows.get(last + 1).is_some_and(|r| r.1) {
            last += 1;
        }
        let line: String = rows[first..=last].iter().map(|r| r.0.as_ref()).collect();
        let key = block.line(&line);
        out.extend(std::iter::repeat_n(key, last - first + 1));
        first = last + 1;
    }
    out
}

/// Whether a key is still open after these lines (it began and hasn't ended).
pub fn key_open_after<S: AsRef<str>>(lines: &[S]) -> bool {
    let mut block = KeyBlock::default();
    for line in lines {
        block.line(line.as_ref());
    }
    block.is_open()
}

/// Each character of `text` inside `spans` turned into a `•`, one per
/// character, so the text keeps its length in characters and its columns.
/// `spans` are sorted byte ranges that don't overlap.
pub fn dot_out(text: &str, spans: &[Span]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for s in spans {
        out.push_str(&text[at..s.start]);
        out.extend(text[s.start..s.end].chars().map(|_| '•'));
        at = s.end;
    }
    out.push_str(&text[at..]);
    out
}

/// A key's body line as it may be shown: every character but spaces a `•`.
pub fn dot_key_line(line: &str) -> String {
    line.chars().map(|c| if c.is_whitespace() { c } else { '•' }).collect()
}

/// The same text with its secrets dotted out, for showing output elsewhere
/// (the "while you were away" card, a notification).
pub fn mask_secrets(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let keys = key_lines(&lines, false);
    lines
        .iter()
        .zip(keys)
        .map(|(line, key)| if key { dot_key_line(line) } else { dot_out(line, &merge(find_secrets(line).into_iter().chain(unfinished_secret(line)).collect())) })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Secrets are covered by your choice, and always while streaming.
pub fn secrets_hidden(hide_secrets: bool, streaming: bool) -> bool {
    hide_secrets || streaming
}
