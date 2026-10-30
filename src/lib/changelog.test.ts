import { describe, expect, it } from "vitest";
import { compareVersions, RELEASES, releasesSince } from "./changelog";

describe("changelog", () => {
  it("reads every release from CHANGELOG.md, newest first", () => {
    expect(RELEASES.length).toBeGreaterThan(2);
    expect(compareVersions(RELEASES[0].version, RELEASES[1].version)).toBe(1);
    expect(RELEASES.at(-1)?.version).toBe("0.1.0");
  });

  it("compares versions by number, not text", () => {
    expect(compareVersions("0.3.10", "0.3.9")).toBe(1);
    expect(compareVersions("0.3.2", "0.3.2")).toBe(0);
  });

  it("picks what you skipped", () => {
    const skipped = releasesSince("0.1.0", "0.3.0").map((r) => r.version);
    expect(skipped).toEqual(["0.3.0"]);
    expect(releasesSince("0.3.0", "0.3.0")).toEqual([]);
  });
});
