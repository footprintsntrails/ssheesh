import { Terminal } from "@xterm/headless";
import { afterEach, describe, expect, it } from "vitest";
import { GIVE_UP_MS, ShadowEcho, type EchoCount } from "./shadowEcho";

// The counter against the real thing: tmux on a private socket, started with
// the options `durable_command` uses (crates/sshee-core/src/ssh.rs), a real
// shell and real password prompts, with xterm reading the bytes after a fixed
// delay. It takes minutes and needs Linux, tmux and python3, so it only runs
// when asked:
//
//   ECHO_TMUX=1 npx vitest run src/lib/shadowEcho.tmux.test.ts
//
// A fixed delay isn't Tor's jitter, and xterm here is headless. It reaches no
// server and writes only in its own temporary folder, though the password
// prompts it tries may leave a line in this computer's system log.

type Bytes = Uint8Array;
interface Child {
  stdin: { write(data: Bytes | string): void };
  stdout: { on(event: "data", fn: (data: Bytes) => void): void };
  stderr: { on(event: "data", fn: (data: Bytes) => void): void };
  kill(): void;
}
interface Node {
  process: {
    env: Record<string, string | undefined>;
    getBuiltinModule(id: "node:child_process"): {
      spawn(cmd: string, args: string[], options: { env: Record<string, string | undefined> }): Child;
      execSync(cmd: string, options?: { stdio: "ignore" }): Bytes;
    };
    getBuiltinModule(id: "node:fs"): { mkdtempSync(prefix: string): string; rmSync(path: string, options: { recursive: boolean; force: boolean }): void };
    getBuiltinModule(id: "node:os"): { tmpdir(): string };
  };
}
const { process } = globalThis as unknown as Node;
const wanted = !!process.env.ECHO_TMUX;

const BRIDGE = `
import fcntl, os, pty, select, struct, sys, termios
pid, fd = pty.fork()
if pid == 0:
    os.execvp(sys.argv[3], sys.argv[3:])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", int(sys.argv[2]), int(sys.argv[1]), 0, 0))
try:
    while True:
        ready, _, _ = select.select([fd, 0], [], [])
        if fd in ready:
            data = os.read(fd, 65536)
            if not data:
                break
            os.write(1, data)
        if 0 in ready:
            data = os.read(0, 65536)
            if not data:
                break
            os.write(fd, data)
except OSError:
    pass
`;

const SCROLL_TITLE = "sshee-scroll #{pane_in_mode} #{scroll_position} #{history_size} #{pane_height} #{history_limit}";
const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));

class Session {
  term = new Terminal({ cols: 100, rows: 30, allowProposedApi: true });
  counts: Partial<Record<EchoCount, number>> = {};
  echoes: number[] = [];
  titles: { title: string; at: number }[] = [];
  private child: Child;
  private ticker: ReturnType<typeof setInterval>;
  private echo: ShadowEcho;
  readonly dir: string;
  readonly socket: string;
  private readonly cp = process.getBuiltinModule("node:child_process");

  constructor(
    readonly rtt: number,
    { title = SCROLL_TITLE, rc = "", shell = "bash --norc --noprofile -i" }: { title?: string; rc?: string; shell?: string } = {},
  ) {
    const fs = process.getBuiltinModule("node:fs");
    this.dir = fs.mkdtempSync(`${process.getBuiltinModule("node:os").tmpdir()}/sshee-echo-`);
    this.socket = `${this.dir}/tmux`;
    this.echo = new ShadowEcho(
      this.term,
      {
        count: (_place, what) => (this.counts[what] = (this.counts[what] ?? 0) + 1),
        echoTook: (_place, ms) => this.echoes.push(ms),
      },
      () => "line",
    );
    this.term.onWriteParsed(() => this.echo.parsed());
    // What the terminal answers by itself goes back and past the counter, as in the app.
    this.term.onData((reply) => {
      this.echo.key(reply);
      this.send(reply);
    });
    this.term.onResize(() => this.echo.reset());
    this.term.buffer.onBufferChange(() => this.echo.reset());
    this.term.onTitleChange((t) => this.titles.push({ title: t, at: Date.now() }));
    this.ticker = setInterval(() => this.echo.tick(), 200);
    const setup = `cd ${this.dir}; export PS1="$ "; ${rc}`;
    const start = `env -i HOME=${this.dir} PATH=${process.env.PATH} TERM=tmux-256color LANG=C.UTF-8 TMUX=$TMUX bash --norc --noprofile -c '${setup} exec ${shell}'`;
    this.child = this.cp.spawn(
      "python3",
      [
        "-c",
        BRIDGE,
        "100",
        "30",
        "tmux",
        "-S",
        this.socket,
        "-f",
        "/dev/null",
        "-T",
        "RGB,sync",
        "new-session",
        "-A",
        "-s",
        "k",
        start,
        ";",
        "set-option",
        "status",
        "off",
        ";",
        "set-option",
        "mouse",
        "on",
        ";",
        "set-option",
        "-s",
        "escape-time",
        "10",
        ";",
        "set-option",
        "-s",
        "focus-events",
        "on",
        ";",
        "set-option",
        "-s",
        "set-clipboard",
        "on",
        ";",
        "set-option",
        "set-titles",
        "on",
        ";",
        "set-option",
        "set-titles-string",
        title,
      ],
      { env: { ...process.env, TERM: "xterm-256color", TMUX: undefined, LANG: "C.UTF-8" } },
    );
    // The delay's both ways: half on the keys, half on the output.
    this.child.stdout.on("data", (data) => setTimeout(() => this.term.write(data), rtt / 2));
  }

  /** Sends to the shell without counting: how a test sets the scene. */
  send(text: string) {
    setTimeout(() => this.child.stdin.write(text), this.rtt / 2);
  }

  /** Runs a command and clears the counts, for the test to count from the moment it runs. */
  async run(command: string, until: string | null) {
    await this.until("$ ");
    this.send(`${command}\r`);
    if (until) await this.until(until);
    await wait(this.rtt + 50);
    this.counts = {};
    this.echoes = [];
  }

  /** Keys as you'd type them: counted, and going out one at a time. */
  async type(keys: string, every = 90) {
    for (const key of keys) {
      this.echo.key(key);
      this.send(key);
      await wait(every);
    }
  }

  async answers() {
    await wait(GIVE_UP_MS + this.rtt + 300);
  }

  screen(): string {
    const b = this.term.buffer.active;
    const rows: string[] = [];
    for (let y = 0; y < this.term.rows; y++) rows.push(b.getLine(b.baseY + y)?.translateToString(true) ?? "");
    return rows.join("\n");
  }

  async until(text: string, ms = 8000) {
    const end = Date.now() + ms;
    while (!this.screen().includes(text)) {
      if (Date.now() > end) throw new Error(`no "${text}" on screen:\n${this.screen()}`);
      await wait(20);
    }
  }

  /** Everything a client can see of the moment: cells, cursor and modes. */
  state() {
    const b = this.term.buffer.active;
    return { screen: this.screen(), x: b.cursorX, y: b.cursorY, type: b.type, modes: { ...this.term.modes } };
  }

  count(what: EchoCount) {
    return this.counts[what] ?? 0;
  }

  close() {
    clearInterval(this.ticker);
    try {
      this.cp.execSync(`tmux -S ${this.socket} kill-server`, { stdio: "ignore" });
    } catch {
      /* gone already */
    }
    this.child.kill();
    this.term.dispose();
    process.getBuiltinModule("node:fs").rmSync(this.dir, { recursive: true, force: true });
  }
}

const SILENT_PROMPTS: [string, string, string][] = [
  ["read -rs", `read -rs -p 'Password: '`, "Password: "],
  ["read -ers, a line editor that's silent", `read -ers -p 'Password: '`, "Password: "],
  ["read -ers dressed as a shell prompt", `clear; read -ers -p '$ '`, "$ "],
  ["Python's getpass", `python3 -c 'import getpass; getpass.getpass()'`, "Password: "],
  ["su", `su nobody -s /bin/sh -c 'su root -c true'`, "Password:"],
  // Not through `su`, which leaves it no terminal to ask on. sudo-rs refuses a user who isn't in sudoers without asking, so it isn't covered.
  ["sudo", `setpriv --reuid=65534 --regid=65534 --clear-groups $(command -v sudo.ws || command -v sudo) -k true`, "password for"],
  ["passwd", `su nobody -s /bin/sh -c passwd`, "assword"],
  ["ssh-keygen, ssh's own way of asking", `ssh-keygen -q -t ed25519 -f key`, "passphrase"],
];

describe.skipIf(!wanted)("on real tmux", { timeout: 40_000 }, () => {
  let s: Session;
  afterEach(() => s?.close());

  it.each([[80], [300]])("at a shell prompt, %i ms: keys are right and none go silent", async (rtt) => {
    s = new Session(rtt);
    await s.until("$ ");
    await s.type("echo hello привет there");
    await s.answers();
    console.log(`prompt, rtt ${rtt}: ${JSON.stringify(s.counts)}, echoes ${Math.min(...s.echoes)}-${Math.max(...s.echoes)} ms`);
    expect(s.count("atOnce")).toBeGreaterThan(0);
    expect(s.count("wrong")).toBe(0);
    expect(s.count("silent")).toBe(0);
    expect(s.count("right")).toBe(s.count("atOnce"));
  });

  it("Backspace and retyping at a shell prompt", async () => {
    s = new Session(80);
    await s.until("$ ");
    await s.type("echo helo", 200);
    await s.type("\x7f", 200);
    await s.type("lo", 200);
    await s.answers();
    console.log(`backspace: ${JSON.stringify(s.counts)}`);
    expect(s.count("wrong")).toBe(0);
    expect(s.count("silent")).toBe(0);
    expect(s.count("right")).toBe(s.count("atOnce"));
  });

  it.each(SILENT_PROMPTS)("%s: nothing could have been drawn", async (_name, command, prompt) => {
    s = new Session(150);
    await s.run(command, prompt);
    await s.type("qwertyuiop");
    await s.answers();
    console.log(`silent, ${_name}: ${JSON.stringify(s.counts)}`);
    expect(s.count("atOnce")).toBe(0);
    expect(s.count("passwordOpen")).toBe(0);
    expect(s.count("waited")).toBe(10);
    expect(s.screen()).not.toContain("qwerty");
  });

  // Worst for the rule: a question that echoes, then a silent read with no text
  // of its own. Nothing on screen changes, so only silence can shut the gate.
  it.each([[80], [300]])("a yes/no question, then a silent read on the same row, %i ms", async (rtt) => {
    s = new Session(rtt);
    await s.run(`read -n1 -p 'Continue? '; read -rs; echo finished`, "Continue? \n");
    await s.type("y", 400);
    await wait(rtt + 50);
    s.counts = {};
    await s.type("qwertyuiop");
    await s.answers();
    console.log(`yes/no then silent, rtt ${rtt}: ${JSON.stringify(s.counts)}`);
    expect(s.count("right")).toBe(0);
    expect(s.count("atPrompt")).toBe(0);
    expect(s.count("silent")).toBe(s.count("atOnce"));
    expect(s.screen()).not.toContain("qwerty");
  });

  it.each([[80], [150], [300]])("a password prompt on a row that already echoed, keys after it shows, %i ms", async (rtt) => {
    s = new Session(rtt);
    await s.run(`sleep 2; read -rs -p ' Password: '`, null);
    await s.type("xy");
    await s.until("xy Password: ");
    await wait(rtt + 50);
    s.counts = {};
    await s.type("qwertyuiop");
    await s.answers();
    console.log(`same row, typed after, rtt ${rtt}: ${JSON.stringify(s.counts)}`);
    expect(s.count("atOnce")).toBe(0);
    expect(s.count("heldSilent")).toBe(10);
  });

  // The rule's hole, measured. What's counted as `atOnce` and not `right` here is
  // what drawing with this rule alone would have put on screen for a moment.
  it.each([[80], [150], [300], [400]])("a password prompt on a row that already echoed, typing straight through, %i ms", async (rtt) => {
    s = new Session(rtt);
    await s.run(`sleep 1; read -rs -p ' Password: '`, null);
    await s.type("abcdefghijklmnopqrstuvwxyz");
    await s.answers();
    const exposed = s.count("wrong") + s.count("dropped") + s.count("silent");
    console.log(`same row, typed through, rtt ${rtt}: exposed ${exposed} of ${s.count("atOnce")}: ${JSON.stringify(s.counts)}`);
    // Only the keys on their way when it showed: after that the gate is shut.
    expect(exposed).toBeLessThanOrEqual(Math.ceil(rtt / 90) + 2);
    expect(exposed).toBeGreaterThan(0);
    expect(s.count("heldSilent")).toBeGreaterThan(0);
  });

  it.each([[80], [300]])("commands one after another, past the bottom of the screen, %i ms", async (rtt) => {
    s = new Session(rtt);
    await s.until("$ ");
    await s.run("seq 1 40", "40\n");
    for (let i = 0; i < 6; i++) {
      await s.type(`echo round${i}\r`);
      await s.until(`round${i}\n$ `);
      await wait(rtt + 50);
    }
    await s.answers();
    console.log(`commands, rtt ${rtt}: ${JSON.stringify(s.counts)}`);
    expect(s.count("wrong")).toBe(0);
    expect(s.count("dropped")).toBe(0);
    expect(s.count("silent")).toBe(0);
    expect(s.count("right") + s.count("unjudged")).toBe(s.count("atOnce"));
  });

  it("editing a line: Tab, arrows, Ctrl+U", async () => {
    s = new Session(150);
    await s.until("$ ");
    await s.type("echo one", 300);
    await s.type("\x15", 500);
    await s.type("ech", 300);
    await s.type("\t", 500);
    await s.type("two", 300);
    await s.type("\x1b[D", 500);
    await s.type("Z", 500);
    await s.type("\x01", 500);
    await s.type("#", 500);
    await s.answers();
    console.log(`editing: ${JSON.stringify(s.counts)}`);
    expect(s.count("silent")).toBe(0);
    expect(s.count("midLine")).toBeGreaterThan(0);
  });

  it("a line longer than the row wraps without a wrong guess being counted right", async () => {
    s = new Session(80);
    await s.until("$ ");
    await s.type("x".repeat(115), 120);
    await s.answers();
    console.log(`wrapping: ${JSON.stringify(s.counts)}`);
    expect(s.count("unfit")).toBeGreaterThan(0);
    expect(s.count("silent")).toBe(0);
  });

  it("vim: insert mode and commands", async () => {
    s = new Session(150);
    await s.run(`vim -u NONE -N -n`, "~");
    await s.type("ihello there");
    await s.type("\x1b");
    await wait(300);
    await s.type("jjkkhh:q!\r");
    await s.answers();
    console.log(`vim: ${JSON.stringify(s.counts)}`);
    expect(s.count("keys")).toBeGreaterThan(0);
  });

  it("less: keys that draw no echo", async () => {
    s = new Session(150);
    await s.run(`seq 1 200 | less`, "1\n2");
    await s.type("jjjjjjkkkkq");
    await s.answers();
    console.log(`less: ${JSON.stringify(s.counts)}`);
    expect(s.count("atOnce")).toBeLessThanOrEqual(s.count("keys"));
  });

  it("a split inside tmux: the pane next door counts as text to the right", async () => {
    s = new Session(80);
    await s.until("$ ");
    process.getBuiltinModule("node:child_process").execSync(`tmux -S ${s.socket} split-window -h -d`, { stdio: "ignore" });
    await wait(500);
    await s.type("echo split");
    await s.answers();
    console.log(`split: ${JSON.stringify(s.counts)}`);
    expect(s.count("atOnce")).toBe(0);
    expect(s.count("midLine")).toBeGreaterThan(0);
  });
});

const installed = (shell: string) => {
  if (!wanted) return false;
  try {
    process.getBuiltinModule("node:child_process").execSync(`command -v ${shell}`, { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
};

// Shells that draw differently: zsh and fish rewrite more of the line as you
// type, and fish suggests the rest of a command to the right of the cursor.
describe.skipIf(!wanted)("other shells", { timeout: 40_000 }, () => {
  let s: Session;
  afterEach(() => s?.close());

  it.skipIf(!installed("zsh")).each([[80], [300]])("zsh, %i ms", async (rtt) => {
    s = new Session(rtt, { shell: "zsh -i" });
    await s.until("$ ");
    await wait(300);
    await s.type("echo hello привет there", 120);
    await s.type("\x7f", 400);
    await s.type("e\r");
    await s.until("there\n");
    await wait(rtt + 300);
    await s.type("echo again", 120);
    await s.answers();
    console.log(`zsh, rtt ${rtt}: ${JSON.stringify(s.counts)}`);
    expect(s.count("atOnce")).toBeGreaterThan(0);
    expect(s.count("wrong")).toBe(0);
    expect(s.count("silent")).toBe(0);
  });

  it.skipIf(!installed("zsh"))("zsh: a silent prompt", async () => {
    s = new Session(150, { shell: "zsh -i" });
    await s.until("$ ");
    await wait(300);
    await s.run(`read -rs "?Password: "`, "Password: ");
    await s.type("qwertyuiop");
    await s.answers();
    console.log(`zsh, silent: ${JSON.stringify(s.counts)}`);
    expect(s.count("atOnce")).toBe(0);
  });

  it.skipIf(!installed("fish")).each([[80], [300]])("fish, with its suggestions, %i ms", async (rtt) => {
    s = new Session(rtt, { shell: "fish --no-config" });
    await wait(1500);
    await s.type("echo hello there\r", 120);
    await s.until("hello there\n");
    await wait(rtt + 300);
    s.counts = {};
    // Now fish offers that line back while the same one is typed.
    await s.type("echo hello there", 120);
    await s.answers();
    console.log(`fish, rtt ${rtt}: ${JSON.stringify(s.counts)}`);
    expect(s.count("silent")).toBe(0);
  });
});

// Whether a client can tell it's at a shell's prompt with nothing added to the
// server, and what it takes to add. These pin down facts about tmux, not this code.
describe.skipIf(!wanted)("what a client can tell inside tmux", { timeout: 40_000 }, () => {
  let s: Session;
  afterEach(() => s?.close());

  it("a real prompt and a silent one can look exactly alike", async () => {
    s = new Session(40);
    await s.run("clear", null);
    await wait(200);
    const prompt = s.state();
    await s.run(`clear; read -ers -p '$ '`, null);
    await wait(200);
    const silent = s.state();
    expect(silent).toEqual(prompt);
  });

  it("the terminal's second screen and paste mode are the same at a prompt and in a running program", async () => {
    s = new Session(40);
    await s.until("$ ");
    await wait(200);
    const atPrompt = s.state();
    await s.run("sleep 3", null);
    const running = s.state();
    expect(atPrompt.type).toBe("alternate");
    expect(running.type).toBe("alternate");
    expect(running.modes.bracketedPasteMode).toBe(atPrompt.modes.bracketedPasteMode);
  });

  it("shell markers don't reach the terminal", async () => {
    s = new Session(40);
    const seen: string[] = [];
    for (const id of [133, 633, 7, 7777]) {
      s.term.parser.registerOscHandler(id, (data) => {
        seen.push(`${id};${data}`);
        return true;
      });
    }
    await s.run(`printf '\\033]133;A\\007\\033]633;A\\007\\033]7;file:///x\\007\\033]7777;mark\\007'; echo done`, "done");
    await wait(300);
    console.log(`markers seen: ${JSON.stringify(seen)}`);
    expect(seen).toEqual([]);
  });

  it("tmux's own view of what's running says the shell during a silent prompt", async () => {
    s = new Session(40, { title: "#{pane_current_command}" });
    await s.run(`read -rs -p 'Password: '`, "Password: ");
    s.send("x");
    await wait(1500);
    const asked = new TextDecoder().decode(process.getBuiltinModule("node:child_process").execSync(`tmux -S ${s.socket} display-message -p '#{pane_current_command}'`)).trim();
    // SSHeesh's foreground poll asks like this, and the title only says it on a redraw.
    console.log(`during a silent prompt: asked ${asked}, title ${s.titles[s.titles.length - 1]?.title}`);
    expect(asked).toBe("bash");
  });

  it("with a hook in the shell, each prompt is announced and a silent one isn't", async () => {
    s = new Session(150, { title: "mark #{@sshee_prompt}", rc: `export PROMPT_COMMAND="tmux set -p @sshee_prompt \\$((++n))";` });
    await s.until("$ ");
    await s.run("echo one", "one\n");
    await wait(500);
    const marks = () => s.titles.filter((t) => /^mark \d/.test(t.title));
    expect(marks().length).toBeGreaterThanOrEqual(2);
    const shown = marks().length;
    // How long the mark and the prompt's own text are apart, on this link.
    for (let i = 0; i < 5; i++) {
      const before = marks().length;
      s.send(`echo gap${i}\r`);
      await s.until(`gap${i}\n`);
      const textAt = Date.now();
      while (marks().length === before && Date.now() < textAt + 2000) await wait(2);
      console.log(`mark came ${marks()[marks().length - 1].at - textAt} ms after the output before the prompt`);
      await wait(300);
    }
    const lastMark = marks()[marks().length - 1];
    expect(marks().length).toBe(shown + 5);
    await s.run(`read -rs -p 'Password: '`, "Password: ");
    const atSilent = marks().length;
    await s.type("qwerty");
    await wait(500);
    expect(marks().length).toBe(atSilent);
    // A read that's asked for isn't a prompt: nothing announces it, whatever it looks like.
    expect(atSilent).toBeLessThanOrEqual(shown + 6);
    // Enter ends the read; the next prompt is announced again.
    s.send("\r");
    await wait(1000);
    expect(marks().length).toBe(atSilent + 1);
    expect(marks()[marks().length - 1].title).not.toBe(lastMark.title);
  });
});
