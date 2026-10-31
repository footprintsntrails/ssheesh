// The workspace is a tree of splits with panes as leaves.

export type Dir = "row" | "col";

export type LayoutNode = { kind: "pane"; id: string } | { kind: "split"; id: string; dir: Dir; children: LayoutNode[] };

export type DropZone = "left" | "right" | "top" | "bottom" | "center";

export type Arrangement = "columns" | "rows" | "grid" | "main";

export function newId(prefix: string): string {
  return `${prefix}_${Math.random().toString(36).slice(2, 10)}`;
}

/** Puts `newPaneId` next to `targetId` (after it, or before). Same-direction splits stay flat. */
export function insertAt(root: LayoutNode, targetId: string, dir: Dir, newPaneId: string, before = false): LayoutNode {
  const leaf: LayoutNode = { kind: "pane", id: newPaneId };
  const go = (node: LayoutNode): LayoutNode => {
    if (node.kind === "pane") {
      if (node.id !== targetId) return node;
      return { kind: "split", id: newId("s"), dir, children: before ? [leaf, node] : [node, leaf] };
    }
    const idx = node.children.findIndex((c) => c.kind === "pane" && c.id === targetId);
    if (idx >= 0 && node.dir === dir) {
      const children = node.children.slice();
      children.splice(before ? idx : idx + 1, 0, leaf);
      return { ...node, children };
    }
    return { ...node, children: node.children.map(go) };
  };
  return go(root);
}

export function splitAt(root: LayoutNode, paneId: string, dir: Dir, newPaneId: string, before = false): LayoutNode {
  return insertAt(root, paneId, dir, newPaneId, before);
}

/** Removes a pane; splits left with one child collapse into it. */
export function removePane(root: LayoutNode, paneId: string): LayoutNode | null {
  const go = (node: LayoutNode): LayoutNode | null => {
    if (node.kind === "pane") return node.id === paneId ? null : node;
    const children = node.children.map(go).filter((c): c is LayoutNode => c !== null);
    if (children.length === 0) return null;
    if (children.length === 1) return children[0];
    return { ...node, children };
  };
  return go(root);
}

/** The tree without the panes in `hidden`; null if nothing is left. */
export function visibleTree(root: LayoutNode, hidden: string[]): LayoutNode | null {
  let tree: LayoutNode | null = root;
  for (const id of hidden) {
    if (!tree) break;
    tree = removePane(tree, id);
  }
  return tree;
}

export function swapPanes(root: LayoutNode, a: string, b: string): LayoutNode {
  const go = (node: LayoutNode): LayoutNode => {
    if (node.kind === "pane") return node.id === a ? { ...node, id: b } : node.id === b ? { ...node, id: a } : node;
    return { ...node, children: node.children.map(go) };
  };
  return go(root);
}

/** Drag-and-drop: moves `dragId` to a side of `targetId`, or swaps them. */
export function movePane(root: LayoutNode, dragId: string, targetId: string, zone: DropZone): LayoutNode {
  if (dragId === targetId) return root;
  if (zone === "center") return swapPanes(root, dragId, targetId);
  const without = removePane(root, dragId);
  if (!without) return root;
  const dir: Dir = zone === "left" || zone === "right" ? "row" : "col";
  return insertAt(without, targetId, dir, dragId, zone === "left" || zone === "top");
}

const leaf = (id: string): LayoutNode => ({ kind: "pane", id });
const group = (dir: Dir, children: LayoutNode[]): LayoutNode =>
  children.length === 1 ? children[0] : { kind: "split", id: newId("s"), dir, children };

/** Rebuilds the tree from a list of panes in one of the preset shapes. */
export function arrange(ids: string[], how: Arrangement): LayoutNode {
  if (ids.length === 0) throw new Error("nothing to arrange");
  if (ids.length === 1) return leaf(ids[0]);
  switch (how) {
    case "columns":
      return group("row", ids.map(leaf));
    case "rows":
      return group("col", ids.map(leaf));
    case "main":
      return group("row", [leaf(ids[0]), group("col", ids.slice(1).map(leaf))]);
    case "grid": {
      const cols = Math.ceil(Math.sqrt(ids.length));
      const rows: LayoutNode[] = [];
      for (let i = 0; i < ids.length; i += cols) rows.push(group("row", ids.slice(i, i + cols).map(leaf)));
      return group("col", rows);
    }
  }
}

export function paneIds(root: LayoutNode): string[] {
  return root.kind === "pane" ? [root.id] : root.children.flatMap(paneIds);
}
