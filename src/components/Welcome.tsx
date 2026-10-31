// The hello. New here: your servers first, then one page to make SSHeesh
// yours with a live preview of the app beside it; the one-minute tour is
// offered at the end. Everything applies at once, every step can be skipped,
// and all of it lives in Settings later.
// Known SSHeesh before a redesign: one screen of what changed, with the old
// way one click away, and every choice you made kept.

import { ArrowLeft, ChevronDown, Eye, EyeOff, FileUp, MessageSquare, Plus, RotateCcw } from "lucide-react";
import { useEffect, useMemo, useState, type ComponentType, type ReactNode } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { isMac, shortcutLabel } from "../lib/keys";
import { useLabels, useUndecidedConfig } from "../lib/labels";
import { toggleStreamer, useNames } from "../lib/privacy";
import { ONBOARDING_VERSION, useSettings, type Settings } from "../lib/settings";
import { hostAddress, useStore } from "../lib/store";
import { ACCENTS, APP_THEMES, resolveScheme, TERMINAL_FONTS, TERMINAL_SCHEMES } from "../lib/themes";
import { MiniApp } from "./MiniApp";
import { Modal } from "./Overlays";
import { Segmented, Slider, Switch } from "./controls";
import { useTour } from "../lib/tour";
import { SceneMainMenu, SceneNewLayout, ScenePeek, SceneTabs } from "./TourScenes";
import { Button, Logo } from "./ui";

function set(patch: Partial<Settings>) {
  useSettings.getState().update(patch);
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <div>
        <div className="text-[12.5px] font-medium text-fg">{label}</div>
        {hint && <div className="text-[11.5px] leading-snug text-fg-3">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

/** A choice with a little picture of what it does. */
function Card({ selected, onClick, title, children }: { selected: boolean; onClick: () => void; title: string; children: ReactNode }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onClick}
      className={cn(
        "flex flex-1 flex-col items-center gap-1.5 rounded-[10px] p-2 text-[11.5px] ring-1 ring-inset transition-colors",
        selected ? "bg-accent/10 font-medium text-fg ring-2 ring-accent" : "text-fg-2 ring-ink/[0.1] hover:bg-ink/[0.04]",
      )}
    >
      <svg viewBox="0 0 64 40" className="h-10 w-16" aria-hidden>
        <rect x="0.5" y="0.5" width="63" height="39" rx="5" fill="none" stroke="currentColor" strokeOpacity="0.35" />
        {children}
      </svg>
      {title}
    </button>
  );
}

const fill = { fill: "currentColor", fillOpacity: 0.28 };
const strong = { fill: "var(--ui-accent)", fillOpacity: 0.85 };

const LOAD_STYLES = [
  { id: "off", title: "Off" },
  { id: "frame", title: "Frame" },
  { id: "bars", title: "Bars" },
  { id: "pulse", title: "Pulse" },
  { id: "count", title: "Count" },
] as const;

/** One server icon drawn with the load style on it. */
function LoadPicture({ style }: { style: (typeof LOAD_STYLES)[number]["id"] }) {
  const accent = { stroke: "var(--ui-accent)", strokeWidth: 1.6, fill: "none", strokeLinecap: "round" } as const;
  const cx = style === "bars" ? 29 : 32;
  const cy = style === "pulse" ? 15 : 18;
  return (
    <>
      <rect x="21" y="7" width="22" height="22" rx="6" {...fill} />
      <circle cx={cx} cy={cy} r="4.5" {...accent} />
      {style === "off" && <circle cx="40" cy="26" r="2.4" fill="var(--ui-ok)" />}
      {style === "frame" && (
        <path d="M32 7H37A6 6 0 0 1 43 13V23A6 6 0 0 1 37 29H27A6 6 0 0 1 21 23V13A6 6 0 0 1 27 7Z" {...accent} strokeWidth={2} pathLength={100} strokeDasharray="38 100" />
      )}
      {style === "bars" &&
        [0.5, 0.75, 0.35].map((v, i) => <rect key={i} x={35.5 + i * 2.5} y={24 - 10 * v} width="1.6" height={10 * v} rx="0.8" fill="var(--ui-accent)" />)}
      {style === "pulse" && <polyline points="25,25 28,23.5 30.5,26 33.5,21.5 36,24.5 39,22.5" {...accent} strokeWidth={1.3} />}
      {style === "count" && (
        <>
          <circle cx="41" cy="27" r="4.2" fill="var(--ui-ok)" />
          <text x="41" y="29" textAnchor="middle" fontSize="6" fontWeight="700" fill="black" fillOpacity="0.75">
            2
          </text>
        </>
      )}
    </>
  );
}

/** A switch with its label and a line about what it does. */
function Toggle({ label, hint, checked, onChange }: { label: string; hint: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between gap-3">
      <span>
        <span className="block text-[12.5px] font-medium text-fg">{label}</span>
        <span className="block text-[11.5px] leading-snug text-fg-3">{hint}</span>
      </span>
      <Switch label={label} checked={checked} onChange={onChange} />
    </label>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-4">
      <h3 className="border-b border-line pb-1.5 text-[11.5px] font-semibold text-fg-3">{title}</h3>
      {children}
    </section>
  );
}

/** Names on screen while you set up: one click hides them, as streamer mode does everywhere. */
function HideNames() {
  const on = useSettings((s) => s.streamerMode);
  return (
    <button
      type="button"
      onClick={toggleStreamer}
      title={`Streamer mode: hide server names and addresses (${shortcutLabel("streamer")})`}
      className={cn("flex h-7 items-center gap-1.5 rounded-[7px] px-2.5 text-[12px] font-medium", on ? "bg-accent/15 text-accent" : "text-fg-3 hover:bg-ink/[0.06] hover:text-fg-2")}
    >
      {on ? <EyeOff size={13} /> : <Eye size={13} />}
      {on ? "Names hidden" : "Hide names"}
    </button>
  );
}

/** The settings "Make it yours" offers, and what "Use defaults" puts back. */
const MADE_YOURS: (keyof Settings)[] = [
  "theme",
  "themeFollowsSystem",
  "accent",
  "terminalScheme",
  "terminalFont",
  "terminalFontSize",
  "cursorStyle",
  "tabsPlacement",
  "viewMode",
  "sidebarMode",
  "sidebarSide",
  "railStatus",
  "compactMode",
  "envStripe",
  "reconnectMode",
  "notifyAttention",
  "highlightErrors",
  "highlightWarnings",
  "logo",
];

const ISSUES = "https://github.com/footprintsntrails/ssheesh/issues/new/choose";

/** What the redesign changed, for people who knew SSHeesh before it. */
function WhatChanged({ onCustomize, onDone }: { onCustomize: () => void; onDone: (tour: boolean) => void }) {
  const s = useSettings();
  // "Show me" plays its picture here, in place of the new layout's; the
  // hello stays open, so you can look at all of them and then choose.
  const [showing, setShowing] = useState<string | null>(null);
  const [take, setTake] = useState(0);
  const rows: { title: string; body: string; scene?: ComponentType }[] = [
    { title: "Main menu", body: "A start page with your servers, agents at work and tabs you closed.", scene: SceneMainMenu },
    { title: "Peek at files", body: `${isMac ? "⌘-click" : "Ctrl+click"} a path in a terminal to see the file beside it.`, scene: ScenePeek },
    { title: "Agents in the sidebar", body: "Claude Code and Codex show up by name, on every server.", scene: SceneTabs },
    { title: "Your own colors and keys", body: "Edit any theme color, and set your own keyboard shortcuts." },
  ];
  const Scene = rows.find((r) => r.title === showing)?.scene ?? SceneNewLayout;
  const show = (r: (typeof rows)[number]) => {
    if (!r.scene) return onCustomize();
    setShowing(r.title);
    setTake((t) => t + 1);
  };
  return (
    <div className="flex flex-col gap-5 p-6">
      <div className="flex items-center gap-3">
        <Logo size={36} winking />
        <h2 className="flex-1 text-[20px] font-semibold tracking-tight text-fg">SSHeesh has a new layout</h2>
        <HideNames />
      </div>
      <div className="relative">
        <Scene key={take} />
        <button
          type="button"
          onClick={() => (showing ? setShowing(null) : setTake((t) => t + 1))}
          className="absolute right-2 bottom-2 flex h-6 items-center gap-1 rounded-[6px] bg-raised/90 px-2 text-[11px] text-fg-2 ring-1 ring-ink/[0.1] hover:text-fg"
        >
          {showing ? (
            <>
              <ArrowLeft size={11} /> The new layout
            </>
          ) : (
            <>
              <RotateCcw size={11} /> Replay
            </>
          )}
        </button>
      </div>
      <p className="-mt-1 text-[13px] text-fg-2">Your servers, sessions and settings are where you left them.</p>
      <div className="grid grid-cols-2 gap-4">
        <div className="flex flex-col gap-2">
          <span className="text-[12px] font-medium text-fg">Tabs</span>
          <Segmented
            value={s.tabsPlacement}
            onChange={(v) => set({ tabsPlacement: v })}
            options={[
              { value: "sidebar", label: "Under each server" },
              { value: "top", label: "Along the top, as before" },
            ]}
          />
        </div>
        <div className="flex flex-col gap-2">
          <span className="text-[12px] font-medium text-fg">Files</span>
          <Segmented
            value={s.viewMode}
            onChange={(v) => set({ viewMode: v })}
            options={[
              { value: "tabs", label: "As a tab" },
              { value: "split", label: "Beside the terminal" },
            ]}
          />
        </div>
      </div>
      <div className="flex flex-col">
        <h3 className="pb-1 text-[11.5px] font-semibold text-fg-3">Also new</h3>
        {rows.map((r) => (
          <div key={r.title} className={cn("-mx-2 flex items-center gap-3 rounded-[8px] border-t border-line px-2 py-2", showing === r.title && "border-transparent bg-accent/10")}>
            <span className="min-w-0 flex-1">
              <span className="block text-[12.5px] font-medium text-fg">{r.title}</span>
              <span className="block text-[11.5px] text-fg-3">{r.body}</span>
            </span>
            <Button size="sm" variant="ghost" onClick={() => show(r)}>
              {showing === r.title ? "Replay" : r.scene ? "Show me" : "Try it"}
            </Button>
          </div>
        ))}
      </div>
      <div className="flex items-center gap-2">
        <button type="button" onClick={() => void backend.openExternal(ISSUES)} className="flex items-center gap-1.5 text-[12px] text-fg-3 hover:text-fg-2">
          <MessageSquare size={12.5} /> Tell us what you miss
        </button>
        <span className="flex-1" />
        <Button variant="ghost" onClick={onCustomize}>
          Customize more
        </Button>
        <Button variant="ghost" onClick={() => onDone(true)}>
          Take the tour
        </Button>
        <Button variant="primary" autoFocus onClick={() => onDone(false)}>
          Start
        </Button>
      </div>
    </div>
  );
}

export function Welcome() {
  const version = useSettings((s) => s.onboardingVersion);
  const s = useSettings();
  const hostsLoaded = useStore((st) => st.hostsLoaded);
  const hosts = useStore((st) => st.hosts);
  const undecided = useUndecidedConfig();
  // Decided once, when the hello opens: whether you used SSHeesh before.
  const [returning] = useState(() => useSettings.getState().onboarded);
  const [step, setStep] = useState<"servers" | "yours" | "changed">(returning ? "changed" : "servers");
  const [keep, setKeep] = useState<string[]>([]);
  const [addAfter, setAddAfter] = useState(false);
  const [more, setMore] = useState(false);

  // The demo build skips the hello unless asked for it.
  const demoSkip = backend.isDemo && !new URLSearchParams(location.search).has("firstrun");
  useEffect(() => {
    if (demoSkip && version < ONBOARDING_VERSION) set({ onboarded: true, onboardingVersion: ONBOARDING_VERSION });
  }, [demoSkip, version]);

  const configIds = useMemo(() => undecided.map((h) => h.id), [undecided]);
  useEffect(() => setKeep(configIds), [configIds]);

  // Mounted only while the hello is due; it waits for your servers to load.
  const open = hostsLoaded && !demoSkip;
  const done = (tour: boolean) => {
    if (configIds.length) useLabels.getState().chooseConfig(configIds, keep);
    set({ onboarded: true, onboardingVersion: ONBOARDING_VERSION });
    if (addAfter) useStore.getState().setAddHostOpen(true);
    else if (tour) useTour.getState().show();
  };
  // Skipping keeps the defaults, and doesn't decide for you: new config
  // servers wait behind a quiet note.
  const skip = () => set({ onboarded: true, onboardingVersion: ONBOARDING_VERSION });
  const schemes = [["auto", "Match app"] as const, ...Object.entries(TERMINAL_SCHEMES).map(([id, v]) => [id, v.name] as const)];
  const names = useNames();

  if (step === "changed")
    return (
      <Modal open={open} onClose={skip} width={680}>
        <WhatChanged onCustomize={() => setStep("yours")} onDone={done} />
      </Modal>
    );

  return (
    <Modal open={open} onClose={skip} width={step === "yours" ? 900 : 620}>
      {step === "servers" ? (
        <div className="flex flex-col gap-5 p-6">
          <div className="flex items-center gap-3">
            <Logo size={36} winking />
            <span className="flex-1 text-[13px] font-medium text-fg-2">Welcome to SSHeesh</span>
            <HideNames />
          </div>
          <div>
            <h2 className="text-[22px] font-semibold tracking-tight text-fg">Your servers</h2>
            <p className="mt-1 text-[13.5px] leading-relaxed text-fg-2">
              {configIds.length > 0
                ? `Found ${configIds.length} in your SSH config. Pick the ones you want; the file isn't changed.`
                : hosts.length
                  ? `Your ${hosts.length} server${hosts.length === 1 ? " is" : "s are"} here.`
                  : "Add your first one: paste its address, or a whole ssh command, and SSHeesh fills in the rest."}
            </p>
          </div>
          {configIds.length > 0 && (
            <div className="flex max-h-[300px] flex-col gap-0.5 overflow-y-auto rounded-[10px] p-1 ring-1 ring-inset ring-ink/[0.08]">
              {undecided.map((h) => (
                <label key={h.id} className="flex h-10 cursor-pointer items-center gap-3 rounded-[7px] px-2.5 hover:bg-ink/[0.04]">
                  <input
                    type="checkbox"
                    checked={keep.includes(h.id)}
                    onChange={() => setKeep((k) => (k.includes(h.id) ? k.filter((x) => x !== h.id) : [...k, h.id]))}
                    className="accent-[var(--ui-accent)]"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] text-fg">{names.host(h)}</span>
                    {!names.on && hostAddress(h) !== h.name && <span className="block truncate font-mono text-[11px] text-fg-3">{hostAddress(h)}</span>}
                  </span>
                </label>
              ))}
            </div>
          )}
          <label className="flex cursor-pointer items-center gap-2.5 rounded-[10px] bg-ink/[0.04] px-3 py-2.5 text-[13px] text-fg">
            <input type="checkbox" checked={addAfter} onChange={(e) => setAddAfter(e.target.checked)} className="accent-[var(--ui-accent)]" />
            <Plus size={14} className="text-accent" /> {configIds.length || hosts.length ? "Add another server when I'm done" : "Add a server when I'm done"}
          </label>
          <p className="text-[11.5px] text-fg-3">Nothing connects yet. No account, no cloud: your servers and keys stay on this computer.</p>
          <div className="flex items-center gap-3">
            <Button variant="ghost" onClick={skip}>
              Skip setup
            </Button>
            <span className="flex-1" />
            <Button variant="primary" autoFocus onClick={() => setStep("yours")}>
              {configIds.length ? (
                <>
                  <FileUp size={14} /> Add {keep.length} server{keep.length === 1 ? "" : "s"}
                </>
              ) : (
                "Next"
              )}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex max-h-[calc(84vh-16px)] flex-col p-6">
          <div className="flex items-start gap-3">
            <div className="flex-1">
              <h2 className="text-[22px] font-semibold tracking-tight text-fg">Make it yours</h2>
              <p className="mt-1 text-[13px] text-fg-2">Everything changes as you click. It's all in Settings later.</p>
            </div>
            <HideNames />
          </div>

          <div className="mt-5 flex min-h-0 flex-1 items-start gap-7">
            <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-6 self-stretch overflow-y-auto pr-1">
              <Group title="Look">
                <Field label="Theme">
                  <div className="grid grid-cols-4 gap-2" role="radiogroup" aria-label="Theme">
                    {APP_THEMES.map((t) => (
                      <button
                        key={t.id}
                        role="radio"
                        aria-checked={s.theme === t.id}
                        onClick={() => set(s.themeFollowsSystem ? { theme: t.id, ...(t.light ? { lightTheme: t.id } : { darkTheme: t.id }) } : { theme: t.id })}
                        className="group flex flex-col items-start gap-1 text-left"
                      >
                        <span
                          className={cn("relative block h-[38px] w-full overflow-hidden rounded-[8px] ring-1", s.theme === t.id ? "ring-2 ring-accent" : "ring-ink/[0.1] group-hover:ring-ink/[0.2]")}
                          style={{ background: t.window }}
                        >
                          <span className="absolute top-1.5 right-1.5 bottom-1.5 left-[34%] rounded-[3px]" style={{ background: t.canvas }}>
                            <span className="mt-1.5 ml-1.5 block h-1 w-[40%] rounded-full" style={{ background: s.accent }} />
                          </span>
                        </span>
                        <span className={cn("text-[11.5px]", s.theme === t.id ? "font-medium text-fg" : "text-fg-2")}>{t.name}</span>
                      </button>
                    ))}
                  </div>
                </Field>
                <Toggle label="Match my system" hint="Light by day, dark by night, as your system switches." checked={s.themeFollowsSystem} onChange={(v) => set({ themeFollowsSystem: v })} />
                <Field label="Accent color" hint="Focus and the main buttons.">
                  <div className="flex items-center gap-2" role="radiogroup" aria-label="Accent color">
                    {ACCENTS.map((a) => (
                      <button
                        key={a.value}
                        role="radio"
                        aria-checked={s.accent === a.value}
                        aria-label={a.name}
                        title={a.name}
                        onClick={() => set({ accent: a.value })}
                        className={cn("size-6 rounded-full ring-1 ring-inset ring-black/20", s.accent === a.value && "ring-2 ring-fg/80 ring-offset-2 ring-offset-[var(--ui-raised)]")}
                        style={{ background: a.value }}
                      />
                    ))}
                  </div>
                </Field>
                <Field label="Terminal colors" hint="“Match app” follows your theme; the others are well-known palettes.">
                  <div className="grid grid-cols-3 gap-1.5" role="radiogroup" aria-label="Terminal colors">
                    {schemes.map(([id, name]) => {
                      const t = resolveScheme(id, s.theme, s.accent);
                      return (
                        <button
                          key={id}
                          role="radio"
                          aria-checked={s.terminalScheme === id}
                          onClick={() => set({ terminalScheme: id })}
                          className={cn("flex items-center gap-2 rounded-[8px] px-2 py-1.5 text-left ring-1", s.terminalScheme === id ? "ring-2 ring-accent" : "ring-ink/[0.08]")}
                          style={{ background: t.background }}
                        >
                          <span className="flex shrink-0 gap-[2px]">
                            {[t.red, t.green, t.yellow, t.blue].map((c, i) => (
                              <span key={i} className="size-1.5 rounded-full" style={{ background: c }} />
                            ))}
                          </span>
                          <span className="truncate text-[11px] font-medium" style={{ color: t.foreground }}>
                            {name}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                </Field>
                <Field label="Terminal font and size">
                  <div className="flex items-center gap-3">
                    <select
                      aria-label="Font"
                      value={s.terminalFont}
                      onChange={(e) => set({ terminalFont: e.target.value })}
                      className="h-8 rounded-[8px] bg-ink/[0.05] px-2.5 text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] focus:ring-accent/70"
                    >
                      {TERMINAL_FONTS.map((f) => (
                        <option key={f.id} value={f.id}>
                          {f.name}
                        </option>
                      ))}
                    </select>
                    <div className="min-w-[150px] flex-1">
                      <Slider label="Text size" value={s.terminalFontSize} min={10} max={20} onChange={(v) => set({ terminalFontSize: v })} format={(v) => `${v} px`} />
                    </div>
                  </div>
                </Field>
              </Group>

              <Group title="Layout">
                <div className="grid grid-cols-2 gap-4">
                  <Field label="Tabs">
                    <div className="flex gap-2" role="radiogroup" aria-label="Where tabs sit">
                      <Card selected={s.tabsPlacement === "sidebar"} onClick={() => set({ tabsPlacement: "sidebar" })} title="Under each server">
                        <rect x="4" y="4" width="18" height="32" rx="2" {...fill} />
                        {[9, 15, 21].map((y) => (
                          <rect key={y} x={y === 9 ? 6 : 9} y={y} width={y === 9 ? 14 : 11} height="3" rx="1" {...(y === 15 ? strong : { fill: "currentColor", fillOpacity: 0.6 })} />
                        ))}
                      </Card>
                      <Card selected={s.tabsPlacement === "top"} onClick={() => set({ tabsPlacement: "top" })} title="Along the top">
                        {[4, 18, 32].map((x, i) => (
                          <rect key={x} x={x} y="4" width="12" height="5" rx="1.5" {...(i === 0 ? strong : fill)} />
                        ))}
                        <rect x="4" y="11" width="56" height="25" rx="2" {...fill} />
                      </Card>
                    </div>
                  </Field>
                  <Field label="Files and other tabs">
                    <div className="flex gap-2" role="radiogroup" aria-label="Tabs on screen">
                      <Card selected={s.viewMode === "tabs"} onClick={() => set({ viewMode: "tabs" })} title="One at a time">
                        <rect x="4" y="4" width="56" height="32" rx="2" {...strong} />
                      </Card>
                      <Card selected={s.viewMode === "split"} onClick={() => set({ viewMode: "split" })} title="Side by side">
                        <rect x="4" y="4" width="34" height="32" rx="2" {...strong} />
                        <rect x="41" y="4" width="19" height="32" rx="2" {...fill} />
                      </Card>
                    </div>
                  </Field>
                </div>
                <Field label="Server list" hint={s.viewMode === "tabs" ? `${shortcutLabel("toggleSplit")} or dragging a tab onto the page shows two side by side any time.` : undefined}>
                  <Segmented
                    value={s.sidebarMode}
                    onChange={(v) => set({ sidebarMode: v })}
                    options={[
                      { value: "wide", label: "Names" },
                      { value: "narrow", label: "Icons" },
                    ]}
                  />
                </Field>
              </Group>

              <Group title="Safety and privacy">
                <Toggle
                  label="Streamer mode"
                  hint={`Hides server names, addresses and users everywhere, for streams and screen shares. ${shortcutLabel("streamer")} any time.`}
                  checked={s.streamerMode}
                  onChange={(v) => v !== s.streamerMode && toggleStreamer()}
                />
                <Toggle
                  label="A colored line for Production and Staging"
                  hint="Labeled servers always show the word. This adds a thin red or yellow line along the top of the window."
                  checked={s.envStripe}
                  onChange={(v) => set({ envStripe: v })}
                />
              </Group>

              <section className="flex flex-col gap-4 pb-1">
                <button type="button" onClick={() => setMore((m) => !m)} aria-expanded={more} className="flex items-center gap-1.5 self-start text-[12.5px] font-medium text-fg-2 hover:text-fg">
                  <ChevronDown size={14} className={cn("transition-transform", !more && "-rotate-90")} /> More options
                </button>
                {more && (
                  <>
                    <Field label="Cursor">
                      <Segmented
                        value={s.cursorStyle}
                        onChange={(v) => set({ cursorStyle: v })}
                        options={[
                          { value: "bar", label: "Bar" },
                          { value: "block", label: "Block" },
                          { value: "underline", label: "Line" },
                        ]}
                      />
                    </Field>
                    <Field label="Sidebar side">
                      <Segmented
                        value={s.sidebarSide}
                        onChange={(v) => set({ sidebarSide: v })}
                        options={[
                          { value: "left", label: "Left" },
                          { value: "right", label: "Right" },
                        ]}
                      />
                    </Field>
                    <Field label="Server load" hint={s.railStatus === "off" ? "Servers show only whether they're connected." : "How busy each open server is, at a glance in the list."}>
                      <div className="flex gap-2" role="radiogroup" aria-label="Server load">
                        {LOAD_STYLES.map(({ id, title }) => (
                          <Card key={id} selected={s.railStatus === id} onClick={() => set({ railStatus: id })} title={title}>
                            <LoadPicture style={id} />
                          </Card>
                        ))}
                      </div>
                    </Field>
                    <Toggle
                      label="Compact mode"
                      hint={`Just your work. The sidebar and top row slide in at the window's edge. ${shortcutLabel("compact")} toggles it.`}
                      checked={s.compactMode}
                      onChange={(v) => set({ compactMode: v })}
                    />
                    <Field
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
                    </Field>
                    <Toggle
                      label="Tell me when a terminal needs me"
                      hint="When an AI agent or a program waits for an answer, or a long run goes quiet."
                      checked={s.notifyAttention}
                      onChange={(v) => set({ notifyAttention: v })}
                    />
                    <Toggle
                      label="Highlight errors and warnings"
                      hint="ERROR in red and WARN in yellow, so problems jump out of the output."
                      checked={s.highlightErrors || s.highlightWarnings}
                      onChange={(v) => set({ highlightErrors: v, highlightWarnings: v })}
                    />
                    <Field label="Logo" hint="In the sidebar's corner. The winking one blinks at you now and then.">
                      <Segmented
                        value={s.logo}
                        onChange={(v) => set({ logo: v })}
                        options={[
                          { value: "winking", label: "Winking" },
                          { value: "still", label: "Still" },
                          { value: "hidden", label: "Hidden" },
                        ]}
                      />
                    </Field>
                    <p className="text-[11.5px] text-fg-3">Your own theme colors, keyboard shortcuts and the rest are in Settings ({shortcutLabel("settings")}).</p>
                  </>
                )}
              </section>
            </div>

            <div className="sticky top-0 hidden w-[330px] shrink-0 flex-col gap-2 md:flex">
              <MiniApp />
              <p className="text-center text-[11px] text-fg-3">Your SSHeesh, as you set it up</p>
            </div>
          </div>

          <div className="mt-5 flex items-center gap-3">
            {returning ? (
              <>
                <Button variant="ghost" onClick={() => setStep("changed")}>
                  Back
                </Button>
                <span className="flex-1" />
              </>
            ) : (
              <>
                <Button variant="ghost" onClick={() => setStep("servers")}>
                  Back
                </Button>
                <Button variant="ghost" onClick={() => useSettings.getState().reset(MADE_YOURS)}>
                  Use defaults
                </Button>
                <span className="flex-1" />
              </>
            )}
            <Button variant="ghost" onClick={() => done(false)}>
              {returning ? "Done" : "Start using SSHeesh"}
            </Button>
            {!returning && (
              <Button variant="primary" autoFocus onClick={() => done(true)}>
                Take the 1-minute tour
              </Button>
            )}
          </div>
        </div>
      )}
    </Modal>
  );
}
