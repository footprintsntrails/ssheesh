// Ctrl+click (⌘-click on a Mac) on a file path or web link in a terminal's
// output: links open in the browser, paths in the Files pane beside the
// terminal, with the file picked out and previewed.

import { backend } from "./backend";
import { joinPath, parentPath } from "./format";
import { resolvePath, type TextLink } from "./paths";
import { markHintSeen } from "./settings";
import { isSplit, panesOn, useStore } from "./store";
import { placeBeside } from "./tabs";
import { toOpError, type TargetRef } from "./types";

export interface LinkSource {
  paneId: string;
  target: TargetRef;
  sessionKey: string;
  /** The terminal runs in tmux, which knows its folder. */
  durable: boolean;
  /** The folder the shell last reported (OSC 7), if it did. */
  cwd: string | null;
}

export async function openTerminalLink(from: LinkSource, link: TextLink) {
  const st = useStore.getState();
  // You found it: the tip about it never needs to show.
  markHintSeen("peek");
  if (link.kind === "url") {
    backend.openExternal(link.target).catch((e) => st.toast({ tone: "error", title: "Couldn't open the link", body: toOpError(e).message }));
    return;
  }
  let cwd = from.cwd ?? "~";
  if (!/^[/~]/.test(link.target) && from.target.kind === "host" && from.durable) {
    cwd = await backend.sessionCwd(from.target, from.sessionKey).catch(() => cwd);
  }
  const path = resolvePath(link.target, cwd);
  if (path === "~" || path === "/") return showInFiles(from.paneId, from.target, path);
  const name = path.slice(path.lastIndexOf("/") + 1);
  const parent = /^~\/[^/]+$/.test(path) ? "~" : parentPath(path);
  let listing;
  try {
    listing = await backend.listDir(from.target, parent);
  } catch (e) {
    st.toast({ tone: "error", title: `Can't open ${link.target}`, body: toOpError(e).message });
    return;
  }
  const entry = listing.entries.find((e) => e.name === name);
  if (!entry) {
    st.toast({ tone: "info", title: `There's no ${name} there`, body: `Looked in ${listing.path}.` });
    return;
  }
  if (entry.kind === "dir") showInFiles(from.paneId, from.target, joinPath(listing.path, name));
  else showInFiles(from.paneId, from.target, listing.path, name);
}

/**
 * Shows a folder in a Files pane for the same server, beside the terminal
 * you clicked in (also when you work with one tab at a time): the Files tab
 * you have if there is one, else a new one. `reveal` picks out a file in it
 * and previews it.
 */
export function showInFiles(from: string, target: TargetRef, path: string, reveal?: string) {
  const st = useStore.getState();
  const existing = panesOn(st, target).find((p) => p.kind === "files");
  let id: string;
  if (existing) {
    id = existing.id;
    st.setPane(id, { path, reveal });
  } else {
    id = st.openIn("files", target, { from, split: true, path });
    if (reveal) st.setPane(id, { reveal });
  }
  // Already beside it on screen: just look there. Otherwise put it there.
  const s = useStore.getState();
  if (existing && isSplit(s, target) && !s.hidden.includes(id)) s.focus(id);
  else placeBeside(id, "right", from);
}
