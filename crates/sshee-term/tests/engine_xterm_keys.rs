//! Every key in `engine_xterm_keys.json` sends what xterm.js 6.0 sent for it in SSHeesh 0.10.
//! The table comes from xterm.js's own code (see `engine_xterm_keys.gen.mjs`), with what the
//! system typed for each key (Ctrl+Alt+C on a US layout on Windows types nothing).

use sshee_term::input::{xterm_key, Code, Key, KeyAction, KeyEvent, KeyOptions, Mods, Platform};

#[test]
fn keys_match_xterm_js() {
    let table: serde_json::Value = serde_json::from_str(include_str!("engine_xterm_keys.json")).expect("table");
    let rows = table["rows"].as_array().expect("rows");
    assert!(rows.len() > 5000, "the table has {} keys", rows.len());
    let mut wrong = Vec::new();
    for row in rows {
        let r = row.as_array().expect("row");
        let (key, code) = (r[0].as_str().unwrap(), r[1].as_str().unwrap());
        let mods = r[2].as_u64().unwrap() as u8;
        let (platform, mac_option_is_meta) = match r[3].as_str().unwrap() {
            "l" => (Platform::Other, true),
            "w" => (Platform::Windows, true),
            "m" => (Platform::Mac, true),
            "M" => (Platform::Mac, false),
            other => panic!("platform {other}"),
        };
        let app_cursor = r[4].as_u64().unwrap() == 1;
        let expected = match (r[5].as_str().unwrap(), r[6].as_str().unwrap()) {
            (sent, "") if !sent.is_empty() => KeyAction::Send(sent.to_owned()),
            ("", "") => KeyAction::Nothing,
            ("", "pageUp") => KeyAction::PageUp,
            ("", "pageDown") => KeyAction::PageDown,
            ("", "selectAll") => KeyAction::SelectAll,
            other => panic!("unexpected row {other:?}"),
        };
        // What the system typed: a browser sent keypress only then.
        let text = r[7].as_str().map(|t| t.chars().next().expect("one character"));
        let ev = KeyEvent::new(Key::from_dom(key), Code::from_dom(code), Mods::from_bits(mods)).with_text(text);
        let opts = KeyOptions { platform, mac_option_is_meta, app_cursor };
        let got = xterm_key(&ev, &opts);
        if got != expected {
            wrong.push(format!("{row}: got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{} of {} keys differ from xterm.js:\n{}", wrong.len(), rows.len(), wrong.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
