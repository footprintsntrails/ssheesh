import { describe, expect, it } from "vitest";
import { blankLabel, blanks, fill } from "./snippets";

describe("snippet blanks", () => {
  it("finds each blank once, with what it falls back to", () => {
    expect(blanks("docker logs -f --tail {{lines:100}} {{container}} && echo {{ container }}")).toEqual([
      { name: "lines", fallback: "100" },
      { name: "container", fallback: "" },
    ]);
    expect(blanks("uptime")).toEqual([]);
  });
  it("fills them in, falling back when left empty", () => {
    const cmd = "docker logs --tail {{lines:100}} {{container}}";
    expect(fill(cmd, { container: "web", lines: "" })).toBe("docker logs --tail 100 web");
    expect(fill(cmd, { container: "web", lines: " 20 " })).toBe("docker logs --tail 20 web");
    expect(fill("echo {{a}}-{{a}}", { a: "x" })).toBe("echo x-x");
  });
  it("leaves braces that aren't blanks alone", () => {
    expect(blanks("awk '{print $1}' | jq '{a: .b}'")).toEqual([]);
    expect(fill("awk '{print $1}'", {})).toBe("awk '{print $1}'");
  });
  it("labels a blank in words", () => {
    expect(blankLabel("max_lines")).toBe("Max lines");
    expect(blankLabel("container")).toBe("Container");
  });

  it("leaves Go templates alone", () => {
    const cmd = "docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' {{container}}";
    expect(blanks(cmd).map((b) => b.name)).toEqual(["container"]);
    expect(fill(cmd, { container: "web" })).toBe("docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' web");
  });
});
