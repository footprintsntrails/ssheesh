import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { backend } from "./backend";
import { HOST_COLORS, hostColor, lookOf, useLabels } from "./labels";
import { classifyHost, detectRole, startRoleDetection } from "./roles";
import { useStore } from "./store";
import type { Container, ContainerList, WebApp, WebAppScan } from "./types";

const app = (port: number, process: string | null, more: Partial<WebApp> = {}): WebApp => ({
  port,
  remoteHost: "127.0.0.1",
  process,
  knownAs: null,
  title: null,
  scheme: null,
  status: null,
  web: false,
  public: false,
  ...more,
});
const page = (port: number, process: string | null, more: Partial<WebApp> = {}) => app(port, process, { scheme: "http", status: 200, web: true, ...more });
const scan = (...apps: WebApp[]): WebAppScan => ({ apps, curl: true, asRoot: true });
const container = (name: string, running = true): Container => ({
  engine: "docker",
  id: name,
  name,
  image: name,
  running,
  state: running ? "running" : "exited",
  status: "",
  ports: "",
  project: null,
});
const list = (...containers: Container[]): ContainerList => ({ engines: containers.length ? ["docker"] : [], containers, problems: [] });
const none = list();

describe("classifyHost", () => {
  it("sees a website in a web server, or a page on 80 or 443", () => {
    expect(classifyHost(scan(app(8081, "caddy")), none)).toBe("globe");
    expect(classifyHost(scan(app(80, "nginx", { status: 301, scheme: "http", web: true })), none)).toBe("globe");
    expect(classifyHost(scan(page(443, null, { scheme: "https" })), none)).toBe("globe");
    expect(classifyHost(scan(page(443, "docker-proxy", { knownAs: "Docker" })), list(container("traefik"))), "a site in a container").toBe("globe");
  });

  it("takes 80 and 443 at their word only when the server can't ask them", () => {
    const quiet = app(443, null, { web: true });
    expect(classifyHost(scan(quiet), none)).toBe("server");
    expect(classifyHost({ ...scan(quiet), curl: false }, none)).toBe("globe");
  });

  it("doesn't call any web app a website", () => {
    expect(classifyHost(scan(page(3000, "grafana"), page(9090, "prometheus")), none)).toBe("server");
  });

  it("puts a website above its database", () => {
    expect(classifyHost(scan(app(80, "nginx"), app(5432, "postgres", { knownAs: "PostgreSQL" })), none)).toBe("globe");
  });

  it("sees databases by name, by program and behind Docker's proxy", () => {
    expect(classifyHost(scan(app(6379, "redis-server", { knownAs: "Redis" })), none)).toBe("database");
    expect(classifyHost(scan(app(9000, "clickhouse-serv")), none)).toBe("database");
    expect(classifyHost(scan(app(3306, "docker-proxy", { knownAs: "Docker" })), list(container("mysql"))), "above the containers").toBe("database");
    expect(classifyHost(scan(app(3306, null)), none), "no program name without root").toBe("database");
  });

  it("doesn't take a look-alike for a database", () => {
    expect(classifyHost(scan(page(9187, "postgres_export")), none)).toBe("server");
    expect(classifyHost(scan(app(11211, "memcached", { knownAs: "Memcached" })), none)).toBe("server");
    expect(classifyHost(scan(app(6379, "node")), none), "the program says what it is").toBe("server");
  });

  it("calls a container host without a website or database a box", () => {
    expect(classifyHost(scan(page(9000, "docker-proxy", { knownAs: "Docker", title: "Portainer" })), none)).toBe("box");
    expect(classifyHost(scan(), list(container("worker")))).toBe("box");
    expect(classifyHost(null, list(container("worker"))), "no scan at all").toBe("box");
    expect(classifyHost(scan(), list(container("old", false))), "only stopped ones").toBe("server");
  });

  it("calls the rest a server", () => {
    expect(classifyHost(scan(app(53, "named", { knownAs: "DNS" })), none)).toBe("server");
    expect(classifyHost(null, null)).toBe("server");
  });
});

describe("looks", () => {
  it("gives every server a steady color that isn't gray", () => {
    const ids = ["cfg:prod-api", "cfg:homelab", "cfg:pi", "saved:db"];
    for (const id of ids) {
      expect(hostColor(id)).toBe(hostColor(id));
      expect(HOST_COLORS.slice(0, -1)).toContain(hostColor(id));
    }
    expect(new Set(ids.map(hostColor)).size, "the demo's servers differ").toBe(ids.length);
  });

  it("shows yours, else the guess, else a plain server", () => {
    const yours = { icon: "game", color: "#ff9f43" };
    expect(lookOf("cfg:pi", yours, { icon: "globe" })).toEqual({ ...yours, auto: false });
    expect(lookOf("cfg:pi", undefined, { icon: "globe" })).toEqual({ icon: "globe", color: hostColor("cfg:pi"), auto: true });
    expect(lookOf("cfg:pi", undefined, undefined).icon).toBe("server");
  });
});

describe("startRoleDetection", () => {
  const db = { kind: "host", hostId: "saved:db" } as const;
  let stop = () => {};

  beforeEach(() => {
    // Detection waits a few seconds after a connect; tests skip ahead.
    vi.useFakeTimers();
    useLabels.setState({ looks: {}, autoLooks: {} });
    useStore.setState({
      sessions: {},
      panes: {
        a: { id: "a", kind: "terminal", target: db },
        b: { id: "b", kind: "terminal", target: db },
        c: { id: "c", kind: "terminal", target: { kind: "host", hostId: "cfg:homelab" } },
        d: { id: "d", kind: "terminal", target: { kind: "local" } },
      },
    });
  });

  afterEach(() => {
    stop();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  const connect = (id: string) => {
    useStore.getState().setSession(id, { kind: "connected", durable: true });
    vi.advanceTimersByTime(5000);
  };

  it("looks once per server, on its first connect, and keeps the answer", async () => {
    const scans = vi.spyOn(backend, "scanWebApps").mockResolvedValue(scan(app(5432, "postgres", { knownAs: "PostgreSQL" })));
    vi.spyOn(backend, "listContainers").mockResolvedValue(none);
    stop = startRoleDetection();
    connect("d");
    expect(scans).not.toHaveBeenCalled();
    connect("a");
    connect("b");
    expect(scans).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => expect(useLabels.getState().autoLooks["saved:db"]).toEqual({ icon: "database" }));
    useStore.getState().setSession("a", { kind: "reconnecting", attempt: 1, retryInMs: 1000, reason: "" });
    connect("a");
    expect(scans).toHaveBeenCalledTimes(1);
  });

  it("keeps a plain server too, and leaves servers you picked a look for alone", async () => {
    const scans = vi.spyOn(backend, "scanWebApps").mockResolvedValue(scan());
    vi.spyOn(backend, "listContainers").mockResolvedValue(none);
    useLabels.getState().setLook("cfg:homelab", { icon: "home", color: HOST_COLORS[1] });
    stop = startRoleDetection();
    connect("c");
    expect(scans).not.toHaveBeenCalled();
    connect("a");
    await vi.waitFor(() => expect(useLabels.getState().autoLooks["saved:db"]).toEqual({ icon: "server" }));
  });

  it("stays quiet when it fails, and tries again on a later connect", async () => {
    const scans = vi.spyOn(backend, "scanWebApps").mockRejectedValue({ code: "network", message: "down" });
    vi.spyOn(backend, "listContainers").mockResolvedValue(list(container("worker")));
    stop = startRoleDetection();
    connect("a");
    expect(scans).toHaveBeenCalledTimes(1);
    await vi.runAllTimersAsync();
    expect(useLabels.getState().autoLooks["saved:db"]).toBeUndefined();
    expect(useStore.getState().toasts).toEqual([]);

    scans.mockResolvedValue(scan());
    useStore.getState().setSession("a", { kind: "disconnected", reason: "" });
    connect("a");
    await vi.waitFor(() => expect(useLabels.getState().autoLooks["saved:db"]).toEqual({ icon: "box" }));
  });
});

describe("detectRole", () => {
  afterEach(() => vi.restoreAllMocks());

  it("goes by the containers when the server can't list its ports", async () => {
    vi.spyOn(backend, "scanWebApps").mockRejectedValue({ code: "unsupported", message: "Neither ss nor netstat is installed." });
    vi.spyOn(backend, "listContainers").mockResolvedValue(list(container("worker")));
    expect(await detectRole("cfg:pi")).toBe("box");
  });

  it("trusts a website it found even when the containers didn't answer", async () => {
    vi.spyOn(backend, "scanWebApps").mockResolvedValue(scan(app(80, "nginx")));
    vi.spyOn(backend, "listContainers").mockRejectedValue({ code: "network", message: "down" });
    expect(await detectRole("cfg:pi")).toBe("globe");
  });

  it("doesn't settle on a plain server with half the answer", async () => {
    vi.spyOn(backend, "scanWebApps").mockResolvedValue(scan());
    vi.spyOn(backend, "listContainers").mockRejectedValue({ code: "network", message: "down" });
    expect(await detectRole("cfg:homelab")).toBeNull();
  });
});
