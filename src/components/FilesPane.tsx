import { ArrowUp, ChevronLeft, ChevronRight, Clock, CornerLeftUp, Copy, CornerDownRight, Download, Ellipsis, Eye, File, FileArchive, FileCode, FileImage, FilePlus, FileTerminal, FileText, Folder, FolderPlus, Globe, HardDrive, House, Link, Package, PanelLeft, PanelRight, Pencil, Plus, RefreshCw, RotateCcw, ScrollText, Search, Settings2, SquareTerminal, Star, Timer, Trash, Upload, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { backend } from "../lib/backend";
import { folderName, LINUX_PLACES, useBookmarks } from "../lib/bookmarks";
import { bytes, cn, joinPath, modified, parentPath, shellQuote } from "../lib/format";
import { isMac } from "../lib/keys";
import { useSettings } from "../lib/settings";
import { useStore, type PaneState } from "../lib/store";
import { targetKey, toOpError, type DirListing, type FileEntry, type OpError, type TargetRef, type TextFile } from "../lib/types";
import { ContextMenu, type Menu, type MenuItem } from "./ContextMenu";
import { Button, IconButton, ResizeHandle, Spinner } from "./ui";
import { useNames } from "../lib/privacy";
import { findTerminal } from "../lib/terminals";
import { pathCrumbs } from "../lib/paths";
import { uploadTo } from "../lib/upload";
import { track } from "../lib/transfers";

type SortKey = "name" | "size" | "modified";

interface Preview {
  name: string;
  path: string;
  file: TextFile | null;
  draft: string;
  saving: boolean;
}


const CODE = /\.(js|mjs|cjs|ts|tsx|jsx|py|rb|go|rs|java|kt|c|h|cc|cpp|hpp|php|json|ya?ml|toml|ini|conf|cfg|env|sql|html?|css|scss|lua|swift|vue|svelte|xml|dockerfile)$/i;
const TEXT = /\.(md|txt|log|csv|rst)$/i;
const IMAGE = /\.(png|jpe?g|gif|webp|svg|ico|bmp|avif|heic)$/i;
const ARCHIVE = /\.(zip|tar|gz|tgz|bz2|xz|7z|rar|zst|deb|rpm)$/i;
const SCRIPT = /\.(sh|bash|zsh|fish)$/i;

function EntryIcon({ entry }: { entry: FileEntry }) {
  const common = { size: 15, strokeWidth: 1.7, className: "shrink-0" };
  if (entry.kind === "dir")
    return <Folder {...common} className="shrink-0 text-accent" fill="currentColor" fillOpacity={0.18} />;
  if (entry.kind === "link") return <Link {...common} className="shrink-0 text-fg-3" />;
  const n = entry.name;
  if (SCRIPT.test(n)) return <FileTerminal {...common} className="shrink-0 text-ok/80" />;
  if (CODE.test(n) || n === "Dockerfile" || n.startsWith(".env")) return <FileCode {...common} className="shrink-0 text-[var(--ui-file-code)]" />;
  if (TEXT.test(n)) return <FileText {...common} className="shrink-0 text-fg-2" />;
  if (IMAGE.test(n)) return <FileImage {...common} className="shrink-0 text-[var(--ui-file-image)]" />;
  if (ARCHIVE.test(n)) return <FileArchive {...common} className="shrink-0 text-[var(--ui-file-archive)]" />;
  return <File {...common} className="shrink-0 text-fg-3" />;
}

function sortEntries(list: FileEntry[], key: SortKey, asc: boolean): FileEntry[] {
  const dir = asc ? 1 : -1;
  return list.slice().sort((a, b) => {
    const ad = a.kind === "dir" ? 0 : 1;
    const bd = b.kind === "dir" ? 0 : 1;
    if (ad !== bd) return ad - bd;
    if (key === "size" && a.size !== b.size) return (a.size - b.size) * dir;
    if (key === "modified" && a.modified !== b.modified) return (a.modified - b.modified) * dir;
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }) * (key === "name" ? dir : 1);
  });
}

/**
 * Always-visible path. Each folder in it is a button that goes there; a
 * click on the current folder or the free space after it turns the bar
 * into a text field to type or paste a path.
 */
function AddressBar({ path, onGo }: { path: string; onGo: (p: string) => void }) {
  const names = useNames();
  const [value, setValue] = useState(path);
  const [editing, setEditing] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const trail = useRef<HTMLDivElement>(null);
  const crumbs = useMemo(() => pathCrumbs(path), [path]);
  useEffect(() => {
    if (!editing) setValue(path);
  }, [path, editing]);
  // A long path shows its end: the folder you're in.
  useLayoutEffect(() => {
    const el = trail.current;
    if (el) el.scrollLeft = el.scrollWidth;
  }, [path, editing]);
  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);

  if (!editing) {
    return (
      <div
        role="navigation"
        aria-label="Folder path"
        onClick={() => setEditing(true)}
        title="Click a folder to go there. Click the empty space to type a path."
        className="flex h-7 min-w-0 flex-1 items-center rounded-[7px] bg-ink/[0.05] ring-1 ring-inset ring-ink/[0.07] hover:ring-ink/[0.12]"
      >
        <div
          ref={trail}
          onWheel={(e) => {
            if (e.deltaY) e.currentTarget.scrollLeft += e.deltaY;
          }}
          className="flex h-full min-w-0 items-center overflow-x-auto pl-1 pr-1.5 font-mono text-[12px] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          {crumbs.map((c, i) => {
            const last = i === crumbs.length - 1;
            const root = i === 0 && (c.label === "/" || c.label === "~" || c.label.endsWith(":"));
            return (
              <span key={c.path} className="flex shrink-0 items-center">
                {i > 0 && !(i === 1 && crumbs[0].label === "/") && <span className="px-px text-fg-3/60">/</span>}
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (last) setEditing(true);
                    else onGo(c.path);
                  }}
                  title={last ? "Type a path" : `Go to ${names.text(c.path)}`}
                  className={cn(
                    "flex h-[22px] items-center rounded-[5px] px-1 hover:bg-ink/[0.08]",
                    last ? "font-medium text-fg" : "text-fg-2 hover:text-fg",
                  )}
                >
                  {root && c.label === "/" ? <HardDrive size={13} className="text-fg-3" aria-label="Top of the disk" /> : root && c.label === "~" ? <House size={13} className="text-fg-3" aria-label="Home folder" /> : names.text(c.label)}
                </button>
                {i === 0 && c.label === "/" && crumbs.length > 1 && <span className="px-px text-fg-3/60">/</span>}
              </span>
            );
          })}
        </div>
        <div className="h-full min-w-3 flex-1 cursor-text" />
      </div>
    );
  }

  return (
    <label className="relative flex h-7 min-w-0 flex-1 items-center">
      <span className="sr-only">Folder path</span>
      <HardDrive size={13} className="pointer-events-none absolute left-2.5 text-fg-3" />
      <input
        ref={input}
        autoFocus
        value={value}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => setEditing(false)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            onGo(value.trim() || "~");
            e.currentTarget.blur();
          }
          if (e.key === "Escape") {
            setValue(path);
            e.currentTarget.blur();
          }
        }}
        className="h-7 w-full min-w-0 rounded-[7px] bg-ink/[0.05] pr-2.5 pl-7 font-mono text-[12px] text-fg outline-none ring-1 ring-inset ring-ink/[0.07] focus:ring-2 focus:ring-accent/60"
      />
    </label>
  );
}

function InlineName({ initial, onDone }: { initial: string; onDone: (name: string | null) => void }) {
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.focus();
    const dot = initial.lastIndexOf(".");
    el.setSelectionRange(0, dot > 0 ? dot : initial.length);
  }, [initial]);
  return (
    <input
      ref={ref}
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") onDone(value.trim() || null);
        if (e.key === "Escape") onDone(null);
      }}
      onBlur={() => onDone(value.trim() && value.trim() !== initial ? value.trim() : null)}
      spellCheck={false}
      className="h-[22px] min-w-0 flex-1 rounded-[5px] bg-canvas px-1.5 text-[13px] text-fg outline-none ring-2 ring-accent/70"
    />
  );
}

const EMPTY: string[] = [];

/** Height of a row in the list, in CSS pixels (`h-7`). */
const ROW = 28;
/** Past this many items only the rows in view (and a few around them) are drawn. */
const WINDOW_FROM = 250;

/**
 * Folders seen lately, per server. Going back, up, or into a folder the
 * pointer rested on shows its last listing at once while a fresh one loads.
 */
const listings = new Map<string, DirListing>();
const cacheKey = (tk: string, path: string) => `${tk}\n${path}`;

function remember(tk: string, asked: string, l: DirListing) {
  for (const p of new Set([asked, l.path])) {
    const k = cacheKey(tk, p);
    listings.delete(k);
    listings.set(k, l);
  }
  while (listings.size > 300) listings.delete(listings.keys().next().value as string);
}

let prefetching = false;
/** Lists a folder ahead of time, one at a time, unless it's already known. */
function prefetch(target: TargetRef, tk: string, path: string) {
  if (prefetching || listings.has(cacheKey(tk, path))) return;
  prefetching = true;
  backend
    .listDir(target, path)
    .then((l) => remember(tk, path, l))
    .catch(() => {})
    .finally(() => (prefetching = false));
}

const PLACE_ICONS: Record<string, typeof File> = { "/var/log": ScrollText, "/etc": Settings2, "/var/www": Globe, "/opt": Package, "/tmp": Timer };

function PlaceItem({
  icon: Icon,
  label,
  title,
  active,
  onClick,
  onRemove,
  iconClass,
  narrow,
  starred,
}: {
  icon: typeof File;
  label: string;
  title: string;
  active: boolean;
  onClick: () => void;
  onRemove?: () => void;
  iconClass?: string;
  narrow: boolean;
  /** A bookmark: in the narrow column it's a folder with a small star. */
  starred?: boolean;
}) {
  if (narrow) {
    return (
      <button
        type="button"
        title={title}
        onClick={onClick}
        className={cn(
          "flex w-full flex-col items-center gap-[3px] rounded-[9px] pt-[7px] pb-[5px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent/60",
          active ? "bg-accent/15 text-accent" : "text-fg-2 hover:bg-ink/[0.06] hover:text-fg",
        )}
      >
        <span className="relative">
          <Icon size={18} strokeWidth={1.9} className="shrink-0" />
          {starred && <Star size={9} strokeWidth={0} className="absolute -right-[5px] -bottom-[3px] fill-warn" />}
        </span>
        <span className="w-full truncate px-1 text-center text-[10px] font-medium leading-[12px]">{label}</span>
      </button>
    );
  }
  return (
    <div
      role="button"
      tabIndex={0}
      title={title}
      onClick={onClick}
      onKeyDown={(e) => e.key === "Enter" && onClick()}
      className={cn(
        "group flex h-7 items-center gap-2 rounded-[7px] pr-1 pl-2 text-[12.5px] outline-none",
        active ? "bg-ink/[0.09] text-fg" : "text-fg-2 hover:bg-ink/[0.045] hover:text-fg",
      )}
    >
      <Icon size={13.5} strokeWidth={1.8} className={cn("shrink-0", iconClass ?? "text-fg-3")} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {onRemove && (
        <button
          aria-label={`Remove ${label} from bookmarks`}
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
          className="hidden size-5 items-center justify-center rounded text-fg-3 hover:bg-ink/[0.08] hover:text-fg group-hover:flex"
        >
          <X size={11} />
        </button>
      )}
    </div>
  );
}

const NARROW_WIDTH = 64;
/** Dragging the wide column narrower than this folds it into the narrow one. */
const FOLD_AT = 118;

/** Each machine's home folder, looked up once, for the Home place. */
const homes = new Map<string, string>();
function useHomeFolder(target: TargetRef): string | null {
  const key = targetKey(target);
  const [home, setHome] = useState(homes.get(key) ?? null);
  useEffect(() => {
    if (homes.has(key)) return;
    let live = true;
    backend
      .listDir(target, "~")
      .then((l) => {
        homes.set(key, l.path);
        if (live) setHome(l.path);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
    // target is identified by its key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return home;
}

function Places({
  target,
  remote,
  dir,
  pins,
  recent,
  onGo,
  onUnpin,
  onForgetRecent,
  compact,
}: {
  target: TargetRef;
  remote: boolean;
  dir: string;
  pins: string[];
  recent: string[];
  onGo: (path: string) => void;
  onUnpin: (path: string) => void;
  onForgetRecent: (path: string) => void;
  /** Make room for the preview when the pane is narrow. */
  compact: boolean;
}) {
  const mode = useSettings((st) => st.placesMode);
  const right = useSettings((st) => st.placesSide === "right");
  const names = useNames();
  const wideWidth = useSettings((st) => st.placesWidth);
  const narrow = mode === "narrow";
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const width = dragWidth ?? (narrow ? NARROW_WIDTH : wideWidth);
  // Pointer moves arrive faster than renders, so the running width lives here.
  const live = useRef<number | null>(null);
  const before = useRef(wideWidth);
  const heading = narrow ? "mx-3 my-2 h-px bg-ink/[0.08]" : "px-2 pt-3 pb-1 text-[11px] font-semibold text-fg-3";
  const recentShown = recent.filter((p) => !pins.includes(p) && p !== dir).slice(0, 5);

  // While dragging, the column follows the pointer; it snaps to the narrow
  // or wide form depending on where it ends up.
  const onResize = (dx: number) => {
    const st = useSettings.getState();
    if (live.current === null) before.current = st.placesWidth;
    // On the right, the edge you drag is the left one, so dragging left widens it.
    let next = Math.round(Math.min(360, Math.max(NARROW_WIDTH, (live.current ?? width) + (right ? -dx : dx))));
    // Folding keeps the wide width from before the drag; unfolding brings it back.
    if (next < FOLD_AT && st.placesMode === "wide") st.update({ placesMode: "narrow", placesWidth: before.current });
    else if (next >= FOLD_AT && st.placesMode === "narrow") {
      next = st.placesWidth;
      st.update({ placesMode: "wide" });
    } else if (next >= FOLD_AT) st.update({ placesWidth: next });
    live.current = next;
    setDragWidth(next);
  };
  useEffect(() => {
    if (dragWidth === null) return;
    const done = () => {
      live.current = null;
      setDragWidth(null);
    };
    window.addEventListener("pointerup", done);
    return () => window.removeEventListener("pointerup", done);
  }, [dragWidth]);

  const hidden = useSettings((st) => st.hiddenPlaces);
  const home = useHomeFolder(target);
  const shown = (id: string) => !hidden.includes(id);
  const column = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [drag, setDrag] = useState<{ id: string; label: string; icon: typeof File; x: number; y: number; out: boolean } | null>(null);
  const justDragged = useRef(false);

  /** Takes something out of the column: a place hides, a bookmark or recent folder goes. */
  const takeOut = (id: string) => {
    const [kind, ...rest] = id.split(":");
    const path = rest.join(":");
    if (kind === "pin") onUnpin(path);
    else if (kind === "recent") onForgetRecent(path);
    else useSettings.getState().update({ hiddenPlaces: [...new Set([...useSettings.getState().hiddenPlaces, path])] });
  };

  // Hold an item and drag it out of the column to take it out, like the macOS Dock.
  const startDrag = (e: React.PointerEvent, id: string, label: string, icon: typeof File) => {
    if (e.button !== 0) return;
    const from = { x: e.clientX, y: e.clientY };
    let moved = false;
    const outside = (x: number, y: number) => {
      const r = column.current?.getBoundingClientRect();
      return !!r && (x < r.left - 16 || x > r.right + 16 || y < r.top - 16 || y > r.bottom + 16);
    };
    const move = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - from.x, ev.clientY - from.y) < 6) return;
      moved = true;
      setDrag({ id, label, icon, x: ev.clientX, y: ev.clientY, out: outside(ev.clientX, ev.clientY) });
    };
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setDrag(null);
      if (!moved) return;
      justDragged.current = true;
      setTimeout(() => (justDragged.current = false), 0);
      if (outside(ev.clientX, ev.clientY)) takeOut(id);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const entry = (id: string, props: Omit<Parameters<typeof PlaceItem>[0], "narrow">) => (
    <div
      key={id}
      onPointerDown={(e) => startDrag(e, id, props.label, props.icon)}
      onClickCapture={(e) => {
        if (justDragged.current) {
          e.stopPropagation();
          e.preventDefault();
        }
      }}
      className={cn(drag?.id === id && "opacity-40")}
    >
      <PlaceItem {...props} narrow={narrow} />
    </div>
  );

  const places: { id: string; label: string; icon: typeof File; path: string; title: string }[] = [
    { id: "home", label: "Home", icon: House, path: home ?? "~", title: home ? `Your home folder: ${home}` : "Your home folder" },
    // "Root" would read like the root user's folder; this is the top of the disk.
    { id: "root", label: "Disk", icon: HardDrive, path: "/", title: "/ — the top of the whole disk" },
    ...(remote ? LINUX_PLACES.map((p) => ({ id: p.path, label: p.label, icon: PLACE_ICONS[p.path] ?? Folder, path: p.path, title: p.path })) : []),
  ];
  const visiblePlaces = places.filter((p) => shown(p.id));
  const showRecent = shown("recent") && recentShown.length > 0;

  // Right-click: tick what the column shows.
  const menuItems: (MenuItem | "sep")[] = [
    ...places.map((p) => ({
      label: p.label,
      icon: p.icon,
      checked: shown(p.id),
      run: () => {
        const st = useSettings.getState();
        st.update({ hiddenPlaces: shown(p.id) ? [...st.hiddenPlaces, p.id] : st.hiddenPlaces.filter((h) => h !== p.id) });
      },
    })),
    "sep",
    {
      label: "Recent folders",
      icon: Clock,
      checked: shown("recent"),
      run: () => {
        const st = useSettings.getState();
        st.update({ hiddenPlaces: shown("recent") ? [...st.hiddenPlaces, "recent"] : st.hiddenPlaces.filter((h) => h !== "recent") });
      },
    },
    ...(hidden.length ? ["sep" as const, { label: "Show everything", icon: RotateCcw, run: () => useSettings.getState().update({ hiddenPlaces: [] }) }] : []),
  ];

  return (
    <div
      ref={column}
      style={{ width }}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      // The wide column only shows once the list beside it still has room.
      className={cn(
        "relative shrink-0 flex-col border-line",
        right ? "order-last border-l" : "border-r",
        compact ? "hidden @3xl:flex" : narrow ? "hidden @xs:flex" : "hidden @lg:flex",
      )}
    >
      <ResizeHandle
        edge={right ? "left" : "right"}
        label="Resize places"
        onResize={onResize}
        onReset={() => useSettings.getState().update({ placesMode: "wide", placesWidth: 172 })}
      />
      <div className={cn("min-h-0 flex-1 overflow-y-auto pb-2", narrow ? "flex flex-col gap-0.5 px-1.5 pt-1.5" : "px-1.5")}>
        {!narrow && visiblePlaces.length > 0 && <div className={heading}>Places</div>}
        {visiblePlaces.map((p) =>
          entry(`place:${p.id}`, { icon: p.icon, label: p.label, title: names.text(p.title), active: p.path !== "~" && dir === p.path, onClick: () => onGo(p.path) }),
        )}

        <div className={heading}>{!narrow && "Bookmarks"}</div>
        {pins.length === 0
          ? !narrow && (
              <p className="px-2 py-1 text-[11.5px] leading-snug text-fg-3">
                Star a folder <Star size={10.5} className="inline -translate-y-px" /> to keep it here.
              </p>
            )
          : pins.map((p) =>
              entry(`pin:${p}`, {
                icon: narrow ? Folder : Star,
                iconClass: narrow ? undefined : "fill-warn text-warn",
                starred: true,
                label: names.text(folderName(p)),
                title: names.text(p),
                active: dir === p,
                onClick: () => onGo(p),
                onRemove: () => onUnpin(p),
              }),
            )}

        {showRecent && (
          <>
            <div className={heading}>{!narrow && "Recent"}</div>
            {recentShown.map((p) =>
              entry(`recent:${p}`, { icon: Clock, label: names.text(folderName(p)), title: names.text(p), active: false, onClick: () => onGo(p) }),
            )}
          </>
        )}
      </div>
      {menu && <ContextMenu menu={menu} items={menuItems} onClose={() => setMenu(null)} />}
      {drag && (
        <div
          aria-hidden
          style={{ left: drag.x + 10, top: drag.y + 8 }}
          className={cn(
            "glass pointer-events-none fixed z-50 flex items-center gap-1.5 rounded-[8px] px-2 py-1 text-[12px] text-fg",
            drag.out && "ring-1 ring-bad/50",
          )}
        >
          <drag.icon size={13} />
          {drag.label}
          {drag.out && <span className="text-[11px] text-bad">· remove</span>}
        </div>
      )}
    </div>
  );
}

type Creating = { kind: "file" | "folder" } | { kind: "archive"; names: string[] } | null;

export function FilesPane({ pane, target, focused }: { pane: PaneState; target: TargetRef; focused: boolean }) {
  const setPane = useStore((s) => s.setPane);
  const toast = useStore((s) => s.toast);
  const confirm = useStore((s) => s.confirm);
  const openIn = useStore((s) => s.openIn);
  const dropActive = useStore((s) => s.dropPane === pane.id);
  const remote = target.kind === "host";
  const path = pane.path ?? "~";

  const [listing, setListing] = useState<DirListing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<OpError | null>(null);
  const [reload, setReload] = useState(0);
  const [filter, setFilter] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [sort, setSort] = useState<{ key: SortKey; asc: boolean }>({ key: "name", asc: true });
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  /** A new file, folder or archive being named in the list. */
  const [creating, setCreating] = useState<Creating>(null);
  const [toolMenu, setToolMenu] = useState<{ x: number; y: number; items: (MenuItem | "sep")[] } | null>(null);
  const [menu, setMenu] = useState<(Menu & { entry: FileEntry | null }) | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const history = useRef<string[]>([]);
  /** Folders you went back from, for Forward. */
  const future = useRef<string[]>([]);
  const skipFetch = useRef<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const tk = targetKey(target);
  const pins = useBookmarks((b) => b.pins[tk] ?? EMPTY);
  const recent = useBookmarks((b) => b.recent[tk] ?? EMPTY);
  const toggleBookmark = useBookmarks((b) => b.toggle);
  const removeBookmark = useBookmarks((b) => b.remove);
  const visit = useBookmarks((b) => b.visit);
  const placesMode = useSettings((st) => st.placesMode);
  const placesSide = useSettings((st) => st.placesSide);
  const previewWidth = useSettings((st) => st.previewWidth);

  useEffect(() => {
    if (skipFetch.current === path) {
      skipFetch.current = null;
      return;
    }
    let alive = true;
    const cached = listings.get(cacheKey(tk, path));
    if (cached) {
      setListing(cached);
      setError(null);
    }
    setLoading(true);
    backend
      .listDir(target, path)
      .then((l) => {
        remember(tk, path, l);
        if (!alive) return;
        setListing(l);
        setError(null);
        visit(tk, l.path);
        if (l.path !== path) {
          skipFetch.current = l.path;
          setPane(pane.id, { path: l.path });
        }
      })
      .catch((e) => alive && setError(toOpError(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tk, path, reload]);

  const dir = listing?.path ?? path;
  const refresh = useCallback(() => setReload((n) => n + 1), []);

  const visible = useMemo(() => {
    if (!listing) return [];
    const q = filter.trim().toLowerCase();
    const list = listing.entries.filter(
      (e) => (showHidden || !e.name.startsWith(".")) && (!q || e.name.toLowerCase().includes(q)),
    );
    return sortEntries(list, sort.key, sort.asc);
  }, [listing, filter, showHidden, sort]);

  const hiddenCount = listing ? listing.entries.filter((e) => e.name.startsWith(".")).length : 0;
  const selectedSet = useMemo(() => new Set(selected), [selected]);
  const selectedEntries = visible.filter((e) => selectedSet.has(e.name));

  // Big folders draw only the rows in view.
  const [listEl, setListEl] = useState<HTMLDivElement | null>(null);
  const [view, setView] = useState({ top: 0, height: 800 });
  useEffect(() => {
    if (!listEl) return;
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setView({ top: listEl.scrollTop, height: listEl.clientHeight }));
    };
    update();
    listEl.addEventListener("scroll", update, { passive: true });
    const ro = new ResizeObserver(update);
    ro.observe(listEl);
    return () => {
      cancelAnimationFrame(frame);
      listEl.removeEventListener("scroll", update);
      ro.disconnect();
    };
  }, [listEl]);
  /** Every folder but the top of the disk starts with "..", the folder above. */
  const upRow = !!listing && dir !== "/" && !/^[A-Za-z]:\/?$/.test(dir);
  /** Space above the first row: the list's padding, "..", and a row being named. */
  const lead = 4 + (upRow ? ROW : 0) + (creating ? ROW : 0);
  const windowed = visible.length > WINDOW_FROM;
  const first = windowed ? Math.max(0, Math.floor((view.top - lead) / ROW) - 20) : 0;
  const last = windowed ? Math.min(visible.length, Math.ceil((view.top + view.height - lead) / ROW) + 20) : visible.length;
  const scrollToRow = (i: number) => {
    const el = listRef.current;
    if (!el) return;
    const top = lead + i * ROW;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + ROW > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW - el.clientHeight;
  };

  // A file to pick out, from a Ctrl+click on its path in a terminal.
  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  useEffect(() => {
    const name = pane.reveal;
    if (!name || !listing || listing.path !== path) return;
    const entry = listing.entries.find((e) => e.name === name);
    // A listing from the cache may not have it yet; the fresh one will tell.
    if (!entry && loading) return;
    setPane(pane.id, { reveal: undefined });
    if (!entry) return;
    if (name.startsWith(".")) setShowHidden(true);
    setFilter("");
    setSelected([name]);
    setAnchor(name);
    requestAnimationFrame(() => scrollToRow(visibleRef.current.findIndex((e) => e.name === name)));
    if (entry.kind !== "dir") void openPreview(entry);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.reveal, listing, loading]);

  const navigate = (to: string, remember = true) => {
    if (to === dir) return refresh();
    if (remember) {
      history.current.push(dir);
      future.current = [];
    }
    setSelected([]);
    setAnchor(null);
    setFilter("");
    setPreview(null);
    setPane(pane.id, { path: to });
  };

  const goBack = () => {
    const prev = history.current.pop();
    if (!prev) return;
    future.current.push(dir);
    navigate(prev, false);
  };

  const goForward = () => {
    const next = future.current.pop();
    if (!next) return;
    history.current.push(dir);
    navigate(next, false);
  };

  const fail = (title: string, e: unknown) => toast({ tone: "error", title, body: toOpError(e).message });

  const openPreview = async (entry: FileEntry) => {
    const full = joinPath(dir, entry.name);
    setPreview({ name: entry.name, path: full, file: null, draft: "", saving: false });
    try {
      const file = await backend.readFile(target, full);
      setPreview((p) => (p && p.path === full ? { ...p, file, draft: file.content ?? "" } : p));
    } catch (e) {
      setPreview(null);
      fail(`Couldn't open ${entry.name}`, e);
    }
  };

  const open = (entry: FileEntry) => {
    if (entry.kind === "dir") navigate(joinPath(dir, entry.name));
    else void openPreview(entry);
  };

  const savePreview = async () => {
    if (!preview?.file) return;
    setPreview({ ...preview, saving: true });
    try {
      await backend.writeFile(target, preview.path, preview.draft);
      setPreview((p) => (p ? { ...p, saving: false, file: p.file && { ...p.file, content: p.draft } } : p));
      toast({ tone: "success", title: `Saved ${preview.name}` });
      refresh();
    } catch (e) {
      setPreview((p) => (p ? { ...p, saving: false } : p));
      fail(`Couldn't save ${preview.name}`, e);
    }
  };

  // Progress for both shows on a card in the corner, with a Stop button.
  const download = async (entries: FileEntry[]) => {
    if (!remote || entries.length === 0) return;
    for (const e of entries) {
      try {
        const saved = await track("download", e.name, (hooks) => backend.downloadPath(target, joinPath(dir, e.name), e.kind === "dir", hooks));
        toast({ tone: "success", title: `Downloaded ${e.name}`, body: saved });
      } catch (err) {
        if (toOpError(err).code === "cancelled") toast({ tone: "info", title: "Download stopped", body: e.name });
        else fail(`Couldn't download ${e.name}`, err);
      }
    }
  };

  const upload = async (paths: string[]) => {
    if (paths.length === 0) return;
    await uploadTo(target, dir, paths);
    refresh();
  };

  const remove = async (entries: FileEntry[]) => {
    if (entries.length === 0) return;
    const what = entries.length === 1 ? `“${entries[0].name}”` : `${entries.length} items`;
    const ok = await confirm({
      title: `Delete ${what}?`,
      body: remote ? "It's deleted on the server right away. This can't be undone." : "This can't be undone.",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    setBusy(`Deleting ${what}…`);
    for (const e of entries) {
      await backend.deletePath(target, joinPath(dir, e.name)).catch((err) => fail(`Couldn't delete ${e.name}`, err));
    }
    setBusy(null);
    setSelected([]);
    refresh();
  };

  const finishRename = async (from: string, to: string | null) => {
    setRenaming(null);
    if (!to || to === from) return;
    try {
      await backend.renamePath(target, joinPath(dir, from), joinPath(dir, to));
      setSelected([to]);
      refresh();
    } catch (e) {
      fail("Couldn't rename", e);
    }
  };

  const finishCreate = async (typed: string | null) => {
    const c = creating;
    setCreating(null);
    if (!typed || !c) return;
    const name = c.kind === "archive" && !/\.(tar\.gz|tgz)$/.test(typed) ? `${typed}.tar.gz` : typed;
    if (listing?.entries.some((e) => e.name === name)) {
      toast({ tone: "error", title: `“${name}” already exists here`, body: "Pick another name." });
      return;
    }
    const what = c.kind === "folder" ? "the folder" : c.kind === "file" ? "the file" : "the archive";
    try {
      if (c.kind === "archive") {
        setBusy(`Packing ${name}…`);
        await backend.archivePaths(target, dir, c.names, name);
        toast({ tone: "success", title: `Made ${name}`, body: `${c.names.length} item${c.names.length === 1 ? "" : "s"} packed` });
      } else if (c.kind === "folder") await backend.makeDir(target, joinPath(dir, name));
      else await backend.writeFile(target, joinPath(dir, name), "");
      setSelected([name]);
      refresh();
    } catch (e) {
      fail(`Couldn't create ${what}`, e);
    } finally {
      if (c.kind === "archive") setBusy(null);
    }
  };

  /** Archive the selection, or everything in this folder when nothing's selected. */
  const archiveItem = (): MenuItem => {
    const names = selectedEntries.length ? selectedEntries.map((e) => e.name) : visible.map((e) => e.name);
    return {
      label: selectedEntries.length === 1 ? `Archive “${selectedEntries[0].name}”` : selectedEntries.length ? `Archive ${names.length} items` : "Archive everything here",
      icon: FileArchive,
      disabled: names.length === 0,
      run: () => setCreating({ kind: "archive", names }),
    };
  };

  const newItems = (): (MenuItem | "sep")[] => [
    { label: "New file", icon: FilePlus, run: () => setCreating({ kind: "file" }) },
    { label: "New folder", icon: FolderPlus, run: () => setCreating({ kind: "folder" }) },
    "sep",
    archiveItem(),
  ];

  /** Everything the toolbar holds, for the ⋯ menu; narrow panes hide some buttons. */
  const moreItems = (): (MenuItem | "sep")[] => [
    { label: "Back", icon: ChevronLeft, disabled: history.current.length === 0, run: goBack },
    { label: "Forward", icon: ChevronRight, disabled: future.current.length === 0, run: goForward },
    { label: "Up one folder", icon: ArrowUp, disabled: dir === "/", run: () => navigate(parentPath(dir)) },
    "sep",
    { label: "Show hidden files", icon: Eye, checked: showHidden, run: () => setShowHidden((v) => !v) },
    "sep",
    { label: pins.includes(dir) ? "Remove bookmark" : "Bookmark this folder", icon: Star, run: () => toggleBookmark(tk, dir) },
    ...(remote ? [{ label: "Upload files…", icon: Upload, run: pickAndUpload }] : []),
    { label: "Refresh", icon: RefreshCw, run: refresh },
  ];

  const openToolMenu = (e: React.MouseEvent<HTMLButtonElement>, items: (MenuItem | "sep")[]) => {
    const r = e.currentTarget.getBoundingClientRect();
    setToolMenu({ x: r.left, y: r.bottom + 4, items });
  };

  const copyPath = (p: string) => {
    navigator.clipboard
      .writeText(p)
      .then(() => toast({ tone: "info", title: "Copied path", body: p }))
      .catch(() => toast({ tone: "error", title: "Couldn't copy", body: p }));
  };

  const terminalHere = (folder: string) =>
    openIn("terminal", target, { from: pane.id, split: true, initialCommand: `cd -- ${shellQuote(folder)} && clear` });

  /** Moves the terminal you already use on this machine into the folder. */
  const mainTerminalHere = (folder: string) => {
    const st = useStore.getState();
    const id = st.mainTerminalFor(target);
    if (!id) return terminalHere(folder);
    // Ctrl+U first clears anything half-typed on the prompt.
    backend.writeSession(id, `\x15cd -- ${shellQuote(folder)}\r`);
    st.focus(id);
    findTerminal(id)?.focus();
  };

  const pickAndUpload = async () => {
    const paths = await backend.pickFiles();
    void upload(paths);
  };

  // Files dropped from the desktop onto this pane (routed by App).
  useEffect(() => {
    const onDrop = (e: Event) => {
      const { paneId, paths } = (e as CustomEvent<{ paneId: string; paths: string[] }>).detail;
      if (paneId === pane.id) void upload(paths);
    };
    window.addEventListener("sshee:drop", onDrop);
    return () => window.removeEventListener("sshee:drop", onDrop);
  });

  const click = (e: React.MouseEvent, entry: FileEntry) => {
    const name = entry.name;
    if (e.shiftKey && anchor) {
      const a = visible.findIndex((x) => x.name === anchor);
      const b = visible.findIndex((x) => x.name === name);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      setSelected(visible.slice(lo, hi + 1).map((x) => x.name));
    } else if (isMac ? e.metaKey : e.ctrlKey) {
      setSelected((s) => (s.includes(name) ? s.filter((x) => x !== name) : [...s, name]));
      setAnchor(name);
    } else {
      setSelected([name]);
      setAnchor(name);
    }
  };

  // Resting the pointer on a folder lists it ahead of time, so opening it is instant.
  const hoverTimer = useRef(0);
  const hoverFolder = (name: string) => {
    window.clearTimeout(hoverTimer.current);
    hoverTimer.current = window.setTimeout(() => prefetch(target, tk, joinPath(dir, name)), 150);
  };

  const typeahead = useRef({ text: "", at: 0 });
  const onListKey = (e: ReactKeyboardEvent) => {
    if (renaming || creating) return;
    const mod = isMac ? e.metaKey : e.ctrlKey;
    const idx = visible.findIndex((x) => x.name === (anchor ?? selected[selected.length - 1]));
    const pick = (i: number) => {
      const entry = visible[Math.max(0, Math.min(visible.length - 1, i))];
      if (!entry) return;
      setSelected([entry.name]);
      setAnchor(entry.name);
      scrollToRow(visible.indexOf(entry));
    };
    if (e.altKey && e.key === "ArrowLeft") goBack();
    else if (e.altKey && e.key === "ArrowRight") goForward();
    else if (e.key === "ArrowDown") pick(idx + 1);
    else if (e.key === "ArrowUp" && mod) navigate(parentPath(dir));
    else if (e.key === "ArrowUp") pick(idx < 0 ? 0 : idx - 1);
    else if (e.key === "Enter" && selectedEntries.length === 1) open(selectedEntries[0]);
    else if (e.key === "Backspace" && mod) void remove(selectedEntries);
    else if (e.key === "Delete") void remove(selectedEntries);
    else if (e.key === "Backspace") navigate(parentPath(dir));
    else if (e.key === "F2" && selectedEntries.length === 1) setRenaming(selectedEntries[0].name);
    else if (e.key === "a" && mod) setSelected(visible.map((x) => x.name));
    else if (e.key === "f" && mod) filterRef.current?.focus();
    else if (e.key === "Escape") setSelected([]);
    else if (e.key.length === 1 && !mod && !e.altKey) {
      const t = typeahead.current;
      t.text = Date.now() - t.at < 700 ? t.text + e.key.toLowerCase() : e.key.toLowerCase();
      t.at = Date.now();
      const hit = visible.findIndex((x) => x.name.toLowerCase().startsWith(t.text));
      if (hit >= 0) pick(hit);
    } else return;
    e.preventDefault();
  };

  const menuItems = (entry: FileEntry | null): (MenuItem | "sep")[] => {
    if (!entry) {
      return [
        { label: "New file", icon: FilePlus, run: () => setCreating({ kind: "file" }) },
        { label: "New folder", icon: FolderPlus, run: () => setCreating({ kind: "folder" }) },
        ...(remote ? [{ label: "Upload files…", icon: Upload, run: pickAndUpload }] : []),
        { label: "Open in main terminal", icon: CornerDownRight, run: () => mainTerminalHere(dir) },
        { label: "Open new terminal here", icon: SquareTerminal, run: () => terminalHere(dir) },
        "sep",
        { label: pins.includes(dir) ? "Remove bookmark" : "Bookmark this folder", icon: Star, run: () => toggleBookmark(tk, dir) },
        { label: "Copy folder path", icon: Copy, run: () => copyPath(dir) },
        { label: "Show hidden files", icon: Eye, checked: showHidden, run: () => setShowHidden((v) => !v) },
        { label: "Refresh", icon: RefreshCw, run: refresh },
      ];
    }
    const many = selected.length > 1 && selected.includes(entry.name);
    const targets = many ? selectedEntries : [entry];
    const full = joinPath(dir, entry.name);
    return [
      ...(!many ? [{ label: entry.kind === "dir" ? "Open" : "Open preview", icon: entry.kind === "dir" ? Folder : FileText, run: () => open(entry), hint: "↵" }] : []),
      ...(remote ? [{ label: many ? `Download ${targets.length} items` : "Download", icon: Download, run: () => download(targets) }] : []),
      ...(!many
        ? [
            { label: "Rename", icon: Pencil, run: () => setRenaming(entry.name), hint: "F2" },
            { label: `Archive “${entry.name}”`, icon: FileArchive, run: () => setCreating({ kind: "archive", names: [entry.name] }) },
            { label: "Copy path", icon: Copy, run: () => copyPath(full) },
            ...(entry.kind === "dir"
              ? [{ label: pins.includes(full) ? "Remove bookmark" : "Add to bookmarks", icon: Star, run: () => toggleBookmark(tk, full) }]
              : []),
            ...(entry.kind === "dir"
              ? [
                  { label: "Open in main terminal", icon: CornerDownRight, run: () => mainTerminalHere(full) },
                  { label: "Open new terminal here", icon: SquareTerminal, run: () => terminalHere(full) },
                ]
              : []),
          ]
        : []),
      "sep",
      { label: many ? `Delete ${targets.length} items` : "Delete", icon: Trash, run: () => remove(targets), danger: true },
    ];
  };

  const headerCell = (key: SortKey, label: string, className?: string) => (
    <button
      onClick={() => setSort((s) => ({ key, asc: s.key === key ? !s.asc : key === "name" }))}
      className={cn("flex items-center gap-1 truncate hover:text-fg-2", sort.key === key && "text-fg-2", className)}
    >
      {label}
      {sort.key === key && <span className="text-[9px]">{sort.asc ? "▲" : "▼"}</span>}
    </button>
  );

  const authProblem = error && (error.code === "auth" || error.code === "hostKey");
  const dirty = preview?.file?.content != null && preview.draft !== preview.file.content;

  return (
    <div
      // The mouse's side buttons go back and forward, as in a browser.
      onMouseUp={(e) => {
        if (e.button !== 3 && e.button !== 4) return;
        e.preventDefault();
        if (e.button === 3) goBack();
        else goForward();
      }}
      className="@container absolute inset-0 flex flex-col"
    >
      {/* toolbar */}
      <div className="flex h-10 shrink-0 items-center gap-1 border-b border-line px-1.5">
        <span className="hidden @xs:flex">
          <IconButton
            label={`Places: ${placesMode}. Click for ${placesMode === "wide" ? "narrow" : placesMode === "narrow" ? "hidden" : "wide"}`}
            active={placesMode !== "hidden"}
            onClick={() =>
              useSettings.getState().update({ placesMode: placesMode === "wide" ? "narrow" : placesMode === "narrow" ? "hidden" : "wide" })
            }
            className="size-6"
          >
            {placesSide === "right" ? <PanelRight size={14} /> : <PanelLeft size={14} />}
          </IconButton>
        </span>
        <div className="mx-0.5 hidden h-4 w-px bg-ink/[0.08] @xs:block" />
        <span className="hidden items-center @xs:flex">
          <IconButton label="Back (mouse back button, Alt+←)" onClick={goBack} disabled={history.current.length === 0} className="size-6">
            <ChevronLeft size={15} />
          </IconButton>
          {future.current.length > 0 && (
            <IconButton label="Forward (mouse forward button, Alt+→)" onClick={goForward} className="size-6">
              <ChevronRight size={15} />
            </IconButton>
          )}
          <IconButton label="Up one folder" onClick={() => navigate(parentPath(dir))} disabled={dir === "/"} className="size-6">
            <ArrowUp size={14} />
          </IconButton>
        </span>
        <AddressBar path={dir} onGo={(p) => navigate(p)} />
        {/* The address bar always stays. As the pane narrows, the filter and the
            less-used buttons step out first; the ⋯ menu always has them all. */}
        <div className="relative ml-1 hidden w-[120px] shrink-0 items-center @lg:flex @3xl:w-[160px]">
          <Search size={12.5} className="pointer-events-none absolute left-2 text-fg-3" />
          <input
            ref={filterRef}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && (setFilter(""), listRef.current?.focus())}
            placeholder="Filter"
            spellCheck={false}
            className="h-7 w-full rounded-md bg-ink/[0.045] pl-7 pr-2 text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.06] placeholder:text-fg-3 focus:ring-accent/60"
          />
        </div>
        <span className="hidden items-center gap-1 @2xl:flex">
          <IconButton
            label={pins.includes(dir) ? "Remove bookmark" : "Bookmark this folder"}
            active={pins.includes(dir)}
            onClick={() => toggleBookmark(tk, dir)}
            className="size-6"
          >
            <Star size={14} className={cn(pins.includes(dir) && "fill-warn text-warn")} />
          </IconButton>
          {remote && (
            <IconButton label="Upload files" onClick={pickAndUpload} className="size-6">
              <Upload size={14} />
            </IconButton>
          )}
        </span>
        <span className="hidden @3xl:flex">
          <IconButton label="Refresh" onClick={refresh} className="size-6">
            <RefreshCw size={13} className={cn(loading && listing && "animate-spin")} />
          </IconButton>
        </span>
        <IconButton label="New file, folder or archive" onClick={(e) => openToolMenu(e, newItems())} className="size-6">
          <Plus size={15} />
        </IconButton>
        <IconButton label="More" onClick={(e) => openToolMenu(e, moreItems())} className="size-6">
          <Ellipsis size={15} />
        </IconButton>
        {toolMenu && <ContextMenu menu={toolMenu} items={toolMenu.items} onClose={() => setToolMenu(null)} />}
      </div>

      <div className="flex min-h-0 flex-1">
        {placesMode !== "hidden" && (
          <Places
            target={target}
            compact={!!preview}
            remote={remote}
            dir={dir}
            pins={pins}
            recent={recent}
            onGo={(p) => navigate(p)}
            onUnpin={(p) => removeBookmark(tk, p)}
            onForgetRecent={(p) => useBookmarks.getState().forgetRecent(tk, p)}
          />
        )}
        {/* list */}
        <div className={cn("@container relative min-w-[220px] flex-1 flex-col", preview ? "hidden @2xl:flex" : "flex")}>
          {error && !listing ? (
            <div className="flex flex-1 items-center justify-center p-6">
              <div className="max-w-[320px] text-center">
                <p className="text-[13px] font-medium text-fg">{authProblem ? "Sign in first" : "Can't open this folder"}</p>
                <p className="mt-1.5 text-[12.5px] leading-relaxed text-fg-3">{error.message}</p>
                <div className="mt-4 flex justify-center gap-2">
                  {authProblem && (
                    <Button size="sm" variant="primary" onClick={() => openIn("terminal", target, { from: pane.id, split: true })}>
                      <SquareTerminal size={13} /> Open terminal
                    </Button>
                  )}
                  <Button size="sm" onClick={refresh}>
                    Try again
                  </Button>
                </div>
              </div>
            </div>
          ) : (
            <>
              <div className="grid h-7 shrink-0 grid-cols-[minmax(0,1fr)_68px] items-center gap-3 border-b border-line pl-4 pr-3 text-[11px] font-medium text-fg-3 @md:grid-cols-[minmax(0,1fr)_68px_118px] @2xl:grid-cols-[minmax(0,1fr)_68px_90px_118px]">
                {headerCell("name", "Name")}
                {headerCell("size", "Size", "justify-end")}
                <span className="hidden @2xl:block">Permissions</span>
                {headerCell("modified", "Modified", "hidden @md:flex")}
              </div>
              {loading && listing && (
                <div className="pointer-events-none absolute inset-x-0 top-7 z-10 h-px animate-pulse bg-accent/70" />
              )}
              <div
                ref={(el) => {
                  listRef.current = el;
                  setListEl(el);
                }}
                tabIndex={0}
                onKeyDown={onListKey}
                onClick={(e) => e.target === e.currentTarget && setSelected([])}
                onContextMenu={(e) => {
                  if (e.target !== e.currentTarget) return;
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, entry: null });
                }}
                className="min-h-0 flex-1 overflow-y-auto py-1 outline-none"
              >
                {!listing &&
                  Array.from({ length: 9 }, (_, i) => (
                    <div key={i} className="flex h-7 items-center gap-2.5 px-4">
                      <div className="size-3.5 rounded bg-ink/[0.06]" />
                      <div className="h-2.5 animate-pulse rounded bg-ink/[0.06]" style={{ width: `${30 + ((i * 37) % 45)}%` }} />
                    </div>
                  ))}

                {upRow && (
                  <div
                    onDoubleClick={() => navigate(parentPath(dir))}
                    onClick={() => setSelected([])}
                    title="Double-click to go to the folder above"
                    className="mx-1.5 flex h-7 items-center gap-2.5 rounded-md px-2.5 text-[13px] text-fg-2 hover:bg-ink/[0.035]"
                  >
                    <CornerLeftUp size={15} strokeWidth={1.7} className="text-fg-3" />
                    ..
                  </div>
                )}
                {creating && (
                  <div className="mx-1.5 flex h-7 items-center gap-2.5 rounded-md bg-ink/[0.04] px-2.5">
                    {creating.kind === "folder" ? (
                      <Folder size={15} strokeWidth={1.7} className="text-accent" fill="currentColor" fillOpacity={0.18} />
                    ) : creating.kind === "file" ? (
                      <FileText size={15} strokeWidth={1.7} className="text-fg-3" />
                    ) : (
                      <FileArchive size={15} strokeWidth={1.7} className="text-[var(--ui-file-archive)]" />
                    )}
                    <InlineName
                      initial={
                        creating.kind === "archive"
                          ? `${creating.names.length === 1 ? creating.names[0] : folderName(dir) || "archive"}.tar.gz`
                          : creating.kind === "folder"
                            ? "New folder"
                            : "untitled.txt"
                      }
                      onDone={finishCreate}
                    />
                  </div>
                )}

                {first > 0 && <div style={{ height: first * ROW }} />}
                {visible.slice(first, last).map((entry) => {
                  const isSel = selectedSet.has(entry.name);
                  return (
                    <div
                      key={entry.name}
                      data-name={entry.name}
                      onPointerEnter={entry.kind === "dir" ? () => hoverFolder(entry.name) : undefined}
                      onPointerLeave={entry.kind === "dir" ? () => window.clearTimeout(hoverTimer.current) : undefined}
                      onClick={(e) => click(e, entry)}
                      onDoubleClick={() => open(entry)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        if (!selected.includes(entry.name)) {
                          setSelected([entry.name]);
                          setAnchor(entry.name);
                        }
                        setMenu({ x: e.clientX, y: e.clientY, entry });
                      }}
                      className={cn(
                        "mx-1.5 grid h-7 grid-cols-[minmax(0,1fr)_68px] items-center gap-3 rounded-md px-2.5 text-[13px] @md:grid-cols-[minmax(0,1fr)_68px_118px] @2xl:grid-cols-[minmax(0,1fr)_68px_90px_118px]",
                        isSel ? (focused ? "bg-accent/85 text-white" : "bg-ink/[0.09] text-fg") : "text-fg hover:bg-ink/[0.035]",
                      )}
                    >
                      <span className="flex min-w-0 items-center gap-2.5">
                        <span className={cn(isSel && focused && "[&_svg]:text-white")}>
                          <EntryIcon entry={entry} />
                        </span>
                        {renaming === entry.name ? (
                          <InlineName initial={entry.name} onDone={(n) => finishRename(entry.name, n)} />
                        ) : (
                          <span className={cn("truncate", entry.name.startsWith(".") && !isSel && "text-fg-2")}>{entry.name}</span>
                        )}
                        {entry.isLink && entry.kind !== "link" && (
                          <span className={cn("shrink-0 text-[11px]", isSel && focused ? "text-white/70" : "text-fg-3")}>↗</span>
                        )}
                      </span>
                      <span className={cn("text-right text-[12px] tabular-nums", isSel && focused ? "text-white/80" : "text-fg-3")}>
                        {entry.kind === "dir" ? "—" : bytes(entry.size)}
                      </span>
                      <span className={cn("hidden font-mono text-[11px] @2xl:block", isSel && focused ? "text-white/70" : "text-fg-3")}>
                        {entry.mode}
                      </span>
                      <span className={cn("hidden truncate text-[12px] tabular-nums @md:block", isSel && focused ? "text-white/80" : "text-fg-3")}>
                        {modified(entry.modified)}
                      </span>
                    </div>
                  );
                })}
                {last < visible.length && <div style={{ height: (visible.length - last) * ROW }} />}

                {listing && visible.length === 0 && !creating && (
                  <div className="flex flex-col items-center px-6 pt-16 text-center">
                    <p className="text-[13px] text-fg-2">{filter ? `Nothing matches “${filter}”` : "This folder is empty"}</p>
                    {remote && !filter && <p className="mt-1 text-[12px] text-fg-3">Drop files here to upload them.</p>}
                  </div>
                )}
              </div>
            </>
          )}

          {/* status bar */}
          <div className="flex h-7 shrink-0 items-center gap-3 overflow-hidden border-t border-line px-3 text-[11.5px] whitespace-nowrap text-fg-3 tabular-nums">
            {listing && (
              <span className="truncate">
                {visible.length} {visible.length === 1 ? "item" : "items"}
                {!showHidden && hiddenCount > 0 && `, ${hiddenCount} hidden`}
              </span>
            )}
            {selectedEntries.length > 0 && (
              <span className="truncate text-fg-2">
                {selectedEntries.length} selected
                {selectedEntries.some((e) => e.kind !== "dir") &&
                  `, ${bytes(selectedEntries.filter((e) => e.kind !== "dir").reduce((a, e) => a + e.size, 0))}`}
              </span>
            )}
            <span className="flex-1" />
            {busy && (
              <span className="flex items-center gap-1.5 text-fg-2">
                <Spinner className="size-3" /> {busy}
              </span>
            )}
          </div>

          {dropActive && (
            <div className="pointer-events-none absolute inset-1.5 z-20 flex items-center justify-center rounded-[10px] border-2 border-dashed border-accent/60 bg-accent/[0.07]">
              <div className="glass rounded-xl px-4 py-3 text-center">
                <Upload size={18} className="mx-auto text-accent" />
                <p className="mt-1.5 text-[13px] font-medium text-fg">{remote ? "Drop to upload here" : "Drop to copy here"}</p>
                <p className="mt-0.5 max-w-[260px] truncate font-mono text-[11px] text-fg-3">{dir}</p>
              </div>
            </div>
          )}
        </div>

        {/* preview / editor */}
        {preview && (
          // Beside the list when there's room; in a narrow pane it takes the whole width.
          <div style={{ width: previewWidth }} className="relative flex min-w-[220px] flex-1 flex-col border-l border-line bg-ink/[0.012] @2xl:max-w-[70%] @2xl:flex-none">
            <ResizeHandle
              edge="left"
              label="Resize preview"
              onResize={(dx) => {
                const w = useSettings.getState().previewWidth - dx;
                useSettings.getState().update({ previewWidth: Math.round(Math.min(900, Math.max(220, w))) });
              }}
              onReset={() => useSettings.getState().update({ previewWidth: 420 })}
            />
            <div className="flex h-9 shrink-0 items-center gap-2 border-b border-line pl-3 pr-1.5">
              <span className="truncate text-[12.5px] font-medium text-fg">{preview.name}</span>
              {dirty && <span className="size-1.5 shrink-0 rounded-full bg-accent" title="Unsaved changes" />}
              {preview.file && <span className="shrink-0 text-[11px] text-fg-3">{bytes(preview.file.size)}</span>}
              <span className="flex-1" />
              {dirty && (
                <Button size="sm" variant="primary" disabled={preview.saving} onClick={savePreview} className="h-6 px-2">
                  {preview.saving ? <Spinner className="size-3" /> : "Save"}
                </Button>
              )}
              {remote && (
                <IconButton
                  label="Download"
                  className="size-6"
                  onClick={() => download([{ name: preview.name, kind: "file", isLink: false, size: 0, modified: 0, mode: "" }])}
                >
                  <Download size={13.5} />
                </IconButton>
              )}
              <IconButton
                label="Close preview"
                className="size-6"
                onClick={async () => {
                  if (dirty && !(await confirm({ title: "Discard your changes?", confirmLabel: "Discard", danger: true }))) return;
                  setPreview(null);
                }}
              >
                <X size={14} />
              </IconButton>
            </div>
            {!preview.file ? (
              <div className="flex flex-1 items-center justify-center text-fg-3">
                <Spinner />
              </div>
            ) : preview.file.content == null ? (
              <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
                <p className="text-[12.5px] text-fg-2">
                  {preview.file.binary ? "This is a binary file." : "This file is too big to edit here (over 2 MB)."}
                </p>
                {remote && (
                  <Button
                    size="sm"
                    onClick={() => download([{ name: preview.name, kind: "file", isLink: false, size: 0, modified: 0, mode: "" }])}
                  >
                    <Download size={13} /> Download
                  </Button>
                )}
              </div>
            ) : (
              <textarea
                value={preview.draft}
                spellCheck={false}
                autoCorrect="off"
                autoCapitalize="off"
                wrap="off"
                onChange={(e) => setPreview({ ...preview, draft: e.target.value })}
                onKeyDown={(e) => {
                  if ((isMac ? e.metaKey : e.ctrlKey) && e.key.toLowerCase() === "s") {
                    e.preventDefault();
                    void savePreview();
                  }
                }}
                className="min-h-0 flex-1 resize-none bg-transparent p-3 font-mono text-[12px] leading-[1.6] text-fg-2 outline-none focus:text-fg"
              />
            )}
          </div>
        )}
      </div>

      {menu && <ContextMenu menu={menu} items={menuItems(menu.entry)} onClose={() => setMenu(null)} />}
    </div>
  );
}
