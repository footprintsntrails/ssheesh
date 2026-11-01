// A click on a file path or web link in a terminal's
// output: links open in the browser, paths in the Files pane beside the
// terminal, with the file picked out and previewed.

import { backend } from "./backend";
import { joinPath, parentPath } from "./format";
import { resolvePath, type TextLink } from "./paths";
import { markHintSeen } from "./settings";
import { panesOn, screenOf, sideBySide, useStore } from "./store";
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
  const relative = !/^[/~]/.test(link.target);
  // A big folder on a slow link takes a moment: say it's on its way.
  const name = link.target.slice(link.target.lastIndexOf("/") + 1);
  let looking: string | null = null;
  const slow = window.setTimeout(() => (looking = st.toast({ tone: "info", title: `Looking for ${name}…` })), 500);
  const settled = () => {
    window.clearTimeout(slow);
    if (looking) st.dismissToast(looking);
  };
  try {
    await find(from, link, relative);
  } finally {
    settled();
  }
}

async function find(from: LinkSource, link: TextLink, relative: boolean) {
  const st = useStore.getState();
  let cwd = from.cwd ?? "~";
  if (relative && from.target.kind === "host" && from.durable) {
    cwd = await backend.sessionCwd(from.target, from.sessionKey).catch(() => cwd);
  }
  const path = resolvePath(link.target, cwd);
  if (path === "~" || path === "/") return showInFiles(from.paneId, from.target, path);
  let found = await look(from.target, path);
  // Not in the terminal's folder: a program (an agent, say) may have printed
  // it from its own folder, so the server looks around for it.
  if (found.kind !== "found" && relative && from.target.kind === "host") {
    const there = await backend.findPath(from.target, link.target, cwd).catch(() => null);
    if (there) found = await look(from.target, there);
  }
  if (found.kind === "missing") {
    st.toast({
      tone: "error",
      title: `${link.target} isn't there`,
      body: relative ? "Not in the terminal's folder, the folders your programs run in, or your home folder." : `Nothing by that name in ${found.dir}: it may have been moved or deleted.`,
    });
    return;
  }
  if (found.kind === "error") {
    st.toast({ tone: "error", title: `Can't open ${link.target}`, body: found.message });
    return;
  }
  const { dir, name, isDir } = found;
  if (isDir) showInFiles(from.paneId, from.target, joinPath(dir, name));
  else showInFiles(from.paneId, from.target, dir, name, link.line ? [link.line, link.endLine ?? link.line] : undefined);
}

type Looked = { kind: "found"; dir: string; name: string; isDir: boolean } | { kind: "missing"; dir: string } | { kind: "error"; message: string };

/** Where a path is: found (and whether it's a folder), not in its folder, or its folder can't be opened (and why). */
async function look(target: TargetRef, path: string): Promise<Looked> {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const parent = /^~\/[^/]+$/.test(path) ? "~" : parentPath(path);
  try {
    const listing = await backend.listDir(target, parent);
    const entry = listing.entries.find((e) => e.name === name);
    return entry ? { kind: "found", dir: listing.path, name, isDir: entry.kind === "dir" } : { kind: "missing", dir: listing.path };
  } catch (e) {
    const err = toOpError(e);
    // A folder that's gone means the file is too.
    return err.code === "notFound" ? { kind: "missing", dir: parent } : { kind: "error", message: err.message };
  }
}

/**
 * Shows a folder in a Files pane for the same server, beside the terminal
 * you clicked in (also when you work with one tab at a time): the Files tab
 * you have if there is one, else a new one. `reveal` picks out a file in it
 * and previews it, at `lines` (first and last) when given.
 */
export function showInFiles(from: string, target: TargetRef, path: string, reveal?: string, lines?: [number, number]) {
  const st = useStore.getState();
  const existing = panesOn(st, target).find((p) => p.kind === "files");
  let id: string;
  if (existing) {
    id = existing.id;
    st.setPane(id, { path, reveal, revealLines: lines });
  } else {
    id = st.openIn("files", target, { from, split: true, path });
    if (reveal) st.setPane(id, { reveal, revealLines: lines });
  }
  // Already beside it on screen: just look there. Otherwise put it there.
  const s = useStore.getState();
  const screen = screenOf(s, id);
  if (existing && screen === screenOf(s, from) && sideBySide(s, screen) && !s.hidden.includes(id)) s.focus(id);
  else placeBeside(id, "right", from);
}
