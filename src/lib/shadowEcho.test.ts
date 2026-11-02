import { Terminal } from "@xterm/headless";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { GIVE_UP_MS, rowLook, ShadowEcho, type EchoCount, type EchoPlace } from "./shadowEcho";

// The screen here is a real xterm, fed the bytes tmux 3.6 was recorded sending
// around bash: a key's echo is the plain character, Backspace comes as
// `?2026h \b ESC[K ?2026l`, and attaching puts the terminal on its second screen.

const BACKSPACE_ECHO = "\x1b[?2026h\b\x1b[K\x1b[?2026l";

class Rig {
  term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true, scrollback: 100 });
  counts: Partial<Record<EchoCount, number>> = {};
  echoes: number[] = [];
  place: EchoPlace = "line";
  echo = new ShadowEcho(
    this.term,
    {
      count: (_place, what) => (this.counts[what] = (this.counts[what] ?? 0) + 1),
      echoTook: (_place, ms) => this.echoes.push(ms),
    },
    () => this.place,
  );

  /** What the server sent, in the buffer. */
  async out(text: string) {
    await new Promise<void>((done) => this.term.write(text, done));
    this.echo.parsed();
  }

  type(keys: string) {
    for (const key of keys) this.echo.key(key);
  }

  /** Types and gets the echo back straight away, key by key. */
  async typeAndEcho(keys: string) {
    for (const key of keys) {
      this.echo.key(key);
      vi.advanceTimersByTime(100);
      await this.out(key);
    }
  }

  /** A fresh shell prompt with the gate shut. */
  async prompt(text = "$ ") {
    this.echo.reset();
    await this.out(`\r\n${text}`);
  }
}

let rig: Rig;
const count = (what: EchoCount) => rig.counts[what] ?? 0;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  rig = new Rig();
});
afterEach(() => {
  rig.term.dispose();
  vi.useRealTimers();
});

describe("the gate", () => {
  it("makes the first key on a line wait, and lets the next ones through once its echo lands", async () => {
    await rig.prompt();
    await rig.typeAndEcho("ls");
    expect(count("waited")).toBe(1);
    expect(count("atOnce")).toBe(1);
    expect(count("right")).toBe(1);
    expect(count("wrong")).toBe(0);
  });

  it("treats keys typed before the first echo as waiting too", async () => {
    await rig.prompt();
    rig.type("abc");
    vi.advanceTimersByTime(300);
    await rig.out("abc");
    expect(count("waited")).toBe(3);
    expect(count("atOnce")).toBe(0);
    rig.type("d");
    expect(count("atOnce")).toBe(1);
  });

  it("judges keys that were typed ahead when their echoes land together", async () => {
    await rig.prompt();
    await rig.typeAndEcho("a");
    rig.type("bcd");
    vi.advanceTimersByTime(250);
    await rig.out("bcd");
    expect(count("atOnce")).toBe(3);
    expect(count("right")).toBe(3);
    expect(rig.echoes).toEqual([100, 250, 250, 250]);
  });

  it("predicts Cyrillic: half of what the owner types", async () => {
    await rig.prompt();
    await rig.typeAndEcho("привет");
    expect(count("atOnce")).toBe(5);
    expect(count("right")).toBe(5);
  });

  it("reads the cell, not the bytes: colors and a redraw around the echo change nothing", async () => {
    await rig.prompt();
    await rig.typeAndEcho("a");
    rig.type("b");
    await rig.out("\x1b[?2026h\x1b[?25l\x1b[1;32mb\x1b[0m\x1b[?25h\x1b[?2026l");
    expect(count("right")).toBe(1);
    // zsh rewrites the previous character along with each new one.
    rig.type("c");
    await rig.out("\bbc");
    expect(count("right")).toBe(2);
  });

  it("works on tmux's second screen, where scrolled rows have no history", async () => {
    await rig.out("\x1b[?1049h\x1b[H$ ");
    await rig.typeAndEcho("hi");
    expect(count("right")).toBe(1);
  });

  it("still knows the row when the normal screen has scrolled", async () => {
    for (let i = 0; i < 40; i++) await rig.out(`line ${i}\r\n`);
    await rig.out("$ ");
    await rig.typeAndEcho("hi");
    expect(count("right")).toBe(1);
  });
});

describe("what shuts the gate", () => {
  beforeEach(async () => {
    await rig.prompt();
    await rig.typeAndEcho("ab");
    rig.counts = {};
  });

  it("Enter, and the next line starts over", async () => {
    rig.echo.key("\r");
    await rig.out("\r\n$ ");
    rig.type("x");
    expect(count("atOnce")).toBe(0);
    expect(count("waited")).toBe(1);
  });

  it.each([
    ["Tab", "\t"],
    ["Left", "\x1b[D"],
    ["Right in application mode", "\x1bOC"],
    ["Up", "\x1b[A"],
    ["Ctrl+C", "\x03"],
    ["Ctrl+U", "\x15"],
    ["Alt+B", "\x1bb"],
    ["Alt+Shift+P, which starts like the terminal's replies", "\x1bP"],
    ["F5", "\x1b[15~"],
    ["Esc", "\x1b"],
  ])("%s", async (_name, key) => {
    rig.echo.key(key);
    rig.type("x");
    expect(count("atOnce")).toBe(0);
    expect(count("keys")).toBe(2);
  });

  it.each([
    ["a click", "\x1b[<0;10;5M"],
    ["an old-style click", "\x1b[M !!"],
    ["a paste", "\x1b[200~text\x1b[201~"],
    ["text in one piece", "пример"],
  ])("%s, which isn't a key at all", async (_name, data) => {
    rig.echo.key(data);
    rig.type("x");
    expect(count("atOnce")).toBe(0);
    expect(count("keys")).toBe(1);
  });

  it("a wrong character, and what was on its way is dropped", async () => {
    rig.type("xyz");
    await rig.out("Q");
    expect(count("wrong")).toBe(1);
    expect(count("dropped")).toBe(2);
    rig.type("k");
    expect(count("waited")).toBe(1);
  });

  it("a cursor that moved with no key behind it", async () => {
    await rig.out("  ");
    rig.type("x");
    expect(count("atOnce")).toBe(0);
  });

  it("text before the typing changing under the same cursor", async () => {
    // Rewrites "$ " on the row and comes back to the same place.
    const row = rig.term.buffer.active.cursorY + 1;
    await rig.out(`\x1b[s\x1b[${row};1H%\x1b[u`);
    rig.type("x");
    expect(count("atOnce")).toBe(0);
  });

  it("a new size", async () => {
    rig.echo.reset();
    rig.type("x");
    expect(count("atOnce")).toBe(0);
  });

  it("a key that isn't guessed: a wide character, or the end of the row", async () => {
    rig.echo.key("中");
    expect(count("unfit")).toBe(1);
    await rig.out("\x1b[80G");
    rig.type("x");
    expect(count("atOnce")).toBe(0);
  });

  it("text to the right: nothing in mid-line is guessed", async () => {
    await rig.out("tail\x1b[4D");
    rig.type("x");
    expect(count("midLine")).toBe(1);
    expect(count("atOnce")).toBe(0);
  });
});

describe("Backspace", () => {
  it("is judged by the cell it clears and the cursor", async () => {
    await rig.prompt();
    await rig.typeAndEcho("abc");
    rig.counts = {};
    rig.echo.key("\x7f");
    expect(count("atOnce")).toBe(1);
    expect(count("right")).toBe(0);
    await rig.out(BACKSPACE_ECHO);
    expect(count("right")).toBe(1);
  });

  it("isn't guessed over a key still on its way", async () => {
    await rig.prompt();
    await rig.typeAndEcho("ab");
    rig.counts = {};
    rig.type("c");
    rig.echo.key("\x7f");
    expect(count("waited")).toBe(1);
    expect(count("atOnce")).toBe(1);
    rig.type("d");
    expect(count("atOnce")).toBe(1);
  });

  it("can run ahead of its answers, and be typed after", async () => {
    await rig.prompt();
    await rig.typeAndEcho("abcd");
    rig.counts = {};
    rig.echo.key("\x7f");
    rig.echo.key("\x7f");
    rig.type("x");
    expect(count("atOnce")).toBe(3);
    await rig.out(`${BACKSPACE_ECHO}${BACKSPACE_ECHO}x`);
    expect(count("right")).toBe(3);
    expect(count("wrong")).toBe(0);
    rig.type("y");
    expect(count("atOnce")).toBe(4);
  });

  it("isn't guessed past where the proved typing starts", async () => {
    await rig.prompt();
    await rig.typeAndEcho("ab");
    rig.counts = {};
    for (let i = 0; i < 2; i++) {
      rig.echo.key("\x7f");
      await rig.out(BACKSPACE_ECHO);
    }
    expect(count("right")).toBe(2);
    rig.echo.key("\x7f");
    expect(count("waited")).toBe(1);
  });
});

// Silent prompts: every key here would be a password character on screen if
// the gate let it through. These are the cases mosh and VS Code get wrong.
describe("a prompt that doesn't echo", () => {
  it("on a new row: every key waits and nothing could have been drawn", async () => {
    await rig.prompt("Password: ");
    rig.type("hunter2-x");
    vi.advanceTimersByTime(GIVE_UP_MS);
    rig.echo.tick();
    expect(count("atOnce")).toBe(0);
    expect(count("waited")).toBe(9);
    expect(count("heldSilent")).toBe(9);
    expect(count("passwordOpen")).toBe(0);
  });

  it("with stray redraws arriving meanwhile: tmux sends empty frames at silent prompts", async () => {
    await rig.prompt("Password: ");
    for (const key of "secret") {
      rig.echo.key(key);
      vi.advanceTimersByTime(90);
      await rig.out("\x1b[?2026h\x1b[?25l\x1b[?12l\x1b[?25h\x1b[?2026l");
    }
    expect(count("atOnce")).toBe(0);
  });

  it("on a row that echoed before it, once the prompt has shown (mosh's hole)", async () => {
    // `sleep 2; read -rs -p ' Password: '`, with `xy` typed during the sleep.
    await rig.prompt("");
    await rig.typeAndEcho("xy");
    await rig.out(" Password: ");
    rig.counts = {};
    rig.type("qwerty");
    vi.advanceTimersByTime(GIVE_UP_MS);
    rig.echo.tick();
    expect(count("atOnce")).toBe(0);
    expect(count("heldSilent")).toBe(6);
  });

  it("on such a row, for the keys on their way when it shows: counted, and every one shows as not coming back right", async () => {
    await rig.prompt("");
    await rig.typeAndEcho("xy");
    rig.counts = {};
    rig.type("qwer");
    expect(count("atOnce")).toBe(4);
    await rig.out(" Password: ");
    expect(count("right")).toBe(0);
    expect(count("wrong") + count("dropped")).toBe(4);
    rig.type("ty");
    expect(count("atOnce")).toBe(4);
  });

  it("right after a row that echoed, when it asks on the same row with no text: the cursor stays and keys go silent", async () => {
    await rig.prompt();
    await rig.typeAndEcho("yes");
    rig.counts = {};
    rig.type("abc");
    vi.advanceTimersByTime(GIVE_UP_MS);
    rig.echo.tick();
    expect(count("atOnce")).toBe(3);
    expect(count("silent")).toBe(3);
    expect(count("right")).toBe(0);
    // Silence shuts the gate.
    rig.type("d");
    expect(count("atOnce")).toBe(3);
    expect(count("waited")).toBe(1);
  });

  it("a password prompt that does echo opens the gate, and that's what the zero is there to catch", async () => {
    await rig.prompt("Password: ");
    await rig.typeAndEcho("ab");
    expect(count("passwordOpen")).toBe(1);
  });

  it("asterisks for each key prove nothing (sudo-rs)", async () => {
    await rig.prompt("[sudo] пароль для k: ");
    rig.type("a");
    await rig.out("*");
    rig.type("b");
    await rig.out("*");
    expect(count("atOnce")).toBe(0);
    expect(count("passwordOpen")).toBe(0);
  });

  it("a type-ahead after Enter never opens the gate before the output has come", async () => {
    await rig.prompt();
    await rig.typeAndEcho("ls");
    rig.counts = {};
    rig.echo.key("\r");
    rig.type("pw");
    expect(count("waited")).toBe(2);
    expect(count("atOnce")).toBe(0);
    await rig.out("\r\nPassword: ");
    rig.type("z");
    expect(count("atOnce")).toBe(0);
  });
});

describe("what the counts can be trusted for", () => {
  it("a space proves nothing, because it looks the same before and after", async () => {
    await rig.prompt();
    rig.type(" ");
    await rig.out(" ");
    rig.type("a");
    expect(count("waited")).toBe(2);
  });

  it("a space doesn't count as right just because the cell is blank", async () => {
    await rig.prompt();
    await rig.typeAndEcho("a");
    rig.counts = {};
    rig.type(" ");
    await rig.out("");
    expect(count("right")).toBe(0);
    await rig.out(" ");
    expect(count("right")).toBe(1);
  });

  it("keys before an Enter can come true, and aren't wrong if the screen scrolled first", async () => {
    await rig.prompt();
    await rig.typeAndEcho("ab");
    rig.counts = {};
    rig.type("cd");
    rig.echo.key("\r");
    await rig.out("cd\r\n$ ");
    expect(count("right")).toBe(2);
    expect(count("wrong")).toBe(0);
    // tmux scrolls the row itself, so the cells are gone before they're read.
    await rig.out("\x1b[?1049h\x1b[H$ ");
    await rig.typeAndEcho("ab");
    rig.counts = {};
    rig.type("c");
    rig.echo.key("\r");
    await rig.out("\x1b[H\x1b[2J$ \r\n");
    expect(count("wrong")).toBe(0);
    expect(count("unjudged")).toBe(1);
  });

  it("the terminal's own replies and focus reports aren't keys and change nothing", async () => {
    await rig.prompt();
    await rig.typeAndEcho("ab");
    rig.counts = {};
    for (const reply of ["\x1b[?1;2c", "\x1b[>0;276;0c", "\x1b[0n", "\x1b[24;80R", "\x1b]11;rgb:0000/0000/0000\x1b\\", "\x1bP1$r0m\x1b\\", "\x1b[I", "\x1b[O", "\x1b[?2026;2$y"]) {
      rig.echo.key(reply);
    }
    expect(count("keys")).toBe(0);
    rig.type("c");
    expect(count("atOnce")).toBe(1);
  });

  it("counts each key under the place it was typed in, even if the answer comes after a move", async () => {
    const places: EchoPlace[] = [];
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
    let place: EchoPlace = "agent";
    const echo = new ShadowEcho(term, { count: (p, what) => what === "heldSilent" && places.push(p), echoTook: () => {} }, () => place);
    echo.key("a");
    place = "line";
    vi.advanceTimersByTime(GIVE_UP_MS);
    echo.tick();
    expect(places).toEqual(["agent"]);
    term.dispose();
  });

  it("tells rows apart well enough for an estimate, and no more", () => {
    expect(rowLook("user@host:~$ ")).toBe("prompt");
    expect(rowLook("root@host:/# ")).toBe("prompt");
    expect(rowLook("~/code ❯ ")).toBe("prompt");
    expect(rowLook("PS C:\\Users> ")).toBe("prompt");
    expect(rowLook(">>> ")).toBe("other");
    expect(rowLook("")).toBe("other");
    expect(rowLook("Password: ")).toBe("password");
    expect(rowLook("[sudo] password for k: ")).toBe("password");
    expect(rowLook("[sudo] пароль для k: ")).toBe("password");
    expect(rowLook("Enter passphrase for key: ")).toBe("password");
    // What a client can't ever tell: this one doesn't echo.
    expect(rowLook("$ ")).toBe("prompt");
  });
});

describe("it keeps and sends nothing", () => {
  const SECRET = "hunter2-пароль";

  /** Every string reachable from a value, besides functions and the terminal it reads. */
  function strings(value: unknown, skip: unknown, seen = new Set<unknown>()): string[] {
    if (typeof value === "string") return [value];
    if (typeof value !== "object" || value === null || value === skip || seen.has(value)) return [];
    seen.add(value);
    return Object.values(value).flatMap((v) => strings(v, skip, seen));
  }

  it("holds no typed text while keys are on their way or after", async () => {
    await rig.prompt();
    rig.type(SECRET);
    const onTheWay = strings(rig.echo, rig.term);
    await rig.out(SECRET);
    vi.advanceTimersByTime(GIVE_UP_MS);
    rig.echo.tick();
    // Only names of places and kinds of row: nothing of the keys or the row's text.
    expect([...onTheWay, ...strings(rig.echo, rig.term)].filter((text) => !["line", "agent", "screen", "prompt", "password", "other"].includes(text))).toEqual([]);
    expect(onTheWay).toContain("line");
  });

  it("forgets the keys that got no answer", async () => {
    await rig.prompt("Password: ");
    rig.type(SECRET);
    const pending = (rig.echo as unknown as { pending: unknown[] }).pending;
    expect(pending.length).toBeGreaterThan(0);
    vi.advanceTimersByTime(GIVE_UP_MS);
    rig.echo.tick();
    expect(pending.length).toBe(0);
  });

  it("gets a different fingerprint each time, so none can be looked up", () => {
    const seen = new Set<number>();
    for (let i = 0; i < 5; i++) {
      const echo = new ShadowEcho(rig.term, { count: () => {}, echoTook: () => {} }, () => "line");
      echo.key("a");
      seen.add((echo as unknown as { pending: { want: number }[] }).pending[0].want);
    }
    expect(seen.size).toBeGreaterThan(1);
  });

  it("only reads the terminal: any other use of it would throw", async () => {
    const reads = new Set(["cols", "buffer"]);
    const guarded = new Proxy(rig.term, {
      get(target, name) {
        if (typeof name === "string" && !reads.has(name)) throw new Error(`touched ${name}`);
        return Reflect.get(target, name);
      },
    });
    const echo = new ShadowEcho(guarded, { count: () => {}, echoTook: () => {} }, () => "line");
    await rig.out("$ ");
    for (const key of ["a", "b", "\x7f", "\r", "\t", "\x1b[D", "中"]) {
      echo.key(key);
      echo.parsed();
    }
    echo.reset();
    echo.tick();
  });

  it("can't draw, send or save: it imports nothing and names nothing that could", () => {
    type Fs = { readFileSync(path: URL, encoding: "utf8"): string };
    const fs = (globalThis as unknown as { process: { getBuiltinModule(id: string): Fs } }).process.getBuiltinModule("node:fs");
    const code = fs
      .readFileSync(new URL("./shadowEcho.ts", import.meta.url), "utf8")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/^\s*\/\/.*$/gm, "");
    expect(code).not.toMatch(/\bimport\b|\brequire\b/);
    expect(code).not.toMatch(/\b(?:document|window|fetch|XMLHttpRequest|WebSocket|localStorage|sessionStorage|indexedDB|navigator|console|invoke|backend|postMessage)\b/);
    expect(code).not.toMatch(/\.(?:write|writeln|input|paste|registerDecoration|registerMarker|appendChild|createElement)\b/);
  });
});
