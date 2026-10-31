import { beforeEach, describe, expect, it } from "vitest";
import { paneIds, type LayoutNode } from "./layout";
import { activePane, currentTarget, panesOn, sideBySide, splitByMachine, tabsOn, useStore, type PaneState } from "./store";
import { useSettings } from "./settings";
import { nextTab, placeBeside, splitPane, toggleSplit, toOwnTab } from "./tabs";
import type { TargetRef } from "./types";

const web: TargetRef = { kind: "host", hostId: "cfg:web" };
const db: TargetRef = { kind: "host", hostId: "cfg:db" };
const pane = (id: string, target: TargetRef, kind: PaneState["kind"] = "terminal", n?: number): PaneState => ({ id, kind, target, n });
const row = (...ids: string[]): LayoutNode => ({ kind: "split", id: `s-${ids.join("")}`, dir: "row", children: ids.map((id) => ({ kind: "pane", id })) });

function start(panes: PaneState[], layouts: Record<string, LayoutNode>, focused: string) {
  useStore.setState({
    panes: Object.fromEntries(panes.map((p) => [p.id, p])),
    layouts,
    focused,
    home: false,
    hidden: [],
    tabOrder: [],
    splitOn: {},
    zoomed: null,
    closedTabs: [],
    lastFocusedFor: {},
    sessions: {},
  });
}

describe("one layout per machine", () => {
  beforeEach(() => start([pane("w1", web, "terminal", 1), pane("w2", web, "files", 1), pane("d1", db, "terminal", 1)], { "host:cfg:web": row("w1", "w2"), "host:cfg:db": row("d1") }, "w1"));

  it("opens a new tab next to the machine's own tab, not the one on screen", () => {
    const id = useStore.getState().openIn("terminal", db);
    const s = useStore.getState();
    expect(paneIds(s.layouts["host:cfg:db"])).toEqual(["d1", id]);
    expect(paneIds(s.layouts["host:cfg:web"])).toEqual(["w1", "w2"]);
    expect(s.panes[id].n).toBe(2);
    expect(currentTarget(s)).toEqual(db);
  });

  it("closing a tab brings up the next one on the same machine, then the main menu", () => {
    useStore.getState().closePane("w1");
    expect(useStore.getState().focused).toBe("w2");
    useStore.getState().closePane("w2");
    const s = useStore.getState();
    expect(s.home).toBe(true);
    expect(s.layouts["host:cfg:web"]).toBeUndefined();
    expect(s.closedTabs.map((c) => c.kind)).toEqual(["files", "terminal"]);
  });

  it("a split is a new terminal on the same machine, beside the one you split", () => {
    const id = useStore.getState().split("w2", "col");
    const s = useStore.getState();
    expect(s.panes[id]).toMatchObject({ kind: "terminal", target: web, n: 2 });
    expect(paneIds(s.layouts["host:cfg:web"])).toEqual(["w1", "w2", id]);
    expect(paneIds(s.layouts["host:cfg:db"])).toEqual(["d1"]);
    expect(s.focused).toBe(id);
  });

  it("on the main menu there's no tab to act on", () => {
    useStore.getState().showHome();
    expect(activePane(useStore.getState())).toBeNull();
    expect(currentTarget(useStore.getState())).toBeNull();
  });

  it("a pane filling the window stays with its machine", () => {
    useStore.getState().setSplit(web, true);
    useStore.getState().toggleZoom("w1");
    expect(useStore.getState().zoomed).toBe("w1");
    useStore.getState().goTo(db);
    expect(useStore.getState().zoomed).toBeNull();
  });

  it("going to another pane stops one filling the window, so it's on screen", () => {
    useStore.getState().setSplit(web, true);
    useStore.getState().toggleZoom("w1");
    useStore.getState().focus("w2");
    expect(useStore.getState().zoomed).toBeNull();
  });

  it("one tab at a time, nothing can fill the window: every tab does already", () => {
    useStore.getState().toggleZoom("w1");
    expect(useStore.getState().zoomed).toBeNull();
  });

  it("arranging and the side-by-side workspace touch only that machine", () => {
    useStore.getState().openWorkspace(db);
    const s = useStore.getState();
    expect(paneIds(s.layouts["host:cfg:web"])).toEqual(["w1", "w2"]);
    expect(panesOn(s, db).map((p) => p.kind)).toEqual(["terminal", "files"]);
    expect(s.splitOn["host:cfg:db"]).toBe(true);
  });

  it("tabs keep the order you dragged them to", () => {
    useStore.getState().setTabOrder(["w2"]);
    expect(tabsOn(useStore.getState(), web).map((p) => p.id)).toEqual(["w2", "w1"]);
  });
});

describe("side by side, one tab at a time", () => {
  const onScreen = () => {
    const s = useStore.getState();
    return sideBySide(s, web) ? panesOn(s, web).filter((p) => !s.hidden.includes(p.id)).map((p) => p.id) : [s.focused];
  };
  beforeEach(() => {
    start([pane("w1", web, "terminal", 1), pane("w2", web, "files", 1), pane("w3", web, "terminal", 2)], { "host:cfg:web": row("w1", "w2", "w3") }, "w1");
    placeBeside("w2", "right", "w1");
  });

  it("a tab dropped beside makes a pair", () => {
    expect(onScreen().sort()).toEqual(["w1", "w2"]);
  });

  it("a new tab opens on its own, and the pair comes back from one of its tabs", () => {
    const id = useStore.getState().openIn("terminal", web);
    expect(onScreen()).toEqual([id]);
    useStore.getState().focus("w1");
    expect(onScreen().sort()).toEqual(["w1", "w2"]);
  });

  it("another tab shows on its own instead of joining the pair", () => {
    useStore.getState().focus("w3");
    expect(onScreen()).toEqual(["w3"]);
    useStore.getState().focus("w2");
    expect(onScreen().sort()).toEqual(["w1", "w2"]);
  });

  it("closing the split keeps both tabs", () => {
    toOwnTab("w2");
    const s = useStore.getState();
    expect(Object.keys(s.panes).sort()).toEqual(["w1", "w2", "w3"]);
    expect(onScreen()).toEqual(["w1"]);
  });

  it("with side by side as the layout, a tab you go to joins the others on screen", () => {
    useSettings.setState({ viewMode: "split" });
    try {
      useStore.getState().focus("w3");
      expect(onScreen().sort()).toEqual(["w1", "w2", "w3"]);
    } finally {
      useSettings.setState({ viewMode: "tabs" });
    }
  });

  it("with side by side as the layout, × takes a pane off the screen and keeps its tab", () => {
    useSettings.setState({ viewMode: "split" });
    try {
      useStore.getState().setHidden(["w3"], false);
      toOwnTab("w2");
      const s = useStore.getState();
      expect(Object.keys(s.panes).sort()).toEqual(["w1", "w2", "w3"]);
      expect(onScreen().sort()).toEqual(["w1", "w3"]);
    } finally {
      useSettings.setState({ viewMode: "tabs" });
    }
  });

  it("× on one of three panes leaves the other two side by side", () => {
    useStore.getState().focus("w3");
    toggleSplit();
    expect(onScreen().sort()).toEqual(["w1", "w2", "w3"]);
    toOwnTab("w3");
    expect(onScreen().sort()).toEqual(["w1", "w2"]);
  });

  it("switching the layout to side by side keeps the tab you're on on screen", () => {
    useStore.getState().focus("w3");
    try {
      useSettings.getState().update({ viewMode: "split" });
      expect(useStore.getState().hidden).not.toContain("w3");
    } finally {
      useSettings.getState().update({ viewMode: "tabs" });
    }
  });

  it("splitting a pane of the pair adds a third instead of swapping one out", () => {
    splitPane("w1", "row");
    expect(onScreen()).toHaveLength(3);
  });

  it("Ctrl+Tab goes through every tab, not only the pair", () => {
    const seen = new Set<string>();
    for (let i = 0; i < 3; i++) {
      nextTab(1);
      seen.add(useStore.getState().focused);
    }
    expect([...seen].sort()).toEqual(["w1", "w2", "w3"]);
  });

  it("the shortcut goes back to one tab at a time, and again to the pair", () => {
    toggleSplit();
    expect(onScreen()).toEqual([useStore.getState().focused]);
    toggleSplit();
    expect(onScreen().sort()).toEqual(["w1", "w2"]);
  });
});

describe("from 0.5's single layout", () => {
  it("splits it per machine, keeps each arrangement and numbers every tab", () => {
    const saved = splitByMachine({
      root: { kind: "split", id: "s", dir: "row", children: [{ kind: "pane", id: "a" }, row("b", "c"), { kind: "pane", id: "e" }] },
      panes: {
        a: pane("a", web),
        b: pane("b", db),
        c: pane("c", web, "terminal", 1),
        e: { id: "e", kind: "empty", target: null },
      },
      focused: "e",
    });
    const layouts = saved.layouts as Record<string, LayoutNode>;
    const panes = saved.panes as Record<string, PaneState>;
    expect(paneIds(layouts["host:cfg:web"])).toEqual(["a", "c"]);
    expect(paneIds(layouts["host:cfg:db"])).toEqual(["b"]);
    expect(panes.e).toBeUndefined();
    expect([panes.a.n, panes.c.n]).toEqual([2, 1]);
    expect(saved.home).toBe(true);
  });
});
