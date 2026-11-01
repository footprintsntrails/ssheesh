import { describe, expect, it } from "vitest";
import { parseColor } from "./color";
import {
  APP_THEMES,
  BUILT_IN_COLORS,
  customThemeVars,
  resolveScheme,
  setCustomThemeSource,
  THEME_COLORS,
  themeColors,
  themeFromJson,
  themeInfo,
  themeToJson,
  weakContrast,
  type CustomTheme,
} from "./themes";

// Vitest empties CSS imports (?raw too), so index.css is read from disk.
type Fs = { readFileSync(path: URL, encoding: "utf8"): string };
const fs = (globalThis as unknown as { process: { getBuiltinModule(id: string): Fs } }).process.getBuiltinModule("node:fs");
const css = fs.readFileSync(new URL("../index.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

/** What index.css sets for a theme: every block for :root or that [data-theme], in order. */
function cssVars(id: string): Record<string, string> {
  const vars: Record<string, string> = {};
  for (const [, selectors, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const list = selectors.split(",").map((s) => s.trim());
    if (!list.includes(":root") && !list.includes(`[data-theme="${id}"]`)) continue;
    for (const [, name, value] of body.matchAll(/(--ui-[\w-]+):\s*([^;]+);/g)) vars[name] = value.trim();
  }
  return vars;
}

const mine = (over: Partial<CustomTheme> = {}): CustomTheme => ({ id: "custom-test", name: "Mine", base: "midnight", colors: {}, ...over });

describe("built-in theme colors", () => {
  it("match index.css", () => {
    for (const { id } of APP_THEMES) {
      const vars = cssVars(id);
      for (const { key, cssVar } of THEME_COLORS) {
        const want = parseColor(key === "ink" ? `rgb(${vars[cssVar]})` : vars[cssVar]);
        const have = parseColor(BUILT_IN_COLORS[id][key]);
        expect(want, `${id} ${cssVar}`).not.toBeNull();
        expect({ ...have, a: Math.round(have!.a * 100) }, `${id} ${cssVar}`).toEqual({ ...want, a: Math.round(want!.a * 100) });
      }
    }
  });

  it("are solid under menus and dialogs", () => {
    for (const { id } of APP_THEMES) expect(parseColor(BUILT_IN_COLORS[id].glass)?.a, id).toBe(1);
  });

  it("read well: every text color passes 4.5:1 on every background", () => {
    for (const { id } of APP_THEMES) expect(weakContrast(BUILT_IN_COLORS[id]), id).toEqual([]);
  });
});

describe("custom themes", () => {
  it("take their base's colors until changed", () => {
    expect(themeColors(mine())).toEqual(BUILT_IN_COLORS.midnight);
    expect(themeColors(mine({ base: "daylight" }))).toEqual(BUILT_IN_COLORS.daylight);
    expect(themeColors(mine({ colors: { fg: "#FF0000" } })).fg).toBe("#ff0000");
  });

  it("ignore broken stored colors, and alpha where it isn't allowed", () => {
    const c = themeColors(mine({ colors: { fg: "nope", window: "#11223380" } }));
    expect(c.fg).toBe(BUILT_IN_COLORS.midnight.fg);
    expect(c.window).toBe("#112233");
  });

  it("flip lines and shading with the window until you set them", () => {
    expect(themeColors(mine({ colors: { window: "#fafafa" } })).ink).toBe(BUILT_IN_COLORS.daylight.ink);
    expect(themeColors(mine({ colors: { window: "#fafafa", ink: "#ff00ff" } })).ink).toBe("#ff00ff");
  });

  it("set CSS variables, frosting glass only when it's see-through", () => {
    const solid = customThemeVars(mine());
    expect(solid["--ui-window"]).toBe("#0f1013");
    expect(solid["--ui-ink"]).toBe("255 255 255");
    expect(solid["color-scheme"]).toBe("dark");
    expect(solid["--ui-glass-filter"]).toBeUndefined();
    const clear = customThemeVars(mine({ colors: { glass: "#1f212680" } }));
    expect(clear["--ui-glass-filter"]).toContain("blur");
    expect(parseColor(clear["--ui-glass-solid"])?.a).toBe(1);
  });

  it("are found by the terminal's Match app scheme", () => {
    const t = mine({ colors: { canvas: "#ff000080", window: "#000000" } });
    setCustomThemeSource(() => [t]);
    try {
      expect(themeInfo(t.id).name).toBe("Mine");
      // A see-through pane, as it looks over the window.
      expect(resolveScheme("auto", t.id, "#5b9dff").background).toBe("#800000");
      expect(themeInfo("custom-gone").id).toBe("ssheesh");
    } finally {
      setCustomThemeSource(() => []);
    }
  });

  it("warn about text that's hard to read", () => {
    const weak = weakContrast(themeColors(mine({ colors: { fg3: "#333333" } })));
    expect(weak.map((w) => w.text)).toEqual(["fg3", "fg3", "fg3", "fg3"]);
    expect(weak[0].ratio).toBeLessThan(4.5);
  });
});

describe("sharing themes", () => {
  it("round-trips through JSON", () => {
    const t = mine({ name: "Night owl", colors: { fg: "#ffcc00", glass: "#1f2126cc" } });
    const back = themeFromJson(themeToJson(t));
    expect(back).toEqual({ theme: { name: "Night owl", base: "midnight", colors: { fg: "#ffcc00", glass: "#1f2126cc" } } });
  });

  it("keeps a set Lines and shading color, and leaves an automatic one out", () => {
    expect(JSON.parse(themeToJson(mine())).colors.ink).toBeUndefined();
    expect(JSON.parse(themeToJson(mine({ colors: { ink: "#ff00ff" } }))).colors.ink).toBe("#ff00ff");
  });

  it("ignores keys it doesn't know", () => {
    const r = themeFromJson(JSON.stringify({ name: "X", base: "oled", colors: { fg: "#123456", sparkle: "#ffffff" }, extra: 1 }));
    expect(r).toEqual({ theme: { name: "X", base: "oled", colors: { fg: "#123456" } } });
  });

  it("guesses the base from the window when it's missing", () => {
    const r = themeFromJson(JSON.stringify({ colors: { window: "#fdfdfd" } }));
    expect("theme" in r && r.theme.base).toBe("daylight");
    expect("theme" in r && r.theme.name).toBe("Imported theme");
  });

  it("refuses what isn't a theme, saying why", () => {
    const bad = [
      "",
      "not json",
      "[]",
      "null",
      JSON.stringify({ ssheesh: "host", colors: { fg: "#000000" } }),
      JSON.stringify({ name: 5, colors: { fg: "#000000" } }),
      JSON.stringify({ base: "neon", colors: { fg: "#000000" } }),
      JSON.stringify({ colors: [] }),
      JSON.stringify({ colors: { fg: "red" } }),
      JSON.stringify({ colors: { fg: "123456" } }),
      JSON.stringify({ colors: { fg: "#12345" } }),
      JSON.stringify({ colors: { fg: 0x123456 } }),
      JSON.stringify({ colors: { fg: "#12345680" } }),
      JSON.stringify({ colors: { nothing: "#123456" } }),
    ];
    for (const text of bad) {
      const r = themeFromJson(text);
      expect("error" in r, text).toBe(true);
      if ("error" in r) expect(r.error.length).toBeGreaterThan(10);
    }
  });
});
