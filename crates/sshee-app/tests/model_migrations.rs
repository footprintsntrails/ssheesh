//! 0.10.14's migrations, case by case: `settings.test.ts` and `store.test.ts` ("from 0.5's
//! single layout") brought over one to one, plus the labels step and the cases where this
//! port keeps more than 0.10 would.

use sshee_app::model::{Json, JsonMap, Labels, Platform, Settings, Store, Workspace};

fn map(text: &str) -> JsonMap {
    match Json::parse(text).unwrap() {
        Json::Object(map) => map,
        other => panic!("not an object: {other:?}"),
    }
}

fn settings(saved: &str, from: u64, platform: Platform) -> JsonMap {
    Settings::migrate(map(saved), from, platform)
}

fn text(map: &JsonMap, key: &str) -> Option<String> {
    map.get(key).and_then(Json::as_str).map(str::to_owned)
}

#[test]
fn turns_compact_mode_on_as_both_bars_and_off_as_off() {
    assert_eq!(text(&settings(r#"{"compactMode":true}"#, 2, Platform::Linux), "compactMode").unwrap(), "both");
    assert_eq!(text(&settings(r#"{"compactMode":false}"#, 2, Platform::Linux), "compactMode").unwrap(), "off");
    assert_eq!(text(&settings("{}", 1, Platform::Linux), "compactMode").unwrap(), "off");
}

#[test]
fn switches_load_beside_server_icons_off_once_for_everybody() {
    assert_eq!(text(&settings(r#"{"railStatus":"bars"}"#, 2, Platform::Linux), "railStatus").unwrap(), "off");
    // Picked again after the update, it stays.
    let kept = settings(r#"{"railStatus":"bars","compactMode":"sidebar"}"#, 3, Platform::Linux);
    assert_eq!(kept, map(r#"{"railStatus":"bars","compactMode":"sidebar"}"#));
}

#[test]
fn moves_the_look_that_came_with_the_app_to_ssheesh_default_once() {
    let old = r##"{"theme":"midnight","darkTheme":"midnight","lightTheme":"daylight","accent":"#5b9dff"}"##;
    let moved = settings(old, 3, Platform::Linux);
    assert_eq!(moved, map(r##"{"theme":"ssheesh","darkTheme":"ssheesh","lightTheme":"daylight","accent":"#4eba65"}"##));
    // A theme or accent picked by hand stays, and Midnight picked again after the update stays.
    let violet = settings(&old.replace("#5b9dff", "#b07cff"), 3, Platform::Linux);
    assert_eq!((text(&violet, "theme").unwrap(), text(&violet, "accent").unwrap()), ("midnight".into(), "#b07cff".into()));
    let graphite = settings(&old.replacen("midnight", "graphite", 1), 3, Platform::Linux);
    assert_eq!((text(&graphite, "theme").unwrap(), text(&graphite, "accent").unwrap()), ("graphite".into(), "#5b9dff".into()));
    assert_eq!(settings(old, 4, Platform::Linux), map(old));
    // An accent never saved was the old blue too.
    assert_eq!(text(&settings(r#"{"theme":"midnight"}"#, 3, Platform::Linux), "accent").unwrap(), "#4eba65");
}

#[test]
fn still_moves_macs_to_the_gpu_from_version_1() {
    assert_eq!(settings(r#"{"gpuRendering":false}"#, 1, Platform::Mac)["gpuRendering"], Json::Bool(true));
    assert_eq!(settings(r#"{"gpuRendering":false}"#, 2, Platform::Mac)["gpuRendering"], Json::Bool(false));
    assert_eq!(settings(r#"{"gpuRendering":false}"#, 1, Platform::Windows)["gpuRendering"], Json::Bool(false));
}

#[test]
fn migrating_keeps_settings_0_10_no_longer_reads() {
    let moved = settings(r#"{"placesWidth":180,"placesMode":"wide","compactMode":true}"#, 1, Platform::Linux);
    let keys: Vec<&str> = moved.keys().map(String::as_str).collect();
    assert_eq!(keys, ["placesWidth", "placesMode", "compactMode", "railStatus"]);
}

#[test]
fn labels_2_drops_looks_saved_as_if_picked_and_keeps_the_rest() {
    let saved = map(
        r##"{"env":{"cfg:db-main":"prod"},"looks":{
            "saved:a1":{"icon":"server","color":"#5B9DFF"},
            "saved:a2":{"icon":"server","color":"#7577e3"},
            "cfg:web-7731":{"icon":"globe","color":"#5b9dff"},
            "saved:a3":{"icon":"server"}}}"##,
    );
    let moved = Labels::migrate(saved.clone(), 1, Platform::Linux);
    let looks: Vec<&str> = moved["looks"].as_object().unwrap().keys().map(String::as_str).collect();
    // 0.10 would throw on saved:a3 (no color) and lose every label; it's kept here.
    assert_eq!(looks, ["saved:a2", "cfg:web-7731", "saved:a3"]);
    assert_eq!(Labels::migrate(saved.clone(), 2, Platform::Linux), saved);
}

fn pane(id: &str, host: &str, kind: &str, n: Option<u32>) -> String {
    let n = n.map(|n| format!(r#","n":{n}"#)).unwrap_or_default();
    format!(r#""{id}":{{"id":"{id}","kind":"{kind}","target":{{"kind":"host","hostId":"{host}"}}{n}}}"#)
}

fn ids(node: &Json) -> Vec<String> {
    let fields = node.as_object().unwrap();
    if fields["kind"].as_str() == Some("pane") {
        return vec![fields["id"].as_str().unwrap().to_owned()];
    }
    let Json::Array(children) = &fields["children"] else { panic!("no children") };
    children.iter().flat_map(ids).collect()
}

#[test]
fn splits_0_5s_layout_per_machine_keeps_each_arrangement_and_numbers_every_tab() {
    let saved = map(&format!(
        r#"{{"root":{{"kind":"split","id":"s","dir":"row","children":[{{"kind":"pane","id":"a"}},
            {{"kind":"split","id":"s-bc","dir":"row","children":[{{"kind":"pane","id":"b"}},{{"kind":"pane","id":"c"}}]}},
            {{"kind":"pane","id":"e"}}]}},
          "panes":{{{},{},{},"e":{{"id":"e","kind":"empty","target":null}}}},
          "focused":"e"}}"#,
        pane("a", "cfg:web", "terminal", None),
        pane("b", "cfg:db", "terminal", None),
        pane("c", "cfg:web", "terminal", Some(1)),
    ));
    let moved = Workspace::migrate(saved, 1, Platform::Linux);
    let layouts = moved["layouts"].as_object().unwrap();
    let panes = moved["panes"].as_object().unwrap();
    assert_eq!(ids(&layouts["host:cfg:web"]), ["a", "c"]);
    assert_eq!(ids(&layouts["host:cfg:db"]), ["b"]);
    assert!(!panes.contains_key("e"));
    assert_eq!((panes["a"].as_object().unwrap()["n"].as_u64(), panes["c"].as_object().unwrap()["n"].as_u64()), (Some(2), Some(1)));
    assert_eq!(moved["home"], Json::Bool(true));
    assert!(!moved.contains_key("root"));
}

#[test]
fn a_pane_without_a_machine_keeps_its_session_where_0_10_would_drop_it() {
    let saved = map(
        r#"{"root":{"kind":"pane","id":"t"},
            "panes":{"t":{"id":"t","kind":"terminal","target":null,"sessionKey":"q7w8e9r0t1"}},"focused":"t"}"#,
    );
    let moved = Workspace::migrate(saved, 1, Platform::Linux);
    assert_eq!(moved["panes"].as_object().unwrap()["t"].as_object().unwrap()["sessionKey"], Json::from("q7w8e9r0t1"));
    // No machine, so no screen: the tab can't show, but its session is on record.
    assert!(moved["layouts"].as_object().unwrap().is_empty());
}

#[test]
fn a_broken_split_reads_as_empty_instead_of_losing_every_tab() {
    let saved = map(&format!(
        r#"{{"root":{{"kind":"split","id":"s","dir":"row","children":[{{"kind":"pane","id":"a"}},{{"kind":"split","id":"broken"}}]}},
          "panes":{{{}}}}}"#,
        pane("a", "cfg:web", "terminal", None),
    ));
    let moved = Workspace::migrate(saved, 1, Platform::Linux);
    assert_eq!(ids(&moved["layouts"].as_object().unwrap()["host:cfg:web"]), ["a"]);
}

#[test]
fn layouts_come_in_the_order_machines_first_appear() {
    let saved = map(&format!(
        r#"{{"root":{{"kind":"split","id":"s","dir":"row","children":[{{"kind":"pane","id":"x"}},{{"kind":"pane","id":"y"}},{{"kind":"pane","id":"z"}}]}},
          "panes":{{{},{},{}}}}}"#,
        pane("z", "cfg:zeta", "terminal", None),
        pane("x", "cfg:alpha", "terminal", None),
        pane("y", "cfg:zeta", "files", None),
    ));
    let moved = Workspace::migrate(saved, 1, Platform::Linux);
    let order: Vec<&str> = moved["layouts"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(order, ["host:cfg:zeta", "host:cfg:alpha"]);
}
