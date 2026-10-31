import { describe, expect, it } from "vitest";
import { contrast, hsvToRgb, isLightColor, over, parseColor, parseHex, rgbToHsv, toHex } from "./color";

describe("parseHex", () => {
  it("reads six and eight digits, with or without #", () => {
    expect(parseHex("#5b9dff")).toEqual({ r: 91, g: 157, b: 255, a: 1 });
    expect(parseHex("5B9DFF")).toEqual({ r: 91, g: 157, b: 255, a: 1 });
    expect(parseHex(" #00000080 ")?.a).toBeCloseTo(128 / 255);
  });

  it("refuses anything else", () => {
    for (const bad of ["", "#", "#fff", "#ffff", "#12345", "#1234567", "#123456789", "#gggggg", "red", "rgb(0,0,0)"]) {
      expect(parseHex(bad), bad).toBeNull();
    }
  });
});

describe("parseColor", () => {
  it("reads CSS rgb() as index.css writes it", () => {
    expect(parseColor("rgb(26 28 33 / 0.62)")).toEqual({ r: 26, g: 28, b: 33, a: 0.62 });
    expect(parseColor("rgba(255, 255, 255, 0.5)")).toEqual({ r: 255, g: 255, b: 255, a: 0.5 });
    expect(parseColor("rgb(1 2 3 / 50%)")?.a).toBe(0.5);
    expect(parseColor("rgb(1 2 3)")?.a).toBe(1);
  });

  it("refuses out-of-range values", () => {
    expect(parseColor("rgb(256 0 0)")).toBeNull();
    expect(parseColor("rgb(0 0 0 / 2)")).toBeNull();
  });
});

describe("toHex", () => {
  it("writes alpha only when see-through", () => {
    expect(toHex({ r: 91, g: 157, b: 255, a: 1 })).toBe("#5b9dff");
    expect(toHex({ r: 0, g: 0, b: 0, a: 0.5 })).toBe("#00000080");
    expect(toHex({ r: 300, g: -4, b: 12.4, a: 1 })).toBe("#ff000c");
  });

  it("round-trips", () => {
    for (const hex of ["#0f1013", "#eceef1", "#c792eacc", "#ffffff00"]) expect(toHex(parseHex(hex)!)).toBe(hex);
  });
});

describe("contrast", () => {
  it("matches WCAG's numbers", () => {
    expect(contrast(parseHex("#000000")!, parseHex("#ffffff")!)).toBeCloseTo(21);
    expect(contrast(parseHex("#777777")!, parseHex("#ffffff")!)).toBeCloseTo(4.48, 2);
    expect(contrast(parseHex("#ffffff")!, parseHex("#ffffff")!)).toBe(1);
  });

  it("doesn't care which color is which", () => {
    const [a, b] = [parseHex("#5b9dff")!, parseHex("#0f1013")!];
    expect(contrast(a, b)).toBe(contrast(b, a));
  });
});

describe("over", () => {
  it("lays a see-through color over a solid one", () => {
    const half = over({ r: 255, g: 255, b: 255, a: 0.5 }, { r: 0, g: 0, b: 0, a: 1 });
    expect(toHex(half)).toBe("#808080");
    expect(over({ r: 9, g: 9, b: 9, a: 1 }, { r: 0, g: 0, b: 0, a: 1 })).toEqual({ r: 9, g: 9, b: 9, a: 1 });
  });
});

describe("isLightColor", () => {
  it("splits light from dark", () => {
    expect(isLightColor(parseHex("#eceef2")!)).toBe(true);
    expect(isLightColor(parseHex("#0f1013")!)).toBe(false);
  });
});

describe("HSV", () => {
  it("round-trips through the picker's space", () => {
    for (const hex of ["#5b9dff", "#ff6159", "#3dd68c", "#000000", "#ffffff", "#808080"]) {
      expect(toHex(hsvToRgb(rgbToHsv(parseHex(hex)!)))).toBe(hex);
    }
  });

  it("names hues in degrees", () => {
    expect(rgbToHsv(parseHex("#ff0000")!)).toEqual({ h: 0, s: 1, v: 1 });
    expect(rgbToHsv(parseHex("#00ff00")!).h).toBe(120);
    expect(rgbToHsv(parseHex("#0000ff")!).h).toBe(240);
    expect(toHex(hsvToRgb({ h: 60, s: 1, v: 1 }))).toBe("#ffff00");
  });
});
