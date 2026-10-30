// Files dropped on a server's terminal: ask where they should go, starting
// with the folder that terminal is in.

import { FileUp, LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { useNames } from "../lib/privacy";
import type { TargetRef } from "../lib/types";
import { uploadTo } from "../lib/upload";
import { Modal } from "./Overlays";
import { Button } from "./ui";

interface Pending {
  target: TargetRef;
  paths: string[];
  /** The terminal's session, to find the folder it's in. */
  sessionKey?: string;
}

export const useUploadAsk = create<{ pending: Pending | null; ask(p: Pending): void; close(): void }>((set) => ({
  pending: null,
  ask: (pending) => set({ pending }),
  close: () => set({ pending: null }),
}));

/** The folder a server terminal is in, or the home folder when it can't tell. */
async function startFolder(target: TargetRef, sessionKey?: string): Promise<string> {
  if (target.kind === "host" && sessionKey) {
    const cwd = await backend.sessionCwd(target, sessionKey).catch(() => "");
    if (cwd) return cwd;
  }
  return (await backend.listDir(target, "~").catch(() => null))?.path ?? "~";
}

export function UploadDialog() {
  const pending = useUploadAsk((s) => s.pending);
  const close = useUploadAsk((s) => s.close);
  const names = useNames();
  const [dir, setDir] = useState("");
  const [finding, setFinding] = useState(false);

  useEffect(() => {
    if (!pending) return;
    setDir("");
    setFinding(true);
    let live = true;
    void startFolder(pending.target, pending.sessionKey).then((d) => {
      if (!live) return;
      setDir(d);
      setFinding(false);
    });
    return () => {
      live = false;
    };
  }, [pending]);

  if (!pending) return null;
  const files = pending.paths.map((p) => p.split(/[\\/]/).pop() ?? p);
  // Progress shows on a card in the corner, so the dialog can go right away.
  const send = () => {
    if (!dir.trim()) return;
    close();
    void uploadTo(pending.target, dir.trim(), pending.paths);
  };

  return (
    <Modal open onClose={close} width={460}>
      <form
        className="p-5"
        onSubmit={(e) => {
          e.preventDefault();
          send();
        }}
      >
        <div className="flex items-start gap-3">
          <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent">
            <FileUp size={15} />
          </span>
          <div className="min-w-0">
            <h2 className="text-[15px] font-semibold text-fg">
              Upload {files.length === 1 ? `“${files[0]}”` : `${files.length} items`} to {names.target(pending.target)}?
            </h2>
            {files.length > 1 && (
              <p className="mt-1 truncate text-[12px] text-fg-3">
                {files.slice(0, 4).join(", ")}
                {files.length > 4 && ` and ${files.length - 4} more`}
              </p>
            )}
          </div>
        </div>
        <label className="mt-4 block">
          <span className="text-[12px] font-medium text-fg-2">Into this folder</span>
          <div className="relative mt-1.5">
            <input
              autoFocus
              value={dir}
              disabled={finding}
              onChange={(e) => setDir(e.target.value)}
              placeholder={finding ? "Finding the terminal's folder…" : "/path/on/the/server"}
              spellCheck={false}
              className="h-9 w-full rounded-[9px] bg-ink/[0.05] px-3 font-mono text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] focus:ring-2 focus:ring-accent/70 disabled:opacity-60"
            />
            {finding && <LoaderCircle size={14} className="absolute top-2.5 right-3 animate-spin text-fg-3" />}
          </div>
          <span className="mt-1 block text-[11.5px] text-fg-3">The folder this terminal is in. Change it if you like.</span>
        </label>
        <div className="mt-5 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={finding || !dir.trim()}>
            <FileUp size={13} /> Upload
          </Button>
        </div>
      </form>
    </Modal>
  );
}
