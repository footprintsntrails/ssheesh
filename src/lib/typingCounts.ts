// The counts from shadowEcho.ts, added up and saved on this computer: numbers
// only, under their own name, with nothing of what was typed or where. Nothing
// reads them but Settings → Terminal.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { quietStorage } from "./persist";
import type { EchoCount, EchoPlace } from "./shadowEcho";

export type EchoRoute = "tor" | "direct";

/** How long echoes took: how many fell at or under each of `ECHO_MS`, or above the last. */
export type EchoTally = Partial<Record<EchoCount, number>> & { echoes?: number[] };
export type TypingCounts = Partial<Record<EchoRoute, Partial<Record<EchoPlace, EchoTally>>>>;

const ECHO_MS = [10, 20, 40, 60, 80, 100, 150, 200, 300, 400, 600, 800, 1200, 2000, 3000];

interface CountsStore {
  counts: TypingCounts;
}

export const useTypingCounts = create<CountsStore>()(
  persist((): CountsStore => ({ counts: {} }), {
    name: "sshee-typing-counts",
    storage: quietStorage(),
  }),
);

function change(route: EchoRoute, place: EchoPlace, patch: (t: EchoTally) => EchoTally) {
  const { counts } = useTypingCounts.getState();
  const places = counts[route] ?? {};
  useTypingCounts.setState({ counts: { ...counts, [route]: { ...places, [place]: patch(places[place] ?? {}) } } });
}

export function addCount(route: EchoRoute, place: EchoPlace, what: EchoCount) {
  change(route, place, (t) => ({ ...t, [what]: (t[what] ?? 0) + 1 }));
}

export function addEcho(route: EchoRoute, place: EchoPlace, ms: number) {
  change(route, place, (t) => {
    const echoes = [...(t.echoes ?? [])];
    const slot = ECHO_MS.findIndex((top) => ms <= top);
    const i = slot === -1 ? ECHO_MS.length : slot;
    while (echoes.length <= i) echoes.push(0);
    echoes[i]++;
    return { ...t, echoes };
  });
}

export function clearTypingCounts() {
  useTypingCounts.setState({ counts: {} });
}

/** The echo time `share` of all echoes came within, by the slot it fell in. Rough on purpose: no key's own time is kept. Above the last slot it's Infinity. */
export function echoWithin(echoes: number[] | undefined, share: number): number | null {
  const total = (echoes ?? []).reduce((a, b) => a + b, 0);
  if (!total) return null;
  let seen = 0;
  for (let i = 0; i < echoes!.length; i++) {
    seen += echoes![i];
    if (seen >= total * share) return ECHO_MS[i] ?? Infinity;
  }
  return Infinity;
}

/** What the counts come to for one way of connecting. A share is null with nothing to go on. */
export interface TypingSummary {
  /** Every key typed. */
  keys: number;
  /** Characters and Backspace at a command line or the like, as against an agent or a full-screen program. */
  lineTyped: number;
  agentShare: number | null;
  screenShare: number | null;
  /** Of what's typed at a command line: could have shown at once, by the rule a client can check alone. */
  atOnceShare: number | null;
  /** The same on rows that look like a shell's prompt: nearer what a prompt the server announces would give. */
  atPromptShare: number | null;
  /** Of those with an answer: the guess was right. */
  rightShare: number | null;
  /** Could have shown, and came back different or was overtaken by something that did: a wrong picture until the answer. */
  wrong: number;
  /** Could have shown, and nothing came back: what could have been a password on screen. */
  silent: number;
  /** Had to wait and nothing came back: what the gate held back. */
  heldSilent: number;
  /** The gate opened on a row that reads like a password prompt. */
  passwordOpen: number;
  /** Text to the right of the cursor stopped it guessing (zsh's and fish's suggestions, a right-hand prompt). */
  midLine: number;
  echoMedian: number | null;
  echoP90: number | null;
}

const share = (part: number, whole: number) => (whole > 0 ? part / whole : null);

export function typingSummary(places: Partial<Record<EchoPlace, EchoTally>> | undefined): TypingSummary {
  const line = places?.line ?? {};
  const agent = places?.agent ?? {};
  const screen = places?.screen ?? {};
  const all = [line, agent, screen];
  const sum = (what: EchoCount) => all.reduce((n, t) => n + (t[what] ?? 0), 0);
  const typed = sum("typed");
  const lineTyped = line.typed ?? 0;
  const wrong = (line.wrong ?? 0) + (line.dropped ?? 0);
  const silent = line.silent ?? 0;
  const right = line.right ?? 0;
  return {
    keys: sum("keys"),
    lineTyped,
    agentShare: share(agent.typed ?? 0, typed),
    screenShare: share(screen.typed ?? 0, typed),
    atOnceShare: share(line.atOnce ?? 0, lineTyped),
    atPromptShare: share(line.atPrompt ?? 0, lineTyped),
    rightShare: share(right, right + wrong + silent),
    wrong,
    silent,
    heldSilent: sum("heldSilent"),
    passwordOpen: sum("passwordOpen"),
    midLine: line.midLine ?? 0,
    echoMedian: echoWithin(line.echoes, 0.5),
    echoP90: echoWithin(line.echoes, 0.9),
  };
}
