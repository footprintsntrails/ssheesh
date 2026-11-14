//! Settings → Keyboard: every shortcut and its keys. Click one and press the new keys; ones
//! the terminal or the system needs are refused, with the reason under the row, and so are
//! keys another shortcut has. A port of 0.10's `ShortcutsEditor.tsx`; the rules are
//! `sshee_app::settings_store::keymap`.
//!
//! While a shortcut is recorded its key cap has the keyboard, and every key stops there: the
//! window's own shortcuts don't fire, and Esc ends the recording instead of closing Settings.

use rune::{col, row, text, El, KeyEvent, PhysicalInput, Tx};
use sshee_app::model::Keys;
use sshee_app::settings_store::keymap::{self, Keyboard};

use super::keyboard;
use crate::icons::Icon;
use crate::kit::form::{form_group, form_heading, form_row, RowText};
use crate::kit::keycap::{key_cap_button, recorded, CapState, Recorded};
use crate::kit::push::icon_button;
use crate::shell::Shell;
use crate::theme::{self, Theme};

/// The shortcut being recorded, and what the last try ran into.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recording {
    /// Its id.
    pub id: Option<&'static str>,
    /// The modifiers held so far, as shown: "Ctrl+Shift+".
    pub held: String,
    /// Why keys were refused, and for which shortcut: shown under its row.
    pub problem: Option<(&'static str, String)>,
}

/// The element key of a shortcut's key cap.
fn cap_key(id: &str) -> String {
    format!("keys-{id}")
}

/// A click on a key cap: start recording for it, or stop if it was recording.
fn record(s: &mut Shell, id: &'static str, tx: &mut Tx) {
    let keys = &mut s.screens.settings.keys;
    *keys = Recording { id: (keys.id != Some(id)).then_some(id), ..Recording::default() };
    if keys.id.is_some() {
        // The cap takes the keyboard, so the next keys are its own.
        tx.focus(cap_key(id), false);
    }
    tx.instant();
}

/// New keys for a shortcut: `Some(Some(keys))` those, `Some(None)` none, `None` its own again.
fn save(s: &mut Shell, id: &'static str, to: Option<Option<Keys>>, tx: &mut Tx) {
    let mac = s.options.mac_keys;
    s.change_settings(tx, |st| st.set(|v| keymap::set_keys(&mut v.shortcuts, id, to, mac)));
}

/// A key while `id` is recorded. Always taken: nothing typed here reaches anything else.
fn key(s: &mut Shell, id: &'static str, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.screens.settings.keys.id != Some(id) {
        return false;
    }
    if tx.key_repeat() {
        return true;
    }
    let on = keyboard(s);
    match recorded(ev, tx.physical_key(), tx.key_base()) {
        Recorded::Nothing => {}
        Recorded::Cancel => s.screens.settings.keys = Recording::default(),
        // Only the command bar's second key can be taken away.
        Recorded::Clear if id == "paletteAlt" => {
            save(s, id, Some(None), tx);
            s.screens.settings.keys = Recording::default();
        }
        Recorded::Clear => {}
        Recorded::Chord(keys) => match keymap::problem(id, &keys, on, &s.settings.get().shortcuts) {
            Some(why) => s.screens.settings.keys = Recording { id: Some(id), held: String::new(), problem: Some((id, why)) },
            None => {
                save(s, id, Some(Some(keys)), tx);
                s.screens.settings.keys = Recording::default();
            }
        },
    }
    tx.instant();
    true
}

/// Modifiers going down and up while recording, and the cap losing the keyboard. True when
/// it was the recorder's (Rune draws again only for an event that was taken).
fn physical(s: &mut Shell, id: &'static str, input: &PhysicalInput, tx: &mut Tx) -> bool {
    if s.screens.settings.keys.id != Some(id) {
        return false;
    }
    match *input {
        PhysicalInput::Modifiers(m) => {
            let held = keymap::keys("", m.ctrl, m.logo, m.alt, m.shift);
            let any = m.ctrl || m.logo || m.alt || m.shift;
            s.screens.settings.keys.held = if any { keymap::keys_label(&held, keyboard(s)) } else { String::new() };
            tx.instant();
            true
        }
        // A click anywhere else ends the recording, keeping what was refused on screen.
        PhysicalInput::FocusLost => {
            let problem = s.screens.settings.keys.problem.take();
            s.screens.settings.keys = Recording { problem, ..Recording::default() };
            tx.instant();
            true
        }
        // The key itself comes as a key event, named by the layout (`key`).
        PhysicalInput::Key { .. } => false,
    }
}

/// A shortcut's key cap, as a recorder when it can be changed.
fn cap(s: &Shell, t: &'static Theme, id: &'static str, name: &str, on: Keyboard) -> El {
    let state = &s.screens.settings.keys;
    let now = keymap::current_keys(id, on.mac(), &s.settings.get().shortcuts).map(|k| keymap::keys_label(&k, on));
    if !keymap::works(id) {
        return key_cap_button(t, name, now.as_deref().unwrap_or("None"), CapState::Disabled).key(cap_key(id));
    }
    let live = state.id == Some(id);
    let shown = if live {
        if state.held.is_empty() { "Press keys…".to_owned() } else { state.held.clone() }
    } else {
        now.unwrap_or_else(|| "None".to_owned())
    };
    key_cap_button(t, name, &shown, if live { CapState::Recording } else { CapState::Rest })
        .key(cap_key(id))
        .description(if live { "Recording. Press the new keys; Esc cancels." } else { "Click to change." })
        .on_press_tx(move |s: &mut Shell, tx| record(s, id, tx))
        .on_key(move |s: &mut Shell, ev, tx| key(s, id, ev, tx))
        .on_physical_input(move |s: &mut Shell, input, tx| physical(s, id, input, tx))
}

/// One shortcut's row: its name, its keys, and the way back to its own.
fn shortcut_row(s: &Shell, t: &'static Theme, id: &'static str, on: Keyboard) -> El {
    let Some(entry) = keymap::entry(id) else { return rune::el() };
    let changed = &s.settings.get().shortcuts;
    let state = &s.screens.settings.keys;
    let works = keymap::works(id);
    // The command bar has two keys on one row.
    let ids: &'static [&'static str] = if id == "palette" { &["palette", "paletteAlt"] } else { std::slice::from_ref(&entry.id) };
    let is_changed = ids.iter().any(|i| changed.contains_key(*i));
    let why = state.problem.as_ref().filter(|(at, _)| ids.contains(at)).map(|(_, why)| why.as_str());

    let mut keys = row().gap(6.0).items_center().child(cap(s, t, id, entry.name, on));
    if id == "palette" {
        let second = keymap::current_keys("paletteAlt", on.mac(), changed);
        if second.is_some() || state.id == Some("paletteAlt") {
            keys = keys.child(text("or").font(theme::NOTE).color(t.l2).single_line()).child(cap(s, t, "paletteAlt", "Command bar, second key", on));
        }
    }
    // Back to its own keys: there only when they were changed, and keeping its room otherwise
    // so the caps stay in one column.
    let own: Vec<String> = ids.iter().filter_map(|i| keymap::default_keys(i, on.mac())).map(|k| keymap::keys_label(&k, on)).collect();
    let reset = if is_changed && works {
        icon_button(t, Icon::Reopen, &format!("Back to {}", own.join(" or ")), true).key(format!("reset-{id}")).on_press_tx(move |s: &mut Shell, tx| {
            let mut next = s.settings.get().shortcuts.clone();
            match keymap::reset(&mut next, ids, keyboard(s)) {
                Ok(()) => {
                    s.change_settings(tx, |st| st.set(|v| v.shortcuts = next));
                    s.screens.settings.keys = Recording::default();
                }
                Err(why) => s.screens.settings.keys = Recording { problem: Some((ids[0], why)), ..Recording::default() },
            }
            tx.instant();
        })
    } else {
        rune::el().size(theme::FORM_CONTROL, theme::FORM_CONTROL)
    };
    let help = match (why, state.id) {
        (None, Some("paletteAlt")) if id == "palette" => Some("Backspace takes the second key away."),
        _ => None,
    };
    let words = RowText { label: entry.name, help, later: (!works).then_some("Comes later.") };
    // Always the same shape, with or without a refusal under it: the key cap that is
    // recording keeps the keyboard while the refusal comes and goes.
    let mut e = col().w_full().child(form_row(t, words, keys.child(reset)));
    if let Some(why) = why {
        // The refusal, in the row's own box, under its name and keys.
        e = e.child(text(why).key(format!("problem-{id}")).font(theme::HELP).color(t.red).wrap().px(12.0).pb(9.0).role("alert"));
    }
    if s.screens.settings.found.as_deref() == Some(format!("shortcut-{id}").as_str()) {
        e = e.fill(t.hov2);
    }
    e.key(format!("shortcut-{id}"))
}

/// The section.
pub(super) fn view(s: &Shell, t: &'static Theme) -> Vec<El> {
    let on = keyboard(s);
    let rule = if on.mac() {
        "They use ⌘, so ⌃ and ⌥ stay with the terminal and with typing."
    } else {
        "They use Ctrl+Shift, so Ctrl with a letter, Alt and AltGr stay with the terminal."
    };
    let mut out = vec![text(format!("Click a shortcut and press the new keys; Esc cancels. {rule}")).key("keys-shortcuts").font(theme::NOTE).color(t.l2).wrap().px(12.0)];
    for (title, ids) in keymap::GROUPS {
        out.push(form_heading(t, title, None));
        out.push(form_group(t, ids.iter().map(|id| shortcut_row(s, t, id, on)).collect::<Vec<El>>()));
    }
    out.push(form_heading(t, "Always the same", None));
    let fixed = keymap::fixed(on)
        .into_iter()
        .enumerate()
        .map(|(i, f)| {
            let mut keys = row().gap(6.0).items_center();
            for (n, k) in f.keys.iter().enumerate() {
                if n > 0 {
                    keys = keys.child(text("or").font(theme::NOTE).color(t.l2).single_line());
                }
                keys = keys.child(key_cap_button(t, f.name, &keymap::keys_label(k, on), CapState::Fixed));
            }
            // The room the others' reset button takes, so every cap ends at the same place.
            keys = keys.child(rune::el().size(theme::FORM_CONTROL, theme::FORM_CONTROL));
            form_row(t, RowText { label: f.name, help: f.note, later: None }, keys).key(format!("fixed-{i}"))
        })
        .collect();
    out.push(form_group(t, fixed));
    out
}
