import { describe, expect, it } from "vitest";
import { FIND_LIMIT, findRows, matchPieces, nearestMatch, rowsFromCapture, type FindRow } from "./termFind";

const plain = (...text: string[]): FindRow[] => text.map((t) => ({ text: t, wrapped: false }));

describe("findRows", () => {
  it("finds every match, case aside, in Russian too", () => {
    expect(findRows(plain("Error here", "no", "an error and ERROR"), "error")).toEqual([
      { row: 0, start: 0, length: 5 },
      { row: 2, start: 3, length: 5 },
      { row: 2, start: 13, length: 5 },
    ]);
    expect(findRows(plain("Ошибка в файле"), "ошибка")).toEqual([{ row: 0, start: 0, length: 6 }]);
  });

  it("takes what you type literally", () => {
    expect(findRows(plain("a.b axb", "(x)"), "a.b")).toEqual([{ row: 0, start: 0, length: 3 }]);
    expect(findRows(plain("a.b axb", "(x)"), "(x)")).toEqual([{ row: 1, start: 0, length: 3 }]);
    expect(findRows(plain("[1] \\d"), "\\d")).toEqual([{ row: 0, start: 4, length: 2 }]);
  });

  it("finds nothing for nothing", () => {
    expect(findRows(plain("text"), "")).toEqual([]);
    expect(findRows([], "x")).toEqual([]);
  });

  it("finds a match that a long line wraps in two", () => {
    const rows: FindRow[] = [
      { text: "first", wrapped: false },
      { text: "abcdeERR", wrapped: false },
      { text: "ORfgh", wrapped: true },
      { text: "last ERROR", wrapped: false },
    ];
    const found = findRows(rows, "error");
    expect(found).toEqual([
      { row: 1, start: 5, length: 5 },
      { row: 3, start: 5, length: 5 },
    ]);
    expect(matchPieces(rows, found[0])).toEqual([
      { row: 1, start: 5, length: 3 },
      { row: 2, start: 0, length: 2 },
    ]);
    expect(matchPieces(rows, found[1])).toEqual([{ row: 3, start: 5, length: 5 }]);
  });

  it("places a match on the right row further along a wrapped line", () => {
    const rows: FindRow[] = [
      { text: "aaaa", wrapped: false },
      { text: "aaaa", wrapped: true },
      { text: "aXaa", wrapped: true },
    ];
    expect(findRows(rows, "x")).toEqual([{ row: 2, start: 1, length: 1 }]);
  });

  it("stops at the limit", () => {
    expect(findRows(plain("x".repeat(FIND_LIMIT + 50)), "x")).toHaveLength(FIND_LIMIT);
  });
});

describe("rowsFromCapture", () => {
  it("joins a row that fills the width to the next", () => {
    expect(rowsFromCapture("12345\n678\nabc\n", 5)).toEqual([
      { text: "12345", wrapped: false },
      { text: "678", wrapped: true },
      { text: "abc", wrapped: false },
    ]);
  });

  it("keeps empty rows, so rows count from the bottom stay right", () => {
    expect(rowsFromCapture("a\n\n\n", 80).map((r) => r.text)).toEqual(["a", "", ""]);
  });
});

describe("nearestMatch", () => {
  // Rows 0..9; matches on rows 1, 5 and 8.
  const matches = [1, 5, 8].map((row) => ({ row, start: 0, length: 1 }));

  it("starts at the newest match in view or above it", () => {
    expect(nearestMatch(matches, 10, 0)).toBe(2);
    expect(nearestMatch(matches, 10, 2)).toBe(1);
    expect(nearestMatch(matches, 10, 8)).toBe(0);
  });

  it("falls back to the newest when all are below the place", () => {
    expect(nearestMatch(matches, 10, 9)).toBe(2);
  });
});
