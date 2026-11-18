//! Toasts and confirmations (`shell::notes`, `kit::toast`, `kit::dialog`): they show, leave by
//! themselves, wait under the pointer, take the keyboard while a question is open, and hide
//! names in streamer mode. Same harness as the window's tests.

use crate::shell::{Confirm, Toast};

use super::*;

impl Window {
    fn titles(&self) -> Vec<String> {
        self.rt.state.screens.notes.titles().into_iter().map(str::to_owned).collect()
    }

    fn pass(&mut self, seconds: f64) {
        for _ in 0..(seconds * 60.0).ceil() as usize {
            self.rt.frame(1.0 / 60.0);
        }
        self.settle();
    }
}

#[test]
fn a_toast_shows_in_the_corner_and_leaves_by_itself() {
    let mut w = linux();
    w.rt.update(|s, _| {
        s.toast(Toast::info("Copied the path").body("/srv/shop/deploy.sh"));
    });
    w.settle();
    assert!(w.tree().contains("Copied the path") && w.tree().contains("/srv/shop/deploy.sh"));
    // Over the status bar, at the right, once it has come in (`kit::motion`).
    for _ in 0..15 {
        w.rt.frame(1.0 / 60.0);
    }
    let [x, y, width, height] = w.rect("toast-1").expect("the toast is drawn");
    assert_eq!(width, 300.0);
    assert!(x + width <= 1280.0 - 14.0 && y + height <= 800.0 - 24.0, "{x} {y} {width} {height}");
    w.pass(3.0);
    assert_eq!(w.titles(), ["Copied the path"]);
    w.pass(1.5);
    assert!(w.titles().is_empty() && w.rect("toast-1").is_none());
}

#[test]
fn an_error_stays_longer_and_at_most_four_are_shown() {
    let mut w = linux();
    w.rt.update(|s, _| {
        s.toast(Toast::error("Couldn't rename").body("Permission denied"));
        for n in 1..=4 {
            s.toast(Toast::success(format!("Downloaded file {n}")));
        }
    });
    w.settle();
    // The fifth pushed the oldest out.
    assert_eq!(w.titles(), ["Downloaded file 1", "Downloaded file 2", "Downloaded file 3", "Downloaded file 4"]);
    w.rt.update(|s, _| {
        s.toast(Toast::error("Couldn't delete shop").body("Permission denied"));
    });
    w.pass(4.5);
    assert_eq!(w.titles(), ["Couldn't delete shop"]);
    w.pass(3.0);
    assert!(w.titles().is_empty());
}

#[test]
fn a_toasts_button_does_its_thing_and_the_cross_dismisses() {
    let mut w = linux();
    w.rt.update(|s, _| {
        s.toast(Toast::success("Moved site.zip to shop").action("Undo", |s| {
            s.toast(Toast::info("Moved back"));
        }));
        s.toast(Toast::info("Copied the path"));
    });
    w.settle();
    w.click("toast-2-close");
    assert_eq!(w.titles(), ["Moved site.zip to shop"]);
    w.click("toast-1-action");
    assert_eq!(w.titles(), ["Moved back"]);
}

#[test]
fn a_toast_under_the_pointer_waits() {
    let mut w = linux();
    w.rt.update(|s, _| {
        s.toast(Toast::info("Copied the path"));
    });
    w.settle();
    let (x, y) = w.rt.find("toast-1").unwrap();
    w.rt.pointer_move(x, y);
    w.pass(6.0);
    assert_eq!(w.titles(), ["Copied the path"]);
    w.rt.pointer_move(600.0, 400.0);
    w.pass(2.5);
    assert!(w.titles().is_empty());
}

/// "Remove web-7731?" asked; its yes opens the main menu (something to see happen).
fn ask(w: &mut Window, undoable: bool) {
    w.rt.update(move |s, _| {
        s.confirm(Confirm::new("Remove web-7731?", "Its tabs close. Its shells in tmux stay on the server.", "Remove").destructive(undoable), |s, tx| s.show_home(tx));
    });
    w.settle();
}

#[test]
fn a_confirmation_takes_the_keyboard_and_answers_return_and_esc() {
    let mut w = linux();
    w.click("server-1");
    ask(&mut w, true);
    assert!(w.rt.find("dialog").is_some() && w.rt.find("dialog-layer").is_some());
    assert!(w.tree().contains("Remove web-7731?"));
    // Keys don't reach the terminal behind it, and no shortcut fires.
    w.key(Key::Text("x".into()), "");
    w.key(Key::Command('t'), "cs");
    assert_eq!((w.written(0), w.row()), (String::new(), "*Terminal@web-7731".to_owned()));
    // Esc: nothing happens, and the terminal has the keyboard again.
    w.key(Key::Escape, "");
    assert!(w.rt.find("dialog").is_none());
    assert!(!w.rt.state.bench.showing_home());
    w.key(Key::Text("y".into()), "");
    assert_eq!(w.written(0), "y");
    // Return presses Remove, as it can be undone.
    ask(&mut w, true);
    w.key(Key::Enter, "");
    assert!(w.rt.state.bench.showing_home());
}

#[test]
fn what_cant_be_undone_takes_return_as_cancel_and_a_click_answers() {
    let mut w = linux();
    w.click("server-1");
    ask(&mut w, false);
    w.key(Key::Enter, "");
    assert!(w.rt.find("dialog").is_none());
    assert!(!w.rt.state.bench.showing_home());
    ask(&mut w, false);
    w.click("dialog-yes");
    assert!(w.rt.state.bench.showing_home());
}

#[test]
fn names_in_toasts_and_questions_are_hidden_while_streaming() {
    let mut w = linux();
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.rt.update(|s, _| {
        s.toast(Toast::error("Couldn't reach web-7731").body("deploy@203.0.113.17 refused"));
    });
    w.settle();
    // The toast, then the question (which hides what is behind it from the screen reader).
    for (tree, said) in [(w.tree(), "Couldn't reach"), ({ ask(&mut w, true); w.tree() }, "Remove")] {
        assert!(tree.contains(said), "{tree}");
        for hidden in ["web-7731", "203.0.113.17"] {
            assert!(!tree.contains(hidden), "{hidden} shows: {tree}");
        }
    }
}
