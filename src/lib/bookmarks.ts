// Folder bookmarks and recently visited folders, per machine.

import { create } from "zustand";
import { persist } from "zustand/middleware";

const MAX_RECENT = 6;

interface BookmarksState {
  /** targetKey → pinned folder paths, in the order they were added. */
  pins: Record<string, string[]>;
  /** targetKey → recently opened folders, newest first. */
  recent: Record<string, string[]>;
  toggle(target: string, path: string): void;
  remove(target: string, path: string): void;
  visit(target: string, path: string): void;
  forgetRecent(target: string, path: string): void;
}

export const useBookmarks = create<BookmarksState>()(
  persist(
    (set) => ({
      pins: {},
      recent: {},
      toggle: (t, path) =>
        set((s) => {
          const list = s.pins[t] ?? [];
          return { pins: { ...s.pins, [t]: list.includes(path) ? list.filter((p) => p !== path) : [...list, path] } };
        }),
      remove: (t, path) => set((s) => ({ pins: { ...s.pins, [t]: (s.pins[t] ?? []).filter((p) => p !== path) } })),
      forgetRecent: (t, path) => set((s) => ({ recent: { ...s.recent, [t]: (s.recent[t] ?? []).filter((p) => p !== path) } })),
      visit: (t, path) =>
        set((s) => {
          const list = s.recent[t] ?? [];
          if (list[0] === path) return s;
          return { recent: { ...s.recent, [t]: [path, ...list.filter((p) => p !== path)].slice(0, MAX_RECENT) } };
        }),
    }),
    { name: "sshee-bookmarks", version: 1 },
  ),
);

export function folderName(path: string): string {
  if (path === "/") return "/";
  return path.replace(/\/+$/, "").split("/").pop() || path;
}

/** Well-known Linux folders worth one click. */
export const LINUX_PLACES: { path: string; label: string }[] = [
  { path: "/var/log", label: "Logs" },
  { path: "/etc", label: "Config" },
  { path: "/var/www", label: "Web" },
  { path: "/opt", label: "Apps" },
  { path: "/tmp", label: "Temp" },
];
