// Dragging files from one server's Files onto another's beside it: a copy
// straight from one to the other, into the folder that's open there or the
// folder you let go on. Pointer events rather than HTML drag and drop, which
// the window's own file drops switch off on Windows.

import type { PointerEvent as ReactPointerEvent } from "react";
import { create } from "zustand";
import { targetName } from "./privacy";
import { useStore } from "./store";
import { sameTarget, type TargetRef } from "./types";
import { copyToServer, filesLabel } from "./upload";

/** Where dragged files would land: a Files pane, and the folder there. */
export interface FileDrop {
  pane: string;
  dir: string;
}

/** The drop under the pointer while files are dragged, for the pane to light up. */
export const useFileDrag = create<{ over: FileDrop | null }>(() => ({ over: null }));

/** Another server's Files under the pointer, and the folder: the row you're over, else the one open. */
function dropAt(x: number, y: number, from: TargetRef): FileDrop | null {
  const el = document.elementFromPoint(x, y);
  const pane = el?.closest<HTMLElement>("[data-files-pane]");
  const id = pane?.dataset.filesPane;
  const to = id ? useStore.getState().panes[id]?.target : undefined;
  if (!pane || !id || !to || to.kind !== "host" || sameTarget(to, from)) return null;
  const dir = el?.closest<HTMLElement>("[data-folder]")?.dataset.folder ?? pane.dataset.filesDir;
  return dir ? { pane: id, dir } : null;
}

/** Starts dragging `paths` (all in one folder on `from`) once the pointer moves a little. */
export function dragFiles(e: ReactPointerEvent, from: TargetRef, paths: string[]) {
  if (e.button !== 0 || !paths.length) return;
  const sx = e.clientX;
  const sy = e.clientY;
  const label = filesLabel(paths);
  let ghost: HTMLDivElement | null = null;
  const move = (ev: PointerEvent) => {
    if (!ghost) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      ghost = document.createElement("div");
      ghost.className = "file-drag-ghost";
      document.body.appendChild(ghost);
    }
    const over = dropAt(ev.clientX, ev.clientY, from);
    const to = over && useStore.getState().panes[over.pane]?.target;
    ghost.textContent = to ? `Copy ${label} to ${targetName(to)}` : label;
    ghost.dataset.ok = to ? "yes" : "no";
    ghost.style.transform = `translate(${ev.clientX + 14}px, ${ev.clientY + 12}px)`;
    useFileDrag.setState((s) => (s.over?.pane === over?.pane && s.over?.dir === over?.dir ? s : { over }));
  };
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    if (!ghost) return;
    ghost.remove();
    const over = dropAt(ev.clientX, ev.clientY, from);
    useFileDrag.setState({ over: null });
    // The click that ends a drag isn't one.
    const swallow = (c: MouseEvent) => c.stopPropagation();
    window.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
    const to = over && useStore.getState().panes[over.pane]?.target;
    if (over && to) void copyToServer(from, paths, to, over.dir);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}
