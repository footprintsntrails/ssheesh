import { useEffect } from "react";
import { AddHostDialog, CommandPalette, ConfirmDialog, Toasts } from "./components/Overlays";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { Workspace } from "./components/Workspace";
import { backend } from "./lib/backend";
import { matchShortcut } from "./lib/keys";
import { cn } from "./lib/format";
import { applyAppearance, useSettings } from "./lib/settings";
import { useStore } from "./lib/store";
import { startUpdateChecks } from "./lib/updates";

function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const id = matchShortcut(e);
      if (!id) return;
      e.preventDefault();
      e.stopPropagation();
      const s = useStore.getState();
      if (id === "palette") s.setPaletteOpen(!s.paletteOpen);
      else if (id === "addHost") s.setAddHostOpen(true);
      else if (id === "splitRight") s.split(s.focused, "row");
      else if (id === "splitDown") s.split(s.focused, "col");
      else if (id === "closePane") s.closePane(s.focused);
      else if (id === "settings") s.settingsOpen ? s.closeSettings() : s.openSettings();
      else if (id === "zoomPane") s.toggleZoom();
      else if (id === "vitals") {
        const st = useSettings.getState();
        st.update({ showVitals: !st.showVitals });
      } else if (id === "sidebar") {
        const st = useSettings.getState();
        st.update({ sidebarVisible: !st.sidebarVisible });
      }
    };
    // Capture phase, so shortcuts work even while a terminal has focus.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
}

/** Routes files dragged in from the desktop to the Files pane under the cursor. */
function useFileDrop() {
  useEffect(
    () =>
      backend.onFileDrop((e) => {
        const s = useStore.getState();
        if (e.type === "leave") return s.setDropPane(null);
        const el = document.elementFromPoint(e.x, e.y)?.closest<HTMLElement>('[data-pane-kind="files"]');
        const paneId = el?.dataset.paneId ?? null;
        const remote = paneId ? s.panes[paneId]?.target?.kind === "host" : false;
        if (e.type === "over") return s.setDropPane(remote ? paneId : null);
        s.setDropPane(null);
        if (paneId && remote && e.paths.length) {
          window.dispatchEvent(new CustomEvent("sshee:drop", { detail: { paneId, paths: e.paths } }));
        } else if (e.paths.length) {
          s.toast({ tone: "info", title: "Drop files onto a server's Files pane to upload them" });
        }
      }),
    [],
  );
}

export function App() {
  const loadHosts = useStore((s) => s.loadHosts);
  useEffect(() => {
    void loadHosts();
  }, [loadHosts]);
  useShortcuts();
  useFileDrop();
  useEffect(() => useSettings.subscribe(applyAppearance), []);
  // Keep the session core's reconnect behaviour in step with Settings.
  const reconnectMode = useSettings((s) => s.reconnectMode);
  useEffect(() => backend.setAutoReconnect(reconnectMode === "auto"), [reconnectMode]);
  useEffect(() => startUpdateChecks(), []);
  const sidebarVisible = useSettings((s) => s.sidebarVisible);
  const sidebarSide = useSettings((s) => s.sidebarSide);

  // Right-click belongs to our own menus, not the webview's.
  useEffect(() => {
    if (backend.isDemo) return;
    const block = (e: MouseEvent) => {
      if (!(e.target as HTMLElement)?.closest("input,textarea,.xterm")) e.preventDefault();
    };
    window.addEventListener("contextmenu", block);
    return () => window.removeEventListener("contextmenu", block);
  }, []);

  return (
    <div className={cn("flex h-full", sidebarSide === "right" && "flex-row-reverse")}>
      {sidebarVisible && <Sidebar />}
      <Workspace />
      <SettingsDialog />
      <CommandPalette />
      <AddHostDialog />
      <ConfirmDialog />
      <Toasts />
    </div>
  );
}
