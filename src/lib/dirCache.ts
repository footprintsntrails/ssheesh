// Folders seen lately, per machine. Going back, up, or into a folder the
// pointer rested on shows its last listing at once while a fresh one loads,
// and asking for a folder that's already on its way shares that answer.

import { backend } from "./backend";
import type { DirListing, FileEntry, TargetRef } from "./types";
import { targetKey } from "./types";

export interface Cached {
  listing: DirListing;
  /** When it arrived, in ms. */
  at: number;
}

export interface DirCache {
  /** The last listing of a folder, under the path asked or the one it resolved to. */
  get(tk: string, path: string): Cached | undefined;
  /** Lists a folder and keeps the answer. Asking again while it's on its way shares it. */
  load(target: TargetRef, path: string): Promise<DirListing>;
  /** Lists a folder ahead of time, one at a time, unless it's known or on its way. */
  prefetch(target: TargetRef, path: string): void;
  /** Forgets a folder and everything in it (deleted, renamed). */
  forget(tk: string, path: string): void;
  /** The app changed something in `dir`: forget its listing and tell whoever shows it. */
  changed(tk: string, dir: string): void;
  /** Hears about `changed`. Returns a function that stops listening. */
  watch(fn: (tk: string, dir: string) => void): () => void;
}

const MAX = 300;

const key = (tk: string, path: string) => `${tk}\n${path}`;

const sameEntry = (a: FileEntry, b: FileEntry) =>
  a.name === b.name && a.kind === b.kind && a.isLink === b.isLink && a.size === b.size && a.modified === b.modified && a.mode === b.mode;

/** Nothing changed: the old listing stays, so nothing that shows it has to sort or draw again. */
export function sameListing(a: DirListing, b: DirListing): boolean {
  return a.path === b.path && a.entries.length === b.entries.length && a.entries.every((e, i) => sameEntry(e, b.entries[i]));
}

/** Whether `path` is `dir` or inside it. */
const within = (path: string, dir: string) => path === dir || path.startsWith(dir.endsWith("/") ? dir : `${dir}/`);

export function createDirCache(list: (target: TargetRef, path: string) => Promise<DirListing>, now: () => number = Date.now): DirCache {
  const listings = new Map<string, Cached>();
  const inflight = new Map<string, Promise<DirListing>>();
  const watchers = new Set<(tk: string, dir: string) => void>();
  let prefetching = false;

  const remember = (tk: string, asked: string, l: DirListing): DirListing => {
    const old = listings.get(key(tk, l.path))?.listing;
    const kept = old && sameListing(old, l) ? old : l;
    for (const p of new Set([asked, l.path])) {
      const k = key(tk, p);
      listings.delete(k);
      listings.set(k, { listing: kept, at: now() });
    }
    while (listings.size > MAX) listings.delete(listings.keys().next().value as string);
    return kept;
  };

  /** Drops cached folders that match, and every request on its way for that machine: it may predate the change. */
  const drop = (tk: string, match: (path: string) => boolean) => {
    const prefix = `${tk}\n`;
    for (const [k, c] of listings) {
      if (k.startsWith(prefix) && (match(k.slice(prefix.length)) || match(c.listing.path))) listings.delete(k);
    }
    for (const k of inflight.keys()) if (k.startsWith(prefix)) inflight.delete(k);
  };

  const cache: DirCache = {
    get: (tk, path) => listings.get(key(tk, path)),

    load(target, path) {
      const tk = targetKey(target);
      const k = key(tk, path);
      const pending = inflight.get(k);
      if (pending) return pending;
      const p = list(target, path).then(
        (l) => {
          // Forgotten while it was on its way: hand it over, but don't keep it.
          if (inflight.get(k) !== p) return l;
          inflight.delete(k);
          return remember(tk, path, l);
        },
        (e) => {
          if (inflight.get(k) === p) inflight.delete(k);
          throw e;
        },
      );
      inflight.set(k, p);
      return p;
    },

    prefetch(target, path) {
      const k = key(targetKey(target), path);
      if (prefetching || listings.has(k) || inflight.has(k)) return;
      prefetching = true;
      cache
        .load(target, path)
        .catch(() => {})
        .finally(() => (prefetching = false));
    },

    forget: (tk, path) => drop(tk, (p) => within(p, path)),

    changed(tk, dir) {
      drop(tk, (p) => p === dir);
      for (const fn of [...watchers]) fn(tk, dir);
    },

    watch(fn) {
      watchers.add(fn);
      return () => {
        watchers.delete(fn);
      };
    },
  };
  return cache;
}

export const dirCache = createDirCache((target, path) => backend.listDir(target, path));
