// First launch: a guided tour of the main ways to make SSHeesh yours, with a
// live preview of the app next to the choices. Everything applies at once,
// every step can be skipped, and all of it lives in Settings later.

import { Check, Command, MousePointer2, Plus, RotateCw } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { useLabels, useUndecidedConfig } from "../lib/labels";
import { useSettings, type Settings } from "../lib/settings";
import { hostAddress, useStore } from "../lib/store";
import { ACCENTS, APP_THEMES, resolveScheme, TERMINAL_FONTS, TERMINAL_SCHEMES } from "../lib/themes";
import { MiniApp } from "./MiniApp";
import { Modal } from "./Overlays";
import { Segmented, Slider, Switch } from "./SettingsDialog";
import { Button, Logo } from "./ui";

const STEPS = ["Welcome", "Colors", "Terminal", "Layout", "Behavior", "Servers", "Tips"] as const;

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

export function Welcome() {
  const onboarded = useSettings((s) => s.onboarded);
  const s = useSettings();
  const hostsLoaded = useStore((st) => st.hostsLoaded);
  const hosts = useStore((st) => st.hosts);
  const undecided = useUndecidedConfig();
  const [step, setStep] = useState(0);
  const [keep, setKeep] = useState<string[]>([]);
  const [addAfter, setAddAfter] = useState(false);

  // The demo build skips the hello unless asked for it.
  const demoSkip = backend.isDemo && !new URLSearchParams(location.search).has("firstrun");
  useEffect(() => {
    if (demoSkip && !onboarded) set({ onboarded: true });
  }, [demoSkip, onboarded]);

  const configIds = useMemo(() => undecided.map((h) => h.id), [undecided]);
  useEffect(() => setKeep(configIds), [configIds]);

  const open = hostsLoaded && !onboarded && !demoSkip;
  const last = STEPS.length - 1;
  const finish = () => {
    if (configIds.length) useLabels.getState().chooseConfig(configIds, keep);
    set({ onboarded: true });
    if (addAfter) useStore.getState().setAddHostOpen(true);
  };
  // Skipping doesn't decide for you: new config servers wait behind a quiet note.
  const skip = () => set({ onboarded: true });
  const focus = step === 2 ? "terminal" : step === 5 ? "sidebar" : undefined;
  const schemes = [["auto", "Match app"] as const, ...Object.entries(TERMINAL_SCHEMES).map(([id, v]) => [id, v.name] as const)];

  return (
    <Modal open={open} onClose={skip} width={860}>
      <div className="flex min-h-[min(480px,calc(84vh-16px))] flex-col p-6">
        <div className="mb-4 flex items-center gap-2 text-[11.5px] text-fg-3">
          {STEPS.map((name, i) => (
            <button
              key={name}
              type="button"
              onClick={() => setStep(i)}
              aria-current={i === step || undefined}
              className={cn("rounded-full px-2 py-0.5 transition-colors", i === step ? "bg-accent/15 font-medium text-accent" : "hover:text-fg-2")}
            >
              {name}
            </button>
          ))}
        </div>

        <div className="flex min-h-0 flex-1 items-start gap-6">
          <div className="flex min-w-0 flex-1 flex-col gap-5">
            {step === 0 && (
              <div className="flex flex-col items-start">
                <Logo size={72} winking />
                <h2 className="mt-4 text-[24px] font-semibold tracking-tight text-fg">Welcome to SSHeesh</h2>
                <p className="mt-2 max-w-[440px] text-[13.5px] leading-relaxed text-fg-2">
                  A terminal for your servers that you make your own. Let's set it up the way you like it: colors, terminal, layout and
                  how it behaves. It takes about a minute, you'll see every change as you make it, and all of it stays in Settings for later.
                </p>
              </div>
            )}

            {step === 1 && (
              <>
                <h2 className="text-[20px] font-semibold tracking-tight text-fg">Colors</h2>
                <Field label="Theme">
                  <div className="grid grid-cols-4 gap-2" role="radiogroup" aria-label="Theme">
                    {APP_THEMES.map((t) => (
                      <button
                        key={t.id}
                        role="radio"
                        aria-checked={s.theme === t.id}
                        onClick={() =>
                          set(s.themeFollowsSystem ? { theme: t.id, ...(t.light ? { lightTheme: t.id } : { darkTheme: t.id }) } : { theme: t.id })
                        }
                        className="group flex flex-col items-start gap-1 text-left"
                      >
                        <span
                          className={cn("relative block h-[44px] w-full overflow-hidden rounded-[8px] ring-1", s.theme === t.id ? "ring-2 ring-accent" : "ring-ink/[0.1] group-hover:ring-ink/[0.2]")}
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
                <label className="flex items-center justify-between gap-3">
                  <span>
                    <span className="block text-[12.5px] font-medium text-fg">Match my system</span>
                    <span className="block text-[11.5px] text-fg-3">Light by day, dark by night, as your system switches.</span>
                  </span>
                  <Switch label="Match my system" checked={s.themeFollowsSystem} onChange={(v) => set({ themeFollowsSystem: v })} />
                </label>
                <Field label="Accent color" hint="Buttons, selection and focus.">
                  <div className="flex items-center gap-2" role="radiogroup" aria-label="Accent color">
                    {ACCENTS.map((a) => (
                      <button
                        key={a.value}
                        role="radio"
                        aria-checked={s.accent === a.value}
                        aria-label={a.name}
                        title={a.name}
                        onClick={() => set({ accent: a.value })}
                        className="flex size-6 items-center justify-center rounded-full ring-1 ring-inset ring-black/20"
                        style={{ background: a.value }}
                      >
                        {s.accent === a.value && <Check size={13} strokeWidth={3} className="text-white" />}
                      </button>
                    ))}
                  </div>
                </Field>
              </>
            )}

            {step === 2 && (
              <>
                <h2 className="text-[20px] font-semibold tracking-tight text-fg">Terminal</h2>
                <Field label="Color scheme" hint="“Match app” follows your theme; the others are well-known palettes.">
                  <div className="grid grid-cols-3 gap-1.5" role="radiogroup" aria-label="Color scheme">
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
                <div className="grid grid-cols-2 gap-4">
                  <Field label="Font">
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
                  </Field>
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
                </div>
                <Field label="Text size">
                  <Slider label="Text size" value={s.terminalFontSize} min={10} max={20} onChange={(v) => set({ terminalFontSize: v })} format={(v) => `${v} px`} />
                </Field>
              </>
            )}

            {step === 3 && (
              <>
                <h2 className="text-[20px] font-semibold tracking-tight text-fg">Layout</h2>
                <div className="grid grid-cols-2 gap-4">
                  <Field label="Server list" hint="Icons save room.">
                    <div className="flex gap-2" role="radiogroup" aria-label="Server list">
                      <Card selected={s.sidebarMode === "wide"} onClick={() => set({ sidebarMode: "wide" })} title="Names">
                        <rect x="4" y="4" width="20" height="32" rx="2" {...fill} />
                        {[9, 15, 21, 27].map((y) => (
                          <rect key={y} x="7" y={y} width="14" height="3" rx="1" fill="currentColor" fillOpacity="0.6" />
                        ))}
                      </Card>
                      <Card selected={s.sidebarMode === "narrow"} onClick={() => set({ sidebarMode: "narrow" })} title="Icons">
                        <rect x="4" y="4" width="8" height="32" rx="2" {...fill} />
                        {[8, 15, 22, 29].map((y) => (
                          <rect key={y} x="5.5" y={y} width="5" height="5" rx="1.2" {...strong} />
                        ))}
                      </Card>
                    </div>
                  </Field>
                  <Field label="Sidebar side" hint="Where the list sits.">
                    <div className="flex gap-2" role="radiogroup" aria-label="Sidebar side">
                      <Card selected={s.sidebarSide === "left"} onClick={() => set({ sidebarSide: "left" })} title="Left">
                        <rect x="4" y="4" width="14" height="32" rx="2" {...strong} />
                      </Card>
                      <Card selected={s.sidebarSide === "right"} onClick={() => set({ sidebarSide: "right" })} title="Right">
                        <rect x="46" y="4" width="14" height="32" rx="2" {...strong} />
                      </Card>
                    </div>
                  </Field>
                </div>
                <Field
                  label="Server load"
                  hint={
                    s.railStatus === "off"
                      ? "Servers show only whether they're connected."
                      : "How busy each open server is. The list shows small CPU and memory meters; icons show the style you pick."
                  }
                >
                  <div className="flex gap-2" role="radiogroup" aria-label="Server load">
                    {LOAD_STYLES.map(({ id, title }) => (
                      <Card key={id} selected={s.railStatus === id} onClick={() => set({ railStatus: id })} title={title}>
                        <LoadPicture style={id} />
                      </Card>
                    ))}
                  </div>
                </Field>
                <Field label="Bookmarks in the file manager">
                  <div className="flex gap-2" role="radiogroup" aria-label="Bookmarks column">
                    {(["wide", "narrow", "hidden"] as const).map((m) => (
                      <Card key={m} selected={s.placesMode === m} onClick={() => set({ placesMode: m })} title={m === "wide" ? "Wide" : m === "narrow" ? "Narrow" : "Hidden"}>
                        {m !== "hidden" && <rect x="4" y="4" width={m === "wide" ? 18 : 8} height="32" rx="2" {...strong} />}
                        {[10, 17, 24].map((y) => (
                          <rect key={y} x={m === "wide" ? 26 : m === "narrow" ? 16 : 8} y={y} width="30" height="3" rx="1" fill="currentColor" fillOpacity="0.5" />
                        ))}
                      </Card>
                    ))}
                  </div>
                </Field>
                <div className="flex flex-col gap-3">
                  <label className="flex items-center justify-between gap-3">
                    <span>
                      <span className="block text-[12.5px] font-medium text-fg">Server vitals</span>
                      <span className="block text-[11.5px] text-fg-3">A one-line CPU, memory, network and disk readout under your panes.</span>
                    </span>
                    <Switch label="Server vitals" checked={s.showVitals} onChange={(v) => set({ showVitals: v })} />
                  </label>
                  <label className="flex items-center justify-between gap-3">
                    <span>
                      <span className="block text-[12.5px] font-medium text-fg">Compact mode</span>
                      <span className="block text-[11.5px] text-fg-3">
                        Just your panes. The sidebar and toolbar slide in when you point at the window's edge. {shortcutLabel("compact")} toggles it.
                      </span>
                    </span>
                    <Switch label="Compact mode" checked={s.compactMode} onChange={(v) => set({ compactMode: v })} />
                  </label>
                </div>
              </>
            )}

            {step === 4 && (
              <>
                <h2 className="text-[20px] font-semibold tracking-tight text-fg">Behavior</h2>
                <Field
                  label="When the connection drops"
                  hint={
                    s.reconnectMode === "auto"
                      ? "Reconnects on its own and lands you back in the same session."
                      : s.reconnectMode === "ask"
                        ? "Shows a Reconnect button over the terminal and waits for you."
                        : "Stays quiet: the pane shows Disconnected with a small Reconnect button."
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
                <label className="flex items-center justify-between gap-3">
                  <span>
                    <span className="block text-[12.5px] font-medium text-fg">Tell me when a terminal needs me</span>
                    <span className="block text-[11.5px] text-fg-3">When an AI agent or a program waits for an answer, or a long run goes quiet.</span>
                  </span>
                  <Switch label="Tell me when a terminal needs me" checked={s.notifyAttention} onChange={(v) => set({ notifyAttention: v })} />
                </label>
                <label className="flex items-center justify-between gap-3">
                  <span>
                    <span className="block text-[12.5px] font-medium text-fg">Highlight errors and warnings</span>
                    <span className="block text-[11.5px] text-fg-3">ERROR in red and WARN in yellow, so problems jump out of the output.</span>
                  </span>
                  <Switch
                    label="Highlight errors and warnings"
                    checked={s.highlightErrors || s.highlightWarnings}
                    onChange={(v) => set({ highlightErrors: v, highlightWarnings: v })}
                  />
                </label>
                <Field label="Logo" hint="In the sidebar's corner. The winking one blinks at you now and then.">
                  <div className="flex items-center gap-3">
                    {s.logo !== "hidden" && <Logo size={30} winking={s.logo === "winking"} />}
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
                </Field>
              </>
            )}

            {step === 5 && (
              <>
                <h2 className="text-[20px] font-semibold tracking-tight text-fg">Your servers</h2>
                {configIds.length > 0 ? (
                  <>
                    <p className="text-[13px] leading-relaxed text-fg-2">
                      You have {configIds.length} server{configIds.length === 1 ? "" : "s"} in ~/.ssh/config. Which ones should show up here? Your
                      config file isn't changed.
                    </p>
                    <div className="flex max-h-[220px] flex-col gap-0.5 overflow-y-auto rounded-[10px] p-1 ring-1 ring-inset ring-ink/[0.08]">
                      {undecided.map((h) => (
                        <label key={h.id} className="flex h-10 cursor-pointer items-center gap-3 rounded-[7px] px-2.5 hover:bg-ink/[0.04]">
                          <input
                            type="checkbox"
                            checked={keep.includes(h.id)}
                            onChange={() => setKeep((k) => (k.includes(h.id) ? k.filter((x) => x !== h.id) : [...k, h.id]))}
                            className="accent-[var(--ui-accent)]"
                          />
                          <span className="min-w-0 flex-1">
                            <span className="block truncate text-[13px] text-fg">{h.name}</span>
                            {hostAddress(h) !== h.name && <span className="block truncate font-mono text-[11px] text-fg-3">{hostAddress(h)}</span>}
                          </span>
                        </label>
                      ))}
                    </div>
                  </>
                ) : (
                  <p className="text-[13px] leading-relaxed text-fg-2">
                    {hosts.length
                      ? `You already have ${hosts.length} server${hosts.length === 1 ? "" : "s"} here. Add more any time with the + in the sidebar.`
                      : "Add the first one: paste its address, or a whole ssh command, and SSHeesh fills in the rest."}
                  </p>
                )}
                <label className="flex cursor-pointer items-center gap-2.5 rounded-[10px] bg-ink/[0.04] px-3 py-2.5 text-[13px] text-fg">
                  <input type="checkbox" checked={addAfter} onChange={(e) => setAddAfter(e.target.checked)} className="accent-[var(--ui-accent)]" />
                  <Plus size={14} className="text-accent" /> Add a server when I'm done
                </label>
              </>
            )}

            {step === 6 && (
              <>
                <h2 className="text-[20px] font-semibold tracking-tight text-fg">Three things worth knowing</h2>
                <ul className="flex flex-col gap-3.5">
                  {[
                    { icon: Command, title: `${shortcutLabel("palette")} finds anything`, body: "Servers, files, settings and actions, all from one search box." },
                    { icon: MousePointer2, title: "Right-click almost anything", body: "Servers, files, containers and panes all have a menu of what you can do with them." },
                    { icon: RotateCw, title: "Dropped connections aren't the end", body: "With tmux on the server, your shell keeps running, and SSHeesh shows what you missed." },
                  ].map(({ icon: I, title, body }) => (
                    <li key={title} className="flex gap-3">
                      <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-[9px] bg-accent/12 text-accent">
                        <I size={15} />
                      </span>
                      <span>
                        <span className="block text-[13.5px] font-medium text-fg">{title}</span>
                        <span className="block text-[12.5px] leading-snug text-fg-3">{body}</span>
                      </span>
                    </li>
                  ))}
                </ul>
                <p className="text-[12.5px] text-fg-3">Everything you picked is in Settings ({shortcutLabel("settings")}), with a search box and a Reset in every section.</p>
              </>
            )}
          </div>

          <div className="sticky top-0 hidden w-[320px] shrink-0 flex-col gap-2 md:flex">
            <MiniApp focus={focus} />
            <p className="text-center text-[11px] text-fg-3">Your SSHeesh, as you set it up</p>
          </div>
        </div>

        <div className="mt-auto flex items-center gap-3 pt-6">
          <span className="text-[11.5px] text-fg-3 tabular-nums">
            {step + 1} of {STEPS.length}
          </span>
          <span className="flex-1" />
          {step === 0 ? (
            <Button variant="ghost" onClick={skip}>
              Skip setup
            </Button>
          ) : (
            <Button variant="ghost" onClick={() => setStep((x) => x - 1)}>
              Back
            </Button>
          )}
          {step < last ? (
            <Button variant="primary" onClick={() => setStep((x) => x + 1)}>
              {step === 0 ? "Let's go" : "Next"}
            </Button>
          ) : (
            <Button variant="primary" onClick={finish}>
              Start using SSHeesh
            </Button>
          )}
        </div>
      </div>
    </Modal>
  );
}
