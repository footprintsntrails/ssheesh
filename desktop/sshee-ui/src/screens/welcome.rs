//! The welcome (0.10's `Welcome.tsx`, its servers step): at 1.0's first start here, your
//! servers first. 1.0 keeps a list of its own, so it offers to bring 0.10's over and to pick
//! from `~/.ssh/config`, or to add one by hand; then the tour. Skipping decides nothing for
//! you, and all of it is in "Add a server" later.
//!
//! ```text
//! welcome-scrim   dims the window; modal (Esc skips)
//! └── welcome     welcome-old, welcome-config, welcome-add · welcome-skip, welcome-tour
//! ```
//!
//! Whether it's due is `sshee_app::tour::welcome_due` (Settings' `onboardingVersion`); the
//! program says whether this start may show it at all (`Backend::welcome`), so a demo and a
//! test start without it. A pick opens "Add a server" at that list once its sources are read
//! ([`tick`]).

use rune::{col, row, spacer, text, El, Tx};
use sshee_app::servers::Source;
use sshee_app::tour;

use super::checkup::{body, dialog, headed};
use crate::icons::{logo, Icon};
use crate::kit::list::{list_row, ListRow};
use crate::kit::push::{push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

pub const WIDTH: f32 = 560.0;

#[derive(Default)]
pub struct State {
    pub open: bool,
    /// "Add a server" was opened from here, at this list: shown once its sources are read.
    pub then_list: Option<Source>,
}

/// At the window's start: the welcome when it's due and the program allows it; else "What's
/// new" when an update brought something to read. Either way the version is remembered.
pub fn start(s: &mut Shell) {
    let allowed = s.backend_mut().welcome();
    let due = allowed && tour::welcome_due(s.settings.get());
    if due {
        s.screens.welcome.open = true;
    }
    super::whats_new::start(s, due);
}

impl Shell {
    /// Done with the welcome, by any way out: it doesn't come back.
    fn welcome_done(&mut self) {
        self.screens.welcome.open = false;
        self.settings.update(tour::welcomed);
    }

    /// Settings → About's "Show the welcome again".
    pub fn open_welcome(&mut self, tx: &mut Tx) {
        self.screens.close_all();
        self.screens.welcome.open = true;
        tx.instant();
    }

    /// "Skip", and Esc.
    pub fn skip_welcome(&mut self, tx: &mut Tx) {
        self.welcome_done();
        self.settle(tx);
    }

    /// One of the ways to bring servers in: "Add a server", at that list when there's one.
    fn welcome_add(&mut self, list: Option<Source>, tx: &mut Tx) {
        self.welcome_done();
        self.open_add_server(tx);
        self.screens.welcome.then_list = list;
    }

    fn welcome_tour(&mut self, tx: &mut Tx) {
        self.welcome_done();
        self.open_tour(0, tx);
    }
}

/// Every frame: "Add a server" opened from the welcome goes to its list once it's read.
pub fn tick(s: &mut Shell) -> bool {
    let Some(source) = s.screens.welcome.then_list else { return false };
    let add = &s.screens.add_server;
    if !add.open {
        s.screens.welcome.then_list = None;
        return false;
    }
    if add.sources.is_none() {
        return false;
    }
    s.screens.welcome.then_list = None;
    s.add_server_list(source);
    true
}

/// The welcome, when it's open.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    if !s.screens.welcome.open {
        return None;
    }
    let servers = s.bench.servers().iter().filter(|sv| !sv.is_local()).count();
    let (old, config) = (s.screens.home.old_new, s.screens.home.config_new);
    let head = row()
        .w_full()
        .gap(12.0)
        .items_center()
        .child(logo(36.0))
        .child(text("Welcome to SSHeesh").font(theme::TAB_NAME).color(t.l2on).single_line());
    let mut page = body("welcome-body")
        .gap(16.0)
        .child(
            col()
                .w_full()
                .gap(6.0)
                .child(text("Your servers").font(theme::PAGE_TITLE).color(t.l1).single_line().role("heading").level(1))
                .child(text(tour::welcome_line(servers, old, config)).key("welcome-line").font(theme::TEXT).color(t.l2on).wrap()),
        );
    let plural = |n: usize, one: &str, many: &str| if n == 1 { one.to_owned() } else { many.replace('#', &n.to_string()) };
    let mut ways = col().key("welcome-ways").w_full().mx(-8.0).gap(2.0);
    if old > 0 {
        let label = plural(old, "Bring over your server from SSHeesh 0.10", "Bring over your # servers from SSHeesh 0.10");
        let spec = ListRow { icon: Some(Icon::Download), label: &label, ..ListRow::default() };
        ways = ways.child(list_row(t, &spec).key("welcome-old").hover(t.hov2).focusable(true).role("button").on_press_tx(|s: &mut Shell, tx| s.welcome_add(Some(Source::Old), tx)));
    }
    if config > 0 {
        let label = plural(config, "Pick it from ~/.ssh/config", "Pick from # in ~/.ssh/config");
        let spec = ListRow { icon: Some(Icon::Doc), label: &label, ..ListRow::default() };
        ways = ways.child(list_row(t, &spec).key("welcome-config").hover(t.hov2).focusable(true).role("button").on_press_tx(|s: &mut Shell, tx| s.welcome_add(Some(Source::SshConfig), tx)));
    }
    let spec = ListRow { icon: Some(Icon::Plus), label: if servers + old + config == 0 { "Add a server" } else { "Add another by hand" }, note: "paste an address or a whole ssh command", ..ListRow::default() };
    ways = ways.child(list_row(t, &spec).key("welcome-add").hover(t.hov2).focusable(true).role("button").on_press_tx(|s: &mut Shell, tx| s.welcome_add(None, tx)));
    page = page
        .child(ways)
        .child(text("Nothing connects yet. No account, no cloud: your servers and keys stay on this computer.").font(theme::SMALL).color(t.l2).wrap())
        .child(
            row()
                .w_full()
                .gap(8.0)
                .pt(4.0)
                .child(push_button(t, "Skip", Push::Quiet, true).key("welcome-skip").on_press_tx(|s: &mut Shell, tx| s.skip_welcome(tx)))
                .child(spacer())
                .child(push_button(t, "Take the one-minute tour", Push::Primary, true).key("welcome-tour").default_button().on_press_tx(|s: &mut Shell, tx| s.welcome_tour(tx))),
        );
    Some(dialog(t, "welcome-scrim", "welcome", "Welcome to SSHeesh", WIDTH, headed(t, head, page, 16.0), |s: &mut Shell, tx| s.skip_welcome(tx)))
}
