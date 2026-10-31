// Shift+click a server: it opens in a window of its own, as Shift+click on a
// link does in a browser. Handy for two servers on two screens.

import { backend } from "./backend";
import { isMac, isWindows } from "./keys";
import { useSettings } from "./settings";
import { useStore } from "./store";
import { toOpError, type TargetRef } from "./types";

export async function openInNewWindow(target: TargetRef) {
  const toast = useStore.getState().toast;
  if (backend.isDemo) {
    toast({ tone: "info", title: "New windows open in the app", body: "The demo runs in a single browser tab." });
    return;
  }
  const label = `server-${Math.random().toString(36).slice(2, 10)}`;
  try {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const w = new WebviewWindow(label, {
      url: `/?window=${label}&open=${encodeURIComponent(JSON.stringify(target))}`,
      title: "SSHeesh",
      width: 1200,
      height: 800,
      minWidth: 880,
      minHeight: 540,
      ...(isMac ? { titleBarStyle: "overlay" as const, hiddenTitle: true } : {}),
      // On Windows SSHeesh draws its own title bar, unless you switched to the system's.
      ...(isWindows ? { decorations: useSettings.getState().nativeTitleBar } : {}),
    });
    void w.once("tauri://error", (e) => toast({ tone: "error", title: "Couldn't open a new window", body: String(e.payload) }));
  } catch (e) {
    toast({ tone: "error", title: "Couldn't open a new window", body: toOpError(e).message });
  }
}
