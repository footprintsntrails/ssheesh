import { ChevronDown, CircleAlert, History, RotateCw, ShieldAlert, ShieldQuestion, Wifi, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../lib/format";
import { useStore, type PaneState, type Recap } from "../lib/store";
import { lineTone } from "../lib/highlight";
import { useNames } from "../lib/privacy";
import { useSettings } from "../lib/settings";
import { getTerminal } from "../lib/terminals";
import { resolveScheme } from "../lib/themes";
import type { Fingerprint, HostKeyCheck, SessionState } from "../lib/types";
import { Button, Face } from "./ui";

function useCountdown(state: SessionState | undefined): number {
  const [left, setLeft] = useState(0);
  useEffect(() => {
    if (state?.kind !== "reconnecting") return;
    const until = Date.now() + state.retryInMs;
    const tick = () => setLeft(Math.max(0, Math.ceil((until - Date.now()) / 1000)));
    tick();
    const t = setInterval(tick, 250);
    return () => clearInterval(t);
  }, [state]);
  return left;
}

/** Shows briefly after a dropped connection comes back. */
function useBackOnline(state: SessionState | undefined): SessionState | null {
  const prev = useRef<SessionState | undefined>(undefined);
  const [shown, setShown] = useState<SessionState | null>(null);
  useEffect(() => {
    const was = prev.current;
    prev.current = state;
    const wasDown = was?.kind === "reconnecting" || was?.kind === "disconnected" || (was?.kind === "connecting" && was.attempt > 1);
    if (state?.kind === "connected" && wasDown) {
      setShown(state);
      const t = setTimeout(() => setShown(null), 3200);
      return () => clearTimeout(t);
    }
  }, [state]);
  return shown;
}

function awayFor(ms: number): string {
  const min = Math.round(ms / 60000);
  if (min < 1) return "less than a minute";
  if (min < 60) return `${min} min`;
  return `${Math.floor(min / 60)} h ${min % 60} min`;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** What a durable session printed while the connection was down. */
function RecapCard({ recap, onShow, onClose }: { recap: Recap; onShow: () => void; onClose: () => void }) {
  const parts = [plural(recap.lines.length, "new line")];
  if (recap.errors) parts.push(plural(recap.errors, "error"));
  if (recap.warnings) parts.push(plural(recap.warnings, "warning"));
  return (
    <div className="pointer-events-none absolute inset-x-0 top-3 flex justify-center px-3">
      <div className="glass pointer-events-auto flex max-w-full animate-pop-in items-center gap-3 rounded-[14px] py-2 pr-2 pl-3">
        <Wifi size={14} className="shrink-0 text-ok" />
        <div className="min-w-0">
          <div className="truncate text-[12.5px] font-medium text-fg">
            While you were away <span className="font-normal text-fg-3">· {awayFor(recap.until - recap.since)}</span>
          </div>
          <div className="truncate text-[11.5px] text-fg-3">
            {parts.map((p, i) => (
              <span key={p} className={cn(i > 0 && p.includes("error") && "font-medium text-bad", i > 0 && p.includes("warning") && "text-warn")}>
                {i > 0 && " · "}
                {p}
              </span>
            ))}
          </div>
        </div>
        <Button size="sm" variant="primary" onClick={onShow}>
          <History size={12.5} /> Show
        </Button>
        <button
          type="button"
          aria-label="Dismiss"
          onClick={onClose}
          className="inline-flex size-7 items-center justify-center rounded-md text-fg-3 hover:bg-ink/[0.07] hover:text-fg"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}

function RecapPanel({ recap, onClose }: { recap: Recap; onClose: () => void }) {
  const names = useNames();
  const time = (t: number) => new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return (
    <div className="absolute inset-x-2 bottom-2 z-10 flex max-h-[60%] animate-slide-up flex-col overflow-hidden rounded-[12px] glass">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-line pr-1.5 pl-3">
        <History size={13.5} className="text-fg-3" />
        <span className="text-[12.5px] font-medium text-fg">While you were away</span>
        <span className="text-[11.5px] text-fg-3 tabular-nums">
          {time(recap.since)} to {time(recap.until)}
        </span>
        <span className="flex-1" />
        <button
          type="button"
          aria-label="Close"
          onClick={onClose}
          className="inline-flex size-7 items-center justify-center rounded-md text-fg-3 hover:bg-ink/[0.07] hover:text-fg"
        >
          <ChevronDown size={15} />
        </button>
      </div>
      <pre className="min-h-0 flex-1 overflow-auto px-3 py-2 font-mono text-[11.5px] leading-[1.55] text-fg-2 select-text">
        {recap.lines.map((l, i) => (
          <div key={i} className={cn(lineTone(l) === "error" && "text-bad", lineTone(l) === "warning" && "text-warn")}>
            {names.text(l) || " "}
          </div>
        ))}
      </pre>
    </div>
  );
}

function FingerprintList({ label, items, tone }: { label: string; items: Fingerprint[]; tone?: "old" | "new" }) {
  return (
    <div className="mt-3">
      <div className="text-[11px] font-semibold text-fg-3">{label}</div>
      {items.map((f) => (
        <div
          key={f.sha256}
          className={cn(
            "mt-1 flex items-baseline gap-2 rounded-[7px] bg-ink/[0.05] px-2.5 py-1.5 font-mono text-[11.5px] break-all select-text",
            tone === "old" && "text-fg-3 line-through decoration-fg-3/60",
            tone !== "old" && "text-fg",
          )}
        >
          <span className="shrink-0 text-fg-3 no-underline">{f.kind || "key"}</span>
          {f.sha256}
        </div>
      ))}
    </div>
  );
}

/** Asked once per server instead of ssh's yes/no prompt in the terminal. */
function HostKeyCard({ check, name, onAnswer }: { check: HostKeyCheck; name: string; onAnswer: (trust: boolean) => void }) {
  if (check.kind === "unknown") {
    return (
      <div className="absolute inset-0 z-10 flex items-center justify-center p-6">
        <div role="dialog" aria-label={`First connection to ${name}`} className="glass w-[440px] max-w-full animate-pop-in rounded-2xl p-5">
          <div className="flex items-start gap-3">
            <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent">
              <ShieldQuestion size={16} />
            </span>
            <div className="min-w-0">
              <h3 className="text-[14px] font-semibold text-fg">First time connecting to {name}</h3>
              <p className="mt-1 text-[12.5px] leading-relaxed text-fg-2">
                Make sure this is really your server. Compare the fingerprint below with the one in your hosting provider's
                console. SSHeesh remembers it, and warns you if it ever changes.
              </p>
            </div>
          </div>
          <FingerprintList label="Server fingerprint" items={check.fingerprints} />
          <div className="mt-5 flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => onAnswer(false)}>
              Cancel
            </Button>
            <Button size="sm" variant="primary" autoFocus onClick={() => onAnswer(true)}>
              Trust and connect
            </Button>
          </div>
        </div>
      </div>
    );
  }
  if (check.kind !== "changed") return null;
  return (
    <div className="absolute inset-0 z-10 flex items-center justify-center bg-bad/[0.06] p-6">
      <div role="alertdialog" aria-label={`${name}'s identity changed`} className="glass w-[460px] max-w-full animate-pop-in rounded-2xl p-5 ring-1 ring-bad/40">
        <div className="flex items-start gap-3">
          <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-bad/15 text-bad">
            <ShieldAlert size={16} />
          </span>
          <div className="min-w-0">
            <h3 className="text-[14px] font-semibold text-fg">{name}'s identity changed</h3>
            <p className="mt-1 text-[12.5px] leading-relaxed text-fg-2">
              This happens when a server is reinstalled, or its address now points to another machine. It's also exactly what
              an attack looks like. If you didn't change anything on this server, don't connect.
            </p>
          </div>
        </div>
        <FingerprintList label="Before" items={check.old} tone="old" />
        <FingerprintList label="Now" items={check.new} tone="new" />
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <Button size="sm" variant="danger" onClick={() => onAnswer(true)}>
            I reinstalled it. Trust the new key
          </Button>
          <Button size="sm" variant="primary" autoFocus onClick={() => onAnswer(false)}>
            Don't connect
          </Button>
        </div>
      </div>
    </div>
  );
}

export function TerminalPane({ pane, focused }: { pane: PaneState; focused: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const pad = useSettings((s) => s.terminalPadding);
  const background = useSettings((s) => resolveScheme(s.terminalScheme, s.theme, s.accent).background);
  const state = useStore((s) => s.sessions[pane.id]);
  const closePane = useStore((s) => s.closePane);
  const countdown = useCountdown(state);
  const backOnline = useBackOnline(state);
  const recap = useStore((s) => s.recaps[pane.id]);
  const hostKey = useStore((s) => s.hostKeyPrompts[pane.id]);
  const names = useNames();
  const setRecap = useStore((s) => s.setRecap);
  const [recapOpen, setRecapOpen] = useState(false);
  useEffect(() => {
    if (!recap) setRecapOpen(false);
  }, [recap]);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const h = getTerminal(pane);
    h.mount(el);
    const ro = new ResizeObserver(() => h.scheduleFit());
    ro.observe(el);
    return () => {
      ro.disconnect();
      h.unmount();
    };
    // The handle is per pane; target/key never change for a terminal pane.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.id]);

  useEffect(() => {
    if (focused) getTerminal(pane).focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focused, pane.id]);

  const reconnectMode = useSettings((s) => s.reconnectMode);
  const updateSettings = useSettings((s) => s.update);
  const offline =
    state?.kind === "reconnecting" || state?.kind === "disconnected" || (state?.kind === "connecting" && state.attempt > 1);
  const stopped = state?.kind === "ended" || state?.kind === "failed";

  return (
    <div className="absolute inset-0" style={{ background }}>
      <div
        ref={box}
        style={{ padding: `${Math.round(pad * 0.66)}px ${Math.round(pad / 3)}px ${Math.round(pad * 0.66)}px ${pad}px` }}
        className={cn(
          "absolute inset-0 transition-[opacity,filter] duration-300",
          (offline || stopped) && "opacity-40 saturate-50",
        )}
      />

      {state?.kind === "disconnected" && reconnectMode === "ask" && (
        <div className="pointer-events-none absolute inset-x-0 top-3 flex justify-center px-3">
          <div className="glass pointer-events-auto flex max-w-full animate-pop-in items-center gap-3 rounded-[14px] py-2 pr-2 pl-3">
            <Face mood="lost" />
            <div className="min-w-0">
              <div className="truncate text-[12.5px] font-medium text-fg">{state.reason}</div>
              <button
                type="button"
                onClick={() => updateSettings({ reconnectMode: "auto" })}
                className="text-[11.5px] text-fg-3 underline-offset-2 hover:text-fg-2 hover:underline"
              >
                Always reconnect automatically
              </button>
            </div>
            <Button size="sm" variant="primary" autoFocus onClick={() => getTerminal(pane).reconnectNow()}>
              <RotateCw size={12.5} /> Reconnect
            </Button>
          </div>
        </div>
      )}

      {state?.kind === "reconnecting" && (
        <div className="pointer-events-none absolute inset-x-0 top-3 flex justify-center px-3">
          <div className="glass pointer-events-auto flex max-w-full animate-pop-in items-center gap-3 rounded-[14px] py-2 pl-3 pr-2">
            <span className="relative flex size-2 shrink-0">
              <span className="absolute inset-0 animate-ping rounded-full bg-warn opacity-60" />
              <span className="relative size-2 rounded-full bg-warn" />
            </span>
            <div className="min-w-0">
              <div className="truncate text-[12.5px] font-medium text-fg">{state.reason}</div>
              <div className="text-[11.5px] text-fg-3 tabular-nums">
                {countdown > 0 ? `Reconnecting in ${countdown}s` : "Reconnecting…"}
                {state.attempt > 1 && ` · attempt ${state.attempt}`}
              </div>
            </div>
            <Button size="sm" onClick={() => getTerminal(pane).reconnectNow()}>
              <RotateCw size={12.5} /> Now
            </Button>
          </div>
        </div>
      )}

      {recap && !recapOpen && !offline && (
        <RecapCard recap={recap} onShow={() => setRecapOpen(true)} onClose={() => setRecap(pane.id, null)} />
      )}
      {recap && recapOpen && (
        <RecapPanel
          recap={recap}
          onClose={() => {
            setRecapOpen(false);
            setRecap(pane.id, null);
          }}
        />
      )}

      {backOnline && !recap && (
        <div className="pointer-events-none absolute inset-x-0 top-3 flex justify-center px-3">
          <div className="glass flex animate-pop-in items-center gap-2.5 rounded-full py-1.5 pl-2.5 pr-3.5 text-[12px]">
            <Wifi size={13} className="text-ok" />
            <span className="font-medium text-fg">Back online</span>
            <span className="text-fg-3">
              {backOnline.kind === "connected" && backOnline.durable ? "everything's where you left it" : "started a new shell"}
            </span>
          </div>
        </div>
      )}

      {hostKey && <HostKeyCard check={hostKey} name={names.target(pane.target)} onAnswer={(trust) => void getTerminal(pane).answerIdentity(trust)} />}

      {stopped && (
        <div className="absolute inset-0 flex items-center justify-center p-6">
          <div className="glass w-[340px] max-w-full animate-pop-in rounded-2xl p-5">
            <div className="flex items-start gap-3">
              <span
                className={cn(
                  "mt-0.5 inline-flex size-7 shrink-0 items-center justify-center rounded-full",
                  state.kind === "failed" ? "bg-bad/15 text-bad" : "bg-ink/[0.07] text-fg-2",
                )}
              >
                <CircleAlert size={15} />
              </span>
              <div className="min-w-0">
                <h3 className="text-[14px] font-semibold text-fg">
                  {state.kind === "failed" ? "Couldn't connect" : "Session ended"}
                </h3>
                <p className="mt-1 text-[12.5px] leading-relaxed text-fg-2">
                  {state.kind === "failed" ? state.message : "The shell exited. Start a new one whenever you're ready."}
                </p>
              </div>
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={() => closePane(pane.id)}>
                <X size={13} /> Close
              </Button>
              {state.kind === "failed" && state.message.includes("identity changed") ? (
                <Button size="sm" variant="primary" onClick={() => void getTerminal(pane).checkIdentity(true)}>
                  <ShieldAlert size={12.5} /> Review the change
                </Button>
              ) : (
                <Button size="sm" variant="primary" onClick={() => getTerminal(pane).start()}>
                  <RotateCw size={12.5} /> {state.kind === "failed" ? "Try again" : "Reconnect"}
                </Button>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
