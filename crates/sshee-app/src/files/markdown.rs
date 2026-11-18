//! A Markdown file as the preview shows it formatted: blocks (headings, paragraphs, lists,
//! quotes, code, tables, rules) of text with styled ranges (bold, italic, code, struck, links).
//! The screen draws each block; nothing here draws. 0.10 used react-markdown with GitHub's
//! extensions (tables, task lists, strikethrough, bare links); this reads the same files well
//! enough to read them, not to the letter of CommonMark.
//!
//! The file comes from a server, so nothing in it is trusted: raw HTML is dropped, images are
//! not fetched (their alt text stands in), and only `http:`, `https:` and `mailto:` links may
//! leave the app ([`is_external`]).

/// How a range of a block's text is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Style {
    Strong,
    Emph,
    Code,
    Strike,
    /// A link to `url`; only [`is_external`] ones open (in the system's browser).
    Link(String),
    /// An image not loaded: its alt text in a quiet chip; `src` is where it would come from.
    Image(String),
}

/// A styled byte range of a block's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

/// A run of text with its styled ranges. Ranges may nest (a link in bold).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Styled {
    pub text: String,
    pub marks: Vec<Mark>,
}

/// What a list item starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bullet {
    Dot,
    Number(u64),
    /// A task, done or not: drawn as a box the file ticks, never changed here.
    Task(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `level` 1 to 6.
    Heading { level: u8, text: Styled },
    Paragraph(Styled),
    /// One list item; `depth` 0 for the outermost list.
    Item { depth: u8, bullet: Bullet, text: Styled },
    Quote(Styled),
    /// Lines as they are; `lang` is what followed the opening fence.
    Code { lang: String, text: String },
    Rule,
    Table { head: Vec<Styled>, rows: Vec<Vec<Styled>> },
}

/// Links that may leave the app, for the system's browser. Anything else stays put.
pub fn is_external(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("mailto:")
}

/// Leading spaces, a tab counting four.
fn indent(line: &str) -> usize {
    let mut n = 0;
    for c in line.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n += 4,
            _ => break,
        }
    }
    n
}

fn is_rule(line: &str) -> bool {
    let t: String = line.trim().chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// `#`s, a space, the heading.
fn heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start();
    if indent(line) > 3 {
        return None;
    }
    let level = t.bytes().take_while(|b| *b == b'#').count();
    let rest = &t[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    // A closing run of #s goes.
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some((level as u8, if text.is_empty() && rest.trim().chars().all(|c| c == '#') { "" } else { text }))
}

/// "- x", "* x", "+ x", "1. x", "1) x": its indent, bullet and what follows.
fn list_item(line: &str) -> Option<(usize, Bullet, &str)> {
    let ind = indent(line);
    let t = line.trim_start();
    let (bullet, rest) = if let Some(rest) = t.strip_prefix(['-', '*', '+']) {
        (Bullet::Dot, rest)
    } else {
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let rest = t[digits..].strip_prefix(['.', ')'])?;
        (Bullet::Number(t[..digits].parse().ok()?), rest)
    };
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let rest = rest.trim_start();
    let task = |mark: &str, done: bool| rest.strip_prefix(mark).filter(|r| r.is_empty() || r.starts_with(' ')).map(|r| (Bullet::Task(done), r.trim_start()));
    let (bullet, rest) = task("[ ]", false).or_else(|| task("[x]", true)).or_else(|| task("[X]", true)).unwrap_or((bullet, rest));
    Some((ind, bullet, rest))
}

/// "```lang" or "~~~": the fence and what follows it.
fn fence(line: &str) -> Option<(String, &str)> {
    if indent(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then(|| (c.to_string().repeat(n), t[n..].trim()))
}

/// A table's cells, the pipes at the ends optional.
fn cells(line: &str) -> Vec<&str> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(str::trim).collect()
}

/// The row under a table's head: `|---|:--:|`, each cell dashes with an optional colon at
/// either end.
fn is_table_delimiter(line: &str) -> bool {
    line.contains('|')
        && cells(line).iter().all(|c| {
            let c = c.strip_prefix(':').unwrap_or(c);
            let c = c.strip_suffix(':').unwrap_or(c);
            !c.is_empty() && c.chars().all(|x| x == '-')
        })
}

/// A whole line of raw HTML: dropped.
fn is_html(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('<') && t.ends_with('>') && !t.starts_with("<http") && !t.starts_with("<mailto:")
}

/// The file's blocks, top to bottom.
pub fn parse(source: &str) -> Vec<Block> {
    parse_lines(source).into_iter().map(|(block, _)| block).collect()
}

/// The lines (first and last, counted from 1) of the blocks that lines `first` to `last` touch,
/// by their places in [`parse_lines`]: what a path like "notes.md:12-40" marks in the
/// formatted file. Empty when none does.
pub fn blocks_at(blocks: &[(Block, [u32; 2])], first: u32, last: u32) -> std::ops::Range<usize> {
    let hit = |r: &[u32; 2]| r[1] >= first && r[0] <= last;
    match blocks.iter().position(|(_, r)| hit(r)) {
        Some(from) => from..from + blocks[from..].iter().take_while(|(_, r)| hit(r)).count(),
        None => 0..0,
    }
}

/// [`parse`], with the lines each block was made from (first and last, counted from 1).
pub fn parse_lines(source: &str) -> Vec<(Block, [u32; 2])> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out: Vec<(Block, [u32; 2])> = Vec::new();
    // The first and the last line of the paragraph, quote or item being gathered.
    let mut open: Option<[u32; 2]> = None;
    let at = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    // A paragraph (or a quote's) being gathered, line by line.
    let mut para: Vec<&str> = Vec::new();
    let mut quote: Vec<String> = Vec::new();
    // The list item being gathered, and the indents of the open lists.
    let mut item: Option<(u8, Bullet, Vec<&str>)> = None;
    let mut list_indents: Vec<usize> = Vec::new();
    let flush = |out: &mut Vec<(Block, [u32; 2])>, para: &mut Vec<&str>, quote: &mut Vec<String>, item: &mut Option<(u8, Bullet, Vec<&str>)>, open: &mut Option<[u32; 2]>| {
        let lines = open.take().unwrap_or_default();
        if !para.is_empty() {
            out.push((Block::Paragraph(inline(&para.join("\n"))), lines));
            para.clear();
        }
        if !quote.is_empty() {
            out.push((Block::Quote(inline(&quote.join("\n"))), lines));
            quote.clear();
        }
        if let Some((depth, bullet, text)) = item.take() {
            out.push((Block::Item { depth, bullet, text: inline(&text.join("\n")) }, lines));
        }
    };
    // The paragraph, quote or item being gathered goes on to line `n`.
    let grow = |open: &mut Option<[u32; 2]>, n: u32| {
        *open = Some(open.map_or([n, n], |[from, _]| [from, n]));
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        // This line's number, counted from 1.
        let n = at(i);
        if line.trim().is_empty() {
            flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
            // A list goes on over blank lines while its next line is an item or indented.
            let next = lines[i..].iter().find(|l| !l.trim().is_empty());
            if !next.is_some_and(|l| list_item(l).is_some() || (!list_indents.is_empty() && indent(l) >= 2)) {
                list_indents.clear();
            }
            continue;
        }
        if let Some((close, lang)) = fence(line) {
            flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
            let mut body = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                i += 1;
                if l.trim_start().starts_with(&close) && l.trim().chars().all(|c| close.starts_with(c)) {
                    break;
                }
                body.push(l);
            }
            out.push((Block::Code { lang: lang.to_owned(), text: body.join("\n") }, [n, at(i)]));
            continue;
        }
        // Indented code, where it can't be a paragraph's or an item's next line.
        if indent(line) >= 4 && para.is_empty() && item.is_none() && list_indents.is_empty() && quote.is_empty() {
            let mut body = vec![line.get(4.min(line.len())..).unwrap_or("").to_owned()];
            while i < lines.len() && (indent(lines[i]) >= 4 || lines[i].trim().is_empty()) {
                body.push(lines[i].get(4.min(lines[i].len())..).unwrap_or("").to_owned());
                i += 1;
            }
            while body.last().is_some_and(|l| l.trim().is_empty()) {
                body.pop();
            }
            let last = n + at(body.len()).saturating_sub(1);
            out.push((Block::Code { lang: String::new(), text: body.join("\n") }, [n, last]));
            continue;
        }
        // A line of = or - under a paragraph makes it a heading.
        if !para.is_empty() && item.is_none() {
            let t = line.trim();
            if !t.is_empty() && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-')) {
                let level = if t.starts_with('=') { 1 } else { 2 };
                let from = open.take().map_or(n, |[from, _]| from);
                out.push((Block::Heading { level, text: inline(&para.join("\n")) }, [from, n]));
                para.clear();
                continue;
            }
        }
        if is_rule(line) && indent(line) <= 3 {
            flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
            list_indents.clear();
            out.push((Block::Rule, [n, n]));
            continue;
        }
        if let Some((level, text)) = heading(line) {
            flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
            list_indents.clear();
            out.push((Block::Heading { level, text: inline(text) }, [n, n]));
            continue;
        }
        if let Some(rest) = line.trim_start().strip_prefix('>') {
            if !para.is_empty() || item.is_some() {
                flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
            }
            list_indents.clear();
            // Nested quotes read as one.
            let mut rest = rest;
            while let Some(r) = rest.trim_start().strip_prefix('>') {
                rest = r;
            }
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            grow(&mut open, n);
            if rest.trim().is_empty() {
                if !quote.is_empty() {
                    quote.push(String::new());
                }
            } else {
                quote.push(rest.to_owned());
            }
            continue;
        }
        if !quote.is_empty() {
            flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
        }
        if let Some((ind, bullet, rest)) = list_item(line) {
            // A numbered item interrupts a paragraph only when it starts at 1.
            if para.is_empty() || !matches!(bullet, Bullet::Number(n) if n != 1) {
                flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
                while list_indents.last().is_some_and(|&d| d > ind) {
                    list_indents.pop();
                }
                if list_indents.last() != Some(&ind) {
                    list_indents.push(ind);
                }
                let depth = (list_indents.len() - 1).min(u8::MAX as usize) as u8;
                item = Some((depth, bullet, vec![rest]));
                grow(&mut open, n);
                continue;
            }
        }
        // A table: a row of cells, then a row of dashes.
        if para.is_empty() && item.is_none() && line.contains('|') && lines.get(i).is_some_and(|l| is_table_delimiter(l)) {
            let head: Vec<Styled> = cells(line).into_iter().map(inline).collect();
            let width = head.len();
            i += 1;
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                let mut row: Vec<Styled> = cells(lines[i]).into_iter().take(width).map(inline).collect();
                row.resize(width, Styled::default());
                rows.push(row);
                i += 1;
            }
            out.push((Block::Table { head, rows }, [n, at(i)]));
            continue;
        }
        if is_html(line) {
            continue;
        }
        grow(&mut open, n);
        match &mut item {
            // An item's next line, indented or lazy.
            Some((_, _, text)) => text.push(line.trim()),
            None => {
                list_indents.clear();
                para.push(line.trim_start());
            }
        }
    }
    flush(&mut out, &mut para, &mut quote, &mut item, &mut open);
    out
}

/// A block's inline Markdown, as text with styled ranges.
pub fn inline(source: &str) -> Styled {
    let mut out = Styled::default();
    let chars: Vec<char> = source.chars().collect();
    run(&chars, &mut out);
    out
}

/// Where `close` (a run of chars) next starts in `chars` from `from`, not right after an
/// opening and with something before it.
fn find(chars: &[char], from: usize, close: &[char]) -> Option<usize> {
    let n = close.len();
    (from + 1..=chars.len().saturating_sub(n)).find(|&j| chars[j..j + n] == *close && !chars[j - 1].is_whitespace() && chars[j - 1] != '\\')
}

/// The `]` that closes the `[` at `open`, nested brackets counted.
fn closing_bracket(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (j, c) in chars.iter().enumerate().skip(open) {
        match c {
            '\\' => {}
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// `(url "title")` at `at`: the url and where it ends.
fn destination(chars: &[char], at: usize) -> Option<(String, usize)> {
    if chars.get(at) != Some(&'(') {
        return None;
    }
    let end = (at + 1..chars.len()).find(|&j| chars[j] == ')')?;
    let inside: String = chars[at + 1..end].iter().collect();
    let url = inside.split_whitespace().next().unwrap_or("").trim_start_matches('<').trim_end_matches('>').to_owned();
    Some((url, end + 1))
}

fn bare_link_end(chars: &[char], at: usize) -> Option<usize> {
    let rest: String = chars[at..chars.len().min(at + 8)].iter().collect::<String>().to_ascii_lowercase();
    if !(rest.starts_with("https://") || rest.starts_with("http://")) {
        return None;
    }
    if at > 0 && chars[at - 1].is_alphanumeric() {
        return None;
    }
    let mut end = at;
    while end < chars.len() && !chars[end].is_whitespace() && chars[end] != '<' {
        end += 1;
    }
    // Punctuation at the end belongs to the sentence.
    while end > at && matches!(chars[end - 1], '.' | ',' | ':' | ';' | '!' | '?' | ')' | '\'' | '"') {
        end -= 1;
    }
    (end > at + 8).then_some(end)
}

fn run(chars: &[char], out: &mut Styled) {
    let mut i = 0;
    let wrap = |out: &mut Styled, inner: &[char], style: Style| {
        let start = out.text.len();
        run(inner, out);
        out.marks.push(Mark { start, end: out.text.len(), style });
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if chars.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) => {
                out.text.push(chars[i + 1]);
                i += 2;
            }
            // A hard break: a backslash at the end of a line.
            '\\' if chars.get(i + 1) == Some(&'\n') => {
                out.text.push('\n');
                i += 2;
            }
            '\n' => {
                // Two spaces before it break the line; else it's a space.
                let hard = out.text.ends_with("  ");
                let trimmed = out.text.trim_end_matches(' ').len();
                out.text.truncate(trimmed);
                out.text.push(if hard { '\n' } else { ' ' });
                i += 1;
            }
            '`' => {
                let n = chars[i..].iter().take_while(|x| **x == '`').count();
                let ticks = vec!['`'; n];
                let close = (i + n..=chars.len().saturating_sub(n)).find(|&j| chars[j..j + n] == *ticks && chars.get(j + n) != Some(&'`') && chars[j - 1] != '`');
                match close {
                    Some(j) => {
                        let code: String = chars[i + n..j].iter().map(|c| if *c == '\n' { ' ' } else { *c }).collect();
                        let code = if code.starts_with(' ') && code.ends_with(' ') && code.trim() != "" { code[1..code.len() - 1].to_owned() } else { code };
                        let start = out.text.len();
                        out.text.push_str(&code);
                        out.marks.push(Mark { start, end: out.text.len(), style: Style::Code });
                        i = j + n;
                    }
                    None => {
                        out.text.extend(&chars[i..i + n]);
                        i += n;
                    }
                }
            }
            '*' | '_' | '~' => {
                let n = chars[i..].iter().take_while(|x| **x == c).count();
                // A word's inner underscore (snake_case) is just a letter.
                let in_word = c == '_' && i > 0 && chars[i - 1].is_alphanumeric();
                let opens = chars.get(i + n).is_some_and(|x| !x.is_whitespace()) && !in_word;
                let (style, width) = match (c, n) {
                    ('~', 2..) => (Some(Style::Strike), 2),
                    ('~', _) => (None, n),
                    (_, 2..) => (Some(Style::Strong), 2),
                    _ => (Some(Style::Emph), 1),
                };
                let close = style.as_ref().filter(|_| opens).and_then(|_| find(chars, i + width, &vec![c; width]));
                let close = close.filter(|&j| j > i + width && (c != '_' || !chars.get(j + width).is_some_and(|x| x.is_alphanumeric())));
                match (style, close) {
                    (Some(style), Some(j)) => {
                        wrap(out, &chars[i + width..j], style);
                        i = j + width;
                    }
                    _ => {
                        out.text.extend(&chars[i..i + n]);
                        i += n;
                    }
                }
            }
            '!' if chars.get(i + 1) == Some(&'[') => {
                let parsed = closing_bracket(chars, i + 1).and_then(|close| destination(chars, close + 1).map(|(url, end)| (close, url, end)));
                match parsed {
                    Some((close, src, end)) => {
                        let alt: String = chars[i + 2..close].iter().collect();
                        let alt = if alt.trim().is_empty() { "Image".to_owned() } else { alt };
                        let start = out.text.len();
                        out.text.push_str(&alt);
                        out.marks.push(Mark { start, end: out.text.len(), style: Style::Image(src) });
                        i = end;
                    }
                    None => {
                        out.text.push('!');
                        i += 1;
                    }
                }
            }
            '[' => {
                let parsed = closing_bracket(chars, i).and_then(|close| destination(chars, close + 1).map(|(url, end)| (close, url, end)));
                match parsed {
                    Some((close, url, end)) => {
                        wrap(out, &chars[i + 1..close], Style::Link(url));
                        i = end;
                    }
                    None => {
                        out.text.push('[');
                        i += 1;
                    }
                }
            }
            '<' => {
                let end = (i + 1..chars.len()).find(|&j| chars[j] == '>');
                let inside: Option<String> = end.map(|e| chars[i + 1..e].iter().collect());
                match (end, inside) {
                    // An autolink.
                    (Some(e), Some(url)) if is_external(&url) && !url.contains(char::is_whitespace) => {
                        let start = out.text.len();
                        out.text.push_str(url.strip_prefix("mailto:").unwrap_or(&url));
                        out.marks.push(Mark { start, end: out.text.len(), style: Style::Link(url) });
                        i = e + 1;
                    }
                    // Raw HTML: dropped.
                    (Some(e), Some(tag)) if tag.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!') => i = e + 1,
                    _ => {
                        out.text.push('<');
                        i += 1;
                    }
                }
            }
            'h' | 'H' => match bare_link_end(chars, i) {
                Some(end) => {
                    let url: String = chars[i..end].iter().collect();
                    let start = out.text.len();
                    out.text.push_str(&url);
                    out.marks.push(Mark { start, end: out.text.len(), style: Style::Link(url) });
                    i = end;
                }
                None => {
                    out.text.push(c);
                    i += 1;
                }
            },
            _ => {
                out.text.push(c);
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(s: &Styled) -> Vec<(&str, Style)> {
        s.marks.iter().map(|m| (&s.text[m.start..m.end], m.style.clone())).collect()
    }

    #[test]
    fn inline_styles_links_and_code() {
        let s = inline("Run **`./deploy.sh`** then *check* the [logs](https://example.com/logs \"Logs\") or ~~not~~.");
        assert_eq!(s.text, "Run ./deploy.sh then check the logs or not.");
        assert_eq!(
            marks(&s),
            [
                ("./deploy.sh", Style::Code),
                ("./deploy.sh", Style::Strong),
                ("check", Style::Emph),
                ("logs", Style::Link("https://example.com/logs".into())),
                ("not", Style::Strike)
            ]
        );
        // snake_case stays, escapes and stray stars stay text.
        assert_eq!(inline("a_b_c and \\*x\\* and 2 * 3").text, "a_b_c and *x* and 2 * 3");
        assert!(inline("a_b_c").marks.is_empty());
        // Bare links and autolinks, the sentence's full stop left out.
        let s = inline("See https://example.com/a. Or <mailto:ops@example.com>.");
        assert_eq!(s.text, "See https://example.com/a. Or ops@example.com.");
        assert_eq!(marks(&s)[0], ("https://example.com/a", Style::Link("https://example.com/a".into())));
        assert_eq!(marks(&s)[1].1, Style::Link("mailto:ops@example.com".into()));
        // Raw HTML goes; an image is its alt text.
        let s = inline("A <b>bold</b> ![the logo](logo.png) here");
        assert_eq!(s.text, "A bold the logo here");
        assert_eq!(marks(&s), [("the logo", Style::Image("logo.png".into()))]);
        // Lines join with a space; two spaces at the end break the line.
        assert_eq!(inline("one\ntwo  \nthree").text, "one two\nthree");
    }

    #[test]
    fn each_block_knows_its_lines() {
        let doc = "# shop\n\nThe shop's web app.\nIt sells.\n\n    ./deploy.sh\n    ./check.sh\n\n- one\n  more\n- two\n\n> Ask\n> first\n\n```sh\nls\n```\n| a | b |\n|---|---|\n| 1 | 2 |\nTitle\n=====\n";
        let lines: Vec<[u32; 2]> = parse_lines(doc).into_iter().map(|(_, l)| l).collect();
        assert_eq!(lines, [[1, 1], [3, 4], [6, 7], [9, 10], [11, 11], [13, 14], [16, 18], [19, 21], [22, 23]]);
        // "README.md:4-9" marks the paragraph, the code and the first item: one run of blocks.
        let blocks = parse_lines(doc);
        assert_eq!(blocks_at(&blocks, 4, 9), 1..4);
        assert_eq!(blocks_at(&blocks, 12, 12), 0..0);
        assert_eq!(blocks_at(&blocks, 22, 22), 8..9);
        assert_eq!(blocks_at(&blocks, 90, 99), 0..0);
    }

    #[test]
    fn blocks_of_a_readme() {
        let doc = "# shop\n\nThe shop's web app.\nIt sells.\n\n## Deploying\n\n    ./deploy.sh 2024-10-12\n\n- one\n- two\n  - inner\n- [x] done\n- [ ] todo\n\n1. first\n2. second\n\n> Ask before\n> touching it.\n\n---\n\n```sh\nls -la\n```\n\n| Name | Size |\n|---|---:|\n| a | 1 |\n| b |\n\nTitle\n=====\n<div align=\"center\">\n";
        let blocks = parse(doc);
        let shapes: Vec<String> = blocks
            .iter()
            .map(|b| match b {
                Block::Heading { level, text } => format!("h{level} {}", text.text),
                Block::Paragraph(t) => format!("p {}", t.text),
                Block::Item { depth, bullet, text } => format!("li{depth} {bullet:?} {}", text.text),
                Block::Quote(t) => format!("> {}", t.text),
                Block::Code { lang, text } => format!("code[{lang}] {text}"),
                Block::Rule => "---".into(),
                Block::Table { head, rows } => format!("table {} x {}", head.len(), rows.len()),
            })
            .collect();
        assert_eq!(
            shapes,
            [
                "h1 shop",
                "p The shop's web app. It sells.",
                "h2 Deploying",
                "code[] ./deploy.sh 2024-10-12",
                "li0 Dot one",
                "li0 Dot two",
                "li1 Dot inner",
                "li0 Task(true) done",
                "li0 Task(false) todo",
                "li0 Number(1) first",
                "li0 Number(2) second",
                "> Ask before touching it.",
                "---",
                "code[sh] ls -la",
                "table 2 x 2",
                "h1 Title",
            ]
        );
        let Block::Table { rows, .. } = &blocks[14] else { panic!() };
        assert_eq!(rows[1][1], Styled::default(), "a short row is filled out");
    }

    #[test]
    fn only_web_and_mail_links_leave_the_app() {
        assert!(is_external("https://example.com") && is_external("HTTP://x") && is_external("mailto:a@example.com"));
        assert!(!is_external("file:///etc/passwd") && !is_external("javascript:alert(1)") && !is_external("docs/setup.md") && !is_external("#top"));
    }

    #[test]
    fn odd_input_never_panics() {
        for doc in ["", "#", "####### seven", "[", "![", "**", "`", "``a`", "<", "- ", "1.", "|", "|\n|-|", "> ", "```", "~~~\nx", "\t\tcode", "é*ü*", "[a](", "[a]( b )", "_é_", "***"] {
            let _ = parse(doc);
            let _ = inline(doc);
        }
        assert_eq!(parse("####### seven"), [Block::Paragraph(inline("####### seven"))]);
        assert_eq!(parse("***"), [Block::Rule]);
    }
}
