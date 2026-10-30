import { Command } from "cmdk";
import { Activity, CircleAlert, CircleCheck, Columns2, Columns3, Eye, EyeOff, FolderOpen, Info, Laptop, LayoutDashboard, LayoutGrid, Minimize2, PanelLeft, Plus, RefreshCw, Rows2, Rows3, Search, Settings, SquareTerminal, Star, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { folderName, useBookmarks } from "../lib/bookmarks";
import { useSettings } from "../lib/settings";
import { useStore } from "../lib/store";
import { toggleStreamer, useNames } from "../lib/privacy";
import { useUpdates } from "../lib/updates";
import { targetKey, type TargetRef } from "../lib/types";
import { Button, Kbd } from "./ui";
import { useVisibleHosts } from "../lib/labels";

// ---------------------------------------------------------------- command palette

function Item({ onSelect, icon: Icon, label, hint, value }: { onSelect: () => void; icon: typeof Search; label: string; hint?: string; value?: string }) {
  return (
    <Command.Item
      value={value ?? label}
      onSelect={onSelect}
      className="flex h-9 items-center gap-2.5 rounded-[9px] px-2.5 text-[13px] text-fg-2 data-[selected=true]:bg-ink/[0.08] data-[selected=true]:text-fg"
    >
      <Icon size={14.5} strokeWidth={1.8} className="shrink-0 text-fg-3" />
      <span className="flex-1 truncate">{label}</span>
      {hint && <span className="shrink-0 truncate font-mono text-[11px] text-fg-3">{hint}</span>}
    </Command.Item>
  );
}

export function CommandPalette() {
  const open = useStore((s) => s.paletteOpen);
  const setOpen = useStore((s) => s.setPaletteOpen);
  const hosts = useVisibleHosts();
  const focused = useStore((s) => s.panes[s.focused]);
  const openIn = useStore((s) => s.openIn);
  const split = useStore((s) => s.split);
  const closePane = useStore((s) => s.closePane);
  const setAddHostOpen = useStore((s) => s.setAddHostOpen);
  const openSettings = useStore((s) => s.openSettings);
  const openWorkspace = useStore((s) => s.openWorkspace);
  const arrangePanes = useStore((s) => s.arrangePanes);
  const checkUpdates = useUpdates((u) => u.check);
  const pins = useBookmarks((b) => b.pins);
  const names = useNames();
  const compactMode = useSettings((s) => s.compactMode);
  const toggleSidebar = () => {
    const st = useSettings.getState();
    st.update({ sidebarVisible: !st.sidebarVisible });
  };

  const run = (fn: () => void) => () => {
    setOpen(false);
    fn();
  };

  const targets: { target: TargetRef; name: string; hint: string }[] = [
    { target: { kind: "local" }, name: "This computer", hint: "local" },
    ...hosts.map((h) => ({ target: { kind: "host", hostId: h.id } as TargetRef, name: names.host(h), hint: names.address(h) })),
  ];

  return (
    <Command.Dialog
      open={open}
      onOpenChange={setOpen}
      label="Jump to"
      loop
      filter={(value, search) => {
        const v = value.toLowerCase();
        return search.toLowerCase().split(/\s+/).every((w) => v.includes(w)) ? 1 : 0;
      }}
      overlayClassName="fixed inset-0 z-40 bg-black/25 animate-fade-in"
      contentClassName="fixed left-1/2 top-[14vh] z-50 w-[580px] max-w-[calc(100vw-32px)] -translate-x-1/2 outline-none"
      className="glass animate-pop-in overflow-hidden rounded-[16px]"
    >
      <div className="flex items-center gap-2.5 border-b border-ink/[0.07] px-4">
        <Search size={16} className="shrink-0 text-fg-3" />
        <Command.Input
          placeholder="Search servers and actions…"
          className="h-[50px] flex-1 bg-transparent text-[15px] text-fg outline-none placeholder:text-fg-3"
        />
        <Kbd>esc</Kbd>
      </div>
      <Command.List className="max-h-[min(420px,60vh)] overflow-y-auto p-1.5 [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2.5 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:text-fg-3">
        <Command.Empty className="px-3 py-8 text-center text-[13px] text-fg-3">Nothing found.</Command.Empty>

        <Command.Group heading="Connect">
          {targets.map(({ target, name, hint }) => (
            <Item
              key={`t-${name}`}
              value={`connect ${name} ${hint}`}
              icon={target.kind === "local" ? Laptop : SquareTerminal}
              label={name}
              hint={hint}
              onSelect={run(() => openIn("terminal", target))}
            />
          ))}
        </Command.Group>

        <Command.Group heading="Bookmarks">
          {targets.flatMap(({ target, name }) =>
            (pins[targetKey(target)] ?? []).map((path) => (
              <Item
                key={`b-${name}-${path}`}
                value={`bookmark folder ${folderName(path)} ${path} ${name}`}
                icon={Star}
                label={`${folderName(path)} on ${name}`}
                hint={path}
                onSelect={run(() => openIn("files", target, { path, split: true }))}
              />
            )),
          )}
        </Command.Group>

        <Command.Group heading="Workspaces">
          {targets.map(({ target, name }) => (
            <Item
              key={`w-${name}`}
              value={`workspace layout ${name}`}
              icon={LayoutDashboard}
              label={`Workspace for ${name}`}
              hint="terminal · files · stats"
              onSelect={run(() => openWorkspace(target))}
            />
          ))}
        </Command.Group>

        <Command.Group heading="Files and stats">
          {targets.map(({ target, name }) => (
            <Item key={`f-${name}`} value={`files browse ${name}`} icon={FolderOpen} label={`Browse files on ${name}`} onSelect={run(() => openIn("files", target))} />
          ))}
          {targets.map(({ target, name }) => (
            <Item key={`s-${name}`} value={`stats metrics cpu memory traffic ${name}`} icon={Activity} label={`Stats for ${name}`} onSelect={run(() => openIn("stats", target))} />
          ))}
        </Command.Group>

        <Command.Group heading="Workspace">
          {focused && (
            <>
              <Item icon={Columns2} label="Split right" hint={shortcutLabel("splitRight")} onSelect={run(() => split(focused.id, "row"))} />
              <Item icon={Rows2} label="Split down" hint={shortcutLabel("splitDown")} onSelect={run(() => split(focused.id, "col"))} />
              <Item icon={X} label="Close pane" hint={shortcutLabel("closePane")} onSelect={run(() => closePane(focused.id))} />
            </>
          )}
          <Item icon={LayoutDashboard} value="arrange main stack" label="Arrange: main + stack" onSelect={run(() => arrangePanes("main"))} />
          <Item icon={Columns3} value="arrange columns" label="Arrange: columns" onSelect={run(() => arrangePanes("columns"))} />
          <Item icon={Rows3} value="arrange rows" label="Arrange: rows" onSelect={run(() => arrangePanes("rows"))} />
          <Item icon={LayoutGrid} value="arrange grid" label="Arrange: grid" onSelect={run(() => arrangePanes("grid"))} />
          <Item icon={PanelLeft} label="Toggle sidebar" hint={shortcutLabel("sidebar")} onSelect={run(toggleSidebar)} />
          <Item icon={Plus} label="Add server…" hint={shortcutLabel("addHost")} onSelect={run(() => setAddHostOpen(true))} />
        </Command.Group>

        <Command.Group heading="App">
          <Item icon={Settings} label="Settings" hint={shortcutLabel("settings")} onSelect={run(() => openSettings())} />
          <Item icon={Settings} value="theme appearance colors accent" label="Change theme and colors" onSelect={run(() => openSettings("appearance"))} />
          <Item icon={SquareTerminal} value="terminal theme font size cursor" label="Terminal look" onSelect={run(() => openSettings("terminal"))} />
          <Item
            icon={Minimize2}
            value="compact mode focus hide sidebar toolbar zen"
            label={compactMode ? "Turn off compact mode" : "Turn on compact mode"}
            hint={shortcutLabel("compact")}
            onSelect={run(() => useSettings.getState().update({ compactMode: !compactMode }))}
          />
          <Item
            icon={names.on ? Eye : EyeOff}
            value="streamer mode hide names addresses privacy screen share recording"
            label={names.on ? "Turn off streamer mode" : "Turn on streamer mode"}
            hint={shortcutLabel("streamer")}
            onSelect={run(toggleStreamer)}
          />
          <Item
            icon={RefreshCw}
            label="Check for updates"
            onSelect={run(() => {
              openSettings("updates");
              void checkUpdates();
            })}
          />
        </Command.Group>
      </Command.List>
    </Command.Dialog>
  );
}

// ---------------------------------------------------------------- dialogs

export function Modal({ open, onClose, children, width = 420 }: { open: boolean; onClose: () => void; children: React.ReactNode; width?: number }) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/30 px-4 pt-[16vh] animate-fade-in" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-modal
        className="glass max-h-[calc(84vh-16px)] animate-pop-in overflow-y-auto rounded-[16px]"
        style={{ width, maxWidth: "100%" }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}



export function ConfirmDialog() {
  const req = useStore((s) => s.confirmRequest);
  const confirmRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (req) setTimeout(() => confirmRef.current?.focus(), 0);
  }, [req]);
  return (
    <Modal open={!!req} onClose={() => req?.resolve(false)} width={380}>
      {req && (
        <div className="p-5">
          <h2 className="text-[14.5px] font-semibold text-fg">{req.title}</h2>
          {req.body && <p className="mt-1.5 text-[12.5px] leading-relaxed text-fg-2">{req.body}</p>}
          <div className="mt-5 flex justify-end gap-2">
            <Button variant="ghost" onClick={() => req.resolve(false)}>
              Cancel
            </Button>
            <Button ref={confirmRef} variant={req.danger ? "danger" : "primary"} onClick={() => req.resolve(true)}>
              {req.confirmLabel}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}

// ---------------------------------------------------------------- toasts

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-[340px] max-w-[calc(100vw-32px)] flex-col gap-2">
      {toasts.map((t) => {
        const Icon = t.tone === "success" ? CircleCheck : t.tone === "error" ? CircleAlert : Info;
        return (
          <div key={t.id} role="status" className="glass pointer-events-auto flex animate-slide-up items-start gap-2.5 rounded-[13px] px-3.5 py-3">
            <Icon size={15} className={cn("mt-px shrink-0", t.tone === "success" && "text-ok", t.tone === "error" && "text-bad", t.tone === "info" && "text-accent")} />
            <div className="min-w-0 flex-1">
              <p className="text-[12.5px] font-medium text-fg">{t.title}</p>
              {t.body && <p className="selectable mt-0.5 break-words text-[11.5px] leading-snug text-fg-3">{t.body}</p>}
            </div>
            <button onClick={() => dismiss(t.id)} className="-mr-1 shrink-0 rounded p-0.5 text-fg-3 hover:text-fg" aria-label="Dismiss">
              <X size={13} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
