// Highlighting in terminal output: errors in red, warnings in yellow, and
// whatever rules you add. Colors are the terminal scheme's own, so they fit
// every theme.

import { useSettings } from "./settings";

export type HighlightColor = "red" | "yellow" | "green" | "blue" | "magenta" | "cyan";

export const HIGHLIGHT_COLORS: HighlightColor[] = ["red", "yellow", "green", "blue", "magenta", "cyan"];

/** SGR parameters for each color; errors are also bold. */
const SGR: Record<HighlightColor, string> = { red: "31", yellow: "33", green: "32", blue: "34", magenta: "35", cyan: "36" };

export interface HighlightRule {
  id: string;
  /** Plain text (any case), or a regular expression written as /like this/. */
  pattern: string;
  color: HighlightColor;
}

export interface Match {
  start: number;
  end: number;
  sgr: string;
}

const ERRORS =
  /\b(?:ERROR|ERR!|FATAL|FAILED|FAILURE|FAIL|PANIC|CRITICAL)\b|\b(?:[Ee]rror|[Ff]atal|[Ff]ailed|[Pp]anic)(?=[:[])|Permission denied|No such file or directory|command not found|Connection refused|Connection timed out|Segmentation fault|Traceback \(most recent call last\)/g;
const WARNINGS = /\b(?:WARNING|WARN|DEPRECATED)\b|\b(?:[Ww]arning|[Dd]eprecated)(?=[:[])/g;
const ADDRESSES = /\bhttps?:\/\/[^\s"'<>]+|(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?(?![\w.])/g;

/** Words that might be cut in half at the end of a chunk; see `highlightTail`. */
const STARTS = [
  "ERROR", "FATAL", "FAILED", "FAILURE", "PANIC", "CRITICAL", "WARNING", "DEPRECATED",
  "Permission denied", "No such file or directory", "command not found", "Connection refused",
  "Connection timed out", "Segmentation fault", "Traceback (most recent call last)",
];

/** Turns a rule into a regex, or null when its pattern doesn't compile. */
export function compileRule(pattern: string): RegExp | null {
  const p = pattern.trim();
  if (!p) return null;
  const re = /^\/(.+)\/([a-z]*)$/.exec(p);
  try {
    if (re) return new RegExp(re[1], re[2].includes("g") ? re[2] : `${re[2]}g`);
    return new RegExp(p.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
  } catch {
    return null;
  }
}

type Settings = ReturnType<typeof useSettings.getState>;
type Rules = { re: RegExp; sgr: string }[];

/** The rules, made again only when a highlight setting changes (settings are replaced, never changed in place). */
let cache: { errors: boolean; warnings: boolean; addresses: boolean; custom: HighlightRule[]; rules: Rules } | null = null;
const cached = (s: Settings) =>
  cache?.errors === s.highlightErrors && cache.warnings === s.highlightWarnings && cache.addresses === s.highlightAddresses && cache.custom === s.highlightRules;

function activeRules(): Rules {
  const s = useSettings.getState();
  if (cache && cached(s)) return cache.rules;
  const rules: Rules = [];
  // Your own rules first: they win where they overlap a built-in one.
  for (const r of s.highlightRules) {
    const re = compileRule(r.pattern);
    if (re) rules.push({ re, sgr: SGR[r.color] });
  }
  if (s.highlightErrors) rules.push({ re: ERRORS, sgr: `1;${SGR.red}` });
  if (s.highlightWarnings) rules.push({ re: WARNINGS, sgr: SGR.yellow });
  if (s.highlightAddresses) rules.push({ re: ADDRESSES, sgr: `4;${SGR.cyan}` });
  cache = { errors: s.highlightErrors, warnings: s.highlightWarnings, addresses: s.highlightAddresses, custom: s.highlightRules, rules };
  return rules;
}

export function highlightOn(): boolean {
  return activeRules().length > 0;
}

/** Non-overlapping matches in `text`, in order. */
export function findMatches(text: string): Match[] {
  const found: Match[] = [];
  for (const { re, sgr } of activeRules()) {
    re.lastIndex = 0;
    for (let m = re.exec(text); m; m = re.exec(text)) {
      if (m[0].length === 0) {
        re.lastIndex++;
        continue;
      }
      found.push({ start: m.index, end: m.index + m[0].length, sgr });
      if (found.length > 400) break;
    }
  }
  // Keep the earliest match where two overlap; on a tie the first rule wins.
  found.sort((a, b) => a.start - b.start);
  const out: Match[] = [];
  for (const m of found) if (!out.length || m.start >= out[out.length - 1].end) out.push(m);
  return out;
}

/** Errors and warnings in some lines, by the built-in rules, whatever the settings say. */
export function countProblems(lines: string[]): { errors: number; warnings: number } {
  const err = new RegExp(ERRORS.source);
  const warn = new RegExp(WARNINGS.source);
  let errors = 0;
  let warnings = 0;
  for (const l of lines) {
    if (err.test(l)) errors++;
    else if (warn.test(l)) warnings++;
  }
  return { errors, warnings };
}

/** "error" or "warning" when a line would be colored as one. */
export function lineTone(line: string): "error" | "warning" | null {
  if (new RegExp(ERRORS.source).test(line)) return "error";
  if (new RegExp(WARNINGS.source).test(line)) return "warning";
  return null;
}

/** How many characters at the end of `text` might be the start of a highlighted word. */
export function highlightTail(text: string): number {
  if (!highlightOn()) return 0;
  const lower = text.toLowerCase();
  let hold = 0;
  for (const w of STARTS) {
    const lw = w.toLowerCase();
    for (let k = Math.min(lw.length - 1, lower.length); k >= 3; k--) {
      if (lw.startsWith(lower.slice(-k))) {
        hold = Math.max(hold, k);
        break;
      }
    }
  }
  return hold;
}
