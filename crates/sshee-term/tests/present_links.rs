//! 0.10's `src/lib/paths.test.ts` for `findLinks` and `resolvePath`, one test
//! for one test, same inputs and expected results. (`pathCrumbs` is the Files
//! address bar's, not the terminal's, and isn't ported here.)

use sshee_term::links::{find_links, hyperlink, link_click, resolve_path, LinkClick, LinkKind, TextLink};

fn targets(text: &str) -> Vec<String> {
    find_links(text).into_iter().map(|l| l.target).collect()
}

fn first(text: &str) -> TextLink {
    find_links(text).into_iter().next().expect("a link")
}

const NONE: [&str; 0] = [];

#[test]
fn finds_absolute_home_and_dot_paths() {
    assert_eq!(targets("wrote /srv/app/docs/SETUP.md"), ["/srv/app/docs/SETUP.md"]);
    assert_eq!(targets("see ~/notes.md and ./run.sh or ../up"), ["~/notes.md", "./run.sh", "../up"]);
}

#[test]
fn finds_relative_paths_with_a_folder_and_a_file() {
    assert_eq!(targets("Edited src/lib/paths.ts"), ["src/lib/paths.ts"]);
    assert_eq!(targets("in packages/web/"), ["packages/web/"]);
}

#[test]
fn leaves_words_that_only_look_like_paths() {
    assert_eq!(targets("and/or TCP/IP 1/2 @types/node README.md / //"), NONE);
}

#[test]
fn takes_line_numbers_off_and_keeps_them() {
    let text = "error at src/main.rs:12:5: oops";
    let l = first(text);
    assert_eq!((l.kind, l.target.as_str(), l.line), (LinkKind::Path, "src/main.rs", Some(12)));
    assert_eq!(&text[l.range], "src/main.rs:12:5");
}

#[test]
fn keeps_a_range_of_lines() {
    let text = "Read ~/notes/plan.md:242-296 now";
    let l = first(text);
    assert_eq!((l.target.as_str(), l.line, l.end_line), ("~/notes/plan.md", Some(242), Some(296)));
    assert_eq!(&text[l.range], "~/notes/plan.md:242-296");
    let l = first("docs/a.md#L10-L20");
    assert_eq!((l.target.as_str(), l.line, l.end_line), ("docs/a.md", Some(10), Some(20)));
    let l = first("docs/a.md#L7");
    assert_eq!((l.target.as_str(), l.line, l.end_line), ("docs/a.md", Some(7), None));
    // A range that runs backwards is just its first line.
    let l = first("src/x.ts:30-10");
    assert_eq!((l.target.as_str(), l.line, l.end_line), ("src/x.ts", Some(30), None));
}

#[test]
fn drops_sentence_punctuation_and_quotes() {
    assert_eq!(targets("Saved to '/etc/nginx/nginx.conf'."), ["/etc/nginx/nginx.conf"]);
    assert_eq!(targets("(see /var/log/syslog), then"), ["/var/log/syslog"]);
    assert_eq!(targets("--config=/etc/app.toml"), ["/etc/app.toml"]);
}

#[test]
fn finds_web_links() {
    let links: Vec<(LinkKind, String)> = find_links("Open http://localhost:3000/ or https://example.com/docs.").into_iter().map(|l| (l.kind, l.target)).collect();
    assert_eq!(links, [(LinkKind::Url, "http://localhost:3000/".to_string()), (LinkKind::Url, "https://example.com/docs".to_string())]);
}

#[test]
fn reports_where_each_link_is() {
    let text = "x /etc/hosts y";
    assert_eq!(&text[first(text).range], "/etc/hosts");
}

#[test]
fn makes_relative_paths_absolute_against_the_terminals_folder() {
    assert_eq!(resolve_path("src/main.rs", "/srv/app"), "/srv/app/src/main.rs");
    assert_eq!(resolve_path("../x/y.md", "/srv/app/"), "/srv/x/y.md");
    assert_eq!(resolve_path("./run.sh", "/"), "/run.sh");
}

#[test]
fn keeps_absolute_and_home_paths() {
    assert_eq!(resolve_path("/etc//hosts", "/srv"), "/etc/hosts");
    assert_eq!(resolve_path("~/a/../b.md", "/srv"), "~/b.md");
}

// Beyond 0.10's tests.

#[test]
fn positions_are_right_after_wide_characters() {
    let text = "файл ~/заметки/план.md готов";
    assert_eq!(&text[first(text).range], "~/заметки/план.md");
    assert_eq!(targets("see file:///srv/app/run.sh"), ["/srv/app/run.sh"]);
}

#[test]
fn opens_only_web_and_file_links_a_program_marks() {
    assert_eq!(hyperlink("https://example.com/a"), Some((LinkKind::Url, "https://example.com/a".to_string())));
    assert_eq!(hyperlink("file://web-7731/srv/my%20notes.md"), Some((LinkKind::Path, "/srv/my notes.md".to_string())));
    assert_eq!(hyperlink("javascript:alert(1)"), None);
    assert_eq!(hyperlink("ssh://db-main"), None);
}

#[test]
fn ctrl_click_opens_at_once_and_a_plain_click_waits() {
    assert_eq!(link_click(true, 1, false), LinkClick::OpenNow);
    assert_eq!(link_click(true, 2, true), LinkClick::OpenNow);
    assert_eq!(link_click(false, 1, false), LinkClick::OpenAfterWait);
    assert_eq!(link_click(false, 2, false), LinkClick::Nothing);
    assert_eq!(link_click(false, 1, true), LinkClick::Nothing);
}

#[test]
fn a_link_that_must_be_asked_about_asks_on_the_same_clicks() {
    assert_eq!(link_click(true, 1, false).asking_first(), LinkClick::AskNow);
    assert_eq!(link_click(false, 1, false).asking_first(), LinkClick::AskAfterWait);
    assert_eq!(link_click(false, 2, false).asking_first(), LinkClick::Nothing);
    assert_eq!(link_click(false, 1, true).asking_first(), LinkClick::Nothing);
    assert!(sshee_term::links::ask_to_open("https://login.example").contains("https://login.example"));
    assert!(sshee_term::links::hidden_target_hint("https://login.example").contains("https://login.example"));
}
