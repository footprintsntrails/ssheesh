// Demo backend for running the UI in a plain browser (`npm run dev`).
// Everything here is sample data: documentation IP ranges, made-up servers.

import type { Backend, TransferHooks } from "./backend";
import type { Container, DirListing, FileEntry, Host, Metrics, SessionEvent, TargetRef, TextFile } from "./types";
import type { WebApp } from "./types";

const hosts: Host[] = [
  { id: "cfg:prod-api", name: "prod-api", hostname: "203.0.113.24", user: "deploy", port: null, identityFile: null, source: "sshConfig" },
  { id: "cfg:homelab", name: "homelab", hostname: "192.168.1.20", user: "root", port: null, identityFile: null, source: "sshConfig" },
  { id: "cfg:pi", name: "pi", hostname: "raspberrypi.local", user: "pi", port: null, identityFile: null, source: "sshConfig" },
  { id: "saved:db", name: "db-replica", hostname: "198.51.100.7", user: "postgres", port: 2222, identityFile: null, source: "saved" },
];

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const fakeCancels = new Map<string, () => void>();
/** Pretends to move `total` bytes over a few seconds; Cancel stops it. */
function fakeTransfer(hooks: TransferHooks, total: number): Promise<void> {
  return new Promise((resolve, reject) => {
    let done = 0;
    const timer = setInterval(() => {
      done = Math.min(total, done + (total / 30) * (0.5 + Math.random()));
      hooks.onProgress({ done, total });
      if (done < total) return;
      clearInterval(timer);
      fakeCancels.delete(hooks.id);
      resolve();
    }, 120);
    fakeCancels.set(hooks.id, () => {
      clearInterval(timer);
      fakeCancels.delete(hooks.id);
      reject({ code: "cancelled", message: "Cancelled." });
    });
  });
}
const now = Math.floor(Date.now() / 1000);

function dir(name: string, ago: number, mode = "drwxr-xr-x"): FileEntry {
  return { name, kind: "dir", isLink: false, size: 4096, modified: now - ago, mode };
}
function file(name: string, size: number, ago: number, mode = "-rw-r--r--"): FileEntry {
  return { name, kind: "file", isLink: false, size, modified: now - ago, mode };
}

const tree: Record<string, FileEntry[]> = {
  "/home/deploy": [
    dir(".ssh", 86400 * 40, "drwx------"),
    dir("app", 3600 * 2),
    dir("backups", 86400 * 3),
    dir("logs", 600),
    file(".bashrc", 3771, 86400 * 90),
    file(".profile", 807, 86400 * 90),
    file("docker-compose.yml", 1843, 3600 * 5),
    file("deploy.sh", 2210, 86400 * 2, "-rwxr-xr-x"),
    file(".env", 612, 86400 * 6, "-rw-------"),
  ],
  "/var/www/app": [
    dir("config", 86400 * 4),
    dir("public", 3600 * 3),
    dir("src", 1200),
    dir("node_modules", 86400),
    file(".env.production", 1024, 86400 * 6, "-rw-------"),
    file("Dockerfile", 864, 86400 * 12),
    file("README.md", 4210, 86400 * 30),
    file("docker-compose.yml", 1843, 3600 * 5),
    file("nginx.conf", 2311, 86400 * 8),
    file("package.json", 1932, 86400 * 2),
    file("release-2026-10-29.tar.gz", 48_211_332, 3600 * 20),
    file("server.log", 7_340_032, 45),
  ],
};

const textFiles: Record<string, string> = {
  // A key made up for the demo; its private half was thrown away.
  "id_ed25519.pub": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII5PL2EOb31cCbJC+OjU74tjgLeisDMLh51dTLBg849P sam@macbook\n",
  "docker-compose.yml": `services:
  api:
    image: ghcr.io/acme/api:2.14.0
    restart: unless-stopped
    env_file: .env.production
    ports:
      - "8080:8080"
    depends_on: [db, cache]
  db:
    image: postgres:17
    volumes:
      - pgdata:/var/lib/postgresql/data
  cache:
    image: redis:8-alpine

volumes:
  pgdata:
`,
};

// A big folder, where only the rows in view get drawn.
const TOOLS = ["git", "python3", "perl", "x86_64-linux-gnu", "systemd", "gpg", "ssh", "lib", "node", "gcc"];
tree["/usr/bin"] = Array.from({ length: 1400 }, (_, i) =>
  file(`${TOOLS[i % TOOLS.length]}-${String((i / TOOLS.length) | 0).padStart(3, "0")}`, 20_000 + ((i * 7919) % 900_000), 86400 * (30 + (i % 300)), "-rwxr-xr-x"),
);

function listing(path: string): DirListing {
  const resolved = path === "~" || path === "" ? "/home/deploy" : path;
  const entries = tree[resolved] ?? [dir("bin", 86400 * 200), dir("etc", 86400 * 2), dir("home", 86400 * 30), dir("var", 3600)];
  return { path: resolved, entries };
}

// ---- fake terminal

const C = {
  dim: "\x1b[2m",
  b: "\x1b[1m",
  g: "\x1b[32m",
  y: "\x1b[33m",
  bl: "\x1b[34m",
  c: "\x1b[36m",
  r: "\x1b[0m",
};

/** A short, friendly session for the README's first picture. */
function simpleBanner(name: string, user: string): string {
  const prompt = `${C.g}${user}@${name}${C.r}:${C.bl}~${C.r}$ `;
  return [
    `${C.dim}Welcome back to ${name}.${C.r}`,
    "",
    `${prompt}uptime`,
    " 14:02:11 up 37 days,  1 user,  load average: 0.12",
    `${prompt}ls`,
    `${C.bl}app${C.r}  ${C.bl}backups${C.r}  ${C.bl}logs${C.r}  docker-compose.yml  notes.txt`,
    prompt,
  ].join("\r\n");
}

function banner(name: string, user: string): string {
  const prompt = `${C.g}${user}@${name}${C.r}:${C.bl}~${C.r}$ `;
  return [
    `${C.dim}Last login: Thu Oct 29 22:41:07 2026 from 198.51.100.3${C.r}`,
    "",
    `${prompt}docker compose ps`,
    `${C.b}NAME          IMAGE                 STATUS               PORTS${C.r}`,
    `app-api-1     acme/api:2.14.0       ${C.g}Up 3 days (healthy)${C.r}  8080/tcp`,
    `app-db-1      postgres:17           ${C.g}Up 3 days${C.r}            5432/tcp`,
    `app-cache-1   redis:8-alpine        ${C.g}Up 3 days${C.r}            6379/tcp`,
    `${prompt}tail -n 4 logs/api.log`,
    `${C.dim}2026-10-29T23:10:02Z${C.r} ${C.c}INFO${C.r}  GET /v1/orders 200 18ms`,
    `${C.dim}2026-10-29T23:10:04Z${C.r} ${C.c}INFO${C.r}  POST /v1/checkout 201 64ms`,
    `${C.dim}2026-10-29T23:10:05Z${C.r} ${C.y}WARN${C.r}  slow query: orders_by_customer (412ms)`,
    `${C.dim}2026-10-29T23:10:09Z${C.r} ${C.c}INFO${C.r}  GET /healthz 200 1ms`,
    prompt,
  ].join("\r\n");
}

interface FakeSession {
  emit: (e: SessionEvent) => void;
  /** Sends without adding to the server's scrollback (ssh's own messages). */
  raw: (e: SessionEvent) => void;
  line: string;
  prompt: string;
  timers: number[];
  /** What tmux on the "server" has printed, for sessionHistory. */
  scrollback: string;
}

const sessions = new Map<string, FakeSession>();
const trustedDemo = new Set<string>();
const demoTested = new Set<string>();
const byKey = new Map<string, FakeSession>();
const redrawTimers = new Map<string, number>();

// What the demo server prints while the connection is down.
const WHILE_AWAY = [
  " ✔ Container app-db-1     Healthy",
  " ✔ Container app-api-1    Recreated",
  " ✔ Container app-cache-1  Started",
  "Running migrations… 3 applied",
  "2026-10-30T02:41:07Z WARN  cache warm-up took 2.4s",
  "2026-10-30T02:41:09Z ERROR payment webhook timed out (retrying)",
  "2026-10-30T02:41:12Z INFO  webhook delivered on retry 1",
  "Deploy finished in 1m 48s",
];

const stripAnsi = (s: string) => s.replace(/\x1b\[[0-9;]*[A-Za-z]/g, "");

function hostFor(t: TargetRef): { name: string; user: string } {
  if (t.kind === "local") return { name: "macbook", user: "me" };
  const h = hosts.find((x) => x.id === t.hostId);
  return { name: h?.name ?? "server", user: h?.user ?? "root" };
}

let autoReconnect = true;

/** A coding agent that works for a few seconds, then rings the bell for an answer. */
function simulateAgent(s: FakeSession) {
  const steps = [
    `${C.dim}✻ Reading src/routes/checkout.ts…${C.r}`,
    `${C.dim}✻ Writing a fix for the webhook retry…${C.r}`,
    `${C.dim}✻ Running npm test… 42 passed${C.r}`,
  ];
  steps.forEach((line, i) => s.timers.push(window.setTimeout(() => s.emit({ type: "output", data: `${line}\r\n` }), 900 * (i + 1))));
  s.timers.push(
    window.setTimeout(() => {
      s.emit({ type: "output", data: `\x07${C.y}?${C.r} Apply this change to src/routes/checkout.ts? ${C.dim}(y/n)${C.r} ` });
    }, 900 * (steps.length + 1)),
  );
}

function simulateDrop(s: FakeSession) {
  s.raw({ type: "output", data: "\r\nConnection to 203.0.113.24 closed by remote host.\r\n" });
  // The server carries on without us; tmux keeps it all.
  s.scrollback += `\r\n${WHILE_AWAY.join("\r\n")}\r\n${s.prompt}`;
  if (!autoReconnect) {
    s.emit({ type: "status", state: { kind: "disconnected", reason: "The server stopped responding" } });
    return;
  }
  s.emit({
    type: "status",
    state: { kind: "reconnecting", attempt: 1, retryInMs: 4000, reason: "The server stopped responding" },
  });
  s.timers.push(
    window.setTimeout(() => {
      s.emit({ type: "status", state: { kind: "connecting", attempt: 2 } });
      s.raw({ type: "status", state: { kind: "connected", durable: true } });
      s.raw({ type: "output", data: `\x1b[H\x1b[2J${WHILE_AWAY.slice(-5).join("\r\n")}\r\n${s.prompt}` });
    }, 4000),
  );
}

// ---- containers

const containers: Record<string, Container[]> = {
  "cfg:prod-api": [
    { engine: "docker", id: "a1b2c3d4e5f6", name: "app-api-1", image: "ghcr.io/acme/api:2.14.0", running: true, state: "running", status: "Up 3 days (healthy)", ports: "0.0.0.0:8080->8080/tcp", project: "app" },
    { engine: "docker", id: "b2c3d4e5f6a1", name: "app-db-1", image: "postgres:17", running: true, state: "running", status: "Up 3 days", ports: "5432/tcp", project: "app" },
    { engine: "docker", id: "c3d4e5f6a1b2", name: "app-cache-1", image: "redis:8-alpine", running: true, state: "running", status: "Up 3 days", ports: "6379/tcp", project: "app" },
    { engine: "docker", id: "d4e5f6a1b2c3", name: "nightly-backup", image: "restic/restic:0.18", running: false, state: "exited", status: "Exited (0) 6 hours ago", ports: "", project: null },
  ],
  "cfg:homelab": [
    { engine: "docker", id: "e5f6a1b2c3d4", name: "jellyfin", image: "jellyfin/jellyfin", running: true, state: "running", status: "Up 12 days", ports: "8096/tcp", project: "media" },
    { engine: "docker", id: "f6a1b2c3d4e5", name: "pihole", image: "pihole/pihole", running: true, state: "running", status: "Up 12 days (healthy)", ports: "53/udp", project: null },
    { engine: "docker", id: "a1b2c3d4e5f7", name: "home-assistant", image: "homeassistant/home-assistant", running: true, state: "running", status: "Up 4 days", ports: "", project: null },
  ],
};

// ---- metrics

const counters = new Map<string, { cpuT: number; cpuI: number; rx: number; tx: number }>();

/** How busy each demo machine is: homelab's disk is nearly full. */
const LOADS: Record<string, { cpu: number; mem: number; disk: number }> = {
  "cfg:prod-api": { cpu: 0.3, mem: 0.6, disk: 0.44 },
  "cfg:homelab": { cpu: 0.08, mem: 0.34, disk: 0.91 },
};

function metricsFor(t: TargetRef): Metrics {
  const key = t.kind === "local" ? "local" : t.hostId;
  const load = LOADS[key] ?? { cpu: 0.18, mem: 0.39, disk: 0.44 };
  const c = counters.get(key) ?? { cpuT: 1e7, cpuI: 8e6, rx: 2.31e12, tx: 1.12e12 };
  const busy = load.cpu + 0.12 * Math.sin(Date.now() / 9000) + Math.random() * 0.08;
  c.cpuT += 1200;
  c.cpuI += 1200 * (1 - busy);
  c.rx += 1.4e6 + Math.random() * 2.6e6;
  c.tx += 2.5e5 + Math.random() * 7e5;
  counters.set(key, c);
  const name = hostFor(t).name;
  return {
    atMs: Date.now(),
    hostname: name,
    kernel: "Linux 6.12.0-28-generic",
    cores: 8,
    uptimeSecs: 86400 * 37 + 3600 * 5,
    load: [1.42 + Math.random() * 0.3, 1.18, 0.97],
    cpuTotal: c.cpuT,
    cpuIdle: c.cpuI,
    memTotal: 32 * 2 ** 30,
    memAvailable: 32 * 2 ** 30 * (1 - load.mem) + Math.random() * 2 ** 28,
    swapTotal: 4 * 2 ** 30,
    swapFree: 3.7 * 2 ** 30,
    netRx: c.rx,
    netTx: c.tx,
    netIfaces: ["eth0"],
    month: { month: "2026-10", rx: 412.6 * 2 ** 30, tx: 128.3 * 2 ** 30 },
    disks: [
      { mount: "/", size: 480 * 2 ** 30, used: 480 * 2 ** 30 * load.disk },
      { mount: "/var/lib/postgresql", size: 960 * 2 ** 30, used: 702 * 2 ** 30 },
    ],
  };
}

export const mockBackend: Backend = {
  isDemo: true,
  appVersion: async () => "0.3.5",
  localIdentity: async () => ({ user: "sam", hostname: "sams-macbook.local" }),
  async checkUpdate() {
    await wait(900);
    return {
      version: "0.3.6",
      notes: "Faster file browser\nNew terminal themes\nFixes for reconnecting after sleep",
      date: new Date().toISOString(),
    };
  },
  async installUpdate(onProgress) {
    for (let i = 1; i <= 20; i++) {
      await wait(90);
      onProgress(i / 20);
    }
  },
  async installPortableUpdate(onProgress) {
    await this.installUpdate(onProgress);
  },
  async relaunch() {
    location.reload();
  },
  async openExternal(url) {
    window.open(url, "_blank");
  },
  listHosts: async () => hosts.slice(),
  async addHost(input) {
    const [userPart, hostPart] = input.address.includes("@") ? input.address.split("@") : [null, input.address];
    const h: Host = {
      id: `saved:${Date.now()}`,
      name: input.name || hostPart,
      hostname: hostPart,
      user: userPart,
      port: null,
      identityFile: input.identityFile ?? null,
      source: "saved",
      rememberedPassword: !!input.password && !!input.rememberPassword,
    };
    hosts.push(h);
    return { host: h, warning: null };
  },
  async setZoom() {},
  isPortable: async () => false,
  async notify(title, body) {
    console.info(`[notification] ${title}: ${body}`);
  },
  async forgetPassword(id) {
    const h = hosts.find((x) => x.id === id);
    if (h) h.rememberedPassword = false;
  },
  async removeHost(id) {
    const i = hosts.findIndex((h) => h.id === id);
    if (i >= 0) hosts.splice(i, 1);
  },

  async openSession(paneId, target, sessionKey, _cols, _rows, container, onEvent) {
    const { name, user } = hostFor(target);
    const prompt = container
      ? `${C.g}root@${container.id}${C.r}:${C.bl}/app${C.r}# `
      : `${C.g}${user}@${name}${C.r}:${C.bl}~${C.r}$ `;
    const s: FakeSession = { emit: onEvent, raw: onEvent, line: "", prompt, timers: [], scrollback: "" };
    s.emit = (e) => {
      if (e.type === "output") s.scrollback += e.data;
      onEvent(e);
    };
    sessions.set(paneId, s);
    byKey.set(sessionKey, s);
    onEvent({ type: "status", state: { kind: "connecting", attempt: 1 } });
    s.timers.push(
      window.setTimeout(() => {
        onEvent({ type: "status", state: { kind: "connected", durable: target.kind !== "local" } });
        s.emit({
          type: "output",
          data: container?.run === "logs"
            ? "2026-10-29T23:10:02Z INFO  GET /v1/orders 200 18ms\r\n2026-10-29T23:10:04Z INFO  POST /v1/checkout 201 64ms\r\n"
            : container || target.kind === "local"
              ? prompt
              : new URLSearchParams(location.search).get("scene") === "simple"
                ? simpleBanner(name, user)
                : banner(name, user),
        });
        if (new URLSearchParams(location.search).get("scene") === "reconnect" && target.kind !== "local") {
          s.timers.push(window.setTimeout(() => simulateDrop(s), 600));
        }
      }, 350),
    );
  },
  writeSession(paneId, data) {
    const s = sessions.get(paneId);
    if (!s) return;
    for (const ch of data) {
      if (ch === "\r") {
        const cmd = s.line.trim();
        s.line = "";
        s.emit({ type: "output", data: "\r\n" });
        if (cmd === "drop") return simulateDrop(s);
        if (cmd === "claude") return simulateAgent(s);
        const canned: Record<string, string> = {
          ls: `${C.bl}dist${C.r}  Dockerfile  ${C.bl}node_modules${C.r}  package.json  README.md  ${C.bl}src${C.r}`,
          uptime: " 23:14:02 up 37 days,  5:02,  1 user,  load average: 1.42, 1.18, 0.97",
          whoami: "root",
          "node -v": "v22.11.0",
        };
        if (cmd && canned[cmd]) s.emit({ type: "output", data: `${canned[cmd]}\r\n` });
        else if (cmd) s.emit({ type: "output", data: `${C.dim}(demo) try “drop” to see a reconnect${C.r}\r\n` });
        s.emit({ type: "output", data: s.prompt });
      } else if (ch === "\x7f") {
        if (s.line) {
          s.line = s.line.slice(0, -1);
          s.emit({ type: "output", data: "\b \b" });
        }
      } else if (ch >= " ") {
        s.line += ch;
        s.emit({ type: "output", data: ch });
      }
    }
  },
  resizeSession(paneId, _cols, rows) {
    // Like tmux, redraw the screen when the size changes.
    const s = sessions.get(paneId);
    if (!s?.scrollback) return;
    window.clearTimeout(redrawTimers.get(paneId));
    redrawTimers.set(
      paneId,
      window.setTimeout(() => {
        const lines = s.scrollback.replace(/\x08 \x08/g, "").split("\r\n").slice(-Math.max(1, rows - 1));
        s.raw({ type: "output", data: `\x1b[H\x1b[2J${lines.join("\r\n")}` });
      }, 120),
    );
  },
  setAutoReconnect(enabled) {
    autoReconnect = enabled;
  },
  reconnectSession(paneId) {
    const s = sessions.get(paneId);
    if (!s) return;
    s.raw({ type: "status", state: { kind: "connected", durable: true } });
    s.raw({ type: "output", data: `\x1b[H\x1b[2J${WHILE_AWAY.slice(-5).join("\r\n")}\r\n${s.prompt}` });
  },
  async localAuth() {
    await wait(120);
    return {
      keys: [
        { path: "~/.ssh/id_ed25519", name: "id_ed25519", kind: "ED25519", comment: "sam@macbook" },
        { path: "~/.ssh/work_rsa", name: "work_rsa", kind: "RSA", comment: "sam@acme" },
      ],
      agents: [
        { name: "ssh-agent", socket: null, keys: 1 },
        { name: "1Password", socket: "~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock", keys: 3 },
      ],
    };
  },
  async testConnection(input) {
    await wait(900);
    const host = input.address.split("@").pop()?.split(":")[0] ?? "";
    if (!demoTested.has(host)) {
      return {
        hostKey: { kind: "unknown", fingerprints: [{ kind: "ED25519", sha256: "SHA256:8vKq2mT0pXc4LhRj7nWfB1sYaE6uDzG3oQ9iN5kHtVw" }] },
        signedIn: false,
        tmux: null,
        message: null,
      };
    }
    if (input.password === "wrong") {
      return { hostKey: { kind: "known" }, signedIn: false, tmux: null, message: "The server didn't accept that password." };
    }
    return { hostKey: { kind: "known" }, signedIn: true, tmux: !host.startsWith("10."), message: null };
  },
  async trustTestHostKey(input) {
    demoTested.add(input.address.split("@").pop()?.split(":")[0] ?? "");
  },
  async checkHostKey(hostId, changed) {
    await wait(250);
    // The demo's "pi" is a server you've never connected to.
    if (hostId === "cfg:pi" && !trustedDemo.has(hostId)) {
      return { kind: "unknown", fingerprints: [{ kind: "ED25519", sha256: "SHA256:q3pV0mXh7cJ4c1nQyTz8bW2fK9sL6aRgE5uYdN0vH1o" }] };
    }
    if (changed) return { kind: "changed", old: [{ kind: "ED25519", sha256: "SHA256:7Yb0kq…" }], new: [{ kind: "ED25519", sha256: "SHA256:Lm2x9P…" }] };
    return { kind: "known" };
  },
  async trustHostKey(hostId) {
    trustedDemo.add(hostId);
  },
  async tmuxScroll(_target, _key, position) {
    // The demo terminals aren't tmux, so they scroll on their own.
    return { inMode: !!position, position: position ?? 0, history: 0, height: 24, limit: 2000 };
  },
  async sessionHistory(_target, sessionKey) {
    const s = byKey.get(sessionKey);
    return s ? stripAnsi(s.scrollback).replace(/\r/g, "").replace(/\x08 \x08/g, "") : "";
  },
  closeSession(paneId) {
    sessions.get(paneId)?.timers.forEach(clearTimeout);
    sessions.delete(paneId);
  },

  async listDir(_t, path) {
    await wait(140);
    return listing(path);
  },
  async readFile(_t, path): Promise<TextFile> {
    await wait(120);
    const name = path.split("/").pop() ?? "";
    if (name.endsWith(".gz")) return { path, content: null, binary: true, size: 48_211_332 };
    return { path, content: textFiles[name] ?? `# ${name}\n\nSample content from the demo backend.\n`, binary: false, size: 1843 };
  },
  async writeFile(_t, path) {
    await wait(200);
    const parent = path.slice(0, path.lastIndexOf("/")) || "/";
    const name = path.split("/").pop()!;
    if (!(tree[parent] ?? []).some((x) => x.name === name)) tree[parent] = [...(tree[parent] ?? []), file(name, 0, 0)];
  },
  async archivePaths(_t, dir, _names, archive) {
    await wait(700);
    tree[dir] = [...(tree[dir] ?? []), file(archive, 18 * 2 ** 20, 0)];
  },
  async makeDir(_t, path) {
    await wait(120);
    const parent = path.slice(0, path.lastIndexOf("/")) || "/";
    tree[parent] = [...(tree[parent] ?? []), dir(path.split("/").pop()!, 0)];
  },
  async renamePath(_t, from, to) {
    await wait(120);
    const parent = from.slice(0, from.lastIndexOf("/")) || "/";
    const e = tree[parent]?.find((x) => x.name === from.split("/").pop());
    if (e) e.name = to.split("/").pop()!;
  },
  async deletePath(_t, path) {
    await wait(120);
    const parent = path.slice(0, path.lastIndexOf("/")) || "/";
    tree[parent] = (tree[parent] ?? []).filter((x) => x.name !== path.split("/").pop());
  },
  async downloadPath(_t, path, _isDir, hooks) {
    await fakeTransfer(hooks, 48 * 1024 * 1024);
    return `~/Downloads/${path.split("/").pop()}`;
  },
  async uploadPaths(_t, _paths, _dir, _overwrite, hooks) {
    await fakeTransfer(hooks, 23 * 1024 * 1024);
  },
  async writeClipboard(text) {
    await navigator.clipboard.writeText(text);
  },
  cancelTransfer(id) {
    fakeCancels.get(id)?.();
  },
  async sessionCwd() {
    return "/var/www/app";
  },
  async detectAgents() {
    await wait(300);
    return ["claude", "codex", "opencode", "omp"];
  },
  async paneInfo() {
    return { command: "zsh", cwd: "/var/www/app", idle: true };
  },
  async securityCheckup() {
    await wait(1400);
    return {
      asRoot: true,
      checks: [
        { id: "password-login", title: "Password login", verdict: "bad", detail: "Anyone can try passwords on this server, and bots do, all day. Keys are much harder to guess.", fix: "printf 'PasswordAuthentication no\\nKbdInteractiveAuthentication no\\n' | sudo tee /etc/ssh/sshd_config.d/00-ssheesh.conf && sudo systemctl reload ssh", fixNote: "Turns password login off. Check that logging in with your key works first, or you could lock yourself out." },
        { id: "root-login", title: "Root login", verdict: "good", detail: "root can only log in with a key.", fix: null, fixNote: null },
        { id: "open-ports", title: "Open ports", verdict: "warn", detail: "Open to the internet: 22 (ssh), 80 and 443 (web), 5432 (PostgreSQL). A database usually shouldn't be reachable from outside.", fix: null, fixNote: null },
        { id: "firewall", title: "Firewall", verdict: "good", detail: "ufw is on.", fix: null, fixNote: null },
        { id: "docker-ports", title: "Docker ports", verdict: "warn", detail: "Containers publish 5432 on every address. Docker's ports go around ufw, so the firewall doesn't block them. Publish them as 127.0.0.1:5432:5432 in your compose file instead.", fix: null, fixNote: null },
        { id: "auto-updates", title: "Automatic updates", verdict: "good", detail: "Security updates install by themselves.", fix: null, fixNote: null },
        { id: "security-updates", title: "Security updates", verdict: "bad", detail: "7 security fixes are waiting, including openssh-server, openssl and the kernel. They fix CVE-2026-10001, CVE-2026-10002 and 4 more.", fix: "sudo apt-get update && sudo apt-get upgrade --with-new-pkgs", fixNote: "Installs the fixes. A kernel fix needs a restart afterwards." },
        { id: "brute-force", title: "Brute-force protection", verdict: "warn", detail: "Nothing slows down password guessing (no fail2ban or CrowdSec), and password login is on.", fix: "sudo apt-get install -y fail2ban", fixNote: "Installs fail2ban, which blocks addresses that keep failing to log in." },
        { id: "reboot", title: "Restart", verdict: "good", detail: "No updates are waiting for a restart.", fix: null, fixNote: null },
      ],
    };
  },
  async diskReport() {
    await wait(900);
    const GB = 1024 ** 3;
    return {
      asRoot: true,
      volumes: [
        { mount: "/", size: 80 * GB, used: 71.4 * GB },
        { mount: "/mnt/backups", size: 500 * GB, used: 212 * GB },
      ],
      docker: { images: 14.2 * GB, imagesReclaimable: 2.6 * GB, unusedImages: 7, containers: 1.1 * GB, containersReclaimable: 0.4 * GB, volumes: 6.3 * GB, volumesReclaimable: 0, buildCache: 3.7 * GB, buildCacheReclaimable: 3.7 * GB },
      journal: 1.6 * GB,
      cleanups: [
        { id: "docker-images", title: "Unused Docker images", detail: "Removes the 7 images no container uses; they download again when needed.", command: "docker image prune -a", bytes: 2.6 * GB },
        { id: "docker-build-cache", title: "Docker build cache", detail: "Leftovers from building images. The next build just takes a bit longer.", command: "docker builder prune -a", bytes: 3.7 * GB },
        { id: "journal", title: "Old system logs", detail: "Trims the systemd journal to the newest 200 MB.", command: "sudo journalctl --vacuum-size=200M", bytes: 1.4 * GB },
        { id: "apt-cache", title: "Downloaded packages", detail: "Installers apt keeps after installing. Nothing installed is removed.", command: "sudo apt-get clean", bytes: 0.6 * GB },
      ],
    };
  },
  async diskBiggest(_t, path) {
    await wait(700);
    const GB = 1024 ** 3;
    const join = (name: string) => (path === "/" ? `/${name}` : `${path}/${name}`);
    const rows: [string, number, boolean][] =
      path === "/"
        ? [["var", 38.2 * GB, true], ["home", 14.1 * GB, true], ["usr", 9.3 * GB, true], ["opt", 4.8 * GB, true], ["snap", 2.2 * GB, true], ["root", 1.4 * GB, true], ["boot", 0.3 * GB, true], ["swapfile", 2 * GB, false]]
        : [["lib", 29.4 * GB, true], ["log", 6.1 * GB, true], ["cache", 1.9 * GB, true], ["backups", 0.6 * GB, true], ["www", 0.2 * GB, true]];
    return rows.map(([name, bytes, isDir]) => ({ path: join(name), name, bytes, isDir })).sort((a, b) => b.bytes - a.bytes);
  },
  async pickFiles() {
    return [];
  },
  onFileDrop(handler) {
    // The browser demo has no desktop drag and drop; tests and recordings
    // play one through this hook instead.
    (window as unknown as { __ssheeshDrop?: typeof handler }).__ssheeshDrop = handler;
    return () => {
      delete (window as unknown as { __ssheeshDrop?: typeof handler }).__ssheeshDrop;
    };
  },

  async listContainers(target) {
    await wait(300);
    const list = target.kind === "host" ? (containers[target.hostId] ?? []) : [];
    return { engines: list.length ? ["docker"] : [], containers: list.map((c) => ({ ...c })), problems: [] };
  },
  async containerAction(target, _engine, id, action) {
    await wait(500);
    const c = target.kind === "host" ? containers[target.hostId]?.find((x) => x.id === id) : undefined;
    if (!c) return;
    c.running = action !== "stop";
    c.state = c.running ? "running" : "exited";
    c.status = c.running ? "Up 1 second" : "Exited (0) 1 second ago";
  },

  async fetchMetrics(target) {
    await wait(60);
    return metricsFor(target);
  },

  async scanWebApps(target) {
    await wait(1100);
    if (target.kind !== "host") throw { code: "unsupported", message: "Tunnels are for servers." };
    const scan = demoWebApps[target.hostId] ?? { apps: [], curl: true };
    return { apps: scan.apps.map((a) => ({ ...a })), curl: scan.curl, asRoot: true };
  },
  async startTunnel(id, hostId, forward, onState) {
    await wait(80);
    const port = forward.localPort;
    if ([...demoTunnels].some(([other, t]) => other !== id && t.port === port)) {
      throw { code: "exists", message: `Another tunnel already uses port ${port} on this computer.` };
    }
    if (demoBusyPorts.has(port)) throw { code: "exists", message: `Port ${port} on this computer is already in use.` };
    demoTunnels.get(id)?.timers.forEach(clearTimeout);
    const timers: ReturnType<typeof setTimeout>[] = [];
    demoTunnels.set(id, { port, timers });
    onState({ kind: "starting" });
    timers.push(setTimeout(() => onState({ kind: "on", warning: null }), 600 + Math.random() * 600));
    // The homelab's Wi-Fi drops now and then; its tunnels come back by themselves.
    if (hostId === "cfg:homelab") {
      timers.push(setTimeout(() => onState({ kind: "reconnecting", attempt: 1, retryInMs: 2000, reason: "The server stopped responding" }), 9000));
      timers.push(setTimeout(() => onState({ kind: "on", warning: null }), 11800));
    }
  },
  stopTunnel(id) {
    demoTunnels.get(id)?.timers.forEach(clearTimeout);
    demoTunnels.delete(id);
  },
  async localPortProblem(port) {
    await wait(40);
    if (!Number.isInteger(port) || port < 1 || port > 65535) return "Pick a port on this computer, from 1 to 65535.";
    if (port < 1024) return `Port ${port} on this computer needs administrator rights. Pick one above 1023.`;
    if (demoBusyPorts.has(port) || [...demoTunnels.values()].some((t) => t.port === port)) return `Port ${port} on this computer is already in use.`;
    return null;
  },
  async pickLocalPort(preferred, avoid) {
    await wait(40);
    const free = (p: number) => p >= 1024 && p <= 65535 && !avoid.includes(p) && !demoBusyPorts.has(p) && ![...demoTunnels.values()].some((t) => t.port === p);
    const pick = [preferred, preferred < 1024 ? preferred + 8000 : 0, preferred + 10000].find(free);
    return pick ?? 49152 + Math.floor(Math.random() * 16000);
  },
};

// ---------------------------------------------------------------- tunnels (demo)

/** Ports "this computer" already uses in the demo: AirPlay takes 5000 and 7000 on a Mac. */
const demoBusyPorts = new Set([5000, 7000]);
const demoTunnels = new Map<string, { port: number; timers: ReturnType<typeof setTimeout>[] }>();

function app(port: number, process: string | null, title: string | null, more: Partial<WebApp> = {}): WebApp {
  const web = more.web ?? true;
  return { port, remoteHost: "127.0.0.1", process, knownAs: null, title, scheme: web ? "http" : null, status: web ? 200 : null, web, public: false, ...more };
}

const demoWebApps: Record<string, { apps: WebApp[]; curl: boolean }> = {
  "cfg:prod-api": {
    curl: true,
    apps: [
      app(80, "nginx", null, { status: 301, public: true }),
      app(3000, "grafana", "Grafana"),
      app(8080, "docker-proxy", "Acme API · Swagger UI", { knownAs: "Docker" }),
      app(9090, "prometheus", "Prometheus Time Series Collection and Processing Server"),
      app(15672, "beam.smp", "RabbitMQ Management"),
      app(5432, "postgres", null, { knownAs: "PostgreSQL", web: false }),
      app(6379, "redis-server", null, { knownAs: "Redis", web: false }),
    ],
  },
  "cfg:homelab": {
    curl: true,
    apps: [
      app(5000, "python3", "Photo frame"),
      app(8096, "jellyfin", "Jellyfin", { public: true, remoteHost: "192.168.1.20" }),
      app(8123, "python3", "Home Assistant"),
      app(9000, "docker-proxy", "Portainer", { knownAs: "Docker", public: true }),
      app(32400, "Plex Media Serv", "Plex", { status: 401, public: true }),
      app(445, "smbd", null, { knownAs: "Windows file sharing", web: false, public: true }),
    ],
  },
  "cfg:pi": {
    curl: false,
    apps: [
      app(80, "lighttpd", null, { scheme: null, status: null, public: true }),
      app(1880, "node", null, { scheme: null, status: null }),
      app(53, "pihole-FTL", null, { knownAs: "DNS", web: false, public: true }),
    ],
  },
  "saved:db": {
    curl: true,
    apps: [app(9187, "postgres_export", "Postgres Exporter"), app(5432, "postgres", null, { knownAs: "PostgreSQL", web: false, public: true })],
  },
};
