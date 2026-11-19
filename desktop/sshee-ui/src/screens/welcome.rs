//! The welcome (0.10's `Welcome.tsx`), shown once to everyone (`sshee_app::tour::welcome_due`):
//! a few short pages in one dialog, each with Skip.
//!
//! 1. **Your servers.** 1.0 keeps a list of its own, so it offers to bring 0.10's over and to
//!    pick from `~/.ssh/config`, or to add one by hand. A pick opens "Add a server" at that
//!    list once its sources are read ([`tick`]); when it closes, the welcome comes back at the
//!    next page.
//! 2. **Layout.** Where tabs go, one at a time or side by side, and compact mode, each with a
//!    picture of the window that way; naming the server on every tab, the vitals in the
//!    status bar and the line for Production and Staging.
//! 3. **Sidebar.** Its side, names or icons, line or pixel icons, each with a picture; how
//!    busy a server looks, and the logo.
//! 4. **How it looks.** Match my system, light or dark, as pictures of the window in each; the
//!    dark theme, the accent, the interface size and animations.
//! 5. **In the terminal.** Copying, and what a dropped connection does, each with a short loop
//!    of the real terminal doing it ([`pictures`]).
//! 6. **Three things people miss**, with their shortcuts; and the tour.
//!
//! A choice is a row of Settings (`sshee_app::tour::Page::rows`), changed through it as
//! Settings does and shown at once behind the dialog. What's on when it opens stays picked:
//! Continue through to Done changes nothing, and Skip (or Esc) at any page decides nothing.
//!
//! ```text
//! welcome-scrim   dims the window; modal (Esc skips)
//! └── welcome     welcome-dot-<i> · welcome-page-<id>:
//!                   servers   welcome-old, welcome-config, welcome-add
//!                   layout    welcome-<row>-<value> (options), welcome-<row>-picture,
//!                   sidebar     welcome-<row> (a switch, segmented or pop-up control)
//!                   look      welcome-look-<value> (cards), welcome-<row>, welcome-accent-<i>
//!                   terminal  welcome-<row>-<value> (options), welcome-<row>-picture
//!                   tips      welcome-tip-<id>, welcome-tour
//!                 welcome-skip · welcome-back · welcome-next (Return)
//! welcome-menu    a pop-up control's menu, over everything (`menu`)
//! ```
//!
//! The program says whether this start may show it at all (`Backend::welcome`), so a demo and
//! a test start without it.

mod pictures;

use rune::{col, el, row, spacer, text, Color, El, Tx};
use sshee_app::palette::shortcut_label;
use sshee_app::servers::{Ask, Source, Sources};
use sshee_app::settings_store::catalog::{self, ChoiceLook, Control, Row, Special};
use sshee_app::settings_store::looks;
use sshee_app::settings_store::schemes::Appearance;
use sshee_app::tour::{self, Page, PAGES, TIPS};

use super::checkup::{body, dialog, headed};
use crate::icons::{icon, logo, Icon};
use crate::kit::form::{form_group, form_row, RowText};
use crate::kit::list::{list_row, ListRow};
use crate::kit::menu::key_cap;
use crate::kit::popup::{popup_button, popup_menu};
use crate::kit::push::{push_button, Push};
use crate::kit::segmented::segmented;
use crate::kit::switch::switch;
use crate::looks::color;
use crate::shell::Shell;
use crate::theme::{self, Theme};

pub use pictures::Pictures;

pub const WIDTH: f32 = 600.0;
/// Every page is this tall, so the buttons under it stay where the pointer is.
const PAGE_HEIGHT: f32 = 488.0;
// With its head and foot it fits a dialog (at most 680 points tall).
const _: () = assert!(PAGE_HEIGHT < 680.0 - 140.0);
/// A picture column's picture, as wide as it is tall times this: the window's own shape.
const WINDOW_SHAPE: f32 = 1.6;

#[derive(Default)]
pub struct State {
    pub open: bool,
    /// The page on screen, in `tour::PAGES`.
    pub at: usize,
    /// "Add a server" was opened from here, at this list: shown once its sources are read.
    pub then_list: Option<Source>,
    /// "Add a server" was opened from the first page: once it closes, the welcome comes back
    /// at the next.
    resume: bool,
    /// The pictures, decoded on Rune's image thread once the welcome first opens.
    pictures: Option<Pictures>,
    /// The row whose pop-up menu is open.
    menu: Option<&'static str>,
    /// What XPipe and the other SSH apps on this computer hold, once the backend has looked.
    pub(crate) found: Option<Sources>,
}

/// At the window's start: the welcome when it's due and the program allows it; else "What's
/// new" when an update brought something to read. Either way the version is remembered.
pub fn start(s: &mut Shell) {
    let allowed = s.backend_mut().welcome();
    let due = allowed && tour::welcome_due(s.settings.get());
    if due {
        s.show_welcome(0);
    }
    super::whats_new::start(s, due);
}

impl Shell {
    fn show_welcome(&mut self, at: usize) {
        let state = &mut self.screens.welcome;
        state.open = true;
        state.at = at.min(PAGES.len() - 1);
        state.resume = false;
        state.menu = None;
        // What could be brought over from XPipe and other apps: files to read, so the backend
        // looks and says later (`Shell::servers_answer`).
        self.backend_mut().servers(Ask::Sources);
    }

    /// Done with the welcome, by any way out: it doesn't come back.
    fn welcome_done(&mut self) {
        self.screens.welcome.open = false;
        self.screens.welcome.menu = None;
        self.settings.update(tour::welcomed);
    }

    /// Settings → About's "Show the welcome again".
    pub fn open_welcome(&mut self, tx: &mut Tx) {
        self.screens.close_all();
        self.show_welcome(0);
        tx.instant();
    }

    /// "Skip", and Esc, at any page: nothing more is changed.
    pub fn skip_welcome(&mut self, tx: &mut Tx) {
        self.welcome_done();
        self.settle(tx);
    }

    /// One of the ways to bring servers in: "Add a server", at that list when there's one;
    /// the welcome comes back at its next page when that closes.
    fn welcome_add(&mut self, list: Option<Source>, tx: &mut Tx) {
        self.welcome_done();
        self.open_add_server(tx);
        let state = &mut self.screens.welcome;
        state.then_list = list;
        state.resume = true;
    }

    /// Continue (`by` 1) or Back (-1); Continue on the last page is Done.
    fn welcome_step(&mut self, by: i32, tx: &mut Tx) {
        let at = self.screens.welcome.at;
        if by > 0 && at + 1 >= PAGES.len() {
            self.skip_welcome(tx);
        } else {
            self.screens.welcome.at = tour::page(at, by);
            self.screens.welcome.menu = None;
            tx.instant();
        }
    }

    fn welcome_tour(&mut self, tx: &mut Tx) {
        self.welcome_done();
        self.open_tour(0, tx);
    }

    /// A choice picked: through its row of Settings, at once.
    fn welcome_pick(&mut self, key: &'static str, value: &'static str, tx: &mut Tx) {
        self.screens.welcome.menu = None;
        tx.instant();
        // What's already picked stays as it is: writing it again could swap the active theme.
        if tour::chosen(&self.settings, key).as_deref() == Some(value) {
            return;
        }
        self.change_settings(tx, |st| {
            tour::pick(st, key, value);
        });
    }
}

/// Every frame: "Add a server" opened from the welcome goes to its list once it's read, and
/// the welcome comes back at its next page once that's closed.
pub fn tick(s: &mut Shell) -> bool {
    // The pictures start decoding once there's a waker: at the window's start there isn't yet.
    if s.screens.welcome.open && s.screens.welcome.pictures.is_none() {
        if let Some(waker) = s.waker() {
            s.screens.welcome.pictures = Some(Pictures::load(waker));
            return true;
        }
    }
    if s.screens.welcome.resume && s.screens.welcome.then_list.is_none() && !s.screens.any_open() && !s.screens.settings.open {
        s.show_welcome(1);
        return true;
    }
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
    let state = &s.screens.welcome;
    if !state.open {
        return None;
    }
    let page = PAGES[state.at];
    let head = row()
        .w_full()
        .gap(12.0)
        .items_center()
        .child(logo(32.0))
        .child(text("Welcome to SSHeesh").font(theme::TITLE).color(t.l1).single_line().grow(1.0).shrink().min_w(0.0))
        .child(dots(t, state.at));
    let content = match page {
        Page::Servers => servers(s, t),
        Page::Layout => layout(s, t),
        Page::Sidebar => sidebar(s, t),
        Page::Look => look(s, t),
        Page::Terminal => terminal(s, t),
        Page::Tips => tips(s, t),
    };
    let words = col().w_full().gap(4.0).child(text(page.title()).key("welcome-title").font(theme::PAGE_TITLE).color(t.l1).single_line().role("heading").level(1));
    let page_el = col().key(format!("welcome-page-{}", page.id())).w_full().min_h(PAGE_HEIGHT).gap(16.0).child(words.children(content.line.map(|line| text(line).key("welcome-line").font(theme::TEXT).color(t.l2on).wrap()))).children(content.parts);
    let page_body = body("welcome-body").gap(0.0).pb(8.0).child(page_el);
    let card = col().w_full().shrink().min_h(0.0).child(headed(t, head, page_body, 16.0)).child(foot(t, state.at));
    Some(dialog(t, "welcome-scrim", "welcome", "Welcome to SSHeesh", WIDTH, card, |s: &mut Shell, tx| s.skip_welcome(tx)))
}

/// What a page holds under its title.
struct Content {
    line: Option<String>,
    parts: Vec<El>,
}

/// Where the welcome is: a dot a page, the one on screen long. A dot goes to its page.
fn dots(t: &Theme, at: usize) -> El {
    let mut dots = row().key("welcome-dots").gap(6.0).items_center().role("tablist").label_static("Pages");
    for (i, page) in PAGES.iter().enumerate() {
        let on = i == at;
        dots = dots.child(
            el().key(format!("welcome-dot-{i}"))
                .size(if on { 18.0 } else { 6.0 }, 6.0)
                .radius(3.0)
                .fill(if on { t.blue } else { t.l2.alpha(0.4) })
                .focusable(true)
                .role("tab")
                .checked(on)
                .label(format!("{} of {}: {}", i + 1, PAGES.len(), page.title()))
                .on_press_tx(move |s: &mut Shell, tx| {
                    s.screens.welcome.at = i;
                    s.screens.welcome.menu = None;
                    tx.instant();
                }),
        );
    }
    row().gap(10.0).items_center().child(text(format!("{} of {}", at + 1, PAGES.len())).font(theme::VALUE).color(t.l2).single_line()).child(dots)
}

/// Skip at the left (the tour, on the last page); Back and Continue at the right.
fn foot(t: &Theme, at: usize) -> El {
    let last = at + 1 == PAGES.len();
    let start = if last {
        push_button(t, "Take the one-minute tour", Push::Quiet, true).key("welcome-tour").on_press_tx(|s: &mut Shell, tx| s.welcome_tour(tx))
    } else {
        push_button(t, "Skip", Push::Quiet, true).key("welcome-skip").on_press_tx(|s: &mut Shell, tx| s.skip_welcome(tx))
    };
    let mut foot = row().w_full().px(20.0).pt(12.0).pb(16.0).gap(8.0).items_center().border_sides([1.0, 0.0, 0.0, 0.0], t.hair).child(start).child(spacer());
    if at > 0 {
        foot = foot.child(push_button(t, "Back", Push::Quiet, true).key("welcome-back").on_press_tx(|s: &mut Shell, tx| s.welcome_step(-1, tx)));
    }
    foot.child(push_button(t, if last { "Done" } else { "Continue" }, Push::Primary, true).key("welcome-next").default_button().on_press_tx(|s: &mut Shell, tx| s.welcome_step(1, tx)))
}

// ---- 1. your servers

fn servers(s: &Shell, t: &Theme) -> Content {
    let servers = s.bench.servers().iter().filter(|sv| !sv.is_local()).count();
    let (old, config) = (s.screens.home.old_new, s.screens.home.config_new);
    let plural = |n: usize, one: &str, many: &str| if n == 1 { one.to_owned() } else { many.replace('#', &n.to_string()) };
    let mut ways = col().key("welcome-ways").w_full().gap(2.0);
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
    // XPipe and the other SSH apps, each only when it has servers this list hasn't.
    let elsewhere = s.screens.welcome.found.as_ref().map(tour::found_elsewhere).unwrap_or_default();
    for &(source, n) in &elsewhere {
        let key = match source {
            Source::App(app) => app.key(),
            _ => "xpipe",
        };
        let label = tour::bring_over(source, n);
        let spec = ListRow { icon: Some(Icon::Download), label: &label, ..ListRow::default() };
        ways = ways.child(list_row(t, &spec).key(format!("welcome-{key}")).hover(t.hov2).focusable(true).role("button").on_press_tx(move |s: &mut Shell, tx| s.welcome_add(Some(source), tx)));
    }
    let apps: usize = elsewhere.iter().map(|(_, n)| n).sum();
    let spec = ListRow {
        icon: Some(Icon::Plus),
        label: if servers + old + config + apps == 0 { "Add a server" } else { "Add another by hand" },
        note: "paste an address or a whole ssh command",
        ..ListRow::default()
    };
    ways = ways.child(list_row(t, &spec).key("welcome-add").hover(t.hov2).focusable(true).role("button").on_press_tx(|s: &mut Shell, tx| s.welcome_add(None, tx)));
    Content {
        line: Some(tour::welcome_line(servers, old, config, apps)),
        parts: vec![
            ways.mx(-8.0),
            spacer(),
            text("Nothing connects yet. No account, no cloud: your servers and keys stay on this computer.").font(theme::SMALL).color(t.l2).wrap(),
        ],
    }
}

// ---- 2. layout, 3. sidebar

fn layout(s: &Shell, t: &'static Theme) -> Content {
    Content {
        line: Some("Changes show at once, behind this window. Everything here is in Settings too.".into()),
        parts: vec![pictured(s, t, &["layout-tabs", "layout-view", "layout-compact"]), settings_rows(s, t, &["layout-tab-server", "layout-vitals", "layout-env-line"])],
    }
}

fn sidebar(s: &Shell, t: &'static Theme) -> Content {
    Content {
        line: None,
        parts: vec![pictured(s, t, &["layout-sidebar-side", "layout-server-list", "layout-server-icons"]), settings_rows(s, t, &["layout-server-load", "look-logo"])],
    }
}

/// The rows `keys` side by side, each a picture of the window as it picks, its name and its
/// options under it.
fn pictured(s: &Shell, t: &'static Theme, keys: &[&str]) -> El {
    let mut columns = row().w_full().gap(16.0).equal_columns(keys.len()).items_start();
    for r in keys.iter().filter_map(|key| catalog::row(key)).filter(|r| r.works()) {
        let now = tour::chosen(&s.settings, r.key).unwrap_or_default();
        let picture = match &s.screens.welcome.pictures {
            Some(p) => p.layout(t, r.key, &now),
            None => el().fill(t.page),
        };
        let picture = framed(picture.key(format!("welcome-{}-picture", r.key)).w_full().aspect_ratio(WINDOW_SHAPE), 1.0, t.hair);
        columns = columns.child(
            col()
                .key(format!("welcome-{}", r.key))
                .min_w(0.0)
                .gap(8.0)
                .child(picture)
                .child(text(column_name(r)).font(theme::TEXT_BOLD).color(t.l1).single_line().truncate().role("heading").level(2).mt(4.0))
                .child(options(s, t, r, &now).mx(-8.0)),
        );
    }
    columns
}

/// A picture column's name: Settings' "Side" is the sidebar's, under its heading there.
fn column_name(r: &Row) -> &'static str {
    match r.key {
        "layout-sidebar-side" => "Side",
        _ => r.label,
    }
}

/// The rows `keys` as Settings has them, in one group: those that show with the settings as
/// they are (naming the server on every tab, only while tabs run along the top).
fn settings_rows(s: &Shell, t: &'static Theme, keys: &[&str]) -> El {
    let rows = keys
        .iter()
        .filter_map(|key| catalog::row(key))
        .filter(|r| r.works() && r.shows(&s.settings, s.options.platform))
        .map(|r| {
            let (label, help) = match r.key {
                // Light has one theme, which "Light" is; Settings' help is about its own grid.
                "look-theme" => ("Dark theme", Some("Whenever the window is dark.".to_owned())),
                _ => (r.label, r.help(&s.settings, super::settings::keyboard(s))),
            };
            let words = RowText { label, help: help.as_deref(), later: None };
            form_row(t, words, control(s, t, r))
        })
        .collect();
    form_group(t, rows)
}

/// A row's control in the welcome, picking through it: a switch, a segmented control, a
/// pop-up button (its menu is [`menu`]), the dark themes, the accents.
fn control(s: &Shell, t: &'static Theme, r: &'static Row) -> El {
    let key = r.key;
    let now = tour::chosen(&s.settings, key).unwrap_or_default();
    let options = tour::options(key);
    let at = options.iter().position(|(v, _)| *v == now);
    match r.control {
        Control::Switch { .. } => {
            let on = now == "on";
            switch(t, r.label, on, true).key(format!("welcome-{key}")).on_press_tx(move |s: &mut Shell, tx| s.welcome_pick(key, if on { "off" } else { "on" }, tx))
        }
        Control::Choice { look: ChoiceLook::Popup, .. } => {
            let labels = r.choice_labels(&s.settings, super::settings::keyboard(s));
            let shown = at.and_then(|i| labels.get(i).copied()).unwrap_or(now.as_str());
            popup_button(t, r.label, shown, true).key(format!("welcome-{key}")).on_press_tx(move |s: &mut Shell, tx| {
                s.screens.welcome.menu = Some(key);
                tx.instant();
            })
        }
        Control::Choice { .. } => choice_segmented(s, t, r).key(format!("welcome-{key}")),
        Control::Special(Special::Theme) => {
            let names: Vec<&str> = options.iter().map(|(_, name)| *name).collect();
            segmented(t, r.label, &names, at, true, move |s: &mut Shell, i, tx| {
                if let Some(&(value, _)) = tour::options(key).get(i) {
                    s.welcome_pick(key, value, tx);
                }
            })
            .key(format!("welcome-{key}"))
        }
        Control::Special(Special::Accent) => accents(t, &now),
        _ => el(),
    }
}

/// The accents as round swatches, the one in use ringed (Settings' own colour is there).
fn accents(t: &Theme, now: &str) -> El {
    let mut swatches = row().key("welcome-look-accent").gap(8.0).items_center().role("radiogroup").label_static("Accent color").focus_group();
    for (i, &(name, value)) in looks::ACCENTS.iter().enumerate() {
        let picked = value == now;
        let mut dot = el()
            .key(format!("welcome-accent-{i}"))
            .size(18.0, 18.0)
            .radius(9.0)
            .fill(color(looks::accent_swatch(value)))
            .border(0.5, Color::BLACK.alpha(0.2))
            .role("radio")
            .checked(picked)
            .label(name)
            .help(name)
            .focusable(true)
            .focus_selected(picked)
            .on_press_tx(move |s: &mut Shell, tx| s.welcome_pick("look-accent", value, tx));
        if picked {
            dot = dot.outline(2.0, 2.0, t.l1);
        }
        swatches = swatches.child(dot);
    }
    swatches
}

/// The open pop-up control's menu, over everything.
pub fn menu(s: &Shell, t: &'static Theme) -> Option<El> {
    let state = &s.screens.welcome;
    let key = state.menu.filter(|_| state.open)?;
    let r = catalog::row(key)?;
    let labels = r.choice_labels(&s.settings, super::settings::keyboard(s));
    let now = tour::chosen(&s.settings, key);
    let options = tour::options(key);
    let selected = options.iter().position(|(v, _)| Some(*v) == now.as_deref());
    Some(
        popup_menu(
            t,
            &format!("welcome-{key}"),
            &labels,
            selected,
            move |s: &mut Shell, i, tx| {
                if let Some(&(value, _)) = tour::options(key).get(i) {
                    s.welcome_pick(key, value, tx);
                }
            },
            |s: &mut Shell, tx| {
                s.screens.welcome.menu = None;
                tx.instant();
            },
        )
        .key("welcome-menu"),
    )
}

// ---- 4. how it looks

fn look(s: &Shell, t: &'static Theme) -> Content {
    let store = &s.settings;
    let now = Appearance::of(store.get());
    let mut cards = row().key("welcome-look").w_full().gap(12.0).equal_columns(3).role("radiogroup").label_static("Appearance").focus_group();
    for (appearance, value, label) in Appearance::CHOICES {
        let picked = appearance == now;
        let picture = s.screens.welcome.pictures.as_ref().map(|p| p.look(value)).unwrap_or_else(|| el().fill(t.page));
        cards = cards.child(
            col()
                .key(format!("welcome-look-{value}"))
                .min_w(0.0)
                .gap(8.0)
                .role("radio")
                .checked(picked)
                .label(label)
                .focusable(true)
                .focus_selected(picked)
                .on_press_tx(move |s: &mut Shell, tx| s.welcome_pick("look-appearance", value, tx))
                .child(framed(picture.w_full().aspect_ratio(1.6), if picked { 2.0 } else { 1.0 }, if picked { t.blue } else { t.hair }))
                .child(text(label).font(if picked { theme::TEXT_BOLD } else { theme::TEXT }).color(if picked { t.l1 } else { t.l2on }).single_line().truncate().a11y_hidden()),
        );
    }
    Content { line: None, parts: vec![cards, settings_rows(s, t, &["look-theme", "look-accent", "look-size", "look-animations"])] }
}

/// A picture with rounded corners and a line round it, drawn over its edge.
fn framed(picture: El, width: f32, color: rune::Color) -> El {
    let ring = el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(theme::RADIUS_GROUP).border(width, color);
    picture.radius(theme::RADIUS_GROUP).clip_rounded(theme::RADIUS_GROUP).child(ring).a11y_hidden()
}

/// A choice row of Settings as a segmented control, picking through the welcome.
fn choice_segmented(s: &Shell, t: &Theme, r: &'static Row) -> El {
    let Control::Choice { options, get, .. } = r.control else { return el() };
    let labels = r.choice_labels(&s.settings, super::settings::keyboard(s));
    let now = get(&s.settings);
    let key = r.key;
    segmented(t, r.label, &labels, options.iter().position(|(v, _)| *v == now), true, move |s: &mut Shell, i, tx| s.welcome_pick(key, options[i].0, tx))
}

// ---- 3. in the terminal

fn terminal(s: &Shell, t: &'static Theme) -> Content {
    let mut both = row().w_full().gap(16.0).equal_columns(2).items_start();
    for r in Page::Terminal.choices() {
        let now = tour::chosen(&s.settings, r.key).unwrap_or_default();
        let line = match r.key {
            "term-copy" => tour::copy_line(&now, s.options.mac_keys),
            _ => r.help(&s.settings, super::settings::keyboard(s)).unwrap_or_default(),
        };
        let picture = match &s.screens.welcome.pictures {
            Some(p) => p.terminal(t, r.key, &now, s.options.mac_keys),
            None => el().fill(t.page),
        };
        let [w, h] = pictures::TERMINAL_SIZE;
        let picture = framed(picture.key(format!("welcome-{}-picture", r.key)).w_full().aspect_ratio(w / h), 1.0, t.hair);
        both = both.child(
            col()
                .key(format!("welcome-{}", r.key))
                .min_w(0.0)
                .gap(8.0)
                .child(picture)
                .child(text(r.label).font(theme::TEXT_BOLD).color(t.l1).single_line().role("heading").level(2).mt(4.0))
                .child(options(s, t, r, &now).mx(-8.0))
                .child(text(line).key(format!("welcome-{}-line", r.key)).font(theme::NOTE).color(t.l2).wrap()),
        );
    }
    Content { line: Some("On a server with tmux, your shell keeps running while you're away.".into()), parts: vec![both] }
}

/// A choice row's options, one under another: the picked one marked.
fn options(s: &Shell, t: &Theme, r: &'static Row, now: &str) -> El {
    let Control::Choice { options, .. } = r.control else { return el() };
    let labels = r.choice_labels(&s.settings, super::settings::keyboard(s));
    let mut list = col().w_full().gap(2.0).role("radiogroup").label(r.label).focus_group();
    for (i, (value, _)) in options.iter().enumerate() {
        let picked = *value == now;
        let label = labels.get(i).copied().unwrap_or(value);
        let key = r.key;
        let mut option = row()
            .key(format!("welcome-{key}-{value}"))
            .w_full()
            .h(theme::ROW - 2.0)
            .px(8.0)
            .gap(8.0)
            .radius(theme::RADIUS_ROW)
            .items_center()
            .role("radio")
            .checked(picked)
            .label(label)
            .focusable(true)
            .focus_selected(picked)
            .press(t.down)
            .on_press_tx(move |s: &mut Shell, tx| s.welcome_pick(key, value, tx))
            .child(text(label).font(theme::TEXT).color(t.l1).single_line().truncate().grow(1.0).shrink().min_w(0.0).a11y_hidden());
        option = if picked { option.fill(t.selrail).child(icon(Icon::Check, theme::ICON, t.blue)) } else { option.hover(t.hov2) };
        list = list.child(option);
    }
    list
}

// ---- 4. three things people miss

fn tips(s: &Shell, t: &Theme) -> Content {
    let mut list = col().key("welcome-tips").w_full().gap(4.0);
    for tip in TIPS {
        let sign = match tip.id {
            "palette" => Icon::Search,
            "files" => Icon::Folder,
            _ => Icon::Agent,
        };
        let mut line = row()
            .key(format!("welcome-tip-{}", tip.id))
            .w_full()
            .py(10.0)
            .gap(14.0)
            .items_start()
            .role("group")
            .label(tip.title)
            .child(el().size(32.0, 32.0).radius(theme::RADIUS).fill(t.hov2).items_center().justify_center().child(icon(sign, theme::ICON, t.l2on)))
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(tip.title).font(theme::TEXT_BOLD).color(t.l1).single_line())
                    .child(text(tip.body).font(theme::NOTE).color(t.l2on).wrap()),
            );
        if let Some(keys) = shortcut_label(tip.keys, s.options.mac_keys, &s.options.shortcuts) {
            line = line.child(key_cap(t, &keys, false).mt(6.0));
        }
        list = list.child(line);
    }
    Content { line: Some("Each has a shortcut, and the palette finds every other one.".into()), parts: vec![list] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_its_place() {
        // The catalog's rows exist and work (sshee_app::tour's test); here, every one is drawn.
        for page in PAGES {
            for key in page.rows() {
                assert!(sshee_app::settings_store::catalog::row(key).is_some(), "{key}");
            }
        }
    }
}
