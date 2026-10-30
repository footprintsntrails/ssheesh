// Appearance presets: app themes, accent colors, terminal color schemes, fonts.

import type { ITheme } from "@xterm/xterm";
import { isMac } from "./keys";

export type AppThemeId = "midnight" | "graphite" | "oled" | "daylight";

export const APP_THEMES: { id: AppThemeId; name: string; window: string; canvas: string; fg: string; light?: boolean }[] = [
  { id: "midnight", name: "Midnight", window: "#0f1013", canvas: "#0a0b0d", fg: "#eceef1" },
  { id: "graphite", name: "Graphite", window: "#161616", canvas: "#101010", fg: "#ededed" },
  { id: "oled", name: "OLED", window: "#000000", canvas: "#000000", fg: "#f2f2f2" },
  { id: "daylight", name: "Daylight", window: "#eceef2", canvas: "#ffffff", fg: "#14171c", light: true },
];

export const ACCENTS: { name: string; value: string }[] = [
  { name: "Blue", value: "#5b9dff" },
  { name: "Indigo", value: "#7c83ff" },
  { name: "Violet", value: "#b07cff" },
  { name: "Pink", value: "#ff6fb1" },
  { name: "Orange", value: "#ff9f43" },
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
  const t = APP_THEMES.find((x) => x.id === appTheme) ?? APP_THEMES[0];
  const base = t.light ? DAY : NIGHT;
  return {
    ...base,
    background: t.canvas,
    cursorAccent: t.canvas,
    cursor: accent,
    selectionBackground: `${accent}55`,
  };
}
