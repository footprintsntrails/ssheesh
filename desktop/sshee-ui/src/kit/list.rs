//! List rows: what a command palette, a picker or a start page lists. An icon, a name, a
//! quieter note after it, and at the end what the row has to say ("Prod", "Needs you") and
//! the key that picks it. One row at most is the blue one Return picks (the design's `.mi`).

use rune::{text, El};

use crate::icons::{icon, Icon};
use crate::kit::chip::chip;
use crate::kit::dot;
use crate::kit::menu::{key_cap, RowState};
use crate::theme::{self, Theme};

/// What a row shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListRow<'a> {
    pub icon: Option<Icon>,
    pub label: &'a str,
    /// The label is a command: in the terminal's font.
    pub mono: bool,
    /// A quieter word or two after the label; empty for none.
    pub note: &'a str,
    /// The red "Prod" chip.
    pub prod: bool,
    /// The orange dot and "Needs you".
    pub needs_you: bool,
    /// A quiet word at the row's end ("working", "Reopen"); empty for none.
    pub tail: &'a str,
    /// A key cap at the end.
    pub keys: Option<&'a str>,
    pub state: RowState,
    /// It takes something away: red.
    pub destructive: bool,
    /// A check mark's place at the start: `Some(true)` draws it.
    pub checked: Option<bool>,
}

/// One row. The caller adds its key and `.on_press_tx(..)`.
pub fn list_row(t: &Theme, spec: &ListRow) -> El {
    let on_color = spec.state != RowState::Rest;
    let ink = if on_color {
        t.on_color
    } else if spec.destructive {
        t.red
    } else {
        t.l1
    };
    let quiet = if on_color { t.on_color } else { t.l2 };
    let mut row = rune::row().h(theme::ROW).w_full().px(8.0).gap(9.0).radius(theme::RADIUS_ROW).items_center().role("option");
    row = match spec.state {
        RowState::Rest => row.press(t.down),
        RowState::Hot => row.fill(t.blue),
        RowState::Down => row.fill(t.blue_down),
    };
    if let Some(on) = spec.checked {
        row = row.checked(on).child(if on { icon(Icon::Check, theme::ICON, ink) } else { rune::el().size(theme::ICON, theme::ICON) });
    }
    if let Some(glyph) = spec.icon {
        row = row.child(icon(glyph, theme::ICON, if spec.destructive && !on_color { t.red } else { quiet }));
    }
    let label = text(spec.label).key("label").font(if spec.mono { theme::CODE } else { theme::TEXT }).color(ink).single_line().truncate().shrink();
    row = row.child(label);
    if !spec.note.is_empty() {
        row = row.child(text(spec.note).key("note").font(theme::NOTE).color(quiet).single_line().truncate().shrink().min_w(0.0));
    }
    if spec.prod {
        row = row.child(chip(t, "Prod", if on_color { t.on_color } else { t.red }).key("prod"));
    }
    if spec.needs_you {
        let orange = if on_color { t.on_color } else { t.orange };
        row = row.child(
            rune::row().key("needs").gap(6.0).items_center().child(dot(7.0, if on_color { t.on_color } else { t.orange_dot })).child(text("Needs you").font(theme::NEEDS).color(orange).single_line()),
        );
    }
    row = row.child(rune::spacer());
    if !spec.tail.is_empty() {
        row = row.child(text(spec.tail).key("tail").font(theme::NOTE).color(quiet).single_line());
    }
    if let Some(keys) = spec.keys {
        row = row.child(key_cap(t, keys, on_color));
    }
    row
}

/// A group's heading over its rows: "On web-7731", "Go to".
pub fn list_heading(t: &Theme, heading: &str) -> El {
    rune::row().w_full().px(8.0).pt(9.0).pb(3.0).child(text(heading).font(theme::SMALL).color(t.l2).single_line())
}

/// A thin line between two parts of a menu.
pub fn list_rule(t: &Theme) -> El {
    rune::el().h(1.0).mx(8.0).my(5.0).fill(t.hair).a11y_hidden()
}
