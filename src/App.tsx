import { useEffect } from "react";
import { AddHostDialog } from "./components/AddServerDialog";
import { ConfigImportDialog } from "./components/ConfigImport";
import { Welcome } from "./components/Welcome";
import { WhatsNew } from "./components/WhatsNew";
import { EdgeReveal } from "./components/EdgeReveal";
import { CommandPalette, ConfirmDialog, Toasts } from "./components/Overlays";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { Workspace } from "./components/Workspace";
import { backend } from "./lib/backend";
import { matchShortcut } from "./lib/keys";
import { cn } from "./lib/format";
import { applyAppearance, useSettings } from "./lib/settings";
import { useStore } from "./lib/store";
import { startUpdateChecks } from "./lib/updates";
import { toggleStreamer, useLearnLocalIdentity } from "./lib/privacy";

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
      else if (id === "streamer") toggleStreamer();
      else if (id === "compact") {
        const st = useSettings.getState();
        st.update({ compactMode: !st.compactMode });
      }
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

/** With "Match my system" on, swaps the light and dark theme as the system does. */
function useFollowSystemTheme() {
  const follow = useSettings((s) => s.themeFollowsSystem);
  useEffect(() => {
    if (!follow) return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const st = useSettings.getState();
      const want = mq.matches ? st.darkTheme : st.lightTheme;
      if (st.theme !== want) st.update({ theme: want });
    };
    apply();
    mq.addEventListener("change", apply);
    const unsub = useSettings.subscribe((s, prev) => {
      if (s.darkTheme !== prev.darkTheme || s.lightTheme !== prev.lightTheme) apply();
    });
    return () => {
      mq.removeEventListener("change", apply);
      unsub();
    };
  }, [follow]);
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
  useLearnLocalIdentity();
  useEffect(() => useSettings.subscribe(applyAppearance), []);
  // Keep the session core's reconnect behaviour in step with Settings.
  const reconnectMode = useSettings((s) => s.reconnectMode);
  useEffect(() => backend.setAutoReconnect(reconnectMode === "auto"), [reconnectMode]);
  useEffect(() => startUpdateChecks(), []);
  const uiZoom = useSettings((s) => s.uiZoom);
  useEffect(() => void backend.setZoom(uiZoom / 100).catch(() => {}), [uiZoom]);
  useFollowSystemTheme();
  const sidebarVisible = useSettings((s) => s.sidebarVisible);
  const sidebarSide = useSettings((s) => s.sidebarSide);
  const compact = useSettings((s) => s.compactMode);

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
      {sidebarVisible && !compact && <Sidebar />}
      {sidebarVisible && compact && (
        <EdgeReveal side={sidebarSide} label="Servers">
          <Sidebar />
        </EdgeReveal>
      )}
      <Workspace />
      <SettingsDialog />
      <CommandPalette />
      <AddHostDialog />
      <ConfigImportDialog />
      <Welcome />
      <WhatsNew />
      <ConfirmDialog />
      <Toasts />
    </div>
  );
}
