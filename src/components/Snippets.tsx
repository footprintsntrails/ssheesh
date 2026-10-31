// Snippets: the list, the editor, and the little form that asks for a
// snippet's blanks before it runs in the server's terminal.

import { Braces, ClipboardPaste, Pencil, Play, Plus, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { blankLabel, blanks, fill, useSnippets, type Snippet } from "../lib/snippets";
import { currentTarget, useStore } from "../lib/store";
import { sameTarget, type Container, type TargetRef } from "../lib/types";
import { Modal } from "./Overlays";
import { Button, IconButton } from "./ui";

type View = { kind: "list" } | { kind: "edit"; snippet: Partial<Snippet> } | { kind: "fill"; snippet: Snippet; target: TargetRef };

const useView = create<{ view: View | null }>(() => ({ view: null }));
const close = () => useView.setState({ view: null });

export function openSnippets() {
  useView.setState({ view: { kind: "list" } });
}

export function newSnippet(command = "") {
  useView.setState({ view: { kind: "edit", snippet: { command } } });
}

/** The machine a snippet runs on: the focused pane's. */
function focusedTarget(): TargetRef | null {
  return currentTarget(useStore.getState());
}

/** Runs a snippet in the focused machine's terminal, asking for its blanks first. */
export function runSnippet(snippet: Snippet, target = focusedTarget()) {
  if (!target) {
    useStore.getState().toast({ tone: "info", title: "Pick a server first", body: "Snippets run in the terminal of the server you're looking at." });
    return;
  }
  if (blanks(snippet.command).length) useView.setState({ view: { kind: "fill", snippet, target } });
  else send(snippet.command, target, true);
}

/** Types a command into the machine's terminal (the focused one if it's there), with Enter or without. */
function send(command: string, target: TargetRef, enter: boolean) {
  const st = useStore.getState();
  const focused = st.panes[st.focused];
  const id =
    focused?.kind === "terminal" && !focused.container && sameTarget(focused.target, target) ? focused.id : st.mainTerminalFor(target);
  if (!id) {
    st.openIn("terminal", target, { split: true, initialCommand: enter ? command : undefined });
    return;
  }
  st.focus(id);
  void import("../lib/terminals").then((t) => {
    const term = t.findTerminal(id);
    // Pasting goes in as a paste, so a shell doesn't run each line of a
    // multi-line snippet as it arrives.
    if (enter) backend.writeSession(id, `${command}\r`);
    else if (term) term.term.paste(command);
    else backend.writeSession(id, command);
    term?.focus();
  });
}

function Title({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  return (
    <div className="flex items-start justify-between gap-3 px-5 pt-5 pb-3">
      <h2 className="text-[16px] font-semibold tracking-tight text-fg">{children}</h2>
      <IconButton label="Close" onClick={onClose} className="-mt-1 -mr-2 size-7">
        <X size={15} />
      </IconButton>
    </div>
  );
}

function List() {
  const snippets = useSnippets((s) => s.snippets);
  const remove = useSnippets((s) => s.remove);
  return (
    <>
      <Title onClose={close}>Snippets</Title>
      <p className="-mt-1 px-5 pb-3 text-[12.5px] leading-relaxed text-fg-3">
        Commands you keep, run in the terminal of the server you're looking at. Find them in the command palette too: type <span className="font-mono text-fg-2">cmd:</span>
      </p>
      <div className="flex flex-col gap-1 px-3 pb-3">
        {snippets.map((s) => (
          <div key={s.id} className="group flex items-center gap-2 rounded-[10px] px-2 py-1.5 hover:bg-ink/[0.04]">
            <div className="min-w-0 flex-1">
              <div className="truncate text-[13px] font-medium text-fg">{s.name}</div>
              <div className="truncate font-mono text-[11.5px] text-fg-3">{s.command}</div>
            </div>
            <IconButton label="Edit" onClick={() => useView.setState({ view: { kind: "edit", snippet: s } })} className="size-7 opacity-60 group-hover:opacity-100">
              <Pencil size={13} />
            </IconButton>
            <IconButton label="Delete" onClick={() => remove(s.id)} className="size-7 opacity-60 group-hover:opacity-100">
              <Trash2 size={13} />
            </IconButton>
            <Button size="sm" onClick={() => runSnippet(s)}>
              <Play size={12} /> Run
            </Button>
          </div>
        ))}
        {!snippets.length && <p className="px-2 py-6 text-center text-[12.5px] text-fg-3">No snippets yet.</p>}
      </div>
      <div className="flex justify-end border-t border-ink/[0.07] px-5 py-3">
        <Button size="sm" variant="primary" onClick={() => newSnippet()}>
          <Plus size={13} /> New snippet
        </Button>
      </div>
    </>
  );
}

const field =
  "w-full rounded-[8px] bg-ink/[0.05] px-2.5 text-[13px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] placeholder:text-fg-3 focus:ring-2 focus:ring-accent/60";

function Edit({ snippet }: { snippet: Partial<Snippet> }) {
  const save = useSnippets((s) => s.save);
  const [name, setName] = useState(snippet.name ?? "");
  const [command, setCommand] = useState(snippet.command ?? "");
  const found = blanks(command);
  const ok = name.trim() && command.trim();
  const done = () => {
    if (!ok) return;
    save({ id: snippet.id, name, command });
    useView.setState({ view: { kind: "list" } });
  };
  return (
    <>
      <Title onClose={close}>{snippet.id ? "Edit snippet" : "New snippet"}</Title>
      <div className="flex flex-col gap-3 px-5 pb-4">
        <label className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium text-fg-2">Name</span>
          <input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Restart the web app" className={cn(field, "h-8")} />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium text-fg-2">Command</span>
          <textarea
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && done()}
            placeholder="docker compose restart {{service}}"
            spellCheck={false}
            rows={3}
            className={cn(field, "resize-y py-2 font-mono text-[12.5px]")}
          />
        </label>
        <p className="flex gap-2 text-[11.5px] leading-relaxed text-fg-3">
          <Braces size={13} className="mt-0.5 shrink-0" />
          <span>
            Leave a blank as <span className="font-mono text-fg-2">{"{{name}}"}</span>, or <span className="font-mono text-fg-2">{"{{name:default}}"}</span>, and SSHeesh asks for it
            each time. <span className="font-mono text-fg-2">{"{{container}}"}</span> lets you pick one of the server's containers.
            {found.length > 0 && <span className="text-fg-2"> Blanks here: {found.map((b) => b.name).join(", ")}.</span>}
          </span>
        </p>
      </div>
      <div className="flex justify-end gap-2 border-t border-ink/[0.07] px-5 py-3">
        <Button size="sm" onClick={() => useView.setState({ view: { kind: "list" } })}>
          Cancel
        </Button>
        <Button size="sm" variant="primary" disabled={!ok} onClick={done}>
          Save
        </Button>
      </div>
    </>
  );
}

function Fill({ snippet, target }: { snippet: Snippet; target: TargetRef }) {
  const list = useMemo(() => blanks(snippet.command), [snippet.command]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [containers, setContainers] = useState<Container[] | null>(null);
  const first = useRef<HTMLInputElement | HTMLSelectElement | null>(null);
  const wantsContainer = list.some((b) => /^containers?$/i.test(b.name));
  useEffect(() => {
    if (!wantsContainer) return;
    backend
      .listContainers(target)
      .then((l) => setContainers([...l.containers].sort((a, b) => Number(b.running) - Number(a.running) || a.name.localeCompare(b.name))))
      .catch(() => setContainers([]));
  }, [target, wantsContainer]);
  useEffect(() => first.current?.focus(), []);
  const command = fill(snippet.command, values);
  const missing = list.some((b) => !values[b.name]?.trim() && !b.fallback);
  const go = (enter: boolean) => {
    if (missing) return;
    close();
    send(command, target, enter);
  };
  return (
    <>
      <Title onClose={close}>{snippet.name}</Title>
      <form
        className="flex flex-col gap-3 px-5 pb-4"
        onSubmit={(e) => {
          e.preventDefault();
          go(true);
        }}
      >
        {list.map((b, i) => {
          const set = (v: string) => setValues((x) => ({ ...x, [b.name]: v }));
          const pick = /^containers?$/i.test(b.name) && containers && containers.length > 0;
          return (
            <label key={b.name} className="flex flex-col gap-1.5">
              <span className="text-[12px] font-medium text-fg-2">
                {blankLabel(b.name)}
                {b.fallback && <span className="font-normal text-fg-3"> · {b.fallback} if left empty</span>}
              </span>
              {pick ? (
                <select
                  ref={i === 0 ? (el) => void (first.current = el) : undefined}
                  value={values[b.name] ?? ""}
                  onChange={(e) => set(e.target.value)}
                  className={cn(field, "h-8")}
                >
                  <option value="">Pick a container…</option>
                  {containers.map((c) => (
                    <option key={c.id} value={c.name}>
                      {c.name}
                      {c.running ? "" : " (stopped)"}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  ref={i === 0 ? (el) => void (first.current = el) : undefined}
                  value={values[b.name] ?? ""}
                  onChange={(e) => set(e.target.value)}
                  placeholder={b.fallback || (/^containers?$/i.test(b.name) && containers === null ? "Looking for containers…" : "")}
                  spellCheck={false}
                  className={cn(field, "h-8 font-mono text-[12.5px]")}
                />
              )}
            </label>
          );
        })}
        <div className="rounded-[8px] bg-ink/[0.04] px-2.5 py-2 font-mono text-[12px] break-all text-fg-2">{command}</div>
        <button type="submit" hidden />
      </form>
      <div className="flex justify-end gap-2 border-t border-ink/[0.07] px-5 py-3">
        <Button size="sm" disabled={missing} onClick={() => go(false)} title="Type it into the terminal without pressing Enter">
          <ClipboardPaste size={13} /> Paste
        </Button>
        <Button size="sm" variant="primary" disabled={missing} onClick={() => go(true)}>
          <Play size={12} /> Run
        </Button>
      </div>
    </>
  );
}

export function Snippets() {
  const view = useView((s) => s.view);
  return (
    <Modal open={!!view} onClose={close} width={view?.kind === "list" ? 520 : 460}>
      {view?.kind === "list" && <List />}
      {view?.kind === "edit" && <Edit key={view.snippet.id ?? "new"} snippet={view.snippet} />}
      {view?.kind === "fill" && <Fill key={view.snippet.id} snippet={view.snippet} target={view.target} />}
    </Modal>
  );
}
