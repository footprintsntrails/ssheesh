//! Attention: the bell, OSC 9 and OSC 777, "went quiet", and the system
//! notification, which in 1.0 never carries a secret or (while streaming) a
//! name. 0.10 had no tests for these (they lived in `terminals.ts`); the
//! rules are its, and the notification text is checked with a made-up token.

use sshee_term::attention::{bell, notice, notification, osc777, osc9, place_name, Ask, Attention, AttentionKind, NoticeContext, Place, QuietWatch};
use sshee_term::present::Look;
use sshee_term::privacy::{HostNames, Names};
use std::sync::Arc;

/// A made-up GitHub token: the right shape, nobody's.
fn token() -> String {
    format!("ghp_{}", "Zx9Q".repeat(9))
}

fn look(streaming: bool, hide_secrets: bool) -> Look {
    let names = Names::new([HostNames { name: "web-7731", hostname: "198.51.100.23", user: Some("deploy") }], []);
    Look { streaming, hide_secrets, names: Arc::new(names), ..Default::default() }
}

fn cx<'a>(look: &'a Look, place: &'a str) -> NoticeContext<'a> {
    NoticeContext { watched: false, app_focused: false, notify: true, previous: None, place, look }
}

#[test]
fn a_token_in_an_osc_9_message_never_reaches_the_notification() {
    let tok = token();
    let ask = osc9(&format!("Build done, token {tok} saved")).expect("a message");
    // Secrets shown in the terminal (Settings) and not streaming: still dotted outside the app.
    for look in [look(false, false), look(false, true), look(true, false)] {
        let n = notice(AttentionKind::Input, ask.message.clone(), 5, &cx(&look, "web-7731")).expect("a notice").notification.expect("a notification");
        assert!(!n.body.contains(&tok) && !n.body.contains("Zx9Q"), "{}", n.body);
        assert!(n.body.starts_with("Build done, token ") && n.body.ends_with(" saved"), "{}", n.body);
        assert!(!n.title.contains("Zx9Q"));
    }
}

#[test]
fn a_token_split_by_control_codes_is_still_found() {
    let tok = token();
    let (a, b) = tok.split_at(10);
    let ask = osc9(&format!("token {a}\x07\x1b{b} and\u{202E} more")).expect("a message");
    let look = look(false, false);
    let n = notification(&Attention { kind: AttentionKind::Input, message: ask.message, at: 0 }, "web-7731", &look);
    assert!(!n.body.contains("Zx9Q") && !n.body.contains('\u{202E}') && !n.body.contains('\x1b'), "{:?}", n.body);
}

#[test]
fn while_streaming_names_and_addresses_are_dotted_in_title_and_body() {
    let look = look(true, false);
    let place = place_name(Place::Host { index: 1, name: "web-7731" }, true);
    assert_eq!(place, "Server 2");
    let ask = osc777("notify;deploy@web-7731;reached 198.51.100.23").expect("a notify");
    let n = notice(AttentionKind::Input, ask.message, 1, &cx(&look, &place)).unwrap().notification.unwrap();
    assert_eq!(n.title, "Server 2 needs you");
    // The address is a saved server's, so it goes whole, dots and all, as in 0.10.
    assert_eq!(n.body, "••••••@••••••••: reached •••••••••••••");
    // Had the app passed the real name by mistake, it would be dotted too.
    let n = notice(AttentionKind::Done, None, 1, &cx(&look, "web-7731")).unwrap().notification.unwrap();
    assert_eq!(n.title, "•••••••• went quiet");
    assert_eq!(n.body, "Output stopped after a long run. It may be done.");
}

#[test]
fn the_message_in_the_app_follows_the_terminals_rules() {
    let tok = token();
    let a = Attention { kind: AttentionKind::Input, message: Some(format!("deploy {tok}")), at: 0 };
    assert_eq!(a.shown_message(&look(false, false)), Some(format!("deploy {tok}")));
    assert!(!a.shown_message(&look(false, true)).unwrap().contains("Zx9Q"));
    assert!(!a.shown_message(&look(true, false)).unwrap().contains("deploy"));
}

#[test]
fn reads_what_programs_send() {
    assert_eq!(bell(), Ask { message: None });
    assert_eq!(osc9("Claude is waiting"), Some(Ask { message: Some("Claude is waiting".into()) }));
    assert_eq!(osc9(""), Some(Ask { message: None }));
    // Progress, and ConEmu's numbered commands, aren't messages.
    assert_eq!(osc9("4;1;50"), None);
    assert_eq!(osc9("9;C:\\Users"), None);
    assert_eq!(osc777("notify;Build;finished"), Some(Ask { message: Some("Build: finished".into()) }));
    assert_eq!(osc777("notify;;only body"), Some(Ask { message: Some("only body".into()) }));
    assert_eq!(osc777("notify"), Some(Ask { message: None }));
    assert_eq!(osc777("precmd;x"), None);
}

#[test]
fn notifies_once_per_wait_and_only_in_the_background() {
    let look = look(false, true);
    let base = cx(&look, "This computer");
    assert!(notice(AttentionKind::Input, None, 0, &NoticeContext { watched: true, ..base }).is_none());
    let n = notice(AttentionKind::Input, None, 0, &base).unwrap();
    assert_eq!(n.attention, Attention { kind: AttentionKind::Input, message: None, at: 0 });
    let shown = n.notification.unwrap();
    assert_eq!((shown.title.as_str(), shown.body.as_str()), ("This computer needs you", "A program in the terminal is waiting for you."));
    // The tab already says so: the tab is updated, nothing pops up again.
    assert!(notice(AttentionKind::Input, None, 0, &NoticeContext { previous: Some(AttentionKind::Input), ..base }).unwrap().notification.is_none());
    assert!(notice(AttentionKind::Input, None, 0, &NoticeContext { app_focused: true, ..base }).unwrap().notification.is_none());
    assert!(notice(AttentionKind::Input, None, 0, &NoticeContext { notify: false, ..base }).unwrap().notification.is_none());
    assert!(notice(AttentionKind::Done, None, 0, &NoticeContext { previous: Some(AttentionKind::Input), ..base }).unwrap().notification.is_some());
}

#[test]
fn a_long_body_is_cut_after_its_secrets_are_dotted() {
    let tok = token();
    // The token straddles the cut: cut first, and its first half would no longer look like a token.
    let message = format!("{}{tok}", "x ".repeat(240));
    let n = notification(&Attention { kind: AttentionKind::Input, message: Some(message), at: 0 }, "web-7731", &look(false, false));
    assert_eq!(n.body.chars().count(), 500);
    assert!(n.body.ends_with('…') && !n.body.contains("ghp_"), "{}", n.body);
}

#[test]
fn says_a_long_unwatched_run_went_quiet_once() {
    let mut w = QuietWatch::default();
    for t in (0..=20_000).step_by(1000) {
        w.output(t, false);
    }
    assert!(!w.check(25_000));
    assert!(w.check(26_000));
    assert!(!w.check(30_000));
    // A short run, or one you watched, doesn't.
    let mut short = QuietWatch::default();
    short.output(0, false);
    short.output(10_000, false);
    assert!(!short.check(60_000));
    let mut watched = QuietWatch::default();
    for t in (0..=20_000).step_by(1000) {
        watched.output(t, true);
    }
    assert!(!watched.check(60_000));
    // Output after a gap starts a new run, which can be noted again.
    for t in (40_000..=60_000).step_by(1000) {
        w.output(t, false);
    }
    assert!(w.check(66_000));
}

#[test]
fn a_server_name_inside_a_token_doesnt_let_the_token_into_the_notification() {
    // A server called "web" dots "WEB" inside the token while streaming; the token is still found.
    let names = Names::new([HostNames { name: "web", hostname: "198.51.100.41", user: None }, HostNames { name: "api.example.net", hostname: "198.51.100.40", user: None }], []);
    let look = Look { streaming: true, names: Arc::new(names), ..Default::default() };
    let tok = "ghp_Ab3Kq9m2Lp7XWEBz4Rt8Vn6Yc1Hd5Fg0JkQx7";
    let a = Attention { kind: AttentionKind::Input, message: Some(format!("token {tok}\nAPI_KEY=Qm7vT2xLp9")), at: 1 };
    let n = notification(&a, "Server 1", &look);
    assert_eq!(n.body, format!("token {}\n•••_KEY={}", "•".repeat(tok.len()), "•".repeat(10)));
}
