//! Snippets (0.10's `Snippets.tsx`): the list, the editor, and the small form that asks for a
//! snippet's blanks before it goes into the terminal of the server on screen. A snippet can be
//! kept for one server; then it's offered only there, here and in the palette.
//!
//! ```text
//! snip-scrim   dims the window; modal (Esc closes)
//! └── snippets
//!     list     snip-<i>: snip-run-<i>, snip-edit-<i>, snip-delete-<i> · snip-new · snip-close
//!     edit     snip-name, snip-command, snip-scope-all / snip-scope-here · snip-cancel, snip-save
//!     fill     blank-<name> … · snip-preview · snip-paste, snip-go
//!     a Production server: snip-ask (snip-no, snip-yes) before anything runs with Return
//! ```
//!
//! The rules (blanks, filling them, which server a snippet is for) are `sshee_app::snippets`.
//! The list is kept by the backend (`Backend::snippets`, `Backend::snippets_changed`): the
//! program's in its own folder, a demo's and a test's in memory. Running types the command in
//! as a paste and presses Return ([`Shell::run_on`]); "Paste" leaves Return to you.

use std::collections::HashMap;

use rune::{col, el, row, spacer, text, text_input, El, TextEdit, Tx};
use sshee_app::model::{Snippet, Snippets};
use sshee_app::snippets::{self, blanks, complete, fill, only_on};

use super::checkup::{body, close_button, command_block, dialog, header, note};
use crate::icons::Icon;
use crate::kit::chip::chip;
use crate::kit::field::{code_input, field, input, labelled};
use crate::kit::push::{choice, choices, icon_button, push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

pub const WIDTH: f32 = 540.0;

/// Which page is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    List,
    /// A new snippet (`None`) or one to change.
    Edit(Option<String>),
    /// A snippet's blanks, before it goes to `server`.
    Fill { id: String, server: String },
}

pub struct State {
    pub page: Option<Page>,
    /// Every snippet, as the backend keeps them.
    pub list: Snippets,
    pub name: TextEdit,
    pub command: TextEdit,
    /// The snippet being edited is kept for the server on screen.
    pub only_here: bool,
    pub error: Option<&'static str>,
    /// The blanks being filled, by name.
    pub blanks: HashMap<String, TextEdit>,
    /// Asked before running on a Production server: the command and where.
    pub asking: Option<(String, String)>,
    /// Why a run did nothing.
    pub note: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State { page: None, list: Snippets::default(), name: TextEdit::default(), command: TextEdit::default(), only_here: false, error: None, blanks: HashMap::new(), asking: None, note: None }
    }
}

impl State {
    pub fn is_open(&self) -> bool {
        self.page.is_some()
    }

    /// Closes it; the list stays.
    pub fn close(&mut self) {
        self.page = None;
        self.asking = None;
        self.note = None;
        self.error = None;
        self.blanks.clear();
    }
}

/// The list kept by the backend, read once when the window starts.
pub fn start(s: &mut Shell) {
    if let Some(list) = s.backend_mut().snippets() {
        s.screens.snippets.list = list;
    }
}

/// The snippets offered on the machine on screen (on the main menu: those for every server).
pub fn offered(s: &Shell) -> Vec<Snippet> {
    let server = s.bench.current_key();
    snippets::for_server(&s.screens.snippets.list.snippets, server).into_iter().cloned().collect()
}

impl Shell {
    pub fn open_snippets(&mut self, tx: &mut Tx) {
        self.screens.close_all();
        self.screens.snippets.page = Some(Page::List);
        tx.instant();
    }

    pub fn close_snippets(&mut self, tx: &mut Tx) {
        self.screens.snippets.close();
        self.settle(tx);
    }

    /// "New snippet", with `command` in it (the palette's typed command).
    pub fn new_snippet(&mut self, command: &str, tx: &mut Tx) {
        self.screens.close_all();
        self.snippet_edit(None);
        self.screens.snippets.command = TextEdit::new(command);
        tx.instant();
    }

    fn snippet_edit(&mut self, id: Option<String>) {
        let state = &mut self.screens.snippets;
        let found = id.as_deref().and_then(|id| state.list.snippets.iter().find(|s| s.id == id)).cloned();
        state.name = TextEdit::new(found.as_ref().map_or("", |s| s.name.as_str()));
        state.name.focused = true;
        state.command = TextEdit::new(found.as_ref().map_or("", |s| s.command.as_str()));
        state.only_here = found.as_ref().and_then(only_on).is_some();
        state.error = None;
        state.page = Some(Page::Edit(found.map(|s| s.id)));
    }

    fn snippet_save(&mut self) {
        let here = self.bench.current_key().map(str::to_owned);
        let state = &mut self.screens.snippets;
        let Some(Page::Edit(id)) = state.page.clone() else { return };
        if let Some(why) = snippets::check(&state.name.text, &state.command.text) {
            state.error = Some(why);
            return;
        }
        let only = if state.only_here { here } else { None };
        let (name, command) = (state.name.text.clone(), state.command.text.clone());
        snippets::save(&mut state.list, id.as_deref(), &name, &command, only.as_deref());
        state.page = Some(Page::List);
        self.snippets_changed();
    }

    fn snippet_delete(&mut self, id: &str) {
        snippets::remove(&mut self.screens.snippets.list, id);
        self.snippets_changed();
    }

    fn snippets_changed(&mut self) {
        let list = self.screens.snippets.list.clone();
        self.backend_mut().snippets_changed(&list);
    }

    /// The snippets the backend had kept from 0.10, at the first start: in place of the
    /// starters, unless some were saved here already.
    pub fn seed_snippets(&mut self, list: Snippets) {
        if self.screens.snippets.list == Snippets::default() {
            self.screens.snippets.list = list;
            self.snippets_changed();
        }
    }

    /// Runs a snippet on the machine on screen: its blanks asked first.
    fn snippet_run(&mut self, id: &str, tx: &mut Tx) {
        let Some(snippet) = self.screens.snippets.list.snippets.iter().find(|s| s.id == id).cloned() else { return };
        let Some(server) = self.bench.current_key().map(str::to_owned) else {
            self.screens.snippets.note = Some("Pick a server first: snippets run in the terminal of the server you're looking at.".into());
            return;
        };
        if !blanks(&snippet.command).is_empty() {
            let state = &mut self.screens.snippets;
            state.blanks = blanks(&snippet.command).into_iter().map(|b| (b.name, TextEdit::default())).collect();
            if let Some(first) = blanks(&snippet.command).first().and_then(|b| state.blanks.get_mut(&b.name)) {
                first.focused = true;
            }
            state.page = Some(Page::Fill { id: snippet.id, server });
            return;
        }
        self.snippet_send(&server, &snippet.command, true, tx);
    }

    /// Into the terminal: asked first on Production when it would run.
    fn snippet_send(&mut self, server: &str, command: &str, enter: bool, tx: &mut Tx) {
        let production = self.bench.server(server).is_some_and(|s| s.is_prod());
        if enter && production && self.screens.snippets.asking.is_none() {
            self.screens.snippets.asking = Some((server.to_owned(), command.to_owned()));
            return;
        }
        self.screens.snippets.close();
        self.run_on(&server.to_owned(), command, enter, tx);
    }

    fn snippet_fill_go(&mut self, enter: bool, tx: &mut Tx) {
        let state = &self.screens.snippets;
        let Some(Page::Fill { id, server }) = state.page.clone() else { return };
        let Some(snippet) = state.list.snippets.iter().find(|s| s.id == id) else { return };
        let values = |name: &str| state.blanks.get(name).map(|e| e.text.clone());
        if !complete(&snippet.command, &values) {
            return;
        }
        let command = fill(&snippet.command, &values);
        self.snippet_send(&server, &command, enter, tx);
    }

    /// The Production question's yes.
    fn snippet_yes(&mut self, tx: &mut Tx) {
        if let Some((server, command)) = self.screens.snippets.asking.clone() {
            self.screens.snippets.close();
            self.run_on(&server, &command, true, tx);
        }
    }
}

fn server_name(s: &Shell, key: &str) -> String {
    s.bench.server(key).map(|sv| s.server_shown(key, &sv.name).into_owned()).unwrap_or_default()
}

/// The question before a command runs with Return on a Production server.
fn production_question(t: &Theme, s: &Shell) -> Option<El> {
    let (server, _) = s.screens.snippets.asking.as_ref()?;
    let name = server_name(s, server);
    Some(
        row()
            .key("snip-ask")
            .w_full()
            .px(12.0)
            .py(10.0)
            .gap(10.0)
            .items_center()
            .radius(theme::RADIUS_ROW)
            .fill(t.red.alpha(0.08))
            .border(1.0, t.red.alpha(0.35))
            .role("alertdialog")
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(format!("Run it on {name}?")).font(theme::TEXT_BOLD).color(t.red).wrap())
                    .child(text("It's a Production server, and Return runs the command at once.").font(theme::NOTE).color(t.l2on).wrap()),
            )
            .child(push_button(t, "Cancel", Push::Quiet, true).key("snip-no").on_press_tx(|s: &mut Shell, _| s.screens.snippets.asking = None))
            .child(push_button(t, "Run on Production", Push::Destructive, true).key("snip-yes").on_press_tx(|s: &mut Shell, tx| s.snippet_yes(tx))),
    )
}

fn list_page(t: &Theme, s: &Shell) -> El {
    let state = &s.screens.snippets;
    let here = s.bench.current_key();
    let on = here.map(|k| server_name(s, k));
    let sub = match &on {
        Some(name) => format!("Run in the terminal of {name}. In the palette too: type cmd:"),
        None => "Commands you keep. Open a server to run one there.".to_owned(),
    };
    let mut page = body("snip-body").child(header(t, Icon::Terminal, "Snippets", &sub, vec![close_button(t, "snip-close").on_press_tx(|s: &mut Shell, tx| s.close_snippets(tx))]));
    let mut list = col().key("snip-list").w_full().gap(2.0).mx(-8.0);
    let shown = snippets::for_server(&state.list.snippets, here);
    for (i, sn) in shown.iter().enumerate() {
        let (run, edit, del) = (sn.id.clone(), sn.id.clone(), sn.id.clone());
        let mut words = col().grow(1.0).shrink().min_w(0.0).gap(1.0);
        let mut title = row().gap(6.0).items_center().child(text(sn.name.as_str()).font(theme::TAB_NAME).color(t.l1).single_line().truncate().shrink());
        if only_on(sn).is_some() {
            title = title.child(chip(t, "This server", t.l2on));
        }
        words = words.child(title).child(text(s.text_shown(&sn.command).into_owned()).font(theme::CODE).color(t.l2).single_line().truncate());
        list = list.child(
            row()
                .key(format!("snip-{i}"))
                .w_full()
                .px(8.0)
                .py(6.0)
                .gap(6.0)
                .items_center()
                .radius(theme::RADIUS_ROW)
                .hover(t.hov2)
                .child(words)
                .child(icon_button(t, Icon::Pencil, "Edit", true).key(format!("snip-edit-{i}")).on_press_tx(move |s: &mut Shell, _| s.snippet_edit(Some(edit.clone()))))
                .child(icon_button(t, Icon::Bin, "Delete", true).key(format!("snip-delete-{i}")).on_press_tx(move |s: &mut Shell, _| s.snippet_delete(&del)))
                .child(push_button(t, "Run", Push::Quiet, here.is_some()).key(format!("snip-run-{i}")).on_press_tx(move |s: &mut Shell, tx| s.snippet_run(&run, tx))),
        );
    }
    if shown.is_empty() {
        list = list.child(row().w_full().py(20.0).justify_center().child(text("No snippets yet.").font(theme::NOTE).color(t.l2).single_line()));
    }
    page = page.child(list);
    if let Some(ask) = production_question(t, s) {
        page = page.child(ask);
    }
    if let Some(why) = &state.note {
        page = page.child(note(t, why, false).key("snip-note"));
    }
    page.child(row().w_full().child(spacer()).child(push_button(t, "New snippet", Push::Primary, true).key("snip-new").on_press_tx(|s: &mut Shell, _| s.snippet_edit(None))))
}

fn edit_page(t: &Theme, s: &Shell, id: Option<&str>) -> El {
    let state = &s.screens.snippets;
    let found = blanks(&state.command.text);
    let ready = snippets::check(&state.name.text, &state.command.text).is_none();
    let mut page = body("snip-body").child(header(
        t,
        Icon::Pencil,
        if id.is_some() { "Edit snippet" } else { "New snippet" },
        "Leave blanks to fill in each time it runs",
        vec![close_button(t, "snip-close").on_press_tx(|s: &mut Shell, tx| s.close_snippets(tx))],
    ));
    let name = input(t, &state.name, "Restart the web app").key("snip-name").on_edit(|s: &mut Shell| &mut s.screens.snippets.name);
    page = page.child(labelled(t, "Name", field(t, name, state.error == Some("Give it a name."))));
    let command = text_input(&state.command, theme::CODE)
        .key("snip-command")
        .w_full()
        .min_h(54.0)
        .color(t.l1)
        .placeholder("docker compose restart {{service}}", t.l2)
        .caret(t.ring, t.ring.alpha(0.35))
        .label_static("Command")
        .on_edit(|s: &mut Shell| &mut s.screens.snippets.command);
    let command_box = el()
        .w_full()
        .px(8.0)
        .py(6.0)
        .radius(theme::RADIUS_ROW)
        .fill(t.field())
        .border(1.0, if state.error == Some("Write the command.") { t.red } else { t.hair })
        .focus_within_ring(theme::RING, 0.0, t.ring)
        .cursor(rune::Cursor::Text)
        .child(command);
    page = page.child(labelled(t, "Command", command_box));
    let mut hint = String::from("Leave a blank as {{name}}, or {{name:default}}, and SSHeesh asks for it each time.");
    if !found.is_empty() {
        let names: Vec<String> = found.iter().map(|b| b.name.clone()).collect();
        hint.push_str(&format!(" Blanks here: {}.", names.join(", ")));
    }
    page = page.child(text(hint).key("snip-hint").font(theme::SMALL).color(t.l2).wrap());
    if let Some(here) = s.bench.current_key() {
        let name = server_name(s, here);
        let only = state.only_here;
        page = page.child(labelled(
            t,
            "Offer it on",
            choices(
                t,
                [
                    choice(t, "Every server", !only).key("snip-scope-all").on_press_tx(|s: &mut Shell, _| s.screens.snippets.only_here = false),
                    choice(t, &format!("Only {name}"), only).key("snip-scope-here").on_press_tx(|s: &mut Shell, _| s.screens.snippets.only_here = true),
                ],
            )
            .label_static("Offer it on"),
        ));
    }
    if let Some(why) = state.error {
        page = page.child(text(why).key("snip-error").font(theme::NOTE).color(t.red).single_line().role("alert"));
    }
    let mut save = push_button(t, "Save", Push::Primary, ready).key("snip-save");
    if ready {
        save = save.on_press_tx(|s: &mut Shell, _| s.snippet_save());
    }
    page.child(
        row()
            .w_full()
            .gap(8.0)
            .child(spacer())
            .child(push_button(t, "Cancel", Push::Quiet, true).key("snip-cancel").on_press_tx(|s: &mut Shell, _| s.screens.snippets.page = Some(Page::List)))
            .child(save),
    )
}

fn fill_page(t: &Theme, s: &Shell, id: &str, server: &str) -> El {
    let state = &s.screens.snippets;
    let Some(sn) = state.list.snippets.iter().find(|x| x.id == id) else { return body("snip-body") };
    let name = server_name(s, server);
    let prod = s.bench.server(server).is_some_and(|sv| sv.is_prod());
    let mut page = body("snip-body").child(header(t, Icon::Terminal, &sn.name, &format!("Into the terminal of {name}"), vec![close_button(t, "snip-close").on_press_tx(|s: &mut Shell, tx| s.close_snippets(tx))]));
    if prod {
        page = page.child(row().gap(6.0).items_center().child(chip(t, "Prod", t.red)).child(text("Run asks once more before it goes in with Return.").font(theme::SMALL).color(t.l2).single_line()));
    }
    for b in blanks(&sn.command) {
        let blank = b.name.clone();
        let edit = state.blanks.get(&b.name).cloned().unwrap_or_default();
        let hint = if b.wants_container() { "A container's name".to_owned() } else { b.fallback.clone() };
        let entry = code_input(t, &edit, &hint).key(format!("blank-{}", b.name)).on_edit(move |s: &mut Shell| s.screens.snippets.blanks.entry(blank.clone()).or_default());
        let label = if b.fallback.is_empty() { b.label() } else { format!("{} · {} if left empty", b.label(), b.fallback) };
        page = page.child(labelled(t, &label, field(t, entry, false)));
    }
    let values = |n: &str| state.blanks.get(n).map(|e| e.text.clone());
    let ready = complete(&sn.command, &values);
    page = page.child(command_block(t, &s.text_shown(&fill(&sn.command, &values))).key("snip-preview"));
    if let Some(ask) = production_question(t, s) {
        page = page.child(ask);
    }
    let mut go = push_button(t, "Run", Push::Primary, ready && state.asking.is_none()).key("snip-go");
    if ready && state.asking.is_none() {
        go = go.default_button().on_press_tx(|s: &mut Shell, tx| s.snippet_fill_go(true, tx));
    }
    page.child(
        row()
            .w_full()
            .gap(8.0)
            .child(spacer())
            .child(push_button(t, "Paste", Push::Quiet, ready).key("snip-paste").help("Type it into the terminal without pressing Return").on_press_tx(|s: &mut Shell, tx| s.snippet_fill_go(false, tx)))
            .child(go),
    )
}

/// The dialog, when it's open.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let page = match s.screens.snippets.page.as_ref()? {
        Page::List => list_page(t, s),
        Page::Edit(id) => edit_page(t, s, id.as_deref()),
        Page::Fill { id, server } => fill_page(t, s, id, server),
    };
    Some(dialog(t, "snip-scrim", "snippets", "Snippets", WIDTH, page, |s: &mut Shell, tx| s.close_snippets(tx)))
}
