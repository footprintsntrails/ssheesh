//! JSON as 0.10 wrote it, and reading it without giving up on the first bad value.
//!
//! [`Json`] is a JSON value that keeps object keys in the order they were written, as
//! JavaScript does: the order of machines in the workspace's `layouts` is the order of the
//! tab strip, so it has to survive the trip. `serde_json::Value` sorts keys unless a feature
//! is switched on for every crate in the build, which this crate shouldn't decide for the
//! others.
//!
//! [`lenient`] reads a typed value out of a [`Json`] and, when something in it doesn't fit,
//! leaves out only the smallest part that doesn't (one field, one list item, one map entry)
//! and says where, instead of failing the whole store.
//!
//! [`Json::parse`] reads what `JSON.parse` reads, also half a character that 0.10 can save and
//! serde_json refuses (`reader`).

mod reader;

use indexmap::IndexMap;
use serde::de::value::{Error as DeError, MapDeserializer, SeqDeserializer};
use serde::de::{DeserializeOwned, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{forward_to_deserialize_any, Deserialize, Serialize, Serializer};
use std::fmt;

/// A JSON object with its keys in written order.
pub type JsonMap = IndexMap<String, Json>;

/// A JSON value with object keys in written order.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Json>),
    Object(JsonMap),
}

impl Json {
    /// Parses JSON text as `JSON.parse` does, keeping key order.
    ///
    /// Half a UTF-16 surrogate pair, which a JavaScript string can hold and `JSON.stringify`
    /// writes as an escape (`"\ud83c"`), becomes U+FFFD: serde_json alone would refuse the
    /// whole text. [`Json::parse_noting`] says where that happened.
    pub fn parse(text: &str) -> Result<Json, serde_json::Error> {
        Self::parse_noting(text, &mut vec![])
    }

    /// [`Json::parse`], with every string that had half a character listed in `problems` by
    /// its place from the top ([`Fix::HalfCharacter`]).
    pub fn parse_noting(text: &str, problems: &mut Vec<Problem>) -> Result<Json, serde_json::Error> {
        serde_json::from_str(text).or_else(|error| {
            // Only for a text serde_json refused. If half characters were all that was wrong,
            // the reader's value is what serde_json's would be with them replaced.
            let read = reader::read(text);
            match read.value {
                Some(value) if read.whole && !read.mended.is_empty() => {
                    problems.extend(read.mended);
                    Ok(value)
                }
                _ => Err(error),
            }
        })
    }

    /// What was whole in a text that's cut short or damaged: everything up to where it stops
    /// making sense, with the lists and objects open there closed (an object's last key
    /// without its value is left out). `None` if nothing was. The whole value when the text is
    /// sound. For finding out what a broken store held, never for reading it as the store.
    pub fn parse_partial(text: &str) -> Option<Json> {
        match serde_json::from_str(text) {
            Ok(value) => Some(value),
            Err(_) => reader::read(text).value,
        }
    }

    /// Compact JSON text, as `JSON.stringify` writes it.
    pub fn to_text(&self) -> String {
        // Writing a `Json` can't fail: its keys are strings and its numbers are finite.
        serde_json::to_string(self).expect("a Json value always serializes")
    }

    /// A typed value as JSON, with numbers written the way JavaScript writes them.
    pub fn of<T: Serialize>(value: &T) -> Json {
        // The model's types only have string map keys, so serializing them can't fail.
        let text = serde_json::to_string(value).expect("model types always serialize");
        let mut json = Json::parse(&text).expect("serde_json reads what it wrote");
        json.js_numbers();
        json
    }

    /// Reads a typed value strictly: the first thing that doesn't fit fails it all.
    pub fn typed<T: DeserializeOwned>(&self) -> Result<T, DeError> {
        T::deserialize(self)
    }

    pub fn as_object(&self) -> Option<&JsonMap> {
        match self {
            Json::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Number(n) => n.as_u64(),
            _ => None,
        }
    }

    /// JavaScript's truthiness: what `if (x)` does with it.
    pub fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(b) => *b,
            Json::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
            Json::String(s) => !s.is_empty(),
            Json::Array(_) | Json::Object(_) => true,
        }
    }

    /// What kind of value this is, in words, for reports. Never the value itself: stores
    /// hold snippets with tokens in them.
    pub fn kind(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "true or false",
            Json::Number(_) => "a number",
            Json::String(_) => "text",
            Json::Array(_) => "a list",
            Json::Object(_) => "an object",
        }
    }

    /// Whole numbers as integers, as `JSON.stringify` writes them (13, not 13.0).
    pub fn js_numbers(&mut self) {
        match self {
            Json::Number(n) => {
                if let Some(x) = n.as_f64().filter(|_| n.is_f64()) {
                    // Above 2^53 a double isn't a whole number JavaScript can count to exactly.
                    if x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_992.0 {
                        *n = if x >= 0.0 { (x as u64).into() } else { (x as i64).into() };
                    }
                }
            }
            Json::Array(items) => items.iter_mut().for_each(Json::js_numbers),
            Json::Object(map) => map.values_mut().for_each(Json::js_numbers),
            _ => {}
        }
    }

    /// Every string under the key `key`, at any depth, in written order.
    pub fn strings_under(&self, key: &str, out: &mut Vec<String>) {
        match self {
            Json::Array(items) => items.iter().for_each(|item| item.strings_under(key, out)),
            Json::Object(map) => {
                for (k, v) in map {
                    match v {
                        Json::String(s) if k == key => out.push(s.clone()),
                        _ => v.strings_under(key, out),
                    }
                }
            }
            _ => {}
        }
    }
}

impl From<&str> for Json {
    fn from(s: &str) -> Self {
        Json::String(s.to_owned())
    }
}

impl From<bool> for Json {
    fn from(b: bool) -> Self {
        Json::Bool(b)
    }
}

impl From<u64> for Json {
    fn from(n: u64) -> Self {
        Json::Number(n.into())
    }
}

impl From<JsonMap> for Json {
    fn from(map: JsonMap) -> Self {
        Json::Object(map)
    }
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => s.serialize_unit(),
            Json::Bool(b) => s.serialize_bool(*b),
            Json::Number(n) => n.serialize(s),
            Json::String(text) => s.serialize_str(text),
            Json::Array(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Json::Object(map) => {
                let mut out = s.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    out.serialize_entry(k, v)?;
                }
                out.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        d.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, b: bool) -> Result<Json, E> {
        Ok(Json::Bool(b))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Json, E> {
        Ok(Json::Number(n.into()))
    }

    fn visit_u64<E>(self, n: u64) -> Result<Json, E> {
        Ok(Json::Number(n.into()))
    }

    fn visit_f64<E>(self, x: f64) -> Result<Json, E> {
        // JavaScript writes NaN and the infinities as null.
        Ok(serde_json::Number::from_f64(x).map_or(Json::Null, Json::Number))
    }

    fn visit_str<E>(self, s: &str) -> Result<Json, E> {
        Ok(Json::String(s.to_owned()))
    }

    fn visit_string<E>(self, s: String) -> Result<Json, E> {
        Ok(Json::String(s))
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_none<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Json, D::Error> {
        Json::deserialize(d)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Json, A::Error> {
        // A key written twice keeps its first place and its last value, as in JSON.parse.
        let mut map = JsonMap::new();
        while let Some((k, v)) = access.next_entry::<String, Json>()? {
            map.insert(k, v);
        }
        Ok(Json::Object(map))
    }
}

/// Reading typed values straight out of a borrowed [`Json`].
impl<'de> Deserializer<'de> for &'de Json {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self {
            Json::Null => visitor.visit_unit(),
            Json::Bool(b) => visitor.visit_bool(*b),
            Json::Number(n) => {
                if let Some(u) = n.as_u64() {
                    visitor.visit_u64(u)
                } else if let Some(i) = n.as_i64() {
                    visitor.visit_i64(i)
                } else {
                    visitor.visit_f64(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            Json::String(s) => visitor.visit_borrowed_str(s),
            Json::Array(items) => {
                let mut seq = SeqDeserializer::new(items.iter());
                let value = visitor.visit_seq(&mut seq)?;
                seq.end()?;
                Ok(value)
            }
            Json::Object(map) => {
                let mut access = MapDeserializer::new(map.iter().map(|(k, v)| (k.as_str(), v)));
                let value = visitor.visit_map(&mut access)?;
                access.end()?;
                Ok(value)
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self {
            Json::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        // Only unit variants written as strings: the model has no other enums in serde form.
        match self {
            Json::String(s) => visitor.visit_enum(s.as_str().into_deserializer()),
            other => Err(serde::de::Error::custom(format_args!("expected text, found {}", other.kind()))),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

impl<'de> IntoDeserializer<'de, DeError> for &'de Json {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

/// Something in a store that couldn't be read as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where it was, from the store's `state`: `uiZoom`, `panes.p_x1.kind`, `snippets[3]`.
    pub at: String,
    /// What was there instead, in words ("text", "a list"), never the value itself.
    pub found: &'static str,
    /// What was done about it.
    pub fix: Fix,
    shown: String,
}

/// What was done about a [`Problem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
    /// Left out: the default stands in for it.
    LeftOut,
    /// Kept, with half a character in the text (half of a UTF-16 surrogate pair, which 0.10's
    /// cutting of names to a length can leave) replaced by U+FFFD.
    HalfCharacter,
}

impl Problem {
    /// `at` with server and machine ids (`cfg:web-01`, `host:saved:x1`) as `…`: a report may
    /// be shared, and a server's name can be private.
    pub fn shown_at(&self) -> &str {
        &self.shown
    }

    /// The same problem with its place counted from inside `key` (`state.uiZoom` becomes
    /// `uiZoom`). A place outside `key` stays as it is.
    pub fn inside(mut self, key: &str) -> Problem {
        fn strip(path: &mut String, key: &str) {
            let rest = match path.strip_prefix(key) {
                Some(rest) if rest.is_empty() || rest.starts_with('[') => rest,
                Some(rest) => match rest.strip_prefix('.') {
                    Some(rest) => rest,
                    None => return,
                },
                None => return,
            };
            *path = rest.to_owned();
        }
        strip(&mut self.at, key);
        strip(&mut self.shown, key);
        self
    }
}

/// A place in a store, as it is and as a report may show it.
#[derive(Clone)]
struct Place {
    at: String,
    shown: String,
}

impl Place {
    fn root() -> Place {
        Place { at: String::new(), shown: String::new() }
    }

    /// The entry `key` of an object here. Only server and machine ids have a colon; a field
    /// name or a pane id never does.
    fn key(&self, key: &str) -> Place {
        let shown = if key.contains(':') { "…" } else { key };
        let join = |base: &str, part: &str| if base.is_empty() { part.to_owned() } else { format!("{base}.{part}") };
        Place { at: join(&self.at, key), shown: join(&self.shown, shown) }
    }

    /// Item `i` of a list here.
    fn item(&self, i: usize) -> Place {
        Place { at: format!("{}[{i}]", self.at), shown: format!("{}[{i}]", self.shown) }
    }

    fn problem(self, found: &Json) -> Problem {
        Problem { at: self.at, found: found.kind(), fix: Fix::LeftOut, shown: self.shown }
    }
}

/// The place as [`Problem::shown_at`] gives it.
impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.fix {
            Fix::LeftOut => write!(f, "{}: unreadable ({}), left out", self.shown_at(), self.found),
            Fix::HalfCharacter => write!(f, "{}: half a character in the text, replaced with \u{fffd}", self.shown_at()),
        }
    }
}

/// Reads `T` out of `state`, leaving out only what doesn't fit, each listed in `problems`.
///
/// `T` should read from a partial object (`#[serde(default)]` on every struct), so that a
/// field can go without its struct going too. Returns `None` only if nothing at all fits,
/// for example when `state` isn't an object.
pub fn lenient<T: DeserializeOwned>(state: &Json, problems: &mut Vec<Problem>) -> Option<T> {
    if let Ok(value) = T::deserialize(state) {
        return Some(value);
    }
    let fits = |candidate: Json| T::deserialize(&candidate).is_ok();
    let salvaged = salvage(state, &fits, &Place::root(), problems)?;
    T::deserialize(&salvaged).ok()
}

/// The largest part of `value` that `fits` in its place: objects and lists keep every entry
/// that fits, and an entry that doesn't is salvaged the same way.
///
/// `fits` puts a candidate in this place with nothing beside it, all the way up to the top,
/// and reads the result as `T`. So each entry is tried on its own, and a check costs the size
/// of what is checked rather than of the store: a list with one bad item among thousands is
/// salvaged in about the time it takes to read it. That keeps the same entries as trying each
/// one beside the ones kept before it, because the model's structs read each field on its own
/// (`#[serde(default)]`) and its maps and lists each entry on its own. Where that isn't so (a
/// list of fixed length, `[u32; 2]`), what was kept is checked as a whole once more, and if it
/// doesn't fit, the entries are tried again beside the ones kept before them.
fn salvage(value: &Json, fits: &dyn Fn(Json) -> bool, at: &Place, problems: &mut Vec<Problem>) -> Option<Json> {
    if fits(value.clone()) {
        return Some(value.clone());
    }
    // If nothing in it can be saved, the caller reports the whole of it, not each part.
    let before = problems.len();
    let mut salvaged = keep_what_fits(value, fits, at, problems, false);
    if salvaged.is_none() && matches!(value, Json::Object(_) | Json::Array(_)) {
        problems.truncate(before);
        salvaged = keep_what_fits(value, fits, at, problems, true);
    }
    if salvaged.is_none() {
        problems.truncate(before);
    }
    salvaged
}

/// One pass of [`salvage`] over the entries of an object or a list, each tried alone or
/// (`beside_kept`) beside the ones kept before it. `None` for anything else, or when what was
/// kept doesn't fit as a whole.
fn keep_what_fits(value: &Json, fits: &dyn Fn(Json) -> bool, at: &Place, problems: &mut Vec<Problem>, beside_kept: bool) -> Option<Json> {
    let out = match value {
        Json::Object(map) => {
            let mut kept = JsonMap::new();
            for (key, item) in map {
                let path = at.key(key);
                let in_place = |candidate: Json| {
                    let mut trial = if beside_kept { kept.clone() } else { JsonMap::new() };
                    trial.insert(key.clone(), candidate);
                    fits(Json::Object(trial))
                };
                match salvage(item, &in_place, &path, problems) {
                    Some(good) => {
                        kept.insert(key.clone(), good);
                    }
                    None => problems.push(path.problem(item)),
                }
            }
            Json::Object(kept)
        }
        Json::Array(items) => {
            let mut kept = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let path = at.item(i);
                let in_place = |candidate: Json| {
                    let mut trial = if beside_kept { kept.clone() } else { Vec::new() };
                    trial.push(candidate);
                    fits(Json::Array(trial))
                };
                match salvage(item, &in_place, &path, problems) {
                    Some(good) => kept.push(good),
                    None => problems.push(path.problem(item)),
                }
            }
            Json::Array(kept)
        }
        _ => return None,
    };
    fits(out.clone()).then_some(out)
}

/// A choice 0.10 saved as a string. A value this version doesn't know (from a newer
/// version, or written by hand) is kept as `Other`, so it goes back out as it came in.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident = $text:literal,)+
        }
        default $default:ident
    ) => {
        $crate::model::json::string_enum! { $(#[$meta])* pub enum $name { $($(#[$vmeta])* $variant = $text,)+ } }

        impl Default for $name {
            fn default() -> Self {
                Self::$default
            }
        }
    };
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident = $text:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
            /// A value this version doesn't know, kept as it was written.
            Other(String),
        }

        impl $name {
            /// As 0.10 writes it.
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $text,)+
                    Self::Other(s) => s,
                }
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                match s {
                    $($text => Self::$variant,)+
                    other => Self::Other(other.to_owned()),
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                Ok(Self::from(s.as_str()))
            }
        }
    };
}

pub(crate) use string_enum;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize, PartialEq)]
    #[serde(default)]
    struct Pair {
        name: String,
        size: u32,
        tags: Vec<String>,
        sub: IndexMap<String, Pair>,
    }

    #[test]
    fn keeps_key_order_and_writes_numbers_like_javascript() {
        let text = r#"{"z":1,"a":{"y":2.5,"b":[1,2]},"m":null,"n":-3}"#;
        let json = Json::parse(text).unwrap();
        assert_eq!(json.to_text(), text);
        assert_eq!(Json::of(&serde_json::json!(13.0)).to_text(), "13");
        assert_eq!(Json::of(&serde_json::json!(-2.0)).to_text(), "-2");
        assert_eq!(Json::of(&serde_json::json!(1.22)).to_text(), "1.22");
        assert_eq!(Json::of(&f64::NAN).to_text(), "null");
    }

    #[test]
    fn leaves_out_only_what_does_not_fit() {
        let json = Json::parse(r#"{"name":"web-7731","size":"big","tags":["a",5,"b"],"sub":{"x":{"size":2},"y":{"size":-1}},"more":1}"#).unwrap();
        let mut problems = vec![];
        let got: Pair = lenient(&json, &mut problems).unwrap();
        assert_eq!(got.name, "web-7731");
        assert_eq!(got.size, 0);
        assert_eq!(got.tags, ["a", "b"]);
        assert_eq!(got.sub.len(), 2);
        assert_eq!(got.sub["x"].size, 2);
        let at: Vec<_> = problems.iter().map(|p| p.at.as_str()).collect();
        assert_eq!(at, ["size", "tags[1]", "sub.y.size"]);
        assert_eq!(problems[0].found, "text");
    }

    #[test]
    fn a_part_that_cannot_be_saved_is_reported_once() {
        #[derive(Debug, Default, Deserialize)]
        #[serde(default)]
        struct Lines {
            lines: Option<[u32; 2]>,
        }
        let mut problems = vec![];
        let got: Lines = lenient(&Json::parse(r#"{"lines":[3,"x"]}"#).unwrap(), &mut problems).unwrap();
        assert_eq!(got.lines, None);
        assert_eq!(problems.len(), 1);
        assert_eq!((problems[0].at.as_str(), problems[0].found), ("lines", "a list"));
    }

    #[test]
    fn hides_server_ids_in_the_place_it_shows() {
        let place = Place::root().key("looks").key("cfg:web.example.org").key("color");
        assert_eq!(place.at, "looks.cfg:web.example.org.color");
        assert_eq!(place.shown, "looks.….color");
        assert_eq!(Place::root().key("byHost").key("saved:k7q2").item(0).key("forward").shown, "byHost.…[0].forward");
        assert_eq!(Place::root().key("panes").key("p_x1").key("kind").shown, "panes.p_x1.kind");
        assert_eq!(Place::root().key("snippets").item(3).shown, "snippets[3]");
    }

    #[test]
    fn half_a_character_is_mended_and_its_place_noted() {
        let text = r#"{"state":{"looks":{"cfg:web-7731":{"name":"x\ud83c"}}},"version":2}"#;
        let mut problems = vec![];
        let json = Json::parse_noting(text, &mut problems).unwrap();
        assert_eq!(json.to_text(), r#"{"state":{"looks":{"cfg:web-7731":{"name":"x�"}}},"version":2}"#);
        let problem = problems.remove(0).inside("state");
        assert_eq!((problem.at.as_str(), problem.shown_at(), problem.fix), ("looks.cfg:web-7731.name", "looks.….name", Fix::HalfCharacter));
        assert_eq!(problem.to_string(), "looks.….name: half a character in the text, replaced with \u{fffd}");
        // Anything else wrong beside it still makes the text unreadable.
        assert!(Json::parse(r#"{"a":"x\ud83c","#).is_err());
        assert!(Json::parse(r#"{"a":"x\ud83c"} x"#).is_err());
        // A place outside the key stays as it was.
        assert_eq!(Place::root().key("version").problem(&Json::Null).inside("state").at, "version");
        assert_eq!(Place::root().key("state").item(2).problem(&Json::Null).inside("state").at, "[2]");
    }

    #[test]
    fn gives_up_only_when_nothing_fits() {
        let mut problems = vec![];
        assert!(lenient::<Pair>(&Json::from("not an object"), &mut problems).is_none());
    }
}
