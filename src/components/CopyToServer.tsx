// Copying files to another server: right-click files in Files, pick the
// server and the folder there. Mount <CopyToServer /> once;
// openCopyToServer(from, paths) shows it.

import { ArrowUp, Folder, LoaderCircle, Send } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { useNames } from "../lib/privacy";
import { useStore } from "../lib/store";
import { toOpError, type TargetRef } from "../lib/types";
import { copyToServer, filesLabel } from "../lib/upload";
import { HostIcon } from "./HostIcon";
import { Modal } from "./Overlays";
import { Button } from "./ui";

interface Asked {
  from: TargetRef;
  paths: string[];
}

const useCopyAsk = create<{ asked: Asked | null; lastHost: string | null; lastDir: Record<string, string> }>(() => ({ asked: null, lastHost: null, lastDir: {} }));

/** Asks where `paths` (all in one folder on `from`) should go. */
export function openCopyToServer(from: TargetRef, paths: string[]) {
  if (paths.length) useCopyAsk.setState({ asked: { from, paths } });
}

const close = () => useCopyAsk.setState({ asked: null });

export function CopyToServer() {
  const asked = useCopyAsk((s) => s.asked);
  return (
    <Modal open={!!asked} onClose={close} width={480}>
      {asked && <CopyDialog asked={asked} />}
    </Modal>
  );
}

/** The folder on the server you're looking at, and the folders in it. */
type Place = { path: string; dirs: string[] } | null;

const parentOf = (p: string) => (p === "/" ? "/" : p.replace(/\/[^/]+\/?$/, "") || "/");

function CopyDialog({ asked }: { asked: Asked }) {
  const names = useNames();
  const hosts = useStore((s) => s.hosts);
  const others = hosts.filter((h) => !(asked.from.kind === "host" && asked.from.hostId === h.id));
  const [hostId, setHostId] = useState(() => {
    const last = useCopyAsk.getState().lastHost;
    return others.some((h) => h.id === last) ? last : (others[0]?.id ?? null);
  });
  const to: TargetRef | null = hostId ? { kind: "host", hostId } : null;
  const [place, setPlace] = useState<Place>(null);
  const [typed, setTyped] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const asking = useRef(0);

  // Looks inside a folder there; the path you typed stays if it can't.
  const go = (path: string) => {
    if (!to) return;
    const n = ++asking.current;
    setLoading(true);
    setError(null);
    void backend
      .listDir(to, path)
      .then((l) => {
        if (n !== asking.current) return;
        const dirs = l.entries.filter((e) => e.kind === "dir" && !e.name.startsWith(".")).map((e) => e.name);
        setPlace({ path: l.path, dirs: dirs.sort((a, b) => a.localeCompare(b)) });
        setTyped(l.path);
      })
      .catch((e) => n === asking.current && setError(toOpError(e).message))
      .finally(() => n === asking.current && setLoading(false));
  };

  // Each server opens where you copied to last time, else in its home folder.
  useEffect(() => {
    if (!hostId) return;
    setPlace(null);
    setTyped("");
    go(useCopyAsk.getState().lastDir[hostId] ?? "~");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId]);

  const label = filesLabel(asked.paths);
  const ready = !!to && !!place && !loading && typed.trim() === place.path;
  const send = () => {
    if (!to || !place || !hostId) return;
    useCopyAsk.setState((s) => ({ lastHost: hostId, lastDir: { ...s.lastDir, [hostId]: place.path } }));
    close();
    // Progress shows on a card in the corner.
    void copyToServer(asked.from, asked.paths, to, place.path);
  };

  return (
    <form
      className="p-5"
      onSubmit={(e) => {
        e.preventDefault();
        // Enter in the path goes there first; once there, it copies.
        if (typed.trim() && typed.trim() !== place?.path) go(typed.trim());
        else if (ready) send();
      }}
    >
      <div className="flex items-start gap-3">
        <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent">
          <Send size={14} />
        </span>
        <div className="min-w-0">
          <h2 className="text-[15px] font-semibold text-fg">
            Copy {asked.paths.length === 1 ? `“${label}”` : label} to {asked.from.kind === "host" ? "another server" : "a server"}
          </h2>
          <p className="mt-0.5 truncate text-[12px] text-fg-3">
            From {names.target(asked.from)}
            {asked.paths.length > 1 && `: ${asked.paths.slice(0, 4).map((p) => p.split(/[\\/]/).pop()).join(", ")}${asked.paths.length > 4 ? ` and ${asked.paths.length - 4} more` : ""}`}
          </p>
        </div>
      </div>

      {others.length === 0 ? (
        <p className="mt-4 text-[12.5px] text-fg-2">There's no other server to copy to yet. Add one from the sidebar first.</p>
      ) : (
        <>
          <span className="mt-4 block text-[12px] font-medium text-fg-2">To</span>
          <div role="radiogroup" aria-label="Server" className="mt-1.5 flex max-h-[132px] flex-wrap gap-1.5 overflow-y-auto">
            {others.map((h) => (
              <button
                key={h.id}
                type="button"
                role="radio"
                aria-checked={h.id === hostId}
                onClick={() => setHostId(h.id)}
                className={cn(
                  "flex h-8 min-w-0 items-center gap-2 rounded-[9px] px-2.5 text-[12.5px] ring-1 ring-inset transition-colors",
                  h.id === hostId ? "bg-accent/12 text-fg ring-accent/70" : "bg-ink/[0.04] text-fg-2 ring-ink/[0.08] hover:bg-ink/[0.07] hover:text-fg",
                )}
              >
                <HostIcon hostId={h.id} size={14} />
                <span className="max-w-[180px] truncate">{names.host(h)}</span>
              </button>
            ))}
          </div>

          <span className="mt-4 block text-[12px] font-medium text-fg-2">Into this folder</span>
          <div className="mt-1.5 flex gap-1.5">
            <button
              type="button"
              aria-label="Folder above"
              title="Folder above"
              disabled={!place || place.path === "/"}
              onClick={() => place && go(parentOf(place.path))}
              className="flex size-9 shrink-0 items-center justify-center rounded-[9px] bg-ink/[0.05] text-fg-2 ring-1 ring-inset ring-ink/[0.08] hover:text-fg disabled:opacity-40"
            >
              <ArrowUp size={14} />
            </button>
            <div className="relative min-w-0 flex-1">
              <input
                aria-label="Folder on the server"
                value={names.on ? names.text(typed) : typed}
                readOnly={names.on}
                onChange={(e) => setTyped(e.target.value)}
                placeholder="/path/on/the/server"
                spellCheck={false}
                className="h-9 w-full rounded-[9px] bg-ink/[0.05] px-3 pr-8 font-mono text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] focus:ring-2 focus:ring-accent/70"
              />
              {loading && <LoaderCircle size={14} className="absolute top-2.5 right-3 animate-spin text-fg-3" />}
            </div>
          </div>
          <div className="mt-1.5 h-[168px] overflow-y-auto rounded-[9px] bg-ink/[0.025] p-1 ring-1 ring-inset ring-ink/[0.06]">
            {error ? (
              <p className="p-2 text-[12px] leading-snug text-bad">{names.text(error)}</p>
            ) : place && place.dirs.length === 0 && !loading ? (
              <p className="p-2 text-[12px] text-fg-3">No folders inside. The files go right here.</p>
            ) : (
              place?.dirs.map((d) => (
                <button
                  key={d}
                  type="button"
                  onClick={() => go(place.path === "/" ? `/${d}` : `${place.path}/${d}`)}
                  className="flex h-7 w-full items-center gap-2 rounded-[6px] px-2 text-left text-[12.5px] text-fg-2 hover:bg-ink/[0.06] hover:text-fg"
                >
                  <Folder size={13} className="shrink-0 text-accent" />
                  <span className="truncate">{names.text(d)}</span>
                </button>
              ))
            )}
          </div>
        </>
      )}

      <div className="mt-5 flex items-center justify-end gap-2">
        <Button type="button" variant="ghost" onClick={close}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" disabled={!ready}>
          <Send size={13} /> Copy here
        </Button>
      </div>
    </form>
  );
}
