//! Stores 0.10 can leave that are hard to read: half a character JavaScript is happy with,
//! a workspace cut short or damaged, a newer workspace this version reads only in part, and
//! stores big or deep enough to make a careless reader take minutes or crash.

mod model_support;

use model_support::*;
use sshee_app::import::{import, ImportOptions, Outcome};
use sshee_app::model::{Fix, Json, Platform, Settings, Store, Workspace};
use std::time::{Duration, Instant};

fn linux() -> ImportOptions {
    ImportOptions { platform: Platform::Linux, tor_on: false }
}

fn in_use() -> Json {
    get(&fixture("ts-in-use.json"), &["stores"]).clone()
}

/// `JSON.stringify(text.slice(0, units))`: JavaScript cuts by UTF-16 units, and writes a half
/// of a surrogate pair that's left as an escape (since ES2019).
fn js_cut(text: &str, units: usize) -> String {
    let cut: Vec<u16> = text.encode_utf16().take(units).collect();
    let mut out = String::from("\"");
    for c in char::decode_utf16(cut) {
        match c {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok(c) if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            Ok(c) => out.push(c),
            Err(half) => out.push_str(&format!("\\u{:04x}", half.unpaired_surrogate())),
        }
    }
    out + "\""
}

/// A store's text with the string `"@@here@@"` replaced by `json`, written as is.
fn with_text_at(store: &Json, json: &str) -> String {
    let text = store.to_text();
    assert!(text.contains("\"@@here@@\""));
    text.replace("\"@@here@@\"", json)
}

#[test]
fn a_theme_name_cut_in_half_an_emoji_keeps_the_settings() {
    // ThemeEditor.tsx cuts names with `slice(0, 40)`: here between the two halves of 🌙.
    let name = "Night shift for the late on-call rotaaa🌙";
    let cut = js_cut(name, 40);
    assert_eq!(cut, "\"Night shift for the late on-call rotaaa\\ud83c\"");
    let mut stores = in_use();
    let Json::Array(themes) = get_mut(&mut stores, &[Settings::KEY, "state"]).get_mut("customThemes").unwrap() else { unreachable!() };
    let Json::Object(theme) = &mut themes[0] else { unreachable!() };
    theme.insert("name".into(), Json::from("@@here@@"));
    let text = with_text_at(get(&stores, &[Settings::KEY]), &cut);
    // JSON.parse reads this; serde_json alone refuses all of it.
    assert!(serde_json::from_str::<serde_json::Value>(&text).is_err());

    let mut raw = raw_from(&in_use());
    raw.stores.insert(Settings::KEY.into(), text);
    let imported = import(&raw, &linux());
    let report = imported.report.store(Settings::KEY).unwrap();
    assert_eq!(report.outcome, Outcome::Read { version: 4 });
    let s = &imported.state.settings;
    assert_eq!(s.custom_themes[0].name, "Night shift for the late on-call rotaaa\u{fffd}");
    assert_eq!(s.theme, "custom-r8t2w6yz");
    assert_eq!((s.shortcuts.len(), s.highlight_rules.len()), (2, 2));
    assert!(s.streamer_mode && s.hide_secrets);
    // Noted where, never what; and it's no reason for doubt.
    let at: Vec<_> = report.problems.iter().map(|p| (p.at.as_str(), p.fix)).collect();
    assert_eq!(at, [("customThemes[0].name", Fix::HalfCharacter)]);
    let text = imported.report.to_string();
    assert!(text.contains("  customThemes[0].name: half a character in the text, replaced with \u{fffd}\n"), "{text}");
    assert!(!text.contains("Night shift"), "{text}");
    assert_eq!(imported.report.privacy, None);
    // What goes back is what 0.10 showed: the half drawn as U+FFFD.
    let back = written(&imported.state.to_web_stores());
    assert_eq!(get(&back[Settings::KEY], &["state", "customThemes"]).to_text().matches('\u{fffd}').count(), 1);
}

#[test]
fn a_folder_with_half_a_character_keeps_every_tab() {
    let mut stores = in_use();
    let panes = get_mut(&mut stores, &[Workspace::KEY, "state", "panes"]);
    let (id, _) = panes.iter().find(|(_, p)| get(p, &["kind"]).as_str() == Some("files")).unwrap();
    let id = id.clone();
    let Json::Object(pane) = panes.get_mut(&id).unwrap() else { unreachable!() };
    pane.insert("path".into(), Json::from("@@here@@"));
    // A lone second half, and a first half before another escape.
    let text = with_text_at(get(&stores, &[Workspace::KEY]), r#""/srv/\udf19logs/\uD83CA""#);
    let mut raw = raw_from(&in_use());
    raw.stores.insert(Workspace::KEY.into(), text);
    let imported = import(&raw, &linux());
    let report = imported.report.store(Workspace::KEY).unwrap();
    assert_eq!(report.outcome, Outcome::Read { version: 2 });
    assert_eq!(imported.state.workspace.panes[&id].path.as_deref(), Some("/srv/\u{fffd}logs/\u{fffd}A"));
    assert_eq!(report.problems.iter().map(|p| p.at.clone()).collect::<Vec<_>>(), [format!("panes.{id}.path")]);
    assert_eq!(imported.report.sessions.len(), 8);
    assert!(imported.report.lost_sessions.is_empty());
    assert_eq!(imported.report.privacy, None);
}

#[test]
fn a_workspace_cut_short_lists_every_session_it_held() {
    let stores = in_use();
    let text = get(&stores, &[Workspace::KEY]).to_text();
    // Cut inside the sixth `sessionKey`: five whole ones before it.
    let sixth = text.match_indices("\"sessionKey\"").nth(5).unwrap().0;
    let cut = &text[..sixth + 18];
    let mut raw = raw_from(&stores);
    raw.stores.insert(Workspace::KEY.into(), cut.to_owned());
    let imported = import(&raw, &linux());
    assert_eq!(imported.report.store(Workspace::KEY).unwrap().outcome, Outcome::Unreadable { why: "not JSON".into() });
    assert!(imported.report.sessions.is_empty());

    // Each with its pane and its server, for the tmux search.
    let panes = get(get(&stores, &[Workspace::KEY]), &["state", "panes"]).as_object().unwrap();
    let lost = &imported.report.lost_sessions;
    assert_eq!(lost.len(), 5);
    for s in lost {
        let pane = &panes[&s.pane];
        assert_eq!(get(pane, &["sessionKey"]).as_str(), Some(s.session_key.as_str()));
        let host = get(pane, &["target", "hostId"]).as_str().unwrap();
        assert_eq!(s.machine.as_deref(), Some(format!("host:{host}").as_str()));
        assert!(s.durable);
    }
    let privacy = imported.report.privacy.as_ref().unwrap();
    assert_eq!(
        privacy.reasons,
        ["sshee-workspace couldn't be read (not JSON)", "sshee-workspace's terminal sessions couldn't all be checked", "5 terminal session(s) didn't come over"]
    );
    let report = imported.report.to_string();
    assert!(report.contains("Terminal sessions that didn't come over: 5 (5 may still run on a server)"), "{report}");
    // And 0.10 gets back what it had.
    assert_eq!(imported.state.to_web_stores()[Workspace::KEY], cut);
}

#[test]
fn damage_before_the_panes_still_finds_every_session() {
    let stores = in_use();
    let text = get(&stores, &[Workspace::KEY]).to_text();
    let before = session_keys(get(&stores, &[Workspace::KEY]));
    // A flipped byte in `layouts`, before any pane: a parser stops there, the search doesn't.
    let at = text.find("\"layouts\":{").unwrap() + 10;
    let mut damaged = text.clone().into_bytes();
    assert_eq!(damaged[at], b'{');
    damaged[at] = b'#';
    let mut raw = raw_from(&stores);
    raw.stores.insert(Workspace::KEY.into(), String::from_utf8(damaged).unwrap());
    let imported = import(&raw, &linux());
    let lost: Vec<_> = imported.report.lost_sessions.iter().map(|s| s.session_key.clone()).collect();
    assert_eq!(lost, before);
    // Where their panes were couldn't be read: the tmux search looks on every server.
    assert!(imported.report.lost_sessions.iter().all(|s| s.pane.is_empty() && s.machine.is_none() && s.durable));
}

#[test]
fn a_newer_workspace_this_version_reads_in_part_reports_the_sessions_it_left_out() {
    let mut stores = in_use();
    let workspace = get_mut(&mut stores, &[Workspace::KEY]);
    workspace.insert("version".into(), Json::from(3));
    let panes = get_mut(&mut stores, &[Workspace::KEY, "state", "panes"]);
    let (id, pane) = panes.iter().find(|(_, p)| get(p, &["kind"]).as_str() == Some("terminal")).map(|(k, v)| (k.clone(), v.clone())).unwrap();
    // Say a later version keeps a pane as a list of its states.
    panes.insert(id.clone(), Json::Array(vec![pane.clone()]));
    let raw = raw_from(&stores);
    let imported = import(&raw, &linux());
    let report = imported.report.store(Workspace::KEY).unwrap();
    assert_eq!(report.outcome, Outcome::Newer { version: 3 });
    assert_eq!(report.problems.iter().map(|p| p.at.clone()).collect::<Vec<_>>(), [format!("panes.{id}")]);
    // 1.x runs on the typed copy, which hasn't got it: that's a lost session, even though
    // the store itself is kept for 0.10 as it was.
    let lost = &imported.report.lost_sessions;
    assert_eq!(lost.len(), 1);
    assert_eq!((lost[0].pane.as_str(), Some(lost[0].session_key.as_str())), (id.as_str(), get(&pane, &["sessionKey"]).as_str()));
    assert_eq!(imported.report.sessions.len(), 7);
    assert_eq!(imported.report.privacy.unwrap().reasons, ["sshee-workspace was saved by a newer version (3)", "1 terminal session(s) didn't come over"]);
    assert_eq!(imported.state.to_web_stores()[Workspace::KEY], raw.stores[Workspace::KEY]);
}

#[test]
fn a_store_nested_past_reason_is_unreadable_without_a_crash() {
    let deep = "[".repeat(100_000);
    for key in [Settings::KEY, Workspace::KEY] {
        let mut raw = raw_from(&in_use());
        raw.stores.insert(key.into(), deep.clone());
        let imported = import(&raw, &linux());
        assert_eq!(imported.report.store(key).unwrap().outcome, Outcome::Unreadable { why: "not JSON".into() });
    }
}

/// A run that took minutes before salvaging tried each entry on its own.
fn quickly<T>(what: &str, run: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = run();
    // Well under a second even in a debug build; minutes before.
    assert!(start.elapsed() < Duration::from_secs(20), "{what} took {:?}", start.elapsed());
    out
}

#[test]
fn one_bad_item_in_a_long_list_costs_no_more_than_the_list() {
    let mut stores = in_use();
    let mut hints: Vec<Json> = (0..16_384).map(|i| Json::from(format!("hint-{i}").as_str())).collect();
    hints.push(Json::from(7));
    get_mut(&mut stores, &[Settings::KEY, "state"]).insert("hintsSeen".into(), Json::Array(hints));
    let imported = quickly("16k hints and one number", || import_on(&stores, Platform::Linux));
    assert_eq!(imported.state.settings.hints_seen.len(), 16_384);
    let report = imported.report.store(Settings::KEY).unwrap();
    assert_eq!(report.problems.iter().map(|p| p.at.as_str()).collect::<Vec<_>>(), ["hintsSeen[16384]"]);
}

#[test]
fn many_bad_items_are_each_left_out_alone() {
    let mut stores = in_use();
    let rules: Vec<Json> = (0..4_000)
        .map(|i| match i % 2 {
            0 => Json::parse(&format!(r#"{{"id":"r{i}","pattern":"x{i}","color":"red"}}"#)).unwrap(),
            _ => Json::parse(&format!(r#"{{"id":"r{i}","pattern":{i},"color":"red"}}"#)).unwrap(),
        })
        .collect();
    get_mut(&mut stores, &[Settings::KEY, "state"]).insert("highlightRules".into(), Json::Array(rules));
    let imported = quickly("4000 rules, every other one broken", || import_on(&stores, Platform::Linux));
    // A broken rule keeps its other fields: only the pattern falls back.
    assert_eq!(imported.state.settings.highlight_rules.len(), 4_000);
    let report = imported.report.store(Settings::KEY).unwrap();
    assert_eq!(report.problems.len(), 2_000);
    assert_eq!(report.problems[0].at, "highlightRules[1].pattern");
}

#[test]
fn a_0_5_workspace_with_a_thousand_machines_comes_over_quickly() {
    // 0.5's one layout for every machine, one tab on each of 1000 servers.
    let n = 1000;
    let leaves: Vec<String> = (0..n).map(|i| format!(r#"{{"kind":"pane","id":"p{i}"}}"#)).collect();
    let panes: Vec<String> = (0..n)
        .map(|i| format!(r#""p{i}":{{"id":"p{i}","kind":"terminal","target":{{"kind":"host","hostId":"cfg:web-{i}"}},"sessionKey":"k{i}"}}"#))
        .collect();
    let text = format!(
        r#"{{"state":{{"root":{{"kind":"split","id":"s1","dir":"row","children":[{}]}},"panes":{{{}}},"focused":"p0"}},"version":1}}"#,
        leaves.join(","),
        panes.join(",")
    );
    let mut raw = raw_from(&in_use());
    raw.stores.insert(Workspace::KEY.into(), text);
    let imported = quickly("1000 machines from 0.5", || import(&raw, &linux()));
    assert_eq!(imported.report.store(Workspace::KEY).unwrap().outcome, Outcome::Migrated { from: 1, to: 2 });
    let w = &imported.state.workspace;
    assert_eq!(w.layouts.len(), n);
    // The tab strip keeps the order the panes were in.
    assert_eq!(w.layouts.keys().take(3).cloned().collect::<Vec<_>>(), ["host:cfg:web-0", "host:cfg:web-1", "host:cfg:web-2"]);
    assert_eq!(w.layouts["host:cfg:web-7"].pane_ids(), ["p7"]);
    assert_eq!(imported.report.sessions.len(), n);
    assert!(imported.report.lost_sessions.is_empty());
}
