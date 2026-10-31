// Small controls shared by Settings, the hello and the tool dialogs: a
// switch, a segmented choice and a slider. Kept apart from Settings so those
// can load without it.

import type { ReactNode } from "react";
import { cn } from "../lib/format";

/** The anchor Settings search scrolls to for a setting's label. */
export const settingId = (label: string) => `setting-${label.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={cn(
        // Never squeezed by a long label next to it.
        "relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors",
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
    <div className="flex h-8 w-fit items-center rounded-[9px] bg-ink/[0.05] p-[3px] ring-1 ring-inset ring-ink/[0.06]">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
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
