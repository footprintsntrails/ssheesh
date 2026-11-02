import { Terminal } from "@xterm/headless";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { GIVE_UP_MS, ShadowEcho } from "./shadowEcho";
import { addCount, addEcho, clearTypingCounts, currentCounts, echoWithin, SAVE_EVERY_KEYS, typingSummary, type EchoTally } from "./typingCounts";

beforeEach(() => clearTypingCounts());

describe("what's saved", () => {
  const NAME = "sshee-typing-counts";
  /** Longer than a save waits. */
  const settled = () => new Promise((done) => setTimeout(done, 350));
  let disk: Map<string, string>;

  /** A store that really saves, as after starting SSHeesh: the one imported above found no storage. */
  async function startApp() {
    vi.resetModules();
    return import("./typingCounts");
  }

  /** Types at a password prompt, on a real terminal, until the keys are given up on. */
  async function typePassword(app: Awaited<ReturnType<typeof startApp>>, password: string) {
    vi.useFakeTimers({ toFake: ["Date"] });
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    const echo = new ShadowEcho(term, { count: (place, what) => app.addCount("tor", place, what), echoTook: (place, ms) => app.addEcho("tor", place, ms) }, () => "line");
    await new Promise<void>((done) => term.write("[sudo] password for k: ", done));
    echo.parsed();
    for (const key of password) echo.key(key);
    vi.advanceTimersByTime(GIVE_UP_MS);
    echo.tick();
    vi.useRealTimers();
    term.dispose();
  }

  beforeEach(() => {
    disk = new Map();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => disk.get(key) ?? null,
      setItem: (key: string, value: string) => disk.set(key, value),
      removeItem: (key: string) => disk.delete(key),
    });
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("doesn't change when a password is typed, so two readings say nothing", async () => {
    const app = await startApp();
    for (let i = 0; i < SAVE_EVERY_KEYS; i++) app.addCount("tor", "line", "keys");
    await settled();
    const before = disk.get(NAME);
    expect(before).toContain(`"keys":${SAVE_EVERY_KEYS}`);
    await typePassword(app, "hunter2");
    await settled();
    expect(disk.get(NAME)).toBe(before);
    // Settings still sees them, while SSHeesh runs.
    expect(currentOf(app).tor?.line?.heldSilent).toBe(7);
    expect(currentOf(app).tor?.line?.keys).toBe(SAVE_EVERY_KEYS + 7);
  });

  it("right after starting the counts over, shows nothing of a password", async () => {
    const app = await startApp();
    app.clearTypingCounts();
    await typePassword(app, "hunter2");
    await settled();
    expect(disk.get(NAME) ?? "").not.toMatch(/keys|typed|waited|heldSilent/);
  });

  it("never holds the count of keys that got no answer, nor anything labelled a password", async () => {
    const app = await startApp();
    await typePassword(app, "hunter2");
    for (let i = 0; i < SAVE_EVERY_KEYS; i++) app.addCount("tor", "line", "keys");
    await settled();
    const text = disk.get(NAME) ?? "";
    expect(text).toContain("waited");
    expect(text).not.toContain("heldSilent");
    expect(text).not.toMatch(/password|hunter2/i);
    expect(currentOf(app).tor?.line?.heldSilent).toBe(7);
  });

  it("isn't written when SSHeesh closes: the last keys go uncounted, and start again from what was saved", async () => {
    let app = await startApp();
    for (let i = 0; i < SAVE_EVERY_KEYS + 5; i++) app.addCount("tor", "line", "keys");
    await typePassword(app, "hunter2");
    await settled();
    app = await startApp();
    await settled();
    expect(currentOf(app).tor?.line).toEqual({ keys: SAVE_EVERY_KEYS });
  });

  it("goes on from what another window saved, losing nothing of this one's", async () => {
    const app = await startApp();
    app.addCount("tor", "line", "keys");
    await typePassword(app, "ab");
    disk.set(NAME, JSON.stringify({ state: { saved: { tor: { line: { keys: 500 } } } }, version: 0 }));
    await app.useTypingCounts.persist.rehydrate();
    expect(currentOf(app).tor?.line).toMatchObject({ keys: 503, heldSilent: 2 });
  });

  const currentOf = (app: Awaited<ReturnType<typeof startApp>>) => app.currentCounts();
});

describe("the counts", () => {
  it("adds up by way of connecting and by place", () => {
    addCount("tor", "line", "keys");
    addCount("tor", "line", "keys");
    addCount("tor", "agent", "keys");
    addCount("direct", "line", "keys");
    const counts = currentCounts();
    expect(counts.tor?.line?.keys).toBe(2);
    expect(counts.tor?.agent?.keys).toBe(1);
    expect(counts.direct?.line?.keys).toBe(1);
  });

  it("are only numbers: what's saved can't hold text", () => {
    addCount("tor", "line", "atOnce");
    addEcho("tor", "line", 120);
    const walk = (v: unknown): unknown[] => (typeof v === "object" && v !== null ? Object.values(v).flatMap(walk) : [v]);
    expect(walk(currentCounts()).every((v) => typeof v === "number")).toBe(true);
  });

  it("clear", () => {
    addCount("tor", "line", "keys");
    clearTypingCounts();
    expect(currentCounts()).toEqual({});
  });

  it("add up across a save, echo times too, and keep what isn't saved", () => {
    addEcho("tor", "line", 5);
    addCount("tor", "line", "heldSilent");
    for (let i = 0; i < SAVE_EVERY_KEYS; i++) addCount("tor", "line", "keys");
    addEcho("tor", "line", 5);
    addEcho("tor", "line", 100);
    addCount("tor", "line", "keys");
    const line = currentCounts().tor?.line;
    expect(line?.keys).toBe(SAVE_EVERY_KEYS + 1);
    expect(line?.heldSilent).toBe(1);
    expect(line?.echoes).toEqual([2, 0, 0, 0, 0, 1]);
  });
});

describe("echo times", () => {
  const echoes = (...ms: number[]) => {
    clearTypingCounts();
    ms.forEach((m) => addEcho("tor", "line", m));
    return currentCounts().tor?.line?.echoes;
  };

  it("are kept by slot, with no key's own time", () => {
    expect(echoes(5, 9, 100)).toEqual([2, 0, 0, 0, 0, 1]);
  });

  it("give a median and a p90 by the slot they fall in", () => {
    const e = echoes(90, 90, 90, 90, 90, 90, 90, 90, 90, 390);
    expect(echoWithin(e, 0.5)).toBe(100);
    expect(echoWithin(e, 0.9)).toBe(100);
    expect(echoWithin(e, 1)).toBe(400);
  });

  it("say so when past the last slot, and when there are none", () => {
    expect(echoWithin(echoes(9000), 0.5)).toBe(Infinity);
    expect(echoWithin(undefined, 0.5)).toBeNull();
    expect(echoWithin([], 0.5)).toBeNull();
  });
});

describe("the summary", () => {
  it("says nothing with nothing to go on", () => {
    const s = typingSummary(undefined);
    expect(s.keys).toBe(0);
    expect(s.atOnceShare).toBeNull();
    expect(s.rightShare).toBeNull();
    expect(s.echoMedian).toBeNull();
  });

  it("splits typing by where it went, and judges only the command line", () => {
    const line: EchoTally = { keys: 120, typed: 100, atOnce: 66, atPrompt: 40, right: 57, wrong: 1, dropped: 1, silent: 2, unjudged: 5, waited: 40, midLine: 3, heldSilent: 8, echoes: [0, 0, 0, 0, 0, 10] };
    const agent: EchoTally = { keys: 80, typed: 80, atOnce: 70, right: 1, wrong: 50, heldSilent: 2 };
    const screen: EchoTally = { keys: 20, typed: 20, passwordOpen: 1 };
    const s = typingSummary({ line, agent, screen });
    expect(s.keys).toBe(220);
    expect(s.lineTyped).toBe(100);
    expect(s.agentShare).toBe(0.4);
    expect(s.screenShare).toBe(0.1);
    expect(s.atOnceShare).toBe(0.66);
    expect(s.atPromptShare).toBe(0.4);
    expect(s.rightShare).toBe(57 / 61);
    expect(s.wrong).toBe(2);
    expect(s.silent).toBe(2);
    expect(s.heldSilent).toBe(10);
    expect(s.passwordOpen).toBe(1);
    expect(s.midLine).toBe(3);
    expect(s.echoMedian).toBe(100);
  });
});
