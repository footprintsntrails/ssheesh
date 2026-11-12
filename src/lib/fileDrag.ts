// Dragging files out of a Files list. Onto a folder of the same server (or
// the same server's other tab) they move there, as in a file manager; onto
// another server's Files they're copied straight across; let go outside the
// window, a server's files download to this computer. Pointer events rather
// than HTML drag and drop, which the window's own file drops switch off on
// Windows.

import type { PointerEvent as ReactPointerEvent } from "react";
import { create } from "zustand";
import { targetName } from "./privacy";
import { useStore } from "./store";
import { sameTarget, type TargetRef } from "./types";
import { copyToServer, filesLabel } from "./upload";

/** What's being dragged: `paths`, all in the folder `dir` on `from`. */
export interface Dragged {
  from: TargetRef;
  dir: string;
  paths: string[];
}

/** What's under the pointer: a Files pane's machine and folder, and the folder row there, if any. */
export interface Under {
  target: TargetRef;
  paneDir: string;
  folder: string | undefined;
}

export type Landing = { kind: "move" | "copy"; dir: string } | { kind: "download" };

/** Where dragged files would land in the window: a Files pane, the folder there, and how. */
export interface FileDrop {
  pane: string;
  dir: string;
  kind: "move" | "copy";
}

/** The drop under the pointer while files are dragged, for the pane to light up. */
export const useFileDrag = create<{ over: FileDrop | null }>(() => ({ over: null }));

const trimmed = (p: string) => (p.length > 1 ? p.replace(/\/+$/, "") : p);

/**
 * What letting go here would do, or null for nothing. The same machine
 * moves, unless the files are there already or a folder would go into
 * itself; another server copies; outside the window a server's files download.
 */
export function dropFor(d: Dragged, under: Under | "outside" | null): Landing | null {
  if (!under) return null;
  if (under === "outside") return d.from.kind === "host" ? { kind: "download" } : null;
  const dir = trimmed(under.folder ?? under.paneDir);
  if (!sameTarget(under.target, d.from)) return under.target.kind === "host" ? { kind: "copy", dir } : null;
  if (dir === trimmed(d.dir)) return null;
  if (d.paths.some((p) => dir === trimmed(p) || dir.startsWith(`${trimmed(p)}/`))) return null;
  return { kind: "move", dir };
}

/** The pointer is past the window's edge. */
function outside(x: number, y: number): boolean {
  return x < 0 || y < 0 || x >= window.innerWidth || y >= window.innerHeight;
}

function under(x: number, y: number): { pane: string; under: Under } | "outside" | null {
  if (outside(x, y)) return "outside";
  const el = document.elementFromPoint(x, y);
  const pane = el?.closest<HTMLElement>("[data-files-pane]");
  const id = pane?.dataset.filesPane;
  const target = id ? useStore.getState().panes[id]?.target : undefined;
  const paneDir = pane?.dataset.filesDir;
  if (!id || !target || !paneDir) return null;
  return { pane: id, under: { target, paneDir, folder: el?.closest<HTMLElement>("[data-folder]")?.dataset.folder } };
}

export interface DragActions {
  /** Moves the dragged files into `dir` on their own machine. */
  move(dir: string): void;
  /** Downloads them to this computer. */
  download(): void;
}

/** Starts dragging `d` once the pointer moves a little. */
export function dragFiles(e: ReactPointerEvent, d: Dragged, actions: DragActions) {
  if (e.button !== 0 || !d.paths.length) return;
  const sx = e.clientX;
  const sy = e.clientY;
  const label = filesLabel(d.paths);
  let ghost: HTMLDivElement | null = null;
  const at = (x: number, y: number) => {
    const u = under(x, y);
    const landing = dropFor(d, u === "outside" || !u ? u : u.under);
    const drop: FileDrop | null = landing && landing.kind !== "download" && u && u !== "outside" ? { pane: u.pane, dir: landing.dir, kind: landing.kind } : null;
    return { landing, drop, to: u && u !== "outside" ? u.under.target : null };
  };
  const move = (ev: PointerEvent) => {
    if (!ghost) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      ghost = document.createElement("div");
      ghost.className = "file-drag-ghost";
      document.body.appendChild(ghost);
    }
    const { landing, drop, to } = at(ev.clientX, ev.clientY);
    ghost.textContent =
      landing?.kind === "download"
        ? `Let go to download ${label}`
        : landing?.kind === "copy" && to
          ? `Copy ${label} to ${targetName(to)}`
          : landing?.kind === "move"
            ? `Move ${label} to ${landing.dir.split("/").pop() || "/"}`
            : label;
    ghost.dataset.ok = landing ? "yes" : "no";
    // Past the edge the label waits just inside it: nothing draws outside the window.
    const x = Math.min(Math.max(ev.clientX + 14, 8), window.innerWidth - ghost.offsetWidth - 8);
    const y = Math.min(Math.max(ev.clientY + 12, 8), window.innerHeight - ghost.offsetHeight - 8);
    ghost.style.transform = `translate(${x}px, ${y}px)`;
    useFileDrag.setState((s) => (s.over?.pane === drop?.pane && s.over?.dir === drop?.dir && s.over?.kind === drop?.kind ? s : { over: drop }));
  };
  const stop = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", stop);
    ghost?.remove();
    useFileDrag.setState({ over: null });
  };
  const up = (ev: PointerEvent) => {
    const dragged = !!ghost;
    stop();
    if (!dragged) return;
    // The click that ends a drag isn't one.
    const swallow = (c: MouseEvent) => c.stopPropagation();
    window.addEventListener("click", swallow, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
    const { landing, to } = at(ev.clientX, ev.clientY);
    if (landing?.kind === "download") actions.download();
    else if (landing?.kind === "move") actions.move(landing.dir);
    else if (landing?.kind === "copy" && to) void copyToServer(d.from, d.paths, to, landing.dir);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", stop);
}
