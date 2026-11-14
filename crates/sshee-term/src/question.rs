//! Reading an AI agent's question off its terminal screen, so it can be shown and answered
//! outside the terminal: who asks, what about, the question, and the answers to pick from.
//!
//! The reader gets the screen as the app shows it (secrets already dotted, see `present`),
//! and the agent running there (the app knows it from the foreground program). It knows the
//! layout of each agent's question and nothing else: a screen it doesn't fully recognise is
//! [`Reading::Unsure`], never a guess. What comes out of here ends up typed into someone's
//! server, so every doubt is a refusal:
//!
//! - the question must be the last thing on the screen (text printed after it means the
//!   screen moved on, or the "question" is a file's content scrolling by);
//! - the agent's own cursor must be on exactly one answer (prose that merely lists
//!   "1. … 2. …" has none);
//! - numbered answers must count 1, 2, 3… with none missing;
//! - the top of the question must be on screen (one taller than the screen can't be shown
//!   whole, so it can't be answered blind);
//! - an answer picked by ticking several boxes is not one answer.
//!
//! How an answer is given differs by agent, and is part of what's read ([`How`], [`Keys`]):
//! Claude Code takes the answer's digit; Codex prints each answer's own key beside it;
//! OpenCode and oh-my-pi have no keys for answers, only a highlight to move and Return.

use std::hash::{Hash, Hasher};
use std::ops::Range;

/// The agents whose questions are known here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Asker {
    ClaudeCode,
    Codex,
    OpenCode,
    /// oh-my-pi.
    Omp,
}

/// One row of the screen as shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// Runs of characters (by their place in `text`) drawn on a highlight: inverse video, or a
    /// background of their own. OpenCode marks the answer its cursor is on this way only.
    pub lit: Vec<Range<usize>>,
}

impl Line {
    pub fn plain(text: &str) -> Line {
        Line { text: text.to_owned(), lit: Vec::new() }
    }
}

/// What kind of answer an option is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChoiceKind {
    Plain,
    /// A standing permission ("Yes, and don't ask again for…", "Allow always"): the agent
    /// won't ask the next time.
    Always,
    /// "No, and tell … what to do differently": the agent then waits for your words.
    Text,
}

/// What to type for an answer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Keys {
    /// These characters, as typed.
    Text(String),
    Escape,
}

impl Keys {
    /// As the app names it: "Sent 1 to Claude Code", "Sent Esc to Codex".
    pub fn shown(&self) -> &str {
        match self {
            Keys::Text(t) => t,
            Keys::Escape => "Esc",
        }
    }
}

/// One answer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Choice {
    /// Its place among the answers, from 1: the agent's own number where it prints one.
    pub number: u8,
    /// As the agent words it, without the key hint at its end ("(esc)").
    pub label: String,
    pub kind: ChoiceKind,
    /// The key that picks it, where the agent has one ([`How::Key`]).
    pub keys: Option<Keys>,
}

/// Which way the agent's highlight moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Up and Down.
    Rows,
    /// Left and Right.
    Columns,
}

/// How an answer is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum How {
    /// Each answer has a key of its own ([`Choice::keys`]).
    Key,
    /// The highlight is moved onto the answer, then Return.
    Highlight(Axis),
}

/// A question, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub asker: Asker,
    /// What it's about, as the agent heads it ("Bash command", "Edit file").
    pub title: Option<String>,
    /// What's shown above the question: the command, or the summary of a change.
    pub context: Vec<String>,
    pub question: String,
    pub options: Vec<Choice>,
    /// The answer the agent's own cursor is on, by its place in `options`.
    pub highlighted: usize,
    pub how: How,
}

impl Question {
    /// A number that is the same for the same question and answers, wherever the agent's
    /// cursor is: what was drawn is compared with what's on screen now before anything is
    /// sent.
    pub fn fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.asker, &self.title, &self.context, &self.question, &self.options, self.how).hash(&mut h);
        h.finish()
    }

    /// The same question with the same answers (the cursor may have moved).
    pub fn same_as(&self, other: &Question) -> bool {
        self.fingerprint() == other.fingerprint()
    }
}

/// Why a screen that looks like a question isn't read as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unsure {
    /// The top of the question is above the screen.
    Cut,
    /// The answers' numbers don't count 1, 2, 3….
    Numbers,
    /// The agent's cursor is on no answer, or on more than one.
    Cursor,
    /// Something is printed after the answers.
    Trailing,
    /// No question is worded above the answers.
    NoQuestion,
    /// Several answers can be ticked.
    Several,
    /// An answer has no key the reader knows.
    NoKey,
    /// The screen can't be read now: the terminal hasn't started, or it's scrolled back. Said
    /// by whoever holds the screen, not by [`read`].
    Unseen,
}

/// What the reader makes of a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// No question is on the screen.
    Nothing,
    /// Something that may be a question, which the reader won't guess about.
    Unsure(Unsure),
    Question(Question),
}

impl Reading {
    pub fn question(&self) -> Option<&Question> {
        match self {
            Reading::Question(q) => Some(q),
            _ => None,
        }
    }
}

/// Reads the question on `screen` (its rows, top to bottom) as `asker` lays one out.
pub fn read(asker: Asker, screen: &[Line]) -> Reading {
    match asker {
        Asker::ClaudeCode => numbered(asker, screen),
        Asker::Codex => numbered(asker, screen),
        Asker::OpenCode => opencode(screen),
        Asker::Omp => omp(screen),
    }
}

/// [`read`] for rows without highlights (tests, a tmux capture).
pub fn read_text<S: AsRef<str>>(asker: Asker, rows: &[S]) -> Reading {
    let lines: Vec<Line> = rows.iter().map(|r| Line::plain(r.as_ref())).collect();
    read(asker, &lines)
}

// ---------------------------------------------------------------------------------------------
// Rows

/// The marks agents draw before the answer their cursor is on.
const CURSORS: [char; 6] = ['❯', '›', '>', '▶', '▸', '\u{f054}'];

fn is_side(c: char) -> bool {
    matches!(c, '│' | '┃' | '║' | '▏' | '▕')
}

fn is_solid(c: char) -> bool {
    matches!(c, '─' | '━' | '═' | '╭' | '╮' | '╰' | '╯' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┏' | '┓' | '┗' | '┛')
}

fn is_dashed(c: char) -> bool {
    matches!(c, '╌' | '╍' | '┄' | '┅' | '┈' | '┉')
}

/// A box's top or bottom, or a rule across: line characters and nothing else, or mostly line
/// characters around a title ("╭─ Question ────╮").
fn is_border(inner: &str) -> bool {
    let solid = inner.chars().filter(|c| is_solid(*c)).count();
    let other = inner.chars().filter(|c| !is_solid(*c) && *c != ' ').count();
    solid >= 4 && (other == 0 || (solid >= 12 && solid >= 3 * other))
}

/// A row with the sides of the boxes around it taken off.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    /// How many boxes it's inside.
    depth: usize,
    kind: RowKind,
}

#[derive(Debug, Clone, PartialEq)]
enum RowKind {
    Blank,
    /// A box's top or bottom, or a rule across.
    Border,
    /// A dashed rule: agents draw these inside a question, around a diff.
    Dashes,
    /// Text, and how far it's indented inside its box.
    Text { indent: usize, text: String },
}

impl Row {
    fn of(line: &str) -> Row {
        let mut rest = line.trim_end();
        let mut depth = 0;
        // Up to two boxes: a question's, and a diff's inside it.
        for _ in 0..2 {
            let inner = rest.trim_start();
            let Some(first) = inner.chars().next().filter(|c| is_side(*c)) else { break };
            rest = &inner[first.len_utf8()..];
            if let Some(last) = rest.chars().next_back().filter(|c| is_side(*c)) {
                rest = rest[..rest.len() - last.len_utf8()].trim_end();
            }
            // One space of padding inside the side belongs to the box.
            rest = rest.strip_prefix(' ').unwrap_or(rest);
            depth += 1;
        }
        let text = rest.trim_end();
        let inner = text.trim_start();
        let kind = if inner.is_empty() {
            RowKind::Blank
        } else if is_border(inner) {
            RowKind::Border
        } else if inner.chars().filter(|c| is_dashed(*c)).count() >= 4 && inner.chars().all(|c| is_dashed(c) || c == ' ') {
            RowKind::Dashes
        } else {
            RowKind::Text { indent: text.chars().count() - inner.chars().count(), text: inner.to_owned() }
        };
        Row { depth, kind }
    }

    fn text(&self) -> Option<(usize, &str)> {
        match &self.kind {
            RowKind::Text { indent, text } => Some((*indent, text)),
            _ => None,
        }
    }

    fn is_blank(&self) -> bool {
        self.kind == RowKind::Blank
    }
}

/// An answer's row in a numbered list: "❯ 1. Yes", "  2. No".
#[derive(Debug, Clone, PartialEq)]
struct NumberedRow {
    cursor: bool,
    number: u8,
    label: String,
    /// Where the label starts, in characters from the row's start.
    label_at: usize,
}

fn numbered_row(indent: usize, text: &str) -> Option<NumberedRow> {
    let mut rest = text;
    let mut at = indent;
    let mut cursor = false;
    if let Some(c) = rest.chars().next().filter(|c| CURSORS.contains(c)) {
        let after = &rest[c.len_utf8()..];
        let trimmed = after.trim_start();
        // A cursor stands apart from the number: ">1." is something else.
        if trimmed.len() == after.len() {
            return None;
        }
        at += 1 + (after.chars().count() - trimmed.chars().count());
        rest = trimmed;
        cursor = true;
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 2 {
        return None;
    }
    let number: u8 = rest[..digits].parse().ok()?;
    let after = rest[digits..].strip_prefix('.')?;
    let label = after.trim_start();
    if label.is_empty() || label.len() == after.len() {
        return None;
    }
    at += digits + 1 + (after.chars().count() - label.chars().count());
    Some(NumberedRow { cursor, number, label: label.to_owned(), label_at: at })
}

/// The key hint some answers end with: "(esc)", "(shift+tab)", "(y)".
fn split_hint(label: &str) -> (&str, Option<&str>) {
    let trimmed = label.trim_end();
    if let Some(open) = trimmed.rfind(" (") {
        let hint = &trimmed[open + 2..];
        if let Some(hint) = hint.strip_suffix(')') {
            let is_keys = !hint.is_empty() && hint.len() <= 12 && hint.chars().all(|c| c.is_ascii_lowercase() || c == '+' || c == '-');
            if is_keys {
                return (trimmed[..open].trim_end(), Some(hint));
            }
        }
    }
    (trimmed, None)
}

/// What kind of answer a label words.
fn kind_of(label: &str) -> ChoiceKind {
    let l = label.to_lowercase();
    // "Don't" comes with a straight or a curly apostrophe.
    let l = l.replace('\u{2019}', "'");
    if l.contains("tell ") && l.contains("differently") || l.starts_with("other") {
        ChoiceKind::Text
    } else if l.contains("don't ask again") || l.contains("always") || l.contains("allow all") || l.contains("for this session") || l.contains("during this session") {
        ChoiceKind::Always
    } else {
        ChoiceKind::Plain
    }
}

/// Rows that may follow the answers: nothing, the box's bottom, the agent's line of key hints.
fn is_footer(asker: Asker, row: &Row) -> bool {
    match &row.kind {
        RowKind::Blank | RowKind::Border => true,
        RowKind::Dashes => false,
        RowKind::Text { text, .. } => {
            let t = text.to_lowercase();
            match asker {
                Asker::ClaudeCode => t.starts_with("esc to cancel") || t.starts_with("press esc") || t.starts_with("tab to "),
                Asker::Codex => t.starts_with("press enter to confirm"),
                Asker::OpenCode => t.contains("select") && t.contains("confirm"),
                Asker::Omp => t.contains("enter") && (t.contains("cancel") || t.contains("select") || t.contains("answer")),
            }
        }
    }
}

/// Lines with the indentation they all share taken off, and no blank ones at either end.
fn tidy(lines: Vec<(usize, String)>) -> Vec<String> {
    let shared = lines.iter().filter(|(_, t)| !t.is_empty()).map(|(i, _)| *i).min().unwrap_or(0);
    let mut out: Vec<String> = lines.into_iter().map(|(i, t)| if t.is_empty() { t } else { format!("{}{t}", " ".repeat(i - shared)) }).collect();
    while out.first().is_some_and(String::is_empty) {
        out.remove(0);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Claude Code and Codex: a numbered list with a cursor

/// The answers at the bottom of the screen: their rows, and where they start.
struct Answers {
    rows: Vec<NumberedRow>,
    first: usize,
    depth: usize,
}

/// Finds a numbered list that ends the screen. `Ok(None)`: there is none.
fn numbered_answers(asker: Asker, rows: &[Row]) -> Result<Option<Answers>, Unsure> {
    let parsed = |i: usize| rows[i].text().and_then(|(indent, text)| numbered_row(indent, text));
    let Some(last) = (0..rows.len()).rev().find(|&i| parsed(i).is_some()) else { return Ok(None) };
    let depth = rows[last].depth;
    // Upwards: answers, and the wrapped ends of long ones (indented under their label).
    let mut found: Vec<NumberedRow> = Vec::new();
    let mut wrapped: Vec<(usize, String)> = Vec::new();
    let mut first = last + 1;
    for i in (0..=last).rev() {
        if rows[i].depth != depth {
            break;
        }
        if let Some(mut row) = parsed(i) {
            if wrapped.iter().any(|(indent, _)| *indent < row.label_at) {
                break;
            }
            for (_, more) in wrapped.drain(..).rev() {
                row.label.push(' ');
                row.label.push_str(&more);
            }
            found.push(row);
            first = i;
        } else if let Some((indent, text)) = rows[i].text() {
            wrapped.push((indent, text.to_owned()));
        } else {
            break;
        }
    }
    found.reverse();
    // Downwards: the last answer's wrapped end, then nothing but the footer.
    let mut below = last + 1;
    while let Some((indent, text)) = rows.get(below).and_then(Row::text) {
        let end = found.last_mut().expect("the row at `last` is an answer");
        if rows[below].depth != depth || indent < end.label_at || is_footer(asker, &rows[below]) {
            break;
        }
        end.label.push(' ');
        end.label.push_str(text);
        below += 1;
    }
    let cursors = found.iter().filter(|r| r.cursor).count();
    // A list no cursor is on is prose: an agent's message may well end in one.
    if cursors == 0 {
        return Ok(None);
    }
    if rows[below..].iter().any(|r| !is_footer(asker, r)) {
        return Err(Unsure::Trailing);
    }
    if cursors > 1 {
        return Err(Unsure::Cursor);
    }
    if found.len() < 2 || found.len() > 9 || found.iter().enumerate().any(|(i, r)| r.number as usize != i + 1) {
        return Err(Unsure::Numbers);
    }
    Ok(Some(Answers { rows: found, first, depth }))
}

/// The key that picks a numbered answer.
fn key_for(asker: Asker, number: u8, hint: Option<&str>) -> Option<Keys> {
    match asker {
        // The digit picks the answer at once.
        Asker::ClaudeCode => Some(Keys::Text(number.to_string())),
        // Codex prints each answer's key after it: "(y)", "(a)", "(esc)".
        Asker::Codex => match hint {
            Some("esc") => Some(Keys::Escape),
            Some(key) if key.len() == 1 => Some(Keys::Text(key.to_owned())),
            _ => None,
        },
        Asker::OpenCode | Asker::Omp => None,
    }
}

fn numbered(asker: Asker, screen: &[Line]) -> Reading {
    let rows: Vec<Row> = screen.iter().map(|l| Row::of(&l.text)).collect();
    let answers = match numbered_answers(asker, &rows) {
        Ok(Some(a)) => a,
        Ok(None) => return Reading::Nothing,
        Err(why) => return Reading::Unsure(why),
    };
    let mut options = Vec::with_capacity(answers.rows.len());
    for row in &answers.rows {
        let (label, hint) = split_hint(&row.label);
        let Some(keys) = key_for(asker, row.number, hint) else { return Reading::Unsure(Unsure::NoKey) };
        options.push(Choice { number: row.number, label: label.to_owned(), kind: kind_of(label), keys: Some(keys) });
    }
    let highlighted = answers.rows.iter().position(|r| r.cursor).expect("one cursor was counted");
    let head = match asker {
        Asker::Codex => codex_head(&rows, &answers),
        _ => claude_head(&rows, &answers),
    };
    match head {
        Ok((title, context, question)) => Reading::Question(Question { asker, title, context, question, options, highlighted, how: How::Key }),
        Err(why) => Reading::Unsure(why),
    }
}

type Head = (Option<String>, Vec<String>, String);

/// Claude Code: a box, or a rule across its top. Inside: what it's about, the command or the
/// change, then the question right above the answers.
fn claude_head(rows: &[Row], answers: &Answers) -> Result<Head, Unsure> {
    let Some(at) = answers.first.checked_sub(1) else { return Err(Unsure::NoQuestion) };
    let question = match rows[at].text() {
        Some((_, text)) if rows[at].depth == answers.depth && text.ends_with('?') => text.to_owned(),
        _ => return Err(Unsure::NoQuestion),
    };
    // Up to the question's own top: the box that holds the answers, or the rule above them.
    let is_top = |r: &Row| r.kind == RowKind::Border && (r.depth < answers.depth || answers.depth == 0);
    let Some(top) = (0..at).rev().find(|&i| is_top(&rows[i])) else { return Err(Unsure::Cut) };
    let mut lines: Vec<(usize, String)> = Vec::new();
    for row in &rows[top + 1..at] {
        match &row.kind {
            RowKind::Text { indent, text } => lines.push((*indent + 2 * row.depth.saturating_sub(answers.depth), text.clone())),
            RowKind::Blank => lines.push((0, String::new())),
            RowKind::Border | RowKind::Dashes => {}
        }
    }
    let mut lines = tidy(lines);
    // The first line heads it, when a blank or the context follows: "Bash command".
    let title = (!lines.is_empty()).then(|| lines.remove(0));
    Ok((title, tidy(lines.into_iter().map(|l| (l.len() - l.trim_start().len(), l.trim_start().to_owned())).collect()), question))
}

/// Codex: the question first ("Would you like to run the following command?"), then its
/// reason and the command, then the answers.
fn codex_head(rows: &[Row], answers: &Answers) -> Result<Head, Unsure> {
    let above = &rows[..answers.first];
    // The question is the nearest line above that ends in a question mark (a command, which
    // may end in one too, starts with "$").
    let Some(at) = above.iter().rposition(|r| r.text().is_some_and(|(_, t)| t.ends_with('?') && !t.starts_with('$'))) else {
        // Text up to the very top: the question may be above the screen.
        return Err(if above.first().is_some_and(|r| !r.is_blank()) { Unsure::Cut } else { Unsure::NoQuestion });
    };
    let question = rows[at].text().map(|(_, t)| t.to_owned()).unwrap_or_default();
    let lines: Vec<(usize, String)> = above[at + 1..]
        .iter()
        .map(|r| match r.text() {
            Some((indent, text)) => (indent, text.to_owned()),
            None => (0, String::new()),
        })
        .collect();
    Ok((None, tidy(lines), question))
}

// ---------------------------------------------------------------------------------------------
// OpenCode: buttons in a row, the one its cursor is on drawn on a highlight

/// A button and where it stands in its row, in characters.
type Button = (&'static str, Range<usize>);

const OPENCODE_BUTTONS: [&[&str]; 2] = [&["Allow once", "Allow always", "Reject"], &["Confirm", "Cancel"]];

/// Where `label` stands as a whole word in `text`, in characters.
fn find_button(text: &str, label: &str, from: usize) -> Option<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let want: Vec<char> = label.chars().collect();
    (from..chars.len().saturating_sub(want.len() - 1)).find_map(|i| {
        let fits = chars[i..i + want.len()] == want[..];
        let before = i == 0 || chars[i - 1] == ' ';
        let after = chars.get(i + want.len()).is_none_or(|c| *c == ' ');
        (fits && before && after).then_some(i..i + want.len())
    })
}

fn opencode(screen: &[Line]) -> Reading {
    // The buttons: the last row that has a whole set of them, in order.
    let mut found: Option<(usize, Vec<Button>)> = None;
    for (i, line) in screen.iter().enumerate().rev() {
        for set in OPENCODE_BUTTONS {
            let mut from = 0;
            let mut places = Vec::new();
            for label in set {
                let Some(at) = find_button(&line.text, label, from) else { break };
                from = at.end;
                places.push((*label, at));
            }
            if places.len() == set.len() {
                found = Some((i, places));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let Some((at, buttons)) = found else { return Reading::Nothing };
    let rows: Vec<Row> = screen.iter().map(|l| Row::of(&l.text)).collect();
    // The line of key hints may share the buttons' row or follow it; nothing else may.
    if rows[at + 1..].iter().any(|r| !is_footer(Asker::OpenCode, r)) {
        return Reading::Unsure(Unsure::Trailing);
    }
    // Its heading: "△ Permission required".
    let Some(top) = (0..at).rev().find(|&i| rows[i].text().is_some_and(|(_, t)| t.starts_with('△'))) else {
        return Reading::Unsure(Unsure::Cut);
    };
    let question = rows[top].text().map(|(_, t)| t.trim_start_matches('△').trim().to_owned()).unwrap_or_default();
    if question.is_empty() {
        return Reading::Unsure(Unsure::NoQuestion);
    }
    let lines: Vec<(usize, String)> = rows[top + 1..at].iter().map(|r| (0, r.text().map(|(_, t)| t.to_owned()).unwrap_or_default())).collect();
    // The highlight: exactly one button is drawn on it, whole.
    let lit = |r: &Range<usize>| screen[at].lit.iter().any(|l| l.start <= r.start && r.end <= l.end);
    let on: Vec<usize> = buttons.iter().enumerate().filter(|(_, (_, r))| lit(r)).map(|(i, _)| i).collect();
    let [highlighted] = on[..] else { return Reading::Unsure(Unsure::Cursor) };
    let options = buttons
        .iter()
        .enumerate()
        .map(|(i, (label, _))| Choice {
            number: i as u8 + 1,
            label: (*label).to_owned(),
            // "Reject" then asks for your words; "Allow always" asks once more to confirm.
            kind: match *label {
                "Reject" => ChoiceKind::Text,
                "Allow always" => ChoiceKind::Always,
                _ => ChoiceKind::Plain,
            },
            keys: None,
        })
        .collect();
    Reading::Question(Question { asker: Asker::OpenCode, title: None, context: tidy(lines), question, options, highlighted, how: How::Highlight(Axis::Columns) })
}

// ---------------------------------------------------------------------------------------------
// oh-my-pi: a list of answers, each with a radio mark, a cursor before the one it's on

const RADIO_OFF: [&str; 3] = ["○", "( )", "\u{f10c}"];
const RADIO_ON: [&str; 3] = ["◉", "(o)", "\u{f192}"];
const BOXES: [&str; 6] = ["☐", "☑", "[ ]", "[x]", "\u{f096}", "\u{f14a}"];

/// An answer's row: "❯ ○ Label", "  ○ Label". `Err`: it's a tick box.
fn radio_row(text: &str, indent: usize) -> Option<Result<(bool, usize, String), Unsure>> {
    let mut rest = text;
    let mut at = indent;
    let mut cursor = false;
    if let Some(c) = rest.chars().next().filter(|c| CURSORS.contains(c)) {
        let after = rest[c.len_utf8()..].strip_prefix(' ')?;
        rest = after;
        at += 2;
        cursor = true;
    }
    if BOXES.iter().any(|b| rest.starts_with(b)) {
        return Some(Err(Unsure::Several));
    }
    let mark = RADIO_OFF.iter().chain(RADIO_ON.iter()).find(|m| rest.starts_with(**m))?;
    let label = rest[mark.len()..].strip_prefix(' ')?.trim();
    if label.is_empty() {
        return None;
    }
    Some(Ok((cursor, at + mark.chars().count() + 1, label.to_owned())))
}

fn omp(screen: &[Line]) -> Reading {
    let rows: Vec<Row> = screen.iter().map(|l| Row::of(&l.text)).collect();
    let parsed = |i: usize| rows[i].text().and_then(|(indent, text)| radio_row(text, indent));
    let Some(last) = (0..rows.len()).rev().find(|&i| parsed(i).is_some()) else { return Reading::Nothing };
    let depth = rows[last].depth;
    // Upwards: answers, and the notes under each (indented further than its label).
    let mut found: Vec<(bool, usize, String)> = Vec::new();
    let mut first = last;
    for i in (0..=last).rev() {
        if rows[i].depth != depth {
            break;
        }
        match parsed(i) {
            Some(Err(why)) => return Reading::Unsure(why),
            Some(Ok(row)) => {
                found.push(row);
                first = i;
            }
            None if rows[i].text().is_some() => {}
            None => break,
        }
    }
    found.reverse();
    // Downwards: the last answer's notes (indented under it), then only the footer.
    let label_at = found.last().map_or(0, |r| r.1);
    let mut below = last + 1;
    while rows.get(below).is_some_and(|r| r.depth == depth && r.text().is_some_and(|(indent, _)| indent >= label_at) && !is_footer(Asker::Omp, r)) {
        below += 1;
    }
    if rows[below..].iter().any(|r| !is_footer(Asker::Omp, r)) {
        return Reading::Unsure(Unsure::Trailing);
    }
    let on: Vec<usize> = found.iter().enumerate().filter(|(_, r)| r.0).map(|(i, _)| i).collect();
    let [highlighted] = on[..] else { return if on.is_empty() { Reading::Nothing } else { Reading::Unsure(Unsure::Cursor) } };
    if found.len() < 2 || found.len() > 9 {
        return Reading::Unsure(Unsure::Numbers);
    }
    // The question: the text above the answers, up to the dialog's top.
    let is_top = |r: &Row| r.kind == RowKind::Border && (r.depth < depth || depth == 0);
    let Some(top) = (0..first).rev().find(|&i| is_top(&rows[i])) else { return Reading::Unsure(Unsure::Cut) };
    let lines: Vec<(usize, String)> = rows[top + 1..first]
        .iter()
        .map(|r| match r.text() {
            Some((indent, text)) => (indent, text.to_owned()),
            None => (0, String::new()),
        })
        .collect();
    let mut lines = tidy(lines);
    let Some(question) = lines.pop().filter(|q| !q.is_empty()) else { return Reading::Unsure(Unsure::NoQuestion) };
    // What stands above the question's last paragraph is its context.
    let mut question = question.trim().to_owned();
    while lines.last().is_some_and(|l| !l.is_empty()) {
        question = format!("{} {question}", lines.pop().unwrap_or_default().trim());
    }
    let options = found
        .iter()
        .enumerate()
        .map(|(i, (_, _, label))| {
            let label = label.trim_end_matches(" (Recommended)").to_owned();
            Choice { number: i as u8 + 1, kind: kind_of(&label), label, keys: None }
        })
        .collect();
    Reading::Question(Question { asker: Asker::Omp, title: None, context: tidy(lines.into_iter().map(|l| (l.len() - l.trim_start().len(), l.trim_start().to_owned())).collect()), question, options, highlighted, how: How::Highlight(Axis::Rows) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(screen: &str) -> Vec<&str> {
        screen.lines().collect()
    }

    #[test]
    fn a_numbered_row_is_a_cursor_a_number_and_a_label() {
        let r = numbered_row(1, "❯ 1. Yes").unwrap();
        assert_eq!((r.cursor, r.number, r.label.as_str(), r.label_at), (true, 1, "Yes", 6));
        let r = numbered_row(3, "2. Yes, and don't ask again").unwrap();
        assert_eq!((r.cursor, r.number, r.label_at), (false, 2, 6));
        for not in ["Yes", "1.Yes", "1. ", ">1. Yes", "123. Yes", "- 1. Yes", "1) Yes"] {
            assert_eq!(numbered_row(0, not), None, "{not}");
        }
    }

    #[test]
    fn a_rows_boxes_come_off() {
        assert_eq!(Row::of("│ ❯ 1. Yes                │"), Row { depth: 1, kind: RowKind::Text { indent: 0, text: "❯ 1. Yes".into() } });
        assert_eq!(Row::of("│ │ src/app.rs          │ │"), Row { depth: 2, kind: RowKind::Text { indent: 0, text: "src/app.rs".into() } });
        assert_eq!(Row::of("╭──────────────╮").kind, RowKind::Border);
        assert_eq!(Row::of("│ ╰──────────╯ │"), Row { depth: 1, kind: RowKind::Border });
        assert_eq!(Row::of("╌╌╌╌╌╌╌╌╌╌╌╌").kind, RowKind::Dashes);
        assert_eq!(Row::of("│              │").kind, RowKind::Blank);
        assert_eq!(Row::of("   plain").text(), Some((3, "plain")));
        // A few dashes in a sentence are text.
        assert!(Row::of("a ── b").text().is_some());
    }

    #[test]
    fn key_hints_come_off_labels() {
        assert_eq!(split_hint("No, and tell Claude what to do differently (esc)"), ("No, and tell Claude what to do differently", Some("esc")));
        assert_eq!(split_hint("Yes, allow all edits during this session (shift+tab)"), ("Yes, allow all edits during this session", Some("shift+tab")));
        assert_eq!(split_hint("Yes, proceed (y)"), ("Yes, proceed", Some("y")));
        // Words in brackets that aren't keys stay.
        assert_eq!(split_hint("Run it (takes a while, sorry)"), ("Run it (takes a while, sorry)", None));
        assert_eq!(split_hint("Yes"), ("Yes", None));
    }

    #[test]
    fn answers_are_plain_standing_or_your_own_words() {
        assert_eq!(kind_of("Yes"), ChoiceKind::Plain);
        assert_eq!(kind_of("Yes, and don't ask again for psql commands in /srv/shop-db"), ChoiceKind::Always);
        assert_eq!(kind_of("Yes, and don\u{2019}t ask again for this command"), ChoiceKind::Always);
        assert_eq!(kind_of("Yes, allow all edits during this session"), ChoiceKind::Always);
        assert_eq!(kind_of("Allow always"), ChoiceKind::Always);
        assert_eq!(kind_of("No, and tell Claude what to do differently"), ChoiceKind::Text);
        assert_eq!(kind_of("Other (type your own)"), ChoiceKind::Text);
        assert_eq!(kind_of("No"), ChoiceKind::Plain);
    }

    #[test]
    fn a_fingerprint_ignores_where_the_cursor_is() {
        let a = "────────\n Bash command\n\n   ls\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. No, and tell Claude what to do differently (esc)\n";
        let b = a.replace("❯ 1.", "  1.").replace("  2.", "❯ 2.");
        let (Reading::Question(a), Reading::Question(b)) = (read_text(Asker::ClaudeCode, &rows(a)), read_text(Asker::ClaudeCode, &rows(&b))) else { panic!() };
        assert_eq!((a.highlighted, b.highlighted), (0, 1));
        assert!(a.same_as(&b));
        let c = Question { context: vec!["rm -rf build".into()], ..a.clone() };
        assert!(!a.same_as(&c));
    }
}
