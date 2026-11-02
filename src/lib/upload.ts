// Sending files to a folder: from this computer to a server over ssh (or a
// copy when the folder is on this computer), or from one server straight to
// another. Asks before replacing anything. What goes from this computer to a
// server loses its metadata first, unless that's turned off in Settings.

import { backend, type CopiedFile, type TransferHooks } from "./backend";
import { cleaningNote, watchCleaning } from "./cleaning";
import { dirCache } from "./dirCache";
import { targetName } from "./privacy";
import { useSettings } from "./settings";
import { useStore } from "./store";
import { track } from "./transfers";
import { targetKey, toOpError, type Cleaning, type TargetRef } from "./types";

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

/** The toast's line under a finished upload: what came out first, when anything did. */
function doneBody(dir: string, cleaning: Cleaning | undefined): string {
  return cleaning?.removed.length ? `${cleaningNote(cleaning)} · ${dir}` : dir;
}

/** Uploads `paths` into `dir`; true when it went through. */
/** Private by the look of it: keys, credentials and where they're kept. */
const PRIVATE = /(^|[\\/])(\.ssh|\.gnupg|\.aws|\.azure|\.kube|\.docker|\.password-store|\.env(\.[^\\/]*)?|\.netrc|\.npmrc|\.pgpass|id_(rsa|dsa|ecdsa|ed25519)[^\\/]*|[^\\/]*\.(pem|key|p12|pfx|kdbx)|credentials[^\\/]*|secrets?(\.[^\\/]*)?)($|[\\/])/i;

/** The most files named when asking; the rest are counted. */
const NAMED = 6;

/**
 * What to ask before files on the clipboard go to a server. Dragging or
 * picking files chooses them there and then; these were copied some time ago,
 * maybe by another program, and what's sent can't be taken back. So it says
 * what they are, in full, and where they're going.
 */
export function pasteQuestion(files: CopiedFile[], server: string, dir: string, show: (text: string) => string = (t) => t): { title: string; body: string; risky: boolean; confirmLabel: string } {
  const risky = files.filter((f) => PRIVATE.test(f.path));
  const named = files.slice(0, NAMED).map((f) => show(f.path) + (f.isDir ? " (folder, with all that's in it)" : ""));
  if (files.length > NAMED) named.push(`and ${files.length - NAMED} more`);
  const warning = risky.length ? `\n\nThis looks private: ${risky.slice(0, 3).map((f) => show(f.path)).join(", ")}${risky.length > 3 ? "…" : ""}. Once on the server, it can't be taken back.` : "";
  return {
    title: `Upload ${files.length === 1 ? "this" : `these ${files.length} items`} to ${server}?`,
    body: `${named.join("\n")}\n\nInto ${show(dir)}${warning}`,
    risky: risky.length > 0,
    confirmLabel: risky.length ? "Upload anyway" : "Upload",
  };
}

export async function uploadTo(target: TargetRef, dir: string, paths: string[]): Promise<boolean> {
  if (!paths.length) return false;
  const label = filesLabel(paths);
  const host = target.kind === "host";
  const clean = useSettings.getState().cleanUploads;
  let cleaning: Cleaning | undefined;
  const ok = await sending(label, host ? "upload" : "copy", (overwrite, hooks) =>
    backend.uploadPaths(target, paths, dir, overwrite, clean, watchCleaning(hooks, (c) => (cleaning = c))),
  );
  if (ok) useStore.getState().toast({ tone: "success", title: `${host ? "Uploaded" : "Copied"} ${label}`, body: doneBody(dir, cleaning) });
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
  const clean = useSettings.getState().cleanUploads;
  let cleaning: Cleaning | undefined;
  const ok = await sending(label, "copy", (overwrite, hooks) =>
    backend.copyToServer(from, to, paths, dir, overwrite, clean, watchCleaning(hooks, (c) => (cleaning = c))),
  );
  if (ok) {
    dirCache.changed(targetKey(to), dir);
    useStore.getState().toast({ tone: "success", title: `Copied ${label} to ${targetName(to)}`, body: doneBody(dir, cleaning) });
  }
  return ok;
}
