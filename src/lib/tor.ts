// Tor, from the UI's side: its status in the store, and switching it.
// The Rust side (crates/sshee-core/src/tor.rs) does the rest.

import { backend } from "./backend";
import { useStore } from "./store";
import { toOpError, type TorStatus } from "./types";

/** Keeps the store's Tor status current, from the app's start on. */
export function startTorWatch(): () => void {
  const seen = (tor: TorStatus) => {
    useStore.setState({ tor });
    void backend.torSshOptions().then((torSsh) => useStore.setState({ torSsh }), () => useStore.setState({ torSsh: null }));
  };
  void backend.torStatus().then(seen);
  return backend.onTorStatus(seen);
}

function stayedNote(stayed: number): string {
  if (stayed === 0) return "";
  const which = stayed === 1 ? "A terminal on a server without tmux" : `${stayed} terminals on servers without tmux`;
  return ` ${which} stay${stayed === 1 ? "s" : ""} as ${stayed === 1 ? "it is" : "they are"} until ${stayed === 1 ? "it reconnects" : "they reconnect"}.`;
}

/** Turns Tor on or off (the button, Settings, the palette) and says what that did. */
export async function switchTor(on: boolean): Promise<void> {
  const { toast } = useStore.getState();
  try {
    const { status, stayed, warning } = await backend.setTor(on);
    useStore.setState({ tor: status });
    void backend.torSshOptions().then((torSsh) => useStore.setState({ torSsh }), () => useStore.setState({ torSsh: null }));
    if (warning) toast({ tone: "error", title: on ? "Tor is on for now" : "Tor is off for now", body: warning });
    toast(
      on
        ? { tone: "info", title: "Tor is on", body: `Servers see a Tor address instead of yours, and your internet provider doesn't see which servers you use.${stayedNote(stayed)}` }
        : { tone: "info", title: "Tor is off", body: `Servers see your own address again.${stayedNote(stayed)}` },
    );
  } catch (e) {
    toast({ tone: "error", title: on ? "Couldn't turn Tor on" : "Couldn't turn Tor off", body: toOpError(e).message });
  }
}

/** Every server picks its fastest route again, for when one turned slow. */
export async function newTorRoutes(): Promise<void> {
  const { toast } = useStore.getState();
  try {
    const { stayed } = await backend.newTorRoutes();
    toast({ tone: "info", title: "New Tor routes", body: `Terminals reconnect on each server's fastest new route.${stayedNote(stayed)}` });
  } catch (e) {
    toast({ tone: "error", title: "Couldn't pick new Tor routes", body: toOpError(e).message });
  }
}
