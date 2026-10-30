import { Activity, Box, ChevronRight, Copy, FolderOpen, Layers, LayoutDashboard, Laptop, Play, Plus, RotateCw, ScrollText, Search, Server, Square, SquareTerminal, Trash } from "lucide-react";
import { useCallback, useEffect, useState, type MouseEvent } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { isMac, shortcutLabel } from "../lib/keys";
import { useSettings } from "../lib/settings";
import { hostAddress, useStore } from "../lib/store";
import { sameTarget, targetKey, toOpError, type Container, type ContainerList, type Host, type OpError, type TargetRef } from "../lib/types";
import { ContextMenu, type Menu, type MenuItem } from "./ContextMenu";
import { Face, healthOf, IconButton, ResizeHandle, Spinner, type Health, type Mood } from "./ui";

const RANK: Record<Health, number> = { idle: 0, busy: 1, live: 2, warn: 3, bad: 4 };

/** The most urgent status among this machine's open terminals. */
function useHealth(target: TargetRef): Health {
  return useStore((s) => {
    let best: Health = "idle";
    for (const p of Object.values(s.panes)) {
      if (p.kind !== "terminal" || !sameTarget(p.target, target)) continue;
      const h = healthOf(s.sessions[p.id]);
      if (RANK[h] > RANK[best]) best = h;
    }
    return best;
  });
}

/** Status by the logo's own faces: >_ connected, >_< reconnecting, x_x failed. */
function StatusMark({ health }: { health: Health }) {
  if (health === "idle") return null;
  const mood: Mood = health === "live" ? "ok" : health === "warn" ? "lost" : health === "bad" ? "dead" : "busy";
  return <Face mood={mood} className="h-3 w-[22px]" />;
}

const POLL_MS = 10000;

function useContainers(target: TargetRef) {
  const [list, setList] = useState<ContainerList | null>(null);
  const [error, setError] = useState<OpError | null>(null);
  const key = targetKey(target);
  const refresh = useCallback(() => {
    backend
      .listContainers(target)
      .then((l) => {
        setList(l);
        setError(null);
      })
      .catch((e) => setError(toOpError(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  useEffect(() => {
    refresh();
    const t = setInterval(refresh, POLL_MS);
    return () => clearInterval(t);
  }, [refresh]);
  return { list, error, refresh };
}

function TreeNote({ children }: { children: React.ReactNode }) {
  return <p className="flex items-start gap-1.5 py-1 pr-2 pl-9 text-[11.5px] leading-snug text-fg-3">{children}</p>;
}

function ContainerRow({ target, c, indent, onChanged }: { target: TargetRef; c: Container; indent: boolean; onChanged: () => void }) {
  const openIn = useStore((s) => s.openIn);
  const toast = useStore((s) => s.toast);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [busy, setBusy] = useState(false);

  const open = (run: "shell" | "logs") =>
    openIn("terminal", target, { container: { engine: c.engine, id: c.id, name: c.name, run } });

  const act = async (action: "start" | "stop" | "restart") => {
    setBusy(true);
    try {
      await backend.containerAction(target, c.engine, c.id, action);
      toast({ tone: "success", title: `${action === "stop" ? "Stopped" : action === "start" ? "Started" : "Restarted"} ${c.name}` });
    } catch (e) {
      toast({ tone: "error", title: `Couldn't ${action} ${c.name}`, body: toOpError(e).message });
    }
    setBusy(false);
    onChanged();
  };

  const items: (MenuItem | "sep")[] = [
    { label: "Open shell", icon: SquareTerminal, run: () => open("shell"), disabled: !c.running },
    { label: "Follow logs", icon: ScrollText, run: () => open("logs") },
    "sep",
    ...(c.running
      ? [
          { label: "Restart", icon: RotateCw, run: () => act("restart") },
          { label: "Stop", icon: Square, run: () => act("stop") },
        ]
      : [{ label: "Start", icon: Play, run: () => act("start") }]),
    "sep",
    { label: "Copy name", icon: Copy, run: () => void navigator.clipboard.writeText(c.name) },
  ];

  return (
    <>
      <button
        type="button"
        title={`${c.image}\n${c.status}${c.ports ? `\n${c.ports}` : ""}`}
        onClick={() =>
          c.running ? open("shell") : toast({ tone: "info", title: `${c.name} is stopped`, body: "Right-click it to start it or read its logs." })
        }
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
        className={cn(
          "flex h-7 w-full items-center gap-2 rounded-[6px] pr-1.5 text-left text-[12.5px] outline-none hover:bg-ink/[0.05] focus-visible:bg-ink/[0.07]",
          indent ? "pl-12" : "pl-9",
        )}
      >
        <Box size={13.5} strokeWidth={1.8} className={cn("shrink-0", c.running ? "text-accent" : "text-fg-3")} />
        <span className={cn("min-w-0 flex-1 truncate", c.running ? "text-fg" : "text-fg-3")}>{c.name}</span>
        {busy ? <Spinner className="size-3 text-fg-3" /> : !c.running && <span className="text-[11px] text-fg-3">stopped</span>}
      </button>
      {menu && <ContextMenu menu={menu} items={items} onClose={() => setMenu(null)} />}
    </>
  );
}

/** XPipe-style tree: the containers living on a machine, grouped by Compose project. */
function ContainerTree({ target }: { target: TargetRef }) {
  const { list, error, refresh } = useContainers(target);
  if (error)
    return (
      <TreeNote>
        {error.code === "auth" || error.code === "hostKey" ? "Open a terminal to sign in, then containers show up here." : error.message}
      </TreeNote>
    );
  if (!list)
    return (
      <TreeNote>
        <Spinner className="mt-px size-3" /> Looking for containers
      </TreeNote>
    );
  if (list.engines.length === 0 && list.problems.length === 0) return <TreeNote>No Docker or Podman here.</TreeNote>;

  const projects = [...new Set(list.containers.map((c) => c.project).filter((p): p is string => !!p))];
  const loose = list.containers.filter((c) => !c.project);
  return (
    <div className="flex flex-col gap-px pb-1">
      {list.problems.map((p) => (
        <TreeNote key={p}>{p}</TreeNote>
      ))}
      {projects.map((p) => (
        <div key={p} className="flex flex-col gap-px">
          <div className="flex h-6 items-center gap-1.5 pl-9 text-[11.5px] text-fg-3" title="Docker Compose project">
            <Layers size={12} /> {p}
          </div>
          {list.containers
            .filter((c) => c.project === p)
            .map((c) => (
              <ContainerRow key={c.id} target={target} c={c} indent onChanged={refresh} />
            ))}
        </div>
      ))}
      {loose.map((c) => (
        <ContainerRow key={c.id} target={target} c={c} indent={false} onChanged={refresh} />
      ))}
      {list.engines.length > 0 && list.containers.length === 0 && <TreeNote>No containers yet.</TreeNote>}
    </div>
  );
}

function Row({ target, label, address, host }: { target: TargetRef; label: string; address?: string; host?: Host }) {
  const openIn = useStore((s) => s.openIn);
  const openWorkspace = useStore((s) => s.openWorkspace);
  const loadHosts = useStore((s) => s.loadHosts);
  const confirm = useStore((s) => s.confirm);
  const toast = useStore((s) => s.toast);
  const active = useStore((s) => sameTarget(s.panes[s.focused]?.target, target));
  const health = useHealth(target);
  const [menu, setMenu] = useState<Menu | null>(null);
  const Icon = target.kind === "local" ? Laptop : Server;

  const items: (MenuItem | "sep")[] = [
    { label: "New terminal", icon: SquareTerminal, run: () => openIn("terminal", target) },
    { label: "Browse files", icon: FolderOpen, run: () => openIn("files", target) },
    { label: "Show stats", icon: Activity, run: () => openIn("stats", target) },
    { label: "Open workspace", icon: LayoutDashboard, run: () => openWorkspace(target) },
    ...(address
      ? (["sep", { label: "Copy address", icon: Copy, run: () => void navigator.clipboard.writeText(address).then(() => toast({ tone: "info", title: "Copied", body: address })) }] as (MenuItem | "sep")[])
      : []),
    ...(host?.source === "saved"
      ? ([
          "sep",
          {
            label: "Remove from list",
            icon: Trash,
            danger: true,
            run: async () => {
              if (!(await confirm({ title: `Remove ${host.name}?`, body: "Only removes it from SSHeesh. Nothing changes on the server.", confirmLabel: "Remove", danger: true }))) return;
              await backend.removeHost(host.id);
              void loadHosts();
            },
          },
        ] as (MenuItem | "sep")[])
      : []),
  ];

  const key = targetKey(target);
  const expanded = useSettings((st) => st.expanded.includes(key));
  const toggle = () => {
    const st = useSettings.getState();
    st.update({ expanded: expanded ? st.expanded.filter((k) => k !== key) : [...st.expanded, key] });
  };

  return (
    <>
      <div className="group/row relative flex items-center">
      <button
        type="button"
        aria-label={expanded ? `Hide containers on ${label}` : `Show containers on ${label}`}
        aria-expanded={expanded}
        onClick={toggle}
        className="absolute left-0 z-10 flex h-7 w-5 items-center justify-center rounded text-fg-3 hover:text-fg"
      >
        <ChevronRight size={12} strokeWidth={2.4} className={cn("transition-transform", expanded && "rotate-90")} />
      </button>
      <button
        type="button"
        title={address}
        aria-current={active || undefined}
        onClick={() => openIn("terminal", target)}
        onContextMenu={(e: MouseEvent) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
        className={cn(
          "flex h-7 w-full items-center gap-2 rounded-[6px] pr-1.5 pl-5 text-left text-[13px] outline-none",
          active ? "bg-ink/[0.1] text-fg" : "text-fg hover:bg-ink/[0.05] focus-visible:bg-ink/[0.07]",
        )}
      >
        <Icon size={15} strokeWidth={1.8} className="shrink-0 text-accent" />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        <StatusMark health={health} />
      </button>
      </div>
      {expanded && <ContainerTree target={target} />}
      {menu && <ContextMenu menu={menu} items={items} onClose={() => setMenu(null)} />}
    </>
  );
}

export function Sidebar() {
  const hosts = useStore((s) => s.hosts);
  const loaded = useStore((s) => s.hostsLoaded);
  const setPaletteOpen = useStore((s) => s.setPaletteOpen);
  const setAddHostOpen = useStore((s) => s.setAddHostOpen);
  const side = useSettings((s) => s.sidebarSide);
  const width = useSettings((s) => s.sidebarWidth);
  const update = useSettings((s) => s.update);

  return (
    <aside
      aria-label="Servers"
      style={{ width }}
      className={cn("relative flex shrink-0 flex-col border-line", side === "left" ? "border-r" : "border-l")}
    >
      <ResizeHandle
        edge={side === "left" ? "right" : "left"}
        label="Resize sidebar"
        onResize={(dx) => {
          const w = useSettings.getState().sidebarWidth + (side === "left" ? dx : -dx);
          update({ sidebarWidth: Math.round(Math.min(420, Math.max(180, w))) });
        }}
        onReset={() => update({ sidebarWidth: 232 })}
      />
      {/* Title bar space; on macOS the window buttons sit here. */}
      <div data-tauri-drag-region className={cn("h-[46px] shrink-0", isMac && side === "left" && "pl-[80px]")} />

      <div className="px-2.5 pb-3">
        <button
          type="button"
          onClick={() => setPaletteOpen(true)}
          className="flex h-7 w-full items-center gap-1.5 rounded-[7px] bg-ink/[0.07] px-2 text-[13px] text-fg-3 hover:bg-ink/[0.09]"
        >
          <Search size={13} strokeWidth={2} />
          <span className="flex-1 text-left">Search</span>
          <span className="text-[11.5px]">{shortcutLabel("palette")}</span>
        </button>
      </div>

      <nav className="flex-1 overflow-y-auto px-2.5 pb-3">
        <div className="flex h-6 items-center justify-between pr-0.5 pl-2">
          <h2 className="text-[11px] font-semibold text-fg-2">Servers</h2>
          <IconButton label="Add server" onClick={() => setAddHostOpen(true)} className="size-5 rounded-[5px]">
            <Plus size={14} strokeWidth={2} />
          </IconButton>
        </div>
        <div className="mt-0.5 flex flex-col gap-px">
          <Row target={{ kind: "local" }} label="This computer" />
          {hosts.map((h) => (
            <Row key={h.id} target={{ kind: "host", hostId: h.id }} label={h.name} address={hostAddress(h)} host={h} />
          ))}
        </div>
        {loaded && hosts.length === 0 && (
          <p className="mt-3 px-2 text-[12px] leading-snug text-fg-3">
            Add a server with the plus button. Hosts in ~/.ssh/config appear here on their own.
          </p>
        )}
      </nav>
    </aside>
  );
}
