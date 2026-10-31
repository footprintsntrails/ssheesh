// Update checks: manual from Settings, or in the background depending on
// Settings → Updates.

import { create } from "zustand";
import { backend } from "./backend";
import { useSettings } from "./settings";
import { toOpError } from "./types";

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; version: string; notes: string }
  | { kind: "downloading"; version: string; progress: number | null }
  | { kind: "ready"; version: string }
  | { kind: "error"; message: string };

export const RELEASES = "https://github.com/footprintsntrails/ssheesh/releases/latest";

interface UpdatesState {
  current: string;
  /** The portable Windows build: it updates by swapping in a new .exe. */
  portable: boolean;
  status: UpdateStatus;
  lastChecked: number | null;
  check(opts?: { quiet?: boolean }): Promise<void>;
  /** `auto`: started by itself after a check, so a failure doesn't open the browser. */
  install(opts?: { auto?: boolean }): Promise<void>;
  /** An automatic install failed this run: later checks only offer the update. */
  autoFailed: boolean;
  restart(): Promise<void>;
}

export const useUpdates = create<UpdatesState>()((set, get) => ({
  current: "",
  portable: false,
  status: { kind: "idle" },
  lastChecked: null,
  autoFailed: false,

  async check({ quiet = false } = {}) {
    const busy = get().status.kind;
    if (busy === "checking" || busy === "downloading" || busy === "ready") return;
    set({ status: { kind: "checking" } });
    try {
      const info = await backend.checkUpdate();
      set({ lastChecked: Date.now() });
      if (!info) return set({ status: { kind: "upToDate" } });
      set({ status: { kind: "available", version: info.version, notes: info.notes } });
      if (useSettings.getState().updateMode === "auto" && !get().autoFailed) await get().install({ auto: true });
    } catch (e) {
      // Background checks fail quietly (offline, server down); manual ones say why.
      set({ status: quiet ? { kind: "idle" } : { kind: "error", message: toOpError(e).message } });
    }
  },

  async install({ auto = false } = {}) {
    const s = get().status;
    if (s.kind !== "available") return;
    const portable = get().portable;
    set({ status: { kind: "downloading", version: s.version, progress: 0 } });
    const onProgress = (progress: number | null) => set({ status: { kind: "downloading", version: s.version, progress } });
    try {
      await (portable ? backend.installPortableUpdate(onProgress) : backend.installUpdate(onProgress));
      set({ status: { kind: "ready", version: s.version } });
    } catch (e) {
      // Releases before 0.4.2 have no signed portable file; the download page always works.
      const fallback = portable ? " Get the new file from the download page instead." : "";
      set({ status: { kind: "error", message: `The update didn't install: ${toOpError(e).message}${fallback}` } });
      // The download page opens when you asked for the update, not behind your back every few hours.
      if (auto) set({ autoFailed: true });
      else if (portable) void backend.openExternal(RELEASES);
    }
  },

  async restart() {
    await backend.relaunch();
  },
}));

const SIX_HOURS = 6 * 60 * 60 * 1000;

let started = false;

/** Called once at startup. */
export function startUpdateChecks() {
  if (started) return;
  started = true;
  void backend.appVersion().then((current) => useUpdates.setState({ current }));
  void backend
    .isPortable()
    .then((portable) => useUpdates.setState({ portable }))
    .catch(() => {});
  const tick = () => {
    if (useSettings.getState().updateMode !== "manual") void useUpdates.getState().check({ quiet: true });
  };
  setTimeout(tick, 5000);
  setInterval(tick, SIX_HOURS);
}
