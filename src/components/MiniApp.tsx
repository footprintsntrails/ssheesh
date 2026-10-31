// A small live picture of the app, drawn from the current settings: theme,
// accent, terminal colors and font size, sidebar form and side, where tabs
// sit, one tab or two side by side, vitals, the Prod line and compact mode.
// Used while choosing.

import { cn } from "../lib/format";
import { HOST_COLORS } from "../lib/labels";
import { useSettings } from "../lib/settings";
import { fontStack, resolveScheme } from "../lib/themes";

const SERVERS = [HOST_COLORS[0], HOST_COLORS[2], HOST_COLORS[4], HOST_COLORS[3]];
const NAMES = ["prod-api", "homelab", "pi", "db-replica"];

/** The first rail icon's load, in the chosen style, at miniature size. */
function MiniLoad() {
  const style = useSettings((s) => s.railStatus);
  if (style === "frame")
    return (
      <svg viewBox="0 0 14 14" className="absolute -top-[3px] -left-[3px] size-5" aria-hidden>
        <path
          d="M7 0.75H9.75A3.5 3.5 0 0 1 13.25 4.25V9.75A3.5 3.5 0 0 1 9.75 13.25H4.25A3.5 3.5 0 0 1 0.75 9.75V4.25A3.5 3.5 0 0 1 4.25 0.75Z"
          fill="none"
          stroke="var(--ui-accent)"
          strokeWidth="1.2"
          pathLength={100}
          strokeDasharray="40 100"
        />
      </svg>
    );
  if (style === "bars")
    return (
      <span className="absolute top-1/2 -right-[5px] flex h-2.5 -translate-y-1/2 items-end gap-px">
        <span className="h-1/2 w-[1.5px] rounded-full bg-accent" />
        <span className="h-3/4 w-[1.5px] rounded-full bg-accent" />
      </span>
    );
  if (style === "pulse")
    return (
      <svg viewBox="0 0 14 4" className="absolute -bottom-[6px] left-0 h-1 w-3.5" aria-hidden>
        <polyline points="0,3 3,2 5,3.5 8,0.8 10,2.6 14,1.5" fill="none" stroke="var(--ui-accent)" strokeWidth="1" />
      </svg>
    );
  if (style === "count") return <span className="absolute -right-[3px] -bottom-[3px] size-[7px] rounded-full bg-ok ring-1 ring-[var(--ui-window)]" />;
  return null;
}

export function MiniApp() {
  const s = useSettings();
  const t = resolveScheme(s.terminalScheme, s.theme, s.accent);
  const showSidebar = s.sidebarVisible && !s.compactMode;
  const narrow = s.sidebarMode === "narrow";
  // Streamer mode hides names here too, so you see what it does.
  const user = s.streamerMode ? "you@server-1" : "deploy@prod";
  const mono = { fontFamily: fontStack(s.terminalFont), fontSize: Math.max(7, s.terminalFontSize * 0.62), lineHeight: s.terminalLineHeight };

  const tabsInSidebar = s.tabsPlacement === "sidebar";
  const split = s.viewMode === "split";
  const sidebar = showSidebar && (
    <div
      className={cn("flex shrink-0 flex-col gap-1 border-line p-1.5 transition-all", s.sidebarSide === "left" ? "border-r" : "border-l")}
      style={{ width: narrow ? 26 : 86 }}
    >
      <div className="mb-1 h-2.5 rounded-[3px] bg-ink/[0.08]" />
      {NAMES.map((n, i) =>
        narrow ? (
          <span key={n} className="flex flex-col items-center gap-[3px]">
            <span className="relative mx-auto size-3.5 rounded-[4px]" style={{ background: `${SERVERS[i]}40`, boxShadow: `inset 0 0 0 1.5px ${SERVERS[i]}` }}>
              {i === 0 && <MiniLoad />}
            </span>
            {i === 0 && tabsInSidebar && [0, 1, 2].map((t) => <span key={t} className={cn("size-2 rounded-[2px]", t === 0 ? "bg-ink/[0.3]" : "bg-ink/[0.12]")} />)}
          </span>
        ) : (
          <span key={n} className="flex flex-col gap-[3px]">
            <span className="flex flex-col gap-[3px] rounded-[3px] px-1 py-[2px]">
              <span className="flex items-center gap-1">
                <span className="size-1.5 shrink-0 rounded-[2px]" style={{ background: SERVERS[i] }} />
                <span className="truncate text-[7px] leading-none text-fg-2">{s.streamerMode ? `server ${i + 1}` : n}</span>
              </span>
              {i === 0 && s.railStatus !== "off" && (
                <span className="flex gap-1 pb-[1px] pl-2.5">
                  <span className="h-[2px] flex-1 rounded-full bg-ink/[0.12]">
                    <span className="block h-full w-2/5 rounded-full bg-accent" />
                  </span>
                  <span className="h-[2px] flex-1 rounded-full bg-ink/[0.12]">
                    <span className="block h-full w-3/5 rounded-full bg-accent" />
                  </span>
                </span>
              )}
            </span>
            {i === 0 &&
              tabsInSidebar &&
              ["Terminal", "Claude Code", "Files"].map((t, k) => (
                <span key={t} className={cn("ml-2 flex items-center gap-1 rounded-[3px] px-1 py-[1.5px]", (k === 0 || (split && k === 2)) && "bg-ink/[0.09]")}>
                  <span className={cn("size-1 shrink-0 rounded-[1px]", k === 1 ? "bg-[#d97757]" : "bg-ink/[0.3]")} />
                  <span className="truncate text-[6.5px] leading-none text-fg-2">{t}</span>
                  {k === 1 && <span className="ml-auto size-1 shrink-0 rounded-full bg-warn" />}
                </span>
              ))}
          </span>
        ),
      )}
      {s.logo !== "hidden" && <span className="mt-auto size-3.5 rounded-[4px] bg-[#232428] ring-1 ring-ink/[0.1]" />}
    </div>
  );

  return (
    <div
      aria-label="Preview of your choices"
      className="relative flex h-[230px] w-full flex-col overflow-hidden rounded-[12px] bg-window shadow-[0_10px_30px_rgb(0_0_0/0.35)] ring-1 ring-ink/[0.1]"
    >
      {s.envStripe && <span className={cn("absolute inset-x-0 top-0 h-[2px] bg-bad")} />}
      {!s.compactMode && (
        <div className={cn("flex h-5 shrink-0 items-center gap-1 px-2")}>
          <span className="size-1.5 rounded-[2px]" style={{ background: SERVERS[0] }} />
          <span className="h-1.5 w-9 rounded-full bg-ink/[0.14]" />
          {!tabsInSidebar &&
            [0, 1, 2].map((k) => <span key={k} className={cn("ml-0.5 h-2.5 w-7 rounded-[3px]", k === 0 ? "bg-ink/[0.12]" : "bg-ink/[0.05]")} />)}
          <span className="flex-1" />
          <span className="h-2 w-8 rounded-[3px] bg-ink/[0.07]" />
        </div>
      )}
      <div className={cn("flex min-h-0 flex-1", s.sidebarSide === "right" && "flex-row-reverse")}>
        {sidebar}
        <div className="flex min-w-0 flex-1 flex-col p-1" style={{ gap: Math.max(2, s.paneGap / 2) }}>
          <div className="flex min-h-0 flex-1" style={{ gap: Math.max(2, s.paneGap / 2) }}>
            <div
              className={cn("min-w-0 flex-[1.3] overflow-hidden p-1.5 ring-1 ring-ink/[0.12]")}
              style={{ background: t.background, color: t.foreground, borderRadius: Math.max(3, s.paneRadius / 2), ...mono }}
            >
              <div className="truncate">
                <span style={{ color: t.green }}>{user}</span>:<span style={{ color: t.blue }}>~</span>$ uptime
              </div>
              <div className="truncate opacity-90">up 37 days, load 0.12</div>
              <div className="truncate">
                <span style={{ color: t.green }}>{user}</span>:<span style={{ color: t.blue }}>~</span>$ ls
              </div>
              <div className="truncate">
                <span style={{ color: t.blue }}>app</span> <span style={{ color: t.blue }}>logs</span> notes.txt
              </div>
              <div className="truncate">
                <span style={{ color: t.red, fontWeight: 700 }}>ERROR</span> <span style={{ color: t.yellow }}>WARN</span>
              </div>
              <div className="truncate">
                <span style={{ color: t.green }}>{user}</span>:<span style={{ color: t.blue }}>~</span>${" "}
                <span className="inline-block h-[1em] w-[2px] align-middle" style={{ background: t.cursor }} />
              </div>
            </div>
            {split && (
              <div
                className={cn("flex min-w-0 flex-1 overflow-hidden bg-ink/[0.02] ring-1 ring-ink/[0.08]")}
                style={{ borderRadius: Math.max(3, s.paneRadius / 2) }}
              >
                <div className="flex min-w-0 flex-1 flex-col gap-1 p-1.5">
                  {[60, 75, 50, 68, 40].map((w, i) => (
                    <span key={i} className="flex items-center gap-1">
                      <span className="size-1.5 shrink-0 rounded-[2px]" style={{ background: i < 2 ? "var(--ui-accent)" : "rgb(var(--ui-ink) / 0.2)" }} />
                      <span className="h-1 rounded-full bg-ink/[0.12]" style={{ width: `${w}%` }} />
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
      {s.showVitals && !s.compactMode && (
        <div className={cn("flex h-4 shrink-0 items-center gap-2 px-2")}>
          {[16, 20, 26].map((w, i) => (
            <span key={i} className="h-1 rounded-full bg-ink/[0.14]" style={{ width: w }} />
          ))}
          <span className="h-1 w-6 rounded-full bg-accent/70" />
        </div>
      )}
    </div>
  );
}
