import type { ButtonHTMLAttributes, ComponentProps, ReactNode } from "react";
import { cn } from "../lib/format";
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
export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-baseline select-none", className)} aria-label="SSHeesh">
      <span className="font-mono text-[13px] font-semibold tracking-[-0.02em] text-fg">ssh</span>
      <span className="font-serif text-[16px] italic text-fg -ml-px">eesh</span>
    </span>
  );
}

export function Monogram({ name, health }: { name: string; health: Health }) {
  const letters = name
    .replace(/[^a-zA-Z0-9]+/g, " ")
    .trim()
    .split(" ")
    .map((w) => w[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();
  return (
    <span className="relative inline-flex size-[26px] shrink-0 items-center justify-center rounded-[7px] bg-gradient-to-b from-ink/[0.09] to-ink/[0.03] text-[10.5px] font-semibold tracking-wide text-fg-2 ring-1 ring-inset ring-ink/[0.07]">
      {letters || "?"}
      <StatusDot health={health} className="absolute -right-0.5 -bottom-0.5 rounded-full ring-2 ring-window" />
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
 * A grab strip on a panel's edge. Drag to resize, double-click to reset.
 * `edge` is the side of the panel the strip sits on.
 */
export function ResizeHandle({ edge, onResize, onReset, label }: { edge: "left" | "right"; onResize: (dx: number) => void; onReset?: () => void; label: string }) {
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
      if (dx) onResize(dx);
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
