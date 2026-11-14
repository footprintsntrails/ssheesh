//! Uploads and downloads in progress, and notes about what just finished or failed: cards in
//! the window's bottom right corner, over whatever tab is on screen (0.10's Transfers.tsx and
//! its toasts). A card says how far the transfer got and how fast it goes, and has a button
//! to stop it; a note stays until it's dismissed. The words are sshee-app's
//! (`sshee_app::transfers::Transfer::card`, `sshee_app::files::Notice`). Names and paths in
//! them go through `Shell::text_shown`, for streamer mode.

use rune::{col, el, row, text, El};
use sshee_app::files::Tone;
use sshee_app::transfers::Kind;

use crate::icons::{icon, Icon};
use crate::kit::push::{push_button, Push};
use crate::kit::popover::popover;
use crate::shell::Shell;
use crate::theme::{self, Theme};

const WIDTH: f32 = 300.0;

/// A small cross: stop a transfer, dismiss a note.
fn cross(t: &Theme, name: String) -> El {
    el().size(20.0, 20.0).mr(-4.0).radius(5.0).items_center().justify_center().hover(t.hov2).press(t.down).role("button").label(name.clone()).help(name).child(icon(Icon::Close, 14.0, t.l2on))
}

/// The cards, or nothing when nothing runs and nothing is to be said.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let (notices, transfers) = (&s.screens.notices, s.screens.transfers.items());
    if notices.is_empty() && transfers.is_empty() {
        return None;
    }
    let mut stack = col().key("transfers").pin(None, None, Some(14.0), Some(theme::STATUS_HEIGHT + 12.0)).w(WIDTH).gap(8.0);
    for (i, n) in notices.iter().enumerate() {
        let mut head = row().w_full().gap(8.0).items_center().child(
            text(s.text_shown(&n.title).into_owned()).font(theme::TAB_NAME).color(if n.tone == Tone::Error { t.red } else { t.l1 }).single_line().truncate().grow(1.0).min_w(0.0),
        );
        if let Some(undo) = n.undo.clone() {
            head = head.child(push_button(t, "Undo", Push::Quiet, true).key(format!("note-undo-{i}")).on_press_tx(move |s: &mut Shell, tx| {
                if let Some(tab) = s.bench.active_id() {
                    super::files::undo_move(s, tab, &undo);
                }
                super::files::dismiss_notice(s, i);
                tx.instant();
            }));
        }
        head = head.child(cross(t, "Dismiss".into()).key(format!("note-close-{i}")).on_press_tx(move |s: &mut Shell, tx| {
            super::files::dismiss_notice(s, i);
            tx.instant();
        }));
        let mut card = popover(t, WIDTH).key(format!("note-{i}")).role("status").px(12.0).py(9.0).gap(3.0).child(head);
        if let Some(body) = &n.body {
            card = card.child(text(s.text_shown(body).into_owned()).font(theme::NOTE).color(t.l2).max_lines(3));
        }
        stack = stack.child(card);
    }
    for tr in transfers {
        let card = tr.card();
        let id = tr.id;
        let glyph = if tr.kind == Kind::Download { Icon::Download } else { Icon::Upload };
        let mut stop = cross(t, card.stop_label.clone()).key(format!("transfer-stop-{}", id.0));
        stop = if card.can_stop {
            stop.on_press_tx(move |s: &mut Shell, tx| {
                super::files::stop_transfer(s, id);
                tx.instant();
            })
        } else {
            stop.opacity(0.4)
        };
        // How far it got: a quiet track, filled from the left. A size not known yet shows a third.
        let filled = card.fraction.map_or(1.0 / 3.0, |f| f.clamp(0.0, 1.0) as f32);
        let mut foot = row().w_full().gap(8.0).child(text(&card.line).font(theme::STATUS_VALUE).color(t.l2).single_line().truncate().grow(1.0).min_w(0.0));
        if let Some(rate) = &card.rate {
            foot = foot.child(text(rate).font(theme::STATUS_VALUE).color(t.l2).single_line());
        }
        let mut body = popover(t, WIDTH)
            .key(format!("transfer-{}", id.0))
            .role("status")
            .px(12.0)
            .py(10.0)
            .gap(7.0)
            .child(
                row().w_full().gap(8.0).items_center().child(icon(if card.waiting { Icon::Warning } else { glyph }, 14.0, if card.waiting { t.orange } else { t.l2on })).child(
                    text(s.text_shown(&card.label).into_owned()).font(theme::TAB_NAME).color(t.l1).single_line().truncate().grow(1.0).min_w(0.0),
                ).child(stop),
            )
            .child(el().w_full().h(4.0).radius(2.0).fill(t.hov2).child(el().w_frac(filled).h(4.0).radius(2.0).fill(if card.waiting { t.l2 } else { t.l1 })))
            .child(foot);
        if let Some(cleaned) = &card.cleaned {
            body = body.child(text(s.text_shown(cleaned).into_owned()).font(theme::SMALL).color(t.l2).single_line().truncate());
        }
        stack = stack.child(body);
    }
    Some(stack)
}
