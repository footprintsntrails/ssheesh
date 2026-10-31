// Tabs, for the "Tabs" layout: every terminal and file manager of the machine you're on is a tab
// at the top. Drag one to move it, middle-click or its × to close it,
// right-click for more, and + for a new one. Ctrl+Tab goes to the next.

import { Activity, FolderOpen, Plus, SquareTerminal, X, type LucideIcon } from "lucide-react";
import { useRef, useState } from "react";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { paneIds } from "../lib/layout";
import { useSettings } from "../lib/settings";
import { useStore, type PaneKind, type PaneState } from "../lib/store";
import { sameTarget, type TargetRef } from "../lib/types";
import { ContextMenu, type MenuItem } from "./ContextMenu";

const KIND: Record<PaneKind, { label: string; icon: LucideIcon }> = {
  terminal: { label: "Terminal", icon: SquareTerminal },
  files: { label: "Files", icon: FolderOpen },
  stats: { label: "Stats", icon: Activity },
  empty: { label: "New tab", icon: Plus },
};

type Snapshot = Pick<ReturnType<typeof useStore.getState>, "root" | "panes" | "tabOrder">;

/**
 * A machine's panes in layout order, which is the order they were opened in.
 * Stats aren't a tab: they're in the vitals bar at the bottom, whose details
 * open the full view.
 */
function panesOf(s: Snapshot, target: TargetRef): PaneState[] {
  return paneIds(s.root)
    .map((id) => s.panes[id])
    .filter((p): p is PaneState => !!p && p.kind !== "stats" && sameTarget(p.target, target));
}

/** A machine's tabs: in the order you dragged them to, the rest as they were opened. */
export function tabsFor(s: Snapshot, target: TargetRef): PaneState[] {
  const rank = (id: string) => {
    const i = s.tabOrder.indexOf(id);
    return i < 0 ? Number.MAX_SAFE_INTEGER : i;
  };
  return panesOf(s, target)
    .map((p, i) => ({ p, i }))
    .sort((a, b) => rank(a.p.id) - rank(b.p.id) || a.i - b.i)
    .map((x) => x.p);
}

/**
 * "Terminal", "Terminal 2"…: each pane keeps the number it got when it
 * opened, so moving or closing tabs never renames the others. Panes from
 * before numbers existed count in the order they were opened.
 */
function tabLabel(p: PaneState, opened: PaneState[]): string {
  if (p.container) return p.container.name;
  const n = p.n ?? opened.filter((x) => x.kind === p.kind).indexOf(p) + 1;
  return n > 1 ? `${KIND[p.kind].label} ${n}` : KIND[p.kind].label;
}

/** Closes a pane. As tabs, the one next to it comes up, as in a browser. */
export function closeTab(id: string) {
  const s = useStore.getState();
  const p = s.panes[id];
  if (!p) return;
  const tabs = useSettings.getState().viewMode === "tabs" && p.target ? tabsFor(s, p.target) : [];
  const i = tabs.findIndex((t) => t.id === id);
  const next = tabs[i + 1] ?? tabs[i - 1];
  s.closePane(id);
  if (next && s.focused === id) useStore.getState().focus(next.id);
}

/** Ctrl+Tab: the next tab (or, side by side, the next pane on screen). */
export function nextTab(step: 1 | -1) {
  const s = useStore.getState();
  const target = s.panes[s.focused]?.target;
  const ids =
    useSettings.getState().viewMode === "tabs"
      ? target
        ? tabsFor(s, target).map((p) => p.id)
        : []
      : paneIds(s.root).filter((id) => !s.hidden.includes(id));
  if (ids.length < 2) return;
  const i = ids.indexOf(s.focused);
  s.focus(ids[(i + step + ids.length) % ids.length]);
}

export function TabStrip({ target }: { target: TargetRef }) {
  const root = useStore((s) => s.root);
  const panes = useStore((s) => s.panes);
  const tabOrder = useStore((s) => s.tabOrder);
  const focused = useStore((s) => s.focused);
  const attention = useStore((s) => s.attention);
  const focus = useStore((s) => s.focus);
  const openIn = useStore((s) => s.openIn);
  const setTabOrder = useStore((s) => s.setTabOrder);
  const snap = { root, panes, tabOrder };
  const opened = panesOf(snap, target);
  const tabs = tabsFor(snap, target);

  // Dragging: the tabs show the new order as you go, and it sticks on release.
  const [order, setOrder] = useState<string[] | null>(null);
  const press = useRef<{ id: string; x: number; moved: boolean } | null>(null);
  const els = useRef(new Map<string, HTMLElement>());
  const shown = order ? (order.map((id) => tabs.find((t) => t.id === id)).filter(Boolean) as PaneState[]) : tabs;

  const [menu, setMenu] = useState<{ x: number; y: number; items: (MenuItem | "sep")[] } | null>(null);
  const newItems = (): (MenuItem | "sep")[] => [
    { label: "New terminal", icon: SquareTerminal, hint: shortcutLabel("newTerminal"), run: () => openIn("terminal", target, { split: true }) },
    { label: "Files", icon: FolderOpen, run: () => openIn("files", target, { split: true }) },
  ];
  const tabItems = (id: string): (MenuItem | "sep")[] => {
    const i = tabs.findIndex((t) => t.id === id);
    const others = tabs.filter((t) => t.id !== id);
    const right = tabs.slice(i + 1);
    return [
      { label: "Close tab", icon: X, hint: shortcutLabel("closePane"), run: () => closeTab(id) },
      { label: "Close other tabs", icon: X, disabled: !others.length, run: () => others.forEach((t) => closeTab(t.id)) },
      { label: "Close tabs to the right", icon: X, disabled: !right.length, run: () => right.forEach((t) => closeTab(t.id)) },
      "sep",
      ...newItems(),
    ];
  };

  const move = (e: React.PointerEvent) => {
    const p = press.current;
    if (!p || (!p.moved && Math.abs(e.clientX - p.x) < 5)) return;
    p.moved = true;
    const others = (order ?? tabs.map((t) => t.id)).filter((id) => id !== p.id);
    let at = others.length;
    for (let i = 0; i < others.length; i++) {
      const r = els.current.get(others[i])?.getBoundingClientRect();
      if (r && e.clientX < r.left + r.width / 2) {
        at = i;
        break;
      }
    }
    setOrder([...others.slice(0, at), p.id, ...others.slice(at)]);
  };

  return (
    <div className="flex min-w-0 items-center gap-1">
      <div
        role="tablist"
        aria-label="Tabs"
        onWheel={(e) => {
          if (e.deltaY) e.currentTarget.scrollLeft += e.deltaY;
        }}
        className="flex h-7 min-w-0 items-center gap-0.5 overflow-x-auto rounded-[9px] bg-ink/[0.045] p-[3px] ring-1 ring-inset ring-ink/[0.06] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {shown.map((p) => {
          const active = p.id === focused;
          const { icon: Icon } = KIND[p.kind];
          const label = tabLabel(p, opened);
          const dragging = order !== null && press.current?.id === p.id;
          return (
            <div
              key={p.id}
              ref={(el) => {
                if (el) els.current.set(p.id, el);
                else els.current.delete(p.id);
              }}
              role="tab"
              tabIndex={0}
              aria-selected={active}
              title={`${label}. Drag to move, middle-click to close`}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                press.current = { id: p.id, x: e.clientX, moved: false };
                e.currentTarget.setPointerCapture(e.pointerId);
              }}
              onPointerMove={move}
              onPointerUp={() => {
                const p0 = press.current;
                press.current = null;
                if (p0?.moved && order) setTabOrder(order);
                else if (p0) focus(p.id);
                setOrder(null);
              }}
              onPointerCancel={() => {
                press.current = null;
                setOrder(null);
              }}
              onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), focus(p.id))}
              onMouseDown={(e) => e.button === 1 && e.preventDefault()}
              onAuxClick={(e) => e.button === 1 && closeTab(p.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, items: tabItems(p.id) });
              }}
              className={cn(
                "group flex h-[22px] max-w-[160px] shrink-0 cursor-default items-center gap-1.5 rounded-[6px] pr-1 pl-2.5 text-[12px] font-medium transition-colors select-none",
                active
                  ? "bg-ink/[0.11] text-fg shadow-[0_1px_2px_rgb(0_0_0/0.35),inset_0_1px_0_rgb(255_255_255/0.07)]"
                  : "text-fg-3 hover:bg-ink/[0.05] hover:text-fg-2",
                dragging && "ring-1 ring-accent/60",
              )}
            >
              <Icon size={12.5} strokeWidth={2} className="shrink-0" />
              <span className="truncate">{label}</span>
              {attention[p.id] && !active && <span className="size-1.5 shrink-0 rounded-full bg-accent" aria-label="Needs you" />}
              <button
                type="button"
                aria-label={`Close ${label}`}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(p.id);
                }}
                className={cn(
                  "flex size-4 shrink-0 items-center justify-center rounded-[4px] text-fg-3 hover:bg-ink/[0.1] hover:text-fg",
                  active ? "opacity-70" : "opacity-0 group-hover:opacity-70",
                )}
              >
                <X size={11} strokeWidth={2.2} />
              </button>
            </div>
          );
        })}
      </div>
      <button
        type="button"
        aria-label="New tab"
        title={`New tab (${shortcutLabel("newTerminal")} for a terminal)`}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: r.left, y: r.bottom + 4, items: newItems() });
        }}
        className="flex size-7 shrink-0 items-center justify-center rounded-[8px] text-fg-3 hover:bg-ink/[0.06] hover:text-fg"
      >
        <Plus size={14} />
      </button>
      {menu && <ContextMenu menu={menu} items={menu.items} onClose={() => setMenu(null)} />}
    </div>
  );
}
