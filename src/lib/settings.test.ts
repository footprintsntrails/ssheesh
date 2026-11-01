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
