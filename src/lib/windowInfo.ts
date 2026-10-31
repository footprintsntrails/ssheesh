// Which window this is. SSHeesh opens more windows when you Shift+click a
// server; each has its own tabs, while settings and labels are shared.

import type { TargetRef } from "./types";

const params = new URLSearchParams(typeof location === "undefined" ? "" : location.search);

/** This window's label: "main" for the first one. */
export const windowLabel = params.get("window") ?? "main";

export const isMainWindow = windowLabel === "main";

/** The machine this window was opened for, if any. */
export function openedFor(): TargetRef | null {
  try {
    const raw = params.get("open");
    return raw ? (JSON.parse(raw) as TargetRef) : null;
  } catch {
    return null;
  }
}
