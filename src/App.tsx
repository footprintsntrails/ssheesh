import { useEffect } from "react";
import { AddHostDialog } from "./components/AddServerDialog";
import { ConfigImportDialog } from "./components/ConfigImport";
import { Welcome } from "./components/Welcome";
import { WhatsNew } from "./components/WhatsNew";
import { UploadDialog, useUploadAsk } from "./components/UploadDialog";
import { findTerminal } from "./lib/terminals";
import { shellQuote } from "./lib/format";
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
import { ServerTools } from "./components/ServerTools";
import { Recipes } from "./components/Recipes";
import { Snippets } from "./components/Snippets";
import { Tunnels } from "./components/Tunnels";
import { restoreTunnels } from "./lib/tunnels";

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
        // Files land in a file manager's open folder, or, on a server's
        // terminal, wherever you confirm (it starts at the terminal's folder).
        const el = document.elementFromPoint(e.x, e.y)?.closest<HTMLElement>("[data-pane-id]");
        const pane = el?.dataset.paneId ? s.panes[el.dataset.paneId] : undefined;
        const takes = !!pane?.target && (pane.kind === "files" || pane.kind === "terminal");
        if (e.type === "over") return s.setDropPane(takes && pane ? pane.id : null);
        s.setDropPane(null);
        if (!e.paths.length) return;
        if (pane?.kind === "files" && pane.target) {
          window.dispatchEvent(new CustomEvent("sshee:drop", { detail: { paneId: pane.id, paths: e.paths } }));
        } else if (pane?.kind === "terminal" && pane.target?.kind === "host") {
          useUploadAsk.getState().ask({ target: pane.target, paths: e.paths, sessionKey: pane.sessionKey });
        } else if (pane?.kind === "terminal") {
          // This computer's terminal: type the paths, like any terminal app does.
          backend.writeSession(pane.id, `${e.paths.map(shellQuote).join(" ")} `);
          findTerminal(pane.id)?.focus();
        } else {
          s.toast({ tone: "info", title: "Drop files on a terminal or a file manager", body: "On a server they're uploaded; you pick the folder." });
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
  // Saved tunnels that were on when SSHeesh closed come back on.
  useEffect(() => restoreTunnels(), []);
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
      <UploadDialog />
      <ServerTools />
      <Recipes />
      <Snippets />
      <Tunnels />
      <ConfirmDialog />
      <Toasts />
    </div>
  );
}
