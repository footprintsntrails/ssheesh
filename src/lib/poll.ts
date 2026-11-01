// How often things that look at a server again and again (vitals,
// containers) do so, and when they'd better wait.

import type { SessionState } from "./types";

/** The wait before the next look after `failures` failed ones in a row: `base`, doubling with each, never past `max`. */
export function backoff(base: number, failures: number, max: number): number {
  return Math.min(max, base * 2 ** Math.max(0, failures));
}

/**
 * Every terminal open on a server is waiting for its connection to come
 * back, so a look at that server would only wait or fail too. Terminals not
 * started yet don't count, nor do servers with none open.
 */
export function connectionDown(sessions: (SessionState | undefined)[]): boolean {
  const known = sessions.filter((s) => s !== undefined);
  return known.length > 0 && known.every((s) => s.kind === "reconnecting" || s.kind === "disconnected");
}
