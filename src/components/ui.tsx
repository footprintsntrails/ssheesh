import { useEffect, useRef, useState, type ButtonHTMLAttributes, type ComponentProps, type ReactNode } from "react";
import { cn } from "../lib/format";
import { ENV_LABELS, type EnvLabel } from "../lib/labels";
import { useSettings } from "../lib/settings";
import type { SessionState } from "../lib/types";

type ButtonProps = ComponentProps<"button"> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
};

export function Button({ variant = "secondary", size = "md", className, ...rest }: ButtonProps) {
  return (
    <button
      {...rest}
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium whitespace-nowrap transition-[background,color,box-shadow,opacity] disabled:opacity-40",
        size === "sm" ? "h-7 px-2.5 text-[12.5px]" : "h-8 px-3.5 text-[13px]",
        variant === "primary" &&
          "bg-accent text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.25),0_1px_2px_rgb(0_0_0/0.4)] hover:brightness-110 active:brightness-95",
        variant === "secondary" &&
          "bg-ink/[0.07] text-fg shadow-[inset_0_1px_0_rgb(255_255_255/0.06)] ring-1 ring-ink/[0.06] hover:bg-ink/[0.1]",
        variant === "ghost" && "text-fg-2 hover:bg-ink/[0.06] hover:text-fg",
        variant === "danger" && "bg-bad text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.2)] hover:brightness-110",
        className,
      )}
    />
  );
}

export function IconButton({
  label,
  className,
  active,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean }) {
  return (
    <button
      {...rest}
      aria-label={label}
      title={label}
      className={cn(
        "inline-flex size-7 shrink-0 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-ink/[0.07] hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent",
        active && "bg-ink/[0.08] text-fg",
        className,
      )}
    />
  );
}

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-[5px] px-1 font-sans text-[10.5px] font-medium text-fg-3 ring-1 ring-ink/[0.08] bg-ink/[0.03]",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

export function Spinner({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" className={cn("size-3.5 animate-spin", className)} aria-hidden>
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeOpacity="0.2" strokeWidth="2" />
      <path d="M14 8a6 6 0 0 0-6-6" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

export type Health = "idle" | "live" | "busy" | "warn" | "bad";

export function healthOf(s: SessionState | undefined): Health {
  if (!s) return "idle";
  switch (s.kind) {
    case "connected":
      return "live";
    case "connecting":
      return "busy";
    case "reconnecting":
    case "disconnected":
      return "warn";
    case "failed":
      return "bad";
    default:
      return "idle";
  }
}

export function StatusDot({ health, className }: { health: Health; className?: string }) {
  const color = {
    idle: "bg-fg-3/50",
    live: "bg-ok",
    busy: "bg-fg-2",
    warn: "bg-warn",
    bad: "bg-bad",
  }[health];
  const pulse = health === "busy" || health === "warn";
  return (
    <span className={cn("relative inline-flex size-[7px] shrink-0", className)}>
      {pulse && <span className={cn("absolute inset-0 rounded-full opacity-60 animate-ping", color)} />}
      <span
        className={cn(
          "relative inline-flex size-[7px] rounded-full",
          color,
          health === "live" && "shadow-[0_0_8px_rgb(61_214_140/0.55)]",
        )}
      />
    </span>
  );
}

/** "ssh" in mono + "eesh" in serif italic: the name is the joke. */
/** "Prod", "Staging" or "Dev" in the label's color. */
export function EnvChip({ env, className }: { env: EnvLabel | null; className?: string }) {
  if (!env) return null;
  const l = ENV_LABELS[env];
  return (
    <span
      title={`Labeled ${l.name}`}
      className={cn("shrink-0 rounded-[4px] px-[5px] text-[10.5px] font-semibold leading-[16px] tracking-[0.01em]", l.text, l.tint, className)}
    >
      {l.short}
    </span>
  );
}



export type Mood = "ok" | "busy" | "lost" | "dead" | "done";

export function moodOf(s: SessionState | undefined): Mood {
  switch (s?.kind) {
    case "connected":
      return "ok";
    case "reconnecting":
    case "disconnected":
      return "lost";
    case "failed":
      return "dead";
    case "ended":
      return "done";
    default:
      return s && s.kind === "connecting" && s.attempt > 1 ? "lost" : "busy";
  }
}

const MOOD_LABEL: Record<Mood, string> = {
  ok: "Connected",
  busy: "Connecting",
  lost: "Connection lost, reconnecting",
  dead: "Couldn't connect",
  done: "Session ended",
};

/**
 * The logo as a status light: >_ all good, >_< connection lost,
 * x_x couldn't connect, -_- session ended. Shape, not only color.
 */
export function Face({ mood, className }: { mood: Mood; className?: string }) {
  const cursor = { ok: "var(--ui-ok)", busy: "var(--ui-fg-3)", lost: "var(--ui-warn)", dead: "var(--ui-bad)", done: "var(--ui-fg-3)" }[mood];
  const eye = { strokeWidth: 2.4, strokeLinecap: "square" as const, fill: "none", stroke: "currentColor" };
  return (
    <svg viewBox="0 0 30 16" className={cn("h-3.5 w-[26px] shrink-0 text-fg-2", className)} role="img" aria-label={MOOD_LABEL[mood]}>
      {(mood === "ok" || mood === "busy") && <polyline points="3,3 9,8 3,13" {...eye} />}
      {mood === "lost" && (
        <>
          <polyline points="3,3 9,8 3,13" {...eye} />
          <polyline points="27,3 21,8 27,13" {...eye} />
        </>
      )}
      {mood === "dead" && (
        <>
          <path d="M3 4 L9 11 M9 4 L3 11 M21 4 L27 11 M27 4 L21 11" {...eye} />
        </>
      )}
      {mood === "done" && <path d="M3 8 H9 M21 8 H27" {...eye} />}
      <rect
        x={mood === "ok" || mood === "busy" ? 12 : 11}
        y="11"
        width="8"
        height="3"
        style={{ fill: cursor }}
        className={cn(mood === "busy" && "animate-pulse")}
      />
    </svg>
  );
}

/**
 * The app's mark: a terminal window making a >_< face. When `winking`, every
 * so often it blinks, or winks with its right eye.
 */
export function Logo({ size = 20, winking = false, className }: { size?: number; winking?: boolean; className?: string }) {
  const [face, setFace] = useState<"open" | "blink" | "wink">("open");
  useEffect(() => {
    if (!winking) return;
    let timer = 0;
    const later = () => {
      timer = window.setTimeout(() => {
        const kind = Math.random() < 0.4 ? "wink" : "blink";
        setFace(kind);
        timer = window.setTimeout(() => {
          setFace("open");
          later();
        }, kind === "wink" ? 450 : 170);
      }, 4500 + Math.random() * 9000);
    };
    later();
    return () => window.clearTimeout(timer);
  }, [winking]);
  const eye = { fill: "none", stroke: "#f2f0ea", strokeWidth: 20, strokeLinecap: "square" as const };
  const left = face === "blink" ? "M32 112 L61 112" : "M32 86 L61 112 L32 138";
  const right = face === "open" ? "M168 86 L139 112 L168 138" : "M139 112 L168 112";
  return (
    <svg viewBox="0 0 200 200" width={size} height={size} role="img" aria-label="SSHeesh" className={cn("shrink-0", className)}>
      <g transform="translate(19.5 19.5) scale(0.805)">
        <rect x="8" y="8" width="184" height="184" rx="44" fill="#232428" />
        <path d="M52 8 H148 A44 44 0 0 1 192 52 V46 H8 V52 A44 44 0 0 1 52 8 Z" fill="#2f3136" />
        <rect x="8.5" y="8.5" width="183" height="183" rx="43.5" fill="none" stroke="#ffffff" strokeOpacity="0.09" />
        <circle cx="35" cy="27" r="5.5" fill="#5c5f67" />
        <circle cx="52" cy="27" r="5.5" fill="#5c5f67" />
        <circle cx="69" cy="27" r="5.5" fill="#5c5f67" />
        <path d={left} {...eye} />
        <path d={right} {...eye} />
        <rect x="76" y="140" width="48" height="20" fill="#3dd68c" />
      </g>
    </svg>
  );
}

/** The logo as Settings → Appearance asks for it, or nothing. */
export function AppLogo({ size, className }: { size?: number; className?: string }) {
  const style = useSettings((s) => s.logo);
  const animations = useSettings((s) => s.animations);
  if (style === "hidden") return null;
  return <Logo size={size} winking={style === "winking" && animations} className={className} />;
}

/**
 * A grab strip on a panel's edge. Drag to resize, double-click to reset.
 * `edge` is the side of the panel the strip sits on.
 */
export function ResizeHandle({ edge, onResize, onReset, label }: { edge: "left" | "right"; onResize: (dx: number) => void; onReset?: () => void; label: string }) {
  // A drag outlives the render it started in; always call the newest handler.
  const handler = useRef(onResize);
  handler.current = onResize;
  const start = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    let last = e.clientX;
    document.body.style.cursor = "col-resize";
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - last;
      last = ev.clientX;
      if (dx) handler.current(dx);
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      document.body.style.cursor = "";
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      title={`${label}. Double-click to reset.`}
      onPointerDown={start}
      onDoubleClick={onReset}
      className={cn(
        "group/handle absolute top-0 bottom-0 z-30 w-[9px] cursor-col-resize touch-none",
        edge === "right" ? "-right-[5px]" : "-left-[5px]",
      )}
    >
      <span className="absolute inset-y-0 left-1/2 w-[2px] -translate-x-1/2 rounded-full bg-transparent transition-colors group-hover/handle:bg-accent/70 group-active/handle:bg-accent" />
    </div>
  );
}
