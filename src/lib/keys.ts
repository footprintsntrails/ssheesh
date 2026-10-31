// Keyboard shortcuts. On macOS they use ⌘; elsewhere Ctrl+Shift, so plain
// Ctrl keys (Ctrl+C, Ctrl+D, Ctrl+W…) keep working inside terminals.

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

export type ShortcutId = "palette" | "splitRight" | "splitDown" | "closePane" | "newTerminal" | "addHost" | "settings" | "sidebar" | "vitals" | "zoomPane" | "streamer" | "compact";

interface Combo {
  key: string;
  /** On Mac: Shift is added. Elsewhere Shift is the default; `false` drops it. */
  shift?: boolean;
}

const MAC: Record<ShortcutId, Combo> = {
  palette: { key: "k" },
  splitRight: { key: "d" },
  splitDown: { key: "d", shift: true },
  closePane: { key: "w" },
  newTerminal: { key: "t" },
  addHost: { key: "n", shift: true },
  settings: { key: "," },
  sidebar: { key: "b" },
  vitals: { key: "j" },
  zoomPane: { key: "enter", shift: true },
  streamer: { key: "h", shift: true },
  compact: { key: "m", shift: true },
};

const OTHER: Record<ShortcutId, Combo> = {
  palette: { key: "k" },
  splitRight: { key: "d" },
  splitDown: { key: "e" },
  closePane: { key: "w" },
  newTerminal: { key: "t" },
  addHost: { key: "n" },
  settings: { key: ",", shift: false },
  sidebar: { key: "b" },
  vitals: { key: "j" },
  zoomPane: { key: "enter" },
  streamer: { key: "h" },
  compact: { key: "m" },
};

export function matches(e: KeyboardEvent, id: ShortcutId): boolean {
  const c = (isMac ? MAC : OTHER)[id];
  const key = c.key === "," ? (e.code === "Comma" ? "," : "") : e.key.toLowerCase();
  if (key !== c.key || e.altKey) return false;
  if (isMac) return e.metaKey && !e.ctrlKey && !!c.shift === e.shiftKey;
  return e.ctrlKey && !e.metaKey && (c.shift ?? true) === e.shiftKey;
}

export function matchShortcut(e: KeyboardEvent): ShortcutId | null {
  for (const id of Object.keys(MAC) as ShortcutId[]) if (matches(e, id)) return id;
  return null;
}

/** Display form: "⌘K", "⇧⌘D" on Mac; "Ctrl+Shift+K" elsewhere. */
export function shortcutLabel(id: ShortcutId): string {
  const c = (isMac ? MAC : OTHER)[id];
  const k = c.key === "enter" ? (isMac ? "↩" : "Enter") : c.key.toUpperCase();
  if (isMac) return `${c.shift ? "⇧" : ""}⌘${k}`;
  return (c.shift ?? true) ? `Ctrl+Shift+${k}` : `Ctrl+${k}`;
}

export const modKey = isMac ? "⌘" : "Ctrl";

/** Ctrl+Tab and Ctrl+Shift+Tab, as in a browser (on a Mac too): +1 or -1. */
export function tabStep(e: KeyboardEvent): 1 | -1 | null {
  if (e.key !== "Tab" || !e.ctrlKey || e.altKey || e.metaKey) return null;
  return e.shiftKey ? -1 : 1;
}
