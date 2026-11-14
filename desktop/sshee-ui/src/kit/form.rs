//! A settings form, as in a Mac settings window: boxes of grouped rows, each row with its
//! name at the left and its control at the right, and one short line of help under the name
//! only where the name isn't enough.

use rune::{text, El};

use crate::theme::{self, Theme};

/// A row's words and whether it can be used.
#[derive(Clone, Copy, Debug, Default)]
pub struct RowText<'a> {
    pub label: &'a str,
    /// One line of help under the name.
    pub help: Option<&'a str>,
    /// Why the row can't be used yet, in one line. It goes under the help, and the name goes
    /// quiet.
    pub later: Option<&'a str>,
}

fn words(t: &Theme, row: &RowText) -> El {
    let mut words = rune::col().key("words").grow(1.0).shrink().min_w(0.0).gap(2.0);
    if !row.label.is_empty() {
        words = words.child(text(row.label).key("label").font(theme::TEXT).color(if row.later.is_some() { t.l2 } else { t.l1 }).wrap());
    }
    if let Some(help) = row.help {
        words = words.child(text(help).key("help").font(theme::HELP).color(t.l2).wrap());
    }
    if let Some(later) = row.later {
        words = words.child(text(later).key("later").font(theme::HELP).color(t.l2).wrap());
    }
    words
}

/// A row: its words at the left, `control` at the right. The caller adds the key.
pub fn form_row(t: &Theme, row: RowText, control: El) -> El {
    rune::row().w_full().min_h(40.0).px(12.0).py(8.0).gap(16.0).items_center().role("group").label(row.label).child(words(t, &row)).child(control)
}

/// A row whose control is too wide to sit beside its name: the words, then `control` under
/// them at full width.
pub fn form_row_stacked(t: &Theme, row: RowText, control: El) -> El {
    rune::col().w_full().px(12.0).py(10.0).gap(8.0).role("group").label(row.label).child(words(t, &row)).child(control)
}

/// A box of rows, a hairline between each two.
pub fn form_group(t: &Theme, rows: Vec<El>) -> El {
    let mut group = rune::col().w_full().radius(theme::RADIUS_GROUP).fill(t.group).border(1.0, t.hair2);
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            // The line starts where the words do, as in a Mac list.
            group = group.child(rune::el().h(1.0).ml(12.0).fill(t.hair2).a11y_hidden());
        }
        group = group.child(row);
    }
    group
}

/// The small heading over a group, with one line under it when it needs one.
pub fn form_heading(t: &Theme, title: &str, help: Option<&str>) -> El {
    let mut heading =
        rune::col().w_full().px(12.0).pt(8.0).gap(2.0).child(text(title).font(theme::GROUP_TITLE).color(t.l2).single_line().role("heading").level(2));
    if let Some(help) = help {
        heading = heading.child(text(help).font(theme::HELP).color(t.l2).wrap());
    }
    heading
}

/// A quiet box with a line of text: a notice inside a section ("Names are hidden as well as
/// SSHeesh can spot them…").
pub fn form_note(t: &Theme, note: &str) -> El {
    rune::row().w_full().px(12.0).py(9.0).radius(theme::RADIUS_GROUP).fill(t.hov2).child(text(note).font(theme::NOTE).color(t.l2on).wrap().grow(1.0).shrink())
}
