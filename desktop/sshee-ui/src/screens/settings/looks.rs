//! Settings → Appearance's themes: the themes as small pictures, the theme for each side while
//! following the system, the accent, and the editor for a theme of your own with its colour
//! picker (a port of 0.10's Appearance section, `ThemeEditor.tsx` and `ColorPicker.tsx`). The
//! rules are `sshee_app::settings_store::looks`; turning a theme into the window's tokens is
//! `crate::looks`.

use std::f32::consts::FRAC_PI_2;

use rune::{col, row, text, Color, El, Key, KeyEvent, Phase, TextEdit, Tx};
use sshee_app::settings_store::looks::{self, Hsv, Resolved, COLOR_KEYS};
use sshee_app::settings_store::schemes::{self, parse_hex};

use crate::icons::Icon;
use crate::kit::field::{code_input, field, input};
use crate::kit::form::{form_group, form_heading, form_row, RowText};
use crate::kit::popup::{popup_button, popup_menu};
use crate::kit::push::{icon_button, push_button, Push};
use crate::looks::color;
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The colour being picked: a theme colour's key, or "accent".
pub struct Picking {
    pub key: &'static str,
    /// Kept apart from the colour, so a grey keeps the hue the slider is at.
    pub hsv: Hsv,
    pub alpha: f64,
    /// The hex field, while it is typed in.
    pub draft: TextEdit,
}

/// Appearance's own state.
#[derive(Default)]
pub struct Looks {
    /// The theme of yours whose colours are being edited.
    pub editing: Option<String>,
    /// Its name, while it is typed.
    pub name: TextEdit,
    /// "Delete …?" is asked.
    pub deleting: bool,
    /// "Copy to share" was pressed.
    pub copied: bool,
    /// Import is open, with what was pasted and why it didn't read.
    pub importing: bool,
    pub import: TextEdit,
    pub import_error: Option<String>,
    pub picking: Option<Picking>,
}

/// The theme on screen now.
fn on_screen(s: &Shell) -> Resolved {
    let st = s.settings.get();
    looks::on_screen(st, s.dark(s.screens.settings.system_dark.get()))
}

// ---------------------------------------------------------------- the themes

/// The themes as small pictures, four to a line, and what can be done with them.
pub(super) fn theme_grid(s: &Shell, t: &'static Theme) -> El {
    let st = s.settings.get();
    let now = on_screen(s);
    let accent = color(looks::accent(st).unwrap_or(looks::DESIGN_BLUE));
    let all = looks::all(st);
    let mut grid = col().key(super::rows::control_key("look-theme")).w_full().gap(10.0).role("radiogroup").label_static("Theme").focus_group();
    for (line, four) in all.chunks(4).enumerate() {
        let mut cards = row().w_full().gap(10.0).equal_columns(4);
        for (at, theme) in four.iter().enumerate() {
            cards = cards.child(theme_card(t, line * 4 + at, theme, theme.id == now.id, accent));
        }
        grid = grid.child(cards);
    }
    let mut buttons = row().gap(8.0).items_center();
    if st.custom_themes.iter().any(|c| c.id == now.id) {
        let id = now.id.clone();
        buttons = buttons.child(push_button(t, "Edit colors", Push::Quiet, true).key("theme-edit").on_press_tx(move |s: &mut Shell, tx| open_editor(s, &id, tx)));
    }
    buttons = buttons
        .child(push_button(t, "Duplicate and edit", Push::Quiet, true).key("theme-duplicate").on_press_tx(|s: &mut Shell, tx| {
            let from = on_screen(s).id;
            let mut id = String::new();
            s.change_settings(tx, |st| id = looks::duplicate(st, &from));
            s.screens.settings.looks.importing = false;
            open_editor(s, &id, tx);
        }))
        .child(push_button(t, "Import", Push::Quiet, true).key("theme-import").on_press_tx(|s: &mut Shell, tx| {
            let l = &mut s.screens.settings.looks;
            l.importing = !l.importing;
            l.import_error = None;
            tx.instant();
        }));
    grid = grid.child(buttons);
    if s.screens.settings.looks.importing {
        grid = grid.child(import_box(s, t));
    }
    grid
}

/// One theme: its window, sidebar and pane in its colours, the accent and two lines of text.
fn theme_card(t: &'static Theme, i: usize, theme: &Resolved, picked: bool, accent: Color) -> El {
    let c = |k: usize| color(theme.colors[k] | 0xff);
    let pane = color(looks::over(theme.colors[1], theme.colors[0] | 0xff));
    let strip = if theme.light { Color::WHITE.alpha(0.6) } else { Color::WHITE.alpha(0.05) };
    let picture = row()
        .w_full()
        .h(64.0)
        .padding(8.0)
        .gap(8.0)
        .radius(theme::RADIUS_GROUP)
        .fill(c(0))
        .border(if picked { 2.0 } else { 1.0 }, if picked { t.blue } else { t.hair })
        .child(rune::el().w(28.0).h_full().radius(4.0).fill(strip))
        .child(
            col()
                .grow(1.0)
                .h_full()
                .radius(5.0)
                .fill(pane)
                .padding(8.0)
                .gap(6.0)
                .child(rune::el().w(36.0).h(4.0).radius(2.0).fill(accent))
                .child(rune::el().w(54.0).h(4.0).radius(2.0).fill(c(4).alpha(0.4)))
                .child(rune::el().w(28.0).h(4.0).radius(2.0).fill(c(4).alpha(0.25))),
        )
        .a11y_hidden();
    let id = theme.id.clone();
    col()
        .key(format!("theme-{i}"))
        .min_w(0.0)
        .gap(6.0)
        .role("radio")
        .checked(picked)
        .label(theme.name.clone())
        .focusable(true)
        .focus_selected(picked)
        .on_press_tx(move |s: &mut Shell, tx| s.change_settings(tx, |st| looks::pick(st, &id)))
        // The arrow keys flip through them, each shown at once (0.10's).
        .on_key(move |s: &mut Shell, ev: &KeyEvent, tx| {
            let step = match ev.key {
                Key::Right | Key::Down => 1,
                Key::Left | Key::Up => -1,
                _ => return false,
            };
            let all = looks::all(s.settings.get());
            let next = &all[(i as i32 + step).rem_euclid(all.len() as i32) as usize].id;
            let next = next.clone();
            s.change_settings(tx, |st| looks::pick(st, &next));
            let at = looks::all(s.settings.get()).iter().position(|t| t.id == next).unwrap_or(0);
            tx.focus(format!("theme-{at}"), true);
            true
        })
        .child(picture)
        .child(text(theme.name.clone()).font(if picked { theme::TEXT_BOLD } else { theme::NOTE }).color(if picked { t.l1 } else { t.l2 }).single_line().truncate().a11y_hidden())
}

/// Paste a theme someone shared ("Copy to share", on their computer) to add it.
fn import_box(s: &Shell, t: &'static Theme) -> El {
    let l = &s.screens.settings.looks;
    let pasted = rune::text_input(&l.import, theme::CODE)
        .key("theme-import-text")
        .min_h(90.0)
        .w_full()
        .color(t.l1)
        .placeholder("Paste a theme here. It starts with { \"ssheesh\": \"theme\"", t.l2)
        .label("Theme to import")
        .on_edit(|s: &mut Shell| &mut s.screens.settings.looks.import)
        .on_input_change(|s: &mut Shell, tx| {
            s.screens.settings.looks.import_error = None;
            tx.instant();
        });
    let mut out = col()
        .w_full()
        .gap(8.0)
        .padding(10.0)
        .radius(theme::RADIUS_GROUP)
        .fill(t.hov)
        .child(rune::el().w_full().padding(8.0).radius(theme::RADIUS_ROW).fill(t.field()).border(1.0, t.hair).child(pasted));
    if let Some(why) = &l.import_error {
        out = out.child(text(why.clone()).key("theme-import-error").font(theme::HELP).color(t.red).wrap().role("alert"));
    }
    let can = !l.import.text.trim().is_empty();
    let mut add = push_button(t, "Add theme", Push::Primary, can).key("theme-import-add");
    if can {
        add = add.on_press_tx(|s: &mut Shell, tx| {
            let text = s.screens.settings.looks.import.text.clone();
            let mut done = Err(String::new());
            s.change_settings(tx, |st| done = looks::import(st, &text));
            let l = &mut s.screens.settings.looks;
            match done {
                Ok(_) => {
                    l.importing = false;
                    l.import.clear();
                }
                Err(why) => l.import_error = Some(why),
            }
            tx.instant();
        });
    }
    out.child(
        row().w_full().gap(8.0).justify_end().child(
            push_button(t, "Cancel", Push::Quiet, true).key("theme-import-cancel").on_press_tx(|s: &mut Shell, tx| {
                let l = &mut s.screens.settings.looks;
                l.importing = false;
                l.import_error = None;
                tx.instant();
            }),
        ).child(add),
    )
}

// ---------------------------------------------------------------- light and dark

/// While following the system: the theme by day and the one by night, each a pop-up button.
pub(super) fn light_dark(s: &Shell, t: &'static Theme) -> El {
    let st = s.settings.get();
    let name = |id: &str| looks::resolve(id, &st.custom_themes).name;
    let button = |side: &'static str, words: &'static str, id: &str| {
        row().gap(6.0).items_center().child(text(words).font(theme::NOTE).color(t.l2).single_line()).child(
            popup_button(t, &format!("{words} theme"), &name(id), true).key(format!("look-{side}")).on_press_tx(move |s: &mut Shell, tx| {
                s.screens.settings.menu = Some(if side == "light" { "look-light" } else { "look-dark" });
                tx.instant();
            }),
        )
    };
    row().key(super::rows::control_key("look-light-dark")).gap(12.0).items_center().child(button("light", "Light", &st.light_theme)).child(button("dark", "Dark", &st.dark_theme))
}

/// The pop-up menu of light or dark themes, when one is open.
pub(super) fn menu(s: &Shell, t: &'static Theme, key: &str) -> Option<El> {
    let light = match key {
        "look-light" => true,
        "look-dark" => false,
        _ => return None,
    };
    let st = s.settings.get();
    let themes: Vec<Resolved> = looks::all(st).into_iter().filter(|r| r.light == light).collect();
    let now = if light { &st.light_theme } else { &st.dark_theme };
    let labels: Vec<&str> = themes.iter().map(|r| r.name.as_str()).collect();
    let ids: Vec<String> = themes.iter().map(|r| r.id.clone()).collect();
    let selected = ids.iter().position(|id| id == now);
    Some(popup_menu(
        t,
        key,
        &labels,
        selected,
        move |s: &mut Shell, i, tx| {
            s.screens.settings.menu = None;
            let id = ids[i].clone();
            s.change_settings(tx, move |st| {
                st.set(|v| {
                    if light {
                        v.light_theme = id;
                    } else {
                        v.dark_theme = id;
                    }
                })
            });
            tx.instant();
        },
        |s: &mut Shell, tx| {
            s.screens.settings.menu = None;
            tx.instant();
        },
    ))
}

// ---------------------------------------------------------------- the accent

/// The accents to pick from, and one of your own.
pub(super) fn accent(s: &Shell, t: &'static Theme) -> El {
    let now = s.settings.get().accent.clone();
    let mut swatches = row().key(super::rows::control_key("look-accent")).gap(8.0).items_center().role("radiogroup").label_static("Accent color");
    for (i, (name, value)) in looks::ACCENTS.iter().enumerate() {
        let picked = now.eq_ignore_ascii_case(value);
        swatches = swatches.child(
            dot(t, color(looks::accent_swatch(value)), 20.0, picked)
                .key(format!("accent-{i}"))
                .role("radio")
                .checked(picked)
                .label(*name)
                .help(*name)
                .focusable(true)
                .on_press_tx(move |s: &mut Shell, tx| {
                    s.screens.settings.looks.picking = None;
                    s.change_settings(tx, |st| st.set(|v| v.accent = (*value).to_owned()));
                }),
        );
    }
    let own = !looks::accent_is_preset(&now);
    let mut mine = dot(t, if own { color(looks::accent_swatch(&now)) } else { Color::WHITE }, 20.0, own)
        .key("accent-own")
        .role("button")
        .label_static("Your own accent color")
        .help("Your own accent color")
        .focusable(true)
        .on_press_tx(|s: &mut Shell, tx| toggle_picker(s, "accent", tx));
    if !own {
        // Every colour, for "pick your own" (0.10's rainbow).
        mine = mine.linear_gradient(FRAC_PI_2, rainbow());
    }
    swatches.child(mine)
}

fn rainbow() -> Vec<(f32, Color)> {
    ["#ff5f5f", "#ffcf5f", "#7fe07f", "#5fd7ff", "#7c83ff", "#e07fff", "#ff5f5f"]
        .iter()
        .enumerate()
        .filter_map(|(i, h)| Some((i as f32 / 6.0, rune::color::hex(h)?)))
        .collect()
}

/// A round swatch, ringed when it is the one in use.
fn dot(t: &Theme, fill: Color, size: f32, picked: bool) -> El {
    let mut e = rune::el().size(size, size).radius(size / 2.0).fill(fill).border(0.5, Color::BLACK.alpha(0.2));
    if picked {
        e = e.outline(2.0, 2.0, t.l1);
    }
    e
}

// ---------------------------------------------------------------- the colour picker

/// The two colours of the terminal's "Your own" scheme, as the picker knows them.
pub(super) const TERM_BG: &str = "term-bg";
pub(super) const TERM_FG: &str = "term-fg";

/// The terminal's own background and text, solid: what's set, else a dark terminal's.
pub(super) fn own_terminal(s: &Shell) -> (u32, u32) {
    match schemes::resolve(schemes::OWN, s.settings.get().terminal_colors.as_ref()) {
        schemes::Resolved::Own { background, foreground } => ((background << 8) | 0xff, (foreground << 8) | 0xff),
        _ => (0x1c1c1eff, 0xebebf0ff),
    }
}

/// The colour `key` stands for now: the accent, the terminal's own background or text, or a
/// colour of the theme being edited.
fn current(s: &Shell, key: &str) -> Option<u32> {
    if key == "accent" {
        return Some(looks::accent_swatch(&s.settings.get().accent));
    }
    if key == TERM_BG || key == TERM_FG {
        let (bg, fg) = own_terminal(s);
        return Some(if key == TERM_BG { bg } else { fg });
    }
    let id = s.screens.settings.looks.editing.as_ref()?;
    let theme = looks::resolve(id, &s.settings.get().custom_themes);
    Some(theme.colors[looks::key_index(key)?])
}

/// Opens the picker for `key`, or closes it. An opened one is scrolled into sight: in a short
/// window it opens below the bottom edge.
pub(super) fn toggle_picker(s: &mut Shell, key: &'static str, tx: &mut Tx) {
    if s.screens.settings.looks.picking.as_ref().is_some_and(|p| p.key == key) {
        s.screens.settings.looks.picking = None;
    } else if let Some(c) = current(s, key) {
        s.screens.settings.looks.picking = Some(Picking { key, hsv: looks::to_hsv(c), alpha: looks::alpha_of(c), draft: TextEdit::new(looks::to_hex(c)) });
        tx.scroll_into_view(format!("picker-{key}"));
    }
    tx.instant();
}

/// The picked colour goes where it belongs, at once.
fn apply(s: &mut Shell, c: u32, tx: &mut Tx) {
    let Some(p) = &s.screens.settings.looks.picking else { return };
    let key = p.key;
    if key == "accent" {
        s.change_settings(tx, |st| st.set(|v| v.accent = looks::to_hex(c | 0xff)));
    } else if key == TERM_BG || key == TERM_FG {
        // Picking one of them is choosing "Your own".
        let (bg, fg) = own_terminal(s);
        let (bg, fg) = if key == TERM_BG { (c | 0xff, fg) } else { (bg, c | 0xff) };
        s.change_settings(tx, |st| {
            st.set(|v| {
                let mut own = v.terminal_colors.clone().unwrap_or_default();
                own.background = looks::to_hex(bg);
                own.foreground = looks::to_hex(fg);
                v.terminal_colors = Some(own);
                v.terminal_scheme = schemes::OWN.to_owned();
            })
        });
    } else if let Some(id) = s.screens.settings.looks.editing.clone() {
        s.change_settings(tx, |st| looks::set_color(st, &id, key, Some(c)));
    }
    tx.instant();
}

/// The picker moved: the colour from its hue, saturation, brightness and opacity.
fn moved(s: &mut Shell, hsv: Hsv, alpha: f64, tx: &mut Tx) {
    let Some(p) = &mut s.screens.settings.looks.picking else { return };
    let solid = p.key == "accent" || p.key == TERM_BG || p.key == TERM_FG || COLOR_KEYS.iter().any(|k| k.key == p.key && !k.alpha);
    let alpha = if solid { 1.0 } else { alpha };
    p.hsv = hsv;
    p.alpha = alpha;
    let c = looks::from_hsv(hsv, alpha);
    p.draft.replace_text(looks::to_hex(c));
    apply(s, c, tx);
}

/// The picker for the colour being picked, when it is `key`: a saturation and brightness
/// square, the hue, the opacity for a colour that may be see-through, the hex code and a row
/// of ready colours.
pub(super) fn picker(s: &Shell, t: &'static Theme, key: &str) -> Option<El> {
    let p = s.screens.settings.looks.picking.as_ref().filter(|p| p.key == key)?;
    let alpha = key != "accent" && COLOR_KEYS.iter().any(|k| k.key == key && k.alpha);
    let (w, h) = (200.0, 138.0);
    let hue = color(looks::from_hsv(Hsv { h: p.hsv.h, s: 1.0, v: 1.0 }, 1.0));
    let now = color(looks::from_hsv(p.hsv, 1.0));
    let mark = rune::el()
        .absolute(p.hsv.s as f32 * w - 7.0, (1.0 - p.hsv.v as f32) * h - 7.0)
        .size(14.0, 14.0)
        .radius(7.0)
        .fill(now)
        .border(2.0, Color::WHITE)
        .a11y_hidden();
    let (hsv, a) = (p.hsv, p.alpha);
    let square = rune::el()
        .key("pick-square")
        .size(w, h)
        .radius(theme::RADIUS_ROW)
        .fill(hue)
        .linear_gradient(FRAC_PI_2, [(0.0, Color::WHITE), (1.0, Color::WHITE.alpha(0.0))])
        .role("slider")
        .label_static("Saturation and brightness")
        .a11y_value(format!("Saturation {}%, brightness {}%", (hsv.s * 100.0).round(), (hsv.v * 100.0).round()))
        .focusable(true)
        .on_drag(move |s: &mut Shell, d, tx| {
            if d.phase == Phase::Up && d.cancelled() {
                return;
            }
            let (sat, val) = (f64::from((d.x / w).clamp(0.0, 1.0)), f64::from(1.0 - (d.y / h).clamp(0.0, 1.0)));
            let Some(p) = &s.screens.settings.looks.picking else { return };
            let next = Hsv { s: sat, v: val, ..p.hsv };
            let alpha = p.alpha;
            moved(s, next, alpha, tx);
        })
        .on_key(move |s: &mut Shell, ev: &KeyEvent, tx| {
            let step = if ev.mods.shift { 0.1 } else { 0.01 };
            let (ds, dv) = match ev.key {
                Key::Left => (-step, 0.0),
                Key::Right => (step, 0.0),
                Key::Up => (0.0, step),
                Key::Down => (0.0, -step),
                _ => return false,
            };
            moved(s, Hsv { s: (hsv.s + ds).clamp(0.0, 1.0), v: (hsv.v + dv).clamp(0.0, 1.0), ..hsv }, a, tx);
            true
        })
        .child(rune::el().absolute(0.0, 0.0).size(w, h).radius(theme::RADIUS_ROW).gradient(Color::BLACK.alpha(0.0), Color::BLACK).a11y_hidden())
        .child(mark);
    let bar = |k: &'static str, name: &'static str, at: f64, stops: Vec<(f32, Color)>, set: fn(Hsv, f64, f64) -> (Hsv, f64)| {
        let width = 220.0;
        rune::el()
            .key(k)
            .size(width, 14.0)
            .radius(7.0)
            .linear_gradient(FRAC_PI_2, stops)
            .border(0.5, Color::BLACK.alpha(0.2))
            .role("slider")
            .label_static(name)
            .focusable(true)
            .on_drag(move |s: &mut Shell, d, tx| {
                let f = f64::from((d.x / width).clamp(0.0, 1.0));
                let Some(p) = &s.screens.settings.looks.picking else { return };
                let (hsv, alpha) = set(p.hsv, p.alpha, f);
                moved(s, hsv, alpha, tx);
            })
            .child(rune::el().absolute(at as f32 * (width - 14.0), 0.0).size(14.0, 14.0).radius(7.0).fill(Color::WHITE).border(1.0, Color::BLACK.alpha(0.3)).a11y_hidden())
    };
    let hues: Vec<(f32, Color)> = (0..=6).map(|i| (i as f32 / 6.0, color(looks::from_hsv(Hsv { h: f64::from(i) * 60.0, s: 1.0, v: 1.0 }, 1.0)))).collect();
    let mut sliders = col().gap(12.0).child(bar("pick-hue", "Hue", p.hsv.h / 360.0, hues, |hsv, a, f| (Hsv { h: f * 359.0, ..hsv }, a)));
    if alpha {
        let stops = vec![(0.0, now.alpha(0.0)), (1.0, now)];
        let solid = if p.alpha >= 1.0 { "Solid".to_owned() } else { format!("{}% solid", (p.alpha * 100.0).round()) };
        sliders = sliders.child(
            row().gap(8.0).items_center().child(bar("pick-alpha", "How solid", p.alpha, stops, |hsv, _, f| (hsv, f))).child(text(solid).key("pick-solid").font(theme::NOTE).color(t.l2).single_line()),
        );
    }
    let typed = p.draft.text.trim().to_owned();
    let bad = parse_hex(&typed).is_none_or(|c| !alpha && c & 0xff != 0xff);
    let hex = code_input(t, &p.draft, "#rrggbb")
        .key("pick-hex")
        .on_edit(|s: &mut Shell| &mut s.screens.settings.looks.picking.as_mut().expect("the picker is open").draft)
        .on_input_change(|s: &mut Shell, tx| {
            let Some(p) = &mut s.screens.settings.looks.picking else { return };
            let alpha = p.key != "accent" && COLOR_KEYS.iter().any(|k| k.key == p.key && k.alpha);
            // A colour is taken once the code is whole; half a code waits.
            let Some(c) = parse_hex(&p.draft.text).filter(|c| alpha || c & 0xff == 0xff) else { return tx.instant() };
            p.hsv = looks::to_hsv(c);
            p.alpha = looks::alpha_of(c);
            apply(s, c, tx);
        });
    sliders = sliders.child(field(t, hex, bad && !typed.is_empty()).w(140.0));
    if bad && !typed.is_empty() {
        let words = if alpha { "Use #rrggbb, or #rrggbbaa for a see-through color." } else { "Use #rrggbb: six digits and letters a to f." };
        sliders = sliders.child(text(words).key("pick-error").font(theme::HELP).color(t.red).wrap());
    }
    // Ready colours: the theme's own, then the accents, black and white.
    let mut ready: Vec<u32> = vec![];
    if let Some(id) = &s.screens.settings.looks.editing {
        ready.extend(looks::resolve(id, &s.settings.get().custom_themes).colors.iter().map(|c| c | 0xff));
    }
    ready.extend(looks::ACCENTS.iter().map(|(_, v)| looks::accent_swatch(v)));
    ready.extend([0x000000ff, 0xffffffff]);
    let mut seen = vec![];
    let mut swatches = row().gap(5.0).flex_wrap().role("group").label_static("Ready colors");
    for c in ready {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        let n = seen.len();
        swatches = swatches.child(
            rune::el().key(format!("pick-ready-{n}")).size(18.0, 18.0).radius(4.0).fill(color(c)).border(0.5, Color::BLACK.alpha(0.2)).role("button").label(looks::to_hex(c)).on_press_tx(move |s: &mut Shell, tx| {
                let alpha = s.screens.settings.looks.picking.as_ref().map_or(1.0, |p| p.alpha);
                moved(s, looks::to_hsv(c), alpha, tx);
            }),
        );
    }
    sliders = sliders.child(swatches.w(220.0));
    Some(row().key(format!("picker-{key}")).w_full().gap(16.0).padding(12.0).radius(theme::RADIUS_GROUP).fill(t.hov).child(square).child(sliders))
}

// ---------------------------------------------------------------- the editor

fn open_editor(s: &mut Shell, id: &str, tx: &mut Tx) {
    let name = s.settings.get().custom_themes.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default();
    let l = &mut s.screens.settings.looks;
    l.editing = Some(id.to_owned());
    l.name = TextEdit::new(name);
    l.deleting = false;
    l.copied = false;
    l.picking = None;
    tx.scroll_into_view("settings-head".to_owned());
    tx.instant();
}

/// Back to Appearance from the editor.
pub(super) fn close_editor(s: &mut Shell, tx: &mut Tx) {
    let l = &mut s.screens.settings.looks;
    l.editing = None;
    l.picking = None;
    l.deleting = false;
    tx.instant();
}

/// A theme of your own: its name, what can be done with it, and every colour.
pub(super) fn editor(s: &Shell, t: &'static Theme) -> Vec<El> {
    let st = s.settings.get();
    let l = &s.screens.settings.looks;
    let Some(id) = l.editing.clone() else { return vec![] };
    let Some(mine) = st.custom_themes.iter().find(|c| c.id == id) else { return vec![] };
    let theme = looks::resolve(&id, &st.custom_themes);
    let base = looks::resolve(&mine.base, &[]).name;
    let active = on_screen(s).id == id;
    let mut out = vec![];
    out.push(
        row().child(push_button(t, "← Appearance", Push::Quiet, true).key("editor-back").on_press_tx(close_editor)),
    );
    let name = input(t, &l.name, "Theme name").key("editor-name").on_edit(|s: &mut Shell| &mut s.screens.settings.looks.name).on_input_change(|s: &mut Shell, tx| {
        let (Some(id), name) = (s.screens.settings.looks.editing.clone(), s.screens.settings.looks.name.text.clone()) else { return };
        s.change_settings(tx, |st| looks::rename(st, &id, &name));
    });
    out.push(field(t, name, false).w(320.0));
    let said = if active || !st.theme_follows_system {
        format!("Started from {base}. Changes show everywhere as you make them and are saved right away.")
    } else {
        format!("Started from {base}. It's not showing now because Match my system picked your other theme; turn that off in Appearance to see changes as you make them.")
    };
    out.push(text(said).key("editor-note").font(theme::HELP).color(t.l2).wrap().px(2.0));
    let mut buttons = row().gap(8.0).flex_wrap();
    if !active && !st.theme_follows_system {
        let use_id = id.clone();
        buttons = buttons.child(push_button(t, "Use this theme", Push::Primary, true).key("editor-use").on_press_tx(move |s: &mut Shell, tx| s.change_settings(tx, |st| looks::pick(st, &use_id))));
    }
    let share = looks::to_json(mine);
    buttons = buttons.child(push_button(t, if l.copied { "Copied" } else { "Copy to share" }, Push::Quiet, true).key("editor-copy").on_press_tx(move |s: &mut Shell, tx| {
        tx.copy(share.clone());
        s.screens.settings.looks.copied = true;
        tx.instant();
    }));
    let reset_id = id.clone();
    buttons = buttons
        .child(push_button(t, "Reset all colors", Push::Quiet, !mine.colors.is_empty()).key("editor-reset").on_press_tx(move |s: &mut Shell, tx| s.change_settings(tx, |st| looks::reset_colors(st, &reset_id))))
        .child(push_button(t, "Delete", Push::Caution, true).key("editor-delete").on_press_tx(|s: &mut Shell, tx| {
            s.screens.settings.looks.deleting = true;
            tx.instant();
        }));
    out.push(buttons);
    if l.deleting {
        let gone = id.clone();
        out.push(
            row()
                .key("editor-deleting")
                .w_full()
                .gap(8.0)
                .px(12.0)
                .py(9.0)
                .items_center()
                .radius(theme::RADIUS_GROUP)
                .fill(t.red.alpha(0.1))
                .role("alertdialog")
                .child(text(format!("Delete “{}”? Anything using it goes back to {base}.", mine.name)).font(theme::NOTE).color(t.l1).wrap().grow(1.0).shrink())
                .child(push_button(t, "Keep it", Push::Quiet, true).key("editor-keep").on_press_tx(|s: &mut Shell, tx| {
                    s.screens.settings.looks.deleting = false;
                    tx.instant();
                }))
                .child(push_button(t, "Delete", Push::Destructive, true).key("editor-delete-yes").on_press_tx(move |s: &mut Shell, tx| {
                    s.change_settings(tx, |st| looks::delete(st, &gone));
                    close_editor(s, tx);
                })),
        );
    }
    let weak = looks::weak_contrast(&theme.colors);
    if !weak.is_empty() {
        out.push(
            row().key("editor-weak").w_full().px(12.0).py(9.0).radius(theme::RADIUS_GROUP).fill(t.orange.alpha(0.12)).role("status").child(
                text("Some text may be hard to read: the marked text colors are below 4.5:1 against what they sit on. You can keep them; it's your call.")
                    .font(theme::NOTE)
                    .color(t.l1)
                    .wrap()
                    .grow(1.0)
                    .shrink(),
            ),
        );
    }
    let mut group = "";
    let mut rows: Vec<El> = vec![];
    for (i, k) in COLOR_KEYS.iter().enumerate() {
        if k.group != group {
            if !rows.is_empty() {
                out.push(form_group(t, std::mem::take(&mut rows)));
            }
            group = k.group;
            out.push(form_heading(t, group, None));
        }
        let own = mine.colors.contains_key(k.key);
        let low: Vec<String> = weak.iter().filter(|w| w.text == i).map(|w| format!("{} ({:.1}:1)", looks::background_name(w.on), w.ratio)).collect();
        let help = if low.is_empty() { k.hint.to_owned() } else { format!("{} Hard to read on {}.", k.hint, low.join(", ")) };
        let label = if own { format!("{} (changed)", k.label) } else { k.label.to_owned() };
        let key = k.key;
        let reset_id = id.clone();
        let mut control = row()
            .gap(8.0)
            .items_center()
            .child(text(looks::to_hex(theme.colors[i])).font(theme::CODE).color(t.l2).single_line())
            .child(
                rune::el()
                    .key(format!("color-{key}"))
                    .size(28.0, 28.0)
                    .radius(7.0)
                    .fill(color(theme.colors[i]))
                    .border(1.0, t.hair)
                    .role("button")
                    .label(format!("Pick {}", k.label))
                    .focusable(true)
                    .on_press_tx(move |s: &mut Shell, tx| toggle_picker(s, key, tx)),
            );
        control = if own {
            control.child(icon_button(t, Icon::Refresh, &format!("Back to {base}'s color"), true).key(format!("color-reset-{key}")).on_press_tx(move |s: &mut Shell, tx| {
                s.screens.settings.looks.picking = None;
                s.change_settings(tx, |st| looks::set_color(st, &reset_id, key, None));
            }))
        } else {
            control.child(rune::el().size(theme::FORM_CONTROL, theme::FORM_CONTROL))
        };
        let words = RowText { label: &label, help: Some(&help), later: None };
        // The hex and the swatch stay where they are; an open picker goes under the row.
        rows.push(match picker(s, t, key) {
            Some(open) => col().w_full().child(form_row(t, words, control)).child(col().w_full().px(12.0).pb(12.0).child(open)).key(format!("row-{key}")),
            None => form_row(t, words, control).key(format!("row-{key}")),
        });
    }
    out.push(form_group(t, rows));
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_colour_has_its_words() {
        for k in sshee_app::settings_store::looks::COLOR_KEYS {
            assert!(!k.label.is_empty() && k.hint.ends_with('.'), "{}", k.key);
        }
    }
}
