// A connected server's load in the sidebar: two small meters under its
// name in the list, a gauge on its icon in the rail, and a card with the
// numbers when you hover an icon there.

import { TriangleAlert } from "lucide-react";
import type { CSSProperties } from "react";
import { cn, percent, uptime } from "../lib/format";
import { useMetrics } from "../lib/metrics";
import type { EnvLabel } from "../lib/labels";
import type { TargetRef } from "../lib/types";
import { memFraction, Sparkline } from "./StatsPane";
import { EnvChip, type Health } from "./ui";

/** Past this, a disk or memory gets a warning line of its own. */
const FULL = 0.9;

interface Load {
  cpu: number;
  mem: number;
  disk: number;
  history: number[];
  uptimeSecs: number;
  warning: string | null;
}

function useLoad(target: TargetRef): Load | null {
  const { metrics: m, samples } = useMetrics(target);
  if (!m) return null;
  const mem = memFraction(m);
  const fullest = m.disks.reduce<{ mount: string; used: number } | null>((a, d) => {
    const used = d.size ? d.used / d.size : 0;
    return !a || used > a.used ? { mount: d.mount, used } : a;
  }, null);
  const disk = fullest?.used ?? 0;
  const warning =
    disk >= FULL
      ? `${fullest?.mount === "/" ? "Disk" : fullest?.mount} ${percent(disk)} full`
      : mem >= FULL
        ? `Memory ${percent(mem)} used`
        : null;
  return {
    cpu: samples[samples.length - 1]?.cpu ?? 0,
    mem,
    disk,
    history: samples.map((s) => s.cpu),
    uptimeSecs: m.uptimeSecs,
    warning,
  };
}

/** A thin meter in the accent color, amber once it's nearly full. */
function Gauge({ value, height }: { value: number; height: number }) {
  const v = Math.max(0, Math.min(1, value));
  return (
    <span className="block w-full overflow-hidden rounded-full bg-ink/[0.08]" style={{ height }}>
      <span className={cn("block h-full rounded-full transition-[width] duration-700", v >= FULL ? "bg-warn" : "bg-accent")} style={{ width: `${v * 100}%` }} />
    </span>
  );
}

function Warning({ text, className }: { text: string; className?: string }) {
  return (
    <span className={cn("flex min-w-0 items-center gap-1 text-warn", className)}>
      <TriangleAlert size={11} strokeWidth={2.2} className="shrink-0" />
      <span className="truncate">{text}</span>
    </span>
  );
}

/** CPU and memory under a connected server's name in the list. */
export function LoadLine({ target }: { target: TargetRef }) {
  const load = useLoad(target);
  const summary = load && `CPU ${percent(load.cpu)} · Memory ${percent(load.mem)} · Disk ${percent(load.disk)}`;
  return (
    <span title={summary ?? undefined} className="flex flex-col gap-1.5 pb-2 pl-[23px] text-[10.5px] leading-none text-fg-3">
      <span className="flex items-center gap-1.5">
        <span>CPU</span>
        <span className="min-w-0 flex-1">
          <Gauge value={load?.cpu ?? 0} height={3} />
        </span>
        <span className="ml-1">RAM</span>
        <span className="min-w-0 flex-1">
          <Gauge value={load?.mem ?? 0} height={3} />
        </span>
      </span>
      {load?.warning && <Warning text={load.warning} className="text-[11px]" />}
    </span>
  );
}

export type RailStatus = "frame" | "bars" | "pulse" | "count";

const tone = (load: Load | null) => (load?.warning ? "var(--ui-warn)" : "var(--ui-accent)");

/** The rail button's rounded square, traced clockwise from the top middle. */
const FRAME = "M20 1.25H29.75A9 9 0 0 1 38.75 10.25V29.75A9 9 0 0 1 29.75 38.75H10.25A9 9 0 0 1 1.25 29.75V10.25A9 9 0 0 1 10.25 1.25Z";

/**
 * The load drawn on a server's 40px rail button: a border that fills with
 * CPU, upright meters for CPU, memory and disk, a CPU pulse under the icon,
 * or the number of open panes. Amber means a disk or memory is nearly full;
 * the count turns blue while a terminal there waits for you.
 */
export function RailLoad({ target, style, panes, waiting }: { target: TargetRef; style: RailStatus; panes: number; waiting: boolean }) {
  const load = useLoad(target);
  if (style === "count") {
    return (
      <span
        className={cn(
          "pointer-events-none absolute -right-[3px] -bottom-[3px] flex h-[15px] min-w-[15px] items-center justify-center rounded-full px-[4px] text-[10px] font-semibold tabular-nums text-black/75 ring-2 ring-[var(--ui-window)]",
          waiting ? "bg-accent" : load?.warning ? "bg-warn" : "bg-ok",
        )}
      >
        {panes}
      </span>
    );
  }
  if (style === "bars") {
    return (
      <span className="pointer-events-none absolute top-1/2 right-[4px] flex h-5 -translate-y-1/2 gap-[1.5px]">
        {[load?.cpu ?? 0, load?.mem ?? 0, load?.disk ?? 0].map((v, i) => (
          <span key={i} className="flex w-[3px] items-end overflow-hidden rounded-full bg-ink/[0.12]">
            <span
              className={cn("w-full rounded-full transition-[height] duration-700", v >= FULL ? "bg-warn" : "bg-accent")}
              style={{ height: `${Math.max(12, v * 100)}%` }}
            />
          </span>
        ))}
      </span>
    );
  }
  if (style === "pulse") {
    const history = load?.history ?? [];
    // Scaled to the recent peak so a calm server still shows a heartbeat.
    const max = Math.min(1, Math.max(0.2, ...history) * 1.3);
    return (
      <span className="pointer-events-none absolute bottom-[10px] left-1/2 w-[26px] -translate-x-1/2">
        <Sparkline series={history} max={max} color={tone(load)} height={9} width={26} />
      </span>
    );
  }
  const filled = Math.max(1, (load?.cpu ?? 0) * 100);
  return (
    <svg viewBox="0 0 40 40" className="pointer-events-none absolute inset-0 size-10" aria-hidden>
      <path d={FRAME} fill="none" strokeWidth="2.5" strokeOpacity={load?.warning ? 0.3 : 0.08} style={{ stroke: load?.warning ? "var(--ui-warn)" : "rgb(var(--ui-ink))" }} />
      <path
        d={FRAME}
        fill="none"
        pathLength={100}
        strokeDasharray={`${filled} 100`}
        strokeLinecap="round"
        strokeWidth="2.5"
        className="transition-[stroke-dasharray] duration-700"
        style={{ stroke: tone(load) }}
      />
    </svg>
  );
}

const STATUS: Record<Health, string> = {
  idle: "Not open. Click for a terminal.",
  busy: "Connecting…",
  live: "Connected",
  warn: "Connection lost, reconnecting…",
  bad: "Couldn't connect",
};

function CardLoad({ target }: { target: TargetRef }) {
  const load = useLoad(target);
  const rows: [string, number][] = [
    ["CPU", load?.cpu ?? 0],
    ["Memory", load?.mem ?? 0],
    ["Disk", load?.disk ?? 0],
  ];
  return (
    <>
      <div className="mt-3 grid grid-cols-[auto_1fr_auto] items-center gap-x-2.5 gap-y-2 text-[11.5px]">
        {rows.map(([label, v]) => (
          <div key={label} className="contents">
            <span className="text-fg-3">{label}</span>
            <Gauge value={v} height={4} />
            <span className="w-8 text-right tabular-nums text-fg-2">{load ? percent(v) : "…"}</span>
          </div>
        ))}
      </div>
      {load?.warning && <Warning text={load.warning} className="mt-2.5 text-[11.5px]" />}
      {load && <div className="mt-2.5 text-[11px] text-fg-3">Up {uptime(load.uptimeSecs)}</div>}
    </>
  );
}

/** Name, address, state and load of a rail icon, shown while hovering it. */
export function RailCard({
  target,
  at,
  side,
  label,
  address,
  env,
  health,
  waiting,
  panes,
}: {
  target: TargetRef;
  at: DOMRect;
  side: "left" | "right";
  label: string;
  address?: string;
  env: EnvLabel | null;
  health: Health;
  waiting: boolean;
  panes: number;
}) {
  const pos: CSSProperties = {
    top: Math.max(8, Math.min(at.top - 6, window.innerHeight - 220)),
    ...(side === "left" ? { left: at.right + 10 } : { right: window.innerWidth - at.left + 10 }),
  };
  const status = health === "live" ? `${STATUS.live} · ${panes} ${panes === 1 ? "pane" : "panes"} open` : STATUS[health];
  return (
    <div role="tooltip" style={pos} className="glass pointer-events-none fixed z-50 w-[228px] animate-fade-in rounded-[11px] px-3.5 py-3">
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-fg">{label}</span>
        <EnvChip env={env} />
      </div>
      {address && <div className="mt-0.5 truncate font-mono text-[11px] text-fg-3">{address}</div>}
      <div className={cn("mt-2 text-[11.5px]", health === "bad" ? "text-bad" : health === "warn" ? "text-warn" : "text-fg-2")}>{status}</div>
      {waiting && <div className="mt-1 text-[11.5px] font-medium text-accent">A terminal is waiting for you</div>}
      {health === "live" && <CardLoad target={target} />}
    </div>
  );
}
