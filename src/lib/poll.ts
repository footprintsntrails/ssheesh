// How often things that look at a server again and again (vitals,
// containers) do so, and when they'd better wait.

import type { SessionState } from "./types";

/** The wait before the next look after `failures` failed ones in a row: `base`, doubling with each, never past `max`. */
export function backoff(base: number, failures: number, max: number): number {
  return Math.min(max, base * 2 ** Math.max(0, failures));
}

/**
 * Every terminal open on a server is waiting for its connection to come
 * back, or let go of it while unused, so a look at that server would only
 * wait, fail, or keep it busy for nobody. Terminals not started yet don't
 * count, nor do servers with none open.
 */
export function connectionDown(sessions: (SessionState | undefined)[]): boolean {
  const known = sessions.filter((s) => s !== undefined);
  return known.length > 0 && known.every((s) => s.kind === "reconnecting" || s.kind === "disconnected" || s.kind === "asleep");
}

/** A connection that was down, or asleep, and is coming back. */
export function wasDown(s: SessionState | undefined): boolean {
  return s?.kind === "reconnecting" || s?.kind === "disconnected" || s?.kind === "asleep" || (s?.kind === "connecting" && s.attempt > 1);
}

/**
 * A terminal unused this long (no typing, no output) lets go of its
 * connection, after `minutes` from Settings (0: never). Only one whose shell
 * lives in tmux on a server, where it keeps running: any other shell would
 * end with the connection.
 */
export function sleepsNow(state: SessionState | undefined, onServer: boolean, idleMs: number, minutes: number): boolean {
  return minutes > 0 && onServer && state?.kind === "connected" && state.durable && idleMs >= minutes * 60_000;
}
