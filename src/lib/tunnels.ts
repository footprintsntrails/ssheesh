// Tunnels: a server's port, here on this computer as localhost.
//
// Saved tunnels are kept per server (by host id) with their on/off switch;
// the ones left on start again when SSHeesh opens. "Open" on one of the
// server's web apps starts a temporary tunnel that ends when you stop it or
// quit, unless you keep it. Each tunnel's ssh process lives in Rust; its
// state comes back here.

import { create } from "zustand";
import { persist } from "zustand/middleware";
import { backend } from "./backend";
import { newId } from "./layout";
import { toOpError, type Forward, type OpError, type TunnelState, type WebApp } from "./types";

export type LocalForward = Extract<Forward, { kind: "local" }>;

export interface SavedTunnel {
  id: string;
  /** Optional; without one it's described by its ports. */
  name?: string;
  forward: Forward;
  /** The switch. On tunnels start again when SSHeesh opens. */
  on: boolean;
  /** How its web app is opened in the browser, when it was kept from one. */
  scheme?: "http" | "https";
}

/** A tunnel "Open" started: it ends when stopped or when SSHeesh quits. */
export interface TempTunnel {
  id: string;
  forward: LocalForward;
  scheme: "http" | "https";
  /** The page title or program, to name it by if you keep it. */
  label: string | null;
}

/** The server itself, as a tunnel's remote address. */
export const SERVER = "127.0.0.1";

interface SavedState {
  /** hostId → its saved tunnels, in the order they were added. */
  byHost: Record<string, SavedTunnel[]>;
  add(hostId: string, t: SavedTunnel): void;
  update(hostId: string, id: string, patch: Partial<Omit<SavedTunnel, "id">>): void;
  remove(hostId: string, id: string): void;
}

export const useSavedTunnels = create<SavedState>()(
  persist(
    (set) => ({
      byHost: {},
      add: (h, t) => set((s) => ({ byHost: { ...s.byHost, [h]: [...(s.byHost[h] ?? []), t] } })),
      update: (h, id, patch) => set((s) => ({ byHost: { ...s.byHost, [h]: (s.byHost[h] ?? []).map((t) => (t.id === id ? { ...t, ...patch } : t)) } })),
      remove: (h, id) => set((s) => ({ byHost: { ...s.byHost, [h]: (s.byHost[h] ?? []).filter((t) => t.id !== id) } })),
    }),
    { name: "sshee-tunnels", version: 1 },
  ),
);

interface RuntimeState {
  /** Tunnel id → how it's doing. Missing means off. */
  status: Record<string, TunnelState>;
  /** hostId → tunnels opened from "Web apps on this server". */
  temp: Record<string, TempTunnel[]>;
}

export const useTunnelRuntime = create<RuntimeState>(() => ({ status: {}, temp: {} }));

const ACTIVE: TunnelState["kind"][] = ["starting", "on", "reconnecting"];

export function isActive(s: TunnelState | undefined): boolean {
  return !!s && ACTIVE.includes(s.kind);
}

/** How many of a server's tunnels are on (or starting, or reconnecting). */
export function useActiveTunnels(hostId: string): number {
  const saved = useSavedTunnels((s) => s.byHost[hostId]);
  return useTunnelRuntime((rt) => {
    const ids = [...(rt.temp[hostId] ?? []), ...(saved ?? [])].map((t) => t.id);
    return ids.filter((id) => isActive(rt.status[id])).length;
  });
}

function setStatus(id: string, s: TunnelState | null) {
  useTunnelRuntime.setState((rt) => {
    const status = { ...rt.status };
    if (s) status[id] = s;
    else delete status[id];
    return { status };
  });
}

/** Bumped on every start and stop, so a stopped tunnel's late news is dropped. */
const generation = new Map<string, number>();

function bump(id: string): number {
  const g = (generation.get(id) ?? 0) + 1;
  generation.set(id, g);
  return g;
}

/** Starts (or restarts with new settings) a tunnel. Problems show up in its status. */
export async function startTunnel(hostId: string, id: string, forward: Forward): Promise<void> {
  const g = bump(id);
  setStatus(id, { kind: "starting" });
  try {
    await backend.startTunnel(id, hostId, forward, (s) => {
      if (generation.get(id) === g) setStatus(id, s);
    });
  } catch (e) {
    if (generation.get(id) === g) setStatus(id, { kind: "failed", message: toOpError(e).message });
  }
}

export function stopTunnel(id: string) {
  bump(id);
  backend.stopTunnel(id);
  setStatus(id, null);
}

/** Resolves once the tunnel is on; rejects if it fails, drops or takes too long first. */
export function waitUntilOn(id: string, ms = 30_000): Promise<void> {
  return new Promise((resolve, reject) => {
    let unsubscribe = () => {};
    let timer: ReturnType<typeof setTimeout> | undefined;
    const finish = (error?: OpError) => {
      clearTimeout(timer);
      unsubscribe();
      if (error) reject(error);
      else resolve();
    };
    const check = (s: TunnelState | undefined): boolean => {
      if (!s) finish({ code: "cancelled", message: "The tunnel was stopped." });
      else if (s.kind === "on") finish();
      else if (s.kind === "failed") finish({ code: "failed", message: s.message });
      else if (s.kind === "reconnecting") finish({ code: "network", message: `${s.reason}.` });
      else return false;
      return true;
    };
    if (check(useTunnelRuntime.getState().status[id])) return;
    timer = setTimeout(() => finish({ code: "network", message: "The server took too long to answer." }), ms);
    unsubscribe = useTunnelRuntime.subscribe((rt) => void check(rt.status[id]));
  });
}

/** Local ports other tunnels have claimed, on or off, so a new one picks another. */
export function claimedPorts(): number[] {
  const saved = Object.values(useSavedTunnels.getState().byHost).flat();
  const temp = Object.values(useTunnelRuntime.getState().temp).flat();
  return [...saved, ...temp].map((t) => t.forward.localPort);
}

const sameRemote = (f: Forward, app: WebApp) => f.kind === "local" && f.remotePort === app.port && (f.remoteHost.trim() || SERVER) === app.remoteHost;

/** A tunnel of this server's that already leads to `app`, preferring one that's on. */
export function tunnelTo(hostId: string, app: WebApp): { id: string; forward: LocalForward; saved: boolean } | null {
  const { status, temp } = useTunnelRuntime.getState();
  const saved = useSavedTunnels.getState().byHost[hostId] ?? [];
  const all = [...(temp[hostId] ?? []).map((t) => ({ ...t, saved: false })), ...saved.map((t) => ({ ...t, saved: true }))];
  const matches = all.filter((t) => sameRemote(t.forward, app));
  const best = matches.find((t) => isActive(status[t.id])) ?? matches[0];
  return best ? { id: best.id, forward: best.forward as LocalForward, saved: best.saved } : null;
}

export function appUrl(app: Pick<WebApp, "scheme">, localPort: number): string {
  return `${app.scheme ?? "http"}://localhost:${localPort}/`;
}

/**
 * Makes a server's app reachable here and, with `browse`, opens it in the
 * browser: through a tunnel already leading there, or a new temporary one on
 * the same port number when it's free. Resolves with the port here.
 */
export async function openWebApp(hostId: string, app: WebApp, browse: boolean): Promise<number> {
  const existing = tunnelTo(hostId, app);
  let port: number;
  if (existing) {
    port = existing.forward.localPort;
    if (!isActive(useTunnelRuntime.getState().status[existing.id])) {
      if (existing.saved) useSavedTunnels.getState().update(hostId, existing.id, { on: true });
      await startTunnel(hostId, existing.id, existing.forward);
    }
    await waitUntilOn(existing.id);
  } else {
    port = await backend.pickLocalPort(app.port, claimedPorts());
    const id = newId("tun");
    const forward: LocalForward = { kind: "local", localPort: port, remoteHost: app.remoteHost, remotePort: app.port };
    const label = app.title ?? app.knownAs ?? app.process;
    useTunnelRuntime.setState((rt) => ({ temp: { ...rt.temp, [hostId]: [...(rt.temp[hostId] ?? []), { id, forward, scheme: app.scheme ?? "http", label }] } }));
    await startTunnel(hostId, id, forward);
    try {
      await waitUntilOn(id);
    } catch (e) {
      closeTemp(hostId, id);
      throw e;
    }
  }
  if (browse) await backend.openExternal(appUrl(app, port));
  return port;
}

/** Stops a temporary tunnel and forgets it. */
export function closeTemp(hostId: string, id: string) {
  stopTunnel(id);
  useTunnelRuntime.setState((rt) => ({ temp: { ...rt.temp, [hostId]: (rt.temp[hostId] ?? []).filter((t) => t.id !== id) } }));
}

/** Turns a temporary tunnel into a saved one, without restarting it. */
export function keepTemp(hostId: string, id: string) {
  const t = useTunnelRuntime.getState().temp[hostId]?.find((x) => x.id === id);
  if (!t) return;
  useSavedTunnels.getState().add(hostId, { id, name: t.label ?? undefined, forward: t.forward, on: true, scheme: t.scheme });
  useTunnelRuntime.setState((rt) => ({ temp: { ...rt.temp, [hostId]: (rt.temp[hostId] ?? []).filter((x) => x.id !== id) } }));
}

export function setSavedOn(hostId: string, t: SavedTunnel, on: boolean) {
  useSavedTunnels.getState().update(hostId, t.id, { on });
  if (on) void startTunnel(hostId, t.id, t.forward);
  else stopTunnel(t.id);
}

/** Saves a new tunnel (switched on) or changes one; an on tunnel picks up its new settings. */
export function saveTunnel(hostId: string, id: string | null, name: string, forward: Forward) {
  const store = useSavedTunnels.getState();
  const old = id ? store.byHost[hostId]?.find((t) => t.id === id) : undefined;
  const clean = name.trim() || undefined;
  if (old) {
    store.update(hostId, old.id, { name: clean, forward });
    if (old.on) void startTunnel(hostId, old.id, forward);
    return;
  }
  const fresh: SavedTunnel = { id: newId("tun"), name: clean, forward, on: true };
  store.add(hostId, fresh);
  void startTunnel(hostId, fresh.id, forward);
}

export function removeSaved(hostId: string, id: string) {
  stopTunnel(id);
  useSavedTunnels.getState().remove(hostId, id);
}

/** A removed server takes its tunnels with it: stopped now, and not saved for next time. */
export function forgetHostTunnels(hostId: string) {
  for (const t of useSavedTunnels.getState().byHost[hostId] ?? []) removeSaved(hostId, t.id);
  for (const t of useTunnelRuntime.getState().temp[hostId] ?? []) closeTemp(hostId, t.id);
}

let restored = false;

/** Starts the saved tunnels that were on when SSHeesh last closed. Once per run. */
export function restoreTunnels() {
  if (restored) return;
  restored = true;
  for (const [hostId, list] of Object.entries(useSavedTunnels.getState().byHost)) {
    for (const t of list) if (t.on) void startTunnel(hostId, t.id, t.forward);
  }
}

/** Why a port number typed in a form won't do, or null. */
export function portError(text: string, what: string): string | null {
  const n = Number(text);
  if (!text.trim()) return `Enter ${what}.`;
  if (!Number.isInteger(n) || n < 1 || n > 65535) return `${what[0].toUpperCase()}${what.slice(1)} is a number from 1 to 65535.`;
  return null;
}

/** Why a server-side address won't do, or null. Empty means the server itself. */
export function hostError(text: string): string | null {
  const h = text.trim();
  if (!h) return null;
  if (h.startsWith("-") || h.length > 253 || !/^[A-Za-z0-9._:%[\]-]+$/.test(h)) return `"${h}" isn't an address SSHeesh can use.`;
  return null;
}

/** "This computer's port 8080 opens the server's port 3000", in plain words. */
export function describeForward(f: Forward): string {
  if (f.kind === "socks") return `Apps set to use localhost:${f.localPort} as a SOCKS proxy go out through the server.`;
  const host = f.remoteHost.trim() || SERVER;
  const where = host === SERVER || host === "localhost" ? `the server's port ${f.remotePort}` : `port ${f.remotePort} on ${host}, reached through the server`;
  return `localhost:${f.localPort} on this computer opens ${where}.`;
}

/** A short name for a saved tunnel without one. */
export function tunnelTitle(t: Pick<SavedTunnel, "name" | "forward">): string {
  if (t.name) return t.name;
  const f = t.forward;
  if (f.kind === "socks") return "Browse through the server";
  const host = f.remoteHost.trim() || SERVER;
  return host === SERVER ? `Server port ${f.remotePort}` : `${host}:${f.remotePort}`;
}
