//! What a key, a paste, the mouse or the focus sends to the program in the terminal.
//!
//! Keys are encoded the way xterm.js 6.0 did in SSHeesh 0.10, byte for byte, so nothing a
//! program reads changes with 1.0: [`xterm_key`] is a port of xterm.js's
//! `common/input/Keyboard.ts` (`evaluateKeyboardEvent`) and of the parts of its keydown and
//! keypress handlers (`browser/CoreBrowserTerminal.ts`) that decide what goes out.
//! `tests/engine_xterm_keys.json` holds what xterm.js itself sends for some 6,000 keys, made
//! by `tests/engine_xterm_keys.gen.mjs`, and the tests check every one.
//!
//! Mouse reports follow xterm.js's `CoreMouseService` (same codes and protocol rules), focus
//! reports and bracketed paste its `Clipboard.ts`. Pastes are cleaned as 0.10's `format.ts`
//! (`withoutKeys`, `pastedText`) did.
//!
//! Ported from xterm.js under the MIT licence:
//!
//! > Copyright (c) 2017-2019, The xterm.js authors (<https://github.com/xtermjs/xterm.js>)
//! > Copyright (c) 2014-2016, SourceLair Private Company (<https://www.sourcelair.com>)
//! > Copyright (c) 2012-2013, Christopher Jeffrey (<https://github.com/chjj/>)
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining a copy of this
//! > software and associated documentation files (the "Software"), to deal in the Software
//! > without restriction, including without limitation the rights to use, copy, modify, merge,
//! > publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
//! > to whom the Software is furnished to do so, subject to the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be included in all copies or
//! > substantial portions of the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
//! > INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
//! > PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
//! > FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//! > OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! > DEALINGS IN THE SOFTWARE.

use crate::engine::{Modes, MouseEncoding, MouseProtocol, Size};

// ---------------------------------------------------------------------------------------------
// Keys.

/// The computer the app runs on: keys differ between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Mac,
    Windows,
    /// Linux and the rest.
    Other,
}

impl Platform {
    /// The one this build is for.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Other
        }
    }
}

/// Modifiers held with a key. `meta` is ⌘ on a Mac and the Windows or Super key elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
    /// AltGr is held (Windows reports it as Ctrl+Alt as well).
    pub alt_graph: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { shift: false, alt: false, ctrl: false, meta: false, alt_graph: false };

    /// From xterm.js's modifier number: 1 Shift, 2 Alt, 4 Ctrl, 8 Meta, plus 16 for AltGr.
    pub fn from_bits(bits: u8) -> Self {
        Mods { shift: bits & 1 != 0, alt: bits & 2 != 0, ctrl: bits & 4 != 0, meta: bits & 8 != 0, alt_graph: bits & 16 != 0 }
    }

    /// The modifier number in `CSI 1 ; n A` and the like, less one (0 when none is held).
    fn number(self) -> u8 {
        (self.shift as u8) | (self.alt as u8) << 1 | (self.ctrl as u8) << 2 | (self.meta as u8) << 3
    }
}

/// A key that doesn't type a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NamedKey {
    Backspace,
    Tab,
    Enter,
    Escape,
    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    /// F1 to F24.
    F(u8),
    Shift,
    Control,
    Alt,
    AltGraph,
    Meta,
    CapsLock,
    ContextMenu,
    Pause,
    /// Any other key that types nothing (media keys and the like).
    Other,
}

/// What a key is: named, the character it types on the current layout, or a dead key that
/// starts a composed character (the character itself arrives later, as text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Named(NamedKey),
    Char(char),
    Dead,
}

impl Key {
    /// From a browser's `KeyboardEvent.key` (also winit's naming): "a", "Enter", "F5", "Dead".
    pub fn from_dom(key: &str) -> Key {
        let mut chars = key.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return Key::Char(c);
        }
        let named = match key {
            "Dead" => return Key::Dead,
            "Backspace" => NamedKey::Backspace,
            "Tab" => NamedKey::Tab,
            "Enter" => NamedKey::Enter,
            "Escape" => NamedKey::Escape,
            "ArrowLeft" => NamedKey::ArrowLeft,
            "ArrowUp" => NamedKey::ArrowUp,
            "ArrowRight" => NamedKey::ArrowRight,
            "ArrowDown" => NamedKey::ArrowDown,
            "Insert" => NamedKey::Insert,
            "Delete" => NamedKey::Delete,
            "Home" => NamedKey::Home,
            "End" => NamedKey::End,
            "PageUp" => NamedKey::PageUp,
            "PageDown" => NamedKey::PageDown,
            "Shift" => NamedKey::Shift,
            "Control" => NamedKey::Control,
            "Alt" => NamedKey::Alt,
            "AltGraph" => NamedKey::AltGraph,
            "Meta" => NamedKey::Meta,
            "CapsLock" => NamedKey::CapsLock,
            "ContextMenu" => NamedKey::ContextMenu,
            "Pause" => NamedKey::Pause,
            _ => match key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
                Some(n @ 1..=24) => NamedKey::F(n),
                _ => NamedKey::Other,
            },
        };
        Key::Named(named)
    }
}

/// Where the key is on the keyboard, named after the US key there (a browser's
/// `KeyboardEvent.code`, winit's `KeyCode`). Only the places that change what is sent are named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    /// The letter key `A` to `Z` (uppercase).
    Letter(char),
    /// The digit key 0 to 9 on the main block.
    Digit(u8),
    /// The digit key 0 to 9 on the keypad.
    Numpad(u8),
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
    Semicolon,
    Equal,
    Comma,
    Minus,
    Period,
    Slash,
    Backquote,
    BracketLeft,
    Backslash,
    BracketRight,
    Quote,
    Space,
    /// Any other place, or none (a key the system pressed by itself).
    Other,
}

impl Code {
    /// From a browser's `KeyboardEvent.code`: "KeyA", "Digit1", "BracketLeft", "Numpad7".
    pub fn from_dom(code: &str) -> Code {
        if let Some(rest) = code.strip_prefix("Key") {
            let mut c = rest.chars();
            if let (Some(l @ 'A'..='Z'), None) = (c.next(), c.next()) {
                return Code::Letter(l);
            }
        }
        if let Some(d) = code.strip_prefix("Digit").and_then(single_digit) {
            return Code::Digit(d);
        }
        if let Some(d) = code.strip_prefix("Numpad").and_then(single_digit) {
            return Code::Numpad(d);
        }
        match code {
            "NumpadAdd" => Code::NumpadAdd,
            "NumpadSubtract" => Code::NumpadSubtract,
            "NumpadMultiply" => Code::NumpadMultiply,
            "NumpadDivide" => Code::NumpadDivide,
            "NumpadDecimal" => Code::NumpadDecimal,
            "NumpadEnter" => Code::NumpadEnter,
            "Semicolon" => Code::Semicolon,
            "Equal" => Code::Equal,
            "Comma" => Code::Comma,
            "Minus" => Code::Minus,
            "Period" => Code::Period,
            "Slash" => Code::Slash,
            "Backquote" => Code::Backquote,
            "BracketLeft" => Code::BracketLeft,
            "Backslash" => Code::Backslash,
            "BracketRight" => Code::BracketRight,
            "Quote" => Code::Quote,
            "Space" => Code::Space,
            _ => Code::Other,
        }
    }
}

fn single_digit(s: &str) -> Option<u8> {
    let mut c = s.chars();
    match (c.next(), c.next()) {
        (Some(d @ '0'..='9'), None) => Some(d as u8 - b'0'),
        _ => None,
    }
}

/// A key pressed: what it is, where it is, the modifiers held, and what the system typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub key: Key,
    pub code: Code,
    pub mods: Mods,
    /// The character the system typed for this press with every modifier applied, if it typed
    /// one: winit's `text_with_all_modifiers()` (its first character). `None` for a dead key and
    /// for a chord that types nothing, such as Ctrl+Alt+C on a US or Russian layout on Windows
    /// (no AltGr character there; the browser then sent no keypress, and 0.10 sent nothing).
    /// Only read when the key is left to the system to type (see [`xterm_key`]).
    pub text: Option<char>,
}

impl KeyEvent {
    /// A key that types its own character (`text` is `key`'s character), as a key pressed with
    /// no modifier or only Shift does. Use [`KeyEvent::with_text`] for what the system said.
    pub fn new(key: Key, code: Code, mods: Mods) -> Self {
        let text = match key {
            Key::Char(c) => Some(c),
            _ => None,
        };
        KeyEvent { key, code, mods, text }
    }

    /// The same key with what the system typed for it (see [`KeyEvent::text`]).
    pub fn with_text(self, text: Option<char>) -> Self {
        KeyEvent { text, ..self }
    }

    /// The `keyCode` a browser gives this key, which xterm.js's logic goes by: a Latin letter's
    /// own code (so Ctrl+Z is the key marked Z on a German keyboard too), else the code of the
    /// US key at that place (so Ctrl+С on a Russian layout is Ctrl+C), as 0.10's `keyOf` and
    /// `missingKeyCode` (keys.ts) made sure on every system.
    pub fn legacy_key_code(&self) -> u32 {
        match self.key {
            Key::Named(n) => named_key_code(n),
            Key::Char(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase() as u32,
            Key::Char(c) => place_key_code(self.code).unwrap_or(match c {
                '0'..='9' => c as u32,
                _ => 0,
            }),
            Key::Dead => place_key_code(self.code).unwrap_or(0),
        }
    }
}

fn named_key_code(n: NamedKey) -> u32 {
    match n {
        NamedKey::Backspace => 8,
        NamedKey::Tab => 9,
        NamedKey::Enter => 13,
        NamedKey::Shift => 16,
        NamedKey::Control => 17,
        NamedKey::Alt => 18,
        NamedKey::Pause => 19,
        NamedKey::CapsLock => 20,
        NamedKey::Escape => 27,
        NamedKey::PageUp => 33,
        NamedKey::PageDown => 34,
        NamedKey::End => 35,
        NamedKey::Home => 36,
        NamedKey::ArrowLeft => 37,
        NamedKey::ArrowUp => 38,
        NamedKey::ArrowRight => 39,
        NamedKey::ArrowDown => 40,
        NamedKey::Insert => 45,
        NamedKey::Delete => 46,
        NamedKey::Meta => 91,
        NamedKey::ContextMenu => 93,
        NamedKey::F(n) => 111 + n as u32,
        NamedKey::AltGraph => 225,
        NamedKey::Other => 0,
    }
}

fn place_key_code(code: Code) -> Option<u32> {
    Some(match code {
        Code::Letter(l) => l as u32,
        Code::Digit(d) => 48 + d as u32,
        Code::Numpad(d) => 96 + d as u32,
        Code::NumpadMultiply => 106,
        Code::NumpadAdd => 107,
        Code::NumpadSubtract => 109,
        Code::NumpadDecimal => 110,
        Code::NumpadDivide => 111,
        Code::Semicolon => 186,
        Code::Equal => 187,
        Code::Comma => 188,
        Code::Minus => 189,
        Code::Period => 190,
        Code::Slash => 191,
        Code::Backquote => 192,
        Code::BracketLeft => 219,
        Code::Backslash => 220,
        Code::BracketRight => 221,
        Code::Quote => 222,
        Code::Space => 32,
        Code::NumpadEnter | Code::Other => return None,
    })
}

/// What the terminal's own key handling depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyOptions {
    pub platform: Platform,
    /// Option on a Mac sends Esc and the key (`macOptionIsMeta`, on in 0.10) instead of typing
    /// a character.
    pub mac_option_is_meta: bool,
    /// The program asked for application cursor keys (`?1h`, [`Modes::app_cursor`]).
    pub app_cursor: bool,
}

impl KeyOptions {
    pub fn new(platform: Platform, modes: &Modes) -> Self {
        KeyOptions { platform, mac_option_is_meta: true, app_cursor: modes.app_cursor }
    }
}

/// What a key does in the terminal.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum KeyAction {
    /// Send these bytes to the program.
    Send(String),
    /// Scroll the history a page up or down (Shift+Page Up/Down) instead.
    PageUp,
    PageDown,
    /// Select everything (⌘A on a Mac).
    SelectAll,
    /// Nothing: a modifier on its own, a dead key, a combination programs don't get.
    Nothing,
}

const ESC: &str = "\x1b";

/// xterm.js's `KEYCODE_KEY_MAPPINGS`: what digits and punctuation type, plain and with Shift
/// (US layout), for Alt combinations.
fn keycode_mapping(key_code: u32) -> Option<(char, char)> {
    Some(match key_code {
        48 => ('0', ')'),
        49 => ('1', '!'),
        50 => ('2', '@'),
        51 => ('3', '#'),
        52 => ('4', '$'),
        53 => ('5', '%'),
        54 => ('6', '^'),
        55 => ('7', '&'),
        56 => ('8', '*'),
        57 => ('9', '('),
        186 => (';', ':'),
        187 => ('=', '+'),
        188 => (',', '<'),
        189 => ('-', '_'),
        190 => ('.', '>'),
        191 => ('/', '?'),
        192 => ('`', '~'),
        219 => ('[', '{'),
        220 => ('\\', '|'),
        221 => (']', '}'),
        222 => ('\'', '"'),
        _ => return None,
    })
}

/// What `evaluateKeyboardEvent` decided: bytes to send, or something else to do.
enum Evaluated {
    Send(String),
    PageUp,
    PageDown,
    SelectAll,
    Nothing,
}

/// A port of xterm.js's `evaluateKeyboardEvent` (common/input/Keyboard.ts).
fn evaluate(ev: &KeyEvent, key_code: u32, opts: &KeyOptions) -> Evaluated {
    let m = ev.mods;
    let modifiers = m.number();
    let is_mac = opts.platform == Platform::Mac;
    let app = opts.app_cursor;
    let send = |s: String| Evaluated::Send(s);
    // "CSI 1 ; n X" with modifiers, else "CSI X" or "SS3 X" in application cursor mode.
    let cursor = |final_char: char| {
        if modifiers != 0 {
            send(format!("{ESC}[1;{}{final_char}", modifiers + 1))
        } else if app {
            send(format!("{ESC}O{final_char}"))
        } else {
            send(format!("{ESC}[{final_char}"))
        }
    };
    // "CSI n ; m ~" with modifiers, else "CSI n ~".
    let tilde = |n: u32| {
        if modifiers != 0 {
            send(format!("{ESC}[{n};{}~", modifiers + 1))
        } else {
            send(format!("{ESC}[{n}~"))
        }
    };
    // F1 to F4: "CSI 1 ; m X" with modifiers, else "SS3 X".
    let pf = |final_char: char| {
        if modifiers != 0 {
            send(format!("{ESC}[1;{}{final_char}", modifiers + 1))
        } else {
            send(format!("{ESC}O{final_char}"))
        }
    };
    match key_code {
        8 => {
            let mut key = if m.ctrl { "\x08".to_owned() } else { "\x7f".to_owned() };
            if m.alt {
                key.insert_str(0, ESC);
            }
            send(key)
        }
        9 => send(if m.shift { format!("{ESC}[Z") } else { "\t".to_owned() }),
        13 => send(if m.alt { format!("{ESC}\r") } else { "\r".to_owned() }),
        27 => send(if m.alt { format!("{ESC}{ESC}") } else { ESC.to_owned() }),
        37 if m.meta => Evaluated::Nothing,
        37 => cursor('D'),
        39 if m.meta => Evaluated::Nothing,
        39 => cursor('C'),
        38 if m.meta => Evaluated::Nothing,
        38 => cursor('A'),
        40 if m.meta => Evaluated::Nothing,
        40 => cursor('B'),
        // Shift+Insert and Ctrl+Insert paste and copy on some systems.
        45 if !m.shift && !m.ctrl => send(format!("{ESC}[2~")),
        45 => Evaluated::Nothing,
        46 => tilde(3),
        36 => cursor('H'),
        35 => cursor('F'),
        33 if m.shift => Evaluated::PageUp,
        33 if m.ctrl => send(format!("{ESC}[5;{}~", modifiers + 1)),
        33 => send(format!("{ESC}[5~")),
        34 if m.shift => Evaluated::PageDown,
        34 if m.ctrl => send(format!("{ESC}[6;{}~", modifiers + 1)),
        34 => send(format!("{ESC}[6~")),
        112 => pf('P'),
        113 => pf('Q'),
        114 => pf('R'),
        115 => pf('S'),
        116 => tilde(15),
        117 => tilde(17),
        118 => tilde(18),
        119 => tilde(19),
        120 => tilde(20),
        121 => tilde(21),
        122 => tilde(23),
        123 => tilde(24),
        _ => evaluate_other(ev, key_code, opts, is_mac),
    }
}

/// The `default:` branch of `evaluateKeyboardEvent`: letters, digits, punctuation, space.
fn evaluate_other(ev: &KeyEvent, key_code: u32, opts: &KeyOptions, is_mac: bool) -> Evaluated {
    let m = ev.mods;
    let one = |c: u32| Evaluated::Send(char::from_u32(c).map(String::from).unwrap_or_default());
    if m.ctrl && !m.shift && !m.alt && !m.meta {
        return match key_code {
            65..=90 => one(key_code - 64),
            32 => one(0),
            // Ctrl+3 to Ctrl+7: escape, file, group, record and unit separators.
            51..=55 => one(key_code - 51 + 27),
            56 => one(0x7f),
            219 => one(0x1b),
            220 => one(0x1c),
            221 => one(0x1d),
            _ => Evaluated::Nothing,
        };
    }
    if (!is_mac || opts.mac_option_is_meta) && m.alt && !m.meta {
        // On a Mac without Option as Meta this is a third level shift (typing a character).
        if let Some((plain, shifted)) = keycode_mapping(key_code) {
            return Evaluated::Send(format!("{ESC}{}", if m.shift { shifted } else { plain }));
        }
        if (65..=90).contains(&key_code) {
            let code = if m.ctrl { key_code - 64 } else { key_code + 32 };
            let mut c = char::from_u32(code).unwrap_or(' ');
            if m.shift {
                c = c.to_ascii_uppercase();
            }
            return Evaluated::Send(format!("{ESC}{c}"));
        }
        if key_code == 32 {
            return Evaluated::Send(format!("{ESC}{}", if m.ctrl { '\0' } else { ' ' }));
        }
        if let (Key::Dead, Code::Letter(l)) = (ev.key, ev.code) {
            // Alt starts a composition with some letters on a US Mac layout (N, E, U…).
            return Evaluated::Send(format!("{ESC}{}", if m.shift { l } else { l.to_ascii_lowercase() }));
        }
        return Evaluated::Nothing;
    }
    if is_mac && !m.alt && !m.ctrl && !m.shift && m.meta {
        return if key_code == 65 { Evaluated::SelectAll } else { Evaluated::Nothing };
    }
    if let Key::Char(c) = ev.key {
        if !m.ctrl && !m.alt && !m.meta && key_code >= 48 {
            // Only keys that type a single character: not Num Lock, volume up and the like.
            return Evaluated::Send(c.to_string());
        }
        if m.ctrl {
            return match c {
                '_' => one(0x1f),
                '@' => one(0),
                _ => Evaluated::Nothing,
            };
        }
    }
    Evaluated::Nothing
}

/// Whether xterm.js leaves this key to the system to type a character with (AltGr on Windows,
/// Option on a Mac when it isn't Meta). `keypress` is the later event that carries the character.
fn third_level_shift(ev: &KeyEvent, key_code: u32, opts: &KeyOptions, keypress: bool) -> bool {
    let m = ev.mods;
    let third = (opts.platform == Platform::Mac && !opts.mac_option_is_meta && m.alt && !m.ctrl && !m.meta)
        || (opts.platform == Platform::Windows && m.alt && m.ctrl && !m.meta)
        || (opts.platform == Platform::Windows && m.alt_graph);
    if keypress {
        return third;
    }
    // Not for arrows, Page Down, Home, Backspace and the like.
    third && (key_code == 0 || key_code > 47)
}

/// What xterm.js 6.0 sent for a key pressed in a fresh terminal (no dead key pending): its
/// keydown evaluation, or the typed character when that left the key to the system.
pub fn xterm_key(ev: &KeyEvent, opts: &KeyOptions) -> KeyAction {
    let key_code = ev.legacy_key_code();
    let m = ev.mods;
    let ignore_composition = opts.platform == Platform::Mac && opts.mac_option_is_meta && m.alt;
    let dead_pending = !ignore_composition && matches!(ev.key, Key::Dead | Key::Named(NamedKey::AltGraph));
    let mut select_all = false;
    let sent = match evaluate(ev, key_code, opts) {
        Evaluated::PageUp => return KeyAction::PageUp,
        Evaluated::PageDown => return KeyAction::PageDown,
        Evaluated::SelectAll => {
            select_all = true;
            None
        }
        Evaluated::Send(s) if !s.is_empty() => Some(s),
        Evaluated::Send(_) | Evaluated::Nothing => None,
    };
    let to_keypress = if third_level_shift(ev, key_code, opts, false) {
        true
    } else if let Some(sent) = sent {
        // Capital letters typed without modifiers go through keypress too (an IME fix in
        // xterm.js); the bytes are the same.
        let capital = matches!(ev.key, Key::Char('A'..='Z')) && !m.ctrl && !m.alt && !m.meta;
        if !capital {
            return if dead_pending { KeyAction::Nothing } else { KeyAction::Send(sent) };
        }
        true
    } else {
        true
    };
    // keypress: a browser sends one only when the system typed a character for the key, and
    // xterm.js takes it with modifiers only when the system typed it at its third level.
    if to_keypress {
        if let (Key::Char(_), Some(typed)) = (ev.key, ev.text) {
            if !(m.alt || m.ctrl || m.meta) || third_level_shift(ev, key_code, opts, true) {
                return KeyAction::Send(typed.to_string());
            }
        }
    }
    if select_all {
        KeyAction::SelectAll
    } else {
        KeyAction::Nothing
    }
}

/// What a key sends in an SSHeesh terminal: [`xterm_key`] plus 0.10's own rule (keys.ts):
/// Shift+Enter types a line feed (a new line in Claude Code, OpenCode and Codex; the same as
/// Enter in a shell). App shortcuts, copy and paste keys are the view's, before this.
pub fn terminal_key(ev: &KeyEvent, opts: &KeyOptions) -> KeyAction {
    if is_terminal_newline(ev) {
        return KeyAction::Send("\n".to_owned());
    }
    xterm_key(ev, opts)
}

/// Shift+Enter and nothing else.
pub fn is_terminal_newline(ev: &KeyEvent) -> bool {
    let m = ev.mods;
    ev.key == Key::Named(NamedKey::Enter) && m.shift && !m.ctrl && !m.alt && !m.meta
}

/// The letter a shortcut means, on any layout: the Latin letter the key types, else the US
/// letter at its place (keys.ts `keyOf`, letters only).
pub fn shortcut_letter(ev: &KeyEvent) -> Option<char> {
    match (ev.key, ev.code) {
        (Key::Char(c), _) if c.is_ascii_alphabetic() => Some(c.to_ascii_lowercase()),
        (_, Code::Letter(l)) => Some(l.to_ascii_lowercase()),
        _ => None,
    }
}

/// A paste key: Ctrl+V on Windows (as in Windows Terminal; clipboard history presses it too),
/// Ctrl+Shift+V elsewhere. ⌘V on a Mac is the menu's, not this.
pub fn is_terminal_paste(ev: &KeyEvent, platform: Platform) -> bool {
    let m = ev.mods;
    if platform == Platform::Mac || !m.ctrl || m.alt || m.meta || shortcut_letter(ev) != Some('v') {
        return false;
    }
    platform == Platform::Windows || m.shift
}

/// Which copy key this is outside a Mac: Ctrl+Shift+C always copies; Ctrl+C copies only while
/// text is selected, and otherwise stops the running command as always.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKey {
    CtrlC,
    CtrlShiftC,
}

pub fn terminal_copy_key(ev: &KeyEvent, platform: Platform) -> Option<CopyKey> {
    let m = ev.mods;
    if platform == Platform::Mac || !m.ctrl || m.alt || m.meta || shortcut_letter(ev) != Some('c') {
        return None;
    }
    Some(if m.shift { CopyKey::CtrlShiftC } else { CopyKey::CtrlC })
}

/// ⌘K on a Mac clears the terminal, as Terminal, iTerm2 and Ghostty do.
pub fn is_mac_clear(ev: &KeyEvent, platform: Platform) -> bool {
    let m = ev.mods;
    platform == Platform::Mac && m.meta && !m.ctrl && !m.alt && !m.shift && shortcut_letter(ev) == Some('k')
}

/// Keys and reports that aren't typing (mouse reports, focus in and out): they don't end
/// scrolling back in tmux (0.10's `NOT_TYPING`).
pub fn is_typing(data: &[u8]) -> bool {
    let not_typing = data.starts_with(b"\x1b[<") || data.starts_with(b"\x1b[M") || data == b"\x1b[I" || data == b"\x1b[O";
    !not_typing
}

// ---------------------------------------------------------------------------------------------
// Pastes and typed text.

/// Text that is safe to paste: line breaks made `\n`, every other control character (Esc,
/// Ctrl+U, the C1 range) gone, tabs kept. What a server copied (OSC 52) or the clipboard holds
/// can't press keys in the shell this way, nor end a bracketed paste early.
pub fn without_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\t' | '\n' => out.push(c),
            '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {}
            _ => out.push(c),
        }
    }
    out
}

/// What a paste sends: [`without_keys`], line breaks as Enter (`\r`), wrapped in
/// `ESC [ 200 ~` … `ESC [ 201 ~` when the program asked for bracketed paste.
pub fn paste(text: &str, bracketed: bool) -> String {
    let body = without_keys(text).replace('\n', "\r");
    if bracketed {
        format!("{ESC}[200~{body}{ESC}[201~")
    } else {
        body
    }
}

/// Text the system typed without key presses (an IME's commit, Windows' clipboard history and
/// emoji panel): typed as it is, or pasted when it holds a line break, as 0.10 did.
pub fn typed_text(text: &str, bracketed: bool) -> String {
    let clean = without_keys(text);
    if clean.contains('\n') {
        paste(&clean, bracketed)
    } else {
        clean
    }
}

/// The text in a paste, from what the clipboard holds by type: plain text when there is some;
/// else a link copied as a link (Copy Link in Safari and Telegram puts no plain text); else the
/// words of rich text. Empty when there is no text at all, as for a picture. (format.ts
/// `pastedText`.)
pub fn pasted_text(plain: &str, uri_list: &str, html: &str) -> String {
    if !plain.is_empty() {
        return plain.to_owned();
    }
    let links: Vec<&str> = split_lines(uri_list).filter(|l| !js_trim(l).is_empty() && !l.starts_with('#')).collect();
    if !links.is_empty() {
        return links.join("\n");
    }
    html_words(html)
}

/// Splits at `\n` and `\r\n`, as `split(/\r?\n/)`.
fn split_lines(s: &str) -> impl Iterator<Item = &str> {
    s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l))
}

/// JavaScript's `trim`: white space, line breaks and the byte-order mark.
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn html_words(html: &str) -> String {
    use std::sync::OnceLock;
    static RES: OnceLock<[regex::Regex; 4]> = OnceLock::new();
    let [code, br, tag, entity] = RES.get_or_init(|| {
        [
            regex::Regex::new(r"(?i)<(?:script(?-u:\b)(?s:.)*?</script>|style(?-u:\b)(?s:.)*?</style>)").expect("script pattern"),
            regex::Regex::new(r"(?i)<br\s*/?>").expect("br pattern"),
            regex::Regex::new(r"<[^>]*>").expect("tag pattern"),
            regex::Regex::new(r"&(#[0-9]+|[a-zA-Z]+);").expect("entity pattern"),
        ]
    });
    let text = code.replace_all(html, "");
    let text = br.replace_all(&text, "\n");
    let text = tag.replace_all(&text, "");
    let text = entity.replace_all(&text, |c: &regex::Captures<'_>| {
        let e = &c[1];
        if let Some(n) = e.strip_prefix('#') {
            // String.fromCharCode: the number's low 16 bits as one UTF-16 unit.
            let n: f64 = n.parse().unwrap_or(0.0);
            let unit = if n.is_finite() { (n % 65536.0) as u32 } else { 0 };
            return char::from_u32(unit).unwrap_or('\u{fffd}').to_string();
        }
        match e.to_ascii_lowercase().as_str() {
            "amp" => "&".to_owned(),
            "lt" => "<".to_owned(),
            "gt" => ">".to_owned(),
            "quot" => "\"".to_owned(),
            "apos" => "'".to_owned(),
            "nbsp" => " ".to_owned(),
            _ => c[0].to_owned(),
        }
    });
    js_trim(&text).to_owned()
}

// ---------------------------------------------------------------------------------------------
// Focus.

/// What gaining or losing the keyboard sends, when the program asked to know (`?1004h`).
pub fn focus_report(focused: bool, modes: &Modes) -> Option<&'static [u8]> {
    modes.focus_reports.then_some(if focused { b"\x1b[I" } else { b"\x1b[O" })
}

// ---------------------------------------------------------------------------------------------
// The mouse.

/// A mouse button. `None` is a move with no button held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    None,
}

/// What the mouse did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    /// A move, with the button held (`MouseButton::None` for none).
    Move(MouseButton),
    WheelUp,
    WheelDown,
}

/// A mouse event over the terminal, at a cell (0-based) with the modifiers held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    pub col: u16,
    pub row: u16,
    pub action: MouseAction,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Whether the mouse goes to the program or selects text here, as xterm.js decided in 0.10
/// (`shouldForceSelection`): when the program asked for the mouse it gets it, except that
/// Shift forces a selection outside a Mac. On a Mac Shift+click goes to the program too (vim,
/// htop and tmux read it); xterm.js's Option+click option for that was off.
pub fn mouse_goes_to_program(modes: &Modes, shift: bool, platform: Platform) -> bool {
    modes.mouse != MouseProtocol::Off && !(shift && platform != Platform::Mac)
}

/// Writes mouse reports as xterm.js's `CoreMouseService` did, and drops repeated moves within
/// one cell. One per terminal.
#[derive(Clone, Debug, Default)]
pub struct MouseReporter {
    last: Option<MouseEvent>,
}

impl MouseReporter {
    pub fn new() -> Self {
        Self::default()
    }

    /// The report for `ev`, or `None` when the program didn't ask for this kind of event (or
    /// the place can't be written in its encoding).
    pub fn report(&mut self, ev: MouseEvent, modes: &Modes, size: Size) -> Option<Vec<u8>> {
        if ev.col >= size.cols || ev.row >= size.rows {
            return None;
        }
        let mut ev = ev;
        match ev.action {
            MouseAction::Press(MouseButton::None) | MouseAction::Release(MouseButton::None) => return None,
            _ => {}
        }
        if matches!(ev.action, MouseAction::Move(_)) && self.last == Some(ev) {
            return None;
        }
        let allowed = match modes.mouse {
            MouseProtocol::Off => false,
            MouseProtocol::Vt200 => !matches!(ev.action, MouseAction::Move(_)),
            MouseProtocol::Drag => !matches!(ev.action, MouseAction::Move(MouseButton::None)),
            MouseProtocol::Any => true,
        };
        if !allowed {
            return None;
        }
        let report = encode_mouse(&mut ev, modes.mouse_encoding);
        self.last = Some(ev);
        report
    }
}

/// xterm.js's `eventCode`.
fn mouse_code(ev: &MouseEvent, sgr: bool) -> u32 {
    let mut code = ((ev.ctrl as u32) * 16) | ((ev.shift as u32) * 4) | ((ev.alt as u32) * 8);
    let button = |b: MouseButton| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::None => 3,
    };
    match ev.action {
        MouseAction::WheelUp => code |= 64,
        MouseAction::WheelDown => code |= 65,
        MouseAction::Press(b) => code |= button(b),
        // Only SGR can say which button was let go; the others report "none".
        MouseAction::Release(b) => code |= if sgr { button(b) } else { 3 },
        MouseAction::Move(b) => code |= button(b) | 32,
    }
    code
}

fn encode_mouse(ev: &mut MouseEvent, encoding: MouseEncoding) -> Option<Vec<u8>> {
    let (col, row) = (ev.col as u32 + 1, ev.row as u32 + 1);
    match encoding {
        MouseEncoding::Sgr => {
            let release = matches!(ev.action, MouseAction::Release(_));
            Some(format!("{ESC}[<{};{col};{row}{}", mouse_code(ev, true), if release { 'm' } else { 'M' }).into_bytes())
        }
        MouseEncoding::Default => {
            let values = [mouse_code(ev, false) + 32, col + 32, row + 32];
            // Past what one byte holds there's no report (as vte and konsole; xterm.js too).
            if values.iter().any(|&v| v > 255) {
                return None;
            }
            let mut out = b"\x1b[M".to_vec();
            out.extend(values.iter().map(|&v| v as u8));
            Some(out)
        }
        MouseEncoding::Utf8 => {
            // Like the default, with values past 127 written as UTF-8 (up to 2047).
            let values = [mouse_code(ev, false) + 32, col + 32, row + 32];
            if values.iter().any(|&v| v > 2047) {
                return None;
            }
            let mut out = b"\x1b[M".to_vec();
            for v in values {
                let mut buf = [0u8; 4];
                out.extend_from_slice(char::from_u32(v)?.encode_utf8(&mut buf).as_bytes());
            }
            Some(out)
        }
    }
}

/// The wheel on the alternate screen when the program didn't ask for the mouse: an arrow up or
/// down per wheel event, as xterm.js did (vim, less and tmux scroll with it).
pub fn alternate_scroll(up: bool, modes: &Modes) -> &'static [u8] {
    match (up, modes.app_cursor) {
        (true, false) => b"\x1b[A",
        (false, false) => b"\x1b[B",
        (true, true) => b"\x1bOA",
        (false, true) => b"\x1bOB",
    }
}

/// How a wheel event measures its movement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WheelDelta {
    /// Pixels (touchpads, and mice on most systems).
    Pixels(f64),
    Lines(f64),
    Pages(f64),
}

/// Turns wheel events into whole lines, keeping the part of a line left over for the next
/// event: a touchpad's small steps, and the fractions of a line Windows reports for a precision
/// touchpad or a free-spinning wheel (winit's line delta is the wheel's 120ths), add up instead
/// of being lost.
///
/// Two kinds, as in 0.10: [`WheelLines::new`] for mouse reports and the arrows the wheel sends
/// on the alternate screen (xterm.js's `consumeWheelEvent`, which slowed small pixel steps to
/// 0.3 as a touchpad's), and [`WheelLines::history`] for scrolling the terminal's own history,
/// which 0.10 scrolled by the exact pixels.
#[derive(Clone, Debug, Default)]
pub struct WheelLines {
    partial: f64,
    /// Pixels exactly, without the touchpad slow-down.
    exact: bool,
}

impl WheelLines {
    /// How xterm.js scaled scrolling with Alt, Ctrl or Shift held (`fastScrollSensitivity`).
    pub const FAST: f64 = 5.0;

    /// For mouse reports and alternate-screen arrows.
    pub fn new() -> Self {
        Self::default()
    }

    /// For scrolling the history: pixel deltas over the cell height, nothing slowed.
    pub fn history() -> Self {
        WheelLines { partial: 0.0, exact: true }
    }

    /// Lines to scroll for one wheel event (negative is up): `cell_height` in the same units as
    /// pixel deltas, `rows` the terminal's height for page deltas, `fast` when Alt or Ctrl is held.
    pub fn lines(&mut self, delta: WheelDelta, cell_height: f64, rows: u16, fast: bool) -> i32 {
        let scale = if fast { Self::FAST } else { 1.0 };
        let amount = match delta {
            WheelDelta::Pixels(dy) => {
                if dy == 0.0 || cell_height <= 0.0 {
                    return 0;
                }
                let amount = dy * scale / cell_height;
                if dy.abs() < 50.0 && !self.exact {
                    // Most likely a touchpad.
                    amount * 0.3
                } else {
                    amount
                }
            }
            WheelDelta::Lines(dy) => dy * scale,
            WheelDelta::Pages(dy) => dy * scale * rows as f64,
        };
        if !amount.is_finite() {
            return 0;
        }
        self.partial += amount;
        let whole = self.partial.trunc();
        self.partial -= whole;
        whole.clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str, code: &str, bits: u8) -> KeyEvent {
        KeyEvent::new(Key::from_dom(k), Code::from_dom(code), Mods::from_bits(bits))
    }

    fn linux() -> KeyOptions {
        KeyOptions { platform: Platform::Other, mac_option_is_meta: true, app_cursor: false }
    }

    fn sent(ev: KeyEvent, opts: KeyOptions) -> String {
        match terminal_key(&ev, &opts) {
            KeyAction::Send(s) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn everyday_keys() {
        assert_eq!(sent(key("a", "KeyA", 0), linux()), "a");
        assert_eq!(sent(key("c", "KeyC", 4), linux()), "\x03");
        // Ctrl+С on a Russian layout is still ^C.
        assert_eq!(sent(key("с", "KeyC", 4), linux()), "\x03");
        // Alt+B on a Russian layout: Esc and the US letter at that place.
        assert_eq!(sent(key("и", "KeyB", 2), linux()), "\x1bb");
        assert_eq!(sent(key("ArrowUp", "ArrowUp", 0), linux()), "\x1b[A");
        assert_eq!(sent(key("ArrowUp", "ArrowUp", 0), KeyOptions { app_cursor: true, ..linux() }), "\x1bOA");
        assert_eq!(sent(key("ArrowLeft", "ArrowLeft", 4), linux()), "\x1b[1;5D");
        assert_eq!(sent(key("Backspace", "Backspace", 0), linux()), "\x7f");
        assert_eq!(sent(key("Backspace", "Backspace", 2), linux()), "\x1b\x7f");
        assert_eq!(sent(key("F5", "F5", 0), linux()), "\x1b[15~");
        assert_eq!(sent(key("Tab", "Tab", 1), linux()), "\x1b[Z");
    }

    #[test]
    fn shift_enter_is_a_line_feed() {
        assert_eq!(sent(key("Enter", "Enter", 1), linux()), "\n");
        assert_eq!(sent(key("Enter", "Enter", 0), linux()), "\r");
        assert_eq!(xterm_key(&key("Enter", "Enter", 1), &linux()), KeyAction::Send("\r".into()));
    }

    #[test]
    fn page_keys_with_shift_scroll() {
        assert_eq!(terminal_key(&key("PageUp", "PageUp", 1), &linux()), KeyAction::PageUp);
        assert_eq!(terminal_key(&key("PageDown", "PageDown", 1), &linux()), KeyAction::PageDown);
        assert_eq!(sent(key("PageUp", "PageUp", 0), linux()), "\x1b[5~");
    }

    #[test]
    fn mac_option_and_command() {
        let mac = KeyOptions { platform: Platform::Mac, mac_option_is_meta: true, app_cursor: false };
        assert_eq!(sent(key("å", "KeyA", 2), mac), "\x1ba");
        assert_eq!(terminal_key(&key("a", "KeyA", 8), &mac), KeyAction::SelectAll);
        assert_eq!(terminal_key(&key("ArrowLeft", "ArrowLeft", 8), &mac), KeyAction::Nothing);
    }

    #[test]
    fn altgr_on_windows_types_the_character() {
        let win = KeyOptions { platform: Platform::Windows, mac_option_is_meta: true, app_cursor: false };
        assert_eq!(sent(key("@", "KeyQ", 2 | 4 | 16), win), "@");
        assert_eq!(sent(key("{", "Digit7", 2 | 4), win), "{");
    }

    #[test]
    fn shortcut_keys() {
        assert!(is_terminal_paste(&key("v", "KeyV", 4), Platform::Windows));
        assert!(!is_terminal_paste(&key("v", "KeyV", 4), Platform::Other));
        assert!(is_terminal_paste(&key("V", "KeyV", 5), Platform::Other));
        assert!(is_terminal_paste(&key("м", "KeyV", 5), Platform::Other));
        assert!(!is_terminal_paste(&key("v", "KeyV", 8), Platform::Mac));
        assert_eq!(terminal_copy_key(&key("c", "KeyC", 4), Platform::Other), Some(CopyKey::CtrlC));
        assert_eq!(terminal_copy_key(&key("C", "KeyC", 5), Platform::Windows), Some(CopyKey::CtrlShiftC));
        assert_eq!(terminal_copy_key(&key("c", "KeyC", 4), Platform::Mac), None);
        assert!(is_mac_clear(&key("k", "KeyK", 8), Platform::Mac));
        assert!(!is_mac_clear(&key("k", "KeyK", 8), Platform::Other));
    }

    #[test]
    fn typing_or_not() {
        assert!(is_typing(b"a"));
        assert!(is_typing(b"\x1b[A"));
        assert!(!is_typing(b"\x1b[<0;3;4M"));
        assert!(!is_typing(b"\x1b[M #$"));
        assert!(!is_typing(b"\x1b[I"));
        assert!(!is_typing(b"\x1b[O"));
        assert!(is_typing(b"\x1b[Ox"));
    }

    #[test]
    fn pastes_lose_their_keys() {
        assert_eq!(without_keys("ls\r\n-la\rx\ty"), "ls\n-la\nx\ty");
        assert_eq!(without_keys("rm\x1b[201~ -rf\x15\u{9b}\u{85}"), "rm[201~ -rf");
        assert_eq!(paste("echo 1\necho 2\n", false), "echo 1\recho 2\r");
        assert_eq!(paste("a\x1b[201~b", true), "\x1b[200~a[201~b\x1b[201~");
        assert_eq!(typed_text("привет", true), "привет");
        assert_eq!(typed_text("one\r\ntwo", true), "\x1b[200~one\rtwo\x1b[201~");
    }

    #[test]
    fn pasted_text_from_clipboard_kinds() {
        assert_eq!(pasted_text("plain", "https://example.org", "<b>x</b>"), "plain");
        assert_eq!(pasted_text("", "# comment\r\nhttps://example.org/a\r\n\r\nhttps://example.org/b", ""), "https://example.org/a\nhttps://example.org/b");
        assert_eq!(
            pasted_text("", "", "<style>p{}</style><p>Hello&nbsp;<b>world</b> &amp; you<br/>bye &#33;&#x41; &unknown;</p><script>x()</script>"),
            "Hello world & you\nbye !&#x41; &unknown;"
        );
        assert_eq!(pasted_text("", "", "<img src=a.png>"), "");
    }

    #[test]
    fn focus_reports_only_when_asked() {
        let mut modes = Modes::default();
        assert_eq!(focus_report(true, &modes), None);
        modes.focus_reports = true;
        assert_eq!(focus_report(true, &modes), Some(&b"\x1b[I"[..]));
        assert_eq!(focus_report(false, &modes), Some(&b"\x1b[O"[..]));
    }

    fn mouse(col: u16, row: u16, action: MouseAction) -> MouseEvent {
        MouseEvent { col, row, action, shift: false, alt: false, ctrl: false }
    }

    #[test]
    fn sgr_mouse_reports() {
        let modes = Modes { mouse: MouseProtocol::Drag, mouse_encoding: MouseEncoding::Sgr, ..Modes::default() };
        let size = Size::new(80, 24);
        let mut r = MouseReporter::new();
        assert_eq!(r.report(mouse(4, 2, MouseAction::Press(MouseButton::Left)), &modes, size).unwrap(), b"\x1b[<0;5;3M");
        assert_eq!(r.report(mouse(5, 2, MouseAction::Move(MouseButton::Left)), &modes, size).unwrap(), b"\x1b[<32;6;3M");
        // The same move again is dropped.
        assert_eq!(r.report(mouse(5, 2, MouseAction::Move(MouseButton::Left)), &modes, size), None);
        assert_eq!(r.report(mouse(5, 2, MouseAction::Release(MouseButton::Left)), &modes, size).unwrap(), b"\x1b[<0;6;3m");
        // Drag mode doesn't report moves without a button.
        assert_eq!(r.report(mouse(9, 9, MouseAction::Move(MouseButton::None)), &modes, size), None);
        assert_eq!(r.report(mouse(0, 0, MouseAction::WheelUp), &modes, size).unwrap(), b"\x1b[<64;1;1M");
        let ctrl = MouseEvent { ctrl: true, ..mouse(0, 0, MouseAction::WheelDown) };
        assert_eq!(r.report(ctrl, &modes, size).unwrap(), b"\x1b[<81;1;1M");
        // Outside the terminal: nothing.
        assert_eq!(r.report(mouse(80, 0, MouseAction::Press(MouseButton::Left)), &modes, size), None);
    }

    #[test]
    fn default_and_utf8_mouse_reports() {
        let size = Size::new(300, 50);
        let mut modes = Modes { mouse: MouseProtocol::Vt200, ..Modes::default() };
        let mut r = MouseReporter::new();
        assert_eq!(r.report(mouse(0, 0, MouseAction::Press(MouseButton::Right)), &modes, size).unwrap(), b"\x1b[M\x22\x21\x21");
        assert_eq!(r.report(mouse(0, 0, MouseAction::Release(MouseButton::Right)), &modes, size).unwrap(), b"\x1b[M\x23\x21\x21");
        // VT200 reports no moves.
        assert_eq!(r.report(mouse(1, 0, MouseAction::Move(MouseButton::Left)), &modes, size), None);
        // Column 224 can't be written in one byte.
        assert_eq!(r.report(mouse(223, 0, MouseAction::Press(MouseButton::Left)), &modes, size), None);
        modes.mouse_encoding = MouseEncoding::Utf8;
        let report = r.report(mouse(223, 0, MouseAction::Press(MouseButton::Left)), &modes, size).unwrap();
        assert_eq!(report, "\x1b[M\u{20}\u{100}\u{21}".as_bytes());
        // Off: nothing at all.
        modes.mouse = MouseProtocol::Off;
        assert_eq!(r.report(mouse(0, 0, MouseAction::Press(MouseButton::Left)), &modes, size), None);
    }

    #[test]
    fn mouse_belongs_to_program_unless_shift_outside_a_mac() {
        let modes = Modes { mouse: MouseProtocol::Vt200, ..Modes::default() };
        for platform in [Platform::Other, Platform::Windows, Platform::Mac] {
            assert!(mouse_goes_to_program(&modes, false, platform));
            assert!(!mouse_goes_to_program(&Modes::default(), false, platform));
        }
        assert!(!mouse_goes_to_program(&modes, true, Platform::Other));
        assert!(!mouse_goes_to_program(&modes, true, Platform::Windows));
        // On a Mac Shift+click is the program's (xterm.js forced a selection there only with
        // Option, and only when asked to).
        assert!(mouse_goes_to_program(&modes, true, Platform::Mac));
    }

    #[test]
    fn alternate_scroll_sends_arrows() {
        let mut modes = Modes::default();
        assert_eq!(alternate_scroll(true, &modes), b"\x1b[A");
        modes.app_cursor = true;
        assert_eq!(alternate_scroll(false, &modes), b"\x1bOB");
    }

    #[test]
    fn wheel_lines_add_up() {
        let mut w = WheelLines::new();
        // A touchpad's small steps: 0.3 of half a 20 px line each, 0.15 of a line.
        let mut total = 0;
        for _ in 0..21 {
            total += w.lines(WheelDelta::Pixels(10.0), 20.0, 24, false);
        }
        assert_eq!(total, 3);
        // A mouse notch of 100 px is 5 lines of 20 px.
        assert_eq!(WheelLines::new().lines(WheelDelta::Pixels(-100.0), 20.0, 24, false), -5);
        assert_eq!(WheelLines::new().lines(WheelDelta::Lines(3.0), 20.0, 24, true), 15);
        assert_eq!(WheelLines::new().lines(WheelDelta::Pages(1.0), 20.0, 24, false), 24);
    }

    #[test]
    fn fractions_of_a_line_add_up() {
        // Windows' precision touchpad: an eighth of a notch (a line) at a time.
        for mut w in [WheelLines::new(), WheelLines::history()] {
            let total: i32 = (0..40).map(|_| w.lines(WheelDelta::Lines(-0.125), 20.0, 24, false)).sum();
            assert_eq!(total, -5);
        }
        let mut w = WheelLines::new();
        let total: i32 = (0..10).map(|_| w.lines(WheelDelta::Pages(0.1), 20.0, 24, false)).sum();
        assert_eq!(total, 24);
        // Nothing left over is lost when the direction stays.
        let mut w = WheelLines::new();
        assert_eq!(w.lines(WheelDelta::Lines(0.6), 20.0, 24, false), 0);
        assert_eq!(w.lines(WheelDelta::Lines(0.6), 20.0, 24, false), 1);
    }

    #[test]
    fn history_scrolls_by_the_exact_pixels() {
        // The same touchpad steps as above: 10 px of 20 px lines are half a line each.
        let mut w = WheelLines::history();
        let total: i32 = (0..21).map(|_| w.lines(WheelDelta::Pixels(10.0), 20.0, 24, false)).sum();
        assert_eq!(total, 10);
        assert_eq!(WheelLines::history().lines(WheelDelta::Pixels(-100.0), 20.0, 24, false), -5);
    }

    #[test]
    fn ctrl_alt_types_only_what_the_system_typed() {
        let win = KeyOptions { platform: Platform::Windows, mac_option_is_meta: true, app_cursor: false };
        // Ctrl+Alt+C on a US or Russian layout types nothing on Windows: nothing goes out.
        assert_eq!(xterm_key(&key("c", "KeyC", 6).with_text(None), &win), KeyAction::Nothing);
        assert_eq!(xterm_key(&key("с", "KeyC", 6).with_text(None), &win), KeyAction::Nothing);
        // AltGr+Q on a German layout types @, which goes out.
        assert_eq!(xterm_key(&key("@", "KeyQ", 6).with_text(Some('@')), &win), KeyAction::Send("@".into()));
        // A dead key's chord types nothing yet.
        assert_eq!(xterm_key(&key("~", "BracketRight", 6).with_text(None), &win), KeyAction::Nothing);
        // Elsewhere Ctrl+Alt+C is Esc and ^C, whatever was typed.
        assert_eq!(xterm_key(&key("c", "KeyC", 6).with_text(None), &linux()), KeyAction::Send("\x1b\x03".into()));
    }
}
