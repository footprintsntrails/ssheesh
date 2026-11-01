// Two looks at a server: a security check-up, and what's using the disk.
// Looking changes nothing. A fix or cleanup runs only when you press Run, on
// the server without a terminal, or in one when it needs you to answer.

import { BookOpen, CircleAlert, CircleCheck, CircleHelp, Copy, File, Folder, FolderOpen, HardDrive, Play, RefreshCw, ShieldCheck, TriangleAlert, X, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { bytes, cn, percent } from "../lib/format";
import { useNames } from "../lib/privacy";
import { useStore } from "../lib/store";
import { RECIPE_FOR_CHECK } from "../lib/recipes";
import { findTerminal } from "../lib/terminals";
import { targetKey, toOpError, type Check, type Cleanup, type OpError, type RunOutcome, type TargetRef } from "../lib/types";
import { Modal } from "./Overlays";
import { openRecipes } from "./Recipes";
import { showFiles } from "../lib/tabs";
import { Segmented } from "./controls";
import { Button, IconButton, Spinner } from "./ui";

type Tool = "checkup" | "disk";

const useTool = create<{ open: { tool: Tool; target: TargetRef } | null }>(() => ({ open: null }));

export function openServerTool(tool: Tool, target: TargetRef) {
  useTool.setState({ open: { tool, target } });
}

const close = () => useTool.setState({ open: null });

export function ServerTools() {
  const open = useTool((s) => s.open);
  if (!open) return null;
  const key = `${open.tool}:${targetKey(open.target)}`;
  return open.tool === "checkup" ? <CheckupDialog key={key} target={open.target} /> : <DiskDialog key={key} target={open.target} />;
}

/** Runs `run` now and on `reload`, keeping the last result while it reloads. */
export function useRun<T>(run: () => Promise<T>) {
  const [state, setState] = useState<{ data?: T; error?: OpError; loading: boolean }>({ loading: true });
  const reload = useCallback(() => {
    let live = true;
    setState((s) => ({ ...s, loading: true, error: undefined }));
    run().then(
      (data) => live && setState({ data, loading: false }),
      (e) => live && setState((s) => ({ ...s, error: toOpError(e), loading: false })),
    );
    return () => {
      live = false;
    };
  }, [run]);
  useEffect(reload, [reload]);
  return { ...state, reload };
}

export function Header({
  icon: Icon,
  title,
  sub,
  loading = false,
  onReload,
  onClose = close,
}: {
  icon: LucideIcon;
  title: string;
  sub: string;
  loading?: boolean;
  /** Shows "Check again" when set. */
  onReload?: () => void;
  onClose?: () => void;
}) {
  return (
    <div className="flex items-start gap-3">
      <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full bg-accent/15 text-accent">
        <Icon size={15} />
      </span>
      <div className="min-w-0 flex-1">
        <h2 className="text-[15px] font-semibold text-fg">{title}</h2>
        <p className="mt-0.5 truncate text-[12px] text-fg-3">{sub}</p>
      </div>
      {onReload && (
        <Button size="sm" variant="ghost" onClick={onReload} disabled={loading}>
          <RefreshCw size={12.5} className={cn(loading && "animate-spin")} /> Check again
        </Button>
      )}
      <IconButton label="Close" onClick={onClose} className="-mr-1.5 size-7">
        <X size={15} />
      </IconButton>
    </div>
  );
}

export function Waiting({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-2 py-8 text-[12.5px] text-fg-3">
      <Spinner className="size-3.5" /> {children}
    </div>
  );
}

export function Note({ tone = "info", children }: { tone?: "info" | "error"; children: ReactNode }) {
  return (
    <p className={cn("mt-3 rounded-[9px] px-3 py-2 text-[12px] leading-snug", tone === "error" ? "bg-bad/10 text-bad" : "bg-ink/[0.05] text-fg-2")}>
      {children}
    </p>
  );
}

const NEEDS_ROOT = "Some of this needs root. Connect as root, or allow sudo without a password, to see everything.";

/**
 * Runs a command in a new terminal on the server, for one that needs you,
 * and says in plain words what to answer there.
 */
export function runInTerminal(target: TargetRef, command: string, ask: string) {
  const st = useStore.getState();
  const id = st.openIn("terminal", target, { initialCommand: command });
  close();
  st.focus(id);
  st.toast({ tone: "info", title: "Your turn, in the terminal", body: ask, action: { label: "Show it", run: () => useStore.getState().focus(id) } });
}

/**
 * Types a command into the server's terminal and runs it, as a recipe's
 * steps do, so what it prints is there to compare with. A terminal busy
 * with something else (or that can't tell) gets a new one beside it.
 */
async function typeAndRun(target: TargetRef, command: string) {
  const st = useStore.getState();
  const main = st.mainTerminalFor(target);
  const key = main ? st.panes[main]?.sessionKey : undefined;
  const info = key ? await backend.paneInfo(target, key).catch(() => null) : null;
  if (main && info?.idle) {
    // Ctrl+E Ctrl+U first: clear anything half-typed at the prompt.
    backend.writeSession(main, `\x05\x15${command}\r`);
    st.focus(main);
    findTerminal(main)?.focus();
    return;
  }
  st.focus(st.openIn("terminal", target, { initialCommand: command }));
}

/**
 * A fix or cleanup to run. Run does it on the server without a terminal and
 * shows how it went; one that needs you (sudo's password, a package manager's
 * "go ahead?") opens in a terminal instead, with what to answer. `display` is
 * what's shown when it differs (streamer mode); `onRan` reloads the dialog.
 * With `inTerminal` (a recipe's steps) Run types it into the server's
 * terminal instead, and `inTerminal` gets the steps out of the way.
 */
export function Command({
  command,
  display,
  note,
  target,
  onRan,
  inTerminal,
}: {
  command: string;
  display?: string;
  note?: string | null;
  target: TargetRef;
  onRan?: () => void;
  inTerminal?: () => void;
}) {
  const names = useNames();
  const toast = useStore((s) => s.toast);
  const confirm = useStore((s) => s.confirm);
  const [ran, setRan] = useState<Exclude<RunOutcome, { kind: "needsYou" }> | "running" | null>(null);
  const run = async () => {
    if (inTerminal) {
      inTerminal();
      await typeAndRun(target, command);
      return;
    }
    if (!(await confirm({ title: `Run this on ${names.target(target)}?`, body: display ?? command, confirmLabel: "Run" }))) return;
    setRan("running");
    try {
      const outcome = await backend.runFix(target, command);
      if (outcome.kind === "needsYou") {
        setRan(null);
        runInTerminal(target, command, outcome.ask);
        return;
      }
      setRan(outcome);
      if (outcome.kind === "done") onRan?.();
    } catch (e) {
      setRan({ kind: "failed", output: toOpError(e).message, code: null });
    }
  };
  const copy = () => void backend.writeClipboard(command).then(() => toast({ tone: "info", title: "Copied", body: display ?? command }));
  const outcome = ran && ran !== "running" ? ran : null;
  return (
    <div className="mt-2.5">
      <code className="selectable block rounded-[9px] bg-ink/[0.05] px-3 py-2 font-mono text-[11.5px] leading-relaxed break-all text-fg ring-1 ring-inset ring-ink/[0.06]">
        {display ?? command}
      </code>
      {note && <p className="mt-1.5 text-[11.5px] leading-snug text-fg-3">{note}</p>}
      <div className="mt-1.5 flex flex-wrap gap-1.5">
        <Button size="sm" onClick={() => void run()} disabled={ran === "running"}>
          {ran === "running" ? <Spinner className="size-3" /> : <Play size={12.5} />} {ran === "running" ? "Running…" : "Run"}
        </Button>
        <Button size="sm" variant="ghost" onClick={copy}>
          <Copy size={12.5} /> Copy
        </Button>
      </div>
      {outcome && (
        <div className="mt-2">
          <p className={cn("flex items-center gap-1.5 text-[12px] font-medium", outcome.kind === "done" ? "text-ok" : "text-bad")}>
            {outcome.kind === "done" ? <CircleCheck size={13} /> : <CircleAlert size={13} />}
            {outcome.kind === "done" ? "Done" : outcome.code != null ? `It stopped with an error (code ${outcome.code})` : "It stopped before the end"}
          </p>
          {outcome.output && (
            <pre className="selectable mt-1 max-h-32 overflow-auto rounded-[9px] bg-ink/[0.04] px-3 py-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all text-fg-2">
              {names.text(outcome.output)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- security check-up

const VERDICT: Record<Check["verdict"], { icon: LucideIcon; className: string; order: number }> = {
  bad: { icon: CircleAlert, className: "text-bad", order: 0 },
  warn: { icon: TriangleAlert, className: "text-warn", order: 1 },
  unknown: { icon: CircleHelp, className: "text-fg-3", order: 2 },
  good: { icon: CircleCheck, className: "text-ok", order: 3 },
};

function CheckupDialog({ target }: { target: TargetRef }) {
  const names = useNames();
  const run = useCallback(() => backend.securityCheckup(target), [target]);
  const { data, error, loading, reload } = useRun(run);
  // Problems first; the order within a verdict stays the server's.
  const checks = data ? [...data.checks].sort((a, b) => VERDICT[a.verdict].order - VERDICT[b.verdict].order) : [];
  const count = (v: Check["verdict"]) => checks.filter((c) => c.verdict === v).length;
  const summary = [
    count("bad") && <span key="b" className="text-bad">{count("bad")} to fix</span>,
    count("warn") && <span key="w" className="text-warn">{count("warn")} worth a look</span>,
    count("good") && <span key="g" className="text-ok">{count("good")} good</span>,
    count("unknown") && <span key="u">{count("unknown")} couldn't tell</span>,
  ].filter(Boolean);
  return (
    <Modal open onClose={close} width={600}>
      <div className="p-5">
        <Header icon={ShieldCheck} title="Security check-up" sub={`${names.target(target)} · nothing changes until you run a fix`} loading={loading} onReload={reload} />
        {loading && !data && <Waiting>Checking logins, open ports, the firewall and updates…</Waiting>}
        {error && <Note tone="error">{error.message}</Note>}
        {data && (
          <>
            <p className="mt-4 flex flex-wrap gap-x-3 text-[12.5px] font-medium text-fg-3">{summary}</p>
            {!data.asRoot && <Note>{NEEDS_ROOT}</Note>}
            <ul className={cn("mt-3 flex flex-col gap-2 transition-opacity", loading && "opacity-50")}>
              {checks.map((c) => {
                const { icon: Icon, className } = VERDICT[c.verdict];
                return (
                  <li key={c.id} className="flex items-start gap-2.5 rounded-[11px] bg-ink/[0.035] px-3.5 py-3 ring-1 ring-inset ring-ink/[0.06]">
                    <Icon size={15} className={cn("mt-px shrink-0", className)} />
                    <div className="min-w-0 flex-1">
                      <p className="text-[13px] font-medium text-fg">{c.title}</p>
                      <p className="selectable mt-0.5 text-[12px] leading-relaxed text-fg-2">{names.text(c.detail)}</p>
                      {c.fix && c.verdict !== "good" && <Command command={c.fix} note={c.fixNote} target={target} onRan={reload} />}
                      {!c.fix && c.fixNote && c.verdict !== "good" && <p className="mt-1.5 text-[11.5px] leading-snug text-fg-3">{c.fixNote}</p>}
                      {RECIPE_FOR_CHECK[c.id] && c.verdict !== "good" && (
                        <button
                          onClick={() => {
                            close();
                            openRecipes(target, RECIPE_FOR_CHECK[c.id]);
                          }}
                          className="mt-2 inline-flex items-center gap-1 text-[11.5px] font-medium text-accent hover:underline"
                        >
                          <BookOpen size={12} /> Open the recipe
                        </button>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          </>
        )}
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- disk usage

export function Bar({ fraction, className }: { fraction: number; className?: string }) {
  return (
    <div className="h-1.5 overflow-hidden rounded-full bg-ink/[0.08]">
      <div className={cn("h-full rounded-full bg-accent", className)} style={{ width: `${Math.max(1, Math.min(1, fraction) * 100)}%` }} />
    </div>
  );
}

export function Section({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className="mt-5">
      <div className="mb-2 flex items-baseline justify-between gap-3">
        <h3 className="text-[12px] font-semibold text-fg-2">{title}</h3>
        {aside}
      </div>
      {children}
    </section>
  );
}

/** A cleanup, and when it comes in strengths (how old temporary files must be), a pick. */
function CleanupCard({ cleanup: c, target, onRan }: { cleanup: Cleanup; target: TargetRef; onRan: () => void }) {
  const [pick, setPick] = useState(() => Math.max(0, c.choices?.findIndex((x) => x.command === c.command) ?? 0));
  const chosen = c.choices?.[pick];
  const size = chosen ? chosen.bytes : c.bytes;
  return (
    <li className="rounded-[11px] bg-ink/[0.035] px-3.5 py-3 ring-1 ring-inset ring-ink/[0.06]">
      <div className="flex items-baseline justify-between gap-3">
        <p className="text-[13px] font-medium text-fg">{c.title}</p>
        {size != null && (
          <span className={cn("shrink-0 text-[12px] font-medium tabular-nums", size > 0 ? "text-ok" : "text-fg-3")}>
            {size > 0 ? `frees about ${bytes(size)}` : "nothing that old"}
          </span>
        )}
      </div>
      <p className="mt-0.5 text-[12px] leading-relaxed text-fg-2">{c.detail}</p>
      {c.choices && c.choices.length > 1 && (
        <div className="mt-2" role="radiogroup" aria-label={`${c.title}: how old`}>
          <Segmented value={String(pick)} options={c.choices.map((x, i) => ({ value: String(i), label: x.label }))} onChange={(v) => setPick(Number(v))} />
        </div>
      )}
      <Command command={chosen?.command ?? c.command} target={target} onRan={onRan} />
    </li>
  );
}

function DiskDialog({ target }: { target: TargetRef }) {
  const names = useNames();
  const [path, setPath] = useState("/");
  const report = useRun(useCallback(() => backend.diskReport(target), [target]));
  const list = useRun(useCallback(() => backend.diskBiggest(target, path), [target, path]));
  const r = report.data;
  const top = list.data?.[0]?.bytes ?? 0;
  const crumbs = path === "/" ? [] : path.split("/").filter(Boolean);
  const dockerRows: [string, number, number][] = r?.docker
    ? [
        ["Images", r.docker.images, r.docker.imagesReclaimable],
        ["Containers", r.docker.containers, r.docker.containersReclaimable],
        ["Volumes", r.docker.volumes, r.docker.volumesReclaimable],
        ["Build cache", r.docker.buildCache, r.docker.buildCacheReclaimable],
      ]
    : [];

  return (
    <Modal open onClose={close} width={620}>
      <div className="p-5">
        <Header
          icon={HardDrive}
          title="Free up disk space"
          sub={`${names.target(target)} · nothing is deleted until you run a cleanup`}
          loading={report.loading || list.loading}
          onReload={() => {
            report.reload();
            list.reload();
          }}
        />
        {report.loading && !r && <Waiting>Looking at the disks, Docker and logs…</Waiting>}
        {report.error && <Note tone="error">{report.error.message}</Note>}
        {r && !r.asRoot && <Note>{NEEDS_ROOT}</Note>}

        {r && r.volumes.length > 0 && (
          <Section title="Disks">
            <div className="flex flex-col gap-2.5">
              {r.volumes.map((v) => {
                const full = v.size ? v.used / v.size : 0;
                return (
                  <div key={v.mount}>
                    <div className="mb-1 flex justify-between gap-3 text-[12px] tabular-nums">
                      <span className="truncate font-mono text-fg">{names.text(v.mount)}</span>
                      <span className={cn("shrink-0", full >= 0.9 ? "text-bad" : full >= 0.8 ? "text-warn" : "text-fg-3")}>
                        {bytes(v.used)} of {bytes(v.size)} · {percent(full)}
                      </span>
                    </div>
                    <Bar fraction={full} className={cn(full >= 0.9 && "bg-bad", full >= 0.8 && full < 0.9 && "bg-warn")} />
                  </div>
                );
              })}
            </div>
          </Section>
        )}

        {r && r.cleanups.length > 0 && (
          <Section title="Safe to clean up">
            <ul className="flex flex-col gap-2">
              {r.cleanups.map((c) => (
                <CleanupCard key={c.id} cleanup={c} target={target} onRan={() => (report.reload(), list.reload())} />
              ))}
            </ul>
          </Section>
        )}

        <Section
          title="Biggest here"
          aside={
            <button onClick={() => showFiles(target, path)} className="inline-flex items-center gap-1 text-[11.5px] text-fg-3 hover:text-fg">
              <FolderOpen size={12} /> Open in Files
            </button>
          }
        >
          <nav className="mb-2 flex flex-wrap items-center gap-0.5 font-mono text-[12px]" aria-label="Folder">
            <button onClick={() => setPath("/")} className={cn("rounded px-1 hover:bg-ink/[0.07]", crumbs.length ? "text-fg-3" : "text-fg")}>
              /
            </button>
            {crumbs.map((part, i) => {
              const to = `/${crumbs.slice(0, i + 1).join("/")}`;
              return (
                <span key={to} className="flex items-center gap-0.5">
                  {i > 0 && <span className="text-fg-3">/</span>}
                  <button onClick={() => setPath(to)} className={cn("rounded px-1 hover:bg-ink/[0.07]", i === crumbs.length - 1 ? "text-fg" : "text-fg-3")}>
                    {names.text(part)}
                  </button>
                </span>
              );
            })}
          </nav>
          {list.loading && !list.data && <Waiting>Measuring folders. On a big disk this can take a minute…</Waiting>}
          {list.error && <Note tone="error">{list.error.message}</Note>}
          {list.data && (
            <ul className={cn("flex flex-col transition-opacity", list.loading && "opacity-50")}>
              {list.data.length === 0 && <li className="py-2 text-[12px] text-fg-3">Nothing here takes up space.</li>}
              {list.data.map((u) => (
                <li key={u.path}>
                  <button
                    disabled={!u.isDir || list.loading}
                    onClick={() => setPath(u.path)}
                    title={u.isDir ? `Look inside ${names.text(u.path)}` : names.text(u.path)}
                    className="grid w-full grid-cols-[16px_minmax(0,1fr)_96px_64px] items-center gap-2.5 rounded-[7px] px-2 py-1.5 text-left text-[12.5px] enabled:hover:bg-ink/[0.06]"
                  >
                    {u.isDir ? <Folder size={13.5} className="text-accent" /> : <File size={13.5} className="text-fg-3" />}
                    <span className="truncate text-fg">{names.text(u.name)}</span>
                    <Bar fraction={top ? u.bytes / top : 0} />
                    <span className="text-right text-fg-2 tabular-nums">{bytes(u.bytes)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Section>

        {dockerRows.length > 0 && (
          <Section title="Docker">
            <div className="grid grid-cols-2 gap-2">
              {dockerRows.map(([label, size, unused]) => (
                <div key={label} className="rounded-[9px] bg-ink/[0.035] px-3 py-2 ring-1 ring-inset ring-ink/[0.06]">
                  <p className="text-[11.5px] text-fg-3">{label}</p>
                  <p className="text-[13px] font-medium text-fg tabular-nums">
                    {bytes(size)}
                    {unused > 0 && <span className="ml-1.5 text-[11.5px] font-normal text-fg-3">{bytes(unused)} unused</span>}
                  </p>
                </div>
              ))}
            </div>
          </Section>
        )}

        {r?.journal != null && <p className="mt-4 text-[12px] text-fg-3">System logs (journal): {bytes(r.journal)}</p>}
      </div>
    </Modal>
  );
}
