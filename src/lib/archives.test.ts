import { describe, expect, it } from "vitest";
import { archiveKind, canUnpack } from "./archives";

describe("archives", () => {
  it("tells an archive by its ending", () => {
    expect(archiveKind("github-agency-vps.zip")).toBe("zip");
    expect(archiveKind("Site.ZIP")).toBe("zip");
    expect(archiveKind("backup.tar.gz")).toBe("tar");
    expect(archiveKind("a.tgz")).toBe("tar");
    expect(archiveKind("access.log.gz")).toBe("squeezed");
    expect(archiveKind("x.7z")).toBe("7z");
    expect(archiveKind("x.rar")).toBe("rar");
    expect(archiveKind(".zip")).toBeNull();
    expect(archiveKind("notes.txt")).toBeNull();
    expect(archiveKind("package.deb")).toBeNull();
  });

  it("offers on Windows only what its tar reads", () => {
    expect(canUnpack("a.zip", true)).toBe(true);
    expect(canUnpack("a.tar.gz", true)).toBe(true);
    expect(canUnpack("a.7z", true)).toBe(false);
    expect(canUnpack("a.log.gz", true)).toBe(false);
    expect(canUnpack("a.7z", false)).toBe(true);
    expect(canUnpack("readme.md", false)).toBe(false);
  });
});
