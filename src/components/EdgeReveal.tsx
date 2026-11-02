import { useEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "../lib/format";

/**
 * Compact mode: a thin strip at the window's edge. Point at it and the panel
 * slides in over the panes; move away and it slides back. The top bar does
 * this within the workspace (its parent), so a sidebar that stays isn't covered.
 */
export function EdgeReveal({ side, label, children }: { side: "left" | "right" | "top"; label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const timer = useRef<number>(0);
  /** The last pointer move that reached the panel, its popups (portals) included. */
  const inside = useRef<Event | null>(null);
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

  // Enter and leave alone could leave the panel open for good: one that
  // appears under a pointer already on its way back never hears it come in,
  // so never hears it go; a drag holds the pointer; and the pointer can
  // leave the window. While it's open, follow the pointer itself: the window
  // hears every move after React has passed it through the panel, popups
  // and all. A menu opened from the panel keeps it open.
  useEffect(() => {
    if (!open) return;
    let away = false;
    const onMove = (e: PointerEvent) => {
      const over = inside.current === e || !!document.querySelector('[role="menu"]');
      if (over && away) {
        away = false;
        window.clearTimeout(timer.current);
      } else if (!over && !away) {
        away = true;
        hide();
      }
    };
    const onOut = (e: MouseEvent) => {
      if (!e.relatedTarget) hide();
    };
    window.addEventListener("pointermove", onMove);
    document.addEventListener("mouseout", onOut);
    window.addEventListener("blur", hide);
    return () => {
      window.removeEventListener("pointermove", onMove);
      document.removeEventListener("mouseout", onOut);
      window.removeEventListener("blur", hide);
    };
  }, [open]);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const place = side === "top" ? "absolute" : "fixed";
  const edge = side === "left" ? "inset-y-0 left-0 w-[6px]" : side === "right" ? "inset-y-0 right-0 w-[6px]" : "inset-x-0 top-0 h-[4px]";
  const panel = side === "left" ? "inset-y-0 left-0 border-r" : side === "right" ? "inset-y-0 right-0 border-l" : "inset-x-0 top-0 border-b";
  return (
    <>
      <div aria-hidden className={cn(place, "z-40", edge)} onMouseEnter={side === "top" ? showSoon : show} onMouseLeave={side === "top" && !open ? hide : undefined} />
      {open && (
        <div
          role="region"
          aria-label={label}
          onMouseEnter={show}
          onMouseLeave={hide}
          onPointerMove={(e) => (inside.current = e.nativeEvent)}
          className={cn(place, "z-40 flex animate-fade-in border-line bg-window shadow-[0_8px_40px_rgb(0_0_0/0.45)]", panel)}
        >
          {children}
        </div>
      )}
    </>
  );
}
