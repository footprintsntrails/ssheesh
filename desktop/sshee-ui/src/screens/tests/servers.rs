//! "Add a server" with its keys, password managers, the eye, the check, a look and a label set
//! right away; a server's menu with Stats, Containers, side by side, its address and its
//! password; the look picker's "Automatic", any colour and a picture; and changes that fail
//! saying so. The made-up list is `super::book`.

use rune::Key;
use sshee_app::model::EnvLabel;
use sshee_app::servers::{Answer, Ask, PickedLook, SignIn};
use sshee_app::workbench::TabKind;

use super::{Window, DB, WEB};

#[test]
fn the_form_offers_the_keys_and_managers_found_here_and_shows_the_password_on_request() {
    let mut w = Window::open(false);
    w.click("add-server");
    assert!(w.asks().contains(&Ask::LocalAuth));
    let tree = w.tree();
    assert!(tree.contains("Your usual keys") && tree.contains("id_ed25519") && tree.contains("Another key file"), "{tree}");
    // A key found in ~/.ssh is used as it is.
    w.type_text("deploy@203.0.113.90");
    w.click("key-0");
    assert!(!w.has("add-key-file"));
    w.click("key-other");
    w.reveal("add-key-file");
    assert!(w.has("add-key-file"));
    // A password manager: the one there is is chosen.
    w.click("add-manager");
    assert_eq!(w.rt.state.screens.add_server.sign_in, SignIn::Manager);
    w.reveal("manager-0");
    assert!(w.tree().contains("1Password") && w.tree().contains("2 keys · you approve each sign-in there"), "{}", w.tree());
    // The eye: the field shows its characters, still a secure field.
    w.click("add-password");
    assert!(w.tree().contains("Show password"));
    w.click("add-eye");
    assert!(w.rt.state.screens.add_server.show_password && w.tree().contains("Hide password"));
    // Signing in with the manager reaches the backend with its socket.
    w.click("add-manager");
    w.click("add-save");
    let added = w.asks().into_iter().find_map(|a| match a {
        Ask::Add(new) => Some(new),
        _ => None,
    });
    assert_eq!(added.and_then(|n| n.identity_agent).as_deref(), Some("/run/op.sock"));
}

#[test]
fn a_server_is_checked_before_it_is_added_and_its_new_identity_trusted_by_hand() {
    let mut w = Window::open(false);
    w.click("add-server");
    w.type_text("root@203.0.113.90");
    w.click("add-test");
    let tree = w.tree();
    assert!(tree.contains("The server answered") && tree.contains("First time here. Is this really your server?"), "{tree}");
    assert!(tree.contains(sshee_app::servers::MADE_UP_FINGERPRINT));
    w.click("add-trust");
    assert!(w.asks().iter().any(|a| matches!(a, Ask::TrustTest(_))), "trust wasn't pressed");
    w.reveal("add-tested");
    let tree = w.tree();
    assert!(tree.contains("Signed in") && tree.contains("tmux is installed"), "{tree}");
    assert!(w.asks().iter().any(|a| matches!(a, Ask::TrustTest(_))));
    // A change to where it connects makes the check stale.
    w.click("add-port");
    w.type_text("2222");
    assert!(!w.tree().contains("Signed in"));
}

#[test]
fn an_icon_a_colour_and_a_label_are_given_as_the_server_is_added() {
    let mut w = Window::open(false);
    w.click("add-server");
    w.type_text("root@203.0.113.90");
    w.click("add-look");
    w.click("look-rocket");
    w.click("look-color-2");
    w.click("look-env-staging");
    w.click("add-save");
    let new = w.asks().into_iter().find_map(|a| match a {
        Ask::Add(new) => Some(new),
        _ => None,
    });
    let new = new.unwrap();
    assert_eq!(new.look, Some(PickedLook { icon: "rocket".into(), color: "#9d69d2".into(), image: None }));
    assert_eq!(new.env, Some(EnvLabel::Staging));
    // In the sidebar right away, with its look, and a toast says it was added.
    let added = w.rt.state.bench.servers().last().unwrap().clone();
    assert_eq!(added.icon, "rocket");
    assert!(added.env == Some(EnvLabel::Staging));
    assert_eq!(w.rt.state.screens.server_menu.faces[&added.key()].look.color, "#9d69d2");
    assert!(w.rt.state.screens.notes.titles().contains(&"Added 203.0.113.90"));
}

#[test]
fn the_keychain_refusing_a_password_is_said_not_dropped() {
    let mut w = Window::open(false);
    w.rt.update(|s, _| {
        s.screens.add_server.open = true;
        s.servers_answer(Answer::Added { server: WEB.into(), name: "web-7731".into(), warning: Some("Couldn't save the password in your keychain (locked).".into()) });
    });
    w.settle();
    assert!(w.rt.state.screens.notes.titles().contains(&"Added web-7731"));
}

#[test]
fn the_dialog_scrolls_in_a_low_window() {
    let mut w = Window::open(false);
    w.rt.set_viewport((1280.0, 600.0), 1.0);
    w.settle();
    w.click("add-server");
    let tree = w.tree();
    assert!(tree.contains("Add and connect"));
    // The buttons stay in the window; the body between scrolls.
    let (_, y) = w.rt.find("add-save").unwrap();
    assert!(y < 600.0, "the button is below the window: {y}");
    assert!(w.has("add-body"));
}

#[test]
fn the_menu_opens_stats_containers_and_terminal_and_files_side_by_side() {
    let mut w = Window::open(false);
    w.right_click("server-2");
    w.click("sm-3");
    assert_eq!(w.rt.state.bench.active().map(|t| (t.kind, t.server.as_str())), Some((TabKind::Stats, DB)));
    w.right_click("server-2");
    w.click("sm-4");
    assert_eq!(w.rt.state.bench.active().map(|t| (t.kind, t.server.as_str())), Some((TabKind::Containers, DB)));
    w.right_click("server-2");
    w.click("sm-2");
    let active = w.rt.state.bench.active().unwrap().clone();
    assert_eq!((active.kind, active.server.as_str()), (TabKind::Terminal, DB));
    let files = w.rt.state.bench.tabs_on(DB).find(|t| t.kind == TabKind::Files).unwrap().id;
    assert!(w.rt.state.bench.together(active.id, files));
}

#[test]
fn the_address_is_copied_except_in_streamer_mode_and_a_saved_password_forgotten() {
    let mut w = Window::open(false);
    // web-7731 has a saved password; db-main doesn't.
    w.right_click("server-1");
    let tree = w.tree();
    assert!(tree.contains("Copy address") && tree.contains("Forget saved password"), "{tree}");
    w.key(Key::Escape, "");
    w.right_click("server-2");
    assert!(!w.tree().contains("Forget saved password"));
    w.click("sm-6");
    assert!(w.rt.state.screens.notes.titles().contains(&"Copied the address"));
    // Forgetting it: asked first, then of the backend, said in a toast, and the row is gone.
    w.right_click("server-1");
    w.click("sm-12");
    assert!(w.tree().contains("Forget the saved password for web-7731?"), "{}", w.tree());
    w.click("dialog-yes");
    assert!(w.asks().contains(&Ask::ForgetPassword { server: WEB.into() }));
    assert!(w.rt.state.screens.notes.titles().iter().any(|t| t.starts_with("Forgot the saved password")));
    w.right_click("server-1");
    assert!(!w.tree().contains("Forget saved password"));
    w.key(Key::Escape, "");
    // Streamer mode: no address to copy.
    w.rt.update(|s, tx| s.toggle_streamer(tx));
    w.settle();
    w.right_click("server-1");
    assert!(!w.tree().contains("Copy address"));
}

#[test]
fn a_look_can_be_automatic_any_colour_or_a_picture_and_a_failure_says_so() {
    let mut w = Window::open(false);
    w.right_click("server-2");
    w.click("sm-8");
    assert!(w.tree().contains("Automatic: not checked yet"));
    // Any colour, typed.
    let typed = |w: &mut Window, text: &'static str| {
        w.rt.update(move |s, _| s.screens.server_menu.hex = rune::TextEdit::new(text));
        w.click("sm-hex-use");
    };
    typed(&mut w, "#ABC");
    assert_eq!(w.rt.state.screens.server_menu.faces[DB].look.color, "#aabbcc");
    // One that isn't a colour is said on the page.
    typed(&mut w, "blue");
    assert!(w.tree().contains("That isn't a color."));
    // One of the more colours.
    w.click("more-3");
    assert_eq!(w.rt.state.screens.server_menu.faces[DB].look.color, sshee_app::labels::MORE_COLORS[3]);
    // "Check the server now": a guess, and Automatic goes back to it.
    w.click("sm-check");
    assert!(w.asks().contains(&Ask::Detect { server: DB.into(), first: false }));
    w.click("sm-auto");
    assert!(!w.rt.state.screens.server_menu.faces[DB].look.picked);
    assert!(w.tree().contains("Automatic: database"));
    // A picture: drawn in the sidebar instead of the icon, never in streamer mode.
    w.rt.update(|s, _| s.servers_answer(Answer::Changed));
    w.rt.update(|s, _| {
        s.backend_mut().servers(Ask::SetImage { server: DB.into(), path: "/pictures/db.png".into() });
    });
    w.settle();
    assert!(w.rt.state.screens.server_menu.faces[DB].picture.is_some());
    assert!(w.has("sm-no-picture"));
    // A change that fails, while the page is open: said there.
    w.rt.update(|s, _| s.servers_answer(Answer::Refused { server: DB.into(), doing: "Couldn't change the server's look".into(), why: "The disk is full.".into() }));
    w.settle();
    assert!(w.tree().contains("Couldn't change the server's look. The disk is full."));
    // With the menu closed: a toast.
    w.click("sm-done");
    w.rt.update(|s, _| s.servers_answer(Answer::Refused { server: DB.into(), doing: "Couldn't rename the server".into(), why: "The disk is full.".into() }));
    w.settle();
    assert!(w.rt.state.screens.notes.titles().contains(&"Couldn't rename the server"));
}

#[test]
fn the_first_connect_guesses_the_icon_once_and_new_config_hosts_are_noted_in_the_sidebar() {
    let mut w = Window::open(false);
    // db-main's terminal connects: it is looked at, once.
    w.click("server-2");
    let detects = |w: &Window| w.asks().iter().filter(|a| matches!(a, Ask::Detect { first: true, .. })).count();
    assert_eq!(detects(&w), 0, "db-main has a look picked");
    w.right_click("server-2");
    w.click("sm-8");
    w.click("sm-auto");
    w.click("sm-done");
    // Back on "Automatic", with nothing guessed yet: the next connect looks.
    let tab = w.rt.state.bench.active_id().unwrap();
    w.rt.update(move |s, _| s.link_status(tab, crate::terminal::LinkStatus::Connected { durable: false }));
    w.settle();
    assert_eq!(detects(&w), 1);
    w.rt.update(move |s, _| s.link_status(tab, crate::terminal::LinkStatus::Connected { durable: false }));
    w.settle();
    assert_eq!(detects(&w), 1, "a guess is kept: no second look");
    // cache-7731 is in the ssh config and not decided: the sidebar says so, and Review opens it.
    assert!(w.tree().contains("1 new in ~/.ssh/config · Review"));
    w.click("config-new");
    assert!(w.tree().contains("Servers from ~/.ssh/config"));
}
