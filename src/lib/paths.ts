// Paths and links: the Files address bar's clickable folders, and the file
// paths and web links in a terminal's output that open with a click.

export interface Crumb {
  label: string;
  /** The folder this part of the path opens. */
  path: string;
}

/** "/srv/app/logs" → "/", "srv", "app", "logs", each with the folder it opens. */
export function pathCrumbs(path: string): Crumb[] {
  const drive = /^([A-Za-z]:)(\/|$)/.exec(path);
  let root: Crumb | null = null;
  let rest = path;
  if (drive) {
    root = { label: drive[1], path: `${drive[1]}/` };
    rest = path.slice(drive[0].length);
  } else if (path === "~" || path.startsWith("~/")) {
    root = { label: "~", path: "~" };
    rest = path.slice(1);
  } else if (path.startsWith("/")) {
    root = { label: "/", path: "/" };
  }
  const crumbs = root ? [root] : [];
  let at = root?.path ?? "";
  for (const part of rest.split("/").filter(Boolean)) {
    at = at === "" ? part : at.endsWith("/") ? at + part : `${at}/${part}`;
    crumbs.push({ label: part, path: at });
  }
  return crumbs;
}

export interface TextLink {
  /** Where it sits in the text: [start, end). */
  start: number;
  end: number;
  kind: "url" | "path";
  /** The URL, or the path without a trailing ":line:column". */
  target: string;
  /** "src/main.rs:12" → 12. */
  line?: number;
}

/** Characters that end a link: spaces, quotes, brackets and separators. */
const STOP = /[\s"'`<>()[\]{}|;,]/;
/** Punctuation that ends a sentence rather than the path. */
const TRAILING = /[.:!?]+$/;

/**
 * Finds web links and file paths in a line of terminal output. A path is
 * one that starts at a folder (`/etc/hosts`, `~/notes.md`, `./run.sh`,
 * `../x`) or a relative one with a folder and a file name in it
 * (`src/main.rs:12`). Bare words with a slash ("and/or", "TCP/IP") don't
 * count, nor do lone file names: too many of those aren't files.
 */
export function findLinks(text: string): TextLink[] {
  const out: TextLink[] = [];
  let i = 0;
  while (i < text.length) {
    if (STOP.test(text[i])) {
      i++;
      continue;
    }
    let j = i;
    while (j < text.length && !STOP.test(text[j])) j++;
    const link = classify(text.slice(i, j));
    if (link) out.push({ ...link, start: i + link.start, end: i + link.end });
    i = j;
  }
  return out;
}

function classify(word: string): TextLink | null {
  if (/^https?:\/\//i.test(word)) {
    const url = word.replace(TRAILING, "");
    return url.length > 8 ? { start: 0, end: url.length, kind: "url", target: url } : null;
  }
  // "--config=/etc/x.conf", "path=/srv": the path is after the last "=".
  let start = 0;
  const eq = word.lastIndexOf("=");
  if (eq >= 0) start = eq + 1;
  if (word.startsWith("file://", start)) start += "file://".length;
  let token = word.slice(start).replace(TRAILING, "");
  let line: number | undefined;
  const pos = /^(.+?):(\d+)(?::\d+)?$/.exec(token);
  if (pos) {
    line = Number(pos[2]);
    token = pos[1];
  }
  if (!isPath(token)) return null;
  const end = start + (pos ? word.slice(start).replace(TRAILING, "").length : token.length);
  return { start, end, kind: "path", target: token, line };
}

function isPath(s: string): boolean {
  if (!/[A-Za-z0-9_]/.test(s)) return false;
  if (/^(?:~|\.{1,2})?\//.test(s)) return !s.startsWith("//");
  // Relative: a folder and then a file with an extension, or a folder ending in "/".
  if (!s.includes("/") || s.startsWith("@") || /^[a-z][a-z0-9+.-]*:/i.test(s)) return false;
  const last = s.slice(s.lastIndexOf("/") + 1);
  return s.endsWith("/") ? /^[\w.-]+(?:\/[\w.-]+)*\/$/.test(s) : /^[\w@.+-]+(?:\/[\w@.+-]+)*$/.test(s) && /\.[A-Za-z0-9]{1,8}$/.test(last);
}

/** A path from the terminal, made absolute against the terminal's folder. */
export function resolvePath(path: string, cwd: string): string {
  if (path.startsWith("/") || path === "~" || path.startsWith("~/")) return normalize(path);
  const base = cwd.replace(/\/+$/, "") || "/";
  return normalize(`${base}/${path}`);
}

/** Folds "." and ".." and repeated slashes away. */
function normalize(path: string): string {
  const home = path === "~" || path.startsWith("~/");
  const parts: string[] = [];
  for (const part of (home ? path.slice(1) : path).split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  const trail = parts.join("/");
  return home ? (trail ? `~/${trail}` : "~") : `/${trail}`;
}
