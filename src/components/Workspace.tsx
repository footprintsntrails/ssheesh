import { ArrowDownToLine, Columns2, EyeOff, FolderOpen, House, Minimize2, PanelLeft, PanelRight, RotateCw, Settings, Wrench } from "lucide-react";
import { Fragment, useMemo, useState } from "react";
import { Group, Panel, Separator } from "react-resizable-panels";
import { cn } from "../lib/format";
import { isMac, shortcutLabel } from "../lib/keys";
import { useSettings } from "../lib/settings";
import { useUpdates } from "../lib/updates";
import { visibleTree, type LayoutNode } from "../lib/layout";
import { toggleStreamer, useNames } from "../lib/privacy";
import { ENV_LABELS, useEnv } from "../lib/labels";
import { currentTarget, hostAddress, panesOn, showingHome, useMachine, useSplit, useStore } from "../lib/store";
import { showFiles, toggleSplit } from "../lib/tabs";
import { targetKey, type TargetRef } from "../lib/types";
import { ContextMenu, type Menu } from "./ContextMenu";
import { Home } from "./Home";
import { HostIcon } from "./HostIcon";
import { PaneView } from "./Pane";
import { TabStrip, toolItems } from "./Tabs";
import { VitalsBar } from "./StatsPane";
import { EdgeReveal } from "./EdgeReveal";
import { useCustomTitleBar, WindowControls } from "./WindowControls";
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

/** While one pane fills the window, a way back to the whole machine. */
function ZoomBadge({ target }: { target: TargetRef | null }) {
  const split = useSplit(target);
  const zoomed = useStore((s) => (split ? s.zoomed : null));
  const { open: count } = useMachine(target);
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

/** Shows up only when there's an update to act on, and calmly. */
function UpdateButton() {
  const status = useUpdates((u) => u.status);
  const restart = useUpdates((u) => u.restart);
  const openSettings = useStore((s) => s.openSettings);
  if (status.kind === "ready")
    return (
      <button
        type="button"
        onClick={restart}
        className="flex h-7 items-center gap-1.5 rounded-[7px] bg-accent/15 px-2.5 text-[12px] font-medium text-accent hover:bg-accent/20"
      >
        <RotateCw size={12.5} /> Restart to update
      </button>
    );
  if (status.kind === "available" || status.kind === "downloading")
    return (
      <button
        type="button"
        onClick={() => openSettings("updates")}
        className="flex h-7 items-center gap-1.5 rounded-[7px] px-2 text-[12px] font-medium text-fg-2 hover:bg-ink/[0.06]"
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

/** The machine you're on, like a browser's address: its icon, name, label and address. */
function AddressChip({ target }: { target: TargetRef }) {
  const names = useNames();
  const hosts = useStore((s) => s.hosts);
  const host = target.kind === "host" ? hosts.find((h) => h.id === target.hostId) : undefined;
  const env = useEnv(target);
  const [menu, setMenu] = useState<Menu | null>(null);
  const address = host ? hostAddress(host) : undefined;
  return (
    <>
      <button
        type="button"
        title={names.on || !address ? "Tools for this machine" : `${address}. Click for tools`}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: r.left, y: r.bottom + 4 });
        }}
        className="flex h-7 min-w-0 items-center gap-2 rounded-[7px] px-2 hover:bg-ink/[0.06]"
      >
        <HostIcon hostId={host?.id} local={target.kind === "local"} size={14} strokeWidth={1.9} className="shrink-0" />
        <span className="truncate text-[13px] font-semibold text-fg">{names.target(target)}</span>
        <EnvChip env={env} />
        {host && !names.on && <span className="hidden truncate text-[12px] text-fg-3 @2xl:inline">{names.address(host)}</span>}
      </button>
      {menu && <ContextMenu menu={menu} items={toolItems(target, names.on ? undefined : address)} onClose={() => setMenu(null)} />}
    </>
  );
}

/** Top row: the machine you're on, then a few buttons. Everything else lives in the sidebar. */
function Toolbar({ target }: { target: TargetRef | null }) {
  const sidebarSide = useSettings((s) => s.sidebarSide);
  const sidebarVisible = useSettings((s) => s.sidebarVisible);
  const updateSettings = useSettings((s) => s.update);
  const split = useSplit(target);
  const tabsOnTop = useSettings((s) => s.tabsPlacement === "top");
  const [tools, setTools] = useState<Menu | null>(null);
  // macOS draws the window buttons at the top left, over whatever is there.
  const narrowRail = useSettings((s) => s.sidebarMode === "narrow");
  const compactMode = useSettings((s) => s.compactMode);
  const clearTrafficLights = isMac && (!sidebarVisible || sidebarSide === "right" || compactMode);
  // A narrow rail is too thin for the window buttons; they spill over the toolbar.
  const nudgeForRail = isMac && sidebarVisible && sidebarSide === "left" && narrowRail && !compactMode;
  const SidebarIcon = sidebarSide === "left" ? PanelLeft : PanelRight;
  // Windows' window buttons sit at the window's top right: here, unless the
  // sidebar is on the right (it has them then) or compact mode keeps them up top.
  const custom = useCustomTitleBar();
  const controls = custom && !compactMode && (!sidebarVisible || sidebarSide === "left");

  return (
    <header
      data-tauri-drag-region
      className={cn("@container flex h-[40px] shrink-0 items-center gap-2", controls ? "pr-0" : "pr-2", clearTrafficLights ? "pl-[80px]" : nudgeForRail ? "pl-[24px]" : "pl-2")}
    >
      <IconButton
        label={`${sidebarVisible ? "Hide" : "Show"} sidebar (${shortcutLabel("sidebar")})`}
        onClick={() => updateSettings({ sidebarVisible: !sidebarVisible })}
        className={cn(sidebarSide === "right" && "order-last")}
      >
        <SidebarIcon size={15} />
      </IconButton>
      {(!sidebarVisible || compactMode) && (
        <IconButton label="Main menu" onClick={() => useStore.getState().showHome()} active={!target}>
          <House size={15} />
        </IconButton>
      )}

      <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center gap-2">
        {target ? <AddressChip target={target} /> : <span className="px-2 text-[13px] font-semibold text-fg">Main menu</span>}
        {target && tabsOnTop && <TabStrip target={target} />}
      </div>

      <div className="flex items-center gap-0.5">
        <StreamerBadge />
        <ZoomBadge target={target} />
        <UpdateButton />
        {target && (
          <>
            <IconButton label="Files" onClick={() => showFiles(target)}>
              <FolderOpen size={15} />
            </IconButton>
            <IconButton
              label="Tools"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                setTools({ x: r.right - 220, y: r.bottom + 4 });
              }}
            >
              <Wrench size={15} />
            </IconButton>
            <IconButton
              label={`${split ? "One tab at a time" : "Tabs side by side"} (${shortcutLabel("toggleSplit")})`}
              active={split}
              onClick={toggleSplit}
              data-hint="split"
            >
              <Columns2 size={15} />
            </IconButton>
          </>
        )}
        <IconButton label={`Settings (${shortcutLabel("settings")})`} onClick={() => useStore.getState().openSettings()}>
          <Settings size={15} />
        </IconButton>
      </div>
      {controls && (
        <div className="order-last ml-1">
          <WindowControls />
        </div>
      )}
      {tools && target && <ContextMenu menu={tools} items={toolItems(target)} onClose={() => setTools(null)} />}
    </header>
  );
}

/** A thin Production or Staging line along the top of the window, when you want one. */
export function EnvStripe() {
  const on = useSettings((s) => s.envStripe);
  const target = useStore(currentTarget);
  const env = useEnv(on ? target : null);
  if (!env || env === "dev") return null;
  return <div aria-hidden className={cn("pointer-events-none fixed inset-x-0 top-0 z-50 h-[2px]", ENV_LABELS[env].bar)} />;
}

export function Workspace() {
  const custom = useCustomTitleBar();
  const home = useStore(showingHome);
  const target = useStore(currentTarget);
  const key = target ? targetKey(target) : "";
  const layout = useStore((s) => (key ? s.layouts[key] : undefined));
  const hidden = useStore((s) => s.hidden);
  const focused = useStore((s) => s.focused);
  const split = useSplit(target);
  const zoomed = useStore((s) => (s.zoomed && s.panes[s.zoomed] && s.panes[s.zoomed].target && targetKey(s.panes[s.zoomed].target!) === key ? s.zoomed : null));
  // One machine on screen at a time, with its own layout: as tabs, the
  // focused pane fills the window; side by side, the panes you switched on share it.
  const tree = useMemo(() => (split && layout ? visibleTree(layout, hidden) : null), [split, layout, hidden]);
  const showVitals = useSettings((s) => s.showVitals && !s.compactMode) && !!target;
  const compact = useSettings((s) => s.compactMode);
  const updateSettings = useSettings((s) => s.update);
  const openStats = () => {
    if (!target) return;
    const st = useStore.getState();
    const existing = panesOn(st, target).find((p) => p.kind === "stats");
    // One at a time the details fill the window, so a second click goes back to the terminal.
    const back = !split && existing && st.focused === existing.id ? st.mainTerminalFor(target) : null;
    if (back) st.focus(back);
    else if (existing) st.focus(existing.id);
    else st.openIn("stats", target, { split: true, dir: "col" });
  };
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      {compact ? (
        <>
          {/* Room to drag the window, and for the window buttons. */}
          <div data-tauri-drag-region className={cn("flex shrink-0 justify-end", isMac ? "h-[30px]" : custom ? "h-8" : "h-[var(--ui-gap)]")}>
            {custom && <WindowControls height={32} />}
          </div>
          <EdgeReveal side="top" label="Toolbar">
            <div className="w-full">
              <Toolbar target={target} />
            </div>
          </EdgeReveal>
        </>
      ) : (
        <Toolbar target={target} />
      )}
      <div className="min-h-0 flex-1" style={{ padding: showVitals ? "0 var(--ui-gap)" : "0 var(--ui-gap) var(--ui-gap)" }}>
        {home ? (
          <Home />
        ) : zoomed && split ? (
          <PaneView key={zoomed} id={zoomed} />
        ) : tree ? (
          <Tree node={tree} />
        ) : (
          <PaneView key={focused} id={focused} />
        )}
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
