// Settings → Keyboard: every shortcut and its keys. Click one and press new
// keys; ones the terminal or the system needs are refused, with the reason.

import { Plus, RotateCcw, X } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "../lib/format";
import { settingId } from "./controls";
import {
  defaultKeys,
  isMac,
  isWindows,
  keysFromEvent,
  keysLabel,
  pauseShortcuts,
  sameKeys,
  SHORTCUT_GROUPS,
  SHORTCUT_NAMES,
  shortcutKeys,
  shortcutProblem,
  type Keys,
  type ShortcutId,
} from "../lib/keys";
import { useSettings } from "../lib/settings";

/** Modifiers held so far, while recording: "Ctrl+Shift+" or "⇧⌘". */
function heldLabel(e: KeyboardEvent): string {
  return keysLabel({ key: "", ctrl: e.ctrlKey, meta: e.metaKey, alt: e.altKey, shift: e.shiftKey });
}

// Keys that work the same everywhere and can't be changed, listed so the page has them all.
const FIXED: { name: string; note?: string; keys: Keys[] }[] = [
  { name: "Next tab", keys: [{ key: "tab", ctrl: true }] },
  { name: "Previous tab", keys: [{ key: "tab", ctrl: true, shift: true }] },
  ...(isMac
    ? [
        { name: "Clear the terminal", keys: [{ key: "k", meta: true }] },
        { name: "Copy", keys: [{ key: "c", meta: true }] },
        { name: "Paste", keys: [{ key: "v", meta: true }] },
      ]
    : [
        { name: "Copy in a terminal", note: "Ctrl+C copies too while text is selected.", keys: [{ key: "c", ctrl: true, shift: true }] },
        { name: "Paste in a terminal", keys: [{ key: "v", ctrl: true, shift: true }, ...(isWindows ? [{ key: "v", ctrl: true }] : [])] },
      ]),
  { name: "New line in AI coding agents", note: "Claude Code, OpenCode and Codex. A shell takes it as Enter.", keys: [{ key: "enter", shift: true }] },
];

function Keycap({ children, live, onClick, label }: { children: string; live?: boolean; onClick?: () => void; label?: string }) {
  const look = "inline-flex h-7 min-w-[56px] items-center justify-center rounded-[7px] px-2.5 text-[12px] font-medium whitespace-nowrap ring-1 ring-inset";
  if (!onClick) return <kbd className={cn(look, "bg-ink/[0.03] font-sans text-fg-3 ring-ink/[0.07]")}>{children}</kbd>;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      aria-pressed={live}
      className={cn(look, "transition-colors", live ? "bg-accent/15 text-fg ring-accent" : "bg-ink/[0.05] text-fg ring-ink/[0.1] hover:bg-ink/[0.09]")}
    >
      {children}
    </button>
  );
}

export function ShortcutsEditor() {
  const overrides = useSettings((s) => s.shortcuts);
  const update = useSettings((s) => s.update);
  const [recording, setRecording] = useState<ShortcutId | null>(null);
  const [held, setHeld] = useState("");
  const [problem, setProblem] = useState<{ id: ShortcutId; text: string } | null>(null);

  /** New keys for a shortcut; `undefined` gives it back its own. */
  const save = (id: ShortcutId, k: Keys | null | undefined) => {
    const next = { ...useSettings.getState().shortcuts };
    const own = defaultKeys(id);
    // Its own keys again need no entry, so Reset shows as done.
    if (k === undefined || (k === null ? own === null : !!own && sameKeys(k, own))) delete next[id];
    else next[id] = k;
    update({ shortcuts: next });
  };

  const record = (id: ShortcutId) => {
    setProblem(null);
    setHeld("");
    setRecording(recording === id ? null : id);
  };

  useEffect(() => {
    if (!recording) return;
    const stop = () => {
      setRecording(null);
      setHeld("");
    };
    // Shortcuts are paused, and stopping the keys here keeps them from the
    // terminal and Esc from closing Settings.
    const onDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const k = keysFromEvent(e);
      if (!k) return setHeld(heldLabel(e));
      const plain = !k.ctrl && !k.meta && !k.alt && !k.shift;
      if (plain && k.key === "escape") return stop();
      if (plain && (k.key === "backspace" || k.key === "delete") && recording === "paletteAlt") {
        save("paletteAlt", null);
        return stop();
      }
      const why = shortcutProblem(recording, k);
      setHeld("");
      if (why) return setProblem({ id: recording, text: why });
      setProblem(null);
      save(recording, k);
      stop();
    };
    const onUp = (e: KeyboardEvent) => setHeld(heldLabel(e));
    const onClick = () => stop();
    pauseShortcuts(true);
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    // A click anywhere else cancels; the keycap's own click lands first and is handled there.
    window.addEventListener("mousedown", onClick);
    return () => {
      pauseShortcuts(false);
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
      window.removeEventListener("mousedown", onClick);
    };
  }, [recording]);

  const reset = (ids: ShortcutId[]) => {
    for (const id of ids) {
      const own = defaultKeys(id);
      const why = own && shortcutProblem(id, own);
      if (why) return setProblem({ id: ids[0], text: why });
    }
    setProblem(null);
    const next = { ...useSettings.getState().shortcuts };
    for (const id of ids) delete next[id];
    update({ shortcuts: next });
  };

  const cap = (id: ShortcutId, name: string) => {
    const k = shortcutKeys(id);
    const live = recording === id;
    return (
      <Keycap live={live} onClick={() => record(id)} label={live ? `Recording keys for ${name}. Esc cancels.` : `${name}: ${k ? keysLabel(k) : "none"}. Click to change.`}>
        {live ? held || "Press keys…" : k ? keysLabel(k) : "None"}
      </Keycap>
    );
  };

  return (
    <>
      <p className="mt-1 mb-2 max-w-[520px] text-[11.5px] leading-snug text-fg-3">
        Click a shortcut and press the new keys; Esc cancels.{" "}
        {isMac
          ? "They use ⌘, so ⌃ and ⌥ stay with the terminal and with typing."
          : "They use Ctrl+Shift, so Ctrl with a letter, Alt and AltGr stay with the terminal."}
      </p>
      {SHORTCUT_GROUPS.map((g) => (
        <section key={g.title} aria-label={g.title}>
          <h4 className="mt-5 text-[13px] font-semibold text-fg">{g.title}</h4>
          {g.ids.map((id) => {
            const name = SHORTCUT_NAMES[id];
            const ids: ShortcutId[] = id === "palette" ? ["palette", "paletteAlt"] : [id];
            const changed = ids.some((x) => overrides[x] !== undefined);
            const second = id === "palette" ? shortcutKeys("paletteAlt") : null;
            const why = problem && ids.includes(problem.id) ? problem.text : null;
            return (
              // The id Settings' search scrolls to.
              <div key={id} id={settingId(name)} className="border-b border-line py-2.5 last:border-0">
                <div className="flex items-center gap-3">
                  <span className="min-w-0 flex-1 text-[13px] text-fg">{name}</span>
                  <div className="flex shrink-0 items-center gap-1.5">
                    {cap(id, name)}
                    {id === "palette" &&
                      (second || recording === "paletteAlt" ? (
                        <>
                          <span className="text-[11.5px] text-fg-3">or</span>
                          {cap("paletteAlt", "Command bar, second key")}
                          {recording !== "paletteAlt" && (
                            <button
                              aria-label="Remove the command bar's second key"
                              title="Remove this key"
                              onClick={() => save("paletteAlt", null)}
                              className="inline-flex size-6 items-center justify-center rounded-md text-fg-3 hover:bg-ink/[0.07] hover:text-fg"
                            >
                              <X size={12} />
                            </button>
                          )}
                        </>
                      ) : (
                        <button
                          onClick={() => record("paletteAlt")}
                          className="inline-flex h-7 items-center gap-1 rounded-[7px] px-2 text-[12px] text-fg-3 hover:bg-ink/[0.06] hover:text-fg-2"
                        >
                          <Plus size={12} /> Second key
                        </button>
                      ))}
                  </div>
                  <button
                    onClick={() => reset(ids)}
                    disabled={!changed}
                    aria-label={`Reset ${name}`}
                    title={`Back to ${ids.map((x) => defaultKeys(x)).filter((k): k is Keys => !!k).map((k) => keysLabel(k)).join(" or ")}`}
                    className={cn("inline-flex size-7 shrink-0 items-center justify-center rounded-md text-fg-3 hover:bg-ink/[0.07] hover:text-fg", !changed && "invisible")}
                  >
                    <RotateCcw size={12.5} />
                  </button>
                </div>
                {recording === "paletteAlt" && id === "palette" && !why && (
                  <div className="mt-1.5 text-[11.5px] text-fg-3">Backspace takes the second key away.</div>
                )}
                {why && (
                  <div role="alert" className="mt-1.5 text-[11.5px] leading-snug text-bad">
                    {why}
                  </div>
                )}
              </div>
            );
          })}
        </section>
      ))}
      <h4 className="mt-6 text-[13px] font-semibold text-fg">Always the same</h4>
      {FIXED.map((f) => (
        <div key={f.name} className="flex items-center gap-3 border-b border-line py-2.5 last:border-0">
          <div className="min-w-0 flex-1">
            <div className="text-[13px] text-fg-2">{f.name}</div>
            {f.note && <div className="mt-0.5 text-[11.5px] leading-snug text-fg-3">{f.note}</div>}
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            {f.keys.map((k, i) => (
              <span key={i} className="flex items-center gap-1.5">
                {i > 0 && <span className="text-[11.5px] text-fg-3">or</span>}
                <Keycap>{keysLabel(k)}</Keycap>
              </span>
            ))}
          </div>
          <span className="size-7 shrink-0" />
        </div>
      ))}
    </>
  );
}
