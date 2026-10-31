import { beforeEach, describe, expect, it } from "vitest";
import {
  claimedPorts,
  describeForward,
  hostError,
  keepTemp,
  openWebApp,
  portError,
  setSavedOn,
  startTunnel,
  tunnelTitle,
  useSavedTunnels,
  useTunnelRuntime,
  waitUntilOn,
} from "./tunnels";
import type { WebApp } from "./types";

const grafana: WebApp = {
  port: 3000,
  remoteHost: "127.0.0.1",
  process: "grafana",
  knownAs: null,
  title: "Grafana",
  scheme: "http",
  status: 200,
  web: true,
  public: false,
};

beforeEach(() => {
  useSavedTunnels.setState({ byHost: {} });
  useTunnelRuntime.setState({ status: {}, temp: {} });
});

describe("opening a server's web app", () => {
  it("starts a temporary tunnel on the same port, reuses it, and can keep it", async () => {
    const port = await openWebApp("cfg:prod-api", grafana, false);
    expect(port).toBe(3000);
    const [temp] = useTunnelRuntime.getState().temp["cfg:prod-api"];
    expect(temp.forward).toEqual({ kind: "local", localPort: 3000, remoteHost: "127.0.0.1", remotePort: 3000 });
    expect(useTunnelRuntime.getState().status[temp.id]).toEqual({ kind: "on", warning: null });

    expect(await openWebApp("cfg:prod-api", grafana, false)).toBe(3000);
    expect(useTunnelRuntime.getState().temp["cfg:prod-api"]).toHaveLength(1);

    keepTemp("cfg:prod-api", temp.id);
    const [saved] = useSavedTunnels.getState().byHost["cfg:prod-api"];
    expect(saved).toMatchObject({ id: temp.id, name: "Grafana", on: true, scheme: "http" });
    expect(useTunnelRuntime.getState().temp["cfg:prod-api"]).toHaveLength(0);
    expect(claimedPorts()).toEqual([3000]);

    setSavedOn("cfg:prod-api", saved, false);
    expect(useTunnelRuntime.getState().status[temp.id]).toBeUndefined();
    expect(useSavedTunnels.getState().byHost["cfg:prod-api"][0].on).toBe(false);
  });

  it("picks another port when this computer's is taken", async () => {
    // The demo's "this computer" has AirPlay on 5000.
    const port = await openWebApp("cfg:homelab", { ...grafana, port: 5000, title: "Photo frame" }, false);
    expect(port).toBe(15000);
  });

  it("says plainly when a saved tunnel's port is taken", async () => {
    await startTunnel("cfg:pi", "t1", { kind: "socks", localPort: 5000 });
    expect(useTunnelRuntime.getState().status.t1).toEqual({ kind: "failed", message: "Port 5000 on this computer is already in use." });
    await expect(waitUntilOn("t1")).rejects.toMatchObject({ message: "Port 5000 on this computer is already in use." });
  });
});

describe("tunnel forms and labels", () => {
  it("checks ports and addresses", () => {
    expect(portError("", "the server's port")).toBe("Enter the server's port.");
    expect(portError("70000", "a port on this computer")).toBe("A port on this computer is a number from 1 to 65535.");
    expect(portError("8080", "a port on this computer")).toBeNull();
    expect(hostError("")).toBeNull();
    expect(hostError("10.0.0.5")).toBeNull();
    expect(hostError("::1")).toBeNull();
    expect(hostError("-oProxyCommand=sh")).not.toBeNull();
    expect(hostError("db host")).not.toBeNull();
  });

  it("describes tunnels in plain words", () => {
    expect(describeForward({ kind: "local", localPort: 8080, remoteHost: "127.0.0.1", remotePort: 3000 })).toBe(
      "localhost:8080 on this computer opens the server's port 3000.",
    );
    expect(describeForward({ kind: "local", localPort: 5433, remoteHost: "10.0.0.5", remotePort: 5432 })).toBe(
      "localhost:5433 on this computer opens port 5432 on 10.0.0.5, reached through the server.",
    );
    expect(describeForward({ kind: "socks", localPort: 1080 })).toContain("SOCKS proxy");
    expect(tunnelTitle({ forward: { kind: "local", localPort: 8080, remoteHost: "", remotePort: 3000 } })).toBe("Server port 3000");
    expect(tunnelTitle({ name: "Grafana", forward: { kind: "socks", localPort: 1080 } })).toBe("Grafana");
  });
});
