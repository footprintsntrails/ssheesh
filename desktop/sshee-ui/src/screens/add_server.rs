//! "Add a server" (0.10's `AddServerDialog`, `ConfigImport` and `XpipeImport`): a form for one
//! server typed by hand, and above it the places servers can be brought in from: 0.10's own
//! list, `~/.ssh/config`, XPipe's vault, and other SSH apps' saved sessions (PuTTY, KiTTY,
//! WinSCP, MobaXterm, FileZilla, SecureCRT, Remmina), each in the same pick list.
//!
//! ```text
//! add-scrim    dims the window; the dialog is modal (Esc closes; a click outside doesn't,
//! └── add-dialog                                      so a slip doesn't lose what was typed)
//!     Form   add-body (scrolls) · add-from-<source> rows · add-address, add-port, add-user ·
//!            add-key | add-password | add-manager · key-usual, key-0 …, key-other, add-key-file ·
//!            add-secret, add-eye, add-remember · manager-0 … · add-look (icons open in place:
//!            look-<id>), look-color-0 … 5, look-hex (your own colour), look-env-<prod|staging|
//!            dev> · add-picture, add-no-picture · add-name · add-test, test-step-0 … (step-copy:
//!            tmux's install command), add-trust · add-error · add-cancel, add-save
//!     List   add-back · pick-0 … (ticks) · add-all, add-none · add-cancel, add-import
//! ```
//!
//! The form's rules (what an address may be, what's wrong with a port) are
//! `sshee_app::servers::Form`. Saving, the keychain, a check of the server and reading other
//! programs' files wait, so the window only asks (`Ask`) and the backend answers later
//! ([`Shell::servers_answer`]): the dialog says "Adding…" or "Checking…" meanwhile and shows
//! what went wrong in the backend's own words. The password is typed into a secure field (no
//! screen reader, trace or tree ever holds it; the eye shows its characters, nothing else) and
//! leaves this file only inside an `Ask::Add` or `Ask::Test`.

use std::path::PathBuf;

use rune::{col, row, text, text_input, El, FileDialogOptions, ScrollSpec, TextEdit, Tx};
use sshee_app::labels::{env_name, env_order, icon_name, parse_color, HOST_COLORS, HOST_ICONS};
use sshee_app::model::EnvLabel;
use sshee_app::servers::{
    import_button, old_passwords_question, Answer, Ask, Connection, Form, LocalAuth, Password, PickedLook, SignIn, Source, Sources, StepTone, Tested, OLD_PASSWORDS_MEANING,
};

use crate::icons::{icon, Icon};
use crate::kit::field::{code_input, field, input, labelled};
use crate::kit::list::{list_row, ListRow};
use crate::kit::popover::popover;
use crate::kit::push::{check_box, choice, choices, icon_button, push_button, Push};
use crate::shell::{Confirm, Shell, Toast};
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

/// Which key signs in, when it's a key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum KeyChoice {
    /// Your usual keys: ssh picks.
    #[default]
    Usual,
    /// One of the keys found in `~/.ssh`, by path.
    Found(String),
    /// A key file typed in.
    Other,
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
    pub key: KeyChoice,
    /// Secure: drawn as dots (unless the eye is open), and its text is wiped when the dialog
    /// closes.
    pub password: TextEdit,
    pub show_password: bool,
    pub sign_in: SignIn,
    pub remember: bool,
    /// The password manager's socket.
    pub agent: String,
    /// The keys and managers found here; `None` while the backend looks.
    pub local: Option<LocalAuth>,
    /// An icon, a colour picked in the form; `None`: the guessed icon in its own colour.
    pub look: Option<PickedLook>,
    pub env: Option<EnvLabel>,
    /// The icons are open under the name.
    pub icons_open: bool,
    /// A colour of your own, typed as `#2389e2`: it wins over the swatches while it reads as one.
    pub hex: TextEdit,
    /// A picture of your own for its icon, made into one once the server is added
    /// (`Ask::SetImage`, as the server's menu does).
    pub picture: Option<PathBuf>,
    /// What's wrong, under the form.
    pub error: Option<String>,
    /// The backend is saving.
    pub busy: bool,
    /// The backend is checking the server.
    pub testing: bool,
    /// What the last check found, and the form it was made for: a change to how it connects
    /// makes it stale.
    pub tested: Option<(Tested, Connection)>,
    /// What the sources hold; `None` while the backend looks.
    pub sources: Option<Sources>,
    /// Open this source's list as soon as the sources are known ("Review" in the sidebar).
    pub want: Option<Source>,
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
            key: KeyChoice::Usual,
            password: TextEdit::default().secure(),
            show_password: false,
            sign_in: SignIn::Key,
            remember: true,
            agent: String::new(),
            local: None,
            look: None,
            env: None,
            icons_open: false,
            hex: TextEdit::default(),
            picture: None,
            error: None,
            busy: false,
            testing: false,
            tested: None,
            sources: None,
            want: None,
            picks: Vec::new(),
            notice: None,
        }
    }
}

impl State {
    /// The form as its rules read it. `with_password`: for saving and checking; what is drawn
    /// never needs the password, so it isn't copied on every frame.
    fn form(&self, with_password: bool) -> Form {
        let key_file = match &self.key {
            KeyChoice::Usual => String::new(),
            KeyChoice::Found(path) => path.clone(),
            KeyChoice::Other => self.key_file.text.clone(),
        };
        Form {
            address: self.address.text.clone(),
            port: self.port.text.clone(),
            user: self.user.text.clone(),
            name: self.name.text.clone(),
            sign_in: self.sign_in,
            key_file,
            password: if with_password { Password::new(self.password.text.clone()) } else { Password::new("•".repeat(self.password.text.chars().count())) },
            remember: self.remember,
            agent: self.agent.clone(),
            look: self.look(),
            env: self.env.clone(),
        }
    }

    /// The look picked, with the colour typed in the field when it reads as one.
    fn look(&self) -> Option<PickedLook> {
        let Some(color) = parse_color(&self.hex.text) else { return self.look.clone() };
        let icon = self.look.as_ref().map(|l| l.icon.clone()).filter(|i| !i.is_empty()).unwrap_or_else(|| "server".into());
        Some(PickedLook { icon, color, image: None })
    }

    /// The last check, while it still holds for the form.
    fn check_shown(&self) -> Option<&Tested> {
        let (tested, made_for) = self.tested.as_ref()?;
        (*made_for == self.form(false).connection()).then_some(tested)
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
        // What could be brought in, and the keys here: files to read, so the backend looks
        // and says later.
        self.backend_mut().servers(Ask::Sources);
        self.backend_mut().servers(Ask::LocalAuth);
        tx.instant();
    }

    /// "N new in ~/.ssh/config · Review": the dialog, on that list.
    pub fn review_config(&mut self, tx: &mut Tx) {
        self.open_add_server(tx);
        self.screens.add_server.want = Some(Source::SshConfig);
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

    /// "Check before adding", and "Trust it and continue" (`trust`).
    fn add_server_test(&mut self, trust: bool) {
        let state = &mut self.screens.add_server;
        if state.testing || state.busy {
            return;
        }
        match state.form(true).check() {
            Ok(server) => {
                state.error = None;
                state.testing = true;
                state.tested = None;
                self.backend_mut().servers(if trust { Ask::TrustTest(server) } else { Ask::Test(server) });
            }
            Err(why) => state.error = Some(why),
        }
    }

    /// Opens a source's list with its servers ticked as the source says.
    pub(crate) fn add_server_list(&mut self, source: Source) {
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

    /// "SSHeesh 0.10 saved passwords for 3 servers. Use them here?": 0.10's keychain is read
    /// only on a yes.
    pub fn offer_old_passwords(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.confirm(Confirm::new(old_passwords_question(n), OLD_PASSWORDS_MEANING, "Use them"), |s, _| s.backend_mut().servers(Ask::UseOldPasswords));
    }

    /// The backend's answer to an [`Ask`], from its thread through `Remote`.
    pub fn servers_answer(&mut self, answer: Answer) {
        let state = &mut self.screens.add_server;
        match answer {
            Answer::Sources(sources) => {
                state.sources = Some(sources);
                if let Some(source) = state.want.take().filter(|_| state.open) {
                    self.add_server_list(source);
                }
            }
            Answer::LocalAuth(local) => state.local = Some(local),
            Answer::Added { server, name, warning } => {
                // "Add and connect": the dialog goes and the new server's terminal opens. A
                // half-right add (the keychain refused the password) still added it, and says.
                let picture = state.picture.take();
                *state = State::default();
                // Its picture: made into its icon now that it has a place in the list.
                if let Some(path) = picture {
                    self.backend_mut().servers(Ask::SetImage { server: server.clone(), path });
                }
                if self.bench.server(&server).is_some() {
                    self.bench.go_to(&server);
                }
                self.screens.home.settle_due = true;
                let shown = self.server_shown(&server, &name).into_owned();
                self.toast(match warning {
                    Some(warning) => Toast::info(format!("Added {shown}")).body(warning),
                    None => Toast::success(format!("Added {shown}")),
                });
            }
            Answer::Tested(tested) => {
                state.testing = false;
                let made_for = state.form(false).connection();
                state.tested = Some((tested, made_for));
            }
            Answer::Imported { note, old_passwords, .. } => {
                state.busy = false;
                state.page = Page::Form;
                state.notice = Some(note);
                if state.open {
                    self.backend_mut().servers(Ask::Sources);
                }
                self.offer_old_passwords(old_passwords);
            }
            // A server taken out of the list stops its tunnels and forgets them (0.10 did);
            // one from ~/.ssh/config is only hidden, and may come back with them.
            Answer::Removed { server } => {
                if let Some(host) = sshee_app::servers::host_id(&server).filter(|_| !server.starts_with("host:cfg:")) {
                    for ask in self.screens.tunnels.tunnels.forget(host) {
                        self.backend_mut().tunnels(ask);
                    }
                }
            }
            // The list that comes with it says the rest.
            Answer::Changed => {}
            Answer::Detected { server, icon } => self.server_detected(&server, icon.is_some()),
            Answer::Note(note) => {
                let note = self.text_shown(&note).into_owned();
                self.toast(Toast::info(note));
            }
            Answer::Refused { server, doing, why } => self.server_menu_refused(&server, &doing, &why),
            Answer::Failed(why) => {
                state.busy = false;
                state.testing = false;
                if state.open {
                    state.error = Some(why);
                } else {
                    self.toast(Toast::error("Something didn't work").body(why));
                }
            }
        }
    }
}

/// A part of the form with its name over it.
fn section(t: &Theme, name: &str, body: El) -> El {
    col().w_full().gap(8.0).child(text(name).font(theme::GROUP).color(t.l2).single_line()).child(body)
}

/// [`section`] with a "?" after its name.
fn section_with(t: &Theme, name: &str, mark: El, body: El) -> El {
    col().w_full().gap(8.0).child(row().gap(4.0).items_center().child(text(name).font(theme::GROUP).color(t.l2).single_line()).child(mark)).child(body)
}

/// The red line under the form.
fn error_line(t: &Theme, why: &str) -> El {
    row().key("add-error").w_full().gap(6.0).items_start().role("alert").child(icon(Icon::Warning, 13.0, t.red).mt(1.0)).child(text(why).font(theme::NOTE).color(t.red).grow(1.0).shrink().min_w(0.0).wrap())
}

/// One of a list of choices, with a line under it (0.10's `Choice`): a ring, filled when
/// chosen. The caller adds the key and the press.
fn radio(t: &Theme, title: &str, badge: Option<&str>, detail: &str, chosen: bool, mono: bool) -> El {
    let ring = rune::el().size(16.0, 16.0).radius(8.0).items_center().justify_center();
    let ring = if chosen { ring.fill(t.blue).child(rune::el().size(6.0, 6.0).radius(3.0).fill(t.on_color)) } else { ring.border(1.0, t.l2.alpha(0.5)) };
    let mut head = row().gap(6.0).items_center().child(text(title).font(if mono { theme::CODE } else { theme::TEXT }).color(t.l1).single_line().truncate().shrink().min_w(0.0));
    if let Some(badge) = badge {
        head = head.child(rune::el().px(5.0).radius(4.0).fill(t.hov2).child(text(badge).font(theme::SMALL).color(t.l2).single_line()));
    }
    let mut words = col().grow(1.0).shrink().min_w(0.0).gap(1.0).child(head);
    if !detail.is_empty() {
        words = words.child(text(detail).font(theme::SMALL).color(t.l2).single_line().truncate());
    }
    let b = row()
        .w_full()
        .px(10.0)
        .py(6.0)
        .gap(10.0)
        .items_center()
        .radius(theme::RADIUS_ROW)
        .border(1.0, if chosen { t.blue.alpha(0.6) } else { t.hair })
        .focusable(true)
        .role("radio")
        .checked(chosen)
        .label(if detail.is_empty() { title.to_owned() } else { format!("{title}. {detail}") })
        .child(ring)
        .child(words.a11y_hidden());
    if chosen {
        b.fill(t.blue.alpha(0.1))
    } else {
        b.hover(t.hov2).press(t.down)
    }
}

/// "Sign in with": a key (yours, one found in `~/.ssh`, or a file), a password, or a password
/// manager.
fn sign_in(s: &Shell, state: &State, t: &Theme) -> El {
    let managers = state.local.as_ref().map(|l| l.managers()).unwrap_or_default();
    let mut ways = vec![
        choice(t, "SSH key", state.sign_in == SignIn::Key).key("add-key").on_press_tx(|s: &mut Shell, _| s.screens.add_server.sign_in = SignIn::Key),
        choice(t, "Password", state.sign_in == SignIn::Password).key("add-password").on_press_tx(|s: &mut Shell, _| s.screens.add_server.sign_in = SignIn::Password),
    ];
    if !managers.is_empty() || state.sign_in == SignIn::Manager {
        ways.push(choice(t, "Password manager", state.sign_in == SignIn::Manager).key("add-manager").on_press_tx(|s: &mut Shell, _| {
            let st = &mut s.screens.add_server;
            st.sign_in = SignIn::Manager;
            // The only one there is, chosen for you.
            if st.agent.is_empty() {
                if let Some(socket) = st.local.as_ref().and_then(|l| l.managers().first().and_then(|a| a.socket.clone())) {
                    st.agent = socket;
                }
            }
        }));
    }
    let how = match state.sign_in {
        SignIn::Key => {
            let usual = state.local.as_ref().map(LocalAuth::usual_keys_note).unwrap_or_else(|| "ssh picks from the keys in ~/.ssh".into());
            let mut keys = col().key("add-keys").w_full().gap(6.0).role("radiogroup").label_static("Key");
            keys = keys.child(radio(t, "Your usual keys", None, &usual, state.key == KeyChoice::Usual, false).key("key-usual").on_press_tx(|s: &mut Shell, _| s.screens.add_server.key = KeyChoice::Usual));
            for (i, k) in state.local.iter().flat_map(|l| l.keys.iter()).enumerate() {
                let path = k.path.clone();
                let chosen = state.key == KeyChoice::Found(path.clone());
                keys = keys.child(
                    radio(t, &k.name, Some(&k.kind), LocalAuth::key_note(k), chosen, true).key(format!("key-{i}")).on_press_tx(move |s: &mut Shell, _| s.screens.add_server.key = KeyChoice::Found(path.clone())),
                );
            }
            keys = keys.child(radio(t, "Another key file", None, "", state.key == KeyChoice::Other, false).key("key-other").on_press_tx(|s: &mut Shell, _| {
                let st = &mut s.screens.add_server;
                st.key = KeyChoice::Other;
                st.key_file.focused = true;
            }));
            if state.key == KeyChoice::Other {
                let key_file = code_input(t, &state.key_file, "~/.ssh/my_key").key("add-key-file").on_edit(|s: &mut Shell| &mut s.screens.add_server.key_file);
                keys = keys.child(field(t, key_file, false));
            }
            keys
        }
        SignIn::Password => {
            let secret = text_input(&state.password, theme::TEXT)
                .secure()
                .revealed(state.show_password)
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
            let eye = icon_button(t, if state.show_password { Icon::EyeOff } else { Icon::Eye }, if state.show_password { "Hide password" } else { "Show password" }, true)
                .key("add-eye")
                .on_press_tx(|s: &mut Shell, _| s.screens.add_server.show_password ^= true);
            let kept = if state.remember { "Saved in your system keychain, never in SSHeesh's own files." } else { "Kept only until you quit SSHeesh." };
            col()
                .w_full()
                .gap(6.0)
                .child(row().w_full().gap(4.0).items_center().child(field(t, secret, false).grow(1.0).min_w(0.0)).child(eye))
                .child(check_box(t, "Remember password", state.remember).key("add-remember").on_press_tx(|s: &mut Shell, _| s.screens.add_server.remember ^= true))
                .child(text(kept).font(theme::SMALL).color(t.l2).single_line())
        }
        SignIn::Manager => {
            let mut list = col().key("add-managers").w_full().gap(6.0).role("radiogroup").label_static("Password manager");
            for (i, a) in managers.iter().enumerate() {
                let socket = a.socket.clone().unwrap_or_default();
                let chosen = state.agent == socket;
                list = list.child(radio(t, &a.name, None, &LocalAuth::manager_note(a), chosen, false).key(format!("manager-{i}")).on_press_tx(move |s: &mut Shell, _| s.screens.add_server.agent = socket.clone()));
            }
            if managers.is_empty() {
                list = list.child(text("No password manager with keys is running here.").font(theme::SMALL).color(t.l2).wrap());
            }
            list
        }
    };
    let mark = super::help::tip(s, t, "add-signin-about", "signing in", super::help::SIGN_IN);
    section_with(t, "Sign in with", mark, col().w_full().gap(8.0).items_start().child(choices(t, ways).label_static("Sign in with")).child(how))
}

/// "In your list": the icon (its picker opens in place), the colour, the label, the name.
fn in_your_list(state: &State, t: &Theme, shown_host: &str) -> El {
    let picked = state.look();
    let look = picked.clone().unwrap_or_default();
    let icon_id = if look.icon.is_empty() { "server" } else { look.icon.as_str() };
    let ink = rune::color::hex(&look.color).unwrap_or(t.l2);
    let button = rune::el()
        .key("add-look")
        .size(theme::FIELD, theme::FIELD)
        .radius(theme::RADIUS_ROW)
        .items_center()
        .justify_center()
        .border(1.0, if state.icons_open { t.blue.alpha(0.6) } else { t.hair })
        .fill(t.field())
        .hover(t.hov2)
        .focusable(true)
        .role("button")
        .label(format!("Icon: {}. Change", icon_name(icon_id)))
        .help("Icon")
        .on_press_tx(|s: &mut Shell, _| s.screens.add_server.icons_open ^= true)
        .child(icon(Icon::for_server(icon_id), 17.0, if picked.is_some() { ink } else { t.l2 }));
    let name = input(t, &state.name, if shown_host.is_empty() { "Name" } else { shown_host }).key("add-name").on_edit(|s: &mut Shell| &mut s.screens.add_server.name);
    let mut body = col().w_full().gap(8.0).child(row().w_full().gap(8.0).items_center().child(button).child(field(t, name, false).grow(1.0).min_w(0.0)));
    if state.icons_open {
        let mut icons = row().key("add-icons").w_full().flex_wrap().gap(4.0).padding(6.0).radius(theme::RADIUS).fill(t.hov2).role("radiogroup").label_static("Icon");
        for (id, name) in HOST_ICONS {
            let chosen = state.look.as_ref().is_some_and(|l| l.icon == id);
            let mut b = rune::el()
                .key(format!("look-{id}"))
                .size(32.0, 32.0)
                .radius(theme::RADIUS)
                .items_center()
                .justify_center()
                .focusable(true)
                .role("radio")
                .checked(chosen)
                .label_static(name)
                .help(name)
                .on_press_tx(move |s: &mut Shell, _| {
                    let st = &mut s.screens.add_server;
                    // An icon alone picks no colour: it stays quiet.
                    let color = st.look.as_ref().map(|l| l.color.clone()).unwrap_or_default();
                    st.look = Some(PickedLook { icon: id.to_owned(), color, image: None });
                    st.icons_open = false;
                })
                .child(icon(Icon::for_server(id), theme::ICON_RAIL, ink));
            b = if chosen { b.fill(t.selrail) } else { b.hover(t.down) };
            icons = icons.child(b);
        }
        body = body.child(icons);
    }
    let mut colors = row().gap(6.0).items_center().role("radiogroup").label_static("Icon color");
    for (i, hex) in HOST_COLORS.into_iter().enumerate() {
        let chosen = picked.as_ref().is_some_and(|l| l.color.eq_ignore_ascii_case(hex));
        let mut b = rune::el()
            .key(format!("look-color-{i}"))
            .size(22.0, 22.0)
            .radius(11.0)
            .items_center()
            .justify_center()
            .focusable(true)
            .role("radio")
            .checked(chosen)
            .label(format!("Color {}", i + 1))
            .on_press_tx(move |s: &mut Shell, _| {
                let st = &mut s.screens.add_server;
                let icon = st.look.as_ref().map(|l| l.icon.clone()).filter(|i| !i.is_empty()).unwrap_or_else(|| "server".into());
                st.look = Some(PickedLook { icon, color: hex.to_owned(), image: None });
                // A swatch picked: the colour typed before steps aside.
                st.hex = TextEdit::default();
            })
            .child(rune::el().size(16.0, 16.0).radius(8.0).fill(rune::color::hex(hex).unwrap_or(t.l2)));
        if chosen {
            b = b.border(1.5, t.l1);
        }
        colors = colors.child(b);
    }
    let labels = choices(
        t,
        env_order().into_iter().map(|env| {
            let chosen = state.env.as_ref() == Some(&env);
            let key = format!("look-env-{}", env.as_str());
            let name = env_name(&env).to_owned();
            choice(t, &name, chosen).key(key).on_press_tx(move |s: &mut Shell, _| {
                let st = &mut s.screens.add_server;
                // A click on the label it has takes it off.
                st.env = if st.env.as_ref() == Some(&env) { None } else { Some(env.clone()) };
            })
        }),
    )
    .label_static("Label");
    // Any other colour, typed: it shows on the icon as soon as it reads as one.
    let typed_bad = !state.hex.text.trim().is_empty() && parse_color(&state.hex.text).is_none();
    let hex = code_input(t, &state.hex, "#2389e2").key("look-hex").label_static("Your own color").on_edit(|s: &mut Shell| &mut s.screens.add_server.hex);
    colors = colors.child(field(t, hex, typed_bad).w(92.0).ml(4.0));
    body = body.child(row().w_full().gap(12.0).items_center().flex_wrap().child(colors).child(rune::spacer()).child(labels));
    // A picture of your own in place of the icon.
    let mut picture = row().w_full().gap(8.0).items_center();
    match &state.picture {
        Some(path) => {
            let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            picture = picture
                .child(icon(Icon::Image, 15.0, t.l2))
                .child(text(file).key("add-picture-name").font(theme::NOTE).color(t.l1).single_line().truncate().shrink().min_w(0.0))
                .child(push_button(t, "Pick another…", Push::Quiet, true).key("add-picture").on_press_tx(|_: &mut Shell, tx| pick_picture(tx)))
                .child(push_button(t, "Remove", Push::Quiet, true).key("add-no-picture").on_press_tx(|s: &mut Shell, _| s.screens.add_server.picture = None));
        }
        None => picture = picture.child(push_button(t, "Use a picture…", Push::Quiet, true).key("add-picture").help("A picture of your own in place of the icon").on_press_tx(|_: &mut Shell, tx| pick_picture(tx))),
    }
    body = body.child(picture);
    section(t, "In your list", body)
}

/// "Copy": the command to the clipboard, and a toast that says so.
fn copy_command(t: &Theme, command: &str) -> El {
    let words = command.to_owned();
    push_button(t, "Copy", Push::Quiet, true).key("step-copy").label(format!("Copy {command}")).on_press_tx(move |s: &mut Shell, tx| {
        tx.copy(words.clone());
        s.toast(Toast::info("Copied").body(words.clone()));
    })
}

/// "Use a picture…": the system's picker; the file is made into the icon once the server is
/// added.
fn pick_picture(tx: &mut Tx) {
    let options = FileDialogOptions { title: "Choose a picture".into(), ..Default::default() }.filter("Pictures", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "ico"]);
    tx.open_file(options, |s: &mut Shell, result, _| match result {
        Ok(Some(path)) => s.screens.add_server.picture = Some(path),
        Ok(None) => {}
        Err(e) => s.screens.add_server.error = Some(format!("Couldn't open the file picker. {e}")),
    });
}

/// "Check before adding" and what it found (0.10's `TestResult`).
fn check(s: &Shell, state: &State, t: &Theme, ready: bool) -> El {
    let label = if state.testing { "Checking…" } else { "Check before adding" };
    let mut out = col().w_full().gap(8.0).items_start().child(push_button(t, label, Push::Quiet, ready && !state.testing && !state.busy).key("add-test").on_press_tx(|s: &mut Shell, _| s.add_server_test(false)));
    let Some(tested) = state.check_shown() else { return out };
    let mut steps = col().key("add-tested").w_full().gap(6.0);
    for (i, step) in tested.steps().into_iter().enumerate() {
        let (mark, color) = match step.tone {
            StepTone::Ok => (Icon::Check, t.green),
            StepTone::Bad => (Icon::Warning, t.red),
            StepTone::Warn => (Icon::Warning, t.orange_dot),
            StepTone::Ask => (Icon::Shield, t.blue),
        };
        let mut words = col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(text(&step.title).font(theme::TEXT).color(t.l1).wrap());
        if let Some(detail) = &step.detail {
            words = words.child(text(detail).font(theme::SMALL).color(t.l2).wrap());
        }
        // The command that fixes it, to copy (0.10's install line for tmux).
        if let Some(command) = &step.command {
            words = words.child(
                row()
                    .key("step-command")
                    .w_full()
                    .gap(6.0)
                    .items_center()
                    .child(rune::el().px(8.0).py(4.0).radius(theme::RADIUS_ROW).fill(t.hov2).grow(1.0).min_w(0.0).child(text(command).font(theme::CODE).color(t.l1).wrap()))
                    .child(copy_command(t, command)),
            );
        }
        if step.tone == StepTone::Ask {
            for (kind, sha) in tested.fingerprints() {
                words = words.child(rune::el().w_full().px(8.0).py(5.0).radius(theme::RADIUS_ROW).fill(t.hov2).child(text(format!("{kind} {sha}")).font(theme::CODE).color(t.l1).wrap()));
            }
            let trust = push_button(t, "Trust it and continue", Push::Primary, !state.testing).key("add-trust").on_press_tx(|s: &mut Shell, _| s.add_server_test(true));
            words = words.child(row().gap(6.0).items_center().child(trust).child(super::help::tip(s, t, "add-print-about", "the server's fingerprint", super::help::FINGERPRINT)));
        }
        steps = steps.child(row().key(format!("test-step-{i}")).w_full().gap(8.0).items_start().child(icon(mark, 14.0, color).mt(2.0)).child(words));
    }
    out = out.child(steps);
    out
}

fn form_page(s: &Shell, state: &State, t: &Theme) -> El {
    let form = state.form(false);
    let mut page = col().w_full().px(20.0).pt(4.0).pb(16.0).gap(16.0);

    // Where servers can be brought in from, once the backend has looked.
    if let Some(sources) = &state.sources {
        let offers = sources.offers();
        if !offers.is_empty() {
            let mut rows = col().key("add-sources").w_full().mx(-8.0);
            for (source, count) in offers {
                let key = match source {
                    Source::Old => "add-from-old".to_owned(),
                    Source::SshConfig => "add-from-config".to_owned(),
                    Source::Xpipe => "add-from-xpipe".to_owned(),
                    Source::App(app) => format!("add-from-{}", app.key()),
                };
                let spec = ListRow { icon: Some(Icon::Plus), label: source.label(), note: &count, ..ListRow::default() };
                rows = rows.child(list_row(t, &spec).key(key).hover(t.hov2).focusable(true).role("button").on_press_tx(move |s: &mut Shell, _| s.add_server_list(source)));
            }
            page = page.child(rows);
        }
        if sources.old_passwords > 0 {
            let n = sources.old_passwords;
            let spec = ListRow { icon: Some(Icon::Key), label: "Use 0.10's saved passwords", note: &format!("{n}"), ..ListRow::default() };
            page = page.child(list_row(t, &spec).key("add-old-passwords").mx(-8.0).hover(t.hov2).focusable(true).role("button").on_press_tx(move |s: &mut Shell, _| s.offer_old_passwords(n)));
        }
    }
    if let Some(notice) = &state.notice {
        let notice = s.text_shown(notice);
        page = page.child(row().key("add-notice").w_full().gap(6.0).items_start().child(icon(Icon::Check, 13.0, t.l2on).mt(1.0)).child(text(notice.as_ref()).font(theme::NOTE).color(t.l2on).grow(1.0).shrink().min_w(0.0).wrap()));
    }

    // Where.
    let address = input(t, &state.address, "203.0.113.10").key("add-address").on_edit(|s: &mut Shell| &mut s.screens.add_server.address);
    // A pasted `ssh root@host -p 2222` sets the port and the user: their fields say what it
    // read, in the placeholder's grey, instead of the usual 22 and root.
    let read = form.parsed();
    let port = input(t, &state.port, read.port.as_deref().unwrap_or("22")).key("add-port").on_edit(|s: &mut Shell| &mut s.screens.add_server.port);
    let user = input(t, &state.user, read.user.as_deref().unwrap_or("root")).key("add-user").on_edit(|s: &mut Shell| &mut s.screens.add_server.user);
    let bad_address = state.error.as_deref().is_some_and(|e| e == sshee_app::servers::NO_ADDRESS || e == sshee_app::servers::BAD_ADDRESS);
    let bad_port = state.error.as_deref() == Some(sshee_app::servers::BAD_PORT);
    let mut place = col().w_full().gap(8.0).child(
        row()
            .w_full()
            .gap(8.0)
            .child(labelled(t, "Address", field(t, address, bad_address)).grow(1.0).min_w(0.0))
            .child(
                col()
                    .gap(4.0)
                    .w(76.0)
                    .child(row().gap(2.0).items_center().child(text("Port").font(theme::SMALL).color(t.l2).single_line().a11y_hidden()).child(super::help::tip(s, t, "add-port-about", "the port", super::help::PORT).my(-3.0)))
                    .child(field(t, port, bad_port)),
            )
            .child(labelled(t, "User", field(t, user, false)).w(120.0)),
    );
    // A pasted ssh command, read back: what the box means. Until then, that one can be pasted.
    let mut read_back = false;
    if let Ok(reads) = form.check() {
        if reads.address != state.address.text.trim() && state.user.text.is_empty() && state.port.text.is_empty() {
            let key = reads.identity_file.as_deref().map(|k| format!(", key {k}")).unwrap_or_default();
            place = place.child(text(format!("Reads as {}{key}", reads.address)).key("add-reads").font(theme::SMALL).color(t.l2).single_line().truncate());
            read_back = true;
        }
    }
    if !read_back && state.address.text.trim().is_empty() {
        place = place.child(text("Or paste a whole ssh command, like ssh root@host -p 2222.").key("add-paste-hint").font(theme::SMALL).color(t.l2).wrap());
    }
    page = page.child(section(t, "Where", place));
    page = page.child(sign_in(s, state, t));
    let shown_host = form.parsed().host;
    page = page.child(in_your_list(state, t, shown_host.trim()));
    page = page.child(check(s, state, t, form.ready()));

    page
}

fn list_page(state: &State, t: &Theme, source: Source, streaming: bool) -> El {
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
        // Streamer mode: neither the name nor the address, only dots where the name is.
        let (name, note) = match streaming {
            true => ("•".repeat(c.name.chars().count().clamp(4, 12)), ""),
            false => (c.name.clone(), if c.address == c.name { "" } else { c.address.as_str() }),
        };
        let spec = ListRow { label: &name, note, checked: Some(ticked), ..ListRow::default() };
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
/// The dialog, when it's open: a last child of the window's root. Its body scrolls between the
/// title and the buttons, so it fits a low window (600 points) whole.
pub fn view(s: &Shell, t: &'static Theme, cx: &mut rune::Cx<Shell>) -> Option<El> {
    // As tall as the window lets it be, with a margin; the body scrolls in what is left after
    // the title, the error line and the buttons.
    let room = cx.layout_size("window").map_or(crate::shell::SIZE.1, |[_, h]| h);
    let dialog_max = (room - 48.0).clamp(240.0, 720.0);
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
            let head = text("Add a server").font(theme::TITLE).color(t.l1).single_line();
            let body_max = (dialog_max - 56.0 - 68.0 - if state.error.is_some() { 40.0 } else { 0.0 }).max(80.0);
            let page = col().w_full().child(rune::el().w_full().px(20.0).pt(18.0).pb(12.0).child(head)).child(
                col().key("add-body").w_full().max_h(body_max).scroll(ScrollSpec { stick_bottom: false }).child(form_page(s, state, t)),
            );
            // What is wrong stays in sight, over the buttons, however far the body scrolled.
            let page = match &state.error {
                Some(why) => page.child(rune::el().w_full().px(20.0).pt(10.0).child(error_line(t, why))),
                None => page,
            };
            (page, save)
        }
        Page::List(source) => {
            let label = if state.busy { "Adding…".to_owned() } else { import_button(source, state.picks.len()) };
            let mut import = push_button(t, &label, Push::Primary, !state.busy).key("add-import");
            if !state.busy {
                import = import.default_button().on_press_tx(|s: &mut Shell, _| s.add_server_import());
            }
            (list_page(state, t, source, s.streaming()), import)
        }
    };
    let foot = row().w_full().h(54.0).px(20.0).mt(14.0).gap(8.0).items_center().border_sides([1.0, 0.0, 0.0, 0.0], t.hair).child(rune::spacer()).child(cancel).child(done);
    // As tall as the window lets it be, with a margin.
    let dialog = popover(t, WIDTH)
        .key("add-dialog")
        .label_static("Add a server")
        .max_h(dialog_max)
        .modal_tx(false, |s: &mut Shell, tx| {
            // Esc leaves the dialog, unless the backend is in the middle of saving.
            if !s.screens.add_server.busy {
                s.close_add_server(tx);
            }
        })
        .child(page)
        .child(foot);
    let dialog = crate::kit::motion::enters(dialog, crate::kit::motion::Entrance::Card);
    Some(crate::kit::motion::enters(rune::el().key("add-scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(t.scrim()).items_center().justify_center().child(dialog), crate::kit::motion::Entrance::Scrim))
}
