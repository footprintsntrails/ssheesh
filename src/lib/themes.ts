// Appearance presets: app themes, accent colors, terminal color schemes, fonts.

import type { ITheme } from "@xterm/xterm";
import { contrast, isLightColor, over, parseHex, toHex, type Rgba } from "./color";
import { isMac } from "./keys";

export type BuiltInThemeId = "midnight" | "graphite" | "oled" | "daylight";
/** One made in Settings → Appearance: "custom-" and a random id. */
export type CustomThemeId = `custom-${string}`;
export type AppThemeId = BuiltInThemeId | CustomThemeId;

/** What the theme pickers show of a theme. */
export interface ThemeInfo {
  id: AppThemeId;
  name: string;
  window: string;
  canvas: string;
  fg: string;
  light?: boolean;
}

export const APP_THEMES: (ThemeInfo & { id: BuiltInThemeId })[] = [
  { id: "midnight", name: "Midnight", window: "#0f1013", canvas: "#0a0b0d", fg: "#eceef1" },
  { id: "graphite", name: "Graphite", window: "#161616", canvas: "#101010", fg: "#ededed" },
  { id: "oled", name: "OLED", window: "#000000", canvas: "#000000", fg: "#f2f2f2" },
  { id: "daylight", name: "Daylight", window: "#eceef2", canvas: "#ffffff", fg: "#14171c", light: true },
];

// ---------------------------------------------------------------- your own themes

export type ThemeColorKey =
  | "window"
  | "canvas"
  | "raised"
  | "ink"
  | "fg"
  | "fg2"
  | "fg3"
  | "ok"
  | "warn"
  | "bad"
  | "glass"
  | "fileCode"
  | "fileImage"
  | "fileArchive";

/**
 * The colors a theme editor can change, in the order it shows them, with the
 * CSS variable each one sets (index.css). `alpha`: it may be see-through.
 */
export const THEME_COLORS: { key: ThemeColorKey; cssVar: string; group: string; label: string; hint: string; alpha?: boolean }[] = [
  { key: "window", cssVar: "--ui-window", group: "Window and panels", label: "Window", hint: "Behind everything: the sidebar, the toolbar and the gaps between panes." },
  {
    key: "canvas",
    cssVar: "--ui-canvas",
    group: "Window and panels",
    label: "Panes",
    hint: "Terminals (with the Match app color scheme) and the home page. See-through panes take on the window's color.",
    alpha: true,
  },
  { key: "raised", cssVar: "--ui-raised", group: "Window and panels", label: "Raised controls", hint: "The picked option in switches, badges and pop-up lists." },
  { key: "ink", cssVar: "--ui-ink", group: "Window and panels", label: "Lines and shading", hint: "Borders, dividers and hover tints, drawn faintly. Picks white or black to suit the window until you change it." },
  { key: "fg", cssVar: "--ui-fg", group: "Text", label: "Text", hint: "Names, titles and everything you read first." },
  { key: "fg2", cssVar: "--ui-fg-2", group: "Text", label: "Secondary text", hint: "Details next to the main text." },
  { key: "fg3", cssVar: "--ui-fg-3", group: "Text", label: "Hints", hint: "Small print, like this line." },
  { key: "ok", cssVar: "--ui-ok", group: "Status colors", label: "Good", hint: "Connected, healthy, done." },
  { key: "warn", cssVar: "--ui-warn", group: "Status colors", label: "Needs attention", hint: "A terminal waiting for you, a disk nearly full." },
  { key: "bad", cssVar: "--ui-bad", group: "Status colors", label: "Problem", hint: "Errors and lost connections." },
  {
    key: "glass",
    cssVar: "--ui-glass",
    group: "Glass",
    label: "Menus and dialogs",
    hint: "Menus, dialogs, the command bar and notices. Solid unless you make it see-through; then what's behind shows through, blurred.",
    alpha: true,
  },
  { key: "fileCode", cssVar: "--ui-file-code", group: "Files", label: "Code", hint: "Scripts, configs and .env files in the file manager.", alpha: true },
  { key: "fileImage", cssVar: "--ui-file-image", group: "Files", label: "Images", hint: "Pictures and icons.", alpha: true },
  { key: "fileArchive", cssVar: "--ui-file-archive", group: "Files", label: "Archives", hint: "Zip, tar and other packed files.", alpha: true },
];

const DARK_STATUS = { ok: "#3dd68c", warn: "#f5b841", bad: "#ff6159" };
const DARK_FILES = { fileCode: "#c792eacc", fileImage: "#f5c451cc", fileArchive: "#ff9f6bcc" };

/** The built-in themes' colors, as index.css sets them. A test keeps the two in step. */
export const BUILT_IN_COLORS: Record<BuiltInThemeId, Record<ThemeColorKey, string>> = {
  midnight: {
    window: "#0f1013", canvas: "#0a0b0d", raised: "#1a1c20", ink: "#ffffff", fg: "#eceef1", fg2: "#a2a8b1", fg3: "#8a919b",
    ...DARK_STATUS, glass: "#1f2126", ...DARK_FILES,
  },
  graphite: {
    window: "#161616", canvas: "#101010", raised: "#222222", ink: "#ffffff", fg: "#ededed", fg2: "#a6a6a6", fg3: "#8e8e8e",
    ...DARK_STATUS, glass: "#282828", ...DARK_FILES,
  },
  oled: {
    window: "#000000", canvas: "#000000", raised: "#111111", ink: "#ffffff", fg: "#f2f2f2", fg2: "#a0a0a0", fg3: "#8a8a8a",
    ...DARK_STATUS, glass: "#161616", ...DARK_FILES,
  },
  daylight: {
    window: "#eceef2", canvas: "#ffffff", raised: "#ffffff", ink: "#0f172a", fg: "#14171c", fg2: "#4d5560", fg3: "#626973",
    ok: "#15803d", warn: "#9a5800", bad: "#c4352d", glass: "#ffffff", fileCode: "#8250df", fileImage: "#a16207", fileArchive: "#c2410c",
  },
};

export interface CustomTheme {
  id: CustomThemeId;
  name: string;
  /** The built-in theme it started from. Colors you haven't changed come from there. */
  base: BuiltInThemeId;
  /** Only the colors you changed, as #rrggbb or #rrggbbaa. */
  colors: Partial<Record<ThemeColorKey, string>>;
}

const BUILT_IN_IDS = APP_THEMES.map((t) => t.id);

export function isBuiltInTheme(id: string): id is BuiltInThemeId {
  return (BUILT_IN_IDS as string[]).includes(id);
}

// Settings hands its list over here: themes.ts can't import settings.ts, which imports it.
let customSource: () => CustomTheme[] = () => [];
export function setCustomThemeSource(get: () => CustomTheme[]) {
  customSource = get;
}

export function findCustomTheme(id: string, list = customSource()): CustomTheme | undefined {
  return list.find((t) => t.id === id);
}

export function newCustomThemeId(): CustomThemeId {
  return `custom-${Math.random().toString(36).slice(2, 10)}`;
}

const solid = (hex: string) => parseHex(hex) ?? { r: 0, g: 0, b: 0, a: 1 };

/**
 * Every color of a custom theme: its own on top of its base's. "Lines and
 * shading" follows the window (white on dark, near-black on light) until set.
 */
export function themeColors(t: CustomTheme): Record<ThemeColorKey, string> {
  const colors = { ...(BUILT_IN_COLORS[t.base] ?? BUILT_IN_COLORS.midnight) };
  for (const { key, alpha } of THEME_COLORS) {
    const c = parseHex(t.colors?.[key] ?? "");
    if (c) colors[key] = toHex(alpha ? c : { ...c, a: 1 });
  }
  if (!parseHex(t.colors?.ink ?? "")) colors.ink = isLightColor(solid(colors.window)) ? BUILT_IN_COLORS.daylight.ink : BUILT_IN_COLORS.midnight.ink;
  return colors;
}

/** `top` as it looks over the window: what contrast is measured against. */
export function seenOver(top: string, window: string): Rgba {
  return over(solid(top), { ...solid(window), a: 1 });
}

/** The CSS a custom theme sets on the page, over its base's [data-theme]. */
export function customThemeVars(t: CustomTheme): Record<string, string> {
  const colors = themeColors(t);
  const vars: Record<string, string> = {};
  for (const { key, cssVar } of THEME_COLORS) vars[cssVar] = colors[key];
  const ink = solid(colors.ink);
  vars["--ui-ink"] = `${ink.r} ${ink.g} ${ink.b}`;
  // Frost only when there's something to see through, and solid where frost can't be drawn.
  if (solid(colors.glass).a < 1) {
    vars["--ui-glass-filter"] = "blur(28px) saturate(175%)";
    vars["--ui-glass-solid"] = toHex(over(solid(colors.glass), seenOver(colors.raised, colors.window)));
  }
  vars["color-scheme"] = isLightColor(solid(colors.window)) ? "light" : "dark";
  return vars;
}

/** A built-in or custom theme by id, as the pickers show it. Unknown ids fall back to Midnight. */
export function themeInfo(id: string, list = customSource()): ThemeInfo {
  const custom = findCustomTheme(id, list);
  if (!custom) return APP_THEMES.find((t) => t.id === id) ?? APP_THEMES[0];
  const c = themeColors(custom);
  const window = solid(c.window);
  return { id: custom.id, name: custom.name, window: toHex({ ...window, a: 1 }), canvas: toHex(seenOver(c.canvas, c.window)), fg: c.fg, light: isLightColor(window) };
}

/** Text that may be hard to read: each text color on each background it sits on, below 4.5:1. */
export function weakContrast(colors: Record<ThemeColorKey, string>): { text: ThemeColorKey; on: ThemeColorKey; ratio: number }[] {
  const out: { text: ThemeColorKey; on: ThemeColorKey; ratio: number }[] = [];
  for (const text of ["fg", "fg2", "fg3"] as const) {
    for (const on of ["window", "canvas", "raised", "glass"] as const) {
      const ratio = contrast(seenOver(colors[text], colors.window), seenOver(colors[on], colors.window));
      if (ratio < 4.5) out.push({ text, on, ratio });
    }
  }
  return out;
}

/** A theme as text to share: paste it into Settings → Appearance → Import on any computer. */
export function themeToJson(t: CustomTheme): string {
  const colors: Partial<Record<ThemeColorKey, string>> = themeColors(t);
  // Left out, it keeps following the window.
  if (!t.colors.ink) delete colors.ink;
  return JSON.stringify({ ssheesh: "theme", version: 1, name: t.name, base: t.base, colors }, null, 2);
}

/**
 * Reads a theme someone shared. Strict about what it knows (every color must
 * be #rrggbb or #rrggbbaa), quiet about keys it doesn't. Keeps only colors that
 * differ from the base, so Reset brings the base's back.
 */
export function themeFromJson(text: string): { theme: Omit<CustomTheme, "id"> } | { error: string } {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    return { error: "That isn't a theme. Copy all of it, from the first { to the last }." };
  }
  if (!data || typeof data !== "object" || Array.isArray(data)) return { error: "That isn't a theme. It should start with { and end with }." };
  const o = data as Record<string, unknown>;
  if (o.ssheesh !== undefined && o.ssheesh !== "theme") return { error: "That's something else from SSHeesh, not a theme." };
  if (o.name !== undefined && typeof o.name !== "string") return { error: "The theme's name should be text in quotes." };
  if (o.base !== undefined && (typeof o.base !== "string" || !isBuiltInTheme(o.base))) {
    return { error: `"base" should be one of ${BUILT_IN_IDS.join(", ")}.` };
  }
  if (!o.colors || typeof o.colors !== "object" || Array.isArray(o.colors)) return { error: "The theme has no colors." };
  const given = o.colors as Record<string, unknown>;
  const colors: Partial<Record<ThemeColorKey, string>> = {};
  for (const { key, label, alpha } of THEME_COLORS) {
    const v = given[key];
    if (v === undefined) continue;
    const c = typeof v === "string" && /^\s*#/.test(v) ? parseHex(v) : null;
    if (!c) return { error: `"${key}" (${label}) should be a color like #1f2937, not ${JSON.stringify(v)}.` };
    if (c.a < 1 && !alpha) return { error: `"${key}" (${label}) can't be see-through. Use six digits after the #.` };
    colors[key] = toHex(c);
  }
  if (!Object.keys(colors).length) return { error: "None of the theme's colors are ones SSHeesh knows." };
  const base: BuiltInThemeId = (o.base as BuiltInThemeId | undefined) ?? (colors.window && isLightColor(solid(colors.window)) ? "daylight" : "midnight");
  for (const key of Object.keys(colors) as ThemeColorKey[]) {
    if (key !== "ink" && colors[key] === BUILT_IN_COLORS[base][key]) delete colors[key];
  }
  const name = (typeof o.name === "string" ? o.name : "").trim().slice(0, 40) || "Imported theme";
  return { theme: { name, base, colors } };
}

// No orange, pink, red, yellow or green: they would read as a warning or a status.
export const ACCENTS: { name: string; value: string }[] = [
  { name: "Blue", value: "#5b9dff" },
  { name: "Indigo", value: "#7c83ff" },
  { name: "Violet", value: "#b07cff" },
  { name: "Mint", value: "#2bc4b4" },
  { name: "Silver", value: "#9aa3ad" },
];

export interface TerminalScheme {
  name: string;
  light?: boolean;
  theme: ITheme;
}

const NIGHT: ITheme = {
  background: "#0a0b0d",
  foreground: "#d8dce2",
  cursor: "#5b9dff",
  cursorAccent: "#0a0b0d",
  selectionBackground: "rgba(91, 157, 255, 0.32)",
  selectionInactiveBackground: "rgba(255, 255, 255, 0.12)",
  black: "#1c1e22",
  red: "#ff6b6b",
  green: "#3dd68c",
  yellow: "#f5c451",
  blue: "#5b9dff",
  magenta: "#c792ea",
  cyan: "#4fd1c5",
  white: "#c9cdd3",
  brightBlack: "#5f656e",
  brightRed: "#ff8a85",
  brightGreen: "#6be3a8",
  brightYellow: "#ffd479",
  brightBlue: "#86b6ff",
  brightMagenta: "#dbaaf7",
  brightCyan: "#7be0d6",
  brightWhite: "#f5f6f8",
};

const DAY: ITheme = {
  background: "#ffffff",
  foreground: "#1f2328",
  cursor: "#2f6fdb",
  cursorAccent: "#ffffff",
  selectionBackground: "rgba(47, 111, 219, 0.2)",
  selectionInactiveBackground: "rgba(0, 0, 0, 0.08)",
  black: "#24292f",
  red: "#cf222e",
  green: "#1a7f37",
  yellow: "#9a6700",
  blue: "#0969da",
  magenta: "#8250df",
  cyan: "#1b7c83",
  white: "#6e7781",
  brightBlack: "#57606a",
  brightRed: "#a40e26",
  brightGreen: "#2da44e",
  brightYellow: "#bf8700",
  brightBlue: "#218bff",
  brightMagenta: "#a475f9",
  brightCyan: "#3192aa",
  brightWhite: "#8c959f",
};

/** Well-known open palettes (all MIT-licensed upstream). */
export const TERMINAL_SCHEMES: Record<string, TerminalScheme> = {
  "sshee-night": { name: "SSHeesh Night", theme: NIGHT },
  "sshee-day": { name: "SSHeesh Day", light: true, theme: DAY },
  "tokyo-night": {
    name: "Tokyo Night",
    theme: {
      background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5", cursorAccent: "#1a1b26", selectionBackground: "#33467c",
      black: "#15161e", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68", blue: "#7aa2f7", magenta: "#bb9af7", cyan: "#7dcfff", white: "#a9b1d6",
      brightBlack: "#414868", brightRed: "#f7768e", brightGreen: "#9ece6a", brightYellow: "#e0af68", brightBlue: "#7aa2f7", brightMagenta: "#bb9af7", brightCyan: "#7dcfff", brightWhite: "#c0caf5",
    },
  },
  "catppuccin-mocha": {
    name: "Catppuccin Mocha",
    theme: {
      background: "#1e1e2e", foreground: "#cdd6f4", cursor: "#f5e0dc", cursorAccent: "#1e1e2e", selectionBackground: "#585b70",
      black: "#45475a", red: "#f38ba8", green: "#a6e3a1", yellow: "#f9e2af", blue: "#89b4fa", magenta: "#f5c2e7", cyan: "#94e2d5", white: "#bac2de",
      brightBlack: "#585b70", brightRed: "#f38ba8", brightGreen: "#a6e3a1", brightYellow: "#f9e2af", brightBlue: "#89b4fa", brightMagenta: "#f5c2e7", brightCyan: "#94e2d5", brightWhite: "#a6adc8",
    },
  },
  dracula: {
    name: "Dracula",
    theme: {
      background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f2", cursorAccent: "#282a36", selectionBackground: "#44475a",
      black: "#21222c", red: "#ff5555", green: "#50fa7b", yellow: "#f1fa8c", blue: "#bd93f9", magenta: "#ff79c6", cyan: "#8be9fd", white: "#f8f8f2",
      brightBlack: "#6272a4", brightRed: "#ff6e6e", brightGreen: "#69ff94", brightYellow: "#ffffa5", brightBlue: "#d6acff", brightMagenta: "#ff92df", brightCyan: "#a4ffff", brightWhite: "#ffffff",
    },
  },
  nord: {
    name: "Nord",
    theme: {
      background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9", cursorAccent: "#2e3440", selectionBackground: "#434c5e",
      black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b", blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
      brightBlack: "#4c566a", brightRed: "#bf616a", brightGreen: "#a3be8c", brightYellow: "#ebcb8b", brightBlue: "#81a1c1", brightMagenta: "#b48ead", brightCyan: "#8fbcbb", brightWhite: "#eceff4",
    },
  },
  "one-dark": {
    name: "One Dark",
    theme: {
      background: "#282c34", foreground: "#abb2bf", cursor: "#528bff", cursorAccent: "#282c34", selectionBackground: "#3e4451",
      black: "#282c34", red: "#e06c75", green: "#98c379", yellow: "#e5c07b", blue: "#61afef", magenta: "#c678dd", cyan: "#56b6c2", white: "#abb2bf",
      brightBlack: "#5c6370", brightRed: "#e06c75", brightGreen: "#98c379", brightYellow: "#e5c07b", brightBlue: "#61afef", brightMagenta: "#c678dd", brightCyan: "#56b6c2", brightWhite: "#ffffff",
    },
  },
  "gruvbox-dark": {
    name: "Gruvbox Dark",
    theme: {
      background: "#282828", foreground: "#ebdbb2", cursor: "#ebdbb2", cursorAccent: "#282828", selectionBackground: "#504945",
      black: "#282828", red: "#cc241d", green: "#98971a", yellow: "#d79921", blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#a89984",
      brightBlack: "#928374", brightRed: "#fb4934", brightGreen: "#b8bb26", brightYellow: "#fabd2f", brightBlue: "#83a598", brightMagenta: "#d3869b", brightCyan: "#8ec07c", brightWhite: "#ebdbb2",
    },
  },
  "solarized-dark": {
    name: "Solarized Dark",
    theme: {
      background: "#002b36", foreground: "#839496", cursor: "#93a1a1", cursorAccent: "#002b36", selectionBackground: "#073642",
      black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900", blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
      brightBlack: "#586e75", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3",
    },
  },
};

export const TERMINAL_FONTS: { id: string; name: string; stack: string }[] = [
  {
    id: "system",
    name: isMac ? "SF Mono" : "System",
    stack: '"SF Mono", SFMono-Regular, Menlo, "Cascadia Mono", Consolas, "JetBrains Mono Variable", monospace',
  },
  { id: "jetbrains", name: "JetBrains Mono", stack: '"JetBrains Mono Variable", monospace' },
  { id: "menlo", name: "Menlo", stack: 'Menlo, "SF Mono", monospace' },
  { id: "cascadia", name: "Cascadia Mono", stack: '"Cascadia Mono", "Cascadia Code", Consolas, monospace' },
  { id: "consolas", name: "Consolas", stack: "Consolas, monospace" },
];

/** Nerd Font icons for prompts like oh-my-posh and Starship, after the chosen font. */
export const ICON_FONT = '"Symbols Nerd Font Mono"';

export function fontStack(id: string): string {
  const stack = (TERMINAL_FONTS.find((f) => f.id === id) ?? TERMINAL_FONTS[0]).stack;
  return stack.replace(/,\s*monospace$/, `, ${ICON_FONT}, monospace`);
}

/** "auto" follows the app theme and blends into the pane background. */
export function resolveScheme(id: string, appTheme: AppThemeId, accent: string): ITheme {
  if (id !== "auto" && TERMINAL_SCHEMES[id]) return TERMINAL_SCHEMES[id].theme;
  const t = themeInfo(appTheme);
  const base = t.light ? DAY : NIGHT;
  return {
    ...base,
    background: t.canvas,
    cursorAccent: t.canvas,
    cursor: accent,
    selectionBackground: `${accent}55`,
  };
}
