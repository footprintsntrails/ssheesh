// Working with tabs: opening one next to the tab you were on, the next one,
// showing two side by side and back. The sidebar's tab list, the strip
// along the top, the keyboard and the command bar all go through here.

import type { DropZone } from "./layout";
import { currentTarget, isSplit, panesOn, tabsOn, useStore, type PaneKind } from "./store";
import { sameTarget, type TargetRef } from "./types";

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
  const ids = isSplit(s, target)
    ? panesOn(s, target)
        .filter((p) => !s.hidden.includes(p.id))
        .map((p) => p.id)
    : tabsOn(s, target).map((p) => p.id);
  if (ids.length < 2) return;
  const i = ids.indexOf(s.focused);
  s.focus(ids[(i + step + ids.length) % ids.length]);
}

/**
 * Shows tab `id` beside `nextTo` (the tab on screen, by default). One at a
 * time, the machine switches to side by side with just these two on screen;
 * its other tabs keep running and come back with a click.
 */
export function placeBeside(id: string, zone: DropZone = "right", nextTo?: string) {
  const s = useStore.getState();
  const other = nextTo ?? s.focused;
  const target = s.panes[id]?.target;
  if (!target || id === other || !sameTarget(target, s.panes[other]?.target)) return;
  if (!isSplit(s, target)) {
    const rest = tabsOn(s, target)
      .map((t) => t.id)
      .filter((t) => t !== id && t !== other);
    s.setSplit(target, true);
    useStore.setState({ hidden: [...new Set([...s.hidden.filter((h) => h !== id && h !== other), ...rest])] });
  }
  useStore.getState().movePane(id, other, zone);
}

/**
 * Takes a pane out of side by side: it goes back to being a tab of its own,
 * still running. With one pane left on screen, the machine is back to one
 * tab at a time.
 */
export function toOwnTab(id: string) {
  const s = useStore.getState();
  const target = s.panes[id]?.target;
  if (!target || !isSplit(s, target)) return;
  const rest = panesOn(s, target).filter((p) => p.id !== id && !s.hidden.includes(p.id));
  if (rest.length > 1) return s.setHidden([id], true);
  s.setSplit(target, false);
  if (rest[0]) s.focus(rest[0].id);
}

/** One tab at a time, or the tabs you switched on side by side, on the machine on screen. */
export function toggleSplit() {
  const s = useStore.getState();
  const target = currentTarget(s);
  if (!target) return;
  s.setSplit(target, !isSplit(s, target));
  // The tab you're on is always among the ones on screen.
  if (s.hidden.includes(s.focused)) s.setHidden([s.focused], false);
}

/** Files of this machine: the tab you have, or a new one. */
export function showFiles(target: TargetRef) {
  const s = useStore.getState();
  const files = panesOn(s, target).find((p) => p.kind === "files");
  if (files) s.focus(files.id);
  else newTab("files", target);
}
