// The changelog, bundled into the app, so "What's new" works offline and
// shows exactly the notes of the versions you skipped.

import raw from "../../CHANGELOG.md?raw";

export interface Release {
  version: string;
  title: string;
  /** Markdown under the heading. */
  body: string;
}

export const RELEASES: Release[] = raw
  .split(/^## /m)
  .slice(1)
  .map((section) => {
    const [head, ...rest] = section.split("\n");
    const [version, ...title] = head.split(" — ");
    return { version: version.trim(), title: title.join(" — ").trim(), body: rest.join("\n").trim() };
  })
  .filter((r) => /^\d+\.\d+\.\d+$/.test(r.version));

/** -1, 0 or 1, comparing "0.3.2" with "0.3.10" the way people expect. */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d) return Math.sign(d);
  }
  return 0;
}

/** Releases newer than `since` up to and including `current`, newest first. */
export function releasesSince(since: string | null, current: string): Release[] {
  return RELEASES.filter((r) => compareVersions(r.version, current) <= 0 && (!since || compareVersions(r.version, since) > 0));
}
