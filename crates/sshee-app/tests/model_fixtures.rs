//! The model against 0.10's own stores: defaults, a workspace in daily use, and stores left
//! by 0.3.5 to 0.7.3 next to what 0.10.14's code made of them. The fixtures come from the TS
//! code itself (`fixtures/gen/model_fixtures.gen.ts`).

mod model_support;

use model_support::*;
use sshee_app::import::Outcome;
use sshee_app::model::{AppState, Json, Platform, Settings, Store, Workspace};
use sshee_app::raw::STORE_KEYS;

#[test]
fn defaults_are_the_ones_0_10_writes_byte_for_byte() {
    let defaults = fixture("ts-defaults.json");
    for (name, stores) in defaults.as_object().unwrap() {
        let ours = AppState::defaults(platform(name)).to_web_stores();
        assert_eq!(ours.len(), STORE_KEYS.len());
        for key in STORE_KEYS {
            assert_eq!(ours[key], get(stores, &[key]).to_text(), "{key} on {name}");
        }
    }
}

#[test]
fn defaults_come_over_as_defaults_with_nothing_in_doubt() {
    let defaults = fixture("ts-defaults.json");
    for (name, stores) in defaults.as_object().unwrap() {
        let imported = import_on(stores, platform(name));
        assert_eq!(imported.state, AppState::defaults(platform(name)), "{name}");
        for store in &imported.report.stores {
            assert!(matches!(store.outcome, Outcome::Read { .. }), "{}: {:?}", store.key, store.outcome);
            assert!(store.problems.is_empty(), "{}: {:?}", store.key, store.problems);
        }
        assert_eq!(imported.report.privacy, None);
    }
}

#[test]
fn gpu_drawing_defaults_follow_the_system_as_in_0_10() {
    assert!(Settings::defaults(Platform::Windows).gpu_rendering);
    assert!(Settings::defaults(Platform::Mac).gpu_rendering);
    assert!(!Settings::defaults(Platform::Linux).gpu_rendering);
    // A store saved without the field gets the default of the system it was saved on.
    let state = Json::parse(r#"{"state":{"theme":"oled"},"version":4}"#).unwrap();
    let stores = Json::Object([(Settings::KEY.to_owned(), state)].into_iter().collect());
    assert!(import_on(&stores, Platform::Mac).state.settings.gpu_rendering);
    assert!(!import_on(&stores, Platform::Linux).state.settings.gpu_rendering);
}

#[test]
fn a_day_of_use_goes_round_trip_unchanged() {
    let fixture = fixture("ts-in-use.json");
    let stores = get(&fixture, &["stores"]);
    let imported = import_on(stores, platform(get(&fixture, &["platform"]).as_str().unwrap()));
    for store in &imported.report.stores {
        assert!(matches!(store.outcome, Outcome::Read { .. }), "{}: {:?}", store.key, store.outcome);
        assert!(store.problems.is_empty(), "{}: {:?}", store.key, store.problems);
    }
    assert_eq!(imported.report.privacy, None);

    let back = imported.state.to_web_stores();
    for key in STORE_KEYS {
        let before = get(stores, &[key]);
        // Same JSON: every key, every value, numbers as JavaScript writes them.
        assert_eq!(&Json::parse(&back[key]).unwrap(), before, "{key}");
        // And byte for byte where 0.10's own key order is fixed. A snippet saved by hand is
        // `{name, command, id}`, a starter `{id, name, command}`: same JSON, other order.
        if key != "sshee-snippets" {
            assert_eq!(back[key], before.to_text(), "{key}");
        }
    }
}

#[test]
fn a_day_of_use_reads_as_it_was_meant() {
    let fixture = fixture("ts-in-use.json");
    let state = import_on(get(&fixture, &["stores"]), Platform::Linux).state;
    let s = &state.settings;
    assert_eq!(s.theme, "custom-r8t2w6yz");
    assert_eq!(s.custom_themes[0].colors["canvas"], "#07080a");
    assert_eq!(s.shortcuts["splitRight"].as_ref().unwrap().shift, Some(true));
    assert_eq!(s.shortcuts["paletteAlt"], None);
    assert_eq!(s.terminal_font_size, 14.5);
    assert_eq!(s.sidebar_width, 251.5);
    assert!(s.streamer_mode && s.hide_secrets && s.tor_hide_typing);
    assert_eq!(s.extra["placesWidth"], Json::from(212));

    let w = &state.workspace;
    // The tab strip shows machines in this order.
    let screens: Vec<&str> = w.layouts.keys().map(String::as_str).collect();
    assert_eq!(screens, ["host:cfg:web-7731", "host:cfg:db-main", "local", "host:saved:k7q2m9xa"]);
    assert_eq!(w.closed_tabs.len(), 2);
    assert!(!w.home);

    assert_eq!(state.snippets.snippets.len(), 5);
    assert_eq!(state.tunnels.by_host["cfg:db-main"][0].forward.remote_port, Some(5432));
    let direct = &state.typing_counts.saved["direct"]["line"];
    assert_eq!(direct.count("keys"), 150);
    assert_eq!(direct.echoes().iter().sum::<u64>(), 7);
}

#[test]
fn older_versions_come_over_as_0_10_14_migrates_them() {
    let upgrades = fixture("ts-upgrades.json");
    let Json::Array(cases) = get(&upgrades, &["cases"]) else { panic!("cases") };
    assert!(cases.len() >= 5);
    for case in cases {
        let from = get(case, &["from"]).as_str().unwrap();
        let on = get(case, &["platform"]).as_str().unwrap();
        let saved = get(case, &["saved"]);
        let migrated = get(case, &["migrated"]);
        let imported = import_on(saved, platform(on));
        let back_text = imported.state.to_web_stores();
        let back = written(&back_text);
        for (key, expected) in migrated.as_object().unwrap() {
            assert_eq!(&back[key], expected, "{key} from {from} on {on}");
            // `Json`'s `==` doesn't see key order, and the order of `layouts` is the order of
            // the tab strip: byte for byte, as 0.10.14 wrote it. Snippets differ only in the
            // order of a snippet's fields (see `a_day_of_use_goes_round_trip_unchanged`).
            if key != "sshee-snippets" {
                assert_eq!(back_text[key], expected.to_text(), "{key} from {from} on {on}");
            }
            let report = imported.report.store(key).unwrap();
            assert!(report.problems.is_empty(), "{key} from {from}: {:?}", report.problems);
            let saved_version = get(saved, &[key, "version"]).as_u64().unwrap();
            let outcome = &report.outcome;
            match saved_version == get(expected, &["version"]).as_u64().unwrap() {
                true => assert!(matches!(outcome, Outcome::Read { .. }), "{key} from {from}: {outcome:?}"),
                false => assert_eq!(*outcome, Outcome::Migrated { from: saved_version, to: get(expected, &["version"]).as_u64().unwrap() }),
            }
        }
        // Settings and tabs were there: nothing to doubt, nothing turned on.
        assert_eq!(imported.report.privacy, None, "from {from}");
        // Every terminal still has its tmux session.
        let before = session_keys(get(saved, &[Workspace::KEY]));
        assert!(!before.is_empty());
        let after = session_keys(&back[Workspace::KEY]);
        for key in &before {
            assert!(after.contains(key), "{key} from {from}");
        }
        assert!(imported.report.lost_sessions.is_empty());
    }
}

#[test]
fn settings_written_back_after_a_change_have_the_0_10_shape() {
    let mut state = AppState::defaults(Platform::Linux);
    state.settings.ui_zoom = 125.0;
    state.settings.terminal_line_height = 1.4;
    state.settings.streamer_mode = true;
    let back = Json::parse(&state.to_web_stores()[Settings::KEY]).unwrap();
    assert_eq!(get(&back, &["version"]), &Json::from(4));
    assert_eq!(get(&back, &["state", "uiZoom"]).to_text(), "125");
    assert_eq!(get(&back, &["state", "terminalLineHeight"]).to_text(), "1.4");
    assert_eq!(get(&back, &["state", "streamerMode"]), &Json::Bool(true));
    assert_eq!(get(&back, &["state", "terminalColors"]), &Json::Null);
}
