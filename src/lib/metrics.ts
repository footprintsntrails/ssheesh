// Live server metrics, shared: the vitals bar and a Stats pane looking at
// the same machine use one poller.

import { useEffect, useState } from "react";
import { backend } from "./backend";
import { targetKey, toOpError, type Metrics, type OpError, type TargetRef } from "./types";

const POLL_MS = 2000;
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
}

const EMPTY: MetricsState = { metrics: null, samples: [], error: null };
const pollers = new Map<string, Poller>();

function start(key: string, target: TargetRef): Poller {
  const p: Poller = { state: EMPTY, listeners: new Set(), timer: 0, prev: null };
  const publish = (next: Partial<MetricsState>) => {
    p.state = { ...p.state, ...next };
    p.listeners.forEach((l) => l(p.state));
  };
  const tick = async () => {
    if (document.visibilityState === "visible") {
      try {
        const m = await backend.fetchMetrics(target);
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
        publish({ metrics: m, samples, error: null });
      } catch (e) {
        publish({ error: toOpError(e) });
      }
    }
    if (pollers.get(key) === p) p.timer = window.setTimeout(tick, POLL_MS);
  };
  void tick();
  return p;
}

export function useMetrics(target: TargetRef): MetricsState {
  const key = targetKey(target);
  const [state, setState] = useState<MetricsState>(() => pollers.get(key)?.state ?? EMPTY);
  useEffect(() => {
    let p = pollers.get(key);
    if (!p) {
      p = start(key, target);
      pollers.set(key, p);
    }
    const poller = p;
    poller.listeners.add(setState);
    setState(poller.state);
    return () => {
      poller.listeners.delete(setState);
      if (poller.listeners.size === 0) {
        clearTimeout(poller.timer);
        pollers.delete(key);
      }
    };
    // The target is identified by its key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return state;
}
