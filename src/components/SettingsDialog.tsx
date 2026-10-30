import {
  Check,
  Columns3,
  Download,
  ExternalLink,
  EyeOff,
  Globe,
  Info,
  LayoutDashboard,
  LayoutGrid,
  Palette,
  Plus,
  PanelLeft,
  PanelRight,
  RefreshCw,
  RotateCcw,
  Search,
  Sparkles,
  Rows3,
  Server,
  ShieldCheck,
  SquareTerminal,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import type { Arrangement } from "../lib/layout";
import { useSettings, type Settings } from "../lib/settings";
import { useStore } from "../lib/store";
import { ACCENTS, APP_THEMES, resolveScheme, TERMINAL_FONTS, TERMINAL_SCHEMES, fontStack, type AppThemeId } from "../lib/themes";
import { useUpdates } from "../lib/updates";
import { useConfigImport } from "./ConfigImport";
import { showCurrentNotes } from "./WhatsNew";
import { compileRule, HIGHLIGHT_COLORS, type HighlightColor, type HighlightRule } from "../lib/highlight";
import { Button, Logo, Spinner } from "./ui";

const REPO = "https://github.com/footprintsntrails/ssheesh";

const SECTIONS = [
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
  { id: "layout", label: "Layout", icon: LayoutDashboard },
  { id: "privacy", label: "Privacy", icon: ShieldCheck },
  { id: "updates", label: "Updates", icon: RefreshCw },
  { id: "about", label: "About", icon: Info },
] as const;

// ---------------------------------------------------------------- controls

const settingId = (label: string) => `setting-${label.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;

/** Every setting, for search. Words are other ways people might say it. */
const SEARCH_INDEX: { section: string; label: string; words: string }[] = [
  { section: "appearance", label: "Theme", words: "dark light colors look midnight graphite oled daylight" },
  { section: "appearance", label: "Match my system", words: "auto dark mode light mode night day schedule" },
  { section: "appearance", label: "Accent color", words: "highlight color blue" },
  { section: "appearance", label: "Interface size", words: "zoom scale bigger smaller ui text" },
  { section: "appearance", label: "Fade panes you're not using", words: "dim inactive focus opacity" },
  { section: "appearance", label: "Animations", words: "motion reduce effects" },
  { section: "appearance", label: "Logo", words: "icon wink animated mascot face" },
  { section: "terminal", label: "Color scheme", words: "terminal theme dracula nord tokyo catppuccin solarized gruvbox colors" },
  { section: "terminal", label: "Font", words: "typeface mono jetbrains menlo cascadia sf" },
  { section: "terminal", label: "Font size", words: "text bigger smaller" },
  { section: "terminal", label: "Line height", words: "spacing lines" },
  { section: "terminal", label: "Padding", words: "margin space terminal edge" },
  { section: "terminal", label: "Cursor", words: "bar block underline caret" },
  { section: "terminal", label: "Blinking cursor", words: "blink caret" },
  { section: "terminal", label: "When the connection drops", words: "reconnect disconnect auto ask network" },
  { section: "terminal", label: "Tell me when a terminal needs me", words: "notification bell alert agent claude codex done attention" },
  { section: "terminal", label: "Errors", words: "highlight red error output logs" },
  { section: "terminal", label: "Warnings", words: "highlight yellow warn output logs" },
  { section: "terminal", label: "Addresses", words: "highlight ip links urls" },
  { section: "terminal", label: "Your own words", words: "highlight custom rules regex keywords" },
  { section: "layout", label: "Arrange open panes", words: "layout grid columns rows split" },
  { section: "layout", label: "Sidebar", words: "left right hide show server list" },
  { section: "layout", label: "Compact mode", words: "zen focus hide toolbar minimal" },
  { section: "layout", label: "Server list", words: "sidebar narrow icons names rail" },
  { section: "layout", label: "Places and bookmarks", words: "files narrow wide hidden favorites" },
  { section: "layout", label: "Server vitals", words: "stats cpu memory bar metrics" },
  { section: "layout", label: "Space between panes", words: "gap spacing" },
  { section: "layout", label: "Corner roundness", words: "radius rounded corners" },
  { section: "privacy", label: "Streamer mode", words: "hide names ip addresses screen share recording obs twitch" },
  { section: "privacy", label: "What SSHeesh connects to", words: "network telemetry tracking internet" },
  { section: "updates", label: "Install updates", words: "auto update version check" },
];

function Row({ label, hint, children, stack }: { label: string; hint?: string; children: ReactNode; stack?: boolean }) {
  return (
    <div
      id={settingId(label)}
      className={cn("flex gap-4 border-b border-line py-3.5 last:border-0", stack ? "flex-col" : "items-center justify-between")}
    >
      <div className="min-w-0">
        <div className="text-[13px] text-fg">{label}</div>
        {hint && <div className="mt-0.5 text-[11.5px] leading-snug text-fg-3">{hint}</div>}
      </div>
      <div className={cn(stack ? "w-full" : "shrink-0")}>{children}</div>
    </div>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative h-[22px] w-[38px] rounded-full transition-colors",
        checked ? "bg-accent" : "bg-ink/[0.14]",
      )}
    >
      <span
        className={cn(
          "absolute top-[2px] left-[2px] size-[18px] rounded-full bg-white shadow-[0_1px_3px_rgb(0_0_0/0.35)] transition-transform",
          checked && "translate-x-4",
        )}
      />
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: ReactNode }[]; onChange: (v: T) => void }) {
  return (
    <div className="flex h-8 items-center rounded-[9px] bg-ink/[0.05] p-[3px] ring-1 ring-inset ring-ink/[0.06]">
      {options.map((o) => (
        <button
          key={o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            "flex h-[26px] items-center gap-1.5 rounded-[6px] px-3 text-[12px] font-medium transition-all",
            value === o.value ? "bg-raised text-fg shadow-[0_1px_2px_rgb(0_0_0/0.25)] ring-1 ring-ink/[0.06]" : "text-fg-3 hover:text-fg-2",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  format = (v) => String(v),
  label,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  format?: (v: number) => string;
  label: string;
}) {
  const fill = ((value - min) / (max - min)) * 100;
  return (
    <div className="flex w-[220px] items-center gap-3">
      <input
        type="range"
        aria-label={label}
        className="slider flex-1"
        min={min}
        max={max}
        step={step}
        value={value}
        style={{ ["--fill" as string]: `${fill}%` }}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <span className="w-12 text-right text-[12px] tabular-nums text-fg-2">{format(value)}</span>
    </div>
  );
}

function SectionTitle({ children, onReset }: { children: string; onReset?: () => void }) {
  return (
    <div className="mb-1 flex items-center justify-between pr-10">
      <h3 className="text-[17px] font-semibold tracking-tight text-fg">{children}</h3>
      {onReset && (
        <button onClick={onReset} className="flex items-center gap-1 text-[11.5px] text-fg-3 hover:text-fg-2">
          <RotateCcw size={11} /> Reset
        </button>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- sections

function Appearance({ s, set, reset }: SectionProps) {
  // Picking a theme while following the system sets it for that side (light or dark).
  const pick = (id: AppThemeId) => {
    const light = APP_THEMES.find((t) => t.id === id)?.light;
    set(s.themeFollowsSystem ? { theme: id, ...(light ? { lightTheme: id } : { darkTheme: id }) } : { theme: id });
  };
  // Arrow keys walk through the themes and show each one right away.
  const onKey = (e: React.KeyboardEvent) => {
    const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const i = APP_THEMES.findIndex((t) => t.id === s.theme);
    const next = APP_THEMES[(i + step + APP_THEMES.length) % APP_THEMES.length];
    pick(next.id);
    (e.currentTarget.querySelector(`[data-theme-id="${next.id}"]`) as HTMLElement | null)?.focus();
  };
  const themeSelect = (value: AppThemeId, onChange: (v: AppThemeId) => void, light: boolean, label: string) => (
    <select
      aria-label={label}
      value={value}
      onChange={(e) => onChange(e.target.value as AppThemeId)}
      className="h-8 rounded-[8px] bg-ink/[0.05] px-2.5 text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] focus:ring-accent/70"
    >
      {APP_THEMES.filter((t) => !!t.light === light).map((t) => (
        <option key={t.id} value={t.id}>
          {t.name}
        </option>
      ))}
    </select>
  );
  return (
    <>
      <SectionTitle onReset={() => reset(["theme", "accent", "themeFollowsSystem", "lightTheme", "darkTheme", "uiZoom", "dimInactive", "animations", "logo"])}>
        Appearance
      </SectionTitle>
      <Row label="Theme" hint="Use the arrow keys to flip through them." stack>
        <div role="radiogroup" aria-label="Theme" onKeyDown={onKey} className="grid grid-cols-4 gap-2.5">
          {APP_THEMES.map((t) => (
            <button
              key={t.id}
              role="radio"
              aria-checked={s.theme === t.id}
              tabIndex={s.theme === t.id ? 0 : -1}
              data-theme-id={t.id}
              onClick={() => pick(t.id)}
              className="group flex flex-col items-start gap-1.5 rounded-[10px] text-left outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
            >
              <div
                className={cn(
                  "relative h-[64px] w-full overflow-hidden rounded-[10px] ring-1 transition-shadow",
                  s.theme === t.id ? "ring-2 ring-accent" : "ring-ink/[0.1] group-hover:ring-ink/[0.2]",
                )}
                style={{ background: t.window }}
              >
                <div className="absolute inset-y-2 left-2 w-[26%] rounded-[4px]" style={{ background: t.light ? "#ffffff99" : "#ffffff0d" }} />
                <div className="absolute top-2 right-2 bottom-2 left-[34%] rounded-[5px]" style={{ background: t.canvas, boxShadow: `inset 0 0 0 1px ${t.light ? "#0000000f" : "#ffffff12"}` }}>
                  <div className="mt-2 ml-2 h-1 w-[40%] rounded-full" style={{ background: s.accent }} />
                  <div className="mt-1.5 ml-2 h-1 w-[60%] rounded-full opacity-40" style={{ background: t.fg }} />
                  <div className="mt-1.5 ml-2 h-1 w-[30%] rounded-full opacity-25" style={{ background: t.fg }} />
                </div>
              </div>
              <span className={cn("text-[12px]", s.theme === t.id ? "font-medium text-fg" : "text-fg-2")}>{t.name}</span>
            </button>
          ))}
        </div>
      </Row>
      <Row label="Match my system" hint="A light theme by day and a dark one by night, switching when your system does.">
        <Switch label="Match my system" checked={s.themeFollowsSystem} onChange={(v) => set({ themeFollowsSystem: v })} />
      </Row>
      {s.themeFollowsSystem && (
        <Row label="Light and dark">
          <div className="flex items-center gap-2 text-[12px] text-fg-3">
            Light {themeSelect(s.lightTheme, (v) => set({ lightTheme: v }), true, "Light theme")}
            Dark {themeSelect(s.darkTheme, (v) => set({ darkTheme: v }), false, "Dark theme")}
          </div>
        </Row>
      )}
      <Row label="Accent color" hint="Buttons, selection, focus and the glow behind the sidebar.">
        <div className="flex items-center gap-2">
          {ACCENTS.map((a) => (
            <button
              key={a.value}
              title={a.name}
              aria-label={a.name}
              onClick={() => set({ accent: a.value })}
              className="flex size-6 items-center justify-center rounded-full ring-1 ring-inset ring-black/20 transition-transform hover:scale-110"
              style={{ background: a.value }}
            >
              {s.accent === a.value && <Check size={13} strokeWidth={3} className="text-white drop-shadow" />}
            </button>
          ))}
        </div>
      </Row>
      <Row label="Interface size" hint="Everything but the terminal text, which has its own size under Terminal.">
        <Slider label="Interface size" value={s.uiZoom} min={85} max={130} step={5} onChange={(v) => set({ uiZoom: v })} format={(v) => `${v}%`} />
      </Row>
      <Row label="Fade panes you're not using" hint="Makes it obvious where you're typing.">
        <Slider label="Fade panes you're not using" value={s.dimInactive} min={0} max={50} step={5} onChange={(v) => set({ dimInactive: v })} format={(v) => (v ? `${v}%` : "Off")} />
      </Row>
      <Row label="Animations">
        <Switch label="Animations" checked={s.animations} onChange={(v) => set({ animations: v })} />
      </Row>
      <Row label="Logo" hint="In the sidebar and on empty panes. Now and then the winking one blinks at you.">
        <div className="flex items-center gap-3">
          {s.logo !== "hidden" && <Logo size={26} winking={s.logo === "winking" && s.animations} />}
          <Segmented
            value={s.logo}
            onChange={(v) => set({ logo: v })}
            options={[
              { value: "winking", label: "Winking" },
              { value: "still", label: "Still" },
              { value: "hidden", label: "Hidden" },
            ]}
          />
        </div>
      </Row>
    </>
  );
}

export function TerminalPreview({ s }: { s: Settings }) {
  const t = resolveScheme(s.terminalScheme, s.theme, s.accent);
  return (
    <div
      className="overflow-hidden rounded-[10px] px-3.5 py-3 ring-1 ring-ink/[0.08]"
      style={{ background: t.background, color: t.foreground, fontFamily: fontStack(s.terminalFont), fontSize: s.terminalFontSize, lineHeight: s.terminalLineHeight }}
    >
      <div>
        <span style={{ color: t.green }}>deploy@prod-api</span>:<span style={{ color: t.blue }}>~</span>$ docker compose ps
      </div>
      <div>
        app-api-1 <span style={{ color: t.green }}>Up 3 days (healthy)</span>
      </div>
      <div>
        <span style={{ color: t.brightBlack }}>23:10:05</span> <span style={{ color: t.yellow }}>WARN</span> slow query{" "}
        <span style={{ color: t.magenta }}>(412ms)</span> <span style={{ color: t.red }}>✗</span> <span style={{ color: t.cyan }}>retry</span>
      </div>
      <div>
        <span style={{ color: t.green }}>deploy@prod-api</span>:<span style={{ color: t.blue }}>~</span>${" "}
        <span
          className={cn("inline-block align-middle", s.cursorBlink && "animate-pulse")}
          style={{
            background: t.cursor,
            width: s.cursorStyle === "block" ? "0.6em" : s.cursorStyle === "bar" ? 2 : "0.6em",
            height: s.cursorStyle === "underline" ? 2 : "1.1em",
            verticalAlign: s.cursorStyle === "underline" ? "baseline" : "middle",
          }}
        />
      </div>
    </div>
  );
}

function Terminal({ s, set, reset }: SectionProps) {
  const schemes = [["auto", { name: "Match app" }] as const, ...Object.entries(TERMINAL_SCHEMES)];
  return (
    <>
      <SectionTitle
        onReset={() =>
          reset([
            "terminalScheme",
            "terminalFont",
            "terminalFontSize",
            "terminalLineHeight",
            "cursorStyle",
            "cursorBlink",
            "terminalPadding",
            "reconnectMode",
            "highlightErrors",
            "highlightWarnings",
            "highlightAddresses",
            "highlightRules",
            "notifyAttention",
          ])
        }
      >
        Terminal
      </SectionTitle>
      <div className="pt-2 pb-1">
        <TerminalPreview s={s} />
      </div>
      <Row label="Color scheme" stack>
        <div className="grid grid-cols-3 gap-2">
          {schemes.map(([id, scheme]) => {
            const t = resolveScheme(id, s.theme, s.accent);
            const active = s.terminalScheme === id;
            return (
              <button
                key={id}
                onClick={() => set({ terminalScheme: id })}
                className={cn(
                  "flex items-center gap-2.5 rounded-[9px] p-2 text-left ring-1 transition-shadow",
                  active ? "ring-2 ring-accent" : "ring-ink/[0.08] hover:ring-ink/[0.18]",
                )}
                style={{ background: t.background }}
              >
                <span className="flex shrink-0 gap-[3px]">
                  {[t.red, t.green, t.yellow, t.blue, t.magenta].map((c, i) => (
                    <span key={i} className="size-2 rounded-full" style={{ background: c }} />
                  ))}
                </span>
                <span className="truncate text-[11.5px] font-medium" style={{ color: t.foreground }}>
                  {scheme.name}
                </span>
              </button>
            );
          })}
        </div>
      </Row>
      <Row label="Font">
        <select
          value={s.terminalFont}
          onChange={(e) => set({ terminalFont: e.target.value })}
          className="h-8 w-[220px] rounded-[8px] bg-ink/[0.05] px-2.5 text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] focus:ring-accent/70"
        >
          {TERMINAL_FONTS.map((f) => (
            <option key={f.id} value={f.id}>
              {f.name}
            </option>
          ))}
        </select>
      </Row>
      <Row label="Font size">
        <Slider label="Font size" value={s.terminalFontSize} min={10} max={20} onChange={(v) => set({ terminalFontSize: v })} format={(v) => `${v} px`} />
      </Row>
      <Row label="Line height">
        <Slider label="Line height" value={s.terminalLineHeight} min={1} max={1.6} step={0.02} onChange={(v) => set({ terminalLineHeight: v })} format={(v) => v.toFixed(2)} />
      </Row>
      <Row label="Padding">
        <Slider label="Padding" value={s.terminalPadding} min={0} max={28} onChange={(v) => set({ terminalPadding: v })} format={(v) => `${v} px`} />
      </Row>
      <Row label="Cursor">
        <div className="flex items-center gap-3">
          <Segmented
            value={s.cursorStyle}
            onChange={(v) => set({ cursorStyle: v })}
            options={[
              { value: "bar", label: "Bar" },
              { value: "block", label: "Block" },
              { value: "underline", label: "Line" },
            ]}
          />
        </div>
      </Row>
      <Row
        label="When the connection drops"
        hint={
          s.reconnectMode === "auto"
            ? "Reconnects on its own and lands you back in the same session."
            : s.reconnectMode === "ask"
              ? "Shows a Reconnect button over the terminal and waits for you."
              : "Stays quiet: the tab shows Disconnected with a small Reconnect button."
        }
      >
        <Segmented
          value={s.reconnectMode}
          onChange={(v) => set({ reconnectMode: v })}
          options={[
            { value: "auto", label: "Reconnect" },
            { value: "ask", label: "Ask me" },
            { value: "off", label: "Off" },
          ]}
        />
      </Row>
      <Row label="Blinking cursor">
        <Switch label="Blinking cursor" checked={s.cursorBlink} onChange={(v) => set({ cursorBlink: v })} />
      </Row>

      <Row
        label="Tell me when a terminal needs me"
        hint="When a program rings the bell or asks for you (AI coding agents do), or a long run goes quiet. Shows on the pane, and as a system notification while SSHeesh is in the background."
      >
        <Switch label="Tell me when a terminal needs me" checked={s.notifyAttention} onChange={(v) => set({ notifyAttention: v })} />
      </Row>

      <h4 className="mt-6 text-[13px] font-semibold text-fg">Highlighting</h4>
      <p className="mt-0.5 text-[11.5px] leading-snug text-fg-3">Colors words in terminal output so problems jump out. Uses your color scheme's own colors.</p>
      <Row label="Errors" hint="ERROR, FATAL, failed:, Permission denied, command not found…">
        <Switch label="Highlight errors" checked={s.highlightErrors} onChange={(v) => set({ highlightErrors: v })} />
      </Row>
      <Row label="Warnings" hint="WARN, warning:, deprecated…">
        <Switch label="Highlight warnings" checked={s.highlightWarnings} onChange={(v) => set({ highlightWarnings: v })} />
      </Row>
      <Row label="Addresses" hint="IP addresses and web links.">
        <Switch label="Highlight addresses" checked={s.highlightAddresses} onChange={(v) => set({ highlightAddresses: v })} />
      </Row>
      <Row label="Your own words" hint="Plain text matches in any case. Write /like this/ for a regular expression." stack>
        <HighlightRules s={s} rules={s.highlightRules} onChange={(highlightRules) => set({ highlightRules })} />
      </Row>
    </>
  );
}

function HighlightRules({ s, rules, onChange }: { s: Settings; rules: HighlightRule[]; onChange: (r: HighlightRule[]) => void }) {
  const [draft, setDraft] = useState("");
  const [color, setColor] = useState<HighlightColor>("magenta");
  const scheme = resolveScheme(s.terminalScheme, s.theme, s.accent);
  const swatch = (c: HighlightColor) => scheme[c] ?? "#888888";
  const invalid = draft.trim() !== "" && !compileRule(draft);
  const add = () => {
    if (!draft.trim() || invalid) return;
    onChange([...rules, { id: Math.random().toString(36).slice(2, 10), pattern: draft.trim(), color }]);
    setDraft("");
  };
  return (
    <div className="flex flex-col gap-2">
      {rules.length > 0 && (
        <ul className="flex flex-col gap-1 rounded-[10px] p-1.5 ring-1 ring-inset ring-ink/[0.08]" style={{ background: scheme.background }}>
          {rules.map((r) => (
            <li key={r.id} className="flex h-7 items-center gap-2 rounded-[6px] px-2 hover:bg-white/[0.04]">
              <button
                title="Change color"
                aria-label={`Color for ${r.pattern}: ${r.color}. Click to change.`}
                onClick={() => {
                  const next = HIGHLIGHT_COLORS[(HIGHLIGHT_COLORS.indexOf(r.color) + 1) % HIGHLIGHT_COLORS.length];
                  onChange(rules.map((x) => (x.id === r.id ? { ...x, color: next } : x)));
                }}
                className="size-3 shrink-0 rounded-full ring-1 ring-black/20"
                style={{ background: swatch(r.color) }}
              />
              <span className="min-w-0 flex-1 truncate font-mono text-[12px]" style={{ color: swatch(r.color) }}>
                {r.pattern}
              </span>
              <button
                aria-label={`Remove ${r.pattern}`}
                onClick={() => onChange(rules.filter((x) => x.id !== r.id))}
                className="inline-flex size-5 items-center justify-center rounded text-fg-3 hover:text-fg"
              >
                <X size={12} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder="nginx, or /took \d+ms/"
          aria-label="New word to highlight"
          spellCheck={false}
          className={cn(
            "h-8 min-w-0 flex-1 rounded-[8px] bg-ink/[0.05] px-2.5 font-mono text-[12px] text-fg outline-none ring-1 ring-inset placeholder:text-fg-3",
            invalid ? "ring-bad/70" : "ring-ink/[0.08] focus:ring-accent/70",
          )}
        />
        <div className="flex items-center gap-1" role="radiogroup" aria-label="Color">
          {HIGHLIGHT_COLORS.map((c) => (
            <button
              key={c}
              role="radio"
              aria-checked={color === c}
              aria-label={c}
              onClick={() => setColor(c)}
              className={cn("size-4 rounded-full ring-offset-1 ring-offset-transparent", color === c ? "ring-2 ring-fg/80" : "ring-1 ring-black/20")}
              style={{ background: swatch(c) }}
            />
          ))}
        </div>
        <Button size="sm" disabled={!draft.trim() || invalid} onClick={add}>
          <Plus size={12.5} /> Add
        </Button>
      </div>
      {invalid && <div className="text-[11.5px] text-bad">That regular expression doesn't work. Check the brackets and slashes.</div>}
    </div>
  );
}

function Layout({ s, set, reset }: SectionProps) {
  const arrangePanes = useStore((st) => st.arrangePanes);
  const ARRANGE: { id: Arrangement; label: string; icon: typeof Columns3 }[] = [
    { id: "main", label: "Main + stack", icon: LayoutDashboard },
    { id: "columns", label: "Columns", icon: Columns3 },
    { id: "rows", label: "Rows", icon: Rows3 },
    { id: "grid", label: "Grid", icon: LayoutGrid },
  ];
  return (
    <>
      <SectionTitle onReset={() => reset(["sidebarSide", "sidebarVisible", "sidebarMode", "railStatus", "compactMode", "showVitals", "placesMode", "paneGap", "paneRadius"])}>Layout</SectionTitle>
      <Row label="Arrange open panes" hint="Rearranges what's open. Nothing gets closed. You can also drag a pane by its title bar." stack>
        <div className="grid grid-cols-4 gap-2">
          {ARRANGE.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => arrangePanes(id)}
              className="flex flex-col items-center gap-1.5 rounded-[10px] bg-ink/[0.03] py-3 text-[12px] text-fg-2 ring-1 ring-inset ring-ink/[0.07] transition-colors hover:bg-ink/[0.06] hover:text-fg"
            >
              <Icon size={18} strokeWidth={1.6} />
              {label}
            </button>
          ))}
        </div>
      </Row>
      <Row label="Sidebar" hint={`Show or hide with ${shortcutLabel("sidebar")}.`}>
        <div className="flex items-center gap-3">
          <Segmented
            value={s.sidebarSide}
            onChange={(v) => set({ sidebarSide: v })}
            options={[
              { value: "left", label: <><PanelLeft size={13} /> Left</> },
              { value: "right", label: <><PanelRight size={13} /> Right</> },
            ]}
          />
          <Switch label="Show sidebar" checked={s.sidebarVisible} onChange={(v) => set({ sidebarVisible: v })} />
        </div>
      </Row>
      <Row label="Compact mode" hint={`Hides the sidebar, toolbar and vitals. Point at the window's edge to bring them back. ${shortcutLabel("compact")} turns it on and off.`}>
        <Switch label="Compact mode" checked={s.compactMode} onChange={(v) => set({ compactMode: v })} />
      </Row>
      <Row label="Server list" hint="Names, or just icons to save room. Dragging the sidebar's edge switches too. Right-click a server to pick its icon.">
        <Segmented
          value={s.sidebarMode}
          onChange={(v) => set({ sidebarMode: v })}
          options={[
            { value: "wide", label: "Names" },
            { value: "narrow", label: "Icons" },
          ]}
        />
      </Row>
      <Row
        label="Server load"
        hint="How busy each open server is: meters in the list, this style on icons. Point at an icon for the numbers. Amber means a disk or memory is nearly full."
      >
        <Segmented
          value={s.railStatus}
          onChange={(v) => set({ railStatus: v })}
          options={[
            { value: "off", label: "Off" },
            { value: "frame", label: "Frame" },
            { value: "bars", label: "Bars" },
            { value: "pulse", label: "Pulse" },
            { value: "count", label: "Count" },
          ]}
        />
      </Row>
      <HiddenServers />
      <Row label="Places and bookmarks" hint="The column on the left of the file manager. Drag its edge to fold or unfold it.">
        <Segmented
          value={s.placesMode}
          onChange={(v) => set({ placesMode: v })}
          options={[
            { value: "wide", label: "Wide" },
            { value: "narrow", label: "Narrow" },
            { value: "hidden", label: "Hidden" },
          ]}
        />
      </Row>
      <Row label="Server vitals" hint="A one-line CPU, memory, network and disk readout under your panes.">
        <Switch label="Show server vitals" checked={s.showVitals} onChange={(v) => set({ showVitals: v })} />
      </Row>
      <Row label="Space between panes">
        <Slider label="Space between panes" value={s.paneGap} min={0} max={16} onChange={(v) => set({ paneGap: v })} format={(v) => `${v} px`} />
      </Row>
      <Row label="Corner roundness">
        <Slider label="Corner roundness" value={s.paneRadius} min={0} max={18} onChange={(v) => set({ paneRadius: v })} format={(v) => `${v} px`} />
      </Row>
    </>
  );
}

function ago(ms: number): string {
  const m = Math.round((Date.now() - ms) / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  return `${Math.round(m / 60)} h ago`;
}

/** Everything the app itself reaches out to. Kept short on purpose. */
function NetworkList({ updates }: { updates: boolean }) {
  const items = [
    { icon: Server, title: "Your servers", body: "Only the ones you open, through your own ssh." },
    {
      icon: Globe,
      title: "GitHub, for updates",
      body: updates ? "When the app starts and every few hours. Turn it off in Updates." : "Turned off in Updates.",
    },
  ];
  return (
    <div className="mt-2 divide-y divide-line rounded-[12px] bg-ink/[0.03] ring-1 ring-inset ring-ink/[0.07]">
      {items.map(({ icon: Icon, title, body }) => (
        <div key={title} className="flex items-start gap-3 px-4 py-3">
          <Icon size={15} strokeWidth={1.8} className="mt-0.5 shrink-0 text-fg-3" />
          <div className="min-w-0">
            <div className="text-[13px] text-fg">{title}</div>
            <div className="text-[11.5px] leading-snug text-fg-3">{body}</div>
          </div>
        </div>
      ))}
      <div className="px-4 py-3 text-[11.5px] leading-snug text-fg-3">
        That's all. No account, no analytics, no crash reports. Your server list and settings stay on this computer.
      </div>
    </div>
  );
}

function Privacy({ s, set }: SectionProps) {
  return (
    <>
      <SectionTitle>Privacy</SectionTitle>
      <Row
        label="Streamer mode"
        hint={`Hides server names, addresses, IPs and user names in the sidebar, titles, stats and terminal output. For streams, screen shares and screenshots. ${shortcutLabel("streamer")} turns it on and off.`}
      >
        <Switch checked={s.streamerMode} onChange={(v) => set({ streamerMode: v })} label="Streamer mode" />
      </Row>
      {s.streamerMode && (
        <div className="mt-1 flex items-start gap-2.5 rounded-[10px] bg-accent/10 px-3.5 py-3 text-[12px] leading-snug text-fg-2">
          <EyeOff size={14} className="mt-px shrink-0 text-accent" />
          <span>
            Names are hidden as well as SSHeesh can spot them. Glance over your screen before you go live: a name inside a file
            or a log line it has never seen can still slip through.
          </span>
        </div>
      )}
      <h4 id={settingId("What SSHeesh connects to")} className="mt-6 text-[13px] font-semibold text-fg">
        What SSHeesh connects to
      </h4>
      <NetworkList updates={s.updateMode !== "manual"} />
    </>
  );
}

/**
 * The headline of each change in the release notes: the bold lead-in of a
 * bullet, or its first sentence. Stops before the download instructions.
 */
function highlights(notes: string): string[] {
  return notes
    .split(/^## Download/m)[0]
    .split("\n")
    .filter((l) => /^\s*[-*]\s+/.test(l))
    .map((l) => {
      const lead = /\*\*(.+?)\*\*/.exec(l)?.[1];
      const text = (lead ?? l.replace(/^\s*[-*]\s+/, "")).replace(/[*`]/g, "").trim();
      return lead ? text.replace(/\.$/, "") : (text.split(/(?<=\.)\s/)[0] ?? text);
    })
    .filter(Boolean)
    .slice(0, 8);
}

/** Which ~/.ssh/config servers show up in the list. */
function HiddenServers() {
  const hosts = useStore((st) => st.hosts);
  const count = hosts.filter((h) => h.source === "sshConfig").length;
  if (!count) return null;
  return (
    <Row label="Servers from ~/.ssh/config" hint="Choose which of them show up in the sidebar. Your config file isn't changed.">
      <Button size="sm" onClick={() => useConfigImport.getState().show("all")}>
        Choose…
      </Button>
    </Row>
  );
}

function Updates({ s, set }: SectionProps) {
  const { current, status, lastChecked, check, install, restart, portable } = useUpdates();
  return (
    <>
      <SectionTitle>Updates</SectionTitle>
      <div className="mt-2 rounded-[12px] bg-ink/[0.03] p-4 ring-1 ring-inset ring-ink/[0.07]">
        <div className="flex items-center gap-3">
          <Logo size={40} winking={s.animations} />
          <div className="min-w-0 flex-1">
            <div className="text-[14px] font-semibold text-fg">SSHeesh {current || "…"}</div>
            <div className="text-[12px] text-fg-3">
              {status.kind === "checking" && "Checking for updates…"}
              {status.kind === "upToDate" && `You're up to date${lastChecked ? ` · checked ${ago(lastChecked)}` : ""}`}
              {status.kind === "available" && `Version ${status.version} is available`}
              {status.kind === "downloading" &&
                `Downloading ${status.version}${status.progress != null ? ` · ${Math.round(status.progress * 100)}%` : "…"}`}
              {status.kind === "ready" && `Version ${status.version} is installed. Restart to use it.`}
              {status.kind === "error" && <span className="text-bad">{status.message}</span>}
              {status.kind === "idle" && (lastChecked ? `Last checked ${ago(lastChecked)}` : "Early preview build")}
            </div>
          </div>
          {status.kind === "available" && (
            <Button variant="primary" size="sm" onClick={install}>
              <Download size={13} /> {portable ? "Download" : "Install"}
            </Button>
          )}
          {status.kind === "ready" && (
            <Button variant="primary" size="sm" onClick={restart}>
              Restart now
            </Button>
          )}
          {(status.kind === "idle" || status.kind === "upToDate" || status.kind === "error" || status.kind === "checking") && (
            <Button size="sm" disabled={status.kind === "checking"} onClick={() => check()}>
              {status.kind === "checking" ? <Spinner className="size-3" /> : <RefreshCw size={12.5} />} Check now
            </Button>
          )}
        </div>
        {status.kind === "downloading" && (
          <div className="mt-3 h-[5px] overflow-hidden rounded-full bg-ink/[0.08]">
            <div
              className={cn("h-full rounded-full bg-accent transition-[width]", status.progress == null && "w-1/3 animate-pulse")}
              style={status.progress != null ? { width: `${status.progress * 100}%` } : undefined}
            />
          </div>
        )}
        {status.kind === "available" && status.notes && (
          <div className="mt-3 border-t border-line pt-3">
            <div className="text-[11px] font-semibold text-fg-3">What's new</div>
            <ul className="mt-1.5 list-disc space-y-0.5 pl-4 text-[12.5px] text-fg-2">
              {highlights(status.notes).map((l) => (
                <li key={l}>{l}</li>
              ))}
            </ul>
          </div>
        )}
      </div>
      <Row
        label="Install updates"
        hint={
          portable
            ? "This is the portable version: when there's an update, SSHeesh tells you and Download opens the new file. Your servers and settings stay put."
            : "Updates are checked when the app starts and every few hours. Restarting reconnects your terminals."
        }
      >
        <Segmented
          value={s.updateMode}
          onChange={(v) => set({ updateMode: v })}
          options={[
            { value: "auto", label: "Automatically" },
            { value: "notify", label: "Ask me" },
            { value: "manual", label: "Never check" },
          ]}
        />
      </Row>
    </>
  );
}

function About() {
  const current = useUpdates((u) => u.current);
  const open = (url: string) => void backend.openExternal(url);
  return (
    <>
      <SectionTitle>About</SectionTitle>
      <div className="mt-3 flex items-center gap-4">
        <Logo size={56} winking />
        <div>
          <div className="text-[16px] font-semibold text-fg">SSHeesh</div>
          <div className="text-[12.5px] text-fg-3">Version {current || "…"} · early preview, expect rough edges</div>
        </div>
      </div>
      <p className="mt-4 max-w-[460px] text-[12.5px] leading-relaxed text-fg-2">
        Servers, terminals and files that don't fall over. Free, with open source code under the CC BY-NC-SA 4.0 license.
      </p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button size="sm" onClick={() => open(REPO)}>
          <ExternalLink size={12.5} /> GitHub
        </Button>
        <Button size="sm" onClick={() => open(`${REPO}/issues/new/choose`)}>
          <ExternalLink size={12.5} /> Request a feature
        </Button>
        <Button size="sm" onClick={showCurrentNotes}>
          <Sparkles size={12.5} /> What's new
        </Button>
        <Button size="sm" onClick={() => open(`${REPO}/releases`)}>
          <ExternalLink size={12.5} /> All releases
        </Button>
      </div>
    </>
  );
}

interface SectionProps {
  s: Settings;
  set: (p: Partial<Settings>) => void;
  reset: (keys?: (keyof Settings)[]) => void;
}

export function SettingsDialog() {
  const open = useStore((st) => st.settingsOpen);
  const section = useStore((st) => st.settingsSection);
  const openSettings = useStore((st) => st.openSettings);
  const close = useStore((st) => st.closeSettings);
  const s = useSettings();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  const [query, setQuery] = useState("");
  const results = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return SEARCH_INDEX.filter((e) => words.every((w) => `${e.label} ${e.words}`.toLowerCase().includes(w)));
  }, [query]);

  /** Opens the setting's section, scrolls to it and points it out. */
  const jump = (section: string, label: string) => {
    openSettings(section);
    setQuery("");
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const el = document.getElementById(settingId(label));
        if (!el) return;
        el.scrollIntoView({ block: "center", behavior: "smooth" });
        el.classList.add("setting-found");
        setTimeout(() => el.classList.remove("setting-found"), 1400);
      }),
    );
  };

  if (!open) return null;
  const props: SectionProps = { s, set: s.update, reset: s.reset };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6 animate-fade-in" onMouseDown={close}>
      <div
        role="dialog"
        aria-modal
        aria-label="Settings"
        onMouseDown={(e) => e.stopPropagation()}
        className="glass flex h-[min(620px,calc(100vh-48px))] w-[min(820px,100%)] animate-pop-in overflow-hidden rounded-[18px]"
      >
        <nav className="flex w-[190px] shrink-0 flex-col gap-0.5 border-r border-line p-2.5">
          <div className="px-2 pt-1.5 pb-2 text-[12px] font-semibold text-fg-3">Settings</div>
          <label className="relative mb-2 block">
            <span className="sr-only">Search settings</span>
            <Search size={12.5} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-fg-3" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && results[0]) jump(results[0].section, results[0].label);
                if (e.key === "Escape" && query) {
                  e.stopPropagation();
                  setQuery("");
                }
              }}
              placeholder="Search"
              spellCheck={false}
              className="h-7 w-full rounded-[7px] bg-ink/[0.06] pr-2 pl-7 text-[12.5px] text-fg outline-none placeholder:text-fg-3 focus:ring-2 focus:ring-accent/60"
            />
          </label>
          {query &&
            (results.length ? (
              results.map((r) => (
                <button
                  key={r.label}
                  onClick={() => jump(r.section, r.label)}
                  className="flex flex-col items-start rounded-[8px] px-2.5 py-1.5 text-left hover:bg-ink/[0.06]"
                >
                  <span className="text-[12.5px] text-fg">{r.label}</span>
                  <span className="text-[11px] text-fg-3">{SECTIONS.find((x) => x.id === r.section)?.label}</span>
                </button>
              ))
            ) : (
              <p className="px-2.5 py-1.5 text-[12px] text-fg-3">No setting matches “{query}”.</p>
            ))}
          {!query && SECTIONS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => openSettings(id)}
              className={cn(
                "flex h-8 items-center gap-2.5 rounded-[8px] px-2.5 text-[13px] transition-colors",
                section === id ? "bg-accent text-white" : "text-fg-2 hover:bg-ink/[0.06] hover:text-fg",
              )}
            >
              <Icon size={14.5} strokeWidth={1.8} />
              {label}
            </button>
          ))}
        </nav>
        <div className="relative min-w-0 flex-1 overflow-y-auto px-7 py-6">
          <button
            onClick={close}
            aria-label="Close settings"
            className="absolute top-4 right-4 inline-flex size-7 items-center justify-center rounded-md text-fg-3 hover:bg-ink/[0.07] hover:text-fg"
          >
            <X size={15} />
          </button>
          {section === "appearance" && <Appearance {...props} />}
          {section === "terminal" && <Terminal {...props} />}
          {section === "layout" && <Layout {...props} />}
          {section === "privacy" && <Privacy {...props} />}
          {section === "updates" && <Updates {...props} />}
          {section === "about" && <About />}
        </div>
      </div>
    </div>
  );
}
