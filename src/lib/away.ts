// "While you were away": when a durable session comes back after a dropped
// connection, compare the server's scrollback with what was on screen when
// the connection went, and keep what's new.

import type { Terminal } from "@xterm/xterm";
import { countProblems } from "./highlight";
import { maskText, streamerOn } from "./privacy";
import type { Recap } from "./store";

const SSH_NOISE = /^(Connection to .* closed|client_loop: |packet_write_wait|Timeout, server .* not responding|Connection reset by|Broken pipe)/;

/**
 * The last few finished lines above the cursor: they mark where the screen
 * was. Rows that wrapped are joined back into one line, the way tmux reports
 * its scrollback.
 */
export function anchorLines(term: Terminal, count = 3): string[] {
  const buf = term.buffer.active;
  const row = (y: number, trim: boolean) => buf.getLine(y)?.translateToString(trim) ?? "";
  // A row continues the one above when xterm wrapped it, or when the one above
  // is full to the edge (tmux redraws long lines as separate full rows).
  const continues = (y: number) => y > 0 && (!!buf.getLine(y)?.isWrapped || row(y - 1, true).length >= term.cols - 1);
  const out: string[] = [];
  let y = buf.baseY + buf.cursorY - 1;
  while (y >= 0 && out.length < count) {
    let text = row(y, true);
    let top = y;
    while (continues(top)) {
      top--;
      text = row(top, false) + text;
    }
    y = top - 1;
    // ssh's own goodbye isn't in the server's scrollback.
    if (SSH_NOISE.test(text)) continue;
    if (!text.trim()) {
      if (out.length) break;
      continue;
    }
    out.unshift(text);
  }
  return out;
}

/** Lines after the last place the anchor shows up, or null if it doesn't. */
export function linesAfter(history: string[], anchor: string[]): string[] | null {
  if (!anchor.length) return null;
  const norm = (s: string) => s.trimEnd();
  const want = anchor.map(norm);
  for (let i = history.length - want.length; i >= 0; i--) {
    if (want.every((w, k) => norm(history[i + k]) === w)) {
      const rest = history.slice(i + want.length);
      while (rest.length && !rest[rest.length - 1].trim()) rest.pop();
      return rest;
    }
  }
  return null;
}

/** The line the cursor was on when the connection dropped, usually the prompt. */
export function cursorLine(term: Terminal): string {
  const buf = term.buffer.active;
  return buf.getLine(buf.baseY + buf.cursorY)?.translateToString(true) ?? "";
}

export function buildRecap(historyText: string, anchor: string[], current: string, since: number): Recap | null {
  let history = historyText.replace(/\r/g, "").split("\n");
  // The screen showed hidden names in streamer mode; compare like with like.
  if (streamerOn()) history = history.map(maskText);
  const lines = linesAfter(history, anchor);
  // That line was already on screen when the connection dropped.
  if (lines?.length && current.trim() && lines[0].trimEnd() === current.trimEnd()) lines.shift();
  if (!lines?.length) return null;
  return { since, until: Date.now(), lines, ...countProblems(lines) };
}
