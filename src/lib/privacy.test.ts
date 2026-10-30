import { beforeEach, describe, expect, it } from "vitest";
import { maskText, usePrivacy } from "./privacy";
import { useSettings } from "./settings";
import { useStore } from "./store";
import type { Host } from "./types";

const host = (over: Partial<Host>): Host => ({
  id: "h1",
  name: "prod-api",
  hostname: "203.0.113.10",
  user: "deploy",
  port: null,
  identityFile: null,
  source: "saved",
  ...over,
});

beforeEach(() => {
  useStore.setState({ hosts: [host({}), host({ id: "h2", name: "homelab", hostname: "nas.home.example.net", user: "root" })] });
  usePrivacy.setState({ learned: [] });
  useSettings.setState({ streamerMode: true });
});

describe("maskText", () => {
  it("hides server names, users and addresses without moving columns", () => {
    const line = "deploy@prod-api:~$ ping 203.0.113.10";
    const out = maskText(line);
    expect(out).toBe("••••••@••••••••:~$ ping ••••••••••••");
    expect(out.length).toBe(line.length);
  });

  it("hides a full hostname and its short form", () => {
    expect(maskText("ssh nas.home.example.net")).toBe("ssh ••••••••••••••••••••");
    expect(maskText("nas login:")).toBe("••• login:");
  });

  it("leaves names everybody has alone", () => {
    expect(maskText("root@box:/root#")).toContain("root@");
  });

  it("hides IPv6 addresses but not clock times", () => {
    expect(maskText("from 2001:db8::1 at 12:34:56")).toBe("from ••••:•••::• at 12:34:56");
    expect(maskText("fe80::1c2d:3e4f:5a6b:7c8d")).not.toMatch(/[0-9a-f]/);
  });

  it("leaves terminal color codes intact", () => {
    const sgr = "\x1b[38:2::255:0:0mred\x1b[0m";
    expect(maskText(sgr)).toBe(sgr);
  });

  it("hides names learned at run time", () => {
    usePrivacy.getState().learn(["web-7731.example.com"]);
    expect(maskText("user@web-7731:~$")).toBe("user@••••••••:~$");
  });
});
