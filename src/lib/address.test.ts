import { describe, expect, it } from "vitest";
import { parseAddress } from "./address";

describe("parseAddress", () => {
  it("reads user@host:port", () => {
    expect(parseAddress("root@203.0.113.10:2222")).toEqual({ host: "203.0.113.10", user: "root", port: "2222", identityFile: null });
  });

  it("reads a plain host", () => {
    expect(parseAddress("  nas.local ")).toEqual({ host: "nas.local", user: null, port: null, identityFile: null });
  });

  it("reads a whole ssh command", () => {
    expect(parseAddress("ssh -p 2200 -i ~/.ssh/work deploy@web.example.com")).toEqual({
      host: "web.example.com",
      user: "deploy",
      port: "2200",
      identityFile: "~/.ssh/work",
    });
  });

  it("reads -l, glued values and skips flags and remote commands", () => {
    expect(parseAddress("ssh -A -p2201 -l pi raspberrypi.local uptime")).toEqual({ host: "raspberrypi.local", user: "pi", port: "2201", identityFile: null });
  });

  it("reads IPv6 in brackets and ssh:// links", () => {
    expect(parseAddress("[2001:db8::7]:2222").host).toBe("2001:db8::7");
    expect(parseAddress("ssh://admin@router.lan:2022")).toEqual({ host: "router.lan", user: "admin", port: "2022", identityFile: null });
  });
});
