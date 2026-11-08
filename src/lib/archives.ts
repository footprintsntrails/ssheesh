// Archives Files can unpack, told by the end of their name. The Rust side
// (`PACKED` in crates/sshee-core/src/files.rs) must match.

export type ArchiveKind = "zip" | "tar" | "squeezed" | "7z" | "rar";

const ENDINGS: [string, ArchiveKind][] = [
  [".tar.gz", "tar"],
  [".tar.bz2", "tar"],
  [".tar.xz", "tar"],
  [".tar.zst", "tar"],
  [".tgz", "tar"],
  [".tbz2", "tar"],
  [".tbz", "tar"],
  [".txz", "tar"],
  [".tzst", "tar"],
  [".tar", "tar"],
  [".zip", "zip"],
  [".gz", "squeezed"],
  [".bz2", "squeezed"],
  [".xz", "squeezed"],
  [".zst", "squeezed"],
  [".7z", "7z"],
  [".rar", "rar"],
];

/** What kind of archive `name` is, or null if Files can't unpack it. */
export function archiveKind(name: string): ArchiveKind | null {
  const lower = name.toLowerCase();
  return ENDINGS.find(([end]) => lower.length > end.length && lower.endsWith(end))?.[1] ?? null;
}

/**
 * Whether Files offers to unpack `name`: anywhere a server is, and on this
 * computer too, except that Windows (no sh there) unpacks only .zip and .tar.
 */
export function canUnpack(name: string, onThisWindows: boolean): boolean {
  const kind = archiveKind(name);
  return kind !== null && (!onThisWindows || kind === "zip" || kind === "tar");
}
