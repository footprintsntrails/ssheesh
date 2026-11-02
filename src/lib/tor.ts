// Tor, from the UI's side: its status in the store, and switching it.
// The Rust side (crates/sshee-core/src/tor.rs) does the rest.

import { backend } from "./backend";
import { useStore } from "./store";
import { toOpError, type TorStatus } from "./types";

/** How long Tor gets to start before you're told it hasn't, and why. */
const SLOW_START_MS = 25000;

/** Says "Tor is on" once it really is, after you turned it on. */
let announceOn = false;
/** What's been said about Tor not starting, so it's said once. */
let toldTrouble = "";
let slowStart: ReturnType<typeof setTimeout> | undefined;

export async function showTorLog(): Promise<void> {
  const { toast } = useStore.getState();
  try {
    const path = await backend.torLogPath();
    if (path) await backend.revealFiles([path]);
  } catch (e) {
    toast({ tone: "error", title: "Couldn't show Tor's log", body: toOpError(e).message });
  }
}

function tellTrouble(body: string) {
  if (toldTrouble === body) return;
  toldTrouble = body;
  useStore.getState().toast({
    tone: "error",
    title: "Tor isn't connected yet",
    body: `${body} Your servers wait until it is: SSHeesh won't connect them around Tor.`,
    action: { label: "Show log", run: () => void showTorLog() },
  });
}

/** What the rest of the app does when Tor's status changes. */
function follow(tor: TorStatus) {
  if (tor.state !== "starting") {
    clearTimeout(slowStart);
    slowStart = undefined;
  }
  if (tor.state === "on") {
    toldTrouble = "";
    if (announceOn) {
      announceOn = false;
      useStore.getState().toast({ tone: "success", title: "Tor is on", body: "Servers see a Tor address instead of yours, and your internet provider doesn't see which servers you use." });
    }
  } else if (tor.state === "off") {
    announceOn = false;
    toldTrouble = "";
  } else if (tor.state === "failed") {
    tellTrouble(tor.message);
  } else if (slowStart === undefined) {
    slowStart = setTimeout(() => {
      const now = useStore.getState().tor;
      if (now.state === "starting") tellTrouble(now.note ?? "It's taking longer than it should.");
    }, SLOW_START_MS);
  }
}

/** Keeps the store's Tor status current, from the app's start on. */
export function startTorWatch(): () => void {
  const seen = (tor: TorStatus) => {
    useStore.setState({ tor });
    follow(tor);
    void backend.torSshOptions().then((torSsh) => useStore.setState({ torSsh }), () => useStore.setState({ torSsh: null }));
  };
  void backend.torStatus().then(seen);
  return backend.onTorStatus(seen);
}

/** Starts a Tor that won't start again from nothing. */
export async function startTorAfresh(): Promise<void> {
  toldTrouble = "";
  announceOn = true;
  try {
    await backend.startTorAfresh();
  } catch (e) {
    useStore.getState().toast({ tone: "error", title: "Couldn't start Tor again", body: toOpError(e).message });
  }
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
    announceOn = on;
    const { status, stayed, warning } = await backend.setTor(on);
    useStore.setState({ tor: status });
    follow(status);
    void backend.torSshOptions().then((torSsh) => useStore.setState({ torSsh }), () => useStore.setState({ torSsh: null }));
    if (warning) toast({ tone: "error", title: on ? "Tor is on for now" : "Tor is off for now", body: warning });
    // Turning it on says so when it is (see `follow`): it may take a while, or not happen.
    if (!on) toast({ tone: "info", title: "Tor is off", body: `Servers see your own address again.${stayedNote(stayed)}` });
    else if (stayed) toast({ tone: "info", title: "Moving your terminals to Tor", body: stayedNote(stayed).trim() });
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
