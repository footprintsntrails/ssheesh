// A look inside a folder from the ≫ beside it, without going there.

import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { dirCache } from "../lib/dirCache";
import { bytes, cn } from "../lib/format";
import { targetKey, toOpError, type FileEntry, type TargetRef } from "../lib/types";
import { Spinner } from "./ui";

const WIDTH = 300;
const MAX_HEIGHT = 380;
/** Enough to see what's there; a folder of 10,000 files is better opened. */
const SHOWN = 200;

/**
 * What's in `path`, in a list beside the ≫ that opened it (`at`, the top
 * left of the list). A click on a file or folder calls `onOpen`. A click
 * elsewhere closes it, except on another ≫ (`[data-peek-arrow]`), which
 * switches or closes it by itself.
 */
export function FolderPeek({
  target,
  path,
  at,
  showHidden,
  sorted,
  icon,
  onOpen,
  onClose,
}: {
  target: TargetRef;
  path: string;
  at: { x: number; y: number };
  showHidden: boolean;
  sorted: (entries: FileEntry[]) => FileEntry[];
  icon: (entry: FileEntry) => ReactNode;
  onOpen: (entry: FileEntry | null) => void;
  /** `refocus`: the keyboard closed it, so the keyboard goes back to the list. */
  onClose: (refocus: boolean) => void;
}) {
  const id = useId();
  const box = useRef<HTMLDivElement>(null);
  const tk = targetKey(target);
  const [entries, setEntries] = useState<FileEntry[] | null>(dirCache.get(tk, path)?.listing.entries ?? null);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState(-1);
  const [top, setTop] = useState(at.y);

  useEffect(() => {
    let live = true;
    dirCache
      .load(target, path)
      .then((l) => live && setEntries(l.entries))
      .catch((e) => live && setError(toOpError(e).message));
    return () => {
      live = false;
    };
    // target is identified by its key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tk, path]);

  const all = entries ? sorted(showHidden ? entries : entries.filter((e) => !e.name.startsWith("."))) : [];
  const rows = all.slice(0, SHOWN);

  // Stays on screen: moves up as far as a long list needs.
  useLayoutEffect(() => {
    const h = box.current?.offsetHeight ?? 0;
    setTop(Math.max(8, Math.min(at.y, window.innerHeight - h - 8)));
  }, [at.y, rows.length, error]);

  useEffect(() => box.current?.focus({ preventScroll: true }), []);

  useEffect(() => {
    const away = (e: MouseEvent) => {
      const t = e.target as Element | null;
      if (!box.current?.contains(t) && !t?.closest?.("[data-peek-arrow]")) onClose(false);
    };
    window.addEventListener("mousedown", away, true);
    return () => window.removeEventListener("mousedown", away, true);
  }, [onClose]);

  const onKey = (e: ReactKeyboardEvent) => {
    const go = (i: number) => {
      const next = (i + rows.length) % rows.length;
      setActive(next);
      document.getElementById(`${id}-${next}`)?.scrollIntoView({ block: "nearest" });
    };
    if (e.key === "Escape" || e.key === "ArrowLeft") onClose(true);
    else if (e.key === "ArrowDown" && rows.length) go(active + 1);
    else if (e.key === "ArrowUp" && rows.length) go(active < 0 ? rows.length - 1 : active - 1);
    else if (e.key === "Enter" && rows[active]) onOpen(rows[active]);
    else if (e.key === "Tab") onClose(true);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const left = Math.max(8, Math.min(at.x, window.innerWidth - WIDTH - 8));
  return (
    <div
      ref={box}
      role="menu"
      aria-label={`What's in ${path}`}
      aria-activedescendant={active >= 0 ? `${id}-${active}` : undefined}
      tabIndex={-1}
      onKeyDown={onKey}
      onContextMenu={(e) => e.preventDefault()}
      style={{ left, top, width: WIDTH, maxHeight: Math.min(MAX_HEIGHT, window.innerHeight - 16) }}
      // Solid, not frosted: the list under it would show through the text.
      className="fixed z-50 flex max-w-[calc(100vw-16px)] animate-pop-in flex-col overflow-y-auto overscroll-contain rounded-[11px] border border-ink/[0.1] bg-raised p-1 shadow-[0_24px_64px_-18px_rgb(0_0_0/0.55),0_2px_8px_rgb(0_0_0/0.2),inset_0_1px_0_rgb(255_255_255/0.07)] outline-none"
    >
      {error ? (
        <p className="px-2.5 py-2 text-[12px] leading-snug text-fg-2">{error}</p>
      ) : !entries ? (
        <div className="flex h-8 items-center gap-2 px-2.5 text-[12px] text-fg-3">
          <Spinner /> Looking inside…
        </div>
      ) : rows.length === 0 ? (
        <p className="px-2.5 py-2 text-[12px] text-fg-3">This folder is empty</p>
      ) : (
        rows.map((entry, i) => (
          <button
            key={entry.name}
            id={`${id}-${i}`}
            type="button"
            role="menuitem"
            tabIndex={-1}
            title={entry.name}
            onPointerEnter={() => setActive(i)}
            onClick={() => onOpen(entry)}
            className={cn("flex h-7 w-full shrink-0 items-center gap-2 rounded-[7px] px-2 text-left text-[13px] text-fg", i === active && "bg-ink/[0.08]")}
          >
            {icon(entry)}
            <span className={cn("min-w-0 flex-1 truncate", entry.name.startsWith(".") && "text-fg-2")}>{entry.name}</span>
            {entry.kind !== "dir" && <span className="shrink-0 text-[11px] text-fg-3 tabular-nums">{bytes(entry.size)}</span>}
          </button>
        ))
      )}
      {all.length > rows.length && (
        <button type="button" role="menuitem" tabIndex={-1} onClick={() => onOpen(null)} className="h-7 w-full shrink-0 rounded-[7px] px-2 text-left text-[12px] text-fg-3 hover:bg-ink/[0.08] hover:text-fg">
          And {all.length - rows.length} more: open the folder
        </button>
      )}
    </div>
  );
}
