// Sending files from this computer to a folder: on a server over ssh, or a
// copy when the folder is on this computer. Asks before replacing anything.

import { backend } from "./backend";
import { useStore } from "./store";
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
  const verb = target.kind === "host" ? "Uploaded" : "Copied";
  try {
    await backend.uploadPaths(target, paths, dir, false);
  } catch (e) {
    const err = toOpError(e);
    if (err.code !== "exists") {
      toast({ tone: "error", title: target.kind === "host" ? "Upload failed" : "Copy failed", body: err.message });
      return false;
    }
    if (!(await confirm({ title: "Replace existing files?", body: err.message, confirmLabel: "Replace" }))) return false;
    try {
      await backend.uploadPaths(target, paths, dir, true);
    } catch (e2) {
      toast({ tone: "error", title: target.kind === "host" ? "Upload failed" : "Copy failed", body: toOpError(e2).message });
      return false;
    }
  }
  toast({ tone: "success", title: `${verb} ${label}`, body: dir });
  return true;
}
