//! The tour (0.10's `Tour.tsx`, the simple one 1.0 ships: plan decision Р7) and hints
//! (`Hints.tsx`).
//!
//! ```text
//! tour-scrim   dims the window; modal (Esc closes)
//! └── tour     tour-dot-<i> · the card: its sign, title, line, detail and keys
//!              tour-back, tour-next (Return), tour-close
//! hint         a small note at the bottom right: hint-ok ("Got it"), hint-stop ("No more tips")
//! ```
//!
//! The cards and when a hint is due are `sshee_app::tour`. A hint never takes the keyboard:
//! Esc and Ctrl+C stay with whatever runs in the terminal. What's seen is Settings'
//! `hintsSeen`, as in 0.10.

use rune::{col, el, row, spacer, text, El, Tx};
use sshee_app::palette::shortcut_label;
use sshee_app::tour::{self, Hint, Moment, STEPS};
use sshee_app::workbench::{Link, TabKind};

use super::checkup::{close_button, dialog, headed};
use crate::icons::{icon, Icon};
use crate::kit::menu::key_cap;
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

pub const WIDTH: f32 = 560.0;

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub at: usize,
    /// The hint on screen.
    pub hint: Option<&'static Hint>,
    /// When the last hint showed (the window's clock), and how many this run.
    last_hint: Option<f64>,
    shown: usize,
    /// When the window started, by its clock.
    started: Option<f64>,
}

impl Shell {
    /// The tour, at card `at`: from the welcome, the palette and Settings → About.
    pub fn open_tour(&mut self, at: usize, tx: &mut Tx) {
        self.screens.close_all();
        self.screens.tour.open = true;
        self.screens.tour.at = at.min(STEPS.len() - 1);
        tx.instant();
    }

    pub fn close_tour(&mut self, tx: &mut Tx) {
        self.screens.tour.open = false;
        self.settle(tx);
    }

    fn tour_step(&mut self, by: i32, tx: &mut Tx) {
        let at = self.screens.tour.at;
        if by > 0 && at + 1 >= STEPS.len() {
            self.close_tour(tx);
        } else {
            self.screens.tour.at = tour::step(at, by);
        }
    }

    /// "Got it", or "No more tips" (`all`).
    fn hint_seen(&mut self, all: bool) {
        let Some(hint) = self.screens.tour.hint.take() else { return };
        let id = if all { "*" } else { hint.id };
        self.settings.update(|s| tour::mark_seen(s, id));
    }
}

/// The window as hints see it now.
fn moment(s: &Shell, clock: f64) -> Moment {
    let state = &s.screens.tour;
    let server = s.bench.current_key();
    let tabs = server.map_or(0, |k| s.bench.tabs_on(k).count());
    let active = s.bench.active();
    Moment {
        since_start: clock - state.started.unwrap_or(clock),
        since_last: state.last_hint.map(|t| clock - t),
        shown_this_run: state.shown,
        busy: s.screens.any_open() || s.screens.settings.open,
        tabs_on_server: tabs,
        side_by_side: s.tabs_on_screen().len() > 1,
        terminal_connected: active.is_some_and(|t| t.kind == TabKind::Terminal && matches!(t.link, Link::Connected { .. })),
        agent_working: s.bench.tabs().iter().any(|t| t.agent.is_some()),
        on_a_server: s.bench.current_server().is_some_and(|sv| !sv.is_local()),
    }
}

/// Every frame: a hint when one's moment has come. True: one showed.
pub fn tick(s: &mut Shell, clock: f64) -> bool {
    s.screens.tour.started.get_or_insert(clock);
    if s.screens.tour.hint.is_some() {
        return false;
    }
    let Some(hint) = tour::next_hint(&s.settings.get().hints_seen, &moment(s, clock)) else { return false };
    let state = &mut s.screens.tour;
    state.hint = Some(hint);
    state.last_hint = Some(clock);
    state.shown += 1;
    true
}

/// A card's sign.
fn sign(id: &str) -> Icon {
    match id {
        "find" => Icon::Search,
        "tabs" => Icon::Terminal,
        "durable" => Icon::Wifi,
        "check" => Icon::Shield,
        "split" => Icon::SideBySide,
        _ => Icon::EyeOff,
    }
}

/// The tour, when it's open.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.tour;
    if !state.open {
        return None;
    }
    let step = &STEPS[state.at];
    let mut dots = row().key("tour-dots").gap(6.0).items_center().role("tablist");
    for (i, st) in STEPS.iter().enumerate() {
        let on = i == state.at;
        dots = dots.child(
            el().key(format!("tour-dot-{i}"))
                .size(if on { 18.0 } else { 6.0 }, 6.0)
                .radius(3.0)
                .fill(if on { t.blue } else { t.l2.alpha(0.4) })
                .focusable(true)
                .role("tab")
                .checked(on)
                .label(format!("Step {}: {}", i + 1, st.title))
                .on_press_tx(move |s: &mut Shell, _| s.screens.tour.at = i),
        );
    }
    let top = row().w_full().gap(10.0).items_center().child(text(format!("{} of {}", state.at + 1, STEPS.len())).font(theme::VALUE).color(t.l2).single_line()).child(dots).child(spacer()).child(close_button(t, "tour-close").on_press_tx(|s: &mut Shell, tx| s.close_tour(tx)));
    let art = el()
        .key("tour-art")
        .w_full()
        .h(150.0)
        .radius(theme::RADIUS_GROUP)
        .fill(t.blue.alpha(0.08))
        .border(1.0, t.hair2)
        .items_center()
        .justify_center()
        .child(el().size(64.0, 64.0).radius(32.0).fill(t.pop).border(1.0, t.rim).items_center().justify_center().child(icon(sign(step.id), 28.0, t.blue)));
    let mut words = col()
        .w_full()
        .gap(6.0)
        .child(text(step.title).key("tour-title").font(theme::PAGE_TITLE).color(t.l1).wrap().role("heading").level(1))
        .child(text(step.body).font(theme::TEXT).color(t.l2on).wrap())
        .child(text(step.detail).font(theme::NOTE).color(t.l2).wrap());
    if let Some(keys) = step.keys.and_then(|k| shortcut_label(k, s.options.mac_keys, &s.options.shortcuts)) {
        words = words.child(row().pt(2.0).gap(8.0).items_center().child(text("Shortcut").font(theme::SMALL).color(t.l2).single_line()).child(key_cap(t, &keys, false)));
    }
    let last = state.at + 1 == STEPS.len();
    let foot = row()
        .w_full()
        .gap(8.0)
        .child(spacer())
        .child(push_button(t, "Back", Push::Quiet, state.at > 0).key("tour-back").on_press_tx(|s: &mut Shell, tx| s.tour_step(-1, tx)))
        .child(push_button(t, if last { "Done" } else { "Next" }, Push::Primary, true).key("tour-next").default_button().on_press_tx(|s: &mut Shell, tx| s.tour_step(1, tx)));
    let page = super::checkup::body("tour-body").pb(18.0).gap(16.0).child(art).child(words).child(foot);
    Some(dialog(t, "tour-scrim", "tour", "Tour", WIDTH, headed(t, top, page, 16.0), |s: &mut Shell, tx| s.close_tour(tx)))
}

/// The hint on screen: at the bottom right, over the page, never taking the keyboard.
pub fn hint(s: &Shell, t: &'static Theme) -> Option<El> {
    let h = s.screens.tour.hint?;
    if s.screens.any_open() {
        return None;
    }
    let card = col()
        .key("hint")
        .w(300.0)
        .px(14.0)
        .py(12.0)
        .gap(6.0)
        .radius(theme::RADIUS_POPOVER)
        .fill(t.pop)
        .border(1.0, t.rim)
        .shadow(12.0, 32.0, rune::Color::BLACK.alpha(0.25))
        .role("note")
        .label(h.title)
        .child(text(h.title).font(theme::TEXT_BOLD).color(t.l1).wrap())
        .child(text(h.body).font(theme::NOTE).color(t.l2on).wrap())
        .child(
            row()
                .w_full()
                .pt(4.0)
                .gap(8.0)
                .items_center()
                .child(text("No more tips").key("hint-stop").font(theme::SMALL).color(t.l2).single_line().role("button").on_press_tx(|s: &mut Shell, _| s.hint_seen(true)))
                .child(spacer())
                .child(push_button(t, "Got it", Push::Quiet, true).key("hint-ok").on_press_tx(|s: &mut Shell, _| s.hint_seen(false))),
        );
    Some(card.pin(None, None, Some(16.0), Some(theme::STATUS_HEIGHT + 14.0)))
}
