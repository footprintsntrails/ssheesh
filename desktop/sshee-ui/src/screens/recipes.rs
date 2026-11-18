//! Recipes (0.10's `Recipes.tsx`): step-by-step guides for common server jobs. Each step says
//! what it does and why, then offers its command to paste into the server's terminal (without
//! pressing Return) or copy. After a paste the dialog steps aside into a small bar at the top
//! right, so you can read the command, run it and come back to the same step.
//!
//! ```text
//! recipes-scrim   dims the window; modal (Esc closes)
//! └── recipes     recipes-close · os-<i> (the server's system)
//!     list        recipe-row-<i>
//!     a recipe    recipes-back · field-<id> (key-<i>, key-paste) · step-<i>: tick-<i>,
//!                 cmd-<i>, paste-<i>, copy-<i>, fails-<i> (paste-fix-<i>) · recipes-over
//! recipe-aside    stepped aside: aside-back, aside-close
//! ```
//!
//! The recipes and their rules are `sshee_app::recipes` (`book`); what a recipe needs to know
//! about the server (its kernel, how it's reached) and this computer (its keys) comes from
//! the backend (`Ask::Facts`, `Ask::ReadKey`), through [`deliver`]. Ticks and what was typed
//! are kept until the program quits, per server and recipe.

use std::collections::{BTreeMap, HashMap, HashSet};

use rune::{col, el, row, spacer, text, El, TextEdit, Tx};
use sshee_app::recipes::{self, book, done_in, field_error, field_values, fill_in_first, guess_os, missing, parse_public_key, progress, Ask, Ctx, Facts, FieldKind, Group, Os, Recipe, Reply, Step};

use super::checkup::{body, close_button, command_block, dialog, header, headed, note};
use crate::icons::{icon, Icon};
use crate::kit::field::{code_input, field};
use crate::kit::push::{choice, choices, icon_button, push_button, Push};
use crate::shell::Shell;
use crate::theme::{self, Theme};

pub const WIDTH: f32 = 620.0;

/// A recipe for a server.
type Key = (String, &'static str);

#[derive(Default)]
pub struct State {
    /// Open for this server.
    pub open: Option<String>,
    /// The recipe on screen; `None` for the list.
    pub recipe: Option<&'static str>,
    /// Stepped aside after a paste: the bar brings the steps back.
    pub aside: bool,
    /// Ticked-off step titles.
    pub done: HashMap<Key, Vec<String>>,
    /// What was typed into a recipe's fields, by field id (`keyFile`, `_keyChoice` and
    /// `_keyPaste` are the key field's own).
    pub typed: HashMap<Key, BTreeMap<String, String>>,
    /// The recipe on screen's text fields.
    pub edits: HashMap<&'static str, TextEdit>,
    /// The system picked by hand, by server.
    pub os: HashMap<String, Os>,
    pub facts: HashMap<String, Facts>,
    /// Steps opened again after they were ticked, and "If it didn't work" opened, by title.
    pub expanded: HashSet<String>,
    pub failing: HashSet<String>,
    /// A key being read, and why one couldn't be.
    pub reading: Option<String>,
    pub key_error: Option<String>,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open.is_some() && !self.aside
    }

    /// Closes it, keeping ticks and what was typed for the next time.
    pub(crate) fn close(&mut self) {
        self.open = None;
        self.recipe = None;
        self.aside = false;
        self.edits.clear();
    }

    fn key(&self) -> Option<Key> {
        Some((self.open.clone()?, self.recipe?))
    }

    /// The server's system: picked, else guessed from its kernel, else Debian.
    fn os_of(&self, server: &str) -> (Os, bool) {
        if let Some(os) = self.os.get(server) {
            return (*os, false);
        }
        match self.facts.get(server).and_then(|f| f.kernel.as_deref()).and_then(guess_os) {
            Some(os) => (os, true),
            None => (Os::Debian, false),
        }
    }
}

impl Shell {
    /// Opens the recipes for `server`: the list, or one recipe.
    pub fn open_recipes(&mut self, server: &str, recipe: Option<&'static str>, tx: &mut Tx) {
        let server = if server.is_empty() { self.bench.current_key().unwrap_or("local").to_owned() } else { server.to_owned() };
        self.screens.close_all();
        let state = &mut self.screens.recipes;
        state.open = Some(server.clone());
        state.aside = false;
        let asked = state.facts.contains_key(&server);
        self.recipes_show(recipe);
        if !asked {
            self.backend_mut().recipes(Ask::Facts { server });
        }
        tx.instant();
    }

    pub fn close_recipes(&mut self, tx: &mut Tx) {
        self.screens.recipes.close();
        self.settle(tx);
    }

    /// Shows a recipe (or the list), its fields filled as they were left.
    fn recipes_show(&mut self, recipe: Option<&'static str>) {
        let state = &mut self.screens.recipes;
        state.recipe = recipe.and_then(|id| book::RECIPE_IDS.iter().copied().find(|r| *r == id));
        state.edits.clear();
        state.expanded.clear();
        state.failing.clear();
        state.key_error = None;
        let (Some(key), Some(r)) = (state.key(), state.recipe.and_then(recipes::recipe)) else { return };
        let server = key.0.clone();
        let typed = state.typed.get(&key).cloned().unwrap_or_default();
        let (os, _) = state.os_of(&server);
        let base = Ctx::new(os, state.facts.get(&server).unwrap_or(&Facts::default()), BTreeMap::new());
        for f in &r.fields {
            if let FieldKind::Text { .. } = f.kind {
                let text = typed.get(f.id).cloned().or_else(|| f.initial.as_ref().map(|t| t.get(&base))).unwrap_or_default();
                state.edits.insert(f.id, TextEdit::new(&text));
            }
        }
        state.edits.insert("_keyPaste", TextEdit::new(typed.get("_keyPaste").map_or("", String::as_str)));
        // The usual key, picked for you the first time.
        if r.wants_key() && !typed.contains_key("_keyChoice") {
            if let Some(first) = state.facts.get(&server).and_then(|f| f.keys.first()).map(|k| k.path.clone()) {
                self.recipes_pick_key(&first);
            }
        }
    }

    /// A text field changed: what it holds is kept.
    fn recipes_typed(&mut self, id: &'static str) {
        let state = &mut self.screens.recipes;
        let Some(key) = state.key() else { return };
        let text = state.edits.get(id).map(|e| e.text.clone()).unwrap_or_default();
        let typed = state.typed.entry(key).or_default();
        if id == "_keyPaste" {
            typed.insert("publicKey".into(), parse_public_key(&text, None).unwrap_or_default());
            typed.insert("keyFile".into(), String::new());
        }
        typed.insert(id.into(), text);
    }

    fn recipes_set(&mut self, id: &str, value: &str) {
        let state = &mut self.screens.recipes;
        if let Some(key) = state.key() {
            state.typed.entry(key).or_default().insert(id.into(), value.into());
        }
    }

    /// A key on this computer: its `.pub` is read by the backend.
    fn recipes_pick_key(&mut self, path: &str) {
        let state = &mut self.screens.recipes;
        let Some(key) = state.key() else { return };
        let typed = state.typed.entry(key.clone()).or_default();
        typed.insert("_keyChoice".into(), path.into());
        typed.insert("publicKey".into(), String::new());
        typed.insert("keyFile".into(), String::new());
        state.reading = Some(path.into());
        state.key_error = None;
        self.backend_mut().recipes(Ask::ReadKey { server: key.0, path: path.into() });
    }

    fn recipes_paste_choice(&mut self) {
        let state = &mut self.screens.recipes;
        let Some(key) = state.key() else { return };
        let pasted = state.edits.get("_keyPaste").map(|e| e.text.clone()).unwrap_or_default();
        let typed = state.typed.entry(key).or_default();
        typed.insert("_keyChoice".into(), "paste".into());
        typed.insert("publicKey".into(), parse_public_key(&pasted, None).unwrap_or_default());
        typed.insert("keyFile".into(), String::new());
        state.reading = None;
        state.key_error = None;
        if let Some(e) = state.edits.get_mut("_keyPaste") {
            e.focused = true;
        }
    }

    fn recipes_toggle(&mut self, title: &'static str) {
        let state = &mut self.screens.recipes;
        let Some(key) = state.key() else { return };
        let done = state.done.entry(key).or_default();
        match done.iter().position(|d| d == title) {
            Some(at) => {
                done.remove(at);
            }
            None => done.push(title.into()),
        }
        state.expanded.remove(title);
    }

    pub(crate) fn recipes_start_over(&mut self) {
        let state = &mut self.screens.recipes;
        if let Some(key) = state.key() {
            state.done.remove(&key);
        }
    }

    /// Pastes a step's command into the terminal (this computer's for a local step), without
    /// Return, and steps aside.
    fn recipes_paste(&mut self, command: String, local: bool, tx: &mut Tx) {
        let Some(server) = self.screens.recipes.open.clone() else { return };
        let on = if local { "local".to_owned() } else { server };
        self.screens.recipes.aside = true;
        self.run_on(&on, &command, false, tx);
    }

    /// The bar's "Back to the steps".
    fn recipes_back(&mut self, tx: &mut Tx) {
        self.screens.close_all();
        self.screens.recipes.aside = false;
        tx.instant();
    }
}

/// The backend's answers about a recipe's facts and keys.
pub fn deliver(s: &mut Shell, reply: Reply) {
    match reply {
        Reply::Facts { server, facts } => {
            let state = &mut s.screens.recipes;
            state.facts.insert(server.clone(), facts);
            // Fields that start from the server's port, and the usual key, now it's known.
            if state.open.as_deref() == Some(server.as_str()) {
                let recipe = state.recipe;
                s.recipes_show(recipe);
            }
        }
        Reply::Key { server, path, line } => {
            let state = &mut s.screens.recipes;
            if state.reading.as_deref() != Some(path.as_str()) || state.open.as_deref() != Some(server.as_str()) {
                // A slow read mustn't undo a choice made since.
                return;
            }
            state.reading = None;
            let Some(key) = state.key() else { return };
            let name = path.rsplit(['/', '\\']).next().unwrap_or("").to_owned();
            match line.and_then(|l| parse_public_key(&l, Some(&name))) {
                Some(line) => {
                    let typed = state.typed.entry(key).or_default();
                    typed.insert("publicKey".into(), line);
                    typed.insert("keyFile".into(), path);
                }
                None => state.key_error = Some(format!("Couldn't read a public key from {path}.pub. Pick another key, or paste one.")),
            }
        }
        Reply::Checked { .. } | Reply::Fixed { .. } => {}
    }
}

/// A recipe's sign.
fn sign(id: &str) -> Icon {
    match id {
        "ssh-keys" => Icon::Key,
        "firewall" => Icon::Shield,
        "fail2ban" => Icon::Padlock,
        "auto-updates" => Icon::Refresh,
        "sudo-user" => Icon::Bot,
        "docker" => Icon::Cube,
        "swap" => Icon::Drive,
        "caddy-https" => Icon::Globe,
        _ => Icon::Doc,
    }
}

/// The server's system: two choices, and whether it was guessed.
fn os_switch(t: &Theme, os: Os, guessed: bool) -> El {
    let picks = choices(
        t,
        Os::ALL.iter().enumerate().map(|(i, o)| {
            let o = *o;
            choice(t, o.label(), o == os).key(format!("os-{i}")).on_press_tx(move |s: &mut Shell, _| {
                if let Some(server) = s.screens.recipes.open.clone() {
                    s.screens.recipes.os.insert(server, o);
                }
            })
        }),
    )
    .label_static("The server's system");
    let mut out = row().gap(10.0).items_center().child(picks);
    if guessed {
        out = out.child(text("Guessed from the server's kernel").font(theme::SMALL).color(t.l2).single_line());
    }
    out
}

/// A small tag beside a step's title.
fn tag(t: &Theme, words: &str, ok: bool) -> El {
    let (fill, ink) = if ok { (t.green.alpha(0.14), t.green) } else { (t.hov2, t.l2on) };
    row().h(17.0).px(6.0).gap(4.0).radius(theme::RADIUS_CHIP).fill(fill).items_center().child(text(words).font(theme::CHIP).color(ink).single_line())
}

fn list_page(t: &Theme, s: &Shell, server: &str, server_name: &str) -> El {
    let state = &s.screens.recipes;
    let (os, guessed) = state.os_of(server);
    let head = header(t, Icon::Doc, "Recipes", &format!("{server_name} · step by step, and you run every command"), vec![close_button(t, "recipes-close").on_press_tx(|s: &mut Shell, tx| s.close_recipes(tx))]);
    let mut page = body("recipes-body").child(os_switch(t, os, guessed));
    let mut n = 0;
    for group in Group::ALL {
        let mut rows = col().w_full().gap(6.0);
        for r in book::all().into_iter().filter(|r| r.group == group) {
            let steps = r.steps_for(os);
            let ticked = state.done.get(&(server.to_owned(), r.id)).map_or(0, |d| done_in(&steps, d));
            let id = r.id;
            let words = col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(text(r.title).font(theme::TEXT_BOLD).color(t.l1).wrap()).child(text(r.summary).font(theme::NOTE).color(t.l2).wrap());
            let mut item = row()
                .key(format!("recipe-row-{n}"))
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(12.0)
                .items_center()
                .radius(theme::RADIUS_GROUP)
                .fill(t.group)
                .border(1.0, t.hair2)
                .hover(t.hov2)
                .press(t.down)
                .focusable(true)
                .role("button")
                .label(r.title)
                .on_press_tx(move |s: &mut Shell, _| s.recipes_show(Some(id)))
                .child(el().size(32.0, 32.0).radius(theme::RADIUS).fill(t.blue.alpha(0.12)).items_center().justify_center().child(icon(sign(id), theme::ICON, t.blue)))
                .child(words);
            if ticked > 0 {
                item = item.child(if ticked >= steps.len() { tag(t, "Done", true) } else { text(format!("{ticked} of {}", steps.len())).font(theme::VALUE).color(t.l2).single_line() });
            }
            rows = rows.child(item.child(icon(Icon::ChevronRight, 14.0, t.l2)));
            n += 1;
        }
        page = page.child(col().w_full().gap(8.0).child(text(group.label()).font(theme::GROUP_TITLE).color(t.l2).single_line().role("heading").level(2)).child(rows));
    }
    headed(head, page.child(text("Commands that change the system start with sudo, which runs them as root and may ask for your password. Nothing runs until you press Return in the terminal.").font(theme::SMALL).color(t.l2).wrap()), 14.0)
}

/// The key field: a key from this computer, or one pasted in.
fn key_field(t: &Theme, s: &Shell, r: &Recipe, typed: &BTreeMap<String, String>, facts: Option<&Facts>) -> El {
    let state = &s.screens.recipes;
    let f = r.fields.iter().find(|f| matches!(f.kind, FieldKind::Key));
    let label = f.map_or("Your key", |f| f.label);
    let chosen = typed.get("_keyChoice").map_or("", String::as_str);
    let mut out = col().key("field-publicKey").w_full().gap(6.0).child(text(label).font(theme::TAB_NAME).color(t.l1).single_line());
    let Some(facts) = facts else {
        return out.child(text("Looking for keys on this computer…").font(theme::SMALL).color(t.l2).single_line());
    };
    let mut picks = col().w_full().gap(4.0).role("radiogroup").label(label);
    let pick = |key: String, title: String, detail: String, badge: Option<String>, on: bool, path: Option<String>| {
        // A key by its file's name, in the terminal's font; "Paste a public key" in words.
        let font = if path.is_some() { theme::CODE } else { theme::TEXT };
        let mut words = row().gap(8.0).items_center().child(text(title).font(font).color(t.l1).single_line());
        if let Some(b) = badge {
            words = words.child(tag(t, &b, false));
        }
        let dot = el().size(14.0, 14.0).radius(7.0).items_center().justify_center();
        let dot = if on { dot.fill(t.blue).child(el().size(6.0, 6.0).radius(3.0).fill(t.on_color)) } else { dot.border(1.0, t.l2) };
        let mut item = row()
            .key(key)
            .w_full()
            .px(10.0)
            .py(6.0)
            .gap(10.0)
            .items_center()
            .radius(theme::RADIUS_ROW)
            .border(1.0, if on { t.blue.alpha(0.6) } else { t.hair })
            .focusable(true)
            .role("radio")
            .checked(on)
            .child(dot)
            .child(col().grow(1.0).shrink().min_w(0.0).child(words).child(text(detail).font(theme::SMALL).color(t.l2).single_line().truncate()));
        item = if on { item.fill(t.blue.alpha(0.1)) } else { item.hover(t.hov2) };
        match path {
            Some(path) => item.on_press_tx(move |s: &mut Shell, _| s.recipes_pick_key(&path)),
            None => item.on_press_tx(|s: &mut Shell, _| s.recipes_paste_choice()),
        }
    };
    for (i, k) in facts.keys.iter().enumerate() {
        picks = picks.child(pick(format!("key-{i}"), k.name.clone(), format!("{}.pub", s.text_shown(&k.path)), Some(k.kind.clone()), chosen == k.path, Some(k.path.clone())));
    }
    picks = picks.child(pick("key-paste-choice".into(), "Paste a public key".into(), "From a password manager, or another computer".into(), None, chosen == "paste", None));
    out = out.child(picks);
    if chosen == "paste" {
        let edit = state.edits.get("_keyPaste").cloned().unwrap_or_default();
        let pasted = edit.text.clone();
        let bad = !pasted.trim().is_empty() && f.is_some_and(|f| field_error(f, &pasted).is_some());
        out = out.child(field(t, code_input(t, &edit, "ssh-ed25519 AAAA…").key("key-paste").on_edit(|s: &mut Shell| s.screens.recipes.edits.entry("_keyPaste").or_default()).on_input_change(|s: &mut Shell, _| s.recipes_typed("_keyPaste")), bad));
        if bad {
            out = out.child(text("That isn't a public key. One starts with ssh-ed25519, ssh-rsa or ecdsa-sha2-.").font(theme::SMALL).color(t.red).wrap());
        }
    }
    if facts.keys.is_empty() {
        out = out.child(text("No key in ~/.ssh on this computer yet. Make one with the first step, then open this recipe again.").font(theme::SMALL).color(t.l2).wrap());
    }
    if state.reading.is_some() {
        out = out.child(text("Reading the key…").font(theme::SMALL).color(t.l2).single_line());
    }
    if let Some(why) = &state.key_error {
        out = out.child(text(s.text_shown(why).into_owned()).font(theme::SMALL).color(t.red).wrap().role("alert"));
    } else if let Some(f) = f {
        out = out.child(text(format!("{} Only its public half is used.", f.help)).font(theme::SMALL).color(t.l2).wrap());
    }
    out
}

fn fields(t: &Theme, s: &Shell, r: &Recipe, typed: &BTreeMap<String, String>, facts: Option<&Facts>) -> El {
    let mut box_ = col().key("recipe-fields").w_full().px(14.0).py(12.0).gap(12.0).radius(theme::RADIUS_GROUP).fill(t.group).border(1.0, t.hair2).child(text("Fill in first").font(theme::GROUP_TITLE).color(t.l2).single_line());
    for f in &r.fields {
        let item = match &f.kind {
            FieldKind::Key => key_field(t, s, r, typed, facts),
            FieldKind::Choice(options) => {
                let current = typed.get(f.id).cloned().or_else(|| f.initial.as_ref().map(|i| i.get(&Ctx::default()))).unwrap_or_default();
                let id = f.id;
                let picks = choices(
                    t,
                    options.iter().map(|(value, label)| {
                        let value = *value;
                        choice(t, label, current == value).key(format!("field-{id}-{value}")).on_press_tx(move |s: &mut Shell, _| s.recipes_set(id, value))
                    }),
                )
                .label_static(f.label);
                col().key(format!("field-{id}")).gap(6.0).items_start().child(text(f.label).font(theme::TAB_NAME).color(t.l1).single_line()).child(picks).child(text(f.help).font(theme::SMALL).color(t.l2).wrap())
            }
            FieldKind::Text { placeholder, .. } => {
                let id = f.id;
                let edit = s.screens.recipes.edits.get(id).cloned().unwrap_or_default();
                let error = field_error(f, &edit.text);
                let entry = code_input(t, &edit, placeholder).key(format!("field-{id}")).on_edit(move |s: &mut Shell| s.screens.recipes.edits.entry(id).or_default()).on_input_change(move |s: &mut Shell, _| s.recipes_typed(id));
                col()
                    .gap(5.0)
                    .w_full()
                    .child(text(f.label).font(theme::TAB_NAME).color(t.l1).single_line())
                    .child(field(t, entry, error.is_some()).w(280.0))
                    .child(text(error.unwrap_or(f.help)).font(theme::SMALL).color(if error.is_some() { t.red } else { t.l2 }).wrap())
            }
        };
        box_ = box_.child(item);
    }
    box_
}

/// Paste and Copy for a command.
fn command_buttons(t: &Theme, key: String, command: &str, local: bool, server_name: &str) -> El {
    let (paste, copy) = (command.to_owned(), command.to_owned());
    let into = if local { "Paste into a terminal on this computer, without pressing Return".to_owned() } else { format!("Paste into the terminal on {server_name}, without pressing Return") };
    row()
        .gap(8.0)
        .items_center()
        .child(push_button(t, "Paste into terminal", Push::Quiet, true).key(format!("paste-{key}")).help(into).on_press_tx(move |s: &mut Shell, tx| s.recipes_paste(paste.clone(), local, tx)))
        .child(push_button(t, "Copy", Push::Quiet, true).key(format!("copy-{key}")).on_press_tx(move |_: &mut Shell, tx| tx.copy(copy.clone())))
}

#[allow(clippy::too_many_arguments)]
fn step_card(t: &Theme, s: &Shell, r: &Recipe, step: &Step, n: usize, ctx: &Ctx, done: bool, has_key: bool, server_name: &str) -> El {
    let state = &s.screens.recipes;
    let title = step.title;
    let open = !done || state.expanded.contains(title);
    let mark = el()
        .key(format!("tick-{n}"))
        .size(22.0, 22.0)
        .radius(11.0)
        .items_center()
        .justify_center()
        .focusable(true)
        .role("checkbox")
        .checked(done)
        .label(if done { format!("Step {} is done. Untick it", n + 1) } else { format!("Tick off step {}", n + 1) })
        .help(if done { "Untick" } else { "Tick off once it worked" })
        .on_press_tx(move |s: &mut Shell, _| s.recipes_toggle(title));
    let mark = if done { mark.fill(t.green).child(icon(Icon::Check, 13.0, t.on_color)) } else { mark.border(1.0, t.l2).hover(t.hov2).child(text((n + 1).to_string()).font(theme::LIST_NUMBER).color(t.l2on).single_line().a11y_hidden()) };

    let mut head = row().gap(8.0).items_center().flex_wrap();
    let heading = text(title).font(theme::TEXT_BOLD).color(if done { t.l2on } else { t.l1 }).wrap().shrink().min_w(0.0);
    head = head.child(if done {
        heading.role("button").focusable(true).on_press_tx(move |s: &mut Shell, _| {
            let e = &mut s.screens.recipes.expanded;
            if !e.remove(title) {
                e.insert(title.to_owned());
            }
        })
    } else {
        heading
    });
    if step.local {
        head = head.child(tag(t, "On your computer", false));
    }
    if step.check {
        head = head.child(tag(t, "Check", true));
    }
    let mut words = col().grow(1.0).shrink().min_w(0.0).gap(6.0).child(head);
    if open {
        if let Some(warn) = &step.warn {
            words = words.child(
                row().w_full().px(10.0).py(8.0).gap(8.0).items_start().radius(theme::RADIUS_ROW).fill(t.orange.alpha(0.12)).role("note").child(icon(Icon::Warning, 14.0, t.orange).mt(1.0)).child(text(warn.get(ctx)).font(theme::NOTE).color(t.l1).wrap().grow(1.0).shrink().min_w(0.0)),
            );
        }
        words = words.child(text(s.text_shown(&step.explain.get(ctx)).into_owned()).font(theme::NOTE).color(t.l2on).wrap().selectable(true));
        if step.skip_if_key && has_key {
            words = words.child(text("SSHeesh found a key on this computer, so you can skip this step.").font(theme::SMALL).color(t.green).wrap());
        }
        let wanting = missing(r, step, &ctx.values);
        if wanting.is_empty() {
            let command = step.command.get(ctx);
            words = words.child(command_block(t, &s.text_shown(&command)).key(format!("cmd-{n}"))).child(command_buttons(t, n.to_string(), &command, step.local, server_name));
        } else {
            words = words.child(note(t, &fill_in_first(&wanting), false));
        }
        if let Some(expect) = &step.expect {
            words = words.child(
                row().w_full().gap(4.0).child(text("You should see:").font(theme::SMALL).color(t.l2on).single_line()).child(text(s.text_shown(&expect.get(ctx)).into_owned()).font(theme::SMALL).color(t.l2).wrap().grow(1.0).shrink().min_w(0.0)),
            );
        }
        if let Some((why, fix)) = &step.if_fails {
            let showing = state.failing.contains(title);
            let toggle = row()
                .key(format!("fails-{n}"))
                .gap(4.0)
                .items_center()
                .focusable(true)
                .role("button")
                .expanded(showing)
                .label_static("If it didn't work")
                .on_press_tx(move |s: &mut Shell, _| {
                    let f = &mut s.screens.recipes.failing;
                    if !f.remove(title) {
                        f.insert(title.to_owned());
                    }
                })
                .child(icon(if showing { Icon::ChevronDown } else { Icon::ChevronRight }, 12.0, t.l2on))
                .child(text("If it didn't work").font(theme::CONTROL_TEXT).color(t.l2on).single_line());
            words = words.child(toggle);
            if showing {
                let fix = fix.get(ctx);
                words = words
                    .child(text(why.get(ctx)).font(theme::NOTE).color(t.l2on).wrap())
                    .child(command_block(t, &s.text_shown(&fix)))
                    .child(command_buttons(t, format!("fix-{n}"), &fix, false, server_name));
            }
        }
    }
    let mut card = row().key(format!("step-{n}")).w_full().px(14.0).py(12.0).gap(10.0).items_start().radius(theme::RADIUS_GROUP).border(1.0, if done { t.green.alpha(0.3) } else { t.hair2 });
    card = card.fill(if done { t.green.alpha(0.06) } else { t.group });
    card.child(mark).child(words)
}

fn recipe_page(t: &Theme, s: &Shell, server: &str, server_name: &str, r: &Recipe) -> El {
    let state = &s.screens.recipes;
    let key = (server.to_owned(), r.id);
    let (os, guessed) = state.os_of(server);
    let facts = state.facts.get(server);
    let empty = Facts::default();
    let typed = state.typed.get(&key).cloned().unwrap_or_default();
    let base = Ctx::new(os, facts.unwrap_or(&empty), BTreeMap::new());
    let values = field_values(r, &typed, &base);
    let ctx = Ctx::new(os, facts.unwrap_or(&empty), values);
    let steps = r.steps_for(os);
    let done = state.done.get(&key).cloned().unwrap_or_default();
    let count = done_in(&steps, &done);
    let has_key = facts.is_some_and(|f| !f.keys.is_empty());

    let back = row()
        .key("recipes-back")
        .h(theme::CONTROL)
        .px(4.0)
        .ml(-4.0)
        .gap(2.0)
        .items_center()
        .radius(theme::RADIUS_ROW)
        .hover(t.hov2)
        .press(t.down)
        .focusable(true)
        .role("button")
        .label_static("All recipes")
        .on_press_tx(|s: &mut Shell, _| s.recipes_show(None))
        .child(icon(Icon::Back, 14.0, t.l2on))
        .child(text("All recipes").font(theme::CONTROL_TEXT).color(t.l2on).single_line().a11y_hidden());
    let head = header(t, sign(r.id), r.title, &format!("{server_name} · you run every command yourself"), vec![close_button(t, "recipes-close").on_press_tx(|s: &mut Shell, tx| s.close_recipes(tx))]);
    let mut page = body("recipes-body")
        .child(row().w_full().gap(8.0).items_center().child(back).child(spacer()).child(os_switch(t, os, guessed)))
        .child(col().w_full().gap(3.0).child(text(r.summary).font(theme::TEXT).color(t.l1).wrap()).child(text(format!("For: {}", r.who)).font(theme::NOTE).color(t.l2).wrap()));
    if !r.fields.is_empty() {
        page = page.child(fields(t, s, r, &typed, facts));
    }
    let mut list = col().key("recipe-steps").w_full().gap(8.0);
    for (n, step) in steps.iter().enumerate() {
        list = list.child(step_card(t, s, r, step, n, &ctx, done.iter().any(|d| d == step.title), has_key, server_name));
    }
    page = page.child(list);
    let mut foot = row().w_full().h(theme::CONTROL).gap(10.0).items_center().child(text(progress(count, steps.len())).key("recipes-progress").font(theme::NOTE).color(t.l2).single_line().role("status"));
    if count > 0 {
        foot = foot.child(spacer()).child(push_button(t, "Start over", Push::Quiet, true).key("recipes-over").on_press_tx(|s: &mut Shell, _| s.recipes_start_over()));
    }
    headed(head, page.child(foot), 14.0)
}

/// The dialog, when it's open and not stepped aside.
pub fn view(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.recipes;
    let server = state.open.as_deref().filter(|_| !state.aside)?;
    let server_name = s.bench.server(server).map(|sv| s.server_shown(server, &sv.name).into_owned()).unwrap_or_else(|| "This computer".into());
    let page = match state.recipe.and_then(recipes::recipe) {
        Some(r) => recipe_page(t, s, server, &server_name, &r),
        None => list_page(t, s, server, &server_name),
    };
    Some(dialog(t, "recipes-scrim", "recipes", "Recipes", WIDTH, page, |s: &mut Shell, tx| s.close_recipes(tx)))
}

/// The recipe stepped aside: a small bar at the top right, clear of the terminal's prompt.
pub fn aside(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.recipes;
    let server = state.open.as_deref().filter(|_| state.aside)?;
    let r = state.recipe.and_then(recipes::recipe)?;
    let (os, _) = state.os_of(server);
    let steps = r.steps_for(os);
    let done = state.done.get(&(server.to_owned(), r.id)).map_or(0, |d| done_in(&steps, d));
    let bar = row()
        .key("recipe-aside")
        .h(36.0)
        .pl(12.0)
        .pr(4.0)
        .gap(10.0)
        .items_center()
        .radius(18.0)
        .fill(t.pop)
        .border(1.0, t.rim)
        .shadow(8.0, 24.0, rune::Color::BLACK.alpha(0.22))
        .role("region")
        .label_static("Recipe")
        .child(icon(sign(r.id), 14.0, t.blue))
        .child(text(r.title).font(theme::TAB_NAME).color(t.l1).single_line().truncate().max_w(240.0))
        .child(text(format!("{done} of {} done", steps.len())).font(theme::VALUE).color(t.l2).single_line())
        .child(push_button(t, "Back to the steps", Push::Primary, true).key("aside-back").on_press_tx(|s: &mut Shell, tx| s.recipes_back(tx)))
        .child(icon_button(t, Icon::Close, "Close the recipe", true).key("aside-close").on_press_tx(|s: &mut Shell, tx| s.close_recipes(tx)));
    Some(bar.pin(None, Some(theme::BAR_HEIGHT + 10.0), Some(16.0), None))
}
