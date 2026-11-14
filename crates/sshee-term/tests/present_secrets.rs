//! 0.10's `src/lib/secrets.test.ts`, one test for one test, same inputs and
//! expected results. Every token and key here is made up.

use sshee_term::secrets::{ends_key, find_secrets, key_lines, key_open_after, key_rows, looks_like_key_body, mask_secrets};
use std::time::{Duration, Instant};

/// The secret parts of a line, as text.
fn secrets(line: &str) -> Vec<&str> {
    find_secrets(line).into_iter().map(|s| &line[s]).collect()
}

const NONE: [&str; 0] = [];

#[test]
fn knows_tokens_by_their_shape() {
    let gh = format!("ghp_{}", "a1B2".repeat(9));
    assert_eq!(secrets(&format!("token is {gh} ok")), [gh.as_str()]);
    assert_eq!(secrets("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123"), ["sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123"]);
    assert_eq!(secrets("export OPENAI=sk-proj-AbCdEfGhIjKlMnOpQrStUv"), ["sk-proj-AbCdEfGhIjKlMnOpQrStUv"]);
    assert_eq!(secrets("AKIAIOSFODNN7EXAMPLE"), ["AKIAIOSFODNN7EXAMPLE"]);
    let bot = format!("1234567890:AA{}", "x".repeat(33));
    assert_eq!(secrets(&format!("bot {bot} started")), [bot.as_str()]);
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
    assert_eq!(secrets(&format!("Authorization: Bearer {jwt}")), [jwt]);
}

#[test]
fn covers_the_value_not_the_name() {
    assert_eq!(secrets("DB_PASSWORD=hunter22"), ["hunter22"]);
    assert_eq!(secrets("  api_key: 'with spaces in it'"), ["with spaces in it"]);
    assert_eq!(secrets(r#"{"client_secret": "abcd1234"}"#), ["abcd1234"]);
    assert_eq!(secrets("GITHUB_TOKEN = abc123def"), ["abc123def"]);
    assert_eq!(secrets("DATABASE_URL=postgres://app:s3cr3t@db:5432/app"), ["s3cr3t"]);
}

#[test]
fn still_finds_a_password_in_a_url_that_streamer_mode_dotted_out_in_part() {
    // A server whose user is "postgres": streaming hides that word, scheme and all.
    assert_eq!(secrets("DATABASE_URL=••••••••://app:s3cr3t@db:5432/app"), ["s3cr3t"]);
    assert_eq!(secrets("url: my••••••://••••:s3cr3t@db/app"), ["s3cr3t"]);
}

#[test]
fn leaves_ordinary_output_alone() {
    for line in [
        "max_tokens: 4096",
        "token_count: 12",
        "PasswordAuthentication no",
        "[sudo] password for deploy:",
        "PASSWORD=true",
        "api_key: ${API_KEY}",
        "commit 3f9a2c1e8b7d6a5f4e3d2c1b0a9f8e7d6c5b4a39",
        "https://example.com/docs/tokens",
        "Bearer",
    ] {
        assert_eq!(secrets(line), NONE, "{line}");
    }
}

// Private keys. A made-up key, its markers put together here so no scanner takes the file for a real one.
fn key() -> Vec<String> {
    let kind = ["OPENSSH", "PRIVATE", "KEY"].join(" ");
    vec![
        format!("-----BEGIN {kind}-----"),
        "NotARealKeyJustLettersAndDigits0123456789abcdefghijklmnopqrstuvwxyzAB".to_string(),
        "MoreMadeUpLettersHere".to_string(),
        format!("-----END {kind}-----"),
    ]
}

#[test]
fn covers_the_lines_between_begin_and_end() {
    let mut lines = vec!["$ cat id_ed25519".to_string()];
    lines.extend(key());
    lines.push("$ ".to_string());
    assert_eq!(key_lines(&lines, false), [false, false, true, true, false, false]);
}

#[test]
fn knows_a_key_goes_on_from_above_the_screen() {
    let key = key();
    assert!(key_open_after(&key[..2]));
    assert!(!key_open_after(&key));
    assert_eq!(key_lines(&key[2..], true), [true, false]);
}

#[test]
fn masks_text_shown_elsewhere() {
    let key = key();
    let dots = |s: &str| "•".repeat(s.chars().count());
    assert_eq!(
        mask_secrets(&format!("ok\nTOKEN=abcdef\n{}", key.join("\n"))),
        format!("ok\nTOKEN=••••••\n{}\n{}\n{}\n{}", key[0], dots(&key[1]), dots(&key[2]), key[3])
    );
}

// Beyond 0.10's tests: what the hand-written look-behind and ASCII word rules must keep.

#[test]
fn a_url_password_needs_the_scheme_to_start_a_word() {
    // In 0.10 `(?<![\w+.•-])`: "xpostgres" can't be cut to "postgres" by starting later inside a word.
    assert_eq!(secrets("see_postgres://app:s3cr3t@db"), NONE);
    assert_eq!(secrets("(postgres://app:s3cr3t@db)"), ["s3cr3t"]);
}

#[test]
fn a_token_after_a_cyrillic_letter_still_starts_a_word() {
    // JavaScript's \b is ASCII: a Cyrillic letter before a token is a boundary.
    let gh = format!("ghp_{}", "a1B2".repeat(9));
    assert_eq!(secrets(&format!("токен{gh}")), [gh.as_str()]);
}

#[test]
fn a_password_that_starts_with_a_dollar_sign_is_still_a_password() {
    assert_eq!(secrets("DB_PASSWORD=$uperS3cret9"), ["$uperS3cret9"]);
    assert_eq!(secrets("DB_PASSWORD=$3cr3t!x"), ["$3cr3t!x"]);
    // A variable is `${...}`, or a name in one case.
    assert_eq!(secrets("PASSWORD=$DB_PASS"), NONE);
    assert_eq!(secrets("password: $db_pass"), NONE);
    assert_eq!(secrets("api_key: ${Api.Key}"), NONE);
}

#[test]
fn a_long_run_before_a_url_takes_time_in_step_with_its_length() {
    // Every place in the run fails the look-behind; trying each again took time growing with its square.
    let text = format!("_{}://u:pwd@", "a".repeat(50_000));
    let started = Instant::now();
    assert_eq!(secrets(&text), NONE);
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    // A real one after it is still found.
    assert_eq!(secrets(&format!("{text} postgres://app:s3cr3t@db")), ["s3cr3t"]);
    assert_eq!(secrets("see_postgres://app:s3cr3t@db postgres://app:hunter22@db"), ["hunter22"]);
}

#[test]
fn a_keys_short_last_line_is_covered_too() {
    let key = key();
    let lines = [key[0].as_str(), key[1].as_str(), "Qm7vT2xLp9A=", key[3].as_str()];
    assert_eq!(key_lines(&lines, false), [false, true, true, false]);
    assert_eq!(key_lines(&["Qm7vT2xLp9Ab3K==", key[3].as_str()], true), [true, false]);
    // Outside a key a short word is a word.
    assert_eq!(key_lines(&["Qm7vT2xLp9A=", "done"], false), [false, false]);
    // A screen that shows only that line and the END is worth looking above.
    assert!(!looks_like_key_body("Qm7vT2xLp9A=") && ends_key(&key[3]));
}

#[test]
fn rows_a_narrow_pane_wrapped_a_keys_lines_over_are_covered_whole() {
    let key = key();
    let body = &key[1];
    // 50 columns: the 69-character body line leaves its last 19 on a row of their own.
    let rows = [(key[0].as_str(), false), (&body[..50], false), (&body[50..], true), (&body[..50], false), (&body[50..], true), (key[3].as_str(), false)];
    assert_eq!(key_rows(&rows, false), [false, true, true, true, true, false]);
    // A BEGIN line cut in two by a pane narrower than it is still known.
    let rows = [(&key[0][..20], false), (&key[0][20..], true), (body.as_str(), false), (key[3].as_str(), false)];
    assert_eq!(key_rows(&rows, false), [false, false, true, false]);
}

/// What `unfinished_secret` covers of a line's end, as text.
fn unfinished(line: &str) -> Option<&str> {
    sshee_term::secrets::unfinished_secret(line).map(|s| &line[s])
}

#[test]
fn the_start_of_a_secret_still_being_printed_is_known_at_a_lines_end() {
    let body = "a1B2".repeat(9);
    // A token's fixed prefix and whatever of its body came so far, blanks after it or not.
    assert_eq!(unfinished(&format!("echo ghp_{}", &body[..5])), Some(&*format!("ghp_{}", &body[..5])));
    assert_eq!(unfinished("token is ghp_   "), Some("ghp_"));
    assert_eq!(unfinished("x github_pat_11AB"), Some("github_pat_11AB"));
    assert_eq!(unfinished("key sk-ant-api03-abc"), Some("sk-ant-api03-abc"));
    assert_eq!(unfinished("AKIAIOSF"), Some("AKIAIOSF"));
    assert_eq!(unfinished("t=eyJhbGciOi.eyJz"), Some("eyJhbGciOi.eyJz"));
    // A name's value with its quote still open, or shorter than a whole value.
    assert_eq!(unfinished(r#"{"token": "abc"#), Some("abc"));
    assert_eq!(unfinished(r#"  "client_secret": "with spa"#), Some("with spa"));
    assert_eq!(unfinished("DB_PASSWORD=hu"), Some("hu"));
    assert_eq!(unfinished("Authorization: Bearer eyJhbG"), Some("eyJhbG"));
    // Not at the end, finished, a setting, or no secret's start at all.
    for line in [
        &*format!("echo ghp_{} done", &body[..5]),
        r#"{"token": "abcd1234"}"#,
        "PASSWORD=no",
        "gh",
        "disk-",
        "plain text",
        "",
    ] {
        assert_eq!(unfinished(line), None, "{line}");
    }
}

#[test]
fn text_shown_elsewhere_hides_a_secret_cut_off_mid_print() {
    let body = "a1B2".repeat(9);
    assert_eq!(mask_secrets(&format!("waiting\ntoken ghp_{}", &body[..7])), format!("waiting\ntoken {}", "•".repeat(11)));
}
