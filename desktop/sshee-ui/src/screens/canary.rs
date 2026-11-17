//! A newer test build: one quiet line across the top of the page, as the privacy notice is
//! drawn ("A new test build is ready" — Restart to update, Later). The program finds, checks
//! and stages the build (`desktop/app/src/update.rs`) and hands the window a
//! [`Notice`](sshee_app::canary::Notice) through `Remote` ([`show`]); the words and buttons
//! are `sshee_app::canary`'s. Settings → Updates → "Check for test builds" off hides offers.

use rune::{row, text, El};
use sshee_app::canary::{Button, ByHand, Notice, RELEASE_PAGE};

use crate::icons::{icon, Icon};
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

#[derive(Debug, Default)]
pub struct State {
    pub notice: Option<Notice>,
    /// The version "Later" put off: its line stays away until a newer one.
    later: Option<String>,
}

/// A notice from the program.
pub fn show(s: &mut Shell, notice: Notice) {
    s.screens.canary.notice = Some(notice);
}

fn press(s: &mut Shell, button: Button, tx: &mut rune::Tx) {
    let Some(notice) = s.screens.canary.notice.clone() else { return };
    match button {
        Button::Restart => match s.backend_mut().restart_to_update() {
            // The window closes as its close button would (tabs and settings are kept); the
            // program's helper starts the new build once it has.
            Ok(()) => tx.close_window(),
            Err(why) => s.screens.canary.notice = Some(Notice::ByHand { version: notice.version().to_owned(), why: ByHand::Failed(why) }),
        },
        Button::Download => {
            let _ = tx.open_url(RELEASE_PAGE);
            s.screens.canary.later = Some(notice.version().to_owned());
        }
        Button::Later => s.screens.canary.later = Some(notice.version().to_owned()),
        Button::Ok => s.screens.canary.notice = None,
    }
    tx.instant();
}

/// The line, while there's something to say.
pub fn bar(s: &Shell, t: &'static Theme) -> Option<El> {
    let notice = s.screens.canary.notice.as_ref()?;
    let offer = !matches!(notice, Notice::RolledBack { .. });
    let off = !s.settings.canary_updates() || s.settings.update_mode() == sshee_app::model::UpdateMode::Manual;
    if offer && (off || s.screens.canary.later.as_deref() == Some(notice.version())) {
        return None;
    }
    let mut line = row()
        .key("canary-notice")
        .w_full()
        .min_h(36.0)
        .px(14.0)
        .py(7.0)
        .gap(10.0)
        .items_center()
        .fill(t.group)
        .border_sides([0.0, 0.0, 1.0, 0.0], t.hair2)
        .role("status")
        .child(icon(Icon::Info, 12.0, t.l2on))
        .child(text(notice.text()).key("canary-notice-text").font(theme::NOTE).color(t.l1).wrap().grow(1.0).shrink());
    for (i, &button) in notice.buttons().iter().enumerate() {
        let kind = if i == 0 && button == Button::Restart { Push::Primary } else { Push::Quiet };
        line = line.child(push_button(t, button.label(), kind, true).key(button.key()).on_press_tx(move |s: &mut Shell, tx| press(s, button, tx)));
    }
    Some(line)
}

/// `content` under the line, when there is one.
pub fn with_bar(s: &Shell, t: &'static Theme, content: El) -> El {
    match bar(s, t) {
        Some(line) => rune::col().key("page-with-canary").w_full().h_full().child(line).child(rune::el().w_full().grow(1.0).min_h(0.0).child(content)),
        None => content,
    }
}
