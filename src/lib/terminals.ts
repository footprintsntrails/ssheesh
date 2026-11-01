// xterm instances live outside React. A pane can move around the split tree
// (which remounts its component), but the terminal and its scrollback must
// survive that, so each pane's terminal is created once and its DOM node is
// re-parented into whichever container is showing it.

import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal, type IBufferLine } from "@xterm/xterm";
import { backend, type WelcomeSpec } from "./backend";
import { isMac, isMacClear, isTerminalNewline, isTerminalPaste, isWindows, matchShortcut, terminalCopyKey } from "./keys";
import { findLinks } from "./paths";
import { parseScrollTitle, tmuxPosition, tmuxView, type ScrollView } from "./scroll";
import { openTerminalLink } from "./terminalLinks";
import { anchorLines, buildRecap, cursorLine } from "./away";
import { OutputPipeline } from "./output";
import { maskText, streamerOn, targetName } from "./privacy";
import { findSecrets, keyLines, keyOpenAfter, looksLikeKeyBody, secretsHidden } from "./secrets";
import { markHintSeen, useSettings } from "./settings";
import { fontStack, ICON_FONT, resolveScheme } from "./themes";
import { useStore, type Attention, type PaneState, type Progress } from "./store";
import type { ContainerRef, HostKeyCheck, SessionEvent, SessionState, TargetRef, TmuxScroll } from "./types";
import { toOpError } from "./types";

/** Current terminal look from Settings. */
function lookOptions() {
  const s = useSettings.getState();
  return {
    fontFamily: fontStack(s.terminalFont),
    fontSize: s.terminalFontSize,
    lineHeight: s.terminalLineHeight,
    cursorStyle: s.cursorStyle,
    cursorBlink: s.cursorBlink,
    theme: resolveScheme(s.terminalScheme, s.theme, s.accent),
  };
}

let parking: HTMLDivElement | null = null;
function park(): HTMLDivElement {
  if (!parking) {
    parking = document.createElement("div");
    parking.style.cssText = "position:fixed;left:-10000px;top:0;width:900px;height:600px;visibility:hidden;";
    document.body.appendChild(parking);
  }
  return parking;
}

/** A buffer row's text, and the column each of its characters sits in. */
function rowText(line: IBufferLine, cols: number): { text: string; cells: number[] } {
  let text = "";
  const cells: number[] = [];
  for (let x = 0; x < cols; x++) {
    const cell = line.getCell(x);
    if (!cell) break;
    // The second half of a wide character has no text of its own.
    if (cell.getWidth() === 0) continue;
    const chars = cell.getChars() || " ";
    for (const ch of chars) {
      text += ch;
      cells.push(x);
    }
  }
  return { text, cells };
}

const OPEN_HINT = `${isMac ? "⌘" : "Ctrl"}+click to open`;

/** Keys and mouse reports that aren't typing: they don't end scrolling back. */
const NOT_TYPING = /^\x1b\[(?:<|M|I$|O$)/;
/**
 * Turns off mouse, focus and paste reports. Whatever asked for them (tmux,
 * an agent, vim) is gone with the connection; left on, every click and drag
 * meanwhile reached the next shell as text ("32;29;29M"). tmux asks again
 * once it's back.
 */
const STOP_REPORTS = "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1004l\x1b[?2004l";
/** tmux leaves scrolling back on this key (bound in `durable_command`). */
const LEAVE_SCROLL = "\x1b[9006~";

/** Copies to the system clipboard; a failure isn't worth interrupting anyone over. */
function copyText(text: string): Promise<void> {
  return backend.writeClipboard(text).catch(() => {});
}

/** A covered secret on screen: where, and what it is. */
interface SecretMark {
  row: number;
  x: number;
  width: number;
  text: string;
}

class TerminalHandle {
  readonly term: Terminal;
  readonly el: HTMLDivElement;
  private fit = new FitAddon();
  private gpu: WebglAddon | null = null;
  private started = false;
  private disposed = false;
  private fitTimer = 0;
  private last?: SessionState;
  private pendingCommand?: string;
  private output = new OutputPipeline((text) => this.term.write(text));
  /** Start of the current run of output nobody was watching, and the last output. */
  private busySince = 0;
  private lastOutput = 0;
  private quietNoted = true;
  /** When the connection dropped, and the lines that were on screen then. */
  private away: { since: number; anchor: string[]; current: string } | null = null;
  /** Where tmux on the server is scrolled to; null when the history is this terminal's own. */
  private tmux: TmuxScroll | null = null;
  private durable = false;
  /** The shell's folder, when it reports it (OSC 7). */
  private cwd: string | null = null;
  private scrollListeners = new Set<() => void>();
  private scrollFrame = 0;
  /** A tmux scroll to send once the one in flight is answered; null asks where it is. */
  private scrollWant: number | null | undefined;
  private scrollBusy = false;
  /** Paths and links on screen, faintly underlined so you can tell they open. */
  private linkLayer: HTMLDivElement | null = null;
  private markTimer = 0;
  /** Secrets on screen, covered with dots, and the ones you chose to see. */
  private secretLayer: HTMLDivElement | null = null;
  private secrets: SecretMark[] = [];
  private secretsDrawn = "";
  private revealed = new Set<string>();

  constructor(
    readonly paneId: string,
    readonly target: TargetRef,
    readonly sessionKey: string,
    initialCommand?: string,
    readonly container: ContainerRef | null = null,
  ) {
    this.pendingCommand = initialCommand;
    this.el = document.createElement("div");
    this.el.className = "term-host";
    park().appendChild(this.el);
    this.term = new Terminal({
      ...lookOptions(),
      cursorWidth: 2,
      scrollback: 10000,
      macOptionIsMeta: true,
      drawBoldTextInBrightColors: false,
      // Needed for the Unicode 11 widths below.
      allowProposedApi: true,
    });
    this.term.loadAddon(this.fit);
    // Emoji take two cells, as the shell and tmux on the server count them.
    // With xterm's default (Unicode 6) widths the cursor drifts after one.
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = "11";
    this.term.open(this.el);
    this.setGpu(useSettings.getState().gpuRendering);
    this.term.attachCustomKeyEventHandler((e) => {
      const copy = terminalCopyKey(e);
      if (copy) {
        const text = this.term.getSelection();
        if (text || copy === "ctrl-shift-c") {
          if (e.type === "keydown") {
            e.preventDefault();
            if (text) void copyText(text);
            this.term.clearSelection();
          }
          return false;
        }
      }
      // Shift+Page Up/Down scrolls back through the history, tmux's included.
      if (e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && (e.key === "PageUp" || e.key === "PageDown")) {
        if (e.type === "keydown") {
          e.preventDefault();
          const v = this.scrollView();
          this.scrollTo(v.top + (e.key === "PageUp" ? -1 : 1) * Math.max(1, v.rows - 1));
        }
        return false;
      }
      if (isMacClear(e)) {
        if (e.type === "keydown") {
          e.preventDefault();
          this.term.clear();
          noteMacClear();
        }
        return false;
      }
      if (isTerminalNewline(e)) {
        if (e.type === "keydown") {
          e.preventDefault();
          this.term.input("\n");
        }
        return false;
      }
      // App shortcuts win over the terminal. A paste key is left to the
      // browser, which fires the paste event xterm handles.
      return matchShortcut(e) === null && !isTerminalPaste(e);
    });
    this.term.onData((d) => {
      // Typing while scrolled back goes to the prompt, as in other terminals,
      // instead of tmux's copy mode.
      if (this.tmux?.inMode && !NOT_TYPING.test(d)) {
        backend.writeSession(paneId, LEAVE_SCROLL);
        this.setTmux({ ...this.tmux, inMode: false, position: 0 });
      }
      backend.writeSession(paneId, d);
    });
    // On Windows the console between us and ssh (ConPTY) answers "which
    // terminal are you?" (DA1) itself. Our answer came on top of it, and the
    // extra one landed in the shell as "61;6;7;21;22;23;24;28;32;42c".
    if (isWindows) this.term.parser.registerCsiHandler({ final: "c" }, (params) => params.length === 0 || params[0] === 0);
    // The scrollbar: tmux reports its scroll position in the title, and a
    // plain shell's history is this terminal's own.
    this.term.onTitleChange((title) => {
      const s = parseScrollTitle(title);
      if (s) this.setTmux(s);
    });
    this.term.onScroll(() => this.emitScroll());
    this.term.onWriteParsed(() => this.emitScroll());
    // tmux only reports on a redraw, so count lines going up into its history meanwhile.
    this.term.onLineFeed(() => {
      const t = this.tmux;
      if (!t || t.inMode || t.history >= t.limit || this.term.buffer.active.cursorY !== this.term.rows - 1) return;
      t.history++;
      this.emitScroll();
    });
    this.term.parser.registerOscHandler(7, (data) => {
      try {
        const path = decodeURIComponent(new URL(data).pathname);
        this.cwd = /^\/[A-Za-z]:/.test(path) ? path.slice(1) : path;
      } catch {
        /* not a file:// URL */
      }
      return true;
    });
    // Paths and links in the output open with Ctrl+click (⌘-click on a Mac).
    this.term.registerLinkProvider({
      provideLinks: (y, callback) => {
        const line = this.term.buffer.active.getLine(y - 1);
        if (!line) return callback(undefined);
        const { text, cells } = rowText(line, this.term.cols);
        const links = findLinks(text).map((link) => ({
          range: { start: { x: cells[link.start] + 1, y }, end: { x: cells[link.end - 1] + 1, y } },
          text: text.slice(link.start, link.end),
          decorations: { pointerCursor: true, underline: true },
          activate: (e: MouseEvent) => {
            if (!(isMac ? e.metaKey : e.ctrlKey)) return;
            void openTerminalLink({ paneId, target, sessionKey, durable: this.durable, cwd: this.cwd }, link);
          },
          hover: () => {
            this.el.title = OPEN_HINT;
          },
          leave: () => {
            this.el.title = "";
          },
        }));
        callback(links.length ? links : undefined);
      },
    });
    // Secrets are covered as the text is drawn, never a frame later.
    this.term.onRender(() => this.markSecrets());
    // Paths stay faintly underlined, not only under the mouse.
    this.term.onWriteParsed(() => this.markLinksSoon());
    this.term.onScroll(() => this.markLinksSoon());
    this.term.onResize(() => this.markLinksSoon());
    this.term.onResize(({ cols, rows }) => backend.resizeSession(paneId, cols, rows));
    // Programs that want you: AI agents and long jobs ring the bell or send
    // a notification (OSC 9 for iTerm2 and Windows Terminal, OSC 777 for others).
    this.term.onBell(() => this.notice({ kind: "input" }));
    this.term.parser.registerOscHandler(9, (data) => {
      if (data.startsWith("4;")) this.onProgress(data);
      // "9;9;C:\…" and the like are ConEmu's numbered commands (a prompt
      // reporting its folder), not a message for you.
      else if (!/^\d+;/.test(data)) this.notice({ kind: "input", message: data });
      return true;
    });
    // A program's own copy (OSC 52): tmux sends one for a mouse selection,
    // and Claude Code, OpenCode and vim for theirs. Reading the clipboard
    // back ("?") isn't allowed: a server has no business seeing it.
    this.term.parser.registerOscHandler(52, (data) => {
      const payload = data.slice(data.indexOf(";") + 1);
      if (payload && payload !== "?" && payload.length < 4_000_000) {
        try {
          void copyText(new TextDecoder().decode(Uint8Array.from(atob(payload), (c) => c.charCodeAt(0))));
        } catch {
          /* not base64 */
        }
      }
      return true;
    });
    // Selecting with the mouse copies, once the selection settles.
    let selecting = 0;
    this.term.onSelectionChange(() => {
      window.clearTimeout(selecting);
      if (!useSettings.getState().copyOnSelect || !this.term.hasSelection()) return;
      selecting = window.setTimeout(() => {
        const text = this.term.getSelection();
        if (text) void copyText(text);
      }, 250);
    });
    this.term.parser.registerOscHandler(777, (data) => {
      const [kind, title, body] = data.split(";");
      if (kind === "notify") this.notice({ kind: "input", message: [title, body].filter(Boolean).join(": ") });
      return true;
    });
  }

  /**
   * Draws with the GPU: far faster for programs that redraw a lot (Claude
   * Code, OpenCode, htop), and box-drawing and Powerline shapes join up
   * with no gaps between lines. Without WebGL, or once the browser takes the
   * context back (it caps how many a page has), the DOM renderer carries on.
   */
  setGpu(on: boolean) {
    if (this.disposed || on === !!this.gpu) return;
    if (!on) {
      this.gpu?.dispose();
      this.gpu = null;
      return;
    }
    try {
      const gpu = new WebglAddon();
      gpu.onContextLoss(() => {
        gpu.dispose();
        if (this.gpu === gpu) this.gpu = null;
      });
      this.term.loadAddon(gpu);
      this.gpu = gpu;
    } catch {
      /* no WebGL2 here */
    }
  }

  /** You're looking at this terminal right now. */
  private watched(): boolean {
    return document.hasFocus() && document.visibilityState === "visible" && useStore.getState().focused === this.paneId;
  }

  private notice(a: Omit<Attention, "at">) {
    if (this.disposed || this.watched()) return;
    const st = useStore.getState();
    const prev = st.attention[this.paneId];
    st.setAttention(this.paneId, { ...a, at: Date.now() });
    // One system notification per wait, only while SSHeesh is in the background.
    if (prev?.kind === a.kind || document.hasFocus() || !useSettings.getState().notifyAttention) return;
    const name = targetName(this.target);
    const message = a.message ? (streamerOn() ? maskText(a.message) : a.message) : undefined;
    void backend.notify(
      a.kind === "input" ? `${name} needs you` : `${name} went quiet`,
      message ?? (a.kind === "input" ? "A program in the terminal is waiting for you." : "Output stopped after a long run. It may be done."),
    );
  }

  /** OSC 9;4;state;value, the progress report Windows Terminal popularized. */
  private onProgress(data: string) {
    const [, state, value] = data.split(";");
    const states: Record<string, Progress["state"] | null> = { "0": null, "1": "normal", "2": "error", "3": "indeterminate", "4": "paused" };
    const kind = states[state ?? "0"];
    useStore.getState().setProgress(this.paneId, kind ? { state: kind, value: Math.max(0, Math.min(100, Number(value) || 0)) } : null);
  }

  /** Notes output for the "went quiet after a long run" check. */
  private noteOutput() {
    const now = Date.now();
    if (this.watched()) {
      this.busySince = 0;
    } else if (!this.busySince || now - this.lastOutput > 4000) {
      this.busySince = now;
      this.quietNoted = false;
    }
    this.lastOutput = now;
  }

  /** Reads the server's scrollback once tmux has redrawn, and keeps what's new. */
  private collectRecap(away: { since: number; anchor: string[]; current: string }) {
    if (!away.anchor.length) return;
    setTimeout(() => {
      if (this.disposed) return;
      backend
        .sessionHistory(this.target, this.sessionKey, 4000)
        .then((text) => {
          const recap = buildRecap(text, away.anchor, away.current, away.since);
          if (recap && !this.disposed) useStore.getState().setRecap(this.paneId, recap);
        })
        .catch(() => {});
    }, 700);
  }

  /** Called every few seconds: a long unwatched run that stopped means it's probably done. */
  checkQuiet(now: number) {
    if (this.quietNoted || !this.busySince) return;
    if (now - this.lastOutput >= 6000 && this.lastOutput - this.busySince >= 15000) {
      this.quietNoted = true;
      this.notice({ kind: "done" });
    }
  }

  mount(container: HTMLElement) {
    if (this.disposed) return;
    container.appendChild(this.el);
    this.refit();
    if (!this.started) this.start();
  }

  unmount() {
    if (!this.disposed && this.el.parentElement !== park()) park().appendChild(this.el);
  }

  /**
   * A new size redraws the whole screen on the server (tmux), so fitting on
   * every frame of a drag made the text jump up and down. It fits once the
   * size settles.
   */
  scheduleFit() {
    window.clearTimeout(this.fitTimer);
    this.fitTimer = window.setTimeout(() => this.refit(), 90);
  }

  // ---- scrollbar

  onScrollChange(fn: () => void): () => void {
    this.scrollListeners.add(fn);
    return () => this.scrollListeners.delete(fn);
  }

  private emitScroll() {
    cancelAnimationFrame(this.scrollFrame);
    this.scrollFrame = requestAnimationFrame(() => this.scrollListeners.forEach((fn) => fn()));
  }

  private setTmux(s: TmuxScroll | null) {
    this.tmux = s;
    this.emitScroll();
  }

  /** What the scrollbar shows. */
  scrollView(): ScrollView {
    if (this.tmux) return tmuxView(this.tmux);
    const b = this.term.buffer.active;
    return { total: b.length, top: b.viewportY, rows: this.term.rows };
  }

  /** Scrolls so line `top` is the first in view. */
  scrollTo(top: number) {
    const t = this.tmux;
    if (!t) {
      this.term.scrollToLine(Math.max(0, Math.round(top)));
      return;
    }
    const position = tmuxPosition(t, top);
    this.setTmux({ ...t, inMode: position > 0, position });
    this.askTmux(position);
  }

  /** Asks tmux how much history there is, for an exact scrollbar. */
  refreshScroll() {
    if (this.tmux) this.askTmux(null);
  }

  /** One tmux scroll at a time; while one is out, only the latest wish waits. */
  private askTmux(position: number | null) {
    this.scrollWant = position;
    if (this.scrollBusy) return;
    this.scrollBusy = true;
    const next = () => {
      const want = this.scrollWant;
      this.scrollWant = undefined;
      if (want === undefined || this.disposed) {
        this.scrollBusy = false;
        return;
      }
      backend
        .tmuxScroll(this.target, this.sessionKey, want)
        .then((s) => {
          // Mid-drag the next wish is already shown; don't jump back.
          if (this.scrollWant === undefined && this.tmux) this.setTmux(s);
        })
        .catch(() => {})
        .finally(next);
    };
    next();
  }

  refit() {
    if (this.disposed || !this.el.isConnected || this.el.parentElement === parking) return;
    try {
      this.fit.fit();
    } catch {
      /* not laid out yet */
    }
  }

  focus() {
    this.term.focus();
  }

  applyLook() {
    if (this.disposed) return;
    Object.assign(this.term.options, lookOptions());
    // The GPU renderer keeps glyphs it drew before a font arrived.
    this.term.clearTextureAtlas();
    this.scheduleFit();
  }

  /** (Re)starts the session: first open, or after it ended/failed. */
  start() {
    this.started = true;
    if (this.target.kind === "host" && !verifiedHosts.has(this.target.hostId)) {
      void this.checkIdentity(false);
      return;
    }
    this.open();
  }

  /**
   * Before the first connection to a server this run: is its identity (host
   * key) trusted? If not, the pane asks you instead of ssh asking yes/no.
   */
  async checkIdentity(changed: boolean) {
    if (this.target.kind !== "host") return this.open();
    const hostId = this.target.hostId;
    const st = useStore.getState();
    st.setSession(this.paneId, { kind: "connecting", attempt: 1 });
    let check: HostKeyCheck;
    try {
      check = await backend.checkHostKey(hostId, changed);
    } catch {
      check = { kind: "unreachable", message: "" };
    }
    if (this.disposed) return;
    if (check.kind === "unknown" || check.kind === "changed") {
      st.setHostKeyPrompt(this.paneId, check);
      return;
    }
    if (check.kind === "known") verifiedHosts.add(hostId);
    this.open();
  }

  /** Your answer to the identity question. Trusting it opens every terminal waiting on that server. */
  async answerIdentity(trust: boolean) {
    if (this.target.kind !== "host") return;
    const hostId = this.target.hostId;
    const st = useStore.getState();
    st.setHostKeyPrompt(this.paneId, null);
    if (!trust) {
      st.setSession(this.paneId, { kind: "failed", message: "Not connected. You didn't confirm this server's identity." });
      return;
    }
    try {
      await backend.trustHostKey(hostId);
    } catch (e) {
      st.setSession(this.paneId, { kind: "failed", message: toOpError(e).message });
      return;
    }
    verifiedHosts.add(hostId);
    for (const h of handles.values()) {
      if (h === this || h.target.kind !== "host" || h.target.hostId !== hostId || !st.hostKeyPrompts[h.paneId]) continue;
      st.setHostKeyPrompt(h.paneId, null);
      h.open();
    }
    this.open();
  }

  /** What this session shows first, from Settings; "first time" is decided here, once per server. */
  private welcome(): WelcomeSpec | undefined {
    if (this.target.kind !== "host" || this.container) return undefined;
    const s = useSettings.getState();
    let mode: WelcomeSpec["mode"] = s.welcomeMode === "first" ? "auto" : s.welcomeMode;
    if (s.welcomeMode === "first" && !s.welcomedHosts.includes(this.target.hostId)) {
      mode = "ssheesh";
      s.update({ welcomedHosts: [...s.welcomedHosts, this.target.hostId] });
    }
    return { mode, parts: s.welcomeParts, note: s.welcomeNote };
  }

  private markLinksSoon() {
    window.clearTimeout(this.markTimer);
    this.markTimer = window.setTimeout(() => this.markLinks(), 250);
  }

  /**
   * Underlines the paths and links on screen, on a layer of its own over the
   * text. It works on tmux's screen too (tmux draws on the terminal's second
   * screen, where xterm keeps no marks), and it's drawn again from what's on
   * screen each time, so no line keeps a mark its text has lost.
   */
  private markLinks() {
    if (this.disposed) return;
    const screen = this.term.element?.querySelector<HTMLElement>(".xterm-screen");
    if (!screen) return;
    if (this.linkLayer?.parentElement !== screen) {
      this.linkLayer = document.createElement("div");
      this.linkLayer.className = "term-link-layer";
      screen.appendChild(this.linkLayer);
    }
    const { cols, rows } = this.term;
    const w = screen.clientWidth / cols;
    const h = screen.clientHeight / rows;
    const buf = this.term.buffer.active;
    const marks = document.createDocumentFragment();
    for (let r = 0; r < rows; r++) {
      const line = buf.getLine(buf.viewportY + r);
      if (!line) continue;
      const { text, cells } = rowText(line, cols);
      for (const link of findLinks(text)) {
        const x = cells[link.start];
        const mark = document.createElement("span");
        mark.className = "term-link-mark";
        mark.style.cssText = `left:${x * w}px;top:${(r + 1) * h - 2}px;width:${(cells[link.end - 1] + 1 - x) * w}px`;
        marks.appendChild(mark);
      }
    }
    this.linkLayer.replaceChildren(marks);
  }

  /**
   * Covers tokens, keys and passwords on screen with dots, on a layer over
   * the text as the link marks are. It's drawn again whenever the terminal
   * draws (onRender), in that same frame, so a secret never shows for a
   * moment first. A click shows that secret, except while streaming.
   */
  markSecrets() {
    if (this.disposed) return;
    const screen = this.term.element?.querySelector<HTMLElement>(".xterm-screen");
    if (!screen) return;
    const found = secretsHidden() ? this.secretsOnScreen() : [];
    const { cols, rows } = this.term;
    const w = screen.clientWidth / cols;
    const h = screen.clientHeight / rows;
    const theme = this.term.options.theme ?? {};
    const drawn = `${w}|${h}|${theme.background}|${theme.foreground}|${found.map((f) => `${f.row},${f.x},${f.width}`).join(";")}`;
    if (drawn === this.secretsDrawn && this.secretLayer?.parentElement === screen) return;
    this.secretsDrawn = drawn;
    this.secrets = found;
    if (this.secretLayer?.parentElement !== screen) {
      const layer = document.createElement("div");
      layer.className = "term-secret-layer";
      // Clicks on a covered secret are its own, not the start of a selection.
      layer.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
      });
      layer.addEventListener("click", (e) => {
        e.stopPropagation();
        const i = Number((e.target as HTMLElement).dataset.secret);
        const mark = this.secrets[i];
        if (!mark || streamerOn()) return;
        this.revealed.add(mark.text);
        this.secretsDrawn = "";
        this.markSecrets();
      });
      this.secretLayer = layer;
      screen.appendChild(layer);
    }
    // The cover is the terminal's own background, so nothing shows through.
    const bg = (theme.background ?? "#000000").slice(0, 7);
    this.secretLayer.style.setProperty("--secret-bg", bg);
    this.secretLayer.style.setProperty("--secret-dot", `color-mix(in srgb, ${theme.foreground ?? "#ffffff"} 55%, ${bg})`);
    this.secretLayer.style.setProperty("--secret-cell", `${w}px`);
    const title = streamerOn() ? "Hidden while streaming" : "Hidden secret. Click to show it";
    const marks = document.createDocumentFragment();
    found.forEach((f, i) => {
      const mark = document.createElement("span");
      mark.className = "term-secret";
      mark.dataset.secret = String(i);
      mark.title = title;
      mark.style.cssText = `left:${f.x * w}px;top:${f.row * h}px;width:${f.width * w}px;height:${h}px`;
      marks.appendChild(mark);
    });
    this.secretLayer.replaceChildren(marks);
  }

  /** Where the secrets on screen are: a mark per row each one covers. */
  private secretsOnScreen(): SecretMark[] {
    const buf = this.term.buffer.active;
    const { cols, rows } = this.term;
    const top = buf.viewportY;
    const plain = (y: number) => buf.getLine(y)?.translateToString(true) ?? "";
    const texts = Array.from({ length: rows }, (_, r) => plain(top + r));
    // A key's body on screen may have its BEGIN line above it.
    const lookBack = texts.some(looksLikeKeyBody);
    const above = lookBack ? Array.from({ length: Math.min(top, 80) }, (_, i) => plain(top - Math.min(top, 80) + i)) : [];
    const keys = keyLines(texts, keyOpenAfter(above));
    // A row that runs to the last column may go on in the next (a long token wraps).
    const runsOn = (y: number) => !!buf.getLine(y + 1)?.isWrapped || (buf.getLine(y)?.getCell(cols - 1)?.getChars() ?? "").trim() !== "";
    const found: SecretMark[] = [];
    let r = 0;
    while (r < rows) {
      if (keys[r]) {
        const line = buf.getLine(top + r);
        const { text, cells } = line ? rowText(line, cols) : { text: "", cells: [] };
        const first = text.search(/\S/);
        const last = text.trimEnd().length - 1;
        if (first >= 0) found.push({ row: r, x: cells[first], width: cells[last] + 1 - cells[first], text: `key:${top + r}` });
        r++;
        continue;
      }
      let end = r;
      while (end + 1 < rows && !keys[end + 1] && runsOn(top + end)) end++;
      // Most lines have nothing: the quick look skips the exact one.
      if (findSecrets(texts.slice(r, end + 1).join("")).length) {
        let text = "";
        const at: { row: number; col: number }[] = [];
        for (let k = r; k <= end; k++) {
          const line = buf.getLine(top + k);
          if (!line) continue;
          const t = rowText(line, cols);
          text += t.text;
          for (const col of t.cells) at.push({ row: k, col });
        }
        for (const s of findSecrets(text)) {
          const secret = text.slice(s.start, s.end);
          if (this.revealed.has(secret)) continue;
          for (let i = s.start; i < s.end; ) {
            let j = i;
            while (j + 1 < s.end && at[j + 1].row === at[i].row) j++;
            found.push({ row: at[i].row, x: at[i].col, width: at[j].col + 1 - at[i].col, text: secret });
            i = j + 1;
          }
        }
      }
      r = end + 1;
    }
    return found;
  }

  private open() {
    const { cols, rows } = this.term;
    backend
      .openSession(this.paneId, this.target, this.sessionKey, cols, rows, this.container, (e) => this.onEvent(e), this.welcome())
      .catch((e) => useStore.getState().setSession(this.paneId, { kind: "failed", message: toOpError(e).message }));
  }

  reconnectNow() {
    if (this.last?.kind === "reconnecting" || this.last?.kind === "disconnected") backend.reconnectSession(this.paneId);
    else this.start();
  }

  private onEvent(e: SessionEvent) {
    if (this.disposed) return;
    if (e.type === "output") {
      this.output.push(e.data);
      this.noteOutput();
      return;
    }
    const prev = this.last;
    this.last = e.state;
    if (e.state.kind === "connected") {
      this.durable = e.state.durable;
      // A new shell has its history here, not in tmux.
      if (!e.state.durable) this.setTmux(null);
    }
    useStore.getState().setSession(this.paneId, e.state);
    if (prev?.kind === "connected" && e.state.kind !== "connected") {
      this.output.flush();
      this.term.write(STOP_REPORTS);
    }
    const down = e.state.kind === "reconnecting" || e.state.kind === "disconnected";
    if (down && !this.away && this.target.kind === "host") this.away = { since: Date.now(), anchor: anchorLines(this.term), current: cursorLine(this.term) };
    if (e.state.kind === "connected" && this.away) {
      const away = this.away;
      this.away = null;
      if (e.state.durable) this.collectRecap(away);
    }
    if (e.state.kind === "connected") {
      if ((prev?.kind === "reconnecting" || prev?.kind === "disconnected") && !e.state.durable) {
        this.term.write("\r\n\x1b[2m── reconnected · this is a new shell ──\x1b[0m\r\n");
      }
      if (this.pendingCommand) {
        const cmd = this.pendingCommand;
        this.pendingCommand = undefined;
        setTimeout(() => backend.writeSession(this.paneId, `${cmd}\r`), 150);
      }
    }
  }

  /**
   * Wipes what's on screen and asks the server side to draw it again, so a
   * switch of streamer mode applies to text that was already there. tmux
   * redraws on a size change; a plain shell shows a fresh prompt on Enter.
   */
  redraw() {
    if (this.disposed || !this.started) return;
    this.term.reset();
    const { cols, rows } = this.term;
    backend.resizeSession(this.paneId, cols, Math.max(2, rows - 1));
    setTimeout(() => backend.resizeSession(this.paneId, cols, rows), 80);
  }

  dispose(endRemote: boolean) {
    if (this.disposed) return;
    this.disposed = true;
    window.clearTimeout(this.markTimer);
    backend.closeSession(this.paneId, this.target, this.sessionKey, endRemote);
    this.term.dispose();
    this.el.remove();
  }
}

const handles = new Map<string, TerminalHandle>();
/** Servers whose identity was confirmed this run. */
const verifiedHosts = new Set<string>();

export function getTerminal(pane: PaneState): TerminalHandle {
  let h = handles.get(pane.id);
  if (!h) {
    h = new TerminalHandle(pane.id, pane.target ?? { kind: "local" }, pane.sessionKey ?? pane.id, pane.initialCommand, pane.container ?? null);
    handles.set(pane.id, h);
    if (pane.initialCommand) useStore.getState().setPane(pane.id, { initialCommand: undefined });
  }
  return h;
}

export function findTerminal(paneId: string): TerminalHandle | undefined {
  return handles.get(paneId);
}

/** The first ⌘K: say where the command bar went, once. */
function noteMacClear() {
  if (useSettings.getState().hintsSeen.includes("mac-clear")) return;
  markHintSeen("mac-clear");
  useStore.getState().toast({ tone: "info", title: "⌘K clears the terminal now", body: "As in other Mac terminals. The command bar moved to ⌘L." });
}

export function disposeTerminal(paneId: string, endRemote: boolean, pane?: PaneState) {
  const h = handles.get(paneId);
  handles.delete(paneId);
  if (h) return h.dispose(endRemote);
  // A tab you never opened this run has no terminal here, but its session
  // still runs on the server: end it too.
  if (endRemote && pane?.target?.kind === "host" && pane.sessionKey) backend.closeSession(paneId, pane.target, pane.sessionKey, true);
}

// Settings changes restyle every open terminal; web fonts may arrive after
// terminals measured their cells.
const LOOK_KEYS = ["terminalScheme", "terminalColors", "terminalFont", "terminalFontSize", "terminalLineHeight", "cursorStyle", "cursorBlink", "theme", "accent"] as const;
useSettings.subscribe((s, prev) => {
  if (LOOK_KEYS.every((k) => s[k] === prev[k])) return;
  for (const h of handles.values()) h.applyLook();
});
if (typeof window !== "undefined") {
  setInterval(() => {
    const now = Date.now();
    for (const h of handles.values()) h.checkQuiet(now);
  }, 2000);
  // Coming back to the window counts as seeing the focused terminal.
  window.addEventListener("focus", () => {
    const st = useStore.getState();
    if (st.attention[st.focused]) st.setAttention(st.focused, null);
  });
}

useSettings.subscribe((s, prev) => {
  if (s.gpuRendering === prev.gpuRendering) return;
  for (const h of handles.values()) h.setGpu(s.gpuRendering);
});
useSettings.subscribe((s, prev) => {
  if (s.hideSecrets === prev.hideSecrets) return;
  for (const h of handles.values()) h.markSecrets();
});
useSettings.subscribe((s, prev) => {
  if (s.streamerMode === prev.streamerMode) return;
  for (const h of handles.values()) h.redraw();
});
if (typeof document !== "undefined" && document.fonts) {
  // The icon font loads up front, so a prompt's icons are there on first draw.
  const icons = document.fonts.load(`13px ${ICON_FONT}`, "\ue0b0").catch(() => []);
  void Promise.all([document.fonts.ready, icons]).then(() => {
    for (const h of handles.values()) h.applyLook();
  });
}
