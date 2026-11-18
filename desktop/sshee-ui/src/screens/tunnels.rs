//! Tunnels for one server (0.10's `Tunnels.tsx`): its saved tunnels with their switches and
//! how each is doing, and the form that adds or changes one. Three kinds: one of the server's
//! ports here on this computer, a SOCKS proxy that browses through the server, and (1.0's
//! own) a port of this computer's on the server.
//!
//! ```text
//! tunnels-scrim, tunnels-dialog      modal: Esc and a click outside close it
//!   tunnels-add                       "Add a tunnel"
//!   tunnel-<id>                       a row: tunnel-<id>-switch, -retry, -edit, -remove
//!   tunnels-form                      seg-0..2, tun-remote-port, tun-local-port, tun-host,
//!                                     tun-name, tun-error, tun-cancel, tun-save
//!   tunnels-done
//! ```
//!
//! The rules and the words are `sshee_app::tunnels`'. Every change goes to the backend as an
//! `Ask` (start, stop, keep the list); each tunnel's state comes back through
//! [`Remote::tunnels`]. The list is per server by host id, as 0.10 kept it.

use rune::{col, row, text, El, TextEdit, Tx};
use sshee_app::tunnels::{describe, digits, is_active, status_words, title, Ask, Form, Kind, Reply, Tone, Tunnels};

use crate::icons::{icon, Icon};
use crate::kit::field::{code_input, field, input, labelled};
use crate::kit::popover::popover;
use crate::kit::push::{icon_button, push_button, Push};
use crate::kit::segmented::segmented;
use crate::kit::switch::switch;
use crate::shell::{Remote, Shell};
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
    /// News of the tunnels: the list kept from last time, or one tunnel's state.
    pub fn tunnels(&self, reply: Reply) {
        self.update(move |s| s.screens.tunnels.tunnels.deliver(reply));
    }
}

/// The host id tunnels are kept under, for a server's key.
fn host_of(key: &str) -> Option<String> {
    sshee_app::servers::host_id(key).map(str::to_owned)
}

impl Shell {
    /// A server's tunnels, over the window.
    pub fn open_tunnels(&mut self, server: &str, tx: &mut Tx) {
        if host_of(server).is_none() {
            return;
        }
        self.screens.close_all();
        let st = &mut self.screens.tunnels;
        st.open = Some(server.to_owned());
        st.editing = None;
        st.error = None;
        // A server with nothing saved opens on the form.
        let host = host_of(server).unwrap_or_default();
        if st.tunnels.of(&host).is_empty() {
            self.tunnel_form(None);
        }
        tx.focus("tunnels-dialog", false);
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
    let mut page = col().w_full().px(20.0).pt(18.0).pb(6.0).gap(14.0).child(head);
    let mut intro = "A tunnel carries a port through your SSH login: one of the server's, here on this computer as localhost, or one of this computer's, on the server. Nothing is opened to the internet.".to_owned();
    if s.bench.tor() != sshee_app::workbench::Tor::Off {
        intro.push_str(" With Tor on, tunnels go through it too.");
    }
    page = page.child(text(intro).key("tunnels-intro").font(theme::NOTE).color(t.l2on).wrap().w_full());

    // The saved ones.
    let list = st.tunnels.of(&host);
    let adding = st.editing == Some(Editing::New);
    let mut heading = row().w_full().items_center().child(text("Saved tunnels").font(theme::GROUP).color(t.l2).single_line()).child(rune::spacer());
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
            text("Nothing saved yet. Saved tunnels that are on start again whenever SSHeesh opens.").key("tunnels-empty").font(theme::NOTE).color(t.l2).wrap().w_full().py(4.0),
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
        .child(page)
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
    if tone == Tone::Good {
        status = status.child(crate::kit::dot(6.0, t.green));
    } else if is_active(state) {
        status = status.child(crate::kit::progress_ring(11.0, 1.4, tone_color));
    }
    status = status.child(text(words).key(format!("tunnel-{id}-state")).font(theme::SMALL).color(tone_color).wrap().shrink().min_w(0.0));
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
    let mut body = col().key("tunnels-form").w_full().px(14.0).py(14.0).gap(12.0).radius(theme::RADIUS_GROUP).fill(t.group).border(1.0, t.ring.alpha(0.5)).child(
        segmented(t, "What it does", &kinds, picked, true, |s: &mut Shell, i, tx: &mut Tx| {
            let st = &mut s.screens.tunnels;
            st.kind = Kind::ALL[i];
            st.error = None;
            tx.instant();
        })
        .key("tun-kind"),
    );
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
