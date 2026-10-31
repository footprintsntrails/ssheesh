import { describe, expect, it } from "vitest";
import { coverSquare, firstThatFits, IMAGE_SIZES, isHostImage, lookOf, MAX_IMAGE_CHARS } from "./labels";

describe("server pictures", () => {
  it("crops to the middle square", () => {
    expect(coverSquare(200, 100)).toEqual({ x: 50, y: 0, side: 100 });
    expect(coverSquare(64, 256)).toEqual({ x: 0, y: 96, side: 64 });
    expect(coverSquare(48, 48)).toEqual({ x: 0, y: 0, side: 48 });
  });

  it("keeps the largest size that fits under the cap", () => {
    const tried: number[] = [];
    // A busy photo: 6 characters per pixel at 64 is too long, at 48 it fits.
    const busy = (size: number) => {
      tried.push(size);
      return "data:image/png;base64," + "A".repeat(size * size * 6);
    };
    const kept = firstThatFits(busy);
    expect(tried).toEqual([64, 48]);
    expect(kept!.length).toBeLessThanOrEqual(MAX_IMAGE_CHARS);
    expect(firstThatFits(() => "data:image/png;base64,AAAA")).toBe("data:image/png;base64,AAAA");
    expect(firstThatFits(() => "A".repeat(MAX_IMAGE_CHARS + 1))).toBeNull();
    expect(IMAGE_SIZES[0]).toBe(64);
  });

  it("only shows small PNG data URLs", () => {
    expect(isHostImage("data:image/png;base64,iVBORw0KGgo=")).toBe(true);
    expect(isHostImage(undefined)).toBe(false);
    expect(isHostImage("data:image/svg+xml;base64,PHN2Zz4=")).toBe(false);
    expect(isHostImage("https://example.com/icon.png")).toBe(false);
    expect(isHostImage("data:image/png;base64," + "A".repeat(MAX_IMAGE_CHARS))).toBe(false);
  });

  it("keeps your picture with your look, and old looks without one still work", () => {
    const yours = { icon: "game", color: "#7577e3", image: "data:image/png;base64,iVBORw0KGgo=" };
    expect(lookOf("cfg:prod-api", yours, { icon: "globe" })).toEqual({ ...yours, auto: false });
    expect(lookOf("cfg:prod-api", { icon: "game", color: "#7577e3" }, undefined).image).toBeUndefined();
  });
});
