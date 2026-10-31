import { afterEach, describe, expect, it } from "vitest";
import {
  defaultKeys,
  keysFromEvent,
  keysLabel,
  matchShortcut,
  pauseShortcuts,
  reservedReason,
  setShortcutSource,
  SHORTCUT_GROUPS,
  SHORTCUT_IDS,
  shortcutKeys,
  shortcutProblem,
  type Keys,
  type ShortcutOverrides,
} from "./keys";

/** A keydown as the browser would send it. */
function press(code: string, mods: { ctrl?: boolean; meta?: boolean; alt?: boolean; shift?: boolean; key?: string } = {}): KeyboardEvent {
  const key = mods.key ?? (code.startsWith("Key") ? code.slice(3).toLowerCase() : code);
  return {
    code,
    key,
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
