import { create } from "zustand";
import { persist } from "zustand/middleware";
import { useShallow } from "zustand/react/shallow";
import { backend } from "./backend";
import { arrange, movePane, newId, paneIds, removePane, splitAt, visibleTree, type Arrangement, type Dir, type DropZone, type LayoutNode } from "./layout";
import { quietStorage } from "./persist";
import { useSettings } from "./settings";
import type { ContainerRef, Host, HostKeyCheck, SessionState, TargetRef } from "./types";
import { isMainWindow } from "./windowInfo";
import { sameTarget, targetKey, toOpError } from "./types";

export type PaneKind = "empty" | "terminal" | "files" | "stats";

export interface PaneState {
  id: string;
  kind: PaneKind;
  target: TargetRef | null;
  /** Names the tmux session on the server; stable so reopening reattaches. */
  sessionKey?: string;
  /** Files pane: current folder. */
  path?: string;
  /** Files pane: a file in that folder to pick out and preview, once. */
  reveal?: string;
  /** Terminal pane: typed once after the first connect (e.g. `cd` into a folder). */
  initialCommand?: string;
  /** Terminal pane: opened inside this container. */
  container?: ContainerRef;
  /** "Terminal 2": the lowest number free among the machine's panes of this kind when it opened. It never changes. */
  n?: number;
}

/** The lowest number no other pane of this kind on this machine has. */
function freeNumber(panes: Record<string, PaneState>, kind: PaneKind, target: TargetRef | null, except?: string): number {
  const taken = new Set(Object.values(panes).filter((p) => p.id !== except && p.kind === kind && sameTarget(p.target, target)).map((p) => p.n ?? 1));
  let n = 1;
  while (taken.has(n)) n++;
  return n;
}

export interface Toast {
  id: string;
  tone: "info" | "success" | "error";
  title: string;
  body?: string;
}

export interface ConfirmRequest {
  title: string;
  body?: string;
  confirmLabel: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}

/** A terminal wants you: a program rang the bell or sent a notification, or a long run went quiet. */
export interface Attention {
  kind: "input" | "done";
  message?: string;
  at: number;
}

/** What a durable session printed while the connection was down. */
export interface Recap {
  since: number;
  until: number;
  lines: string[];
  errors: number;
  warnings: number;
}

/** Progress a program reported with OSC 9;4, shown as a bar under the pane title. */
export interface Progress {
  state: "normal" | "error" | "paused" | "indeterminate";
  /** 0 to 100. */
  value: number;
}

/** A tab you closed, to bring back with Ctrl+Shift+Z or from Home. */
export interface ClosedTab {
  kind: Exclude<PaneKind, "empty">;
  target: TargetRef;
  path?: string;
  container?: ContainerRef;
  closedAt: number;
}

const CLOSED_KEPT = 20;

interface OpenOptions {
  path?: string;
  initialCommand?: string;
  /** Pane to split from; defaults to the focused one. */
  from?: string;
  dir?: Dir;
  /** Always open in a new split, even if the source pane is empty. */
  split?: boolean;
  /** Put the new pane left of (or above) the source instead of after it. */
  before?: boolean;
  container?: ContainerRef;
}

interface AppState {
  hosts: Host[];
  hostsLoaded: boolean;
  /**
   * Each machine's panes and how they're arranged, by target key. Only one
   * machine is on screen at a time, so each keeps its own layout.
   */
  layouts: Record<string, LayoutNode>;
  /** Machines shown side by side (true) or one tab at a time (false); the rest follow Settings. */
  splitOn: Record<string, boolean>;
  panes: Record<string, PaneState>;
  /** The pane you're on; on the main menu, the one you'll come back to. */
  focused: string;
  sessions: Record<string, SessionState | undefined>;
  paletteOpen: boolean;
  addHostOpen: boolean;
  toasts: Toast[];
  confirmRequest: ConfirmRequest | null;
  /** Files pane currently under a file being dragged in from the desktop. */
  dropPane: string | null;
  settingsOpen: boolean;
  settingsSection: string;
  /** A pane temporarily filling the whole workspace. */
  zoomed: string | null;
  /**
   * Panes switched off in the toolbar: kept, with their sessions running,
   * but not drawn. Focusing one brings it back.
   */
  hidden: string[];
  /** Switches panes off (or back on) without closing them. */
  setHidden(ids: string[], hidden: boolean): void;
  /** Tabs in the order you dragged them to; panes not in it follow in layout order. */
  tabOrder: string[];
  /** Puts these tabs in this order, ahead of the rest. */
  setTabOrder(ids: string[]): void;
  /** Pane being dragged by its header, and where it would land. */
  dragPane: string | null;
  dropTarget: { paneId: string; zone: DropZone } | null;

  loadHosts(): Promise<void>;
  focus(id: string): void;
  attention: Record<string, Attention>;
  progress: Record<string, Progress>;
  setAttention(id: string, a: Attention | null): void;
  setProgress(id: string, p: Progress | null): void;
  /** A server identity question waiting on a terminal pane. */
  hostKeyPrompts: Record<string, HostKeyCheck>;
  setHostKeyPrompt(id: string, c: HostKeyCheck | null): void;
  recaps: Record<string, Recap>;
  setRecap(id: string, r: Recap | null): void;
  /** Last terminal you used on each machine, by target key. */
  lastTerminal: Record<string, string>;
  /** The terminal you'd call "the" terminal for a machine: the one you used last. */
  mainTerminalFor(target: TargetRef): string | null;
  setPane(id: string, patch: Partial<PaneState>): void;
  setSession(id: string, state: SessionState): void;
  openIn(kind: Exclude<PaneKind, "empty">, target: TargetRef, opts?: OpenOptions): string;
  split(id: string, dir: Dir): string;
  closePane(id: string): void;
  setPaletteOpen(open: boolean): void;
  setAddHostOpen(open: boolean): void;
  /** Shows a note in the corner; returns its id, to take it away early. */
  toast(t: Omit<Toast, "id">): string;
  dismissToast(id: string): void;
  confirm(req: Omit<ConfirmRequest, "resolve">): Promise<boolean>;
  setDropPane(id: string | null): void;
  openSettings(section?: string): void;
  toggleZoom(id?: string): void;
  closeSettings(): void;
  setDrag(id: string | null): void;
  setDropTarget(t: { paneId: string; zone: DropZone } | null): void;
  movePane(dragId: string, targetId: string, zone: DropZone): void;
  arrangePanes(how: Arrangement): void;
  /** Terminal + files for one machine, side by side. */
  openWorkspace(target: TargetRef): void;
  /** One tab at a time or side by side, for one machine. */
  setSplit(target: TargetRef, split: boolean): void;
  /** The main menu is showing instead of a server's tabs. */
  home: boolean;
  showHome(): void;
  /** The tab you were on last on each machine, by target key. */
  lastFocusedFor: Record<string, string>;
  /** Goes to a machine: the tab you were on there, or a new terminal. */
  goTo(target: TargetRef): void;
  /** Newest first. */
  closedTabs: ClosedTab[];
  /** Brings back the tab closed last, or the one given. */
  reopenClosed(at?: number): void;
  /** What each terminal is running right now ("claude", "vim"), when known. */
  foreground: Record<string, string>;
}

type View = Pick<AppState, "home" | "panes" | "focused">;

/** The main menu is showing: asked for, or nothing of a machine is open. */
export function showingHome(s: View): boolean {
  return s.home || !s.panes[s.focused]?.target;
}

/** The pane you're on, or null on the main menu. Shortcuts and commands act on this. */
export function activePane(s: View): PaneState | null {
  return showingHome(s) ? null : s.panes[s.focused];
}

/** The machine on screen, or null on the main menu. */
export function currentTarget(s: View): TargetRef | null {
  return activePane(s)?.target ?? null;
}

const machine = (p: PaneState | undefined): string | null => (p?.target ? targetKey(p.target) : null);

const NO_IDS: string[] = [];

/** A machine's panes, in their layout's order. */
export function panesOn(s: Pick<AppState, "layouts" | "panes">, target: TargetRef): PaneState[] {
  const layout = s.layouts[targetKey(target)];
  return (layout ? paneIds(layout) : NO_IDS).map((id) => s.panes[id]).filter((p): p is PaneState => !!p);
}

/** A machine's tabs: in the order you dragged them to, the rest as they were opened. */
export function tabsOn(s: Pick<AppState, "layouts" | "panes" | "tabOrder">, target: TargetRef): PaneState[] {
  const rank = (id: string) => {
    const i = s.tabOrder.indexOf(id);
    return i < 0 ? Number.MAX_SAFE_INTEGER : i;
  };
  return panesOn(s, target)
    .filter((p) => p.kind !== "empty")
    .map((p, i) => ({ p, i }))
    .sort((a, b) => rank(a.p.id) - rank(b.p.id) || a.i - b.i)
    .map((x) => x.p);
}

/** Side by side on this machine: your choice for it, or the one in Settings. */
export function isSplit(s: Pick<AppState, "splitOn">, target: TargetRef | null): boolean {
  const own = target ? s.splitOn[targetKey(target)] : undefined;
  return own ?? useSettings.getState().viewMode === "split";
}

/** The same, as a hook that follows both your choice and Settings. */
export function useSplit(target: TargetRef | null): boolean {
  const own = useStore((s) => (target ? s.splitOn[targetKey(target)] : undefined));
  const fallback = useSettings((s) => s.viewMode === "split");
  return own ?? fallback;
}

type SplitView = Pick<AppState, "splitOn" | "hidden" | "lastFocusedFor">;

/**
 * Panes side by side on screen for this machine right now. With side by side
 * as your layout (Settings), whenever the machine is split. With one tab at a
 * time, the tabs you put side by side are a pair, as in a browser: another
 * tab shows on its own, and going back to one of the pair shows both again.
 */
export function sideBySide(s: SplitView, target: TargetRef | null): boolean {
  if (!isSplit(s, target)) return false;
  if (useSettings.getState().viewMode === "split" || !target) return true;
  const front = s.lastFocusedFor[targetKey(target)];
  return !front || !s.hidden.includes(front);
}

/** The same, as a hook. */
export function useSideBySide(target: TargetRef | null): boolean {
  const split = useSplit(target);
  const asTabs = useSettings((s) => s.viewMode === "tabs");
  const alone = useStore((s) => {
    const front = target ? s.lastFocusedFor[targetKey(target)] : undefined;
    return !!front && s.hidden.includes(front);
  });
  return split && !(asTabs && alone);
}

/** The tab you were last on there, if it's still there; else that machine's first tab. */
function anchorOn(s: AppState, key: string): string | undefined {
  const layout = s.layouts[key];
  if (!layout) return undefined;
  const ids = paneIds(layout);
  const last = s.lastFocusedFor[key];
  return last && ids.includes(last) ? last : ids[0];
}

/**
 * What every way of landing on a pane shares: it's on screen, it's the one
 * you'd come back to on its machine, and what it asked you for is seen.
 */
function focusing(s: AppState, panes: Record<string, PaneState>, id: string): Partial<AppState> {
  const p = panes[id];
  const key = machine(p);
  const patch: Partial<AppState> = { focused: id, home: false };
  // With side by side as the layout, going to a tab puts it back on screen.
  // As tabs, a tab outside the pair side by side shows on its own instead.
  if (s.hidden.includes(id) && useSettings.getState().viewMode === "split") patch.hidden = s.hidden.filter((h) => h !== id);
  if (key) patch.lastFocusedFor = { ...s.lastFocusedFor, [key]: id };
  if (key && p?.kind === "terminal") patch.lastTerminal = { ...s.lastTerminal, [key]: id };
  // Going to another pane stops one filling the window: the pane you go to is always on screen.
  if (s.zoomed && s.zoomed !== id) patch.zoomed = null;
  if (s.attention[id]) {
    const { [id]: _seen, ...rest } = s.attention;
    patch.attention = rest;
  }
  return patch;
}

export function sessionKey(): string {
  return Math.random().toString(36).slice(2, 12);
}

export const useStore = create<AppState>()(
  persist(
    (set, get) => ({
      hosts: [],
      hostsLoaded: false,
      layouts: {},
      splitOn: {},
      panes: {},
      focused: "",
      sessions: {},
      paletteOpen: false,
      addHostOpen: false,
      toasts: [],
      confirmRequest: null,
      dropPane: null,

      async loadHosts() {
        try {
          const hosts = await backend.listHosts();
          set({ hosts, hostsLoaded: true });
        } catch (e) {
          set({ hostsLoaded: true });
          get().toast({ tone: "error", title: "Couldn't read your servers", body: toOpError(e).message });
        }
      },

      focus(id) {
        const s = get();
        if (!s.panes[id] || (s.focused === id && !s.home && !s.hidden.includes(id))) return;
        set(focusing(s, s.panes, id));
      },

      home: true,
      showHome: () => set({ home: true, zoomed: null }),

      lastFocusedFor: {},
      goTo(target) {
        const there = anchorOn(get(), targetKey(target));
        if (there) get().focus(there);
        else get().openIn("terminal", target);
      },

      closedTabs: [],
      reopenClosed(at = 0) {
        const s = get();
        const c = s.closedTabs[at];
        if (!c) return;
        set({ closedTabs: s.closedTabs.filter((_, i) => i !== at) });
        get().openIn(c.kind, c.target, { path: c.path, container: c.container });
      },

      foreground: {},

      attention: {},
      progress: {},

      setAttention(id, a) {
        set((s) => {
          const next = { ...s.attention };
          if (a) next[id] = a;
          else delete next[id];
          return { attention: next };
        });
      },

      recaps: {},

      hostKeyPrompts: {},

      setHostKeyPrompt(id, c) {
        set((s) => {
          const next = { ...s.hostKeyPrompts };
          if (c) next[id] = c;
          else delete next[id];
          return { hostKeyPrompts: next };
        });
      },

      setRecap(id, r) {
        set((s) => {
          const next = { ...s.recaps };
          if (r) next[id] = r;
          else delete next[id];
          return { recaps: next };
        });
      },

      setProgress(id, p) {
        set((s) => {
          const next = { ...s.progress };
          if (p) next[id] = p;
          else delete next[id];
          return { progress: next };
        });
      },

      lastTerminal: {},

      mainTerminalFor(target) {
        const s = get();
        // The server's own shell: never a container's shell or its logs.
        const isOn = (id: string | undefined) => !!id && s.panes[id]?.kind === "terminal" && !s.panes[id].container && sameTarget(s.panes[id].target, target);
        const last = s.lastTerminal[targetKey(target)];
        if (isOn(last)) return last;
        return panesOn(s, target).find((p) => isOn(p.id))?.id ?? null;
      },

      setPane(id, patch) {
        set((s) => (s.panes[id] ? { panes: { ...s.panes, [id]: { ...s.panes[id], ...patch } } } : s));
      },

      setSession(id, state) {
        set((s) => ({ sessions: { ...s.sessions, [id]: state } }));
      },

      openIn(kind, target, opts = {}) {
        const s = get();
        const key = targetKey(target);
        // It opens next to a tab of the same machine: the one you named, else the one you were on there.
        const from = opts.from && machine(s.panes[opts.from]) === key ? opts.from : anchorOn(s, key);
        const source = from ? s.panes[from] : undefined;
        const reuse = !!from && source?.kind === "empty" && !opts.split;
        const fresh: Omit<PaneState, "id"> = {
          kind,
          target,
          sessionKey: kind === "terminal" ? sessionKey() : undefined,
          path: opts.path,
          initialCommand: opts.initialCommand,
          container: opts.container,
          n: freeNumber(s.panes, kind, target, reuse ? from : undefined),
        };
        let panes = { ...s.panes };
        let layouts = { ...s.layouts };
        // An empty pane of another machine you picked this one from has done its job.
        const stray = opts.from && opts.from !== from && s.panes[opts.from]?.kind === "empty" ? opts.from : null;
        if (stray) {
          const k = machine(s.panes[stray]);
          const left = k && layouts[k] ? removePane(layouts[k], stray) : null;
          if (k && left) layouts[k] = left;
          else if (k) delete layouts[k];
          delete panes[stray];
        }
        let id: string;
        if (reuse && from) {
          id = from;
          panes[id] = { id, ...fresh };
        } else {
          id = newId("p");
          panes[id] = { id, ...fresh };
          // A terminal opened next to files or stats goes on the left, where you read first.
          const before = opts.before ?? (kind === "terminal" && !!source && source.kind !== "terminal");
          layouts[key] = layouts[key] && from ? splitAt(layouts[key], from, opts.dir ?? "row", id, before) : { kind: "pane", id };
        }
        // As tabs, a new tab opens on its own, leaving a pair side by side as it
        // was; what should show beside calls placeBeside after.
        const alone = isSplit(s, target) && useSettings.getState().viewMode === "tabs";
        set({ panes, layouts, ...focusing(s, panes, id), ...(alone ? { hidden: [...s.hidden, id] } : {}), zoomed: null });
        return id;
      },

      // A split is a new terminal on the server you're on, right away: no
      // asking where to, and never another server.
      split(id, dir) {
        const target = get().panes[id]?.target;
        if (!target) return id;
        return get().openIn("terminal", target, { from: id, dir, split: true, before: false });
      },

      closePane(id) {
        const s = get();
        const pane = s.panes[id];
        if (!pane) return;
        if (pane.kind === "terminal") {
          void import("./terminals").then((t) => t.disposeTerminal(id, true, pane));
        }
        const key = machine(pane);
        const tabs = pane.target ? tabsOn(s, pane.target).map((p) => p.id) : [];
        const { [id]: _gone, ...panes } = s.panes;
        const { [id]: _status, ...sessions } = s.sessions;
        const { [id]: _running, ...foreground } = s.foreground;
        const layouts = { ...s.layouts };
        if (key && layouts[key]) {
          const left = removePane(layouts[key], id);
          if (left) layouts[key] = left;
          else delete layouts[key];
        }
        const closedTabs =
          pane.kind !== "empty" && pane.target
            ? [{ kind: pane.kind, target: pane.target, path: pane.path, container: pane.container, closedAt: Date.now() }, ...s.closedTabs].slice(0, CLOSED_KEPT)
            : s.closedTabs;
        const base: Partial<AppState> = {
          layouts,
          panes,
          sessions,
          foreground,
          closedTabs,
          hidden: s.hidden.filter((h) => h !== id),
          tabOrder: s.tabOrder.filter((t) => t !== id),
          zoomed: s.zoomed === id ? null : s.zoomed,
        };
        if (s.focused !== id) return set(base);
        // The next tab comes up, as in a browser: the one after it, else the one
        // before; side by side, one that's on screen. Closing a machine's last
        // tab goes to the main menu rather than to another machine.
        const at = tabs.indexOf(id);
        const others = tabs.filter((t) => t !== id);
        const shown = pane.target && sideBySide(s, pane.target) ? others.filter((t) => !s.hidden.includes(t)) : others;
        const pick = (list: string[]) => list.find((t) => tabs.indexOf(t) > at) ?? [...list].reverse().find((t) => tabs.indexOf(t) < at);
        const next = pick(shown) ?? pick(others) ?? (key && layouts[key] ? paneIds(layouts[key])[0] : undefined);
        const after = { ...s, ...base } as AppState;
        set({ ...base, ...(next ? focusing(after, panes, next) : { home: true, zoomed: null }) });
      },

      tabOrder: [],
      setTabOrder(ids) {
        const s = get();
        set({ tabOrder: [...ids, ...s.tabOrder.filter((id) => !ids.includes(id))].filter((id) => s.panes[id]) });
      },

      hidden: [],
      setHidden(ids, hide) {
        const s = get();
        const hidden = hide ? [...new Set([...s.hidden, ...ids])] : s.hidden.filter((h) => !ids.includes(h));
        // Something of the machine always stays on screen.
        const target = s.panes[ids[0]]?.target;
        const shown = target ? panesOn(s, target).filter((p) => !hidden.includes(p.id)) : [];
        if (target && !shown.length) return;
        const focused = hidden.includes(s.focused) && shown.length ? shown[0].id : s.focused;
        // Focus moves the way every other move does, so the machine knows its front tab.
        const moved = focused !== s.focused ? focusing({ ...s, hidden }, s.panes, focused) : { focused };
        set({ hidden, ...moved, zoomed: s.zoomed && hidden.includes(s.zoomed) ? null : s.zoomed });
      },

      setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
      setAddHostOpen: (addHostOpen) => set({ addHostOpen }),

      toast(t) {
        const id = newId("t");
        set((s) => ({ toasts: [...s.toasts.slice(-3), { id, ...t }] }));
        setTimeout(() => get().dismissToast(id), t.tone === "error" ? 7000 : 3800);
        return id;
      },
      dismissToast(id) {
        set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
      },

      confirm(req) {
        return new Promise<boolean>((resolve) => {
          set({
            confirmRequest: {
              ...req,
              resolve: (ok) => {
                set({ confirmRequest: null });
                resolve(ok);
              },
            },
          });
        });
      },

      setDropPane: (dropPane) => set((s) => (s.dropPane === dropPane ? s : { dropPane })),

      settingsOpen: false,
      settingsSection: "appearance",
      zoomed: null,
      toggleZoom(id) {
        const s = get();
        const pane = id ?? activePane(s)?.id;
        // One tab at a time, every pane fills the window already.
        if (!pane || !sideBySide(s, s.panes[pane]?.target ?? null)) return set({ zoomed: null });
        set({ ...focusing(s, s.panes, pane), zoomed: s.zoomed ? null : pane });
      },
      openSettings: (section) => set((s) => ({ settingsOpen: true, settingsSection: section ?? s.settingsSection })),
      closeSettings: () => set({ settingsOpen: false }),

      dragPane: null,
      dropTarget: null,
      setDrag: (dragPane) => set({ dragPane, dropTarget: null }),
      setDropTarget: (t) =>
        set((s) =>
          s.dropTarget?.paneId === t?.paneId && s.dropTarget?.zone === t?.zone ? s : { dropTarget: t },
        ),
      movePane(dragId, targetId, zone) {
        const s = get();
        const key = machine(s.panes[dragId]);
        if (!key || key !== machine(s.panes[targetId]) || !s.layouts[key]) return;
        set({ layouts: { ...s.layouts, [key]: movePane(s.layouts[key], dragId, targetId, zone) }, ...focusing(s, s.panes, dragId) });
      },
      arrangePanes(how) {
        const s = get();
        const pane = activePane(s);
        const key = machine(pane ?? undefined);
        if (!pane || !key || !s.layouts[key]) return;
        // Keep the focused pane first: it becomes the big one in "main".
        const ids = paneIds(s.layouts[key]);
        const ordered = [pane.id, ...ids.filter((id) => id !== pane.id)];
        set({ layouts: { ...s.layouts, [key]: arrange(ordered, how) }, zoomed: null });
      },
      openWorkspace(target) {
        const s = get();
        const key = targetKey(target);
        const panes = { ...s.panes };
        const mine = panesOn(s, target);
        const ensure = (kind: Exclude<PaneKind, "empty">) => {
          const existing = mine.find((p) => p.kind === kind);
          if (existing) return existing.id;
          const empty = mine.find((p) => p.kind === "empty");
          const id = empty?.id ?? newId("p");
          panes[id] = { id, kind, target, sessionKey: kind === "terminal" ? sessionKey() : undefined, n: freeNumber(panes, kind, target, id) };
          return id;
        };
        // Stats are in the vitals bar; the full view opens from there.
        const main = [ensure("terminal"), ensure("files")];
        const rest = mine.map((p) => p.id).filter((id) => !main.includes(id));
        set({
          panes,
          layouts: { ...s.layouts, [key]: arrange([...main, ...rest], "main") },
          splitOn: { ...s.splitOn, [key]: true },
          hidden: s.hidden.filter((h) => !main.includes(h)),
          ...focusing(s, panes, main[0]),
          zoomed: null,
        });
      },
      setSplit(target, split) {
        set((s) => ({ splitOn: { ...s.splitOn, [targetKey(target)]: split } }));
      },
    }),
    {
      name: "sshee-workspace",
      version: 2,
      // A second window keeps its tabs to itself, for as long as it's open.
      storage: quietStorage(() => (isMainWindow ? localStorage : sessionStorage)),
      partialize: (s) => ({
        layouts: s.layouts,
        splitOn: s.splitOn,
        panes: s.panes,
        focused: s.focused,
        hidden: s.hidden,
        tabOrder: s.tabOrder,
        home: s.home,
        lastFocusedFor: s.lastFocusedFor,
        closedTabs: s.closedTabs,
      }),
      migrate: (saved, version) => (version < 2 ? splitByMachine(saved as Saved1) : saved) as AppState,
    },
  ),
);

/** What 0.5 saved: one layout for every machine's panes. */
interface Saved1 {
  root?: LayoutNode;
  panes?: Record<string, PaneState>;
  focused?: string;
  [key: string]: unknown;
}

/**
 * From 0.5's one layout to one per machine, keeping each machine's
 * arrangement. The start page's empty pane goes (the main menu took its
 * place), and every tab gets the number it shows as.
 */
export function splitByMachine(saved: Saved1): Record<string, unknown> {
  const { root, ...rest } = saved;
  const panes: Record<string, PaneState> = {};
  for (const p of Object.values(saved.panes ?? {})) if (p.target) panes[p.id] = { ...p };
  const layouts: Record<string, LayoutNode> = {};
  const order = root ? paneIds(root) : [];
  for (const key of new Set(Object.values(panes).map((p) => machine(p)!))) {
    const others = order.filter((id) => machine(panes[id]) !== key);
    const tree = root ? visibleTree(root, others) : null;
    if (tree) layouts[key] = tree;
    const taken = new Set<string>();
    for (const id of tree ? paneIds(tree) : []) {
      const p = panes[id];
      if (p.n) taken.add(`${p.kind}:${p.n}`);
    }
    for (const id of tree ? paneIds(tree) : []) {
      const p = panes[id];
      if (p.n) continue;
      let n = 1;
      while (taken.has(`${p.kind}:${n}`)) n++;
      taken.add(`${p.kind}:${n}`);
      p.n = n;
    }
  }
  const focused = saved.focused && panes[saved.focused] ? saved.focused : "";
  return { ...rest, layouts, splitOn: {}, panes, focused, home: !focused };
}

/** Changing the default in Settings applies to every machine, also ones you'd switched by hand. */
useSettings.subscribe((s, prev) => {
  if (s.viewMode === prev.viewMode) return;
  // Every machine follows the new default, and the tab you were on shows.
  const st = useStore.getState();
  const fronts = new Set([st.focused, ...Object.values(st.lastFocusedFor)]);
  useStore.setState({ splitOn: {}, hidden: st.hidden.filter((h) => !fronts.has(h)) });
});

/**
 * Per machine: how many tabs, whether one needs you, and how its terminals
 * are doing. Worked out once per change to panes, sessions or attention,
 * not once per row.
 */
export interface MachineSummary {
  open: number;
  waiting: boolean;
  /** Its terminals' sessions. */
  sessions: (SessionState | undefined)[];
}

const NOTHING: MachineSummary = { open: 0, waiting: false, sessions: [] };
let summaryOf: { panes: unknown; sessions: unknown; attention: unknown; map: Map<string, MachineSummary> } | null = null;

function summaries(s: AppState): Map<string, MachineSummary> {
  if (summaryOf && summaryOf.panes === s.panes && summaryOf.sessions === s.sessions && summaryOf.attention === s.attention) return summaryOf.map;
  const map = new Map<string, MachineSummary>();
  for (const p of Object.values(s.panes)) {
    const key = machine(p);
    if (!key || p.kind === "empty") continue;
    const m = map.get(key) ?? { open: 0, waiting: false, sessions: [] };
    m.open++;
    if (s.attention[p.id]?.kind === "input") m.waiting = true;
    if (p.kind === "terminal") m.sessions.push(s.sessions[p.id]);
    map.set(key, m);
  }
  summaryOf = { panes: s.panes, sessions: s.sessions, attention: s.attention, map };
  return map;
}

export function machineSummary(s: AppState, target: TargetRef | null): MachineSummary {
  return (target && summaries(s).get(targetKey(target))) || NOTHING;
}

/** A machine's open tabs and whether one needs you; re-renders only when those change. */
export function useMachine(target: TargetRef | null): Pick<MachineSummary, "open" | "waiting"> {
  return useStore(
    useShallow((s) => {
      const m = machineSummary(s, target);
      return { open: m.open, waiting: m.waiting };
    }),
  );
}

export function targetLabel(t: TargetRef | null | undefined, hosts: Host[]): string {
  if (!t) return "Nothing open";
  if (t.kind === "local") return "This computer";
  return hosts.find((h) => h.id === t.hostId)?.name ?? "Unknown server";
}

export function hostAddress(h: Host): string {
  const port = h.port && h.port !== 22 ? `:${h.port}` : "";
  return `${h.user ? `${h.user}@` : ""}${h.hostname}${port}`;
}

/** Seeds a showcase layout the first time the demo backend runs. */
export function seedDemoLayout() {
  if (!backend.isDemo || localStorage.getItem("sshee-workspace")) return;
  void import("./bookmarks").then(({ useBookmarks }) =>
    useBookmarks.setState({
      pins: { "host:cfg:prod-api": ["/var/www/app", "/etc/nginx", "/home/deploy/backups"] },
      recent: { "host:cfg:prod-api": ["/var/www/app", "/var/log/nginx", "/home/deploy"] },
    }),
  );
  const t: TargetRef = { kind: "host", hostId: "cfg:prod-api" };
  const a = newId("p");
  const b = newId("p");
  const key = targetKey(t);
  // ?scene=simple: one calm terminal, for the README's first picture.
  if (new URLSearchParams(location.search).get("scene") === "simple") {
    useStore.setState({ layouts: { [key]: { kind: "pane", id: a } }, panes: { [a]: { id: a, kind: "terminal", target: t, sessionKey: "demo", n: 1 } }, focused: a, home: false });
    return;
  }
  // A server with a shell, an agent and its files, and a second server with
  // an agent of its own, so the sidebar shows what it's for.
  const c = newId("p");
  const d = newId("p");
  const lab: TargetRef = { kind: "host", hostId: "cfg:homelab" };
  useStore.setState({
    layouts: {
      [key]: { kind: "split", id: newId("s"), dir: "row", children: [{ kind: "pane", id: a }, { kind: "pane", id: c }, { kind: "pane", id: b }] },
      [targetKey(lab)]: { kind: "pane", id: d },
    },
    panes: {
      [a]: { id: a, kind: "terminal", target: t, sessionKey: "demo", n: 1 },
      [c]: { id: c, kind: "terminal", target: t, sessionKey: "demo-claude", n: 2 },
      [b]: { id: b, kind: "files", target: t, path: "/var/www/app", n: 1 },
      [d]: { id: d, kind: "terminal", target: lab, sessionKey: "demo-codex", n: 1 },
    },
    hidden: [c],
    focused: a,
    home: false,
    attention: { [c]: { kind: "input", message: "Claude Code: Run the test suite now?", at: Date.now() } },
    lastFocusedFor: { [key]: a, [targetKey(lab)]: d },
  });
}
