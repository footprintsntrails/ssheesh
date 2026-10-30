// User preferences. Saved on this computer and applied live.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { HighlightRule } from "./highlight";
import { ACCENTS, type AppThemeId } from "./themes";

export type UpdateMode = "auto" | "notify" | "manual";

/** After a dropped connection: reconnect by itself, show a Reconnect button, or stay quiet. */
export type ReconnectMode = "auto" | "ask" | "off";

export interface Settings {
  theme: AppThemeId;
  /** Switch between a light and a dark theme with the system. */
  themeFollowsSystem: boolean;
  lightTheme: AppThemeId;
  darkTheme: AppThemeId;
  /** Interface size in percent; the terminal font has its own size. */
  uiZoom: number;
  /** How much to fade panes you're not using, 0 to 50 percent. */
  dimInactive: number;
  animations: boolean;
  /** The >_< mark in the sidebar and empty panes. */
  logo: "winking" | "still" | "hidden";
  /** Hides the sidebar, toolbar and vitals; they slide in at the window's edges. */
  compactMode: boolean;
  /** The first-launch hello has been seen (or skipped). */
  onboarded: boolean;
  /** The version whose "What's new" you've seen, to show notes after an update. */
  lastSeenVersion: string;
  accent: string;
  terminalScheme: string;
  terminalFont: string;
  terminalFontSize: number;
  terminalLineHeight: number;
  cursorStyle: "bar" | "block" | "underline";
  cursorBlink: boolean;
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
  placesWidth: number;
  /** The file manager's places and bookmarks column. */
  placesMode: "wide" | "narrow" | "hidden";
  /** Places you took out of the file manager's column ("home", "/var/log", "recent"…). */
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
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "midnight",
  themeFollowsSystem: false,
  lightTheme: "daylight",
  darkTheme: "midnight",
  uiZoom: 100,
  dimInactive: 0,
  animations: true,
  logo: "winking",
  compactMode: false,
  onboarded: false,
  lastSeenVersion: "",
  accent: ACCENTS[0].value,
  terminalScheme: "auto",
  terminalFont: "system",
  terminalFontSize: 13,
  terminalLineHeight: 1.22,
  cursorStyle: "bar",
  cursorBlink: true,
  terminalPadding: 12,
  reconnectMode: "ask",
  sidebarSide: "left",
  sidebarVisible: true,
  sidebarWidth: 232,
  sidebarMode: "wide",
  railStatus: "frame",
  expanded: [],
  placesWidth: 172,
  placesMode: "narrow",
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
    { name: "sshee-settings", version: 1 },
  ),
);

/** Pushes theme, accent and spacing into CSS variables. */
export function applyAppearance(s: Settings) {
  const root = document.documentElement;
  root.dataset.theme = s.theme;
  root.classList.toggle("reduce-motion", !s.animations);
  root.style.setProperty("--ui-accent", s.accent);
  root.style.setProperty("--ui-gap", `${s.paneGap}px`);
  root.style.setProperty("--ui-radius", `${s.paneRadius}px`);
}
