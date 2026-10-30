// A small live picture of the app, drawn from the current settings: theme,
// accent, terminal colors and font size, sidebar form and side, the files
// pane's places column, vitals and compact mode. Used while choosing.

import { cn } from "../lib/format";
import { useSettings } from "../lib/settings";
import { fontStack, resolveScheme } from "../lib/themes";

const SERVERS = ["#5b9dff", "#3dd68c", "#ff6fb1", "#b07cff"];
const NAMES = ["prod-api", "homelab", "pi", "db-replica"];

export function MiniApp({ className, focus }: { className?: string; focus?: "sidebar" | "terminal" | "files" | "vitals" }) {
  const s = useSettings();
  const t = resolveScheme(s.terminalScheme, s.theme, s.accent);
  const showSidebar = s.sidebarVisible && !s.compactMode;
  const narrow = s.sidebarMode === "narrow";
  const ring = (part: typeof focus) => focus === part && "ring-2 ring-accent ring-offset-1 ring-offset-transparent";
  const mono = { fontFamily: fontStack(s.terminalFont), fontSize: Math.max(7, s.terminalFontSize * 0.62), lineHeight: s.terminalLineHeight };

  const sidebar = showSidebar && (
    <div
      className={cn("flex shrink-0 flex-col gap-1 border-line p-1.5 transition-all", s.sidebarSide === "left" ? "border-r" : "border-l", ring("sidebar"))}
      style={{ width: narrow ? 26 : 86 }}
    >
      <div className="mb-1 h-2.5 rounded-[3px] bg-ink/[0.08]" />
      {NAMES.map((n, i) =>
        narrow ? (
          <span key={n} className="mx-auto size-3.5 rounded-[4px]" style={{ background: `${SERVERS[i]}40`, boxShadow: `inset 0 0 0 1.5px ${SERVERS[i]}` }} />
        ) : (
          <span key={n} className={cn("flex items-center gap-1 rounded-[3px] px-1 py-[2px]", i === 0 && "bg-ink/[0.09]")}>
            <span className="size-1.5 shrink-0 rounded-[2px]" style={{ background: SERVERS[i] }} />
            <span className="truncate text-[7px] leading-none text-fg-2">{n}</span>
          </span>
        ),
      )}
      {s.logo !== "hidden" && <span className="mt-auto size-3.5 rounded-[4px] bg-[#232428] ring-1 ring-ink/[0.1]" />}
    </div>
  );

  const placesWidth = s.placesMode === "wide" ? 26 : s.placesMode === "narrow" ? 10 : 0;

  return (
    <div
      aria-label="Preview of your choices"
      className={cn("flex h-[230px] w-full flex-col overflow-hidden rounded-[12px] bg-window shadow-[0_10px_30px_rgb(0_0_0/0.35)] ring-1 ring-ink/[0.1]", className)}
    >
      {!s.compactMode && (
        <div className="flex h-5 shrink-0 items-center gap-1 px-2">
          <span className="h-1.5 w-10 rounded-full bg-ink/[0.14]" />
          <span className="flex-1" />
          <span className="h-2.5 w-14 rounded-[3px] bg-ink/[0.07]" />
        </div>
      )}
      <div className={cn("flex min-h-0 flex-1", s.sidebarSide === "right" && "flex-row-reverse")}>
        {sidebar}
        <div className="flex min-w-0 flex-1 flex-col p-1" style={{ gap: Math.max(2, s.paneGap / 2) }}>
          <div className="flex min-h-0 flex-1" style={{ gap: Math.max(2, s.paneGap / 2) }}>
            <div
              className={cn("min-w-0 flex-[1.3] overflow-hidden p-1.5 ring-1 ring-ink/[0.12]", ring("terminal"))}
              style={{ background: t.background, color: t.foreground, borderRadius: Math.max(3, s.paneRadius / 2), ...mono }}
            >
              <div className="truncate">
                <span style={{ color: t.green }}>deploy@prod</span>:<span style={{ color: t.blue }}>~</span>$ uptime
              </div>
              <div className="truncate opacity-90">up 37 days, load 0.12</div>
              <div className="truncate">
                <span style={{ color: t.green }}>deploy@prod</span>:<span style={{ color: t.blue }}>~</span>$ ls
              </div>
              <div className="truncate">
                <span style={{ color: t.blue }}>app</span> <span style={{ color: t.blue }}>logs</span> notes.txt
              </div>
              <div className="truncate">
                <span style={{ color: t.red, fontWeight: 700 }}>ERROR</span> <span style={{ color: t.yellow }}>WARN</span>
              </div>
              <div className="truncate">
                <span style={{ color: t.green }}>deploy@prod</span>:<span style={{ color: t.blue }}>~</span>${" "}
                <span className="inline-block h-[1em] w-[2px] align-middle" style={{ background: t.cursor }} />
              </div>
            </div>
            <div
              className={cn("flex min-w-0 flex-1 overflow-hidden bg-ink/[0.02] ring-1 ring-ink/[0.08]", ring("files"))}
              style={{ borderRadius: Math.max(3, s.paneRadius / 2) }}
            >
              {placesWidth > 0 && (
                <div className="flex shrink-0 flex-col items-center gap-1 border-r border-line py-1.5 transition-all" style={{ width: placesWidth }}>
                  {[0, 1, 2, 3].map((i) =>
                    placesWidth > 12 ? (
                      <span key={i} className="h-1 w-4/5 rounded-full bg-ink/[0.14]" />
                    ) : (
                      <span key={i} className="size-1.5 rounded-[2px] bg-ink/[0.2]" />
                    ),
                  )}
                </div>
              )}
              <div className="flex min-w-0 flex-1 flex-col gap-1 p-1.5">
                {[60, 75, 50, 68, 40].map((w, i) => (
                  <span key={i} className="flex items-center gap-1">
                    <span className="size-1.5 shrink-0 rounded-[2px]" style={{ background: i < 2 ? "var(--ui-accent)" : "rgb(var(--ui-ink) / 0.2)" }} />
                    <span className="h-1 rounded-full bg-ink/[0.12]" style={{ width: `${w}%` }} />
                  </span>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>
      {s.showVitals && !s.compactMode && (
        <div className={cn("flex h-4 shrink-0 items-center gap-2 px-2", ring("vitals"))}>
          {[16, 20, 26].map((w, i) => (
            <span key={i} className="h-1 rounded-full bg-ink/[0.14]" style={{ width: w }} />
          ))}
          <span className="h-1 w-6 rounded-full bg-accent/70" />
        </div>
      )}
    </div>
  );
}
