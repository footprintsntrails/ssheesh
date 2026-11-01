// Live server metrics, shared: the vitals bar and a Stats pane looking at
// the same machine use one poller.

import { useEffect, useState } from "react";
import { backend } from "./backend";
import { backoff, connectionDown } from "./poll";
import { usePrivacy } from "./privacy";
import { machineSummary, useStore } from "./store";
import { targetKey, toOpError, type Metrics, type OpError, type TargetRef } from "./types";

const POLL_MS = 2000;
/** The longest wait between looks while they keep failing. */
const MAX_WAIT_MS = 60_000;
/** The name, kernel, cores, disks and month's traffic change slowly: read this often. */
const FULL_EVERY_MS = 30_000;
export const HISTORY = 60;

export interface Sample {
  cpu: number;
  rx: number;
  tx: number;
}

export interface MetricsState {
  metrics: Metrics | null;
  samples: Sample[];
  error: OpError | null;
}

interface Poller {
  state: MetricsState;
  listeners: Set<(s: MetricsState) => void>;
  timer: number;
  prev: Metrics | null;
  /** The last full look, for the slow facts a quick one leaves out, and when it was. */
  slow: Metrics | null;
  slowAt: number;
  /** Looks that failed in a row. */
  failures: number;
  /** Waiting for the server's connection to come back. */
  paused: boolean;
  /** No more looks: the machine can't give stats, or nobody's watching. */
  done: boolean;
  stop: () => void;
}

const EMPTY: MetricsState = { metrics: null, samples: [], error: null };
const pollers = new Map<string, Poller>();

/** A quick look, with the slow facts from the last full one. */
export function withSlowFacts(m: Metrics, slow: Metrics): Metrics {
  return { ...m, hostname: slow.hostname, kernel: slow.kernel, cores: slow.cores, disks: slow.disks, month: slow.month };
}

function start(target: TargetRef): Poller {
  const p: Poller = { state: EMPTY, listeners: new Set(), timer: 0, prev: null, slow: null, slowAt: 0, failures: 0, paused: false, done: false, stop: () => {} };
  const publish = (next: Partial<MetricsState>) => {
    p.state = { ...p.state, ...next };
    p.listeners.forEach((l) => l(p.state));
  };
  const down = () => target.kind === "host" && connectionDown(machineSummary(useStore.getState(), target).sessions);
  const later = (ms: number) => {
    if (!p.done) p.timer = window.setTimeout(tick, ms);
  };
  const tick = async () => {
    if (p.done) return;
    if (down()) {
      // Picked up again when the connection is back.
      p.paused = true;
      return;
    }
    if (document.visibilityState !== "visible") return later(POLL_MS);
    const full = !p.slow || Date.now() - p.slowAt >= FULL_EVERY_MS;
    try {
      let m = await backend.fetchMetrics(target, full);
      if (p.done) return;
      if (full) {
        p.slow = m;
        p.slowAt = Date.now();
        usePrivacy.getState().learn([m.hostname]);
      } else if (p.slow) {
        m = withSlowFacts(m, p.slow);
      }
      const prev = p.prev;
      let samples = p.state.samples;
      if (prev) {
        const dt = Math.max(0.001, (m.atMs - prev.atMs) / 1000);
        const total = m.cpuTotal - prev.cpuTotal;
        const cpu = total > 0 ? Math.max(0, Math.min(1, 1 - (m.cpuIdle - prev.cpuIdle) / total)) : 0;
        const rx = Math.max(0, (m.netRx - prev.netRx) / dt);
        const tx = Math.max(0, (m.netTx - prev.netTx) / dt);
        samples = [...samples.slice(-(HISTORY - 1)), { cpu, rx, tx }];
      }
      p.prev = m;
      p.failures = 0;
      publish({ metrics: m, samples, error: null });
    } catch (e) {
      if (p.done) return;
      const error = toOpError(e);
      publish({ error });
      // macOS and Windows have no /proc: asking again won't change that.
      if (error.code === "unsupported") return;
      p.failures++;
    }
    later(backoff(POLL_MS, p.failures, MAX_WAIT_MS));
  };
  const unsubscribe = useStore.subscribe(() => {
    if (!p.paused || down()) return;
    p.paused = false;
    p.failures = 0;
    void tick();
  });
  p.stop = () => {
    p.done = true;
    clearTimeout(p.timer);
    unsubscribe();
  };
  void tick();
  return p;
}

/** Follows a machine's metrics, from one poller per machine, until the function it returns is called. */
export function watchMetrics(target: TargetRef, listener: (s: MetricsState) => void): () => void {
  const key = targetKey(target);
  let p = pollers.get(key);
  if (!p) {
    p = start(target);
    pollers.set(key, p);
  }
  const poller = p;
  poller.listeners.add(listener);
  listener(poller.state);
  return () => {
    poller.listeners.delete(listener);
    if (poller.listeners.size === 0) {
      poller.stop();
      pollers.delete(key);
    }
  };
}

export function useMetrics(target: TargetRef): MetricsState {
  const key = targetKey(target);
  const [state, setState] = useState<MetricsState>(() => pollers.get(key)?.state ?? EMPTY);
  // The target is identified by its key.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => watchMetrics(target, setState), [key]);
  return state;
}
