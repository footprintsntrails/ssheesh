// Sending files to a folder: from this computer to a server over ssh (or a
// copy when the folder is on this computer), or from one server straight to
// another. Asks before replacing anything.

import { backend, type TransferHooks } from "./backend";
import { dirCache } from "./dirCache";
import { targetName } from "./privacy";
import { useStore } from "./store";
import { track } from "./transfers";
import { targetKey, toOpError, type TargetRef } from "./types";

/** "notes.txt" or "3 items", for messages. */
export function filesLabel(paths: string[]): string {
  return paths.length === 1 ? (paths[0].split(/[\\/]/).pop() ?? paths[0]) : `${paths.length} items`;
}

/**
 * Runs a transfer that never replaces anything. When something's in the way
 * it asks once and runs it again, replacing: the far side checks the whole
 * batch before anything moves, so the question names all of it, and Cancel
 * leaves it all. True when it went through; otherwise it says why.
 */
async function sending(label: string, kind: "upload" | "copy", run: (overwrite: boolean, hooks: TransferHooks) => Promise<void>): Promise<boolean> {
  const { toast, confirm } = useStore.getState();
  const upload = kind === "upload";
  const send = (overwrite: boolean) => track(kind, label, (hooks) => run(overwrite, hooks));
  const failed = (e: unknown) => {
    const err = toOpError(e);
    if (err.code === "cancelled") toast({ tone: "info", title: upload ? "Upload stopped" : "Copy stopped", body: label });
    else toast({ tone: "error", title: upload ? "Upload failed" : "Copy failed", body: err.message });
    return false;
  };
  try {
    await send(false);
  } catch (e) {
    const err = toOpError(e);
    if (err.code !== "exists") return failed(e);
    const body = `${err.message} If you cancel, nothing is ${upload ? "uploaded" : "copied"}.`;
    if (!(await confirm({ title: "Replace existing files?", body, confirmLabel: "Replace" }))) return false;
    try {
      await send(true);
    } catch (e2) {
      return failed(e2);
    }
  }
  return true;
}

/** Uploads `paths` into `dir`; true when it went through. */
export async function uploadTo(target: TargetRef, dir: string, paths: string[]): Promise<boolean> {
  if (!paths.length) return false;
  const label = filesLabel(paths);
  const host = target.kind === "host";
  const ok = await sending(label, host ? "upload" : "copy", (overwrite, hooks) => backend.uploadPaths(target, paths, dir, overwrite, hooks));
  if (ok) useStore.getState().toast({ tone: "success", title: `${host ? "Uploaded" : "Copied"} ${label}`, body: dir });
  return ok;
}

/**
 * Copies `paths`, all in one folder on `from` (a server or this computer),
 * into `dir` on the server `to`; true when it went through. A Files tab open
 * on that folder shows them right away.
 */
export async function copyToServer(from: TargetRef, paths: string[], to: TargetRef, dir: string): Promise<boolean> {
  if (!paths.length) return false;
  const label = filesLabel(paths);
  const ok = await sending(label, "copy", (overwrite, hooks) => backend.copyToServer(from, to, paths, dir, overwrite, hooks));
  if (ok) {
    dirCache.changed(targetKey(to), dir);
    useStore.getState().toast({ tone: "success", title: `Copied ${label} to ${targetName(to)}`, body: dir });
  }
  return ok;
}
