import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { backend } from "./backend";
import { watchMetrics, type MetricsState } from "./metrics";
import { useStore } from "./store";
import type { Metrics, OpError, SessionState, TargetRef } from "./types";

const web: TargetRef = { kind: "host", hostId: "cfg:web" };
const full = { atMs: 0, hostname: "web-1", kernel: "Linux 6.8", cores: 4, disks: [{ mount: "/", size: 10, used: 4 }], month: null, cpuTotal: 100, cpuIdle: 50, memTotal: 8, netRx: 0, netTx: 0 } as unknown as Metrics;
const quick = { ...full, hostname: "", kernel: "", cores: 0, disks: [] } as Metrics;

function sessionOnWeb(state: SessionState) {
  useStore.setState({ panes: { t1: { id: "t1", kind: "terminal", target: web } }, sessions: { t1: state } });
}

describe("the vitals poller", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("window", globalThis);
    vi.stubGlobal("document", { visibilityState: "visible" });
    useStore.setState({ panes: {}, sessions: {} });
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("stops asking a machine that can't give stats", async () => {
    const unsupported: OpError = { code: "unsupported", message: "Stats work on Linux machines for now." };
    const fetch = vi.spyOn(backend, "fetchMetrics").mockRejectedValue(unsupported);
    let seen: MetricsState | null = null;
    const stop = watchMetrics({ kind: "local" }, (s) => (seen = s));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(seen!.error).toEqual(unsupported);
    stop();
  });

  it("asks less often while looks keep failing, and as before once one works", async () => {
    const fetch = vi.spyOn(backend, "fetchMetrics").mockRejectedValue({ code: "network", message: "Can't reach the server" });
    const stop = watchMetrics(web, () => {});
    // At 0, 4, 12 and 28 s, instead of every 2 s.
    await vi.advanceTimersByTimeAsync(59_000);
    expect(fetch).toHaveBeenCalledTimes(4);
    fetch.mockResolvedValue(full);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(fetch).toHaveBeenCalledTimes(5);
    await vi.advanceTimersByTimeAsync(6_000);
    expect(fetch).toHaveBeenCalledTimes(8);
    stop();
  });

  it("reads the slow facts now and then, and keeps them in between", async () => {
    const fetch = vi.spyOn(backend, "fetchMetrics").mockImplementation(async (_t, wantFull) => (wantFull ? full : quick));
    let seen: MetricsState | null = null;
    const stop = watchMetrics(web, (s) => (seen = s));
    await vi.advanceTimersByTimeAsync(31_000);
    const asked = fetch.mock.calls.map(([, f]) => f);
    expect(asked[0]).toBe(true);
    expect(asked.slice(1, 15).every((f) => f === false)).toBe(true);
    expect(asked[15]).toBe(true);
    expect(seen!.metrics).toMatchObject({ hostname: "web-1", cores: 4, disks: full.disks });
    stop();
  });

  it("waits while the server's connection is down and looks again once it's back", async () => {
    sessionOnWeb({ kind: "reconnecting", attempt: 1, retryInMs: 1000, reason: "Connection lost" });
    const fetch = vi.spyOn(backend, "fetchMetrics").mockResolvedValue(full);
    const stop = watchMetrics(web, () => {});
    await vi.advanceTimersByTimeAsync(20_000);
    expect(fetch).not.toHaveBeenCalled();
    sessionOnWeb({ kind: "connected", durable: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(fetch).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(fetch).toHaveBeenCalledTimes(2);
    stop();
  });

  it("stops when nobody's watching", async () => {
    const fetch = vi.spyOn(backend, "fetchMetrics").mockResolvedValue(full);
    const stop = watchMetrics(web, () => {});
    await vi.advanceTimersByTimeAsync(0);
    stop();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});
