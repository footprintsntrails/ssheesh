import { describe, expect, it } from "vitest";
import { linesAfter } from "./away";

describe("linesAfter", () => {
  const history = ["$ deploy", "pulling", "done", "$ ", "new 1", "new 2", "$ ", ""];

  it("returns what came after the last screen lines", () => {
    expect(linesAfter(history, ["pulling", "done"])).toEqual(["$ ", "new 1", "new 2", "$ "]);
  });

  it("ignores trailing spaces and blank lines", () => {
    expect(linesAfter(["a  ", "b", "c", "", ""], ["a", "b"])).toEqual(["c"]);
  });

  it("gives up when the screen lines aren't in the scrollback", () => {
    expect(linesAfter(history, ["something else"])).toBeNull();
  });
});
