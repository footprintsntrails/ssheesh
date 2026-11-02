import { RefreshCw, Settings } from "lucide-react";
import { useState } from "react";
import { cn } from "../lib/format";
import { TOR_GLYPH } from "../lib/labels";
import { activePane, useStore } from "../lib/store";
import { newTorRoutes, switchTor } from "../lib/tor";
import { ContextMenu, type Menu } from "./ContextMenu";
import { IconButton } from "./ui";

/**
 * Tor on and off in one click: above Settings at the sidebar's foot, and in
 * the top bar while the sidebar is away. On, it's green; starting,
 * it pulses; when Tor can't start, a red dot says so, and nothing connects
 * until it does. Right-click for new routes and Settings.
 */
export function TorButton({ size = 20, className }: { size?: number; className?: string }) {
  const tor = useStore((s) => s.tor);
  const [menu, setMenu] = useState<Menu | null>(null);
  const on = tor.state !== "off";
  const label =
    tor.state === "off"
      ? "Connect through Tor: servers see a Tor address, not yours"
      : tor.state === "starting"
        ? `Tor is starting… ${tor.percent}%`
        : tor.state === "on"
          ? "Tor is on: servers see a Tor address, not yours. Click to turn it off"
          : `Tor can't start: ${tor.message}`;
  return (
    <>
      <IconButton
        label={label}
        aria-pressed={on}
        onClick={() => void switchTor(!on)}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
        className={cn("relative", className, on && "bg-tor/15 text-tor hover:bg-tor/20 hover:text-tor")}
      >
        <TOR_GLYPH size={size} className={cn(tor.state === "starting" && "animate-pulse")} />
        {tor.state === "failed" && <span className="absolute top-1.5 right-1.5 size-1.5 rounded-full bg-bad" />}
      </IconButton>
      {menu && (
        <ContextMenu
          menu={menu}
          items={[
            { label: on ? "Turn Tor off" : "Turn Tor on", icon: TOR_GLYPH as unknown as typeof Settings, run: () => void switchTor(!on) },
            { label: "New Tor routes", icon: RefreshCw, run: () => void newTorRoutes(), disabled: tor.state !== "on" },
            { label: "Tor settings…", icon: Settings, run: () => useStore.getState().openSettings("privacy") },
          ]}
          onClose={() => setMenu(null)}
        />
      )}
    </>
  );
}

/**
 * Beside the server's name and address while Tor is on: this server sees
 * Tor, not you. Amber when the terminal you're on doesn't go through Tor
 * (a server on your own network, or one without tmux that connected
 * before Tor was on and moves when it reconnects).
 */
export function TorMark() {
  const tor = useStore((s) => s.tor.state);
  const session = useStore((s) => {
    const pane = activePane(s);
    return pane ? s.sessions[pane.id] : undefined;
  });
  if (tor === "off") return null;
  const direct = session?.kind === "connected" && session.tor === false;
  return (
    <span
      title={
        direct
          ? "This terminal doesn't go through Tor: the server is on your own network, or it connected before Tor was on and moves when it reconnects"
          : "Through Tor: this server sees a Tor address, not yours"
      }
      className={cn("shrink-0", direct ? "text-warn" : "text-tor")}
    >
      <TOR_GLYPH size={16} />
    </span>
  );
}
