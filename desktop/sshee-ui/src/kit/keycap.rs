//! Key caps for a list of shortcuts: the keys a shortcut has, in a soft tile. One that can be
//! changed is a button: a click turns it into a recorder that shows the modifiers held so far
//! and takes the next key pressed with them. One that can't is quieter and does nothing.
//!
//! What is pressed while recording is named here as the shortcut table names keys
//! ([`crate::shell::keys`]): the Latin letter the layout types, else the key's place.

use rune::{text, El, Key, KeyEvent, PhysicalKeyCode};
use sshee_app::model::Keys;
use sshee_app::settings_store::keymap;

use crate::theme::{self, Theme};

/// How a key cap is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapState {
    /// It has its keys: a click records new ones.
    Rest,
    /// Recording: ringed in the focus colour, waiting for keys.
    Recording,
    /// Keys that can't be changed.
    Fixed,
    /// Its shortcut does nothing yet: shown, dimmed, not to be pressed.
    Disabled,
}

/// A key cap saying `keys` ("Ctrl+Shift+T", "Press keys…"), named `name` for a screen
/// reader. The caller adds the key, `.on_press_tx(..)` and, while recording, `.on_key(..)`.
pub fn key_cap_button(t: &Theme, name: &str, keys: &str, state: CapState) -> El {
    let pressable = matches!(state, CapState::Rest | CapState::Recording);
    let (fill, ink) = match state {
        CapState::Fixed => (t.hov, t.l2),
        _ => (t.control, t.l1),
    };
    let mut e = rune::row()
        .h(theme::FORM_CONTROL)
        .min_w(56.0)
        .px(10.0)
        .radius(theme::RADIUS_ROW)
        .fill(fill)
        .items_center()
        .justify_center()
        .label(name)
        .a11y_value(keys)
        .child(text(keys).font(theme::CONTROL_TEXT).color(ink).single_line().a11y_hidden());
    if state != CapState::Fixed {
        e = e.role("button").focusable(pressable).disabled(!pressable);
    }
    match state {
        CapState::Rest => e.hover(t.control_hover).press(t.control_down),
        // The ring is the keyboard's focus ring: this is where keys go now.
        CapState::Recording => e.outline(2.0, 0.0, t.ring).no_focus_ring().checked(true),
        CapState::Fixed => e,
        CapState::Disabled => e.layer_opacity(Theme::DISABLED),
    }
}

/// What a key press means to a recorder.
#[derive(Clone, Debug, PartialEq)]
pub enum Recorded {
    /// Esc alone: stop recording, change nothing.
    Cancel,
    /// Backspace or Delete alone: take the keys away (where that is offered).
    Clear,
    /// A key with what was held: the keys to try.
    Chord(Keys),
    /// A key no shortcut can be on: ignored.
    Nothing,
}

/// Names the keys the letters and digits don't cover, as 0.10's `keyOf` does.
fn named(key: &Key) -> Option<String> {
    let name = match key {
        Key::Escape => "escape",
        Key::Backspace => "backspace",
        Key::Delete => "delete",
        Key::Insert => "insert",
        Key::Home => "home",
        Key::End => "end",
        Key::PageUp => "pageup",
        Key::PageDown => "pagedown",
        Key::Up => "up",
        Key::Down => "down",
        Key::Left => "left",
        Key::Right => "right",
        Key::Function(n) => return Some(format!("f{n}")),
        Key::Text(s) if s == " " => "space",
        Key::Command(' ') => "space",
        _ => return None,
    };
    Some(name.to_owned())
}

/// Reads a key press for a recorder. `physical` and `base` are `Tx::physical_key` and
/// `Tx::key_base` in the key's handler.
pub fn recorded(ev: &KeyEvent, physical: Option<PhysicalKeyCode>, base: Option<char>) -> Recorded {
    let Some(key) = named(&ev.key).or_else(|| crate::shell::keys::key_name(ev, physical, base)) else { return Recorded::Nothing };
    let m = ev.mods;
    let plain = !m.ctrl && !m.logo && !m.alt && !m.shift;
    match key.as_str() {
        "escape" if plain => Recorded::Cancel,
        "backspace" | "delete" if plain => Recorded::Clear,
        _ => Recorded::Chord(keymap::keys(&key, m.ctrl, m.logo, m.alt, m.shift)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::Mods;

    fn press(key: Key, held: &str) -> KeyEvent {
        let ctrl = held.contains('c');
        KeyEvent { key, mods: Mods { shift: held.contains('s'), alt: held.contains('a'), ctrl, logo: held.contains('m'), cmd: ctrl } }
    }

    fn chord(key: &str, held: &str) -> Recorded {
        Recorded::Chord(keymap::keys(key, held.contains('c'), held.contains('m'), held.contains('a'), held.contains('s')))
    }

    #[test]
    fn a_press_is_read_as_the_shortcut_table_names_keys() {
        assert_eq!(recorded(&press(Key::Command('k'), "cs"), None, None), chord("k", "cs"));
        // German: the key at US Y's place types Z; the shortcut is on Z.
        assert_eq!(recorded(&press(Key::Command('z'), "cs"), Some(PhysicalKeyCode::KeyY), Some('z')), chord("z", "cs"));
        // Russian: the key at T's place types "е"; the shortcut goes by the place.
        assert_eq!(recorded(&press(Key::Command('t'), "cs"), Some(PhysicalKeyCode::KeyT), Some('е')), chord("t", "cs"));
        // Shift+1 types "!": still the 1 key.
        assert_eq!(recorded(&press(Key::Command('!'), "cs"), Some(PhysicalKeyCode::Digit1), Some('1')), chord("1", "cs"));
        assert_eq!(recorded(&press(Key::Function(5), "c"), None, None), chord("f5", "c"));
        assert_eq!(recorded(&press(Key::Up, "cs"), None, None), chord("up", "cs"));
        assert_eq!(recorded(&press(Key::Enter, "cs"), None, None), chord("enter", "cs"));
        assert_eq!(recorded(&press(Key::Text(" ".into()), "m"), None, None), chord("space", "m"));
        // A plain letter is a chord too: the rules refuse it, in words.
        assert_eq!(recorded(&press(Key::Text("k".into()), ""), None, None), chord("k", ""));
    }

    #[test]
    fn esc_cancels_and_backspace_clears_only_alone() {
        assert_eq!(recorded(&press(Key::Escape, ""), None, None), Recorded::Cancel);
        assert_eq!(recorded(&press(Key::Backspace, ""), None, None), Recorded::Clear);
        assert_eq!(recorded(&press(Key::Delete, ""), None, None), Recorded::Clear);
        // With something held they are chords, for the rules to judge.
        assert!(matches!(recorded(&press(Key::Escape, "cs"), None, None), Recorded::Chord(k) if k.key == "escape"));
        assert!(matches!(recorded(&press(Key::Backspace, "c"), None, None), Recorded::Chord(k) if k.key == "backspace"));
        assert_eq!(recorded(&press(Key::Text("привет".into()), ""), None, None), Recorded::Nothing);
    }
}
