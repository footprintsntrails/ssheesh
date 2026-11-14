//! A Rune key event named as the shortcut table names keys (0.10's `keyOf` in
//! src/lib/keys.ts): the Latin letter the key types, so Z is the key marked Z on a German or
//! French keyboard; on a layout without Latin letters (Russian, Greek), and for digits and
//! punctuation on every layout, the key at that place on a US keyboard, because what those
//! type differs too much between layouts (AZERTY's digit row types `&é"'(` unshifted).

use rune::{Key, KeyEvent, PhysicalKeyCode};

/// The key's name for [`sshee_app::workbench::shortcuts`]: "t", "3", ",", "tab", "enter".
/// `physical` and `base` are `Tx::physical_key` and `Tx::key_base` in the key's handler;
/// both are `None` for a key with no physical origin (a test's, a script's), which is then
/// named by what it types. `None`: a key no shortcut can be on.
pub fn key_name(ev: &KeyEvent, physical: Option<PhysicalKeyCode>, base: Option<char>) -> Option<String> {
    // The letter the layout types on this key.
    if let Some(letter) = base.filter(char::is_ascii_alphabetic) {
        return Some(letter.to_ascii_lowercase().to_string());
    }
    // Else the key's place.
    if let Some(name) = physical.and_then(place_name) {
        return Some(name);
    }
    match &ev.key {
        Key::Command(c) if c.is_ascii() => Some(c.to_ascii_lowercase().to_string()),
        Key::Text(s) if s.len() == 1 && s.is_ascii() => Some(s.to_ascii_lowercase()),
        Key::Tab => Some("tab".into()),
        Key::Enter => Some("enter".into()),
        _ => None,
    }
}

/// A physical key's name by its place: winit names them as the web's `KeyboardEvent.code`
/// does (`KeyT`, `Digit3`, `Comma`).
fn place_name(code: PhysicalKeyCode) -> Option<String> {
    let name = format!("{code:?}");
    if let Some(letter) = name.strip_prefix("Key").filter(|l| l.len() == 1) {
        return Some(letter.to_ascii_lowercase());
    }
    if let Some(digit) = name.strip_prefix("Digit").filter(|d| d.len() == 1) {
        return Some(digit.to_owned());
    }
    let named = match name.as_str() {
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Semicolon" => ";",
        "Quote" => "'",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Minus" => "-",
        "Equal" => "=",
        "Backquote" => "`",
        "Space" => "space",
        "Enter" | "NumpadEnter" => "enter",
        "Tab" => "tab",
        _ => return None,
    };
    Some(named.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::Mods;

    fn ev(key: Key) -> KeyEvent {
        KeyEvent { key, mods: Mods::default() }
    }

    #[test]
    fn a_latin_letter_is_the_one_the_key_types() {
        // German: the key at US Y's place types Z.
        assert_eq!(key_name(&ev(Key::Command('z')), Some(PhysicalKeyCode::KeyY), Some('z')).as_deref(), Some("z"));
        // AZERTY: the key at US Q's place types A.
        assert_eq!(key_name(&ev(Key::Command('a')), Some(PhysicalKeyCode::KeyQ), Some('a')).as_deref(), Some("a"));
        // Shift held: still the letter.
        assert_eq!(key_name(&ev(Key::Command('t')), Some(PhysicalKeyCode::KeyT), Some('T')).as_deref(), Some("t"));
    }

    #[test]
    fn a_layout_without_latin_letters_goes_by_the_keys_place() {
        // Russian: the key at T's place types "е". On macOS it arrives as text.
        assert_eq!(key_name(&ev(Key::Text("е".into())), Some(PhysicalKeyCode::KeyT), Some('е')).as_deref(), Some("t"));
        assert_eq!(key_name(&ev(Key::Command('t')), Some(PhysicalKeyCode::KeyT), Some('е')).as_deref(), Some("t"));
    }

    #[test]
    fn digits_and_punctuation_go_by_place_on_every_layout() {
        // AZERTY: the 1 key types "&" unshifted.
        assert_eq!(key_name(&ev(Key::Command('&')), Some(PhysicalKeyCode::Digit1), Some('&')).as_deref(), Some("1"));
        // US with Shift: "!" is still the 1 key.
        assert_eq!(key_name(&ev(Key::Command('!')), Some(PhysicalKeyCode::Digit1), Some('1')).as_deref(), Some("1"));
        assert_eq!(key_name(&ev(Key::Command(',')), Some(PhysicalKeyCode::Comma), Some(',')).as_deref(), Some(","));
        assert_eq!(key_name(&ev(Key::Tab), Some(PhysicalKeyCode::Tab), None).as_deref(), Some("tab"));
    }

    #[test]
    fn a_key_with_no_physical_origin_is_named_by_what_it_types() {
        assert_eq!(key_name(&ev(Key::Command('w')), None, None).as_deref(), Some("w"));
        assert_eq!(key_name(&ev(Key::Text("T".into())), None, None).as_deref(), Some("t"));
        assert_eq!(key_name(&ev(Key::Tab), None, None).as_deref(), Some("tab"));
        assert_eq!(key_name(&ev(Key::Up), None, None), None);
        assert_eq!(key_name(&ev(Key::Text("привет".into())), None, None), None);
    }
}
