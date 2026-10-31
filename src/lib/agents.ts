// AI coding agents on a server: which ones are installed (asked once per
// server per run, after its first terminal connects), and starting one in
// the folder a terminal is in.

import { useEffect } from "react";
import { create } from "zustand";
import { backend } from "./backend";
import { shellQuote } from "./format";
import { useStore, type PaneState } from "./store";
import { findTerminal } from "./terminals";
import { targetKey, type TargetRef } from "./types";

export type AgentId = "claude" | "codex" | "opencode" | "omp";

/** In the order they show up; `id` is also the command that starts each one. */
export const AGENTS: { id: AgentId; name: string }[] = [
  { id: "claude", name: "Claude Code" },
  { id: "codex", name: "Codex" },
  { id: "opencode", name: "OpenCode" },
  { id: "omp", name: "oh-my-pi" },
];

const NONE: AgentId[] = [];
const useFound = create<Record<string, AgentId[]>>(() => ({}));
const asked = new Set<string>();

/** Asks a server once per run which agents it has. */
function detect(key: string, target: TargetRef) {
  if (asked.has(key)) return;
  asked.add(key);
  backend
    .detectAgents(target)
    .then((ids) => useFound.setState({ [key]: AGENTS.filter((a) => ids.includes(a.id)).map((a) => a.id) }))
    .catch(() => asked.delete(key));
}

/** The agents on this terminal's server, once it's connected. */
export function useAgents(target: TargetRef | null, connected: boolean): AgentId[] {
  const key = target?.kind === "host" ? targetKey(target) : null;
  useEffect(() => {
    if (key && target && connected) detect(key, target);
  }, [key, target, connected]);
  return useFound((s) => (key ? (s[key] ?? NONE) : NONE));
}

/** The agent a terminal is running, judging by its foreground program. */
export function agentRunning(command: string | undefined): AgentId | null {
  if (!command) return null;
  const c = command.toLowerCase();
  return AGENTS.find((a) => a.id === c)?.id ?? null;
}

export function agentName(id: AgentId): string {
  return AGENTS.find((a) => a.id === id)?.name ?? id;
}

const WATCH_MS = 5000;

/**
 * Keeps the store's `foreground` current for terminals on servers, so tabs
 * can show the agent working in them: one question per server every few
 * seconds, and none while the window is hidden.
 */
export function startForegroundWatch(): () => void {
  let stopped = false;
  let timer = 0;
  const tick = async () => {
    if (document.visibilityState === "visible") {
      const s = useStore.getState();
      // Every terminal of a server you're connected to, also tabs you haven't
      // opened since SSHeesh started: their sessions run on the server anyway.
      const online = new Map<string, TargetRef>();
      for (const [id, st] of Object.entries(s.sessions)) {
        const t = s.panes[id]?.target;
        if (st?.kind === "connected" && t?.kind === "host") online.set(targetKey(t), t);
      }
      const byHost = new Map<string, { target: TargetRef; panes: PaneState[] }>();
      for (const p of Object.values(s.panes)) {
        if (p.kind !== "terminal" || p.target?.kind !== "host" || !p.sessionKey || p.container) continue;
        const key = targetKey(p.target);
        const target = online.get(key);
        if (!target) continue;
        // Servers without an agent aren't asked: there'd be nothing to name.
        const found = useFound.getState()[key];
        if (!found) detect(key, target);
        if (!found?.length) continue;
        const entry = byHost.get(key) ?? { target, panes: [] };
        entry.panes.push(p);
        byHost.set(key, entry);
      }
      const answers = await Promise.all(
        [...byHost.values()].map(async ({ target, panes }) => ({ panes, list: await backend.foreground(target).catch(() => null) })),
      );
      // One update for all of them, and none when nothing changed.
      const before = useStore.getState().foreground;
      const next = { ...before };
      for (const { panes, list } of answers) {
        if (!list) continue;
        for (const p of panes) {
          const command = list.find((f) => f.sessionKey === p.sessionKey)?.command;
          if (command) next[p.id] = command;
          else delete next[p.id];
        }
      }
      const keys = Object.keys(next);
      if (keys.length !== Object.keys(before).length || keys.some((k) => next[k] !== before[k])) useStore.setState({ foreground: next });
    }
    if (!stopped) timer = window.setTimeout(tick, WATCH_MS);
  };
  timer = window.setTimeout(tick, 1500);
  return () => {
    stopped = true;
    window.clearTimeout(timer);
  };
}

/**
 * Starts an agent where you are: in this terminal when its shell is waiting
 * for a command, otherwise in a new terminal beside it, in the same folder,
 * so whatever is running keeps going.
 */
export async function startAgent(pane: PaneState, agent: AgentId) {
  if (!pane.target) return;
  const info = pane.sessionKey ? await backend.paneInfo(pane.target, pane.sessionKey).catch(() => null) : null;
  if (!info || info.idle) {
    // Ctrl+E Ctrl+U first: clear anything half-typed at the prompt.
    backend.writeSession(pane.id, `\x05\x15${agent}\r`);
    findTerminal(pane.id)?.focus();
    return;
  }
  useStore.getState().openIn("terminal", pane.target, { from: pane.id, split: true, initialCommand: `cd ${shellQuote(info.cwd)} && ${agent}` });
}
