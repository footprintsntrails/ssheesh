import { Check, File } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../lib/format";

export interface Menu {
  x: number;
  y: number;
}

export interface MenuItem {
  label: string;
  icon: typeof File;
  run: () => void;
  hint?: string;
  danger?: boolean;
  disabled?: boolean;
  /** Shows a check mark: the item is a choice that's currently on. */
  checked?: boolean;
  iconClass?: string;
}

/** Right-click menu: frosted, keyboard-dismissable, kept inside the window. */
export function ContextMenu({ menu, items, onClose }: { menu: Menu; items: (MenuItem | "sep")[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: menu.x, y: menu.y });
  // Whatever held the keyboard gets it back: otherwise it goes away with the menu.
  const [opener] = useState(() => document.activeElement);
  useEffect(
    () => () => {
      if (document.activeElement === document.body && opener instanceof HTMLElement) opener.focus({ preventScroll: true });
    },
    [opener],
  );
  useEffect(() => {
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      setPos({
        x: Math.min(menu.x, window.innerWidth - r.width - 8),
        y: Math.min(menu.y, window.innerHeight - r.height - 8),
      });
    }
    // The keys are the menu's while it's open: arrows move through it, and
    // nothing (Esc, Enter, Delete) reaches what's behind it, such as a file list.
    // The menu itself holds them, so no row looks chosen before you choose one.
    if (!el?.contains(document.activeElement)) el?.focus({ preventScroll: true });
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent) {
        const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
        const from = at < 0 && step < 0 ? 0 : at;
        if (step && buttons.length) buttons[(from + step + buttons.length) % buttons.length].focus();
        else if (e.key === "Enter") buttons[at]?.click();
        else if (e.key !== "Escape" && e.key !== "Delete" && e.key !== "Backspace" && e.key !== "Tab") return;
        e.preventDefault();
        e.stopPropagation();
        if (!step && e.key !== "Enter") onClose();
        return;
      }
      if (e.type === "mousedown" && ref.current?.contains(e.target as Node)) return;
      onClose();
    };
    window.addEventListener("mousedown", close, true);
    window.addEventListener("keydown", close, true);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("keydown", close, true);
      window.removeEventListener("blur", close);
    };
  }, [menu, onClose]);
  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      style={{ left: pos.x, top: pos.y }}
      // Its own width, not what's left up to the window's edge: there, its rows would wrap over each other.
      className="glass fixed z-50 w-max min-w-[200px] max-w-[min(360px,calc(100vw-16px))] animate-fade-in rounded-[11px] p-1 outline-none"
    >
      {items.map((it, i) =>
        it === "sep" ? (
          <div key={i} className="mx-2 my-1 h-px bg-ink/[0.07]" />
        ) : (
          <button
            key={it.label}
            role={it.checked === undefined ? "menuitem" : "menuitemcheckbox"}
            aria-checked={it.checked}
            disabled={it.disabled}
            onClick={() => {
              onClose();
              it.run();
            }}
            className={cn(
              "flex h-7 w-full items-center gap-2.5 rounded-[7px] px-2 text-left text-[12.5px] outline-none disabled:opacity-35",
              it.danger ? "text-bad hover:bg-bad/15 focus-visible:bg-bad/15" : "text-fg hover:bg-accent/90 hover:text-on-accent focus-visible:bg-accent/90 focus-visible:text-on-accent",
            )}
          >
            <it.icon size={13.5} strokeWidth={1.8} className={cn("shrink-0", it.iconClass ?? "opacity-80")} />
            <span className="min-w-0 flex-1 truncate">{it.label}</span>
            {it.hint && <span className="text-[11px] opacity-50">{it.hint}</span>}
            {it.checked && <Check size={13} strokeWidth={2.2} className="shrink-0" />}
          </button>
        ),
      )}
    </div>
  );
}
