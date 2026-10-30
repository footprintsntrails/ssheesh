import { create } from "zustand";
import { persist } from "zustand/middleware";
import { backend } from "./backend";
import { arrange, movePane, newId, paneIds, removePane, splitAt, type Arrangement, type Dir, type DropZone, type LayoutNode } from "./layout";
import type { ContainerRef, Host, SessionState, TargetRef } from "./types";
import { sameTarget, toOpError } from "./types";

export type PaneKind = "empty" | "terminal" | "files" | "stats";

export interface PaneState {
  id: string;
  kind: PaneKind;
  target: TargetRef | null;
  /** Names the tmux session on the server; stable so reopening reattaches. */
  sessionKey?: string;
  /** Files pane: current folder. */
  path?: string;
  /** Terminal pane: typed once after the first connect (e.g. `cd` into a folder). */
  initialCommand?: string;
  /** Terminal pane: opened inside this container. */
  container?: ContainerRef;
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

interface OpenOptions {
  path?: string;
  initialCommand?: string;
  /** Pane to split from; defaults to the focused one. */
  from?: string;
  dir?: Dir;
  /** Always open in a new split, even if the source pane is empty. */
  split?: boolean;
  container?: ContainerRef;
}

interface AppState {
  hosts: Host[];
  hostsLoaded: boolean;
  root: LayoutNode;
  panes: Record<string, PaneState>;
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
  /** Pane being dragged by its header, and where it would land. */
  dragPane: string | null;
  dropTarget: { paneId: string; zone: DropZone } | null;

  loadHosts(): Promise<void>;
  focus(id: string): void;
  setPane(id: string, patch: Partial<PaneState>): void;
  setSession(id: string, state: SessionState): void;
  openIn(kind: Exclude<PaneKind, "empty">, target: TargetRef, opts?: OpenOptions): string;
  split(id: string, dir: Dir): string;
  closePane(id: string): void;
  setPaletteOpen(open: boolean): void;
  setAddHostOpen(open: boolean): void;
  toast(t: Omit<Toast, "id">): void;
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
  /** Terminal + files + stats for one machine, side by side. */
  openWorkspace(target: TargetRef): void;
}

const firstPane = newId("p");

export function sessionKey(): string {
  return Math.random().toString(36).slice(2, 12);
}

export const useStore = create<AppState>()(
  persist(
    (set, get) => ({
      hosts: [],
      hostsLoaded: false,
      root: { kind: "pane", id: firstPane },
      panes: { [firstPane]: { id: firstPane, kind: "empty", target: null } },
      focused: firstPane,
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
        if (get().focused !== id) set({ focused: id });
      },

      setPane(id, patch) {
        set((s) => (s.panes[id] ? { panes: { ...s.panes, [id]: { ...s.panes[id], ...patch } } } : s));
      },

      setSession(id, state) {
        set((s) => ({ sessions: { ...s.sessions, [id]: state } }));
      },

      openIn(kind, target, opts = {}) {
        const s = get();
        const from = opts.from ?? s.focused;
        const source = s.panes[from];
        const fresh: Omit<PaneState, "id"> = {
          kind,
          target,
          sessionKey: kind === "terminal" ? sessionKey() : undefined,
          path: opts.path,
          initialCommand: opts.initialCommand,
          container: opts.container,
        };
        if (source && source.kind === "empty" && !opts.split) {
          set({ panes: { ...s.panes, [from]: { id: from, ...fresh } }, focused: from });
          return from;
        }
        const id = newId("p");
        const root = source ? splitAt(s.root, from, opts.dir ?? "row", id) : s.root;
        set({ root, panes: { ...s.panes, [id]: { id, ...fresh } }, focused: id, zoomed: null });
        return id;
      },

      split(id, dir) {
        const s = get();
        const nid = newId("p");
        set({
          root: splitAt(s.root, id, dir, nid),
          panes: { ...s.panes, [nid]: { id: nid, kind: "empty", target: s.panes[id]?.target ?? null } },
          focused: nid,
          zoomed: null,
        });
        return nid;
      },

      closePane(id) {
        const s = get();
        const pane = s.panes[id];
        if (!pane) return;
        if (pane.kind === "terminal") {
          void import("./terminals").then((t) => t.disposeTerminal(id, true));
        }
        const order = paneIds(s.root);
        const { [id]: _gone, ...panes } = s.panes;
        const { [id]: _status, ...sessions } = s.sessions;
        let root = removePane(s.root, id);
        if (!root) {
          const fresh = newId("p");
          root = { kind: "pane", id: fresh };
          panes[fresh] = { id: fresh, kind: "empty", target: null };
        }
        const remaining = paneIds(root);
        const idx = order.indexOf(id);
        const focused = s.focused === id ? (remaining[Math.max(0, idx - 1)] ?? remaining[0]) : s.focused;
        set({ root, panes, sessions, focused, zoomed: s.zoomed === id ? null : s.zoomed });
      },

      setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
      setAddHostOpen: (addHostOpen) => set({ addHostOpen }),

      toast(t) {
        const id = newId("t");
        set((s) => ({ toasts: [...s.toasts.slice(-3), { id, ...t }] }));
        setTimeout(() => get().dismissToast(id), t.tone === "error" ? 7000 : 3800);
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
        const target = id ?? s.focused;
        set({ zoomed: s.zoomed ? null : target, focused: target });
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
        set((s) => ({ root: movePane(s.root, dragId, targetId, zone), focused: dragId }));
      },
      arrangePanes(how) {
        const s = get();
        const ids = paneIds(s.root);
        // Keep the focused pane first: it becomes the big one in "main".
        const ordered = [s.focused, ...ids.filter((id) => id !== s.focused)].filter((id) => s.panes[id]);
        set({ root: arrange(ordered, how), zoomed: null });
      },
      openWorkspace(target) {
        const s = get();
        const panes = { ...s.panes };
        const find = (kind: PaneKind) => Object.values(panes).find((p) => p.kind === kind && sameTarget(p.target, target))?.id;
        const ensure = (kind: Exclude<PaneKind, "empty">) => {
          const existing = find(kind);
          if (existing) return existing;
          const focusedPane = panes[s.focused];
          if (kind === "terminal" && focusedPane?.kind === "empty") {
            panes[s.focused] = { id: s.focused, kind, target, sessionKey: sessionKey() };
            return s.focused;
          }
          const id = newId("p");
          panes[id] = { id, kind, target, sessionKey: kind === "terminal" ? sessionKey() : undefined };
          return id;
        };
        const main = [ensure("terminal"), ensure("files"), ensure("stats")];
        const rest = paneIds(s.root).filter((id) => !main.includes(id) && panes[id]);
        const root = arrange([...main, ...rest], "main");
        set({ panes, root, focused: main[0], zoomed: null });
      },
    }),
    {
      name: "sshee-workspace",
      version: 1,
      partialize: (s) => ({ root: s.root, panes: s.panes, focused: s.focused }),
    },
  ),
);

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
  useStore.setState({
    root: { kind: "split", id: newId("s"), dir: "row", children: [{ kind: "pane", id: a }, { kind: "pane", id: b }] },
    panes: {
      [a]: { id: a, kind: "terminal", target: t, sessionKey: "demo" },
      [b]: { id: b, kind: "files", target: t, path: "/var/www/app" },
    },
    focused: a,
  });
}
