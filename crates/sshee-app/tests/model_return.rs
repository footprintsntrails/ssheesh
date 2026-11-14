//! A return from 0.10.14 (plan, section 6.5): someone went to 1.x, back to 0.10.14, opened
//! terminals there, and came to 1.x again. 1.x takes in only the terminals with new tmux
//! sessions and keeps everything of its own.

mod model_support;

use model_support::*;
use sshee_app::model::{Dir, NodeKind, Pane, PaneKind, Platform, SessionRef, TargetRef, Workspace};

/// 1.x after the first import of the day-of-use stores, and the sessions it brought over.
fn first_import() -> (Workspace, Vec<String>) {
    let imported = import_on(get(&fixture("ts-in-use.json"), &["stores"]), Platform::Linux);
    let seen = imported.report.sessions.iter().map(|s| s.session_key.clone()).collect();
    (imported.state.workspace, seen)
}

fn terminal(id: &str, target: TargetRef, key: &str, n: u32) -> Pane {
    Pane { id: id.into(), kind: PaneKind::Terminal, target: Some(target), session_key: Some(key.into()), n: Some(n), ..Pane::default() }
}

fn session(pane: &str, machine: &str, key: &str) -> SessionRef {
    SessionRef { pane: pane.into(), machine: Some(machine.into()), session_key: key.into(), durable: true }
}

#[test]
fn only_terminals_opened_in_0_10_14_come_in_and_nothing_here_changes() {
    let (mut ours, seen) = first_import();
    // 0.10.14's store is as the first import found it: 1.x never writes to it.
    let mut theirs = ours.clone();
    // In 1.x: a tab closed, and one opened.
    ours.panes.shift_remove("p_dhecbt0t");
    ours.panes.insert("p_new1x".into(), terminal("p_new1x", TargetRef::host("cfg:web-7731"), "k1x", 4));
    // Back in 0.10.14: a terminal on db-main, and one on this computer.
    theirs.panes.insert("p_k9".into(), terminal("p_k9", TargetRef::host("cfg:db-main"), "k9", 3));
    theirs.panes.insert("p_loc2".into(), terminal("p_loc2", TargetRef::local(), "kloc", 2));

    let before = ours.clone();
    let adopted = ours.adopt_new_sessions(&theirs, &seen);
    assert_eq!(adopted, [session("p_k9", "host:cfg:db-main", "k9")]);
    // Everything 1.x had is as it was; the tab closed in 1.x stays closed; the local
    // terminal ended with 0.10.14.
    for (id, pane) in &before.panes {
        assert_eq!(&ours.panes[id], pane, "{id}");
    }
    assert_eq!(ours.panes.len(), before.panes.len() + 1);
    assert!(!ours.panes.contains_key("p_dhecbt0t") && !ours.panes.contains_key("p_loc2"));
    assert_eq!(ours.panes["p_k9"].n, Some(3));
    // On db-main's screen beside its terminal, as 0.10 opens a tab.
    let db = &ours.layouts["host:cfg:db-main"];
    assert_eq!((db.kind.clone(), db.dir.clone()), (NodeKind::Split, Some(Dir::Row)));
    assert_eq!(db.pane_ids(), ["p_3et92i0c", "p_k9"]);
    for (key, layout) in &before.layouts {
        if key != "host:cfg:db-main" {
            assert_eq!(&ours.layouts[key], layout, "{key}");
        }
    }
    assert_eq!(ours.focused, before.focused);
    // Coming back once more with the same store takes in nothing.
    assert!(ours.adopt_new_sessions(&theirs, &seen).is_empty());
}

#[test]
fn a_taken_id_or_number_gets_the_next_free_one() {
    let (mut ours, seen) = first_import();
    let mut theirs = Workspace::default();
    // 0.10.14 can't know 1.x's ids and numbers: this one's id and "Terminal 1" are taken.
    theirs.panes.insert("p_3et92i0c".into(), terminal("p_3et92i0c", TargetRef::host("cfg:db-main"), "k9", 1));
    let adopted = ours.adopt_new_sessions(&theirs, &seen);
    assert_eq!(adopted, [session("p_k9", "host:cfg:db-main", "k9")]);
    assert_eq!(ours.panes["p_3et92i0c"].session_key.as_deref(), Some("izq2ryolkg"));
    let new = &ours.panes["p_k9"];
    assert_eq!((new.id.as_str(), new.n), ("p_k9", Some(3)));
}

#[test]
fn a_new_tab_goes_where_0_10_would_open_it() {
    let (mut ours, seen) = first_import();
    let mut theirs = Workspace::default();
    theirs.panes.insert("p_a".into(), terminal("p_a", TargetRef::host("cfg:web-7731"), "ka", 9));
    theirs.panes.insert("p_b".into(), terminal("p_b", TargetRef::host("saved:n3w0ne"), "kb", 1));
    let adopted = ours.adopt_new_sessions(&theirs, &seen);
    assert_eq!(adopted.len(), 2);
    // After the tab last used on web-7731, in the same row.
    let web = ours.layouts["host:cfg:web-7731"].pane_ids();
    assert_eq!(web, ["p_dw2vwdy8", "p_dhecbt0t", "p_1omplald", "p_a", "p_03oxci2w", "p_hp5avjy0"]);
    // A server without a screen gets one, at the end of the tab strip.
    assert_eq!(ours.layouts.keys().last().map(String::as_str), Some("host:saved:n3w0ne"));
    assert_eq!(ours.layouts["host:saved:n3w0ne"].pane_ids(), ["p_b"]);

    // A terminal opened beside files goes before them, where you read first.
    let (mut ours, seen) = first_import();
    ours.last_focused_for.insert("host:cfg:web-7731".into(), "p_hp5avjy0".into());
    ours.adopt_new_sessions(&theirs, &seen);
    let web = ours.layouts["host:cfg:web-7731"].pane_ids();
    assert_eq!(web, ["p_dw2vwdy8", "p_dhecbt0t", "p_1omplald", "p_03oxci2w", "p_a", "p_hp5avjy0"]);
}
