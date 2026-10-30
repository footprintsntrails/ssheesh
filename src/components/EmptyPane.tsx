import { Activity, FolderOpen, Laptop, Plus, SquareTerminal } from "lucide-react";
import { useEffect } from "react";
import { shortcutLabel } from "../lib/keys";
import { hostAddress, useStore, type PaneState } from "../lib/store";
import type { TargetRef } from "../lib/types";
import { Button, IconButton, Kbd, Monogram } from "./ui";

export function EmptyPane({ pane, focused }: { pane: PaneState; focused: boolean }) {
  const hosts = useStore((s) => s.hosts);
  const openIn = useStore((s) => s.openIn);
  const setAddHostOpen = useStore((s) => s.setAddHostOpen);
  const targets: { target: TargetRef; name: string; sub: string }[] = [
    { target: { kind: "local" }, name: "This computer", sub: "local shell" },
    ...hosts.slice(0, 8).map((h) => ({ target: { kind: "host", hostId: h.id } as TargetRef, name: h.name, sub: hostAddress(h) })),
  ];

  // Number keys pick a row while this pane is focused.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || (e.target as HTMLElement)?.closest("input,textarea")) return;
      const n = Number(e.key);
      if (n >= 1 && n <= targets.length) {
        e.preventDefault();
        openIn("terminal", targets[n - 1].target, { from: pane.id });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="absolute inset-0 overflow-y-auto">
      <div className="mx-auto flex min-h-full w-full max-w-[400px] flex-col justify-center px-6 py-10">
        <h2 className="font-serif text-[28px] italic leading-none text-fg">Where to?</h2>
        <p className="mt-2 text-[12.5px] text-fg-3">Terminals reconnect on their own when the network blinks.</p>

        <div className="mt-6 flex flex-col gap-0.5">
          {targets.map(({ target, name, sub }, i) => (
            <div
              key={name + i}
              role="button"
              tabIndex={0}
              onClick={() => openIn("terminal", target, { from: pane.id })}
              onKeyDown={(e) => e.key === "Enter" && openIn("terminal", target, { from: pane.id })}
              className="group flex h-11 items-center gap-3 rounded-[10px] px-2.5 outline-none transition-colors hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06]"
            >
              {target.kind === "local" ? (
                <span className="inline-flex size-[26px] items-center justify-center rounded-[7px] bg-ink/[0.06] text-fg-2 ring-1 ring-inset ring-ink/[0.07]">
                  <Laptop size={14} strokeWidth={1.75} />
                </span>
              ) : (
                <Monogram name={name} health="idle" />
              )}
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium text-fg">{name}</div>
                <div className="truncate font-mono text-[10.5px] text-fg-3">{sub}</div>
              </div>
              <div className="flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                <IconButton
                  label="Files"
                  className="size-6"
                  onClick={(e) => {
                    e.stopPropagation();
                    openIn("files", target, { from: pane.id });
                  }}
                >
                  <FolderOpen size={13} />
                </IconButton>
                <IconButton
                  label="Stats"
                  className="size-6"
                  onClick={(e) => {
                    e.stopPropagation();
                    openIn("stats", target, { from: pane.id });
                  }}
                >
                  <Activity size={13} />
                </IconButton>
                <IconButton label="Terminal" className="size-6">
                  <SquareTerminal size={13} />
                </IconButton>
              </div>
              {i < 9 && <Kbd className="group-hover:hidden">{i + 1}</Kbd>}
            </div>
          ))}
        </div>

        {hosts.length === 0 && (
          <Button variant="primary" className="mt-4 self-start" onClick={() => setAddHostOpen(true)}>
            <Plus size={14} /> Add your first server
          </Button>
        )}

        <div className="mt-8 flex flex-wrap items-center gap-x-4 gap-y-2 text-[11.5px] text-fg-3">
          <span className="inline-flex items-center gap-1.5">
            <Kbd>{shortcutLabel("palette")}</Kbd> search
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Kbd>{shortcutLabel("splitRight")}</Kbd> split
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Kbd>{shortcutLabel("closePane")}</Kbd> close
          </span>
        </div>
      </div>
    </div>
  );
}
