import { describe, expect, it } from "vitest";
import { cleaningNote, watchCleaning } from "./cleaning";
import type { Cleaning, TransferProgress } from "./types";

const none: Cleaning = { cleaned: 0, clean: 0, asIs: 0, folders: 0, removed: [] };

describe("cleaningNote", () => {
  it("names what came out of a file", () => {
    expect(cleaningNote({ ...none, cleaned: 1, removed: ["location", "camera", "dates"] })).toBe("Removed location, camera, dates");
  });

  it("says how many of a batch had something in them", () => {
    expect(cleaningNote({ ...none, cleaned: 2, clean: 1, asIs: 2, removed: ["author"] })).toBe("Removed author · 2 of 5 files");
  });

  it("tells a clean file from one it doesn't clean", () => {
    expect(cleaningNote({ ...none, clean: 1 })).toBe("No metadata to remove");
    expect(cleaningNote({ ...none, asIs: 1 })).toBe("As is: SSHeesh doesn't clean this kind of file");
    expect(cleaningNote({ ...none, clean: 1, asIs: 2 })).toBe("Nothing removed");
  });

  it("says a folder's files go as they are, and nothing before it knows", () => {
    expect(cleaningNote({ ...none, folders: 1 })).toBe("Files inside folders go as they are");
    expect(cleaningNote(none)).toBe("");
  });
});

describe("watchCleaning", () => {
  it("passes progress on and keeps what cleaning did", () => {
    const heard: TransferProgress[] = [];
    let seen: Cleaning | undefined;
    const hooks = watchCleaning({ id: "x1", onProgress: (p) => heard.push(p) }, (c) => (seen = c));
    hooks.onProgress({ done: 1, total: 2, waiting: false });
    expect(seen).toBeUndefined();
    const cleaning = { ...none, cleaned: 1, removed: ["camera"] };
    hooks.onProgress({ done: 2, total: 2, waiting: false, cleaning });
    expect(seen).toEqual(cleaning);
    expect(heard).toHaveLength(2);
    expect(hooks.id).toBe("x1");
  });
});
