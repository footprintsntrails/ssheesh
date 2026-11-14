// Makes the fixtures sshee-app's model tests read, from 0.10's own stores.
//
// Each scenario starts the stores as the app does, on an empty storage or on
// what an older version left there, does what a person would do, and writes
// down what zustand saved: the seven `sshee-*` values. Older versions run from
// their own git tag, so their stores have the real old shapes, and 0.10.14's
// code then migrates them as it does on a real update. Random ids and the
// clock are fixed, so the files only change when the TS code does.
//
// Everything in here is made up: servers web-7731 and db-main, addresses from
// 203.0.113.0/24, a token that was never real.
//
//   npx vitest run --config crates/sshee-app/tests/fixtures/gen/vitest.config.ts

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, expect, it, vi } from "vitest";

const REPO = fileURLToPath(new URL("../../../../../", import.meta.url));
const OUT = fileURLToPath(new URL("../", import.meta.url));

const KEYS = ["sshee-settings", "sshee-labels", "sshee-workspace", "sshee-snippets", "sshee-bookmarks", "sshee-tunnels", "sshee-typing-counts"];

/** 13 Nov 2026, for `closedAt`. */
const NOW = 1_794_528_000_000;

type Platform = "linux" | "mac" | "windows";
type Stores = Record<string, unknown>;
// The stores' modules, from whichever version: typed loosely on purpose.
type Mod = any; // eslint-disable-line @typescript-eslint/no-explicit-any

const web = { kind: "host", hostId: "cfg:web-7731" };
const db = { kind: "host", hostId: "cfg:db-main" };
const nas = { kind: "host", hostId: "saved:k7q2m9xa" };
const local = { kind: "local" };

/** localStorage as the web view has it: strings in, strings out. */
class MemoryStorage {
  private items = new Map<string, string>();
  get length() {
    return this.items.size;
  }
  key(i: number) {
    return [...this.items.keys()][i] ?? null;
  }
  getItem(key: string) {
    return this.items.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.items.set(key, String(value));
  }
  removeItem(key: string) {
    this.items.delete(key);
  }
  clear() {
    this.items.clear();
  }
}

const scratch = mkdtempSync(join(tmpdir(), "sshee-fixtures-"));
afterAll(() => rmSync(scratch, { recursive: true, force: true }));

/** `src/lib` of a release (null: this checkout), with this checkout's node_modules beside it. */
function libOf(tag: string | null): string {
  if (!tag) return join(REPO, "src/lib");
  const dir = join(scratch, tag);
  mkdirSync(dir, { recursive: true });
  const tar = join(scratch, `${tag}.tar`);
  execFileSync("git", ["-C", REPO, "archive", `--output=${tar}`, tag, "src/lib", "package.json"]);
  execFileSync("tar", ["-x", "-f", tar, "-C", dir]);
  symlinkSync(join(REPO, "node_modules"), join(dir, "node_modules"));
  return join(dir, "src/lib");
}
const libs = new Map<string | null, string>();

interface App {
  settings: Mod;
  labels: Mod;
  store: Mod;
  snippets: Mod;
  bookmarks: Mod;
  tunnels: Mod;
  typing: Mod;
  storage: MemoryStorage;
  flush(): void;
}

/** Starts a version's stores on `stored`, as the app does when it opens. */
async function boot(platform: Platform, stored: Stores = {}, tag: string | null = null): Promise<App> {
  vi.resetModules();
  let seed = 7731;
  Math.random = () => (seed = (seed * 48271) % 2147483647) / 2147483647;
  vi.spyOn(Date, "now").mockReturnValue(NOW);
  const storage = new MemoryStorage();
  for (const [key, value] of Object.entries(stored)) storage.setItem(key, JSON.stringify(value));
  const flushers: (() => void)[] = [];
  const win = {
    localStorage: storage,
    sessionStorage: new MemoryStorage(),
    addEventListener: (type: string, fn: () => void) => void (type === "pagehide" && flushers.push(fn)),
    removeEventListener() {},
  };
  const agent = {
    linux: "Mozilla/5.0 (X11; Linux x86_64)",
    mac: "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5)",
    windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
  }[platform];
  vi.stubGlobal("window", win);
  vi.stubGlobal("localStorage", storage);
  vi.stubGlobal("sessionStorage", win.sessionStorage);
  vi.stubGlobal("navigator", { userAgent: agent });
  if (!libs.has(tag)) libs.set(tag, libOf(tag));
  const lib = libs.get(tag)!;
  const load = async (name: string): Promise<Mod> => {
    try {
      return await import(/* @vite-ignore */ join(lib, `${name}.ts`));
    } catch (e) {
      // Stores a version didn't have yet (typing counts came in 0.10.7).
      if (String(e).includes("Failed to load") || String(e).includes("Cannot find")) return null;
      throw e;
    }
  };
  // Settings first, as the app's own imports have it.
  const settings = await load("settings");
  const store = await load("store");
  return {
    settings,
    store,
    labels: await load("labels"),
    snippets: await load("snippets"),
    bookmarks: await load("bookmarks"),
    tunnels: await load("tunnels"),
    typing: await load("typingCounts"),
    storage,
    flush: () => flushers.forEach((f) => f()),
  };
}

/** What's in storage once every store has saved what it holds, as after any change in the app. */
function saved(app: App, only?: string[]): Stores {
  const all = [
    app.settings?.useSettings,
    app.labels?.useLabels,
    app.store?.useStore,
    app.snippets?.useSnippets,
    app.bookmarks?.useBookmarks,
    app.tunnels?.useSavedTunnels,
    app.typing?.useTypingCounts,
  ];
  for (const s of all) s?.setState({});
  app.flush();
  const out: Stores = {};
  for (const key of only ?? KEYS) {
    const raw = app.storage.getItem(key);
    if (raw !== null) out[key] = JSON.parse(raw);
  }
  return out;
}

function write(name: string, value: unknown) {
  mkdirSync(OUT, { recursive: true });
  writeFileSync(join(OUT, name), JSON.stringify(value, null, 2) + "\n");
}

/** A person's 0.10.14: several servers, tabs side by side, a tab beside another server's, and the rest. */
async function inUse(app: App) {
  const settings = app.settings.useSettings;
  settings.getState().update({
    onboarded: true,
    onboardingVersion: 2,
    hintsSeen: ["split-drag", "files-drop"],
    lastSeenVersion: "0.10.14",
    customThemes: [{ id: "custom-r8t2w6yz", name: "Night shift", base: "graphite", colors: { window: "#0b0d10", canvas: "#07080a" } }],
    theme: "custom-r8t2w6yz",
    accent: "#7c83ff",
    shortcuts: { splitRight: { key: "d", ctrl: true, shift: true }, paletteAlt: null },
    uiZoom: 110,
    dimInactive: 15,
    terminalScheme: "own",
    terminalColors: { background: "#101418", foreground: "#e0e6ee" },
    terminalFontSize: 14.5,
    terminalLineHeight: 1.3,
    cursorStyle: "block",
    idleSleep: 45,
    sidebarWidth: 251.5,
    sidebarMode: "wide",
    railStatus: "bars",
    compactMode: "sidebar",
    compactLast: "sidebar",
    tabsPlacement: "sidebar",
    expanded: ["host:cfg:web-7731"],
    hiddenPlaces: ["recent", "/tmp"],
    updateMode: "notify",
    streamerMode: true,
    highlightAddresses: true,
    highlightRules: [
      { id: "q8w7e6r5", pattern: "/timeout after \\d+ ?ms/", color: "magenta" },
      { id: "z1x2c3v4", pattern: "db-main", color: "cyan" },
    ],
    welcomeMode: "first",
    welcomeParts: ["logo", "status"],
    welcomeNote: "Be gentle with db-main",
    welcomedHosts: ["cfg:web-7731"],
    countTyping: true,
    torHideTyping: true,
  });
  // What 0.5 called the file manager's sidebar, still there after every update since.
  settings.setState({ placesWidth: 212 });

  const st = () => app.store.useStore.getState();
  const w1 = st().openIn("terminal", web);
  const w2 = st().split(w1, "row", true);
  const w3 = st().openIn("files", web, { path: "/var/www/app" });
  st().setPane(w3, { reveal: "index.html", revealLines: [3, 9] });
  const w4 = st().openIn("terminal", web, { container: { engine: "docker", id: "3f9c2a1b7d4e", name: "api", run: "logs" } });
  const d1 = st().openIn("terminal", db, { initialCommand: "cd /srv/db" });
  const d2 = st().split(d1, "col", true);
  const d3 = st().openIn("stats", db);
  st().openIn("terminal", local);
  const n1 = st().openIn("files", nas, { path: "/srv/backups" });
  const n2 = st().openIn("terminal", nas);
  st().split(n2, "row", true);
  st().closePane(d3);
  st().closePane(n1);
  // db-main's second terminal goes beside web-7731's first: a guest on that screen.
  st().movePane(d2, w1, "right");
  st().setTabOrder([w2, w1]);
  st().setSplit(web, true);
  st().setHidden([w4], true);
  st().focus(w2);

  const labels = app.labels.useLabels.getState();
  labels.setEnv("cfg:db-main", "prod");
  labels.setEnv("cfg:web-7731", "staging");
  labels.setEnv("saved:k7q2m9xa", "dev");
  labels.setLook("cfg:web-7731", { icon: "globe", color: "#7577e3" });
  labels.setLook("saved:k7q2m9xa", {
    icon: "backup",
    color: "#0997b4",
    image: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
  });
  labels.setAutoLook("cfg:db-main", "database");
  labels.setAutoLook("cfg:web-7731", "globe");
  labels.chooseConfig(["cfg:web-7731", "cfg:db-main", "cfg:old-box"], ["cfg:web-7731", "cfg:db-main"]);
  labels.setHidden("saved:q1w2e3r4", true);

  const snippets = app.snippets.useSnippets.getState();
  snippets.save({ name: "Deploy with a token", command: "curl -fsS -H 'Authorization: Bearer {{token:sk-test-not-real-0000}}' https://203.0.113.10/deploy" });
  snippets.save({ id: "s-disk", name: "How full the disks are", command: "df -h -x tmpfs -x devtmpfs -x overlay" });
  app.snippets.useSnippets.getState().remove("s-ports");

  const bookmarks = app.bookmarks.useBookmarks.getState();
  bookmarks.toggle("host:cfg:web-7731", "/var/www/app");
  bookmarks.toggle("host:cfg:web-7731", "/etc/nginx");
  app.bookmarks.useBookmarks.getState().visit("host:cfg:web-7731", "/var/log");
  app.bookmarks.useBookmarks.getState().visit("host:cfg:web-7731", "/var/www/app");
  app.bookmarks.useBookmarks.getState().visit("local", "/home/dev/projects");

  const tunnels = app.tunnels.useSavedTunnels.getState();
  tunnels.add("cfg:web-7731", { id: "tun_g7k2", name: "Grafana", forward: { kind: "local", localPort: 3000, remoteHost: "127.0.0.1", remotePort: 3000 }, on: true, scheme: "http" });
  tunnels.add("cfg:web-7731", { id: "tun_s0ck", forward: { kind: "socks", localPort: 1080 }, on: false });
  tunnels.add("cfg:db-main", { id: "tun_pg55", name: "Postgres", forward: { kind: "local", localPort: 15432, remoteHost: "", remotePort: 5432 }, on: true });
  app.tunnels.useSavedTunnels.getState().update("cfg:web-7731", "tun_g7k2", { on: false });

  const t = app.typing;
  for (let i = 0; i < 150; i++) {
    t.addCount("direct", "line", "keys");
    t.addCount("direct", "line", "typed");
  }
  for (let i = 0; i < 40; i++) t.addCount("direct", "line", "atOnce");
  t.addCount("direct", "line", "heldSilent");
  for (const ms of [8, 12, 35, 55, 140, 900, 4200]) t.addEcho("direct", "line", ms);
  for (let i = 0; i < 60; i++) {
    t.addCount("tor", "agent", "keys");
    t.addCount("tor", "agent", "typed");
  }
}

/** What a person might have had in an older version: the same servers, with that version's actions. */
async function olderInUse(app: App, version: string) {
  const settings = app.settings.useSettings;
  // Settings from the version's own defaults, with a few changed by hand.
  settings.setState({ terminalFontSize: 14, cursorStyle: "underline", streamerMode: true });
  if (version === "0.3.5" || version === "0.5.1" || version === "0.6.8") settings.setState({ compactMode: true });
  if (version === "0.7.3") settings.setState({ railStatus: "pulse", compactMode: "tabbar" });

  const st = () => app.store.useStore.getState();
  const w1 = st().openIn("terminal", web);
  st().split(w1, "row");
  const d1 = st().openIn("terminal", db);
  st().openIn("files", web, { path: "/var/www/app" });
  st().openIn("terminal", local);
  st().split(d1, "col");
  if (st().setTabOrder) st().setTabOrder([d1, w1]);
  st().focus(d1);

  const labels = app.labels.useLabels.getState();
  labels.setEnv("cfg:db-main", "prod");
  // Before labels v2, adding a server saved the default look as if picked; v2 drops it.
  labels.setLook("saved:k7q2m9xa", { icon: "server", color: "#5B9DFF" });
  labels.setLook("cfg:web-7731", { icon: "globe", color: "#9d69d2" });
  labels.chooseConfig(["cfg:web-7731", "cfg:db-main"], ["cfg:web-7731", "cfg:db-main"]);

  app.bookmarks.useBookmarks.getState().toggle("host:cfg:web-7731", "/var/www/app");
  app.snippets?.useSnippets.getState().save({ name: "Tail the app log", command: "tail -n {{lines:200}} -f /var/log/app.log" });
  app.tunnels?.useSavedTunnels.getState().add("cfg:web-7731", { id: "tun_g7k2", forward: { kind: "local", localPort: 3000, remoteHost: "127.0.0.1", remotePort: 3000 }, on: true });
}

it("writes 0.10.14's default stores, per system", async () => {
  const out: Record<string, Stores> = {};
  for (const platform of ["linux", "mac", "windows"] as const) out[platform] = saved(await boot(platform));
  expect(Object.keys(out.linux)).toEqual(KEYS);
  write("ts-defaults.json", out);
});

it("writes a 0.10.14 in daily use", async () => {
  const app = await boot("linux");
  await inUse(app);
  const stores = saved(app);
  expect(Object.keys(stores)).toEqual(KEYS);
  write("ts-in-use.json", { platform: "linux", stores });
});

it("writes older versions' stores and what 0.10.14 makes of them", async () => {
  const cases = [];
  for (const [tag, platform] of [
    ["v0.3.5", "linux"],
    ["v0.5.1", "mac"],
    ["v0.5.1", "linux"],
    ["v0.6.8", "mac"],
    ["v0.7.3", "windows"],
  ] as const) {
    const old = await boot(platform, {}, tag);
    await olderInUse(old, tag.slice(1));
    const before = saved(old);
    const now = await boot(platform, before);
    const after = saved(now, Object.keys(before));
    cases.push({ from: tag.slice(1), platform, saved: before, migrated: after });
  }
  write("ts-upgrades.json", { cases });
});
