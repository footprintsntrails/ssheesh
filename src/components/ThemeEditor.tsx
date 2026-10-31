// Settings → Appearance → a theme of your own. Every color has a picker; the
// app restyles as you go. Text that gets hard to read is pointed out, not blocked.

import { ArrowLeft, Check, Copy, RotateCcw, Trash2, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { useSettings } from "../lib/settings";
import { useStore } from "../lib/store";
import { findTerminal } from "../lib/terminals";
import {
  ACCENTS,
  APP_THEMES,
  newCustomThemeId,
  THEME_COLORS,
  themeColors,
  themeFromJson,
  themeToJson,
  weakContrast,
  type AppThemeId,
  type CustomTheme,
  type CustomThemeId,
  type ThemeColorKey,
} from "../lib/themes";
import { checkered, ColorPicker } from "./ColorPicker";
import { Button } from "./ui";

/** What each background is called in a contrast warning. */
const ON: Partial<Record<ThemeColorKey, string>> = { window: "the window", canvas: "panes", raised: "raised controls", glass: "menus and dialogs" };

/**
 * Open terminals take a new pane color right away. They repaint by themselves
 * when the theme changes, but editing one keeps its name.
 */
function repaintTerminals() {
  for (const id of Object.keys(useStore.getState().panes)) findTerminal(id)?.applyLook();
}

export function ThemeEditor({ id, onClose, onUse }: { id: CustomThemeId; onClose: () => void; onUse: (id: AppThemeId) => void }) {
  const theme = useSettings((s) => s.customThemes.find((t) => t.id === id));
  const active = useSettings((s) => s.theme === id);
  const accent = useSettings((s) => s.accent);
  const follows = useSettings((s) => s.themeFollowsSystem);
  const [open, setOpen] = useState<ThemeColorKey | null>(null);
  const [copied, setCopied] = useState(false);
  /** Shown to copy by hand when the clipboard can't be reached. */
  const [shareText, setShareText] = useState("");
  const [deleting, setDeleting] = useState(false);
  const top = useRef<HTMLDivElement>(null);

  // Braces: newer webviews return a promise from scrollIntoView, which an effect mustn't.
  useEffect(() => {
    top.current?.scrollIntoView({ block: "start" });
  }, []);
  // Deleted (from another window, say): back to the list.
  useEffect(() => {
    if (!theme) onClose();
  }, [theme, onClose]);

  const repaint = useRef(0);
  useEffect(() => () => window.clearTimeout(repaint.current), []);

  const colors = useMemo(() => (theme ? themeColors(theme) : null), [theme]);
  const weak = useMemo(() => (colors ? weakContrast(colors) : []), [colors]);
  // The theme's own colors first, then the accents, black and white.
  const palette = useMemo(() => {
    const seen = new Set<string>();
    const out: { name: string; color: string }[] = [];
    const add = (name: string, color: string) => {
      const c = color.slice(0, 7).toLowerCase();
      if (seen.has(c)) return;
      seen.add(c);
      out.push({ name, color: c });
    };
    if (colors) for (const c of THEME_COLORS) add(c.label, colors[c.key]);
    add("Accent", accent);
    for (const a of ACCENTS) add(a.name, a.value);
    add("Black", "#000000");
    add("White", "#ffffff");
    return out;
  }, [colors, accent]);

  if (!theme || !colors) return null;

  const save = (patch: Partial<CustomTheme>) => {
    const st = useSettings.getState();
    st.update({ customThemes: st.customThemes.map((t) => (t.id === id ? { ...t, ...patch } : t)) });
    if (st.theme === id) {
      window.clearTimeout(repaint.current);
      repaint.current = window.setTimeout(repaintTerminals, 120);
    }
  };

  /** A new color, or null to go back to the base's (or, for lines, to following the window). */
  const setColor = (key: ThemeColorKey, hex: string | null) => {
    const t = useSettings.getState().customThemes.find((x) => x.id === id);
    if (!t) return;
    const rest = { ...t.colors };
    delete rest[key];
    // Picking the color it would have anyway needs no entry, so Reset shows as done.
    const fallback = themeColors({ ...t, colors: rest })[key];
    save({ colors: hex === null || hex === fallback ? rest : { ...rest, [key]: hex } });
  };

  const copy = () => {
    const json = themeToJson(theme);
    void backend.writeClipboard(json).then(
      () => {
        setShareText("");
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1600);
      },
      () => setShareText(json),
    );
  };

  const remove = () => {
    const st = useSettings.getState();
    const swap = (v: AppThemeId, fallback: AppThemeId) => (v === id ? fallback : v);
    const baseLight = APP_THEMES.find((t) => t.id === theme.base)?.light;
    st.update({
      customThemes: st.customThemes.filter((t) => t.id !== id),
      theme: swap(st.theme, theme.base),
      lightTheme: swap(st.lightTheme, baseLight ? theme.base : "daylight"),
      darkTheme: swap(st.darkTheme, baseLight ? "midnight" : theme.base),
    });
  };

  const baseName = APP_THEMES.find((t) => t.id === theme.base)?.name ?? "Midnight";
  const changed = Object.keys(theme.colors).length > 0;
  const groups = [...new Set(THEME_COLORS.map((c) => c.group))];

  return (
    <>
      <div ref={top} className="mb-2 scroll-mt-6 pr-10">
        <button onClick={onClose} className="flex items-center gap-1 text-[12px] text-fg-3 hover:text-fg-2">
          <ArrowLeft size={13} /> Appearance
        </button>
      </div>
      <input
        value={theme.name}
        onChange={(e) => save({ name: e.target.value.slice(0, 40) })}
        onBlur={() => !theme.name.trim() && save({ name: `My ${baseName}` })}
        aria-label="Theme name"
        spellCheck={false}
        className="-ml-2 h-9 w-[calc(100%-40px)] rounded-[8px] bg-transparent px-2 text-[17px] font-semibold tracking-tight text-fg outline-none ring-1 ring-inset ring-transparent hover:ring-ink/[0.1] focus:bg-ink/[0.04] focus:ring-accent/70"
      />
      <p className="mt-1 text-[11.5px] leading-snug text-fg-3">
        {active || !follows
          ? `Started from ${baseName}. Changes show everywhere as you make them and are saved right away.`
          : `Started from ${baseName}. It's not showing now because Match my system picked your other theme; turn that off in Appearance to see changes as you make them.`}
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        {!active && !follows && (
          <Button size="sm" variant="primary" onClick={() => onUse(id)}>
            Use this theme
          </Button>
        )}
        <Button size="sm" onClick={copy}>
          {copied ? <Check size={12.5} /> : <Copy size={12.5} />} {copied ? "Copied" : "Copy to share"}
        </Button>
        <Button size="sm" disabled={!changed} onClick={() => save({ colors: {} })}>
          <RotateCcw size={12} /> Reset all colors
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setDeleting(true)}>
          <Trash2 size={12.5} /> Delete
        </Button>
      </div>
      {shareText && (
        <div className="mt-3">
          <div className="mb-1.5 text-[11.5px] text-fg-3">The clipboard didn't answer. Select this and copy it yourself:</div>
          <textarea
            readOnly
            autoFocus
            value={shareText}
            onFocus={(e) => e.currentTarget.select()}
            rows={6}
            aria-label="Theme as text"
            className="block w-full resize-none rounded-[8px] bg-ink/[0.05] p-2.5 font-mono text-[12px] leading-snug text-fg outline-none ring-1 ring-inset ring-ink/[0.08] focus:ring-accent/70"
          />
        </div>
      )}
      {deleting && (
        <div role="alertdialog" aria-label="Delete theme" className="mt-3 flex flex-wrap items-center gap-2 rounded-[10px] bg-bad/10 px-3.5 py-2.5 text-[12.5px] text-fg">
          <span className="min-w-0 flex-1">Delete “{theme.name}”? Anything using it goes back to {baseName}.</span>
          <Button size="sm" variant="ghost" onClick={() => setDeleting(false)}>
            Keep it
          </Button>
          <Button size="sm" variant="danger" onClick={remove}>
            Delete
          </Button>
        </div>
      )}
      {weak.length > 0 && (
        <div role="status" className="mt-3 flex items-start gap-2.5 rounded-[10px] bg-warn/10 px-3.5 py-3 text-[12px] leading-snug text-fg-2">
          <TriangleAlert size={14} className="mt-px shrink-0 text-warn" />
          <span>Some text may be hard to read: the marked text colors are below 4.5:1 against what they sit on. You can keep them; it's your call.</span>
        </div>
      )}
      {groups.map((group) => (
        <section key={group} aria-label={group}>
          <h4 className="mt-6 text-[13px] font-semibold text-fg">{group}</h4>
          {THEME_COLORS.filter((c) => c.group === group).map((c) => {
            const own = theme.colors[c.key] !== undefined;
            const low = weak.filter((w) => w.text === c.key);
            return (
              <div key={c.key} className="border-b border-line py-2.5 last:border-0">
                <div className="flex items-center gap-2">
                  <button
                    onClick={() => setOpen(open === c.key ? null : c.key)}
                    aria-expanded={open === c.key}
                    className="flex min-w-0 flex-1 items-center gap-3 rounded-[8px] text-left outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                  >
                    <span className="size-7 shrink-0 rounded-[7px] ring-1 ring-inset ring-ink/[0.15]" style={checkered(colors[c.key])} />
                    <span className="min-w-0 flex-1">
                      <span className="block text-[13px] text-fg">
                        {c.label}
                        {own && <span className="ml-1.5 text-[11px] text-fg-3">changed</span>}
                      </span>
                      <span className="mt-0.5 block text-[11.5px] leading-snug text-fg-3">{c.hint}</span>
                      {low.length > 0 && (
                        <span className="mt-1 flex items-center gap-1 text-[11.5px] leading-snug text-warn">
                          <TriangleAlert size={12} className="shrink-0" />
                          Hard to read on {low.map((w) => `${ON[w.on]} (${w.ratio.toFixed(1)}:1)`).join(", ")}
                        </span>
                      )}
                    </span>
                    <span className="shrink-0 font-mono text-[12px] text-fg-2">{colors[c.key]}</span>
                  </button>
                  <button
                    onClick={() => setColor(c.key, null)}
                    disabled={!own}
                    aria-label={`Reset ${c.label}`}
                    title={c.key === "ink" ? "Follow the window again" : `Back to ${baseName}'s color`}
                    className={cn("inline-flex size-7 shrink-0 items-center justify-center rounded-md text-fg-3 hover:bg-ink/[0.07] hover:text-fg", !own && "invisible")}
                  >
                    <RotateCcw size={12.5} />
                  </button>
                </div>
                {open === c.key && (
                  <div className="mt-2.5 mr-9">
                    <ColorPicker label={c.label} value={colors[c.key]} alpha={c.alpha} palette={palette} onChange={(hex) => setColor(c.key, hex)} />
                  </div>
                )}
              </div>
            );
          })}
        </section>
      ))}
    </>
  );
}

/** Paste a theme someone shared (Copy to share, on their computer) to add it. */
export function ThemeImport({ onAdd, onCancel }: { onAdd: (t: CustomTheme) => void; onCancel: () => void }) {
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const add = () => {
    const r = themeFromJson(text);
    if ("error" in r) setError(r.error);
    else onAdd({ id: newCustomThemeId(), ...r.theme });
  };
  return (
    <div className="mt-3 rounded-[10px] bg-ink/[0.03] p-3 ring-1 ring-inset ring-ink/[0.07]">
      <textarea
        autoFocus
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setError("");
        }}
        rows={5}
        spellCheck={false}
        aria-label="Theme to import"
        placeholder={'Paste a theme here. It starts with { "ssheesh": "theme"'}
        className="block w-full resize-none rounded-[8px] bg-ink/[0.05] p-2.5 font-mono text-[12px] leading-snug text-fg outline-none ring-1 ring-inset ring-ink/[0.08] placeholder:text-fg-3 focus:ring-accent/70"
      />
      {error && (
        <div role="alert" className="mt-1.5 text-[11.5px] leading-snug text-bad">
          {error}
        </div>
      )}
      <div className="mt-2.5 flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button size="sm" variant="primary" disabled={!text.trim()} onClick={add}>
          Add theme
        </Button>
      </div>
    </div>
  );
}
