import { describe, expect, it } from "vitest";
import { withSlowFacts } from "./metrics";
import { backoff, connectionDown, sleepsNow, wasDown } from "./poll";
import type { Metrics, SessionState } from "./types";

describe("polling", () => {
  it("waits longer after each failure, up to a limit", () => {
    expect([0, 1, 2, 3, 4, 5, 6, 20].map((f) => backoff(2000, f, 60000))).toEqual([2000, 4000, 8000, 16000, 32000, 60000, 60000, 60000]);
    expect(backoff(10000, 0, 60000)).toBe(10000);
  });

  it("waits while every terminal on the server is waiting for its connection", () => {
    const connected: SessionState = { kind: "connected", durable: true };
    const dropped: SessionState = { kind: "reconnecting", attempt: 1, retryInMs: 1000, reason: "Connection lost" };
    const waiting: SessionState = { kind: "disconnected", reason: "Connection lost" };
    expect(connectionDown([dropped, waiting])).toBe(true);
    expect(connectionDown([dropped, connected])).toBe(false);
    // No terminals, or none started yet: a Stats or Files pane still looks.
    expect(connectionDown([])).toBe(false);
    expect(connectionDown([undefined])).toBe(false);
    expect(connectionDown([undefined, dropped])).toBe(true);
    // Signing in for the first time isn't a dropped connection.
    expect(connectionDown([{ kind: "connecting", attempt: 1 }])).toBe(false);
    // Terminals that let go of an unused connection leave the server alone too.
    expect(connectionDown([{ kind: "asleep" }, waiting])).toBe(true);
    expect(connectionDown([{ kind: "asleep" }, connected])).toBe(false);
    expect(connectionDown([{ kind: "ended", code: 0 }])).toBe(false);
  });

  it("keeps the slow facts of the last full look", () => {
    const full = { hostname: "web-1", kernel: "Linux 6.8", cores: 8, disks: [{ mount: "/", size: 10, used: 5 }], month: { month: "2026-10", rx: 1, tx: 2 }, cpuTotal: 100, memTotal: 50 } as Metrics;
    const quick = { hostname: "", kernel: "", cores: 0, disks: [], month: null, cpuTotal: 200, memTotal: 50 } as unknown as Metrics;
    const m = withSlowFacts(quick, full);
    expect(m).toMatchObject({ hostname: "web-1", kernel: "Linux 6.8", cores: 8, disks: full.disks, month: full.month, cpuTotal: 200 });
  });

  it("lets go of a terminal's connection only after it sat unused, and only where tmux keeps its shell", () => {
    const tmux: SessionState = { kind: "connected", durable: true };
    const plain: SessionState = { kind: "connected", durable: false };
    const min = 60_000;
    expect(sleepsNow(tmux, true, 20 * min, 20)).toBe(true);
    expect(sleepsNow(tmux, true, 19 * min, 20)).toBe(false);
    // Off in Settings.
    expect(sleepsNow(tmux, true, 600 * min, 0)).toBe(false);
    // A shell that would end with the connection, and this computer's own.
    expect(sleepsNow(plain, true, 600 * min, 20)).toBe(false);
    expect(sleepsNow(tmux, false, 600 * min, 20)).toBe(false);
    // Not connected now: nothing to let go of.
    expect(sleepsNow({ kind: "asleep" }, true, 600 * min, 20)).toBe(false);
    expect(sleepsNow({ kind: "reconnecting", attempt: 1, retryInMs: 1000, reason: "x" }, true, 600 * min, 20)).toBe(false);
    expect(sleepsNow(undefined, true, 600 * min, 20)).toBe(false);
  });

  it("tells a connection coming back from a first one", () => {
    expect(wasDown({ kind: "asleep" })).toBe(true);
    expect(wasDown({ kind: "connecting", attempt: 2 })).toBe(true);
    expect(wasDown({ kind: "connecting", attempt: 1 })).toBe(false);
    expect(wasDown(undefined)).toBe(false);
  });
});
