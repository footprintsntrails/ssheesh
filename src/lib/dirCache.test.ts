import { describe, expect, it } from "vitest";
import { createDirCache, sameListing } from "./dirCache";
import type { DirListing, FileEntry, TargetRef } from "./types";

const host: TargetRef = { kind: "host", hostId: "web" };
const TK = "host:web";

const file = (name: string, size = 1): FileEntry => ({ name, kind: "file", isLink: false, size, modified: 1700000000, mode: "-rw-r--r--" });

/** A fake server: each call is answered when the test says so. */
function fakeServer(home = "/home/deploy") {
  const calls: { path: string; resolve: (l: DirListing) => void; reject: (e: unknown) => void }[] = [];
  const files = new Map<string, FileEntry[]>();
  const list = (_t: TargetRef, path: string) =>
    new Promise<DirListing>((resolve, reject) => calls.push({ path, resolve, reject }));
  const answer = (i = calls.length - 1) => {
    const c = calls[i];
    const path = c.path === "~" ? home : c.path;
    c.resolve({ path, entries: files.get(path) ?? [] });
  };
  return { calls, files, list, answer };
}

describe("folder cache", () => {
  it("shares a listing that's already on its way", async () => {
    const srv = fakeServer();
    const cache = createDirCache(srv.list);
    const a = cache.load(host, "/var/log");
    const b = cache.load(host, "/var/log");
    expect(srv.calls).toHaveLength(1);
    srv.answer();
    expect(await a).toBe(await b);
    expect(cache.get(TK, "/var/log")?.listing.path).toBe("/var/log");
    // Once it's here, a new load asks again (the caller shows the cached one meanwhile).
    void cache.load(host, "/var/log");
    expect(srv.calls).toHaveLength(2);
  });

  it("keeps a listing under the path asked and the one it resolved to", async () => {
    const srv = fakeServer();
    const cache = createDirCache(srv.list);
    const p = cache.load(host, "~");
    srv.answer();
    await p;
    expect(cache.get(TK, "~")?.listing.path).toBe("/home/deploy");
    expect(cache.get(TK, "/home/deploy")).toBeDefined();
    expect(cache.get("host:other", "~")).toBeUndefined();
  });

  it("keeps the old listing object when nothing changed", async () => {
    const srv = fakeServer();
    srv.files.set("/srv", [file("a"), file("b")]);
    const cache = createDirCache(srv.list);
    let p = cache.load(host, "/srv");
    srv.answer();
    const first = await p;
    p = cache.load(host, "/srv");
    srv.answer();
    expect(await p).toBe(first);
    srv.files.set("/srv", [file("a"), file("b", 2)]);
    p = cache.load(host, "/srv");
    srv.answer();
    expect(await p).not.toBe(first);
  });

  it("prefetches one folder at a time, and only unknown ones", async () => {
    const srv = fakeServer();
    const cache = createDirCache(srv.list);
    cache.prefetch(host, "/etc");
    cache.prefetch(host, "/opt");
    expect(srv.calls.map((c) => c.path)).toEqual(["/etc"]);
    // Opening the folder being prefetched waits for that same answer.
    const opened = cache.load(host, "/etc");
    expect(srv.calls).toHaveLength(1);
    srv.answer();
    expect((await opened).path).toBe("/etc");
    await Promise.resolve();
    cache.prefetch(host, "/etc");
    expect(srv.calls).toHaveLength(1);
    cache.prefetch(host, "/opt");
    expect(srv.calls.map((c) => c.path)).toEqual(["/etc", "/opt"]);
  });

  it("doesn't keep failures", async () => {
    const srv = fakeServer();
    const cache = createDirCache(srv.list);
    const p = cache.load(host, "/root");
    srv.calls[0].reject(new Error("denied"));
    await expect(p).rejects.toThrow("denied");
    expect(cache.get(TK, "/root")).toBeUndefined();
    void cache.load(host, "/root");
    expect(srv.calls).toHaveLength(2);
  });

  it("forgets a folder and everything in it", async () => {
    const srv = fakeServer();
    const cache = createDirCache(srv.list);
    for (const path of ["/srv/app", "/srv/app/logs", "/srv/apple", "/srv"]) {
      const p = cache.load(host, path);
      srv.answer();
      await p;
    }
    cache.forget(TK, "/srv/app");
    expect(cache.get(TK, "/srv/app")).toBeUndefined();
    expect(cache.get(TK, "/srv/app/logs")).toBeUndefined();
    expect(cache.get(TK, "/srv/apple")).toBeDefined();
    expect(cache.get(TK, "/srv")).toBeDefined();
  });

  it("tells watchers about a change, and drops what was on its way", async () => {
    const srv = fakeServer();
    srv.files.set("/home/deploy", [file("old.txt")]);
    const cache = createDirCache(srv.list);
    let p = cache.load(host, "~");
    srv.answer();
    await p;
    const heard: string[] = [];
    const stop = cache.watch((tk, dir) => heard.push(`${tk} ${dir}`));
    // A listing that left before the change mustn't come back as the new one.
    const before = cache.load(host, "/home/deploy");
    cache.changed(TK, "/home/deploy");
    expect(heard).toEqual(["host:web /home/deploy"]);
    expect(cache.get(TK, "~")).toBeUndefined();
    expect(cache.get(TK, "/home/deploy")).toBeUndefined();
    srv.files.set("/home/deploy", [file("old.txt"), file("new.txt")]);
    p = cache.load(host, "/home/deploy");
    expect(srv.calls).toHaveLength(3);
    srv.answer(1);
    await before;
    expect(cache.get(TK, "/home/deploy")).toBeUndefined();
    srv.answer(2);
    expect((await p).entries.map((e) => e.name)).toEqual(["old.txt", "new.txt"]);
    stop();
    cache.changed(TK, "/tmp");
    expect(heard).toHaveLength(1);
  });

  it("evicts the folders seen longest ago", async () => {
    const srv = fakeServer();
    const cache = createDirCache(srv.list);
    for (let i = 0; i < 301; i++) {
      const p = cache.load(host, `/d${i}`);
      srv.answer();
      await p;
    }
    expect(cache.get(TK, "/d0")).toBeUndefined();
    expect(cache.get(TK, "/d300")).toBeDefined();
  });

  it("compares listings field by field", () => {
    const a = { path: "/x", entries: [file("a")] };
    expect(sameListing(a, { path: "/x", entries: [file("a")] })).toBe(true);
    expect(sameListing(a, { path: "/y", entries: [file("a")] })).toBe(false);
    expect(sameListing(a, { path: "/x", entries: [{ ...file("a"), mode: "-rwxr-xr-x" }] })).toBe(false);
  });
});
