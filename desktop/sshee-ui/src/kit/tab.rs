//! A tab in the top row: its kind's icon, its name, its server's name in a quieter tone, then
//! what it has to say ("Prod", "Needs you", "Reconnecting…").
//!
//! - Every tab ends with the same close cross, after its status mark: always on the tab
//!   you're on, under the pointer on the others (its room is kept, so the tab keeps its width).
//! - The tab you're on is a rounded lighter fill; under the pointer any other gets a fainter one.
//! - In a window that isn't in front, the tab you're on goes quiet; "Prod" stays red.
//! - A program's progress (OSC 9;4) is a small ring after the name, filling as it goes
//!   (0.10 spun one); a terminal asleep is a quiet moon, not a warning.
//! - When the row is crowded (`Tab::compact`), the server is named once before its group
//!   instead of in every tab, and a long name fades out at its end.

use rune::{text, Color, El, Tx};
use sshee_app::workbench::Badge;
use sshee_term::osc::{Progress, ProgressState};

use crate::icons::{icon, Icon};
use crate::kit::chip::prod_chip;
use crate::kit::{dot, hollow_dot, progress_arc, progress_ring};
use crate::theme::{self, Theme};

/// What a tab shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab<'a> {
    pub icon: Icon,
    pub name: &'a str,
    /// The server's name; `None` when the row is crowded and the group is named instead.
    pub server: Option<&'a str>,
    /// The red "Prod" chip goes on this tab.
    pub prod: bool,
    pub badge: Option<Badge>,
    /// An agent is at work in it: a quiet ring after its name, when it has nothing else to say.
    pub working: bool,
    /// Its connection is being made: the same ring, named "Connecting".
    pub connecting: bool,
    /// What ran in it went quiet while you weren't looking: a green check (0.10's).
    pub went_quiet: bool,
    /// What the program in it says of its progress (OSC 9;4).
    pub progress: Option<Progress>,
    /// The tab you're on.
    pub active: bool,
    /// The window is in front.
    pub window_active: bool,
    /// The row is crowded: the tab may shrink, and what it has to say is only its dot or ring.
    pub compact: bool,
}

/// The cross at the end of every tab, after what it has to say: the same glyph, size and ink
/// on each. Shown on the tab you're on; on the others, while the pointer is over them (its room
/// is kept, so a tab doesn't change width under the pointer).
fn close_button<S: 'static>(t: &Theme, shown: bool, on_close: impl Fn(&mut S, &mut Tx) + 'static) -> El {
    let e = rune::el()
        .key("close")
        .size(20.0, 20.0)
        .ml(-1.0)
        .mr(-5.0)
        .radius(5.0)
        .items_center()
        .justify_center()
        .hover(t.hov2)
        .press(t.down)
        .role("button")
        .label_static("Close tab")
        .on_press_tx(on_close)
        .child(icon(Icon::Close, 14.0, t.l2on));
    if shown {
        e
    } else {
        e.group_opacity(0.0, 1.0)
    }
}

/// What a tab has to say after its name. In a crowded row only its mark (the dot, the ring),
/// named for a screen reader: the words can't shrink, and would push the tab's cross out
/// over its neighbour.
fn badge(t: &Theme, b: Badge, compact: bool) -> El {
    let row = rune::row().key("badge").gap(6.0).items_center();
    if b == Badge::Asleep {
        // Not trouble: a quiet moon, no words, what it means under the pointer.
        let row = row.label_static(b.label()).child(icon(Icon::Moon, 11.0, t.l2));
        return match b.hint() {
            Some(hint) => row.help(hint),
            None => row,
        };
    }
    let (mark, words) = match b {
        Badge::NeedsYou => (dot(7.0, t.orange_dot), text(b.label()).font(theme::NEEDS).color(t.orange)),
        Badge::Reconnecting => (progress_ring(10.0, 1.5, t.l2), text(b.label()).font(theme::TAB_STATE).color(t.l2on)),
        Badge::Disconnected | Badge::Ended | Badge::Failed | Badge::Asleep => (hollow_dot(8.0, 1.5, t.l2), text(b.label()).font(theme::TAB_STATE).color(t.l2on)),
    };
    if compact {
        row.child(mark).label_static(b.label())
    } else {
        row.child(mark).child(words.single_line())
    }
}

/// The tab. The caller adds its key, `.on_press_tx(..)` (go to it) and `.on_middle_press(..)`
/// (close it); `on_close` is what its cross does.
pub fn tab<S: 'static>(t: &Theme, spec: &Tab, on_close: impl Fn(&mut S, &mut Tx) + 'static) -> El {
    let Tab { active, window_active, compact, .. } = *spec;
    let quiet = active && !window_active;
    // The name: primary on the tab you're on and on one that waits for you; quieter while its
    // connection is being brought back.
    let name_ink = match spec.badge {
        _ if quiet => t.l2,
        Some(Badge::NeedsYou) => t.l1,
        Some(Badge::Reconnecting | Badge::Disconnected | Badge::Ended | Badge::Failed | Badge::Asleep) if !active => t.l2,
        _ if active => t.l1,
        _ => t.l2,
    };
    let icon_ink = if active && !quiet { t.l1 } else { t.l2 };
    let server_ink = if active && !quiet { t.l2on } else { t.l2 };

    let mut e = rune::row()
        .h(theme::CONTROL)
        .px(if compact { 8.0 } else { 10.0 })
        .gap(if compact { 6.0 } else { 7.0 })
        .radius(theme::RADIUS)
        .items_center()
        .focusable(true)
        .role("tab")
        .label(match (spec.server, spec.badge) {
            (Some(server), Some(b)) => format!("{}, {server}, {}", spec.name, b.label()),
            (Some(server), None) if spec.connecting => format!("{}, {server}, connecting", spec.name),
            (Some(server), None) if spec.went_quiet => format!("{}, {server}, went quiet", spec.name),
            (Some(server), None) if spec.progress.is_some() => format!("{}, {server}, {}", spec.name, progress_words(spec.progress)),
            (Some(server), None) if spec.working => format!("{}, {server}, working", spec.name),
            (Some(server), None) => format!("{}, {server}", spec.name),
            (None, Some(b)) => format!("{}, {}", spec.name, b.label()),
            (None, None) => spec.name.to_owned(),
        });
    e = if quiet {
        e.fill(t.hov)
    } else if active {
        let e = e.fill(t.sel);
        // Raised: a light line along its top at night, a hairline and a soft shadow by day.
        if t.dark {
            e.border_sides([0.5, 0.0, 0.0, 0.0], t.tabhi)
        } else {
            e.border(0.5, Color::BLACK.alpha(0.12)).shadow(1.0, 2.0, Color::BLACK.alpha(0.08))
        }
    } else {
        e.hover(t.hov2).press(t.down)
    };
    e = e.hover_group().child(icon(spec.icon, theme::ICON, icon_ink));
    // The name fades out at its end in any tab narrower than its words: a crowded row, or a
    // tab the row holds at its width while the pointer is over it.
    let name = text(spec.name).key("name").font(theme::TAB_NAME).color(name_ink).single_line().truncate_fade().shrink();
    if compact {
        // Its icon and its cross stay inside it; the name gives way.
        e = e.shrink().min_w(theme::ICON + 16.0 + 6.0 + 14.0);
    }
    e = e.child(name);
    if let Some(server) = spec.server {
        e = e.child(text(server).key("server").font(theme::TAB_SERVER).color(server_ink).single_line().pt(1.0));
    }
    if spec.prod {
        e = e.child(prod_chip(t, active));
    }
    if let Some(b) = spec.badge {
        e = e.child(badge(t, b, compact));
    } else if spec.connecting {
        e = e.child(rune::row().key("connecting").items_center().label_static("Connecting").child(progress_ring(10.0, 1.5, t.l2)));
    } else if spec.went_quiet {
        e = e.child(rune::row().key("quiet").items_center().label_static("Went quiet").child(icon(Icon::Check, 12.0, t.green)));
    } else if let Some(p) = spec.progress {
        e = e.child(rune::row().key("progress").items_center().label(progress_words(Some(p))).child(progress_mark(t, p)));
    } else if spec.working {
        e = e.child(rune::row().key("working").items_center().label_static("Working").child(progress_ring(10.0, 1.5, t.l2)));
    }
    e.child(close_button(t, active, on_close))
}

/// What a program's progress says to a screen reader: "42% done", or "Working" when it can't
/// tell how far it is.
pub fn progress_words(p: Option<Progress>) -> String {
    match p {
        Some(Progress { state: ProgressState::Indeterminate, .. }) | None => "Working".to_owned(),
        Some(Progress { state: ProgressState::Error, value }) => format!("Stopped with an error at {}%", value.round()),
        Some(Progress { state: ProgressState::Paused, value }) => format!("Paused at {}%", value.round()),
        Some(Progress { value, .. }) => format!("{}% done", value.round()),
    }
}

/// The colour a program's progress is drawn in: blue as it goes, orange paused, red when it
/// stopped with an error.
pub fn progress_color(t: &Theme, state: ProgressState) -> Color {
    match state {
        ProgressState::Error => t.red,
        ProgressState::Paused => t.orange,
        ProgressState::Normal | ProgressState::Indeterminate => t.blue,
    }
}

/// A program's progress after a tab's name: a ring filling as it goes, or going round when it
/// can't say how far it is.
fn progress_mark(t: &Theme, p: Progress) -> El {
    match p.state {
        ProgressState::Indeterminate => progress_ring(10.0, 1.5, t.l2),
        state => progress_arc(10.0, 1.6, p.value / 100.0, progress_color(t, state), t.hair),
    }
}

/// A group's name when the row is crowded: the server, once, before its tabs. `current`: the
/// server on screen, in primary text.
pub fn group_name(t: &Theme, server: &str, prod: bool, current: bool) -> El {
    let mut e = rune::row().items_center().gap(0.0).pl(5.0).pr(7.0).child(text(server).font(theme::GROUP).color(if current { t.l1 } else { t.l2 }).single_line());
    if prod {
        e = e.child(prod_chip(t, false).ml(7.0));
    }
    e
}
