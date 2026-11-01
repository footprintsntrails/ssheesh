// What cleaning took out of uploaded files, in one line for the transfer
// card and the toast after it.

import type { TransferHooks } from "./backend";
import type { Cleaning } from "./types";

/** "Removed location, camera", "No metadata to remove", or "" when there's nothing to say yet. */
export function cleaningNote(c: Cleaning): string {
  const files = c.cleaned + c.clean + c.asIs;
  if (c.removed.length) {
    const what = `Removed ${c.removed.join(", ")}`;
    return files > 1 ? `${what} · ${c.cleaned} of ${files} files` : what;
  }
  if (!files) return c.folders ? "Files inside folders go as they are" : "";
  if (!c.asIs) return "No metadata to remove";
  return files === 1 ? "As is: SSHeesh doesn't clean this kind of file" : "Nothing removed";
}

/** `hooks` that also hand what cleaning did so far to `seen`. */
export function watchCleaning(hooks: TransferHooks, seen: (c: Cleaning) => void): TransferHooks {
  return {
    ...hooks,
    onProgress: (p) => {
      if (p.cleaning) seen(p.cleaning);
      hooks.onProgress(p);
    },
  };
}
