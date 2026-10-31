// Tabs: every terminal, file manager and stats page of a machine is a tab.
// They sit under their server in the sidebar (or, if you prefer, in a strip
// at the top). Drag one to move it, or onto the screen to show it beside the
// one you're on; middle-click or its × closes it, right-click for more.
// Ctrl+Tab goes to the next. What tabs do lives in lib/tabs.ts.

import { Activity, BookOpen, Box, Cable, Check, Columns2, Copy, FolderOpen, HardDrive, Plus, Rows2, ShieldCheck, SquareTerminal, X, type LucideIcon } from "lucide-react";
import { memo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import { agentName, agentRunning } from "../lib/agents";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import type { DropZone } from "../lib/layout";
import { useNames } from "../lib/privacy";
import { closeTab, newTab, placeBeside, showFiles, toOwnTab } from "../lib/tabs";
import { isSplit, tabsOn, useSplit, useStore, type PaneKind, type PaneState } from "../lib/store";
import { sameTarget, type TargetRef } from "../lib/types";
import { AgentIcon } from "./AgentIcons";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { openRecipes } from "./Recipes";
import { openServerTool } from "./ServerTools";
import { openTunnels } from "./Tunnels";

const KIND: Record<PaneKind, { label: string; icon: LucideIcon }> = {
  terminal: { label: "Terminal", icon: SquareTerminal },
  files: { label: "Files", icon: FolderOpen },
  stats: { label: "Stats", icon: Activity },
  empty: { label: "New tab", icon: Plus },
};

/**
 * "Terminal", "Terminal 2"…: each tab keeps the number it got when it
 * opened, so moving or closing tabs never renames the others. A terminal
 * with an AI agent in front is called after the agent.
 */
export function tabLabel(p: Pick<PaneState, "kind" | "container" | "n">, command?: string): string {
  if (p.container) return p.container.name;
  const agent = p.kind === "terminal" ? agentRunning(command) : null;
  if (agent) return agentName(agent);
  const n = p.n ?? 1;
  return n > 1 ? `${KIND[p.kind].label} ${n}` : KIND[p.kind].label;
}

export function TabIcon({ pane, command, size = 13.5, className }: { pane: Pick<PaneState, "kind" | "container">; command?: string; size?: number; className?: string }) {
  const agent = pane.kind === "terminal" ? agentRunning(command) : null;
  if (agent) return <AgentIcon id={agent} size={size} />;
  const Icon = pane.container ? Box : KIND[pane.kind].icon;
  return <Icon size={size} strokeWidth={1.9} className={cn("shrink-0", className)} />;
}

/** The tools for a machine, as one menu: from the address chip, the Tools button and right-clicks. */
export function toolItems(target: TargetRef, address?: string): (MenuItem | "sep")[] {
  return [
    { label: "New terminal", icon: SquareTerminal, hint: shortcutLabel("newTerminal"), run: () => newTab("terminal", target) },
    { label: "Files", icon: FolderOpen, run: () => showFiles(target) },
    ...(target.kind === "host"
      ? ([
          "sep",
          { label: "Security check-up", icon: ShieldCheck, run: () => openServerTool("checkup", target) },
          { label: "What's using the disk", icon: HardDrive, run: () => openServerTool("disk", target) },
          { label: "Web apps and tunnels", icon: Cable, run: () => openTunnels(target) },
          { label: "Recipes", icon: BookOpen, run: () => openRecipes(target) },
        ] as (MenuItem | "sep")[])
      : []),
    ...(address
      ? ([
          "sep",
          {
            label: "Copy address",
            icon: Copy,
            run: () => void navigator.clipboard.writeText(address).then(() => useStore.getState().toast({ tone: "info", title: "Copied", body: address })),
          },
        ] as (MenuItem | "sep")[])
      : []),
  ];
}

/** Where a pane being dragged would land: a side of a pane of the same machine, or nowhere. */
function dropAt(x: number, y: number, dragId: string): { paneId: string; zone: DropZone } | null {
  const s = useStore.getState();
  const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-pane-id]");
  const id = el?.dataset.paneId;
  if (!el || !id || id === dragId || !sameTarget(s.panes[id]?.target, s.panes[dragId]?.target)) return null;
  const r = el.getBoundingClientRect();
  const fx = (x - r.left) / r.width;
  const fy = (y - r.top) / r.height;
  return { paneId: id, zone: fx < 0.25 ? "left" : fx > 0.75 ? "right" : fy < 0.3 ? "top" : fy > 0.7 ? "bottom" : "center" };
}

/**
 * Drags a pane from its title bar or its tab. Over the screen it shows where
 * it would go and lands there on release; anywhere else `onMove` gets the
 * pointer (to reorder tabs). A title bar dropped on the tab list goes back to
 * being a tab of its own. `onClick` runs when it was a click after all.
 */
export function dragPane(
  e: ReactPointerEvent,
  paneId: string,
  opts: { onMove?: (x: number, y: number) => void; onDrop?: () => void; onClick?: () => void } = {},
) {
  if (e.button !== 0) return;
  const sx = e.clientX;
  const sy = e.clientY;
  let dragging = false;
  const move = (ev: PointerEvent) => {
    const st = useStore.getState();
    if (!dragging) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      dragging = true;
      st.setDrag(paneId);
      document.body.style.cursor = "grabbing";
    }
    const at = dropAt(ev.clientX, ev.clientY, paneId);
    st.setDropTarget(at);
    if (!at) opts.onMove?.(ev.clientX, ev.clientY);
  };
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    document.body.style.cursor = "";
    if (!dragging) return opts.onClick?.();
    const st = useStore.getState();
    const at = st.dropTarget;
    st.setDrag(null);
    if (at) {
      const target = st.panes[paneId]?.target ?? null;
      if (at.zone === "center" && isSplit(st, target)) st.movePane(paneId, at.paneId, "center");
      else placeBeside(paneId, at.zone === "center" ? "right" : at.zone, at.paneId);
    } else if (opts.onDrop) opts.onDrop();
    else if (document.elementFromPoint(ev.clientX, ev.clientY)?.closest("[data-tab-list]")) toOwnTab(paneId);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}

/** What you can do with a tab, for its right-click menu. */
function tabMenu(id: string, target: TargetRef): (MenuItem | "sep")[] {
  const s = useStore.getState();
  const tabs = tabsOn(s, target);
  const i = tabs.findIndex((t) => t.id === id);
  const others = tabs.filter((t) => t.id !== id);
  const below = tabs.slice(i + 1);
  const canBeside = id !== s.focused && sameTarget(s.panes[s.focused]?.target, target);
  return [
    ...(canBeside
      ? ([
          { label: "Show beside this one", icon: Columns2, run: () => placeBeside(id, "right") },
          { label: "Show below this one", icon: Rows2, run: () => placeBeside(id, "bottom") },
          "sep",
        ] as (MenuItem | "sep")[])
      : []),
    { label: "New terminal", icon: SquareTerminal, hint: shortcutLabel("newTerminal"), run: () => newTab("terminal", target) },
    { label: "Files", icon: FolderOpen, run: () => newTab("files", target) },
    "sep",
    { label: "Close tab", icon: X, hint: shortcutLabel("closePane"), run: () => closeTab(id) },
    { label: "Close other tabs", icon: X, disabled: !others.length, run: () => others.forEach((t) => closeTab(t.id)) },
    { label: "Close tabs below", icon: X, disabled: !below.length, run: () => below.forEach((t) => closeTab(t.id)) },
  ];
}

/** Needs you, done, working or broken: shapes, not just colors. */
function TabState({ id }: { id: string }) {
  const attention = useStore((s) => s.attention[id]);
  const progress = useStore((s) => !!s.progress[id]);
  const state = useStore((s) => s.sessions[id]?.kind);
  if (attention?.kind === "input")
    return (
      <span title={attention.message ?? "Waiting for you"} className="shrink-0 text-[11px] font-medium text-warn">
        Needs you
      </span>
    );
  if (state === "failed" || state === "disconnected")
    return (
      <span title={state === "failed" ? "Couldn't connect" : "Disconnected"} className="flex size-3.5 shrink-0 items-center justify-center rounded-full bg-bad text-[10px] font-bold leading-none text-window">
        !
      </span>
    );
  if (attention?.kind === "done") return <Check size={12} strokeWidth={2.6} aria-label="Went quiet" className="shrink-0 text-ok" />;
  if (progress || state === "connecting" || state === "reconnecting")
    return <span aria-label="Working" className="size-2.5 shrink-0 animate-spin rounded-full border-[1.5px] border-accent border-t-transparent" />;
  return null;
}

/** On a narrow tab icon only the urgent states fit: needs you and broken. */
function NarrowState({ id }: { id: string }) {
  const input = useStore((s) => s.attention[id]?.kind === "input");
  const broken = useStore((s) => s.sessions[id]?.kind === "failed" || s.sessions[id]?.kind === "disconnected");
  if (input) return <span aria-label="Needs you" className="block size-2 rounded-full bg-warn ring-2 ring-[var(--ui-window)]" />;
  if (broken) return <span aria-label="Disconnected" className="block size-2 rounded-full bg-bad ring-2 ring-[var(--ui-window)]" />;
  return null;
}

type TabProps = ReturnType<ReturnType<typeof useTabList>["tabProps"]>;

/**
 * A machine's tab list, shared by the sidebar and the strip on top: the
 * tabs in order, dragging to reorder along `axis`, and a right-click menu.
 */
function useTabList(target: TargetRef, axis: "x" | "y") {
  const ids = useStore(useShallow((s) => tabsOn(s, target).map((p) => p.id)));
  const [order, setOrder] = useState<string[] | null>(null);
  const els = useRef(new Map<string, HTMLElement>());
  const [menu, setMenu] = useState<{ x: number; y: number; items: (MenuItem | "sep")[] } | null>(null);
  const shown = order ? order.filter((id) => ids.includes(id)) : ids;

  // Along the list reorders; onto the screen shows it beside.
  const reorder = (id: string) => (x: number, y: number) => {
    const at = axis === "x" ? x : y;
    const others = (order ?? ids).filter((t) => t !== id);
    let slot = others.length;
    for (let i = 0; i < others.length; i++) {
      const r = els.current.get(others[i])?.getBoundingClientRect();
      if (r && at < (axis === "x" ? r.left + r.width / 2 : r.top + r.height / 2)) {
        slot = i;
        break;
      }
    }
    const next = [...others.slice(0, slot), id, ...others.slice(slot)];
    setOrder((o) => (o && o.length === next.length && o.every((t, i) => t === next[i]) ? o : next));
  };

  const tabProps = (id: string) => ({
    ref: (el: HTMLElement | null) => {
      if (el) els.current.set(id, el);
      else els.current.delete(id);
    },
    role: "tab",
    tabIndex: 0,
    onPointerDown: (e: ReactPointerEvent) =>
      dragPane(e, id, {
        onMove: reorder(id),
        onClick: () => useStore.getState().focus(id),
        onDrop: () =>
          setOrder((o) => {
            if (o) useStore.getState().setTabOrder(o);
            return null;
          }),
      }),
    onKeyDown: (e: React.KeyboardEvent) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), useStore.getState().focus(id)),
    onMouseDown: (e: React.MouseEvent) => e.button === 1 && e.preventDefault(),
    onAuxClick: (e: React.MouseEvent) => e.button === 1 && closeTab(id),
    onContextMenu: (e: React.MouseEvent) => {
      e.preventDefault();
      setMenu({ x: e.clientX, y: e.clientY, items: tabMenu(id, target) });
    },
  });

  const menuEl = menu && <ContextMenu menu={menu} items={menu.items} onClose={() => setMenu(null)} />;
  return { shown, tabProps, setMenu, menuEl };
}

function CloseButton({ id, label, className }: { id: string; label: string; className: string }) {
  return (
    <button
      type="button"
      aria-label={`Close ${label}`}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        closeTab(id);
      }}
      className={className}
    >
      <X size={12} strokeWidth={2.2} />
    </button>
  );
}

/** One tab in the sidebar; it only redraws when something about this tab changes. */
const SidebarTab = memo(function SidebarTab({ id, narrow, split, props }: { id: string; narrow: boolean; split: boolean; props: TabProps }) {
  const pane = useStore((s) => s.panes[id]);
  const active = useStore((s) => !s.home && s.focused === id);
  const onScreen = useStore((s) => split && !s.home && !s.hidden.includes(id) && sameTarget(s.panes[s.focused]?.target, s.panes[id]?.target));
  const command = useStore((s) => s.foreground[id]);
  if (!pane) return null;
  const label = tabLabel(pane, command);
  return (
    <div
      {...props}
      aria-selected={active}
      aria-label={label}
      data-hint={agentRunning(command) ? "agent-tab" : undefined}
      title={narrow ? label : `${label}. Drag onto the screen to show it beside, middle-click to close`}
      className={cn(
        "group/tab relative flex cursor-default items-center select-none outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent/60",
        narrow ? "size-8 justify-center rounded-[8px]" : "h-7 gap-2 rounded-[6px] pr-1 pl-[27px] text-[12.5px]",
        active ? "bg-ink/[0.1] text-fg" : onScreen ? "bg-ink/[0.045] text-fg" : "text-fg-2 hover:bg-ink/[0.05] hover:text-fg",
      )}
    >
      <TabIcon pane={pane} command={command} size={narrow ? 15 : 13.5} className={active ? "text-fg" : "text-fg-3"} />
      {narrow ? (
        <span className="absolute -top-0.5 -right-0.5 empty:hidden">
          <NarrowState id={id} />
        </span>
      ) : (
        <>
          <span className="min-w-0 flex-1 truncate">{label}</span>
          <span className="contents group-hover/tab:hidden">
            <TabState id={id} />
          </span>
          <CloseButton
            id={id}
            label={label}
            className="hidden size-5 shrink-0 items-center justify-center rounded-[4px] text-fg-3 hover:bg-ink/[0.1] hover:text-fg group-hover/tab:flex"
          />
        </>
      )}
    </div>
  );
});

/** A machine's tabs, under its row in the sidebar. Narrow, they're icons under the server's icon. */
export function SidebarTabs({ target, narrow }: { target: TargetRef; narrow: boolean }) {
  const names = useNames();
  const split = useSplit(target);
  const { shown, tabProps, menuEl } = useTabList(target, "y");
  if (!shown.length) return null;
  return (
    <div
      data-tab-list
      data-hint="tabs"
      role="tablist"
      aria-orientation="vertical"
      aria-label={`Tabs on ${names.target(target)}`}
      className={cn("flex flex-col", narrow ? "items-center gap-0.5 pb-1" : "gap-px pb-1")}
    >
      {shown.map((id) => (
        <SidebarTab key={id} id={id} narrow={narrow} split={split} props={tabProps(id)} />
      ))}
      {menuEl}
    </div>
  );
}

const StripTab = memo(function StripTab({ id, props }: { id: string; props: TabProps }) {
  const pane = useStore((s) => s.panes[id]);
  const active = useStore((s) => s.focused === id);
  const command = useStore((s) => s.foreground[id]);
  if (!pane) return null;
  const label = tabLabel(pane, command);
  return (
    <div
      {...props}
      aria-selected={active}
      title={`${label}. Drag onto the screen to show it beside, middle-click to close`}
      className={cn(
        "group flex h-7 max-w-[200px] min-w-[96px] shrink-0 cursor-default items-center gap-1.5 rounded-[7px] pr-1 pl-2.5 text-[12px] font-medium transition-colors select-none",
        active ? "bg-ink/[0.1] text-fg" : "text-fg-3 hover:bg-ink/[0.05] hover:text-fg-2",
      )}
    >
      <TabIcon pane={pane} command={command} size={12.5} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <TabState id={id} />
      <CloseButton
        id={id}
        label={label}
        className={cn(
          "flex size-4 shrink-0 items-center justify-center rounded-[4px] text-fg-3 hover:bg-ink/[0.1] hover:text-fg",
          active ? "opacity-70" : "opacity-0 group-hover:opacity-70",
        )}
      />
    </div>
  );
});

/** Tabs in a strip at the top, for those who move them there in Settings. */
export function TabStrip({ target }: { target: TargetRef }) {
  const { shown, tabProps, setMenu, menuEl } = useTabList(target, "x");
  return (
    <div className="flex min-w-0 items-center gap-1">
      <div
        data-tab-list
        role="tablist"
        aria-label="Tabs"
        onWheel={(e) => {
          if (e.deltaY) e.currentTarget.scrollLeft += e.deltaY;
        }}
        className="flex h-7 min-w-0 items-center gap-0.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {shown.map((id) => (
          <StripTab key={id} id={id} props={tabProps(id)} />
        ))}
      </div>
      <button
        type="button"
        aria-label="New tab"
        title={`New tab (${shortcutLabel("newTerminal")} for a terminal)`}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: r.left, y: r.bottom + 4, items: toolItems(target).slice(0, 2) });
        }}
        className="flex size-7 shrink-0 items-center justify-center rounded-[8px] text-fg-3 hover:bg-ink/[0.06] hover:text-fg"
      >
        <Plus size={14} />
      </button>
      {menuEl}
    </div>
  );
}
