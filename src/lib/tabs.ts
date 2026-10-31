// Working with tabs: opening one next to the tab you were on, the next one,
// showing two side by side and back. The sidebar's tab list, the strip
// along the top, the keyboard and the command bar all go through here.

import type { Dir, DropZone } from "./layout";
import { allTabs, currentScreen, currentTarget, panesIn, panesOn, screenOf, sideBySide, tabsOn, useStore, type PaneKind } from "./store";
import { useSettings } from "./settings";
import { targetKey, type TargetRef } from "./types";

/** A new tab on a machine, next to the tab you were on there. */
export function newTab(kind: Exclude<PaneKind, "empty">, target: TargetRef) {
  useStore.getState().openIn(kind, target);
}

/** Closes a tab; the one after it comes up, as in a browser. */
export function closeTab(id: string) {
  useStore.getState().closePane(id);
}

/** Ctrl+Tab: the next tab of the machine on screen (side by side, the next pane on screen). */
export function nextTab(step: 1 | -1) {
  const s = useStore.getState();
  const target = currentTarget(s);
  if (!target) return;
  // One tab at a time it goes through all of them, as a browser does: along
  // the top, every server's. Side by side as the layout, through the panes on screen.
  const screen = currentScreen(s);
  const ids = sideBySide(s, screen) && useSettings.getState().viewMode === "split"
    ? panesIn(s, screen)
        .filter((p) => !s.hidden.includes(p.id))
        .map((p) => p.id)
    : (useSettings.getState().tabsPlacement === "top" ? allTabs(s) : tabsOn(s, target)).map((p) => p.id);
  if (ids.length < 2) return;
  const i = ids.indexOf(s.focused);
  s.focus(ids[(i + step + ids.length) % ids.length]);
}

/**
 * Shows tab `id` beside `nextTo` (the tab on screen, by default), whichever
 * servers they're on. One at a time, these two become the pair on screen, as
 * in a browser; the other tabs keep running and come back with a click.
 */
export function placeBeside(id: string, zone: DropZone = "right", nextTo?: string) {
  const s = useStore.getState();
  const other = nextTo ?? s.focused;
  const screen = screenOf(s, other);
  if (!s.panes[id] || !screen || id === other) return;
  if (useSettings.getState().viewMode === "tabs" || !sideBySide(s, screen)) {
    const rest = panesIn(s, screen)
      .map((t) => t.id)
      .filter((t) => t !== id && t !== other);
    s.setSplit(screen, true);
    useStore.setState({ hidden: [...new Set([...s.hidden.filter((h) => h !== id && h !== other), ...rest])] });
  }
  useStore.getState().movePane(id, other, zone);
}

/**
 * Split right or down: a new terminal on the same server beside this pane,
 * also when you work with one tab at a time (the two become a pair).
 */
export function splitPane(id: string, dir: Dir) {
  const s = useStore.getState();
  // Already side by side: one more pane on screen. Otherwise these two pair up.
  const beside = sideBySide(s, screenOf(s, id));
  const nid = s.split(id, dir, beside);
  if (nid === id) return;
  if (!beside) placeBeside(nid, dir === "row" ? "right" : "bottom", id);
}

/**
 * Takes a pane out of side by side: it goes back to being a tab of its own,
 * still running (another server's, back to its own server). With one pane
 * left on screen, the screen is back to one tab at a time. As tabs, a pane's
 * × does this: closing the split never closes the tab.
 */
export function toOwnTab(id: string) {
  const s = useStore.getState();
  const screen = screenOf(s, id);
  if (!screen || !sideBySide(s, screen)) return;
  const rest = panesIn(s, screen).filter((p) => p.id !== id && !s.hidden.includes(p.id));
  const guest = !!s.panes[id]?.target && targetKey(s.panes[id].target!) !== screen;
  if (rest.length > 1 || guest) s.setHidden([id], true);
  if (rest.length > 1) return;
  useStore.getState().setSplit(screen, false);
  if (rest[0]) useStore.getState().focus(rest[0].id);
}

/** One tab at a time, or the tabs you switched on side by side, on the machine on screen. */
export function toggleSplit() {
  const s = useStore.getState();
  const screen = currentScreen(s);
  if (!screen) return;
  const on = sideBySide(s, screen);
  s.setSplit(screen, !on);
  // The tab you're on is always among the ones on screen.
  if (!on && s.hidden.includes(s.focused)) s.setHidden([s.focused], false);
}

/**
 * A machine's Files (or Stats) tab: the one it has, else a new one, so every
 * way of opening files lands in the same place. Files go to `path` when
 * given. One tab at a time, it fills the window; it never joins tabs you
 * put side by side, unless it's one of them.
 */
export function showTab(kind: "files" | "stats", target: TargetRef, path?: string) {
  const s = useStore.getState();
  const existing = panesOn(s, target).find((p) => p.kind === kind);
  if (!existing) return void s.openIn(kind, target, { path });
  if (path) s.setPane(existing.id, { path });
  s.focus(existing.id);
}

/** Files of this machine, at `path` if given: the Files tab you have, or a new one. */
export function showFiles(target: TargetRef, path?: string) {
  showTab("files", target, path);
}
