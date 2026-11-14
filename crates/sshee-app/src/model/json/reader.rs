//! JSON text read the way `JSON.parse` reads it where serde_json is stricter, and as far as it
//! goes where the text stops making sense.
//!
//! One difference matters for 0.10: a JavaScript string can hold half of a UTF-16 surrogate
//! pair, and 0.10 makes such halves. It cuts a custom theme's name to 40 UTF-16 units
//! (`slice(0, 40)` in ThemeEditor.tsx and themes.ts), which can land between the two halves of
//! an emoji. `JSON.stringify` writes the half that's left as an escape (`"\ud83c"`) and
//! `JSON.parse` reads it back, but serde_json refuses the whole text. A Rust string can't hold
//! half a character, so here it becomes U+FFFD, which is how 0.10 drew it anyway, and the place
//! is noted.
//!
//! The reader keeps its own stack instead of recursing, so a damaged store can't overflow it,
//! and stops at serde_json's depth: deeper isn't anything 0.10 wrote.

use super::{Fix, Json, JsonMap, Place, Problem};

/// The deepest lists and objects go, as in serde_json.
const MAX_DEPTH: usize = 128;

/// What [`read`] made of a text.
pub(super) struct Read {
    /// Everything, or what was whole up to where the text stopped making sense, with the lists
    /// and objects open there closed. `None` when not even the first value began.
    pub value: Option<Json>,
    /// The text was read to its end and nothing in it was wrong but half characters.
    pub whole: bool,
    /// Every string that had half a character, by its place from the top.
    pub mended: Vec<Problem>,
}

/// Reads `text`, never failing: see [`Read`].
pub(super) fn read(text: &str) -> Read {
    let mut reader = Reader { text, bytes: text.as_bytes(), pos: 0, mended: vec![] };
    let mut stack = vec![];
    let mut top = None;
    let whole = reader.values(&mut stack, &mut top);
    // Close what's still open, innermost first. An object's last key without a value goes.
    while let Some(frame) = stack.pop() {
        let value = frame.close();
        match stack.last_mut() {
            Some(parent) => parent.push(value),
            None => top = Some(value),
        }
    }
    Read { value: top, whole, mended: reader.mended }
}

/// A list or an object being read, and where it is.
enum Frame {
    List { items: Vec<Json>, at: Place },
    Object { map: JsonMap, key: Option<String>, at: Place },
}

impl Frame {
    /// The place of the value that comes next in it.
    fn next_place(&self) -> Place {
        match self {
            Frame::List { items, at } => at.item(items.len()),
            Frame::Object { key, at, .. } => at.key(key.as_deref().unwrap_or_default()),
        }
    }

    fn push(&mut self, value: Json) {
        match self {
            Frame::List { items, .. } => items.push(value),
            // A key written twice keeps its first place and its last value, as in JSON.parse.
            Frame::Object { map, key, .. } => {
                if let Some(key) = key.take() {
                    map.insert(key, value);
                }
            }
        }
    }

    fn close(self) -> Json {
        match self {
            Frame::List { items, .. } => Json::Array(items),
            Frame::Object { map, .. } => Json::Object(map),
        }
    }
}

/// What may come next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    Value,
    /// Just after `[`.
    ValueOrClose,
    /// Just after a `,` in an object.
    Key,
    /// Just after `{`.
    KeyOrClose,
    Colon,
    CommaOrClose,
    /// The top value is done: only spaces may follow.
    End,
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    mended: Vec<Problem>,
}

impl Reader<'_> {
    /// Reads values onto `stack` until the text ends or stops making sense. `true` when it all
    /// made sense to the end.
    fn values(&mut self, stack: &mut Vec<Frame>, top: &mut Option<Json>) -> bool {
        let mut want = Want::Value;
        loop {
            self.skip_spaces();
            let Some(&b) = self.bytes.get(self.pos) else { return want == Want::End };
            match want {
                Want::End => return false,
                Want::Colon => {
                    if b != b':' {
                        return false;
                    }
                    self.pos += 1;
                    want = Want::Value;
                }
                Want::CommaOrClose => {
                    let in_list = matches!(stack.last(), Some(Frame::List { .. }));
                    want = match b {
                        b',' if in_list => Want::Value,
                        b',' => Want::Key,
                        b']' if in_list => close(stack, top),
                        b'}' if !in_list => close(stack, top),
                        _ => return false,
                    };
                    self.pos += 1;
                }
                Want::Key | Want::KeyOrClose => {
                    if b == b'}' && want == Want::KeyOrClose {
                        self.pos += 1;
                        want = close(stack, top);
                        continue;
                    }
                    if b != b'"' {
                        return false;
                    }
                    let Some((key, mended)) = self.string() else { return false };
                    let Some(Frame::Object { key: slot, at, .. }) = stack.last_mut() else { return false };
                    if mended {
                        self.mended.push(at.key(&key).mended());
                    }
                    *slot = Some(key);
                    want = Want::Colon;
                }
                Want::Value | Want::ValueOrClose => {
                    if b == b']' && want == Want::ValueOrClose {
                        self.pos += 1;
                        want = close(stack, top);
                        continue;
                    }
                    let value = match b {
                        b'{' | b'[' => {
                            if stack.len() >= MAX_DEPTH {
                                return false;
                            }
                            self.pos += 1;
                            let at = stack.last().map_or_else(Place::root, Frame::next_place);
                            if b == b'{' {
                                stack.push(Frame::Object { map: JsonMap::new(), key: None, at });
                                want = Want::KeyOrClose;
                            } else {
                                stack.push(Frame::List { items: vec![], at });
                                want = Want::ValueOrClose;
                            }
                            continue;
                        }
                        b'"' => {
                            let Some((text, mended)) = self.string() else { return false };
                            if mended {
                                let at = stack.last().map_or_else(Place::root, Frame::next_place);
                                self.mended.push(at.mended());
                            }
                            Some(Json::String(text))
                        }
                        b't' => self.word("true", Json::Bool(true)),
                        b'f' => self.word("false", Json::Bool(false)),
                        b'n' => self.word("null", Json::Null),
                        b'-' | b'0'..=b'9' => self.number(!stack.is_empty()),
                        _ => None,
                    };
                    let Some(value) = value else { return false };
                    want = deliver(stack, top, value);
                }
            }
        }
    }

    fn skip_spaces(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Option<Json> {
        self.bytes[self.pos..].starts_with(word.as_bytes()).then(|| {
            self.pos += word.len();
            value
        })
    }

    /// A number, read by serde_json so it comes out exactly as a whole parse would make it.
    /// One that runs to the very end of a text cut short inside a list or object may itself be
    /// cut short (`12` of `125`), so it isn't taken.
    fn number(&mut self, inside: bool) -> Option<Json> {
        let start = self.pos;
        while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) {
            self.pos += 1;
        }
        if inside && self.pos == self.bytes.len() {
            return None;
        }
        serde_json::from_str(&self.text[start..self.pos]).ok()
    }

    /// The string whose opening quote is at `pos`, and whether half a character in it was
    /// replaced. `None` when it doesn't end, or holds what JSON doesn't allow.
    fn string(&mut self) -> Option<(String, bool)> {
        self.pos += 1;
        let mut out = String::new();
        let mut mended = false;
        loop {
            let start = self.pos;
            while matches!(self.bytes.get(self.pos), Some(&b) if b != b'"' && b != b'\\' && b >= 0x20) {
                self.pos += 1;
            }
            // Only ASCII bytes stop the run, so it ends on a character boundary.
            out.push_str(&self.text[start..self.pos]);
            match *self.bytes.get(self.pos)? {
                b'"' => {
                    self.pos += 1;
                    return Some((out, mended));
                }
                b'\\' => {
                    let escape = *self.bytes.get(self.pos + 1)?;
                    self.pos += 2;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let unit = self.hex4(self.pos)?;
                            self.pos += 4;
                            let low = self.low_half_next();
                            match (unit, low) {
                                (0xD800..=0xDBFF, Some(low)) => {
                                    self.pos += 6;
                                    let c = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                    out.push(char::from_u32(c)?);
                                }
                                (0xD800..=0xDFFF, _) => {
                                    out.push(char::REPLACEMENT_CHARACTER);
                                    mended = true;
                                }
                                _ => out.push(char::from_u32(unit)?),
                            }
                        }
                        _ => return None,
                    }
                }
                // A control character as it is: JSON.parse refuses it too.
                _ => return None,
            }
        }
    }

    /// The second half of a surrogate pair, if an escape of one is next.
    fn low_half_next(&self) -> Option<u32> {
        if self.bytes.get(self.pos..self.pos + 2)? != b"\\u" {
            return None;
        }
        self.hex4(self.pos + 2).filter(|unit| (0xDC00..=0xDFFF).contains(unit))
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        let digits = self.bytes.get(at..at + 4)?;
        digits.iter().try_fold(0u32, |n, &d| Some(n * 16 + (d as char).to_digit(16)?))
    }
}

/// Puts a finished value in the list or object it's in, or at the top.
fn deliver(stack: &mut [Frame], top: &mut Option<Json>, value: Json) -> Want {
    match stack.last_mut() {
        Some(frame) => {
            frame.push(value);
            Want::CommaOrClose
        }
        None => {
            *top = Some(value);
            Want::End
        }
    }
}

/// Closes the innermost list or object and puts it where it goes.
fn close(stack: &mut Vec<Frame>, top: &mut Option<Json>) -> Want {
    match stack.pop() {
        Some(frame) => deliver(stack, top, frame.close()),
        None => Want::End,
    }
}

impl Place {
    fn mended(self) -> Problem {
        Problem { at: self.at, found: "text", fix: Fix::HalfCharacter, shown: self.shown }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_serde_json_reads_the_same_way() {
        for text in [
            r#"{"z":1,"a":{"y":2.5,"b":[1,2,{}]},"m":null,"n":-3,"t":true,"f":false,"s":"x\"\\\/\b\f\n\r\t\u00e9\ud83c\udf19é🌙"}"#,
            r#"  [ 1 , [ ] , { } , "" ]  "#,
            r#"{"a":1,"a":2,"b":3}"#,
            "12345678901234567890123",
            "-0.5e-3",
        ] {
            let read = read(text);
            assert!(read.whole && read.mended.is_empty(), "{text}");
            assert_eq!(read.value.unwrap(), serde_json::from_str::<Json>(text).unwrap(), "{text}");
        }
    }

    #[test]
    fn half_a_character_becomes_a_replacement_and_is_noted() {
        let read = read(r#"{"customThemes":[{"name":"rota\ud83c"}],"k\udf19":["\udf19x","\uD83C\u0041"]}"#);
        assert!(read.whole);
        let value = read.value.unwrap();
        assert_eq!(value.to_text(), r#"{"customThemes":[{"name":"rota�"}],"k�":["�x","�A"]}"#);
        let at: Vec<_> = read.mended.iter().map(|p| p.at.as_str()).collect();
        assert_eq!(at, ["customThemes[0].name", "k�", "k�[0]", "k�[1]"]);
    }

    #[test]
    fn a_text_cut_short_keeps_what_was_whole() {
        let cut = read(r#"{"panes":{"p1":{"id":"p1","sessionKey":"k1"},"p2":{"id":"p2","sessionKey":"k2","n":12"#);
        assert!(!cut.whole);
        assert_eq!(cut.value.unwrap().to_text(),r#"{"panes":{"p1":{"id":"p1","sessionKey":"k1"},"p2":{"id":"p2","sessionKey":"k2"}}}"#);
        assert_eq!(read(r#"{"a":"unfinished"#).value.unwrap().to_text(), "{}");
        assert_eq!(read(r#"[1,2,"#).value.unwrap().to_text(), "[1,2]");
        assert_eq!(read(r#"[1,tru"#).value.unwrap().to_text(), "[1]");
        assert!(read("").value.is_none() && !read("").whole);
    }

    #[test]
    fn stops_at_what_json_parse_refuses_too() {
        for text in ["{\"a\":\"tab\there\"}", "[1,]", "{\"a\" 1}", "[1] 2", "{'a':1}", "[\"\\x\"]", "[01]"] {
            assert!(!read(text).whole, "{text}");
        }
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(!read(&deep).whole);
    }
}
