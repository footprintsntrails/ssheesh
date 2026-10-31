// The main menu: SSHeesh's new-tab page. Search in the middle, your servers as tiles,
// the agents working right now on any of them, and what you closed lately.
// It shows on launch, when nothing is open, and from the logo at the top of
// the sidebar.

import { FileUp, FolderOpen, Laptop, Plus, RotateCcw, Search, SquareTerminal } from "lucide-react";
import { useState, type MouseEvent } from "react";
import { agentName, agentRunning } from "../lib/agents";
import { ago } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { useEnv, useHostLook, useUndecidedConfig, useVisibleHosts } from "../lib/labels";
import { useNames } from "../lib/privacy";
import { hostAddress, useMachine, useStore, type PaneState } from "../lib/store";
import type { TargetRef } from "../lib/types";
import { AgentIcon } from "./AgentIcons";
import { useConfigImport } from "./ConfigImport";
import { ContextMenu, type Menu } from "./ContextMenu";
import { HostIcon } from "./HostIcon";
import { TabIcon, tabLabel, toolItems } from "./Tabs";
import { AppLogo, EnvChip, Kbd } from "./ui";
import { showCurrentNotes } from "./WhatsNew";
import { useUpdates } from "../lib/updates";

/** Needs you, or open, at a glance: shapes, not just colors. */
function TileState({ target }: { target: TargetRef }) {
  const { open, waiting } = useMachine(target);
  if (waiting) return <span aria-label="Needs you" className="absolute -top-1 -right-1 size-3 rounded-full bg-warn ring-[3px] ring-[var(--ui-canvas)]" />;
  if (open)
    return (
      <span aria-label={`${open} open`} className="absolute -right-1.5 -bottom-1.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-raised px-1 text-[10.5px] font-semibold text-fg-2 ring-1 ring-ink/[0.12] tabular-nums">
        {open}
      </span>
    );
  return null;
}

/** A server as a shortcut tile, like the ones on a browser's new tab. */
function Tile({ target, name, address, hostId }: { target: TargetRef; name: string; address?: string; hostId?: string }) {
  const look = useHostLook(hostId);
  const env = useEnv(target);
  const goTo = useStore((s) => s.goTo);
  const [menu, setMenu] = useState<Menu | null>(null);
  // A picture keeps its own colors, on a plain tile.
  const tint = target.kind === "local" || look.image ? undefined : (look.color ?? undefined);
  return (
    <>
      <button
        type="button"
        title={address}
        onClick={() => goTo(target)}
        onContextMenu={(e: MouseEvent) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
        className="group flex w-[104px] flex-col items-center gap-2 rounded-[14px] px-1 pt-3.5 pb-2.5 outline-none transition-colors hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06] focus-visible:ring-2 focus-visible:ring-accent/60"
      >
        <span
          className="relative flex size-12 items-center justify-center rounded-[13px] ring-1 ring-inset ring-ink/[0.08]"
          style={{ background: tint ? `color-mix(in srgb, ${tint} 16%, transparent)` : "rgb(var(--ui-ink) / 0.06)" }}
        >
          {target.kind === "local" ? <Laptop size={22} strokeWidth={1.8} className="text-fg-2" /> : <HostIcon hostId={hostId} size={22} strokeWidth={1.8} />}
          <TileState target={target} />
        </span>
        <span className="flex max-w-full items-center gap-1">
          <span className="truncate text-[12.5px] text-fg">{name}</span>
        </span>
        {env && <EnvChip env={env} className="-mt-1" />}
      </button>
      {menu && <ContextMenu menu={menu} items={toolItems(target, address)} onClose={() => setMenu(null)} />}
    </>
  );
}

/** Agents at work and terminals waiting for you, on every server. */
function RunningNow() {
  const panes = useStore((s) => s.panes);
  const foreground = useStore((s) => s.foreground);
  const attention = useStore((s) => s.attention);
  const focus = useStore((s) => s.focus);
  const names = useNames();
  const rows = Object.values(panes).filter((p) => p.kind === "terminal" && (agentRunning(foreground[p.id]) || attention[p.id]?.kind === "input"));
  if (!rows.length) return null;
  return (
    <section className="flex flex-col gap-1">
      <h2 className="px-2 text-[11.5px] font-semibold text-fg-3">Running now</h2>
      {rows.map((p: PaneState) => {
        const agent = agentRunning(foreground[p.id]);
        const waiting = attention[p.id]?.kind === "input";
        return (
          <button
            key={p.id}
            type="button"
            onClick={() => focus(p.id)}
            className="flex h-9 items-center gap-2.5 rounded-[8px] px-2 text-left outline-none hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06]"
          >
            {agent ? <AgentIcon id={agent} size={15} /> : <SquareTerminal size={15} className="text-fg-3" />}
            <span className="min-w-0 flex-1 truncate text-[13px] text-fg">
              {agent ? agentName(agent) : "Terminal"} <span className="text-fg-3">on {names.target(p.target)}</span>
            </span>
            {waiting ? <span className="text-[11.5px] font-medium text-warn">Needs you</span> : <span className="text-[11.5px] text-fg-3">working</span>}
          </button>
        );
      })}
    </section>
  );
}

/** Tabs you closed lately; a click brings one back. */
function RecentlyClosed() {
  const closed = useStore((s) => s.closedTabs);
  const reopen = useStore((s) => s.reopenClosed);
  const names = useNames();
  if (!closed.length) return null;
  return (
    <section className="flex flex-col gap-1">
      <h2 className="flex items-center justify-between px-2 text-[11.5px] font-semibold text-fg-3">
        Recently closed <span className="font-normal">{shortcutLabel("reopenTab")} brings back the last one</span>
      </h2>
      {closed.slice(0, 5).map((c, i) => (
        <button
          key={c.closedAt + c.kind + i}
          type="button"
          onClick={() => reopen(i)}
          className="flex h-9 items-center gap-2.5 rounded-[8px] px-2 text-left outline-none hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06]"
        >
          <TabIcon pane={c} size={15} className="text-fg-3" />
          <span className="min-w-0 flex-1 truncate text-[13px] text-fg">
            {tabLabel(c)}{" "}
            <span className="text-fg-3">
              on {names.target(c.target)}
              {c.path ? ` · ${names.text(c.path)}` : ""}
            </span>
          </span>
          <span className="text-[11.5px] text-fg-3">{ago(c.closedAt)}</span>
          <RotateCcw size={13} className="text-fg-3" />
        </button>
      ))}
    </section>
  );
}

export function Home() {
  const version = useUpdates((u) => u.current);
  const hosts = useVisibleHosts();
  const undecided = useUndecidedConfig();
  const names = useNames();
  const setPaletteOpen = useStore((s) => s.setPaletteOpen);
  const setAddHostOpen = useStore((s) => s.setAddHostOpen);
  const showImport = useConfigImport((s) => s.show);

  return (
    <div className="h-full overflow-y-auto rounded-[var(--ui-radius)] bg-canvas ring-1 ring-inset ring-ink/[0.07]">
      <div className="mx-auto flex min-h-full w-full max-w-[720px] flex-col px-6 pt-[9vh] pb-10">
        <div className="flex flex-col items-center">
          <AppLogo size={44} />
          <h1 className="mt-3 font-serif text-[30px] italic leading-none text-fg">Where to?</h1>
          <button
            type="button"
            onClick={() => setPaletteOpen(true)}
            className="mt-6 flex h-11 w-full max-w-[520px] items-center gap-2.5 rounded-[12px] bg-ink/[0.05] px-4 text-left text-[14px] text-fg-3 ring-1 ring-inset ring-ink/[0.09] transition-colors hover:bg-ink/[0.07]"
          >
            <Search size={16} strokeWidth={2} />
            <span className="flex-1 truncate">Servers, files, commands and settings</span>
            <Kbd>{shortcutLabel("palette")}</Kbd>
          </button>
        </div>

        <div className="mt-9 flex flex-wrap justify-center gap-1">
          <Tile target={{ kind: "local" }} name="This computer" />
          {hosts.map((h) => (
            <Tile key={h.id} target={{ kind: "host", hostId: h.id }} hostId={h.id} name={names.host(h)} address={names.on ? undefined : hostAddress(h)} />
          ))}
          <button
            type="button"
            onClick={() => setAddHostOpen(true)}
            className="flex w-[104px] flex-col items-center gap-2 rounded-[14px] px-1 pt-3.5 pb-2.5 outline-none hover:bg-ink/[0.05] focus-visible:ring-2 focus-visible:ring-accent/60"
          >
            <span className="flex size-12 items-center justify-center rounded-[13px] text-fg-3 ring-1 ring-ink/[0.12] ring-inset [border-style:dashed]">
              <Plus size={20} strokeWidth={1.8} />
            </span>
            <span className="text-[12.5px] text-fg-2">Add server</span>
          </button>
        </div>

        {undecided.length > 0 && (
          <button
            type="button"
            onClick={() => showImport("new")}
            className="mx-auto mt-3 flex items-center gap-2 rounded-full bg-accent/10 px-3.5 py-1.5 text-[12.5px] font-medium text-accent hover:bg-accent/15"
          >
            <FileUp size={13.5} /> {undecided.length} server{undecided.length === 1 ? "" : "s"} in ~/.ssh/config to pick from
          </button>
        )}

        <div className="mt-9 flex flex-col gap-6">
          <RunningNow />
          <RecentlyClosed />
        </div>

        <div className="mt-auto flex flex-wrap items-center justify-center gap-x-5 gap-y-2 pt-10 text-[11.5px] text-fg-3">
          <span className="inline-flex items-center gap-1.5">
            <Kbd>{shortcutLabel("newTerminal")}</Kbd> new terminal
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Kbd>{shortcutLabel("toggleSplit")}</Kbd> tabs side by side
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Kbd>{shortcutLabel("reopenTab")}</Kbd> reopen closed tab
          </span>
          <span className="inline-flex items-center gap-1.5">
            <FolderOpen size={12} /> right-click a server for its tools
          </span>
        </div>
        <button type="button" onClick={showCurrentNotes} title="What's new in this version" className="mx-auto mt-3 text-[11px] text-fg-3 hover:text-fg-2">
          SSHeesh {version}
        </button>
      </div>
    </div>
  );
}
