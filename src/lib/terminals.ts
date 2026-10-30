// xterm instances live outside React. A pane can move around the split tree
// (which remounts its component), but the terminal and its scrollback must
// survive that, so each pane's terminal is created once and its DOM node is
// re-parented into whichever container is showing it.

import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { backend } from "./backend";
import { matchShortcut } from "./keys";
import { anchorLines, buildRecap, cursorLine } from "./away";
import { OutputPipeline } from "./output";
import { maskText, streamerOn, targetName } from "./privacy";
import { useSettings } from "./settings";
import { fontStack, resolveScheme } from "./themes";
import { useStore, type Attention, type PaneState, type Progress } from "./store";
import type { ContainerRef, HostKeyCheck, SessionEvent, SessionState, TargetRef } from "./types";
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

class TerminalHandle {
  readonly term: Terminal;
  readonly el: HTMLDivElement;
  private fit = new FitAddon();
  private started = false;
  private disposed = false;
  private fitFrame = 0;
  private last?: SessionState;
  private pendingCommand?: string;
  private output = new OutputPipeline((text) => this.term.write(text));
  /** Start of the current run of output nobody was watching, and the last output. */
  private busySince = 0;
  private lastOutput = 0;
  private quietNoted = true;
  /** When the connection dropped, and the lines that were on screen then. */
  private away: { since: number; anchor: string[]; current: string } | null = null;

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
    });
    this.term.loadAddon(this.fit);
    this.term.open(this.el);
    // App shortcuts win over the terminal.
    this.term.attachCustomKeyEventHandler((e) => matchShortcut(e) === null);
    this.term.onData((d) => backend.writeSession(paneId, d));
    this.term.onResize(({ cols, rows }) => backend.resizeSession(paneId, cols, rows));
    // Programs that want you: AI agents and long jobs ring the bell or send
    // a notification (OSC 9 for iTerm2 and Windows Terminal, OSC 777 for others).
    this.term.onBell(() => this.notice({ kind: "input" }));
    this.term.parser.registerOscHandler(9, (data) => {
      if (data.startsWith("4;")) this.onProgress(data);
      else this.notice({ kind: "input", message: data });
      return true;
    });
    this.term.parser.registerOscHandler(777, (data) => {
      const [kind, title, body] = data.split(";");
      if (kind === "notify") this.notice({ kind: "input", message: [title, body].filter(Boolean).join(": ") });
      return true;
    });
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

  scheduleFit() {
    cancelAnimationFrame(this.fitFrame);
    this.fitFrame = requestAnimationFrame(() => this.refit());
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

  private open() {
    const { cols, rows } = this.term;
    backend
      .openSession(this.paneId, this.target, this.sessionKey, cols, rows, this.container, (e) => this.onEvent(e))
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
    useStore.getState().setSession(this.paneId, e.state);
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

export function disposeTerminal(paneId: string, endRemote: boolean) {
  handles.get(paneId)?.dispose(endRemote);
  handles.delete(paneId);
}

// Settings changes restyle every open terminal; web fonts may arrive after
// terminals measured their cells.
const LOOK_KEYS = ["terminalScheme", "terminalFont", "terminalFontSize", "terminalLineHeight", "cursorStyle", "cursorBlink", "theme", "accent"] as const;
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
  if (s.streamerMode === prev.streamerMode) return;
  for (const h of handles.values()) h.redraw();
});
if (typeof document !== "undefined") {
  void document.fonts?.ready.then(() => {
    for (const h of handles.values()) h.applyLook();
  });
}
