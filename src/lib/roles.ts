// What a server is for, so its icon says it like a website's favicon: a
// globe for a website, a cylinder for a database, a box for containers. It's
// guessed once per server, the first time a terminal on it connects, from
// what listens on its ports and whether it runs containers. An icon you pick
// yourself always wins; "Automatic" in the picker goes back to the guess.

import { backend } from "./backend";
import { useLabels } from "./labels";
import { useStore } from "./store";
import { toOpError, type ContainerList, type SessionState, type WebApp, type WebAppScan } from "./types";

/** A key of HOST_ICONS. */
export type HostRole = "globe" | "database" | "box" | "server";

/** Programs that serve websites (or sit in front of them), by the name ss shows. */
const WEB_SERVERS = new Set(["nginx", "openresty", "angie", "caddy", "apache2", "httpd", "lighttpd", "traefik"]);

/** What the scan calls databases (its `knownAs`). Memcached is a cache, so it isn't one. */
const DATABASES = new Set([
  "PostgreSQL",
  "MySQL",
  "MariaDB",
  "Redis",
  "MongoDB",
  "ClickHouse",
  "Elasticsearch",
  "OpenSearch",
  "CouchDB",
  "InfluxDB",
  "Cassandra",
  "SQL Server",
  "Oracle",
]);

/** Database programs the scan has no name for. ss cuts names at 15 letters ("clickhouse-serv"). */
const DATABASE_PROGRAMS = new Set([
  "postgres",
  "postmaster",
  "mysqld",
  "mariadbd",
  "redis-server",
  "valkey-server",
  "keydb-server",
  "mongod",
  "mongos",
  "clickhouse",
  "clickhouse-serv",
  "clickhouse-server",
  "influxd",
  "sqlservr",
]);

/** Database ports, for when Docker's proxy stands in front: the scan then only says "Docker". */
const DATABASE_PORTS = new Set([1433, 1521, 3306, 5432, 5984, 6379, 8086, 9042, 9200, 27017, 33060]);

/** A website: a web server program, or a page on 80 or 443 (any port there, when the server has no curl to ask). */
function isWebsite(app: WebApp, curl: boolean): boolean {
  if (app.process && WEB_SERVERS.has(app.process.toLowerCase())) return true;
  return (app.port === 80 || app.port === 443) && (app.scheme !== null || (!curl && app.web));
}

function isDatabase(app: WebApp): boolean {
  if (app.knownAs && DATABASES.has(app.knownAs)) return true;
  const program = app.process?.toLowerCase() ?? null;
  if (program && DATABASE_PROGRAMS.has(program)) return true;
  return (program === null || program === "docker-proxy") && DATABASE_PORTS.has(app.port);
}

function isDocker(app: WebApp): boolean {
  return app.process === "docker-proxy" || app.knownAs === "Docker" || app.knownAs === "Docker API";
}

/**
 * What a server is for, from its scan and its containers; either may be
 * missing. A website wins: it's what people come to the server for, even
 * when a database sits behind it. A database without a website is the
 * server's job too. Containers come last: Docker says how things run, not
 * what for, so a box only marks a container host whose ports say nothing
 * more. Everything else is a plain server.
 */
export function classifyHost(scan: WebAppScan | null, containers: ContainerList | null): HostRole {
  const apps = scan?.apps ?? [];
  if (apps.some((a) => isWebsite(a, scan?.curl ?? true))) return "globe";
  if (apps.some(isDatabase)) return "database";
  if (containers?.containers.some((c) => c.running) || apps.some(isDocker)) return "box";
  return "server";
}

const looking = new Map<string, Promise<HostRole | null>>();

/**
 * Looks at the server now and saves what it found as the server's guessed
 * icon. Null when it couldn't tell; it's quiet about why. One look at a time
 * per server: a second call while one runs gets the same answer.
 */
export function detectRole(hostId: string): Promise<HostRole | null> {
  const busy = looking.get(hostId);
  if (busy) return busy;
  const target = { kind: "host", hostId } as const;
  const run = Promise.allSettled([backend.scanWebApps(target), backend.listContainers(target)])
    .then(([scanned, listed]) => {
      // A server without ss or netstat never gets a scan, so it goes by its containers alone.
      const scan = scanned.status === "fulfilled" ? scanned.value : toOpError(scanned.reason).code === "unsupported" ? null : undefined;
      const containers = listed.status === "fulfilled" ? listed.value : undefined;
      const role = classifyHost(scan ?? null, containers ?? null);
      // Without the scan, or with only the scan and nothing found, the answer could still change: try again next time.
      if (scan === undefined || (role === "server" && containers === undefined)) return null;
      useLabels.getState().setAutoLook(hostId, role);
      return role;
    })
    .finally(() => looking.delete(hostId));
  looking.set(hostId, run);
  return run;
}

type Sessions = Record<string, SessionState | undefined>;

/**
 * Watches terminals connect and, the first time one on a server does, guesses
 * what the server is for, unless you picked its icon or it was guessed
 * before. A guess is kept for good, even "just a server"; one that fails is
 * tried again on a later connect. Returns the unsubscribe.
 */
/** Waits this long after a connect, so the shell you just opened goes first. */
const AFTER_CONNECT_MS = 4000;

export function startRoleDetection(): () => void {
  // A server that couldn't be read is tried once more in this run, not on every reconnect.
  const tries = new Map<string, number>();
  const waiting = new Set<string>();
  const look = (sessions: Sessions, prev: Sessions) => {
    const { panes } = useStore.getState();
    for (const [id, state] of Object.entries(sessions)) {
      if (state?.kind !== "connected" || prev[id]?.kind === "connected") continue;
      const target = panes[id]?.target;
      if (target?.kind !== "host") continue;
      const { looks, autoLooks } = useLabels.getState();
      const hostId = target.hostId;
      if (looks[hostId] || autoLooks[hostId] || waiting.has(hostId) || (tries.get(hostId) ?? 0) >= 2) continue;
      waiting.add(hostId);
      setTimeout(() => {
        waiting.delete(hostId);
        tries.set(hostId, (tries.get(hostId) ?? 0) + 1);
        void detectRole(hostId);
      }, AFTER_CONNECT_MS);
    }
  };
  look(useStore.getState().sessions, {});
  return useStore.subscribe((s, prev) => {
    if (s.sessions !== prev.sessions) look(s.sessions, prev.sessions);
  });
}
