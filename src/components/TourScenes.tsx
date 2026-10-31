// The little pictures in the tour: each acts out a feature in SSHeesh's own
// colors, on a made-up server (demo-box), never on yours. One story runs
// through them: an agent fixing a bug. Each plays once, about six seconds,
// and stops on a frame that explains itself; hovering pauses it. Every
// element's own style is that last frame, so with reduced motion (the
// system's, or Settings) the scene simply shows it. Pure CSS.

import type { CSSProperties, ReactNode } from "react";
import { cn } from "../lib/format";
import { isMac, shortcutLabel } from "../lib/keys";

const CSS = `
.tour-scene * { animation-duration: 6s; animation-iteration-count: 1; animation-timing-function: cubic-bezier(.2,0,0,1); animation-fill-mode: both; }
.tour-scene:hover * { animation-play-state: paused; }
@keyframes tour-in-1 { 0%, 8% { opacity: 0; transform: translateX(-6px); } 14%, 100% { opacity: 1; transform: none; } }
@keyframes tour-in-2 { 0%, 18% { opacity: 0; transform: translateX(-6px); } 24%, 100% { opacity: 1; transform: none; } }
@keyframes tour-in-3 { 0%, 28% { opacity: 0; transform: translateX(-6px); } 34%, 100% { opacity: 1; transform: none; } }
@keyframes tour-in-late { 0%, 55% { opacity: 0; transform: scale(.7); } 62%, 100% { opacity: 1; transform: none; } }
@keyframes tour-working { 0%, 40% { opacity: 1; } 46%, 100% { opacity: 0; } }
@keyframes tour-bar { 0%, 6% { opacity: 0; transform: translate(-50%, -6px) scale(.98); } 12%, 100% { opacity: 1; transform: translate(-50%, 0); } }
@keyframes tour-type { 0%, 14% { width: 0; } 30%, 100% { width: 3ch; } }
@keyframes tour-results { 0%, 30% { opacity: 0; } 36%, 100% { opacity: 1; } }
@keyframes tour-pick { 0%, 52% { background: transparent; } 58%, 100% { background: rgb(var(--ui-ink) / .1); } }
@keyframes tour-lines { 0% { clip-path: inset(0 0 100% 0); } 22% { clip-path: inset(0 0 60% 0); } 62% { clip-path: inset(0 0 40% 0); } 80%, 100% { clip-path: inset(0 0 0 0); } }
@keyframes tour-blip { 0%, 22% { opacity: 0; } 26%, 58% { opacity: 1; } 63%, 100% { opacity: 0; } }
@keyframes tour-late { 0%, 72% { opacity: 0; transform: translateY(6px); } 78%, 100% { opacity: 1; transform: none; } }
@keyframes tour-cursor { 0%, 8% { transform: translate(230px, 170px); } 34% { transform: translate(106px, 36px); } 44% { transform: translate(106px, 36px) scale(.85); } 48%, 100% { transform: translate(106px, 36px); } }
@keyframes tour-keycap { 0%, 30% { opacity: 0; } 34%, 52% { opacity: 1; } 58%, 100% { opacity: 0; } }
@keyframes tour-underline { 0%, 32% { opacity: 0; } 36%, 100% { opacity: 1; } }
@keyframes tour-open { 0%, 46% { opacity: 0; transform: translateX(20px); } 56%, 100% { opacity: 1; transform: none; } }
@keyframes tour-ghost { 0%, 10% { opacity: 0; transform: translate(0, 0); } 14% { opacity: 1; transform: translate(0, 0); } 44% { opacity: 1; transform: translate(196px, 28px); } 48%, 100% { opacity: 0; transform: translate(196px, 28px); } }
@keyframes tour-zone { 0%, 28% { opacity: 0; } 34%, 44% { opacity: 1; } 50%, 100% { opacity: 0; } }
@keyframes tour-half { 0%, 46% { opacity: 0; flex-grow: 0.001; } 58%, 100% { opacity: 1; flex-grow: 1; } }
@keyframes tour-hide-real { 0%, 38% { opacity: 1; } 46%, 100% { opacity: 0; } }
@keyframes tour-show-masked { 0%, 38% { opacity: 0; } 46%, 100% { opacity: 1; } }
@keyframes tour-knob { 0%, 30% { transform: translateX(0); } 38%, 100% { transform: translateX(10px); } }
@keyframes tour-track { 0%, 30% { background: rgb(var(--ui-ink) / .18); } 38%, 100% { background: var(--ui-accent); } }
@keyframes tour-strip-out { 0%, 30% { opacity: 1; transform: none; } 50%, 100% { opacity: 0; transform: translate(-40px, 30px); } }
@keyframes tour-strip-in { 0%, 42% { opacity: 0; transform: translate(30px, -20px); } 60%, 100% { opacity: 1; transform: none; } }
@media (prefers-reduced-motion: reduce) { .tour-scene * { animation: none !important; } }
:root.reduce-motion .tour-scene * { animation: none !important; }
`;

const anim = (name: string): CSSProperties => ({ animationName: name });

function Frame({ children, label }: { children: ReactNode; label: string }) {
  return (
    <div role="img" aria-label={label} className="tour-scene relative h-[240px] w-full overflow-hidden rounded-[12px] bg-window ring-1 ring-ink/[0.1]">
      <style>{CSS}</style>
      {children}
    </div>
  );
}

const Line = ({ w, className }: { w: string; className?: string }) => <span className={cn("block h-[5px] shrink-0 rounded-full bg-ink/[0.14]", className)} style={{ width: w }} />;

function Side({ children }: { children: ReactNode }) {
  return <div className="absolute inset-y-0 left-0 flex w-[128px] flex-col gap-1 border-r border-line p-2 text-[10.5px]">{children}</div>;
}

function ServerRow({ name, color, active }: { name: ReactNode; color: string; active?: boolean }) {
  return (
    <div className={cn("flex items-center gap-1.5 px-1 py-[3px] text-fg", active && "font-medium")}>
      <span className="size-2.5 shrink-0 rounded-[3px]" style={{ background: color }} />
      {name}
    </div>
  );
}

function TabRow({ label, active, mark, agent, style }: { label: string; active?: boolean; mark?: ReactNode; agent?: boolean; style?: CSSProperties }) {
  return (
    <div className={cn("ml-3 flex items-center gap-1.5 rounded-[4px] px-1.5 py-[3px] text-fg-2", active && "bg-ink/[0.1] text-fg")} style={style}>
      <span className={cn("size-2 shrink-0 rounded-[2px]", agent ? "bg-[#d97757]" : "bg-ink/[0.3]")} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {mark}
    </div>
  );
}

function Page({ className, children }: { className?: string; children?: ReactNode }) {
  return (
    <div className={cn("absolute inset-y-2 right-2 left-[136px] flex flex-col gap-1.5 overflow-hidden rounded-[8px] bg-canvas p-2.5 font-mono text-[10.5px] text-fg-2 ring-1 ring-ink/[0.1]", className)}>
      {children}
    </div>
  );
}

const Prompt = () => <span className="text-ok">you@demo-box</span>;

function Cursor() {
  return (
    <svg viewBox="0 0 16 16" className="absolute top-0 left-0 size-4 drop-shadow" style={{ ...anim("tour-cursor"), transform: "translate(106px, 36px)" }} aria-hidden>
      <path d="M2 1.5 L2 13 L5.2 10.2 L7.4 14.6 L9.4 13.7 L7.2 9.4 L11.6 9.2 Z" fill="white" stroke="black" strokeWidth="1" strokeLinejoin="round" />
    </svg>
  );
}

/** T1: the command bar finds a server, a folder or a command. */
export function SceneCommandBar() {
  return (
    <Frame label="The command bar opens, “dem” is typed and demo-box is picked">
      <div className="absolute top-6 left-1/2 w-[80%] -translate-x-1/2 rounded-[10px] bg-raised p-1.5 text-[11px] shadow-xl ring-1 ring-ink/[0.12]" style={anim("tour-bar")}>
        <div className="flex items-center gap-1.5 border-b border-line px-1.5 pb-1.5 text-fg">
          <span className="text-fg-3">⌕</span>
          <span className="inline-block overflow-hidden whitespace-nowrap" style={{ ...anim("tour-type"), animationTimingFunction: "steps(3)", width: "3ch" }}>
            dem
          </span>
          <span className="h-3 w-px bg-fg-2" />
          <span className="ml-auto font-sans text-[10px] text-fg-3">{shortcutLabel("palette")}</span>
        </div>
        <div className="flex flex-col gap-0.5 pt-1" style={anim("tour-results")}>
          <span className="flex items-center gap-2 rounded-[5px] bg-ink/[0.1] px-1.5 py-1 text-fg" style={anim("tour-pick")}>
            <span className="size-2.5 rounded-[3px] bg-[#2389e2]" /> demo-box <span className="text-fg-3">· server</span>
          </span>
          <span className="px-1.5 py-1 text-fg">
            /srv/demo <span className="text-fg-3">· Files on demo-box</span>
          </span>
          <span className="px-1.5 py-1 text-fg">
            journalctl -f <span className="text-fg-3">· runs on demo-box</span>
          </span>
        </div>
      </div>
    </Frame>
  );
}

/** T2: each server keeps its tabs, and agents show up on every server. */
export function SceneTabs() {
  return (
    <Frame label="Tabs appear under demo-box; Codex works on homelab while Claude Code on demo-box needs you">
      <Side>
        <ServerRow name="demo-box" color="#2389e2" active />
        <TabRow label="Terminal" active style={anim("tour-in-1")} />
        <TabRow
          label="Claude Code"
          agent
          style={anim("tour-in-2")}
          mark={
            <span className="text-[9.5px] font-medium text-warn" style={anim("tour-in-late")}>
              Needs you
            </span>
          }
        />
        <TabRow label="Files" style={anim("tour-in-3")} />
        <ServerRow name="homelab" color="#9d69d2" />
        <TabRow
          label="Codex"
          agent
          mark={
            <span className="relative size-2.5">
              <span className="absolute inset-0 rounded-full border-[1.5px] border-accent border-t-transparent opacity-0" style={anim("tour-working")} />
            </span>
          }
        />
        <ServerRow name="db-main" color="#0997b4" />
      </Side>
      <Page>
        <span className="text-[#d97757]">✻ Claude Code</span>
        <span>● Found it: the rate limit skips checkout.</span>
        <span>  ⎿ Updated src/routes/checkout.ts (+3 −1)</span>
        <span style={anim("tour-in-late")}>
          ● Run the tests now? <span className="text-fg-3">(y/n)</span>
        </span>
      </Page>
    </Frame>
  );
}

/** T3: the connection drops, the session keeps running, and you see what you missed. */
export function SceneDurable() {
  return (
    <Frame label="The connection drops, the tests keep running on the server, and a note sums up what you missed">
      <Page className="left-2">
        <div className="flex flex-col gap-1.5" style={anim("tour-lines")}>
          <span>
            <Prompt />
            :~$ npm test
          </span>
          <span>PASS src/routes/orders.test.ts</span>
          <span>PASS src/routes/checkout.test.ts</span>
          <span>PASS src/middleware/rateLimit.test.ts</span>
          <span className="text-ok">Tests: 48 passed</span>
        </div>
        <div className="absolute inset-x-2 top-2 rounded-[6px] bg-warn/15 px-2 py-1 font-sans text-[10.5px] text-warn opacity-0" style={anim("tour-blip")}>
          Reconnecting… your session keeps running on the server
        </div>
        <div className="absolute inset-x-2 bottom-2 rounded-[6px] bg-raised px-2 py-1.5 font-sans text-[10.5px] text-fg-2 ring-1 ring-ink/[0.12]" style={anim("tour-late")}>
          While you were away: <span className="text-fg">36 lines</span>, no errors
        </div>
      </Page>
    </Frame>
  );
}

/** T4: Ctrl+click a path in the output to peek at the file beside the terminal. */
export function ScenePeek() {
  return (
    <Frame label="A path in the output is Ctrl+clicked and the file opens beside the terminal">
      <div className="absolute inset-2 flex gap-2">
        <div className="relative flex min-w-0 flex-1 flex-col gap-1.5 overflow-hidden rounded-[8px] bg-canvas p-2.5 font-mono text-[10.5px] text-fg-2 ring-1 ring-ink/[0.1]">
          <span className="text-[#d97757]">● Fixed it in</span>
          <span>
            <span className="relative text-fg">
              src/routes/checkout.ts:42
              <span className="absolute inset-x-0 -bottom-[1px] h-px bg-accent" style={anim("tour-underline")} />
            </span>
          </span>
          <span className="text-fg-3">  the limit now applies before payment</span>
          <span className="absolute top-[54px] left-[118px] rounded-[4px] bg-raised px-1.5 py-0.5 font-sans text-[10px] text-fg opacity-0 ring-1 ring-ink/[0.15]" style={anim("tour-keycap")}>
            {isMac ? "⌘" : "Ctrl"} + click
          </span>
        </div>
        <div className="flex w-[46%] shrink-0 flex-col gap-1.5 overflow-hidden rounded-[8px] bg-ink/[0.03] p-2.5 ring-1 ring-ink/[0.1]" style={anim("tour-open")}>
          <span className="truncate text-[10.5px] font-medium text-fg">checkout.ts</span>
          <Line w="70%" />
          <Line w="58%" />
          <span className="-mx-1 rounded-[3px] bg-accent/15 px-1 py-[3px]">
            <Line w="85%" className="bg-accent/60" />
          </span>
          <Line w="62%" />
          <Line w="48%" />
        </div>
      </div>
      <Cursor />
    </Frame>
  );
}

/** T5: drag a tab onto the page to see two at once. */
export function SceneDragSplit() {
  return (
    <Frame label="The Files tab is dragged onto the page and the page splits in two">
      <Side>
        <ServerRow name="demo-box" color="#2389e2" active />
        <TabRow label="Terminal" active />
        <TabRow label="Files" active />
        <TabRow label="Claude Code" agent />
      </Side>
      <div className="absolute inset-y-2 right-2 left-[136px] flex gap-1.5">
        <div className="relative flex min-w-0 flex-[1.1] flex-col gap-1.5 rounded-[8px] bg-canvas p-2.5 font-mono text-[10.5px] text-fg-2 ring-1 ring-ink/[0.1]">
          <span>
            <Prompt />
            :~$ ls
          </span>
          <span>src tests package.json</span>
          <div className="absolute inset-y-1 right-1 w-[46%] rounded-[6px] bg-accent/15 opacity-0 ring-2 ring-accent/60" style={anim("tour-zone")} />
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1.5 overflow-hidden rounded-[8px] bg-ink/[0.03] p-2 ring-1 ring-ink/[0.1]" style={anim("tour-half")}>
          <span className="text-[10.5px] font-medium text-fg">Files</span>
          <Line w="70%" />
          <Line w="55%" />
          <Line w="80%" />
        </div>
      </div>
      <div className="absolute top-[47px] left-[24px] rounded-[4px] bg-raised px-2 py-[3px] text-[10.5px] text-fg opacity-0 shadow-lg ring-1 ring-ink/[0.15]" style={anim("tour-ghost")}>
        Files
      </div>
    </Frame>
  );
}

/** T6: streamer mode hides names and addresses before anyone sees them. */
export function SceneStreamer() {
  // Both in one grid cell, so the line keeps its place as one fades into the other.
  const swap = (real: string, hidden: string) => (
    <span className="inline-grid">
      <span className="[grid-area:1/1] opacity-0" style={anim("tour-hide-real")}>
        {real}
      </span>
      <span className="[grid-area:1/1] whitespace-nowrap text-fg-3" style={anim("tour-show-masked")}>
        {hidden}
      </span>
    </span>
  );
  return (
    <Frame label="Streamer mode is switched on and server names and addresses are replaced">
      <Side>
        <ServerRow name={swap("demo-box", "server 1")} color="#2389e2" />
        <ServerRow name={swap("homelab", "server 2")} color="#9d69d2" />
        <ServerRow name={swap("db-main", "server 3")} color="#0997b4" />
        <div className="mt-auto flex items-center gap-1.5 px-1 text-fg-2">
          <span className="relative h-2.5 w-5 rounded-full bg-accent" style={anim("tour-track")}>
            <span className="absolute top-[1px] left-[1px] size-2 rounded-full bg-white" style={{ ...anim("tour-knob"), transform: "translateX(10px)" }} />
          </span>
          Streamer mode
        </div>
      </Side>
      <Page>
        <span>
          <span className="text-ok">{swap("you@demo-box", "you@server-1")}</span>:~$ ssh-add -l
        </span>
        <span>{swap("Last login from 198.51.100.3", "Last login from address hidden")}</span>
      </Page>
    </Frame>
  );
}

/** For people who knew the old layout: the tabs along the top move under their server. */
export function SceneNewLayout() {
  return (
    <Frame label="The tabs along the top move into the sidebar, under their server">
      <div className="absolute top-2 right-2 left-[136px] flex gap-1 opacity-0" style={anim("tour-strip-out")}>
        {["Terminal", "Claude Code", "Files"].map((t, i) => (
          <span key={t} className={cn("rounded-[5px] px-2 py-[3px] text-[10.5px]", i === 0 ? "bg-ink/[0.1] text-fg" : "text-fg-3")}>
            {t}
          </span>
        ))}
      </div>
      <Side>
        <ServerRow name="demo-box" color="#2389e2" active />
        <div className="flex flex-col gap-1" style={anim("tour-strip-in")}>
          <TabRow label="Terminal" active />
          <TabRow label="Claude Code" agent />
          <TabRow label="Files" />
        </div>
        <ServerRow name="homelab" color="#9d69d2" />
        <ServerRow name="db-main" color="#0997b4" />
      </Side>
      <Page className="top-10">
        <span>
          <Prompt />
          :~$ <span className="inline-block h-2.5 w-1 bg-fg-2 align-middle" />
        </span>
      </Page>
    </Frame>
  );
}
