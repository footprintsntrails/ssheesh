import { describe, expect, it } from "vitest";
import { parseScrollTitle, thumb, tmuxPosition, tmuxView, topForOffset } from "./scroll";

describe("parseScrollTitle", () => {
  it("reads tmux's report, in and out of copy mode", () => {
    expect(parseScrollTitle("sshee-scroll 1 100 480 24 2000")).toEqual({ inMode: true, position: 100, history: 480, height: 24, limit: 2000 });
    expect(parseScrollTitle("sshee-scroll 0  480 24 2000")).toEqual({ inMode: false, position: 0, history: 480, height: 24, limit: 2000 });
  });
  it("ignores other titles", () => {
    expect(parseScrollTitle("vim notes.md")).toBeNull();
    expect(parseScrollTitle("sshee-scroll 1 x 480 24 2000")).toBeNull();
    expect(parseScrollTitle("sshee-scroll 1")).toBeNull();
  });
});

describe("scroll math", () => {
  const s = { inMode: true, position: 100, history: 480, height: 24, limit: 2000 };
  it("turns tmux's position into lines in view and back", () => {
    const v = tmuxView(s);
    expect(v).toEqual({ total: 504, top: 380, rows: 24 });
    expect(tmuxPosition(s, v.top)).toBe(100);
    expect(tmuxPosition(s, -50)).toBe(480);
    expect(tmuxPosition(s, 9999)).toBe(0);
  });
  it("sizes and places the thumb", () => {
    expect(thumb({ total: 100, top: 0, rows: 100 }, 400)).toEqual({ size: 400, offset: 0 });
    const t = thumb({ total: 1000, top: 900, rows: 100 }, 400);
    expect(t).toEqual({ size: 40, offset: 360 });
    expect(thumb({ total: 100000, top: 0, rows: 10 }, 400).size).toBe(24);
  });
  it("maps a dragged thumb back to a line", () => {
    const v = { total: 1000, top: 0, rows: 100 };
    expect(topForOffset(v, 400, 40, 0)).toBe(0);
    expect(topForOffset(v, 400, 40, 360)).toBe(900);
    expect(topForOffset(v, 400, 40, 180)).toBe(450);
    expect(topForOffset(v, 400, 40, 9999)).toBe(900);
  });
});
