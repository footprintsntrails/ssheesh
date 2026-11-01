import { Activity, BellRing, Box, Check, Columns2, Ellipsis, FolderOpen, Maximize2, Minimize2, PictureInPicture2, RotateCw, Rows2, SquareTerminal, X } from "lucide-react";
import { memo, useState, type PointerEvent as ReactPointerEvent } from "react";
import { ContextMenu, type Menu } from "./ContextMenu";
import type { DropZone } from "../lib/layout";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { panesIn, screenOf, useSideBySide, useStore, type PaneState } from "../lib/store";
import { useNames } from "../lib/privacy";
import { ENV_LABELS, useEnv } from "../lib/labels";
import { backend } from "../lib/backend";
import { useSettings } from "../lib/settings";
import { getTerminal } from "../lib/terminals";
import { resolveScheme } from "../lib/themes";
import { targetKey, type SessionState } from "../lib/types";
import { AgentButtons } from "./AgentIcons";
import { EmptyPane } from "./EmptyPane";
import { FilesPane } from "./FilesPane";
import { StatsPane } from "./StatsPane";
import { TerminalPane } from "./TerminalPane";
import { dragPane, tabLabel } from "./Tabs";
import { closeTab, showFiles, splitPane, toOwnTab } from "../lib/tabs";
import { EnvChip, Face, IconButton, moodOf } from "./ui";

/** "Needs you" when a program is waiting, "Went quiet" when a long run stopped. */
function AttentionPill({ id }: { id: string }) {
  const a = useStore((s) => s.attention[id]);
  const names = useNames();
  if (!a) return null;
  const input = a.kind === "input";
  return (
    <span
      title={a.message ? names.text(a.message) : (input ? "A program in this terminal is waiting for you" : "Output stopped after a long run. It may be done.")}
      className={cn(
        "inline-flex h-[20px] shrink-0 items-center gap-1 rounded-full px-2 text-[11px] font-semibold",
        input ? "bg-warn/15 text-warn" : "bg-ink/[0.08] text-fg-2",
      )}
    >
      {input ? <BellRing size={11} strokeWidth={2.2} /> : <Check size={11} strokeWidth={2.4} />}
      <span className="hidden @xs:inline">{input ? "Needs you" : "Went quiet"}</span>
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

/** Only when something's off: connecting, dropped, failed, or a server session tmux can't keep. */
function SessionPill({ state, local }: { state: SessionState | undefined; local: boolean }) {
  if (!state) return null;
  if (state.kind === "connected") {
    if (state.durable || local) return null;
    return (
      <span title="Install tmux on the server to keep sessions alive through disconnects" className="shrink-0 text-[11px] text-fg-3">
        not kept
      </span>
    );
  }
  const label = {
    connecting: state.kind === "connecting" && state.attempt > 1 ? "Reconnecting" : "Connecting",
    connected: "Connected",
    reconnecting: "Reconnecting",
    disconnected: "Disconnected",
    ended: "Ended",
    failed: "Failed",
  }[state.kind];
  return (
    <span
      aria-label={label}
      className="inline-flex h-[20px] shrink-0 items-center gap-1.5 text-[11.5px] font-medium text-fg-2"
    >
      <Face mood={moodOf(state)} />
      <span className="hidden @xs:inline">{label}</span>
    </span>
  );
}

const ICONS = { terminal: SquareTerminal, files: FolderOpen, stats: Activity, empty: SquareTerminal };

function PaneHeader({ pane, focused }: { pane: PaneState; focused: boolean }) {
  const env = useEnv(pane.target);
  const names = useNames();
  const state = useStore((s) => s.sessions[pane.id]);
  const toast = useStore((s) => s.toast);
  const Icon = ICONS[pane.kind];
  const toggleZoom = useStore((s) => s.toggleZoom);
  const [menu, setMenu] = useState<Menu | null>(null);
  const zoomed = useStore((s) => s.zoomed === pane.id);
  const command = useStore((s) => s.foreground[pane.id]);
  const screen = useStore((s) => screenOf(s, pane.id));
  const tabs = !useSideBySide(screen);
  // With others beside it, × only takes the pane off the screen: it goes back
  // to being a tab, still running. Closing the tab is in ⋯ and the tab list;
  // a pane on its own closes with ×.
  const beside = useStore((s) => panesIn(s, screen).filter((p) => !s.hidden.includes(p.id)).length > 1);
  // Beside another server's tab, each says whose it is.
  const mixed = useStore((s) => !tabs && new Set(panesIn(s, screen).filter((p) => !s.hidden.includes(p.id)).map((p) => (p.target ? targetKey(p.target) : ""))).size > 1);
  const leave = !tabs && beside;

  // Drag a pane by its title bar onto another pane: a side to dock there,
  // the middle to swap places.
  const startDrag = (e: ReactPointerEvent) => {
    if (!(e.target as HTMLElement).closest("button,input")) dragPane(e, pane.id);
  };

  const openFilesHere = async () => {
    if (!pane.target) return;
    let path = "~";
    if (pane.target.kind === "host" && pane.sessionKey) {
      path = await backend.sessionCwd(pane.target, pane.sessionKey).catch(() => "~");
    }
    showFiles(pane.target, path);
    if (path === "~" && pane.target.kind === "host") {
      toast({ tone: "info", title: "Opened your home folder", body: "Following the terminal's folder needs tmux on the server." });
    }
  };

  return (
    <div
      onPointerDown={startDrag}
      onDoubleClick={(e) => !tabs && !(e.target as HTMLElement).closest("button") && toggleZoom(pane.id)}
      data-hint={pane.kind === "terminal" ? "terminal" : undefined}
      title={tabs ? "Drag onto the page to show it beside another tab" : "Drag to move, onto the tab list to make it a tab again; double-click to fill the window"}
      className={cn(
        "@container group/header relative flex h-9 shrink-0 items-center gap-2 border-b pr-1 pl-2 @2xs:pr-1.5 @2xs:pl-3",
        pane.kind === "terminal" ? "border-ink/[0.06]" : "border-line",
      )}
    >
      {pane.kind !== "terminal" && <Icon size={13.5} strokeWidth={1.9} className={cn("shrink-0 @max-[9rem]:hidden", focused ? "text-fg-2" : "text-fg-3")} />}
      {pane.container && <Box size={13.5} strokeWidth={1.9} className="shrink-0 text-accent" />}
      <span className={cn("min-w-0 shrink truncate text-[12.5px] font-medium", focused ? "text-fg" : "text-fg-2")}>
        {pane.kind === "empty" ? "New pane" : tabLabel(pane, command, mixed && !pane.container ? names.target(pane.target) : undefined)}
      </span>
      {pane.container && (
        <span className="hidden min-w-0 truncate text-[11.5px] text-fg-3 @sm:inline">
          {pane.container.run === "logs" ? "logs on" : "on"} {names.target(pane.target)}
        </span>
      )}
      {pane.kind !== "empty" && <EnvChip env={env} className="hidden @2xs:inline" />}
      {pane.kind === "terminal" && <AttentionPill id={pane.id} />}
      {pane.kind === "terminal" && (
        <span className="contents @max-[10rem]:hidden">
          <SessionPill state={state} local={pane.target?.kind === "local"} />
        </span>
      )}
      {/* Quiet until you reach for them: the agents' own colors would sit over every terminal. */}
      {pane.kind === "terminal" && (
        <span className="flex opacity-0 transition-opacity group-hover/header:opacity-100 focus-within:opacity-100 @max-[24rem]:hidden">
          <AgentButtons pane={pane} connected={state?.kind === "connected"} />
        </span>
      )}
      {pane.kind === "terminal" && <ProgressLine id={pane.id} />}
      {pane.kind === "terminal" && state?.kind === "disconnected" && (
        <button
          type="button"
          onClick={() => getTerminal(pane).reconnectNow()}
          title="Reconnect"
          className="flex h-6 shrink-0 items-center gap-1 rounded-[6px] bg-accent px-2 text-[11.5px] font-medium text-on-accent hover:brightness-110"
        >
          <RotateCw size={11.5} /> <span className="hidden @xs:inline">Reconnect</span>
        </button>
      )}
      {pane.kind === "files" && pane.path && (
        <span className="hidden min-w-0 truncate font-mono text-[11px] text-fg-3 @sm:inline">{pane.path}</span>
      )}
      <div className="flex-1" />
      <div className="flex shrink-0 items-center">
        {/* Wide enough: every button. Narrow: they fold into ⋯, and Close stays put. */}
        <div className={cn("hidden items-center transition-opacity @sm:flex", focused ? "opacity-100" : "opacity-0 group-hover/pane:opacity-100")}>
          {pane.kind === "terminal" && (
            <IconButton label="Browse files in this folder" onClick={openFilesHere} className="size-6">
              <FolderOpen size={13.5} />
            </IconButton>
          )}
          {/* As tabs, every pane fills the window already. */}
          {!tabs && (
            <>
              <IconButton label="Split right" onClick={() => splitPane(pane.id, "row")} className="size-6">
                <Columns2 size={13.5} />
              </IconButton>
              <IconButton label="Split down" onClick={() => splitPane(pane.id, "col")} className="size-6">
                <Rows2 size={13.5} />
              </IconButton>
              {!leave && (
                <IconButton label="Back to its own tab" onClick={() => toOwnTab(pane.id)} className="size-6">
                  <PictureInPicture2 size={13} />
                </IconButton>
              )}
              <IconButton
                label={`${zoomed ? "Show all panes" : "Fill the window"} (${shortcutLabel("zoomPane")})`}
                onClick={() => toggleZoom(pane.id)}
                className="size-6"
              >
                {zoomed ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
              </IconButton>
            </>
          )}
        </div>
        <span className={cn("@sm:hidden", tabs && pane.kind !== "terminal" && "hidden")}>
          <IconButton
            label="More"
            onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              setMenu({ x: r.left, y: r.bottom + 4 });
            }}
            className="size-6"
          >
            <Ellipsis size={14} />
          </IconButton>
        </span>
        <IconButton
          label={leave ? "Stop showing side by side (the tab stays open)" : "Close pane"}
          onClick={() => (leave ? toOwnTab(pane.id) : closeTab(pane.id))}
          className="size-6"
        >
          <X size={14} />
        </IconButton>
      </div>
      {menu && (
        <ContextMenu
          menu={menu}
          onClose={() => setMenu(null)}
          items={[
            ...(pane.kind === "terminal" ? [{ label: "Browse files in this folder", icon: FolderOpen, run: () => void openFilesHere() }] : []),
            ...(tabs
              ? []
              : [
                  { label: "Split right", icon: Columns2, run: () => splitPane(pane.id, "row") },
                  { label: "Split down", icon: Rows2, run: () => splitPane(pane.id, "col") },
                  { label: zoomed ? "Show all panes" : "Fill the window", icon: zoomed ? Minimize2 : Maximize2, hint: shortcutLabel("zoomPane"), run: () => toggleZoom(pane.id) },
                  { label: "Back to its own tab", icon: PictureInPicture2, run: () => toOwnTab(pane.id) },
                  ...(leave ? [{ label: "Close tab", icon: X, hint: shortcutLabel("closePane"), run: () => closeTab(pane.id) }] : []),
                ]),
          ]}
        />
      )}
    </div>
  );
}

/** Draws one pane; it only redraws when something about this pane changes. */
export const PaneView = memo(function PaneView({ id }: { id: string }) {
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
});

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
