// Keyboard shortcuts. On macOS they use ⌘; elsewhere Ctrl+Shift, so plain
// Ctrl keys (Ctrl+C, Ctrl+D, Ctrl+W…) keep working inside terminals.

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);

export type ShortcutId = "palette" | "splitRight" | "splitDown" | "closePane" | "addHost" | "settings" | "sidebar" | "vitals" | "zoomPane" | "streamer" | "compact";

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
