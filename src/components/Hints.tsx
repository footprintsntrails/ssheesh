// First-use hints: one small note at a time, next to the thing it's about,
// the first time you meet a feature. Each shows once, and never if you've
// already used the feature; "No more tips" stops them all. They never take
// the keyboard: Esc and Ctrl+C stay with whatever runs in the terminal.
// None in the first minute, a pause between them, a few per run at most.
// Anything marked data-hint="…" can be a hint's anchor.

import { X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { agentRunning } from "../lib/agents";
import { isMac, shortcutLabel } from "../lib/keys";
import { markHintSeen, ONBOARDING_VERSION, useSettings } from "../lib/settings";
import { currentScreen, currentTarget, isSplit, machineSummary, useStore } from "../lib/store";
import { toggleSplit } from "../lib/tabs";
import { Button } from "./ui";

type Snapshot = ReturnType<typeof useStore.getState>;

interface Hint {
  id: string;
  /** The data-hint value of what it points at. */
  anchor: string;
  title: string;
  body: () => string;
  /** When it's worth saying. */
  when: (s: Snapshot) => boolean;
  /** You've found it yourself: it never needs saying. */
  used?: (s: Snapshot) => boolean;
  /** One thing you can do about it right there. */
  action?: { label: string; run: () => void };
}

const tabsOnScreenServer = (s: Snapshot) => machineSummary(s, currentTarget(s)).open;
/** Some machine is side by side, by your own choice or from Settings. */
const anySplit = (s: Snapshot) => Object.values(s.splitOn).includes(true) || isSplit(s, null);

/** In the order they're worth knowing. */
export const HINTS: Hint[] = [
  {
    id: "tabs",
    anchor: "tabs",
    title: "Each server has its own tabs",
    body: () => "Drag one onto the screen to see two side by side, and back onto this list to make it a tab again. Right-click a tab for more.",
    when: (s) => tabsOnScreenServer(s) >= 2,
    used: anySplit,
  },
  {
    id: "peek",
    anchor: "terminal",
    title: "Peek at a file",
    body: () => `${isMac ? "⌘-click" : "Ctrl+click"} a path in the output to see the file beside the terminal.`,
    when: (s) => s.sessions[s.focused]?.kind === "connected" && s.panes[s.focused]?.kind === "terminal",
  },
  {
    id: "agents",
    anchor: "agent-tab",
    title: "Agents show up by name",
    body: () => "On every server, so you see them working while you're elsewhere. “Needs you” means one is waiting for an answer.",
    when: (s) => Object.entries(s.foreground).some(([, c]) => !!agentRunning(c)),
  },
  {
    id: "split",
    anchor: "split",
    title: "Two tabs at once",
    body: () => `${shortcutLabel("toggleSplit")} shows this server's tabs side by side; again goes back to one at a time.`,
    when: (s) => tabsOnScreenServer(s) >= 2 && !isSplit(s, currentScreen(s)),
    used: anySplit,
    action: { label: "Try it", run: toggleSplit },
  },
  {
    id: "bookmarks",
    anchor: "bookmarks",
    title: "Bookmarks and recent folders",
    body: () => "They're behind this button, which also bookmarks the folder you're in.",
    when: (s) => s.panes[s.focused]?.kind === "files",
  },
];

const SHOW_AFTER_MS = 1500;
const WIDTH = 280;
/** None this soon after launch, this long apart, and this many per run at most. */
const QUIET_START_MS = 60_000;
const APART_MS = 90_000;
const PER_RUN = 3;
const launched = Date.now();
let lastShown = 0;
let shownThisRun = 0;

/** The hint to show now, if any: the first unseen one whose moment has come and whose anchor is on screen. */
function pick(s: Snapshot): { hint: Hint; el: HTMLElement } | null {
  const st = useSettings.getState();
  const now = Date.now();
  if (now - launched < QUIET_START_MS || now - lastShown < APART_MS || shownThisRun >= PER_RUN) return null;
  if (st.onboardingVersion < ONBOARDING_VERSION || s.paletteOpen || s.settingsOpen || s.addHostOpen || s.confirmRequest) return null;
  if (st.hintsSeen.includes("*")) return null;
  for (const hint of HINTS) {
    if (st.hintsSeen.includes(hint.id)) continue;
    if (hint.used?.(s)) {
      markHintSeen(hint.id);
      continue;
    }
    if (!hint.when(s)) continue;
    const el = [...document.querySelectorAll<HTMLElement>(`[data-hint="${hint.anchor}"]`)].find((e) => e.offsetParent !== null);
    if (el) return { hint, el };
  }
  return null;
}

export function Hints() {
  const [shown, setShown] = useState<{ hint: Hint; el: HTMLElement } | null>(null);
  const [at, setAt] = useState<{ x: number; y: number; side: "right" | "below" } | null>(null);
  const timer = useRef(0);
  const card = useRef<HTMLDivElement>(null);

  // Nothing left to say this run (all seen, tips off, or enough for today): no listening at all.
  const done = useSettings((st) => st.hintsSeen.includes("*") || HINTS.every((h) => st.hintsSeen.includes(h.id)));

  // Looks again whenever the app changes, and settles on a hint after a moment.
  useEffect(() => {
    if (done || shown) return;
    let off: (() => void) | undefined;
    const look = () => {
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        const next = pick(useStore.getState());
        if (!next) return;
        lastShown = Date.now();
        shownThisRun++;
        setShown(next);
      }, SHOW_AFTER_MS);
    };
    // Quiet for the first minute: start listening only then.
    const start = window.setTimeout(() => {
      if (shownThisRun >= PER_RUN) return;
      look();
      const offStore = useStore.subscribe(look);
      const every = window.setInterval(look, 15_000);
      off = () => {
        offStore();
        window.clearInterval(every);
      };
    }, Math.max(0, QUIET_START_MS - (Date.now() - launched)));
    return () => {
      window.clearTimeout(start);
      window.clearTimeout(timer.current);
      off?.();
    };
  }, [shown, done]);

  // Beside its anchor (sidebar items), or under it; inside the window either way.
  useLayoutEffect(() => {
    if (!shown) return setAt(null);
    const place = () => {
      if (!shown.el.isConnected) return setShown(null);
      const r = shown.el.getBoundingClientRect();
      const h = card.current?.offsetHeight ?? 120;
      const next =
        r.right + 12 + WIDTH < window.innerWidth && r.width < 320
          ? { x: r.right + 10, y: Math.min(Math.max(8, r.top + r.height / 2 - 24), window.innerHeight - h - 8), side: "right" as const }
          : { x: Math.min(Math.max(8, r.left), window.innerWidth - WIDTH - 8), y: Math.min(r.bottom + 10, window.innerHeight - h - 8), side: "below" as const };
      setAt((a) => (a && a.x === next.x && a.y === next.y && a.side === next.side ? a : next));
    };
    place();
    window.addEventListener("resize", place);
    const t = window.setInterval(place, 500);
    return () => {
      window.removeEventListener("resize", place);
      window.clearInterval(t);
    };
  }, [shown]);

  if (!shown) return null;
  const gotIt = () => {
    markHintSeen(shown.hint.id);
    setShown(null);
  };
  const noMore = () => {
    markHintSeen("*");
    setShown(null);
  };
  return (
    <div
      ref={card}
      role="dialog"
      aria-label={shown.hint.title}
      className="animate-fade-in fixed z-[60] rounded-[12px] bg-raised p-3.5 shadow-[0_12px_32px_rgb(0_0_0/0.35)] ring-1 ring-ink/[0.12]"
      style={{ left: at?.x ?? -9999, top: at?.y ?? -9999, width: WIDTH }}
    >
      {at && (
        <span
          aria-hidden
          className="absolute size-2.5 rotate-45 bg-raised ring-1 ring-ink/[0.12] [clip-path:polygon(0_0,0_100%,100%_100%)]"
          style={at.side === "right" ? { left: -5, top: 20 } : { left: 18, top: -5, transform: "rotate(135deg)" }}
        />
      )}
      <div className="flex items-start gap-2">
        <p className="flex-1 text-[13px] font-semibold text-fg">{shown.hint.title}</p>
        <button type="button" aria-label="Close tip" tabIndex={-1} onMouseDown={(e) => e.preventDefault()} onClick={gotIt} className="-mt-0.5 -mr-1 flex size-5 items-center justify-center rounded-[5px] text-fg-3 hover:bg-ink/[0.08] hover:text-fg">
          <X size={12} />
        </button>
      </div>
      <p className="mt-1 text-[12.5px] leading-relaxed text-fg-2">{shown.hint.body()}</p>
      <div className="mt-3 flex items-center gap-2">
        <button type="button" onClick={noMore} className="mr-auto text-[11.5px] text-fg-3 hover:text-fg-2">
          No more tips
        </button>
        {shown.hint.action && (
          <Button
            size="sm"
            variant="ghost"
            tabIndex={-1}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              shown.hint.action?.run();
              gotIt();
            }}
          >
            {shown.hint.action.label}
          </Button>
        )}
        <Button size="sm" variant="primary" tabIndex={-1} onMouseDown={(e) => e.preventDefault()} onClick={gotIt}>
          Got it
        </Button>
      </div>
    </div>
  );
}
