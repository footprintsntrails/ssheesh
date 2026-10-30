import { Activity, BellRing, Box, Check, Columns2, FolderOpen, Maximize2, Minimize2, RotateCw, Rows2, ShieldCheck, SquareTerminal, X } from "lucide-react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { DropZone } from "../lib/layout";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { useStore, type PaneState } from "../lib/store";
import { useNames } from "../lib/privacy";
import { ENV_LABELS, useEnv } from "../lib/labels";
import { backend } from "../lib/backend";
import { useSettings } from "../lib/settings";
import { getTerminal } from "../lib/terminals";
import { resolveScheme } from "../lib/themes";
import type { SessionState } from "../lib/types";
import { EmptyPane } from "./EmptyPane";
import { FilesPane } from "./FilesPane";
import { StatsPane } from "./StatsPane";
import { TerminalPane } from "./TerminalPane";
import { EnvChip, Face, IconButton, moodOf } from "./ui";

/** "Needs you" when a program is waiting, "Went quiet" when a long run stopped. */
function AttentionPill({ id }: { id: string }) {
  const a = useStore((s) => s.attention[id]);
  if (!a) return null;
  const input = a.kind === "input";
  return (
    <span
      title={a.message ?? (input ? "A program in this terminal is waiting for you" : "Output stopped after a long run. It may be done.")}
      className={cn(
        "inline-flex h-[20px] shrink-0 items-center gap-1 rounded-full px-2 text-[11px] font-semibold",
        input ? "bg-accent text-white" : "bg-ink/[0.08] text-fg-2",
      )}
    >
      {input ? <BellRing size={11} strokeWidth={2.2} /> : <Check size={11} strokeWidth={2.4} />}
      {input ? "Needs you" : "Went quiet"}
    </span>
  );
}

/** A thin bar under the title while a program reports progress. */
function ProgressLine({ id }: { id: string }) {
  const p = useStore((s) => s.progress[id]);
  if (!p) return null;
  const color = p.state === "error" ? "bg-bad" : p.state === "paused" ? "bg-warn" : "bg-accent";
  return (
    <span aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-[2px] overflow-hidden">
      <span
        className={cn("block h-full transition-[width] duration-300", color, p.state === "indeterminate" && "w-1/3 animate-pulse")}
        style={p.state === "indeterminate" ? undefined : { width: `${p.value}%` }}
      />
    </span>
  );
}

function SessionPill({ state, local }: { state: SessionState | undefined; local: boolean }) {
  if (!state) return null;
  const label = {
    connecting: state.kind === "connecting" && state.attempt > 1 ? "Reconnecting" : "Connecting",
    connected: "Connected",
    reconnecting: "Reconnecting",
    disconnected: "Disconnected",
    ended: "Ended",
    failed: "Failed",
  }[state.kind];
  const durable = state.kind === "connected" && state.durable;
  const tip = durable
    ? "Durable session: it keeps running on the server if the connection drops"
    : state.kind === "connected" && !local
      ? "Install tmux on the server to keep sessions alive through disconnects"
      : undefined;
  return (
    <span
      title={tip}
      className="inline-flex h-[20px] items-center gap-1.5 text-[11.5px] font-medium text-fg-2"
    >
      <Face mood={moodOf(state)} />
      {label}
      {durable && <ShieldCheck size={11.5} className="-mr-0.5 text-ok/80" />}
    </span>
  );
}

const ICONS = { terminal: SquareTerminal, files: FolderOpen, stats: Activity, empty: SquareTerminal };

function PaneHeader({ pane, focused }: { pane: PaneState; focused: boolean }) {
  const env = useEnv(pane.target);
  const names = useNames();
  const state = useStore((s) => s.sessions[pane.id]);
  const split = useStore((s) => s.split);
  const closePane = useStore((s) => s.closePane);
  const openIn = useStore((s) => s.openIn);
  const toast = useStore((s) => s.toast);
  const Icon = ICONS[pane.kind];
  const toggleZoom = useStore((s) => s.toggleZoom);
  const zoomed = useStore((s) => s.zoomed === pane.id);

  // Drag a pane by its title bar onto another pane: a side to dock there,
  // the middle to swap places.
  const startDrag = (e: ReactPointerEvent) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button,input")) return;
    const sx = e.clientX;
    const sy = e.clientY;
    let dragging = false;
    const move = (ev: PointerEvent) => {
      const st = useStore.getState();
      if (!dragging) {
        if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
        dragging = true;
        st.setDrag(pane.id);
        document.body.style.cursor = "grabbing";
      }
      const el = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>("[data-pane-id]");
      const id = el?.dataset.paneId;
      if (!el || !id || id === pane.id) return st.setDropTarget(null);
      const r = el.getBoundingClientRect();
      const x = (ev.clientX - r.left) / r.width;
      const y = (ev.clientY - r.top) / r.height;
      const zone: DropZone = x < 0.25 ? "left" : x > 0.75 ? "right" : y < 0.3 ? "top" : y > 0.7 ? "bottom" : "center";
      st.setDropTarget({ paneId: id, zone });
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.style.cursor = "";
      if (!dragging) return;
      const st = useStore.getState();
      if (st.dropTarget) st.movePane(pane.id, st.dropTarget.paneId, st.dropTarget.zone);
      st.setDrag(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const openFilesHere = async () => {
    if (!pane.target) return;
    let path = "~";
    if (pane.target.kind === "host" && pane.sessionKey) {
      path = await backend.sessionCwd(pane.target, pane.sessionKey).catch(() => "~");
    }
    openIn("files", pane.target, { from: pane.id, split: true, path });
    if (path === "~" && pane.target.kind === "host") {
      toast({ tone: "info", title: "Opened your home folder", body: "Following the terminal's folder needs tmux on the server." });
    }
  };

  return (
    <div
      onPointerDown={startDrag}
      onDoubleClick={(e) => !(e.target as HTMLElement).closest("button") && toggleZoom(pane.id)}
      title="Drag to move, double-click to fill the window"
      className={cn(
        "relative flex h-9 shrink-0 items-center gap-2 border-b pl-3 pr-1.5",
        pane.kind === "terminal" ? "border-ink/[0.06]" : "border-line",
      )}
    >
      {pane.kind !== "terminal" && <Icon size={13.5} strokeWidth={1.9} className={focused ? "text-fg-2" : "text-fg-3"} />}
      {pane.container && <Box size={13.5} strokeWidth={1.9} className="shrink-0 text-accent" />}
      <span className={cn("max-w-[60%] shrink-0 truncate text-[12.5px] font-medium", focused ? "text-fg" : "text-fg-2")}>
        {pane.kind === "empty" ? "New pane" : pane.container ? pane.container.name : names.target(pane.target)}
      </span>
      {pane.container && (
        <span className="min-w-0 truncate text-[11.5px] text-fg-3">
          {pane.container.run === "logs" ? "logs on" : "on"} {names.target(pane.target)}
        </span>
      )}
      {pane.kind !== "empty" && <EnvChip env={env} />}
      {pane.kind === "terminal" && <AttentionPill id={pane.id} />}
      {pane.kind === "terminal" && <SessionPill state={state} local={pane.target?.kind === "local"} />}
      {pane.kind === "terminal" && <ProgressLine id={pane.id} />}
      {pane.kind === "terminal" && state?.kind === "disconnected" && (
        <button
          type="button"
          onClick={() => getTerminal(pane).reconnectNow()}
          className="flex h-6 items-center gap-1 rounded-[6px] bg-accent px-2 text-[11.5px] font-medium text-white hover:brightness-110"
        >
          <RotateCw size={11.5} /> Reconnect
        </button>
      )}
      {pane.kind === "files" && pane.path && (
        <span className="truncate font-mono text-[11px] text-fg-3">{pane.path}</span>
      )}
      <div className="flex-1" />
      <div className={cn("flex items-center transition-opacity", focused ? "opacity-100" : "opacity-0 group-hover/pane:opacity-100")}>
        {pane.kind === "terminal" && (
          <IconButton label="Browse files in this folder" onClick={openFilesHere} className="size-6">
            <FolderOpen size={13.5} />
          </IconButton>
        )}
        <IconButton label="Split right" onClick={() => split(pane.id, "row")} className="size-6">
          <Columns2 size={13.5} />
        </IconButton>
        <IconButton label="Split down" onClick={() => split(pane.id, "col")} className="size-6">
          <Rows2 size={13.5} />
        </IconButton>
        <IconButton
          label={`${zoomed ? "Show all panes" : "Fill the window"} (${shortcutLabel("zoomPane")})`}
          onClick={() => toggleZoom(pane.id)}
          className="size-6"
        >
          {zoomed ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
        </IconButton>
        <IconButton label="Close pane" onClick={() => closePane(pane.id)} className="size-6">
          <X size={14} />
        </IconButton>
      </div>
    </div>
  );
}

export function PaneView({ id }: { id: string }) {
  const pane = useStore((s) => s.panes[id]);
  const focused = useStore((s) => s.focused === id);
  const many = useStore((s) => Object.keys(s.panes).length > 1);
  const env = useEnv(useStore((s) => s.panes[id]?.target));
  const dim = useSettings((s) => s.dimInactive);
  const focus = useStore((s) => s.focus);
  const drop = useStore((s) => (s.dropTarget?.paneId === id ? s.dropTarget.zone : null));
  const terminalBg = useSettings((s) => resolveScheme(s.terminalScheme, s.theme, s.accent).background);
  const dragging = useStore((s) => s.dragPane === id);
  if (!pane) return null;

  return (
    <section
      data-pane-id={id}
      data-pane-kind={pane.kind}
      style={pane.kind === "terminal" ? { background: terminalBg } : undefined}
      onMouseDownCapture={() => focus(id)}
      className={cn(
        "group/pane relative flex h-full w-full flex-col overflow-hidden rounded-[var(--ui-radius)] transition-opacity",
        pane.kind === "terminal" ? "" : "bg-ink/[0.012]",
        dragging && "opacity-45",
      )}
    >
      <PaneHeader pane={pane} focused={focused} />
      <div className="relative min-h-0 flex-1">
        {pane.kind === "empty" && <EmptyPane pane={pane} focused={focused} />}
        {pane.kind === "terminal" && <TerminalPane pane={pane} focused={focused} />}
        {pane.kind === "files" && pane.target && <FilesPane pane={pane} target={pane.target} focused={focused} />}
        {pane.kind === "stats" && pane.target && <StatsPane pane={pane} target={pane.target} />}
      </div>
      {!focused && many && dim > 0 && (
        <div aria-hidden className="pointer-events-none absolute inset-0 z-10 bg-window transition-opacity" style={{ opacity: dim / 100 }} />
      )}
      {/* The frame sits on top so split containers can't clip it. A terminal
          reads as its own block: its own background and a crisp edge. */}
      <div
        aria-hidden
        className={cn(
          "pointer-events-none absolute inset-0 z-20 rounded-[var(--ui-radius)] ring-inset transition-shadow",
          env
            ? cn(focused && many ? "ring-[1.5px]" : "ring-1", ENV_LABELS[env].ring)
            : pane.kind === "terminal"
            ? focused && many
              ? "ring-1 ring-ink/[0.28] shadow-[inset_0_1px_0_rgb(255_255_255/0.06)]"
              : "ring-1 ring-ink/[0.14] shadow-[inset_0_1px_0_rgb(255_255_255/0.05)]"
            : focused && many
              ? "ring-1 ring-ink/[0.16]"
              : "ring-1 ring-ink/[0.07]",
        )}
      />
      {drop && <DropOverlay zone={drop} />}
    </section>
  );
}

const ZONES: Record<DropZone, string> = {
  left: "inset-y-1.5 left-1.5 w-[calc(50%-6px)]",
  right: "inset-y-1.5 right-1.5 w-[calc(50%-6px)]",
  top: "inset-x-1.5 top-1.5 h-[calc(50%-6px)]",
  bottom: "inset-x-1.5 bottom-1.5 h-[calc(50%-6px)]",
  center: "inset-1.5",
};

function DropOverlay({ zone }: { zone: DropZone }) {
  return (
    <div className="pointer-events-none absolute inset-0 z-30">
      <div className={cn("absolute rounded-[calc(var(--ui-radius)-4px)] bg-accent/15 ring-2 ring-accent/70 transition-all duration-150", ZONES[zone])}>
        <span className="glass absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full px-3 py-1 text-[12px] font-medium whitespace-nowrap text-fg">
          {zone === "center" ? "Swap places" : "Move here"}
        </span>
      </div>
    </div>
  );
}
