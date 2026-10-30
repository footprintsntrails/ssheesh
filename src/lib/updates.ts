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
  /** The portable Windows build: updates are a download, not an install. */
  portable: boolean;
  status: UpdateStatus;
  lastChecked: number | null;
  check(opts?: { quiet?: boolean }): Promise<void>;
  install(): Promise<void>;
  restart(): Promise<void>;
}

export const useUpdates = create<UpdatesState>()((set, get) => ({
  current: "",
  portable: false,
  status: { kind: "idle" },
  lastChecked: null,

  async check({ quiet = false } = {}) {
    const busy = get().status.kind;
    if (busy === "checking" || busy === "downloading" || busy === "ready") return;
    set({ status: { kind: "checking" } });
    try {
      const info = await backend.checkUpdate();
      set({ lastChecked: Date.now() });
      if (!info) return set({ status: { kind: "upToDate" } });
      set({ status: { kind: "available", version: info.version, notes: info.notes } });
      if (useSettings.getState().updateMode === "auto" && !get().portable) await get().install();
    } catch (e) {
      // Background checks fail quietly (offline, server down); manual ones say why.
      set({ status: quiet ? { kind: "idle" } : { kind: "error", message: toOpError(e).message } });
    }
  },

  async install() {
    const s = get().status;
    if (s.kind !== "available") return;
    // A portable .exe can't replace itself while it runs: get the new one instead.
    if (get().portable) return void backend.openExternal(RELEASES);
    set({ status: { kind: "downloading", version: s.version, progress: 0 } });
    try {
      await backend.installUpdate((progress) => set({ status: { kind: "downloading", version: s.version, progress } }));
      set({ status: { kind: "ready", version: s.version } });
    } catch (e) {
      set({ status: { kind: "error", message: `The update didn't install: ${toOpError(e).message}` } });
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
