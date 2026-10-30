// xterm instances live outside React. A pane can move around the split tree
// (which remounts its component), but the terminal and its scrollback must
// survive that, so each pane's terminal is created once and its DOM node is
// re-parented into whichever container is showing it.

import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { backend } from "./backend";
import { matchShortcut } from "./keys";
import { useSettings } from "./settings";
import { fontStack, resolveScheme } from "./themes";
import { useStore, type PaneState } from "./store";
import type { ContainerRef, SessionEvent, SessionState, TargetRef } from "./types";
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
      this.term.write(e.data);
      return;
    }
    const prev = this.last;
    this.last = e.state;
    useStore.getState().setSession(this.paneId, e.state);
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

  dispose(endRemote: boolean) {
    if (this.disposed) return;
    this.disposed = true;
    backend.closeSession(this.paneId, this.target, this.sessionKey, endRemote);
    this.term.dispose();
    this.el.remove();
  }
}

const handles = new Map<string, TerminalHandle>();

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
if (typeof document !== "undefined") {
  void document.fonts?.ready.then(() => {
    for (const h of handles.values()) h.applyLook();
  });
}
