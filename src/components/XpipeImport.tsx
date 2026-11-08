// Servers saved in XPipe, brought over in one go: SSHeesh reads XPipe's vault
// on this computer and adds the ones you tick. XPipe itself isn't changed.

import { Download } from "lucide-react";
import { useEffect, useState } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { hostAddress, useStore } from "../lib/store";
import { toOpError, type XpipeScan } from "../lib/types";
import { Modal } from "./Overlays";
import { useNames } from "../lib/privacy";
import { Button } from "./ui";

export const useXpipeImport = create<{ open: boolean; show(): void; close(): void }>((set) => ({
  open: false,
  show: () => set({ open: true }),
  close: () => set({ open: false }),
}));

/** Whether XPipe's vault is on this computer, asked once per Add server dialog. */
export function useHasXpipe(asking: boolean): boolean {
  const [has, setHas] = useState(false);
  useEffect(() => {
    if (!asking) return;
    let alive = true;
    backend.xpipeServers().then(
      (s) => alive && setHas(s.found && s.servers.length + s.locked + s.throughJump > 0),
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [asking]);
  return has;
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

export function XpipeImportDialog() {
  const open = useXpipeImport((s) => s.open);
  const close = useXpipeImport((s) => s.close);
  const hosts = useStore((s) => s.hosts);
  const [scan, setScan] = useState<XpipeScan | null>(null);
  const [keep, setKeep] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const names = useNames();

  // What's in SSHeesh already isn't added twice.
  const have = new Set(hosts.map(hostAddress));
  const fresh = scan?.servers.filter((s) => !have.has(s.address)) ?? [];

  useEffect(() => {
    if (!open) return;
    setScan(null);
    backend.xpipeServers().then(
      (s) => {
        setScan(s);
        const known = new Set(useStore.getState().hosts.map(hostAddress));
        setKeep(s.servers.filter((x) => !known.has(x.address)).map((x) => x.address));
      },
      (e) => {
        useStore.getState().toast({ tone: "error", title: "Couldn't read XPipe's servers", body: toOpError(e).message });
        close();
      },
    );
  }, [open, close]);

  const toggle = (a: string) => setKeep((k) => (k.includes(a) ? k.filter((x) => x !== a) : [...k, a]));

  const add = async () => {
    const chosen = fresh.filter((s) => keep.includes(s.address));
    setBusy(true);
    let added = 0;
    const failed: string[] = [];
    for (const s of chosen) {
      try {
        await backend.addHost({ name: s.name, address: s.address, identityFile: s.identityFile ?? undefined });
        added++;
      } catch {
        failed.push(s.name);
      }
    }
    await useStore.getState().loadHosts();
    setBusy(false);
    close();
    const withPassword = chosen.filter((s) => s.hadPassword).length;
    useStore.getState().toast({
      tone: failed.length ? "info" : "success",
      title: `Added ${plural(added, "server")} from XPipe`,
      body: [
        failed.length ? `Couldn't add ${failed.join(", ")}.` : "",
        withPassword ? `${plural(withPassword, "server")} will ask for a password the first time: XPipe keeps passwords locked.` : "",
      ]
        .filter(Boolean)
        .join(" ") || undefined,
    });
  };

  const notes = scan
    ? [
        scan.servers.length > fresh.length ? `${plural(scan.servers.length - fresh.length, "server is", "servers are")} in SSHeesh already.` : "",
        scan.throughJump ? `${plural(scan.throughJump, "server goes", "servers go")} through another server, which SSHeesh can't do yet, so ${scan.throughJump === 1 ? "it stays" : "they stay"} out.` : "",
        scan.locked ? `${plural(scan.locked, "entry is", "entries are")} encrypted, and only XPipe can open ${scan.locked === 1 ? "it" : "them"}.` : "",
        "Passwords stay in XPipe: a server that needs one asks the first time. Servers XPipe got from ~/.ssh/config are in SSHeesh already.",
      ].filter(Boolean)
    : [];

  return (
    <Modal open={open} onClose={() => !busy && close()} width={480}>
      <div className="p-5">
        <div className="flex items-start gap-3">
          <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent">
            <Download size={15} />
          </span>
          <div>
            <h2 className="text-[15px] font-semibold text-fg">{!scan ? "Reading XPipe's servers…" : fresh.length ? `Found ${plural(fresh.length, "new server")} in XPipe` : "Your XPipe servers are here already"}</h2>
            <p className="mt-1 text-[12.5px] leading-relaxed text-fg-3">{scan && !fresh.length ? "Nothing new to add." : "Pick the ones to add. XPipe isn't changed."}</p>
          </div>
        </div>
        {fresh.length > 0 && (
          <>
            <div className="mt-4 flex max-h-[300px] flex-col gap-0.5 overflow-y-auto rounded-[10px] p-1 ring-1 ring-inset ring-ink/[0.08]">
              {fresh.map((s) => (
                <label key={s.address} className="flex h-10 cursor-pointer items-center gap-3 rounded-[7px] px-2.5 hover:bg-ink/[0.04]">
                  <input type="checkbox" checked={keep.includes(s.address)} onChange={() => toggle(s.address)} className="accent-[var(--ui-accent)]" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] text-fg">{names.text(s.name)}</span>
                    {!names.on && s.address !== s.name && <span className="block truncate font-mono text-[11px] text-fg-3">{s.address}</span>}
                  </span>
                </label>
              ))}
            </div>
            <div className="mt-2 flex gap-3 text-[11.5px]">
              <button type="button" className="text-fg-3 hover:text-fg-2" onClick={() => setKeep(fresh.map((s) => s.address))}>
                Select all
              </button>
              <button type="button" className="text-fg-3 hover:text-fg-2" onClick={() => setKeep([])}>
                Select none
              </button>
            </div>
          </>
        )}
        {notes.length > 0 && (
          <ul className="mt-4 flex list-disc flex-col gap-1 pl-4 text-[11.5px] leading-relaxed text-fg-3">
            {notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        )}
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" onClick={close} disabled={busy}>
            {fresh.length ? "Cancel" : "Close"}
          </Button>
          {fresh.length > 0 && (
            <Button variant="primary" onClick={() => void add()} disabled={busy || !keep.length}>
              {busy ? "Adding…" : `Add ${plural(keep.length, "server")}`}
            </Button>
          )}
        </div>
      </div>
    </Modal>
  );
}
