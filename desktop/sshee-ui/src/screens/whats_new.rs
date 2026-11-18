//! "What's new" (0.10's `WhatsNew.tsx`): shown once after an update, with the notes of every
//! version you skipped; also from the palette. The notes are the repository's CHANGELOG.md,
//! built into the program (`sshee_app::changelog`), so it works offline.
//!
//! ```text
//! news-scrim   dims the window; modal (Esc closes)
//! └── news     news-title · news-<i> per release: its bullets · news-ok ("Got it", Return)
//! ```

use rune::{col, row, spacer, text, El, Span, Tx};
use sshee_app::changelog::{self, Block, Release};

use super::checkup::{close_button, dialog, headed};
use crate::icons::logo;
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

pub const WIDTH: f32 = 580.0;

#[derive(Default)]
pub struct State {
    /// The releases shown, newest first; empty: closed.
    pub releases: Vec<Release>,
}

/// At the window's start (after the welcome decided): the notes since the version seen last,
/// once after an update. The version running is remembered either way.
pub fn start(s: &mut Shell, welcome_due: bool) {
    let Some(current) = s.backend_mut().version() else { return };
    let settings = s.settings.get();
    if changelog::due(&settings.last_seen_version, &current, settings.onboarded, welcome_due) {
        let all = changelog::releases();
        let seen = settings.last_seen_version.clone();
        s.screens.whats_new.releases = changelog::since(&all, &seen, &current).into_iter().cloned().collect();
    }
    if s.settings.get().last_seen_version != current {
        s.settings.update(|v| v.last_seen_version = current);
    }
}

impl Shell {
    /// The notes of the version running (or the newest): the palette, Settings → About.
    pub fn open_whats_new(&mut self, tx: &mut Tx) {
        self.screens.close_all();
        let current = self.backend_mut().version().unwrap_or_default();
        let all = changelog::releases();
        self.screens.whats_new.releases = changelog::current(&all, &current).into_iter().cloned().collect();
        tx.instant();
    }

    pub fn close_whats_new(&mut self, tx: &mut Tx) {
        self.screens.whats_new.releases.clear();
        self.settle(tx);
    }
}

/// A line of notes with its bold and code as the changelog has them.
fn line(t: &Theme, words: &str) -> El {
    let mut content = String::new();
    let mut spans = Vec::new();
    for part in changelog::spans(words) {
        let start = content.len();
        content.push_str(part.text());
        let end = content.len();
        match part {
            changelog::Span::Bold(_) => spans.push(Span::new(start, end).bold().color(t.l1)),
            changelog::Span::Code(_) => spans.push(Span::new(start, end).mono()),
            changelog::Span::Plain(_) => {}
        }
    }
    rune::rich(content, spans).font(theme::NOTE).color(t.l2on).w_full().label(changelog::plain(words))
}

fn release(t: &Theme, r: &Release, i: usize, many: bool) -> El {
    let mut out = col().key(format!("news-{i}")).w_full().gap(8.0);
    if many {
        out = out.child(
            row()
                .gap(6.0)
                .child(text(r.version.as_str()).font(theme::TEXT_BOLD).color(t.l1).single_line())
                .child(text(format!("· {}", r.title)).font(theme::TEXT).color(t.l2).single_line().truncate().shrink()),
        );
    }
    for block in changelog::blocks(&r.body) {
        out = out.child(match block {
            Block::Heading(h) => text(h).font(theme::GROUP_TITLE).color(t.l1).wrap().role("heading").level(3).mt(4.0),
            Block::Paragraph(p) => line(t, &p),
            Block::Bullet(b) => row().w_full().gap(8.0).items_start().child(text("•").font(theme::NOTE).color(t.l2).single_line().a11y_hidden()).child(line(t, &b).grow(1.0).shrink().min_w(0.0)),
        });
    }
    out
}

/// "What's new", when it's open.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let list = &s.screens.whats_new.releases;
    let first = list.first()?;
    let many = list.len() > 1;
    let mut titles = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(text(format!("What's new in SSHeesh {}", first.version)).key("news-title").font(theme::PAGE_TITLE).color(t.l1).single_line().truncate().role("heading").level(1));
    if many {
        titles = titles.child(text(format!("And everything since {}.", list.last().map_or("", |r| r.version.as_str()))).font(theme::NOTE).color(t.l2).single_line());
    } else if !first.title.is_empty() {
        titles = titles.child(text(capitalised(&first.title)).font(theme::NOTE).color(t.l2).single_line().truncate());
    }
    let head = row().w_full().gap(12.0).items_center().child(logo(40.0)).child(titles).child(close_button(t, "news-close").on_press_tx(|s: &mut Shell, tx| s.close_whats_new(tx)));
    let mut notes = col().key("news-notes").w_full().gap(18.0);
    for (i, r) in list.iter().enumerate() {
        notes = notes.child(release(t, r, i, many));
    }
    let page = super::checkup::body("news-body")
        .gap(16.0)
        .child(notes)
        .child(row().w_full().child(spacer()).child(push_button(t, "Got it", Push::Primary, true).key("news-ok").default_button().on_press_tx(|s: &mut Shell, tx| s.close_whats_new(tx))));
    Some(dialog(t, "news-scrim", "news", "What's new", WIDTH, headed(head, page, 16.0), |s: &mut Shell, tx| s.close_whats_new(tx)))
}

/// "rebuilt from the ground up" → "Rebuilt from the ground up".
fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
