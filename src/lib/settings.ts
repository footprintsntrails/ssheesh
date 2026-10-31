// User preferences. Saved on this computer and applied live.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { contrast, parseHex } from "./color";
import { quietStorage } from "./persist";
import type { HighlightRule } from "./highlight";
import { isWindows, setShortcutSource, type ShortcutOverrides } from "./keys";
import { ACCENTS, customThemeVars, setCustomThemeSource, type AppThemeId, type CustomTheme } from "./themes";

export type UpdateMode = "auto" | "notify" | "manual";

/** After a dropped connection: reconnect by itself, show a Reconnect button, or stay quiet. */
export type ReconnectMode = "auto" | "ask" | "off";

export interface Settings {
  theme: AppThemeId;
  /** Switch between a light and a dark theme with the system. */
  themeFollowsSystem: boolean;
  lightTheme: AppThemeId;
  darkTheme: AppThemeId;
  /** Themes you made from a built-in one in Settings → Appearance. */
  customThemes: CustomTheme[];
  /** Shortcuts whose keys you changed; the rest keep their own. */
  shortcuts: ShortcutOverrides;
  /** Interface size in percent; the terminal font has its own size. */
  uiZoom: number;
  /** How much to fade panes you're not using, 0 to 50 percent. */
  dimInactive: number;
  animations: boolean;
  /** The >_< mark in the sidebar and empty panes. */
  logo: "winking" | "still" | "hidden";
  /** Hides the sidebar, toolbar and vitals; they slide in at the window's edges. */
  compactMode: boolean;
  /**
   * "split": Terminal and Files switch on and off, and the ones on
   * share the window. "tabs": one fills the window at a time.
   */
  viewMode: "split" | "tabs";
  /** Tabs sit under their server in the sidebar, or in a strip at the top. */
  tabsPlacement: "sidebar" | "top";
  /** The first-launch hello has been seen (or skipped). */
  onboarded: boolean;
  /** Which hello you've been through: a new one shows once to everybody. */
  onboardingVersion: number;
  /** First-use hints already shown, by id. */
  hintsSeen: string[];
  /** A thin Production or Staging colored line along the window's top edge. */
  envStripe: boolean;
  /** The version whose "What's new" you've seen, to show notes after an update. */
  lastSeenVersion: string;
  accent: string;
  terminalScheme: string;
  terminalFont: string;
  terminalFontSize: number;
  terminalLineHeight: number;
  cursorStyle: "bar" | "block" | "underline";
  cursorBlink: boolean;
  /** Terminals draw with WebGL. On by default on Windows, whose WebView2 is Chromium. */
  gpuRendering: boolean;
  /** Selecting text in a terminal copies it. */
  copyOnSelect: boolean;
  terminalPadding: number;
  reconnectMode: ReconnectMode;
  sidebarSide: "left" | "right";
  sidebarVisible: boolean;
  sidebarWidth: number;
  /** Full list, or a rail of server icons. */
  sidebarMode: "wide" | "narrow";
  /** How the sidebar shows a connected server's load: meters in the list, this style on rail icons. */
  railStatus: "off" | "frame" | "bars" | "pulse" | "count";
  /** Machines whose container list is open in the sidebar. */
  expanded: string[];
  /** Places you took out of the file manager's bookmarks ("home", "/var/log", "recent"…). */
  hiddenPlaces: string[];
  previewWidth: number;
  paneGap: number;
  paneRadius: number;
  showVitals: boolean;
  updateMode: UpdateMode;
  /** Hides server names, addresses and users everywhere, for streams and screen shares. */
  streamerMode: boolean;
  /** Color errors, warnings and addresses in terminal output, plus your own rules. */
  highlightErrors: boolean;
  highlightWarnings: boolean;
  highlightAddresses: boolean;
  highlightRules: HighlightRule[];
  /** System notification when a terminal needs you and SSHeesh isn't in front. */
  notifyAttention: boolean;
  /** Windows: the system's own title bar instead of SSHeesh's. */
  nativeTitleBar: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "midnight",
  themeFollowsSystem: false,
  lightTheme: "daylight",
  darkTheme: "midnight",
  customThemes: [],
  shortcuts: {},
  uiZoom: 100,
  dimInactive: 0,
  animations: true,
  logo: "winking",
  compactMode: false,
  onboarded: false,
  onboardingVersion: 0,
  hintsSeen: [],
  envStripe: false,
  tabsPlacement: "sidebar",
  lastSeenVersion: "",
  accent: ACCENTS[0].value,
  terminalScheme: "auto",
  terminalFont: "system",
  terminalFontSize: 13,
  terminalLineHeight: 1.22,
  cursorStyle: "bar",
  cursorBlink: true,
  gpuRendering: isWindows,
  copyOnSelect: true,
  terminalPadding: 12,
  reconnectMode: "ask",
  sidebarSide: "left",
  sidebarVisible: true,
  sidebarWidth: 232,
  sidebarMode: "wide",
  railStatus: "frame",
  expanded: [],
  // One page at a time; the hello offers side by side.
  viewMode: "tabs",
  hiddenPlaces: [],
  previewWidth: 420,
  paneGap: 6,
  paneRadius: 11,
  showVitals: true,
  updateMode: "auto",
  streamerMode: false,
  highlightErrors: true,
  highlightWarnings: true,
  highlightAddresses: false,
  highlightRules: [],
  notifyAttention: true,
  nativeTitleBar: false,
};

interface SettingsStore extends Settings {
  update(patch: Partial<Settings>): void;
  reset(section?: (keyof Settings)[]): void;
}

export const useSettings = create<SettingsStore>()(
  persist(
    (set) => ({
      ...DEFAULT_SETTINGS,
      update: (patch) => set(patch),
      reset: (keys) =>
        set(keys ? Object.fromEntries(keys.map((k) => [k, DEFAULT_SETTINGS[k]])) : { ...DEFAULT_SETTINGS }),
    }),
    { name: "sshee-settings", version: 1, storage: quietStorage() },
  ),
);

setCustomThemeSource(() => useSettings.getState().customThemes);
setShortcutSource(() => useSettings.getState().shortcuts);

/** Bump to show a new hello once to everybody, existing users too. */
export const ONBOARDING_VERSION = 2;

/** Marks a first-use hint as shown, so it never shows again. */
export function markHintSeen(id: string) {
  const st = useSettings.getState();
  if (!st.hintsSeen.includes(id)) st.update({ hintsSeen: [...st.hintsSeen, id] });
}

/**
 * Text color for an accent fill: near-black on light accents, white on deep
 * ones, whichever reads better (white on the default blue is only 2.7:1).
 */
export function onAccent(hex: string): string {
  const c = parseHex(hex);
  if (!c) return "#06101f";
  return contrast(c, WHITE) >= contrast(c, INK) ? "#ffffff" : "#06101f";
}

const WHITE = { r: 255, g: 255, b: 255, a: 1 };
const INK = { r: 6, g: 16, b: 31, a: 1 };

/** Variables the last custom theme set on the page, to take away when it changes. */
let customVars: string[] = [];

/** Pushes theme, accent and spacing into CSS variables. */
export function applyAppearance(s: Settings) {
  const root = document.documentElement;
  // A custom theme stands on its base's [data-theme] and sets its own colors on top.
  const custom = s.customThemes.find((t) => t.id === s.theme);
  root.dataset.theme = custom ? custom.base : s.theme;
  const vars = custom ? customThemeVars(custom) : {};
  for (const name of customVars) if (!(name in vars)) root.style.removeProperty(name);
  for (const [name, value] of Object.entries(vars)) root.style.setProperty(name, value);
  customVars = Object.keys(vars);
  root.classList.toggle("reduce-motion", !s.animations);
  root.style.setProperty("--ui-accent", s.accent);
  root.style.setProperty("--ui-on-accent", onAccent(s.accent));
  root.style.setProperty("--ui-gap", `${s.paneGap}px`);
  root.style.setProperty("--ui-radius", `${s.paneRadius}px`);
}
