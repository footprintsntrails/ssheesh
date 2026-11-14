//! "Add a server" (0.10's `AddServerDialog`, `ConfigImport` and `XpipeImport`): a form for one
//! server typed by hand, and above it the places servers can be brought in from: 0.10's own
//! list, `~/.ssh/config`, and XPipe's vault.
//!
//! ```text
//! add-scrim    dims the window; the dialog is modal (Esc closes; a click outside doesn't,
//! └── add-dialog                                      so a slip doesn't lose what was typed)
//!     Form   add-from-<source> rows · add-address, add-port, add-user · add-key | add-password,
//!            add-key-file / add-secret, add-remember · add-name · add-error · add-cancel, add-save
//!     List   add-back · pick-0 … (ticks) · add-all, add-none · add-cancel, add-import
//! ```
//!
//! The form's rules (what an address may be, what's wrong with a port) are
//! `sshee_app::servers::Form`. Saving, the keychain and reading other programs' files wait,
//! so the window only asks (`Ask`) and the backend answers later ([`Shell::servers_answer`]):
//! the dialog says "Adding…" meanwhile and shows what went wrong in the backend's own words.
//! The password is typed into a secure field (no screen reader, trace or tree ever holds it)
//! and leaves this file only inside an `Ask::Add`.

use rune::{col, row, text, text_input, El, ScrollSpec, TextEdit, Tx};
use sshee_app::servers::{import_button, Answer, Ask, Form, Password, SignIn, Source, Sources};

use crate::icons::{icon, Icon};
use crate::kit::field::{code_input, field, input, labelled};
use crate::kit::list::{list_row, ListRow};
use crate::kit::popover::popover;
use crate::kit::push::{check_box, choice, choices, push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The dialog's width.
pub const WIDTH: f32 = 520.0;

/// Which page the dialog shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Form,
    /// One source's servers, to tick.
    List(Source),
}

/// The dialog's own state.
pub struct State {
    pub open: bool,
    pub page: Page,
    pub address: TextEdit,
    pub port: TextEdit,
    pub user: TextEdit,
    pub name: TextEdit,
    pub key_file: TextEdit,
    /// Secure: drawn as dots, and its text is wiped when the dialog closes.
    pub password: TextEdit,
    pub sign_in: SignIn,
    pub remember: bool,
    /// What's wrong, under the form.
    pub error: Option<String>,
    /// The backend is saving.
    pub busy: bool,
    /// What the sources hold; `None` while the backend looks.
    pub sources: Option<Sources>,
    /// The ids ticked on the List page.
    pub picks: Vec<String>,
    /// What the last import did, over the form.
    pub notice: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            open: false,
            page: Page::Form,
            address: TextEdit::default(),
            port: TextEdit::default(),
            user: TextEdit::default(),
            name: TextEdit::default(),
            key_file: TextEdit::default(),
            password: TextEdit::default().secure(),
            sign_in: SignIn::Key,
            remember: true,
            error: None,
            busy: false,
            sources: None,
            picks: Vec::new(),
            notice: None,
        }
    }
}

impl State {
    /// The form as its rules read it. `with_password`: for saving; what is drawn never needs
    /// the password, so it isn't copied on every frame.
    fn form(&self, with_password: bool) -> Form {
        Form {
            address: self.address.text.clone(),
            port: self.port.text.clone(),
            user: self.user.text.clone(),
            name: self.name.text.clone(),
            sign_in: self.sign_in,
            key_file: self.key_file.text.clone(),
            password: if with_password { Password::new(self.password.text.clone()) } else { Password::default() },
            remember: self.remember,
        }
    }
}

impl Shell {
    /// "Add a server": the sidebar's "+", the main menu's tile, the palette's row, and
    /// 0.10's shortcut (⇧⌘N, Ctrl+Shift+N).
    pub fn open_add_server(&mut self, tx: &mut Tx) {
        self.screens.close_all();
        let mut state = State { open: true, ..State::default() };
        state.address.focused = true;
        self.screens.add_server = state;
        // What could be brought in: files to read, so the backend looks and says later.
        self.backend_mut().servers(Ask::Sources);
        tx.instant();
    }

    /// Closes it; what was typed is gone (the password's text is wiped by its field).
    pub fn close_add_server(&mut self, tx: &mut Tx) {
        self.screens.add_server = State::default();
        self.settle(tx);
    }

    /// "Add and connect", and Return in any field.
    fn add_server_submit(&mut self) {
        let state = &mut self.screens.add_server;
        if state.busy {
            return;
        }
        match state.form(true).check() {
            Ok(server) => {
                state.error = None;
                state.busy = true;
                self.backend_mut().servers(Ask::Add(server));
            }
            Err(why) => state.error = Some(why),
        }
    }

    /// Opens a source's list with its servers ticked as the source says.
    fn add_server_list(&mut self, source: Source) {
        let state = &mut self.screens.add_server;
        let Some(sources) = &state.sources else { return };
        state.picks = sources.of(source).iter().filter(|c| c.picked).map(|c| c.id.clone()).collect();
        state.page = Page::List(source);
        state.error = None;
    }

    /// The List page's button: bring the ticked ones in.
    fn add_server_import(&mut self) {
        let state = &mut self.screens.add_server;
        let (Page::List(source), Some(sources), false) = (state.page, &state.sources, state.busy) else { return };
        let offered = sources.of(source).iter().map(|c| c.id.clone()).collect();
        let ask = Ask::Import { source, picks: state.picks.clone(), offered };
        state.busy = true;
        self.backend_mut().servers(ask);
    }

    /// The backend's answer to an [`Ask`], from its thread through `Remote`.
    pub fn servers_answer(&mut self, answer: Answer) {
        let state = &mut self.screens.add_server;
        match answer {
            Answer::Sources(sources) => state.sources = Some(sources),
            Answer::Added { server, warning, .. } => {
                // "Add and connect": the dialog goes and the new server's terminal opens. A
                // half-right add (the keychain refused the password) still added it.
                let _ = warning;
                *state = State::default();
                if self.bench.server(&server).is_some() {
                    self.bench.go_to(&server);
                }
                self.screens.home.settle_due = true;
            }
            Answer::Imported { note, .. } => {
                state.busy = false;
                state.page = Page::Form;
                state.notice = Some(note);
                if state.open {
                    self.backend_mut().servers(Ask::Sources);
                }
            }
            // The list that comes with these says the rest.
            Answer::Removed { .. } | Answer::Changed => {}
            Answer::Failed(why) => {
                state.busy = false;
                state.error = Some(why);
            }
        }
    }
}

/// A part of the form with its name over it.
fn section(t: &Theme, name: &str, body: El) -> El {
    col().w_full().gap(8.0).child(text(name).font(theme::GROUP).color(t.l2).single_line()).child(body)
}

/// The red line under the form.
fn error_line(t: &Theme, why: &str) -> El {
    row().key("add-error").w_full().gap(6.0).items_start().role("alert").child(icon(Icon::Warning, 13.0, t.red).mt(1.0)).child(text(why).font(theme::NOTE).color(t.red).grow(1.0).shrink().min_w(0.0).wrap())
}

fn form_page(state: &State, t: &Theme) -> El {
    let form = state.form(false);
    let mut page = col().w_full().px(20.0).pt(18.0).pb(4.0).gap(16.0).child(text("Add a server").font(theme::TITLE).color(t.l1).single_line());

    // Where servers can be brought in from, once the backend has looked.
    if let Some(sources) = &state.sources {
        let offers = sources.offers();
        if !offers.is_empty() {
            let mut rows = col().key("add-sources").w_full().mx(-8.0);
            for (source, count) in offers {
                let key = match source {
                    Source::Old => "add-from-old",
                    Source::SshConfig => "add-from-config",
                    Source::Xpipe => "add-from-xpipe",
                };
                let spec = ListRow { icon: Some(Icon::Plus), label: source.label(), note: &count, ..ListRow::default() };
                rows = rows.child(list_row(t, &spec).key(key).hover(t.hov2).focusable(true).role("button").on_press_tx(move |s: &mut Shell, _| s.add_server_list(source)));
            }
            page = page.child(rows);
        }
    }
    if let Some(notice) = &state.notice {
        page = page.child(row().key("add-notice").w_full().gap(6.0).items_start().child(icon(Icon::Check, 13.0, t.l2on).mt(1.0)).child(text(notice).font(theme::NOTE).color(t.l2on).grow(1.0).shrink().min_w(0.0).wrap()));
    }

    // Where.
    let address = input(t, &state.address, "203.0.113.10, or paste: ssh root@host -p 2222").key("add-address").on_edit(|s: &mut Shell| &mut s.screens.add_server.address);
    let port = input(t, &state.port, "22").key("add-port").on_edit(|s: &mut Shell| &mut s.screens.add_server.port);
    let user = input(t, &state.user, "root").key("add-user").on_edit(|s: &mut Shell| &mut s.screens.add_server.user);
    let bad_address = state.error.as_deref().is_some_and(|e| e == sshee_app::servers::NO_ADDRESS || e == sshee_app::servers::BAD_ADDRESS);
    let bad_port = state.error.as_deref() == Some(sshee_app::servers::BAD_PORT);
    let mut place = col().w_full().gap(8.0).child(
        row()
            .w_full()
            .gap(8.0)
            .child(labelled(t, "Address", field(t, address, bad_address)).grow(1.0).min_w(0.0))
            .child(labelled(t, "Port", field(t, port, bad_port)).w(76.0))
            .child(labelled(t, "User", field(t, user, false)).w(120.0)),
    );
    // A pasted ssh command, read back: what the box means.
    if let Ok(reads) = form.check() {
        if reads.address != state.address.text.trim() && state.user.text.is_empty() && state.port.text.is_empty() {
            let key = reads.identity_file.as_deref().map(|k| format!(", key {k}")).unwrap_or_default();
            place = place.child(text(format!("Reads as {}{key}", reads.address)).key("add-reads").font(theme::SMALL).color(t.l2).single_line().truncate());
        }
    }
    page = page.child(section(t, "Where", place));

    // Sign in with.
    let ways = choices(
        t,
        [
            choice(t, "SSH key", state.sign_in == SignIn::Key).key("add-key").on_press_tx(|s: &mut Shell, _| s.screens.add_server.sign_in = SignIn::Key),
            choice(t, "Password", state.sign_in == SignIn::Password).key("add-password").on_press_tx(|s: &mut Shell, _| s.screens.add_server.sign_in = SignIn::Password),
        ],
    );
    let how = match state.sign_in {
        SignIn::Key => {
            let key_file = code_input(t, &state.key_file, "~/.ssh/my_key").key("add-key-file").on_edit(|s: &mut Shell| &mut s.screens.add_server.key_file);
            col().w_full().gap(4.0).child(field(t, key_file, false)).child(text("Leave it empty to use your usual keys.").font(theme::SMALL).color(t.l2).single_line())
        }
        SignIn::Password => {
            let secret = text_input(&state.password, theme::TEXT)
                .secure()
                .single_line()
                .key("add-secret")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .color(t.l1)
                .placeholder("Password", t.l2)
                .caret(t.ring, t.ring.alpha(0.35))
                .label_static("Password")
                .on_edit(|s: &mut Shell| &mut s.screens.add_server.password);
            let kept = if state.remember { "Saved in your system keychain, never in SSHeesh's own files." } else { "Kept only until you quit SSHeesh." };
            col()
                .w_full()
                .gap(6.0)
                .child(field(t, secret, false))
                .child(check_box(t, "Remember password", state.remember).key("add-remember").on_press_tx(|s: &mut Shell, _| s.screens.add_server.remember ^= true))
                .child(text(kept).font(theme::SMALL).color(t.l2).single_line())
        }
    };
    page = page.child(section(t, "Sign in with", col().w_full().gap(8.0).items_start().child(ways).child(how)));

    // In your list.
    let shown = form.parsed().host;
    let name = input(t, &state.name, if shown.trim().is_empty() { "Name" } else { shown.trim() }).key("add-name").on_edit(|s: &mut Shell| &mut s.screens.add_server.name);
    page = page.child(section(t, "In your list", field(t, name, false)));

    if let Some(why) = &state.error {
        page = page.child(error_line(t, why));
    }
    page
}

fn list_page(state: &State, t: &Theme, source: Source) -> El {
    let Some(sources) = &state.sources else { return col() };
    let offered = sources.of(source);
    let back = rune::el()
        .key("add-back")
        .size(theme::CONTROL, theme::CONTROL)
        .ml(-6.0)
        .radius(theme::RADIUS)
        .items_center()
        .justify_center()
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label_static("Back")
        .on_press_tx(|s: &mut Shell, _| s.screens.add_server.page = Page::Form)
        .child(icon(Icon::Back, theme::ICON, t.l2on));
    let head = col()
        .w_full()
        .gap(4.0)
        .child(row().w_full().gap(4.0).items_center().child(back).child(text(sources.title(source)).font(theme::TITLE).color(t.l1).single_line().truncate().shrink()))
        .child(text(source.promise()).font(theme::NOTE).color(t.l2).w_full().wrap());

    let mut list = col().key("add-list").w_full().max_h(300.0).padding(4.0).radius(theme::RADIUS).border(1.0, t.hair).scroll(ScrollSpec { stick_bottom: false });
    for (i, c) in offered.iter().enumerate() {
        let ticked = state.picks.contains(&c.id);
        let note = if c.address == c.name { "" } else { c.address.as_str() };
        let spec = ListRow { label: &c.name, note, checked: Some(ticked), ..ListRow::default() };
        let id = c.id.clone();
        list = list.child(list_row(t, &spec).key(format!("pick-{i}")).hover(t.hov2).focusable(true).role("checkbox").on_press_tx(move |s: &mut Shell, _| {
            let picks = &mut s.screens.add_server.picks;
            match picks.iter().position(|p| *p == id) {
                Some(at) => {
                    picks.remove(at);
                }
                None => picks.push(id.clone()),
            }
        }));
    }
    let link = |key: &'static str, words: &'static str| {
        text(words).key(key).font(theme::SMALL).color(t.l2on).single_line().role("button").focusable(true)
    };
    let all: Vec<String> = offered.iter().map(|c| c.id.clone()).collect();
    let ticks = row()
        .gap(14.0)
        .child(link("add-all", "Select all").on_press_tx(move |s: &mut Shell, _| s.screens.add_server.picks = all.clone()))
        .child(link("add-none", "Select none").on_press_tx(|s: &mut Shell, _| s.screens.add_server.picks.clear()));
    let mut page = col().w_full().px(20.0).pt(18.0).pb(4.0).gap(12.0).child(head).child(list).child(ticks);
    let notes = sources.notes(source);
    if !notes.is_empty() {
        page = page.child(col().w_full().gap(3.0).children(notes.into_iter().map(|n| text(n).font(theme::SMALL).color(t.l2).w_full().wrap())));
    }
    if let Some(why) = &state.error {
        page = page.child(error_line(t, why));
    }
    page
}

/// The dialog, when it's open: a last child of the window's root.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.add_server;
    if !state.open {
        return None;
    }
    let cancel = push_button(t, "Cancel", Push::Quiet, !state.busy).key("add-cancel").on_press_tx(|s: &mut Shell, tx| s.close_add_server(tx));
    let (page, done) = match state.page {
        Page::Form => {
            let label = if state.busy { "Adding…" } else { "Add and connect" };
            let ready = state.form(false).ready() && !state.busy;
            let mut save = push_button(t, label, Push::Primary, ready).key("add-save");
            if ready {
                // Return in any field presses it.
                save = save.default_button().on_press_tx(|s: &mut Shell, _| s.add_server_submit());
            }
            (form_page(state, t), save)
        }
        Page::List(source) => {
            let label = if state.busy { "Adding…".to_owned() } else { import_button(source, state.picks.len()) };
            let mut import = push_button(t, &label, Push::Primary, !state.busy).key("add-import");
            if !state.busy {
                import = import.default_button().on_press_tx(|s: &mut Shell, _| s.add_server_import());
            }
            (list_page(state, t, source), import)
        }
    };
    let foot = row().w_full().h(54.0).px(20.0).mt(14.0).gap(8.0).items_center().border_sides([1.0, 0.0, 0.0, 0.0], t.hair).child(rune::spacer()).child(cancel).child(done);
    let dialog = popover(t, WIDTH)
        .key("add-dialog")
        .label_static("Add a server")
        .max_h(720.0)
        .modal_tx(false, |s: &mut Shell, tx| {
            // Esc leaves the dialog, unless the backend is in the middle of saving.
            if !s.screens.add_server.busy {
                s.close_add_server(tx);
            }
        })
        .child(page)
        .child(foot);
    Some(rune::el().key("add-scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.scrim()).items_center().justify_center().child(dialog))
}
