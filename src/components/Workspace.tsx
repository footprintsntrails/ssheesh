import { Activity, ArrowDownToLine, Minimize2, Columns2, Columns3, EyeOff, FolderOpen, LayoutDashboard, LayoutGrid, PanelLeft, PanelRight, RotateCw, Rows2, Rows3, Settings, SquareTerminal } from "lucide-react";
import { Fragment, useEffect, useRef, useState } from "react";
import { Group, Panel, Separator } from "react-resizable-panels";
import { cn } from "../lib/format";
import { isMac, shortcutLabel } from "../lib/keys";
import { useSettings } from "../lib/settings";
import { useUpdates } from "../lib/updates";
import type { Arrangement, LayoutNode } from "../lib/layout";
import { toggleStreamer, useNames } from "../lib/privacy";
import { useEnv } from "../lib/labels";
import { useStore, type PaneKind } from "../lib/store";
import { sameTarget } from "../lib/types";
import { PaneView } from "./Pane";
import { VitalsBar } from "./StatsPane";
import { EdgeReveal } from "./EdgeReveal";
import { EnvChip, IconButton } from "./ui";

function Tree({ node }: { node: LayoutNode }) {
  if (node.kind === "pane") return <PaneView id={node.id} />;
  return (
    <Group
      orientation={node.dir === "row" ? "horizontal" : "vertical"}
      className="h-full w-full"
      resizeTargetMinimumSize={{ fine: 14, coarse: 36 }}
    >
      {node.children.map((child, i) => (
        <Fragment key={child.id}>
          {i > 0 && <Separator className="split-handle" />}
          <Panel id={child.id} minSize={140} defaultSize={`${100 / node.children.length}%`}>
            <Tree node={child} />
          </Panel>
        </Fragment>
      ))}
    </Group>
  );
}

const VIEWS: { kind: Exclude<PaneKind, "empty">; label: string; icon: typeof SquareTerminal }[] = [
  { kind: "terminal", label: "Terminal", icon: SquareTerminal },
  { kind: "files", label: "Files", icon: FolderOpen },
  { kind: "stats", label: "Stats", icon: Activity },
];

/** Ready-made layouts: a full workspace for this machine, or a new shape for what's open. */
function ArrangeMenu() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const target = useStore((s) => s.panes[s.focused]?.target ?? null);
  const names = useNames();
  const openWorkspace = useStore((s) => s.openWorkspace);
  const arrangePanes = useStore((s) => s.arrangePanes);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  const shapes: { id: Arrangement; label: string; icon: typeof Columns3 }[] = [
    { id: "main", label: "Main + stack", icon: LayoutDashboard },
    { id: "columns", label: "Columns", icon: Columns3 },
    { id: "rows", label: "Rows", icon: Rows3 },
    { id: "grid", label: "Grid", icon: LayoutGrid },
  ];
  const item = "flex h-8 w-full items-center gap-2.5 rounded-[7px] px-2 text-left text-[12.5px] text-fg hover:bg-accent hover:text-white";

  return (
    <div ref={ref} className="relative">
      <IconButton label="Layouts" active={open} onClick={() => setOpen((v) => !v)}>
        <LayoutDashboard size={15} />
      </IconButton>
      {open && (
        <div className="glass absolute top-9 right-0 z-40 w-[250px] animate-pop-in rounded-[12px] p-1">
          {target && (
            <>
              <button
                className={item}
                onClick={() => {
                  setOpen(false);
                  openWorkspace(target);
                }}
              >
                <SquareTerminal size={14} className="opacity-80" />
                <span className="flex-1 truncate">Workspace for {names.target(target)}</span>
              </button>
              <div className="px-2 pb-1 text-[11px] leading-snug text-fg-3">Terminal, files and stats side by side</div>
              <div className="mx-2 my-1 h-px bg-ink/[0.08]" />
            </>
          )}
          <div className="px-2 pt-1 pb-0.5 text-[11px] font-semibold text-fg-3">Arrange open panes</div>
          {shapes.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              className={item}
              onClick={() => {
                setOpen(false);
                arrangePanes(id);
              }}
            >
              <Icon size={14} className="opacity-80" />
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** While one pane fills the window, a way back to the full layout. */
function ZoomBadge() {
  const zoomed = useStore((s) => s.zoomed);
  const count = useStore((s) => Object.keys(s.panes).length);
  const toggleZoom = useStore((s) => s.toggleZoom);
  if (!zoomed || count < 2) return null;
  return (
    <button
      type="button"
      onClick={() => toggleZoom()}
      className="flex h-7 items-center gap-1.5 rounded-[7px] bg-ink/[0.07] px-2.5 text-[12px] font-medium text-fg hover:bg-ink/[0.1]"
    >
      <Minimize2 size={13} /> Show all {count} panes
      <span className="text-fg-3">{shortcutLabel("zoomPane")}</span>
    </button>
  );
}

/** Shows up only when there's an update to act on. */
function UpdateButton() {
  const status = useUpdates((u) => u.status);
  const restart = useUpdates((u) => u.restart);
  const openSettings = useStore((s) => s.openSettings);
  if (status.kind === "ready")
    return (
      <button
        type="button"
        onClick={restart}
        className="flex h-7 items-center gap-1.5 rounded-[7px] bg-accent px-2.5 text-[12px] font-medium text-white hover:brightness-110"
      >
        <RotateCw size={12.5} /> Restart to update
      </button>
    );
  if (status.kind === "available" || status.kind === "downloading")
    return (
      <button
        type="button"
        onClick={() => openSettings("updates")}
        className="flex h-7 items-center gap-1.5 rounded-[7px] px-2 text-[12px] font-medium text-accent hover:bg-ink/[0.06]"
      >
        <ArrowDownToLine size={13} />
        {status.kind === "downloading" ? "Downloading update" : `Update to ${status.version}`}
      </button>
    );
  return null;
}

/** Visible whenever names are hidden, so you always know, and one click away from off. */
function StreamerBadge() {
  const on = useSettings((s) => s.streamerMode);
  if (!on) return null;
  return (
    <button
      type="button"
      onClick={toggleStreamer}
      title={`Streamer mode hides server names, addresses and users. Click to show them (${shortcutLabel("streamer")})`}
      className="flex h-7 items-center gap-1.5 rounded-[7px] bg-accent/15 px-2.5 text-[12px] font-medium text-accent hover:bg-accent/20"
    >
      <EyeOff size={13} /> Streamer mode
    </button>
  );
}

/** Top bar: what's focused, and one-click views of the same machine. */
function Toolbar() {
  const pane = useStore((s) => s.panes[s.focused]);
  const hosts = useStore((s) => s.hosts);
  const names = useNames();
  const panes = useStore((s) => s.panes);
  const focus = useStore((s) => s.focus);
  const openIn = useStore((s) => s.openIn);
  const split = useStore((s) => s.split);
  const target = pane?.target ?? null;
  const host = target?.kind === "host" ? hosts.find((h) => h.id === target.hostId) : undefined;
  const env = useEnv(target);
  const sidebarSide = useSettings((s) => s.sidebarSide);
  const sidebarVisible = useSettings((s) => s.sidebarVisible);
  const updateSettings = useSettings((s) => s.update);
  const showVitals = useSettings((s) => s.showVitals);
  // macOS draws the window buttons at the top left, over whatever is there.
  const narrowRail = useSettings((s) => s.sidebarMode === "narrow");
  const compactMode = useSettings((s) => s.compactMode);
  const clearTrafficLights = isMac && (!sidebarVisible || sidebarSide === "right" || compactMode);
  // A narrow rail is too thin for the window buttons; they spill over the toolbar.
  const nudgeForRail = isMac && sidebarVisible && sidebarSide === "left" && narrowRail && !compactMode;
  const SidebarIcon = sidebarSide === "left" ? PanelLeft : PanelRight;

  const show = (kind: Exclude<PaneKind, "empty">) => {
    if (!target) return;
    const existing = Object.values(panes).find((p) => p.kind === kind && sameTarget(p.target, target));
    if (existing) focus(existing.id);
    else openIn(kind, target, { split: pane?.kind !== "empty" });
  };

  return (
    <header data-tauri-drag-region className={cn("flex h-[46px] shrink-0 items-center gap-3 pr-3", clearTrafficLights ? "pl-[80px]" : nudgeForRail ? "pl-[24px]" : "pl-2")}>
      <IconButton
        label={`${sidebarVisible ? "Hide" : "Show"} sidebar (${shortcutLabel("sidebar")})`}
        onClick={() => updateSettings({ sidebarVisible: !sidebarVisible })}
        className={cn(sidebarSide === "right" && "order-last")}
      >
        <SidebarIcon size={15} />
      </IconButton>
      <div data-tauri-drag-region className="flex min-w-0 flex-1 items-baseline gap-2">
        <span className="truncate text-[13px] font-semibold text-fg">{target ? names.target(target) : "SSHeesh"}</span>
        <EnvChip env={env} className="self-center" />
        {host && !names.on && <span className="truncate text-[12px] text-fg-3">{names.address(host)}</span>}
      </div>

      <div
        role="tablist"
        aria-label="Views of this machine"
        className={cn(
          "flex h-7 items-center rounded-[9px] bg-ink/[0.045] p-[3px] ring-1 ring-inset ring-ink/[0.06] transition-opacity",
          !target && "pointer-events-none opacity-40",
        )}
      >
        {VIEWS.map(({ kind, label, icon: Icon }) => {
          const active = pane?.kind === kind;
          return (
            <button
              key={kind}
              role="tab"
              aria-selected={active}
              onClick={() => show(kind)}
              className={cn(
                "flex h-[22px] items-center gap-1.5 rounded-[6px] px-2.5 text-[12px] font-medium transition-all",
                active
                  ? "bg-ink/[0.11] text-fg shadow-[0_1px_2px_rgb(0_0_0/0.35),inset_0_1px_0_rgb(255_255_255/0.07)]"
                  : "text-fg-3 hover:text-fg-2",
              )}
            >
              <Icon size={12.5} strokeWidth={2} />
              {label}
            </button>
          );
        })}
      </div>

      <div className="flex items-center gap-0.5">
        <StreamerBadge />
        <ZoomBadge />
        <UpdateButton />
        <IconButton
          label={`${showVitals ? "Hide" : "Show"} server vitals (${shortcutLabel("vitals")})`}
          active={showVitals}
          onClick={() => updateSettings({ showVitals: !showVitals })}
        >
          <Activity size={15} />
        </IconButton>
        <ArrangeMenu />
        <IconButton label={`Split right (${shortcutLabel("splitRight")})`} onClick={() => pane && split(pane.id, "row")}>
          <Columns2 size={15} />
        </IconButton>
        <IconButton label={`Split down (${shortcutLabel("splitDown")})`} onClick={() => pane && split(pane.id, "col")}>
          <Rows2 size={15} />
        </IconButton>
        <IconButton label={`Settings (${shortcutLabel("settings")})`} onClick={() => useStore.getState().openSettings()}>
          <Settings size={15} />
        </IconButton>
      </div>
    </header>
  );
}

export function Workspace() {
  const root = useStore((s) => s.root);
  const zoomed = useStore((s) => (s.zoomed && s.panes[s.zoomed] ? s.zoomed : null));
  const showVitals = useSettings((s) => s.showVitals && !s.compactMode);
  const compact = useSettings((s) => s.compactMode);
  const updateSettings = useSettings((s) => s.update);
  // Vitals follow the focused pane's machine, or the first one on screen.
  const target = useStore((s) => s.panes[s.focused]?.target ?? Object.values(s.panes).find((p) => p.target)?.target ?? null);
  const openStats = () => {
    if (!target) return;
    const st = useStore.getState();
    const existing = Object.values(st.panes).find((p) => p.kind === "stats" && sameTarget(p.target, target));
    if (existing) st.focus(existing.id);
    else st.openIn("stats", target, { split: true, dir: "col" });
  };
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      {compact ? (
        <>
          {/* Room to drag the window, and for the macOS window buttons. */}
          <div data-tauri-drag-region className={cn("shrink-0", isMac ? "h-[30px]" : "h-[var(--ui-gap)]")} />
          <EdgeReveal side="top" label="Toolbar">
            <div className="w-full">
              <Toolbar />
            </div>
          </EdgeReveal>
        </>
      ) : (
        <Toolbar />
      )}
      <div className="min-h-0 flex-1" style={{ padding: showVitals && target ? "0 var(--ui-gap)" : "0 var(--ui-gap) var(--ui-gap)" }}>
        {zoomed ? <PaneView id={zoomed} /> : <Tree node={root} />}
      </div>
      {showVitals && target && (
        <VitalsBar
          key={target.kind === "local" ? "local" : target.hostId}
          target={target}
          onDetails={openStats}
          onHide={() => updateSettings({ showVitals: false })}
        />
      )}
    </main>
  );
}
