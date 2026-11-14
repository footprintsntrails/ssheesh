//! 0.10's `src/lib/termFind.test.ts`, one test for one test, same inputs and
//! expected results.

use sshee_term::find::{find_rows, match_pieces, nearest_match, rows_from_capture, takes_find_key, FindMatch, FindRow, FIND_LIMIT};

fn plain(text: &[&str]) -> Vec<FindRow> {
    text.iter().map(|t| FindRow::new(*t, false)).collect()
}

fn m(row: usize, start: usize, length: usize) -> FindMatch {
    FindMatch { row, start, length }
}

#[test]
fn finds_every_match_case_aside_in_russian_too() {
    assert_eq!(find_rows(&plain(&["Error here", "no", "an error and ERROR"]), "error"), [m(0, 0, 5), m(2, 3, 5), m(2, 13, 5)]);
    assert_eq!(find_rows(&plain(&["Ошибка в файле"]), "ошибка"), [m(0, 0, 6)]);
}

#[test]
fn takes_what_you_type_literally() {
    assert_eq!(find_rows(&plain(&["a.b axb", "(x)"]), "a.b"), [m(0, 0, 3)]);
    assert_eq!(find_rows(&plain(&["a.b axb", "(x)"]), "(x)"), [m(1, 0, 3)]);
    assert_eq!(find_rows(&plain(&[r"[1] \d"]), r"\d"), [m(0, 4, 2)]);
}

#[test]
fn finds_nothing_for_nothing() {
    assert_eq!(find_rows(&plain(&["text"]), ""), []);
    assert_eq!(find_rows(&[], "x"), []);
}

#[test]
fn finds_a_match_that_a_long_line_wraps_in_two() {
    let rows = [FindRow::new("first", false), FindRow::new("abcdeERR", false), FindRow::new("ORfgh", true), FindRow::new("last ERROR", false)];
    let found = find_rows(&rows, "error");
    assert_eq!(found, [m(1, 5, 5), m(3, 5, 5)]);
    assert_eq!(match_pieces(&rows, found[0]), [m(1, 5, 3), m(2, 0, 2)]);
    assert_eq!(match_pieces(&rows, found[1]), [m(3, 5, 5)]);
}

#[test]
fn places_a_match_on_the_right_row_further_along_a_wrapped_line() {
    let rows = [FindRow::new("aaaa", false), FindRow::new("aaaa", true), FindRow::new("aXaa", true)];
    assert_eq!(find_rows(&rows, "x"), [m(2, 1, 1)]);
}

#[test]
fn stops_at_the_limit() {
    let long = "x".repeat(FIND_LIMIT + 50);
    assert_eq!(find_rows(&plain(&[&long]), "x").len(), FIND_LIMIT);
}

#[test]
fn joins_a_row_that_fills_the_width_to_the_next() {
    assert_eq!(rows_from_capture("12345\n678\nabc\n", 5), [FindRow::new("12345", false), FindRow::new("678", true), FindRow::new("abc", false)]);
}

#[test]
fn keeps_empty_rows_so_rows_count_from_the_bottom_stay_right() {
    let texts: Vec<String> = rows_from_capture("a\n\n\n", 80).into_iter().map(|r| r.text).collect();
    assert_eq!(texts, ["a", "", ""]);
}

// nearestMatch: rows 0..9; matches on rows 1, 5 and 8.
fn matches() -> Vec<FindMatch> {
    [1, 5, 8].map(|row| m(row, 0, 1)).to_vec()
}

#[test]
fn starts_at_the_newest_match_in_view_or_above_it() {
    assert_eq!(nearest_match(&matches(), 10, 0), Some(2));
    assert_eq!(nearest_match(&matches(), 10, 2), Some(1));
    assert_eq!(nearest_match(&matches(), 10, 8), Some(0));
}

#[test]
fn falls_back_to_the_newest_when_all_are_below_the_place() {
    assert_eq!(nearest_match(&matches(), 10, 9), Some(2));
    assert_eq!(nearest_match(&[], 10, 0), None);
}

// Beyond 0.10's tests.

#[test]
fn a_row_of_wide_characters_that_fills_the_width_goes_on() {
    // 40 wide characters fill 80 columns: 0.10 counted 40 and missed it.
    let row = "中".repeat(40);
    let rows = rows_from_capture(&format!("{row}\nnext\n"), 80);
    assert!(rows[1].wrapped);
}

#[test]
fn ctrl_f_goes_to_programs_that_read_it() {
    assert!(takes_find_key(None, false, false));
    assert!(!takes_find_key(Some("vim"), true, false));
    assert!(!takes_find_key(Some("Emacs"), true, false));
    assert!(takes_find_key(Some("claude"), true, true));
    // Without tmux, the second screen is a program's own.
    assert!(!takes_find_key(None, false, true));
}
