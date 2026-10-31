// Servers you already have in ~/.ssh/config: SSHeesh asks which ones you
// want in the list instead of adding all of them for you.

import { FileText } from "lucide-react";
import { useEffect, useState } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { hostAddress } from "../lib/store";
import { useLabels, useUndecidedConfig } from "../lib/labels";
import { useStore } from "../lib/store";
import { useSettings } from "../lib/settings";
import type { Host } from "../lib/types";
import { Modal } from "./Overlays";
import { useNames } from "../lib/privacy";
import { Button } from "./ui";

/** "new": only servers you haven't decided about. "all": change your mind about any. */
export const useConfigImport = create<{ open: "new" | "all" | null; show(mode: "new" | "all"): void; close(): void }>((set) => ({
  open: null,
  show: (mode) => set({ open: mode }),
  close: () => set({ open: null }),
}));

export function ConfigImportDialog() {
  const mode = useConfigImport((s) => s.open);
  const close = useConfigImport((s) => s.close);
  const hostsLoaded = useStore((s) => s.hostsLoaded);
  const allHosts = useStore((s) => s.hosts);
  const undecided = useUndecidedConfig();
  const seen = useLabels((s) => s.configSeen);
  const hidden = useLabels((s) => s.hidden);
  const [keep, setKeep] = useState<string[]>([]);
  const names = useNames();

  const list: Host[] = mode === "all" ? allHosts.filter((h) => h.source === "sshConfig") : undecided;

  // The first launch asks inside the welcome. The demo takes every config server
  // unless it's showing the first launch.
  useEffect(() => {
    if (!hostsLoaded || seen.length || !undecided.length) return;
    if (backend.isDemo && !new URLSearchParams(location.search).has("firstrun")) {
      useLabels.getState().chooseConfig(undecided.map((h) => h.id), undecided.map((h) => h.id));
    }
  }, [hostsLoaded, seen.length, undecided]);

  useEffect(() => {
    if (!mode) return;
    setKeep(mode === "all" ? list.filter((h) => seen.includes(h.id) && !hidden.includes(h.id)).map((h) => h.id) : list.map((h) => h.id));
    // Only when the dialog opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  const finish = (chosen: string[]) => {
    useLabels.getState().chooseConfig(
      list.map((h) => h.id),
      chosen,
    );
    close();
  };
  const toggle = (id: string) => setKeep((k) => (k.includes(id) ? k.filter((x) => x !== id) : [...k, id]));

  return (
    <Modal open={!!mode && list.length > 0} onClose={() => finish(mode === "all" ? keep : [])} width={460}>
      <div className="p-5">
        <div className="flex items-start gap-3">
          <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent">
            <FileText size={15} />
          </span>
          <div>
            <h2 className="text-[15px] font-semibold text-fg">
              {mode === "all" ? "Servers from ~/.ssh/config" : `Found ${list.length} server${list.length === 1 ? "" : "s"} in ~/.ssh/config`}
            </h2>
            <p className="mt-1 text-[12.5px] leading-relaxed text-fg-3">
              {mode === "all"
                ? "Choose which ones show up in SSHeesh. Your config file isn't changed."
                : "Want them in SSHeesh too? Pick the ones you use. Your config file isn't changed, and you can change this later in Settings → Layout."}
            </p>
          </div>
        </div>
        <div className="mt-4 flex max-h-[300px] flex-col gap-0.5 overflow-y-auto rounded-[10px] p-1 ring-1 ring-inset ring-ink/[0.08]">
          {list.map((h) => (
            <label key={h.id} className="flex h-10 cursor-pointer items-center gap-3 rounded-[7px] px-2.5 hover:bg-ink/[0.04]">
              <input type="checkbox" checked={keep.includes(h.id)} onChange={() => toggle(h.id)} className="accent-[var(--ui-accent)]" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] text-fg">{names.host(h)}</span>
                {!names.on && hostAddress(h) !== h.name && <span className="block truncate font-mono text-[11px] text-fg-3">{hostAddress(h)}</span>}
              </span>
            </label>
          ))}
        </div>
        <div className="mt-2 flex gap-3 text-[11.5px]">
          <button type="button" className="text-fg-3 hover:text-fg-2" onClick={() => setKeep(list.map((h) => h.id))}>
            Select all
          </button>
          <button type="button" className="text-fg-3 hover:text-fg-2" onClick={() => setKeep([])}>
            Select none
          </button>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          {mode !== "all" && (
            <Button variant="ghost" onClick={() => finish([])}>
              Skip
            </Button>
          )}
          <Button variant="primary" onClick={() => finish(keep)}>
            {mode === "all" ? "Done" : keep.length ? `Add ${keep.length} server${keep.length === 1 ? "" : "s"}` : "Add none"}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** After the first launch, new servers in ~/.ssh/config get a quiet note, not a dialog. */
export function ConfigNotice() {
  const undecided = useUndecidedConfig();
  const onboarded = useSettings((s) => s.onboarded);
  if (!undecided.length || !onboarded) return null;
  return (
    <button
      type="button"
      onClick={() => useConfigImport.getState().show("new")}
      className="mx-2.5 mb-2 flex items-center gap-2 rounded-[8px] bg-accent/10 px-2.5 py-1.5 text-left text-[12px] text-fg-2 hover:bg-accent/15"
    >
      <FileText size={13} className="shrink-0 text-accent" />
      <span className="min-w-0 flex-1">
        {undecided.length} new in ~/.ssh/config · <span className="text-accent">Review</span>
      </span>
    </button>
  );
}
