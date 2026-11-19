//! A tab dragged out into a window of its own, or moved there from its menu: the same tab, its
//! session and what it holds, in the new window; answers for it follow it there.

use super::*;

/// The new window's runtime, made around `shell` as the window's own would be.
fn window_of(shell: Shell) -> Runtime<Shell> {
    let text = rune::text::TextSystem::new(crate::fonts::FONTS, crate::fonts::FAMILIES);
    let mut rt = Runtime::with_text(shell, view, (1280.0, 800.0), Env::new(true, 1.0), text);
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    rt.on_key = Some(|s, ev, tx| s.key(ev, tx));
    // As `shell::open_window_with_tab` has the window's first turn do.
    rt.update(|s, tx| s.window_opened(tx));
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

/// Moves tab `id` of `w` into a new window, reached through a queue of its own.
fn tear_off(w: &mut Window, id: sshee_app::workbench::TabId) -> (Runtime<Shell>, Queue, u16) {
    let posted: Queue = Arc::new(Mutex::new(Vec::new()));
    let queue = posted.clone();
    let remote = Remote::new(Arc::new(move |f| queue.lock().unwrap().push(f)));
    let mut made = None;
    w.rt.update(|s, tx| {
        let no = s.hub().next_window().unwrap();
        made = s.window_with_tab(no, id, remote, tx).map(|shell| (shell, no));
    });
    w.settle();
    let (shell, no) = made.expect("the tab moved");
    (window_of(shell), posted, no)
}

/// Runs what reached the new window.
fn drain(rt: &mut Runtime<Shell>, posted: &Queue) -> usize {
    let jobs: Vec<_> = std::mem::take(&mut *posted.lock().unwrap());
    let n = jobs.len();
    rt.update(|s, tx| {
        for job in jobs {
            job(s, tx);
        }
    });
    rt.frame(1.0 / 60.0);
    n
}

#[test]
fn a_terminal_moved_to_a_new_window_keeps_its_session_and_scrollback() {
    let mut w = linux();
    w.click("server-1");
    w.feed(0, b"web-7731:~$ tail -f app.log\r\nline 1 of the log\r\n");
    w.plus_terminal();
    let first = w.rt.state.bench.tabs()[0].id;
    let (mut second, posted, no) = tear_off(&mut w, first);

    // No new session: the one it had, neither closed nor ended, and its model is the pane's.
    assert_eq!(w.sessions(), 2);
    let seen = w.session(0);
    assert!(!seen.lock().unwrap().closed && !seen.lock().unwrap().ended);
    let model = seen.lock().unwrap().model.clone().unwrap();
    assert!(Arc::ptr_eq(&second.state.panes[&first].model, &model));
    assert!(!w.rt.state.panes.contains_key(&first));

    // The new window shows it, with what it printed before the move.
    assert_eq!(second.state.bench.active_id(), Some(first));
    let tree = second.accessibility_tree();
    assert!(tree.nodes.iter().any(|(_, n)| n.value().is_some_and(|v| v.contains("line 1 of the log"))), "the scrollback didn't come along");

    // The old window goes on with its other tab; the moved one wasn't closed.
    assert_eq!(w.row(), "*Terminal 2@web-7731");
    assert!(w.rt.state.bench.closed().is_empty());

    // Output after the move reaches the new window's pane.
    w.feed(0, b"line 2 after the move\r\n");
    for _ in 0..12 {
        second.frame(1.0 / 60.0);
    }
    let tree = second.accessibility_tree();
    assert!(tree.nodes.iter().any(|(_, n)| n.value().is_some_and(|v| v.contains("line 2 after the move"))));

    // Keys typed in the new window reach the same session.
    second.key(KeyEvent { key: Key::Text("x".into()), mods: Mods::default() });
    second.frame(1.0 / 60.0);
    assert_eq!(w.written(0), "x");

    // Ids stay unique: the new window numbers its own tabs in its range.
    second.update(|s, tx| s.new_terminal(tx));
    w.plus_terminal();
    let ids: Vec<_> = w.rt.state.bench.tabs().iter().chain(second.state.bench.tabs()).map(|t| t.id).collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");
    assert!(second.state.bench.tabs().iter().filter(|t| t.id != first).all(|t| t.id.window() == no));

    // What the session says, and the answers for the tab, go to the window that has it now.
    let status = seen.lock().unwrap().status.clone().unwrap();
    w.posted.lock().unwrap().clear();
    status(LinkStatus::Asleep);
    let routed = Remote::routed(w.rt.state.hub().clone());
    routed.through_tor(first, false);
    routed.open_tab(first, None);
    assert!(w.posted.lock().unwrap().is_empty());
    assert_eq!(drain(&mut second, &posted), 3);
    assert_eq!(second.state.bench.tab(first).unwrap().link, Link::Asleep);
    assert!(second.state.screens.tor.direct.contains(&first));
    assert_eq!(second.state.bench.active_id(), Some(first));

    // Closing the new window closes its tabs, as any other window's: the moved shell ends.
    second.state.close();
    assert!(seen.lock().unwrap().ended);
    assert!(!w.session(1).lock().unwrap().ended);
}

#[test]
fn files_moved_to_a_new_window_keeps_its_folder_and_what_it_holds() {
    let mut w = linux();
    w.click("server-1");
    w.rt.update(|s, tx| s.browse_files(s.bench.active_id().unwrap(), Some("/var/log".into()), tx));
    w.settle();
    let files = w.rt.state.bench.active_id().unwrap();
    assert_eq!(w.rt.state.bench.tab(files).unwrap().kind, TabKind::Files);
    // Something only this tab's state holds.
    w.rt.state.screens.files.get_mut(&files).unwrap().busy = Some("Deleting 3 items…".into());
    let asked = w.world.lock().unwrap().files.len();
    let (second, _, _) = tear_off(&mut w, files);
    let st = &second.state.screens.files[&files];
    assert_eq!(st.browser.dir(), "/var/log");
    assert_eq!(st.busy.as_deref(), Some("Deleting 3 items…"));
    assert_eq!(second.state.bench.tab(files).unwrap().path.as_deref(), Some("/var/log"));
    assert!(!w.rt.state.screens.files.contains_key(&files));
    // Nothing listed again: the same state, not a new one.
    assert_eq!(w.world.lock().unwrap().files.len(), asked);
    // The terminal left behind is on screen in the old window.
    assert_eq!(w.row(), "*Terminal@web-7731");
}

#[test]
fn the_last_tab_moved_out_leaves_its_window_on_the_main_menu() {
    let mut w = linux();
    w.click("server-1");
    let only = w.rt.state.bench.active_id().unwrap();
    let (second, _, _) = tear_off(&mut w, only);
    // 0.10 had no tabs to drag out: the window stays, as closing its last tab leaves it.
    assert!(w.rt.state.bench.tabs().is_empty() && w.rt.state.bench.showing_home());
    assert!(w.rt.find("home-page").is_some());
    assert!(!w.session(0).lock().unwrap().ended && !w.session(0).lock().unwrap().closed);
    assert_eq!(second.state.bench.active_id(), Some(only));
}

#[test]
fn a_tabs_menu_moves_it_to_a_new_window() {
    let mut w = linux();
    w.click("server-1");
    w.plus_terminal();
    let first = w.rt.state.bench.tabs()[0].id;
    let (x, y) = w.rt.find(&format!("tab-{}", first.0)).unwrap();
    w.rt.secondary_down(x, y);
    w.rt.frame(1.0 / 60.0);
    w.rt.secondary_up(x, y);
    w.settle();
    assert!(w.tree().contains("Move to new window"));
    let before = w.rt.state.hub().windows();
    w.click("tab-new-window");
    assert!(w.rt.find("tab-menu").is_none());
    // It left this window without ending, for a window of its own that has it now.
    assert_eq!(w.row(), "*Terminal 2@web-7731");
    assert!(w.rt.state.bench.closed().is_empty());
    assert!(!w.session(0).lock().unwrap().ended && !w.session(0).lock().unwrap().closed);
    let windows = w.rt.state.hub().windows();
    assert_eq!(windows.len(), before.len() + 1);
    let new = *windows.last().unwrap();
    assert_eq!(w.rt.state.hub().window_of(first), new);
    assert_eq!(w.sessions(), 2);
}

#[test]
fn a_tab_is_torn_off_only_well_past_the_windows_edge() {
    let room = [1280.0, 800.0];
    for inside in [[10.0, 10.0], [-30.0, 400.0], [1300.0, 20.0], [640.0, 830.0], [640.0, -39.0]] {
        assert!(!super::super::tabs::past_edge(inside, room), "{inside:?}");
    }
    for out in [[-41.0, 400.0], [1321.0, 20.0], [640.0, 841.0], [640.0, -60.0]] {
        assert!(super::super::tabs::past_edge(out, room), "{out:?}");
    }
    // The new window's frame puts the held point of its tab under the pointer: at 2x, 100
    // points in are 200 pixels, past a 30-pixel title bar.
    assert_eq!(super::super::frame_at((1500, 600), [100.0, 22.0], 2.0, (1, 30)), (1299, 526));
}
