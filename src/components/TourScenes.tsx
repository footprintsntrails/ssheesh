// The little pictures in the tour: each acts out a feature in SSHeesh's own
// look, a little smaller, on a made-up server (demo-box) with made-up files,
// never on yours. One story runs through them: an agent fixing a bug. Each
// plays once, about six seconds, and stops on a frame that explains itself;
// hovering pauses it. Every element's own style is that last frame, so with
// reduced motion (the system's, or Settings) the scene simply shows it.
// Pure CSS. Animated elements never use Tailwind's translate classes: those
// add to the keyframes' transform instead of being replaced by it.

import { ArrowUp, Bookmark, CornerDownLeft, Database, FileCode, FileText, Folder, FolderOpen, Globe, Laptop, Play, Search, Server, SquareTerminal, X, type LucideIcon } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { cn } from "../lib/format";
import { isMac, shortcutLabel } from "../lib/keys";
import { useSettings } from "../lib/settings";
import { AgentIcon } from "./AgentIcons";
import { Logo } from "./ui";

const CSS = `
.tour-scene * { animation-duration: 6s; animation-iteration-count: 1; animation-timing-function: cubic-bezier(.2,0,0,1); animation-fill-mode: both; }
.tour-scene:hover * { animation-play-state: paused; }
@keyframes tour-in-1 { 0%, 8% { opacity: 0; transform: translateX(-6px); } 14%, 100% { opacity: 1; transform: none; } }
@keyframes tour-in-2 { 0%, 18% { opacity: 0; transform: translateX(-6px); } 24%, 100% { opacity: 1; transform: none; } }
@keyframes tour-in-3 { 0%, 28% { opacity: 0; transform: translateX(-6px); } 34%, 100% { opacity: 1; transform: none; } }
@keyframes tour-in-late { 0%, 55% { opacity: 0; } 62%, 100% { opacity: 1; } }
@keyframes tour-working { 0%, 40% { opacity: 1; } 46%, 100% { opacity: 0; } }
@keyframes tour-scrim { 0%, 4% { opacity: 0; } 10%, 100% { opacity: 1; } }
@keyframes tour-bar { 0%, 6% { opacity: 0; transform: translateY(-6px) scale(.98); } 12%, 100% { opacity: 1; transform: none; } }
@keyframes tour-k1 { 0%, 17% { opacity: 0; } 18%, 100% { opacity: 1; } }
@keyframes tour-k2 { 0%, 22% { opacity: 0; } 23%, 100% { opacity: 1; } }
@keyframes tour-k3 { 0%, 27% { opacity: 0; } 28%, 100% { opacity: 1; } }
@keyframes tour-results { 0%, 30% { opacity: 0; } 36%, 100% { opacity: 1; } }
@keyframes tour-pick-off { 0%, 52% { background: rgb(var(--ui-ink) / .09); } 58%, 100% { background: transparent; } }
@keyframes tour-pick { 0%, 52% { background: transparent; } 58%, 100% { background: rgb(var(--ui-ink) / .09); } }
@keyframes tour-lines { 0% { clip-path: inset(0 0 100% 0); } 22% { clip-path: inset(0 0 60% 0); } 62% { clip-path: inset(0 0 40% 0); } 80%, 100% { clip-path: inset(0 0 0 0); } }
@keyframes tour-blip { 0%, 22% { opacity: 0; } 26%, 58% { opacity: 1; } 63%, 100% { opacity: 0; } }
@keyframes tour-late { 0%, 72% { opacity: 0; transform: translateY(6px); } 78%, 100% { opacity: 1; transform: none; } }
@keyframes tour-cursor { 0%, 8% { transform: translate(250px, 190px); } 34% { transform: var(--at); } 44% { transform: var(--at) scale(.85); } 48%, 100% { transform: var(--at); } }
@keyframes tour-keycap { 0%, 30% { opacity: 0; } 34%, 52% { opacity: 1; } 58%, 100% { opacity: 0; } }
@keyframes tour-underline { 0%, 32% { opacity: 0; } 36%, 100% { opacity: 1; } }
@keyframes tour-open { 0%, 46% { opacity: 0; transform: translateX(20px); } 56%, 100% { opacity: 1; transform: none; } }
@keyframes tour-ghost { 0%, 10% { opacity: 0; transform: translate(0, 0); } 14% { opacity: 1; transform: translate(0, 0); } 44% { opacity: 1; transform: var(--to); } 48%, 100% { opacity: 0; transform: var(--to); } }
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

// ------------------------------------------------------------ the made-up world

const DEMO = { name: "demo-box", icon: Globe, color: "#2389e2" };
const LAB = { name: "homelab", icon: Server, color: "#9d69d2" };
const DB = { name: "db-main", icon: Database, color: "#0997b4" };
type Machine = typeof DEMO;

const icon = (Icon: LucideIcon, className = "text-fg-3") => <Icon size={10.5} strokeWidth={1.9} className={cn("shrink-0", className)} />;
const TERMINAL = icon(SquareTerminal);
const FILES = icon(FolderOpen);
const CLAUDE = <AgentIcon id="claude" size={10.5} />;
const CODEX = <AgentIcon id="codex" size={10.5} />;

/** The sidebar, as SSHeesh draws it; `menu` shows the main menu as the page you're on. */
function Side({ children, menu }: { children: ReactNode; menu?: boolean }) {
  return (
    <div className="absolute inset-y-0 left-0 flex w-[186px] flex-col border-r border-line px-1.5 pt-2">
      {menu && (
        <span className="mb-1 flex h-6 shrink-0 items-center gap-1.5 rounded-[6px] bg-ink/[0.1] px-1.5 text-[10.5px] font-semibold text-fg">
          <Logo size={14} /> Main menu
        </span>
      )}
      <div className="flex h-5 shrink-0 items-center px-1.5 text-[9.5px] font-semibold text-fg-2">Servers</div>
      {children}
    </div>
  );
}

function ServerRow({ machine, name, mark }: { machine: Machine; name?: ReactNode; mark?: ReactNode }) {
  const Icon = machine.icon;
  return (
    <div className="flex h-[22px] shrink-0 items-center gap-1.5 px-1.5 text-[11px] font-medium text-fg">
      <Icon size={12} strokeWidth={1.9} className="shrink-0" style={{ color: machine.color }} />
      <span className="min-w-0 flex-1 truncate">{name ?? machine.name}</span>
      {mark}
    </div>
  );
}

/** The scenes draw tabs where yours are: along the top, or under each server. */
const useTopTabs = () => useSettings((s) => s.tabsPlacement === "top");

interface StripTabProps {
  label: string;
  icon: ReactNode;
  active?: boolean;
  mark?: ReactNode;
  style?: CSSProperties;
}

/** Tabs along the top of the page. */
function Strip({ tabs }: { tabs: StripTabProps[] }) {
  return (
    <div className="absolute top-2 right-2 left-[194px] flex h-[22px] items-center gap-0.5">
      {tabs.map((t) => (
        <span
          key={t.label}
          className={cn("flex h-5 min-w-0 shrink-0 items-center gap-1 rounded-[5px] px-1.5 text-[10px] font-medium", t.active ? "bg-ink/[0.1] text-fg" : "text-fg-3")}
          style={t.style}
        >
          {t.icon}
          <span className="truncate">{t.label}</span>
          {t.mark}
        </span>
      ))}
    </div>
  );
}

const spinner = (style?: CSSProperties) => (
  <span className="size-2 shrink-0 rounded-full border-[1.5px] border-accent border-t-transparent opacity-0" style={style} />
);

/** A server's tabs, hanging from a line under its icon. */
function TabGroup({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return (
    <div className="mb-1 ml-[11.5px] flex shrink-0 flex-col gap-px border-l border-ink/[0.12] pl-[5px]" style={style}>
      <span className="pl-1.5 text-[8.5px] leading-[14px] font-medium text-fg-3">Tabs</span>
      {children}
    </div>
  );
}

function TabRow({ label, icon, active, below, mark, style }: { label: string; icon: ReactNode; active?: boolean; below?: ReactNode; mark?: ReactNode; style?: CSSProperties }) {
  return (
    <div className={cn("relative flex min-h-[20px] items-center gap-1.5 rounded-[4px] py-[2px] pr-1 pl-1.5 text-[10.5px] text-fg-2", active && "bg-ink/[0.1] text-fg")} style={style}>
      {active && <span className="absolute inset-y-[4px] -left-[6.5px] w-[2px] rounded-full bg-accent" />}
      {icon}
      <span className="min-w-0 flex-1">
        <span className="block truncate leading-[14px]">{label}</span>
        {below}
      </span>
      {mark}
    </div>
  );
}

const needsYou = (style?: CSSProperties) => (
  <span className="block text-[9.5px] leading-[12px] font-medium text-warn" style={style}>
    Needs you
  </span>
);

/** A pane with its title bar, like the ones on the page. */
function Pane({ title, icon, head, className, style, children }: { title: ReactNode; icon: ReactNode; head?: ReactNode; className?: string; style?: CSSProperties; children?: ReactNode }) {
  return (
    <div className={cn("relative flex min-w-0 flex-col overflow-hidden rounded-[8px] bg-canvas ring-1 ring-ink/[0.1]", className)} style={style}>
      <div className="flex h-[22px] shrink-0 items-center gap-1.5 border-b border-line px-2 text-[10px] font-medium text-fg">
        {icon}
        <span className="min-w-0 truncate">{title}</span>
        {head}
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-[3px] p-2 font-mono text-[10px] leading-[14px] whitespace-pre text-fg-2">{children}</div>
    </div>
  );
}

/** Where panes go: beside the sidebar, or the whole frame without one. */
function Main({ full, className, children }: { full?: boolean; className?: string; children: ReactNode }) {
  return <div className={cn("absolute inset-y-2 right-2 flex gap-1.5", full ? "left-2" : "left-[194px]", className)}>{children}</div>;
}

const Prompt = ({ host = "demo-box" }: { host?: ReactNode }) => (
  <span>
    <span className="text-ok">you@{host}</span>:~/app$
  </span>
);

function Cursor({ at }: { at: string }) {
  return (
    <svg viewBox="0 0 16 16" className="absolute top-0 left-0 size-4 drop-shadow" style={{ ...anim("tour-cursor"), transform: at, ["--at" as string]: at }} aria-hidden>
      <path d="M2 1.5 L2 13 L5.2 10.2 L7.4 14.6 L9.4 13.7 L7.2 9.4 L11.6 9.2 Z" fill="white" stroke="black" strokeWidth="1" strokeLinejoin="round" />
    </svg>
  );
}

/** A line of TypeScript, colored the way the preview colors it. */
const KEYWORDS = new Set(["const", "await", "async", "return", "if", "export", "import", "from"]);
function Code({ text }: { text: string }) {
  return (
    <>
      {text.split(/("[^"]*"|\b\d+\b|\w+)/).map((t, i) =>
        t.startsWith('"') ? (
          <span key={i} className="text-ok">
            {t}
          </span>
        ) : /^\d+$/.test(t) ? (
          <span key={i} className="text-warn">
            {t}
          </span>
        ) : KEYWORDS.has(t) ? (
          <span key={i} className="text-accent">
            {t}
          </span>
        ) : (
          t
        ),
      )}
    </>
  );
}

// ------------------------------------------------------------------- the scenes

/** T1: the command bar finds a folder, a bookmark or a command, and says where it runs. */
export function SceneCommandBar() {
  const Row = ({ icon: Icon, label, hint, style }: { icon: LucideIcon; label: ReactNode; hint: string; style?: CSSProperties }) => (
    <div className="flex h-[22px] items-center gap-2 rounded-[6px] px-2 text-fg" style={style}>
      <Icon size={11} strokeWidth={1.9} className="shrink-0 text-fg-3" />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span className="shrink-0 font-mono text-[9.5px] text-fg-3">{hint}</span>
    </div>
  );
  const Heading = ({ children }: { children: ReactNode }) => <div className="px-2 pt-1.5 pb-0.5 text-[9.5px] font-medium text-fg-3">{children}</div>;
  const top = useTopTabs();
  return (
    <Frame label="The command bar opens, “ngi” is typed, and the Reload nginx command is picked; it says it runs on demo-box">
      <Side>
        <ServerRow machine={DEMO} />
        {!top && (
          <TabGroup>
            <TabRow label="Terminal" icon={TERMINAL} active />
          </TabGroup>
        )}
        <ServerRow machine={LAB} />
        <ServerRow machine={DB} />
      </Side>
      {top && <Strip tabs={[{ label: "Terminal", icon: TERMINAL, active: true }]} />}
      <Main className={top ? "top-9" : undefined}>
        <Pane title="Terminal" icon={TERMINAL} className="flex-1">
          <Prompt />
        </Pane>
      </Main>
      <div className="absolute inset-0 bg-black/35" style={anim("tour-scrim")} />
      <div className="absolute top-5 right-[14%] left-[14%] overflow-hidden rounded-[10px] bg-raised text-[11px] shadow-2xl ring-1 ring-ink/[0.12]" style={anim("tour-bar")}>
        <div className="flex h-8 items-center gap-2 border-b border-line px-2.5 text-[12px] text-fg">
          <Search size={12} className="shrink-0 text-fg-3" />
          <span>
            <span style={anim("tour-k1")}>n</span>
            <span style={anim("tour-k2")}>g</span>
            <span style={anim("tour-k3")}>i</span>
          </span>
          <span className="-ml-1.5 h-3.5 w-px bg-accent" />
          <span className="ml-auto rounded-[4px] px-1 text-[9.5px] text-fg-3 ring-1 ring-ink/[0.1]">{shortcutLabel("palette")}</span>
        </div>
        <div className="p-1" style={anim("tour-results")}>
          <Heading>Bookmarks</Heading>
          <Row icon={Bookmark} label="nginx on demo-box" hint="/etc/nginx" style={{ ...anim("tour-pick-off"), background: "transparent" }} />
          <Row icon={Folder} label="nginx logs on demo-box" hint="/var/log/nginx" />
          <Heading>Snippets · run on demo-box</Heading>
          <Row
            icon={Play}
            label={
              <>
                Reload nginx <span className="font-mono text-[9.5px] text-fg-3">sudo systemctl reload nginx</span>
              </>
            }
            hint=""
            style={{ ...anim("tour-pick"), background: "rgb(var(--ui-ink) / .09)" }}
          />
        </div>
        <div className="flex items-center gap-1 border-t border-line px-2.5 py-1 text-[9.5px] text-fg-3" style={anim("tour-results")}>
          <CornerDownLeft size={9.5} /> runs on <span className="font-medium text-fg-2">demo-box</span>
        </div>
      </div>
    </Frame>
  );
}

/** T2: each server keeps its tabs, and agents show up on every server. */
export function SceneTabs() {
  const top = useTopTabs();
  return (
    <Frame label="demo-box gets its tabs; Codex works on homelab while Claude Code on demo-box needs you">
      <Side>
        <ServerRow
          machine={DEMO}
          mark={
            top && (
              <span className="shrink-0 text-[9.5px] font-medium text-warn" style={anim("tour-in-late")}>
                Needs you
              </span>
            )
          }
        />
        {!top && (
          <TabGroup>
            <TabRow label="Terminal" icon={TERMINAL} style={anim("tour-in-1")} />
            <TabRow label="Terminal 2 (Claude Code)" icon={CLAUDE} active style={anim("tour-in-2")} below={needsYou(anim("tour-in-late"))} />
            <TabRow label="Files" icon={FILES} style={anim("tour-in-3")} />
          </TabGroup>
        )}
        <ServerRow machine={LAB} mark={top && spinner(anim("tour-working"))} />
        {!top && (
          <TabGroup>
            <TabRow label="Terminal (Codex)" icon={CODEX} mark={spinner(anim("tour-working"))} />
          </TabGroup>
        )}
        <ServerRow machine={DB} />
      </Side>
      {top && (
        <Strip
          tabs={[
            { label: "Terminal", icon: TERMINAL, style: anim("tour-in-1") },
            { label: "Terminal 2 (Claude Code)", icon: CLAUDE, active: true, style: anim("tour-in-2"), mark: <span className="size-1.5 shrink-0 rounded-full bg-warn" style={anim("tour-in-late")} /> },
            { label: "Files", icon: FILES, style: anim("tour-in-3") },
          ]}
        />
      )}
      <Main className={top ? "top-9" : undefined}>
        <Pane title="Terminal 2 (Claude Code)" icon={CLAUDE} className="flex-1">
          <span className="text-[#d97757]">✻ Claude Code</span>
          <span className="text-fg">&gt; checkout skips the rate limit, fix it</span>
          <span>● Found it: the limit runs after the payment.</span>
          <span>● Update(src/routes/checkout.ts)</span>
          <span className="text-fg-3">  ⎿ 3 lines added, 1 removed</span>
          <span className="text-fg" style={anim("tour-in-late")}>
            ● Run the tests now?
          </span>
          <span style={anim("tour-in-late")}>
            <span className="text-accent">  ❯ 1. Yes</span>   2. No
          </span>
        </Pane>
      </Main>
    </Frame>
  );
}

/** T3: the connection drops, the session keeps running, and you see what you missed. */
export function SceneDurable() {
  return (
    <Frame label="The connection drops, the tests keep running on the server, and a note sums up what you missed">
      <Main full>
        <Pane
          title="Terminal 2 · demo-box"
          icon={TERMINAL}
          className="flex-1"
          head={
            <span className="ml-auto flex items-center gap-1 rounded-full bg-warn/15 px-1.5 py-px font-sans text-[9.5px] text-warn opacity-0" style={anim("tour-blip")}>
              <span className="size-2 animate-spin rounded-full border-[1.5px] border-warn border-t-transparent" />
              Reconnecting… your session keeps running on the server
            </span>
          }
        >
          <div className="flex flex-col gap-[3px]" style={anim("tour-lines")}>
            <span>
              <Prompt /> npm test
            </span>
            <span>
              <span className="text-ok">PASS</span> src/routes/orders.test.ts
            </span>
            <span>
              <span className="text-ok">PASS</span> src/routes/checkout.test.ts
            </span>
            <span>
              <span className="text-ok">PASS</span> src/middleware/rateLimit.test.ts
            </span>
            <span>
              <span className="text-ok">PASS</span> src/lib/payments.test.ts
            </span>
            <span className="text-ok">Tests: 48 passed, 48 total</span>
            <span>
              <Prompt />
            </span>
          </div>
          <div className="absolute inset-x-2 bottom-2 rounded-[6px] bg-raised px-2 py-1.5 font-sans text-[10.5px] text-fg-2 ring-1 ring-ink/[0.12]" style={anim("tour-late")}>
            While you were away: <span className="text-fg">36 lines</span>, no errors
          </div>
        </Pane>
      </Main>
    </Frame>
  );
}

const CHECKOUT = [
  'router.post("/checkout", async (req, res) => {',
  "  const cart = await carts.get(req.user.id);",
  "  if (!cart.items.length) return res.status(400).end();",
  "",
  '  await rateLimit(req, "checkout");',
  "  const order = await orders.create(cart);",
  "  await payments.charge(order);",
  "  res.status(201).json(order);",
  "});",
];

/** T4: Ctrl+click a path in the output to peek at the file beside the terminal. */
export function ScenePeek() {
  return (
    <Frame label="A path in the output is Ctrl+clicked and the file opens beside the terminal, at the changed line">
      <Main full>
        <Pane title="Terminal 2 (Claude Code)" icon={CLAUDE} className="flex-1">
          <span className="text-fg">● Fixed it in</span>
          <span>
            {"  "}
            <span className="relative text-fg">
              src/routes/checkout.ts:42
              <span className="absolute inset-x-0 -bottom-[1px] h-px bg-accent" style={anim("tour-underline")} />
            </span>
          </span>
          <span className="text-fg-3">  the limit now runs before payment</span>
          <span className="absolute top-[45px] left-[184px] rounded-[4px] bg-raised px-1.5 py-0.5 font-sans text-[10px] text-fg opacity-0 ring-1 ring-ink/[0.15]" style={anim("tour-keycap")}>
            {isMac ? "⌘" : "Ctrl"} + click
          </span>
        </Pane>
        <Pane
          title={
            <>
              checkout.ts <span className="font-normal text-fg-3">src/routes</span>
            </>
          }
          icon={icon(FileCode, "text-accent")}
          head={<X size={10} className="ml-auto shrink-0 text-fg-3" />}
          className="w-[56%] shrink-0"
          style={anim("tour-open")}
        >
          {CHECKOUT.map((line, i) => (
            <span key={i} className={cn("-mx-2 flex gap-2.5 px-2", i === 4 && "bg-accent/15")}>
              <span className="w-3 shrink-0 text-right text-fg-3">{38 + i}</span>
              <span className="min-w-0 truncate">
                <Code text={line} />
              </span>
            </span>
          ))}
        </Pane>
      </Main>
      <Cursor at="translate(88px, 54px)" />
    </Frame>
  );
}

const FILE_ROWS: { name: string; folder?: boolean; size?: string; code?: boolean }[] = [
  { name: "node_modules", folder: true },
  { name: "src", folder: true },
  { name: "tests", folder: true },
  { name: ".env.example", size: "312 B" },
  { name: "package.json", size: "1.9 KB", code: true },
  { name: "README.md", size: "4.1 KB" },
  { name: "tsconfig.json", size: "612 B", code: true },
];

/** T5: drag a tab onto the page to see two at once. */
export function SceneDragSplit() {
  const top = useTopTabs();
  return (
    <Frame label="The Files tab is dragged onto the page and the page splits in two">
      <Side>
        <ServerRow machine={DEMO} />
        {!top && (
          <TabGroup>
            <TabRow label="Terminal" icon={TERMINAL} active />
            <TabRow label="Files" icon={FILES} />
            <TabRow label="Terminal 2 (Claude Code)" icon={CLAUDE} />
          </TabGroup>
        )}
        <ServerRow machine={LAB} />
        <ServerRow machine={DB} />
      </Side>
      {top && (
        <Strip
          tabs={[
            { label: "Terminal", icon: TERMINAL, active: true },
            { label: "Files", icon: FILES },
            { label: "Terminal 2 (Claude Code)", icon: CLAUDE },
          ]}
        />
      )}
      <Main className={top ? "top-9" : undefined}>
        <Pane title="Terminal" icon={TERMINAL} className="flex-[1.1]">
          <span>
            <Prompt /> ls
          </span>
          <span>
            <span className="text-accent">node_modules  src  tests</span>  package.json
          </span>
          <span>README.md  tsconfig.json</span>
          <span>
            <Prompt />
          </span>
          <div className="absolute top-[26px] right-1 bottom-1 w-[46%] rounded-[6px] bg-accent/15 opacity-0 ring-2 ring-accent/60" style={anim("tour-zone")} />
        </Pane>
        <Pane title="Files" icon={FILES} className="flex-1 font-sans" style={anim("tour-half")}>
          <span className="-mt-0.5 mb-0.5 flex items-center gap-1 font-sans text-[9.5px] text-fg-3">
            <ArrowUp size={9} /> / home / you / <span className="text-fg">app</span>
          </span>
          {FILE_ROWS.map((f) => (
            <span key={f.name} className="flex items-center gap-1.5 font-sans text-[10px] text-fg">
              {f.folder ? icon(Folder, "text-accent") : f.code ? icon(FileCode, "text-[#9d69d2]") : icon(FileText)}
              <span className="min-w-0 flex-1 truncate">{f.name}</span>
              <span className="shrink-0 text-[9.5px] text-fg-3 tabular-nums">{f.size ?? "—"}</span>
            </span>
          ))}
        </Pane>
      </Main>
      <div
        className={cn(
          "absolute flex items-center gap-1.5 rounded-[4px] bg-raised px-2 py-[3px] text-[10.5px] text-fg opacity-0 shadow-lg ring-1 ring-ink/[0.15]",
          top ? "top-[10px] left-[268px]" : "top-[85px] left-[24px]",
        )}
        style={{ ...anim("tour-ghost"), ["--to" as string]: top ? "translate(200px, 100px)" : "translate(410px, 40px)" }}
      >
        {FILES} Files
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
      <span className="[grid-area:1/1] whitespace-nowrap" style={anim("tour-show-masked")}>
        {hidden}
      </span>
    </span>
  );
  const top = useTopTabs();
  return (
    <Frame label="Streamer mode is switched on and server names and addresses are replaced">
      <Side>
        <ServerRow machine={DEMO} name={swap("demo-box", "Server 1")} />
        {!top && (
          <TabGroup>
            <TabRow label="Terminal" icon={TERMINAL} active />
          </TabGroup>
        )}
        <ServerRow machine={LAB} name={swap("homelab", "Server 2")} />
        <ServerRow machine={DB} name={swap("db-main", "Server 3")} />
        <div className="mt-auto mb-2 flex items-center gap-1.5 px-1.5 text-[10.5px] text-fg-2">
          <span className="relative h-2.5 w-5 rounded-full bg-accent" style={anim("tour-track")}>
            <span className="absolute top-[1px] left-[1px] size-2 rounded-full bg-white" style={{ ...anim("tour-knob"), transform: "translateX(10px)" }} />
          </span>
          Streamer mode
        </div>
      </Side>
      {top && <Strip tabs={[{ label: "Terminal", icon: TERMINAL, active: true }]} />}
      <Main className={top ? "top-9" : undefined}>
        <Pane title={<>Terminal · {swap("you@demo-box", "Server 1")}</>} icon={TERMINAL} className="flex-1">
          <span>{swap("Last login: Fri Oct 30 09:12 from 198.51.100.3", "Last login: Fri Oct 30 09:12 from address hidden")}</span>
          <span>
            <span className="text-ok">you@{swap("demo-box", "server-1")}</span>:~/app$ ssh-add -l
          </span>
          <span>256 SHA256:Xr4…k2Q you@laptop (ED25519)</span>
        </Pane>
      </Main>
    </Frame>
  );
}

/** For people who knew the old layout: the tabs along the top move under their server. */
export function SceneNewLayout() {
  return (
    <Frame label="The tabs along the top move into the sidebar, under their server">
      <Side>
        <ServerRow machine={DEMO} />
        <TabGroup style={anim("tour-strip-in")}>
          <TabRow label="Terminal" icon={TERMINAL} active />
          <TabRow label="Terminal 2 (Claude Code)" icon={CLAUDE} />
          <TabRow label="Files" icon={FILES} />
        </TabGroup>
        <ServerRow machine={LAB} />
        <ServerRow machine={DB} />
      </Side>
      <div className="absolute top-2 right-2 left-[194px] flex gap-1 opacity-0" style={anim("tour-strip-out")}>
        {[
          ["Terminal", TERMINAL],
          ["Terminal 2 (Claude Code)", CLAUDE],
          ["Files", FILES],
        ].map(([t, i], k) => (
          <span key={t as string} className={cn("flex items-center gap-1 rounded-[5px] px-2 py-[3px] text-[10.5px]", k === 0 ? "bg-ink/[0.1] text-fg" : "text-fg-3")}>
            {i}
            {t}
          </span>
        ))}
      </div>
      <Main className="top-9">
        <Pane title="Terminal" icon={TERMINAL} className="flex-1">
          <span>
            <Prompt /> <span className="inline-block h-2.5 w-1 bg-fg-2 align-middle" />
          </span>
        </Pane>
      </Main>
    </Frame>
  );
}

/** The main menu: a start page like a browser's new tab. */
export function SceneMainMenu() {
  const tiles: { name: string; icon: LucideIcon; color: string | null; waiting?: boolean; open?: number }[] = [
    { name: "This computer", icon: Laptop, color: null },
    { ...DEMO, waiting: true },
    { ...LAB, open: 1 },
    { ...DB },
  ];
  return (
    <Frame label="The main menu shows search, your servers as tiles and the agents at work">
      <Side menu>
        <ServerRow machine={DEMO} />
        <ServerRow machine={LAB} />
        <ServerRow machine={DB} />
      </Side>
      <Main className="flex-col items-center gap-3 rounded-[8px] bg-canvas pt-4 ring-1 ring-ink/[0.1]">
        <span className="flex h-7 w-[78%] shrink-0 items-center gap-2 rounded-[8px] bg-ink/[0.05] px-2.5 text-[10.5px] text-fg-3 ring-1 ring-ink/[0.08]" style={anim("tour-in-1")}>
          <Search size={11} /> Servers, folders, commands
          <span className="ml-auto text-[9.5px]">{shortcutLabel("palette")}</span>
        </span>
        <div className="flex gap-1.5">
          {tiles.map((t, i) => (
            <span key={t.name} className="flex w-[68px] flex-col items-center gap-1" style={anim(i < 2 ? "tour-in-2" : "tour-in-3")}>
              <span
                className="relative flex size-8 items-center justify-center rounded-[9px] ring-1 ring-inset ring-ink/[0.08]"
                style={{ background: t.color ? `color-mix(in srgb, ${t.color} 16%, transparent)` : "rgb(var(--ui-ink) / 0.06)" }}
              >
                <t.icon size={15} strokeWidth={1.8} className={t.color ? undefined : "text-fg-2"} style={t.color ? { color: t.color } : undefined} />
                {t.waiting && <span className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-warn ring-2 ring-[var(--ui-canvas)]" />}
                {t.open && (
                  <span className="absolute -right-1 -bottom-1 flex size-3 items-center justify-center rounded-full bg-raised text-[8px] font-semibold text-fg-2 ring-1 ring-ink/[0.12]">{t.open}</span>
                )}
              </span>
              <span className="max-w-full truncate text-[9.5px] text-fg">{t.name}</span>
            </span>
          ))}
        </div>
        <div className="flex w-[84%] flex-col gap-0.5" style={anim("tour-in-late")}>
          <span className="px-1.5 text-[9px] font-semibold text-fg-3">Running now</span>
          <span className="flex h-5 items-center gap-1.5 rounded-[5px] px-1.5 text-[10.5px] text-fg">
            {CLAUDE}
            <span className="min-w-0 flex-1 truncate">
              Terminal 2 (Claude Code) <span className="text-fg-3">on demo-box</span>
            </span>
            <span className="text-[9.5px] font-medium text-warn">Needs you</span>
          </span>
          <span className="flex h-5 items-center gap-1.5 rounded-[5px] px-1.5 text-[10.5px] text-fg">
            {CODEX}
            <span className="min-w-0 flex-1 truncate">
              Terminal (Codex) <span className="text-fg-3">on homelab</span>
            </span>
            <span className="text-[9.5px] text-fg-3">working</span>
          </span>
        </div>
      </Main>
    </Frame>
  );
}
