//! A tab in the top row: its kind's icon, its name, its server's name in a quieter tone, then
//! what it has to say ("Prod", "Needs you", "Reconnecting…").
//!
//! - The tab you're on is a rounded lighter fill with a close cross at its end.
//! - Under the pointer, any other tab gets a fainter fill and the cross takes the icon's
//!   place: the tab keeps its width.
//! - In a window that isn't in front, the tab you're on goes quiet; "Prod" stays red.
//! - When the row is crowded (`Tab::compact`), the server is named once before its group
//!   instead of in every tab, and a long name fades out at its end.

use rune::{text, Color, El, Tx};
use sshee_app::workbench::Badge;

use crate::icons::{icon, Icon};
use crate::kit::chip::prod_chip;
use crate::kit::{dot, hollow_dot, progress_ring};
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
    /// The tab you're on.
    pub active: bool,
    /// The window is in front.
    pub window_active: bool,
    /// The row is crowded: the tab may shrink, and "Needs you" is only its dot.
    pub compact: bool,
}

/// The icon's place in a tab that isn't the one you're on: the icon, and under the pointer
/// the close cross instead. A press there closes the tab.
fn icon_or_close<S: 'static>(t: &Theme, glyph: Icon, ink: Color, on_close: impl Fn(&mut S, &mut Tx) + 'static) -> El {
    rune::el()
        .key("close")
        .size(theme::ICON, theme::ICON)
        .role("button")
        .label_static("Close tab")
        .on_press_tx(on_close)
        .child(icon(glyph, theme::ICON, ink).absolute(0.0, 0.0).group_opacity(1.0, 0.0))
        .child(icon(Icon::Close, theme::ICON, t.l2on).absolute(0.0, 0.0).group_opacity(0.0, 1.0))
}

/// The cross at the end of the tab you're on.
fn close_button<S: 'static>(t: &Theme, on_close: impl Fn(&mut S, &mut Tx) + 'static) -> El {
    rune::el()
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
        .child(icon(Icon::Close, 14.0, t.l2on))
}

/// What a tab has to say after its name.
fn badge(t: &Theme, b: Badge, compact: bool) -> El {
    let row = rune::row().key("badge").gap(6.0).items_center();
    match b {
        Badge::NeedsYou if compact => row.child(dot(7.0, t.orange_dot)).label_static("Needs you"),
        Badge::NeedsYou => row.child(dot(7.0, t.orange_dot)).child(text(b.label()).font(theme::NEEDS).color(t.orange).single_line()),
        Badge::Reconnecting => row.child(progress_ring(10.0, 1.5, t.l2)).child(text(b.label()).font(theme::TAB_STATE).color(t.l2on).single_line()),
        Badge::Disconnected | Badge::Ended => row.child(hollow_dot(8.0, 1.5, t.l2)).child(text(b.label()).font(theme::TAB_STATE).color(t.l2on).single_line()),
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
        Some(Badge::Reconnecting | Badge::Disconnected | Badge::Ended) if !active => t.l2,
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
    // One cross per tab: at the end of the tab you're on, in the icon's place on the others.
    let (lead_close, end_close) = if active { (None, Some(on_close)) } else { (Some(on_close), None) };
    e = match lead_close {
        Some(close) => e.hover_group().child(icon_or_close(t, spec.icon, icon_ink, close)),
        None => e.child(icon(spec.icon, theme::ICON, icon_ink)),
    };
    // The name fades out at its end in any tab narrower than its words: a crowded row, or a
    // tab the row holds at its width while the pointer is over it.
    let name = text(spec.name).key("name").font(theme::TAB_NAME).color(name_ink).single_line().truncate_fade().shrink();
    if compact {
        e = e.shrink().min_w(theme::ICON + 16.0);
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
    } else if spec.working {
        e = e.child(rune::row().key("working").items_center().label_static("Working").child(progress_ring(10.0, 1.5, t.l2)));
    }
    if let Some(close) = end_close {
        e = e.child(close_button(t, close));
    }
    e
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
