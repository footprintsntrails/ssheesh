//! 0.10's `src/lib/shadowEcho.test.ts`, one test for one test, same inputs
//! and expected counts. The screen there was a real (headless) xterm fed the
//! bytes tmux 3.6 was recorded sending around bash; here it's a real
//! alacritty_terminal grid, the engine 1.0 draws from, fed the same bytes: a
//! key's echo is the plain character, Backspace comes as
//! `?2026h \b ESC[K ?2026l`, and attaching puts the terminal on its second
//! screen.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use sshee_term::echo::{row_look, EchoCount, EchoPlace, EchoScreen, EchoSink, RowLook, ShadowEcho, GIVE_UP_MS};
use std::collections::HashMap;

const BACKSPACE_ECHO: &str = "\x1b[?2026h\x08\x1b[K\x1b[?2026l";

/// An 80x24 terminal with 100 rows of scrollback, as 0.10's test had.
struct Screen {
    term: Term<VoidListener>,
    parser: Processor,
}

impl Screen {
    fn new() -> Self {
        let config = Config { scrolling_history: 100, ..Config::default() };
        Screen { term: Term::new(config, &TermSize::new(80, 24), VoidListener), parser: Processor::new() }
    }

    fn write(&mut self, text: &str) {
        self.parser.advance(&mut self.term, text.as_bytes());
    }
}

/// How the engine track is expected to read its grid for the counter.
impl EchoScreen for Screen {
    fn cols(&self) -> usize {
        self.term.columns()
    }

    fn cursor(&self) -> (usize, usize) {
        let grid = self.term.grid();
        let p = grid.cursor.point;
        ((grid.history_size() as i32 + p.line.0) as usize, p.column.0)
    }

    fn cell(&self, row: usize, col: usize) -> (char, &[char]) {
        let grid = self.term.grid();
        let line = row as i32 - grid.history_size() as i32;
        if line >= grid.screen_lines() as i32 || col >= grid.columns() {
            return (' ', &[]);
        }
        let cell = &grid[Line(line)][Column(col)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            return (' ', &[]);
        }
        (cell.c, cell.zerowidth().unwrap_or(&[]))
    }
}

#[derive(Debug, Default)]
struct Counts {
    counts: HashMap<EchoCount, usize>,
    echoes: Vec<u64>,
    /// Where each `HeldSilent` was counted.
    held: Vec<EchoPlace>,
}

impl EchoSink for Counts {
    fn count(&mut self, place: EchoPlace, what: EchoCount) {
        *self.counts.entry(what).or_default() += 1;
        if what == EchoCount::HeldSilent {
            self.held.push(place);
        }
    }
    fn echo_took(&mut self, _: EchoPlace, ms: u64) {
        self.echoes.push(ms);
    }
}

struct Rig {
    screen: Screen,
    echo: ShadowEcho<Counts>,
    place: EchoPlace,
    /// The clock 0.10's test faked.
    now: u64,
}

impl Rig {
    fn new() -> Self {
        Rig { screen: Screen::new(), echo: ShadowEcho::new(Counts::default()), place: EchoPlace::Line, now: 1_700_000_000_000 }
    }

    fn count(&self, what: EchoCount) -> usize {
        self.echo.sink().counts.get(&what).copied().unwrap_or(0)
    }

    fn forget_counts(&mut self) {
        self.echo.sink_mut().counts.clear();
    }

    fn advance(&mut self, ms: u64) {
        self.now += ms;
    }

    /// What the server sent, in the grid.
    fn out(&mut self, text: &str) {
        self.screen.write(text);
        self.echo.parsed(&self.screen, self.now);
    }

    fn key(&mut self, data: &str) {
        self.echo.key(&self.screen, data, self.place, self.now);
    }

    fn type_keys(&mut self, keys: &str) {
        for c in keys.chars() {
            self.key(&c.to_string());
        }
    }

    /// Types and gets the echo back straight away, key by key.
    fn type_and_echo(&mut self, keys: &str) {
        for c in keys.chars() {
            self.key(&c.to_string());
            self.advance(100);
            self.out(&c.to_string());
        }
    }

    /// A fresh shell prompt with the gate shut.
    fn prompt(&mut self, text: &str) {
        self.echo.reset();
        self.out(&format!("\r\n{text}"));
    }

    fn tick(&mut self) {
        self.echo.tick(&self.screen, self.now);
    }
}

use EchoCount::*;

// ---- the gate

#[test]
fn makes_the_first_key_on_a_line_wait_and_lets_the_next_ones_through_once_its_echo_lands() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("ls");
    assert_eq!((rig.count(Waited), rig.count(AtOnce), rig.count(Right), rig.count(Wrong)), (1, 1, 1, 0));
}

#[test]
fn treats_keys_typed_before_the_first_echo_as_waiting_too() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_keys("abc");
    rig.advance(300);
    rig.out("abc");
    assert_eq!((rig.count(Waited), rig.count(AtOnce)), (3, 0));
    rig.type_keys("d");
    assert_eq!(rig.count(AtOnce), 1);
}

#[test]
fn judges_keys_that_were_typed_ahead_when_their_echoes_land_together() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("a");
    rig.type_keys("bcd");
    rig.advance(250);
    rig.out("bcd");
    assert_eq!((rig.count(AtOnce), rig.count(Right)), (3, 3));
    assert_eq!(rig.echo.sink().echoes, [100, 250, 250, 250]);
}

#[test]
fn predicts_cyrillic_half_of_what_the_owner_types() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("привет");
    assert_eq!((rig.count(AtOnce), rig.count(Right)), (5, 5));
}

#[test]
fn reads_the_cell_not_the_bytes_colors_and_a_redraw_around_the_echo_change_nothing() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("a");
    rig.type_keys("b");
    rig.out("\x1b[?2026h\x1b[?25l\x1b[1;32mb\x1b[0m\x1b[?25h\x1b[?2026l");
    assert_eq!(rig.count(Right), 1);
    // zsh rewrites the previous character along with each new one.
    rig.type_keys("c");
    rig.out("\x08bc");
    assert_eq!(rig.count(Right), 2);
}

#[test]
fn works_on_tmuxs_second_screen_where_scrolled_rows_have_no_history() {
    let mut rig = Rig::new();
    rig.out("\x1b[?1049h\x1b[H$ ");
    rig.type_and_echo("hi");
    assert_eq!(rig.count(Right), 1);
}

#[test]
fn still_knows_the_row_when_the_normal_screen_has_scrolled() {
    let mut rig = Rig::new();
    for i in 0..40 {
        rig.out(&format!("line {i}\r\n"));
    }
    rig.out("$ ");
    rig.type_and_echo("hi");
    assert_eq!(rig.count(Right), 1);
}

// ---- what shuts the gate

/// A prompt with "ab" typed and echoed, counts cleared.
fn typing() -> Rig {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("ab");
    rig.forget_counts();
    rig
}

#[test]
fn enter_and_the_next_line_starts_over() {
    let mut rig = typing();
    rig.key("\r");
    rig.out("\r\n$ ");
    rig.type_keys("x");
    assert_eq!((rig.count(AtOnce), rig.count(Waited)), (0, 1));
}

#[test]
fn keys_that_edit_or_end_the_line() {
    for (name, key) in [
        ("Tab", "\t"),
        ("Left", "\x1b[D"),
        ("Right in application mode", "\x1bOC"),
        ("Up", "\x1b[A"),
        ("Ctrl+C", "\x03"),
        ("Ctrl+U", "\x15"),
        ("Alt+B", "\x1bb"),
        ("Alt+Shift+P, which starts like the terminal's replies", "\x1bP"),
        ("F5", "\x1b[15~"),
        ("Esc", "\x1b"),
    ] {
        let mut rig = typing();
        rig.key(key);
        rig.type_keys("x");
        assert_eq!((rig.count(AtOnce), rig.count(Keys)), (0, 2), "{name}");
    }
}

#[test]
fn things_that_arent_keys_at_all() {
    for (name, data) in [("a click", "\x1b[<0;10;5M"), ("an old-style click", "\x1b[M !!"), ("a paste", "\x1b[200~text\x1b[201~"), ("text in one piece", "пример")] {
        let mut rig = typing();
        rig.key(data);
        rig.type_keys("x");
        assert_eq!((rig.count(AtOnce), rig.count(Keys)), (0, 1), "{name}");
    }
}

#[test]
fn a_wrong_character_and_what_was_on_its_way_is_dropped() {
    let mut rig = typing();
    rig.type_keys("xyz");
    rig.out("Q");
    assert_eq!((rig.count(Wrong), rig.count(Dropped)), (1, 2));
    rig.type_keys("k");
    assert_eq!(rig.count(Waited), 1);
}

#[test]
fn a_cursor_that_moved_with_no_key_behind_it() {
    let mut rig = typing();
    rig.out("  ");
    rig.type_keys("x");
    assert_eq!(rig.count(AtOnce), 0);
}

#[test]
fn text_before_the_typing_changing_under_the_same_cursor() {
    let mut rig = typing();
    // Rewrites "$ " on the row and comes back to the same place.
    let row = rig.screen.term.grid().cursor.point.line.0 + 1;
    rig.out(&format!("\x1b[s\x1b[{row};1H%\x1b[u"));
    rig.type_keys("x");
    assert_eq!(rig.count(AtOnce), 0);
}

#[test]
fn a_new_size() {
    let mut rig = typing();
    rig.echo.reset();
    rig.type_keys("x");
    assert_eq!(rig.count(AtOnce), 0);
}

#[test]
fn a_key_that_isnt_guessed_a_wide_character_or_the_end_of_the_row() {
    let mut rig = typing();
    rig.key("中");
    assert_eq!(rig.count(Unfit), 1);
    rig.out("\x1b[80G");
    rig.type_keys("x");
    assert_eq!(rig.count(AtOnce), 0);
}

#[test]
fn text_to_the_right_nothing_in_mid_line_is_guessed() {
    let mut rig = typing();
    rig.out("tail\x1b[4D");
    rig.type_keys("x");
    assert_eq!((rig.count(MidLine), rig.count(AtOnce)), (1, 0));
}

// ---- Backspace

#[test]
fn backspace_is_judged_by_the_cell_it_clears_and_the_cursor() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("abc");
    rig.forget_counts();
    rig.key("\x7f");
    assert_eq!((rig.count(AtOnce), rig.count(Right)), (1, 0));
    rig.out(BACKSPACE_ECHO);
    assert_eq!(rig.count(Right), 1);
}

#[test]
fn backspace_isnt_guessed_over_a_key_still_on_its_way() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("ab");
    rig.forget_counts();
    rig.type_keys("c");
    rig.key("\x7f");
    assert_eq!((rig.count(Waited), rig.count(AtOnce)), (1, 1));
    rig.type_keys("d");
    assert_eq!(rig.count(AtOnce), 1);
}

#[test]
fn backspace_can_run_ahead_of_its_answers_and_be_typed_after() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("abcd");
    rig.forget_counts();
    rig.key("\x7f");
    rig.key("\x7f");
    rig.type_keys("x");
    assert_eq!(rig.count(AtOnce), 3);
    rig.out(&format!("{BACKSPACE_ECHO}{BACKSPACE_ECHO}x"));
    assert_eq!((rig.count(Right), rig.count(Wrong)), (3, 0));
    rig.type_keys("y");
    assert_eq!(rig.count(AtOnce), 4);
}

#[test]
fn backspace_isnt_guessed_past_where_the_proved_typing_starts() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("ab");
    rig.forget_counts();
    for _ in 0..2 {
        rig.key("\x7f");
        rig.out(BACKSPACE_ECHO);
    }
    assert_eq!(rig.count(Right), 2);
    rig.key("\x7f");
    assert_eq!(rig.count(Waited), 1);
}

// ---- a prompt that doesn't echo. Every key here would be a password
// character on screen if the gate let it through: the cases mosh and VS
// Code get wrong.

#[test]
fn on_a_new_row_every_key_waits_and_nothing_could_have_been_drawn() {
    let mut rig = Rig::new();
    rig.prompt("Password: ");
    rig.type_keys("hunter2-x");
    rig.advance(GIVE_UP_MS);
    rig.tick();
    assert_eq!((rig.count(AtOnce), rig.count(Waited), rig.count(HeldSilent), rig.count(PasswordOpen)), (0, 9, 9, 0));
}

#[test]
fn with_stray_redraws_arriving_meanwhile_tmux_sends_empty_frames_at_silent_prompts() {
    let mut rig = Rig::new();
    rig.prompt("Password: ");
    for c in "secret".chars() {
        rig.key(&c.to_string());
        rig.advance(90);
        rig.out("\x1b[?2026h\x1b[?25l\x1b[?12l\x1b[?25h\x1b[?2026l");
    }
    assert_eq!(rig.count(AtOnce), 0);
}

#[test]
fn on_a_row_that_echoed_before_it_once_the_prompt_has_shown_moshs_hole() {
    // `sleep 2; read -rs -p ' Password: '`, with `xy` typed during the sleep.
    let mut rig = Rig::new();
    rig.prompt("");
    rig.type_and_echo("xy");
    rig.out(" Password: ");
    rig.forget_counts();
    rig.type_keys("qwerty");
    rig.advance(GIVE_UP_MS);
    rig.tick();
    assert_eq!((rig.count(AtOnce), rig.count(HeldSilent)), (0, 6));
}

#[test]
fn on_such_a_row_the_keys_on_their_way_when_it_shows_are_counted_and_none_comes_back_right() {
    let mut rig = Rig::new();
    rig.prompt("");
    rig.type_and_echo("xy");
    rig.forget_counts();
    rig.type_keys("qwer");
    assert_eq!(rig.count(AtOnce), 4);
    rig.out(" Password: ");
    assert_eq!(rig.count(Right), 0);
    assert_eq!(rig.count(Wrong) + rig.count(Dropped), 4);
    rig.type_keys("ty");
    assert_eq!(rig.count(AtOnce), 4);
}

#[test]
fn right_after_a_row_that_echoed_asking_on_the_same_row_with_no_text_keys_go_silent() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("yes");
    rig.forget_counts();
    rig.type_keys("abc");
    rig.advance(GIVE_UP_MS);
    rig.tick();
    assert_eq!((rig.count(AtOnce), rig.count(Silent), rig.count(Right)), (3, 3, 0));
    // Silence shuts the gate.
    rig.type_keys("d");
    assert_eq!((rig.count(AtOnce), rig.count(Waited)), (3, 1));
}

#[test]
fn a_password_prompt_that_does_echo_opens_the_gate_and_thats_what_the_zero_is_there_to_catch() {
    let mut rig = Rig::new();
    rig.prompt("Password: ");
    rig.type_and_echo("ab");
    assert_eq!(rig.count(PasswordOpen), 1);
}

#[test]
fn asterisks_for_each_key_prove_nothing_sudo_rs() {
    let mut rig = Rig::new();
    rig.prompt("[sudo] пароль для k: ");
    rig.type_keys("a");
    rig.out("*");
    rig.type_keys("b");
    rig.out("*");
    assert_eq!((rig.count(AtOnce), rig.count(PasswordOpen)), (0, 0));
}

#[test]
fn a_type_ahead_after_enter_never_opens_the_gate_before_the_output_has_come() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("ls");
    rig.forget_counts();
    rig.key("\r");
    rig.type_keys("pw");
    assert_eq!((rig.count(Waited), rig.count(AtOnce)), (2, 0));
    rig.out("\r\nPassword: ");
    rig.type_keys("z");
    assert_eq!(rig.count(AtOnce), 0);
}

// ---- what the counts can be trusted for

#[test]
fn a_space_proves_nothing_because_it_looks_the_same_before_and_after() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_keys(" ");
    rig.out(" ");
    rig.type_keys("a");
    assert_eq!(rig.count(Waited), 2);
}

#[test]
fn a_space_doesnt_count_as_right_just_because_the_cell_is_blank() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("a");
    rig.forget_counts();
    rig.type_keys(" ");
    rig.out("");
    assert_eq!(rig.count(Right), 0);
    rig.out(" ");
    assert_eq!(rig.count(Right), 1);
}

#[test]
fn keys_before_an_enter_can_come_true_and_arent_wrong_if_the_screen_scrolled_first() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("ab");
    rig.forget_counts();
    rig.type_keys("cd");
    rig.key("\r");
    rig.out("cd\r\n$ ");
    assert_eq!((rig.count(Right), rig.count(Wrong)), (2, 0));
    // tmux scrolls the row itself, so the cells are gone before they're read.
    rig.out("\x1b[?1049h\x1b[H$ ");
    rig.type_and_echo("ab");
    rig.forget_counts();
    rig.type_keys("c");
    rig.key("\r");
    rig.out("\x1b[H\x1b[2J$ \r\n");
    assert_eq!((rig.count(Wrong), rig.count(Unjudged)), (0, 1));
}

#[test]
fn the_terminals_own_replies_and_focus_reports_arent_keys_and_change_nothing() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_and_echo("ab");
    rig.forget_counts();
    for reply in ["\x1b[?1;2c", "\x1b[>0;276;0c", "\x1b[0n", "\x1b[24;80R", "\x1b]11;rgb:0000/0000/0000\x1b\\", "\x1bP1$r0m\x1b\\", "\x1b[I", "\x1b[O", "\x1b[?2026;2$y"] {
        rig.key(reply);
    }
    assert_eq!(rig.count(Keys), 0);
    rig.type_keys("c");
    assert_eq!(rig.count(AtOnce), 1);
}

#[test]
fn counts_each_key_under_the_place_it_was_typed_in_even_if_the_answer_comes_after_a_move() {
    let mut rig = Rig::new();
    rig.place = EchoPlace::Agent;
    rig.key("a");
    rig.place = EchoPlace::Line;
    rig.advance(GIVE_UP_MS);
    rig.tick();
    assert_eq!(rig.echo.sink().held, [EchoPlace::Agent]);
}

#[test]
fn tells_rows_apart_well_enough_for_an_estimate_and_no_more() {
    assert_eq!(row_look("user@host:~$ "), RowLook::Prompt);
    assert_eq!(row_look("root@host:/# "), RowLook::Prompt);
    assert_eq!(row_look("~/code ❯ "), RowLook::Prompt);
    assert_eq!(row_look("PS C:\\Users> "), RowLook::Prompt);
    assert_eq!(row_look(">>> "), RowLook::Other);
    assert_eq!(row_look(""), RowLook::Other);
    assert_eq!(row_look("Password: "), RowLook::Password);
    assert_eq!(row_look("[sudo] password for k: "), RowLook::Password);
    assert_eq!(row_look("[sudo] пароль для k: "), RowLook::Password);
    assert_eq!(row_look("Enter passphrase for key: "), RowLook::Password);
    // What a client can't ever tell: this one doesn't echo.
    assert_eq!(row_look("$ "), RowLook::Prompt);
}

// ---- it keeps and sends nothing

const SECRET: &str = "hunter2-пароль";

#[test]
fn holds_no_typed_text_while_keys_are_on_their_way_or_after() {
    let mut rig = Rig::new();
    rig.prompt("$ ");
    rig.type_keys(SECRET);
    let on_the_way = format!("{:?}", rig.echo);
    rig.out(SECRET);
    rig.advance(GIVE_UP_MS);
    rig.tick();
    let after = format!("{:?}", rig.echo);
    // Only numbers and the names of places and kinds of row: nothing of the keys or the row's text.
    for state in [&on_the_way, &after] {
        assert!(!state.contains("hunter") && !state.contains("пароль") && !state.contains("unter2"), "{state}");
    }
    assert!(on_the_way.contains("Line"));
}

#[test]
fn forgets_the_keys_that_got_no_answer() {
    let mut rig = Rig::new();
    rig.prompt("Password: ");
    rig.type_keys(SECRET);
    assert!(rig.echo.on_the_way() > 0);
    rig.advance(GIVE_UP_MS);
    rig.tick();
    assert_eq!(rig.echo.on_the_way(), 0);
}

/// 0.10 read its own source to check it imported nothing; this reads echo.rs
/// (its tests left out) for anything that could draw, send, save or log.
#[test]
fn cant_draw_send_or_save_it_uses_nothing_that_could() {
    let source = include_str!("../src/echo.rs");
    let code = source.split("#[cfg(test)]").next().unwrap_or(source);
    let code: String = code.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    let uses: Vec<&str> = code.lines().filter(|l| l.starts_with("use ")).collect();
    // `fmt` writes its `Debug` (which leaves the fingerprints out) into whatever asks for it; `hash` makes the fingerprints.
    assert_eq!(
        uses,
        ["use regex::Regex;", "use std::collections::hash_map::RandomState;", "use std::fmt;", "use std::hash::{BuildHasher, Hash, Hasher};", "use std::sync::LazyLock;"]
    );
    for name in ["std::fs", "std::net", "std::io", "std::process", "std::env", "std::thread", "File", "TcpStream", "Command", "println!", "eprintln!", "print!", "dbg!", "log::", "tracing::", "crate::", "super::", "unsafe"] {
        assert!(!code.contains(name), "echo.rs mentions {name}");
    }
}
