//! The question reader against whole screens (`tests/questions/*.txt`).
//!
//! Every screen there is made by hand: none was captured from a running agent. Claude Code's
//! and Codex's are drawn from their layouts as remembered; OpenCode's and oh-my-pi's from the
//! words and rows in those programs' own source (OpenCode 1.18, oh-my-pi 18.18). Each file
//! says so in its first lines, which end at `---`. `⟦ ⟧` mark characters drawn on a highlight.

use sshee_term::question::{read, Asker, Axis, ChoiceKind, How, Keys, Line, Question, Reading, Unsure};

fn screen(name: &str) -> Vec<Line> {
    let path = format!("{}/tests/questions/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let file = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (note, body) = file.split_once("---\n").unwrap_or_else(|| panic!("{name} has no --- line"));
    assert!(note.lines().all(|l| l.starts_with('#')), "{name}: the note before --- is # lines");
    assert!(note.to_lowercase().contains("hand") || note.contains("Made up"), "{name} must say where it comes from");
    body.lines()
        .map(|row| {
            let mut line = Line::default();
            let mut open = None;
            for c in row.chars() {
                match c {
                    '⟦' => open = Some(line.text.chars().count()),
                    '⟧' => line.lit.push(open.take().expect("⟧ after ⟦")..line.text.chars().count()),
                    c => line.text.push(c),
                }
            }
            line
        })
        .collect()
}

fn question(asker: Asker, name: &str) -> Question {
    match read(asker, &screen(name)) {
        Reading::Question(q) => q,
        other => panic!("{name}: {other:?}"),
    }
}

fn labels(q: &Question) -> Vec<(u8, &str, ChoiceKind)> {
    q.options.iter().map(|o| (o.number, o.label.as_str(), o.kind)).collect()
}

fn digit(n: u8) -> Option<Keys> {
    Some(Keys::Text(n.to_string()))
}

#[test]
fn claude_code_asks_before_a_command() {
    let q = question(Asker::ClaudeCode, "claude-bash");
    assert_eq!(q.title.as_deref(), Some("Bash command"));
    assert_eq!(q.context, ["psql -h db-main -d shop -f migrations/0042_orders_index.sql", "Apply migration 0042: orders index"]);
    assert_eq!(q.question, "Do you want to proceed?");
    assert_eq!(
        labels(&q),
        [
            (1, "Yes", ChoiceKind::Plain),
            (2, "Yes, and don't ask again for psql commands in /srv/shop-db", ChoiceKind::Always),
            (3, "No, and tell Claude what to do differently", ChoiceKind::Text),
        ]
    );
    assert_eq!(q.highlighted, 0);
    // The digit is the answer.
    assert_eq!(q.how, How::Key);
    assert_eq!(q.options.iter().map(|o| o.keys.clone()).collect::<Vec<_>>(), [digit(1), digit(2), digit(3)]);
}

#[test]
fn claude_codes_boxed_layout_reads_the_same_way() {
    let q = question(Asker::ClaudeCode, "claude-bash-boxed");
    assert_eq!(q.title.as_deref(), Some("Bash command"));
    assert_eq!(q.context, ["npm test", "Run the test suite"]);
    assert_eq!(q.question, "Do you want to proceed?");
    assert_eq!(q.options.len(), 3);
    assert_eq!(q.options[1].label, "Yes, and don't ask again for npm test commands in /srv/shop");
    // Its cursor is on the second answer.
    assert_eq!(q.highlighted, 1);
}

#[test]
fn claude_code_asks_before_an_edit() {
    let q = question(Asker::ClaudeCode, "claude-edit");
    assert_eq!(q.title.as_deref(), Some("Edit file"));
    assert_eq!(
        q.context,
        ["migrations/0042_orders_index.sql", "1 -CREATE INDEX orders_created ON orders (created_at);", "1 +CREATE INDEX CONCURRENTLY orders_created ON orders (created_at);"]
    );
    assert_eq!(q.question, "Do you want to make this edit to 0042_orders_index.sql?");
    assert_eq!(labels(&q)[1], (2, "Yes, allow all edits during this session", ChoiceKind::Always));
    // Boxed, with the change in a box of its own.
    let q = question(Asker::ClaudeCode, "claude-edit-boxed");
    assert_eq!(q.title.as_deref(), Some("Edit file"));
    assert_eq!(q.context, ["src/app.rs", "", "12 -    let retries = 1;", "12 +    let retries = 3;"]);
    assert_eq!(q.question, "Do you want to make this edit to app.rs?");
    assert_eq!(q.options.len(), 3);
}

#[test]
fn an_answer_that_wraps_is_one_answer() {
    let q = question(Asker::ClaudeCode, "claude-wrapped");
    assert_eq!(
        labels(&q),
        [
            (1, "Yes", ChoiceKind::Plain),
            (2, "Yes, and don't ask again for docker compose commands in /srv/shop", ChoiceKind::Always),
            (3, "No, and tell Claude what to do differently", ChoiceKind::Text),
        ]
    );
    assert_eq!(q.highlighted, 1);
    assert_eq!(q.context, ["docker compose -f deploy/compose.yml restart", "shop-worker-1", "Restart the worker"]);
}

#[test]
fn codex_asks_with_a_key_beside_each_answer() {
    let q = question(Asker::Codex, "codex-command");
    assert_eq!(q.question, "Would you like to run the following command?");
    assert_eq!(q.title, None);
    assert_eq!(q.context, ["Reason: apply migration 0042 to the shop database", "", "$ psql -h db-main -d shop -f migrations/0042_orders_index.sql"]);
    assert_eq!(
        labels(&q),
        [
            (1, "Yes, proceed", ChoiceKind::Plain),
            (2, "Yes, and don't ask again for commands that start with `psql`", ChoiceKind::Always),
            (3, "No, and tell Codex what to do differently", ChoiceKind::Text),
        ]
    );
    // What its own screen says picks each.
    let keys: Vec<Option<Keys>> = q.options.iter().map(|o| o.keys.clone()).collect();
    assert_eq!(keys, [Some(Keys::Text("y".into())), Some(Keys::Text("p".into())), Some(Keys::Escape)]);
    assert_eq!((q.highlighted, q.how), (0, How::Key));
    // Answers that print no key can't be picked from outside.
    assert_eq!(read(Asker::Codex, &screen("codex-no-keys")), Reading::Unsure(Unsure::NoKey));
}

#[test]
fn opencode_asks_with_buttons_and_a_highlight() {
    let q = question(Asker::OpenCode, "opencode-permission");
    assert_eq!(q.question, "Permission required");
    assert_eq!(q.context, ["$ Apply migration 0042", "", "psql -h db-main -d shop -f migrations/0042_orders_index.sql"]);
    assert_eq!(labels(&q), [(1, "Allow once", ChoiceKind::Plain), (2, "Allow always", ChoiceKind::Always), (3, "Reject", ChoiceKind::Text)]);
    // No keys of their own: its highlight is moved sideways, then Return.
    assert_eq!(q.how, How::Highlight(Axis::Columns));
    assert!(q.options.iter().all(|o| o.keys.is_none()));
    assert_eq!(q.highlighted, 0);
    // The highlight moved: the same question, another place.
    let moved = question(Asker::OpenCode, "opencode-moved");
    assert_eq!(moved.highlighted, 2);
    assert!(q.same_as(&moved));
    // "Allow always" asks once more.
    let again = question(Asker::OpenCode, "opencode-always");
    assert_eq!(again.question, "Always allow");
    assert_eq!(labels(&again), [(1, "Confirm", ChoiceKind::Plain), (2, "Cancel", ChoiceKind::Plain)]);
    assert!(!q.same_as(&again));
    // Without a highlight to see, Return could pick anything.
    assert_eq!(read(Asker::OpenCode, &screen("opencode-no-highlight")), Reading::Unsure(Unsure::Cursor));
}

#[test]
fn oh_my_pi_asks_with_a_list_and_a_cursor() {
    let q = question(Asker::Omp, "omp-ask");
    assert_eq!(q.question, "How should I run it on db-main?");
    assert_eq!(q.context, ["Migration 0042 builds an index on orders."]);
    assert_eq!(
        labels(&q),
        [(1, "In one transaction", ChoiceKind::Plain), (2, "With CREATE INDEX CONCURRENTLY", ChoiceKind::Plain), (3, "Other (type your own)", ChoiceKind::Text)]
    );
    assert_eq!((q.highlighted, q.how), (0, How::Highlight(Axis::Rows)));
    // Tick boxes are several answers, not one.
    assert_eq!(read(Asker::Omp, &screen("omp-several")), Reading::Unsure(Unsure::Several));
}

#[test]
fn no_question_on_screen_is_nothing() {
    for asker in [Asker::ClaudeCode, Asker::Codex, Asker::OpenCode, Asker::Omp] {
        assert_eq!(read(asker, &screen("shell")), Reading::Nothing, "{asker:?}");
        assert_eq!(read(asker, &screen("claude-working")), Reading::Nothing, "{asker:?}");
        assert_eq!(read(asker, &[]), Reading::Nothing, "{asker:?}");
    }
    // A message that ends in a numbered list has no cursor on it.
    assert_eq!(read(Asker::ClaudeCode, &screen("claude-prose")), Reading::Nothing);
}

#[test]
fn a_screen_it_cannot_be_sure_of_is_refused() {
    let refused = |name: &str| match read(Asker::ClaudeCode, &screen(name)) {
        Reading::Unsure(why) => why,
        other => panic!("{name}: {other:?}"),
    };
    // Taller than the screen: what it's about can't be shown whole.
    assert_eq!(refused("claude-cut"), Unsure::Cut);
    // Something came after the answers.
    assert_eq!(refused("claude-trailing"), Unsure::Trailing);
    assert_eq!(refused("claude-two-cursors"), Unsure::Cursor);
    assert_eq!(refused("claude-numbers"), Unsure::Numbers);
    assert_eq!(refused("claude-no-question"), Unsure::NoQuestion);
}

#[test]
fn another_agents_layout_is_not_guessed_at() {
    // Claude Code's question read as Codex's has no keys beside its answers.
    assert!(matches!(read(Asker::Codex, &screen("claude-bash")), Reading::Unsure(_)));
    // And as OpenCode's or oh-my-pi's it's no question at all.
    assert_eq!(read(Asker::OpenCode, &screen("claude-bash")), Reading::Nothing);
    assert_eq!(read(Asker::Omp, &screen("claude-bash")), Reading::Nothing);
    assert_eq!(read(Asker::ClaudeCode, &screen("opencode-permission")), Reading::Nothing);
    assert_eq!(read(Asker::ClaudeCode, &screen("omp-ask")), Reading::Nothing);
}

#[test]
fn a_question_that_changed_is_another_question() {
    let first = question(Asker::ClaudeCode, "claude-bash");
    let mut rows = screen("claude-bash");
    for row in &mut rows {
        row.text = row.text.replace("0042_orders_index.sql", "0043_drop_orders.sql");
    }
    let Reading::Question(second) = read(Asker::ClaudeCode, &rows) else { panic!() };
    assert_eq!(second.options, first.options);
    assert!(!first.same_as(&second), "the command changed under the same answers");
    // The cursor moving is not a change.
    let mut rows = screen("claude-bash");
    for row in &mut rows {
        row.text = row.text.replace("❯ 1.", "  1.").replace("   2.", " ❯ 2.");
    }
    let Reading::Question(moved) = read(Asker::ClaudeCode, &rows) else { panic!() };
    assert_eq!(moved.highlighted, 1);
    assert!(first.same_as(&moved));
}

#[test]
fn rows_of_any_shape_never_panic() {
    let odd = ["", " ", "❯", "❯ ", "1.", "❯ 1.", "│", "││││", "│ │ │ │", "△", "○", "❯ ○", "Allow once", "Allow once Allow always Reject", "⟦", "\u{0301}", "1. \u{200d}", "  9. x", "❯ 99. x", "(o) (o)", "────", "╌╌╌╌"];
    for asker in [Asker::ClaudeCode, Asker::Codex, Asker::OpenCode, Asker::Omp] {
        for a in odd {
            for b in odd {
                let rows = [Line::plain(a), Line::plain(b), Line::plain(a)];
                let _ = read(asker, &rows);
            }
        }
    }
}
