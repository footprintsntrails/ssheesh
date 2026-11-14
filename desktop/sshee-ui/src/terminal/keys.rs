//! Rune's key events as sshee-term's: what the key is, where it is on the keyboard, the
//! modifiers held, and what the layout typed. sshee-term's port of xterm.js's key handling
//! (`input::terminal_key`) does the rest.

use rune::{Key as RuneKey, KeyEvent as RuneKeyEvent, PhysicalInput, PhysicalKeyCode};
use sshee_term::input::{Code, Key, KeyEvent, Mods, NamedKey};

/// The key's place, named as a browser's `KeyboardEvent.code` (winit's `KeyCode` uses the same
/// names).
pub fn code_of(physical: Option<PhysicalKeyCode>) -> Code {
    physical.map_or(Code::Other, |k| Code::from_dom(&format!("{k:?}")))
}

/// Rune's modifiers as sshee-term's: the Super/Windows/Command key is Meta. (Windows' AltGr
/// arrives as Ctrl+Alt with the typed text, which sshee-term's third-level rule types.)
pub fn term_mods(m: rune::Mods) -> Mods {
    Mods { shift: m.shift, alt: m.alt, ctrl: m.ctrl, meta: m.logo, alt_graph: false }
}

/// Rune's key as sshee-term's. `physical` and `base` are `Tx::physical_key()` and
/// `Tx::key_base()` (the character the key types on the current layout without modifiers).
/// `None` for keys the terminal never sees (a modifier on its own).
pub fn term_key(ev: &RuneKeyEvent, physical: Option<PhysicalKeyCode>, base: Option<char>) -> Option<KeyEvent> {
    let mods = term_mods(ev.mods);
    let code = code_of(physical);
    let named = |k: NamedKey| Some(KeyEvent::new(Key::Named(k), code, mods).with_text(None));
    match &ev.key {
        RuneKey::Text(s) => {
            let c = s.chars().next()?;
            Some(KeyEvent::new(Key::Char(c), code, mods).with_text(Some(c)))
        }
        // Off macOS a non-ASCII layout's key arrives as the US letter; `base` is what the
        // layout really types there (Ctrl+С on a Russian layout).
        RuneKey::Command(c) => Some(KeyEvent::new(Key::Char(base.unwrap_or(*c)), code, mods).with_text(None)),
        RuneKey::Enter => named(NamedKey::Enter),
        RuneKey::Backspace => named(NamedKey::Backspace),
        RuneKey::Delete => named(NamedKey::Delete),
        RuneKey::Left => named(NamedKey::ArrowLeft),
        RuneKey::Right => named(NamedKey::ArrowRight),
        RuneKey::Up => named(NamedKey::ArrowUp),
        RuneKey::Down => named(NamedKey::ArrowDown),
        RuneKey::Home => named(NamedKey::Home),
        RuneKey::End => named(NamedKey::End),
        RuneKey::PageUp => named(NamedKey::PageUp),
        RuneKey::PageDown => named(NamedKey::PageDown),
        RuneKey::Insert => named(NamedKey::Insert),
        RuneKey::Escape => named(NamedKey::Escape),
        RuneKey::Tab => named(NamedKey::Tab),
        RuneKey::Pause => named(NamedKey::Pause),
        RuneKey::ContextMenu => named(NamedKey::ContextMenu),
        RuneKey::Function(n) => named(NamedKey::F(*n)),
        RuneKey::PrintScreen | RuneKey::ScrollLock => named(NamedKey::Other),
        _ => None,
    }
}

/// Return, Enter on the keypad or Escape pressed, as the key the terminal sends for it: what
/// the #602 guard takes before Rune's key routing while Rune's composition state is stale
/// (`ime.rs`).
pub fn guarded_key(input: &PhysicalInput) -> Option<KeyEvent> {
    let PhysicalInput::Key { key, mods, pressed: true, .. } = *input else { return None };
    let named = match key {
        PhysicalKeyCode::Enter | PhysicalKeyCode::NumpadEnter => NamedKey::Enter,
        PhysicalKeyCode::Escape => NamedKey::Escape,
        _ => return None,
    };
    Some(KeyEvent::new(Key::Named(named), code_of(Some(key)), term_mods(mods)).with_text(None))
}

/// Text typed as one event that is more than one character (a compose sequence, an input
/// method without composition): sent as it is.
pub fn typed_run(ev: &RuneKeyEvent) -> Option<&str> {
    match &ev.key {
        RuneKey::Text(s) if s.chars().nth(1).is_some() && !ev.mods.ctrl && !ev.mods.alt && !ev.mods.logo => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshee_term::engine::Modes;
    use sshee_term::input::{terminal_key, KeyAction, KeyOptions, Platform};

    fn send(ev: RuneKeyEvent, physical: Option<PhysicalKeyCode>, base: Option<char>, modes: Modes) -> KeyAction {
        let k = term_key(&ev, physical, base).expect("a key");
        terminal_key(&k, &KeyOptions::new(Platform::Other, &modes))
    }

    fn ev(key: RuneKey, f: impl FnOnce(&mut rune::Mods)) -> RuneKeyEvent {
        let mut mods = rune::Mods::default();
        f(&mut mods);
        RuneKeyEvent { key, mods }
    }

    #[test]
    fn the_layouts_and_chords_of_the_targets() {
        let m = Modes::default();
        // Typing, on any layout.
        assert_eq!(send(ev(RuneKey::Text("a".into()), |_| {}), Some(PhysicalKeyCode::KeyA), Some('a'), m), KeyAction::Send("a".into()));
        assert_eq!(send(ev(RuneKey::Text("ж".into()), |_| {}), Some(PhysicalKeyCode::Semicolon), Some('ж'), m), KeyAction::Send("ж".into()));
        // Ctrl+С on a Russian layout off macOS arrives as Command('c'): ^C.
        let ctrl = |m: &mut rune::Mods| {
            m.ctrl = true;
            m.cmd = true;
        };
        assert_eq!(send(ev(RuneKey::Command('c'), ctrl), Some(PhysicalKeyCode::KeyC), Some('с'), m), KeyAction::Send("\x03".into()));
        assert_eq!(send(ev(RuneKey::Command('d'), ctrl), Some(PhysicalKeyCode::KeyD), Some('d'), m), KeyAction::Send("\x04".into()));
        // Alt+B: Esc b.
        assert_eq!(send(ev(RuneKey::Text("b".into()), |m| m.alt = true), Some(PhysicalKeyCode::KeyB), Some('b'), m), KeyAction::Send("\x1bb".into()));
        // Arrows, normal and in application cursor mode.
        assert_eq!(send(ev(RuneKey::Up, |_| {}), Some(PhysicalKeyCode::ArrowUp), None, m), KeyAction::Send("\x1b[A".into()));
        let app = Modes { app_cursor: true, ..m };
        assert_eq!(send(ev(RuneKey::Up, |_| {}), Some(PhysicalKeyCode::ArrowUp), None, app), KeyAction::Send("\x1bOA".into()));
        // F5 with Shift.
        assert_eq!(send(ev(RuneKey::Function(5), |m| m.shift = true), Some(PhysicalKeyCode::F5), None, m), KeyAction::Send("\x1b[15;2~".into()));
        // Enter, Backspace, Tab, Escape.
        assert_eq!(send(ev(RuneKey::Enter, |_| {}), None, None, m), KeyAction::Send("\r".into()));
        assert_eq!(send(ev(RuneKey::Backspace, |_| {}), None, None, m), KeyAction::Send("\x7f".into()));
        assert_eq!(send(ev(RuneKey::Tab, |_| {}), None, None, m), KeyAction::Send("\t".into()));
        assert_eq!(send(ev(RuneKey::Escape, |_| {}), None, None, m), KeyAction::Send("\x1b".into()));
        // Shift+Enter: a line feed (agents' new line).
        assert_eq!(send(ev(RuneKey::Enter, |m| m.shift = true), None, None, m), KeyAction::Send("\n".into()));
    }

    #[test]
    fn the_602_guard_takes_return_and_escape_only() {
        let m = Modes::default();
        let opts = KeyOptions::new(Platform::Other, &m);
        let press = |key, mods: rune::Mods, pressed| PhysicalInput::Key { key, mods, pressed, repeat: false };
        let none = rune::Mods::default();
        let send = |input| guarded_key(&input).map(|k| terminal_key(&k, &opts));
        assert_eq!(send(press(PhysicalKeyCode::Enter, none, true)), Some(KeyAction::Send("\r".into())));
        assert_eq!(send(press(PhysicalKeyCode::NumpadEnter, none, true)), Some(KeyAction::Send("\r".into())));
        assert_eq!(send(press(PhysicalKeyCode::Escape, none, true)), Some(KeyAction::Send("\x1b".into())));
        let shift = rune::Mods { shift: true, ..none };
        assert_eq!(send(press(PhysicalKeyCode::Enter, shift, true)), Some(KeyAction::Send("\n".into())), "Shift+Enter is a line feed");
        assert_eq!(send(press(PhysicalKeyCode::Enter, none, false)), None, "releases pass");
        assert_eq!(send(press(PhysicalKeyCode::KeyA, none, true)), None);
        assert_eq!(send(PhysicalInput::FocusLost), None);
    }

    #[test]
    fn places_are_named_as_a_browser_names_them() {
        assert_eq!(code_of(Some(PhysicalKeyCode::KeyQ)), Code::Letter('Q'));
        assert_eq!(code_of(Some(PhysicalKeyCode::Digit7)), Code::Digit(7));
        assert_eq!(code_of(Some(PhysicalKeyCode::Numpad3)), Code::Numpad(3));
        assert_eq!(code_of(Some(PhysicalKeyCode::BracketLeft)), Code::BracketLeft);
        assert_eq!(code_of(Some(PhysicalKeyCode::Space)), Code::Space);
        assert_eq!(code_of(None), Code::Other);
    }
}
