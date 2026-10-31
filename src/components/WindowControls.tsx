// Minimize, maximize and close for SSHeesh's own title bar on Windows, drawn
// like Windows 11's: Segoe Fluent glyphs, 46 px wide buttons, a red Close.
// Hovering maximize opens Snap Layouts, as on a system title bar.

import { useEffect, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { isWindows } from "../lib/keys";
import { useSettings } from "../lib/settings";

/** Windows, in the app, with the system title bar turned off in Settings. */
export function useCustomTitleBar(): boolean {
  const native = useSettings((s) => s.nativeTitleBar);
  return isWindows && !backend.isDemo && !native;
}

/** Keeps the window's frame in step with the setting. */
export function useWindowFrame() {
  const native = useSettings((s) => s.nativeTitleBar);
  useEffect(() => {
    if (!isWindows || backend.isDemo) return;
    void import("@tauri-apps/api/window").then(({ getCurrentWindow }) => getCurrentWindow().setDecorations(native)).catch(() => {});
  }, [native]);
}

const GLYPH = { minimize: "\uE921", maximize: "\uE922", restore: "\uE923", close: "\uE8BB" };
const SNAP_AFTER_MS = 620;

export function WindowControls({ height = 40 }: { height?: number }) {
  const [maximized, setMaximized] = useState(false);
  // Like the system's, the buttons fade while another window is in front.
  const [active, setActive] = useState(() => document.hasFocus());
  useEffect(() => {
    const on = () => setActive(true);
    const off = () => setActive(false);
    window.addEventListener("focus", on);
    window.addEventListener("blur", off);
    return () => {
      window.removeEventListener("focus", on);
      window.removeEventListener("blur", off);
    };
  }, []);
  const snapTimer = useRef(0);
  const win = useRef<import("@tauri-apps/api/window").Window | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
      const w = getCurrentWindow();
      win.current = w;
      const sync = () => void w.isMaximized().then((m) => !gone && setMaximized(m));
      sync();
      // Dragging an edge sends a stream of resizes; ask once it settles.
      let settle: ReturnType<typeof setTimeout> | undefined;
      const off = await w.onResized(() => {
        clearTimeout(settle);
        settle = setTimeout(sync, 150);
      });
      if (gone) off();
      else unlisten = off;
    });
    return () => {
      gone = true;
      unlisten?.();
      window.clearTimeout(snapTimer.current);
    };
  }, []);

  const button = cn("flex w-[46px] items-center justify-center text-[10px] transition-colors", active ? "text-fg-2" : "text-fg-3/70");
  const glyphs = { fontFamily: '"Segoe Fluent Icons", "Segoe MDL2 Assets"' };
  return (
    <div className="flex shrink-0 self-stretch" style={{ height }}>
      <button type="button" aria-label="Minimize" title="Minimize" onClick={() => void win.current?.minimize()} className={cn(button, "hover:bg-ink/[0.07] hover:text-fg")} style={glyphs}>
        {GLYPH.minimize}
      </button>
      <button
        type="button"
        aria-label={maximized ? "Restore" : "Maximize"}
        title={maximized ? "Restore" : "Maximize"}
        onClick={() => {
          window.clearTimeout(snapTimer.current);
          void win.current?.toggleMaximize();
        }}
        onPointerEnter={() => {
          snapTimer.current = window.setTimeout(() => void win.current?.setFocus().then(() => backend.showSnapLayouts()), SNAP_AFTER_MS);
        }}
        onPointerLeave={() => window.clearTimeout(snapTimer.current)}
        className={cn(button, "hover:bg-ink/[0.07] hover:text-fg")}
        style={glyphs}
      >
        {maximized ? GLYPH.restore : GLYPH.maximize}
      </button>
      <button type="button" aria-label="Close" title="Close" onClick={() => void win.current?.close()} className={cn(button, "hover:bg-[#c42b1c] hover:text-white")} style={glyphs}>
        {GLYPH.close}
      </button>
    </div>
  );
}
