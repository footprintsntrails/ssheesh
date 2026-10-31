// The file manager's Bookmarks button: places, bookmarks and recent folders
// in a popover under it, instead of a column that's always there.

import { Bookmark, BookmarkPlus, Clock, EyeOff, Folder, Globe, HardDrive, House, Package, RotateCcw, ScrollText, Settings2, Star, Timer, X } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";
import { folderName, LINUX_PLACES, useBookmarks } from "../lib/bookmarks";
import { dirCache } from "../lib/dirCache";
import { cn } from "../lib/format";
import { useNames } from "../lib/privacy";
import { useSettings } from "../lib/settings";
import { targetKey, type TargetRef } from "../lib/types";
import { ContextMenu, type Menu, type MenuItem } from "./ContextMenu";

const EMPTY: string[] = [];
const PLACE_ICONS: Record<string, typeof Folder> = { "/var/log": ScrollText, "/etc": Settings2, "/var/www": Globe, "/opt": Package, "/tmp": Timer };
/** The popover's width and its tallest height, in CSS pixels. */
const WIDTH = 240;
const MAX_HEIGHT = 440;

/**
 * Each machine's home folder, looked up once, for the Home place. The
 * lookup lists it, and that listing waits in the folder cache for a click.
 */
const homes = new Map<string, string>();
function useHomeFolder(target: TargetRef): string | null {
  const key = targetKey(target);
  const [home, setHome] = useState(homes.get(key) ?? dirCache.get(key, "~")?.listing.path ?? null);
  useEffect(() => {
    if (homes.has(key)) return;
    let live = true;
    dirCache
      .load(target, "~")
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

interface Row {
  key: string;
  label: string;
  /** Where it goes, to list ahead of time while it's highlighted. */
  path?: string;
  title: string;
  icon: typeof Folder;
  /** Dim text on the right. */
  hint?: string;
  /** A bookmark: a yellow star, and an × (or Delete) to remove it. */
  pin?: boolean;
  /** The folder you're in. */
  here?: boolean;
  run: () => void;
  /** What a right-click on this row offers first: hide the place, drop the bookmark or the recent folder. */
  takeOut?: MenuItem;
}

/**
 * The Bookmarks button. It opens Places (Home, the disk, well-known Linux
 * folders), your bookmarks and recent folders under it; picking one goes there.
 */
export function PlacesButton({ target, dir, onGo }: { target: TargetRef; dir: string; onGo: (path: string) => void }) {
  const button = useRef<HTMLButtonElement>(null);
  const id = useId();
  /** Where the popover hangs, and whether the keyboard opened it. */
  const [open, setOpen] = useState<{ x: number; y: number; byKey: boolean } | null>(null);
  const isOpen = open !== null;

  const under = () => {
    const r = button.current?.getBoundingClientRect();
    return r ? { x: r.left, y: r.bottom + 4 } : null;
  };
  const close = useCallback((refocus: boolean) => {
    setOpen(null);
    if (refocus) button.current?.focus();
  }, []);

  // It stays under the button when the window changes size.
  useEffect(() => {
    if (!isOpen) return;
    const follow = () => setOpen((o) => o && { ...o, ...under() });
    window.addEventListener("resize", follow);
    return () => window.removeEventListener("resize", follow);
  }, [isOpen]);

  return (
    <>
      <button
        ref={button}
        type="button"
        aria-label="Bookmarks"
        data-hint="bookmarks"
        aria-haspopup="menu"
        aria-expanded={isOpen}
        aria-controls={isOpen ? id : undefined}
        title="Bookmarks: go to a place, a bookmarked folder or a recent one"
        onClick={(e) => {
          if (open) return close(false);
          const at = under();
          // A click from Enter or Space has no click count.
          if (at) setOpen({ ...at, byKey: e.detail === 0 });
        }}
        className={cn(
          "inline-flex size-6 shrink-0 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-ink/[0.07] hover:text-fg",
          isOpen && "bg-ink/[0.08] text-fg",
        )}
      >
        <Bookmark size={14} />
      </button>
      {open && (
        <PlacesPopover
          id={id}
          at={open}
          anchor={button}
          target={target}
          dir={dir}
          onGo={(p) => {
            close(false);
            onGo(p);
          }}
          onClose={close}
        />
      )}
    </>
  );
}

/**
 * The list itself, as a menu: arrows and Enter pick, Delete removes a
 * bookmark, Escape or a click elsewhere closes it. Right-click to choose
 * which places it shows.
 */
function PlacesPopover({
  id,
  at,
  anchor,
  target,
  dir,
  onGo,
  onClose,
}: {
  id: string;
  at: { x: number; y: number; byKey: boolean };
  anchor: RefObject<HTMLButtonElement | null>;
  target: TargetRef;
  dir: string;
  onGo: (path: string) => void;
  onClose: (refocus: boolean) => void;
}) {
  const names = useNames();
  const tk = targetKey(target);
  const remote = target.kind === "host";
  const pins = useBookmarks((b) => b.pins[tk] ?? EMPTY);
  const recent = useBookmarks((b) => b.recent[tk] ?? EMPTY);
  const hidden = useSettings((st) => st.hiddenPlaces);
  const home = useHomeFolder(target);
  const outer = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);
  // Opened from the keyboard, the first row is ready for Enter.
  const [active, setActive] = useState(at.byKey ? 0 : -1);
  const [menu, setMenu] = useState<(Menu & { row: Row | null }) | null>(null);
  /** Set when the keyboard moved the highlight, so it scrolls into view. */
  const keyed = useRef(false);

  const shown = (place: string) => !hidden.includes(place);
  const toggleShown = (place: string) => {
    const st = useSettings.getState();
    st.update({ hiddenPlaces: st.hiddenPlaces.includes(place) ? st.hiddenPlaces.filter((h) => h !== place) : [...st.hiddenPlaces, place] });
  };

  const places: { id: string; label: string; icon: typeof Folder; path: string; title: string }[] = [
    { id: "home", label: "Home", icon: House, path: home ?? "~", title: home ? `Your home folder: ${home}` : "Your home folder" },
    // "Root" would read like the root user's folder; this is the top of the disk.
    { id: "root", label: "Disk", icon: HardDrive, path: "/", title: "/ — the top of the whole disk" },
    ...(remote ? LINUX_PLACES.map((p) => ({ id: p.path, label: p.label, icon: PLACE_ICONS[p.path] ?? Folder, path: p.path, title: p.path })) : []),
  ];
  const recentShown = recent.filter((p) => !pins.includes(p) && p !== dir).slice(0, 5);

  const sections: { id: string; heading: string; rows: Row[] }[] = [];
  const placeRows = places
    .filter((p) => shown(p.id))
    .map<Row>((p) => ({
      key: `place:${p.id}`,
      label: p.label,
      icon: p.icon,
      title: names.text(p.title),
      path: p.path,
      here: p.path !== "~" && dir === p.path,
      run: () => onGo(p.path),
      takeOut: { label: `Hide ${p.label}`, icon: EyeOff, run: () => toggleShown(p.id) },
    }));
  if (placeRows.length) sections.push({ id: "places", heading: "Places", rows: placeRows });
  sections.push({
    id: "bookmarks",
    heading: "Bookmarks",
    rows: [
      ...pins.map<Row>((p) => ({
        key: `pin:${p}`,
        label: names.text(folderName(p)),
        icon: Star,
        title: names.text(p),
        path: p,
        pin: true,
        here: dir === p,
        run: () => onGo(p),
        takeOut: { label: "Remove bookmark", icon: X, run: () => useBookmarks.getState().remove(tk, p) },
      })),
      // Bookmarking keeps the popover open, so the new bookmark shows up in it.
      ...(pins.includes(dir)
        ? []
        : [{ key: "add", label: "Bookmark this folder", icon: BookmarkPlus, title: names.text(dir), hint: names.text(folderName(dir)), run: () => useBookmarks.getState().toggle(tk, dir) }]),
    ],
  });
  if (shown("recent") && recentShown.length > 0) {
    sections.push({
      id: "recent",
      heading: "Recent",
      rows: recentShown.map<Row>((p) => ({
        key: `recent:${p}`,
        label: names.text(folderName(p)),
        icon: Clock,
        title: names.text(p),
        path: p,
        run: () => onGo(p),
        takeOut: { label: "Remove from recent", icon: X, run: () => useBookmarks.getState().forgetRecent(tk, p) },
      })),
    });
  }
  const rows = sections.flatMap((s) => s.rows);
  const indexOf = new Map(rows.map((r, i) => [r.key, i]));
  // A row can go away under the highlight (a removed bookmark, a hidden place).
  const current = Math.min(active, rows.length - 1);

  // Right-click: the row's own way out, then tick what the popover shows.
  const menuItems = (row: Row | null): (MenuItem | "sep")[] => [
    ...(row?.takeOut ? [row.takeOut, "sep" as const] : []),
    ...places.map((p) => ({ label: p.label, icon: p.icon, checked: shown(p.id), run: () => toggleShown(p.id) })),
    "sep",
    { label: "Recent folders", icon: Clock, checked: shown("recent"), run: () => toggleShown("recent") },
    ...(hidden.length ? ["sep" as const, { label: "Show everything", icon: RotateCcw, run: () => useSettings.getState().update({ hiddenPlaces: [] }) }] : []),
  ];
  const closeMenu = useCallback(() => {
    setMenu(null);
    // A click in the menu took focus; the keys work here again.
    box.current?.focus({ preventScroll: true });
  }, []);

  useEffect(() => box.current?.focus({ preventScroll: true }), []);

  useEffect(() => {
    if (!keyed.current) return;
    keyed.current = false;
    document.getElementById(`${id}-${current}`)?.scrollIntoView({ block: "nearest" });
  }, [current, id]);

  // The highlighted place is listed ahead of time, so picking it opens at once.
  const pointed = current >= 0 ? rows[current]?.path : undefined;
  useEffect(() => {
    if (!pointed || pointed === dir) return;
    const t = window.setTimeout(() => dirCache.prefetch(target, pointed), 150);
    return () => window.clearTimeout(t);
    // target is identified by its key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pointed, dir, tk]);

  const menuOpen = useRef(false);
  menuOpen.current = menu !== null;
  // A click anywhere else closes it; the button toggles it itself. Escape
  // closes it too, unless it's closing the right-click menu over it: this
  // listener was added first, so it runs while that menu still counts as open.
  useEffect(() => {
    const away = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!outer.current?.contains(t) && !anchor.current?.contains(t)) onClose(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && !menuOpen.current && onClose(true);
    window.addEventListener("mousedown", away, true);
    window.addEventListener("keydown", esc, true);
    return () => {
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("keydown", esc, true);
    };
  }, [anchor, onClose]);

  const onKey = (e: ReactKeyboardEvent) => {
    if (menu || rows.length === 0) return;
    const go = (i: number) => {
      keyed.current = true;
      setActive((i + rows.length) % rows.length);
    };
    const row = current >= 0 ? rows[current] : undefined;
    if (e.key === "ArrowDown") go(current + 1);
    else if (e.key === "ArrowUp") go(current < 0 ? rows.length - 1 : current - 1);
    else if (e.key === "Home" || e.key === "PageUp") go(0);
    else if (e.key === "End" || e.key === "PageDown") go(rows.length - 1);
    else if ((e.key === "Enter" || e.key === " ") && row) row.run();
    else if ((e.key === "Delete" || e.key === "Backspace") && row?.pin) row.takeOut?.run();
    else if (e.key === "Tab") onClose(true);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const left = Math.max(8, Math.min(at.x, window.innerWidth - WIDTH - 8));
  const maxHeight = Math.max(120, Math.min(MAX_HEIGHT, window.innerHeight - at.y - 8));

  return (
    // The outer layer only places it: the pop-in's transform would otherwise
    // pull the right-click menu (fixed too) off the pointer.
    <div ref={outer} style={{ left, top: at.y }} className="fixed z-50">
      <div
        ref={box}
        id={id}
        role="menu"
        aria-label="Places and bookmarks"
        aria-activedescendant={current >= 0 ? `${id}-${current}` : undefined}
        tabIndex={-1}
        onKeyDown={onKey}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, row: null });
        }}
        style={{ width: WIDTH, maxHeight }}
        // Solid, not frosted: the list under it would show through the text.
        className="flex max-w-[calc(100vw-16px)] animate-pop-in flex-col rounded-[11px] border border-ink/[0.1] bg-raised shadow-[0_24px_64px_-18px_rgb(0_0_0/0.55),0_2px_8px_rgb(0_0_0/0.2),inset_0_1px_0_rgb(255_255_255/0.07)] outline-none"
      >
        <div onPointerLeave={() => !menu && setActive(-1)} className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-1">
          {sections.map((s, si) => (
            <div key={s.id} role="group" aria-labelledby={`${id}-${s.id}`}>
              <div id={`${id}-${s.id}`} className={cn("px-2 pb-1 text-[11px] font-semibold text-fg-3", si === 0 ? "pt-1" : "pt-2.5")}>
                {s.heading}
              </div>
              {s.rows.map((row) => {
                const i = indexOf.get(row.key)!;
                const hi = i === current;
                return (
                  <div
                    key={row.key}
                    id={`${id}-${i}`}
                    role="menuitem"
                    aria-current={row.here ? "location" : undefined}
                    aria-keyshortcuts={row.pin ? "Delete" : undefined}
                    title={row.title}
                    // Focus stays on the menu, so the keys keep working after a click.
                    onMouseDown={(e) => e.preventDefault()}
                    onPointerMove={() => !hi && setActive(i)}
                    onClick={row.run}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      setActive(i);
                      setMenu({ x: e.clientX, y: e.clientY, row });
                    }}
                    className={cn(
                      "flex h-7 items-center gap-2.5 rounded-[7px] pl-2 text-[12.5px]",
                      row.pin ? "pr-1" : "pr-2",
                      hi ? "bg-accent/90 text-white" : row.here ? "bg-ink/[0.07] text-fg" : "text-fg",
                    )}
                  >
                    <row.icon
                      size={13.5}
                      strokeWidth={1.8}
                      className={cn("shrink-0", row.pin ? (hi ? "fill-current" : "fill-warn text-warn") : hi ? "opacity-90" : "text-fg-3")}
                    />
                    <span className={cn("min-w-0 flex-1 truncate", row.here && "font-medium")}>{row.label}</span>
                    {row.hint && <span className={cn("max-w-[40%] shrink-0 truncate text-[11px]", hi ? "text-white/70" : "text-fg-3")}>{row.hint}</span>}
                    {row.pin && row.takeOut && (
                      <button
                        type="button"
                        tabIndex={-1}
                        aria-label={`Remove ${row.label} from bookmarks`}
                        title="Remove bookmark (Delete)"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={(e) => {
                          e.stopPropagation();
                          row.takeOut?.run();
                        }}
                        className={cn("size-5 shrink-0 items-center justify-center rounded-[5px] text-white/80 hover:bg-white/20 hover:text-white", hi ? "flex" : "hidden")}
                      >
                        <X size={12} />
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>
      {menu && <ContextMenu menu={menu} items={menuItems(menu.row)} onClose={closeMenu} />}
    </div>
  );
}
