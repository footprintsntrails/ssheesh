//! 0.10's `src/lib/away.test.ts` (`linesAfter`), one test for one test, same
//! inputs and expected results, and the parts it didn't test (`anchorLines`,
//! `buildRecap`) over rows made here.

use sshee_term::away::{anchor_lines, build_recap, cursor_line, lines_after, RowList, ANCHOR_LINES};
use sshee_term::present::{Look, RowText};

const HISTORY: [&str; 8] = ["$ deploy", "pulling", "done", "$ ", "new 1", "new 2", "$ ", ""];

#[test]
fn returns_what_came_after_the_last_screen_lines() {
    assert_eq!(lines_after(&HISTORY, &["pulling", "done"]), Some(vec!["$ ".into(), "new 1".into(), "new 2".into(), "$ ".into()]));
}

#[test]
fn ignores_trailing_spaces_and_blank_lines() {
    assert_eq!(lines_after(&["a  ", "b", "c", "", ""], &["a", "b"]), Some(vec!["c".to_string()]));
}

#[test]
fn gives_up_when_the_screen_lines_arent_in_the_scrollback() {
    assert_eq!(lines_after(&HISTORY, &["something else"]), None);
}

/// Rows padded to `cols` as the grid has them.
fn rows(cols: usize, lines: &[(&str, bool)]) -> Vec<RowText> {
    lines
        .iter()
        .map(|(t, wrapped)| {
            let mut r = RowText::plain(&format!("{t:<cols$}"));
            r.wrapped = *wrapped;
            r
        })
        .collect()
}

#[test]
fn anchors_are_the_last_finished_lines_with_wraps_joined_and_sshs_goodbye_left_out() {
    let screen = rows(12, &[("$ deploy", false), ("pulling the ", false), ("image", true), ("done", false), ("Broken pipe", false), ("$ ", false)]);
    let src = RowList { cols: 12, rows: &screen };
    assert_eq!(anchor_lines(&src, 5, ANCHOR_LINES), ["$ deploy", "pulling the image", "done"]);
    assert_eq!(cursor_line(&src, 5), "$");
}

#[test]
fn a_row_full_to_the_edge_goes_on_in_the_next_as_tmux_draws_it() {
    let screen = rows(10, &[("0123456789", false), ("abc", false), ("$ ", false)]);
    assert_eq!(anchor_lines(&RowList { cols: 10, rows: &screen }, 2, ANCHOR_LINES), ["0123456789abc"]);
}

#[test]
fn a_blank_line_ends_the_anchor_once_it_has_lines() {
    let screen = rows(10, &[("old", false), ("", false), ("new", false), ("", false), ("$ ", false)]);
    assert_eq!(anchor_lines(&RowList { cols: 10, rows: &screen }, 4, ANCHOR_LINES), ["new"]);
}

#[test]
fn builds_the_recap_with_its_problems_counted() {
    let history = "$ deploy\r\npulling\r\ndone\r\n$ make\r\nERROR: disk full\r\nwarning: low memory\r\n$ \r\n";
    let anchor = vec!["pulling".to_string(), "done".to_string()];
    let recap = build_recap(history, &anchor, "$ make", 1000, 2000).expect("a recap");
    assert_eq!(recap.lines, ["ERROR: disk full", "warning: low memory", "$ "]);
    assert_eq!((recap.errors, recap.warnings, recap.since, recap.until), (1, 1, 1000, 2000));
    assert_eq!(build_recap("$ deploy\npulling\ndone\n", &anchor, "", 1, 2), None);
}

#[test]
fn the_card_shows_a_key_printed_while_away_as_dots() {
    // A made-up key, its markers put together here so no scanner takes the file for a real one.
    let kind = ["OPENSSH", "PRIVATE", "KEY"].join(" ");
    let history = format!("pulling\ndone\n$ cat id_ed25519\n-----BEGIN {kind}-----\nNotARealKeyJustLettersAndDigits0123\nQm7vT2xLp9A=\n-----END {kind}-----\n$ \n");
    let recap = build_recap(&history, &["pulling".to_string(), "done".to_string()], "", 1, 2).expect("a recap");
    let look = Look { hide_secrets: true, ..Default::default() };
    // The card colours each line on its own, so it takes them one by one: made safe together.
    let shown = recap.shown_lines(&look);
    assert_eq!(shown.len(), recap.lines.len());
    assert_eq!(shown[2], "•".repeat(35));
    assert_eq!(shown[3], "•".repeat(12));
    assert_eq!((shown[0].as_str(), shown[5].as_str()), ("$ cat id_ed25519", "$ "));
    assert!(!shown.iter().any(|l| l.contains("NotAReal") || l.contains("Qm7v")));
    // Secrets shown in Settings: the card shows them too, as the screen does.
    assert_eq!(recap.shown_lines(&Look::default()), recap.lines);
}
