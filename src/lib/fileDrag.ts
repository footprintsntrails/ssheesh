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
  /** The one under the pointer is a folder: the picture that flies with it. */
  folder?: boolean;
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

const svg = (size: number, body: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;
const FOLDER = '<path fill="currentColor" fill-opacity="0.22" d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>';
const FILE = '<path fill="currentColor" fill-opacity="0.18" d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>';
const ARROW_DOWN = '<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>';

const esc = (t: string) => t.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);

/** The card that flies with the pointer: a folder or a file, how many, and what letting go would do. */
function ghostCard(d: Dragged, name: string): { el: HTMLDivElement; say(does: string, ok: boolean): void } {
  const el = document.createElement("div");
  el.className = "file-drag-ghost";
  const many = d.paths.length > 1;
  el.innerHTML = `<div class="fdg-card"><span class="fdg-icon"${many ? " data-many" : ""}>${svg(28, d.folder ? FOLDER : FILE)}${
    many ? `<span class="fdg-count">${d.paths.length}</span>` : ""
  }</span><span class="fdg-text"><span class="fdg-name">${esc(name)}</span><span class="fdg-does"></span></span></div>`;
  const does = el.querySelector<HTMLElement>(".fdg-does")!;
  return {
    el,
    say(text, ok) {
      if (does.textContent !== text) does.textContent = text;
      el.dataset.ok = ok ? "yes" : "no";
    },
  };
}

/** What the window shows while the pointer is past its edge, where nothing of ours can draw. */
function outHint(d: Dragged, name: string, lands: string): HTMLDivElement {
  const el = document.createElement("div");
  el.className = "file-drag-out";
  el.innerHTML = `<div class="fdo-card"><span class="fdo-icon">${svg(54, d.folder ? FOLDER : FILE)}<span class="fdo-arrow">${svg(15, ARROW_DOWN)}</span></span><span class="fdo-title">Let go to download ${esc(name)}</span><span class="fdo-note">${esc(lands)}</span></div>`;
  return el;
}

/** A pointer that carries a folder with an arrow: past the window's edge it's all that still shows. */
function downloadCursor(): string {
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--ui-accent").trim() || "#4eba65";
  const art = `<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><path d="M3 3l7.5 18 2.6-7.4L20.5 11z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/><g transform="translate(12 12)"><path d="M17 16.500a1.700 1.700 0 0 0 1.700-1.700V6.800a1.700 1.700 0 0 0-1.700-1.700h-6.300a1.700 1.700 0 0 1-1.400-.8l-.7-1A1.700 1.700 0 0 0 7.200 2.500H3.400a1.700 1.700 0 0 0-1.700 1.700v10.600a1.700 1.700 0 0 0 1.700 1.700Z" fill="${accent}" stroke="#111" stroke-width="1.2"/><path d="M10.200 7.400v5.600m-2.500-2.300 2.500 2.500 2.500-2.500" fill="none" stroke="#fff" stroke-width="1.700" stroke-linecap="round" stroke-linejoin="round"/></g></svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(art)}") 3 3, copy`;
}

export interface DragActions {
  /** Where a download would land, in a few words, for the note shown while the pointer is outside. */
  lands: string;
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
  let ghost: ReturnType<typeof ghostCard> | null = null;
  let hint: HTMLDivElement | null = null;
  const root = document.documentElement;
  const at = (x: number, y: number) => {
    const u = under(x, y);
    const landing = dropFor(d, u === "outside" || !u ? u : u.under);
    const drop: FileDrop | null = landing && landing.kind !== "download" && u && u !== "outside" ? { pane: u.pane, dir: landing.dir, kind: landing.kind } : null;
    return { landing, drop, to: u && u !== "outside" ? u.under.target : null };
  };
  const move = (ev: PointerEvent) => {
    if (!ghost) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      ghost = ghostCard(d, label);
      document.body.appendChild(ghost.el);
      root.classList.add("file-dragging");
    }
    const { landing, drop, to } = at(ev.clientX, ev.clientY);
    // Past the edge the card can't follow: the window says what letting go
    // does, and the pointer itself carries the picture.
    const out = landing?.kind === "download";
    if (out && !hint) {
      hint = outHint(d, label, actions.lands);
      document.body.appendChild(hint);
      root.style.setProperty("--file-drag-cursor", downloadCursor());
    } else if (!out && hint) {
      hint.remove();
      hint = null;
    }
    root.classList.toggle("file-drag-leaving", out);
    ghost.el.style.display = out ? "none" : "";
    ghost.say(
      landing?.kind === "copy" && to
        ? `Copy to ${targetName(to)}`
        : landing?.kind === "move"
          ? `Move to ${landing.dir.split("/").pop() || "/"}`
          : d.from.kind === "host"
            ? "To a folder, or out to download"
            : "To a folder to move it",
      !!landing,
    );
    ghost.el.style.transform = `translate(${ev.clientX + 12}px, ${ev.clientY + 14}px)`;
    useFileDrag.setState((s) => (s.over?.pane === drop?.pane && s.over?.dir === drop?.dir && s.over?.kind === drop?.kind ? s : { over: drop }));
  };
  const stop = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", stop);
    ghost?.el.remove();
    hint?.remove();
    root.classList.remove("file-dragging", "file-drag-leaving");
    root.style.removeProperty("--file-drag-cursor");
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
