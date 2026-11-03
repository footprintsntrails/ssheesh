import { describe, expect, it } from "vitest";
import { mediaKind } from "./media";

describe("mediaKind", () => {
  it("knows pictures and videos by their names, whatever the case", () => {
    for (const name of ["a.png", "a.JPG", "a.jpeg", "a.gif", "a.webp", "a.svg", "a.heic", "a.tiff"]) expect(mediaKind(name)).toBe("image");
    for (const name of ["a.mp4", "a.MOV", "a.webm", "a.mkv", "my clip.m4v"]) expect(mediaKind(name)).toBe("video");
  });

  it("leaves everything else alone", () => {
    // .ts is TypeScript far more often than a video; a playlist would fetch from the network, around Tor.
    for (const name of ["a.ts", "a.m3u8", "a.txt", "png", "a.png.txt", ".mp4x"]) expect(mediaKind(name)).toBeNull();
  });
});
