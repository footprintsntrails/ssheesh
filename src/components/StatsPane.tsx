import { Activity, SquareTerminal, X } from "lucide-react";
import { useId, type ReactNode } from "react";
import { bytes, cn, monthName, percent, rate, uptime } from "../lib/format";
import { useMetrics, type Sample } from "../lib/metrics";
import { useNames } from "../lib/privacy";
import { useStore, type PaneState } from "../lib/store";
import type { Metrics, OpError, TargetRef } from "../lib/types";
import { Button, IconButton, Spinner } from "./ui";

function Sparkline({ series, max, color, fill = true, height = 28, width = 200 }: { series: number[]; max: number; color: string; fill?: boolean; height?: number; width?: number }) {
  const id = useId();
  if (series.length < 2) return <div style={{ height }} />;
  const n = series.length;
  const pts = series.map((v, i) => [(i / (n - 1)) * width, height - 1.5 - (Math.min(v, max) / (max || 1)) * (height - 3)] as const);
  const line = pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const area = `${line} L${width},${height} L0,${height} Z`;
  return (
    <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="block w-full" style={{ height }} aria-hidden>
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: color, stopOpacity: 0.22 }} />
          <stop offset="1" style={{ stopColor: color, stopOpacity: 0 }} />
        </linearGradient>
      </defs>
      {fill && <path d={area} fill={`url(#${id})`} />}
      <path d={line} fill="none" style={{ stroke: color }} strokeWidth="1.5" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
    </svg>
  );
}

function Meter({ value, height = 4 }: { value: number; height?: number }) {
  const v = Math.max(0, Math.min(1, value));
  return (
    <div className="w-full overflow-hidden rounded-full bg-ink/[0.08]" style={{ height }}>
      <div
        className={cn("h-full rounded-full transition-[width] duration-700", v > 0.9 ? "bg-bad" : v > 0.75 ? "bg-warn" : "bg-accent")}
        style={{ width: `${v * 100}%` }}
      />
    </div>
  );
}

function memFraction(m: Metrics) {
  return m.memTotal ? (m.memTotal - m.memAvailable) / m.memTotal : 0;
}

function netMax(samples: Sample[]) {
  return Math.max(64 * 1024, ...samples.map((s) => Math.max(s.rx, s.tx))) * 1.15;
}

function SignIn({ error, onTerminal }: { error: OpError; onTerminal: () => void }) {
  const auth = error.code === "auth" || error.code === "hostKey";
  return (
    <div className="max-w-[320px] text-center">
      <p className="text-[13px] font-medium text-fg">{auth ? "Sign in first" : "No stats yet"}</p>
      <p className="mt-1.5 text-[12.5px] leading-relaxed text-fg-3">{error.message}</p>
      {auth && (
        <Button size="sm" variant="primary" className="mt-4" onClick={onTerminal}>
          <SquareTerminal size={13} /> Open terminal
        </Button>
      )}
    </div>
  );
}

function Row({ label, value, detail, chart }: { label: string; value: ReactNode; detail?: ReactNode; chart?: ReactNode }) {
  return (
    <div className="grid grid-cols-[84px_minmax(0,1fr)] items-center gap-x-4 border-b border-line px-4 py-3 last:border-0 @lg:grid-cols-[84px_minmax(0,1fr)_minmax(0,1fr)]">
      <div className="text-[12.5px] text-fg-3">{label}</div>
      <div className="min-w-0">
        <div className="text-[15px] font-semibold tabular-nums text-fg">{value}</div>
        {detail && <div className="mt-0.5 truncate text-[12px] tabular-nums text-fg-3">{detail}</div>}
      </div>
      {chart && <div className="col-span-2 mt-2 min-w-0 @lg:col-span-1 @lg:mt-0">{chart}</div>}
    </div>
  );
}

/** The Stats pane: one compact table, readable at a glance. */
export function StatsPane({ pane, target }: { pane: PaneState; target: TargetRef }) {
  const openIn = useStore((s) => s.openIn);
  const names = useNames();
  const { metrics: m, samples, error } = useMetrics(target);
  const last = samples[samples.length - 1];

  if (error && !m) {
    return (
      <div className="absolute inset-0 flex items-center justify-center p-6">
        <SignIn error={error} onTerminal={() => openIn("terminal", target, { from: pane.id, split: true })} />
      </div>
    );
  }
  if (!m) {
    return (
      <div className="absolute inset-0 flex items-center justify-center text-fg-3">
        <Spinner />
      </div>
    );
  }

  const mem = memFraction(m);
  const max = netMax(samples);
  return (
    <div className="@container absolute inset-0 overflow-y-auto">
      <Row
        label="CPU"
        value={last ? percent(last.cpu) : "…"}
        detail={`${m.cores} cores, load ${m.load.map((l) => l.toFixed(2)).join(" ")}`}
        chart={<Sparkline series={samples.map((s) => s.cpu)} max={1} color="var(--ui-accent)" />}
      />
      <Row
        label="Memory"
        value={percent(mem)}
        detail={`${bytes(m.memTotal - m.memAvailable)} of ${bytes(m.memTotal)}${m.swapTotal ? `, swap ${bytes(m.swapTotal - m.swapFree)}` : ""}`}
        chart={<Meter value={mem} height={6} />}
      />
      <Row
        label="Network"
        value={last ? `↓ ${rate(last.rx)}   ↑ ${rate(last.tx)}` : "…"}
        detail={
          m.month
            ? `${monthName(m.month.month)}: ↓ ${bytes(m.month.rx)}  ↑ ${bytes(m.month.tx)}`
            : `Since boot: ↓ ${bytes(m.netRx)}  ↑ ${bytes(m.netTx)}`
        }
        chart={
          <div className="relative">
            <Sparkline series={samples.map((s) => s.rx)} max={max} color="var(--ui-accent)" />
            <div className="absolute inset-0">
              <Sparkline series={samples.map((s) => s.tx)} max={max} color="var(--ui-ok)" fill={false} />
            </div>
          </div>
        }
      />
      {m.disks.map((d) => (
        <Row
          key={d.mount}
          label="Disk"
          value={percent(d.size ? d.used / d.size : 0)}
          detail={`${d.mount}: ${bytes(d.used)} of ${bytes(d.size)}`}
          chart={<Meter value={d.size ? d.used / d.size : 0} height={6} />}
        />
      ))}
      <p className="px-4 py-3 text-[12px] text-fg-3">
        {names.text(m.hostname)}, {m.kernel}, up {uptime(m.uptimeSecs)}
        {!m.month && ". Install vnstat on the server for monthly traffic."}
      </p>
    </div>
  );
}

function Vital({ label, children, className = "flex" }: { label: string; children: ReactNode; className?: string }) {
  return (
    <span className={cn("min-w-0 shrink-0 items-center gap-1.5", className)}>
      <span className="text-fg-3">{label}</span>
      {children}
    </span>
  );
}

/** One-line vitals for the focused machine, under the panes. */
export function VitalsBar({ target, onDetails, onHide }: { target: TargetRef; onDetails: () => void; onHide: () => void }) {
  const { metrics: m, samples, error } = useMetrics(target);
  const last = samples[samples.length - 1];
  const root = m?.disks.find((d) => d.mount === "/") ?? m?.disks[0];
  const max = netMax(samples);

  return (
    <div className="@container flex h-8 shrink-0 items-center gap-5 overflow-hidden pr-1.5 pl-3 text-[12px] tabular-nums text-fg">
      {error && !m ? (
        <span className="truncate text-fg-3">Stats aren't available here: {error.message}</span>
      ) : (
        <>
          <Vital label="CPU">
            <span className="w-9 text-right">{last ? percent(last.cpu) : "…"}</span>
            <span className="hidden w-12 @xl:block">
              <Sparkline series={samples.map((s) => s.cpu)} max={1} color="var(--ui-accent)" height={14} width={48} fill={false} />
            </span>
          </Vital>
          <Vital label="Memory">
            <span>{m ? percent(memFraction(m)) : "…"}</span>
            <span className="hidden w-10 @xl:block">{m && <Meter value={memFraction(m)} height={4} />}</span>
          </Vital>
          {/* Narrow windows keep CPU and memory; network and disk come back as room allows. */}
          <Vital label="Network" className="hidden @xl:flex">
            <span>{last ? `↓ ${rate(last.rx)}  ↑ ${rate(last.tx)}` : "…"}</span>
            <span className="hidden w-12 @3xl:block">
              <Sparkline series={samples.map((s) => s.rx)} max={max} color="var(--ui-accent)" height={14} width={48} fill={false} />
            </span>
          </Vital>
          {root && (
            <Vital label="Disk" className="hidden @md:flex">
              <span>{percent(root.size ? root.used / root.size : 0)}</span>
            </Vital>
          )}
          {m && <span className="hidden text-fg-3 @4xl:inline">up {uptime(m.uptimeSecs)}</span>}
        </>
      )}
      <span className="flex-1" />
      <IconButton label="Open full stats" onClick={onDetails} className="size-6">
        <Activity size={13.5} />
      </IconButton>
      <IconButton label="Hide vitals" onClick={onHide} className="size-6">
        <X size={13.5} />
      </IconButton>
    </div>
  );
}
