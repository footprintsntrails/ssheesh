// The theme editor's color picker: a saturation and brightness square, hue
// and transparency sliders, a hex box and a row of ready colors.

import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { hsvToRgb, parseHex, rgbToHsv, toHex, type Hsv, type Rgba } from "../lib/color";
import { cn } from "../lib/format";

const HUES = "linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00)";
const CHECKS = "repeating-conic-gradient(#9a9a9a 0 25%, #d4d4d4 0 50%) 0 0 / 8px 8px";

/** A swatch's background: see-through colors show over a checkerboard. */
export function checkered(color: string) {
  return { background: `linear-gradient(${color}, ${color}), ${CHECKS}` };
}

const clamp01 = (n: number) => Math.min(1, Math.max(0, n));

/** Greys have no hue of their own: the hue slider stays where it was. */
function hsvOf(c: Rgba, hue: number): Hsv {
  const next = rgbToHsv(c);
  return next.s === 0 || next.v === 0 ? { ...next, h: hue } : next;
}

export function ColorPicker({
  value,
  onChange,
  alpha,
  palette,
  label,
}: {
  value: string;
  onChange: (hex: string) => void;
  /** It may be see-through: shows the transparency slider and takes #rrggbbaa. */
  alpha?: boolean;
  palette: { name: string; color: string }[];
  label: string;
}) {
  const start = parseHex(value) ?? { r: 0, g: 0, b: 0, a: 1 };
  const [hsv, setHsv] = useState<Hsv>(() => rgbToHsv(start));
  const [a, setA] = useState(start.a);
  const [draft, setDraft] = useState(value);
  const [error, setError] = useState("");
  const sent = useRef(value);

  // A change from outside (Reset, say) moves the controls.
  useEffect(() => {
    if (value.toLowerCase() === sent.current.toLowerCase()) return;
    sent.current = value;
    setDraft(value);
    setError("");
    const c = parseHex(value);
    if (!c) return;
    setHsv((h) => hsvOf(c, h.h));
    setA(c.a);
  }, [value]);

  // Dragging fires faster than the screen redraws: the app restyles at most once a frame.
  const frame = useRef(0);
  const pending = useRef(value);
  const latest = useRef(onChange);
  latest.current = onChange;
  useEffect(
    () => () => {
      if (!frame.current) return;
      cancelAnimationFrame(frame.current);
      latest.current(pending.current);
    },
    [],
  );

  const send = (next: Hsv, al: number, typed?: string) => {
    const hex = toHex(hsvToRgb(next, alpha ? al : 1));
    setHsv(next);
    setA(al);
    setDraft(typed ?? hex);
    setError("");
    sent.current = hex;
    pending.current = hex;
    if (!frame.current) {
      frame.current = requestAnimationFrame(() => {
        frame.current = 0;
        latest.current(pending.current);
      });
    }
  };

  const area = useRef<HTMLDivElement>(null);
  const pick = (e: PointerEvent) => {
    const r = area.current!.getBoundingClientRect();
    send({ h: hsv.h, s: clamp01((e.clientX - r.left) / r.width), v: 1 - clamp01((e.clientY - r.top) / r.height) }, a);
  };
  const nudge = (e: KeyboardEvent) => {
    const step = e.shiftKey ? 0.1 : 0.01;
    const d = ({ ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] } as Record<string, number[]>)[e.key];
    if (!d) return;
    e.preventDefault();
    send({ ...hsv, s: clamp01(hsv.s + d[0]), v: clamp01(hsv.v + d[1]) }, a);
  };

  const typed = (text: string) => {
    const c = parseHex(text);
    if (!c || (c.a < 1 && !alpha)) {
      setDraft(text);
      return;
    }
    send(hsvOf(c, hsv.h), c.a, text);
  };
  // Said once you're done typing, not at every half-typed digit.
  const check = () => {
    const c = parseHex(draft);
    if (!c) setError(alpha ? "Use #rrggbb, or #rrggbbaa for a see-through color." : "Use #rrggbb: six digits and letters a to f.");
    else if (c.a < 1 && !alpha) setError("This one can't be see-through. Use six digits after the #.");
  };

  const rgb = hsvToRgb(hsv);
  const solid = `rgb(${rgb.r} ${rgb.g} ${rgb.b})`;
  const invalid = draft !== sent.current && (!parseHex(draft) || (!alpha && parseHex(draft)!.a < 1));

  return (
    <div className="flex gap-4 rounded-[10px] bg-ink/[0.03] p-3 ring-1 ring-inset ring-ink/[0.07]">
      <div
        ref={area}
        role="slider"
        tabIndex={0}
        aria-label={`${label}: saturation and brightness`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(hsv.s * 100)}
        aria-valuetext={`Saturation ${Math.round(hsv.s * 100)}%, brightness ${Math.round(hsv.v * 100)}%`}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          e.currentTarget.focus();
          pick(e);
        }}
        onPointerMove={(e) => e.buttons & 1 && pick(e)}
        onKeyDown={nudge}
        className="relative h-[138px] w-[200px] shrink-0 touch-none rounded-[8px] ring-1 ring-inset ring-ink/[0.12] outline-none focus-visible:ring-2 focus-visible:ring-accent"
        style={{ background: `linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, ${toHex(hsvToRgb({ h: hsv.h, s: 1, v: 1 }))})` }}
      >
        <span
          className="pointer-events-none absolute size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full shadow-[0_0_0_2px_#fff,0_0_0_3px_rgb(0_0_0/0.35)]"
          style={{ left: `${hsv.s * 100}%`, top: `${(1 - hsv.v) * 100}%`, background: solid }}
        />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-3 pt-0.5">
        <input
          type="range"
          aria-label={`${label}: hue`}
          className="color-slider w-full"
          min={0}
          max={359}
          value={Math.round(hsv.h)}
          onChange={(e) => send({ ...hsv, h: Number(e.target.value) }, a)}
          style={{ background: HUES }}
        />
        {alpha && (
          <div className="flex items-center gap-2.5">
            <input
              type="range"
              aria-label={`${label}: how solid`}
              className="color-slider min-w-0 flex-1"
              min={0}
              max={100}
              value={Math.round(a * 100)}
              onChange={(e) => send(hsv, Number(e.target.value) / 100)}
              style={{ background: `linear-gradient(to right, rgb(${rgb.r} ${rgb.g} ${rgb.b} / 0), ${solid}), ${CHECKS}` }}
            />
            <span className="w-[68px] text-right text-[11.5px] tabular-nums text-fg-2">{a >= 1 ? "Solid" : `${Math.round(a * 100)}% solid`}</span>
          </div>
        )}
        <div className="flex items-center gap-2">
          <span className="size-7 shrink-0 rounded-[7px] ring-1 ring-inset ring-ink/[0.15]" style={checkered(sent.current)} />
          <input
            value={draft}
            onChange={(e) => typed(e.target.value)}
            onBlur={check}
            onKeyDown={(e) => e.key === "Enter" && check()}
            aria-label={`${label}: hex code`}
            aria-invalid={invalid || !!error}
            spellCheck={false}
            className={cn(
              "h-7 w-[112px] rounded-[7px] bg-ink/[0.05] px-2 font-mono text-[12px] text-fg outline-none ring-1 ring-inset",
              invalid || error ? "ring-bad/70" : "ring-ink/[0.08] focus:ring-accent/70",
            )}
          />
        </div>
        {error && <div className="-mt-1.5 text-[11.5px] leading-snug text-bad">{error}</div>}
        <div className="flex flex-wrap gap-1.5" role="group" aria-label={`Colors for ${label}`}>
          {palette.map((p) => (
            <button
              key={p.color}
              type="button"
              title={`${p.name} ${p.color}`}
              aria-label={`${p.name}, ${p.color}`}
              onClick={() => send(hsvOf(parseHex(p.color)!, hsv.h), a)}
              className="size-5 rounded-[5px] ring-1 ring-inset ring-ink/[0.18] transition-transform hover:scale-110"
              style={{ background: p.color }}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
