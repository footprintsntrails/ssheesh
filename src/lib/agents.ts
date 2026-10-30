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

/** The agents on this terminal's server, once it's connected. */
export function useAgents(target: TargetRef | null, connected: boolean): AgentId[] {
  const key = target?.kind === "host" ? targetKey(target) : null;
  useEffect(() => {
    if (!key || !target || !connected || asked.has(key)) return;
    asked.add(key);
    backend
      .detectAgents(target)
      .then((ids) => useFound.setState({ [key]: AGENTS.filter((a) => ids.includes(a.id)).map((a) => a.id) }))
      .catch(() => asked.delete(key));
  }, [key, target, connected]);
  return useFound((s) => (key ? (s[key] ?? NONE) : NONE));
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
