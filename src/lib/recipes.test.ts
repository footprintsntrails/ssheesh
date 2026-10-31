import { describe, expect, it } from "vitest";
import {
  RECIPE_FOR_CHECK,
  RECIPES,
  fieldError,
  fieldValues,
  guessOs,
  makeContext,
  missingFields,
  parsePublicKey,
  resolve,
  sshCommand,
  stepsFor,
  type Os,
  type RecipeContext,
  type Value,
} from "./recipes";
import type { Host } from "./types";

const OSES: Os[] = ["debian", "rhel"];
const KEY = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGdlbW9rZXlmb3J0ZXN0c29ubHlub3RyZWFsMTIzNDU2 id_ed25519";

const saved: Host = { id: "saved:a", name: "web", hostname: "203.0.113.10", user: "root", port: 2222, identityFile: null, source: "saved" };
const config: Host = { id: "cfg:web", name: "web", hostname: "203.0.113.10", user: "root", port: null, identityFile: null, source: "sshConfig" };

const SAMPLE: Record<string, string> = {
  publicKey: KEY,
  keyFile: "~/.ssh/id_ed25519",
  newUser: "alex",
  domain: "example.com",
  appPort: "3000",
  swapSize: "2",
};

/** Every combination a recipe can be shown in. */
function contexts(): RecipeContext[] {
  const out: RecipeContext[] = [];
  for (const os of OSES)
    for (const host of [saved, config])
      for (const windows of [false, true])
        for (const sshPort of ["22", "2222"]) out.push(makeContext({ os, host, windows, values: { ...SAMPLE, sshPort } }));
  return out;
}

// Left-over drafting, or a value that never got filled in.
const PLACEHOLDER = /\b(TODO|FIXME|XXX|TBD|lorem|ipsum)\b|YOUR_|your[-_](domain|server|user|key|ip)\b|CHANGE_?ME|<[a-z][a-z _-]*>|\bundefined\b|(?<!\/dev\/)\bnull\b|\bNaN\b|\[object /i;

function allText(c: RecipeContext): { where: string; text: string; command: boolean }[] {
  const out: { where: string; text: string; command: boolean }[] = [];
  for (const r of RECIPES) {
    out.push({ where: `${r.id} title`, text: r.title, command: false }, { where: `${r.id} summary`, text: r.summary, command: false }, { where: `${r.id} who`, text: r.who, command: false });
    for (const f of r.fields ?? []) for (const t of [f.label, f.help, f.placeholder, f.invalid]) if (t !== undefined) out.push({ where: `${r.id} field ${f.id}`, text: t, command: false });
    for (const s of stepsFor(r, c.os)) {
      const at = `${r.id} / ${s.title} (${c.os})`;
      const add = (v: Value | undefined, what: string, command = false) => v !== undefined && out.push({ where: `${at} ${what}`, text: resolve(v, c), command });
      out.push({ where: at, text: s.title, command: false });
      add(s.explain, "explain");
      add(s.command, "command", true);
      add(s.warn, "warn");
      add(s.expect, "expect");
      add(s.ifFails?.explain, "ifFails.explain");
      add(s.ifFails?.command, "ifFails.command", true);
    }
  }
  return out;
}

describe("recipes", () => {
  it("have unique, tidy ids and the basics filled in", () => {
    const ids = RECIPES.map((r) => r.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const r of RECIPES) {
      expect(r.id).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
      expect(r.title.trim()).not.toBe("");
      expect(r.summary.trim()).not.toBe("");
      expect(r.who.trim()).not.toBe("");
    }
  });

  it("give every step an explanation and a command, on every system", () => {
    for (const c of contexts()) {
      for (const r of RECIPES) {
        const steps = stepsFor(r, c.os);
        expect(steps.length, `${r.id} on ${c.os}`).toBeGreaterThanOrEqual(3);
        const titles = steps.map((s) => s.title);
        expect(new Set(titles).size, `${r.id} step titles`).toBe(titles.length);
        for (const s of steps) {
          expect(resolve(s.explain, c).trim(), `${r.id} / ${s.title}`).not.toBe("");
          expect(resolve(s.command, c).trim(), `${r.id} / ${s.title}`).not.toBe("");
          if (s.ifFails) expect(resolve(s.ifFails.command, c).trim()).not.toBe("");
        }
        expect(steps.some((s) => s.check), `${r.id} has a check step`).toBe(true);
      }
    }
  });

  it("leave no empty strings or placeholders, and only example addresses", () => {
    for (const c of contexts()) {
      for (const { where, text, command } of allText(c)) {
        expect(text.trim(), where).not.toBe("");
        expect(text, where).toBe(text.trim());
        expect(PLACEHOLDER.test(text), `${where}: ${text}`).toBe(false);
        for (const ip of text.match(/\b\d{1,3}(\.\d{1,3}){3}\b/g) ?? []) expect(ip, where).toMatch(/^(203\.0\.113\.\d+|127\.0\.0\.1|0\.0\.0\.0)$/);
        // A newline would run a pasted command before you've read it.
        if (command) expect(text, where).not.toMatch(/[\r\n]/);
      }
    }
  });

  it("follow every step that can lock you out with a fresh login check", () => {
    for (const os of OSES) {
      for (const r of RECIPES) {
        const steps = stepsFor(r, os);
        steps.forEach((s, i) => {
          if (!s.lockout) return;
          expect(s.warn, `${r.id} / ${s.title} warns first`).toBeTruthy();
          const after = steps.slice(i + 1);
          expect(after.some((x) => x.check && x.where === "local" && x.ifFails), `${r.id} / ${s.title} is checked from your computer`).toBe(true);
        });
      }
    }
  });

  it("only wait for fields they declare", () => {
    for (const r of RECIPES) {
      const ids = new Set((r.fields ?? []).map((f) => f.id));
      for (const s of r.steps) for (const id of s.needs ?? []) expect(ids.has(id), `${r.id} / ${s.title} needs ${id}`).toBe(true);
      for (const f of r.fields ?? []) if (f.kind === "text") expect(f.pattern, `${r.id} field ${f.id}`).toBeTruthy();
    }
  });

  it("point check-up findings at recipes that exist", () => {
    for (const id of Object.values(RECIPE_FOR_CHECK)) expect(RECIPES.some((r) => r.id === id)).toBe(true);
  });
});

describe("recipe fields", () => {
  const caddy = RECIPES.find((r) => r.id === "caddy-https")!;
  const firewall = RECIPES.find((r) => r.id === "firewall")!;
  const base = makeContext({ os: "debian", host: saved, windows: false, values: {} });

  it("keep only valid values, and fill in defaults", () => {
    expect(fieldValues(caddy, { domain: "https://example.com", appPort: "70000" }, base)).toEqual({});
    expect(fieldValues(caddy, { domain: "app.example.com", appPort: "8080" }, base)).toEqual({ domain: "app.example.com", appPort: "8080" });
    expect(fieldValues(firewall, {}, base)).toEqual({ sshPort: "2222" });
    const step = stepsFor(caddy, "debian").find((s) => s.needs?.includes("appPort"))!;
    expect(missingFields(caddy, step, { domain: "example.com" }).map((f) => f.id)).toEqual(["appPort"]);
  });

  it("reject user names the system already has", () => {
    const user = RECIPES.find((r) => r.id === "sudo-user")!.fields![0];
    expect(fieldError(user, "alex")).toBeNull();
    expect(fieldError(user, "root")).not.toBeNull();
    expect(fieldError(user, "Alex")).not.toBeNull();
  });

  it("read public keys, dropping personal comments", () => {
    const blob = KEY.split(" ")[1];
    expect(parsePublicKey(`ssh-ed25519 ${blob} sam@laptop\n`, "id_ed25519")).toBe(`ssh-ed25519 ${blob} id_ed25519`);
    expect(parsePublicKey(`ssh-ed25519 ${blob} it's mine`)).toBe(`ssh-ed25519 ${blob} its-mine`);
    expect(parsePublicKey(`ssh-ed25519 ${blob}`)).toBe(`ssh-ed25519 ${blob}`);
    expect(parsePublicKey("-----BEGIN OPENSSH PRIVATE KEY-----")).toBeNull();
    expect(parsePublicKey("ssh-ed25519 not-base64!")).toBeNull();
  });
});

describe("ssh commands for checks", () => {
  it("log in afresh, the way SSHeesh reaches the server", () => {
    expect(sshCommand(saved, { keyOnly: true, run: "echo hi" })).toBe("ssh -o ControlPath=none -o PreferredAuthentications=publickey -p 2222 root@203.0.113.10 echo hi");
    expect(sshCommand(saved, { user: "alex", tty: true, run: "sudo whoami" }, true)).toBe("ssh -t -p 2222 alex@203.0.113.10 sudo whoami");
    expect(sshCommand(config, { user: "alex", identity: "~/.ssh/id_ed25519" })).toBe("ssh -o ControlPath=none -i ~/.ssh/id_ed25519 -l alex web");
  });
});

describe("guessOs", () => {
  it("reads the system family from the kernel", () => {
    expect(guessOs("Linux 6.8.0-45-generic")).toBe("debian");
    expect(guessOs("Linux 6.1.0-25-cloud-amd64")).toBe("debian");
    expect(guessOs("Linux 6.12.38+deb13-amd64")).toBe("debian");
    expect(guessOs("Linux 5.14.0-427.13.1.el9_4.x86_64")).toBe("rhel");
    expect(guessOs("Linux 6.11.4-301.fc41.x86_64")).toBe("rhel");
    expect(guessOs("Linux 6.8.12-4-pve")).toBeNull();
  });
});
