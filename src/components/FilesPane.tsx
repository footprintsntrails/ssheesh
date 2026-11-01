import { ArrowUp, ChevronLeft, ChevronRight, CornerLeftUp, Copy, CornerDownRight, Download, Ellipsis, Eye, File, FileArchive, FileCode, FileImage, FilePlus, FileTerminal, FileText, Folder, FolderPlus, HardDrive, House, Link, Pencil, Plus, RefreshCw, Search, Send, SquareTerminal, Star, Trash, Upload, X } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { backend } from "../lib/backend";
import { dirCache } from "../lib/dirCache";
import { folderName, useBookmarks } from "../lib/bookmarks";
import { bytes, cn, joinPath, modified, parentPath, shellOf, typedCd } from "../lib/format";
import { isMac } from "../lib/keys";
import { useSettings } from "../lib/settings";
import { useStore, type PaneState } from "../lib/store";
import { targetKey, toOpError, type DirListing, type FileEntry, type OpError, type TargetRef, type TextFile } from "../lib/types";
import { ContextMenu, type Menu, type MenuItem } from "./ContextMenu";
import { PlacesButton } from "./PlacesPopover";
import { Button, IconButton, ResizeHandle, Spinner } from "./ui";
import { useNames } from "../lib/privacy";
import { findTerminal } from "../lib/terminals";
import { pathCrumbs } from "../lib/paths";
import { uploadTo } from "../lib/upload";
import { openCopyToServer } from "./CopyToServer";
import { dragFiles, useFileDrag } from "../lib/fileDrag";
import { track } from "../lib/transfers";

type SortKey = "name" | "size" | "modified";

/** A file as the app reads it, with what it tells about the text in it. */
type OpenedFile = TextFile & {
  /** Not UTF-8 text: shown, but saving would change it, so it's read-only. */
  lossy?: boolean;
  /** Lines end in \r\n; `content` has plain \n, and saving puts the \r\n back. */
  crlf?: boolean;
};

interface Preview {
  name: string;
  path: string;
  file: OpenedFile | null;
  draft: string;
  saving: boolean;
  /** A Markdown file shown as its text rather than formatted. */
  source: boolean;
  /** Lines to show and mark, first and last: from a path like "notes.md:12-40". */
  lines?: [number, number];
}


/** The text view's padding and line height (p-3, 12px at 1.6), to find a line in it. */
const TEXT_PAD = 12;
const TEXT_LINE = 12 * 1.6;

const CODE = /\.(js|mjs|cjs|ts|tsx|jsx|py|rb|go|rs|java|kt|c|h|cc|cpp|hpp|php|json|ya?ml|toml|ini|conf|cfg|env|sql|html?|css|scss|lua|swift|vue|svelte|xml|dockerfile)$/i;
const TEXT = /\.(md|txt|log|csv|rst)$/i;
const IMAGE = /\.(png|jpe?g|gif|webp|svg|ico|bmp|avif|heic)$/i;
const ARCHIVE = /\.(zip|tar|gz|tgz|bz2|xz|7z|rar|zst|deb|rpm)$/i;
const SCRIPT = /\.(sh|bash|zsh|fish)$/i;
/** Files the preview shows formatted, with a switch to their text. */
const MARKDOWN = /\.(md|markdown)$/i;

// The Markdown renderer (and its parser) loads the first time a Markdown
// file is previewed, alongside the file itself, not when the app starts.
const loadMarkdownView = () => import("./MarkdownView");
const MarkdownView = lazy(() => loadMarkdownView().then((m) => ({ default: m.MarkdownView })));

function PreviewLoading() {
  return (
    <div className="flex flex-1 items-center justify-center text-fg-3">
      <Spinner />
    </div>
  );
}

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

/**
 * "file2" before "file10", case aside. One collator for every comparison:
 * `localeCompare` with options builds one each time, which made sorting a
 * folder of 1,000 files take 50 ms and one of 10,000 over half a second.
 */
const byName = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function sortEntries(list: FileEntry[], key: SortKey, asc: boolean): FileEntry[] {
  const dir = asc ? 1 : -1;
  return list.slice().sort((a, b) => {
    const ad = a.kind === "dir" ? 0 : 1;
    const bd = b.kind === "dir" ? 0 : 1;
    if (ad !== bd) return ad - bd;
    if (key === "size" && a.size !== b.size) return (a.size - b.size) * dir;
    if (key === "modified" && a.modified !== b.modified) return (a.modified - b.modified) * dir;
    return byName.compare(a.name, b.name) * (key === "name" ? dir : 1);
  });
}

/**
 * Always-visible path. Each folder in it is a button that goes there; a
 * click on the current folder or the free space after it turns the bar
 * into a text field to type or paste a path.
 */
function AddressBar({ path, onGo, onPoint }: { path: string; onGo: (p: string) => void; onPoint: (p: string | null) => void }) {
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
                  onPointerEnter={last ? undefined : () => onPoint(c.path)}
                  onPointerLeave={last ? undefined : () => onPoint(null)}
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

/** Every folder but the top of the disk has one above it. */
const upRowFor = (dir: string) => dir !== "/" && !/^[A-Za-z]:\/?$/.test(dir);

/** Height of a row in the list, in CSS pixels (`h-7`). */
const ROW = 28;
/** Past this many items only the rows in view (and 20 around them) are drawn. */
const WINDOW_FROM = 250;
/** The scroll position the drawn rows follow, rounded to this: at most 4 rows off, of the 20 drawn past each edge. */
const VIEW_STEP = 8 * ROW;

/**
 * A listing this young is shown without asking the server again: the
 * folder was just listed, most likely ahead of time because the pointer
 * rested on it. Refresh, and anything the app changes there, still ask.
 */
const FRESH_MS = 3000;

type Creating = { kind: "file" | "folder" } | { kind: "archive"; names: string[] } | null;

export function FilesPane({ pane, target, focused }: { pane: PaneState; target: TargetRef; focused: boolean }) {
  const setPane = useStore((s) => s.setPane);
  const toast = useStore((s) => s.toast);
  const confirm = useStore((s) => s.confirm);
  const openIn = useStore((s) => s.openIn);
  const dropActive = useStore((s) => s.dropPane === pane.id);
  const fileDrop = useFileDrag((s) => (s.over?.pane === pane.id ? s.over : null));
  const names = useNames();
  const remote = target.kind === "host";
  // Files go from here to any other server you have.
  const canCopyOut = useStore((s) => s.hosts.some((h) => !(target.kind === "host" && h.id === target.hostId)));
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
  // The text view's scroll, for the band over the lines a path asked for.
  const textRef = useRef<HTMLTextAreaElement>(null);
  const [textScroll, setTextScroll] = useState(0);
  // Opened at some lines: the first of them a few lines from the top.
  const linesFrom = preview?.lines?.[0];
  useLayoutEffect(() => {
    const el = textRef.current;
    if (!el || !linesFrom) return;
    el.scrollTop = Math.max(0, TEXT_PAD + (linesFrom - 4) * TEXT_LINE);
    setTextScroll(el.scrollTop);
  }, [preview?.path, preview?.file, preview?.source, linesFrom]);
  const [busy, setBusy] = useState<string | null>(null);
  const history = useRef<string[]>([]);
  /** Folders you went back from, for Forward. */
  const future = useRef<string[]>([]);
  const skipFetch = useRef<string | null>(null);
  /** Set by Refresh: list the folder again even if it was listed a moment ago. */
  const forced = useRef(false);
  const listRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const tk = targetKey(target);
  const pins = useBookmarks((b) => b.pins[tk] ?? EMPTY);
  const toggleBookmark = useBookmarks((b) => b.toggle);
  const visit = useBookmarks((b) => b.visit);
  const previewWidth = useSettings((st) => st.previewWidth);
  const dirty = preview?.file?.content != null && preview.draft !== preview.file.content;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  /** Unsaved edits in the preview go only if you say so. */
  const canDiscard = async () =>
    !dirtyRef.current || (await confirm({ title: preview ? `Discard your changes to ${preview.name}?` : "Discard your changes?", confirmLabel: "Discard", danger: true }));

  /** The folder this pane went to itself, to tell it from one picked elsewhere. */
  const moved = useRef<string | null>(null);
  const seenPath = useRef(path);
  // Another folder picked from outside (a terminal's "show in Files"): what
  // was selected, typed or previewed belonged to the one before. Edits not
  // saved yet stay; the preview knows its own file.
  useEffect(() => {
    if (seenPath.current === path) return;
    seenPath.current = path;
    const own = moved.current === path;
    moved.current = null;
    if (own) return;
    setSelected([]);
    setAnchor(null);
    setFilter("");
    setRenaming(null);
    setCreating(null);
    if (!dirtyRef.current) setPreview(null);
  }, [path]);

  useEffect(() => {
    if (skipFetch.current === path) {
      skipFetch.current = null;
      return;
    }
    let alive = true;
    const force = forced.current;
    forced.current = false;
    const show = (l: DirListing) => {
      setListing(l);
      setError(null);
      visit(tk, l.path);
      if (l.path !== path) {
        skipFetch.current = l.path;
        moved.current = l.path;
        setPane(pane.id, { path: l.path });
      }
    };
    const cached = dirCache.get(tk, path);
    setError(null);
    if (cached) setListing(cached.listing);
    // A file to pick out may be newer than a listing from a moment ago.
    if (cached && !force && !pane.reveal && Date.now() - cached.at < FRESH_MS) {
      show(cached.listing);
      setLoading(false);
      return;
    }
    setLoading(true);
    dirCache
      .load(target, path)
      .then((l) => alive && show(l))
      .catch((e) => {
        if (!alive) return;
        const err = toOpError(e);
        setError(err);
        // Offline, this folder as it was stays on screen, under a note that
        // says so. Otherwise (it's gone or closed, or the folder still on
        // screen is the one before) the error shows instead.
        const offline = err.code === "network" || err.code === "auth" || err.code === "hostKey";
        if (cached && !offline) dirCache.forget(tk, path);
        setListing((l) => (offline && l && (l === cached?.listing || l.path === path) ? l : null));
      })
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tk, path, reload]);

  const dir = listing?.path ?? path;
  const refresh = useCallback(() => {
    forced.current = true;
    setReload((n) => n + 1);
  }, []);
  /** After changing something here: every pane showing this folder lists it again, this one too. */
  const changed = () => dirCache.changed(tk, dir);
  const dirRef = useRef(dir);
  dirRef.current = dir;
  useEffect(() => dirCache.watch((k, d) => k === tk && d === dirRef.current && refresh()), [tk, refresh]);

  // The folder above is a likely next stop (Up, Backspace, ".."); list it
  // ahead of time when nothing else is going on.
  useEffect(() => {
    if (!listing || loading || !upRowFor(listing.path)) return;
    const t = window.setTimeout(() => dirCache.prefetch(target, parentPath(listing.path)), 400);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [listing, loading, tk]);

  // Sorted once per listing; typing in the filter only filters.
  const sorted = useMemo(() => (listing ? sortEntries(listing.entries, sort.key, sort.asc) : []), [listing, sort]);
  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return sorted.filter((e) => (showHidden || !e.name.startsWith(".")) && (!q || e.name.toLowerCase().includes(q)));
  }, [sorted, filter, showHidden]);

  const hiddenCount = listing ? listing.entries.filter((e) => e.name.startsWith(".")).length : 0;
  const selectedSet = useMemo(() => new Set(selected), [selected]);
  const selectedEntries = visible.filter((e) => selectedSet.has(e.name));

  // Big folders draw only the rows in view. The place scrolled to is noted
  // in steps of a few rows, well inside the rows drawn beyond the edges, so
  // scrolling draws the list again every few rows instead of every frame.
  const [listEl, setListEl] = useState<HTMLDivElement | null>(null);
  const [view, setView] = useState({ top: 0, height: 800 });
  useEffect(() => {
    if (!listEl) return;
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const top = Math.round(listEl.scrollTop / VIEW_STEP) * VIEW_STEP;
        const height = listEl.clientHeight;
        setView((v) => (v.top === top && v.height === height ? v : { top, height }));
      });
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
  // A new filter starts at the top of what it found, drawn right away, not
  // where a long list was scrolled to.
  useEffect(() => {
    if (!listEl) return;
    listEl.scrollTop = 0;
    setView((v) => (v.top === 0 ? v : { ...v, top: 0 }));
  }, [filter, listEl]);
  /** Every folder but the top of the disk starts with "..", the folder above. */
  const upRow = !!listing && upRowFor(dir);
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

  // A file to pick out, from a click on its path in a terminal.
  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  useEffect(() => {
    const name = pane.reveal;
    if (!name || !listing || listing.path !== path) return;
    const entry = listing.entries.find((e) => e.name === name);
    // A listing from the cache may not have it yet; the fresh one will tell.
    if (!entry && loading) return;
    const done = () => setPane(pane.id, { reveal: undefined, revealLines: undefined });
    if (!entry) return done();
    if (name.startsWith(".")) setShowHidden(true);
    setFilter("");
    setSelected([name]);
    setAnchor(name);
    requestAnimationFrame(() => scrollToRow(visibleRef.current.findIndex((e) => e.name === name)));
    // Asked for until the preview is open: a pane drawn again meanwhile (side
    // by side being set up) opens it then.
    if (entry.kind !== "dir") void openPreview(entry, pane.revealLines).then(done);
    else done();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.reveal, listing, loading]);

  /** Shows another folder here; `navigate` asks about unsaved edits first. */
  const go = (to: string, remember: boolean) => {
    if (remember) {
      history.current.push(dir);
      future.current = [];
    }
    setSelected([]);
    setAnchor(null);
    setFilter("");
    setPreview(null);
    moved.current = to;
    setPane(pane.id, { path: to });
  };

  const navigate = async (to: string, remember = true) => {
    if (to === dir) return refresh();
    if (await canDiscard()) go(to, remember);
  };

  const goBack = async () => {
    const prev = history.current[history.current.length - 1];
    if (!prev || !(await canDiscard())) return;
    history.current.pop();
    future.current.push(dir);
    go(prev, false);
  };

  const goForward = async () => {
    const next = future.current[future.current.length - 1];
    if (!next || !(await canDiscard())) return;
    future.current.pop();
    history.current.push(dir);
    go(next, false);
  };

  const fail = (title: string, e: unknown) => toast({ tone: "error", title, body: toOpError(e).message });

  const openPreview = async (entry: FileEntry, lines?: [number, number]) => {
    const full = joinPath(dir, entry.name);
    if (!(await canDiscard())) {
      // Kept: the list goes back to the file that stays open.
      if (preview && preview.path === joinPath(dir, preview.name)) setSelected([preview.name]);
      return;
    }
    setPreview({ name: entry.name, path: full, file: null, draft: "", saving: false, source: false, lines });
    if (MARKDOWN.test(entry.name)) void loadMarkdownView().catch(() => {});
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
    const file = preview?.file;
    // Only text that opened for editing: a binary or too big file has no
    // text to save, and one that isn't UTF-8 would change.
    if (!preview || !file || file.content == null || file.lossy || preview.saving) return;
    const { path: at, name, draft: sent } = preview;
    setPreview((p) => (p && p.path === at ? { ...p, saving: true } : p));
    try {
      // Lines go back with the endings the file had.
      await backend.writeFile(target, at, file.crlf ? sent.replace(/\r?\n/g, "\r\n") : sent);
      // What was saved is what was sent; anything typed meanwhile is still unsaved.
      setPreview((p) => (p && p.path === at ? { ...p, saving: false, file: p.file && { ...p.file, content: sent } } : p));
      toast({ tone: "success", title: `Saved ${name}` });
      dirCache.changed(tk, parentPath(at));
    } catch (e) {
      setPreview((p) => (p && p.path === at ? { ...p, saving: false } : p));
      fail(`Couldn't save ${name}`, e);
    }
  };

  // Progress for both shows on a card in the corner, with a Stop button.
  // The note that it's done opens its folder, with the file picked out.
  const download = async (items: { name: string; path: string; isDir: boolean }[]) => {
    if (!remote || items.length === 0) return;
    for (const e of items) {
      try {
        const at = await track("download", e.name, (hooks) => backend.downloadPath(target, e.path, e.isDir, hooks));
        const show = () => void backend.revealFiles([at]).catch((err) => fail("Couldn't open the folder", err));
        toast({ tone: "success", title: `Downloaded ${e.name}`, body: at, action: { label: "Show in folder", run: show } });
      } catch (err) {
        if (toOpError(err).code === "cancelled") toast({ tone: "info", title: "Download stopped", body: e.name });
        else fail(`Couldn't download ${e.name}`, err);
      }
    }
  };

  const upload = async (paths: string[]) => {
    if (paths.length === 0) return;
    await uploadTo(target, dir, paths);
    changed();
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
      dirCache.forget(tk, joinPath(dir, e.name));
    }
    setBusy(null);
    setSelected([]);
    changed();
  };

  const finishRename = async (from: string, to: string | null) => {
    setRenaming(null);
    if (!to || to === from) return;
    try {
      await backend.renamePath(target, joinPath(dir, from), joinPath(dir, to));
      dirCache.forget(tk, joinPath(dir, from));
      setSelected([to]);
      changed();
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
      else {
        // Asks the folder itself, not this listing: a file made there since
        // must not be emptied.
        const full = joinPath(dir, name);
        const there = await backend.listDir(target, parentPath(full));
        if (there.entries.some((e) => e.name === full.slice(full.lastIndexOf("/") + 1))) {
          toast({ tone: "error", title: `“${name}” already exists here`, body: "Pick another name." });
          changed();
          return;
        }
        await backend.writeFile(target, full, "");
      }
      setSelected([name]);
      changed();
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

  /** The `cd` into the folder, typed into a terminal; null, said in a toast, if its name would act as keys there. */
  const cdInto = (folder: string, next?: string) => {
    const cmd = typedCd(folder, shellOf(target), next);
    if (cmd === null) toast({ tone: "error", title: "Can't open a terminal there", body: "This folder's name has control characters in it, which a terminal would take as keystrokes." });
    return cmd;
  };

  const terminalHere = (folder: string) => {
    const cmd = cdInto(folder, "clear");
    if (cmd !== null) openIn("terminal", target, { from: pane.id, split: true, initialCommand: cmd });
  };

  /** Moves the terminal you already use on this machine into the folder. */
  const mainTerminalHere = (folder: string) => {
    const st = useStore.getState();
    const id = st.mainTerminalFor(target);
    if (!id) return terminalHere(folder);
    const cmd = cdInto(folder);
    if (cmd === null) return;
    // Ctrl+U first clears anything half-typed on the prompt.
    backend.writeSession(id, `\x15${cmd}\r`);
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

  // Resting the pointer on a folder (or "..", or a folder in the path) lists
  // it ahead of time, so opening it is instant.
  const hoverTimer = useRef(0);
  const pointAt = (folder: string | null) => {
    window.clearTimeout(hoverTimer.current);
    if (folder) hoverTimer.current = window.setTimeout(() => dirCache.prefetch(target, folder), 150);
  };
  const hoverFolder = (name: string) => pointAt(joinPath(dir, name));

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
      ...(remote ? [{ label: many ? `Download ${targets.length} items` : "Download", icon: Download, run: () => download(targets.map((t) => ({ name: t.name, path: joinPath(dir, t.name), isDir: t.kind === "dir" }))) }] : []),
      ...(canCopyOut
        ? [
            {
              label: `Copy ${many ? `${targets.length} items ` : ""}to ${remote ? "another server" : "a server"}…`,
              icon: Send,
              run: () => openCopyToServer(target, targets.map((t) => joinPath(dir, t.name))),
            },
          ]
        : []),
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
        : [{ label: `Archive ${targets.length} items`, icon: FileArchive, run: () => setCreating({ kind: "archive", names: targets.map((t) => t.name) }) }]),
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
  /** Formatted or as text; only a Markdown file that opened as text has the choice. */
  const markdown = preview?.file?.content != null && MARKDOWN.test(preview.name);

  return (
    <div
      // The mouse's side buttons go back and forward, as in a browser.
      onMouseUp={(e) => {
        if (e.button !== 3 && e.button !== 4) return;
        e.preventDefault();
        if (e.button === 3) goBack();
        else goForward();
      }}
      data-files-pane={pane.id}
      data-files-dir={dir}
      className="@container absolute inset-0 flex flex-col"
    >
      {/* toolbar */}
      <div className="flex h-10 shrink-0 items-center gap-1 border-b border-line px-1.5">
        {/* Places, bookmarks and recent folders; it stays in narrow panes too. */}
        <PlacesButton
          target={target}
          dir={dir}
          onGo={(p) => {
            navigate(p);
            listRef.current?.focus();
          }}
        />
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
        <AddressBar path={dir} onGo={(p) => navigate(p)} onPoint={pointAt} />
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
              {/* Couldn't list it again: this is the folder as it was last seen. */}
              {error && listing && (
                <div role="status" className="flex shrink-0 items-center gap-2 border-b border-line bg-warn/10 py-1 pr-1.5 pl-4 text-[12px] text-fg-2">
                  <span className="min-w-0 flex-1 truncate" title={error.message}>
                    Showing this folder as it was. {error.message}
                  </span>
                  {authProblem && (
                    <Button size="sm" variant="ghost" onClick={() => openIn("terminal", target, { from: pane.id, split: true })} className="h-6 px-2">
                      Open terminal
                    </Button>
                  )}
                  <Button size="sm" variant="ghost" onClick={refresh} className="h-6 px-2">
                    Try again
                  </Button>
                </div>
              )}
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
                    onPointerEnter={() => pointAt(parentPath(dir))}
                    onPointerLeave={() => pointAt(null)}
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
                      data-folder={entry.kind === "dir" ? joinPath(dir, entry.name) : undefined}
                      // Dragged onto another server's Files beside this one, it's copied there.
                      onPointerDown={(e) => {
                        if (renaming === entry.name || e.shiftKey || e.metaKey || e.ctrlKey) return;
                        const names = selectedSet.has(entry.name) ? selectedEntries.map((x) => x.name) : [entry.name];
                        dragFiles(e, target, names.map((n) => joinPath(dir, n)));
                      }}
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
                        isSel ? (focused ? "bg-accent/85 text-on-accent" : "bg-ink/[0.09] text-fg") : "text-fg hover:bg-ink/[0.035]",
                      )}
                    >
                      <span className="flex min-w-0 items-center gap-2.5">
                        <span className={cn(isSel && focused && "[&_svg]:text-on-accent")}>
                          <EntryIcon entry={entry} />
                        </span>
                        {renaming === entry.name ? (
                          <InlineName initial={entry.name} onDone={(n) => finishRename(entry.name, n)} />
                        ) : (
                          <span className={cn("truncate", entry.name.startsWith(".") && !isSel && "text-fg-2")}>{entry.name}</span>
                        )}
                        {entry.isLink && entry.kind !== "link" && (
                          <span className={cn("shrink-0 text-[11px]", isSel && focused ? "text-on-accent/70" : "text-fg-3")}>↗</span>
                        )}
                      </span>
                      <span className={cn("text-right text-[12px] tabular-nums", isSel && focused ? "text-on-accent/80" : "text-fg-3")}>
                        {entry.kind === "dir" ? "—" : bytes(entry.size)}
                      </span>
                      <span className={cn("hidden font-mono text-[11px] @2xl:block", isSel && focused ? "text-on-accent/70" : "text-fg-3")}>
                        {entry.mode}
                      </span>
                      <span className={cn("hidden truncate text-[12px] tabular-nums @md:block", isSel && focused ? "text-on-accent/80" : "text-fg-3")}>
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

          {fileDrop && (
            <div className="pointer-events-none absolute inset-1.5 z-20 flex items-center justify-center rounded-[10px] border-2 border-dashed border-accent/60 bg-accent/[0.07]">
              <div className="glass rounded-xl px-4 py-3 text-center">
                <Send size={18} className="mx-auto text-accent" />
                <p className="mt-1.5 text-[13px] font-medium text-fg">Drop to copy to {names.target(target)}</p>
                <p className="mt-0.5 max-w-[260px] truncate font-mono text-[11px] text-fg-3">{names.text(fileDrop.dir)}</p>
              </div>
            </div>
          )}
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
          <div
            style={{ width: previewWidth }}
            // Saving works from the formatted view too, after an edit in Source.
            onKeyDown={(e) => {
              if ((isMac ? e.metaKey : e.ctrlKey) && e.key.toLowerCase() === "s") {
                e.preventDefault();
                void savePreview();
              }
            }}
            className="relative flex min-w-[220px] flex-1 flex-col border-l border-line bg-ink/[0.012] @2xl:max-w-[70%] @2xl:flex-none"
          >
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
              {preview.lines && (
                <span className="shrink-0 text-[11px] font-medium text-accent tabular-nums">
                  {preview.lines[1] > preview.lines[0] ? `lines ${preview.lines[0]}–${preview.lines[1]}` : `line ${preview.lines[0]}`}
                </span>
              )}
              <span className="flex-1" />
              {markdown && (
                <div role="tablist" aria-label="Show the file" className="flex h-6 shrink-0 items-center rounded-[7px] bg-ink/[0.045] p-[2px] ring-1 ring-inset ring-ink/[0.06]">
                  {(["Preview", "Source"] as const).map((v) => {
                    const on = (v === "Source") === preview.source;
                    return (
                      <button
                        key={v}
                        type="button"
                        role="tab"
                        aria-selected={on}
                        title={v === "Preview" ? "Show it formatted" : "Show the text, to edit it"}
                        onClick={() => setPreview((p) => p && { ...p, source: v === "Source" })}
                        className={cn(
                          "flex h-5 items-center rounded-[5px] px-2 text-[11.5px] font-medium transition-colors",
                          on ? "bg-ink/[0.11] text-fg shadow-[0_1px_2px_rgb(0_0_0/0.35),inset_0_1px_0_rgb(255_255_255/0.07)]" : "text-fg-3 hover:text-fg-2",
                        )}
                      >
                        {v}
                      </button>
                    );
                  })}
                </div>
              )}
              {dirty && (
                <Button size="sm" variant="primary" disabled={preview.saving} onClick={savePreview} className="h-6 px-2">
                  {preview.saving ? <Spinner className="size-3" /> : "Save"}
                </Button>
              )}
              {remote && (
                <IconButton
                  label="Download"
                  className="size-6"
                  onClick={() => download([{ name: preview.name, path: preview.path, isDir: false }])}
                >
                  <Download size={13.5} />
                </IconButton>
              )}
              <IconButton
                label="Close preview"
                className="size-6"
                onClick={async () => {
                  if (await canDiscard()) setPreview(null);
                }}
              >
                <X size={14} />
              </IconButton>
            </div>
            {preview.file?.lossy && (
              <p className="shrink-0 border-b border-line bg-warn/10 px-3 py-1.5 text-[12px] leading-relaxed text-fg-2">
                Read-only: this file isn't UTF-8 text, so saving it here would change the characters it couldn't read.
              </p>
            )}
            {!preview.file ? (
              <PreviewLoading />
            ) : preview.file.content == null ? (
              <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
                <p className="text-[12.5px] text-fg-2">
                  {preview.file.binary ? "This is a binary file." : "This file is too big to edit here (over 2 MB)."}
                </p>
                {remote && (
                  <Button
                    size="sm"
                    onClick={() => download([{ name: preview.name, path: preview.path, isDir: false }])}
                  >
                    <Download size={13} /> Download
                  </Button>
                )}
              </div>
            ) : markdown && !preview.source ? (
              <Suspense fallback={<PreviewLoading />}>
                <MarkdownView text={preview.draft} lines={preview.lines} />
              </Suspense>
            ) : (
              <div className="relative flex min-h-0 flex-1">
                {preview.lines && (
                  <div
                    aria-hidden
                    className="lines-band pointer-events-none absolute inset-x-0"
                    style={{
                      top: TEXT_PAD + (preview.lines[0] - 1) * TEXT_LINE - textScroll,
                      height: (preview.lines[1] - preview.lines[0] + 1) * TEXT_LINE,
                    }}
                  />
                )}
                <textarea
                  ref={textRef}
                  value={preview.draft}
                  spellCheck={false}
                  autoCorrect="off"
                  autoCapitalize="off"
                  wrap="off"
                  readOnly={preview.file.lossy}
                  onScroll={(e) => setTextScroll(e.currentTarget.scrollTop)}
                  onChange={(e) => {
                    const draft = e.target.value;
                    // Edited, the marked lines may not be those lines anymore.
                    setPreview((p) => p && { ...p, draft, lines: undefined });
                  }}
                  className="relative min-h-0 flex-1 resize-none bg-transparent p-3 font-mono text-[12px] leading-[1.6] text-fg-2 outline-none focus:text-fg"
                />
              </div>
            )}
          </div>
        )}
      </div>

      {menu && <ContextMenu menu={menu} items={menuItems(menu.entry)} onClose={() => setMenu(null)} />}
    </div>
  );
}
