import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useStore } from "./store";
import { startTorWatch, switchTor } from "./tor";

// The demo backend these run on looks at the window.
vi.stubGlobal("window", globalThis);

const titles = () => useStore.getState().toasts.map((t) => t.title);
const setStuck = (note?: string) => {
  (window as unknown as { __demoTorStuck?: string }).__demoTorStuck = note;
};

describe("what Tor tells you", () => {
  let stop: () => void;
  beforeEach(async () => {
    vi.useFakeTimers();
    useStore.setState({ toasts: [] });
    stop = startTorWatch();
    await vi.advanceTimersByTimeAsync(10);
  });
  afterEach(async () => {
    setStuck(undefined);
    const off = switchTor(false);
    await vi.advanceTimersByTimeAsync(100);
    await off;
    stop();
    vi.useRealTimers();
  });

  it("says it's on only once it is", async () => {
    const turning = switchTor(true);
    await vi.advanceTimersByTimeAsync(100);
    await turning;
    expect(titles()).not.toContain("Tor is on");
    await vi.advanceTimersByTimeAsync(2000);
    expect(useStore.getState().tor.state).toBe("on");
    expect(titles()).toContain("Tor is on");
  });

  it("says why when it doesn't get there, and never that it's on", async () => {
    setStuck("The list of its relays isn't getting through.");
    const turning = switchTor(true);
    await vi.advanceTimersByTimeAsync(300);
    await turning;
    expect(useStore.getState().tor).toMatchObject({ state: "starting", percent: 15 });
    expect(titles()).toEqual([]);
    await vi.advanceTimersByTimeAsync(25000);
    expect(titles()).toEqual(["Tor isn't connected yet"]);
    const shown = useStore.getState().toasts[0];
    expect(shown.body).toContain("isn't getting through");
    expect(shown.body).toContain("won't connect them around Tor");
    expect(shown.action?.label).toBe("Show log");
    // Once.
    await vi.advanceTimersByTimeAsync(60000);
    expect(titles().filter((t) => t === "Tor isn't connected yet").length).toBeLessThanOrEqual(1);
  });
});
