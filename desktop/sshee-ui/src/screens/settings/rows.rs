//! A section's rows, drawn from the catalogue: each row's words at the left and its control
//! at the right, in boxes, with a small heading where the catalogue groups them. A row 1.0
//! doesn't act on yet keeps its place, dimmed, with its one line saying so.

use rune::tok::TextStyle;
use rune::{col, row, text, Color, El, Tx};
use sshee_app::model::{HighlightColor, HighlightRule, Platform};
use sshee_app::settings_store::catalog::{self, Action, ChoiceLook, Control, Row, Section, Special, Status};
use sshee_app::settings_store::schemes::{self, Resolved};
use sshee_app::workbench::Tor;
use sshee_term::engine::Color as TermColor;

use super::{keyboard, TorCommand};
use crate::icons::{icon, logo, Icon};
use crate::kit::field::{code_input, field, input};
use crate::kit::form::{form_group, form_heading, form_note, form_row, form_row_stacked, RowText};
use crate::kit::popup::popup_button;
use crate::kit::push::{icon_button, push_button, toggle_button, Push};
use crate::kit::segmented::segmented;
use crate::kit::slider::{slider, Range};
use crate::kit::switch::switch;
use crate::shell::Shell;
use crate::terminal::palette::{Palette, Scheme};
use crate::theme::{self, Theme};

/// The key of a row's control: what a pop-up menu hangs under, and what a test clicks.
pub(super) fn control_key(row: &str) -> String {
    format!("{row}-control")
}

/// The six highlight colours, in 0.10's order, with their place in a terminal's palette.
const WORD_COLORS: [(HighlightColor, &str, u8); 6] = [
    (HighlightColor::Red, "red", 1),
    (HighlightColor::Yellow, "yellow", 3),
    (HighlightColor::Green, "green", 2),
    (HighlightColor::Blue, "blue", 4),
    (HighlightColor::Magenta, "magenta", 5),
    (HighlightColor::Cyan, "cyan", 6),
];

/// The palette the terminals draw with now, in a window that is dark or isn't.
fn palette(s: &Shell, t: &Theme) -> Palette {
    Palette::of(s.options.pane.scheme, t.dark)
}

/// The control of a row whose value is the store's.
fn control(s: &Shell, t: &'static Theme, r: &'static Row) -> El {
    let on = r.works();
    let store = &s.settings;
    let key = control_key(r.key);
    match r.control {
        Control::Switch { get, set } => {
            let control = switch(t, r.label, get(store), on).key(key);
            if !on {
                return control;
            }
            control.on_press_tx(move |s: &mut Shell, tx| {
                let now = get(&s.settings);
                s.change_settings(tx, |st| set(st, !now));
            })
        }
        Control::Choice { look: ChoiceLook::Segmented, options, get, set } => {
            let labels = r.choice_labels(keyboard(s));
            let now = get(store);
            let selected = options.iter().position(|(value, _)| *value == now);
            segmented(t, r.label, &labels, selected, on, move |s: &mut Shell, i, tx| s.change_settings(tx, |st| set(st, options[i].0))).key(key)
        }
        Control::Choice { look: ChoiceLook::Popup, options, get, .. } => {
            let now = get(store);
            let labels = r.choice_labels(keyboard(s));
            let shown = options.iter().position(|(value, _)| *value == now).and_then(|at| labels.get(at).copied()).unwrap_or(now.as_str());
            let row_key = r.key;
            let button = popup_button(t, r.label, shown, on).key(key);
            if on {
                button.on_press_tx(move |s: &mut Shell, tx| {
                    s.screens.settings.menu = Some(row_key);
                    tx.instant();
                })
            } else {
                button
            }
        }
        Control::Slider { min, max, step, get, set, show } => {
            let value = get(store);
            slider(t, r.label, Range { min, max, step }, value, &show(value), on, move |s: &mut Shell, v, tx| s.change_settings(tx, |st| set(st, v))).key(key)
        }
        Control::Text { placeholder, max, get, set } => {
            let row_key = r.key;
            let fallback = rune::TextEdit::new(get(store));
            let edit = s.screens.settings.texts.get(row_key).unwrap_or(&fallback);
            let typing = input(t, edit, placeholder).key(key).on_edit(move |s: &mut Shell| s.screens.settings.texts.entry(row_key).or_default()).on_input_change(
                move |s: &mut Shell, tx| {
                    // No longer than the row allows, counted in characters.
                    let typed: String = s.screens.settings.texts.get(row_key).map(|e| e.text.chars().take(max).collect()).unwrap_or_default();
                    s.change_settings(tx, |st| set(st, &typed));
                },
            );
            field(t, typing, false).w(220.0)
        }
        Control::Toggles { options, get, toggle } => {
            let now = get(store);
            let mut chips = row().key(key).gap(6.0).flex_wrap().justify_end();
            for (i, (value, label)) in options.iter().enumerate() {
                let picked = now.iter().any(|v| v == value);
                let mut chip = toggle_button(t, label, picked, on).key(format!("toggle-{i}"));
                if on {
                    chip = chip.on_press_tx(move |s: &mut Shell, tx| s.change_settings(tx, |st| toggle(st, value)));
                }
                chips = chips.child(chip);
            }
            chips
        }
        Control::Buttons(buttons) => {
            let mut all = row().key(key).gap(6.0).flex_wrap().justify_end();
            for (i, (label, action)) in buttons.iter().enumerate() {
                let enabled = on && *action != Action::Later;
                let mut button = push_button(t, label, Push::Quiet, enabled).key(format!("button-{i}"));
                match (enabled, action) {
                    (true, Action::Open(url)) => {
                        button = button.on_press_tx(move |_: &mut Shell, tx: &mut Tx| {
                            // A link that doesn't open says nothing: there is nowhere to say it yet.
                            let _ = tx.open_url(url);
                        });
                    }
                    (true, Action::AddServer) => button = button.on_press_tx(|s: &mut Shell, tx: &mut Tx| s.open_add_server(tx)),
                    _ => {}
                }
                all = all.child(button);
            }
            all
        }
        Control::Special(special) => special_control(s, t, r, special),
    }
}

/// A round swatch of one colour, ringed when it is the one in use.
fn swatch(t: &Theme, color: Color, size: f32, picked: bool) -> El {
    let mut dot = rune::el().size(size, size).radius(size / 2.0).fill(color).border(0.5, Color::BLACK.alpha(0.2));
    if picked {
        dot = dot.outline(2.0, 2.0, t.l1);
    }
    dot
}

fn special_control(s: &Shell, t: &'static Theme, r: &'static Row, special: Special) -> El {
    let st = s.settings.get();
    let key = control_key(r.key);
    match special {
        Special::SidebarShown => switch(t, r.label, s.bench.sidebar_shown(), true).key(key).on_press_tx(|s: &mut Shell, tx| s.toggle_sidebar(tx)),
        Special::Theme => popup_button(t, r.label, schemes::theme_name(&st.theme, &st.custom_themes), false).key(key),
        Special::Accent => {
            let mut swatches = row().key(key).gap(8.0).items_center().role("group").label(r.label).layer_opacity(Theme::DISABLED);
            for (name, value) in schemes::ACCENTS {
                if let Some(color) = rune::color::hex(value) {
                    swatches = swatches.child(swatch(t, color, 16.0, st.accent.eq_ignore_ascii_case(value)).label(name));
                }
            }
            swatches
        }
        Special::Tips => {
            let off = st.hints_seen.iter().any(|h| h == "*");
            switch(t, r.label, !off, false).key(key)
        }
        Special::TorSwitch => {
            let on = s.bench.tor() != Tor::Off;
            switch(t, r.label, on, true).key(key).on_press_tx(move |s: &mut Shell, tx| {
                s.backend_mut().tor(if on { TorCommand::Off } else { TorCommand::On });
                tx.instant();
            })
        }
        // The wide ones are drawn by `wide`.
        _ => rune::el(),
    }
}

/// A terminal scheme as a small picture: its background, five of its colours, its name.
fn scheme_card(t: &'static Theme, i: usize, id: &'static str, name: &'static str, colors: &Palette, picked: bool, enabled: bool) -> El {
    let mut dots = row().gap(3.0).a11y_hidden();
    for index in [1, 2, 3, 4, 5] {
        dots = dots.child(rune::el().size(8.0, 8.0).radius(4.0).fill(colors.color(TermColor::Indexed(index))));
    }
    let mut card = row()
        .key(format!("scheme-{i}"))
        .h(34.0)
        .px(10.0)
        .gap(9.0)
        .radius(theme::RADIUS)
        .fill(colors.bg())
        .border(1.0, t.hair)
        .items_center()
        .role("radio")
        .checked(picked)
        .label(name)
        .focusable(enabled)
        .focus_selected(picked)
        .disabled(!enabled)
        .child(dots)
        .child(text(name).font(theme::CONTROL_TEXT).color(colors.fg()).single_line().truncate().grow(1.0).shrink().a11y_hidden());
    if picked {
        // The one in use: ringed in the selection's blue.
        card = card.outline(2.0, 0.0, t.blue);
    }
    if enabled {
        card.on_press_tx(move |s: &mut Shell, tx| s.change_settings(tx, |st| st.set(|v| v.terminal_scheme = id.to_owned())))
    } else {
        card.layer_opacity(Theme::DISABLED)
    }
}

/// The terminal's colour schemes, three to a line.
fn schemes_grid(s: &Shell, t: &'static Theme) -> El {
    let st = s.settings.get();
    let own = schemes::resolve(schemes::OWN, st.terminal_colors.as_ref());
    let mut grid = col().key(control_key("term-scheme")).w_full().gap(8.0).role("radiogroup").label_static("Color scheme").focus_group();
    let choices = schemes::scheme_choices();
    for (line, three) in choices.chunks(3).enumerate() {
        let mut cards = row().w_full().gap(8.0).equal_columns(3);
        for (at, (id, name)) in three.iter().enumerate() {
            let scheme = match schemes::resolve(id, st.terminal_colors.as_ref()) {
                Resolved::MatchApp => Scheme::Fresh,
                Resolved::Fixed(fixed) => Scheme::Fixed(fixed.colors),
                Resolved::Own { background, foreground } => Scheme::Own { background, foreground },
            };
            // "Your own" needs its two colours: picking them comes with the colour picker.
            let enabled = *id != schemes::OWN || own != Resolved::MatchApp;
            let card = scheme_card(t, line * 3 + at, id, name, &Palette::of(scheme, t.dark), st.terminal_scheme == *id, enabled);
            cards = cards.child(card);
        }
        grid = grid.child(cards);
    }
    if own == Resolved::MatchApp {
        grid = grid.child(text("Your own background and text colors come with the color picker.").font(theme::HELP).color(t.l2).wrap());
    }
    grid
}

/// A few lines of a terminal in the colours, size, line height and cursor chosen.
fn terminal_preview(s: &Shell, t: &'static Theme) -> El {
    let st = s.settings.get();
    let p = palette(s, t);
    let size = s.options.pane.font_size;
    let line = (size * s.options.pane.line_height * 1.2).round();
    let font = TextStyle::new(size, 400.0).mono().lh(line);
    let ink = |index: u8| p.color(TermColor::Indexed(index));
    let piece = |words: &str, color: Color| text(words).font(font).color(color).single_line();
    let prompt = || row().child(piece("deploy@web-7731", ink(2))).child(piece(":", p.fg())).child(piece("~", ink(4))).child(piece("$ ", p.fg()));
    let cell = rune::text::advance("0", font);
    let cursor = match st.cursor_style.as_str() {
        "block" => rune::el().size(cell, line - 2.0),
        "underline" => rune::el().size(cell, 2.0).mt(line - 4.0),
        _ => rune::el().size(2.0, line - 2.0),
    };
    col()
        .key("term-preview")
        .w_full()
        .px(14.0)
        .py(12.0)
        .radius(theme::RADIUS_GROUP)
        .fill(p.bg())
        .border(1.0, t.hair2)
        .a11y_hidden()
        .child(prompt().child(piece("docker compose ps", p.fg())))
        .child(row().child(piece("shop-api-1   ", p.fg())).child(piece("Up 3 days (healthy)", ink(2))))
        .child(
            row()
                .child(piece("23:10:05 ", ink(8)))
                .child(piece("WARN", ink(3)))
                .child(piece(" slow query ", p.fg()))
                .child(piece("(412ms)", ink(5)))
                .child(piece(" retry", ink(6))),
        )
        .child(prompt().child(cursor.fill(crate::terminal::palette::rgb(p.cursor))))
}

/// Words of your own to highlight: the ones there are, and a field for one more.
fn highlight_rules(s: &Shell, t: &'static Theme) -> El {
    let state = &s.screens.settings;
    let p = palette(s, t);
    let color_of = |color: &HighlightColor| WORD_COLORS.iter().find(|(c, _, _)| c == color).map_or(1, |(_, _, index)| *index);
    let mut body = col().key(control_key("term-hl-words")).w_full().gap(8.0);
    let rules = &s.settings.get().highlight_rules;
    if !rules.is_empty() {
        let mut list = col().w_full().padding(5.0).gap(2.0).radius(theme::RADIUS).fill(p.bg()).border(1.0, t.hair2);
        for (i, rule) in rules.iter().enumerate() {
            let ink = p.color(TermColor::Indexed(color_of(&rule.color)));
            let id = rule.id.clone();
            list = list.child(
                row()
                    .key(format!("word-{i}"))
                    .w_full()
                    .h(theme::FORM_CONTROL)
                    .px(7.0)
                    .gap(8.0)
                    .items_center()
                    .child(swatch(t, ink, 10.0, false))
                    .child(text(rule.pattern.clone()).font(theme::NOTE.mono()).color(ink).single_line().truncate().grow(1.0).shrink())
                    .child(icon_button(t, Icon::Close, "Remove this word", true).key("remove").on_press_tx(move |s: &mut Shell, tx| {
                        s.change_settings(tx, |st| st.set(|v| v.highlight_rules.retain(|r| r.id != id)));
                    })),
            );
        }
        body = body.child(list);
    }
    let draft = state.word.text.trim();
    let broken = !draft.is_empty() && sshee_term::highlight::compile_rule(draft).is_none();
    // A pattern: in the terminal's font, red while it doesn't work.
    let typing = code_input(t, &state.word, "nginx, or /took \\d+ms/").key("word-new").on_edit(|s: &mut Shell| &mut s.screens.settings.word).on_submit(add_word);
    let field = field(t, typing, broken);
    let mut colors = row().gap(6.0).items_center().role("radiogroup").label_static("Color").focus_group();
    for (i, (_, name, index)) in WORD_COLORS.iter().enumerate() {
        let picked = state.word_color == i;
        colors = colors.child(
            swatch(t, p.color(TermColor::Indexed(*index)), 14.0, picked)
                .key(format!("word-color-{i}"))
                .role("radio")
                .checked(picked)
                .label(*name)
                .focusable(true)
                .focus_selected(picked)
                .on_press_tx(move |s: &mut Shell, tx| {
                    s.screens.settings.word_color = i;
                    tx.instant();
                }),
        );
    }
    let can_add = !draft.is_empty() && !broken;
    let mut add = push_button(t, "Add", Push::Quiet, can_add).key("word-add");
    if can_add {
        add = add.on_press_tx(add_word);
    }
    body = body.child(row().w_full().gap(10.0).items_center().child(field.grow(1.0).shrink().min_w(0.0)).child(colors).child(add));
    if broken {
        body = body.child(text("That regular expression doesn't work. Check the brackets and slashes.").key("word-broken").font(theme::HELP).color(t.red).wrap().role("alert"));
    }
    body
}

/// Adds the word in the field as a rule, if it is one.
fn add_word(s: &mut Shell, tx: &mut Tx) {
    let pattern = s.screens.settings.word.text.trim().to_owned();
    if pattern.is_empty() || sshee_term::highlight::compile_rule(&pattern).is_none() {
        return;
    }
    let color = WORD_COLORS[s.screens.settings.word_color.min(WORD_COLORS.len() - 1)].0.clone();
    // An id of its own among the rules there are: 0.10 made a random one; a count does here.
    let taken: Vec<String> = s.settings.get().highlight_rules.iter().map(|r| r.id.clone()).collect();
    let id = (1..).map(|n| format!("w{n}")).find(|id| !taken.contains(id)).unwrap_or_default();
    s.change_settings(tx, |st| st.set(|v| v.highlight_rules.push(HighlightRule { id, pattern, color, ..HighlightRule::default() })));
    s.screens.settings.word.clear();
}

/// What SSHeesh itself reaches out to. Kept short on purpose.
fn network(s: &Shell, t: &'static Theme) -> El {
    let tor = s.bench.tor() != Tor::Off;
    let item = |glyph: Icon, title: &str, body: &str| {
        row()
            .w_full()
            .px(12.0)
            .py(9.0)
            .gap(10.0)
            .child(icon(glyph, theme::ICON, t.l2).mt(1.0))
            .child(col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(text(title).font(theme::TEXT).color(t.l1).wrap()).child(text(body).font(theme::HELP).color(t.l2).wrap()))
    };
    form_group(
        t,
        vec![
            item(Icon::Server, "Your servers", if tor { "Only the ones you open, through your own ssh and Tor." } else { "Only the ones you open, through your own ssh." }),
            item(Icon::Tor, "The Tor network", if tor { "While Tor is on: your servers are reached through it." } else { "Only while Tor is on." }),
            item(Icon::Globe, "GitHub, for updates", "Not yet: the preview doesn't check for updates."),
            row().w_full().px(12.0).py(9.0).child(
                text("That's all. No account, no analytics, no crash reports. Your server list and settings stay on this computer.")
                    .font(theme::HELP)
                    .color(t.l2)
                    .wrap()
                    .grow(1.0)
                    .shrink(),
            ),
        ],
    )
    .key(control_key("privacy-network"))
}

/// A card with the logo: the version, and for Updates the check that isn't there yet.
fn version_card(t: &'static Theme, key: &str, mark: f32, lines: (&str, &str), button: Option<&str>) -> El {
    let mut card = row()
        .key(key.to_owned())
        .w_full()
        .px(14.0)
        .py(14.0)
        .gap(12.0)
        .radius(theme::RADIUS_GROUP)
        .fill(t.group)
        .border(1.0, t.hair2)
        .items_center()
        .child(logo(mark))
        .child(
            col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(text(lines.0).font(theme::TEXT_BOLD).color(t.l1).single_line()).child(text(lines.1).font(theme::NOTE).color(t.l2).wrap()),
        );
    if let Some(label) = button {
        card = card.child(push_button(t, label, Push::Quiet, false));
    }
    card
}

/// Tor's state under its switch, and what can be done about it.
fn tor_state(s: &Shell, t: &'static Theme) -> Option<El> {
    let (words, button) = match s.bench.tor() {
        Tor::Off => return None,
        Tor::Starting => ("Tor is starting…", None),
        Tor::On { .. } => ("Tor is on. If typing feels slow on a server, pick new routes.", Some(("New routes", TorCommand::NewRoutes))),
        Tor::Failed => ("Tor can't start. Nothing connects until it does; it keeps trying.", Some(("Start fresh", TorCommand::StartAfresh))),
    };
    let mut line = row()
        .key("tor-state")
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(10.0)
        .radius(theme::RADIUS_GROUP)
        .fill(t.tor_tile())
        .items_center()
        .child(icon(Icon::Tor, theme::ICON, t.green))
        .child(text(words).font(theme::NOTE).color(t.l1).wrap().grow(1.0).shrink());
    if let Some((label, command)) = button {
        line = line.child(push_button(t, label, Push::Quiet, true).key("tor-action").on_press_tx(move |s: &mut Shell, tx| {
            s.backend_mut().tor(command);
            tx.instant();
        }));
    }
    Some(line)
}

/// A row too wide for the right-hand side: drawn under its words, or by itself.
fn wide(s: &Shell, t: &'static Theme, r: &'static Row) -> Option<El> {
    let Control::Special(special) = r.control else { return None };
    Some(match special {
        Special::Schemes => schemes_grid(s, t),
        Special::HighlightRules => highlight_rules(s, t),
        _ => return None,
    })
}

/// A row that is a card or a picture of its own, outside the boxes.
fn alone(s: &Shell, t: &'static Theme, r: &'static Row) -> Option<El> {
    let Control::Special(special) = r.control else { return None };
    let version = env!("CARGO_PKG_VERSION");
    Some(match special {
        Special::TerminalPreview => terminal_preview(s, t),
        Special::Network => col().w_full().gap(6.0).child(form_heading(t, r.label, None)).child(network(s, t)),
        Special::UpdateCard => {
            version_card(t, "updates-card", 40.0, (&format!("SSHeesh Preview {version}"), "Early preview build. Checking for updates comes later."), Some("Check now"))
        }
        Special::AboutCard => col()
            .w_full()
            .gap(10.0)
            .child(version_card(t, "about-card", 56.0, ("SSHeesh", &format!("Preview {version} · early preview, expect rough edges")), None))
            .child(
                text("Servers, terminals and files that don't fall over. Free, with open source code under the CC BY-NC-SA 4.0 license.")
                    .font(theme::NOTE)
                    .color(t.l2on)
                    .wrap()
                    .px(12.0),
            ),
        _ => return None,
    })
}

/// One row in its box.
fn boxed(s: &Shell, t: &'static Theme, r: &'static Row) -> El {
    let help = r.help(&s.settings, keyboard(s));
    let later = match r.status {
        Status::Works => None,
        Status::Later(why) => Some(why),
    };
    let words = RowText { label: r.label, help: help.as_deref(), later };
    let mut e = match wide(s, t, r) {
        Some(control) => form_row_stacked(t, words, control),
        None => form_row(t, words, control(s, t, r)),
    };
    if s.screens.settings.found.as_deref() == Some(r.key) {
        // Where a search led.
        e = e.fill(t.hov2);
    }
    e.key(r.key)
}

/// Everything a section shows, top to bottom.
pub(super) fn section(s: &Shell, t: &'static Theme, section: Section) -> Vec<El> {
    let mut out = Vec::new();
    // The privacy settings were turned on as a precaution: Privacy says so first.
    if let (Section::Privacy, Some(notice)) = (section, s.settings.notice()) {
        out.push(form_note(t, &notice.headline()).key("privacy-why"));
    }
    let mut group: Vec<El> = Vec::new();
    let mut heading: Option<&str> = None;
    let flush = |group: &mut Vec<El>, out: &mut Vec<El>| {
        if !group.is_empty() {
            out.push(form_group(t, std::mem::take(group)));
        }
    };
    for r in catalog::section_rows(section, &s.settings, Platform::current()) {
        if let Some(card) = alone(s, t, r) {
            flush(&mut group, &mut out);
            out.push(card.key(r.key));
            continue;
        }
        if r.group != heading {
            flush(&mut group, &mut out);
            heading = r.group;
            if let Some(title) = heading {
                out.push(form_heading(t, title, None));
            }
        }
        group.push(boxed(s, t, r));
        // What belongs right under a row, outside its box.
        let under = match r.key {
            "privacy-streamer" if s.streaming() => Some(form_note(
                t,
                "Names are hidden as well as SSHeesh can spot them. Glance over your screen before you go live: a name inside a file or a log line it has never seen can still slip through.",
            )),
            "tor-switch" => tor_state(s, t),
            _ => None,
        };
        if let Some(under) = under {
            flush(&mut group, &mut out);
            out.push(under);
        }
    }
    flush(&mut group, &mut out);
    out
}
