import { CircleAlert, RotateCw, Wifi, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../lib/format";
import { useStore, type PaneState } from "../lib/store";
import { useSettings } from "../lib/settings";
import { getTerminal } from "../lib/terminals";
import { resolveScheme } from "../lib/themes";
import type { SessionState } from "../lib/types";
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

export function TerminalPane({ pane, focused }: { pane: PaneState; focused: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const pad = useSettings((s) => s.terminalPadding);
  const background = useSettings((s) => resolveScheme(s.terminalScheme, s.theme, s.accent).background);
  const state = useStore((s) => s.sessions[pane.id]);
  const closePane = useStore((s) => s.closePane);
  const countdown = useCountdown(state);
  const backOnline = useBackOnline(state);

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

      {backOnline && (
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
              <Button size="sm" variant="primary" onClick={() => getTerminal(pane).start()}>
                <RotateCw size={12.5} /> {state.kind === "failed" ? "Try again" : "Reconnect"}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
