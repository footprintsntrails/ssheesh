import { describe, expect, it } from "vitest";
import { findLinks, pathCrumbs, resolvePath } from "./paths";

const targets = (text: string) => findLinks(text).map((l) => l.target);

describe("pathCrumbs", () => {
  it("splits a path into folders you can open", () => {
    expect(pathCrumbs("/srv/app/logs")).toEqual([
      { label: "/", path: "/" },
      { label: "srv", path: "/srv" },
      { label: "app", path: "/srv/app" },
      { label: "logs", path: "/srv/app/logs" },
    ]);
  });
  it("handles the root, home and Windows drives", () => {
    expect(pathCrumbs("/")).toEqual([{ label: "/", path: "/" }]);
    expect(pathCrumbs("~/notes").map((c) => c.path)).toEqual(["~", "~/notes"]);
    expect(pathCrumbs("C:/Users/me").map((c) => c.path)).toEqual(["C:/", "C:/Users", "C:/Users/me"]);
    expect(pathCrumbs("/srv/app/").map((c) => c.label)).toEqual(["/", "srv", "app"]);
  });
});

describe("findLinks", () => {
  it("finds absolute, home and dot paths", () => {
    expect(targets("wrote /srv/app/docs/SETUP.md")).toEqual(["/srv/app/docs/SETUP.md"]);
    expect(targets("see ~/notes.md and ./run.sh or ../up")).toEqual(["~/notes.md", "./run.sh", "../up"]);
  });
  it("finds relative paths with a folder and a file", () => {
    expect(targets("Edited src/lib/paths.ts")).toEqual(["src/lib/paths.ts"]);
    expect(targets("in packages/web/")).toEqual(["packages/web/"]);
  });
  it("leaves words that only look like paths", () => {
    expect(targets("and/or TCP/IP 1/2 @types/node README.md / //")).toEqual([]);
  });
  it("takes line numbers off and keeps them", () => {
    const [l] = findLinks("error at src/main.rs:12:5: oops");
    expect(l).toMatchObject({ kind: "path", target: "src/main.rs", line: 12 });
    expect("error at src/main.rs:12:5: oops".slice(l.start, l.end)).toBe("src/main.rs:12:5");
  });
  it("drops sentence punctuation and quotes", () => {
    expect(targets("Saved to '/etc/nginx/nginx.conf'.")).toEqual(["/etc/nginx/nginx.conf"]);
    expect(targets("(see /var/log/syslog), then")).toEqual(["/var/log/syslog"]);
    expect(targets("--config=/etc/app.toml")).toEqual(["/etc/app.toml"]);
  });
  it("finds web links", () => {
    const links = findLinks("Open http://localhost:3000/ or https://example.com/docs.");
    expect(links.map((l) => [l.kind, l.target])).toEqual([
      ["url", "http://localhost:3000/"],
      ["url", "https://example.com/docs"],
    ]);
  });
  it("reports where each link is", () => {
    const text = "x /etc/hosts y";
    const [l] = findLinks(text);
    expect(text.slice(l.start, l.end)).toBe("/etc/hosts");
  });
});

describe("resolvePath", () => {
  it("makes relative paths absolute against the terminal's folder", () => {
    expect(resolvePath("src/main.rs", "/srv/app")).toBe("/srv/app/src/main.rs");
    expect(resolvePath("../x/y.md", "/srv/app/")).toBe("/srv/x/y.md");
    expect(resolvePath("./run.sh", "/")).toBe("/run.sh");
  });
  it("keeps absolute and home paths", () => {
    expect(resolvePath("/etc//hosts", "/srv")).toBe("/etc/hosts");
    expect(resolvePath("~/a/../b.md", "/srv")).toBe("~/b.md");
  });
});
