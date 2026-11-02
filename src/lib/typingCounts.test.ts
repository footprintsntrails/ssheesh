import { beforeEach, describe, expect, it } from "vitest";
import { addCount, addEcho, clearTypingCounts, echoWithin, typingSummary, useTypingCounts, type EchoTally } from "./typingCounts";

beforeEach(() => clearTypingCounts());

describe("the counts", () => {
  it("adds up by way of connecting and by place", () => {
    addCount("tor", "line", "keys");
    addCount("tor", "line", "keys");
    addCount("tor", "agent", "keys");
    addCount("direct", "line", "keys");
    const { counts } = useTypingCounts.getState();
    expect(counts.tor?.line?.keys).toBe(2);
    expect(counts.tor?.agent?.keys).toBe(1);
    expect(counts.direct?.line?.keys).toBe(1);
  });

  it("are only numbers: what's saved can't hold text", () => {
    addCount("tor", "line", "atOnce");
    addEcho("tor", "line", 120);
    const walk = (v: unknown): unknown[] => (typeof v === "object" && v !== null ? Object.values(v).flatMap(walk) : [v]);
    expect(walk(useTypingCounts.getState().counts).every((v) => typeof v === "number")).toBe(true);
  });

  it("clear", () => {
    addCount("tor", "line", "keys");
    clearTypingCounts();
    expect(useTypingCounts.getState().counts).toEqual({});
  });
});

describe("echo times", () => {
  const echoes = (...ms: number[]) => {
    clearTypingCounts();
    ms.forEach((m) => addEcho("tor", "line", m));
    return useTypingCounts.getState().counts.tor?.line?.echoes;
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
