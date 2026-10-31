// Sending files from this computer to a folder: on a server over ssh, or a
// copy when the folder is on this computer. Asks before replacing anything.

import { backend } from "./backend";
import { useStore } from "./store";
import { track } from "./transfers";
import { toOpError, type TargetRef } from "./types";

/** "notes.txt" or "3 items", for messages. */
export function filesLabel(paths: string[]): string {
  return paths.length === 1 ? (paths[0].split(/[\\/]/).pop() ?? paths[0]) : `${paths.length} items`;
}

/** Uploads `paths` into `dir`; true when it went through. */
export async function uploadTo(target: TargetRef, dir: string, paths: string[]): Promise<boolean> {
  if (!paths.length) return false;
  const { toast, confirm } = useStore.getState();
  const label = filesLabel(paths);
  const host = target.kind === "host";
  const verb = host ? "Uploaded" : "Copied";
  const send = (overwrite: boolean) => track(host ? "upload" : "copy", label, (hooks) => backend.uploadPaths(target, paths, dir, overwrite, hooks));
  const failed = (e: unknown) => {
    const err = toOpError(e);
    if (err.code === "cancelled") toast({ tone: "info", title: host ? "Upload stopped" : "Copy stopped", body: label });
    else toast({ tone: "error", title: host ? "Upload failed" : "Copy failed", body: err.message });
    return false;
  };
  try {
    await send(false);
  } catch (e) {
    const err = toOpError(e);
    if (err.code !== "exists") return failed(e);
    // The server checks the whole batch before anything moves, so this
    // names everything that would be replaced, and Cancel leaves it all.
    const body = `${err.message} If you cancel, nothing is ${host ? "uploaded" : "copied"}.`;
    if (!(await confirm({ title: "Replace existing files?", body, confirmLabel: "Replace" }))) return false;
    try {
      await send(true);
    } catch (e2) {
      return failed(e2);
    }
  }
  toast({ tone: "success", title: `${verb} ${label}`, body: dir });
  return true;
}
