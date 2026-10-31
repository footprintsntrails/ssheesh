import { beforeEach, describe, expect, it } from "vitest";
import { paneIds, type LayoutNode } from "./layout";
import { activePane, currentTarget, panesOn, splitByMachine, tabsOn, useStore, type PaneState } from "./store";
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

  it("on the main menu there's no tab to act on", () => {
    useStore.getState().showHome();
    expect(activePane(useStore.getState())).toBeNull();
    expect(currentTarget(useStore.getState())).toBeNull();
  });

  it("a pane filling the window stays with its machine", () => {
    useStore.getState().toggleZoom("w1");
    expect(useStore.getState().zoomed).toBe("w1");
    useStore.getState().goTo(db);
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
