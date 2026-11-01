import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { backend } from "./backend";
import { useUpdates } from "./updates";

const release = (version: string) => ({ version, notes: "", date: null });

describe("installing an update", () => {
  beforeEach(() => {
    useUpdates.setState({ portable: false, autoFailed: false, status: { kind: "available", version: "0.8.1", notes: "" } });
  });
  afterEach(() => vi.restoreAllMocks());

  it("installs the newest version out, not the one found earlier", async () => {
    const check = vi.spyOn(backend, "checkUpdate").mockResolvedValue(release("0.8.2"));
    const install = vi.spyOn(backend, "installUpdate").mockResolvedValue();
    await useUpdates.getState().install();
    expect(check).toHaveBeenCalledBefore(install);
    expect(useUpdates.getState().status).toEqual({ kind: "ready", version: "0.8.2" });
  });

  it("does the same for the portable Windows app", async () => {
    useUpdates.setState({ portable: true });
    vi.spyOn(backend, "checkUpdate").mockResolvedValue(release("0.8.2"));
    const install = vi.spyOn(backend, "installPortableUpdate").mockResolvedValue();
    await useUpdates.getState().install({ auto: true });
    expect(install).toHaveBeenCalledOnce();
    expect(useUpdates.getState().status).toEqual({ kind: "ready", version: "0.8.2" });
  });

  it("installs nothing when the second look fails", async () => {
    vi.spyOn(backend, "checkUpdate").mockRejectedValue({ code: "network", message: "Couldn't reach the update server." });
    const install = vi.spyOn(backend, "installUpdate").mockResolvedValue();
    await useUpdates.getState().install({ auto: true });
    expect(install).not.toHaveBeenCalled();
    expect(useUpdates.getState().status).toMatchObject({ kind: "error" });
  });
});
