//! AI agents in the window: the working mark and "Needs you" on their tabs, the card under a
//! waiting agent's tab where its question is answered without going there, and the list of
//! every agent on every server (Ctrl+Shift+A, ⌘⇧A on a Mac).
//!
//! The rules are not here: which program is an agent, what its tab is called and where it
//! stands are `sshee_app::agents`; what a click or a key on the card does and what is typed
//! for it, `sshee_app::agents::answer`; reading the question off the agent's screen,
//! `sshee_term::question`. This module feeds them (output, bells, the clock, the screen) and
//! draws what they say.
//!
//! How the card comes and goes:
//!
//! - Resting the pointer on a waiting agent's tab opens it. It doesn't have the keyboard
//!   then: keys still go to the terminal, and it leaves when the pointer does.
//! - A click on it, or Return on the agent in the list, gives it the keyboard: a blue ring,
//!   digits and arrows work, and nothing typed reaches the terminal until Esc or a click
//!   outside gives the keyboard back.
//! - A click on the tab itself goes to the tab, as always.
//!
//! What it types goes through the terminal's own key path (`TermPane::type_key`), after the
//! agent's screen was read again in the same handler.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use rune::{canvas, col, el, row, spacer, text, text_input, Align, Cx, El, Key, KeyEvent, LineCap, Mods, Shape, Side, TextEdit, Tx};
use sshee_app::agents::answer::{Card, Did, Outcome, Press, RowLook, Scene, Stroke};
use sshee_app::agents::{server_shown, waited, AgentKind, AgentRow, AgentState, Agents, Standing};
use sshee_app::workbench::{Link, Server, ServerKey, Tab, TabId, TabKind};
use sshee_term::attention::{AttentionKind, QUIET_MS};
use sshee_term::present::Look;
use sshee_term::question::{self, ChoiceKind, Reading, Unsure};

use crate::icons::{icon, Icon};
use crate::kit::chip::{chip, prod_chip};
use crate::kit::dot;
use crate::kit::menu::{key_cap, menu_row, RowState};
use crate::kit::popover::{popover, popover_foot};
use crate::shell::Shell;
use crate::svg::{self, Fit};
use crate::theme::{self, Theme};

#[cfg(test)]
mod tests;

/// The pointer rests on a waiting agent's tab this long before its card opens.
const HOVER_OPEN_MS: u64 = 450;
/// A card without the keyboard leaves this long after the pointer left it and its tab.
const HOVER_CLOSE_MS: u64 = 300;
/// No card opens under the pointer while you type: this long since the last key.
const TYPING_HOLD_MS: u64 = 1_000;
/// The window's clock starts here, so "two minutes ago" at the start is still a time.
const CLOCK_START_MS: u64 = 3_600_000;
/// The most of a command or a change a card shows.
const CONTEXT_LINES: usize = 8;

/// The window's clock in milliseconds.
fn ms(clock: f64) -> u64 {
    CLOCK_START_MS + (clock.max(0.0) * 1000.0) as u64
}

/// The agents' part of the window's state.
pub struct State {
    /// Every terminal's agent and attention.
    pub agents: Agents,
    /// Streamer mode: a card shows no question, a notification no names.
    pub streaming: bool,
    /// Settings: tell me when a terminal needs me.
    pub notify: bool,
    /// The card under a tab, when one is open.
    card: Option<Card>,
    /// The card has the keyboard.
    keyboard: bool,
    /// The card was opened with the keyboard and isn't drawn yet: it is focused once it is
    /// (the list it was picked from takes the focus with it as it closes).
    focus_card: bool,
    /// What is being written as an answer in your own words.
    words: TextEdit,
    /// When keys were last typed in the card's terminal, as the card opened.
    typed_before: Option<f64>,
    /// The list of agents is open: the row Return acts on.
    list: Option<usize>,
    /// The waiting agent's tab the pointer rests on, and since when.
    hover: Option<(TabId, u64)>,
    over_card: bool,
    /// When the pointer left the card and its tab.
    left_at: Option<u64>,
    /// Each terminal's screen as last read: the frame it was read at, and what it read.
    readings: HashMap<TabId, (u64, Reading)>,
    /// The frames each pane had taken at the last poll: more since means output came.
    frames: HashMap<TabId, u64>,
    /// The tabs drawn with the working mark.
    working: HashSet<TabId>,
    /// When output last came anywhere, for the "went quiet" check.
    last_output: Option<u64>,
    /// The servers the backend was last told to ask about agents.
    asking: Vec<ServerKey>,
    /// The clock at the last poll.
    now: u64,
}

impl Default for State {
    fn default() -> Self {
        State {
            agents: Agents::default(),
            streaming: false,
            notify: true,
            card: None,
            keyboard: false,
            focus_card: false,
            words: TextEdit::default(),
            typed_before: None,
            list: None,
            hover: None,
            over_card: false,
            left_at: None,
            readings: HashMap::new(),
            frames: HashMap::new(),
            working: HashSet::new(),
            last_output: None,
            asking: Vec::new(),
            now: CLOCK_START_MS,
        }
    }
}

impl State {
    /// The tabs that are gone take what was kept about them.
    pub fn retain(&mut self, open: &impl Fn(TabId) -> bool) {
        self.agents.retain(open);
        self.readings.retain(|id, _| open(*id));
        self.frames.retain(|id, _| open(*id));
        self.working.retain(|id| open(*id));
        if self.card.as_ref().is_some_and(|c| !open(c.tab)) {
            self.card = None;
            self.keyboard = false;
        }
        if self.hover.is_some_and(|(id, _)| !open(id)) {
            self.hover = None;
        }
    }

    /// An agent is at work in this tab: the quiet ring after its name.
    pub fn working(&self, tab: TabId) -> bool {
        self.working.contains(&tab)
    }

    /// The card that is open, if one is.
    pub fn card(&self) -> Option<&Card> {
        self.card.as_ref()
    }

    /// The card has the keyboard.
    pub fn has_keyboard(&self) -> bool {
        self.card.is_some() && self.keyboard
    }

    /// The list of agents is open.
    pub fn list_open(&self) -> bool {
        self.list.is_some()
    }

    /// How text from a terminal may be shown outside it: secrets always dotted.
    fn look(&self) -> Look {
        Look { streaming: self.streaming, hide_secrets: true, ..Look::default() }
    }
}

/// A tab that has been waiting since before the window opened (a demo's scene): every tab
/// that says "Needs you" has waited `secs`.
pub fn seed_wait(s: &mut Shell, secs: u64) {
    let waiting: Vec<TabId> = s.bench.tabs().iter().filter(|t| t.needs_you).map(|t| t.id).collect();
    let st = &mut s.screens.agent;
    for id in waiting {
        st.agents.waiting_from(id, st.now.saturating_sub(secs * 1000));
    }
}

// ---------------------------------------------------------------------------------------------
// What the window tells the agents

/// Where the window stands for a tab's attention.
fn standing<'a>(window_focused: bool, on_screen: bool, notify: bool, look: &'a Look) -> Standing<'a> {
    Standing { watched: window_focused && on_screen, app_focused: window_focused, notify, look }
}

/// A program in a tab rang the bell or sent a notification (`Shell::poll`).
pub(crate) fn asked(s: &mut Shell, id: TabId, clock: f64) {
    let said = s.panes.get_mut(&id).and_then(|p| p.take_said());
    attention(s, id, AttentionKind::Input, said, ms(clock));
}

/// Marks the tab, and hands the system notification to the backend when one is due.
fn attention(s: &mut Shell, id: TabId, kind: AttentionKind, said: Option<String>, now: u64) {
    let on_screen = s.bench.active_id() == Some(id);
    let st = &mut s.screens.agent;
    let look = st.look();
    let at = standing(s.window_focused, on_screen, st.notify, &look);
    let note = st.agents.ask(&mut s.bench, id, kind, said, now, &at);
    if let (Some(note), Some(tab)) = (note, s.bench.tab(id).cloned()) {
        s.backend_mut().notify(&tab, &note);
    }
}

/// The program in front in each of a server's terminals (`Remote::set_foreground`).
pub(crate) fn foreground(s: &mut Shell, server: &str, running: &[(String, String)]) {
    let tabs: Vec<(TabId, Option<String>)> = s
        .bench
        .tabs_on(server)
        .filter(|t| t.kind == TabKind::Terminal)
        .map(|t| (t.id, t.session_key.as_ref().and_then(|key| running.iter().find(|(k, _)| k == key)).map(|(_, command)| command.clone())))
        .collect();
    for (id, command) in tabs {
        s.screens.agent.agents.set_foreground(&mut s.bench, id, command.as_deref());
    }
}

/// A session's status changed (`Shell::link_status`): the backend hears which servers have a
/// connected terminal, to ask them what their terminals run.
pub(crate) fn linked(s: &mut Shell) {
    let mut keys: Vec<ServerKey> = s
        .bench
        .tabs()
        .iter()
        .filter(|t| t.kind == TabKind::Terminal && t.server != "local" && matches!(t.link, Link::Connected { .. }))
        .map(|t| t.server.clone())
        .collect();
    keys.sort();
    keys.dedup();
    if keys != s.screens.agent.asking {
        let servers: Vec<Server> = keys.iter().filter_map(|k| s.bench.server(k).cloned()).collect();
        s.screens.agent.asking = keys;
        s.backend_mut().agents_on(&servers);
    }
}

/// The end of every change made from the screen (`Shell::settle`): the tab on screen is
/// seen, and going anywhere puts the card and the list away.
pub(crate) fn settled(s: &mut Shell, _tx: &mut Tx) {
    let active = s.bench.active_id();
    let st = &mut s.screens.agent;
    if let Some(id) = active {
        st.agents.seen(id);
    }
    st.card = None;
    st.keyboard = false;
    st.list = None;
    st.left_at = None;
}

// ---------------------------------------------------------------------------------------------
// Reading a tab's screen

/// The question on a tab's screen, read again when the screen changed.
fn reading_of(s: &mut Shell, tab: TabId) -> Reading {
    let unseen = Reading::Unsure(Unsure::Unseen);
    // While streaming a card shows nothing of the terminal, so it reads nothing.
    if s.screens.agent.streaming {
        return unseen;
    }
    let Some(agent) = s.bench.tab(tab).and_then(AgentKind::of) else { return unseen };
    let Some(pane) = s.panes.get(&tab).filter(|p| p.is_open()) else { return unseen };
    let st = &mut s.screens.agent;
    if let Some((_, reading)) = st.readings.get(&tab).filter(|(at, _)| *at == pane.frames) {
        return reading.clone();
    }
    let reading = match pane.view.borrow_mut().question_lines() {
        Some(lines) => question::read(agent.asker(), &lines),
        None => unseen,
    };
    st.readings.insert(tab, (pane.frames, reading.clone()));
    reading
}

/// A tab's screen for a click or a key on its card: with everything its session has sent
/// taken in first. Output the terminal hasn't shown yet (held for a moment, as a burst is)
/// means the card may be out of date, so the screen counts as unreadable and nothing is sent.
fn fresh_reading(s: &mut Shell, tab: TabId, tx: &mut Tx) -> Reading {
    if let Some(pane) = s.panes.get_mut(&tab) {
        if pane.model.is_dirty() {
            pane.poll(tx.clock(), tx);
        }
        if pane.model.is_dirty() {
            return Reading::Unsure(Unsure::Unseen);
        }
    }
    reading_of(s, tab)
}

/// Types an answer's keys into a tab's terminal, as the keyboard would.
fn type_strokes(s: &mut Shell, tab: TabId, strokes: &[Stroke], tx: &mut Tx) {
    let Some(pane) = s.panes.get_mut(&tab) else { return };
    for stroke in strokes {
        let key = match stroke {
            Stroke::Text(t) => Key::Text(t.clone()),
            Stroke::Return => Key::Enter,
            Stroke::Escape => Key::Escape,
            Stroke::Up => Key::Up,
            Stroke::Down => Key::Down,
            Stroke::Left => Key::Left,
            Stroke::Right => Key::Right,
        };
        pane.type_key(&KeyEvent { key, mods: Mods::default() }, tx);
    }
    // The card's own keys aren't "typed in the terminal meanwhile".
    if !strokes.is_empty() {
        s.screens.agent.typed_before = pane.typed_at();
    }
}

/// What is true for the card's tab right now.
fn scene_of<'a>(s: &Shell, tab: TabId, reading: &'a Reading, now: u64) -> Scene<'a> {
    let st = &s.screens.agent;
    let connected = s.bench.tab(tab).is_some_and(|t| matches!(t.link, Link::Connected { .. }));
    let typed = s.pane(tab).and_then(|p| p.typed_at()) != st.typed_before;
    Scene { now, reading, connected, keyboard: st.keyboard, typed }
}

// ---------------------------------------------------------------------------------------------
// The card

/// A tab whose agent waits, and which isn't on screen: the one a card is for.
fn waits_elsewhere(s: &Shell, tab: TabId) -> bool {
    s.bench.active_id() != Some(tab) && s.bench.tab(tab).is_some_and(|t| t.kind == TabKind::Terminal && t.needs_you && AgentKind::of(t).is_some())
}

fn open_card(s: &mut Shell, tab: TabId, keyboard: bool, tx: &mut Tx) {
    let Some(agent) = s.bench.tab(tab).and_then(AgentKind::of) else { return };
    let now = ms(tx.clock());
    s.open_unseen(tab);
    let reading = reading_of(s, tab);
    let prod = s.bench.tab(tab).and_then(|t| s.bench.server(&t.server)).is_some_and(Server::is_prod);
    let typed_before = s.pane(tab).and_then(|p| p.typed_at());
    let st = &mut s.screens.agent;
    st.card = Some(Card::open(tab, agent.name(), prod, &reading, now));
    st.keyboard = keyboard;
    st.words = TextEdit::default();
    st.typed_before = typed_before;
    st.left_at = None;
    st.over_card = false;
    st.list = None;
    st.focus_card = keyboard;
    tx.instant();
}

/// Puts the card away; the keyboard goes back to the terminal on screen.
fn close_card(s: &mut Shell, tx: &mut Tx) {
    let st = &mut s.screens.agent;
    let had_keyboard = st.keyboard;
    st.card = None;
    st.keyboard = false;
    st.left_at = None;
    // The pointer may still rest on the tab: it has to leave and come back to open it again.
    st.hover = None;
    if had_keyboard && s.active_pane().is_some() {
        tx.focus("term", false);
    }
    tx.instant();
}

/// The agent has its answer (from the card, or typed in its terminal): its tab stops saying
/// "Needs you". Called once, when the card first says so.
fn answered(s: &mut Shell, tab: TabId) {
    s.bench.set_needs_you(tab, false);
    s.screens.agent.agents.seen(tab);
}

/// The card says the agent has its answer.
fn has_answer(card: &Card) -> bool {
    matches!(card.outcome(), Some(Outcome::Sent(_) | Outcome::Answered))
}

/// The card takes the keyboard: a click on it.
fn take_keyboard(s: &mut Shell, tx: &mut Tx) {
    let st = &mut s.screens.agent;
    if st.card.is_some() && !st.keyboard {
        st.keyboard = true;
        tx.focus(if st.card.as_ref().is_some_and(|c| c.writing().is_some()) { "agent-words" } else { "agent-card" }, false);
        // Its foot changes words at once: nothing slides on a card you're about to click.
        tx.instant();
    }
}

/// A click or a key on the card: the rules decide, this types what they say.
fn press(s: &mut Shell, press: Press, tx: &mut Tx) {
    let Some(tab) = s.screens.agent.card.as_ref().map(|c| c.tab) else { return };
    let now = ms(tx.clock());
    let reading = fresh_reading(s, tab, tx);
    let scene = scene_of(s, tab, &reading, now);
    let Some(card) = s.screens.agent.card.as_mut() else { return };
    let was_writing = card.writing();
    let had_answer = has_answer(card);
    let did = card.press(press, &scene);
    let writing = card.writing();
    let next = card.next_look();
    if has_answer(card) && !had_answer {
        answered(s, tab);
    }
    match did {
        Did::Typed(strokes) => type_strokes(s, tab, &strokes, tx),
        Did::Closed => return close_card(s, tx),
        Did::GoToTab => return s.focus_tab(tab, tx),
        Did::Nothing => {}
    }
    // The answer in your own words has a field; back from it the card has the keys again.
    if writing != was_writing && s.screens.agent.keyboard {
        if writing.is_some() {
            s.screens.agent.words = TextEdit::default();
        }
        tx.focus(if writing.is_some() { "agent-words" } else { "agent-card" }, false);
    }
    if let Some(at) = next {
        tx.wake_after(Duration::from_millis(at.saturating_sub(now)));
    }
    tx.instant();
}

/// A key while the card has the keyboard. Every key is the card's: none reaches the terminal.
fn card_key(s: &mut Shell, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.shortcut(ev, tx) {
        return true;
    }
    let plain = !ev.mods.ctrl && !ev.mods.alt && !ev.mods.logo;
    let pressed = match &ev.key {
        Key::Escape => Some(Press::Escape),
        Key::Enter if plain => Some(Press::Return),
        Key::Up if plain => Some(Press::Up),
        Key::Down if plain => Some(Press::Down),
        Key::Text(t) if plain => digit(t),
        _ => None,
    };
    if let Some(pressed) = pressed {
        s.screens.agent.keyboard = true;
        press(s, pressed, tx);
    }
    true
}

/// "3" as the digit 3.
fn digit(typed: &str) -> Option<Press> {
    let mut chars = typed.chars();
    match (chars.next().and_then(|c| c.to_digit(10)), chars.next()) {
        (Some(n), None) => Some(Press::Digit(n as u8)),
        _ => None,
    }
}

/// The pointer came onto a tab or left it (`shell/view.rs`).
pub(crate) fn tab_hover(s: &mut Shell, tab: TabId, over: bool, tx: &mut Tx) {
    let now = ms(tx.clock());
    let waits = waits_elsewhere(s, tab);
    let st = &mut s.screens.agent;
    if over && waits {
        st.hover = Some((tab, now));
        st.left_at = None;
        tx.wake_after(Duration::from_millis(HOVER_OPEN_MS));
    } else if st.hover.is_some_and(|(id, _)| id == tab) && !over {
        st.hover = None;
        tx.wake_after(Duration::from_millis(HOVER_CLOSE_MS));
    }
}

fn card_hover(s: &mut Shell, over: bool, tx: &mut Tx) {
    s.screens.agent.over_card = over;
    if !over {
        tx.wake_after(Duration::from_millis(HOVER_CLOSE_MS));
    }
}

// ---------------------------------------------------------------------------------------------
// The list of agents

/// The agents shortcut: opens the list, or puts it away.
pub(crate) fn toggle_list(s: &mut Shell, tx: &mut Tx) {
    if s.screens.agent.list.is_some() {
        return close_list(s, tx);
    }
    let st = &mut s.screens.agent;
    st.card = None;
    st.keyboard = false;
    // The longest wait is first, and chosen.
    st.list = Some(0);
    tx.focus("agents", false);
    tx.instant();
}

fn close_list(s: &mut Shell, tx: &mut Tx) {
    s.screens.agent.list = None;
    if s.active_pane().is_some() {
        tx.focus("term", false);
    }
    tx.instant();
}

/// Return or a click on a row: a waiting agent's card with the keyboard, else its tab.
fn pick(s: &mut Shell, tab: TabId, tx: &mut Tx) {
    s.screens.agent.list = None;
    if waits_elsewhere(s, tab) {
        open_card(s, tab, true, tx);
    } else {
        s.focus_tab(tab, tx);
    }
}

fn list_key(s: &mut Shell, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.shortcut(ev, tx) {
        return true;
    }
    let rows = s.screens.agent.agents.rows(&s.bench, s.screens.agent.now);
    let last = rows.len().saturating_sub(1);
    let chosen = s.screens.agent.list.unwrap_or(0).min(last);
    match &ev.key {
        Key::Escape => close_list(s, tx),
        Key::Down => s.screens.agent.list = Some((chosen + 1).min(last)),
        Key::Up => s.screens.agent.list = Some(chosen.saturating_sub(1)),
        Key::Enter => match rows.get(chosen) {
            Some(row) => pick(s, row.tab, tx),
            None => close_list(s, tx),
        },
        _ => {}
    }
    tx.instant();
    true
}

// ---------------------------------------------------------------------------------------------
// Every frame

/// The start of every frame, after the panes took their output (`Shell::poll`). True when
/// the view must run again.
pub(crate) fn poll(s: &mut Shell, clock: f64, tx: &mut Tx) -> bool {
    let now = ms(clock);
    let active = s.bench.active_id();
    let focused = s.window_focused;
    let mut changed = false;
    let mut wake: Option<u64> = None;
    let mut soon = |at: u64| wake = Some(wake.map_or(at, |w: u64| w.min(at)));

    // Output: a pane that took frames since the last look had some.
    {
        let st = &mut s.screens.agent;
        st.now = now;
        for (id, pane) in &s.panes {
            let seen = st.frames.insert(*id, pane.frames);
            if seen.is_some_and(|n| n != pane.frames) {
                st.agents.output(*id, now, focused && active == Some(*id));
                st.last_output = Some(now);
            }
        }
    }

    // A tab that says "Needs you" without a terminal yet gets one, so its question can be read.
    let unseen: Vec<TabId> = s.bench.tabs().iter().filter(|t| t.needs_you && t.agent.is_some() && t.kind == TabKind::Terminal && !s.panes.contains_key(&t.id)).map(|t| t.id).collect();
    for id in unseen {
        s.open_unseen(id);
    }

    // Long runs that went quiet while nobody looked.
    if let Some(last) = s.screens.agent.last_output {
        if now >= last + QUIET_MS {
            let st = &mut s.screens.agent;
            st.last_output = None;
            let look = st.look();
            let notify = st.notify;
            let quiet = st.agents.check_quiet(&mut s.bench, now, |id| standing(focused, active == Some(id), notify, &look));
            for (id, note) in quiet {
                changed = true;
                if let (Some(note), Some(tab)) = (note, s.bench.tab(id).cloned()) {
                    s.backend_mut().notify(&tab, &note);
                }
            }
        } else {
            soon(last + QUIET_MS);
        }
    }

    // The working mark.
    {
        let st = &mut s.screens.agent;
        let working: HashSet<TabId> = s.bench.tabs().iter().filter(|t| st.agents.working(&s.bench, t.id, now)).map(|t| t.id).collect();
        for until in working.iter().filter_map(|id| st.agents.busy_until(*id, now)) {
            soon(until);
        }
        changed |= working != st.working;
        st.working = working;
    }

    // The pointer rested on a waiting agent's tab.
    if let Some((tab, since)) = s.screens.agent.hover {
        let typing = s.active_pane().and_then(|p| p.typed_at()).is_some_and(|at| ms(at) + TYPING_HOLD_MS > now);
        if s.screens.agent.card.is_none() && s.screens.agent.list.is_none() && waits_elsewhere(s, tab) {
            if now >= since + HOVER_OPEN_MS && !typing {
                open_card(s, tab, false, tx);
                changed = true;
            } else {
                soon((since + HOVER_OPEN_MS).max(now + 50));
            }
        }
    }

    // The card: what the agent's screen says now, an answer on its way, and its leaving.
    if let Some(tab) = s.screens.agent.card.as_ref().map(|c| c.tab) {
        if std::mem::take(&mut s.screens.agent.focus_card) {
            tx.focus("agent-card", false);
            changed = true;
        }
        let reading = reading_of(s, tab);
        let scene = scene_of(s, tab, &reading, now);
        let st = &mut s.screens.agent;
        let before = st.card.clone();
        let strokes = st.card.as_mut().map(|c| c.look(&scene)).unwrap_or_default();
        changed |= st.card != before;
        if st.card.as_ref().is_some_and(has_answer) && !before.as_ref().is_some_and(has_answer) {
            answered(s, tab);
        }
        type_strokes(s, tab, &strokes, tx);
        let st = &mut s.screens.agent;
        let card = st.card.as_ref();
        let done = card.and_then(Card::closes_at).is_some_and(|at| now >= at);
        // Without the keyboard it stays only while the pointer is on it or its tab.
        let pointed = st.over_card || st.hover.is_some_and(|(id, _)| id == tab);
        if st.keyboard || pointed {
            st.left_at = None;
        } else if st.left_at.is_none() {
            st.left_at = Some(now);
        }
        let left = st.left_at.is_some_and(|at| now >= at + HOVER_CLOSE_MS);
        if let Some(at) = card.and_then(Card::next_look) {
            soon(at);
        }
        if let Some(at) = st.left_at {
            soon(at + HOVER_CLOSE_MS);
        }
        if done || left {
            close_card(s, tx);
            changed = true;
        }
    }

    if let Some(at) = wake {
        tx.wake_after(Duration::from_millis(at.saturating_sub(now).max(1)));
    }
    changed
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// What is drawn over the window: the card, the list.
pub fn overlays(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Vec<El> {
    let st = &s.screens.agent;
    let mut out = Vec::new();
    if let Some((card, tab)) = st.card.as_ref().and_then(|c| Some((c, s.bench.tab(c.tab)?))) {
        out.push(card_view(s, card, tab, t));
    }
    if let Some(chosen) = st.list {
        out.push(list_view(s, chosen, t, cx));
    }
    out
}

/// The small arrow from a card to the tab above it.
fn arrow(t: &Theme) -> El {
    let fit = Fit { scale: 1.0, dx: 0.0, dy: 0.0 };
    let shape = Shape::new().fill_path(&svg::path("M0 7L7 0L14 7Z", fit), t.pop).stroke_path(&svg::path("M0 7L7 0L14 7", fit), 1.0, LineCap::Butt, t.rim);
    canvas(shape).key("arrow").size(14.0, 7.0).absolute(20.0, -7.0).a11y_hidden()
}

/// The command or the change, in a quiet box.
fn code_box(t: &Theme, lines: &[String]) -> El {
    let mut shown: Vec<&str> = lines.iter().take(CONTEXT_LINES).map(String::as_str).collect();
    let more = lines.len().saturating_sub(shown.len());
    let note = format!("… {more} more {}", if more == 1 { "line" } else { "lines" });
    if more > 0 {
        shown.push(&note);
    }
    el().key("context")
        .mx(14.0)
        .mt(10.0)
        .px(10.0)
        .py(8.0)
        .radius(theme::RADIUS_ROW)
        .fill(t.code)
        .child(text(shown.join("\n")).font(theme::CODE).color(t.l1).wrap())
}

/// The field the answer in your own words is written in.
fn words_field(words: &TextEdit, t: &'static Theme, hint: &str) -> El {
    row()
        .key("words-box")
        .h(28.0)
        .w_full()
        .mt(2.0)
        .px(8.0)
        .gap(8.0)
        .radius(theme::RADIUS_ROW)
        .fill(if t.dark { t.code } else { t.page })
        .items_center()
        .focus_within_ring(theme::RING, 0.0, t.ring)
        .child(
            text_input(words, theme::TEXT)
                .key("agent-words")
                .single_line()
                .grow(1.0)
                .min_w(0.0)
                .color(t.l1)
                .caret(t.ring, t.ring.alpha(0.3))
                // (The sheet draws a field that already holds words over no state of its own.)
                .placeholder(if words.text.is_empty() { hint } else { "" }, t.l2)
                .label(hint)
                .on_edit(|s: &mut Shell| &mut s.screens.agent.words)
                .on_submit(|s: &mut Shell, tx| {
                    let words = s.screens.agent.words.text.clone();
                    press(s, Press::Words(words), tx);
                })
                .on_input_key(|s: &mut Shell, ev, tx| {
                    // Esc goes back to the answers; every other key is the field's.
                    let back = matches!(ev.key, Key::Escape);
                    if back {
                        press(s, Press::Escape, tx);
                    }
                    back
                }),
        )
        .child(key_cap(t, "↩", false))
}

/// What a card shows.
struct Show<'a> {
    key: &'static str,
    card: &'a Card,
    /// Where the agent is: "on db-main, in /srv/shop-db".
    place: String,
    /// "waiting 2 min".
    waited: Option<String>,
    keyboard: bool,
    streaming: bool,
    words: &'a TextEdit,
    /// The tab it hangs under; none on the component sheet.
    under: Option<TabId>,
}

fn card_view(s: &Shell, card: &Card, tab: &Tab, t: &'static Theme) -> El {
    let st = &s.screens.agent;
    let look = st.look();
    // Who asks, and where. The folder is the terminal's word, so it's made safe like any.
    let server = server_shown(&s.bench, tab, st.streaming);
    let folder = s.pane(card.tab).and_then(|p| p.cwd()).filter(|_| !st.streaming).map(|f| look.safe_text(f));
    let place = match folder {
        Some(folder) => format!("on {server}, in {folder}"),
        None => format!("on {server}"),
    };
    let waited = st.agents.waiting_since(card.tab).map(|since| waited(since, st.now));
    card_el(t, &Show { key: "agent-card", card, place, waited, keyboard: st.keyboard, streaming: st.streaming, words: &st.words, under: Some(card.tab) })
}

fn card_el(t: &'static Theme, show: &Show) -> El {
    let Show { card, keyboard, streaming, .. } = *show;
    let mut head = row()
        .key("who")
        .w_full()
        .items_center()
        .gap(7.0)
        .px(14.0)
        .pt(12.0)
        .child(text(card.agent.as_str()).font(theme::TEXT_BOLD).color(t.l1).single_line())
        .child(text(show.place.as_str()).font(theme::TEXT).color(t.l2).single_line().truncate().shrink());
    if card.prod {
        head = head.child(prod_chip(t, false));
    }
    head = head.child(spacer());
    if let Some(waited) = &show.waited {
        head = head.child(text(waited.as_str()).font(theme::SMALL).color(t.l2).single_line());
    }

    let mut body = col().key("ask").w_full();
    match card.question().filter(|_| !streaming) {
        Some(q) => {
            if !q.context.is_empty() {
                body = body.child(code_box(t, &q.context));
            }
            body = body.child(text(q.question.as_str()).key("question").font(theme::TEXT_BOLD).color(t.l1).px(14.0).pt(12.0).pb(6.0).wrap());
            let mut answers = col().key("answers").w_full().px(6.0).pb(6.0).role("menu");
            // Once the agent stopped asking, the answers are gone with the question.
            let options = if card.gone() { &[][..] } else { &q.options[..] };
            for (i, option) in options.iter().enumerate() {
                if card.writing() == Some(i) {
                    answers = answers.child(words_field(show.words, t, &option.label));
                    continue;
                }
                // Blue is the row Return sends: there is one only while the card has the keys.
                let (state, cap) = match card.row(i) {
                    RowLook::Down => (RowState::Down, option.number.to_string()),
                    RowLook::Armed => (RowState::Hot, format!("{} again", option.number)),
                    RowLook::Hot if keyboard => (RowState::Hot, format!("↩ {}", option.number)),
                    RowLook::Hot | RowLook::Rest => (RowState::Rest, option.number.to_string()),
                };
                let label = match option.kind {
                    ChoiceKind::Text => format!("{}…", option.label),
                    _ => option.label.clone(),
                };
                answers = answers.child(
                    menu_row(t, &label, Some(&cap), state)
                        .key(format!("answer-{}", i + 1))
                        // The card owns the keyboard, not its rows: Return on a row that
                        // merely has focus must not be a click.
                        .focusable(false)
                        .on_press_tx(move |s: &mut Shell, tx| {
                            take_keyboard(s, tx);
                            press(s, Press::Answer(i), tx);
                        }),
                );
            }
            body = body.child(answers);
        }
        None => {
            let line = if streaming { "Its question is hidden while streaming.".to_owned() } else { format!("{} is waiting for you.", card.agent) };
            body = body.child(text(line).key("question").font(theme::TEXT).color(t.l1).px(14.0).pt(12.0).pb(12.0).wrap());
        }
    }
    if let Some(status) = card.status() {
        body = body.child(text(status).key("outcome").font(theme::NOTE).color(t.l1).px(14.0).pb(10.0).wrap());
    }
    let (left, right) = card.hints(keyboard);

    let mut e = popover(t, theme::CARD_WIDTH)
        .key(show.key)
        .focusable(true)
        .label(format!("{} needs you", card.agent))
        .on_key(card_key)
        .on_press_tx(take_keyboard)
        .on_hover_change(card_hover);
    if let Some(tab) = show.under {
        e = e.anchored(format!("tab-{}", tab.0), Side::Bottom, Align::Start, 7.0).child(arrow(t));
    }
    e = e.child(head).child(body);
    if !left.is_empty() || !right.is_empty() {
        e = e.child(popover_foot(t, &left, &right));
    } else {
        e = e.child(el().h(4.0));
    }
    // The ring says who has the keyboard; the field has its own while words are written.
    e = if card.writing().is_some() { e.no_focus_ring() } else { e.focus_within_ring(theme::RING, 0.0, t.ring) };
    if keyboard && show.under.is_some() {
        // Nothing behind it takes a key or a click until it gives the keyboard back.
        e = e.modal_tx(true, close_card);
    }
    e
}

/// The card's states for the component sheet (`--sheet`): made-up questions, nothing behind
/// them. Each line is (its key, its name, the cards).
pub fn sheet(t: &'static Theme) -> Vec<(&'static str, &'static str, Vec<El>)> {
    use sshee_term::question::{read_text, Asker};
    let ask = |command: &str, grant: &str| {
        let screen = format!("────────\n Bash command\n\n   {command}\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and don't ask again for {grant}\n   3. No, and tell Claude what to do differently (esc)\n");
        let rows: Vec<&str> = screen.lines().collect();
        read_text(Asker::ClaudeCode, &rows)
    };
    let migration = ask("psql -h db-main -d shop -f migrations/0042_orders_index.sql", "psql in /srv/shop-db");
    let tests = ask("npm test", "npm test in /srv/shop");
    fn at(reading: &Reading, connected: bool) -> Scene<'_> {
        Scene { now: 1_000, reading, connected, keyboard: true, typed: false }
    }
    let empty = TextEdit::default();
    let words = TextEdit::new("Use a transaction and CONCURRENTLY");
    let tab = TabId(0);
    let draw = |key: &'static str, card: &Card, place: &str, words: &TextEdit| {
        card_el(t, &Show { key, card, place: place.to_owned(), waited: Some("waiting 2 min".to_owned()), keyboard: true, streaming: false, words, under: None })
    };
    // Production: nothing chosen. Elsewhere: the first answer.
    let prod = Card::open(tab, "Claude Code", true, &migration, 0);
    let other = Card::open(tab, "Claude Code", false, &tests, 0);
    // The third answer in your own words; an answer sent.
    let mut writing = Card::open(tab, "Claude Code", true, &migration, 0);
    writing.press(Press::Answer(2), &at(&migration, true));
    let mut sent = Card::open(tab, "Claude Code", false, &tests, 0);
    sent.press(Press::Digit(1), &at(&tests, true));
    // Not sent, the answer kept; a standing permission on Production clicked once; a screen
    // that can't be read.
    let mut kept = Card::open(tab, "Claude Code", true, &migration, 0);
    kept.press(Press::Digit(1), &at(&migration, false));
    let mut armed = Card::open(tab, "Claude Code", true, &migration, 0);
    armed.press(Press::Digit(2), &at(&migration, true));
    let unsure = Reading::Unsure(Unsure::Cut);
    let unread = Card::open(tab, "Codex", false, &unsure, 0);
    let tab_of = |key: &'static str, name: &str, badge, working| {
        let spec = crate::kit::tab::Tab { icon: Icon::Agent, name, server: Some("web-staging"), prod: false, badge, working, active: false, window_active: true, compact: false };
        crate::kit::tab::tab(t, &spec, |_: &mut Shell, _| {}).key(key).on_press_tx(|_: &mut Shell, _| {})
    };
    vec![
        ("l-agent-tab", "Agent's tab", vec![tab_of("t-work", "Terminal 2 (Codex)", None, true), tab_of("t-asks", "Terminal (Claude Code)", Some(sshee_app::workbench::Badge::NeedsYou), false)]),
        ("l-card", "Agent's question", vec![draw("card-prod", &prod, "on db-main, in /srv/shop-db", &empty), draw("card-other", &other, "on web-staging, in /srv/shop", &empty)]),
        ("l-card-2", "Own words, sent", vec![draw("card-words", &writing, "on db-main, in /srv/shop-db", &words), draw("card-sent", &sent, "on web-staging, in /srv/shop", &empty)]),
        ("l-card-3", "Not sent, twice", vec![draw("card-kept", &kept, "on db-main, in /srv/shop-db", &empty), draw("card-armed", &armed, "on db-main, in /srv/shop-db", &empty)]),
        ("l-card-4", "Can't be read", vec![draw("card-unread", &unread, "on web-staging", &empty)]),
    ]
}

/// One agent in the list.
fn agent_row(t: &Theme, r: &AgentRow, server: &str, hot: bool, now: u64) -> El {
    let ink = if hot { t.on_color } else { t.l1 };
    let quiet = if hot { t.on_color } else { t.l2 };
    let mut e = row().h(30.0).w_full().px(8.0).gap(9.0).radius(theme::RADIUS_ROW).items_center().role("menuitem").label(format!("{}, {server}, {}", r.label, r.state.label()));
    e = if hot { e.fill(t.blue) } else { e.hover(t.hov2).press(t.down) };
    e = e
        .child(icon(if r.agent.is_some() { Icon::Agent } else { Icon::Terminal }, theme::ICON, quiet))
        .child(text(r.label.as_str()).font(theme::TEXT).color(ink).single_line().truncate().shrink())
        .child(text(server).font(theme::NOTE).color(quiet).single_line());
    if r.prod {
        e = e.child(chip(t, "Prod", if hot { t.on_color } else { t.red }));
    }
    e = e.child(spacer());
    match r.state {
        AgentState::Waiting => {
            let said = match r.waiting_since {
                Some(since) if now.saturating_sub(since) >= 60_000 => format!("Needs you, {}", waited(since, now).trim_start_matches("waiting ")),
                _ => "Needs you".to_owned(),
            };
            e.child(row().gap(6.0).items_center().child(dot(7.0, if hot { t.on_color } else { t.orange_dot })).child(text(said).font(theme::NEEDS).color(if hot { t.on_color } else { t.orange }).single_line()))
        }
        other => e.child(text(other.label()).font(theme::NOTE).color(quiet).single_line()),
    }
}

fn list_view(s: &Shell, chosen: usize, t: &'static Theme, cx: &mut Cx<Shell>) -> El {
    let st = &s.screens.agent;
    let rows = st.agents.rows(&s.bench, st.now);
    let chosen = chosen.min(rows.len().saturating_sub(1));
    let width = cx.window_layout("window").map_or(crate::shell::SIZE.0, |r| r[2]);
    let mut list = col().key("agents-rows").w_full().px(6.0).pb(6.0).role("menu");
    for (i, r) in rows.iter().enumerate() {
        let tab = r.tab;
        let server = s.bench.tab(tab).map_or_else(|| r.server.clone(), |t| server_shown(&s.bench, t, st.streaming));
        list = list.child(agent_row(t, r, &server, i == chosen, st.now).key(format!("agent-{}", tab.0)).on_press_tx(move |s: &mut Shell, tx| pick(s, tab, tx)));
    }
    if rows.is_empty() {
        list = list.child(text("No agent is running in a terminal.").key("agents-none").font(theme::TEXT).color(t.l2).px(8.0).py(6.0));
    }
    let foot = if rows.is_empty() { popover_foot(t, "", "Esc to close") } else { popover_foot(t, "↩ answers a waiting agent, or goes to its tab", "Esc to close") };
    popover(t, theme::CARD_WIDTH)
        .key("agents")
        .absolute(((width - theme::CARD_WIDTH) / 2.0).max(8.0).round(), 72.0)
        .focusable(true)
        .no_focus_ring()
        .label_static("Agents")
        .on_key(list_key)
        .modal_tx(true, close_list)
        .child(text("Agents").key("agents-title").font(theme::GROUP).color(t.l2).px(14.0).pt(10.0).pb(6.0).single_line())
        .child(list)
        .child(foot)
}
