// Recipes: step-by-step guides for common server jobs. Each step says what it
// does and why, then offers its command to paste into a terminal (without
// pressing Enter) or copy. After a paste the dialog steps aside into a small
// bar, so you can read the command, run it and come back to the same step.

import {
  BookOpen,
  BrickWall,
  Check,
  ChevronLeft,
  ChevronRight,
  Container,
  Globe,
  KeyRound,
  Laptop,
  MemoryStick,
  PackageCheck,
  RotateCcw,
  ShieldBan,
  TriangleAlert,
  UserCog,
  X,
  type LucideIcon,
} from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { isWindows } from "../lib/keys";
import { useNames } from "../lib/privacy";
import {
  GROUP_LABEL,
  OS_LABEL,
  RECIPES,
  fieldError,
  fieldValues,
  guessOs,
  makeContext,
  missingFields,
  parsePublicKey,
  recipeById,
  resolve,
  stepsFor,
  type Field,
  type Os,
  type Recipe,
  type RecipeContext,
  type Step,
} from "../lib/recipes";
import { useStore } from "../lib/store";
import { targetKey, type Host, type TargetRef } from "../lib/types";
import { Modal } from "./Overlays";
import { Command, Header, Note, useRun } from "./ServerTools";
import { Button, IconButton, Spinner } from "./ui";

const ICONS: Record<string, LucideIcon> = {
  "ssh-keys": KeyRound,
  firewall: BrickWall,
  fail2ban: ShieldBan,
  "auto-updates": PackageCheck,
  "sudo-user": UserCog,
  docker: Container,
  swap: MemoryStick,
  "caddy-https": Globe,
};

const LOCAL: TargetRef = { kind: "local" };
const NONE: Record<string, string> = {};
const NO_STEPS: string[] = [];

const inputClass =
  "h-8 w-full min-w-0 rounded-[8px] bg-ink/[0.05] px-2.5 font-mono text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] placeholder:font-sans placeholder:text-fg-3/70 focus:ring-2 focus:ring-accent/70";

interface RecipesState {
  open: { target: TargetRef; recipe: string | null } | null;
  /** Stepped aside after a paste; a small bar brings the steps back. */
  aside: boolean;
  /** Ticked-off step titles, by recipe key. Kept until the app quits. */
  done: Record<string, string[]>;
  /** What was typed into a recipe's fields, by recipe key. */
  typed: Record<string, Record<string, string>>;
  /** The system picked by hand, by target key. */
  os: Record<string, Os>;
  /** The step whose command was pasted last, by recipe key. */
  last: Record<string, string>;
}

const useRecipes = create<RecipesState>(() => ({ open: null, aside: false, done: {}, typed: {}, os: {}, last: {} }));

/** Opens the recipes for a server: the list, or straight at one recipe. */
export function openRecipes(target: TargetRef, recipeId?: string) {
  useRecipes.setState({ open: { target, recipe: recipeById(recipeId)?.id ?? null }, aside: false });
}

const close = () => useRecipes.setState({ open: null, aside: false });
const show = (recipe: string | null) => useRecipes.setState((s) => ({ open: s.open && { ...s.open, recipe }, aside: false }));
const recipeKey = (t: TargetRef, id: string) => `${targetKey(t)}|${id}`;

/** The recipe stepped aside, in the top bar: where the tabs leave room, never over them. */
export function RecipeAside() {
  const open = useRecipes((s) => s.open);
  const aside = useRecipes((s) => s.aside);
  const recipe = recipeById(open?.recipe);
  return open && aside && recipe ? <AsideBar target={open.target} recipe={recipe} /> : null;
}

/** Always mounted; shows the dialog (stepped aside, it's `RecipeAside`). */
export function Recipes() {
  const open = useRecipes((s) => s.open);
  const aside = useRecipes((s) => s.aside);
  const recipe = recipeById(open?.recipe);
  if (!open || (aside && recipe)) return null;
  return (
    <Modal open onClose={close} width={620}>
      <div className="p-5">
        {recipe ? <RecipeView key={recipe.id} target={open.target} recipe={recipe} /> : <RecipeList target={open.target} />}
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- shared bits

function useHost(target: TargetRef): Host | null {
  return useStore((s) => (target.kind === "host" ? (s.hosts.find((h) => h.id === target.hostId) ?? null) : null));
}

/** Guesses from the server's kernel, kept for the session. */
const guesses = new Map<string, Os | null>();

/** The server's kind of system: picked by hand, else guessed, else Debian/Ubuntu. */
function useOs(target: TargetRef) {
  const key = targetKey(target);
  const picked = useRecipes((s) => s.os[key]);
  const [guess, setGuess] = useState(() => guesses.get(key) ?? null);
  useEffect(() => {
    if (picked || target.kind !== "host" || guesses.has(key)) return;
    let live = true;
    backend.fetchMetrics(target).then(
      (m) => {
        const g = guessOs(m.kernel);
        guesses.set(key, g);
        if (live) setGuess(g);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [key, picked]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = useCallback((os: Os) => useRecipes.setState((s) => ({ os: { ...s.os, [key]: os } })), [key]);
  return { os: picked ?? guess ?? "debian", guessed: !picked && !!guess, set };
}

function OsSwitch({ os, guessed, onChange }: { os: Os; guessed: boolean; onChange: (os: Os) => void }) {
  return (
    <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
      <div role="radiogroup" aria-label="The server's system" className="flex h-7 items-center rounded-[8px] bg-ink/[0.05] p-[3px] ring-1 ring-inset ring-ink/[0.06]">
        {(Object.keys(OS_LABEL) as Os[]).map((o) => (
          <button
            key={o}
            role="radio"
            aria-checked={os === o}
            onClick={() => onChange(o)}
            className={cn(
              "h-[22px] rounded-[5px] px-2.5 text-[11.5px] font-medium transition-all",
              os === o ? "bg-raised text-fg shadow-[0_1px_2px_rgb(0_0_0/0.25)] ring-1 ring-ink/[0.06]" : "text-fg-3 hover:text-fg-2",
            )}
          >
            {OS_LABEL[o]}
          </button>
        ))}
      </div>
      {guessed && <span className="text-[11.5px] text-fg-3">Guessed from the server's kernel</span>}
    </div>
  );
}

function Tag({ icon: Icon, tone, children }: { icon?: LucideIcon; tone?: "ok"; children: ReactNode }) {
  return (
    <span className={cn("inline-flex items-center gap-1 rounded-[5px] px-1.5 py-px text-[10.5px] font-semibold", tone === "ok" ? "bg-ok/12 text-ok" : "bg-ink/[0.08] text-fg-2")}>
      {Icon && <Icon size={10.5} />}
      {children}
    </span>
  );
}

function doneIn(steps: Step[], done: string[]): number {
  return steps.filter((s) => done.includes(s.title)).length;
}

// ---------------------------------------------------------------- the list

function RecipeList({ target }: { target: TargetRef }) {
  const names = useNames();
  const { os, guessed, set } = useOs(target);
  const done = useRecipes((s) => s.done);
  const top = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    top.current?.closest('[role="dialog"]')?.scrollTo({ top: 0 });
  }, []);
  return (
    <div ref={top}>
      <Header icon={BookOpen} title="Recipes" sub={`${names.target(target)} · step by step, and you run every command`} onClose={close} />
      <div className="mt-4">
        <OsSwitch os={os} guessed={guessed} onChange={set} />
      </div>
      {(Object.keys(GROUP_LABEL) as Recipe["group"][]).map((g) => (
        <section key={g} className="mt-5">
          <h3 className="mb-2 text-[12px] font-semibold text-fg-2">{GROUP_LABEL[g]}</h3>
          <ul className="flex flex-col gap-1.5">
            {RECIPES.filter((r) => r.group === g).map((r) => {
              const Icon = ICONS[r.id] ?? BookOpen;
              const steps = stepsFor(r, os);
              const ticked = doneIn(steps, done[recipeKey(target, r.id)] ?? NO_STEPS);
              return (
                <li key={r.id}>
                  <button
                    onClick={() => show(r.id)}
                    className="flex w-full items-center gap-3 rounded-[11px] bg-ink/[0.035] px-3.5 py-2.5 text-left ring-1 ring-inset ring-ink/[0.06] transition-colors hover:bg-ink/[0.065]"
                  >
                    <span className="inline-flex size-8 shrink-0 items-center justify-center rounded-[9px] bg-accent/12 text-accent">
                      <Icon size={15} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block text-[13px] font-medium text-fg">{r.title}</span>
                      <span className="mt-0.5 block text-[12px] leading-snug text-fg-3">{r.summary}</span>
                    </span>
                    {ticked > 0 &&
                      (ticked === steps.length ? (
                        <Tag icon={Check} tone="ok">
                          Done
                        </Tag>
                      ) : (
                        <span className="shrink-0 text-[11.5px] text-fg-3 tabular-nums">
                          {ticked} of {steps.length}
                        </span>
                      ))}
                    <ChevronRight size={14} className="shrink-0 text-fg-3" />
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
      <p className="mt-4 text-[11.5px] leading-snug text-fg-3">
        Commands that change the system start with sudo, which runs them as root and may ask for your password. Nothing runs until you press Enter in the terminal.
      </p>
    </div>
  );
}

// ---------------------------------------------------------------- one recipe

function RecipeView({ target, recipe }: { target: TargetRef; recipe: Recipe }) {
  const names = useNames();
  const key = recipeKey(target, recipe.id);
  const host = useHost(target);
  const { os, guessed, set: setOs } = useOs(target);
  const typed = useRecipes((s) => s.typed[key]) ?? NONE;
  const doneList = useRecipes((s) => s.done[key]) ?? NO_STEPS;
  const serverTerminal = useStore((s) => s.mainTerminalFor(target));
  const openIn = useStore((s) => s.openIn);
  const steps = stepsFor(recipe, os);
  const base = useMemo(() => makeContext({ os, host, windows: isWindows, values: {} }), [os, host]);
  const values = useMemo(() => fieldValues(recipe, typed, base), [recipe, typed, base]);
  const ctx = useMemo(() => makeContext({ os, host, windows: isWindows, values }), [os, host, values]);
  const count = doneIn(steps, doneList);

  const setTyped = useCallback((patch: Record<string, string>) => useRecipes.setState((s) => ({ typed: { ...s.typed, [key]: { ...s.typed[key], ...patch } } })), [key]);
  const toggle = (title: string) =>
    useRecipes.setState((s) => {
      const list = s.done[key] ?? [];
      return { done: { ...s.done, [key]: list.includes(title) ? list.filter((t) => t !== title) : [...list, title] } };
    });
  const startOver = () => useRecipes.setState((s) => ({ done: { ...s.done, [key]: [] }, last: { ...s.last, [key]: "" } }));
  const stepAside = (title: string) => useRecipes.setState((s) => ({ aside: true, last: { ...s.last, [key]: title } }));

  // Back at the step you pasted last, or at the top.
  const top = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const last = useRecipes.getState().last[key];
    const at = last ? steps.findIndex((s) => s.title === last) : -1;
    const el = at >= 0 ? document.getElementById(`recipe-step-${at + 1}`) : null;
    if (el) el.scrollIntoView({ block: "center" });
    else top.current?.closest('[role="dialog"]')?.scrollTo({ top: 0 });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div ref={top}>
      <Header icon={ICONS[recipe.id] ?? BookOpen} title={recipe.title} sub={`${names.target(target)} · you run every command yourself`} onClose={close} />
      <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
        <button onClick={() => show(null)} className="-ml-1 inline-flex items-center gap-0.5 rounded-md px-1 py-0.5 text-[12px] text-fg-3 hover:bg-ink/[0.06] hover:text-fg">
          <ChevronLeft size={13} /> All recipes
        </button>
        <OsSwitch os={os} guessed={guessed} onChange={setOs} />
      </div>
      <p className="mt-3 text-[12.5px] leading-relaxed text-fg-2">{recipe.summary}</p>
      <p className="mt-0.5 text-[12px] leading-relaxed text-fg-3">For: {recipe.who}</p>

      {recipe.fields && recipe.fields.length > 0 && <Fields recipe={recipe} typed={typed} base={base} onChange={setTyped} />}

      {!serverTerminal && (
        <Note>
          With a terminal open on {names.target(target)}, each command can be pasted straight into it.{" "}
          <button onClick={() => openIn("terminal", target)} className="font-medium text-accent hover:underline">
            Open a terminal
          </button>
        </Note>
      )}

      <ol className="mt-4 flex flex-col gap-2">
        {steps.map((s, i) => (
          <StepCard
            key={s.title}
            n={i + 1}
            step={s}
            recipe={recipe}
            ctx={ctx}
            target={target}
            done={doneList.includes(s.title)}
            onToggle={() => toggle(s.title)}
            onPaste={() => stepAside(s.title)}
            hasKey={recipe.fields?.some((f) => f.kind === "key") ? !!typed._keyFound : false}
          />
        ))}
      </ol>

      <div className="mt-4 flex min-h-7 items-center justify-between gap-3 text-[12px] text-fg-3">
        <span className="tabular-nums">
          {count === steps.length ? "All done." : `${count} of ${steps.length} steps done. Tick a step off once it worked.`}
        </span>
        {count > 0 && (
          <Button size="sm" variant="ghost" onClick={startOver}>
            <RotateCcw size={12} /> Start over
          </Button>
        )}
      </div>
    </div>
  );
}

function StepCard({
  n,
  step,
  recipe,
  ctx,
  target,
  done,
  onToggle,
  onPaste,
  hasKey,
}: {
  n: number;
  step: Step;
  recipe: Recipe;
  ctx: RecipeContext;
  target: TargetRef;
  done: boolean;
  onToggle: () => void;
  onPaste: () => void;
  hasKey: boolean;
}) {
  const names = useNames();
  const [expanded, setExpanded] = useState(false);
  const local = step.where === "local";
  const localTerminal = useStore((s) => (local ? s.mainTerminalFor(LOCAL) : null));
  const openIn = useStore((s) => s.openIn);
  const collapsed = done && !expanded;
  const missing = missingFields(recipe, step, ctx.values);
  const command = missing.length ? null : resolve(step.command, ctx);
  const warn = resolve(step.warn, ctx);
  const expect = resolve(step.expect, ctx);
  return (
    <li
      id={`recipe-step-${n}`}
      className={cn("rounded-[11px] px-3.5 py-3 ring-1 ring-inset transition-colors", done ? "bg-ok/[0.05] ring-ok/20" : "bg-ink/[0.035] ring-ink/[0.06]")}
    >
      <div className="flex items-start gap-2.5">
        <button
          role="checkbox"
          aria-checked={done}
          aria-label={done ? `Step ${n} is done. Untick it` : `Tick off step ${n}`}
          title={done ? "Untick" : "Tick off once it worked"}
          onClick={() => {
            onToggle();
            setExpanded(false);
          }}
          className={cn(
            "mt-px inline-flex size-5 shrink-0 items-center justify-center rounded-full text-[10.5px] font-semibold tabular-nums ring-1 ring-inset transition-colors",
            done ? "bg-ok text-on-accent ring-ok" : "text-fg-2 ring-ink/25 hover:bg-ink/[0.07] hover:ring-ink/40",
          )}
        >
          {done ? <Check size={12} strokeWidth={3} /> : n}
        </button>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            {done ? (
              <button onClick={() => setExpanded((v) => !v)} aria-expanded={expanded} className="text-left text-[13px] font-medium text-fg-2 hover:text-fg">
                {step.title}
              </button>
            ) : (
              <p className="text-[13px] font-medium text-fg">{step.title}</p>
            )}
            {local && <Tag icon={Laptop}>On your computer</Tag>}
            {step.check && <Tag tone="ok">Check</Tag>}
          </div>
          {!collapsed && (
            <>
              {warn && (
                <p className="mt-2 flex items-start gap-1.5 rounded-[9px] bg-warn/10 px-2.5 py-2 text-[12px] leading-snug text-warn">
                  <TriangleAlert size={13} className="mt-px shrink-0" />
                  <span>{warn}</span>
                </p>
              )}
              <p className="selectable mt-1.5 text-[12px] leading-relaxed text-fg-2">{names.text(resolve(step.explain, ctx))}</p>
              {step.skipIfKey && hasKey && <p className="mt-1.5 text-[11.5px] leading-snug text-ok">SSHeesh found a key on this computer, so you can skip this step.</p>}
              {command ? (
                <Command command={command} display={names.text(command)} target={local ? LOCAL : target} inTerminal={onPaste} />
              ) : (
                <p className="mt-2.5 rounded-[9px] bg-ink/[0.04] px-3 py-2 text-[12px] text-fg-3">
                  Fill in {missing.map((f) => `"${f.label}"`).join(" and ")} above to see the command.
                </p>
              )}
              {local && command && !localTerminal && (
                <p className="mt-1.5 text-[11.5px] leading-snug text-fg-3">
                  Run it in a terminal on this computer.{" "}
                  <button onClick={() => openIn("terminal", LOCAL)} className="font-medium text-accent hover:underline">
                    Open one in SSHeesh
                  </button>
                </p>
              )}
              {expect && (
                <p className="selectable mt-2 text-[11.5px] leading-snug text-fg-3">
                  <span className="font-medium text-fg-2">You should see: </span>
                  {names.text(expect)}
                </p>
              )}
              {step.ifFails && (
                <details className="group mt-2">
                  <summary className="inline-flex cursor-pointer list-none items-center gap-0.5 text-[11.5px] font-medium text-fg-2 select-none hover:text-fg [&::-webkit-details-marker]:hidden">
                    <ChevronRight size={12} className="transition-transform group-open:rotate-90" /> If it didn't work
                  </summary>
                  <p className="selectable mt-1 text-[12px] leading-relaxed text-fg-2">{resolve(step.ifFails.explain, ctx)}</p>
                  {(() => {
                    const fix = resolve(step.ifFails.command, ctx);
                    return <Command command={fix} display={names.text(fix)} target={target} inTerminal={onPaste} />;
                  })()}
                </details>
              )}
            </>
          )}
        </div>
      </div>
    </li>
  );
}

// ---------------------------------------------------------------- fields

function Fields({ recipe, typed, base, onChange }: { recipe: Recipe; typed: Record<string, string>; base: RecipeContext; onChange: (patch: Record<string, string>) => void }) {
  return (
    <section className="mt-4 rounded-[11px] bg-ink/[0.035] px-3.5 py-3 ring-1 ring-inset ring-ink/[0.06]">
      <h3 className="text-[12px] font-semibold text-fg-2">Fill in first</h3>
      <div className="mt-2 flex flex-col gap-3">
        {recipe.fields?.map((f) =>
          f.kind === "key" ? (
            <KeyField key={f.id} field={f} typed={typed} onChange={onChange} />
          ) : (
            <FieldInput key={f.id} field={f} value={typed[f.id] ?? resolve(f.initial, base)} onChange={(v) => onChange({ [f.id]: v })} />
          ),
        )}
      </div>
    </section>
  );
}

function FieldInput({ field, value, onChange }: { field: Field; value: string; onChange: (v: string) => void }) {
  const error = fieldError(field, value);
  const hint = <span className={cn("mt-1 block text-[11.5px] leading-snug", error ? "text-bad" : "text-fg-3")}>{error ?? field.help}</span>;
  if (field.kind === "choice") {
    return (
      <div>
        <span className="text-[12px] font-medium text-fg">{field.label}</span>
        <div role="radiogroup" aria-label={field.label} className="mt-1 flex flex-wrap gap-1.5">
          {field.choices?.map((c) => (
            <button
              key={c.value}
              role="radio"
              aria-checked={value === c.value}
              onClick={() => onChange(c.value)}
              className={cn(
                "h-7 rounded-[8px] px-2.5 text-[12px] font-medium ring-1 ring-inset transition-colors",
                value === c.value ? "bg-accent/12 text-fg ring-accent/60" : "text-fg-2 ring-ink/[0.08] hover:bg-ink/[0.04]",
              )}
            >
              {c.label}
            </button>
          ))}
        </div>
        {hint}
      </div>
    );
  }
  return (
    <label className="block">
      <span className="text-[12px] font-medium text-fg">{field.label}</span>
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={field.placeholder}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        aria-invalid={!!error}
        className={cn(inputClass, "mt-1", error && "ring-bad/60 focus:ring-bad/70")}
      />
      {hint}
    </label>
  );
}

function KeyChoice({ checked, onSelect, title, mono, badge, detail }: { checked: boolean; onSelect: () => void; title: string; mono?: boolean; badge?: string; detail?: string }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      onClick={onSelect}
      className={cn("flex w-full items-center gap-2.5 rounded-[8px] px-2.5 py-1.5 text-left ring-1 ring-inset transition-colors", checked ? "bg-accent/12 ring-accent/60" : "ring-ink/[0.08] hover:bg-ink/[0.04]")}
    >
      <span className={cn("flex size-3.5 shrink-0 items-center justify-center rounded-full ring-1", checked ? "bg-accent ring-accent" : "ring-ink/25")}>
        {checked && <span className="size-1.5 rounded-full bg-white" />}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2 text-[12.5px] text-fg">
          <span className={cn(mono && "font-mono text-[12px]")}>{title}</span>
          {badge && <span className="rounded-[4px] bg-ink/[0.08] px-1.5 text-[10px] font-semibold text-fg-2">{badge}</span>}
        </span>
        {detail && <span className="block truncate text-[11.5px] text-fg-3">{detail}</span>}
      </span>
    </button>
  );
}

/**
 * Picks a key from ~/.ssh on this computer and reads its .pub file, or takes
 * a public key pasted in (from a password manager, say). Stores the key line
 * as `publicKey` and its path as `keyFile`; the `_` entries are this field's
 * own state, kept while the dialog steps aside.
 */
function KeyField({ field, typed, onChange }: { field: Field; typed: Record<string, string>; onChange: (patch: Record<string, string>) => void }) {
  const auth = useRun(useCallback(() => backend.localAuth(), []));
  const keys = auth.data?.keys ?? [];
  const managers = (auth.data?.agents ?? []).filter((a) => a.socket).map((a) => a.name);
  const choice = typed._keyChoice ?? "";
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A slow read mustn't undo a choice made since.
  const wanted = useRef(choice);

  const pick = useCallback(
    (path: string, name: string) => {
      wanted.current = path;
      onChange({ _keyChoice: path, publicKey: "", keyFile: "" });
      setReading(true);
      setError(null);
      backend
        .readFile(LOCAL, `${path}.pub`)
        .then((f) => {
          const line = f.content ? parsePublicKey(f.content, name) : null;
          if (!line) throw new Error("not a key");
          if (wanted.current === path) onChange({ _keyChoice: path, publicKey: line, keyFile: path });
        })
        .catch(() => wanted.current === path && setError(`Couldn't read a public key from ${path}.pub. Pick another key, or paste one.`))
        .finally(() => setReading(false));
    },
    [onChange],
  );

  // The usual key, picked for you the first time.
  useEffect(() => {
    if (!auth.data) return;
    onChange({ _keyFound: auth.data.keys.length ? "1" : "" });
    const first = auth.data.keys[0];
    if (!choice && first) pick(first.path, first.name);
  }, [auth.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const pasted = typed._keyPaste ?? "";
  const pasteError = choice === "paste" && pasted.trim() ? fieldError(field, pasted) : null;
  return (
    <div>
      <span className="text-[12px] font-medium text-fg">{field.label}</span>
      {auth.loading && !auth.data ? (
        <p className="mt-1 flex items-center gap-1.5 text-[11.5px] text-fg-3">
          <Spinner className="size-3" /> Looking for keys on this computer…
        </p>
      ) : (
        <div role="radiogroup" aria-label={field.label} className="mt-1 flex flex-col gap-1">
          {keys.map((k) => (
            <KeyChoice key={k.path} checked={choice === k.path} onSelect={() => pick(k.path, k.name)} title={k.name} mono badge={k.kind} detail={`${k.path}.pub`} />
          ))}
          <KeyChoice
            checked={choice === "paste"}
            onSelect={() => {
              wanted.current = "paste";
              setError(null);
              onChange({ _keyChoice: "paste", publicKey: parsePublicKey(pasted) ?? "", keyFile: "" });
            }}
            title="Paste a public key"
            detail={managers.length ? `From ${managers.join(" or ")}, or another computer` : "From a password manager, or another computer"}
          />
          {choice === "paste" && (
            <input
              autoFocus
              value={pasted}
              onChange={(e) => onChange({ _keyPaste: e.target.value, publicKey: parsePublicKey(e.target.value) ?? "" })}
              placeholder="ssh-ed25519 AAAA…"
              spellCheck={false}
              aria-label="Public key"
              aria-invalid={!!pasteError}
              className={cn(inputClass, pasteError && "ring-bad/60")}
            />
          )}
        </div>
      )}
      {auth.data && keys.length === 0 && (
        <p className="mt-1.5 text-[11.5px] leading-snug text-fg-3">
          No key in ~/.ssh on this computer yet. Make one with the first step, then{" "}
          <button onClick={auth.reload} className="font-medium text-accent hover:underline">
            look again
          </button>
          .
        </p>
      )}
      {reading && (
        <p className="mt-1.5 flex items-center gap-1.5 text-[11.5px] text-fg-3">
          <Spinner className="size-3" /> Reading the key…
        </p>
      )}
      {(error ?? pasteError) && <p className="mt-1.5 text-[11.5px] leading-snug text-bad">{error ?? pasteError}</p>}
      {!error && !pasteError && <p className="mt-1 text-[11.5px] leading-snug text-fg-3">{field.help} Only its public half is used.</p>}
    </div>
  );
}

// ---------------------------------------------------------------- stepped aside

/** Up in the top bar, clear of the terminal's prompt, while a step runs in it. */
function AsideBar({ target, recipe }: { target: TargetRef; recipe: Recipe }) {
  const names = useNames();
  const { os } = useOs(target);
  const done = useRecipes((s) => s.done[recipeKey(target, recipe.id)]) ?? NO_STEPS;
  const steps = stepsFor(recipe, os);
  const Icon = ICONS[recipe.id] ?? BookOpen;
  return (
    <div
      role="region"
      aria-label="Recipe"
      className="flex min-w-0 shrink animate-pop-in items-center gap-2 rounded-full bg-ink/[0.06] py-0.5 pr-0.5 pl-3 ring-1 ring-inset ring-ink/[0.08]"
    >
      <Icon size={13.5} className="shrink-0 text-accent" />
      <span className="min-w-0 max-w-[240px] truncate text-[12.5px] font-medium text-fg" title={`${recipe.title} · ${names.target(target)}`}>
        {recipe.title}
      </span>
      <span className="shrink-0 text-[12px] text-fg-3 tabular-nums">
        {doneIn(steps, done)} of {steps.length} done
      </span>
      <Button size="sm" onClick={() => useRecipes.setState({ aside: false })}>
        Back to the steps
      </Button>
      <IconButton label="Close the recipe" onClick={close} className="rounded-full">
        <X size={14} />
      </IconButton>
    </div>
  );
}
