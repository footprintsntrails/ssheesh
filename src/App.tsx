import { lazy, Suspense, useEffect } from "react";
import { AddHostDialog } from "./components/AddServerDialog";
import { ConfigImportDialog } from "./components/ConfigImport";
import { Hints } from "./components/Hints";
import { useWindowFrame } from "./components/WindowControls";
import { WhatsNew } from "./components/WhatsNew";
import { UploadDialog, useUploadAsk } from "./components/UploadDialog";
import { findTerminal } from "./lib/terminals";
import { shellOf, typedQuote } from "./lib/format";
import { EdgeReveal } from "./components/EdgeReveal";
import { CommandPalette, ConfirmDialog, dialogOpen, Toasts } from "./components/Overlays";
import { Sidebar } from "./components/Sidebar";
import { EnvStripe, Workspace } from "./components/Workspace";
import { backend } from "./lib/backend";
import { matchShortcut, tabStep } from "./lib/keys";
import { closeTab, newTab, nextTab, splitPane, toggleSplit } from "./lib/tabs";
import { cn } from "./lib/format";
import { applyAppearance, compactSidebar, ONBOARDING_VERSION, toggleCompact, useSettings } from "./lib/settings";
import { useTour } from "./lib/tour";
import { activePane, useStore } from "./lib/store";
import { startForegroundWatch } from "./lib/agents";
import { startRoleDetection } from "./lib/roles";
import { useLabels } from "./lib/labels";
import { isMainWindow, openedFor } from "./lib/windowInfo";
import { startUpdateChecks, useUpdates } from "./lib/updates";
import { toggleStreamer, useLearnLocalIdentity } from "./lib/privacy";
import { ServerTools } from "./components/ServerTools";
import { Recipes } from "./components/Recipes";
import { Snippets } from "./components/Snippets";
import { Tunnels } from "./components/Tunnels";
import { CopyToServer } from "./components/CopyToServer";
import { restoreTunnels } from "./lib/tunnels";

function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Behind a dialog nothing opens, closes or switches; its keys are its own.
      // Streamer mode still toggles: hiding names can't wait for a dialog to close.
      if (dialogOpen() && matchShortcut(e) !== "streamer") return;
      const step = tabStep(e);
      if (step) {
        e.preventDefault();
        e.stopPropagation();
        nextTab(step);
        return;
      }
      const id = matchShortcut(e);
      if (!id) return;
      // In a text field (not the terminal's own), Ctrl+Shift+Z is redo.
      const field = (e.target as HTMLElement | null)?.closest?.("input, textarea, [contenteditable='true']");
      if (id === "reopenTab" && field && !field.classList.contains("xterm-helper-textarea")) return;
      e.preventDefault();
      e.stopPropagation();
      const s = useStore.getState();
      // Only what's on screen: on the main menu there's no tab to split or close.
      const pane = activePane(s);
      if (id === "palette") s.setPaletteOpen(!s.paletteOpen);
      else if (id === "addHost") s.setAddHostOpen(true);
      else if (id === "splitRight") pane && splitPane(pane.id, "row");
      else if (id === "splitDown") pane && splitPane(pane.id, "col");
      else if (id === "closePane") pane && closeTab(pane.id);
      // On the main menu, a new terminal goes to the server you were on last.
      else if (id === "newTerminal") newTab("terminal", pane?.target ?? s.panes[s.focused]?.target ?? { kind: "local" });
      else if (id === "reopenTab") s.reopenClosed();
      else if (id === "toggleSplit") toggleSplit();
      else if (id === "settings") s.settingsOpen ? s.closeSettings() : s.openSettings();
      else if (id === "zoomPane") s.toggleZoom();
      else if (id === "streamer") toggleStreamer();
      else if (id === "compact") toggleCompact();
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
          // This computer's terminal: type the paths, like any terminal app does,
          // unless a name would act as keys there.
          const typed = e.paths.map((p) => typedQuote(p, shellOf({ kind: "local" }))).filter((t) => t !== null);
          if (typed.length < e.paths.length) {
            s.toast({ tone: "error", title: "Can't type that name", body: "A file's name has control characters in it, which the terminal would take as keystrokes." });
            return;
          }
          backend.writeSession(pane.id, `${typed.join(" ")} `);
          findTerminal(pane.id)?.focus();
        } else {
          s.toast({ tone: "info", title: "Drop files on a terminal or a file manager", body: "On a server they're uploaded; you pick the folder." });
        }
      }),
    [],
  );
}

// Loaded the first time they're needed, so starting SSHeesh doesn't wait on
// Settings, the hello or the tour.
const SettingsDialog = lazy(() => import("./components/SettingsDialog").then((m) => ({ default: m.SettingsDialog })));
const Welcome = lazy(() => import("./components/Welcome").then((m) => ({ default: m.Welcome })));
const Tour = lazy(() => import("./components/Tour").then((m) => ({ default: m.Tour })));

/** What applyAppearance reads. */
const LOOK = ["theme", "customThemes", "accent", "animations", "paneGap", "paneRadius"] as const;

/** Settings and labels changed in another window show up in this one. */
function useSharedAcrossWindows() {
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === "sshee-settings") void useSettings.persist.rehydrate();
      else if (e.key === "sshee-labels") void useLabels.persist.rehydrate();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
}

export function App() {
  const loadHosts = useStore((s) => s.loadHosts);
  useEffect(() => {
    void loadHosts().then(() => {
      // A window opened for one server goes straight to it.
      const t = openedFor();
      if (t && !isMainWindow) useStore.getState().goTo(t);
    });
  }, [loadHosts]);
  useSharedAcrossWindows();
  useWindowFrame();
  // Shortcut labels all over the app follow your own keys at once.
  useSettings((s) => s.shortcuts);
  useEffect(() => startForegroundWatch(), []);
  useEffect(() => startRoleDetection(), []);
  useShortcuts();
  useFileDrop();
  useLearnLocalIdentity();
  // Colors and spacing are applied again only when one of them changes.
  useEffect(
    () =>
      useSettings.subscribe((s, prev) => {
        if (LOOK.some((k) => s[k] !== prev[k])) applyAppearance(s);
      }),
    [],
  );
  // Keep the session core's reconnect behaviour in step with Settings.
  const reconnectMode = useSettings((s) => s.reconnectMode);
  useEffect(() => backend.setAutoReconnect(reconnectMode === "auto"), [reconnectMode]);
  // Once per app, in its first window.
  useEffect(() => {
    if (isMainWindow) startUpdateChecks();
    else void backend.appVersion().then((current) => useUpdates.setState({ current }));
  }, []);
  // Saved tunnels that were on when SSHeesh closed come back on.
  useEffect(() => (isMainWindow ? restoreTunnels() : undefined), []);
  const uiZoom = useSettings((s) => s.uiZoom);
  useEffect(() => void backend.setZoom(uiZoom / 100).catch(() => {}), [uiZoom]);
  useFollowSystemTheme();
  const sidebarVisible = useSettings((s) => s.sidebarVisible);
  const sidebarSide = useSettings((s) => s.sidebarSide);
  const compact = useSettings((s) => compactSidebar(s.compactMode));
  const settingsOpen = useStore((s) => s.settingsOpen);
  const hello = useSettings((s) => s.onboardingVersion < ONBOARDING_VERSION);
  const tourOpen = useTour((s) => s.open);

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
      <CommandPalette />
      <AddHostDialog />
      <ConfigImportDialog />
      <WhatsNew />
      <UploadDialog />
      <CopyToServer />
      <ServerTools />
      <Recipes />
      <Snippets />
      <Tunnels />
      <Suspense fallback={null}>
        {settingsOpen && <SettingsDialog />}
        {hello && <Welcome />}
        {tourOpen && <Tour />}
      </Suspense>
      <Hints />
      <ConfirmDialog />
      <Toasts />
      <EnvStripe />
    </div>
  );
}
