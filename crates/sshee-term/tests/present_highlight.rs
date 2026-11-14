//! Highlighting. 0.10 had no `highlight.test.ts`: its rules were tested
//! through the output stream (`src/lib/output.test.ts`, "OutputPipeline"),
//! where a match came out wrapped in SGR codes. Those cases are here with the
//! same inputs; the expected SGR becomes the match's text and `Paint::sgr()`.

use sshee_term::highlight::{broken_rules, compile_rule, count_problems, line_tone, HighlightColor, HighlightRule, Highlights, Problems, Tone, JS_TEXT_LIMIT};
use std::time::{Duration, Instant};

/// The settings 0.10 starts with: errors and warnings on, addresses off, no rules.
fn defaults() -> Highlights {
    Highlights::new(true, true, false, &[])
}

/// Each match as its text and the SGR 0.10 put before it.
fn marks<'a>(h: &Highlights, text: &'a str) -> Vec<(&'a str, String)> {
    h.find_matches(text).into_iter().map(|m| (&text[m.range], m.paint.sgr())).collect()
}

fn rule(id: &str, pattern: &str, color: HighlightColor) -> HighlightRule {
    HighlightRule { id: id.to_string(), pattern: pattern.to_string(), color }
}

#[test]
fn passes_output_through_untouched_when_nothing_is_on() {
    let h = Highlights::new(false, false, false, &[]);
    assert!(!h.is_on());
    assert!(marks(&h, "deploy@prod-api ERROR").is_empty());
}

#[test]
fn colors_errors() {
    // 0.10: `ESC[32mbuild ESC[1;31mERROR ESC[0m ESC[32m here`
    assert_eq!(marks(&defaults(), "build ERROR here"), [("ERROR", "1;31".to_string())]);
}

#[test]
fn colors_warnings_with_lowercase_tool_output() {
    assert_eq!(marks(&defaults(), "warning: unused variable"), [("warning", "33".to_string())]);
}

#[test]
fn colors_addresses_when_asked() {
    let h = Highlights::new(true, true, true, &[]);
    assert_eq!(
        marks(&h, "Learn more at https://ubuntu.com/esm or 198.51.100.63:8080 now"),
        [("https://ubuntu.com/esm", "4;36".to_string()), ("198.51.100.63:8080", "4;36".to_string())]
    );
    // A port the sentence's full stop follows is left off, as 0.10's regex backtracked to the address.
    assert_eq!(marks(&h, "at 198.51.100.63:8080."), [("198.51.100.63", "4;36".to_string())]);
}

#[test]
fn applies_your_own_rules_plain_or_as_a_regex() {
    let h = Highlights::new(true, true, false, &[rule("a", "nginx", HighlightColor::Green), rule("b", r"/took \d+ms/", HighlightColor::Cyan)]);
    assert_eq!(marks(&h, "NGINX took 12ms"), [("NGINX", "32".to_string()), ("took 12ms", "36".to_string())]);
}

#[test]
fn ignores_a_rule_that_isnt_a_valid_regex() {
    let rules = [rule("x", "/([/", HighlightColor::Red)];
    let h = Highlights::new(false, false, false, &rules);
    assert!(marks(&h, "fine").is_empty());
    assert_eq!(broken_rules(&rules), ["x"]);
}

#[test]
fn picks_up_a_rule_changed() {
    assert!(marks(&defaults(), "nginx").is_empty());
    let green = Highlights::new(true, true, false, &[rule("a", "nginx", HighlightColor::Green)]);
    assert_eq!(marks(&green, "nginx"), [("nginx", "32".to_string())]);
    let blue = Highlights::new(true, true, false, &[rule("a", "nginx", HighlightColor::Blue)]);
    assert_eq!(marks(&blue, "nginx"), [("nginx", "34".to_string())]);
}

#[test]
fn hides_names_and_highlights_in_one_pass() {
    // 0.10: `••••••@••••••••: ESC[1;31mFATAL ESC[0m` from "deploy@prod-api: FATAL" while streaming.
    // The dots are the presentation pass's (tests/present_pass.rs); the highlight runs on its text.
    assert_eq!(marks(&defaults(), "••••••@••••••••: FATAL"), [("FATAL", "1;31".to_string())]);
}

// Beyond 0.10's tests: the built-in rules' look-arounds and JavaScript regex rules.

#[test]
fn a_capitalised_word_counts_only_before_a_colon_or_bracket() {
    let h = defaults();
    assert_eq!(marks(&h, "Error: disk full"), [("Error", "1;31".to_string())]);
    assert_eq!(marks(&h, "Error[E0308] mismatched"), [("Error", "1;31".to_string())]);
    assert!(marks(&h, "Error handling is fine").is_empty());
    assert!(marks(&h, "ERRORS and ERRORLEVEL").is_empty());
    // 0.10's `ERR!\b` never matched "npm ERR! code": kept as it was.
    assert!(marks(&h, "npm ERR! code").is_empty());
}

#[test]
fn your_rules_win_where_they_overlap_a_built_in_one() {
    let h = Highlights::new(true, true, false, &[rule("a", "ERROR here", HighlightColor::Magenta)]);
    assert_eq!(marks(&h, "build ERROR here"), [("ERROR here", "35".to_string())]);
}

#[test]
fn javascript_regex_rules_keep_working() {
    // Look-behind, a named group and a back-reference: none of these is in the `regex` crate.
    assert!(compile_rule(r"/(?<=user=)\w+/").is_some());
    assert!(compile_rule(r"/(?<word>a)\k<word>/").is_some());
    let h = Highlights::new(false, false, false, &[rule("a", r"/(?<=user=)\w+/i", HighlightColor::Yellow), rule("b", r"/(\d)\1/", HighlightColor::Red)]);
    assert_eq!(marks(&h, "user=db-main code 4477"), [("db", "33".to_string()), ("44", "31".to_string()), ("77", "31".to_string())]);
    // Flags JavaScript refuses.
    assert!(compile_rule("/a/x").is_none());
    assert!(compile_rule("/a/gg").is_none());
    assert!(compile_rule("/a/uv").is_none());
    assert!(compile_rule("   ").is_none());
}

#[test]
fn a_sticky_rule_only_matches_from_the_start_on() {
    let h = Highlights::new(false, false, false, &[rule("a", "/ab/y", HighlightColor::Green)]);
    assert_eq!(marks(&h, "ababxab"), [("ab", "32".to_string()), ("ab", "32".to_string())]);
}

#[test]
fn counts_problems_by_line() {
    let lines = ["ok", "ERROR: x", "Warning: y", "error: and warning: both", "fine"];
    assert_eq!(count_problems(&lines), Problems { errors: 2, warnings: 1 });
    assert_eq!(line_tone("Traceback (most recent call last):"), Some(Tone::Error));
    assert_eq!(line_tone("DEPRECATED api"), Some(Tone::Warning));
    assert_eq!(line_tone("all good"), None);
}

// Your regex rules run over whatever a server prints, inside the pass that draws the screen.

fn one(pattern: &str) -> Highlights {
    Highlights::new(false, false, false, &[rule("r", pattern, HighlightColor::Green)])
}

fn green(text: &str) -> (&str, String) {
    (text, "32".to_string())
}

#[test]
fn a_rule_that_would_backtrack_for_ever_takes_time_in_step_with_the_line() {
    // `(\w+\s?)+:` on a long word with no colon: exponential for a backtracking engine.
    let h = one(r"/(\w+\s?)+:/");
    let line = format!("{} !", "a".repeat(5000));
    let started = Instant::now();
    assert!(marks(&h, &line).is_empty());
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(marks(&h, "key value: x"), [green("key value:")]);
}

#[test]
fn a_rule_only_a_backtracking_engine_runs_must_not_nest_endless_repeats() {
    // A look-behind needs `regress`: with a repeated group that repeats without end inside, it's refused.
    let rules = [rule("nested", r"/(?<=x)(a+)+b/", HighlightColor::Red), rule("fine", r"/(?<=status: )\w+/", HighlightColor::Green)];
    assert!(compile_rule(&rules[0].pattern).is_none());
    assert_eq!(broken_rules(&rules), ["nested"]);
    let h = Highlights::new(false, false, false, &rules);
    assert_eq!(marks(&h, "status: ok"), [green("ok")]);
    // And it isn't run over a line longer than the limit.
    let long = format!("status: ok {}", "x".repeat(JS_TEXT_LIMIT));
    assert!(marks(&h, &long).is_empty());
    // The same nesting without a look-around runs on the linear engine.
    assert!(compile_rule(r"/(a+)+b/").is_some());
}

#[test]
fn your_regex_rules_read_as_javascript_reads_them() {
    // \d, \w and \b are ASCII, as in JavaScript without the `u` flag.
    assert_eq!(marks(&one(r"/\d+/"), "port ٣ 8080"), [green("8080")]);
    assert_eq!(marks(&one(r"/\bdb\w*/i"), "DB_main dbé xdb"), [green("DB_main"), green("db")]);
    // \s takes JavaScript's spaces, the no-break one too.
    assert_eq!(marks(&one(r"/a\sb/"), "a\u{a0}b"), [green("a\u{a0}b")]);
    // A `{` that isn't a repeat, `]`, `/` and `<` stand for themselves.
    assert_eq!(marks(&one(r"/a{x}\]\/\</"), "a{x}]/<"), [green("a{x}]/<")]);
    assert_eq!(marks(&one(r"/x{2,3}?/"), "xxxx"), [green("xx"), green("xx")]);
    // Classes: ranges, escapes inside, a `-` at the end, `[^]` for anything.
    assert_eq!(marks(&one(r"/[\d-]+/"), "call 555-0100 now"), [green("555-0100")]);
    assert_eq!(marks(&one(r"/[a-c\]]+/"), "x]cab]x"), [green("]cab]")]);
    assert_eq!(marks(&one(r"/<[^]*?>/"), "a <b c> d"), [green("<b c>")]);
    // A `.` stops at a line end unless `s`.
    assert_eq!(marks(&one(r"/a.b/"), "a\rb a-b"), [green("a-b")]);
    assert_eq!(marks(&one(r"/a.b/s"), "a\rb"), [green("a\rb")]);
    // Escapes by code, and named groups.
    assert_eq!(marks(&one(r"/\x41\u0042/"), "xABx"), [green("AB")]);
    assert_eq!(marks(&one(r"/(?<word>x+?)y/"), "xxxy"), [green("xxxy")]);
    // `^` and `$` at the line's ends, or every line's with `m`.
    assert_eq!(marks(&one(r"/^ok$/"), "ok"), [green("ok")]);
    assert!(marks(&one(r"/^ok$/"), "ok\nok").is_empty());
    assert_eq!(marks(&one(r"/^ok$/m"), "ok\nok"), [green("ok"), green("ok")]);
}

#[test]
fn rules_put_into_the_linear_engine_match_what_javascripts_engine_matches() {
    // The same rule through `regress` (JavaScript's syntax, backtracking) and as compiled here.
    let patterns = [
        r"\d+ms", r"took \d+(\.\d+)?s", r"\b[A-Z]{2,}\b", r"\S+@\S+", r"^\s*#.*", r"(?:GET|POST|PUT) /\S*", r"\[[^\]]*\]",
        r"[\w.-]+\.(?:log|txt)", r"\(.*?\)", r"user=\w+", r"a|ab|abc", r"(a|ab)(c|bcd)", r"x*", r"\bfoo\B", r"[^a-z\s]+",
        r"[\-+*/]", r"\$\{?\w+\}?", r"\t|\x20{2,}", r"(?<n>\d{1,3})(\.\d{1,3}){3}", r"colou?r", r"\u00e9t\u00e9", r"[^]", r".+?:", r"\.\.\.",
    ];
    let texts = [
        "GET /api/v1 took 12.5s at 2026-11-13 [WARN] from deploy@web-7731 (retry 2) foo bar",
        "  # comment ... user=db_main file app.log and notes.txt colour color été ${HOME} $PATH",
        "abcd abc ab a xxx   tabs\there FOObar fooBAR (a(b)c) a*b+c-d/e",
        "",
    ];
    for pattern in patterns {
        for flags in ["", "i", "s", "m", "gi"] {
            let rule = format!("/{pattern}/{flags}");
            let js = regress::Regex::with_flags(pattern, flags.replace('g', "").as_str()).expect("valid JavaScript");
            let h = one(&rule);
            for text in texts {
                let mut want: Vec<&str> = Vec::new();
                let mut at = 0;
                while at <= text.len() {
                    let Some(m) = js.find_from(text, at).next() else { break };
                    let r = m.range();
                    if r.is_empty() {
                        at = r.start + text[r.start..].chars().next().map_or(1, char::len_utf8);
                        continue;
                    }
                    at = r.end;
                    want.push(&text[r]);
                }
                let got: Vec<&str> = marks(&h, text).into_iter().map(|(m, _)| m).collect();
                assert_eq!(got, want, "{rule} on {text:?}");
            }
        }
    }
}
