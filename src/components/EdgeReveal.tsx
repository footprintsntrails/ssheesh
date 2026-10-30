import { useRef, useState, type ReactNode } from "react";
import { cn } from "../lib/format";

/**
 * Compact mode: a thin strip at the window's edge. Point at it and the panel
 * slides in over the panes; move away and it slides back.
 */
export function EdgeReveal({ side, label, children }: { side: "left" | "right" | "top"; label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const timer = useRef<number>(0);
  const show = () => {
    window.clearTimeout(timer.current);
    setOpen(true);
  };
  const hide = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(false), 280);
  };
  const edge = side === "left" ? "inset-y-0 left-0 w-[6px]" : side === "right" ? "inset-y-0 right-0 w-[6px]" : "inset-x-0 top-0 h-[6px]";
  const panel = side === "left" ? "inset-y-0 left-0" : side === "right" ? "inset-y-0 right-0" : "inset-x-0 top-0";
  return (
    <>
      <div aria-hidden className={cn("fixed z-40", edge)} onMouseEnter={show} />
      {open && (
        <div
          role="region"
          aria-label={label}
          onMouseEnter={show}
          onMouseLeave={hide}
          className={cn("fixed z-40 flex animate-fade-in bg-window shadow-[0_8px_40px_rgb(0_0_0/0.45)]", panel)}
        >
          {children}
        </div>
      )}
    </>
  );
}
