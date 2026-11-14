//! A server's card: what shows beside its icon in the sidebar while the pointer rests there
//! (the design's "carried over" sheet). Its name and label, its address and whether it's
//! reached through Tor, how many tabs are open on it, and who waits there and since when:
//! "Claude Code needs you, 2 min". A sidebar of icons alone needn't be known by heart.
//!
//! What the card says is `sshee_app::home::Card`. It comes up after the pointer has rested a
//! moment, as a tooltip does, and at once when the pointer moves from one icon to the next.
//! It takes no input (the pointer passes through it) and goes when the pointer leaves the
//! icon, the icon is pressed, or something opens over the window.

use std::time::Duration;

use rune::{col, row, text, Align, El, Side, Tx};
use sshee_app::home::Card;

use crate::kit::chip::prod_chip;
use crate::kit::dot;
use crate::kit::popover::popover;
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The card's width, the design's.
pub const WIDTH: f32 = 262.0;
/// How long the pointer rests on an icon before its card shows, in seconds.
const REST: f64 = 0.35;
/// How long after a card went the next icon's card shows without a rest.
const WARM: f64 = 0.4;

/// The card's own state.
#[derive(Default)]
pub struct State {
    /// The sidebar icon under the pointer, by its place in the list of servers.
    pub over: Option<usize>,
    /// When the pointer came over it, on the window's clock.
    since: f64,
    /// The pointer has rested long enough: the card is drawn.
    pub shown: bool,
    /// Until then, another icon's card shows at once.
    warm_until: f64,
}

impl State {
    /// The card goes: the icon was pressed, or something opened over the window.
    pub fn hide(&mut self) {
        self.over = None;
        self.shown = false;
    }
}

impl Shell {
    /// The pointer came over a server's icon in the sidebar, or left it.
    pub fn hover_server(&mut self, index: usize, inside: bool, tx: &mut Tx) {
        let now = tx.clock();
        let card = &mut self.screens.server_card;
        if inside {
            card.over = Some(index);
            card.since = now;
            card.shown = now < card.warm_until;
            if !card.shown {
                // A frame when the rest is over: `tick` shows the card then.
                tx.wake_after(Duration::from_secs_f64(REST + 0.02));
            }
        } else if card.over == Some(index) {
            if card.shown {
                card.warm_until = now + WARM;
            }
            card.hide();
        }
    }
}

/// Every frame: shows the card once the pointer has rested. True when it just came up.
pub(crate) fn tick(s: &mut Shell, now: f64) -> bool {
    let card = &mut s.screens.server_card;
    if card.over.is_some() && !card.shown && now - card.since >= REST {
        card.shown = true;
        return true;
    }
    false
}

/// The card's contents, on the popover surface.
pub fn card(t: &Theme, c: &Card) -> El {
    let mut name = row().gap(7.0).items_center().child(text(&c.name).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate().shrink());
    if c.prod {
        name = name.child(prod_chip(t, false));
    }
    let mut e = popover(t, WIDTH).role("tooltip").px(13.0).py(11.0).gap(5.0).child(name);
    if let Some(address) = &c.address {
        e = e.child(text(address).font(theme::NOTE).color(t.l2).single_line().truncate());
    }
    e = e.child(text(&c.tabs).font(theme::NOTE).color(t.l2).single_line());
    if !c.waiting.is_empty() {
        // Under a hairline: what you'd go there for.
        let mut waits = col().w_full().gap(5.0).mt(3.0).pt(6.0).border_sides([1.0, 0.0, 0.0, 0.0], t.hair);
        for line in &c.waiting {
            waits = waits.child(row().gap(7.0).items_center().child(dot(7.0, t.orange_dot)).child(text(line).font(theme::NOTE).color(t.l1).single_line().truncate().shrink()));
        }
        e = e.child(waits);
    }
    e
}

/// The card of the icon the pointer rests on, when nothing else is open over the window.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.server_card;
    let index = state.over.filter(|_| state.shown && !s.screens.any_open())?;
    let server = s.bench.servers().get(index)?;
    let c = Card::of(&s.bench, &server.key(), &s.screens.home.waits, s.screens.home.now)?;
    Some(card(t, &c).key("server-card").inert().anchored(format!("server-{index}"), Side::Right, Align::Start, 10.0))
}
