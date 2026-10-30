// User preferences. Saved on this computer and applied live.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { ACCENTS, type AppThemeId } from "./themes";

export type UpdateMode = "auto" | "notify" | "manual";

/** After a dropped connection: reconnect by itself, show a Reconnect button, or stay quiet. */
export type ReconnectMode = "auto" | "ask" | "off";

export interface Settings {
  theme: AppThemeId;
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
  /** Machines whose container list is open in the sidebar. */
  expanded: string[];
  placesWidth: number;
  /** The file manager's places and bookmarks column. */
  placesMode: "wide" | "narrow" | "hidden";
  previewWidth: number;
  paneGap: number;
  paneRadius: number;
  showVitals: boolean;
  updateMode: UpdateMode;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "midnight",
  accent: ACCENTS[0].value,
  terminalScheme: "auto",
  terminalFont: "system",
  terminalFontSize: 13,
  terminalLineHeight: 1.22,
  cursorStyle: "bar",
  cursorBlink: true,
  terminalPadding: 12,
  reconnectMode: "auto",
  sidebarSide: "left",
  sidebarVisible: true,
  sidebarWidth: 232,
  expanded: [],
  placesWidth: 172,
  placesMode: "wide",
  previewWidth: 420,
  paneGap: 6,
  paneRadius: 11,
  showVitals: true,
  updateMode: "auto",
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
  root.style.setProperty("--ui-accent", s.accent);
  root.style.setProperty("--ui-gap", `${s.paneGap}px`);
  root.style.setProperty("--ui-radius", `${s.paneRadius}px`);
}
