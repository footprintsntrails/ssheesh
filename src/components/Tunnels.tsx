// Tunnels: the server's web apps in your browser with one click, and saved
// tunnels that keep one of the server's ports here, on this computer.
// Mount <Tunnels /> once; openTunnels(target) shows the dialog.

import { ArrowUpRight, Cable, ChevronRight, Globe, Pencil, Plug, Plus, RotateCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type KeyboardEvent, type ReactNode } from "react";
import { create } from "zustand";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";
import { useNames } from "../lib/privacy";
import { useStore } from "../lib/store";
import {
  appUrl,
  closeTemp,
  describeForward,
  hostError,
  isActive,
  keepTemp,
  openWebApp,
  portError,
  removeSaved,
  saveTunnel,
  setSavedOn,
  startTunnel,
  SERVER,
  tunnelTitle,
  tunnelTo,
  useSavedTunnels,
  useTunnelRuntime,
  type SavedTunnel,
  type TempTunnel,
} from "../lib/tunnels";
import { targetKey, toOpError, type Forward, type TargetRef, type TunnelState, type WebApp, type WebAppScan } from "../lib/types";
import { Modal } from "./Overlays";
import { Header, Note, Section, useRun, Waiting } from "./ServerTools";
import { Segmented, Switch } from "./controls";
import { Button, IconButton, Spinner } from "./ui";

export { useActiveTunnels } from "../lib/tunnels";

const useDialog = create<{ target: TargetRef | null }>(() => ({ target: null }));

export function openTunnels(target: TargetRef) {
  useDialog.setState({ target });
}

const close = () => useDialog.setState({ target: null });

const EMPTY_SAVED: SavedTunnel[] = [];
const EMPTY_TEMP: TempTunnel[] = [];

/** Always mounted: shows the dialog when asked. The main window starts the tunnels left on (App). */
export function Tunnels() {
  const target = useDialog((s) => s.target);
  if (!target) return null;
  return <TunnelsDialog key={targetKey(target)} target={target} />;
}

function TunnelsDialog({ target }: { target: TargetRef }) {
  const names = useNames();
  const scan = useRun(useCallback(() => backend.scanWebApps(target), [target]));
  const hostId = target.kind === "host" ? target.hostId : null;
  return (
    <Modal open onClose={close} width={620}>
      <div className="p-5">
        <Header
          icon={Cable}
          title="Tunnels"
          sub={`${names.target(target)} · the server's apps, here on this computer`}
          loading={scan.loading}
          onReload={hostId ? scan.reload : undefined}
          onClose={close}
        />
        {hostId ? (
          <>
            <p className="mt-3 text-[12px] leading-relaxed text-fg-3">
              A tunnel makes one of the server's ports work on this computer as <span className="font-mono text-fg-2">localhost</span>, through your SSH login.
              Nothing gets opened to the internet.
            </p>
            <WebApps hostId={hostId} scan={scan} />
            <SavedTunnels hostId={hostId} />
          </>
        ) : (
          <Note>Tunnels lead to a server. Open this from one of your servers.</Note>
        )}
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- web apps on this server

function WebApps({ hostId, scan }: { hostId: string; scan: { data?: WebAppScan; error?: { message: string }; loading: boolean } }) {
  const [showOther, setShowOther] = useState(false);
  const temp = useTunnelRuntime((rt) => rt.temp[hostId]) ?? EMPTY_TEMP;
  const apps = scan.data?.apps ?? [];
  const web = apps.filter((a) => a.web);
  const other = apps.filter((a) => !a.web);
  // Opened earlier, but gone from the latest look (the app stopped, say).
  const orphans = scan.data ? temp.filter((t) => !apps.some((a) => a.port === t.forward.remotePort && a.remoteHost === t.forward.remoteHost)) : [];
  return (
    <Section title="Web apps on this server" aside={<span className="text-[11.5px] text-fg-3">Open one in your browser</span>}>
      {scan.loading && !scan.data && <Waiting>Looking at what runs on the server…</Waiting>}
      {scan.error && <Note tone="error">{scan.error.message}</Note>}
      {scan.data && (
        <div className={cn("transition-opacity", scan.loading && "opacity-50")}>
          {!scan.data.curl && web.length > 0 && <Note>Install curl on the server to see each page's title here.</Note>}
          {web.length === 0 && orphans.length === 0 && (
            <p className="py-1 text-[12px] leading-relaxed text-fg-3">
              Nothing here answers like a website.{other.length > 0 && " Other programs that listen are below."}
            </p>
          )}
          <ul className={cn("flex flex-col gap-1.5", !scan.data.curl && web.length > 0 && "mt-2")}>
            {web.map((a) => (
              <AppRow key={`${a.remoteHost}:${a.port}`} hostId={hostId} app={a} />
            ))}
            {orphans.map((t) => (
              <AppRow key={t.id} hostId={hostId} app={orphanApp(t)} />
            ))}
          </ul>
          {other.length > 0 && (
            <>
              <button
                onClick={() => setShowOther((v) => !v)}
                aria-expanded={showOther}
                className="mt-2.5 inline-flex items-center gap-1 text-[12px] font-medium text-fg-3 hover:text-fg"
              >
                <ChevronRight size={13} className={cn("transition-transform", showOther && "rotate-90")} />
                Other ports ({other.length})
              </button>
              {showOther && (
                <>
                  <p className="mt-1 mb-2 text-[11.5px] leading-snug text-fg-3">
                    Not websites. Connect brings one here, so an app on this computer (a database app, say) can use it at localhost.
                  </p>
                  <ul className="flex flex-col gap-1.5">
                    {other.map((a) => (
                      <AppRow key={`${a.remoteHost}:${a.port}`} hostId={hostId} app={a} />
                    ))}
                  </ul>
                </>
              )}
            </>
          )}
        </div>
      )}
    </Section>
  );
}

function orphanApp(t: TempTunnel): WebApp {
  return {
    port: t.forward.remotePort,
    remoteHost: t.forward.remoteHost,
    process: null,
    knownAs: null,
    title: t.label,
    scheme: t.scheme,
    status: null,
    web: true,
    public: false,
  };
}

/** Re-renders when any tunnel changes, and finds the one leading to `app`. */
function useTunnelTo(hostId: string, app: WebApp) {
  useSavedTunnels((s) => s.byHost[hostId]);
  useTunnelRuntime((rt) => rt.temp[hostId]);
  const status = useTunnelRuntime((rt) => rt.status);
  const tunnel = tunnelTo(hostId, app);
  return { tunnel, state: tunnel ? status[tunnel.id] : undefined };
}

function AppRow({ hostId, app }: { hostId: string; app: WebApp }) {
  const names = useNames();
  const { tunnel, state } = useTunnelTo(hostId, app);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = isActive(state) && !!tunnel;

  const open = async (browse: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await openWebApp(hostId, app, browse);
    } catch (e) {
      const err = toOpError(e);
      if (err.code !== "cancelled") setError(err.message);
    } finally {
      setBusy(false);
    }
  };
  const stop = () => {
    if (!tunnel) return;
    if (!tunnel.saved) return closeTemp(hostId, tunnel.id);
    const saved = useSavedTunnels.getState().byHost[hostId]?.find((t) => t.id === tunnel.id);
    if (saved) setSavedOn(hostId, saved, false);
  };

  const title = app.title ?? app.knownAs ?? app.process ?? `Port ${app.port}`;
  const facts = [
    `port ${app.port}`,
    app.title ? (app.knownAs ?? app.process) : app.knownAs && app.process,
    app.remoteHost !== SERVER && `at ${app.remoteHost}`,
    app.status && app.status >= 400 && `answers ${app.status}`,
    app.public && "also open to other computers",
  ].filter(Boolean);
  const Icon = app.web ? Globe : Plug;

  return (
    <li className="rounded-[11px] bg-ink/[0.035] px-3.5 py-2.5 ring-1 ring-inset ring-ink/[0.06]">
      <div className="flex items-center gap-3">
        <Icon size={15} className={cn("shrink-0", app.web ? "text-accent" : "text-fg-3")} />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium text-fg" title={names.text(title)}>
            {names.text(title)}
          </p>
          <p className="truncate text-[11.5px] text-fg-3">{names.text(facts.join(" · "))}</p>
        </div>
        {active ? (
          <div className="flex shrink-0 items-center gap-1">
            <Badge state={state} port={tunnel.forward.localPort} />
            {app.web && state?.kind === "on" && (
              <Button size="sm" variant="ghost" onClick={() => void open(true)} title={`Open ${appUrl(app, tunnel.forward.localPort)}`}>
                <ArrowUpRight size={13} /> Open
              </Button>
            )}
            {!tunnel.saved && (
              <Button size="sm" variant="ghost" onClick={() => keepTemp(hostId, tunnel.id)} title="Save it, and start it again whenever SSHeesh opens">
                Keep
              </Button>
            )}
            <Button size="sm" variant="ghost" onClick={stop}>
              Stop
            </Button>
          </div>
        ) : (
          <Button
            size="sm"
            variant={app.web ? "primary" : "secondary"}
            disabled={busy}
            onClick={() => void open(app.web)}
            title={
              app.web
                ? `Opens the server's port ${app.port} in your browser`
                : `Makes the server's port ${app.port} work on this computer, at localhost`
            }
          >
            {busy ? <Spinner className="size-3" /> : app.web ? <ArrowUpRight size={13} /> : <Cable size={13} />}
            {app.web ? "Open" : "Connect"}
          </Button>
        )}
      </div>
      {error && <p className="mt-1.5 pl-[27px] text-[11.5px] leading-snug text-bad">{error}</p>}
      {!error && !busy && state?.kind === "failed" && <p className="mt-1.5 pl-[27px] text-[11.5px] leading-snug text-bad">{state.message}</p>}
      {active && state?.kind === "reconnecting" && (
        <p className="mt-1.5 pl-[27px] text-[11.5px] leading-snug text-warn">
          <Retry state={state} />
        </p>
      )}
      {active && state?.kind === "on" && state.warning && <p className="mt-1.5 pl-[27px] text-[11.5px] leading-snug text-warn">{state.warning}</p>}
      {active && !app.web && state?.kind === "on" && !state.warning && (
        <p className="mt-1.5 pl-[27px] text-[11.5px] leading-snug text-fg-3">
          Point your app at <span className="font-mono text-fg-2">localhost:{tunnel.forward.localPort}</span> on this computer.
        </p>
      )}
    </li>
  );
}

/** "localhost:3000" once it's on (click to copy), or Starting…/Reconnecting…. */
function Badge({ state, port }: { state: TunnelState | undefined; port: number }) {
  const toast = useStore((s) => s.toast);
  if (state?.kind === "on") {
    const address = `localhost:${port}`;
    return (
      <button
        onClick={() => void backend.writeClipboard(address).then(() => toast({ tone: "info", title: "Copied", body: address }))}
        title="Copy the address"
        className="mr-1 inline-flex items-center gap-1.5 rounded-md px-1.5 py-0.5 font-mono text-[11.5px] text-fg-2 hover:bg-ink/[0.06] hover:text-fg"
      >
        <span className="size-1.5 rounded-full bg-ok" /> {address}
      </button>
    );
  }
  const reconnecting = state?.kind === "reconnecting";
  return (
    <span className={cn("mr-1 inline-flex items-center gap-1.5 text-[11.5px]", reconnecting ? "text-warn" : "text-fg-3")}>
      <Spinner className="size-3" /> {reconnecting ? "Reconnecting…" : "Starting…"}
    </span>
  );
}

// ---------------------------------------------------------------- saved tunnels

function SavedTunnels({ hostId }: { hostId: string }) {
  const list = useSavedTunnels((s) => s.byHost[hostId]) ?? EMPTY_SAVED;
  const [editing, setEditing] = useState<string | null>(null);
  return (
    <Section
      title="Saved tunnels"
      aside={
        editing !== "new" && (
          <button onClick={() => setEditing("new")} className="inline-flex items-center gap-1 text-[11.5px] text-fg-3 hover:text-fg">
            <Plus size={12} /> Add a tunnel
          </button>
        )
      }
    >
      {editing === "new" && <TunnelForm hostId={hostId} onDone={() => setEditing(null)} />}
      {list.length === 0 && editing !== "new" && (
        <p className="py-1 text-[12px] leading-relaxed text-fg-3">
          Nothing saved yet. Keep a web app you opened above, or add a tunnel. Saved tunnels that are on start again whenever SSHeesh opens.
        </p>
      )}
      {list.length > 0 && (
        <ul className={cn("flex flex-col gap-1.5", editing === "new" && "mt-2")}>
          {list.map((t) =>
            editing === t.id ? (
              <li key={t.id}>
                <TunnelForm hostId={hostId} tunnel={t} onDone={() => setEditing(null)} />
              </li>
            ) : (
              <SavedRow key={t.id} hostId={hostId} tunnel={t} onEdit={() => setEditing(t.id)} />
            ),
          )}
        </ul>
      )}
    </Section>
  );
}

function SavedRow({ hostId, tunnel: t, onEdit }: { hostId: string; tunnel: SavedTunnel; onEdit: () => void }) {
  const names = useNames();
  const state = useTunnelRuntime((rt) => rt.status[t.id]);
  const title = tunnelTitle(t);
  const port = t.forward.localPort;
  return (
    <li className="flex items-start gap-3 rounded-[11px] bg-ink/[0.035] px-3.5 py-2.5 ring-1 ring-inset ring-ink/[0.06]">
      <div className="pt-px">
        <Switch checked={t.on} onChange={(on) => setSavedOn(hostId, t, on)} label={`Turn ${title} on or off`} />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] font-medium text-fg">{names.text(title)}</p>
        <p className="text-[11.5px] leading-snug text-fg-3">{names.text(describeForward(t.forward))}</p>
        <p className="mt-1 text-[11.5px] leading-snug">
          <StatusText state={t.on ? state : undefined} port={port} />
        </p>
        {t.on && state?.kind === "on" && state.warning && <p className="mt-1 text-[11.5px] leading-snug text-warn">{state.warning}</p>}
      </div>
      <div className="-mr-1.5 flex shrink-0 items-center">
        {t.forward.kind === "local" && t.on && state?.kind === "on" && (
          <IconButton label={`Open localhost:${port} in your browser`} onClick={() => void backend.openExternal(appUrl({ scheme: t.scheme ?? null }, port))}>
            <ArrowUpRight size={14} />
          </IconButton>
        )}
        {t.on && state?.kind === "failed" && (
          <IconButton label="Try again" onClick={() => void startTunnel(hostId, t.id, t.forward)}>
            <RotateCw size={13} />
          </IconButton>
        )}
        <IconButton label="Edit" onClick={onEdit}>
          <Pencil size={13} />
        </IconButton>
        <IconButton label="Remove" onClick={() => removeSaved(hostId, t.id)}>
          <Trash2 size={13} />
        </IconButton>
      </div>
    </li>
  );
}

/** Off, Starting…, On, Reconnecting (with a countdown) or why it failed. */
function StatusText({ state, port }: { state: TunnelState | undefined; port: number }) {
  switch (state?.kind) {
    case undefined:
    case "off":
      return <span className="text-fg-3">Off</span>;
    case "starting":
      return (
        <span className="inline-flex items-center gap-1.5 text-fg-3">
          <Spinner className="size-3" /> Starting…
        </span>
      );
    case "on":
      return (
        <span className="inline-flex items-center gap-1.5 text-ok">
          <span className="size-1.5 rounded-full bg-ok" /> On at <span className="font-mono">localhost:{port}</span>
        </span>
      );
    case "reconnecting":
      return (
        <span className="inline-flex items-center gap-1.5 text-warn">
          <Spinner className="size-3" /> <Retry state={state} />
        </span>
      );
    case "failed":
      return <span className="text-bad">{state.message}</span>;
  }
}

/** "The server stopped responding. Trying again in 3s…", counting down. */
function Retry({ state }: { state: Extract<TunnelState, { kind: "reconnecting" }> }) {
  const [now, setNow] = useState(() => Date.now());
  const [until, setUntil] = useState(() => Date.now() + state.retryInMs);
  useEffect(() => {
    setUntil(Date.now() + state.retryInMs);
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(timer);
  }, [state]);
  const left = Math.max(0, Math.ceil((until - now) / 1000));
  return (
    <>
      {state.reason}. {left > 0 ? `Trying again in ${left}s…` : "Reconnecting…"}
    </>
  );
}

// ---------------------------------------------------------------- add / edit

const inputClass =
  "h-8 w-full min-w-0 rounded-[8px] bg-ink/[0.05] px-2.5 font-mono text-[12.5px] text-fg outline-none ring-1 ring-inset ring-ink/[0.08] placeholder:text-fg-3/70 focus:ring-2 focus:ring-accent/70";

function Field({ label, hint, className, children }: { label: string; hint?: string; className?: string; children: ReactNode }) {
  return (
    <label className={cn("flex min-w-0 flex-col gap-1", className)}>
      <span className="text-[11.5px] font-medium text-fg-2">{label}</span>
      {children}
      {hint && <span className="text-[11px] leading-snug text-fg-3">{hint}</span>}
    </label>
  );
}

const digits = (v: string) => v.replace(/\D/g, "").slice(0, 5);

function TunnelForm({ hostId, tunnel, onDone }: { hostId: string; tunnel?: SavedTunnel; onDone: () => void }) {
  const was = tunnel?.forward;
  const [kind, setKind] = useState<Forward["kind"]>(was?.kind ?? "local");
  const [remotePort, setRemotePort] = useState(was?.kind === "local" ? String(was.remotePort) : "");
  const [remoteHost, setRemoteHost] = useState(was?.kind === "local" ? was.remoteHost : SERVER);
  // Follows the server's port until you type one of your own.
  const [localPort, setLocalPort] = useState(was ? String(was.localPort) : "");
  const [ownLocal, setOwnLocal] = useState(!!was);
  const [name, setName] = useState(tunnel?.name ?? "");
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  const local = kind === "socks" ? localPort || "1080" : ownLocal ? localPort : remotePort;
  const forward: Forward | null =
    kind === "socks"
      ? { kind, localPort: Number(local) }
      : { kind, localPort: Number(local), remoteHost: remoteHost.trim() || SERVER, remotePort: Number(remotePort) };
  const valid =
    (kind === "local" ? (portError(remotePort, "the server's port") ?? hostError(remoteHost)) : null) ?? portError(local, "a port on this computer");

  const submit = async () => {
    if (checking) return;
    if (valid || !forward) return setError(valid);
    const all = Object.values(useSavedTunnels.getState().byHost).flat();
    if (all.some((t) => t.id !== tunnel?.id && t.forward.localPort === forward.localPort)) {
      return setError(`Another saved tunnel already uses port ${forward.localPort} on this computer.`);
    }
    // A tunnel that's on already holds its own port.
    const holds = tunnel && tunnel.on && tunnel.forward.localPort === forward.localPort && isActive(useTunnelRuntime.getState().status[tunnel.id]);
    if (!holds) {
      setChecking(true);
      const problem = await backend.localPortProblem(forward.localPort).catch((err) => toOpError(err).message);
      setChecking(false);
      if (problem) return setError(problem);
    }
    saveTunnel(hostId, tunnel?.id ?? null, name, forward);
    onDone();
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Enter" && e.target instanceof HTMLInputElement) {
      e.preventDefault();
      void submit();
    }
  };

  return (
    // Not a <form>: the kind switcher's buttons would submit it.
    <div onKeyDown={onKey} className="rounded-[11px] bg-ink/[0.035] p-3.5 ring-1 ring-inset ring-accent/40">
      <Segmented
        value={kind}
        onChange={(k) => {
          setKind(k);
          setError(null);
        }}
        options={[
          { value: "local", label: "A port on the server" },
          { value: "socks", label: "Browse through the server" },
        ]}
      />
      {kind === "local" ? (
        <div className="mt-3 grid grid-cols-2 gap-x-2.5 gap-y-3">
          <Field label="The server's port">
            <input
              autoFocus
              value={remotePort}
              onChange={(e) => {
                setRemotePort(digits(e.target.value));
                setError(null);
              }}
              placeholder="3000"
              inputMode="numeric"
              className={inputClass}
            />
          </Field>
          <Field label="Port on this computer">
            <input
              value={local}
              onChange={(e) => {
                setOwnLocal(true);
                setLocalPort(digits(e.target.value));
                setError(null);
              }}
              placeholder={remotePort || "3000"}
              inputMode="numeric"
              className={inputClass}
            />
          </Field>
          <Field
            label="Address, as the server sees it"
            hint="127.0.0.1 is the server itself. Another address reaches a machine the server can see, like a database on its network."
            className="col-span-2"
          >
            <input
              value={remoteHost}
              onChange={(e) => {
                setRemoteHost(e.target.value);
                setError(null);
              }}
              placeholder={SERVER}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              className={inputClass}
            />
          </Field>
        </div>
      ) : (
        <div className="mt-3">
          <Field
            label="Port on this computer"
            hint={`Set your browser's or app's proxy to SOCKS5 at localhost:${local}. Sites then see the server's address instead of yours, and the server's private network works too.`}
          >
            <input
              autoFocus
              value={localPort}
              onChange={(e) => {
                setLocalPort(digits(e.target.value));
                setError(null);
              }}
              placeholder="1080"
              inputMode="numeric"
              className={cn(inputClass, "w-[120px]")}
            />
          </Field>
        </div>
      )}
      <Field label="Name (optional)" className="mt-3">
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === "socks" ? "Browse through the server" : "Grafana"} className={cn(inputClass, "font-sans")} />
      </Field>
      {!valid && forward && <p className="mt-3 text-[11.5px] leading-snug text-fg-2">{describeForward(forward)}</p>}
      {error && <p className="mt-2 text-[11.5px] leading-snug text-bad">{error}</p>}
      <div className="mt-3 flex justify-end gap-2">
        <Button type="button" size="sm" variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button type="button" size="sm" variant="primary" disabled={checking} onClick={() => void submit()}>
          {checking && <Spinner className="size-3" />}
          {tunnel ? "Save" : "Save and turn on"}
        </Button>
      </div>
    </div>
  );
}
