//! Tor: the popover over the sidebar's onion (0.10's `TorButton.tsx`, its states drawn as the
//! design's sheet has them: off, starting with its ring, on in green, a red square when it
//! can't start). The switch, how far a start has got and what holds it up, the round trip
//! through Tor, the last lines of its log, new routes, and starting afresh. A click on the
//! onion opens it; its switch turns Tor on or off. Off while terminals, Files or tunnels go
//! through Tor, it asks first ("Reconnect without Tor?"): they'd reconnect directly.
//!
//! ```text
//! tor-panel        modal: Esc and a click outside close it
//!   tor-switch     Settings' switch, the same command (`TorCommand::On`/`Off`)
//!   tor-bar        how far a start has got
//!   tor-log        the log's last lines, read when it opens and as the state changes
//!   tor-new-routes, tor-afresh, tor-show-log
//! ```
//!
//! The words are `sshee_app::tor`'s. What the backend says comes in through
//! [`Remote::tor_status`], [`Remote::tor_latency`], [`Remote::tor_log`] and
//! [`Remote::tor_warning`]; the sidebar and the status bar keep reading `bench.tor()`.
//!
//! Toasts say what Tor did (0.10's `src/lib/tor.ts`): on once it is, after you turned it on;
//! not connected after 25 seconds, or failed, with its log a click away; off, and how many
//! terminals stayed as they were ([`Remote::tor_switched`]); new routes. Each terminal on a
//! server shows whether it goes through Tor ([`mark`], [`Remote::through_tor`]).

use std::collections::HashSet;

use rune::{col, row, text, Cx, El, Tx};
use sshee_app::tor::{body, headline, progress, Ask, Detail, Follow, Note, Status};
use sshee_app::workbench::{TabId, TabKind, Tor};

use crate::icons::{icon, Icon};
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::kit::switch::switch;
use crate::screens::settings::TorCommand;
use crate::shell::{Confirm, Remote, Shell};
use crate::theme::{self, Theme};

/// The popover's width.
const WIDTH: f32 = 340.0;

/// The most the log takes; longer, it scrolls, its newest line in sight.
const LOG_HEIGHT: f32 = 132.0;

/// The popover's own state, and what it knows of Tor.
#[derive(Default)]
pub struct State {
    pub open: bool,
    pub detail: Detail,
    /// What has been said about Tor's state.
    pub follow: Follow,
    /// Terminals whose connection doesn't go through Tor, as each said when it connected (a
    /// server on your own network, or one without tmux that connected before Tor was on).
    pub direct: HashSet<TabId>,
}

impl Remote {
    /// Tor's state changed (sshee-core's watcher).
    pub fn tor_status(&self, status: Status) {
        self.every(move |s| status_changed(s, status.clone()));
    }

    /// The round trip through Tor, measured as a route was picked.
    pub fn tor_latency(&self, latency_ms: Option<u32>) {
        self.every(move |s| {
            let d = &mut s.screens.tor.detail;
            d.latency_ms = latency_ms.filter(|_| d.status == Status::On);
            s.bench.set_tor(d.status.shown(d.latency_ms));
        });
    }

    /// The last lines of Tor's log (`sshee_app::tor::log_tail`).
    pub fn tor_log(&self, lines: Vec<String>) {
        self.every(move |s| s.screens.tor.detail.log.clone_from(&lines));
    }

    /// What the switch couldn't do, said in the popover until it's switched again.
    pub fn tor_warning(&self, warning: Option<String>) {
        self.every(move |s| s.screens.tor.detail.warning.clone_from(&warning));
    }

    /// A switch, or new routes, is done: what moved to the new way at once, and how many
    /// terminals (on servers without tmux) `stayed` as they were until they reconnect.
    pub fn tor_switched(&self, command: TorCommand, stayed: usize) {
        self.update(move |s| {
            let note = match command {
                TorCommand::Off => Some(sshee_app::tor::turned_off(stayed)),
                TorCommand::On => sshee_app::tor::turned_on(stayed),
                TorCommand::NewRoutes => Some(sshee_app::tor::new_routes(stayed)),
                TorCommand::StartAfresh => None,
            };
            if let Some(note) = note {
                tell(s, note);
            }
        });
    }

    /// Whether the terminal of `tab` went through Tor when it connected.
    pub fn through_tor(&self, tab: TabId, tor: bool) {
        self.update_tab(tab, move |s| {
            if tor {
                s.screens.tor.direct.remove(&tab);
            } else {
                s.screens.tor.direct.insert(tab);
            }
        });
    }
}

/// A note about Tor, as a toast; trouble offers Tor's log.
fn tell(s: &mut Shell, note: Note) {
    use crate::kit::toast::Tone;
    let tone = match note.tone {
        sshee_app::tor::Tone::Info => Tone::Info,
        sshee_app::tor::Tone::Success => Tone::Success,
        sshee_app::tor::Tone::Error => Tone::Error,
    };
    let mut toast = crate::shell::Toast::new(tone, note.title).body(note.body);
    if note.show_log {
        toast = toast.action("Show log", |s: &mut Shell| s.backend_mut().tor_ask(Ask::ShowLog));
    }
    s.toast(toast);
}

/// A new state: the sidebar and the status bar follow; an open popover reads the log again;
/// what's worth saying is said.
pub fn status_changed(s: &mut Shell, status: Status) {
    let d = &mut s.screens.tor.detail;
    if status != Status::On {
        d.latency_ms = None;
    }
    let changed = d.status != status;
    d.status = status;
    s.bench.set_tor(d.status.shown(d.latency_ms));
    if changed && s.screens.tor.open {
        s.backend_mut().tor_ask(Ask::ReadLog);
    }
    if changed {
        let status = s.screens.tor.detail.status.clone();
        if let Some(note) = s.screens.tor.follow.status(&status) {
            tell(s, note);
        }
    }
}

/// Every frame (`Shell::poll`): a start that takes too long is said. True when it was.
pub(crate) fn tick(s: &mut Shell, clock: f64, tx: &mut Tx) -> bool {
    let status = s.screens.tor.detail.status.clone();
    let (note, wake) = s.screens.tor.follow.tick(&status, clock);
    if let Some(at) = wake {
        tx.wake_at(at);
    }
    match note {
        Some(note) => {
            tell(s, note);
            true
        }
        None => false,
    }
}

/// Whether `tab` shows the onion: Tor is on and it's a server's.
pub fn marked(s: &Shell, tab: TabId) -> bool {
    s.bench.tor() != Tor::Off && s.bench.tab(tab).and_then(|tab| s.bench.server(&tab.server)).is_some_and(|sv| !sv.is_local())
}

/// The onion beside a terminal's name while Tor is on (0.10's `TorMark`): green when it goes
/// through Tor, orange when it doesn't (a server on your own network, or one that connected
/// before Tor was on and moves when it reconnects). Only for a server's tabs.
pub fn mark(s: &Shell, tab: TabId, size: f32, t: &Theme) -> Option<El> {
    let tab = s.bench.tab(tab).filter(|tab| marked(s, tab.id))?;
    let direct = s.screens.tor.direct.contains(&tab.id);
    let (ink, help) = if direct {
        (t.orange, "This terminal doesn't go through Tor: the server is on your own network, or it connected before Tor was on and moves when it reconnects")
    } else {
        (t.green, "Through Tor: this server sees a Tor address, not yours")
    };
    Some(rune::el().key(format!("tor-mark-{}", tab.id.0)).items_center().justify_center().role("img").label_static(help).help(help).child(icon(Icon::Tor, size, ink)))
}

impl Shell {
    /// The sidebar's onion, or the status bar's "Tor 84 ms": the popover, or closed again.
    pub fn toggle_tor(&mut self, tx: &mut Tx) {
        if self.screens.tor.open {
            return self.close_tor(tx);
        }
        self.screens.close_all();
        self.screens.tor.open = true;
        self.backend_mut().tor_ask(Ask::ReadLog);
        tx.focus("tor-switch", false);
        tx.instant();
    }

    pub fn close_tor(&mut self, tx: &mut Tx) {
        self.screens.tor.open = false;
        self.settle(tx);
    }

    /// The sidebar's onion and the top bar's: a click (or Return, or Space) opens the popover,
    /// whose switch turns Tor on or off; a second click closes it. A secondary click does the
    /// same.
    pub fn tor_button(&mut self, tx: &mut Tx) {
        self.toggle_tor(tx);
    }

    /// Tor on or off: the popover's switch, Settings' and the palette's. Turning it off while
    /// something on a server goes through Tor asks first ([`reconnect_without_tor`]): it would
    /// reconnect around Tor and the server would see your own address. Nothing is switched or
    /// reconnected before the answer. Turning it on doesn't ask: what's open moves to Tor, and
    /// a toast says what stayed as it was.
    pub fn switch_tor(&mut self, tx: &mut Tx) {
        let on = status(self).switched_on();
        if let Some(ask) = reconnect_without_tor(self).filter(|_| on) {
            // The popover is a modal of its own: the question takes its place.
            if self.screens.tor.open {
                self.close_tor(tx);
            }
            self.confirm(ask, |s, tx| {
                // Still on: it may have gone off another way while the question was open.
                if status(s).switched_on() {
                    s.turn_tor(false, tx);
                }
            });
            tx.instant();
            return;
        }
        self.turn_tor(!on, tx);
    }

    /// Asks the backend to turn Tor `on` or off, now.
    fn turn_tor(&mut self, on: bool, tx: &mut Tx) {
        self.screens.tor.detail.warning = None;
        self.tor_command(if on { TorCommand::On } else { TorCommand::Off });
        tx.instant();
    }

    /// Asks the backend to switch Tor, start it afresh or pick new routes: turned on, it says
    /// "Tor is on" once it is.
    pub fn tor_command(&mut self, command: TorCommand) {
        match command {
            TorCommand::On | TorCommand::StartAfresh => self.screens.tor.follow.switching(true),
            TorCommand::Off => self.screens.tor.follow.switching(false),
            TorCommand::NewRoutes => {}
        }
        self.backend_mut().tor(command);
    }
}

/// Tor's state as the window shows it: the backend's, or the sidebar's own while the
/// backend hasn't spoken (a demo, a test that sets `bench.tor()` alone).
pub fn status(s: &Shell) -> Status {
    use sshee_app::workbench::Tor;
    match (&s.screens.tor.detail.status, s.bench.tor()) {
        (Status::Off, Tor::On { .. }) => Status::On,
        (Status::Off, Tor::Starting) => Status::Starting { percent: 0, note: None },
        (Status::Off, Tor::Failed) => Status::Failed { message: "Tor didn't start.".into() },
        (st, _) => st.clone(),
    }
}

/// The onion's tooltip, and what a screen reader says of it: the state, and what a click does.
pub fn button_help(s: &Shell) -> String {
    format!("{}. Click for its switch and more", sshee_app::tor::rail_help(&status(s)))
}

/// What goes through Tor now and would reconnect around it were Tor turned off: terminals on
/// servers (but those that already go around it, [`State::direct`]), the other tabs on
/// servers, and the tunnels that are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Through {
    pub terminals: usize,
    pub files: usize,
    /// Stats and Containers tabs.
    pub other_tabs: usize,
    pub tunnels: usize,
}

impl Through {
    pub fn of(s: &Shell) -> Through {
        use sshee_app::tunnels::State as Tunnel;
        let mut through = Through::default();
        for tab in s.bench.tabs() {
            if !s.bench.server(&tab.server).is_some_and(|sv| !sv.is_local()) {
                continue;
            }
            match tab.kind {
                TabKind::Terminal if s.screens.tor.direct.contains(&tab.id) => {}
                TabKind::Terminal => through.terminals += 1,
                TabKind::Files => through.files += 1,
                TabKind::Stats | TabKind::Containers => through.other_tabs += 1,
            }
        }
        through.tunnels = s.screens.tunnels.tunnels.states.values().filter(|st| matches!(st, Tunnel::On { .. } | Tunnel::Reconnecting { .. })).count();
        through
    }

    pub fn is_empty(&self) -> bool {
        *self == Through::default()
    }

    /// "2 terminals, 1 Files tab and 1 tunnel".
    pub fn words(&self) -> String {
        let count = |n: usize, one: &str, many: &str| (n > 0).then(|| format!("{n} {}", if n == 1 { one } else { many }));
        let parts: Vec<String> =
            [count(self.terminals, "terminal", "terminals"), count(self.files, "Files tab", "Files tabs"), count(self.other_tabs, "other tab", "other tabs"), count(self.tunnels, "tunnel", "tunnels")]
                .into_iter()
                .flatten()
                .collect();
        match parts.split_last() {
            None => String::new(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        }
    }
}

/// The question before Tor goes off while something goes through it; `None` when nothing
/// does. Its yes shows your address to servers: red, and Return means Cancel.
pub fn reconnect_without_tor(s: &Shell) -> Option<Confirm> {
    let through = Through::of(s);
    if through.is_empty() {
        return None;
    }
    let body = format!("{} on servers will reconnect directly, and those servers will see your own address instead of a Tor one.", through.words());
    Some(Confirm::new("Reconnect without Tor?", body, "Reconnect without Tor").destructive(false))
}

/// The popover, beside the sidebar's onion while it's open.
pub fn view(s: &Shell, t: &'static Theme, cx: &mut Cx<Shell>) -> Option<El> {
    if !s.screens.tor.open {
        return None;
    }
    let d = &s.screens.tor.detail;
    let status = status(s);
    let latency = match s.bench.tor() {
        sshee_app::workbench::Tor::On { latency_ms } => latency_ms.or(d.latency_ms),
        _ => None,
    };
    let on = status.switched_on();

    // The head: the onion on its tile, the state, the switch.
    let tile_fill = if status == Status::On { t.tor_tile() } else { t.hov2 };
    let ink = match status {
        Status::On | Status::Starting { .. } => t.green,
        _ => t.l2,
    };
    let mut tile = rune::el().size(32.0, 32.0).radius(theme::RADIUS).fill(tile_fill).items_center().justify_center().child(icon(Icon::Tor, theme::ICON_RAIL, ink));
    if let Status::Failed { .. } = status {
        tile = tile.child(rune::el().absolute(21.0, 2.0).size(9.0, 9.0).radius(3.0).fill(t.red).a11y_hidden());
    }
    let mut titles = col().grow(1.0).shrink().min_w(0.0).gap(1.0).child(text(headline(&status)).key("tor-headline").font(theme::TEXT_BOLD).color(t.l1).single_line());
    let sub = match (&status, latency) {
        (Status::On, Some(ms)) => Some(format!("{ms} ms round trip")),
        (Status::Starting { percent, .. }, _) => Some(format!("{percent}% of the way")),
        _ => None,
    };
    if let Some(sub) = sub {
        titles = titles.child(text(sub).key("tor-sub").font(theme::STATUS_VALUE).color(t.l2).single_line());
    }
    let head = row()
        .w_full()
        .gap(10.0)
        .items_center()
        .child(tile)
        .child(titles)
        .child(switch(t, "Connect through Tor", on, true).key("tor-switch").on_press_tx(|s: &mut Shell, tx| s.switch_tor(tx)));

    let mut panel = col().w_full().px(14.0).pt(14.0).pb(12.0).gap(10.0).child(head);
    if let Some(fraction) = progress(&status) {
        let track = rune::el().w_full().h(4.0).radius(2.0).fill(t.control);
        let fill = rune::el().h(4.0).radius(2.0).fill(t.green).w((WIDTH - 28.0) * fraction.max(0.02));
        panel = panel.child(track.key("tor-bar").role("progressbar").label(format!("{}%", (fraction * 100.0).round())).child(fill.absolute(0.0, 0.0)));
    }
    let say = body(&status, latency);
    let say_color = if matches!(status, Status::Failed { .. }) { t.red } else { t.l2on };
    panel = panel.child(text(say).key("tor-body").font(theme::NOTE).color(say_color).wrap().w_full());
    if let Some(warning) = &d.warning {
        panel = panel.child(row().key("tor-warning").w_full().gap(6.0).items_start().child(icon(Icon::Warning, 13.0, t.orange).mt(1.0)).child(text(warning.as_str()).font(theme::NOTE).color(t.l2on).wrap().grow(1.0).shrink().min_w(0.0)));
    }
    // The log, while there's something to say of a start.
    if on && !d.log.is_empty() {
        let mut log = col().key("tor-log").w_full().max_h(LOG_HEIGHT).scroll(rune::ScrollSpec { stick_bottom: true }).px(10.0).py(8.0).gap(3.0).radius(theme::RADIUS_ROW).fill(t.field()).border(1.0, t.hair).role("log").label_static("Tor's log");
        for (i, line) in d.log.iter().enumerate() {
            log = log.child(text(line.as_str()).key(format!("tor-log-{i}")).font(theme::CODE).color(t.l2on).wrap().w_full());
        }
        panel = panel.child(col().w_full().gap(4.0).child(text("Log").font(theme::GROUP_TITLE).color(t.l2).single_line().role("heading").level(2)).child(log));
    }

    // What can be done, by state.
    let mut actions = row().w_full().gap(8.0).items_center();
    match status {
        Status::On => {
            actions = actions.child(
                push_button(t, "New routes", Push::Quiet, true)
                    .key("tor-new-routes")
                    .help("Each server picks a new way through Tor on its next connection: for when typing turned slow")
                    .on_press_tx(|s: &mut Shell, tx| {
                        s.tor_command(TorCommand::NewRoutes);
                        tx.instant();
                    }),
            );
        }
        // A start that's held up, or one that failed: offered, never done by itself.
        Status::Failed { .. } | Status::Starting { note: Some(_), .. } => {
            actions = actions.child(
                push_button(t, "Start fresh", Push::Quiet, true)
                    .key("tor-afresh")
                    .help("Starts Tor again from nothing: it fetches its lists of relays and picks new entry relays")
                    .on_press_tx(|s: &mut Shell, tx| {
                        s.tor_command(TorCommand::StartAfresh);
                        tx.instant();
                    }),
            );
        }
        Status::Off | Status::Starting { .. } => {}
    }
    if on {
        actions = actions.child(rune::spacer()).child(push_button(t, "Show log file", Push::Quiet, true).key("tor-show-log").on_press_tx(|s: &mut Shell, tx| {
            s.backend_mut().tor_ask(Ask::ShowLog);
            tx.instant();
        }));
        panel = panel.child(actions);
    }

    // Beside the onion: right of the sidebar (left of it when the sidebar is at the right),
    // its foot level with the onion's, pulled in from the window's edges.
    let [w, h] = cx.layout_size("window").unwrap_or([crate::shell::SIZE.0, crate::shell::SIZE.1]);
    let height = estimate_height(&status, d);
    let (x, y) = match (cx.window_layout("tor"), cx.window_layout("bar-tor")) {
        (Some([ax, ay, _, ah]), _) if s.sidebar_at_right() => (ax - WIDTH - 8.0, ay + ah - height),
        (Some([ax, ay, aw, ah]), _) => (ax + aw + 8.0, ay + ah - height),
        // The sidebar hidden: under the top bar's onion.
        (None, Some([ax, ay, aw, ah])) => (ax + aw - WIDTH, ay + ah + 6.0),
        // Neither: over the status bar's right end.
        (None, None) => (w - WIDTH - 12.0, h - theme::STATUS_HEIGHT - height - 8.0),
    };
    let x = x.min(w - WIDTH - 8.0).max(8.0).round();
    let y = y.min(h - height - 8.0).max(8.0).round();
    Some(popover(t, WIDTH).key("tor-panel").label_static("Tor").absolute(x, y).modal_tx(true, |s: &mut Shell, tx| s.close_tor(tx)).child(panel))
}

/// About how tall the popover is, to keep it inside the window before it's laid out.
fn estimate_height(status: &Status, d: &Detail) -> f32 {
    let mut h = 14.0 + 34.0 + 10.0 + 48.0 + 12.0;
    if progress(status).is_some() {
        h += 14.0;
    }
    if status.switched_on() {
        h += 38.0;
        if !d.log.is_empty() {
            h += 22.0 + LOG_HEIGHT;
        }
    }
    if d.warning.is_some() {
        h += 40.0;
    }
    h
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod height {
    use super::*;

    #[test]
    fn the_popover_grows_with_what_it_shows() {
        let off = estimate_height(&Status::Off, &Detail::default());
        let on = estimate_height(&Status::On, &Detail { log: vec!["a".into(), "b".into()], ..Detail::default() });
        assert!(on > off + 60.0);
    }
}
