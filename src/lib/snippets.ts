// Snippets: commands you keep and run again, with blanks to fill in. A
// blank is written {{name}}, or {{name:what it is if you leave it empty}}.
// {{container}} offers the server's containers to pick from.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { newId } from "./layout";

export interface Snippet {
  id: string;
  name: string;
  command: string;
}

export interface Blank {
  name: string;
  /** Used when you leave it empty. */
  fallback: string;
}

const BLANK = /\{\{\s*([A-Za-z_][\w-]*)\s*(?::([^}]*))?\}\}/g;

/** The blanks in a command, each once, in order. */
export function blanks(command: string): Blank[] {
  const seen = new Map<string, Blank>();
  for (const m of command.matchAll(BLANK)) {
    if (!seen.has(m[1])) seen.set(m[1], { name: m[1], fallback: (m[2] ?? "").trim() });
  }
  return [...seen.values()];
}

/** The command with its blanks filled in. */
export function fill(command: string, values: Record<string, string>): string {
  return command.replace(BLANK, (_, name: string, fallback?: string) => values[name]?.trim() || (fallback ?? "").trim());
}

/** "max_lines" → "Max lines". */
export function blankLabel(name: string): string {
  const words = name.replace(/[_-]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

const STARTERS: Snippet[] = [
  { id: "s-disk", name: "How full the disks are", command: "df -h -x tmpfs -x devtmpfs" },
  { id: "s-logs", name: "Follow a container's log", command: "docker logs -f --tail {{lines:100}} {{container}}" },
  { id: "s-restart", name: "Restart a service", command: "sudo systemctl restart {{service}} && systemctl status --no-pager {{service}}" },
  { id: "s-ports", name: "What's listening on which port", command: "sudo ss -ltnp" },
  { id: "s-big", name: "Biggest folders here", command: "du -xh --max-depth=1 {{folder:.}} 2>/dev/null | sort -rh | head -n 15" },
];

interface SnippetState {
  snippets: Snippet[];
  save(s: Omit<Snippet, "id"> & { id?: string }): void;
  remove(id: string): void;
}

export const useSnippets = create<SnippetState>()(
  persist(
    (set) => ({
      snippets: STARTERS,
      save: (s) =>
        set((st) => {
          const snippet = { name: s.name.trim(), command: s.command.trim(), id: s.id ?? newId("s") };
          const i = st.snippets.findIndex((x) => x.id === snippet.id);
          return { snippets: i >= 0 ? st.snippets.map((x, j) => (j === i ? snippet : x)) : [...st.snippets, snippet] };
        }),
      remove: (id) => set((st) => ({ snippets: st.snippets.filter((x) => x.id !== id) })),
    }),
    { name: "sshee-snippets", version: 1 },
  ),
);
