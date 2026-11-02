// Finding text in a terminal: in its rows as a terminal keeps them, where a
// long line runs over several rows, and a match may too.

/** One row of a terminal. `wrapped`: it goes on from the row before. */
export interface FindRow {
  text: string;
  wrapped: boolean;
}

/** Where a match starts, and how many characters it has. */
export interface FindMatch {
  row: number;
  start: number;
  length: number;
}

/** A part of a match on one row. */
export type FindPiece = FindMatch;

/** More than anyone goes through one by one. */
export const FIND_LIMIT = 10000;

/**
 * The rows tmux printed (`capture-pane -p`). It says nothing about wrapping
 * there, but a row that fills the width goes on in the next one (a row of
 * wide characters may be missed, and then a match across it isn't found).
 */
export function rowsFromCapture(text: string, cols: number): FindRow[] {
  const lines = text.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  return lines.map((line, i) => ({ text: line, wrapped: i > 0 && lines[i - 1].length >= cols }));
}

/** Every match of `query`, case aside, in order; at most `FIND_LIMIT`. */
export function findRows(rows: FindRow[], query: string): FindMatch[] {
  if (!query) return [];
  const pattern = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "giu");
  const matches: FindMatch[] = [];
  for (let first = 0; first < rows.length && matches.length < FIND_LIMIT; ) {
    let last = first;
    while (rows[last + 1]?.wrapped) last++;
    let line = rows[first].text;
    for (let i = first + 1; i <= last; i++) line += rows[i].text;
    let row = first;
    let rowStart = 0;
    for (const m of line.matchAll(pattern)) {
      if (matches.length >= FIND_LIMIT) break;
      while (row < last && m.index >= rowStart + rows[row].text.length) {
        rowStart += rows[row].text.length;
        row++;
      }
      matches.push({ row, start: m.index - rowStart, length: m[0].length });
    }
    first = last + 1;
  }
  return matches;
}

/** A match, row by row. */
export function matchPieces(rows: FindRow[], match: FindMatch): FindPiece[] {
  const pieces: FindPiece[] = [];
  let { row, start, length } = match;
  while (length > 0 && rows[row]) {
    const here = Math.min(length, rows[row].text.length - start);
    if (here > 0) pieces.push({ row, start, length: here });
    length -= Math.max(here, 0);
    row++;
    start = 0;
    if (!rows[row]?.wrapped) break;
  }
  return pieces;
}

/**
 * The match to start from when a search begins: the nearest at or above a
 * place in the history (`anchor`, rows up from the bottom), as what you're
 * after is mostly recent. Rows are counted from the bottom as new output
 * pushes everything up.
 */
export function nearestMatch(matches: FindMatch[], total: number, anchor: number): number {
  for (let i = matches.length - 1; i >= 0; i--) {
    if (total - 1 - matches[i].row >= anchor) return i;
  }
  return matches.length - 1;
}
