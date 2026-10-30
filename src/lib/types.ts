// Shapes shared with the Rust side (sshee-core). Keep in sync.

export type HostSource = "sshConfig" | "saved";

export interface Host {
  id: string;
  name: string;
  hostname: string;
  user: string | null;
  port: number | null;
  identityFile: string | null;
  source: HostSource;
}

export interface NewHost {
  name?: string;
  address: string;
  identityFile?: string;
}

/** Where something runs: this computer or a server from the list. */
export type TargetRef = { kind: "local" } | { kind: "host"; hostId: string };

export type SessionState =
  | { kind: "connecting"; attempt: number }
  | { kind: "connected"; durable: boolean }
  | { kind: "reconnecting"; attempt: number; retryInMs: number; reason: string }
  | { kind: "disconnected"; reason: string }
  | { kind: "ended"; code: number | null }
  | { kind: "failed"; message: string };

export type SessionEvent = { type: "output"; data: string } | { type: "status"; state: SessionState };

export type Engine = "docker" | "podman";

export interface Container {
  engine: Engine;
  id: string;
  name: string;
  image: string;
  running: boolean;
  state: string;
  status: string;
  ports: string;
  project: string | null;
}

export interface ContainerList {
  engines: Engine[];
  containers: Container[];
  problems: string[];
}

/** A terminal opened into a container: a shell, or following its logs. */
export interface ContainerRef {
  engine: Engine;
  id: string;
  name: string;
  run: "shell" | "logs";
}

export type FileKind = "dir" | "file" | "link" | "other";

export interface FileEntry {
  name: string;
  kind: FileKind;
  isLink: boolean;
  size: number;
  modified: number;
  mode: string;
}

export interface DirListing {
  path: string;
  entries: FileEntry[];
}

export interface TextFile {
  path: string;
  content: string | null;
  binary: boolean;
  size: number;
}

export interface Metrics {
  atMs: number;
  hostname: string;
  kernel: string;
  cores: number;
  uptimeSecs: number;
  load: [number, number, number];
  cpuTotal: number;
  cpuIdle: number;
  memTotal: number;
  memAvailable: number;
  swapTotal: number;
  swapFree: number;
  netRx: number;
  netTx: number;
  netIfaces: string[];
  month: { month: string; rx: number; tx: number } | null;
  disks: { mount: string; size: number; used: number }[];
}

export type ErrorCode = "auth" | "hostKey" | "network" | "notFound" | "exists" | "tooLarge" | "unsupported" | "failed";

export interface OpError {
  code: ErrorCode;
  message: string;
}

export function toOpError(e: unknown): OpError {
  if (e && typeof e === "object" && "code" in e && "message" in e) return e as OpError;
  if (typeof e === "string") return { code: "failed", message: e };
  if (e instanceof Error) return { code: "failed", message: e.message };
  return { code: "failed", message: "Something went wrong." };
}

export function targetKey(t: TargetRef): string {
  return t.kind === "local" ? "local" : `host:${t.hostId}`;
}

export function sameTarget(a: TargetRef | null | undefined, b: TargetRef | null | undefined): boolean {
  return !!a && !!b && targetKey(a) === targetKey(b);
}
