//! Answering an agent's question from its card, without going to its tab: what a click, a
//! digit or Return does, and what is typed into the terminal for it. Without a screen.
//!
//! This types into other people's production servers, so the rules lean one way: when in
//! doubt, nothing is sent and the card says why.
//!
//! - Nothing is sent by pointing at an answer; only by a click, a digit, or Return.
//! - On a Production server no answer is chosen beforehand, so Return alone sends nothing.
//!   Elsewhere the first answer is, and Return sends it.
//! - Digits, arrows and Return count only while the card has the keyboard. While the
//!   terminal has it, they are the terminal's.
//! - For the first [`GRACE_MS`] after a question appears on the card, nothing answers it: a
//!   click meant for what was there a moment ago can't land on it.
//! - A standing permission ("Yes, and don't ask again…") on Production needs the same answer
//!   twice within [`SECOND_CLICK_MS`].
//! - Before anything is typed, the screen is read again ([`Scene::reading`]): if the question
//!   there is no longer the one on the card, the card shows the new one and nothing is sent.
//! - A tab that isn't connected gets nothing: the answer is kept on the card.
//! - "No, and tell it what to do differently" turns into a text field. Its key goes first;
//!   the words follow only once the agent has stopped asking, so they can never land on a
//!   question as answers.
//! - Where the agent has no key per answer (OpenCode, oh-my-pi), its highlight is moved
//!   first and Return follows only once the screen shows the highlight on the chosen answer.
//!
//! The window types what [`Card::press`] and [`Card::look`] return through the terminal's
//! own key path, so the agent gets exactly what the keyboard would have sent.

use sshee_term::question::{Axis, ChoiceKind, How, Keys, Question, Reading};

use crate::workbench::TabId;

/// How long after a question appears on the card nothing answers it.
pub const GRACE_MS: u64 = 300;
/// How long the first click on a standing permission on Production waits for the second.
pub const SECOND_CLICK_MS: u64 = 4_000;
/// How long the agent's highlight has to arrive on the chosen answer.
pub const MOVE_WAIT_MS: u64 = 3_000;
/// How long the agent has to stop asking before your words would follow.
pub const WORDS_WAIT_MS: u64 = 5_000;
/// How long a card that is done stays up.
pub const DONE_SHOWN_MS: u64 = 2_500;
/// The longest answer in your own words.
pub const MAX_WORDS: usize = 2_000;

/// One key for the terminal to type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stroke {
    Text(String),
    Return,
    Escape,
    Up,
    Down,
    Left,
    Right,
}

/// What was done on the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Press {
    /// A click on the answer at this place.
    Answer(usize),
    /// A digit key.
    Digit(u8),
    Return,
    Up,
    Down,
    Escape,
    /// Return in the text field, with what it holds.
    Words(String),
}

/// What is true at the moment of a press or a look.
#[derive(Debug, Clone, Copy)]
pub struct Scene<'a> {
    /// Milliseconds on a clock that only goes forward.
    pub now: u64,
    /// The tab's screen, read just now. A screen that can't be read (no terminal yet,
    /// scrolled back) is `Unsure`.
    pub reading: &'a Reading,
    /// The tab's session is connected.
    pub connected: bool,
    /// The card has the keyboard.
    pub keyboard: bool,
    /// Keys were typed in the tab's terminal since the card opened.
    pub typed: bool,
}

/// Why an answer wasn't sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotSent {
    /// The tab isn't connected. The answer is kept.
    Reconnecting,
    /// The agent's highlight didn't arrive on the answer.
    DidNotMove,
    /// The agent kept asking after the answer's key: the words stayed here.
    StillAsking,
    /// The question changed between the first key and the rest.
    Changed,
}

/// How a card ended, or what went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// What was typed, as the card says it: "1", "y", "Allow once".
    Sent(String),
    NotSent(NotSent),
    /// The question went away after keys were typed in its terminal.
    Answered,
    /// The question went away by itself.
    Stopped,
}

/// What a press did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Did {
    Nothing,
    /// Type these into the tab's terminal, in order.
    Typed(Vec<Stroke>),
    /// The card closes and the keyboard goes back to the terminal.
    Closed,
    /// This answer is given in the tab itself: go there.
    GoToTab,
}

/// An answer on its way that needs another look at the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    /// The highlight was sent towards the answer at `want`.
    Move { want: usize, since: u64 },
    /// The answer's key was typed; the words wait for the question to go.
    Words { words: String, key: String, since: u64 },
}

/// How an answer's row is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowLook {
    Rest,
    /// The one Return sends.
    Hot,
    /// Sent a moment ago.
    Down,
    /// A standing permission on Production, clicked once.
    Armed,
}

/// The card of one tab's question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub tab: TabId,
    /// The agent's name: "Claude Code".
    pub agent: String,
    /// The tab's server is Production.
    pub prod: bool,
    /// The question as the card shows it; `None`: nothing here can be answered.
    question: Option<Question>,
    /// When the question now shown appeared on the card.
    shown_at: u64,
    /// It took another question's place while the card was open.
    replaced: bool,
    selected: Option<usize>,
    armed: Option<(usize, u64)>,
    writing: Option<usize>,
    /// The answer that wasn't sent, kept for another try.
    kept: Option<usize>,
    sent: Option<usize>,
    outcome: Option<(Outcome, u64)>,
    pending: Option<Pending>,
}

fn stroke_of(keys: &Keys) -> Stroke {
    match keys {
        Keys::Text(t) => Stroke::Text(t.clone()),
        Keys::Escape => Stroke::Escape,
    }
}

/// Your words as they may be typed: one line, no control characters (a line break would
/// press Return half-way through).
pub fn clean_words(words: &str) -> String {
    let one_line: String = words.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    one_line.trim().chars().take(MAX_WORDS).collect()
}

impl Card {
    /// A card for `tab`, opened at `now` on what its screen reads.
    pub fn open(tab: TabId, agent: &str, prod: bool, reading: &Reading, now: u64) -> Card {
        let mut card = Card {
            tab,
            agent: agent.to_owned(),
            prod,
            question: None,
            shown_at: now,
            replaced: false,
            selected: None,
            armed: None,
            writing: None,
            kept: None,
            sent: None,
            outcome: None,
            pending: None,
        };
        card.show(reading.question().cloned(), now);
        card
    }

    /// Puts a question on the card, as new: nothing answers it for a moment, and what was
    /// chosen for the one before is forgotten.
    fn show(&mut self, question: Option<Question>, now: u64) {
        // Elsewhere than Production the first answer is the one Return sends.
        self.selected = question.as_ref().filter(|_| !self.prod).map(|_| 0);
        self.question = question;
        self.shown_at = now;
        self.armed = None;
        self.writing = None;
        self.kept = None;
        self.pending = None;
    }

    pub fn question(&self) -> Option<&Question> {
        self.question.as_ref()
    }

    /// The question shown took another's place while the card was open.
    pub fn replaced(&self) -> bool {
        self.replaced
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref().map(|(o, _)| o)
    }

    /// The answer being written in your own words, by its place.
    pub fn writing(&self) -> Option<usize> {
        self.writing
    }

    /// An answer is on its way and waits for the agent's screen.
    pub fn sending(&self) -> bool {
        self.pending.is_some()
    }

    /// The card has nothing more to do: it was answered, here or elsewhere.
    pub fn done(&self) -> bool {
        matches!(self.outcome(), Some(Outcome::Sent(_) | Outcome::Answered | Outcome::Stopped))
    }

    /// The agent no longer asks what the card shows: its answers are of no use.
    pub fn gone(&self) -> bool {
        matches!(self.outcome(), Some(Outcome::Answered | Outcome::Stopped))
    }

    /// When the window may take a card that is done off the screen.
    pub fn closes_at(&self) -> Option<u64> {
        self.outcome.as_ref().filter(|_| self.done()).map(|(_, at)| at + DONE_SHOWN_MS)
    }

    /// When the card next changes without anything happening (a wait running out).
    pub fn next_look(&self) -> Option<u64> {
        let pending = self.pending.as_ref().map(|p| match p {
            Pending::Move { since, .. } => since + MOVE_WAIT_MS + 1,
            Pending::Words { since, .. } => since + WORDS_WAIT_MS + 1,
        });
        let armed = self.armed.map(|(_, at)| at + SECOND_CLICK_MS + 1);
        [pending, armed, self.closes_at()].into_iter().flatten().min()
    }

    /// How the answer at `place` is drawn.
    pub fn row(&self, place: usize) -> RowLook {
        if self.sent == Some(place) {
            RowLook::Down
        } else if self.armed.is_some_and(|(i, _)| i == place) {
            RowLook::Armed
        } else if self.selected == Some(place) && !self.done() {
            RowLook::Hot
        } else {
            RowLook::Rest
        }
    }

    /// The answer at `place` can't be given from the card: the agent has no key for it and
    /// wants words (OpenCode's "Reject", oh-my-pi's "Other").
    pub fn in_tab_only(&self, place: usize) -> bool {
        self.question.as_ref().is_some_and(|q| matches!(q.how, How::Highlight(_)) && q.options.get(place).is_some_and(|o| o.kind == ChoiceKind::Text))
    }

    /// One line about what happened, when something did.
    pub fn status(&self) -> Option<String> {
        let agent = &self.agent;
        if let Some((outcome, _)) = &self.outcome {
            return Some(match outcome {
                Outcome::Sent(what) => format!("Sent {what} to {agent}"),
                Outcome::NotSent(NotSent::Reconnecting) => "Not sent: the tab is reconnecting. Your answer is kept.".to_owned(),
                Outcome::NotSent(NotSent::DidNotMove) => format!("Not sent: {agent} didn't move to that answer."),
                Outcome::NotSent(NotSent::StillAsking) => format!("{agent} is still asking. Your words weren't sent."),
                Outcome::NotSent(NotSent::Changed) => "The question changed. Your words weren't sent.".to_owned(),
                Outcome::Answered => "Answered in the terminal.".to_owned(),
                Outcome::Stopped => format!("{agent} stopped asking."),
            });
        }
        if let Some(Pending::Words { key, .. }) = &self.pending {
            return Some(format!("Sent {key}. Your words follow when {agent} is ready."));
        }
        if self.pending.is_some() {
            return Some(format!("Moving {agent}'s highlight…"));
        }
        self.replaced.then(|| "The question changed while you read. Nothing was sent.".to_owned())
    }

    /// The card's foot: what the keys do now, and how to leave.
    pub fn hints(&self, keyboard: bool) -> (String, String) {
        let esc = if keyboard { "Esc to decide later" } else { "" };
        let Some(q) = &self.question else { return ("Open the tab to answer".to_owned(), esc.to_owned()) };
        if self.done() {
            return (String::new(), String::new());
        }
        if self.writing.is_some() {
            return ("Return sends your words".to_owned(), "Esc goes back".to_owned());
        }
        if let Some((place, _)) = self.armed {
            let n = q.options.get(place).map_or(0, |o| o.number);
            return (format!("{n} again sends it. {} won't ask next time.", self.agent), esc.to_owned());
        }
        if !keyboard {
            return ("Click an answer. Keys still go to the terminal.".to_owned(), String::new());
        }
        let n = q.options.len();
        let digits = match n {
            1 => "1".to_owned(),
            2 => "1 or 2".to_owned(),
            3 => "1, 2 or 3".to_owned(),
            _ => format!("1 to {n}"),
        };
        (format!("Press {digits}, or click an answer"), esc.to_owned())
    }

    /// The screen was read again, or time passed. Brings the card up to date with what the
    /// agent shows now, and returns keys only for an answer already on its way.
    pub fn look(&mut self, at: &Scene) -> Vec<Stroke> {
        if let Some(pending) = self.pending.take() {
            return self.carry_on(pending, at);
        }
        if self.done() {
            return Vec::new();
        }
        // The second click didn't come: the first is forgotten, the row with it.
        if self.armed.is_some_and(|(_, since)| at.now.saturating_sub(since) > SECOND_CLICK_MS) {
            self.armed = None;
            self.selected = None;
        }
        match (&self.question, at.reading) {
            // The agent's own cursor may have moved; the question is the same.
            (Some(shown), Reading::Question(live)) if shown.same_as(live) => {}
            (shown, Reading::Question(live)) => {
                // Said until the card closes: the screen moved under the reader once.
                self.replaced |= shown.is_some();
                self.show(Some(live.clone()), at.now);
                self.outcome = None;
            }
            (Some(_), Reading::Nothing) => self.outcome = Some((if at.typed { Outcome::Answered } else { Outcome::Stopped }, at.now)),
            // It can't be read any more: nothing here can answer it.
            (Some(_), Reading::Unsure(_)) => {
                self.replaced = true;
                self.show(None, at.now);
            }
            (None, _) => {}
        }
        Vec::new()
    }

    /// The second half of an answer, once the agent's screen shows it is safe.
    fn carry_on(&mut self, pending: Pending, at: &Scene) -> Vec<Stroke> {
        if !at.connected {
            self.outcome = Some((Outcome::NotSent(NotSent::Reconnecting), at.now));
            return Vec::new();
        }
        let same = |live: &Question| self.question.as_ref().is_some_and(|q| q.same_as(live));
        match pending {
            Pending::Move { want, since } => match at.reading {
                Reading::Question(live) if same(live) && live.highlighted == want => {
                    let label = live.options.get(want).map(|o| o.label.clone()).unwrap_or_default();
                    self.outcome = Some((Outcome::Sent(label), at.now));
                    self.sent = Some(want);
                    vec![Stroke::Return]
                }
                Reading::Question(live) if same(live) && at.now.saturating_sub(since) <= MOVE_WAIT_MS => {
                    self.pending = Some(Pending::Move { want, since });
                    Vec::new()
                }
                Reading::Question(live) if same(live) => {
                    self.outcome = Some((Outcome::NotSent(NotSent::DidNotMove), at.now));
                    Vec::new()
                }
                // Gone or another question: Return would answer something else.
                _ => {
                    self.outcome = Some((Outcome::NotSent(NotSent::Changed), at.now));
                    Vec::new()
                }
            },
            Pending::Words { words, key, since } => match at.reading {
                // The agent took the key and asks nothing now: it waits for the words.
                Reading::Nothing => {
                    self.outcome = Some((Outcome::Sent(format!("{key} and your words")), at.now));
                    self.sent = self.writing.take();
                    vec![Stroke::Text(words), Stroke::Return]
                }
                Reading::Question(live) if same(live) && at.now.saturating_sub(since) <= WORDS_WAIT_MS => {
                    self.pending = Some(Pending::Words { words, key, since });
                    Vec::new()
                }
                Reading::Question(live) if same(live) => {
                    self.outcome = Some((Outcome::NotSent(NotSent::StillAsking), at.now));
                    Vec::new()
                }
                // Another question, or a screen that can't be read: the words could land on
                // it as answers.
                _ => {
                    self.outcome = Some((Outcome::NotSent(NotSent::Changed), at.now));
                    Vec::new()
                }
            },
        }
    }

    /// A click or a key on the card.
    pub fn press(&mut self, press: Press, at: &Scene) -> Did {
        // First what the screen says now: a question that changed since it was drawn takes
        // the card over as new, and this very press is too early for it.
        let carried = self.look(at);
        if !carried.is_empty() {
            return Did::Typed(carried);
        }
        if press == Press::Escape {
            if self.writing.take().is_some() {
                return Did::Nothing;
            }
            return Did::Closed;
        }
        if self.done() || self.pending.is_some() {
            return Did::Nothing;
        }
        let Some(q) = self.question.clone() else { return Did::Nothing };
        let by_keys = !matches!(press, Press::Answer(_));
        if by_keys && !at.keyboard {
            return Did::Nothing;
        }
        // Moving the choice answers nothing.
        if let Press::Up | Press::Down = press {
            if self.writing.is_none() {
                let last = q.options.len().saturating_sub(1);
                self.selected = Some(match (self.selected, &press) {
                    (None, Press::Down) => 0,
                    (None, _) => last,
                    (Some(i), Press::Down) => (i + 1).min(last),
                    (Some(i), _) => i.saturating_sub(1),
                });
                self.armed = None;
            }
            return Did::Nothing;
        }
        if at.now.saturating_sub(self.shown_at) < GRACE_MS {
            return Did::Nothing;
        }
        // A try after "not sent" starts clean.
        if matches!(self.outcome(), Some(Outcome::NotSent(_))) {
            self.outcome = None;
        }
        match press {
            Press::Words(words) => {
                let Some(place) = self.writing else { return Did::Nothing };
                let words = clean_words(&words);
                if words.is_empty() {
                    return Did::Nothing;
                }
                self.send(&q, place, Some(words), at)
            }
            // While words are being written, the keys are the field's.
            _ if self.writing.is_some() => Did::Nothing,
            Press::Answer(place) if place < q.options.len() => self.choose(&q, place, at),
            Press::Digit(n) => match q.options.iter().position(|o| o.number == n) {
                Some(place) => self.choose(&q, place, at),
                None => Did::Nothing,
            },
            Press::Return => match self.selected {
                Some(place) => self.choose(&q, place, at),
                None => Did::Nothing,
            },
            _ => Did::Nothing,
        }
    }

    /// An answer was picked: what it takes before it is sent.
    fn choose(&mut self, q: &Question, place: usize, at: &Scene) -> Did {
        let option = &q.options[place];
        self.selected = Some(place);
        if option.kind == ChoiceKind::Text {
            if q.how != How::Key {
                return Did::GoToTab;
            }
            self.armed = None;
            self.writing = Some(place);
            return Did::Nothing;
        }
        // A standing permission on Production: once more, soon, on the same answer.
        if option.kind == ChoiceKind::Always && self.prod {
            let again = self.armed.is_some_and(|(i, since)| i == place && at.now.saturating_sub(since) <= SECOND_CLICK_MS);
            if !again {
                self.armed = Some((place, at.now));
                return Did::Nothing;
            }
        }
        self.send(q, place, None, at)
    }

    /// Types the answer, or says why not.
    fn send(&mut self, q: &Question, place: usize, words: Option<String>, at: &Scene) -> Did {
        self.armed = None;
        if !at.connected {
            self.outcome = Some((Outcome::NotSent(NotSent::Reconnecting), at.now));
            self.kept = Some(place);
            self.selected = Some(place);
            return Did::Nothing;
        }
        // `look` has just matched the card to the screen; a card that still differs sends nothing.
        let Some(live) = at.reading.question().filter(|live| q.same_as(live)) else { return Did::Nothing };
        self.kept = None;
        match q.how {
            How::Key => {
                let Some(keys) = &q.options[place].keys else { return Did::Nothing };
                match words {
                    Some(words) => self.pending = Some(Pending::Words { words, key: keys.shown().to_owned(), since: at.now }),
                    None => {
                        self.outcome = Some((Outcome::Sent(keys.shown().to_owned()), at.now));
                        self.sent = Some(place);
                    }
                }
                Did::Typed(vec![stroke_of(keys)])
            }
            How::Highlight(axis) => {
                if live.highlighted == place {
                    self.outcome = Some((Outcome::Sent(q.options[place].label.clone()), at.now));
                    self.sent = Some(place);
                    return Did::Typed(vec![Stroke::Return]);
                }
                let (back, on) = match axis {
                    Axis::Rows => (Stroke::Up, Stroke::Down),
                    Axis::Columns => (Stroke::Left, Stroke::Right),
                };
                let step = if place > live.highlighted { on } else { back };
                self.pending = Some(Pending::Move { want: place, since: at.now });
                Did::Typed(vec![step; place.abs_diff(live.highlighted)])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_term::question::{read_text, Asker, Choice, Unsure};

    const TAB: TabId = TabId(3);

    fn claude(command: &str) -> Reading {
        let screen = format!(
            "────────────────────────────────────────\n Bash command\n\n   {command}\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and don't ask again for psql commands in /srv/shop-db\n   3. No, and tell Claude what to do differently (esc)\n"
        );
        let rows: Vec<&str> = screen.lines().collect();
        let reading = read_text(Asker::ClaudeCode, &rows);
        assert!(reading.question().is_some(), "{reading:?}");
        reading
    }

    fn migration() -> Reading {
        claude("psql -h db-main -d shop -f migrations/0042_orders_index.sql")
    }

    /// OpenCode's three buttons with its highlight on the one at `on`.
    fn buttons(on: usize) -> Reading {
        let choice = |n: u8, label: &str, kind| Choice { number: n, label: label.into(), kind, keys: None };
        Reading::Question(Question {
            asker: Asker::OpenCode,
            title: None,
            context: vec!["psql -f migrations/0042_orders_index.sql".into()],
            question: "Permission required".into(),
            options: vec![choice(1, "Allow once", ChoiceKind::Plain), choice(2, "Allow always", ChoiceKind::Always), choice(3, "Reject", ChoiceKind::Text)],
            highlighted: on,
            how: How::Highlight(Axis::Columns),
        })
    }

    fn scene(reading: &Reading, now: u64) -> Scene<'_> {
        Scene { now, reading, connected: true, keyboard: true, typed: false }
    }

    fn text(s: &str) -> Stroke {
        Stroke::Text(s.to_owned())
    }

    fn typed(strokes: &[Stroke]) -> Did {
        Did::Typed(strokes.to_vec())
    }

    #[test]
    fn on_production_nothing_is_chosen_and_return_alone_sends_nothing() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        assert_eq!([card.row(0), card.row(1), card.row(2)], [RowLook::Rest; 3]);
        assert_eq!(card.press(Press::Return, &scene(&q, 1_000)), Did::Nothing);
        assert_eq!(card.outcome(), None);
        // A digit answers.
        assert_eq!(card.press(Press::Digit(1), &scene(&q, 1_100)), typed(&[text("1")]));
        assert_eq!(card.outcome(), Some(&Outcome::Sent("1".into())));
        assert_eq!(card.status().as_deref(), Some("Sent 1 to Claude Code"));
        assert_eq!(card.row(0), RowLook::Down);
        assert!(card.done());
        assert_eq!(card.closes_at(), Some(1_100 + DONE_SHOWN_MS));
        // And once only.
        assert_eq!(card.press(Press::Digit(1), &scene(&q, 1_200)), Did::Nothing);
        assert_eq!(card.press(Press::Answer(0), &scene(&q, 1_300)), Did::Nothing);
    }

    #[test]
    fn on_production_a_click_or_an_arrow_and_return_answers() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        assert_eq!(card.press(Press::Answer(0), &scene(&q, 500)), typed(&[text("1")]));
        // Arrows choose, Return sends what they chose.
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        assert_eq!(card.press(Press::Down, &scene(&q, 500)), Did::Nothing);
        assert_eq!(card.row(0), RowLook::Hot);
        assert_eq!(card.press(Press::Up, &scene(&q, 600)), Did::Nothing);
        assert_eq!(card.row(0), RowLook::Hot);
        assert_eq!(card.press(Press::Return, &scene(&q, 700)), typed(&[text("1")]));
        // Up from nothing chosen is the last answer.
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        card.press(Press::Up, &scene(&q, 500));
        assert_eq!(card.row(2), RowLook::Hot);
        card.press(Press::Down, &scene(&q, 600));
        assert_eq!(card.row(2), RowLook::Hot, "the choice stops at the end");
    }

    #[test]
    fn elsewhere_the_first_answer_is_chosen_and_return_sends_it() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        assert_eq!([card.row(0), card.row(1)], [RowLook::Hot, RowLook::Rest]);
        assert_eq!(card.press(Press::Return, &scene(&q, 400)), typed(&[text("1")]));
        // One highlight, and it moves: down, then Return sends the second (no second click
        // off Production).
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        card.press(Press::Down, &scene(&q, 400));
        assert_eq!([card.row(0), card.row(1)], [RowLook::Rest, RowLook::Hot]);
        assert_eq!(card.press(Press::Return, &scene(&q, 500)), typed(&[text("2")]));
        assert_eq!(card.status().as_deref(), Some("Sent 2 to Claude Code"));
    }

    #[test]
    fn keys_count_only_while_the_card_has_the_keyboard() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        let terminal_has_it = Scene { keyboard: false, ..scene(&q, 1_000) };
        for press in [Press::Digit(1), Press::Return, Press::Down, Press::Up, Press::Words("go".into())] {
            assert_eq!(card.press(press.clone(), &terminal_has_it), Did::Nothing, "{press:?}");
        }
        assert_eq!(card.row(0), RowLook::Hot, "an arrow moved the choice without the keyboard");
        assert_eq!(card.hints(false).0, "Click an answer. Keys still go to the terminal.");
        assert_eq!(card.hints(true), ("Press 1, 2 or 3, or click an answer".to_owned(), "Esc to decide later".to_owned()));
        // A click needs no keyboard.
        assert_eq!(card.press(Press::Answer(0), &terminal_has_it), typed(&[text("1")]));
    }

    #[test]
    fn nothing_answers_in_the_first_moment() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", false, &q, 10_000);
        for press in [Press::Answer(0), Press::Digit(1), Press::Return] {
            assert_eq!(card.press(press, &scene(&q, 10_000 + GRACE_MS - 1)), Did::Nothing);
        }
        assert_eq!(card.outcome(), None);
        assert_eq!(card.press(Press::Answer(0), &scene(&q, 10_000 + GRACE_MS)), typed(&[text("1")]));
    }

    #[test]
    fn looking_at_the_card_sends_nothing() {
        // Pointing isn't a press: time passes, the screen is read again and again.
        let q = migration();
        for prod in [true, false] {
            let mut card = Card::open(TAB, "Claude Code", prod, &q, 0);
            for now in (0..20_000).step_by(250) {
                assert!(card.look(&scene(&q, now)).is_empty());
            }
            assert_eq!(card.outcome(), None);
            assert_eq!(card.next_look(), None);
        }
    }

    #[test]
    fn a_standing_permission_on_production_takes_two_clicks() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        assert_eq!(card.press(Press::Answer(1), &scene(&q, 1_000)), Did::Nothing);
        assert_eq!(card.row(1), RowLook::Armed);
        assert_eq!(card.hints(true).0, "2 again sends it. Claude Code won't ask next time.");
        assert_eq!(card.next_look(), Some(1_000 + SECOND_CLICK_MS + 1));
        // Another answer in between is its own answer, not the second click.
        let mut other = card.clone();
        assert_eq!(other.press(Press::Digit(1), &scene(&q, 1_500)), typed(&[text("1")]));
        // The second click in time sends it.
        let mut again = card.clone();
        assert_eq!(again.press(Press::Digit(2), &scene(&q, 1_000 + SECOND_CLICK_MS)), typed(&[text("2")]));
        // Too late: it is a first click again.
        assert!(card.look(&scene(&q, 1_000 + SECOND_CLICK_MS + 1)).is_empty());
        assert_eq!(card.row(1), RowLook::Rest);
        assert_eq!(card.press(Press::Answer(1), &scene(&q, 6_000)), Did::Nothing);
        assert_eq!(card.row(1), RowLook::Armed);
        assert_eq!(card.press(Press::Answer(1), &scene(&q, 6_500)), typed(&[text("2")]));
        // Off Production one click is enough.
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        assert_eq!(card.press(Press::Answer(1), &scene(&q, 1_000)), typed(&[text("2")]));
    }

    #[test]
    fn your_own_words_go_after_the_agent_stopped_asking() {
        let q = migration();
        let gone = Reading::Nothing;
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        // The third answer turns into a field; nothing is sent yet.
        assert_eq!(card.press(Press::Digit(3), &scene(&q, 1_000)), Did::Nothing);
        assert_eq!(card.writing(), Some(2));
        assert_eq!(card.hints(true), ("Return sends your words".to_owned(), "Esc goes back".to_owned()));
        // Digits are words now, and an empty field sends nothing.
        assert_eq!(card.press(Press::Digit(1), &scene(&q, 1_100)), Did::Nothing);
        assert_eq!(card.press(Press::Words("   ".into()), &scene(&q, 1_200)), Did::Nothing);
        // Esc goes back to the answers; again, and Return sends: the key first.
        assert_eq!(card.press(Press::Escape, &scene(&q, 1_300)), Did::Nothing);
        assert_eq!(card.writing(), None);
        card.press(Press::Answer(2), &scene(&q, 1_400));
        assert_eq!(card.press(Press::Words("Use a transaction\nand CONCURRENTLY\u{7}".into()), &scene(&q, 2_000)), typed(&[text("3")]));
        assert!(card.sending());
        assert_eq!(card.status().as_deref(), Some("Sent 3. Your words follow when Claude Code is ready."));
        // While the question is still up, the words wait.
        assert!(card.look(&scene(&q, 2_100)).is_empty());
        // It left the screen: the words, on one line, and Return.
        assert_eq!(card.look(&scene(&gone, 2_300)), [text("Use a transaction and CONCURRENTLY"), Stroke::Return]);
        assert_eq!(card.status().as_deref(), Some("Sent 3 and your words to Claude Code"));
        assert!(card.done());
        assert!(card.look(&scene(&gone, 2_400)).is_empty());
    }

    #[test]
    fn words_never_land_on_another_question() {
        let q = migration();
        let other = claude("rm -rf /srv/shop-db/backups");
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        card.press(Press::Digit(3), &scene(&q, 1_000));
        assert_eq!(card.press(Press::Words("1 more thing".into()), &scene(&q, 2_000)), typed(&[text("3")]));
        // The agent asks something else at once: "1" would have said Yes to it.
        assert!(card.look(&scene(&other, 2_200)).is_empty());
        assert_eq!(card.outcome(), Some(&Outcome::NotSent(NotSent::Changed)));
        assert_eq!(card.status().as_deref(), Some("The question changed. Your words weren't sent."));
        // The same when the agent keeps asking past the wait, or the screen can't be read.
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        card.press(Press::Digit(3), &scene(&q, 1_000));
        card.press(Press::Words("no".into()), &scene(&q, 2_000));
        assert_eq!(card.next_look(), Some(2_000 + WORDS_WAIT_MS + 1));
        assert!(card.look(&scene(&q, 2_000 + WORDS_WAIT_MS + 1)).is_empty());
        assert_eq!(card.outcome(), Some(&Outcome::NotSent(NotSent::StillAsking)));
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        card.press(Press::Digit(3), &scene(&q, 1_000));
        card.press(Press::Words("no".into()), &scene(&q, 2_000));
        assert!(card.look(&scene(&Reading::Unsure(Unsure::Trailing), 2_100)).is_empty());
        assert_eq!(card.outcome(), Some(&Outcome::NotSent(NotSent::Changed)));
    }

    #[test]
    fn a_tab_that_is_reconnecting_gets_nothing_and_the_answer_is_kept() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        let down = Scene { connected: false, ..scene(&q, 1_000) };
        assert_eq!(card.press(Press::Digit(1), &down), Did::Nothing);
        assert_eq!(card.outcome(), Some(&Outcome::NotSent(NotSent::Reconnecting)));
        assert_eq!(card.status().as_deref(), Some("Not sent: the tab is reconnecting. Your answer is kept."));
        // Kept: it is the chosen row, on Production too, and nothing goes by itself when the
        // connection is back.
        assert_eq!(card.row(0), RowLook::Hot);
        assert!(!card.done());
        assert!(card.look(&scene(&q, 5_000)).is_empty());
        assert!(card.look(&scene(&q, 9_000)).is_empty());
        // Return sends it, after the screen is read again.
        assert_eq!(card.press(Press::Return, &scene(&q, 9_500)), typed(&[text("1")]));
        assert_eq!(card.status().as_deref(), Some("Sent 1 to Claude Code"));
    }

    #[test]
    fn a_question_that_changed_while_you_read_takes_the_card_and_the_click_is_dropped() {
        let q = migration();
        let other = claude("rm -rf /srv/shop-db/backups");
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        // The click lands the moment the screen shows another question.
        assert_eq!(card.press(Press::Answer(0), &scene(&other, 5_000)), Did::Nothing);
        assert_eq!(card.outcome(), None);
        assert!(card.replaced());
        assert_eq!(card.status().as_deref(), Some("The question changed while you read. Nothing was sent."));
        assert_eq!(card.question().unwrap().context, ["rm -rf /srv/shop-db/backups"]);
        // And a second one right after is still too early.
        assert_eq!(card.press(Press::Answer(0), &scene(&other, 5_000 + GRACE_MS - 1)), Did::Nothing);
        // Later it answers the new question, knowingly.
        assert_eq!(card.press(Press::Answer(0), &scene(&other, 5_000 + GRACE_MS)), typed(&[text("1")]));
        // A first click on a standing permission doesn't carry over to the new question.
        let mut card = Card::open(TAB, "Claude Code", true, &q, 0);
        card.press(Press::Answer(1), &scene(&q, 1_000));
        assert_eq!(card.row(1), RowLook::Armed);
        assert_eq!(card.press(Press::Answer(1), &scene(&other, 2_000)), Did::Nothing);
        assert_eq!(card.row(1), RowLook::Rest);
        assert_eq!(card.press(Press::Answer(1), &scene(&other, 2_500)), Did::Nothing);
        assert_eq!(card.row(1), RowLook::Armed, "the new question needs both clicks itself");
    }

    #[test]
    fn answered_in_the_terminal_or_the_agent_stopped_asking() {
        let q = migration();
        let gone = Reading::Nothing;
        // Keys were typed in its terminal and the question went.
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        assert!(card.look(&Scene { typed: true, ..scene(&gone, 3_000) }).is_empty());
        assert_eq!(card.outcome(), Some(&Outcome::Answered));
        assert_eq!(card.status().as_deref(), Some("Answered in the terminal."));
        assert_eq!(card.press(Press::Digit(1), &scene(&gone, 4_000)), Did::Nothing);
        // It went by itself.
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        assert_eq!(card.press(Press::Return, &scene(&gone, 3_000)), Did::Nothing);
        assert_eq!(card.outcome(), Some(&Outcome::Stopped));
        assert_eq!(card.status().as_deref(), Some("Claude Code stopped asking."));
        assert_eq!(card.closes_at(), Some(3_000 + DONE_SHOWN_MS));
        assert_eq!(card.hints(true), (String::new(), String::new()));
    }

    #[test]
    fn a_screen_that_cannot_be_read_is_never_answered() {
        for reading in [Reading::Unsure(Unsure::Cut), Reading::Unsure(Unsure::Several), Reading::Nothing] {
            for prod in [true, false] {
                let mut card = Card::open(TAB, "Codex", prod, &reading, 0);
                assert_eq!(card.question(), None);
                assert_eq!(card.hints(true).0, "Open the tab to answer");
                for press in [Press::Answer(0), Press::Digit(1), Press::Return, Press::Down, Press::Words("yes".into())] {
                    assert_eq!(card.press(press, &scene(&reading, 2_000)), Did::Nothing);
                }
                assert_eq!(card.outcome(), None);
                // Esc still leaves.
                assert_eq!(card.press(Press::Escape, &scene(&reading, 2_100)), Did::Closed);
            }
        }
        // A question that stops being readable while the card is open: the same.
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        let unsure = Reading::Unsure(Unsure::Trailing);
        assert_eq!(card.press(Press::Return, &scene(&unsure, 2_000)), Did::Nothing);
        assert_eq!(card.question(), None);
        assert_eq!(card.press(Press::Return, &scene(&unsure, 3_000)), Did::Nothing);
        // It reads again: the card takes it as new.
        assert_eq!(card.press(Press::Return, &scene(&q, 4_000)), Did::Nothing);
        assert_eq!(card.press(Press::Return, &scene(&q, 4_000 + GRACE_MS)), typed(&[text("1")]));
    }

    #[test]
    fn a_digit_no_answer_has_does_nothing() {
        let q = migration();
        let mut card = Card::open(TAB, "Claude Code", false, &q, 0);
        for n in [0, 4, 9] {
            assert_eq!(card.press(Press::Digit(n), &scene(&q, 1_000)), Did::Nothing);
        }
        assert_eq!(card.press(Press::Answer(7), &scene(&q, 1_000)), Did::Nothing);
        assert_eq!(card.outcome(), None);
    }

    #[test]
    fn codex_gets_the_key_its_screen_prints() {
        let rows = [
            "  Would you like to run the following command?",
            "",
            "  $ npm test",
            "",
            "› 1. Yes, proceed (y)",
            "  2. Yes, and don't ask again for this command (a)",
            "  3. No, and tell Codex what to do differently (esc)",
            "",
            "  Press enter to confirm or esc to cancel",
        ];
        let q = read_text(Asker::Codex, &rows);
        let mut card = Card::open(TAB, "Codex", false, &q, 0);
        // The card's own keys are still 1, 2, 3; what is typed is Codex's.
        assert_eq!(card.press(Press::Digit(1), &scene(&q, 1_000)), typed(&[text("y")]));
        assert_eq!(card.status().as_deref(), Some("Sent y to Codex"));
        let mut card = Card::open(TAB, "Codex", false, &q, 0);
        card.press(Press::Digit(3), &scene(&q, 1_000));
        assert_eq!(card.press(Press::Words("run only the unit tests".into()), &scene(&q, 2_000)), typed(&[Stroke::Escape]));
        assert_eq!(card.look(&scene(&Reading::Nothing, 2_500)), [text("run only the unit tests"), Stroke::Return]);
        assert_eq!(card.status().as_deref(), Some("Sent Esc and your words to Codex"));
    }

    #[test]
    fn a_highlight_is_moved_first_and_return_follows_only_when_it_arrived() {
        // The highlight is already on the answer: Return.
        let q = buttons(0);
        let mut card = Card::open(TAB, "OpenCode", false, &q, 0);
        assert_eq!(card.press(Press::Digit(1), &scene(&q, 1_000)), typed(&[Stroke::Return]));
        assert_eq!(card.status().as_deref(), Some("Sent Allow once to OpenCode"));
        // It is elsewhere: move it, and wait for the screen to show it there.
        let at_reject = buttons(2);
        let mut card = Card::open(TAB, "OpenCode", false, &at_reject, 0);
        assert_eq!(card.press(Press::Answer(0), &scene(&at_reject, 1_000)), typed(&[Stroke::Left, Stroke::Left]));
        assert!(card.sending());
        assert_eq!(card.outcome(), None);
        // Half-way: nothing yet.
        assert!(card.look(&scene(&buttons(1), 1_100)).is_empty());
        // Arrived: Return.
        assert_eq!(card.look(&scene(&buttons(0), 1_200)), [Stroke::Return]);
        assert_eq!(card.outcome(), Some(&Outcome::Sent("Allow once".into())));
        // It never arrives: no Return.
        let mut card = Card::open(TAB, "OpenCode", false, &at_reject, 0);
        card.press(Press::Answer(0), &scene(&at_reject, 1_000));
        assert!(card.look(&scene(&at_reject, 1_000 + MOVE_WAIT_MS + 1)).is_empty());
        assert_eq!(card.outcome(), Some(&Outcome::NotSent(NotSent::DidNotMove)));
        assert_eq!(card.status().as_deref(), Some("Not sent: OpenCode didn't move to that answer."));
        // The question goes while the highlight is on its way: no Return either.
        let mut card = Card::open(TAB, "OpenCode", false, &at_reject, 0);
        card.press(Press::Answer(0), &scene(&at_reject, 1_000));
        assert!(card.look(&scene(&Reading::Nothing, 1_100)).is_empty());
        assert_eq!(card.outcome(), Some(&Outcome::NotSent(NotSent::Changed)));
        // The connection drops on the way: the same.
        let mut card = Card::open(TAB, "OpenCode", false, &at_reject, 0);
        card.press(Press::Answer(0), &scene(&at_reject, 1_000));
        assert!(card.look(&Scene { connected: false, ..scene(&buttons(0), 1_100) }).is_empty());
        assert_eq!(card.outcome(), Some(&Outcome::NotSent(NotSent::Reconnecting)));
    }

    #[test]
    fn an_answer_in_words_without_a_key_is_given_in_the_tab() {
        let q = buttons(0);
        let mut card = Card::open(TAB, "OpenCode", false, &q, 0);
        assert!(card.in_tab_only(2) && !card.in_tab_only(0));
        assert_eq!(card.press(Press::Digit(3), &scene(&q, 1_000)), Did::GoToTab);
        assert_eq!(card.outcome(), None);
        // "Allow always" on Production still takes two clicks before the highlight moves.
        let mut card = Card::open(TAB, "OpenCode", true, &q, 0);
        assert_eq!(card.press(Press::Answer(1), &scene(&q, 1_000)), Did::Nothing);
        assert_eq!(card.press(Press::Answer(1), &scene(&q, 1_500)), typed(&[Stroke::Right]));
    }

    #[test]
    fn words_are_one_line_without_control_characters() {
        assert_eq!(clean_words("  use a transaction \r\n and\tCONCURRENTLY\u{1b}[A  "), "use a transaction    and CONCURRENTLY [A");
        assert_eq!(clean_words("\n\n"), "");
        assert_eq!(clean_words(&"x".repeat(MAX_WORDS + 50)).len(), MAX_WORDS);
    }
}
