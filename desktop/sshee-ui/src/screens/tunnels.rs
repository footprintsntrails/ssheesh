//! Tunnels for one server (0.10's `Tunnels.tsx`): the web apps it runs, each opened in the
//! browser through a temporary tunnel with one click; its saved tunnels with their switches
//! and how each is doing; and the form that adds or changes one. Three kinds: one of the
//! server's ports here on this computer, a SOCKS proxy that browses through the server, and
//! (1.0's own) a port of this computer's on the server.
//!
//! ```text
//! tunnels-scrim, tunnels-dialog      modal: Esc and a click outside close it
//!   tunnels-reload                    look at the server again
//!   tunnels-apps                      "Web apps on this server": app-<i> rows, with
//!                                     app-<i>-open (or -connect), and while its tunnel is on
//!                                     app-<i>-copy (the address), -keep, -stop;
//!                                     tunnels-other (Other ports), other-<i> rows the same
//!   tunnels-add                       "Add a tunnel"
//!   tunnel-<id>                       a row: tunnel-<id>-switch, -copy, -browse, -retry,
//!                                     -edit, -remove
//!   tunnels-form                      seg-0..2, tun-remote-port, tun-local-port, tun-host,
//!                                     tun-name, tun-error, tun-cancel, tun-save
//!   tunnels-done
//! ```
//!
//! The rules and the words are `sshee_app::tunnels`'. Every change goes to the backend as an
//! `Ask` (start, stop, keep the list, look at the server); each tunnel's state and each look
//! come back through [`Remote::tunnels`]. The list is per server by host id, as 0.10 kept it.
//! A port here is checked free before a tunnel is saved (`Backend::port_problem`), and "Open"
//! asks the backend for a free one (`Backend::free_port`).

use rune::{col, row, text, El, ScrollSpec, TextEdit, Tx};
use sshee_app::tunnels::{app_facts, app_title, app_url, describe, digits, is_active, status_words, title, Ask, Form, Kind, Reply, State as TunnelState, Tone, Tunnels, WebApp};

use crate::icons::{icon, Icon};
use crate::kit::field::{code_input, field, input, labelled};
use crate::kit::popover::popover;
use crate::kit::push::{icon_button, push_button, Push};
use crate::kit::segmented::segmented;
use crate::kit::switch::switch;
use crate::shell::{Remote, Shell, Toast};
use crate::theme::{self, Theme};

const WIDTH: f32 = 560.0;

/// Which tunnel the form is open on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Editing {
    New,
    Saved(String),
}

/// The screen's own state. The tunnels themselves live here too, window-wide.
#[derive(Default)]
pub struct State {
    /// The server it's open on, by its key in the sidebar.
    pub open: Option<String>,
    pub tunnels: Tunnels,
    pub editing: Option<Editing>,
    pub kind: Kind,
    pub remote_port: TextEdit,
    pub local_port: TextEdit,
    pub remote_host: TextEdit,
    pub name: TextEdit,
    /// Why the form wasn't saved.
    pub error: Option<String>,
    /// "Other ports" is open.
    pub show_other: bool,
    /// Why "Open" on a web app did nothing: the app (its address as the server sees it, and
    /// its port), and the words.
    pub app_error: Option<((String, u16), String)>,
}

impl State {
    /// The form as typed.
    fn form(&self) -> Form {
        Form { kind: self.kind, remote_port: digits(&self.remote_port.text), local_port: digits(&self.local_port.text), remote_host: self.remote_host.text.clone(), name: self.name.text.clone() }
    }

    fn fill(&mut self, form: &Form) {
        self.kind = form.kind;
        self.remote_port = TextEdit::new(&form.remote_port);
        self.local_port = TextEdit::new(&form.local_port);
        self.remote_host = TextEdit::new(&form.remote_host);
        self.name = TextEdit::new(&form.name);
        // The keyboard starts in the first field the kind shows.
        if form.kind != Kind::Local {
            self.local_port.focused = true;
        } else {
            self.remote_port.focused = true;
        }
        self.error = None;
    }
}

impl Remote {
    /// News of the tunnels: the list kept from last time, one tunnel's state, a look at a
    /// server. A web app waiting to be opened opens in the browser once its tunnel is on.
    /// Every window keeps the list; the one waiting to open a web app opens it.
    pub fn tunnels(&self, reply: Reply) {
        self.every_tx(move |s, tx| {
            if let Some(url) = s.screens.tunnels.tunnels.deliver(reply.clone()) {
                s.open_in_browser(&url, tx);
            }
        });
    }
}

/// The host id tunnels are kept under, for a server's key.
fn host_of(key: &str) -> Option<String> {
    sshee_app::servers::host_id(key).map(str::to_owned)
}

impl Shell {
    /// A server's web apps and tunnels, over the window: what it runs is looked at again.
    pub fn open_tunnels(&mut self, server: &str, tx: &mut Tx) {
        let Some(host) = host_of(server) else { return };
        self.screens.close_all();
        let st = &mut self.screens.tunnels;
        st.open = Some(server.to_owned());
        st.editing = None;
        st.error = None;
        st.app_error = None;
        st.show_other = false;
        let ask = st.tunnels.scan(&host);
        self.tunnels_send(vec![ask]);
        tx.focus("tunnels-dialog", false);
        tx.instant();
    }

    /// Looks at what the server runs again.
    fn tunnels_reload(&mut self, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let ask = self.screens.tunnels.tunnels.scan(&host);
        self.tunnels_send(vec![ask]);
        tx.instant();
    }

    /// Opens `url` in the browser; says so when it can't.
    pub fn open_in_browser(&mut self, url: &str, tx: &mut Tx) {
        if let Err(e) = tx.open_url(url) {
            self.toast(Toast::error("Couldn't open the browser").body(e.to_string()));
        }
    }

    /// Copies a tunnel's address here, and says so (0.10's badge).
    fn copy_address(&mut self, port: u16, tx: &mut Tx) {
        let address = format!("localhost:{port}");
        tx.copy(address.clone());
        self.toast(Toast::info("Copied").body(address));
        tx.instant();
    }

    /// "Open" (`browse`) or "Connect" on a web app.
    fn app_open(&mut self, app: &WebApp, browse: bool, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let mut tunnels = std::mem::take(&mut self.screens.tunnels.tunnels);
        let opened = tunnels.open_app(&host, app, browse, |claimed| self.backend_mut().free_port(app.port, claimed));
        self.screens.tunnels.tunnels = tunnels;
        match opened {
            Ok((asks, now)) => {
                self.screens.tunnels.app_error = None;
                self.tunnels_send(asks);
                if let Some(url) = now {
                    self.open_in_browser(&url, tx);
                }
            }
            Err(why) => self.screens.tunnels.app_error = Some(((app.remote_host.clone(), app.port), why)),
        }
        tx.instant();
    }

    fn app_keep(&mut self, id: &str, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let asks = self.screens.tunnels.tunnels.keep(&host, id);
        self.tunnels_send(asks);
        tx.instant();
    }

    fn app_stop(&mut self, id: &str, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let asks = self.screens.tunnels.tunnels.stop_app(&host, id);
        self.tunnels_send(asks);
        tx.instant();
    }

    pub fn close_tunnels(&mut self, tx: &mut Tx) {
        self.screens.tunnels.open = None;
        self.screens.tunnels.editing = None;
        self.settle(tx);
    }

    fn tunnels_host(&self) -> Option<String> {
        host_of(self.screens.tunnels.open.as_deref()?)
    }

    fn tunnels_send(&mut self, asks: Vec<Ask>) {
        for ask in asks {
            self.backend_mut().tunnels(ask);
        }
    }

    /// The form, new (`None`) or on a saved tunnel.
    fn tunnel_form(&mut self, id: Option<&str>) {
        let host = self.tunnels_host().unwrap_or_default();
        let st = &mut self.screens.tunnels;
        let form = id.and_then(|id| st.tunnels.of(&host).iter().find(|t| t.id == id)).map(Form::of).unwrap_or_default();
        st.fill(&form);
        st.editing = Some(id.map_or(Editing::New, |id| Editing::Saved(id.to_owned())));
    }

    fn tunnel_save(&mut self, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let st = &mut self.screens.tunnels;
        let id = match &st.editing {
            Some(Editing::Saved(id)) => Some(id.clone()),
            Some(Editing::New) => None,
            None => return,
        };
        let form = st.form();
        let forward = match form.forward() {
            Ok(f) => f,
            Err(why) => {
                st.error = Some(why);
                return tx.instant();
            }
        };
        if let Some(why) = st.tunnels.clash(id.as_deref(), &forward) {
            st.error = Some(why);
            return tx.instant();
        }
        // The port here must be free (0.10's `localPortProblem`), unless the tunnel holds it.
        if let Some(port) = st.tunnels.port_to_check(&host, id.as_deref(), &forward) {
            if let Some(why) = self.backend_mut().port_problem(port) {
                self.screens.tunnels.error = Some(why);
                return tx.instant();
            }
        }
        let st = &mut self.screens.tunnels;
        let asks = st.tunnels.save(&host, id.as_deref(), &form.name, forward);
        st.editing = None;
        st.error = None;
        self.tunnels_send(asks);
        tx.focus("tunnels-dialog", false);
        tx.instant();
    }

    fn tunnel_switch(&mut self, id: &str, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let on = self.screens.tunnels.tunnels.of(&host).iter().find(|t| t.id == id).is_some_and(|t| t.on);
        let asks = self.screens.tunnels.tunnels.set_on(&host, id, !on);
        self.tunnels_send(asks);
        tx.instant();
    }

    fn tunnel_retry(&mut self, id: &str, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let asks = self.screens.tunnels.tunnels.retry(&host, id);
        self.tunnels_send(asks);
        tx.instant();
    }

    fn tunnel_remove(&mut self, id: &str, tx: &mut Tx) {
        let Some(host) = self.tunnels_host() else { return };
        let asks = self.screens.tunnels.tunnels.remove(&host, id);
        if self.screens.tunnels.editing == Some(Editing::Saved(id.to_owned())) {
            self.screens.tunnels.editing = None;
        }
        self.tunnels_send(asks);
        tx.instant();
    }
}

/// The dialog, while it's open.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let st = &s.screens.tunnels;
    let key = st.open.as_deref()?;
    let host = host_of(key)?;
    let server = s.bench.server(key)?;
    let name = s.server_shown(key, &server.name).into_owned();

    let head = row()
        .w_full()
        .gap(10.0)
        .items_center()
        .child(rune::el().size(32.0, 32.0).radius(theme::RADIUS).fill(t.hov2).items_center().justify_center().child(icon(Icon::Link, theme::ICON_RAIL, t.l2on)))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(1.0)
                .child(text("Tunnels").font(theme::TITLE).color(t.l1).single_line().role("heading").level(1))
                .child(text(name).key("tunnels-sub").font(theme::NOTE).color(t.l2).single_line().truncate()),
        );
    let loading = st.tunnels.scans.get(&host).is_some_and(|l| l.loading);
    let reload = if loading {
        rune::el().size(theme::FORM_CONTROL, theme::FORM_CONTROL).items_center().justify_center().child(crate::kit::progress_ring(13.0, 1.5, t.l2)).key("tunnels-loading").label_static("Looking at the server")
    } else {
        icon_button(t, Icon::Refresh, "Look again", true).key("tunnels-reload").on_press_tx(|s: &mut Shell, tx| s.tunnels_reload(tx))
    };
    let head = head.child(reload);
    let mut page = col().w_full().px(20.0).pt(18.0).pb(6.0).gap(14.0).child(head);
    let mut intro = "A tunnel carries a port through your SSH login: one of the server's, here on this computer as localhost, or one of this computer's, on the server. Nothing is opened to the internet.".to_owned();
    if s.bench.tor() != sshee_app::workbench::Tor::Off {
        intro.push_str(" With Tor on, tunnels go through it too.");
    }
    page = page.child(text(intro).key("tunnels-intro").font(theme::NOTE).color(t.l2on).wrap().w_full());
    page = page.child(web_apps(s, t, &host));

    // The saved ones.
    let list = st.tunnels.of(&host);
    let adding = st.editing == Some(Editing::New);
    let mut heading = row()
        .w_full()
        .gap(2.0)
        .items_center()
        .child(text("Saved tunnels").font(theme::GROUP).color(t.l2).single_line())
        .child(super::help::tip(s, t, "tunnels-saved-about", "saved tunnels", super::help::SAVED_TUNNELS))
        .child(rune::spacer());
    if !adding {
        heading = heading.child(
            row()
                .key("tunnels-add")
                .gap(4.0)
                .px(6.0)
                .h(theme::FORM_CONTROL)
                .radius(theme::RADIUS_ROW)
                .items_center()
                .hover(t.hov2)
                .press(t.down)
                .focusable(true)
                .role("button")
                .label_static("Add a tunnel")
                .child(icon(Icon::Plus, 12.0, t.l2on))
                .child(text("Add a tunnel").font(theme::SMALL).color(t.l2on).single_line().a11y_hidden())
                .on_press_tx(|s: &mut Shell, tx| {
                    s.tunnel_form(None);
                    tx.focus("tun-remote-port", false);
                    tx.instant();
                }),
        );
    }
    let mut rows = col().key("tunnels-list").w_full().gap(6.0);
    if adding {
        rows = rows.child(form_view(s, t));
    }
    if list.is_empty() && !adding {
        rows = rows.child(
            text("Nothing saved yet. Keep a web app you opened above, or add a tunnel. Saved tunnels that are on start again whenever SSHeesh opens.")
                .key("tunnels-empty")
                .font(theme::NOTE)
                .color(t.l2)
                .wrap()
                .w_full()
                .py(4.0),
        );
    }
    for tunnel in list {
        if st.editing == Some(Editing::Saved(tunnel.id.clone())) {
            rows = rows.child(form_view(s, t));
        } else {
            rows = rows.child(tunnel_row(s, t, tunnel));
        }
    }
    page = page.child(col().w_full().gap(8.0).child(heading).child(rows));

    let foot = row()
        .w_full()
        .h(54.0)
        .px(20.0)
        .mt(8.0)
        .gap(8.0)
        .items_center()
        .border_sides([1.0, 0.0, 0.0, 0.0], t.hair)
        .child(text(active_note(&st.tunnels, &host)).key("tunnels-count").font(theme::SMALL).color(t.l2).single_line())
        .child(rune::spacer())
        .child(push_button(t, "Done", Push::Quiet, true).key("tunnels-done").on_press_tx(|s: &mut Shell, tx| s.close_tunnels(tx)));
    let dialog = popover(t, WIDTH)
        .key("tunnels-dialog")
        .label_static("Tunnels")
        .max_h(640.0)
        .focusable(true)
        .no_focus_ring()
        .modal_tx(true, |s: &mut Shell, tx| s.close_tunnels(tx))
        .child(col().key("tunnels-body").w_full().shrink().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(page))
        .child(foot);
    let dialog = crate::kit::motion::enters(dialog, crate::kit::motion::Entrance::Card);
    Some(crate::kit::motion::enters(rune::el().key("tunnels-scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.scrim()).items_center().justify_center().child(dialog), crate::kit::motion::Entrance::Scrim))
}

/// "2 on", "None on".
fn active_note(tunnels: &Tunnels, host: &str) -> String {
    match tunnels.active(host) {
        0 => "None on".into(),
        n => format!("{n} on"),
    }
}

/// A section's heading, with a quiet word or button at its end.
fn section_head(t: &Theme, title: &str) -> El {
    row().w_full().items_center().child(text(title).font(theme::GROUP).color(t.l2).single_line()).child(rune::spacer())
}

/// "Web apps on this server": what the last look found, each with its button.
fn web_apps(s: &Shell, t: &'static Theme, host: &str) -> El {
    let st = &s.screens.tunnels;
    let look = st.tunnels.scans.get(host);
    let head = section_head(t, "Web apps on this server").child(text("Open one in your browser").font(theme::SMALL).color(t.l2).single_line());
    let mut out = col().key("tunnels-apps").w_full().gap(8.0).child(head);
    let result = look.and_then(|l| l.result.as_ref());
    let Some(result) = result else {
        return out.child(
            row()
                .key("tunnels-looking")
                .gap(8.0)
                .items_center()
                .py(4.0)
                .child(crate::kit::progress_ring(12.0, 1.4, t.l2))
                .child(text("Looking at what runs on the server…").font(theme::NOTE).color(t.l2).single_line()),
        );
    };
    let scan = match result {
        Ok(scan) => scan,
        Err(why) => return out.child(text(s.text_shown(why).into_owned()).key("tunnels-scan-error").font(theme::NOTE).color(t.red).wrap().w_full()),
    };
    let web: Vec<&WebApp> = scan.web().collect();
    let other: Vec<&WebApp> = scan.other().collect();
    let orphans = st.tunnels.orphans(host);
    if !scan.curl && !web.is_empty() {
        out = out.child(text("Install curl on the server to see each page's title here.").key("tunnels-no-curl").font(theme::NOTE).color(t.l2).wrap().w_full());
    }
    if web.is_empty() && orphans.is_empty() {
        let mut empty = "Nothing here answers like a website.".to_owned();
        if !other.is_empty() {
            empty.push_str(" Other programs that listen are below.");
        }
        out = out.child(text(empty).key("tunnels-no-apps").font(theme::NOTE).color(t.l2).wrap().w_full().py(2.0));
    }
    let mut list = col().w_full().gap(6.0);
    for (i, app) in web.iter().copied().chain(orphans.iter()).enumerate() {
        list = list.child(app_row(s, t, host, app, &format!("app-{i}")));
    }
    out = out.child(list);
    if !other.is_empty() {
        let open = st.show_other;
        let toggle = row()
            .key("tunnels-other")
            .gap(4.0)
            .h(theme::FORM_CONTROL)
            .px(4.0)
            .ml(-4.0)
            .radius(theme::RADIUS_ROW)
            .items_center()
            .hover(t.hov2)
            .press(t.down)
            .focusable(true)
            .role("button")
            .expanded(open)
            .label(format!("Other ports ({})", other.len()))
            .on_press_tx(|s: &mut Shell, tx| {
                s.screens.tunnels.show_other = !s.screens.tunnels.show_other;
                tx.instant();
            })
            .child(icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }, 12.0, t.l2))
            .child(text(format!("Other ports ({})", other.len())).font(theme::SMALL).color(t.l2on).single_line().a11y_hidden());
        out = out.child(row().w_full().child(toggle));
        if open {
            out = out.child(text("Not websites. Connect brings one here, so an app on this computer (a database app, say) can use it at localhost.").font(theme::SMALL).color(t.l2).wrap().w_full());
            let mut list = col().w_full().gap(6.0);
            for (i, app) in other.iter().enumerate() {
                list = list.child(app_row(s, t, host, app, &format!("other-{i}")));
            }
            out = out.child(list);
        }
    }
    out
}

/// One web app: its name and what it is, and Open (or Connect); while its tunnel is on, its
/// address (a click copies it), Keep for a temporary one, and Stop.
fn app_row(s: &Shell, t: &'static Theme, host: &str, app: &WebApp, key: &str) -> El {
    let st = &s.screens.tunnels;
    let to = st.tunnels.tunnel_to(host, app);
    let state = to.as_ref().and_then(|to| st.tunnels.states.get(&to.id));
    let active = to.is_some() && is_active(state);
    let name = s.text_shown(&app_title(app)).into_owned();
    let about = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(1.0)
        .child(text(name.clone()).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate())
        .child(text(s.text_shown(&app_facts(app)).into_owned()).font(theme::SMALL).color(t.l2).single_line().truncate());
    let mut line = row().w_full().gap(10.0).items_center().child(icon(if app.web { Icon::Globe } else { Icon::Link }, 15.0, if app.web { t.blue } else { t.l2 })).child(about);
    let mut buttons = row().gap(4.0).items_center();
    match (&to, active) {
        (Some(to), true) => {
            let port = to.port;
            buttons = buttons.child(match state {
                Some(TunnelState::On { .. }) => address_chip(t, port).key(format!("{key}-copy")),
                Some(TunnelState::Reconnecting { .. }) => busy_words("Reconnecting…", t.orange),
                _ => busy_words("Starting…", t.l2),
            });
            if app.web && matches!(state, Some(TunnelState::On { .. })) {
                let open = app.clone();
                buttons = buttons.child(
                    push_button(t, "Open", Push::Quiet, true)
                        .key(format!("{key}-open"))
                        .help(app_url(app.scheme.as_deref(), port))
                        .on_press_tx(move |s: &mut Shell, tx| s.app_open(&open, true, tx)),
                );
            }
            if !to.saved {
                let id = to.id.clone();
                buttons = buttons.child(
                    push_button(t, "Keep", Push::Quiet, true)
                        .key(format!("{key}-keep"))
                        .help("Save it, and start it again whenever SSHeesh opens")
                        .on_press_tx(move |s: &mut Shell, tx| s.app_keep(&id, tx)),
                );
            }
            let id = to.id.clone();
            buttons = buttons.child(push_button(t, "Stop", Push::Quiet, true).key(format!("{key}-stop")).on_press_tx(move |s: &mut Shell, tx| s.app_stop(&id, tx)));
        }
        _ => {
            let open = app.clone();
            let (label, kind, help) = if app.web {
                ("Open", Push::Primary, format!("Opens the server's port {} in your browser", app.port))
            } else {
                ("Connect", Push::Quiet, format!("Makes the server's port {} work on this computer, at localhost", app.port))
            };
            let browse = app.web;
            buttons = buttons.child(
                push_button(t, label, kind, true)
                    .key(format!("{key}-{}", if browse { "open" } else { "connect" }))
                    .help(help)
                    .on_press_tx(move |s: &mut Shell, tx| s.app_open(&open, browse, tx)),
            );
        }
    }
    line = line.child(buttons);
    let mut card = col().key(key.to_owned()).w_full().px(12.0).py(9.0).gap(4.0).radius(theme::RADIUS_GROUP).fill(t.group).border(1.0, t.hair2).role("group").label(name).child(line);
    // What went wrong, or what to do with it, under the row.
    let said = |words: String, color| text(words).font(theme::SMALL).color(color).wrap().w_full().pl(25.0);
    let error = st.app_error.as_ref().filter(|((h, p), _)| *h == app.remote_host && *p == app.port).map(|(_, why)| why.clone());
    if let Some(why) = error {
        card = card.child(said(why, t.red).key(format!("{key}-error")));
    } else if let Some(TunnelState::Failed { message }) = state {
        card = card.child(said(message.clone(), t.red).key(format!("{key}-error")));
    } else if let (true, Some(TunnelState::Reconnecting { .. })) = (active, state) {
        let (words, _) = status_words(&Default::default(), state);
        card = card.child(said(words, t.orange));
    } else if let (true, Some(TunnelState::On { warning: Some(warning) })) = (active, state) {
        card = card.child(said(warning.clone(), t.orange));
    } else if let (true, Some(TunnelState::On { warning: None }), false, Some(to)) = (active, state, app.web, &to) {
        card = card.child(said(format!("Point your app at localhost:{} on this computer.", to.port), t.l2));
    }
    card
}

/// "● localhost:3000": the address of a tunnel that's on; a click copies it.
fn address_chip(t: &Theme, port: u16) -> El {
    row()
        .gap(6.0)
        .h(theme::FORM_CONTROL)
        .px(6.0)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label(format!("Copy localhost:{port}"))
        .help("Copy the address")
        .on_press_tx(move |s: &mut Shell, tx| s.copy_address(port, tx))
        .child(crate::kit::dot(6.0, t.green))
        .child(text(format!("localhost:{port}")).font(theme::CODE).color(t.l2on).single_line().a11y_hidden())
}

/// "Starting…" with its ring.
fn busy_words(words: &str, color: rune::Color) -> El {
    row().gap(6.0).items_center().px(4.0).child(crate::kit::progress_ring(11.0, 1.4, color)).child(text(words.to_owned()).font(theme::SMALL).color(color).single_line())
}

/// One saved tunnel: its switch, what it is, how it's doing, and what can be done.
fn tunnel_row(s: &Shell, t: &'static Theme, tunnel: &sshee_app::model::SavedTunnel) -> El {
    let st = &s.screens.tunnels;
    let state = if tunnel.on { st.tunnels.states.get(&tunnel.id) } else { None };
    let id = tunnel.id.clone();
    let name = s.text_shown(&title(tunnel)).into_owned();
    let (words, tone) = status_words(&tunnel.forward, state);
    let tone_color = match tone {
        Tone::Quiet => t.l2,
        Tone::Good => t.green,
        Tone::Warn => t.orange,
        Tone::Bad => t.red,
    };
    let mut status = row().gap(6.0).items_center();
    let here = sshee_app::tunnels::listens_here(&tunnel.forward).filter(|_| matches!(state, Some(TunnelState::On { .. })));
    if tone == Tone::Good {
        status = status.child(crate::kit::dot(6.0, t.green));
    } else if is_active(state) {
        status = status.child(crate::kit::progress_ring(11.0, 1.4, tone_color));
    }
    match here {
        // "On at localhost:3000": a click on the address copies it (0.10's badge).
        Some(port) => {
            status = status.child(text("On at").key(format!("tunnel-{id}-state")).font(theme::SMALL).color(tone_color).single_line()).child(
                row()
                    .key(format!("tunnel-{id}-copy"))
                    .px(4.0)
                    .ml(-2.0)
                    .radius(theme::RADIUS_ROW)
                    .hover(t.hov2)
                    .press(t.down)
                    .focusable(true)
                    .role("button")
                    .label(format!("Copy localhost:{port}"))
                    .help("Copy the address")
                    .on_press_tx(move |s: &mut Shell, tx| s.copy_address(port, tx))
                    .child(text(format!("localhost:{port}")).font(theme::CODE).color(tone_color).single_line().a11y_hidden()),
            );
        }
        None => status = status.child(text(words).key(format!("tunnel-{id}-state")).font(theme::SMALL).color(tone_color).wrap().shrink().min_w(0.0)),
    }
    let mut about = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(text(name.clone()).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate())
        .child(text(s.text_shown(&describe(&tunnel.forward)).into_owned()).font(theme::SMALL).color(t.l2).wrap())
        .child(status.mt(2.0));
    if let Some(sshee_app::tunnels::State::On { warning: Some(warning) }) = state {
        about = about.child(text(warning.as_str()).key(format!("tunnel-{id}-warning")).font(theme::SMALL).color(t.orange).wrap());
    }
    let (sw, retry, edit, remove) = (id.clone(), id.clone(), id.clone(), id.clone());
    let mut buttons = row().gap(2.0).items_center().mt(-2.0);
    // A server's port here, on: "Open localhost:3000 in your browser".
    if let (Some(port), sshee_app::model::ForwardKind::Local) = (here, &tunnel.forward.kind) {
        let url = app_url(tunnel.scheme.as_ref().map(|s| s.as_str()), port);
        buttons = buttons.child(
            icon_button(t, Icon::Globe, &format!("Open localhost:{port} in your browser"), true).key(format!("tunnel-{id}-browse")).on_press_tx(move |s: &mut Shell, tx| s.open_in_browser(&url, tx)),
        );
    }
    if matches!(state, Some(sshee_app::tunnels::State::Failed { .. })) {
        buttons = buttons.child(icon_button(t, Icon::Refresh, "Try again", true).key(format!("tunnel-{id}-retry")).on_press_tx(move |s: &mut Shell, tx| s.tunnel_retry(&retry, tx)));
    }
    buttons = buttons
        .child(icon_button(t, Icon::Pencil, "Change", true).key(format!("tunnel-{id}-edit")).on_press_tx(move |s: &mut Shell, tx| {
            s.tunnel_form(Some(&edit));
            tx.focus("tun-remote-port", false);
            tx.instant();
        }))
        .child(icon_button(t, Icon::Bin, "Remove", true).key(format!("tunnel-{id}-remove")).on_press_tx(move |s: &mut Shell, tx| s.tunnel_remove(&remove, tx)));
    row()
        .key(format!("tunnel-{id}"))
        .w_full()
        .px(12.0)
        .py(10.0)
        .gap(12.0)
        .items_start()
        .radius(theme::RADIUS_GROUP)
        .fill(t.group)
        .border(1.0, t.hair2)
        .child(switch(t, &format!("{name}: on or off"), tunnel.on, true).key(format!("tunnel-{id}-switch")).on_press_tx(move |s: &mut Shell, tx| s.tunnel_switch(&sw, tx)))
        .child(about)
        .child(buttons)
}

/// The form that adds a tunnel or changes one.
fn form_view(s: &Shell, t: &'static Theme) -> El {
    let st = &s.screens.tunnels;
    let form = st.form();
    let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.label()).collect();
    let picked = Kind::ALL.iter().position(|k| *k == st.kind);
    let kind = segmented(t, "What it does", &kinds, picked, true, |s: &mut Shell, i, tx: &mut Tx| {
        let st = &mut s.screens.tunnels;
        st.kind = Kind::ALL[i];
        st.error = None;
        tx.instant();
    })
    .key("tun-kind");
    let kind = row().w_full().gap(4.0).items_center().child(kind).child(super::help::tip(s, t, "tun-kind-about", "the kinds of tunnel", super::help::TUNNEL_KINDS));
    let mut body = col().key("tunnels-form").w_full().px(14.0).py(14.0).gap(12.0).radius(theme::RADIUS_GROUP).fill(t.group).border(1.0, t.ring.alpha(0.5)).child(kind);
    let follows = form.local_text();
    let remote_port = code_input(t, &st.remote_port, "3000").key("tun-remote-port").on_edit(|s: &mut Shell| &mut s.screens.tunnels.remote_port).on_submit(|s: &mut Shell, tx| s.tunnel_save(tx));
    let local_hint = if follows.is_empty() { if st.kind == Kind::Socks { "1080".to_owned() } else { "3000".to_owned() } } else { follows.clone() };
    let local_port = code_input(t, &st.local_port, &local_hint).key("tun-local-port").on_edit(|s: &mut Shell| &mut s.screens.tunnels.local_port).on_submit(|s: &mut Shell, tx| s.tunnel_save(tx));
    let error = st.error.as_deref();
    let bad = |what: &str| error.is_some_and(|e| e.contains(what));
    match st.kind {
        Kind::Local => {
            let host = code_input(t, &st.remote_host, sshee_app::tunnels::SERVER).key("tun-host").on_edit(|s: &mut Shell| &mut s.screens.tunnels.remote_host).on_submit(|s: &mut Shell, tx| s.tunnel_save(tx));
            body = body
                .child(
                    row()
                        .w_full()
                        .gap(10.0)
                        .child(labelled(t, "The server's port", field(t, remote_port, bad("server's port"))).grow(1.0).min_w(0.0))
                        .child(labelled(t, "Port on this computer", field(t, local_port, bad("on this computer"))).grow(1.0).min_w(0.0)),
                )
                .child(
                    col()
                        .w_full()
                        .gap(4.0)
                        .child(labelled(t, "Address, as the server sees it", field(t, host, bad("address"))))
                        .child(text("127.0.0.1 is the server itself. Another address reaches a machine the server can see, like a database on its network.").font(theme::HELP).color(t.l2).wrap()),
                );
        }
        Kind::Socks => {
            body = body.child(
                col()
                    .w_full()
                    .gap(4.0)
                    .child(labelled(t, "Port on this computer", field(t, local_port, bad("on this computer")).w(140.0)))
                    .child(text(format!("Set your browser's or app's proxy to SOCKS5 at localhost:{follows}. Sites then see the server's address instead of yours, and the server's private network works too.")).font(theme::HELP).color(t.l2).wrap()),
            );
        }
        Kind::Reverse => {
            body = body
                .child(
                    row()
                        .w_full()
                        .gap(10.0)
                        .child(labelled(t, "Port on this computer", field(t, local_port, bad("on this computer"))).grow(1.0).min_w(0.0))
                        .child(labelled(t, "The server's port", field(t, remote_port, bad("server's port"))).grow(1.0).min_w(0.0)),
                )
                .child(text("Programs on the server reach this computer's port at 127.0.0.1 there: a site you're building, shown to a webhook on the server. Only the server itself can use it.").font(theme::HELP).color(t.l2).wrap());
        }
    }
    let name_hint = match st.kind {
        Kind::Socks => "Browse through the server",
        Kind::Reverse => "My dev site",
        Kind::Local => "Grafana",
    };
    body = body.child(labelled(t, "Name (optional)", field(t, input(t, &st.name, name_hint).key("tun-name").on_edit(|s: &mut Shell| &mut s.screens.tunnels.name).on_submit(|s: &mut Shell, tx| s.tunnel_save(tx)), false)));
    match (error, form.forward()) {
        (Some(why), _) => {
            body = body.child(row().key("tun-error").w_full().gap(6.0).items_start().role("alert").child(icon(Icon::Warning, 13.0, t.red).mt(1.0)).child(text(why).font(theme::NOTE).color(t.red).grow(1.0).shrink().min_w(0.0).wrap()));
        }
        (None, Ok(forward)) => {
            body = body.child(text(s.text_shown(&describe(&forward)).into_owned()).key("tun-reads").font(theme::NOTE).color(t.l2on).wrap().w_full());
        }
        (None, Err(_)) => {}
    }
    let new = st.editing == Some(Editing::New);
    body.child(
        row()
            .w_full()
            .gap(8.0)
            .items_center()
            .child(rune::spacer())
            .child(push_button(t, "Cancel", Push::Quiet, true).key("tun-cancel").on_press_tx(|s: &mut Shell, tx| {
                s.screens.tunnels.editing = None;
                s.screens.tunnels.error = None;
                tx.focus("tunnels-dialog", false);
                tx.instant();
            }))
            .child(push_button(t, if new { "Save and turn on" } else { "Save" }, Push::Primary, true).key("tun-save").on_press_tx(|s: &mut Shell, tx| s.tunnel_save(tx))),
    )
}
