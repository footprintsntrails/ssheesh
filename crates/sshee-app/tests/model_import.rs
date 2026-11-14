//! The import's rules: a broken store never takes the others with it, no tmux session is
//! lost, what's in doubt turns privacy on and says why, a newer store is kept as is, and
//! fields this version doesn't know go back out as they came in.

mod model_support;

use model_support::*;
use sshee_app::import::{import, ImportOptions, Outcome};
use sshee_app::model::{Bookmarks, Json, Labels, PaneKind, Platform, SavedTunnels, Settings, Snippets, Store, TypingCounts, Workspace};
use sshee_app::raw::{RawSource, RawWebState, STORE_KEYS};
use std::collections::BTreeMap;

/// The day-of-use stores, to change one thing in.
fn in_use() -> Json {
    get(&fixture("ts-in-use.json"), &["stores"]).clone()
}

fn without(mut stores: Json, keys: &[&str]) -> Json {
    let Json::Object(map) = &mut stores else { unreachable!() };
    for key in keys {
        map.shift_remove(*key);
    }
    stores
}

fn raw_with(stores: &Json, key: &str, text: &str) -> RawWebState {
    let mut raw = raw_from(stores);
    raw.stores.insert(key.to_owned(), text.to_owned());
    raw
}

fn linux() -> ImportOptions {
    ImportOptions { platform: Platform::Linux, tor_on: false }
}

#[test]
fn no_session_is_lost_from_a_workspace_with_several_screens_tabs_and_splits() {
    let stores = in_use();
    let raw_workspace = get(&stores, &[Workspace::KEY]);
    let before = session_keys(raw_workspace);
    // Four screens, splits, a tab beside another machine's, a hidden one, one in a container.
    assert_eq!(before.len(), 8);
    let imported = import_on(&stores, Platform::Linux);
    assert!(imported.report.lost_sessions.is_empty());
    let sessions = &imported.report.sessions;
    assert_eq!(sessions.len(), before.len());
    // Each session is on the machine its pane was on, and only the local one ended with 0.10.
    let panes = get(raw_workspace, &["state", "panes"]).as_object().unwrap();
    for s in sessions {
        let pane = &panes[&s.pane];
        assert_eq!(get(pane, &["sessionKey"]).as_str(), Some(s.session_key.as_str()));
        let target = get(pane, &["target"]);
        let key = match get(target, &["kind"]).as_str() {
            Some("local") => "local".to_owned(),
            _ => format!("host:{}", get(target, &["hostId"]).as_str().unwrap()),
        };
        assert_eq!(s.machine.as_deref(), Some(key.as_str()));
        assert_eq!(s.durable, key != "local", "{key}");
    }
    assert_eq!(sessions.iter().filter(|s| s.durable).count(), 7);
    // The guest of db-main on web-7731's screen is still db-main's.
    let w = &imported.state.workspace;
    let web_screen = w.layouts["host:cfg:web-7731"].pane_ids();
    let guests: Vec<_> = web_screen.iter().filter(|id| w.panes[**id].target.as_ref().unwrap().key() != "host:cfg:web-7731").collect();
    assert_eq!(guests.len(), 1);
    // And every one goes back out.
    let back = Json::parse(&imported.state.to_web_stores()[Workspace::KEY]).unwrap();
    assert_eq!(session_keys(&back), before);
}

#[test]
fn a_broken_pane_keeps_its_session_and_its_machine() {
    let mut stores = in_use();
    let panes = get_mut(&mut stores, &[Workspace::KEY, "state", "panes"]);
    let (id, _) = panes.iter().find(|(_, p)| get(p, &["kind"]).as_str() == Some("terminal")).map(|(k, v)| (k.clone(), v.clone())).unwrap();
    let Json::Object(pane) = panes.get_mut(&id).unwrap() else { unreachable!() };
    pane.insert("kind".into(), Json::from(5));
    pane.insert("n".into(), Json::from("two"));
    let imported = import_on(&stores, Platform::Linux);
    let report = imported.report.store(Workspace::KEY).unwrap();
    let at: Vec<String> = report.problems.iter().map(|p| p.at.clone()).collect();
    assert_eq!(at, [format!("panes.{id}.kind"), format!("panes.{id}.n")]);
    let pane = &imported.state.workspace.panes[&id];
    assert!(pane.session_key.is_some());
    assert!(pane.target.is_some());
    assert_eq!(pane.kind, PaneKind::Empty);
    assert!(imported.report.lost_sessions.is_empty());
    // A broken pane isn't a privacy matter: settings were there.
    assert_eq!(imported.report.privacy, None);
}

#[test]
fn one_broken_store_leaves_the_others_alone_and_turns_privacy_on() {
    let mut stores = in_use();
    // A day of use with streamer mode off, so turning it on shows.
    get_mut(&mut stores, &[Settings::KEY, "state"]).insert("streamerMode".into(), Json::Bool(false));
    let raw = raw_with(&stores, Labels::KEY, r#"{"state":{"env":{"cfg:db-main""#);
    let imported = import(&raw, &linux());
    let labels = imported.report.store(Labels::KEY).unwrap();
    assert_eq!(labels.outcome, Outcome::Unreadable { why: "not JSON".into() });
    assert_eq!(imported.state.labels, Labels::default());
    for key in STORE_KEYS.iter().filter(|k| **k != Labels::KEY) {
        assert!(matches!(imported.report.store(key).unwrap().outcome, Outcome::Read { .. }), "{key}");
    }
    assert_eq!(imported.state.workspace.sessions().len(), 8);
    let privacy = imported.report.privacy.as_ref().unwrap();
    assert_eq!(privacy.reasons, ["sshee-labels couldn't be read (not JSON)"]);
    assert_eq!(privacy.turned_on, ["streamerMode"]);
    assert!(imported.state.settings.streamer_mode && imported.state.settings.hide_secrets);
}

#[test]
fn stores_that_are_json_but_not_a_store_are_unreadable() {
    let stores = in_use();
    for (text, why) in [
        ("null", "null instead of an object"),
        (r#"{"version":1}"#, "no state"),
        (r#"{"state":[1,2],"version":1}"#, "its state is a list"),
        (r#""text""#, "text instead of an object"),
    ] {
        let imported = import(&raw_with(&stores, Snippets::KEY, text), &linux());
        assert_eq!(imported.report.store(Snippets::KEY).unwrap().outcome, Outcome::Unreadable { why: why.into() });
        assert_eq!(imported.state.snippets, Snippets::default());
        assert!(imported.report.privacy.is_some());
    }
}

#[test]
fn without_settings_privacy_starts_on_and_says_why() {
    let stores = without(in_use(), &[Settings::KEY]);
    let imported = import(&raw_from(&stores), &ImportOptions { platform: Platform::Linux, tor_on: true });
    let s = &imported.state.settings;
    assert!(s.streamer_mode && s.hide_secrets && s.tor_hide_typing);
    let privacy = imported.report.privacy.as_ref().unwrap();
    assert_eq!(privacy.reasons, ["no settings were found"]);
    // Secrets are hidden by default anyway: only what was off is listed.
    assert_eq!(privacy.turned_on, ["streamerMode", "torHideTyping"]);
    assert_eq!(imported.report.store(Settings::KEY).unwrap().outcome, Outcome::Missing);
}

#[test]
fn typing_rhythm_protection_goes_on_only_with_tor_left_on() {
    let stores = without(in_use(), &[Settings::KEY]);
    let imported = import(&raw_from(&stores), &linux());
    assert!(!imported.state.settings.tor_hide_typing);
    assert_eq!(imported.report.privacy.unwrap().turned_on, ["streamerMode"]);
}

#[test]
fn without_tabs_privacy_starts_on() {
    // 0.10's main window saves its tabs every time it starts: none means something's missing.
    let stores = without(in_use(), &[Workspace::KEY]);
    let imported = import_on(&stores, Platform::Linux);
    assert_eq!(imported.report.privacy.unwrap().reasons, ["no tabs were found"]);
    assert!(imported.state.workspace.home);
}

#[test]
fn nothing_at_all_comes_over_as_defaults_and_private() {
    let raw = RawWebState { stores: BTreeMap::new(), window_state: None, source: RawSource::Fixture };
    let imported = import(&raw, &linux());
    assert!(imported.report.stores.iter().all(|s| s.outcome == Outcome::Missing));
    assert_eq!(imported.state.snippets, Snippets::default());
    let privacy = imported.report.privacy.unwrap();
    assert_eq!(privacy.reasons, ["no settings were found", "no tabs were found"]);
    assert!(imported.state.settings.streamer_mode);
}

#[test]
fn stores_0_10_never_wrote_are_not_a_doubt() {
    // 0.10 saves a store only after its first change: most people never had typing counts.
    let stores = without(in_use(), &[Labels::KEY, Snippets::KEY, Bookmarks::KEY, SavedTunnels::KEY, TypingCounts::KEY]);
    let imported = import_on(&stores, Platform::Linux);
    assert_eq!(imported.report.privacy, None);
    assert_eq!(imported.state.snippets.snippets.len(), 5);
    assert_eq!(imported.report.store(TypingCounts::KEY).unwrap().outcome, Outcome::Missing);
}

#[test]
fn a_broken_privacy_setting_turns_privacy_on() {
    for field in ["streamerMode", "hideSecrets", "torHideTyping"] {
        let mut stores = in_use();
        get_mut(&mut stores, &[Settings::KEY, "state"]).insert(field.into(), Json::from("yes"));
        let imported = import(&raw_from(&stores), &ImportOptions { platform: Platform::Linux, tor_on: true });
        let privacy = imported.report.privacy.unwrap_or_else(|| panic!("{field}"));
        assert_eq!(privacy.reasons, ["a privacy setting couldn't be read"], "{field}");
        let s = &imported.state.settings;
        assert!(s.streamer_mode && s.hide_secrets && s.tor_hide_typing, "{field}");
    }
}

#[test]
fn any_other_broken_setting_falls_back_alone() {
    let mut stores = in_use();
    let state = get_mut(&mut stores, &[Settings::KEY, "state"]);
    state.insert("uiZoom".into(), Json::from("big"));
    state.insert("highlightRules".into(), Json::parse(r#"[{"id":"a1","pattern":"oops","color":"cyan"},7]"#).unwrap());
    let imported = import_on(&stores, Platform::Linux);
    let s = &imported.state.settings;
    assert_eq!(s.ui_zoom, 100.0);
    assert_eq!(s.highlight_rules.len(), 1);
    assert_eq!(s.theme, "custom-r8t2w6yz");
    let report = imported.report.store(Settings::KEY).unwrap();
    let at: Vec<&str> = report.problems.iter().map(|p| p.at.as_str()).collect();
    assert_eq!(at, ["uiZoom", "highlightRules[1]"]);
    assert_eq!(imported.report.privacy, None);
}

#[test]
fn a_store_from_a_newer_version_is_kept_as_is() {
    let mut stores = in_use();
    let settings = get_mut(&mut stores, &[Settings::KEY]);
    settings.insert("version".into(), Json::from(9));
    let Json::Object(state) = settings.get_mut("state").unwrap() else { unreachable!() };
    state.insert("paneShadows".into(), Json::from("soft"));
    let raw = raw_from(&stores);
    let imported = import(&raw, &linux());
    assert_eq!(imported.report.store(Settings::KEY).unwrap().outcome, Outcome::Newer { version: 9 });
    // Read as well as it could be...
    assert_eq!(imported.state.settings.theme, "custom-r8t2w6yz");
    // ...and written back exactly as found.
    assert_eq!(imported.state.to_web_stores()[Settings::KEY], raw.stores[Settings::KEY]);
    assert_eq!(imported.report.privacy.unwrap().reasons, ["sshee-settings was saved by a newer version (9)"]);
}

#[test]
fn a_newer_workspace_keeps_every_session() {
    let mut stores = in_use();
    get_mut(&mut stores, &[Workspace::KEY]).insert("version".into(), Json::from(3));
    let imported = import_on(&stores, Platform::Linux);
    assert_eq!(imported.report.sessions.len(), 8);
    assert!(imported.report.lost_sessions.is_empty());
}

#[test]
fn versions_no_release_wrote_are_read_and_doubted() {
    let mut stores = in_use();
    get_mut(&mut stores, &[Snippets::KEY]).insert("version".into(), Json::from(0));
    get_mut(&mut stores, &[Bookmarks::KEY]).shift_remove("version");
    let imported = import_on(&stores, Platform::Linux);
    assert_eq!(imported.report.store(Snippets::KEY).unwrap().outcome, Outcome::Migrated { from: 0, to: 1 });
    assert_eq!(imported.state.snippets.snippets.len(), 5);
    assert_eq!(imported.report.store(Bookmarks::KEY).unwrap().outcome, Outcome::Read { version: 1 });
    assert_eq!(imported.state.bookmarks.pins["host:cfg:web-7731"].len(), 2);
    assert_eq!(
        imported.report.privacy.unwrap().reasons,
        ["sshee-snippets has version 0, which no release wrote", "sshee-bookmarks has no version number"]
    );
}

#[test]
fn fields_this_version_does_not_know_go_back_out_as_they_came_in() {
    let mut stores = in_use();
    let extra = || Json::parse(r#"{"from":"a later 0.10","n":[1,2.5]}"#).unwrap();
    let mut add = |path: &[&str]| {
        get_mut(&mut stores, path).insert("later".into(), extra());
    };
    add(&[Settings::KEY]);
    add(&[Settings::KEY, "state"]);
    add(&[Labels::KEY, "state"]);
    add(&[Labels::KEY, "state", "looks", "cfg:web-7731"]);
    add(&[Labels::KEY, "state", "autoLooks", "cfg:db-main"]);
    add(&[Workspace::KEY, "state"]);
    add(&[Bookmarks::KEY, "state"]);
    add(&[SavedTunnels::KEY, "state"]);
    add(&[TypingCounts::KEY, "state"]);
    add(&[TypingCounts::KEY, "state", "saved", "direct", "line"]);
    let workspace = get_mut(&mut stores, &[Workspace::KEY, "state"]);
    let Json::Object(panes) = workspace.get_mut("panes").unwrap() else { unreachable!() };
    for pane in panes.values_mut() {
        let Json::Object(pane) = pane else { unreachable!() };
        pane.insert("later".into(), extra());
        if let Some(Json::Object(target)) = pane.get_mut("target") {
            target.insert("via".into(), Json::from("jump-01"));
        }
        if let Some(Json::Object(container)) = pane.get_mut("container") {
            container.insert("later".into(), extra());
        }
    }
    let Json::Object(layouts) = workspace.get_mut("layouts").unwrap() else { unreachable!() };
    for node in layouts.values_mut() {
        let Json::Object(node) = node else { unreachable!() };
        node.insert("size".into(), Json::from(50));
    }
    let Json::Array(closed) = workspace.get_mut("closedTabs").unwrap() else { unreachable!() };
    let Json::Object(tab) = &mut closed[0] else { unreachable!() };
    tab.insert("later".into(), extra());
    let settings = get_mut(&mut stores, &[Settings::KEY, "state"]);
    let Json::Array(themes) = settings.get_mut("customThemes").unwrap() else { unreachable!() };
    let Json::Object(theme) = &mut themes[0] else { unreachable!() };
    theme.insert("later".into(), extra());
    let Json::Object(shortcuts) = settings.get_mut("shortcuts").unwrap() else { unreachable!() };
    let Json::Object(keys) = shortcuts.get_mut("splitRight").unwrap() else { unreachable!() };
    keys.insert("later".into(), extra());
    let Json::Array(rules) = settings.get_mut("highlightRules").unwrap() else { unreachable!() };
    let Json::Object(rule) = &mut rules[0] else { unreachable!() };
    rule.insert("later".into(), extra());
    let Json::Object(colors) = settings.get_mut("terminalColors").unwrap() else { unreachable!() };
    colors.insert("later".into(), extra());
    let snippets = get_mut(&mut stores, &[Snippets::KEY, "state"]);
    let Json::Array(list) = snippets.get_mut("snippets").unwrap() else { unreachable!() };
    let Json::Object(snippet) = &mut list[0] else { unreachable!() };
    snippet.insert("later".into(), extra());
    let tunnels = get_mut(&mut stores, &[SavedTunnels::KEY, "state", "byHost"]);
    let Json::Array(list) = tunnels.get_mut("cfg:web-7731").unwrap() else { unreachable!() };
    let Json::Object(tunnel) = &mut list[0] else { unreachable!() };
    tunnel.insert("later".into(), extra());
    let Json::Object(forward) = tunnel.get_mut("forward").unwrap() else { unreachable!() };
    forward.insert("later".into(), extra());

    let imported = import_on(&stores, Platform::Linux);
    assert!(imported.report.stores.iter().all(|s| s.problems.is_empty()));
    let back_text = imported.state.to_web_stores();
    let back = written(&back_text);
    for key in STORE_KEYS {
        let before = get(&stores, &[key]);
        // The envelope's own extra field is zustand's to drop: it writes `{state, version}`.
        let mut expected = before.clone();
        if let Json::Object(env) = &mut expected {
            env.shift_remove("later");
        }
        assert_eq!(back[key], expected, "{key}");
        // In the same order too (`==` on `Json` doesn't see it): a field this version
        // doesn't know goes back where it was, and the tab strip keeps its order. A snippet's
        // fields are in either of two orders in 0.10 (`a_day_of_use_goes_round_trip_unchanged`).
        if key != Snippets::KEY {
            assert_eq!(back_text[key], expected.to_text(), "{key}");
        }
    }
}

#[test]
fn choices_this_version_does_not_know_are_kept() {
    let mut stores = in_use();
    get_mut(&mut stores, &[Settings::KEY, "state"]).insert("logo".into(), Json::from("dancing"));
    let panes = get_mut(&mut stores, &[Workspace::KEY, "state", "panes"]);
    let Json::Object(pane) = panes.values_mut().next().unwrap() else { unreachable!() };
    pane.insert("kind".into(), Json::from("browser"));
    let imported = import_on(&stores, Platform::Linux);
    assert_eq!(imported.state.settings.logo.as_str(), "dancing");
    assert!(imported.state.workspace.panes.values().any(|p| p.kind == PaneKind::Other("browser".into())));
    let back = imported.state.to_web_stores();
    assert_eq!(back[Settings::KEY], get(&stores, &[Settings::KEY]).to_text());
    assert_eq!(back[Workspace::KEY], get(&stores, &[Workspace::KEY]).to_text());
}

#[test]
fn the_report_counts_and_never_shows_names_or_secrets() {
    let mut stores = in_use();
    // A broken look under a server's id: the problem's place must not give the server away.
    let looks = get_mut(&mut stores, &[Labels::KEY, "state", "looks"]);
    let Json::Object(look) = looks.get_mut("cfg:web-7731").unwrap() else { unreachable!() };
    look.insert("color".into(), Json::from(7));
    let imported = import_on(&stores, Platform::Linux);
    let text = imported.report.to_string();
    for secret in ["sk-test-not-real", "web-7731", "db-main", "k7q2m9xa", "203.0.113", "/var/www", "Grafana", "Be gentle"] {
        assert!(!text.contains(secret), "{secret} in:\n{text}");
    }
    // Seven on servers. The one on this computer had no tmux and ended with 0.10.
    assert!(
        text.contains("sshee-workspace: read (version 2). 9 tabs on 4 screens, 7 terminals with a tmux session, 1 local terminal (gone with 0.10), 2 closed tabs"),
        "{text}"
    );
    assert!(text.contains("sshee-snippets: read (version 1). 5 snippets"), "{text}");
    assert!(text.contains("sshee-tunnels: read (version 1). 3 tunnels on 2 servers, 1 switched on"), "{text}");
    assert!(text.contains("looks.….color: unreadable (a number), left out"), "{text}");
    assert!(text.contains("Privacy settings came over as they were."), "{text}");
}

#[test]
fn other_keys_are_left_alone_and_listed() {
    let mut raw = raw_from(&in_use());
    raw.stores.insert("sshee-something-else".into(), "{}".into());
    let imported = import(&raw, &linux());
    assert_eq!(imported.report.ignored, ["sshee-something-else"]);
    assert_eq!(imported.report.privacy, None);
}

#[test]
fn a_store_that_cannot_be_read_goes_back_to_0_10_as_it_was() {
    let cut = r#"{"state":{"env":{"cfg:db-main":"prod"},"looks":{"#;
    let raw = raw_with(&in_use(), Labels::KEY, cut);
    let imported = import(&raw, &linux());
    assert_eq!(imported.report.store(Labels::KEY).unwrap().outcome, Outcome::Unreadable { why: "not JSON".into() });
    assert_eq!(imported.state.labels, Labels::default());
    // 0.10 reads its own value or fails on it as before: never defaults written over it.
    assert_eq!(imported.state.to_web_stores()[Labels::KEY], cut);
}

#[test]
fn a_missing_privacy_setting_turns_privacy_on() {
    // 0.10 saves the whole settings store every time: since version 2 every save has
    // `streamerMode`, since version 3 `hideSecrets`. `torHideTyping` came in 0.10.4 at
    // version 4, so stores from 0.8.0 to 0.10.3 are without it for good reason.
    for (field, version, doubt) in [
        ("streamerMode", 4, true),
        ("streamerMode", 3, true),
        ("streamerMode", 2, true),
        ("streamerMode", 1, false),
        ("hideSecrets", 4, true),
        ("hideSecrets", 3, true),
        ("hideSecrets", 2, false),
        ("torHideTyping", 4, false),
    ] {
        let mut stores = in_use();
        let settings = get_mut(&mut stores, &[Settings::KEY]);
        settings.insert("version".into(), Json::from(version));
        let Json::Object(state) = settings.get_mut("state").unwrap() else { unreachable!() };
        // Off where it was, so turning it back on shows.
        state.insert("streamerMode".into(), Json::Bool(false));
        state.shift_remove(field);
        let imported = import(&raw_from(&stores), &ImportOptions { platform: Platform::Linux, tor_on: true });
        let report = imported.report.store(Settings::KEY).unwrap();
        assert!(report.problems.is_empty(), "{field} at {version}: {:?}", report.problems);
        match doubt {
            true => {
                let privacy = imported.report.privacy.unwrap_or_else(|| panic!("{field} at {version}"));
                assert_eq!(privacy.reasons, ["a privacy setting was missing"], "{field} at {version}");
                let s = &imported.state.settings;
                assert!(s.streamer_mode && s.hide_secrets && s.tor_hide_typing, "{field} at {version}");
            }
            false => assert_eq!(imported.report.privacy, None, "{field} at {version}"),
        }
    }
}

#[test]
fn counts_too_big_to_add_up_do_not_break_the_import() {
    // JavaScript can save a count of 1e19; two of them overflow a u64.
    let mut stores = in_use();
    let counts = r#"{"state":{"saved":{"direct":{"line":{"keys":18446744073709551615}},"tor":{"line":{"keys":1}}}},"version":0}"#;
    get_mut(&mut stores, &[]).insert(TypingCounts::KEY.into(), Json::parse(counts).unwrap());
    let imported = import_on(&stores, Platform::Linux);
    let report = imported.report.store(TypingCounts::KEY).unwrap();
    assert_eq!(report.summary, "18446744073709551615 keys counted");
    assert_eq!(imported.state.to_web_stores()[TypingCounts::KEY], counts);
}
