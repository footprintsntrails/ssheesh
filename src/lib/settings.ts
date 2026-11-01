// User preferences. Saved on this computer and applied live.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { contrast, parseHex } from "./color";
import { quietStorage } from "./persist";
import type { HighlightRule } from "./highlight";
import { isMac, isWindows, setShortcutSource, type ShortcutOverrides } from "./keys";
import { ACCENTS, customThemeVars, DEFAULT_THEME, setCustomThemeSource, setTerminalColorsSource, type AppThemeId, type CustomTheme, type TerminalColors } from "./themes";

export type UpdateMode = "auto" | "notify" | "manual";

export type WelcomeMode = "auto" | "ssheesh" | "first" | "server" | "off";
export type WelcomePart = "logo" | "system" | "usage" | "status";

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
  /**
   * What hides until you point at the window's edge: the sidebar, the top
   * bar (with the tabs), or both, which also hides the vitals.
   */
  compactMode: CompactMode;
  /** What the compact shortcut turns back on: the mode you had last. */
  compactLast: Exclude<CompactMode, "off">;
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
  /** Your own terminal background and text, for the "Your own" scheme. */
  terminalColors: TerminalColors | null;
  terminalFont: string;
  terminalFontSize: number;
  terminalLineHeight: number;
  cursorStyle: "bar" | "block" | "underline";
  cursorBlink: boolean;
  /** Terminals draw with WebGL. On by default on Windows and macOS; off on Linux, where WebKitGTK may have no GPU. */
  gpuRendering: boolean;
  /** Selecting text in a terminal copies it. */
  copyOnSelect: boolean;
  terminalPadding: number;
  reconnectMode: ReconnectMode;
  /**
   * Minutes a terminal on a server can sit unused (no typing, no output)
   * before it lets go of its connection; 0 never. Only where tmux keeps the
   * shell running; a key brings it back.
   */
  idleSleep: number;
  sidebarSide: "left" | "right";
  sidebarVisible: boolean;
  sidebarWidth: number;
  /** Full list, or a rail of server icons. */
  sidebarMode: "wide" | "narrow";
  /** How server icons are drawn: clean lines, or pixels like the >_< logo. */
  iconPack: "line" | "pixel";
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
  /** Tokens, keys and passwords in terminal output show as dots until clicked. Always on while streaming. */
  hideSecrets: boolean;
  /** Files uploaded to a server lose their metadata first (location, camera, author…); a cleaned copy goes, yours stays as it is. */
  cleanUploads: boolean;
  /** Color errors, warnings and addresses in terminal output, plus your own rules. */
  highlightErrors: boolean;
  highlightWarnings: boolean;
  highlightAddresses: boolean;
  highlightRules: HighlightRule[];
  /** System notification when a terminal needs you and SSHeesh isn't in front. */
  notifyAttention: boolean;
  /**
   * What a new terminal on a server shows first. "auto": the server's own
   * welcome if it has one, else SSHeesh's; "first": SSHeesh's the first time,
   * then as "auto".
   */
  welcomeMode: WelcomeMode;
  /** What SSHeesh's welcome shows. */
  welcomeParts: WelcomePart[];
  /** Your own line under it. */
  welcomeNote: string;
  /** Servers that have had SSHeesh's welcome once ("first"). */
  welcomedHosts: string[];
  /** Windows: the system's own title bar instead of SSHeesh's. */
  nativeTitleBar: boolean;
}

/** What compact mode hides. */
export type CompactMode = "off" | "sidebar" | "tabbar" | "both";

/** The sidebar slides in at the window's edge instead of standing there. */
export const compactSidebar = (m: CompactMode) => m === "sidebar" || m === "both";
/** The top bar slides in at the window's top edge instead of standing there. */
export const compactTabbar = (m: CompactMode) => m === "tabbar" || m === "both";

export const DEFAULT_SETTINGS: Settings = {
  theme: DEFAULT_THEME,
  themeFollowsSystem: false,
  lightTheme: "daylight",
  darkTheme: DEFAULT_THEME,
  customThemes: [],
  shortcuts: {},
  uiZoom: 100,
  dimInactive: 0,
  animations: true,
  logo: "winking",
  compactMode: "off",
  compactLast: "both",
  onboarded: false,
  onboardingVersion: 0,
  hintsSeen: [],
  envStripe: false,
  tabsPlacement: "top",
  lastSeenVersion: "",
  accent: ACCENTS[0].value,
  terminalScheme: "auto",
  terminalColors: null,
  terminalFont: "system",
  terminalFontSize: 13,
  terminalLineHeight: 1.22,
  cursorStyle: "bar",
  cursorBlink: true,
  // The Mac's WebKit draws terminals without the GPU with stray color
  // blocks; Linux's WebKitGTK may have no GPU at all.
  gpuRendering: isWindows || isMac,
  copyOnSelect: true,
  terminalPadding: 12,
  reconnectMode: "ask",
  idleSleep: 20,
  sidebarSide: "left",
  sidebarVisible: true,
  sidebarWidth: 232,
  sidebarMode: "narrow",
  iconPack: "line",
  railStatus: "off",
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
  hideSecrets: true,
  cleanUploads: true,
  highlightErrors: true,
  highlightWarnings: true,
  highlightAddresses: false,
  highlightRules: [],
  notifyAttention: true,
  welcomeMode: "auto",
  welcomeParts: ["logo", "system", "usage", "status"],
  welcomeNote: "",
  welcomedHosts: [],
  nativeTitleBar: false,
};

interface SettingsStore extends Settings {
  update(patch: Partial<Settings>): void;
  reset(section?: (keyof Settings)[]): void;
}

/** Settings saved by an older version, brought up to this one. */
export function migrateSettings(saved: unknown, version: number, mac: boolean): Record<string, unknown> {
  const s = { ...(saved as Record<string, unknown>) };
  // 2: Macs draw with the GPU, also where it was off by default before.
  if (version < 2 && mac) s.gpuRendering = true;
  // 3: compact mode picks what hides; load beside server icons is off for everybody.
  if (version < 3) {
    s.compactMode = s.compactMode === true ? "both" : "off";
    s.railStatus = "off";
  }
  // 4: SSHeesh Default is the new look, for whoever kept the old one as it came
  // (Midnight in blue). A theme or accent picked by hand stays; Midnight stays in the list.
  if (version < 4 && s.theme === "midnight" && (s.accent ?? "#5b9dff") === "#5b9dff") {
    s.theme = DEFAULT_THEME;
    s.accent = ACCENTS[0].value;
    if (s.darkTheme === "midnight") s.darkTheme = DEFAULT_THEME;
  }
  return s;
}

export const useSettings = create<SettingsStore>()(
  persist(
    (set) => ({
      ...DEFAULT_SETTINGS,
      update: (patch) => set(patch),
      reset: (keys) =>
        set(keys ? Object.fromEntries(keys.map((k) => [k, DEFAULT_SETTINGS[k]])) : { ...DEFAULT_SETTINGS }),
    }),
    {
      name: "sshee-settings",
      version: 4,
      storage: quietStorage(),
      migrate: (saved, version) => migrateSettings(saved, version, isMac) as unknown as SettingsStore,
    },
  ),
);

setCustomThemeSource(() => useSettings.getState().customThemes);
setTerminalColorsSource(() => useSettings.getState().terminalColors);
setShortcutSource(() => useSettings.getState().shortcuts);

/** Turns compact mode on, as you last had it, or off: the shortcut and the palette. */
export function toggleCompact() {
  const s = useSettings.getState();
  s.update(s.compactMode === "off" ? { compactMode: s.compactLast } : { compactMode: "off", compactLast: s.compactMode });
}

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
