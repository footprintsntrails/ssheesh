// Where a terminal is scrolled to, for its scrollbar. In a server terminal
// the history lives in tmux, which reports its scroll position through the
// window title ("sshee-scroll 1 100 480 24 2000", see SCROLL_TITLE in
// crates/sshee-core/src/ssh.rs).

import type { TmuxScroll } from "./types";

/** Reads tmux's report. Out of copy mode the position is left empty. */
export function parseScrollTitle(title: string): TmuxScroll | null {
  const words = title.trim().split(/\s+/);
  if (words[0] !== "sshee-scroll") return null;
  const n = words.slice(1).map(Number);
  if (n.some((x) => !Number.isInteger(x) || x < 0)) return null;
  if (n.length === 5) return { inMode: n[0] === 1, position: n[1], history: n[2], height: n[3], limit: n[4] };
  if (n.length === 4) return { inMode: n[0] === 1, position: 0, history: n[1], height: n[2], limit: n[3] };
  return null;
}

/** What a scrollbar shows: lines in all, the first one in view, lines on screen. */
export interface ScrollView {
  total: number;
  top: number;
  rows: number;
}

export function tmuxView(s: TmuxScroll): ScrollView {
  return { total: s.history + s.height, top: s.history - Math.min(s.position, s.history), rows: s.height };
}

/** The tmux position (lines scrolled up) that puts line `top` first in view. */
export function tmuxPosition(s: TmuxScroll, top: number): number {
  return Math.max(0, Math.min(s.history, Math.round(s.history - top)));
}

/** Thumb size and place as fractions of the track; a thumb never gets too small to grab. */
export function thumb(view: ScrollView, track: number, min = 24): { size: number; offset: number } {
  if (view.total <= view.rows || track <= 0) return { size: track, offset: 0 };
  const size = Math.max(min, (track * view.rows) / view.total);
  const room = track - size;
  const scrollable = view.total - view.rows;
  return { size, offset: room * Math.max(0, Math.min(1, view.top / scrollable)) };
}

/** The first line in view for a thumb dragged to `offset` pixels. */
export function topForOffset(view: ScrollView, track: number, size: number, offset: number): number {
  const room = track - size;
  if (room <= 0) return 0;
  return Math.round((Math.max(0, Math.min(room, offset)) / room) * Math.max(0, view.total - view.rows));
}
