import {
  Check,
  Columns3,
  Download,
  ExternalLink,
  Info,
  LayoutDashboard,
  LayoutGrid,
  Palette,
  PanelLeft,
  PanelRight,
  RefreshCw,
  RotateCcw,
  Rows3,
  SquareTerminal,
  X,
} from "lucide-react";
import { useEffect, type ReactNode } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import type { Arrangement } from "../lib/layout";
import { useSettings, type Settings } from "../lib/settings";
import { useStore } from "../lib/store";
import { ACCENTS, APP_THEMES, resolveScheme, TERMINAL_FONTS, TERMINAL_SCHEMES, fontStack } from "../lib/themes";
import { useUpdates } from "../lib/updates";
import { Button, Spinner } from "./ui";

const REPO = "https://github.com/footprintsntrails/ssheesh";

const SECTIONS = [
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "terminal", label: "Terminal", icon: SquareTerminal },
  { id: "layout", label: "Layout", icon: LayoutDashboard },
  { id: "updates", label: "Updates", icon: RefreshCw },
  { id: "about", label: "About", icon: Info },
] as const;

// ---------------------------------------------------------------- controls

function Row({ label, hint, children, stack }: { label: string; hint?: string; children: ReactNode; stack?: boolean }) {
  return (
    <div className={cn("flex gap-4 border-b border-line py-3.5 last:border-0", stack ? "flex-col" : "items-center justify-between")}>
      <div className="min-w-0">
        <div className="text-[13px] text-fg">{label}</div>
        {hint && <div className="mt-0.5 text-[11.5px] leading-snug text-fg-3">{hint}</div>}
      </div>
      <div className={cn(stack ? "w-full" : "shrink-0")}>{children}</div>
    </div>
  );
}

function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
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

function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: ReactNode }[]; onChange: (v: T) => void }) {
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

function Slider({
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
  return (
    <>
      <SectionTitle onReset={() => reset(["theme", "accent"])}>Appearance</SectionTitle>
      <Row label="Theme" stack>
        <div className="grid grid-cols-4 gap-2.5">
          {APP_THEMES.map((t) => (
            <button key={t.id} onClick={() => set({ theme: t.id })} className="group flex flex-col items-start gap-1.5 text-left">
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
    </>
  );
}

function TerminalPreview({ s }: { s: Settings }) {
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
          reset(["terminalScheme", "terminalFont", "terminalFontSize", "terminalLineHeight", "cursorStyle", "cursorBlink", "terminalPadding", "reconnectMode"])
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
    </>
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
      <SectionTitle onReset={() => reset(["sidebarSide", "sidebarVisible", "showVitals", "placesMode", "paneGap", "paneRadius"])}>Layout</SectionTitle>
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

function Updates({ s, set }: SectionProps) {
  const { current, status, lastChecked, check, install, restart } = useUpdates();
  return (
    <>
      <SectionTitle>Updates</SectionTitle>
      <div className="mt-2 rounded-[12px] bg-ink/[0.03] p-4 ring-1 ring-inset ring-ink/[0.07]">
        <div className="flex items-center gap-3">
          <img src="/icon.svg" alt="" className="size-10" />
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
              {status.notes
                .split("\n")
                .map((l) => l.replace(/^[-*]\s*/, "").trim())
                .filter(Boolean)
                .slice(0, 8)
                .map((l) => (
                  <li key={l}>{l}</li>
                ))}
            </ul>
          </div>
        )}
      </div>
      <Row label="Install updates" hint="Updates are checked when the app starts and every few hours. Restarting reconnects your terminals.">
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
        <img src="/icon.svg" alt="" className="size-14" />
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
        <Button size="sm" onClick={() => open(`${REPO}/releases`)}>
          <ExternalLink size={12.5} /> Release notes
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
          <div className="px-2 pt-1.5 pb-3 text-[12px] font-semibold text-fg-3">Settings</div>
          {SECTIONS.map(({ id, label, icon: Icon }) => (
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
          {section === "updates" && <Updates {...props} />}
          {section === "about" && <About />}
        </div>
      </div>
    </div>
  );
}
