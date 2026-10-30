// Icon and color for a server: a grid of icons and a row of colors.

import { useEffect, useRef } from "react";
import { cn } from "../lib/format";
import { HOST_COLORS, HOST_ICONS, type HostLook } from "../lib/labels";

export function LookGrid({ look, onChange }: { look: HostLook; onChange: (look: HostLook) => void }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="grid grid-cols-6 gap-1">
        {Object.entries(HOST_ICONS).map(([id, { icon: I, name }]) => (
          <button
            key={id}
            type="button"
            title={name}
            aria-label={name}
            aria-pressed={look.icon === id}
            onClick={() => onChange({ ...look, icon: id })}
            className={cn("flex size-8 items-center justify-center rounded-[7px] hover:bg-ink/[0.08]", look.icon === id && "bg-ink/[0.12]")}
          >
            <I size={16} strokeWidth={1.9} style={{ color: look.color }} />
          </button>
        ))}
      </div>
      <div className="flex justify-between px-0.5" role="radiogroup" aria-label="Color">
        {HOST_COLORS.map((c) => (
          <button
            key={c}
            type="button"
            role="radio"
            aria-checked={look.color === c}
            aria-label={`Color ${c}`}
            onClick={() => onChange({ ...look, color: c })}
            className={cn("size-[16px] rounded-full", look.color === c && "ring-2 ring-fg/80 ring-offset-2 ring-offset-[var(--ui-raised)]")}
            style={{ background: c }}
          />
        ))}
      </div>
    </div>
  );
}

/** Floating picker at a point, closed by clicking elsewhere or Escape. */
export function LookPopover({
  at,
  look,
  onChange,
  onClose,
}: {
  at: { x: number; y: number };
  look: HostLook;
  onChange: (look: HostLook) => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const away = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onClose();
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", away, true);
    window.addEventListener("keydown", esc, true);
    return () => {
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("keydown", esc, true);
    };
  }, [onClose]);
  return (
    <div
      ref={ref}
      role="dialog"
      aria-label="Icon and color"
      style={{ left: Math.min(at.x, window.innerWidth - 252), top: Math.min(at.y, window.innerHeight - 190) }}
      className="glass fixed z-50 w-[236px] animate-pop-in rounded-[12px] p-2"
    >
      <LookGrid look={look} onChange={onChange} />
    </div>
  );
}
