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
    el?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true });
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent) {
        const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
        if (step && buttons.length) buttons[(at + step + buttons.length) % buttons.length].focus();
        else if (e.key === "Enter" && at >= 0) buttons[at].click();
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
      style={{ left: pos.x, top: pos.y }}
      className="glass fixed z-50 min-w-[200px] animate-fade-in rounded-[11px] p-1"
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
              "flex h-7 w-full items-center gap-2.5 rounded-[7px] px-2 text-left text-[12.5px] disabled:opacity-35",
              it.danger ? "text-bad hover:bg-bad/15" : "text-fg hover:bg-accent/90 hover:text-on-accent focus-visible:bg-accent/90 focus-visible:text-on-accent",
            )}
          >
            <it.icon size={13.5} strokeWidth={1.8} className={cn("shrink-0", it.iconClass ?? "opacity-80")} />
            <span className="flex-1">{it.label}</span>
            {it.hint && <span className="text-[11px] opacity-50">{it.hint}</span>}
            {it.checked && <Check size={13} strokeWidth={2.2} className="shrink-0" />}
          </button>
        ),
      )}
    </div>
  );
}
