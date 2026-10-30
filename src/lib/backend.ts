// The one place the UI talks to the outside world. Inside the app this is
// Tauri (Rust); in a plain browser it's a demo backend with sample data, so
// the interface can be designed and screenshotted without servers.

import type { ContainerList, ContainerRef, DirListing, Engine, Host, Metrics, NewHost, SessionEvent, TargetRef, TextFile } from "./types";
import { toOpError } from "./types";

export interface DropEvent {
  type: "over" | "drop" | "leave";
  paths: string[];
  /** CSS pixels, relative to the window. */
  x: number;
  y: number;
}

export interface UpdateInfo {
  version: string;
  notes: string;
  date: string | null;
}

export interface Backend {
  readonly isDemo: boolean;
  appVersion(): Promise<string>;
  /** Asks the release server; null when already up to date. */
  checkUpdate(): Promise<UpdateInfo | null>;
  /** Downloads and installs the update found by checkUpdate. Progress is 0..1, or null if unknown. */
  installUpdate(onProgress: (fraction: number | null) => void): Promise<void>;
  relaunch(): Promise<void>;
  /** Opens a web link in the system browser. */
  openExternal(url: string): Promise<void>;

  listHosts(): Promise<Host[]>;
  addHost(input: NewHost): Promise<Host>;
  removeHost(id: string): Promise<void>;

  openSession(
    paneId: string,
    target: TargetRef,
    sessionKey: string,
    cols: number,
    rows: number,
    container: ContainerRef | null,
    onEvent: (e: SessionEvent) => void,
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
  deletePath(target: TargetRef, path: string): Promise<void>;
  downloadPath(target: TargetRef, path: string, isDir: boolean): Promise<string>;
  uploadPaths(target: TargetRef, paths: string[], remoteDir: string, overwrite: boolean): Promise<void>;
  sessionCwd(target: TargetRef, sessionKey: string): Promise<string>;
  pickFiles(): Promise<string[]>;
  onFileDrop(cb: (e: DropEvent) => void): () => void;

  fetchMetrics(target: TargetRef): Promise<Metrics>;
  listContainers(target: TargetRef): Promise<ContainerList>;
  containerAction(target: TargetRef, engine: Engine, id: string, action: "start" | "stop" | "restart"): Promise<void>;
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

  return {
    isDemo: false,
    appVersion: () => app.getVersion(),
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
    listHosts: () => call("list_hosts"),
    addHost: (input) => call("add_host", { input }),
    removeHost: (id) => call("remove_host", { id }),

    async openSession(paneId, target, sessionKey, cols, rows, container, onEvent) {
      const channel = new Channel<SessionEvent>();
      channel.onmessage = onEvent;
      const c = container && { engine: container.engine, id: container.id, run: container.run };
      await call("open_session", { paneId, target, sessionKey, cols, rows, container: c, onEvent: channel });
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
    deletePath: (target, path) => call("delete_path", { target, path }),
    downloadPath: (target, path, isDir) => call("download_path", { target, path, isDir }),
    uploadPaths: (target, paths, remoteDir, overwrite) =>
      call("upload_paths", { target, paths, remoteDir, overwrite }),
    sessionCwd: (target, sessionKey) => call("session_cwd", { target, sessionKey }),
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
  };
}

export const backend: Backend = isTauri ? await tauriBackend() : (await import("./mock")).mockBackend;
