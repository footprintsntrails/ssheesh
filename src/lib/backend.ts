// The one place the UI talks to the outside world. Inside the app this is
// Tauri (Rust); in a plain browser it's a demo backend with sample data, so
// the interface can be designed and screenshotted without servers.

import type { ContainerList, ContainerRef, DirListing, Engine, Host, Metrics, NewHost, SessionEvent, TargetRef, TextFile, AddedHost, HostKeyCheck, LocalAuth, ConnectionTest, TransferProgress, PaneInfo, Checkup, DiskReport, DiskUsage, TmuxScroll, RunOutcome } from "./types";
import { toOpError } from "./types";
import type { Forward, TunnelState, WebAppScan } from "./types";

export interface DropEvent {
  type: "over" | "drop" | "leave";
  paths: string[];
  /** CSS pixels, relative to the window. */
  x: number;
  y: number;
}

/** An upload or download the UI follows: its id (to cancel it) and where progress goes. */
export interface TransferHooks {
  id: string;
  onProgress: (p: TransferProgress) => void;
}

export interface UpdateInfo {
  version: string;
  notes: string;
  date: string | null;
}

/** The welcome choice as the server-side script takes it. */
export interface WelcomeSpec {
  mode: "auto" | "ssheesh" | "server" | "off";
  parts: string[];
  note: string;
}

export interface Backend {
  readonly isDemo: boolean;
  appVersion(): Promise<string>;
  /** This computer's user and machine name, hidden in streamer mode. */
  localIdentity(): Promise<{ user: string; hostname: string }>;
  /** Asks the release server; null when already up to date. */
  checkUpdate(): Promise<UpdateInfo | null>;
  /** Downloads and installs the update found by checkUpdate. Progress is 0..1, or null if unknown. */
  installUpdate(onProgress: (fraction: number | null) => void): Promise<void>;
  /** The portable .exe's update: downloaded, checked, and swapped in for the next start. */
  installPortableUpdate(onProgress: (fraction: number | null) => void): Promise<void>;
  relaunch(): Promise<void>;
  /** Opens a web link in the system browser. */
  openExternal(url: string): Promise<void>;
  /** Opens the folder these files are in, with them selected, in the system's file manager. */
  revealFiles(paths: string[]): Promise<void>;
  /**
   * Puts text on the system clipboard. Works without a click, for terminal
   * selections and programs' own copies, which webviews often refuse.
   */
  writeClipboard(text: string): Promise<void>;

  listHosts(): Promise<Host[]>;
  addHost(input: NewHost): Promise<AddedHost>;
  removeHost(id: string): Promise<void>;
  /** Keys in ~/.ssh and key agents running on this computer. */
  localAuth(): Promise<LocalAuth>;
  /** Tries a server from the "Add a server" form before it's saved. */
  testConnection(input: NewHost): Promise<ConnectionTest>;
  /** Trusts the identity a connection test showed. */
  trustTestHostKey(input: NewHost): Promise<void>;
  /** Is the server's key known? If not, fetches it so you can check it first. */
  checkHostKey(hostId: string, changed: boolean): Promise<HostKeyCheck>;
  /** Saves the key you were just shown to ~/.ssh/known_hosts. */
  trustHostKey(hostId: string): Promise<void>;
  /** Recent scrollback of a durable (tmux) session. */
  sessionHistory(target: TargetRef, sessionKey: string, lines: number): Promise<string>;
  /** Scrolls a server terminal's tmux history so `position` lines sit below the screen (0 follows the output again); without one, just reports. */
  tmuxScroll(target: TargetRef, sessionKey: string, position: number | null): Promise<TmuxScroll>;
  /** The portable Windows .exe, which updates by downloading the new one. */
  isPortable(): Promise<boolean>;
  /** Scales the whole interface, like a browser's zoom. */
  setZoom(scale: number): Promise<void>;
  /** Shows a system notification; asks for permission the first time. */
  notify(title: string, body: string): Promise<void>;
  /** Drops a server's password from memory and the keychain. */
  forgetPassword(id: string): Promise<void>;

  openSession(
    paneId: string,
    target: TargetRef,
    sessionKey: string,
    cols: number,
    rows: number,
    container: ContainerRef | null,
    onEvent: (e: SessionEvent) => void,
    /** What a new session on a server shows first. */
    welcome?: WelcomeSpec,
  ): Promise<void>;
  writeSession(paneId: string, data: string): void;
  resizeSession(paneId: string, cols: number, rows: number): void;
  reconnectSession(paneId: string): void;
  /** Reconnect by itself after a drop, or wait for the user. Applies to open sessions. */
  setAutoReconnect(enabled: boolean): void;
  closeSession(paneId: string, target: TargetRef, sessionKey: string, endRemote: boolean): void;

  listDir(target: TargetRef, path: string): Promise<DirListing>;
  readFile(target: TargetRef, path: string): Promise<TextFile>;
  writeFile(target: TargetRef, path: string, content: string): Promise<void>;
  makeDir(target: TargetRef, path: string): Promise<void>;
  renamePath(target: TargetRef, from: string, to: string): Promise<void>;
  /** Packs `names` from `dir` into `dir/archive` (.tar.gz). */
  archivePaths(target: TargetRef, dir: string, names: string[], archive: string): Promise<void>;
  deletePath(target: TargetRef, path: string): Promise<void>;
  downloadPath(target: TargetRef, path: string, isDir: boolean, hooks: TransferHooks): Promise<string>;
  uploadPaths(target: TargetRef, paths: string[], remoteDir: string, overwrite: boolean, hooks: TransferHooks): Promise<void>;
  /** Copies `paths`, all in one folder on `from`, into `destDir` on the server `to`. */
  copyToServer(from: TargetRef, to: TargetRef, paths: string[], destDir: string, overwrite: boolean, hooks: TransferHooks): Promise<void>;
  /** Stops an upload or download; its call then fails with code "cancelled". */
  cancelTransfer(id: string): void;
  sessionCwd(target: TargetRef, sessionKey: string): Promise<string>;
  /** Where a relative path a program printed is on the server, searching from `cwd`. */
  findPath(target: TargetRef, path: string, cwd: string): Promise<string>;
  /** AI coding agents installed on the server, by command ("claude", "codex"…). */
  detectAgents(target: TargetRef): Promise<string[]>;
  /** What a terminal runs in the foreground and its folder; fails without tmux. */
  paneInfo(target: TargetRef, sessionKey: string): Promise<PaneInfo>;
  /** Windows 11's Snap Layouts for this window, for the custom title bar's maximize button. */
  showSnapLayouts(): void;
  /** What every SSHeesh terminal on the server runs in the foreground, by session key. */
  foreground(target: TargetRef): Promise<{ sessionKey: string; command: string }[]>;
  /** A read-only security check of the server. */
  securityCheckup(target: TargetRef): Promise<Checkup>;
  /** Runs a fix or cleanup without a terminal, unless it needs you. */
  runFix(target: TargetRef, command: string): Promise<RunOutcome>;
  /** Disks, Docker, the journal and safe cleanups. */
  diskReport(target: TargetRef): Promise<DiskReport>;
  /** The biggest files and folders directly inside `path`, largest first. */
  diskBiggest(target: TargetRef, path: string): Promise<DiskUsage[]>;
  pickFiles(): Promise<string[]>;
  onFileDrop(cb: (e: DropEvent) => void): () => void;

  fetchMetrics(target: TargetRef): Promise<Metrics>;
  listContainers(target: TargetRef): Promise<ContainerList>;
  containerAction(target: TargetRef, engine: Engine, id: string, action: "start" | "stop" | "restart"): Promise<void>;

  /** What listens on the server, likely websites first, with page titles when curl is there. */
  scanWebApps(target: TargetRef): Promise<WebAppScan>;
  /**
   * Starts tunnel `id` to a server, or reattaches to it when it already runs
   * the same way. Fails at once when the port here is taken; after that,
   * how it's doing arrives in `onState`.
   */
  startTunnel(id: string, hostId: string, forward: Forward, onState: (s: TunnelState) => void): Promise<void>;
  stopTunnel(id: string): void;
  /** Why a tunnel couldn't listen on `port` on this computer, or null when it's free. */
  localPortProblem(port: number): Promise<string | null>;
  /** A free port here for the server's `preferred` one: the same number when it's free. */
  pickLocalPort(preferred: number, avoid: number[]): Promise<number>;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriBackend(): Promise<Backend> {
  const { invoke, Channel } = await import("@tauri-apps/api/core");
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  const dialog = await import("@tauri-apps/plugin-dialog");
  const updater = await import("@tauri-apps/plugin-updater");
  const proc = await import("@tauri-apps/plugin-process");
  const app = await import("@tauri-apps/api/app");
  const opener = await import("@tauri-apps/plugin-opener");
  let pending: Awaited<ReturnType<typeof updater.check>> = null;

  // Rust errors arrive as { code, message }; normalise everything else.
  const call = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    try {
      return await invoke<T>(cmd, args);
    } catch (e) {
      throw toOpError(e);
    }
  };
  const fire = (cmd: string, args: Record<string, unknown>) => {
    invoke(cmd, args).catch(() => {});
  };
  const progress = (hooks: TransferHooks) => {
    const channel = new Channel<TransferProgress>();
    channel.onmessage = hooks.onProgress;
    return { transferId: hooks.id, onProgress: channel };
  };

  return {
    isDemo: false,
    appVersion: () => app.getVersion(),
    localIdentity: () => call("local_identity", {}),
    async checkUpdate() {
      try {
        pending = await updater.check();
      } catch (e) {
        const detail = toOpError(e).message;
        throw { code: "network", message: `Couldn't reach the update server. ${detail}` };
      }
      return pending ? { version: pending.version, notes: pending.body ?? "", date: pending.date ?? null } : null;
    },
    async installUpdate(onProgress) {
      if (!pending) throw { code: "failed", message: "Check for updates first." };
      let total = 0;
      let got = 0;
      await pending.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        if (e.event === "Progress") got += e.data.chunkLength;
        onProgress(total ? Math.min(1, got / total) : null);
      });
    },
    relaunch: () => proc.relaunch(),
    openExternal: (url) => opener.openUrl(url),
    revealFiles: (paths) => opener.revealItemInDir(paths),
    listHosts: () => call("list_hosts"),
    addHost: (input) => call("add_host", { input }),
    removeHost: (id) => call("remove_host", { id }),
    forgetPassword: (id) => call("forget_password", { id }),
    sessionHistory: (target, sessionKey, lines) => call("session_history", { target, sessionKey, lines }),
    tmuxScroll: (target, sessionKey, position) => call("tmux_scroll", { target, sessionKey, position }),
    checkHostKey: (hostId, changed) => call("check_host_key", { hostId, changed }),
    localAuth: () => call("local_auth", {}),
    testConnection: (input) => call("test_connection", { input }),
    trustTestHostKey: (input) => call("trust_test_host_key", { input }),
    trustHostKey: (hostId) => call("trust_host_key", { hostId }),
    isPortable: () => call("is_portable", {}),
    async installPortableUpdate(onProgress) {
      const channel = new Channel<TransferProgress>();
      channel.onmessage = ({ done, total }) => onProgress(total ? Math.min(1, done / total) : null);
      await call("install_portable_update", { onProgress: channel });
    },
    async writeClipboard(text) {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
    },
    async setZoom(scale) {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      await getCurrentWebview().setZoom(scale);
    },
    async notify(title, body) {
      const n = await import("@tauri-apps/plugin-notification");
      let ok = await n.isPermissionGranted();
      if (!ok) ok = (await n.requestPermission()) === "granted";
      if (ok) n.sendNotification({ title, body });
    },

    async openSession(paneId, target, sessionKey, cols, rows, container, onEvent, welcome) {
      const channel = new Channel<SessionEvent>();
      channel.onmessage = onEvent;
      const c = container && { engine: container.engine, id: container.id, run: container.run };
      await call("open_session", { paneId, target, sessionKey, cols, rows, container: c, welcome: welcome ?? null, onEvent: channel });
    },
    writeSession: (paneId, data) => fire("write_session", { paneId, data }),
    resizeSession: (paneId, cols, rows) => fire("resize_session", { paneId, cols, rows }),
    reconnectSession: (paneId) => fire("reconnect_session", { paneId }),
    setAutoReconnect: (enabled) => fire("set_auto_reconnect", { enabled }),
    closeSession: (paneId, target, sessionKey, endRemote) =>
      fire("close_session", { paneId, target, sessionKey, endRemote }),

    listDir: (target, path) => call("list_dir", { target, path }),
    readFile: (target, path) => call("read_file", { target, path }),
    writeFile: (target, path, content) => call("write_file", { target, path, content }),
    makeDir: (target, path) => call("make_dir", { target, path }),
    renamePath: (target, from, to) => call("rename_path", { target, from, to }),
    archivePaths: (target, dir, names, archive) => call("archive_paths", { target, dir, names, archive }),
    deletePath: (target, path) => call("delete_path", { target, path }),
    downloadPath: (target, path, isDir, hooks) => call("download_path", { target, path, isDir, ...progress(hooks) }),
    uploadPaths: (target, paths, remoteDir, overwrite, hooks) =>
      call("upload_paths", { target, paths, remoteDir, overwrite, ...progress(hooks) }),
    copyToServer: (from, to, paths, destDir, overwrite, hooks) => call("copy_to_server", { from, to, paths, destDir, overwrite, ...progress(hooks) }),
    cancelTransfer: (id) => fire("cancel_transfer", { id }),
    sessionCwd: (target, sessionKey) => call("session_cwd", { target, sessionKey }),
    findPath: (target, path, cwd) => call("find_path", { target, path, cwd }),
    detectAgents: (target) => call("detect_agents", { target }),
    paneInfo: (target, sessionKey) => call("pane_info", { target, sessionKey }),
    foreground: (target) => call("foreground", { target }),
    showSnapLayouts: () => fire("show_snap_layouts", {}),
    securityCheckup: (target) => call("security_checkup", { target }),
    runFix: (target, command) => call("run_fix", { target, command }),
    diskReport: (target) => call("disk_report", { target }),
    diskBiggest: (target, path) => call("disk_biggest", { target, path }),
    async pickFiles() {
      const picked = await dialog.open({ multiple: true, directory: false, title: "Upload files" });
      if (!picked) return [];
      return Array.isArray(picked) ? picked : [picked];
    },
    onFileDrop(cb) {
      let unlisten: (() => void) | undefined;
      let cancelled = false;
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const p = event.payload;
          const ratio = window.devicePixelRatio || 1;
          if (p.type === "leave") return cb({ type: "leave", paths: [], x: 0, y: 0 });
          const pos = { x: p.position.x / ratio, y: p.position.y / ratio };
          if (p.type === "drop") cb({ type: "drop", paths: p.paths, ...pos });
          else cb({ type: "over", paths: p.type === "enter" ? p.paths : [], ...pos });
        })
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },

    fetchMetrics: (target) => call("fetch_metrics", { target }),
    listContainers: (target) => call("list_containers", { target }),
    containerAction: (target, engine, id, action) => call("container_action", { target, engine, id, action }),

    scanWebApps: (target) => call("scan_web_apps", { target }),
    async startTunnel(id, hostId, forward, onState) {
      const channel = new Channel<TunnelState>();
      channel.onmessage = onState;
      await call("start_tunnel", { id, hostId, forward, onState: channel });
    },
    stopTunnel: (id) => fire("stop_tunnel", { id }),
    localPortProblem: (port) => call("local_port_problem", { port }),
    pickLocalPort: (preferred, avoid) => call("pick_local_port", { preferred, avoid }),
  };
}

export const backend: Backend = isTauri ? await tauriBackend() : (await import("./mock")).mockBackend;
