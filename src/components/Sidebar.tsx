import { Box, ChevronRight, Circle, Copy, EyeOff, House, KeyRound, Palette, Layers, LayoutDashboard, Play, Plus, RotateCw, ScrollText, Search, Square, SquareTerminal, Trash } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type MouseEvent } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { useSettings } from "../lib/settings";
import { currentTarget, hostAddress, machineSummary, showingHome, useMachine, useStore } from "../lib/store";
import { useNames } from "../lib/privacy";
import { ENV_LABELS, ENV_ORDER, useEnv, useLabels, useVisibleHosts } from "../lib/labels";
import { sameTarget, targetKey, toOpError, type Container, type ContainerList, type Host, type OpError, type TargetRef } from "../lib/types";
import { ContextMenu, type Menu, type MenuItem } from "./ContextMenu";
import { HostIcon } from "./HostIcon";
import { LookPopover } from "./LookPicker";
import { ConfigNotice } from "./ConfigImport";
import { LoadLine, RailCard, RailLoad } from "./ServerLoad";
import { AppLogo, EnvChip, Face, healthOf, IconButton, ResizeHandle, Spinner, type Health, type Mood } from "./ui";
import { SidebarTabs } from "./Tabs";
import { newTab } from "../lib/tabs";
import { toolItems } from "./Tabs";
import { openInNewWindow } from "../lib/windows";
import { useCustomTitleBar, WindowControls } from "./WindowControls";

const RANK: Record<Health, number> = { idle: 0, busy: 1, live: 2, warn: 3, bad: 4 };

/** The most urgent status among this machine's open terminals. */
function useHealth(target: TargetRef): Health {
  return useStore((s) => {
    let best: Health = "idle";
    for (const state of machineSummary(s, target).sessions) {
      const h = healthOf(state);
      if (RANK[h] > RANK[best]) best = h;
    }
    return best;
  });
}

/** Status by the logo's own faces: >_< reconnecting, x_x failed, … connecting. */
function StatusMark({ health }: { health: Health }) {
  // Connected is the normal case: only trouble shows.
  if (health === "idle" || health === "live") return null;
  const mood: Mood = health === "warn" ? "lost" : health === "bad" ? "dead" : "busy";
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

const DOCKER_INSTALL = "curl -fsSL https://get.docker.com | sudo sh";

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
        {error.code === "auth"
          ? "Open a terminal to sign in, then containers show up here."
          : error.code === "hostKey"
            ? "Open a terminal to confirm this server's identity, then containers show up here."
            : error.message}
      </TreeNote>
    );
  if (!list)
    return (
      <TreeNote>
        <Spinner className="mt-px size-3" /> Looking for containers
      </TreeNote>
    );
  if (list.engines.length === 0 && list.problems.length === 0)
    return (
      <TreeNote>
        <span>
          No Docker or Podman here. To install Docker, run{" "}
          <button
            type="button"
            title="Copy"
            onClick={() => void navigator.clipboard.writeText(DOCKER_INSTALL).then(() => useStore.getState().toast({ tone: "info", title: "Copied", body: DOCKER_INSTALL }))}
            className="rounded bg-ink/[0.07] px-1 font-mono text-[10.5px] text-fg-2 hover:bg-ink/[0.12]"
          >
            {DOCKER_INSTALL}
          </button>{" "}
          in a terminal.
        </span>
      </TreeNote>
    );

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
      {list.engines.length > 0 && list.containers.length === 0 && (
        <TreeNote>No containers yet. Start some with docker compose up -d, and they show up here.</TreeNote>
      )}
    </div>
  );
}

/** A small status dot for the narrow rail, where the face doesn't fit. */
const HEALTH_DOT: Record<Health, string | null> = { idle: null, busy: "bg-fg-3 animate-pulse", live: null, warn: "bg-warn", bad: "bg-bad" };

function Row({ target, label, address, host, narrow }: { target: TargetRef; label: string; address?: string; host?: Host; narrow: boolean }) {
  const names = useNames();
  const env = useEnv(target);
  const { open: panes, waiting } = useMachine(target);
  const goTo = useStore((s) => s.goTo);
  const openWorkspace = useStore((s) => s.openWorkspace);
  const loadHosts = useStore((s) => s.loadHosts);
  const confirm = useStore((s) => s.confirm);
  const toast = useStore((s) => s.toast);
  const active = useStore((s) => sameTarget(currentTarget(s), target));
  const health = useHealth(target);
  const railStatus = useSettings((s) => s.railStatus);
  const side = useSettings((s) => s.sidebarSide);
  const [hover, setHover] = useState<DOMRect | null>(null);
  const hoverTimer = useRef(0);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [lookAt, setLookAt] = useState<Menu | null>(null);

  const items: (MenuItem | "sep")[] = [
    ...toolItems(target, names.on ? undefined : address),
    { label: "Terminal and files side by side", icon: LayoutDashboard, run: () => openWorkspace(target) },
    ...(host
      ? ([
          "sep",
          { label: "Icon and color…", icon: Palette, run: () => setLookAt(menu) },
          ...ENV_ORDER.map((e) => ({
            label: ENV_LABELS[e].name,
            icon: Circle,
            iconClass: ENV_LABELS[e].dot,
            checked: env === e,
            run: () => useLabels.getState().setEnv(host.id, env === e ? null : e),
          })),
        ] as (MenuItem | "sep")[])
      : []),
    ...(host?.rememberedPassword
      ? ([
          "sep",
          {
            label: "Forget saved password",
            icon: KeyRound,
            run: async () => {
              await backend.forgetPassword(host.id);
              void loadHosts();
              toast({ tone: "info", title: "Password forgotten", body: "It's gone from your keychain. The next login asks for it again." });
            },
          },
        ] as (MenuItem | "sep")[])
      : []),
    ...(host?.source === "sshConfig"
      ? ([
          "sep",
          {
            label: "Hide from list",
            icon: EyeOff,
            run: () => {
              useLabels.getState().setHidden(host.id, true);
              toast({ tone: "info", title: `Hid ${names.host(host)}`, body: "It stays in ~/.ssh/config. Settings → Layout → Servers from ~/.ssh/config brings it back." });
            },
          },
        ] as (MenuItem | "sep")[])
      : []),
    ...(host?.source === "saved"
      ? ([
          "sep",
          {
            label: "Remove server…",
            icon: Trash,
            danger: true,
            run: async () => {
              if (!(await confirm({ title: `Remove ${names.host(host)}?`, body: "Only removes it from SSHeesh. Nothing changes on the server.", confirmLabel: "Remove", danger: true }))) return;
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

  const pickLook = lookAt && host && <LookPopover at={lookAt} hostId={host.id} onClose={() => setLookAt(null)} />;

  if (narrow) {
    // A connected server shows its load instead of a plain dot.
    const showLoad = health === "live" && railStatus !== "off";
    const dot = HEALTH_DOT[health];
    const pulse = showLoad && railStatus === "pulse";
    const bars = showLoad && railStatus === "bars";
    const unhover = () => {
      window.clearTimeout(hoverTimer.current);
      setHover(null);
    };
    return (
      <>
        <button
          type="button"
          aria-label={label}
          aria-current={active || undefined}
          onClick={(e) => {
            unhover();
            if (e.shiftKey) openInNewWindow(target);
            else goTo(target);
          }}
          onPointerEnter={(e) => {
            const at = e.currentTarget.getBoundingClientRect();
            hoverTimer.current = window.setTimeout(() => setHover(at), 350);
          }}
          onPointerLeave={unhover}
          onContextMenu={(e: MouseEvent) => {
            e.preventDefault();
            unhover();
            setMenu({ x: e.clientX, y: e.clientY });
          }}
          className={cn(
            "relative mx-auto flex w-10 shrink-0 items-center justify-center rounded-[10px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent/60",
            pulse ? "h-12" : "h-10",
            active ? "bg-ink/[0.08]" : "hover:bg-ink/[0.06]",
          )}
        >
          <HostIcon
            hostId={host?.id}
            local={target.kind === "local"}
            size={bars ? 18 : 19}
            strokeWidth={1.9}
            className={cn("shrink-0", bars && "-translate-x-[7px]", pulse && "-translate-y-[6px]")}
          />
          {showLoad && <RailLoad target={target} style={railStatus} panes={panes} waiting={waiting} />}
          {env && <span className={cn("absolute bottom-[4px] left-1/2 h-[3px] w-3.5 -translate-x-1/2 rounded-full", ENV_LABELS[env].bar)} />}
          {dot && <span className={cn("absolute right-[5px] bottom-[5px] size-[7px] rounded-full ring-2 ring-[var(--ui-window)]", dot)} />}
        </button>
        <SidebarTabs target={target} narrow />
        {hover && !menu && (
          <RailCard
            target={target}
            at={hover}
            side={side}
            label={label}
            address={names.on ? undefined : address}
            env={env}
            health={health}
            waiting={waiting}
            panes={panes}
          />
        )}
        {menu && <ContextMenu menu={menu} items={items} onClose={() => setMenu(null)} />}
        {pickLook}
      </>
    );
  }

  return (
    <>
      <div className="group/row relative flex items-center">
        <button
          type="button"
          aria-label={expanded ? `Hide containers on ${label}` : `Show containers on ${label}`}
          aria-expanded={expanded}
          onClick={toggle}
          className="absolute top-0 left-0 z-10 flex h-7 w-5 items-center justify-center rounded text-fg-3 opacity-0 group-hover/row:opacity-100 hover:text-fg focus-visible:opacity-100"
        >
          <ChevronRight size={12} strokeWidth={2.4} className={cn("transition-transform", expanded && "rotate-90")} />
        </button>
        <button
          type="button"
          title={names.on ? undefined : address}
          aria-current={active || undefined}
          onClick={(e) => (e.shiftKey ? openInNewWindow(target) : goTo(target))}
          onContextMenu={(e: MouseEvent) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY });
          }}
          className={cn(
            "block w-full rounded-[6px] pr-1.5 pl-5 text-left text-[13px] outline-none text-fg hover:bg-ink/[0.05] focus-visible:bg-ink/[0.07]",
            active && "font-medium",
          )}
        >
          <span className="flex h-7 items-center gap-2">
            <HostIcon hostId={host?.id} local={target.kind === "local"} size={15} strokeWidth={1.8} className="shrink-0" />
            <span className="min-w-0 flex-1 truncate">{label}</span>
            <EnvChip env={env} />
            <StatusMark health={health} />
            <span className="w-5 shrink-0" />
          </span>
          {health === "live" && railStatus !== "off" && <LoadLine target={target} />}
        </button>
        <IconButton
          label={`New terminal on ${label}`}
          onClick={() => newTab("terminal", target)}
          className="absolute top-1 right-1 size-5 rounded-[5px] opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
        >
          <Plus size={13} strokeWidth={2} />
        </IconButton>
      </div>
      <SidebarTabs target={target} narrow={false} />
      {expanded && <ContainerTree target={target} />}
      {menu && <ContextMenu menu={menu} items={items} onClose={() => setMenu(null)} />}
      {pickLook}
    </>
  );
}

/**
 * The logo at the top opens the main menu, like a browser's home button.
 * Search sits beside it; it's also in the main menu and one shortcut away.
 * Neither goes to the bottom, which a window moved half off screen hides.
 */
function HomeButton({ narrow }: { narrow: boolean }) {
  const home = useStore(showingHome);
  const showHome = useStore((s) => s.showHome);
  const logo = useSettings((s) => s.logo !== "hidden");
  return (
    <button
      type="button"
      title="Main menu: your servers, what's running and the tabs you closed"
      aria-label="Main menu"
      aria-current={home || undefined}
      onClick={showHome}
      className={cn(
        "flex shrink-0 items-center gap-2.5 rounded-[10px] text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent/60",
        narrow ? "size-11 justify-center" : "h-12 min-w-0 flex-1 pr-2 pl-1",
        home ? "bg-ink/[0.1]" : "hover:bg-ink/[0.05]",
      )}
    >
      {logo ? <AppLogo size={narrow ? 36 : 40} /> : <House size={narrow ? 18 : 20} strokeWidth={1.8} className="mx-2.5 shrink-0 text-fg-2" />}
      {!narrow && <span className="truncate text-[14px] font-semibold tracking-tight text-fg">Main menu</span>}
    </button>
  );
}

/** Narrower than this while dragging, the list folds into a rail of icons. */
const FOLD_AT = 120;
const RAIL_WIDTH = 60;

export function Sidebar() {
  const hosts = useVisibleHosts();
  const names = useNames();
  const loaded = useStore((s) => s.hostsLoaded);
  const setPaletteOpen = useStore((s) => s.setPaletteOpen);
  const setAddHostOpen = useStore((s) => s.setAddHostOpen);
  const side = useSettings((s) => s.sidebarSide);
  const wideWidth = useSettings((s) => s.sidebarWidth);
  const narrow = useSettings((s) => s.sidebarMode === "narrow");
  const custom = useCustomTitleBar();
  const compact = useSettings((s) => s.compactMode);
  const update = useSettings((s) => s.update);
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const live = useRef<number | null>(null);
  const width = dragWidth ?? (narrow ? RAIL_WIDTH : wideWidth);

  // Width before this drag: folding into the rail keeps it, so unfolding
  // brings the list back exactly as wide as it was.
  const before = useRef(wideWidth);
  // The edge follows the pointer; past FOLD_AT it snaps between list and rail.
  const onResize = (dx: number) => {
    const st = useSettings.getState();
    if (live.current === null) before.current = st.sidebarWidth;
    let next = Math.round(Math.min(420, Math.max(RAIL_WIDTH, (live.current ?? width) + (side === "left" ? dx : -dx))));
    if (next < FOLD_AT) {
      if (st.sidebarMode !== "narrow") st.update({ sidebarMode: "narrow", sidebarWidth: before.current });
    } else if (st.sidebarMode === "narrow") {
      next = st.sidebarWidth;
      st.update({ sidebarMode: "wide" });
    }
    // The width follows the pointer; it's saved once, on release.
    live.current = next;
    setDragWidth(next);
  };
  useEffect(() => {
    if (dragWidth === null) return;
    const done = () => {
      const st = useSettings.getState();
      if (live.current !== null && st.sidebarMode === "wide") st.update({ sidebarWidth: Math.max(150, live.current) });
      live.current = null;
      setDragWidth(null);
    };
    window.addEventListener("pointerup", done);
    return () => window.removeEventListener("pointerup", done);
  }, [dragWidth]);

  const rows = (
    <>
      <Row target={{ kind: "local" }} label="This computer" narrow={narrow} />
      {hosts.map((h) => (
        <Row key={h.id} target={{ kind: "host", hostId: h.id }} label={names.host(h)} address={hostAddress(h)} host={h} narrow={narrow} />
      ))}
    </>
  );

  return (
    <aside
      aria-label="Servers"
      style={{ width }}
      className={cn("relative flex shrink-0 flex-col border-line", side === "left" ? "border-r" : "border-l")}
    >
      <ResizeHandle
        edge={side === "left" ? "right" : "left"}
        label="Resize sidebar"
        onResize={onResize}
        onReset={() => update({ sidebarMode: "wide", sidebarWidth: 232 })}
      />
      {/* Title bar space: macOS puts its window buttons here on the left; on
          Windows a sidebar on the right holds SSHeesh's own. */}
      <div data-tauri-drag-region className="flex h-[40px] shrink-0 justify-end">
        {custom && side === "right" && !compact && <WindowControls />}
      </div>

      {narrow ? (
        <nav className="flex flex-1 flex-col items-center gap-1 overflow-y-auto px-2 pb-3">
          <HomeButton narrow />
          <IconButton label={`Search (${shortcutLabel("palette")})`} onClick={() => setPaletteOpen(true)} className="size-10 rounded-[10px]">
            <Search size={17} strokeWidth={1.9} />
          </IconButton>
          <div className="my-1 h-px w-6 shrink-0 bg-ink/[0.09]" />
          {rows}
          <IconButton label="Add server" onClick={() => setAddHostOpen(true)} className="size-10 shrink-0 rounded-[10px]">
            <Plus size={17} strokeWidth={1.9} />
          </IconButton>
        </nav>
      ) : (
        <>
          <div className="flex items-center gap-1 px-2.5 pb-2">
            <HomeButton narrow={false} />
            <IconButton label={`Search (${shortcutLabel("palette")})`} onClick={() => setPaletteOpen(true)} className="size-8 rounded-[8px]">
              <Search size={15} strokeWidth={2} />
            </IconButton>
          </div>

          <ConfigNotice />
          <nav className="flex-1 overflow-y-auto px-2.5 pb-3">
            <div className="mt-1 flex h-6 items-center justify-between pr-0.5 pl-2">
              <h2 className="text-[11px] font-semibold text-fg-2">Servers</h2>
              <IconButton label="Add server" onClick={() => setAddHostOpen(true)} className="size-5 rounded-[5px]">
                <Plus size={14} strokeWidth={2} />
              </IconButton>
            </div>
            <div className="mt-0.5 flex flex-col gap-px">{rows}</div>
            {loaded && hosts.length === 0 && (
              <p className="mt-3 px-2 text-[12px] leading-snug text-fg-3">
                Add a server with the plus button. Servers from ~/.ssh/config show up here once you pick them.
              </p>
            )}
          </nav>
        </>
      )}
    </aside>
  );
}
