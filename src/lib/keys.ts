// Keyboard shortcuts. On macOS they use ⌘; elsewhere Ctrl+Shift, so plain
// Ctrl keys (Ctrl+C, Ctrl+D, Ctrl+W…) keep working inside terminals.
// Settings → Keyboard changes them, within the same rules (reservedReason).

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);
export const isWindows = typeof navigator !== "undefined" && /Windows/.test(navigator.userAgent);

/**
 * Ctrl+V pastes in a terminal on Windows, as in Windows Terminal, instead of
 * sending ^V. Clipboard history (Win+V) pastes by pressing Ctrl+V for you.
 * Ctrl+Shift+V pastes too.
 */
export function isTerminalPaste(e: KeyboardEvent): boolean {
  if (isMac || !e.ctrlKey || e.altKey || e.metaKey || e.code !== "KeyV") return false;
  // Ctrl+Shift+V everywhere else, as in Linux terminals.
  return isWindows || e.shiftKey;
}

/**
 * Copy keys in a terminal outside macOS (where ⌘C just works): Ctrl+Shift+C,
 * and Ctrl+C while text is selected, as in Windows Terminal. Without a
 * selection, Ctrl+C stops the running command as always.
 */
export function terminalCopyKey(e: KeyboardEvent): "ctrl-c" | "ctrl-shift-c" | null {
  if (isMac || !e.ctrlKey || e.altKey || e.metaKey || e.code !== "KeyC") return null;
  return e.shiftKey ? "ctrl-shift-c" : "ctrl-c";
}

/**
 * Shift+Enter in a terminal types a line feed (Ctrl+J): a new line in Claude
 * Code, OpenCode and Codex, and the same as Enter in a shell.
 */
export function isTerminalNewline(e: KeyboardEvent): boolean {
  return e.key === "Enter" && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey;
}

export type ShortcutId =
  | "palette"
  | "paletteAlt"
  | "splitRight"
  | "splitDown"
  | "closePane"
  | "newTerminal"
  | "reopenTab"
  | "toggleSplit"
  | "addHost"
  | "settings"
  | "sidebar"
  | "vitals"
  | "zoomPane"
  | "streamer"
  | "compact";

interface Combo {
  key: string;
  /** On Mac: Shift is added. Elsewhere Shift is the default; `false` drops it. */
  shift?: boolean;
}

// The command bar sits on L, like a browser's address bar. On a Mac ⌘K
// clears the terminal, as in every Mac terminal, so the bar has only ⌘L;
// elsewhere Ctrl+Shift+K still opens it too, as it always did.
const MAC: Record<ShortcutId, Combo | null> = {
  palette: { key: "l" },
  paletteAlt: null,
  splitRight: { key: "d" },
  splitDown: { key: "d", shift: true },
  closePane: { key: "w" },
  newTerminal: { key: "t" },
  reopenTab: { key: "t", shift: true },
  toggleSplit: { key: "s", shift: true },
  addHost: { key: "n", shift: true },
  settings: { key: "," },
  sidebar: { key: "b" },
  vitals: { key: "j" },
  zoomPane: { key: "enter", shift: true },
  streamer: { key: "h", shift: true },
  compact: { key: "m", shift: true },
};

const OTHER: Record<ShortcutId, Combo | null> = {
  palette: { key: "l" },
  paletteAlt: { key: "k" },
  splitRight: { key: "d" },
  splitDown: { key: "e" },
  closePane: { key: "w" },
  newTerminal: { key: "t" },
  // Ctrl+Shift+T is a new tab, as in Windows Terminal; Z undoes a close.
  reopenTab: { key: "z" },
  toggleSplit: { key: "s" },
  addHost: { key: "n" },
  settings: { key: ",", shift: false },
  sidebar: { key: "b" },
  vitals: { key: "j" },
  zoomPane: { key: "enter" },
  streamer: { key: "h" },
  compact: { key: "m" },
};

export const SHORTCUT_IDS = Object.keys(OTHER) as ShortcutId[];

/** Plain-language names, as Settings → Keyboard lists them. */
export const SHORTCUT_NAMES: Record<ShortcutId, string> = {
  palette: "Command bar",
  paletteAlt: "Command bar",
  splitRight: "Split right",
  splitDown: "Split down",
  closePane: "Close tab",
  newTerminal: "New terminal",
  reopenTab: "Reopen closed tab",
  toggleSplit: "Tabs side by side, or one at a time",
  addHost: "Add a server",
  settings: "Settings",
  sidebar: "Show or hide the sidebar",
  vitals: "Show or hide server vitals",
  zoomPane: "Fill the window with a pane",
  streamer: "Streamer mode",
  compact: "Compact mode",
};

/** How Settings → Keyboard groups them. The command bar's second key shows on its row. */
export const SHORTCUT_GROUPS: { title: string; ids: ShortcutId[] }[] = [
  { title: "Find and open", ids: ["palette", "addHost", "settings"] },
  { title: "Tabs and panes", ids: ["newTerminal", "closePane", "reopenTab", "splitRight", "splitDown", "zoomPane", "toggleSplit"] },
  { title: "Show and hide", ids: ["sidebar", "vitals", "compact", "streamer"] },
];

/** A key and the modifiers held with it. Settings stores your own shortcuts this way. */
export interface Keys {
  /** The key by its place on the keyboard: "k", "5", ",", "enter", "f5", "up"… */
  key: string;
  ctrl?: boolean;
  meta?: boolean;
  alt?: boolean;
  shift?: boolean;
}

/** Shortcuts you changed. `null` takes the keys away (offered only for the command bar's second key). */
export type ShortcutOverrides = Partial<Record<ShortcutId, Keys | null>>;

// Settings hands its overrides over here: keys.ts can't import settings.ts, which imports it.
let overrideSource: () => ShortcutOverrides = () => ({});
export function setShortcutSource(get: () => ShortcutOverrides) {
  overrideSource = get;
}

// While Settings records new keys, the shortcuts stay quiet, so pressing one doesn't fire it.
let paused = false;
export function pauseShortcuts(on: boolean) {
  paused = on;
}

/** Leaves out modifiers that aren't held, so equal combos look equal. */
function tidy(k: Keys): Keys {
  const out: Keys = { key: k.key };
  for (const m of ["ctrl", "meta", "alt", "shift"] as const) if (k[m]) out[m] = true;
  return out;
}

function isKeys(v: unknown): v is Keys {
  if (!v || typeof v !== "object") return false;
  const k = v as Record<string, unknown>;
  return typeof k.key === "string" && k.key !== "" && ["ctrl", "meta", "alt", "shift"].every((m) => k[m] === undefined || typeof k[m] === "boolean");
}

/** The keys a shortcut comes with: ⌘ (and ⇧) on a Mac, Ctrl+Shift (or plain Ctrl) elsewhere. */
export function defaultKeys(id: ShortcutId, mac = isMac): Keys | null {
  const c = (mac ? MAC : OTHER)[id];
  if (!c) return null;
  return tidy(mac ? { key: c.key, meta: true, shift: c.shift } : { key: c.key, ctrl: true, shift: c.shift ?? true });
}

/** The keys a shortcut has now: yours if you changed them, else its own. */
export function shortcutKeys(id: ShortcutId, mac = isMac): Keys | null {
  const mine = overrideSource()?.[id];
  if (mine === null) return null;
  return isKeys(mine) ? tidy(mine) : defaultKeys(id, mac);
}

export function sameKeys(a: Keys, b: Keys): boolean {
  return a.key === b.key && !!a.ctrl === !!b.ctrl && !!a.meta === !!b.meta && !!a.alt === !!b.alt && !!a.shift === !!b.shift;
}

const CODE_KEYS: Record<string, string> = {
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
  Space: "space",
  Enter: "enter",
  NumpadEnter: "enter",
  Tab: "tab",
  Backspace: "backspace",
  Delete: "delete",
  Insert: "insert",
  Escape: "escape",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
};

/** The key pressed, by its place on the keyboard, so other layouts (Russian, German…) work too. */
export function keyOf(e: KeyboardEvent): string {
  const m = /^(?:Key([A-Z])|Digit(\d)|(F\d{1,2}))$/.exec(e.code ?? "");
  if (m) return (m[1] ?? m[2] ?? m[3]).toLowerCase();
  return CODE_KEYS[e.code] ?? e.key.toLowerCase();
}

export function matches(e: KeyboardEvent, id: ShortcutId): boolean {
  const k = shortcutKeys(id);
  if (!k || keyOf(e) !== k.key) return false;
  return e.ctrlKey === !!k.ctrl && e.metaKey === !!k.meta && e.altKey === !!k.alt && e.shiftKey === !!k.shift;
}

export function matchShortcut(e: KeyboardEvent): ShortcutId | null {
  // Every shortcut needs Ctrl or ⌘; plain typing never gets further than this.
  if (!e.ctrlKey && !e.metaKey) return null;
  if (paused) return null;
  for (const id of SHORTCUT_IDS) if (matches(e, id)) return id === "paletteAlt" ? "palette" : id;
  return null;
}

/** ⌘K on a Mac: clear the terminal, as Terminal, iTerm2 and Ghostty do. */
export function isMacClear(e: KeyboardEvent): boolean {
  return isMac && e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && e.code === "KeyK";
}

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "AltGraph", "Meta", "OS", "Super", "Hyper", "Fn", "CapsLock"]);

/** What was pressed, for recording a shortcut. Null while only modifiers are down. */
export function keysFromEvent(e: KeyboardEvent): Keys | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  const altGr = typeof e.getModifierState === "function" && e.getModifierState("AltGraph");
  return tidy({ key: keyOf(e), ctrl: e.ctrlKey, meta: e.metaKey, alt: e.altKey || altGr, shift: e.shiftKey });
}

const KEY_NAMES: Record<string, [mac: string, other: string]> = {
  enter: ["↩", "Enter"],
  tab: ["⇥", "Tab"],
  space: ["Space", "Space"],
  backspace: ["⌫", "Backspace"],
  delete: ["⌦", "Delete"],
  escape: ["Esc", "Esc"],
  insert: ["Insert", "Insert"],
  home: ["Home", "Home"],
  end: ["End", "End"],
  pageup: ["Page Up", "PgUp"],
  pagedown: ["Page Down", "PgDn"],
  up: ["↑", "Up"],
  down: ["↓", "Down"],
  left: ["←", "Left"],
  right: ["→", "Right"],
};

/** Display form: "⌘K", "⇧⌘D" on Mac; "Ctrl+Shift+K" elsewhere. */
export function keysLabel(k: Keys, mac = isMac): string {
  const name = KEY_NAMES[k.key]?.[mac ? 0 : 1] ?? k.key.toUpperCase();
  if (mac) return `${k.ctrl ? "⌃" : ""}${k.alt ? "⌥" : ""}${k.shift ? "⇧" : ""}${k.meta ? "⌘" : ""}${name}`;
  return [k.ctrl && "Ctrl", k.alt && "Alt", k.shift && "Shift", k.meta && (isWindows ? "Win" : "Super"), name].filter(Boolean).join("+");
}

/** A shortcut's keys as the app shows them; "" when it has none. */
export function shortcutLabel(id: ShortcutId): string {
  const k = shortcutKeys(id);
  return k ? keysLabel(k) : "";
}

/** Plain Ctrl without Shift is safe with these: terminal programs don't read them. */
const CTRL_FREE = new Set([",", ".", ";", "'", "="]);

const MAC_TAKEN: Record<string, string> = {
  "⌘K": "⌘K clears the terminal, as in other Mac terminals.",
  "⌘C": "⌘C copies.",
  "⌘V": "⌘V pastes.",
  "⌘A": "⌘A selects all the text.",
  "⌘X": "⌘X cuts text.",
  "⌘Z": "⌘Z undoes typing.",
  "⇧⌘Z": "⇧⌘Z redoes typing.",
  "⌘Q": "⌘Q quits SSHeesh.",
  "⌘H": "⌘H hides SSHeesh, as in every Mac app.",
  "⌘M": "⌘M minimizes the window, as in every Mac app.",
};
const MAC_SYSTEM = ["⌥⌘H", "⌘Space", "⌃⌘Space", "⌥⌘Space", "⌘`", "⇧⌘`", "⌃⌘F", "⌃⌘Q", "⇧⌘3", "⇧⌘4", "⇧⌘5", "⌥⌘D"];

/**
 * Why these keys can't be a shortcut, in one plain sentence, or null if they
 * can. The terminal keeps what programs in it read; the system keeps its own.
 */
export function reservedReason(k: Keys, mac = isMac, windows = isWindows): string | null {
  const name = keysLabel(k, mac);
  if (k.key === "escape") return "Esc is kept for closing things.";
  if (mac) {
    if (!k.meta) {
      if (k.ctrl) return "Terminal programs use ⌃ with a key themselves, so hold ⌘ instead.";
      if (k.alt) return "⌥ types accented letters and symbols, so hold ⌘ instead.";
      return "Hold ⌘ with a key, so plain typing still reaches the terminal.";
    }
    if (k.key === "tab" || MAC_SYSTEM.includes(name)) return `macOS uses ${name} itself.`;
    return MAC_TAKEN[name] ?? null;
  }
  if (k.meta) return windows ? "The Windows key belongs to the system." : "The Super key belongs to the system.";
  if (k.alt) return "Alt and AltGr stay with the terminal and your keyboard layout, so use Ctrl+Shift instead.";
  if (!k.ctrl) return "Hold Ctrl and Shift with a key, so plain typing still reaches the terminal.";
  if (k.key === "tab") return "Ctrl+Tab and Ctrl+Shift+Tab switch tabs.";
  if (k.shift && (k.key === "c" || k.key === "v")) return "Ctrl+Shift+C and Ctrl+Shift+V copy and paste in terminals.";
  if (!k.shift && !CTRL_FREE.has(k.key) && !/^f\d+$/.test(k.key)) return `Terminal programs use ${name} themselves, so add Shift.`;
  return null;
}

/**
 * Why `id` can't have these keys, or null if it can: the terminal or the
 * system keeps them, or another shortcut already has them.
 */
export function shortcutProblem(id: ShortcutId, k: Keys, mac = isMac, windows = isWindows): string | null {
  const reserved = reservedReason(k, mac, windows);
  if (reserved) return reserved;
  for (const other of SHORTCUT_IDS) {
    const has = other === id ? null : shortcutKeys(other, mac);
    if (!has || !sameKeys(has, k)) continue;
    if (SHORTCUT_NAMES[other] === SHORTCUT_NAMES[id]) return `${keysLabel(k, mac)} already opens the command bar.`;
    return `${keysLabel(k, mac)} already does “${SHORTCUT_NAMES[other]}”, so change that one first.`;
  }
  return null;
}

export const modKey = isMac ? "⌘" : "Ctrl";

/** Ctrl+Tab and Ctrl+Shift+Tab, as in a browser (on a Mac too): +1 or -1. */
export function tabStep(e: KeyboardEvent): 1 | -1 | null {
  if (e.key !== "Tab" || !e.ctrlKey || e.altKey || e.metaKey) return null;
  return e.shiftKey ? -1 : 1;
}
