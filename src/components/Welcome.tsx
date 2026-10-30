// First launch: a short hello in three steps. Pick a look, choose which
// servers to bring along, and three tips. Every step can be skipped.

import { Check, Command, MousePointer2, Plus, RotateCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { shortcutLabel } from "../lib/keys";
import { useLabels, useUndecidedConfig } from "../lib/labels";
import { useSettings } from "../lib/settings";
import { hostAddress, useStore } from "../lib/store";
import { ACCENTS, APP_THEMES } from "../lib/themes";
import { Modal } from "./Overlays";
import { Button, Logo } from "./ui";

type Step = 0 | 1 | 2;

export function Welcome() {
  const onboarded = useSettings((s) => s.onboarded);
  const hostsLoaded = useStore((s) => s.hostsLoaded);
  const hosts = useStore((s) => s.hosts);
  const undecided = useUndecidedConfig();
  const theme = useSettings((s) => s.theme);
  const accent = useSettings((s) => s.accent);
  const [step, setStep] = useState<Step>(0);
  const [keep, setKeep] = useState<string[]>([]);
  const [addAfter, setAddAfter] = useState(false);

  // The demo build skips the hello unless asked for it.
  const demoSkip = backend.isDemo && !new URLSearchParams(location.search).has("firstrun");
  useEffect(() => {
    if (demoSkip && !onboarded) useSettings.getState().update({ onboarded: true });
  }, [demoSkip, onboarded]);

  const configIds = useMemo(() => undecided.map((h) => h.id), [undecided]);
  useEffect(() => setKeep(configIds), [configIds]);

  const open = hostsLoaded && !onboarded && !demoSkip;
  const finish = () => {
    if (configIds.length) useLabels.getState().chooseConfig(configIds, keep);
    useSettings.getState().update({ onboarded: true });
    if (addAfter) useStore.getState().setAddHostOpen(true);
  };
  const skip = () => {
    // Skipping doesn't decide for you: new config servers wait behind a quiet note.
    useSettings.getState().update({ onboarded: true });
  };

  const dots = (
    <div className="flex gap-1.5" aria-label={`Step ${step + 1} of 3`}>
      {[0, 1, 2].map((i) => (
        <span key={i} className={cn("h-1.5 rounded-full transition-all", i === step ? "w-5 bg-accent" : "w-1.5 bg-ink/[0.18]")} />
      ))}
    </div>
  );

  return (
    <Modal open={open} onClose={skip} width={520}>
      <div className="flex min-h-[min(420px,calc(84vh-16px))] flex-col p-6">
        {step === 0 && (
          <>
            <Logo size={64} winking />
            <h2 className="mt-4 text-[22px] font-semibold tracking-tight text-fg">Welcome to SSHeesh</h2>
            <p className="mt-1.5 text-[13px] leading-relaxed text-fg-2">
              A terminal for your servers that you make your own. First, how should it look? You can change this any time in Settings.
            </p>
            <div className="mt-5 grid grid-cols-4 gap-2.5" role="radiogroup" aria-label="Theme">
              {APP_THEMES.map((t) => (
                <button
                  key={t.id}
                  role="radio"
                  aria-checked={theme === t.id}
                  onClick={() => useSettings.getState().update({ theme: t.id })}
                  className="group flex flex-col items-start gap-1.5 text-left"
                >
                  <span
                    className={cn("relative block h-[52px] w-full overflow-hidden rounded-[9px] ring-1", theme === t.id ? "ring-2 ring-accent" : "ring-ink/[0.1] group-hover:ring-ink/[0.2]")}
                    style={{ background: t.window }}
                  >
                    <span className="absolute top-2 right-2 bottom-2 left-[34%] rounded-[4px]" style={{ background: t.canvas }}>
                      <span className="mt-2 ml-2 block h-1 w-[40%] rounded-full" style={{ background: accent }} />
                    </span>
                  </span>
                  <span className={cn("text-[12px]", theme === t.id ? "font-medium text-fg" : "text-fg-2")}>{t.name}</span>
                </button>
              ))}
            </div>
            <div className="mt-4 flex items-center gap-2" role="radiogroup" aria-label="Accent color">
              {ACCENTS.map((a) => (
                <button
                  key={a.value}
                  role="radio"
                  aria-checked={accent === a.value}
                  aria-label={a.name}
                  title={a.name}
                  onClick={() => useSettings.getState().update({ accent: a.value })}
                  className="flex size-6 items-center justify-center rounded-full ring-1 ring-inset ring-black/20"
                  style={{ background: a.value }}
                >
                  {accent === a.value && <Check size={13} strokeWidth={3} className="text-white" />}
                </button>
              ))}
            </div>
          </>
        )}

        {step === 1 && (
          <>
            <h2 className="text-[20px] font-semibold tracking-tight text-fg">Your servers</h2>
            {configIds.length > 0 ? (
              <>
                <p className="mt-1.5 text-[13px] leading-relaxed text-fg-2">
                  You have {configIds.length} server{configIds.length === 1 ? "" : "s"} in ~/.ssh/config. Which ones should show up here? Your config file isn't changed.
                </p>
                <div className="mt-4 flex max-h-[220px] flex-col gap-0.5 overflow-y-auto rounded-[10px] p-1 ring-1 ring-inset ring-ink/[0.08]">
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
              <p className="mt-1.5 text-[13px] leading-relaxed text-fg-2">
                {hosts.length
                  ? `You already have ${hosts.length} server${hosts.length === 1 ? "" : "s"} here. Add more any time with the + in the sidebar.`
                  : "Add the first one: paste its address, or a whole ssh command, and SSHeesh fills in the rest."}
              </p>
            )}
            <label className="mt-4 flex cursor-pointer items-center gap-2.5 rounded-[10px] bg-ink/[0.04] px-3 py-2.5 text-[13px] text-fg">
              <input type="checkbox" checked={addAfter} onChange={(e) => setAddAfter(e.target.checked)} className="accent-[var(--ui-accent)]" />
              <Plus size={14} className="text-accent" /> Add a server when I'm done
            </label>
          </>
        )}

        {step === 2 && (
          <>
            <h2 className="text-[20px] font-semibold tracking-tight text-fg">Three things worth knowing</h2>
            <ul className="mt-4 flex flex-col gap-3.5">
              {[
                { icon: Command, title: `${shortcutLabel("palette")} finds anything`, body: "Servers, files, settings and actions, all from one search box." },
                { icon: MousePointer2, title: "Right-click almost anything", body: "Servers, files, containers and panes all have a menu of what you can do with them." },
                { icon: RotateCw, title: "Dropped connections aren't the end", body: "With tmux on the server, your shell keeps running. SSHeesh asks to reconnect and shows what you missed." },
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
          </>
        )}

        <div className="mt-auto flex items-center gap-3 pt-6">
          {dots}
          <span className="flex-1" />
          {step === 0 ? (
            <Button variant="ghost" onClick={skip}>
              Skip
            </Button>
          ) : (
            <Button variant="ghost" onClick={() => setStep((s) => (s - 1) as Step)}>
              Back
            </Button>
          )}
          {step < 2 ? (
            <Button variant="primary" onClick={() => setStep((s) => (s + 1) as Step)}>
              Next
            </Button>
          ) : (
            <Button variant="primary" onClick={finish}>
              Start
            </Button>
          )}
        </div>
      </div>
    </Modal>
  );
}
