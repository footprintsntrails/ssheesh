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
  // The top strip sits right above the panes' title bars: wait a beat, so
  // reaching for a pane's buttons doesn't pop the toolbar over them.
  const showSoon = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(true), 220);
  };
  const hide = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(false), 280);
  };
  const edge = side === "left" ? "inset-y-0 left-0 w-[6px]" : side === "right" ? "inset-y-0 right-0 w-[6px]" : "inset-x-0 top-0 h-[4px]";
  const panel = side === "left" ? "inset-y-0 left-0 border-r" : side === "right" ? "inset-y-0 right-0 border-l" : "inset-x-0 top-0 border-b";
  return (
    <>
      <div aria-hidden className={cn("fixed z-40", edge)} onMouseEnter={side === "top" ? showSoon : show} onMouseLeave={side === "top" && !open ? hide : undefined} />
      {open && (
        <div
          role="region"
          aria-label={label}
          onMouseEnter={show}
          onMouseLeave={hide}
          className={cn("fixed z-40 flex animate-fade-in border-line bg-window shadow-[0_8px_40px_rgb(0_0_0/0.45)]", panel)}
        >
          {children}
        </div>
      )}
    </>
  );
}
