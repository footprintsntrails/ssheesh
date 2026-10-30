import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OutputPipeline, tokenize } from "./output";
import { usePrivacy } from "./privacy";
import { DEFAULT_SETTINGS, useSettings } from "./settings";
import { useStore } from "./store";

const ESC = "\x1b";

function run(chunks: string[]): string {
  let shown = "";
  const p = new OutputPipeline((t) => (shown += t));
  for (const c of chunks) p.push(c);
  vi.runAllTimers();
  return shown;
}

beforeEach(() => {
  vi.useFakeTimers();
  useSettings.setState({ ...DEFAULT_SETTINGS });
  usePrivacy.setState({ learned: [] });
  useStore.setState({
    hosts: [{ id: "h1", name: "prod-api", hostname: "203.0.113.10", user: "deploy", port: null, identityFile: null, source: "saved" }],
  });
});
afterEach(() => vi.useRealTimers());

describe("tokenize", () => {
  it("separates text from escape sequences", () => {
    const { tokens, rest } = tokenize(`a${ESC}[1;31mb${ESC}]0;title\x07c`);
    expect(tokens.map((t) => t.kind)).toEqual(["text", "esc", "text", "esc", "text"]);
    expect(rest).toBe("");
  });

  it("hands back an escape sequence that was cut off", () => {
    const { tokens, rest } = tokenize(`ok${ESC}[38;5`);
    expect(tokens).toEqual([{ kind: "text", s: "ok" }]);
    expect(rest).toBe(`${ESC}[38;5`);
  });
});

describe("OutputPipeline", () => {
  it("passes output through untouched when nothing is on", () => {
    useSettings.setState({ highlightErrors: false, highlightWarnings: false });
    const s = `deploy@prod-api ERROR ${ESC}[2J`;
    expect(run([s])).toBe(s);
  });

  it("colors errors and puts the previous color back", () => {
    const out = run([`${ESC}[32mbuild ERROR here`]);
    expect(out).toBe(`${ESC}[32mbuild ${ESC}[1;31mERROR${ESC}[0m${ESC}[32m here`);
  });

  it("colors warnings with lowercase tool output", () => {
    expect(run(["warning: unused variable"])).toBe(`${ESC}[33mwarning${ESC}[0m: unused variable`);
  });

  it("leaves words inside escape sequences alone", () => {
    const s = `${ESC}]0;ERROR in title\x07ok`;
    expect(run([s])).toBe(s);
  });

  it("catches a word split across chunks", () => {
    expect(run(["oh no ERR", "OR"])).toBe(`oh no ${ESC}[1;31mERROR${ESC}[0m`);
  });

  it("joins an escape sequence split across chunks", () => {
    expect(run([`${ESC}[3`, "1mred"])).toBe(`${ESC}[31mred`);
  });

  it("hides names and highlights in one pass", () => {
    useSettings.setState({ streamerMode: true });
    expect(run(["deploy@prod-api: FATAL"])).toBe(`••••••@••••••••: ${ESC}[1;31mFATAL${ESC}[0m`);
  });

  it("applies your own rules, plain or as a regex", () => {
    useSettings.setState({
      highlightRules: [
        { id: "a", pattern: "nginx", color: "green" },
        { id: "b", pattern: "/took \\d+ms/", color: "cyan" },
      ],
    });
    expect(run(["NGINX took 12ms"])).toBe(`${ESC}[32mNGINX${ESC}[0m ${ESC}[36mtook 12ms${ESC}[0m`);
  });

  it("ignores a rule that isn't a valid regex", () => {
    useSettings.setState({ highlightErrors: false, highlightWarnings: false, highlightRules: [{ id: "x", pattern: "/([/", color: "red" }] });
    expect(run(["fine"])).toBe("fine");
  });
});
