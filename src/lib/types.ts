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
  /** A password for it is in the system keychain. */
  rememberedPassword?: boolean;
}

export interface NewHost {
  name?: string;
  address: string;
  identityFile?: string;
  /** Leave empty to sign in with a key. */
  password?: string;
  /** Keep it in the system keychain, or only until the app quits. */
  rememberPassword?: boolean;
  /** Sign in through a password manager's key agent (its socket path). */
  identityAgent?: string;
}

export interface LocalKey {
  path: string;
  name: string;
  kind: string;
  comment: string;
}

export interface KeyAgent {
  name: string;
  socket: string | null;
  keys: number | null;
}

/** Keys in ~/.ssh and key agents running on this computer. */
export interface LocalAuth {
  keys: LocalKey[];
  agents: KeyAgent[];
}

export interface ConnectionTest {
  hostKey: HostKeyCheck;
  signedIn: boolean;
  tmux: boolean | null;
  message: string | null;
}

export interface AddedHost {
  host: Host;
  /** Set when the password couldn't go into the keychain. */
  warning: string | null;
}

export interface Fingerprint {
  kind: string;
  sha256: string;
}

/** Whether a server's identity (host key) is already trusted. */
export type HostKeyCheck =
  | { kind: "known" }
  | { kind: "unknown"; fingerprints: Fingerprint[] }
  | { kind: "changed"; old: Fingerprint[]; new: Fingerprint[] }
  | { kind: "unreachable"; message: string };

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

export type ErrorCode = "auth" | "hostKey" | "network" | "notFound" | "exists" | "tooLarge" | "unsupported" | "cancelled" | "failed";

/** One finding of the security check-up. */
export interface Check {
  /** "password-login", "root-login", "open-ports", "firewall", "docker-ports", "auto-updates", "brute-force", "reboot". */
  id: string;
  title: string;
  verdict: "good" | "warn" | "bad" | "unknown";
  detail: string;
  /** A command that fixes it, for you to run. */
  fix: string | null;
  fixNote: string | null;
}

export interface Checkup {
  checks: Check[];
  /** False when some checks couldn't read root-only files. */
  asRoot: boolean;
}

export interface DiskVolume {
  mount: string;
  size: number;
  used: number;
}

/** A file or folder in the "biggest" list. */
export interface DiskUsage {
  path: string;
  name: string;
  bytes: number;
  isDir: boolean;
}

/** `docker system df`, in bytes. */
export interface DockerUsage {
  images: number;
  imagesReclaimable: number;
  containers: number;
  containersReclaimable: number;
  volumes: number;
  volumesReclaimable: number;
  buildCache: number;
  buildCacheReclaimable: number;
  /** Images no container uses, when Docker could list them one by one. */
  unusedImages: number | null;
}

/** A safe cleanup: a command you run, and roughly what it frees. */
export interface Cleanup {
  id: string;
  title: string;
  detail: string;
  command: string;
  bytes: number | null;
}

export interface DiskReport {
  volumes: DiskVolume[];
  docker: DockerUsage | null;
  journal: number | null;
  cleanups: Cleanup[];
  asRoot: boolean;
}

/**
 * What a tunnel does: this computer's port opens a port as seen from the
 * server (`remoteHost` "127.0.0.1" is the server itself), or a SOCKS proxy
 * that sends whatever uses it out through the server.
 */
export type Forward =
  | { kind: "local"; localPort: number; remoteHost: string; remotePort: number }
  | { kind: "socks"; localPort: number };

/** How a tunnel is doing. "off" is the UI's own: its switch is off. */
export type TunnelState =
  | { kind: "starting" }
  | { kind: "on"; warning: string | null }
  | { kind: "reconnecting"; attempt: number; retryInMs: number; reason: string }
  | { kind: "failed"; message: string }
  | { kind: "off" };

/** Something listening on TCP on the server. */
export interface WebApp {
  port: number;
  /** Where a tunnel to it connects, as the server sees it: 127.0.0.1 unless it listens on one other address. */
  remoteHost: string;
  /** The program listening ("node", "nginx"), when the server says. */
  process: string | null;
  /** What the port or program is known as ("PostgreSQL", "Docker"). */
  knownAs: string | null;
  /** The page's <title>. */
  title: string | null;
  scheme: "http" | "https" | null;
  /** HTTP status of its front page. */
  status: number | null;
  /** Likely a website: it answered HTTP, or it's a usual web port or program. */
  web: boolean;
  /** Also listens on an address other computers can reach. */
  public: boolean;
}

export interface WebAppScan {
  /** Likely websites first, then by port. */
  apps: WebApp[];
  /** curl is on the server, so page titles could be read. */
  curl: boolean;
  asRoot: boolean;
}

/** A terminal's foreground program and folder, from tmux on the server. */
export interface PaneInfo {
  command: string;
  cwd: string;
  /** The shell is waiting at its prompt. */
  idle: boolean;
}

/** How far an upload or download got. `total` is 0 while the size isn't known. */
export interface TransferProgress {
  done: number;
  total: number;
}

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

/** Where a server terminal's tmux session is scrolled to. */
export interface TmuxScroll {
  /** Scrolled back rather than following the output. */
  inMode: boolean;
  /** Lines scrolled up from the bottom. */
  position: number;
  /** Lines of history above the screen. */
  history: number;
  height: number;
  /** The most history tmux keeps. */
  limit: number;
}
