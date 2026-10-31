import {
  Check,
  ClipboardPaste,
  Columns3,
  Copy,
  Download,
  ExternalLink,
  EyeOff,
  Globe,
  GraduationCap,
  Info,
  Keyboard as KeyboardIcon,
  LayoutDashboard,
  LayoutGrid,
  Palette,
  Plus,
  PanelLeft,
  PanelRight,
  Pencil,
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
import { useMemo, useState, type ReactNode } from "react";
import { backend } from "../lib/backend";
import { ago, cn } from "../lib/format";
import { isMac, isWindows, SHORTCUT_GROUPS, SHORTCUT_NAMES, shortcutLabel } from "../lib/keys";
import type { Arrangement } from "../lib/layout";
import { markHintSeen, useSettings, type Settings, type WelcomeMode, type WelcomePart } from "../lib/settings";
import { useStore } from "../lib/store";
import {
  ACCENTS,
  APP_THEMES,
  findCustomTheme,
  fontStack,
  isBuiltInTheme,
  newCustomThemeId,
  OWN_SCHEME,
  resolveScheme,
  TERMINAL_FONTS,
  TERMINAL_SCHEMES,
  themeInfo,
  type AppThemeId,
  type CustomTheme,
  type CustomThemeId,
} from "../lib/themes";
import { useUpdates } from "../lib/updates";
import { useConfigImport } from "./ConfigImport";
import { showCurrentNotes } from "./WhatsNew";
import { useTour } from "../lib/tour";
import { Segmented, settingId, Slider, Switch } from "./controls";
import { compileRule, HIGHLIGHT_COLORS, type HighlightColor, type HighlightRule } from "../lib/highlight";
import { ShortcutsEditor } from "./ShortcutsEditor";
import { ThemeEditor, ThemeImport } from "./ThemeEditor";
import { ColorButton, OwnAccent } from "./ColorPicker";
import { useEscapeLayer } from "./Overlays";
import { Button, Logo, Spinner } from "./ui";

const REPO = "https://github.com/footprintsntrails/ssheesh";

const SECTIONS = [
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
  { id: "layout", label: "Layout", icon: LayoutDashboard },
  { id: "keyboard", label: "Keyboard", icon: KeyboardIcon },
  { id: "privacy", label: "Privacy", icon: ShieldCheck },
  { id: "updates", label: "Updates", icon: RefreshCw },
  { id: "about", label: "About", icon: Info },
] as const;

// ---------------------------------------------------------------- controls


/** Every setting, for search. Words are other ways people might say it. */
const SEARCH_INDEX: { section: string; label: string; words: string }[] = [
  {
    section: "appearance",
    label: "Theme",
    words: "dark light colors look midnight graphite oled daylight custom own edit editor hex palette transparency see-through opacity glass import export share",
  },
  { section: "appearance", label: "Match my system", words: "auto dark mode light mode night day schedule" },
  { section: "appearance", label: "Accent color", words: "highlight color blue" },
  { section: "appearance", label: "Interface size", words: "zoom scale bigger smaller ui text" },
  { section: "terminal", label: "Welcome", words: "motd welcome banner logo login message greeting" },
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
  ...(isWindows
    ? [{ section: "layout", label: "Use the Windows title bar", words: "title bar window frame decorations native snap drag minimize maximize close buttons" }]
    : []),
  { section: "keyboard", label: "Keyboard shortcuts", words: "keys hotkeys keybindings bindings combos remap" },
  ...SHORTCUT_GROUPS.flatMap((g) => g.ids).map((id) => ({ section: "keyboard", label: SHORTCUT_NAMES[id], words: "shortcut keys hotkey keybinding" })),
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

const WELCOME_MODES: [WelcomeMode, string][] = [
  ["auto", "The server's own welcome, or SSHeesh's if it has none"],
  ["ssheesh", "SSHeesh's, every time"],
  ["first", "SSHeesh's the first time, then the server's"],
  ["server", "The server's own, every time"],
  ["off", "Nothing"],
];
const WELCOME_PARTS: [WelcomePart, string][] = [
  ["logo", "Logo"],
  ["system", "System"],
  ["usage", "Memory and disk"],
  ["status", "Containers and updates"],
];

/** The SSHeesh logo as the welcome draws it, one cell per pixel. */
const WELCOME_LOGO = [".TTTTTTTTT.", "TTTTTTTTTTT", "BBBBBBBBBBB", "BEBBBBBBBEB", "BBEBBBBBEBB", "BEBBBBBBBEB", "BBBBGGGBBBB", ".BBBBBBBBB."];
const WELCOME_COLORS: Record<string, string> = { T: "#2f3136", B: "#232428", E: "#f2f0ea", G: "#3dd68c" };

/** What SSHeesh's welcome will look like, with made-up facts. */
function WelcomePreview({ s }: { s: Settings }) {
  const has = (p: WelcomePart) => s.welcomeParts.includes(p);
  const bar = (pct: number) => (
    <span className="inline-flex h-[2px] w-[110px] translate-y-[-3px] rounded-full bg-[#2b3038]">
      <span className="h-full rounded-full bg-[#3dd68c]" style={{ width: `${pct}%` }} />
    </span>
  );
  return (
    <div className="flex gap-5 overflow-hidden rounded-[10px] bg-[#0a0b0d] px-4 py-3.5 font-mono text-[11px] leading-[17px] text-[#d8dce2] ring-1 ring-ink/[0.08]">
      {has("logo") && (
        <div className="grid shrink-0 grid-cols-[repeat(11,9px)] grid-rows-[repeat(8,9px)] self-start pt-0.5" aria-hidden>
          {WELCOME_LOGO.flatMap((row, r) =>
            [...row].map((c, x) => (
              <span key={`${r}-${x}`} className="flex items-center justify-center text-[6px] text-[#5c5f67]" style={{ background: WELCOME_COLORS[c] }}>
                {r === 1 && x >= 1 && x <= 3 ? "•" : ""}
              </span>
            )),
          )}
        </div>
      )}
      <div className="min-w-0">
        {has("system") && (
          <>
            <div className="truncate">
              Ubuntu 24.04 LTS <span className="text-[#7d8590]">· x86_64 · 8 cores</span>
            </div>
            <div className="truncate text-[#7d8590]">up 41 days · load 0.42 0.38 0.31</div>
          </>
        )}
        {has("usage") && (
          <div className="mt-1.5">
            <div>
              <span className="text-[#5f6775]">mem </span> {bar(70)} 70% <span className="text-[#7d8590]">11.2 / 16 GiB</span>
            </div>
            <div>
              <span className="text-[#5f6775]">disk</span> {bar(46)} 46% <span className="text-[#7d8590]">110 / 240 GB</span>
            </div>
          </div>
        )}
        {has("status") && (
          <div className="mt-1.5 truncate">
            <span className="text-[#3dd68c]">▲</span> 12 containers <span className="text-[#7d8590]">↑</span> 7 updates
          </div>
        )}
        <div className="mt-1.5 truncate text-[#7d8590]">23 updates can be applied immediately.</div>
        {s.welcomeNote.trim() && <div className="mt-1.5 truncate">{s.welcomeNote.trim()}</div>}
      </div>
    </div>
  );
}

/** A group of settings inside a section: a small label over a line, then its rows. */
function SubHeading({ title, hint }: { title: string; hint?: string }) {
  return (
    <div id={settingId(title)} className="mt-7 border-b border-line pb-2">
      <h4 className="text-[11.5px] font-semibold tracking-wide text-fg-3 uppercase">{title}</h4>
      {hint && <p className="mt-1 text-[11.5px] leading-snug text-fg-3">{hint}</p>}
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

/** "My Midnight", or "My Midnight 2" when that's taken. */
function freeName(name: string, list: CustomTheme[]): string {
  const taken = new Set(list.map((t) => t.name));
  if (!taken.has(name)) return name;
  let n = 2;
  while (taken.has(`${name} ${n}`)) n++;
  return `${name} ${n}`;
}

function Appearance({ s, set, reset }: SectionProps) {
  const [editing, setEditing] = useState<CustomThemeId | null>(null);
  const [importing, setImporting] = useState(false);
  const themes = [...APP_THEMES, ...s.customThemes.map((t) => themeInfo(t.id, s.customThemes))];
  const custom = findCustomTheme(s.theme, s.customThemes);
  // Picking a theme while following the system sets it for that side (light or dark).
  const choose = (id: AppThemeId, list = s.customThemes): Partial<Settings> => {
    if (!s.themeFollowsSystem) return { theme: id };
    return { theme: id, ...(themeInfo(id, list).light ? { lightTheme: id } : { darkTheme: id }) };
  };
  const pick = (id: AppThemeId) => set(choose(id));
  const add = (t: CustomTheme) => {
    const list = [...s.customThemes, { ...t, name: freeName(t.name, s.customThemes) }];
    set({ customThemes: list, ...choose(t.id, list) });
  };
  // A copy of the theme you're on, to change. It's in use right away, so edits show live.
  const duplicate = () => {
    const base = custom?.base ?? (isBuiltInTheme(s.theme) ? s.theme : "midnight");
    const name = custom ? `${custom.name} copy` : `My ${themeInfo(s.theme).name}`;
    const t: CustomTheme = { id: newCustomThemeId(), name, base, colors: { ...custom?.colors } };
    add(t);
    setImporting(false);
    setEditing(t.id);
  };
  // Arrow keys walk through the themes and show each one right away.
  const onKey = (e: React.KeyboardEvent) => {
    const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const i = themes.findIndex((t) => t.id === s.theme);
    const next = themes[(i + step + themes.length) % themes.length];
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
      {s.customThemes.length > 0 && (
        <optgroup label="Yours">
          {s.customThemes.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </optgroup>
      )}
    </select>
  );
  if (editing) return <ThemeEditor id={editing} onClose={() => setEditing(null)} onUse={pick} />;
  return (
    <>
      <SectionTitle onReset={() => reset(["theme", "accent", "themeFollowsSystem", "lightTheme", "darkTheme", "uiZoom", "dimInactive", "animations", "logo"])}>
        Appearance
      </SectionTitle>
      <Row label="Theme" hint="Use the arrow keys to flip through them. Duplicate one to change any of its colors, transparency included." stack>
        <div role="radiogroup" aria-label="Theme" onKeyDown={onKey} className="grid grid-cols-4 gap-2.5">
          {themes.map((t) => (
            <button
              key={t.id}
              role="radio"
              aria-checked={s.theme === t.id}
              tabIndex={s.theme === t.id ? 0 : -1}
              data-theme-id={t.id}
              onClick={() => pick(t.id)}
              className="group flex min-w-0 flex-col items-start gap-1.5 rounded-[10px] text-left outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
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
              <span className={cn("w-full truncate text-[12px]", s.theme === t.id ? "font-medium text-fg" : "text-fg-2")}>{t.name}</span>
            </button>
          ))}
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {custom && (
            <Button size="sm" onClick={() => setEditing(custom.id)}>
              <Pencil size={12} /> Edit colors
            </Button>
          )}
          <Button size="sm" onClick={duplicate}>
            <Copy size={12} /> Duplicate and edit
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setImporting(!importing)} aria-expanded={importing}>
            <ClipboardPaste size={12} /> Import
          </Button>
        </div>
        {importing && (
          <ThemeImport
            onAdd={(t) => {
              add(t);
              setImporting(false);
            }}
            onCancel={() => setImporting(false)}
          />
        )}
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
          <OwnAccent value={s.accent} onChange={(accent) => set({ accent })} />
        </div>
      </Row>
      <Row label="Interface size" hint="Everything but the terminal text, which has its own size under Terminal.">
        <Slider label="Interface size" value={s.uiZoom} min={85} max={130} step={5} onChange={(v) => set({ uiZoom: v })} format={(v) => `${v}%`} onRelease />
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
  const schemes = [["auto", { name: "Match app" }] as const, ...Object.entries(TERMINAL_SCHEMES), [OWN_SCHEME, { name: "Your own" }] as const];
  const own = s.terminalScheme === OWN_SCHEME;
  // Your own colors start from what's on screen now.
  const pickOwn = () => {
    const now = resolveScheme(s.terminalScheme, s.theme, s.accent);
    set({ terminalScheme: OWN_SCHEME, terminalColors: s.terminalColors ?? { background: now.background ?? "#0a0b0d", foreground: now.foreground ?? "#d8dce2" } });
  };
  const colors = s.terminalColors ?? { background: "#0a0b0d", foreground: "#d8dce2" };
  const backgrounds = Object.values(TERMINAL_SCHEMES).map((x) => ({ name: x.name, color: x.theme.background ?? "#000000" }));
  const texts = Object.values(TERMINAL_SCHEMES).map((x) => ({ name: x.name, color: x.theme.foreground ?? "#ffffff" }));
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
            "gpuRendering",
            "copyOnSelect",
            "terminalPadding",
            "reconnectMode",
            "highlightErrors",
            "highlightWarnings",
            "highlightAddresses",
            "highlightRules",
            "notifyAttention",
            "welcomeMode",
            "welcomeParts",
            "welcomeNote",
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
                onClick={() => (id === OWN_SCHEME ? pickOwn() : set({ terminalScheme: id }))}
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
      {own && (
        <Row label="Your colors" hint="Background and text. The other colors follow your app theme; the cursor is your accent color.">
          <div className="flex items-center gap-4 text-[12px] text-fg-2">
            {(
              [
                ["background", "Background", backgrounds],
                ["foreground", "Text", texts],
              ] as const
            ).map(([key, name, palette]) => (
              <span key={key} className="flex items-center gap-2">
                <ColorButton
                  label={`Terminal ${name.toLowerCase()}`}
                  value={colors[key]}
                  onChange={(hex) => set({ terminalColors: { ...colors, [key]: hex } })}
                  palette={palette}
                  className="size-7 rounded-[7px] ring-1 ring-inset ring-ink/[0.2] transition-transform hover:scale-105"
                  style={{ background: colors[key] }}
                />
                {name}
              </span>
            ))}
          </div>
        </Row>
      )}
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
        label="Copy text when you select it"
        hint={`Also in server terminals and in programs like Claude Code.${isMac ? "" : " Ctrl+C copies a selection too; with nothing selected it stops the running command. Ctrl+Shift+C and Ctrl+Shift+V always copy and paste."}`}
      >
        <Switch label="Copy text when you select it" checked={s.copyOnSelect} onChange={(v) => set({ copyOnSelect: v })} />
      </Row>
      <Row label="Draw with the GPU" hint="Smoother for programs that redraw a lot, like Claude Code and OpenCode, and box lines join up. Turn it off if a terminal stays blank.">
        <Switch label="Draw with the GPU" checked={s.gpuRendering} onChange={(v) => set({ gpuRendering: v })} />
      </Row>

      <Row
        label="Tell me when a terminal needs me"
        hint="When a program rings the bell or asks for you (AI coding agents do), or a long run goes quiet. Shows on the pane, and as a system notification while SSHeesh is in the background."
      >
        <Switch label="Tell me when a terminal needs me" checked={s.notifyAttention} onChange={(v) => set({ notifyAttention: v })} />
      </Row>

      <SubHeading title="Welcome" hint="What a new terminal on a server shows first. The server's notices, like updates waiting or a restart needed, always show." />
      <Row label="Show" stack>
        <div className="flex flex-col gap-3">
          <select
            aria-label="Welcome"
            value={s.welcomeMode}
            onChange={(e) => set({ welcomeMode: e.target.value as WelcomeMode })}
            className="h-8 w-full rounded-[8px] bg-ink/[0.05] px-2.5 text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] focus:ring-accent/70"
          >
            {WELCOME_MODES.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
          {s.welcomeMode !== "off" && s.welcomeMode !== "server" && <WelcomePreview s={s} />}
        </div>
      </Row>
      {s.welcomeMode !== "off" && s.welcomeMode !== "server" && (
        <>
          <Row label="SSHeesh's shows" hint="Facts come from the server each time, in a blink: no waiting on the network.">
            <div className="flex flex-wrap justify-end gap-1.5">
              {WELCOME_PARTS.map(([part, label]) => {
                const on = s.welcomeParts.includes(part);
                return (
                  <button
                    key={part}
                    type="button"
                    aria-pressed={on}
                    onClick={() => set({ welcomeParts: on ? s.welcomeParts.filter((p) => p !== part) : [...s.welcomeParts, part] })}
                    className={cn(
                      "h-7 rounded-[7px] px-2.5 text-[12px] font-medium ring-1 ring-inset transition-colors",
                      on ? "bg-accent/15 text-fg ring-accent/50" : "text-fg-3 ring-ink/[0.1] hover:text-fg-2",
                    )}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </Row>
          <Row label="Your own line" hint="Under SSHeesh's welcome, like a note to yourself: “production, careful”.">
            <input
              value={s.welcomeNote}
              onChange={(e) => set({ welcomeNote: e.target.value })}
              placeholder="Nothing"
              maxLength={120}
              className="h-8 w-[220px] rounded-[8px] bg-ink/[0.05] px-2.5 text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] placeholder:text-fg-3 focus:ring-accent/70"
            />
          </Row>
        </>
      )}

      <SubHeading title="Highlighting" hint="Colors words in terminal output so problems jump out, in your color scheme's own colors." />
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
      <SectionTitle
        onReset={() =>
          reset(["tabsPlacement", "viewMode", "sidebarSide", "sidebarVisible", "sidebarMode", "railStatus", "compactMode", "showVitals", "envStripe", "paneGap", "paneRadius", "nativeTitleBar"])
        }
      >
        Layout
      </SectionTitle>
      <Row label="Tabs" hint="Under each server in the sidebar, or in a strip along the top. Never both.">
        <Segmented
          value={s.tabsPlacement}
          options={[
            { value: "sidebar", label: "Under each server" },
            { value: "top", label: "Along the top" },
          ]}
          onChange={(v) => set({ tabsPlacement: v })}
        />
      </Row>
      <Row
        label="Tabs on screen"
        hint={
          s.viewMode === "tabs"
            ? `One at a time; the rest keep running. ${shortcutLabel("toggleSplit")}, or dragging a tab onto the page, shows two side by side.`
            : `The tabs you switch on share the window. Drag one by its title bar onto the tab list to make it a tab of its own again. ${shortcutLabel("toggleSplit")} goes back to one at a time.`
        }
      >
        <Segmented
          value={s.viewMode}
          options={[
            { value: "tabs", label: "One at a time" },
            { value: "split", label: "Side by side" },
          ]}
          onChange={(v) => set({ viewMode: v })}
        />
      </Row>
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
      <Row label="A line for Production and Staging" hint="Servers you label Production or Staging always show the word. This adds a thin red or yellow line along the top of the window.">
        <Switch label="A line for Production and Staging" checked={s.envStripe} onChange={(v) => set({ envStripe: v })} />
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
      {isWindows && (
        <Row
          label="Use the Windows title bar"
          hint="SSHeesh draws its own title bar to save room. Switch this on if dragging, snapping or the window buttons misbehave."
        >
          <Switch label="Use the Windows title bar" checked={s.nativeTitleBar} onChange={(v) => set({ nativeTitleBar: v })} />
        </Row>
      )}
    </>
  );
}

function Keyboard({ reset }: SectionProps) {
  return (
    <>
      <SectionTitle onReset={() => reset(["shortcuts"])}>Keyboard</SectionTitle>
      <ShortcutsEditor />
    </>
  );
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
      <SubHeading title="What SSHeesh connects to" />
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
            <Button variant="primary" size="sm" onClick={() => void install()}>
              <Download size={13} /> Install
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
            ? "This is the portable version. It updates itself too: the new file takes this one's place, and your servers and settings stay put. Restarting reconnects your terminals."
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
  const hintsSeen = useSettings((x) => x.hintsSeen);
  const tipsOff = hintsSeen.includes("*");
  const tipsShown = hintsSeen.filter((h) => h !== "*").length;
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
      <SubHeading title="Getting to know SSHeesh" />
      <div className="mt-2.5 flex flex-wrap gap-2">
        <Button size="sm" onClick={() => (useStore.getState().closeSettings(), useTour.getState().show())}>
          <GraduationCap size={12.5} /> Take the tour
        </Button>
        <Button size="sm" onClick={() => (useStore.getState().closeSettings(), useSettings.getState().update({ onboardingVersion: 0, onboarded: false }))}>
          <Sparkles size={12.5} /> Show the welcome again
        </Button>
      </div>
      <Row label="Tips" hint="Small notes the first time you meet a feature, one at a time.">
        <div className="flex items-center gap-2">
          {tipsShown > 0 && (
            <Button size="sm" variant="ghost" onClick={() => useSettings.getState().update({ hintsSeen: [] })}>
              Show them again
            </Button>
          )}
          <Switch label="Tips" checked={!tipsOff} onChange={(v) => (v ? useSettings.getState().update({ hintsSeen: hintsSeen.filter((h) => h !== "*") }) : markHintSeen("*"))} />
        </div>
      </Row>
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

  useEscapeLayer(open, close);

  const [query, setQuery] = useState("");
  // Clicking a section in the list starts it afresh, out of the theme editor too.
  const [visit, setVisit] = useState(0);
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
        className="glass relative flex h-[min(620px,calc(100vh-48px))] w-[min(820px,100%)] animate-pop-in overflow-hidden rounded-[18px]"
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
                  key={`${r.section}-${r.label}`}
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
              onClick={() => {
                openSettings(id);
                setVisit((n) => n + 1);
              }}
              className={cn(
                "flex h-8 items-center gap-2.5 rounded-[8px] px-2.5 text-[13px] transition-colors",
                section === id ? "bg-accent text-on-accent" : "text-fg-2 hover:bg-ink/[0.06] hover:text-fg",
              )}
            >
              <Icon size={14.5} strokeWidth={1.8} />
              {label}
            </button>
          ))}
        </nav>
        {/* Outside the scrolling section, so it stays put. */}
        <button
          onClick={close}
          aria-label="Close settings"
          className="absolute top-4 right-4 z-10 inline-flex size-7 items-center justify-center rounded-md text-fg-3 hover:bg-ink/[0.07] hover:text-fg"
        >
          <X size={15} />
        </button>
        <div key={visit} className="relative min-w-0 flex-1 overflow-y-auto px-7 py-6">
          {section === "appearance" && <Appearance {...props} />}
          {section === "terminal" && <Terminal {...props} />}
          {section === "layout" && <Layout {...props} />}
          {section === "keyboard" && <Keyboard {...props} />}
          {section === "privacy" && <Privacy {...props} />}
          {section === "updates" && <Updates {...props} />}
          {section === "about" && <About />}
        </div>
      </div>
    </div>
  );
}
