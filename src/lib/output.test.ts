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

  it("leaves a program that draws its own screen alone", () => {
    let shown = "";
    let agent = true;
    const p = new OutputPipeline((t) => (shown += t), () => agent);
    // An agent redrawing a line of addresses: its colors, untouched.
    const redraw = `${ESC}[2K${ESC}[32m'198.51.100.63'${ESC}[39m: 1, ERROR`;
    p.push(redraw);
    vi.runAllTimers();
    expect(shown).toBe(redraw);
    agent = false;
    shown = "";
    p.push("ERROR\n");
    vi.runAllTimers();
    expect(shown).toContain(`${ESC}[1;31mERROR`);
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

  it("picks up a rule changed while output flows", () => {
    expect(run(["nginx"])).toBe("nginx");
    useSettings.setState({ highlightRules: [{ id: "a", pattern: "nginx", color: "green" }] });
    expect(run(["nginx"])).toBe(`${ESC}[32mnginx${ESC}[0m`);
    useSettings.setState({ highlightRules: [{ id: "a", pattern: "nginx", color: "blue" }] });
    expect(run(["nginx"])).toBe(`${ESC}[34mnginx${ESC}[0m`);
  });

  it("follows the text style as it always did", () => {
    /** The style as the regex this replaced read it. */
    const regexStyle = (s: string) => {
      let style: string[] = [];
      for (const m of s.matchAll(/\x1b\[([0-9;:]*)m/g)) {
        const p = m[1];
        if (p === "" || p === "0") style = [];
        else if (p.startsWith("0;")) style = [p.slice(2)];
        else style = [...style, p].slice(-24);
      }
      return style;
    };
    const inputs = [
      `${ESC}[1m${ESC}[38:2:255:0:0mred `,
      `${ESC}[1;32m${ESC}[2J${ESC}[?25h${ESC}[Hx `,
      `${ESC}[33m${ESC}[0;1mbold `,
      `${ESC}[31m${ESC}[mplain `,
      `${ESC}[3${ESC}[4mbroken then underlined `,
      `${ESC}[35m${ESC}]0;title ${ESC}[1m\x07after a title `,
      Array.from({ length: 30 }, (_, i) => `${ESC}[${(i % 7) + 31}m`).join("") + "many ",
    ];
    const marking = { highlightRules: [{ id: "x", pattern: "mark", color: "cyan" as const }] };
    for (const quick of [true, false]) {
      for (const input of inputs) {
        // With nothing to hide or color, output takes the quick path, which follows the style all the same.
        const off = { highlightErrors: false, highlightWarnings: false, highlightAddresses: false };
        useSettings.setState({ ...DEFAULT_SETTINGS, ...(quick ? off : marking) });
        const want = regexStyle(input);
        const restore = `${ESC}[0m${want.length ? `${ESC}[${want.join(";")}m` : ""}`;
        let shown = "";
        const p = new OutputPipeline((t) => (shown += t));
        p.push(input);
        vi.runAllTimers();
        shown = "";
        useSettings.setState(marking);
        p.push("mark");
        vi.runAllTimers();
        expect(shown, JSON.stringify(input)).toBe(`${ESC}[36mmark${restore}`);
      }
    }
  });
});
