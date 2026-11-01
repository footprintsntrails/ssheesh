// "Add a server". One address box that understands whatever you paste, a
// clear choice of how to sign in (keys and key managers found for you), how
// the server looks in the list, and a test that walks through every step
// before anything is saved.

import { Check, CircleAlert, Copy, Eye, EyeOff, KeyRound, LoaderCircle, Lock, ShieldQuestion, TriangleAlert, Wand2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { parseAddress } from "../lib/address";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { ENV_LABELS, ENV_ORDER, HOST_COLORS, HOST_ICONS, useLabels, type EnvLabel } from "../lib/labels";
import { useStore } from "../lib/store";
import { toOpError, type ConnectionTest, type LocalAuth, type NewHost } from "../lib/types";
import { LookIcon } from "./HostIcon";
import { LookGrid, OwnHostColor } from "./LookPicker";
import { Modal, useEscapeLayer } from "./Overlays";
import { Button } from "./ui";

const inputClass =
  "h-9 w-full min-w-0 rounded-[9px] bg-ink/[0.05] px-3 text-[13px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] placeholder:text-fg-3/70 focus:ring-2 focus:ring-accent/70";

type Auth = "key" | "password" | "manager";

function Section({ title, children, aside }: { title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between">
        <h3 className="text-[12px] font-semibold text-fg-2">{title}</h3>
        {aside}
      </div>
      {children}
    </section>
  );
}

function Choice({ checked, onSelect, title, detail, badge }: { checked: boolean; onSelect: () => void; title: ReactNode; detail?: ReactNode; badge?: string }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      onClick={onSelect}
      className={cn(
        "flex w-full items-center gap-3 rounded-[9px] px-3 py-2 text-left ring-1 ring-inset transition-colors",
        checked ? "bg-accent/12 ring-accent/60" : "ring-ink/[0.08] hover:bg-ink/[0.04]",
      )}
    >
      <span className={cn("flex size-4 shrink-0 items-center justify-center rounded-full ring-1", checked ? "bg-accent ring-accent" : "ring-ink/25")}>
        {checked && <span className="size-1.5 rounded-full bg-white" />}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2 text-[12.5px] text-fg">
          {title}
          {badge && <span className="rounded-[4px] bg-ink/[0.08] px-1.5 text-[10px] font-semibold text-fg-2">{badge}</span>}
        </span>
        {detail && <span className="block truncate text-[11.5px] text-fg-3">{detail}</span>}
      </span>
    </button>
  );
}

type StepState = "ok" | "bad" | "warn" | "ask" | "wait";

function Step({ state, title, children }: { state: StepState; title: string; children?: ReactNode }) {
  const icon = {
    ok: <Check size={13} strokeWidth={2.6} className="text-ok" />,
    bad: <CircleAlert size={13} className="text-bad" />,
    warn: <TriangleAlert size={13} className="text-warn" />,
    ask: <ShieldQuestion size={13} className="text-accent" />,
    wait: <LoaderCircle size={13} className="animate-spin text-fg-3" />,
  }[state];
  return (
    <li className="flex gap-2.5">
      <span className="mt-[3px] flex size-4 shrink-0 items-center justify-center">{icon}</span>
      <div className="min-w-0 flex-1">
        <div className={cn("text-[12.5px]", state === "wait" ? "text-fg-3" : "text-fg")}>{title}</div>
        {children && <div className="mt-0.5 text-[11.5px] leading-snug text-fg-3">{children}</div>}
      </div>
    </li>
  );
}

function TestResult({ test, busy, onTrust }: { test: ConnectionTest | null; busy: boolean; onTrust: () => void }) {
  const toast = useStore((s) => s.toast);
  // It shows up at the foot of a long form: brought into sight, not left below the fold.
  const result = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (test) result.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [test]);
  if (busy && !test)
    return (
      <ul className="flex flex-col gap-2">
        <Step state="wait" title="Reaching the server…" />
      </ul>
    );
  if (!test) return null;
  const hk = test.hostKey;
  const install = "sudo apt install tmux";
  return (
    <ul ref={result} className="flex flex-col gap-2">
      <Step state={hk.kind === "unreachable" ? "bad" : "ok"} title={hk.kind === "unreachable" ? "Couldn't reach the server" : "The server answered"}>
        {hk.kind === "unreachable" && (hk.message || "Check the address and port, and that the server is on.")}
      </Step>
      {hk.kind === "unknown" && (
        <Step state="ask" title="First time here. Is this really your server?">
          Compare this fingerprint with the one in your hosting provider's console.
          {hk.fingerprints.map((f) => (
            <div key={f.sha256} className="mt-1.5 rounded-[7px] bg-ink/[0.05] px-2 py-1.5 font-mono text-[11px] break-all text-fg select-text">
              <span className="text-fg-3">{f.kind}</span> {f.sha256}
            </div>
          ))}
          <Button type="button" size="sm" variant="primary" className="mt-2" onClick={onTrust} disabled={busy}>
            Trust it and continue
          </Button>
        </Step>
      )}
      {hk.kind === "changed" && (
        <Step state="bad" title="This server's identity changed since you last connected">
          If you didn't reinstall it, don't add it. It may not be your server.
        </Step>
      )}
      {hk.kind === "known" && (
        <Step state={test.signedIn ? "ok" : "bad"} title={test.signedIn ? "Signed in" : "Couldn't sign in"}>
          {!test.signedIn && test.message}
        </Step>
      )}
      {test.signedIn && test.tmux !== null && (
        <Step
          state={test.tmux ? "ok" : "warn"}
          title={test.tmux ? "tmux is installed: sessions survive dropped connections" : "tmux isn't installed"}
        >
          {!test.tmux && (
            <>
              Terminals work, but a dropped connection ends them. Install it on the server with{" "}
              <button
                type="button"
                className="inline-flex items-center gap-1 rounded bg-ink/[0.07] px-1.5 font-mono text-[11px] text-fg hover:bg-ink/[0.12]"
                onClick={() => void navigator.clipboard.writeText(install).then(() => toast({ tone: "info", title: "Copied", body: install }))}
              >
                {install} <Copy size={10} />
              </button>
            </>
          )}
        </Step>
      )}
    </ul>
  );
}

export function AddHostDialog() {
  const open = useStore((s) => s.addHostOpen);
  const setOpen = useStore((s) => s.setAddHostOpen);
  const loadHosts = useStore((s) => s.loadHosts);
  const openIn = useStore((s) => s.openIn);
  const toast = useStore((s) => s.toast);

  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [user, setUser] = useState("");
  const [auth, setAuth] = useState<Auth>("key");
  const [keyPath, setKeyPath] = useState<string>(""); // "" = let ssh pick the default keys
  const [otherKey, setOtherKey] = useState(false);
  const [agent, setAgent] = useState<string>("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [remember, setRemember] = useState(true);
  const [name, setName] = useState("");
  const [icon, setIcon] = useState("server");
  const [color, setColor] = useState(HOST_COLORS[0]);
  // A picture of your own, already made small (hostImageFrom). Never the file.
  const [image, setImage] = useState<string | null>(null);
  const [env, setEnv] = useState<EnvLabel | null>(null);
  const [iconsOpen, setIconsOpen] = useState(false);
  // Esc closes the icon list first, not the whole form with what you typed.
  const closeIcons = useCallback(() => setIconsOpen(false), []);
  useEscapeLayer(iconsOpen, closeIcons);
  // Untouched, the server gets the icon it's detected to need and a color of its own.
  const [lookPicked, setLookPicked] = useState(false);
  const [local, setLocal] = useState<LocalAuth | null>(null);
  const [test, setTest] = useState<ConnectionTest | null>(null);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [filled, setFilled] = useState<string | null>(null);
  const hostRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setHost("");
    setPort("");
    setUser("");
    setAuth("key");
    setKeyPath("");
    setOtherKey(false);
    setAgent("");
    setPassword("");
    setShowPassword(false);
    setRemember(true);
    setName("");
    setIcon("server");
    setColor(HOST_COLORS[0]);
    setImage(null);
    setEnv(null);
    setIconsOpen(false);
    setLookPicked(false);
    setTest(null);
    setError(null);
    setFilled(null);
    backend
      .localAuth()
      .then(setLocal)
      .catch(() => setLocal({ keys: [], agents: [] }));
  }, [open]);

  const managers = useMemo(() => (local?.agents ?? []).filter((a) => a.socket), [local]);
  const sshAgent = local?.agents.find((a) => !a.socket);

  /**
   * Pasting "root@1.2.3.4:2222" or a whole ssh command fills every field.
   * Typed text is left alone until you leave the field, so it never jumps
   * around under your cursor.
   */
  const fillFrom = (value: string) => {
    if (!/[@\s]|^ssh:\/\/|^\[|:\d+$/.test(value.trim())) return false;
    const p = parseAddress(value);
    setHost(p.host);
    if (p.user) setUser(p.user);
    if (p.port) setPort(p.port);
    if (p.identityFile) {
      setAuth("key");
      setKeyPath(p.identityFile);
      setOtherKey(!local?.keys.some((k) => k.path === p.identityFile));
    }
    const got = [p.user && "user", p.port && "port", p.identityFile && "key"].filter(Boolean);
    setFilled(got.length ? `Filled in the ${got.join(", ")} from what you pasted` : null);
    return true;
  };

  const input = (): NewHost => {
    // "1.2.3.4:2222" typed and sent with Enter (no blur to split it): host and port.
    const typed = /^([^:[\]\s]+):(\d{1,5})$/.exec(host.trim());
    const h = typed ? typed[1] : host.trim();
    const p = port.trim() || (typed ? typed[2] : "");
    const address = `${user.trim() ? `${user.trim()}@` : ""}${h.includes(":") ? `[${h}]` : h}${p ? `:${p}` : ""}`;
    return {
      address,
      name: name.trim() || undefined,
      identityFile: auth === "key" && keyPath.trim() ? keyPath.trim() : undefined,
      identityAgent: auth === "manager" && agent ? agent : undefined,
      password: auth === "password" && password ? password : undefined,
      rememberPassword: remember,
    };
  };

  const runTest = async () => {
    setTesting(true);
    setTest(null);
    setError(null);
    try {
      setTest(await backend.testConnection(input()));
    } catch (e) {
      setError(toOpError(e).message);
    } finally {
      setTesting(false);
    }
  };

  const trustAndRetest = async () => {
    setTesting(true);
    try {
      await backend.trustTestHostKey(input());
      setTest(await backend.testConnection(input()));
    } catch (e) {
      setError(toOpError(e).message);
    } finally {
      setTesting(false);
    }
  };

  // Changing how to connect makes the last test stale.
  useEffect(() => setTest(null), [host, port, user, auth, keyPath, agent, password]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!host.trim()) return;
    const p = Number(port.trim());
    if (port.trim() && (!Number.isInteger(p) || p < 1 || p > 65535)) {
      setError("The port is a number from 1 to 65535.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const { host: added, warning } = await backend.addHost(input());
      const labels = useLabels.getState();
      if (lookPicked) labels.setLook(added.id, { icon, color, ...(image && { image }) });
      if (env) labels.setEnv(added.id, env);
      setPassword("");
      await loadHosts();
      setOpen(false);
      toast(warning ? { tone: "info", title: `Added ${added.name}`, body: warning } : { tone: "success", title: `Added ${added.name}` });
      openIn("terminal", { kind: "host", hostId: added.id });
    } catch (err) {
      setError(toOpError(err).message);
    } finally {
      setSaving(false);
    }
  };

  const Icon = HOST_ICONS[icon]?.icon ?? HOST_ICONS.server.icon;

  return (
    <Modal open={open} onClose={() => setOpen(false)} width={540}>
      <form onSubmit={submit} className="flex max-h-[calc(84vh-24px)] flex-col">
        <div className="px-5 pt-5">
          <h2 className="text-[15px] font-semibold text-fg">Add a server</h2>
          <p className="mt-1 text-[12.5px] text-fg-3">Servers in ~/.ssh/config are already in your list.</p>
        </div>

        <div className="mt-4 flex min-h-0 flex-col gap-5 overflow-y-auto px-5 pb-4 [mask-image:linear-gradient(to_bottom,black_calc(100%_-_20px),transparent)]">
          <Section title="Where">
            <div className="flex gap-2">
              <label className="min-w-0 flex-1">
                <span className="sr-only">Address</span>
                <input
                  ref={hostRef}
                  autoFocus
                  required
                  value={host}
                  onChange={(e) => {
                    setHost(e.target.value);
                    setFilled(null);
                  }}
                  onPaste={(e) => {
                    if (fillFrom(e.clipboardData.getData("text"))) e.preventDefault();
                  }}
                  onBlur={(e) => fillFrom(e.target.value)}
                  placeholder="203.0.113.10, or paste: ssh root@host -p 2222"
                  spellCheck={false}
                  autoCapitalize="off"
                  autoCorrect="off"
                  className={cn(inputClass, "font-mono text-[12.5px]")}
                />
              </label>
              <label className="w-[84px] shrink-0">
                <span className="sr-only">Port</span>
                <input
                  value={port}
                  onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))}
                  placeholder="22"
                  inputMode="numeric"
                  aria-label="Port"
                  className={cn(inputClass, "font-mono text-[12.5px]")}
                />
              </label>
            </div>
            <label className="flex items-center gap-2">
              <span className="w-[72px] shrink-0 text-[12px] text-fg-3">User</span>
              <input
                value={user}
                onChange={(e) => setUser(e.target.value)}
                placeholder="root"
                spellCheck={false}
                autoCapitalize="off"
                className={cn(inputClass, "font-mono text-[12.5px]")}
              />
            </label>
            {filled && (
              <p className="flex items-center gap-1.5 text-[11.5px] text-accent">
                <Wand2 size={12} /> {filled}
              </p>
            )}
          </Section>

          <Section title="Sign in with">
            <div role="tablist" className="flex h-8 items-center rounded-[9px] bg-ink/[0.05] p-[3px] ring-1 ring-inset ring-ink/[0.06]">
              {(
                [
                  ["key", "SSH key", KeyRound],
                  ["password", "Password", Lock],
                  ...(managers.length ? [["manager", "Password manager", ShieldQuestion] as const] : []),
                ] as const
              ).map(([id, label, I]) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={auth === id}
                  onClick={() => setAuth(id)}
                  className={cn(
                    "flex h-[26px] flex-1 items-center justify-center gap-1.5 rounded-[6px] text-[12px] font-medium transition-all",
                    auth === id ? "bg-raised text-fg shadow-[0_1px_2px_rgb(0_0_0/0.25)] ring-1 ring-ink/[0.06]" : "text-fg-3 hover:text-fg-2",
                  )}
                >
                  <I size={12.5} /> {label}
                </button>
              ))}
            </div>

            {auth === "key" && (
              <div role="radiogroup" aria-label="Key" className="flex flex-col gap-1.5">
                <Choice
                  checked={!otherKey && keyPath === ""}
                  onSelect={() => {
                    setOtherKey(false);
                    setKeyPath("");
                  }}
                  title="Your usual keys"
                  detail={
                    sshAgent?.keys
                      ? `ssh picks one, including ${sshAgent.keys} loaded in ssh-agent`
                      : "ssh picks from the keys in ~/.ssh"
                  }
                />
                {local?.keys.map((k) => (
                  <Choice
                    key={k.path}
                    checked={!otherKey && keyPath === k.path}
                    onSelect={() => {
                      setOtherKey(false);
                      setKeyPath(k.path);
                    }}
                    title={<span className="font-mono text-[12px]">{k.name}</span>}
                    badge={k.kind}
                    detail={k.comment || k.path}
                  />
                ))}
                <Choice
                  checked={otherKey}
                  onSelect={() => {
                    setOtherKey(true);
                    setKeyPath("");
                  }}
                  title="Another key file"
                />
                {otherKey && (
                  <input
                    autoFocus
                    value={keyPath}
                    onChange={(e) => setKeyPath(e.target.value)}
                    placeholder="~/.ssh/my_key"
                    spellCheck={false}
                    aria-label="Key file"
                    className={cn(inputClass, "font-mono text-[12.5px]")}
                  />
                )}
              </div>
            )}

            {auth === "password" && (
              <div className="flex flex-col gap-2">
                <div className="relative">
                  <input
                    type={showPassword ? "text" : "password"}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                    aria-label="Password"
                    placeholder="Password"
                    className={cn(inputClass, "pr-9")}
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword((v) => !v)}
                    aria-label={showPassword ? "Hide password" : "Show password"}
                    className="absolute inset-y-0 right-1 inline-flex w-7 items-center justify-center text-fg-3 hover:text-fg"
                  >
                    {showPassword ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                </div>
                <label className="flex items-start gap-2 text-[12.5px]">
                  <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} className="mt-[3px] accent-[var(--ui-accent)]" />
                  <span>
                    <span className="text-fg">Remember password</span>
                    <span className="block text-[11.5px] leading-snug text-fg-3">
                      {remember ? "Saved in your system keychain, never in SSHeesh's own files." : "Kept only until you quit SSHeesh."}
                    </span>
                  </span>
                </label>
              </div>
            )}

            {auth === "manager" && (
              <div role="radiogroup" aria-label="Password manager" className="flex flex-col gap-1.5">
                {managers.map((a) => (
                  <Choice
                    key={a.socket}
                    checked={agent === a.socket}
                    onSelect={() => setAgent(a.socket ?? "")}
                    title={a.name}
                    detail={a.keys === null ? "Unlock it to use its keys" : `${a.keys} key${a.keys === 1 ? "" : "s"} · you approve each login there`}
                  />
                ))}
              </div>
            )}
          </Section>

          <Section title="In your list">
            <div className="flex items-center gap-2">
              <button
                type="button"
                aria-label={`Icon: ${image ? "your picture" : HOST_ICONS[icon]?.name}. Change`}
                aria-expanded={iconsOpen}
                onClick={() => setIconsOpen((v) => !v)}
                className={cn(
                  "flex size-9 shrink-0 items-center justify-center rounded-[9px] ring-1 ring-inset",
                  iconsOpen ? "bg-ink/[0.08] ring-accent/60" : "bg-ink/[0.05] ring-ink/[0.08] hover:bg-ink/[0.08]",
                )}
              >
                <LookIcon look={{ icon: Icon, color, image }} size={17} />
              </button>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={host.trim() || "Name"}
                aria-label="Name"
                spellCheck={false}
                className={inputClass}
              />
            </div>
            {/* Icons open in place, under the name, instead of floating over the colors. */}
            {iconsOpen && (
              <div
                ref={(el) => el?.scrollIntoView({ block: "nearest", behavior: "smooth" })}
                className="animate-fade-in rounded-[10px] bg-ink/[0.035] p-1.5 ring-1 ring-inset ring-ink/[0.06]"
              >
                <LookGrid
                  look={{ icon, color, image: image ?? undefined }}
                  colors={false}
                  onChange={(l) => {
                    setIcon(l.icon);
                    setImage(l.image ?? null);
                    setLookPicked(true);
                    setIconsOpen(false);
                  }}
                />
              </div>
            )}
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex gap-1.5" role="radiogroup" aria-label="Icon color">
                {HOST_COLORS.map((c) => (
                  <button
                    key={c}
                    type="button"
                    role="radio"
                    aria-checked={!image && color === c}
                    aria-label={`Color ${c}`}
                    onClick={() => {
                      // A picture has no color: picking one goes back to the icon.
                      setColor(c);
                      setImage(null);
                      setLookPicked(true);
                    }}
                    className={cn("size-[18px] rounded-full", !image && color === c ? "ring-2 ring-fg/80 ring-offset-2 ring-offset-[var(--ui-raised)]" : "")}
                    style={{ background: c }}
                  />
                ))}
                <OwnHostColor
                  color={color}
                  picked={!image}
                  size="size-[18px]"
                  onChange={(c) => {
                    setColor(c);
                    setImage(null);
                    setLookPicked(true);
                  }}
                />
              </div>
              <div className="flex gap-1" role="radiogroup" aria-label="Label">
                {[null, ...ENV_ORDER].map((e) => (
                  <button
                    key={e ?? "none"}
                    type="button"
                    role="radio"
                    aria-checked={env === e}
                    onClick={() => setEnv(e)}
                    className={cn(
                      "h-6 rounded-[6px] px-2 text-[11px] font-semibold ring-1 ring-inset transition-colors",
                      env === e
                        ? e
                          ? cn(ENV_LABELS[e].tint, ENV_LABELS[e].text, "ring-transparent")
                          : "bg-ink/[0.1] text-fg ring-transparent"
                        : "text-fg-3 ring-ink/[0.08] hover:text-fg-2",
                    )}
                  >
                    {e ? ENV_LABELS[e].short : "No label"}
                  </button>
                ))}
              </div>
            </div>
          </Section>

          {(test || testing) && (
            <div className="rounded-[11px] bg-ink/[0.03] p-3 ring-1 ring-inset ring-ink/[0.07]">
              <TestResult test={test} busy={testing} onTrust={() => void trustAndRetest()} />
            </div>
          )}
          {error && (
            <p className="flex items-start gap-1.5 text-[12px] text-bad">
              <CircleAlert size={13} className="mt-px shrink-0" /> {error}
            </p>
          )}
        </div>

        <div className="mt-4 flex items-center gap-2 border-t border-line px-5 py-3.5">
          <Button type="button" variant="ghost" disabled={!host.trim() || testing} onClick={() => void runTest()}>
            {testing ? <LoaderCircle size={13} className="animate-spin" /> : <Check size={13} />} Test connection
          </Button>
          <span className="flex-1" />
          <Button type="button" variant="ghost" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={saving || !host.trim()}>
            Add and connect
          </Button>
        </div>
      </form>
    </Modal>
  );
}
