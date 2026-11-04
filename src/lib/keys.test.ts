import { afterEach, describe, expect, it, vi } from "vitest";
import {
  defaultKeys,
  isAltAlone,
  isMac,
  isMacClear,
  isModLetter,
  isTerminalPaste,
  keyOf,
  keysFromEvent,
  keysLabel,
  matchShortcut,
  missingKeyCode,
  pauseShortcuts,
  reservedReason,
  setShortcutSource,
  SHORTCUT_GROUPS,
  SHORTCUT_IDS,
  shortcutKeys,
  shortcutProblem,
  terminalCopyKey,
  type Keys,
  type ShortcutOverrides,
} from "./keys";

/** A keydown as the browser would send it. */
function press(code: string, mods: { ctrl?: boolean; meta?: boolean; alt?: boolean; shift?: boolean; key?: string; keyCode?: number } = {}): KeyboardEvent {
  const key = mods.key ?? (code.startsWith("Key") ? code.slice(3).toLowerCase() : code);
  return {
    code,
    key,
    keyCode: mods.keyCode ?? 65,
    ctrlKey: !!mods.ctrl,
    metaKey: !!mods.meta,
    altKey: !!mods.alt,
    shiftKey: !!mods.shift,
    getModifierState: () => false,
  } as unknown as KeyboardEvent;
}

const use = (o: ShortcutOverrides) => setShortcutSource(() => o);
afterEach(() => {
  use({});
  pauseShortcuts(false);
});

describe("defaults", () => {
  it("use Ctrl+Shift outside the Mac and ⌘ on it", () => {
    expect(keysLabel(defaultKeys("splitRight", false)!, false)).toBe("Ctrl+Shift+D");
    expect(keysLabel(defaultKeys("settings", false)!, false)).toBe("Ctrl+,");
    expect(keysLabel(defaultKeys("splitDown", true)!, true)).toBe("⇧⌘D");
    expect(keysLabel(defaultKeys("zoomPane", true)!, true)).toBe("⇧⌘↩");
  });

  it("give the command bar a second key only outside the Mac", () => {
    expect(keysLabel(defaultKeys("paletteAlt", false)!, false)).toBe("Ctrl+Shift+K");
    expect(defaultKeys("paletteAlt", true)).toBeNull();
  });

  it("don't clash with each other or with the terminal", () => {
    use({});
    for (const mac of [false, true]) {
      for (const id of SHORTCUT_IDS) {
        const k = defaultKeys(id, mac);
        if (k) expect(shortcutProblem(id, k, mac, false), `${id} mac=${mac}`).toBeNull();
      }
    }
  });

  it("let only finding have Ctrl+F, as it gives the key back to programs that read it", () => {
    use({});
    const ctrlF = { key: "f", ctrl: true };
    expect(shortcutProblem("find", ctrlF, false, false)).toBeNull();
    expect(shortcutProblem("sidebar", ctrlF, false, false)).not.toBeNull();
    expect(shortcutProblem("find", { key: "g", ctrl: true }, false, false)).not.toBeNull();
  });

  it("are all listed in Settings, the command bar's second key on its row", () => {
    const listed = SHORTCUT_GROUPS.flatMap((g) => g.ids);
    expect(new Set(listed).size).toBe(listed.length);
    expect([...listed, "paletteAlt"].sort()).toEqual([...SHORTCUT_IDS].sort());
  });
});

describe("matching", () => {
  it("goes by the key's place, so other layouts work", () => {
    // Ctrl+Shift+L on a Russian layout types "Д", but its code is still KeyL.
    expect(matchShortcut(press("KeyL", { ctrl: true, shift: true, key: "Д" }))).toBe("palette");
    expect(matchShortcut(press("KeyK", { ctrl: true, shift: true }))).toBe("palette");
    expect(matchShortcut(press("Comma", { ctrl: true, key: "," }))).toBe("settings");
    expect(matchShortcut(press("Enter", { ctrl: true, shift: true, key: "Enter" }))).toBe("zoomPane");
  });

  it("leaves Ctrl+letter, Alt and AltGr alone", () => {
    expect(matchShortcut(press("KeyD", { ctrl: true }))).toBeNull();
    expect(matchShortcut(press("KeyD", { ctrl: true, shift: true, alt: true }))).toBeNull();
    expect(matchShortcut(press("KeyL", { ctrl: true, alt: true }))).toBeNull();
  });

  it("follows your own keys", () => {
    use({ splitRight: { key: "f6", ctrl: true } });
    expect(matchShortcut(press("F6", { ctrl: true }))).toBe("splitRight");
    expect(matchShortcut(press("KeyD", { ctrl: true, shift: true }))).toBeNull();
    expect(keysLabel(shortcutKeys("splitRight", false)!, false)).toBe("Ctrl+F6");
  });

  it("can drop the command bar's second key", () => {
    use({ paletteAlt: null });
    expect(matchShortcut(press("KeyK", { ctrl: true, shift: true }))).toBeNull();
    expect(matchShortcut(press("KeyL", { ctrl: true, shift: true }))).toBe("palette");
  });

  it("ignores broken stored keys", () => {
    use({ splitRight: { key: "" } as Keys, splitDown: { key: "x", ctrl: "yes" } as unknown as Keys });
    expect(matchShortcut(press("KeyD", { ctrl: true, shift: true }))).toBe("splitRight");
    expect(matchShortcut(press("KeyE", { ctrl: true, shift: true }))).toBe("splitDown");
  });

  it("stays quiet while new keys are recorded", () => {
    pauseShortcuts(true);
    expect(matchShortcut(press("KeyL", { ctrl: true, shift: true }))).toBeNull();
  });
});

describe("recording", () => {
  it("waits for a key that isn't a modifier", () => {
    expect(keysFromEvent(press("ShiftLeft", { shift: true, key: "Shift" }))).toBeNull();
    expect(keysFromEvent(press("ControlLeft", { ctrl: true, key: "Control" }))).toBeNull();
    expect(keysFromEvent(press("KeyP", { ctrl: true, shift: true, key: "P" }))).toEqual({ key: "p", ctrl: true, shift: true });
    expect(keysFromEvent(press("Digit1", { ctrl: true, shift: true, key: "!" }))).toEqual({ key: "1", ctrl: true, shift: true });
    expect(keysFromEvent(press("ArrowUp", { meta: true, key: "ArrowUp" }))).toEqual({ key: "up", meta: true });
  });

  it("counts AltGr as Alt", () => {
    const e = { ...press("KeyQ", { key: "@" }), getModifierState: (m: string) => m === "AltGraph" } as unknown as KeyboardEvent;
    expect(keysFromEvent(e)).toEqual({ key: "q", alt: true });
  });
});

describe("refusing keys", () => {
  const other = (k: Keys) => reservedReason(k, false, true);
  const mac = (k: Keys) => reservedReason(k, true, false);

  it("keeps Ctrl+letter, Alt, AltGr and plain keys for the terminal on Windows and Linux", () => {
    expect(other({ key: "r", ctrl: true })).toMatch(/Ctrl\+R.*add Shift/);
    expect(other({ key: "left", ctrl: true })).toMatch(/add Shift/);
    expect(other({ key: "x", ctrl: true, alt: true })).toMatch(/AltGr/);
    expect(other({ key: "x", alt: true })).toMatch(/Alt/);
    expect(other({ key: "x", shift: true })).toMatch(/Hold Ctrl/);
    expect(other({ key: "f5" })).toMatch(/Hold Ctrl/);
    expect(other({ key: "x", ctrl: true, shift: true, meta: true })).toMatch(/Windows key/);
    expect(reservedReason({ key: "x", meta: true }, false, false)).toMatch(/Super key/);
  });

  it("keeps terminal copy and paste, tab switching and Esc", () => {
    expect(other({ key: "c", ctrl: true, shift: true })).toMatch(/copy and paste/);
    expect(other({ key: "v", ctrl: true, shift: true })).toMatch(/copy and paste/);
    expect(other({ key: "tab", ctrl: true })).toMatch(/switch tabs/);
    expect(other({ key: "escape", ctrl: true, shift: true })).toMatch(/Esc/);
  });

  it("allows Ctrl+Shift with anything else, and Ctrl with a few quiet keys", () => {
    for (const k of [
      { key: "p", ctrl: true, shift: true },
      { key: "1", ctrl: true, shift: true },
      { key: "up", ctrl: true, shift: true },
      { key: ".", ctrl: true },
      { key: "f6", ctrl: true },
    ]) {
      expect(other(k), JSON.stringify(k)).toBeNull();
    }
  });

  it("keeps ⌘K, copy, paste, editing and system keys on the Mac", () => {
    expect(mac({ key: "k", meta: true })).toMatch(/clears the terminal/);
    expect(mac({ key: "c", meta: true })).toMatch(/copies/);
    expect(mac({ key: "v", meta: true })).toMatch(/pastes/);
    expect(mac({ key: "z", meta: true })).toMatch(/undoes/);
    expect(mac({ key: "q", meta: true })).toMatch(/quits/);
    expect(mac({ key: "space", meta: true })).toMatch(/macOS uses ⌘Space/);
    expect(mac({ key: "tab", meta: true })).toMatch(/macOS/);
    expect(mac({ key: "3", meta: true, shift: true })).toMatch(/macOS/);
  });

  it("asks for ⌘ on the Mac", () => {
    expect(mac({ key: "r", ctrl: true })).toMatch(/hold ⌘/);
    expect(mac({ key: "e", alt: true })).toMatch(/accented/);
    expect(mac({ key: "r" })).toMatch(/Hold ⌘/);
  });

  it("allows other ⌘ keys, with ⇧, ⌥ or ⌃ too", () => {
    for (const k of [
      { key: "p", meta: true },
      { key: "k", meta: true, shift: true },
      { key: "p", meta: true, alt: true },
      { key: "1", meta: true },
    ]) {
      expect(mac(k), JSON.stringify(k)).toBeNull();
    }
  });
});

describe("conflicts", () => {
  it("refuses keys another shortcut has, naming it", () => {
    expect(shortcutProblem("vitals", { key: "d", ctrl: true, shift: true }, false, false)).toMatch(/Ctrl\+Shift\+D already does “Split right”/);
    expect(shortcutProblem("vitals", { key: "d", meta: true }, true, false)).toMatch(/⌘D already does “Split right”/);
  });

  it("sees your own keys, and frees the old ones", () => {
    use({ splitRight: { key: "y", ctrl: true, shift: true } });
    expect(shortcutProblem("vitals", { key: "y", ctrl: true, shift: true }, false, false)).toMatch(/Split right/);
    expect(shortcutProblem("vitals", { key: "d", ctrl: true, shift: true }, false, false)).toBeNull();
  });

  it("lets a shortcut keep its own keys", () => {
    expect(shortcutProblem("splitRight", { key: "d", ctrl: true, shift: true }, false, false)).toBeNull();
  });

  it("won't give the command bar the same key twice", () => {
    expect(shortcutProblem("paletteAlt", { key: "l", ctrl: true, shift: true }, false, false)).toMatch(/already opens the command bar/);
  });
});

describe("isAltAlone", () => {
  const key = (k: string, mods: Partial<KeyboardEvent> = {}) => ({ key: k, ctrlKey: false, shiftKey: false, metaKey: false, altKey: false, ...mods }) as KeyboardEvent;

  it("is Alt, pressed or let go, with nothing else held", () => {
    expect(isAltAlone(key("Alt", { altKey: true }))).toBe(true);
    expect(isAltAlone(key("Alt"))).toBe(true);
  });

  it("leaves the keys it comes with alone: Alt+Space's menu, Alt+F4", () => {
    expect(isAltAlone(key(" ", { altKey: true }))).toBe(false);
    expect(isAltAlone(key("F4", { altKey: true }))).toBe(false);
  });

  it("leaves Alt+Shift, which switches the keyboard language, and AltGr, which types", () => {
    expect(isAltAlone(key("Alt", { altKey: true, shiftKey: true }))).toBe(false);
    expect(isAltAlone(key("AltGraph", { ctrlKey: true, altKey: true }))).toBe(false);
    expect(isAltAlone(key("Alt", { ctrlKey: true, altKey: true }))).toBe(false);
  });
});

describe("keys on other layouts", () => {
  const both = { ctrl: true, shift: true };

  it("find the letter marked on the key where the layout has Latin letters", () => {
    // A German keyboard swaps Z and Y; a French one swaps Z and W, and A and Q.
    expect(matchShortcut(press("KeyY", { ...both, key: "Z" }))).toBe("reopenTab");
    expect(matchShortcut(press("KeyZ", { ...both, key: "Y" }))).toBeNull();
    expect(matchShortcut(press("KeyW", { ...both, key: "Z" }))).toBe("reopenTab");
    expect(matchShortcut(press("KeyZ", { ...both, key: "W" }))).toBe("closePane");
    // Dvorak moves the letters onto other keys altogether.
    expect(matchShortcut(press("KeyP", { ...both, key: "L" }))).toBe("palette");
    expect(matchShortcut(press("Comma", { ...both, key: "W" }))).toBe("closePane");
  });

  it("go by the key's place on a layout without them", () => {
    for (const [code, key, letter] of [
      ["KeyL", "Д", "l"],
      ["KeyL", "д", "l"],
      ["KeyW", "Ц", "w"],
      ["KeyL", "Λ", "l"],
      ["KeyI", "ı", "i"],
    ]) expect(keyOf(press(code, { key }))).toBe(letter);
    expect(keyOf(press("Comma", { ctrl: true, key: "б" }))).toBe(",");
    expect(matchShortcut(press("Comma", { ctrl: true, key: "б" }))).toBe("settings");
  });

  it("keep digits and other keys by their place", () => {
    expect(keyOf(press("Digit1", { key: "&" }))).toBe("1");
    expect(keyOf(press("Comma", { shift: true, key: "<" }))).toBe(",");
    expect(keyOf(press("Enter", { key: "Enter" }))).toBe("enter");
  });

  it("record the letter marked on the key", () => {
    expect(keysFromEvent(press("KeyY", { ...both, key: "Z" }))).toEqual({ key: "z", ctrl: true, shift: true });
    expect(keysFromEvent(press("KeyL", { ...both, key: "Д" }))).toEqual({ key: "l", ctrl: true, shift: true });
  });

  it("copy, paste and clear the same way", () => {
    if (isMac) {
      expect(isMacClear(press("KeyK", { meta: true, key: "л" }))).toBe(true);
      expect(isMacClear(press("KeyV", { meta: true, key: "k" }))).toBe(true);
      return;
    }
    expect(terminalCopyKey(press("KeyC", { ctrl: true, key: "с" }))).toBe("ctrl-c");
    expect(terminalCopyKey(press("KeyC", { ...both, key: "С" }))).toBe("ctrl-shift-c");
    // Dvorak's C is at QWERTY's I, and what's at QWERTY's C types J.
    expect(terminalCopyKey(press("KeyI", { ctrl: true, key: "c" }))).toBe("ctrl-c");
    expect(terminalCopyKey(press("KeyC", { ctrl: true, key: "j" }))).toBeNull();
    expect(isTerminalPaste(press("KeyV", { ...both, key: "М" }))).toBe(true);
    expect(isTerminalPaste(press("Period", { ...both, key: "V" }))).toBe(true);
    expect(isTerminalPaste(press("KeyV", { ...both, key: "K" }))).toBe(false);
  });
});

describe("a key Windows presses by itself", () => {
  // Clipboard history's Ctrl+V has no place on the keyboard (no code).
  const byWindows = (key: string, keyCode: number, mods = {}) => press("", { ctrl: true, key, keyCode, ...mods });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("is still the letter it stands for", () => {
    expect(keyOf(byWindows("v", 86))).toBe("v");
    expect(keyOf(byWindows("м", 86))).toBe("v");
    expect(keyOf(press("", { key: "Enter", keyCode: 13 }))).toBe("enter");
  });

  it("pastes on Windows, on any layout", async () => {
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)" });
    vi.resetModules();
    const windows = await import("./keys");
    expect(windows.isTerminalPaste(byWindows("v", 86))).toBe(true);
    expect(windows.isTerminalPaste(byWindows("м", 86))).toBe(true);
    expect(windows.isModLetter(byWindows("м", 86), "v")).toBe(true);
    // Only that key, and only with Ctrl alone.
    expect(windows.isTerminalPaste(byWindows("с", 67))).toBe(false);
    expect(windows.isTerminalPaste(byWindows("м", 86, { alt: true }))).toBe(false);
    expect(windows.isTerminalPaste(byWindows("м", 86, { meta: true }))).toBe(false);
  });
});

describe("missingKeyCode", () => {
  // What WebKit on Linux says for keys on a layout without Latin letters.
  const none = { keyCode: 0 };

  it("gives Ctrl and Alt combinations the code of the key's place", () => {
    expect(missingKeyCode(press("KeyC", { ...none, ctrl: true, key: "с" }))).toBe(67);
    expect(missingKeyCode(press("KeyB", { ...none, alt: true, key: "и" }))).toBe(66);
    expect(missingKeyCode(press("BracketLeft", { ...none, ctrl: true, key: "х" }))).toBe(219);
    expect(missingKeyCode(press("Backslash", { ...none, ctrl: true, key: "\\" }))).toBe(220);
  });

  it("leaves typing alone, and whatever already has a code", () => {
    expect(missingKeyCode(press("KeyC", { ...none, key: "с" }))).toBeNull();
    expect(missingKeyCode(press("KeyC", { ...none, shift: true, key: "С" }))).toBeNull();
    expect(missingKeyCode(press("KeyC", { ...none, meta: true, key: "с" }))).toBeNull();
    expect(missingKeyCode(press("KeyC", { keyCode: 67, ctrl: true, key: "с" }))).toBeNull();
    expect(missingKeyCode(press("F13", { ...none, ctrl: true, key: "F13" }))).toBeNull();
  });
});

describe("isModLetter", () => {
  const mod = isMac ? { meta: true } : { ctrl: true };

  it("finds the letter on a layout without Latin letters and with Caps Lock", () => {
    expect(isModLetter(press("KeyA", mod), "a")).toBe(true);
    expect(isModLetter(press("KeyA", { ...mod, key: "ф" }), "a")).toBe(true);
    expect(isModLetter(press("KeyA", { ...mod, key: "A" }), "a")).toBe(true);
  });

  it("goes by the letter typed where the layout has Latin letters", () => {
    // AZERTY has its A where QWERTY has Q, and the other way round.
    expect(isModLetter(press("KeyQ", { ...mod, key: "a" }), "a")).toBe(true);
    expect(isModLetter(press("KeyA", { ...mod, key: "q" }), "a")).toBe(false);
  });

  it("leaves other keys and combinations alone", () => {
    expect(isModLetter(press("KeyA"), "a")).toBe(false);
    expect(isModLetter(press("KeyA", { ...mod, shift: true }), "a")).toBe(false);
    expect(isModLetter(press("KeyA", { ...mod, alt: true }), "a")).toBe(false);
    expect(isModLetter(press("KeyB", mod), "a")).toBe(false);
  });
});
