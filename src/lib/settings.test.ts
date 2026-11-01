import { describe, expect, it } from "vitest";
import { compactSidebar, compactTabbar, migrateSettings } from "./settings";

describe("settings from older versions", () => {
  it("turns compact mode on as both bars, and off as off", () => {
    expect(migrateSettings({ compactMode: true }, 2, false).compactMode).toBe("both");
    expect(migrateSettings({ compactMode: false }, 2, false).compactMode).toBe("off");
    expect(migrateSettings({}, 1, false).compactMode).toBe("off");
  });

  it("switches load beside server icons off once, for everybody", () => {
    expect(migrateSettings({ railStatus: "bars" }, 2, false).railStatus).toBe("off");
    // Picked again after the update, it stays.
    expect(migrateSettings({ railStatus: "bars", compactMode: "sidebar" }, 3, false)).toEqual({ railStatus: "bars", compactMode: "sidebar" });
  });

  it("moves the look that came with the app to SSHeesh Default, once", () => {
    const old = { theme: "midnight", darkTheme: "midnight", lightTheme: "daylight", accent: "#5b9dff" };
    expect(migrateSettings(old, 3, false)).toMatchObject({ theme: "ssheesh", darkTheme: "ssheesh", lightTheme: "daylight", accent: "#4eba65" });
    // A theme or accent picked by hand stays, and Midnight picked again after the update stays.
    expect(migrateSettings({ ...old, accent: "#b07cff" }, 3, false)).toMatchObject({ theme: "midnight", accent: "#b07cff" });
    expect(migrateSettings({ ...old, theme: "graphite" }, 3, false)).toMatchObject({ theme: "graphite", accent: "#5b9dff" });
    expect(migrateSettings(old, 4, false)).toEqual(old);
  });

  it("still moves Macs to the GPU from version 1", () => {
    expect(migrateSettings({ gpuRendering: false }, 1, true).gpuRendering).toBe(true);
    expect(migrateSettings({ gpuRendering: false }, 2, true).gpuRendering).toBe(false);
  });
});

describe("compact mode", () => {
  it("hides what you picked", () => {
    expect([compactSidebar("off"), compactSidebar("sidebar"), compactSidebar("tabbar"), compactSidebar("both")]).toEqual([false, true, false, true]);
    expect([compactTabbar("off"), compactTabbar("sidebar"), compactTabbar("tabbar"), compactTabbar("both")]).toEqual([false, false, true, true]);
  });
});
