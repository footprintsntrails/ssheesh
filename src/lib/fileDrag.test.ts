import { describe, expect, it } from "vitest";
import { dropFor, type Dragged } from "./fileDrag";
import type { TargetRef } from "./types";

const web: TargetRef = { kind: "host", hostId: "web" };
const db: TargetRef = { kind: "host", hostId: "db" };
const here: TargetRef = { kind: "local" };
const drag = (from: TargetRef, dir: string, ...names: string[]): Dragged => ({ from, dir, paths: names.map((n) => `${dir}/${n}`) });

describe("where dragged files may land", () => {
  it("moves into a folder of the same list", () => {
    const d = drag(web, "/srv", "a.txt", "b.txt");
    expect(dropFor(d, { target: web, paneDir: "/srv", folder: "/srv/old" })).toEqual({ kind: "move", dir: "/srv/old" });
    // The folder above, by its "..".
    expect(dropFor(d, { target: web, paneDir: "/srv", folder: "/" })).toEqual({ kind: "move", dir: "/" });
    // On this computer too.
    expect(dropFor(drag(here, "/home/me", "a"), { target: here, paneDir: "/home/me", folder: "/home/me/docs" })).toEqual({ kind: "move", dir: "/home/me/docs" });
  });

  it("does nothing where the files already are", () => {
    const d = drag(web, "/srv", "a.txt", "logs");
    // Let go over the list itself, or over a file.
    expect(dropFor(d, { target: web, paneDir: "/srv", folder: undefined })).toBeNull();
    // Another tab showing the same folder.
    expect(dropFor(d, { target: web, paneDir: "/srv/", folder: undefined })).toBeNull();
  });

  it("never moves a folder into itself", () => {
    const d = drag(web, "/srv", "logs", "a.txt");
    expect(dropFor(d, { target: web, paneDir: "/srv", folder: "/srv/logs" })).toBeNull();
    expect(dropFor(d, { target: web, paneDir: "/srv/logs/2026", folder: undefined })).toBeNull();
    // A name that only starts the same is another folder.
    expect(dropFor(d, { target: web, paneDir: "/srv", folder: "/srv/logs-old" })).toEqual({ kind: "move", dir: "/srv/logs-old" });
  });

  it("moves to the same server's other tab, into the folder open there", () => {
    expect(dropFor(drag(web, "/srv", "a.txt"), { target: web, paneDir: "/var/www", folder: undefined })).toEqual({ kind: "move", dir: "/var/www" });
  });

  it("copies to another server, into the folder under the pointer or the one open", () => {
    const d = drag(web, "/srv", "a.txt");
    expect(dropFor(d, { target: db, paneDir: "/backup", folder: undefined })).toEqual({ kind: "copy", dir: "/backup" });
    expect(dropFor(d, { target: db, paneDir: "/backup", folder: "/backup/web" })).toEqual({ kind: "copy", dir: "/backup/web" });
    // This computer's Files takes no copy that way: uploads and downloads have their own.
    expect(dropFor(d, { target: here, paneDir: "/home/me", folder: undefined })).toBeNull();
  });

  it("downloads from a server when let go outside the window", () => {
    expect(dropFor(drag(web, "/srv", "a.txt"), "outside")).toEqual({ kind: "download" });
    // This computer's files are here already.
    expect(dropFor(drag(here, "/home/me", "a"), "outside")).toBeNull();
    expect(dropFor(drag(web, "/srv", "a.txt"), null)).toBeNull();
  });
});
