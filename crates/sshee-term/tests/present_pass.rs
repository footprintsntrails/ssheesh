//! The presentation pass (`present.rs`): masks, secrets, highlights, links
//! and find marks over rows as the screen has them, and the rule it exists
//! for: nothing hidden ever leaves it. All names, addresses and tokens here
//! are made up.

use sshee_term::find::FindRow;
use sshee_term::highlight::{HighlightColor, Highlights};
use sshee_term::links::{LinkClick, LinkKind};
use sshee_term::present::{Edges, FindMarks, Hyperlink, Look, PresentedRow, Presenter, Role, RowText, Screen};
use sshee_term::privacy::{HostNames, Names};
use std::sync::Arc;

const COLS: u16 = 40;

fn token() -> String {
    format!("ghp_{}", "Zx9Q".repeat(9))
}

fn names(learned: &[&str]) -> Arc<Names> {
    let hosts = [
        HostNames { name: "prod-api", hostname: "203.0.113.10", user: Some("deploy") },
        HostNames { name: "db-main", hostname: "db-main.example.net", user: None },
    ];
    Arc::new(Names::new(hosts, learned.iter().copied()))
}

fn look(streaming: bool, hide_secrets: bool) -> Look {
    Look { streaming, hide_secrets, names: names(&[]), highlights: Arc::new(Highlights::new(true, true, false, &[])) }
}

/// A row padded with blanks to `cols`, as the grid has it.
fn row(text: &str, cols: u16) -> RowText {
    let mut r = RowText::plain(text);
    let used = r.cols.last().map_or(0, |&c| c + 1);
    for col in used..cols {
        r.text.push(' ');
        r.cols.push(col);
    }
    r
}

fn rows(texts: &[&str]) -> Vec<RowText> {
    texts.iter().map(|t| row(t, COLS)).collect()
}

fn present(p: &mut Presenter, rows: &[RowText]) -> Vec<Arc<PresentedRow>> {
    p.present(&Screen::new(rows, COLS))
}

fn roles(r: &PresentedRow) -> Vec<(std::ops::Range<u16>, Role)> {
    r.spans.iter().map(|s| (s.cols.clone(), s.role)).collect()
}

/// Nothing of `hidden` anywhere in what the pass handed out.
fn assert_never_shown(out: &[Arc<PresentedRow>], hidden: &[&str]) {
    let all: String = out.iter().map(|r| format!("{}\n{:?}\n{:?}\n", r.text, r.spans, r.links)).collect();
    let joined: String = out.iter().map(|r| r.text.trim_end()).collect();
    for h in hidden {
        assert!(!all.contains(h) && !joined.contains(h), "{h} shown in:\n{all}");
    }
}

#[test]
fn streamer_mode_dots_names_users_and_addresses_in_place() {
    let mut p = Presenter::new(look(true, false));
    let out = present(&mut p, &rows(&["deploy@prod-api:~$ ping 203.0.113.10"]));
    assert_eq!(out[0].text.trim_end(), "••••••@••••••••:~$ ping ••••••••••••");
    assert_eq!(out[0].cols[..6], [0, 1, 2, 3, 4, 5]);
    assert_eq!(roles(&out[0])[..3], [(0..6, Role::Masked), (7..15, Role::Masked), (24..36, Role::Masked)]);
    assert_eq!(out[0].text_in(0..15), "••••••@••••••••");
    assert_never_shown(&out, &["deploy", "prod-api", "203.0.113"]);
}

#[test]
fn a_name_cut_by_the_wrap_is_hidden_on_both_rows() {
    let mut p = Presenter::new(look(true, false));
    let cols = 16;
    let screen = [row("ssh db-main.exam", cols), row("ple.net done", cols).wrapping()];
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[0].text, "ssh ••••••••••••");
    assert_eq!(out[1].text.trim_end(), "••••••• done");
    assert_never_shown(&out, &["db-main", "example", ".net"]);
}

#[test]
fn a_secret_never_reaches_the_row_and_a_click_shows_it() {
    let tok = token();
    let cols = 80;
    let screen = [row(&format!("export GITHUB_TOKEN={tok}"), cols), row("$ ", cols)];
    let s = Screen::new(&screen, cols);
    let mut p = Presenter::new(look(false, true));
    let out = p.present(&s);
    assert_eq!(out[0].text.trim_end(), format!("export GITHUB_TOKEN={}", "•".repeat(40)));
    assert_eq!(roles(&out[0]), [(20..60, Role::Secret { revealable: true })]);
    assert_never_shown(&out, &["Zx9Q", "ghp_"]);
    // A click on it shows it, from then on.
    assert!(!p.reveal(&s, 0, 3), "not on the secret");
    assert!(p.reveal(&s, 0, 25));
    assert!(p.present(&s)[0].text.contains(&tok));
    // Streaming hides it again, and a click shows nothing then.
    p.set_look(look(true, true));
    let out = p.present(&s);
    assert_eq!(roles(&out[0]), [(20..60, Role::Secret { revealable: false })]);
    assert_never_shown(&out, &["Zx9Q"]);
    assert!(!p.reveal(&s, 0, 25));
}

#[test]
fn shown_as_is_when_secrets_arent_hidden() {
    let tok = token();
    let mut p = Presenter::new(look(false, false));
    let out = present(&mut p, &rows(&[&format!("TOKEN={tok}")]));
    assert!(out[0].text.contains(&tok[..30]));
    assert!(out[0].spans.is_empty());
}

#[test]
fn a_token_tmux_drew_over_two_full_rows_is_covered_on_both() {
    let tok = token();
    let cols = 30;
    // No wrap mark: tmux redraws a long line as rows, the first full to the edge.
    let screen = [row(&format!("key {}", &tok[..26]), cols), row(&tok[26..], cols)];
    let mut p = Presenter::new(look(false, true));
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[0].text, format!("key {}", "•".repeat(26)));
    assert_eq!(out[1].text.trim_end(), "•".repeat(14));
    assert_eq!(roles(&out[1]), [(0..14, Role::Secret { revealable: true })]);
    assert_never_shown(&out, &["Zx9Q", "x9QZ", "9QZx", "QZx9"]);
    // Clicking either half shows the whole token.
    assert!(p.reveal(&Screen::new(&screen, cols), 1, 3));
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[1].text.trim_end(), &tok[26..]);
}

#[test]
fn a_private_keys_body_is_covered_by_whole_lines_and_never_shown_by_a_click() {
    let kind = ["OPENSSH", "PRIVATE", "KEY"].join(" ");
    let begin = format!("-----BEGIN {kind}-----");
    let screen = rows(&["$ cat id_ed25519", &begin, "NotARealKeyJustLettersAndDigits0123", "  MoreMadeUpLettersHere", "-----END OPENSSH PRIVATE KEY-----"]);
    let mut p = Presenter::new(look(false, true));
    let s = Screen::new(&screen, COLS);
    let out = p.present(&s);
    assert_eq!(out[2].text.trim_end(), "•".repeat(35));
    assert_eq!(out[3].text.trim_end(), format!("  {}", "•".repeat(21)));
    assert_eq!(roles(&out[3]), [(2..23, Role::Secret { revealable: false })]);
    assert_eq!(out[1].text.trim_end(), begin);
    assert!(!p.reveal(&s, 2, 5));
    assert_never_shown(&out, &["NotAReal", "MoreMadeUp"]);
    // Its BEGIN scrolled above the screen.
    let below = rows(&["MoreMadeUpLettersHere", "-----END OPENSSH PRIVATE KEY-----"]);
    let out = p.present(&Screen { key_open_above: true, ..Screen::new(&below, COLS) });
    assert_eq!(out[0].text.trim_end(), "•".repeat(21));
}

#[test]
fn a_url_password_streamer_mode_dotted_the_user_of_is_still_covered() {
    let mut p = Presenter::new(look(true, false));
    let out = present(&mut p, &rows(&["DATABASE_URL=postgres://deploy:s3cr3t@db"]));
    assert_eq!(out[0].text.trim_end(), "DATABASE_URL=postgres://••••••:••••••@db");
    assert!(roles(&out[0]).contains(&(31..37, Role::Secret { revealable: false })));
    assert_never_shown(&out, &["s3cr3t", "deploy"]);
}

#[test]
fn highlights_come_from_the_shown_text_and_stay_off_for_full_screen_programs() {
    let mut p = Presenter::new(look(false, true));
    let out = present(&mut p, &rows(&["build ERROR here"]));
    let paint = out[0].spans.iter().find_map(|s| if let Role::Paint(paint) = s.role { Some((s.cols.clone(), paint)) } else { None });
    let (cols, paint) = paint.expect("a highlight");
    assert_eq!((cols, paint.color, paint.bold), (6..11, HighlightColor::Red, true));
    let screen = rows(&["build ERROR here"]);
    let out = p.present(&Screen { own_screen: &[true], ..Screen::new(&screen, COLS) });
    assert!(out[0].spans.is_empty());
}

#[test]
fn links_are_found_but_never_over_something_hidden() {
    let mut p = Presenter::new(look(true, true));
    let out = present(&mut p, &rows(&["see /etc/hosts and /home/deploy/app.log", "https://app:s3cr3t@db.example.com/x"]));
    assert_eq!(out[0].links.len(), 1);
    assert_eq!((out[0].links[0].kind, out[0].links[0].target.as_str()), (LinkKind::Path, "/etc/hosts"));
    assert!(roles(&out[0]).contains(&(4..14, Role::Link(0))));
    assert!(out[1].links.is_empty());
    assert_never_shown(&out, &["deploy", "s3cr3t"]);
}

#[test]
fn a_link_wrapped_over_two_rows_is_one_link_on_both() {
    let cols = 20;
    let screen = [row("open https://example", cols), row(".com/docs/a now", cols).wrapping()];
    let mut p = Presenter::new(look(false, true));
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[0].links[0].target, "https://example.com/docs/a");
    assert_eq!(out[1].links[0].target, "https://example.com/docs/a");
    assert!(roles(&out[0]).contains(&(5..20, Role::Link(0))));
    assert!(roles(&out[1]).contains(&(0..11, Role::Link(0))));
}

#[test]
fn a_programs_own_link_opens_only_if_it_hides_nothing() {
    let tok = token();
    let mut screen = rows(&["docs here", "token link", "server link"]);
    screen[0].hyperlinks.push(Hyperlink { cols: 0..4, uri: "https://example.com/docs".into() });
    screen[1].hyperlinks.push(Hyperlink { cols: 0..5, uri: format!("https://example.com/?t={tok}") });
    screen[2].hyperlinks.push(Hyperlink { cols: 0..6, uri: "https://db-main.example.net/".into() });
    let mut p = Presenter::new(look(false, true));
    let out = present(&mut p, &screen);
    assert_eq!(out[0].links[0].target, "https://example.com/docs");
    assert!(roles(&out[0]).contains(&(0..4, Role::Link(0))));
    assert!(out[1].links.is_empty());
    assert_eq!(out[2].links.len(), 1);
    p.set_look(look(true, true));
    let out = present(&mut p, &screen);
    assert!(out[2].links.is_empty());
    assert_never_shown(&out, &["Zx9Q", "db-main"]);
}

#[test]
fn find_marks_matches_across_a_wrap_and_the_current_one() {
    let cols = 10;
    let screen = [row("first  ERR", cols), row("OR here", cols).wrapping(), row("ERROR", cols)];
    let mut p = Presenter::new(look(false, true));
    let find = Some(FindMarks { query: "error", current: Some((2, 0)) });
    let out = p.present(&Screen { find, ..Screen::new(&screen, cols) });
    let found = |r: &PresentedRow| r.spans.iter().filter(|s| matches!(s.role, Role::Found { .. })).map(|s| (s.cols.clone(), s.role)).collect::<Vec<_>>();
    assert_eq!(found(&out[0]), [(7..10, Role::Found { current: false })]);
    assert_eq!(found(&out[1]), [(0..2, Role::Found { current: false })]);
    assert_eq!(found(&out[2]), [(0..5, Role::Found { current: true })]);
}

#[test]
fn find_cant_find_what_is_hidden() {
    let tok = token();
    let mut p = Presenter::new(look(false, true));
    let screen = rows(&[&format!("TOKEN={tok}")]);
    let out = p.present(&Screen { find: Some(FindMarks { query: "Zx9Q", current: None }), ..Screen::new(&screen, COLS) });
    assert!(!out[0].spans.iter().any(|s| matches!(s.role, Role::Found { .. })));
}

#[test]
fn wide_characters_keep_their_columns_and_a_hidden_mark_goes_with_its_letter() {
    let mut p = Presenter::new(Look { names: names(&["数据库", "ab\u{301}c"]), ..look(true, false) });
    let out = present(&mut p, &rows(&["on 数据库 now", "xab\u{301}cx"]));
    assert_eq!(out[0].text.trim_end(), "on ••• now");
    assert_eq!(out[0].cols[..10], [0, 1, 2, 3, 5, 7, 9, 10, 11, 12]);
    assert_eq!(roles(&out[0]), [(3..9, Role::Masked)]);
    assert_eq!(out[1].text.trim_end(), "x•••x");
    assert_eq!(out[1].cols[..5], [0, 1, 2, 3, 4]);
    assert_never_shown(&out, &["数", "据", "\u{301}"]);
}

#[test]
fn the_same_rows_come_back_as_the_same_rows() {
    let mut p = Presenter::new(look(true, true));
    let screen = rows(&["deploy@prod-api:~$ ls", "a", "b"]);
    let first = present(&mut p, &screen);
    let again = present(&mut p, &screen);
    assert!(first.iter().zip(&again).all(|(a, b)| Arc::ptr_eq(a, b)));
    // A row that moved keeps what was made of it.
    let moved = present(&mut p, &rows(&["b", "deploy@prod-api:~$ ls"]));
    assert!(Arc::ptr_eq(&moved[1], &first[0]));
    // New settings make it again.
    p.set_look(look(false, true));
    let plain = present(&mut p, &screen);
    assert!(!Arc::ptr_eq(&plain[0], &first[0]));
    assert!(plain[0].text.starts_with("deploy@prod-api"));
}

#[test]
fn history_for_find_and_text_shown_elsewhere_are_made_safe_the_same_way() {
    let tok = token();
    let look = look(true, true);
    let rows = [FindRow::new("ssh db-main.ex", false), FindRow::new(format!("ample.net {tok}"), true)];
    let safe = look.safe_rows(&rows);
    assert_eq!(safe[0].text.chars().count(), rows[0].text.chars().count());
    assert_eq!(safe[1].text.chars().count(), rows[1].text.chars().count());
    assert!(safe[1].wrapped);
    let all = format!("{}{}", safe[0].text, safe[1].text);
    assert!(!all.contains("db-main") && !all.contains("Zx9Q") && !all.contains("example"), "{all}");
    assert_eq!(look.safe_text(&format!("deploy said {tok}")), format!("•••••• said {}", "•".repeat(40)));
}

// What the pass hides when rows only may be one line, when a name sits inside
// a secret, and when an edge of the screen cuts a line.

#[test]
fn an_address_after_a_row_full_to_the_edge_is_dotted_as_its_own_line() {
    // No wrap marks, as under tmux: the full row is taken to go on, and each line is read alone too.
    let mut p = Presenter::new(look(true, false));
    let cols = 20;
    let screen = [row("abcdefghijklmnopqrst", cols), row("198.51.100.7 up", cols)];
    assert_eq!(p.present(&Screen::new(&screen, cols))[1].text.trim_end(), "•••.••.•••.• up");
    // The address at the end of a full row.
    let cols = 17;
    let screen = [row("from 198.51.100.7", cols), row("Welcome back", cols)];
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[0].text, "from •••.••.•••.•");
    assert_eq!(out[1].text.trim_end(), "Welcome back");
    // An IPv6 address the same way.
    let cols = 20;
    let screen = [row("abcdefghijklmnopqrst", cols), row("2001:db8::1 up", cols)];
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[1].text.trim_end(), "••••:•••::• up");
    assert_never_shown(&out, &["198.51", "2001", "db8"]);
}

#[test]
fn a_token_at_the_start_of_the_row_after_a_full_one_is_covered() {
    let tok = token();
    let screen = rows(&[&"abcdefghij".repeat(4), &tok]);
    let mut p = Presenter::new(look(false, true));
    let out = present(&mut p, &screen);
    assert_eq!(out[0].text, "abcdefghij".repeat(4));
    assert_eq!(out[1].text.trim_end(), "•".repeat(40));
    assert_never_shown(&out, &["Zx9Q"]);
}

#[test]
fn a_server_name_inside_a_secret_doesnt_keep_the_secret_from_being_found() {
    let hosts = [HostNames { name: "api.example.net", hostname: "198.51.100.40", user: None }, HostNames { name: "web", hostname: "198.51.100.41", user: None }];
    let streaming = Look { names: Arc::new(Names::new(hosts, [])), ..look(true, false) };
    let tok = "ghp_Ab3Kq9m2Lp7XWEBz4Rt8Vn6Yc1Hd5Fg0JkQx7";
    let mut p = Presenter::new(streaming.clone());
    let out = present(&mut p, &rows(&["API_KEY=Qm7vT2xLp9", tok]));
    assert_eq!(out[0].text.trim_end(), format!("•••_KEY={}", "•".repeat(10)));
    assert_eq!(out[1].text.trim_end(), "•".repeat(tok.len()));
    assert_never_shown(&out, &["Qm7vT2xLp9", "Ab3Kq9", "z4Rt8", "JkQx7"]);
    // Text shown elsewhere and history for a find get the same.
    assert_eq!(streaming.safe_text("API_KEY=Qm7vT2xLp9"), format!("•••_KEY={}", "•".repeat(10)));
    assert_eq!(streaming.safe_text(tok), "•".repeat(tok.len()));
    assert_eq!(streaming.safe_rows(&[FindRow::new(tok, false)])[0].text, "•".repeat(tok.len()));
}

#[test]
fn a_token_whose_start_scrolled_off_the_top_is_still_covered() {
    let tok = token();
    let cols = 20;
    let line = format!("export GITHUB_TOKEN={tok}");
    let all = [row(&line[..20], cols), row(&line[20..40], cols).wrapping(), row(&line[40..], cols).wrapping()];
    let mut p = Presenter::new(look(false, true));
    // The rows above that its line runs over, given as the top edge's context.
    let screen = Screen { edges: Edges { above: &all[..2], ..Edges::default() }, ..Screen::new(&all[2..], cols) };
    let out = p.present(&screen);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].text, "•".repeat(20));
    assert_eq!(roles(&out[0]), [(0..20, Role::Secret { revealable: true })]);
    // A click shows it, as on any secret.
    assert!(p.reveal(&screen, 0, 3));
    assert_eq!(p.present(&screen)[0].text, &tok[20..]);
    // Without them, a row that goes on from above is hidden whole, and can't be shown.
    let mut p = Presenter::new(look(false, true));
    let alone = Screen::new(&all[2..], cols);
    assert_eq!(p.present(&alone)[0].text, "•".repeat(20));
    assert!(!p.reveal(&alone, 0, 3));
    // Nor when the context was cut short.
    let cut = Screen { edges: Edges { above: &all[1..2], more_above: true, ..Edges::default() }, ..Screen::new(&all[2..], cols) };
    assert_eq!(p.present(&cut)[0].text, "•".repeat(20));
    // Secrets shown: nothing hidden.
    let mut p = Presenter::new(look(false, false));
    assert_eq!(p.present(&alone)[0].text, &tok[20..]);
}

#[test]
fn a_token_cut_by_the_bottom_while_scrolled_up_is_covered() {
    let tok = token();
    let cols = 20;
    let all = [row("$ cat token.txt", cols), row(&tok[..20], cols), row(&tok[20..], cols).wrapping()];
    let mut p = Presenter::new(look(false, true));
    let screen = Screen { edges: Edges { below: &all[2..], ..Edges::default() }, ..Screen::new(&all[..2], cols) };
    let out = p.present(&screen);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].text.trim_end(), "$ cat token.txt");
    assert_eq!(out[1].text, "•".repeat(20));
    // The line goes on past what could be read: hidden whole.
    let cut = Screen { edges: Edges { more_below: true, ..Edges::default() }, ..Screen::new(&all[..2], cols) };
    let out = p.present(&cut);
    assert_eq!(out[1].text, "•".repeat(20));
    assert_never_shown(&out, &["ghp_", "Zx9Q"]);
}

#[test]
fn a_key_in_a_long_json_line_stays_covered_once_its_start_scrolled_off() {
    let kind = ["PRIVATE", "KEY"].join(" ");
    let body = "NotARealKeyJustMadeUpLettersAndDigits0123456789".repeat(20);
    let line = format!(r#"{{"type": "service_account", "private_key": "-----BEGIN {kind}-----\n{body}\n-----END {kind}-----\n", "client_email": "bot@example.net"}}"#);
    let cols = 40;
    let chars: Vec<char> = line.chars().collect();
    let all: Vec<RowText> = chars
        .chunks(cols as usize)
        .enumerate()
        .map(|(i, c)| {
            let r = row(&c.iter().collect::<String>(), cols);
            if i > 0 {
                r.wrapping()
            } else {
                r
            }
        })
        .collect();
    assert!(all.len() > 14);
    let mut p = Presenter::new(look(false, true));
    // Scrolled up into the line: the rows above and below it on screen are given.
    let screen = Screen { edges: Edges { above: &all[..2], below: &all[12..], ..Edges::default() }, ..Screen::new(&all[2..12], cols) };
    let out = p.present(&screen);
    assert_eq!(out.len(), 10);
    assert_never_shown(&out, &["NotAReal", "MadeUp", "0123456"]);
    assert!(out.iter().all(|r| r.spans.iter().all(|s| s.role == Role::Secret { revealable: true })));
    // Without the rows above, the line is hidden whole.
    let out = p.present(&Screen { edges: Edges { below: &all[12..], ..Edges::default() }, ..Screen::new(&all[2..12], cols) });
    assert_never_shown(&out, &["NotAReal", "MadeUp", "0123456"]);
}

#[test]
fn a_keys_short_last_line_and_its_wrapped_tails_are_covered() {
    let kind = ["OPENSSH", "PRIVATE", "KEY"].join(" ");
    let (begin, end) = (format!("-----BEGIN {kind}-----"), format!("-----END {kind}-----"));
    let body = "NotARealKeyJustLettersAndDigits0123456789abcdefghijklmnopqrstuvw";
    assert_eq!(body.len(), 64);
    let cols = 50;
    let screen = [row(&begin, cols), row(&body[..50], cols), row(&body[50..], cols).wrapping(), row("Qm7vT2xLp9A=", cols), row(&end, cols)];
    let mut p = Presenter::new(look(false, true));
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[1].text, "•".repeat(50));
    assert_eq!(out[2].text.trim_end(), "•".repeat(14));
    assert_eq!(out[3].text.trim_end(), "•".repeat(12));
    assert_eq!(out[4].text.trim_end(), end);
    assert_never_shown(&out, &[&body[50..], "Qm7vT2xLp9"]);
    // The same rows read for a find over the history.
    let history: Vec<FindRow> = screen.iter().map(|r| FindRow::new(r.trimmed(), r.wrapped)).collect();
    let safe = look(false, true).safe_rows(&history);
    assert_eq!(safe[2].text, "•".repeat(14));
    assert_eq!(safe[3].text, "•".repeat(12));
}

#[test]
fn highlights_links_and_find_go_line_by_line_past_a_full_row() {
    let mut p = Presenter::new(look(false, true));
    let painted = |r: &PresentedRow| r.spans.iter().filter(|s| matches!(s.role, Role::Paint(_))).map(|s| s.cols.clone()).collect::<Vec<_>>();
    let cols = 20;
    let screen = [row("abcdefghijklmnopqrst", cols), row("ERROR: disk full", cols)];
    let paint = painted(&p.present(&Screen::new(&screen, cols))[1]);
    assert_eq!((paint.len(), paint.first()), (1, Some(&(0..5))));
    // A path on the row after a full one is that path, not the word before it and the path.
    let cols = 21;
    let screen = [row("see the nginx docs at", cols), row("/etc/nginx/nginx.conf", cols)];
    let out = p.present(&Screen::new(&screen, cols));
    assert!(out[0].links.is_empty());
    assert_eq!(out[1].links.len(), 1);
    assert_eq!(out[1].links[0].target, "/etc/nginx/nginx.conf");
    // A web link that ends at the last column, and a word after it.
    let cols = 26;
    let screen = [row("https://example.com/docs/a", cols), row("next steps", cols)];
    let out = p.present(&Screen::new(&screen, cols));
    assert_eq!(out[0].links[0].target, "https://example.com/docs/a");
    assert!(out[1].links.is_empty());
    // Find doesn't match across that guess either, as a find over the history wouldn't.
    let cols = 10;
    let screen = [row("first  ERR", cols), row("OR here", cols)];
    let out = p.present(&Screen { find: Some(FindMarks { query: "error", current: None }), ..Screen::new(&screen, cols) });
    assert!(!out.iter().any(|r| r.spans.iter().any(|s| matches!(s.role, Role::Found { .. }))));
}

#[test]
fn only_rows_a_full_screen_program_wrote_go_without_highlights() {
    let mut p = Presenter::new(look(false, true));
    let painted = |r: &PresentedRow| r.spans.iter().any(|s| matches!(s.role, Role::Paint(_)));
    let screen = rows(&["make: ERROR: x", "Error: the agent's own words", "ERROR again"]);
    let out = p.present(&Screen { own_screen: &[false, true], ..Screen::new(&screen, COLS) });
    assert!(painted(&out[0]), "written before the agent: keeps its colours");
    assert!(!painted(&out[1]), "written by the agent: plain");
    assert!(painted(&out[2]), "rows past the flags count as not");
}

#[test]
fn a_programs_link_over_other_text_names_where_it_goes_and_asks_first() {
    let mut screen = rows(&["github.com/acme/app", "https://example.com/docs", "example.com/guide/", "notes.md"]);
    screen[0].hyperlinks.push(Hyperlink { cols: 0..19, uri: "https://evil.example/login".into() });
    screen[1].hyperlinks.push(Hyperlink { cols: 0..24, uri: "https://example.com/docs".into() });
    screen[2].hyperlinks.push(Hyperlink { cols: 0..18, uri: "https://example.com/guide".into() });
    screen[3].hyperlinks.push(Hyperlink { cols: 0..8, uri: "file://web-7731/srv/app/notes.md".into() });
    let mut p = Presenter::new(look(false, true));
    let out = present(&mut p, &screen);
    let program = |r: &PresentedRow| r.links.iter().find(|l| l.from_program).cloned().expect("a program's link");
    // Text that isn't where the link goes: the hint names the place, and a click asks.
    let evil = program(&out[0]);
    assert_eq!((evil.kind, evil.target.as_str(), evil.disguised), (LinkKind::Url, "https://evil.example/login", true));
    assert_eq!(evil.click(false, 1, false), LinkClick::AskAfterWait);
    assert_eq!(evil.click(true, 1, false), LinkClick::AskNow);
    assert_eq!(evil.click(false, 2, false), LinkClick::Nothing);
    assert!(evil.hint().contains("https://evil.example/login"));
    // Text that is the link, with or without its scheme or last slash: opens as any link.
    for r in [&out[1], &out[2]] {
        let ok = program(r);
        assert!(!ok.disguised, "{}", ok.target);
        assert_eq!(ok.click(false, 1, false), LinkClick::OpenAfterWait);
        assert_eq!(ok.hint(), sshee_term::links::OPEN_HINT);
    }
    // A file's name over its path: the hint names the path; Files opens it without asking.
    let file = program(&out[3]);
    assert_eq!((file.kind, file.target.as_str(), file.disguised), (LinkKind::Path, "/srv/app/notes.md", true));
    assert!(file.hint().contains("/srv/app/notes.md"));
    assert_eq!(file.click(false, 1, false), LinkClick::OpenAfterWait);
}

#[test]
fn a_programs_link_the_terminal_wrapped_is_one_link_that_shows_its_target() {
    let cols = 20;
    let url = "https://example.com/docs/setup";
    let mut screen = [row(&url[..20], cols), row(&url[20..], cols).wrapping()];
    screen[0].hyperlinks.push(Hyperlink { cols: 0..20, uri: url.into() });
    screen[1].hyperlinks.push(Hyperlink { cols: 0..10, uri: url.into() });
    let mut p = Presenter::new(look(false, true));
    let out = p.present(&Screen::new(&screen, cols));
    for r in &out {
        let link = r.links.iter().find(|l| l.from_program).expect("the link on both rows");
        assert!(!link.disguised);
        assert_eq!(link.target, url);
    }
}

#[test]
fn copying_or_showing_a_keys_lines_one_at_a_time_still_gets_dots() {
    let look = look(false, true);
    let kind = ["OPENSSH", "PRIVATE", "KEY"].join(" ");
    let lines = [format!("-----BEGIN {kind}-----"), "NotARealKeyJustLettersAndDigits0123".to_string(), "Qm7vT2xLp9A=".to_string(), format!("-----END {kind}-----"), "$ ls".to_string()];
    let safe = look.safe_lines(&lines, false);
    assert_eq!(safe[1], "•".repeat(35));
    assert_eq!(safe[2], "•".repeat(12));
    assert_eq!((safe[0].as_str(), safe[4].as_str()), (lines[0].as_str(), "$ ls"));
    // What a copy goes through, one line at a time, in order.
    let copy = look.copy_presenter(false);
    fn takes(_: &dyn sshee_term::select::CopyPresenter) {}
    takes(&copy);
    let copied: Vec<String> = lines.iter().map(|l| copy(l)).collect();
    assert_eq!(copied, safe);
    // A key that began above the first line copied.
    let copy = look.copy_presenter(true);
    assert_eq!(copy("NotARealKeyJustLettersAndDigits0123"), "•".repeat(35));
    assert_eq!(copy(&lines[3]), lines[3]);
    assert_eq!(copy("$ ls"), "$ ls");
}

/// A secret printed a few characters at a time (a slow link, a read boundary, an agent
/// streaming its reply): at no point does the part that came show, on screen or in a copy.
#[test]
fn a_secret_printed_in_pieces_never_shows_any_of_it() {
    use sshee_term::engine::{row_texts, EngineConfig, Size};
    use sshee_term::model::Model;
    let tok = token();
    // (the line, where in it begins what must never show, that part): a token's prefix is the
    // same for every token, its body isn't.
    let json = format!("{{\"token\": \"{tok}\"}}\r\n");
    let bare = format!("echo {tok} done\r\n");
    let named = "{\"password\": \"correct-horse-battery\"}\r\n".to_string();
    let cases = [(json.clone(), json.find("ghp_").unwrap() + 4, &tok[4..]), (bare.clone(), bare.find("ghp_").unwrap() + 4, &tok[4..]), (named.clone(), named.find("correct").unwrap(), "correct-horse-battery")];
    for (line, from, secret) in cases {
        let m = Model::new(Size::new(COLS, 4), &EngineConfig::default());
        let mut p = Presenter::new(look(false, true));
        for cut in 1..line.len() {
            m.feed(&line.as_bytes()[cut - 1..cut]);
            let f = m.frame(true);
            let rows: Vec<_> = f.rows.into_iter().map(|r| r.row).collect();
            let texts = row_texts(&rows, false);
            let out = p.present(&Screen::new(&texts, COLS));
            let came = cut.saturating_sub(from).min(secret.len());
            // Every character of the line is one column here: what is drawn from `from` on.
            let shown: String = out.iter().map(|r| r.text.as_str()).collect::<String>().chars().skip(from).collect();
            // A copy reads the rows a line wrapped over as that one line.
            let l = look(false, true);
            let copy = l.copy_presenter(false);
            let copied: String = copy(&texts.iter().map(|t| t.text.as_str()).collect::<String>()).chars().skip(from).collect();
            for c in secret[..came].chars() {
                assert!(!shown.contains(c), "{c:?} shown after {cut} bytes of {line:?}: {shown}");
                assert!(!copied.contains(c), "{c:?} copied after {cut} bytes of {line:?}: {copied}");
            }
        }
    }
}

/// A line that keeps growing (a server printing one long line, or a progress bar without a
/// line break) is a new group every snapshot: the cache keeps only rows that were on screen,
/// and starts over before it holds more than a few screens of them.
#[test]
fn a_growing_long_line_keeps_the_cache_small() {
    let cols = 40;
    let mut p = Presenter::new(look(false, true));
    let mut all: Vec<RowText> = Vec::new();
    for n in 0..300 {
        let mut r = row(&format!("{n:0>40}"), cols);
        if n > 0 {
            r = r.wrapping();
        }
        all.push(r);
        let top = all.len().saturating_sub(10);
        let screen = Screen { edges: Edges { above: &all[..top], ..Edges::default() }, ..Screen::new(&all[top..], cols) };
        assert_eq!(p.present(&screen).len(), all.len() - top);
        assert!(p.cached_rows() <= 512, "{} rows cached after {n} snapshots", p.cached_rows());
    }
}
