//! Shared by the `model_*` tests: the fixtures `fixtures/gen/model_fixtures.gen.ts` made
//! from 0.10's own stores, turned into what the web view would hold.

#![allow(dead_code)]

use sshee_app::import::{import, ImportOptions, Imported};
use sshee_app::model::{Json, JsonMap, Platform};
use sshee_app::raw::{RawSource, RawWebState};
use std::collections::BTreeMap;
use std::path::Path;

/// A fixture file, parsed with its key order.
pub fn fixture(name: &str) -> Json {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    Json::parse(&text).unwrap()
}

/// `{key: {"state":…,"version":N}}` as localStorage holds it: each value as the compact
/// text zustand wrote.
pub fn raw_from(stores: &Json) -> RawWebState {
    let stores = stores.as_object().expect("stores are an object");
    RawWebState {
        stores: stores.iter().map(|(k, v)| (k.clone(), v.to_text())).collect(),
        window_state: None,
        source: RawSource::Fixture,
    }
}

pub fn platform(name: &str) -> Platform {
    match name {
        "linux" => Platform::Linux,
        "mac" => Platform::Mac,
        "windows" => Platform::Windows,
        other => panic!("no platform {other}"),
    }
}

pub fn import_on(stores: &Json, platform: Platform) -> Imported {
    import(&raw_from(stores), &ImportOptions { platform, tor_on: false })
}

/// A field of an object, by path: `get(&json, &["stores", "sshee-settings"])`.
pub fn get<'a>(json: &'a Json, path: &[&str]) -> &'a Json {
    path.iter().fold(json, |at, key| at.as_object().and_then(|o| o.get(*key)).unwrap_or_else(|| panic!("no {key} in {path:?}")))
}

pub fn get_mut<'a>(json: &'a mut Json, path: &[&str]) -> &'a mut JsonMap {
    let mut at = json;
    for key in path {
        let Json::Object(map) = at else { panic!("{key} in {path:?} isn't in an object") };
        at = map.get_mut(*key).unwrap_or_else(|| panic!("no {key} in {path:?}"));
    }
    match at {
        Json::Object(map) => map,
        _ => panic!("{path:?} isn't an object"),
    }
}

/// The written-back stores, parsed.
pub fn written(stores: &BTreeMap<String, String>) -> BTreeMap<String, Json> {
    stores.iter().map(|(k, v)| (k.clone(), Json::parse(v).unwrap())).collect()
}

/// Every `sessionKey` in a JSON value, in order.
pub fn session_keys(json: &Json) -> Vec<String> {
    let mut out = vec![];
    json.strings_under("sessionKey", &mut out);
    out
}
