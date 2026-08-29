import type { SplitDirection, WorkspaceLayout } from "./types";

export type DropZone = "left" | "right" | "top" | "bottom";

export interface PaneRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface SplitDivider {
  path: number[];
  direction: SplitDirection;
  parent: PaneRect;
  position: number;
}

export function normalizeLayout(
  layout: WorkspaceLayout | null,
  paneIds: string[]
): WorkspaceLayout | null {
  const allowed = new Set(paneIds);
  const next = prune(layout, allowed);
  const present = new Set(next ? leafOrder(next) : []);
  const missing: string[] = [];
  for (const paneId of paneIds) {
    // Tracked as they are added: a duplicate id in `paneIds` would otherwise
    // produce two leaves for the same pane.
    if (present.has(paneId)) continue;
    present.add(paneId);
    missing.push(paneId);
  }
  if (missing.length === 0) return next;

  // Each missing pane used to be grafted on as `{existing, new}` at ratio 0.5,
  // which halved everything already there once per pane — restoring four panes
  // left the original layout at a sixteenth of the workspace. Giving the new
  // panes a shrinking share keeps every pane's area proportional instead.
  let grafted = next;
  let total = next ? leafOrder(next).length : 0;
  for (const paneId of missing) {
    const leaf: WorkspaceLayout = { type: "pane", pane_id: paneId };
    if (!grafted) {
      grafted = leaf;
      total = 1;
      continue;
    }
    total += 1;
    // The subtree already holds `total - 1` panes, so giving it that share of
    // the split leaves every pane the same width.
    grafted = {
      type: "split",
      direction: "horizontal",
      ratio: (total - 1) / total,
      first: grafted,
      second: leaf
    };
  }
  return grafted;
}

export function splitPane(
  layout: WorkspaceLayout | null,
  paneId: string,
  targetId: string | null,
  zone: DropZone
): WorkspaceLayout {
  const withoutPane = removePane(layout, paneId);
  const leaf: WorkspaceLayout = { type: "pane", pane_id: paneId };
  const appended = (base: WorkspaceLayout | null): WorkspaceLayout =>
    base ? { type: "split", direction: "horizontal", first: base, second: leaf } : leaf;
  if (!withoutPane || !targetId) return appended(withoutPane);
  // A target that is not in the tree cannot be split against. Appending keeps
  // the pane in the layout; returning the tree unchanged would have dropped it.
  if (!leafOrder(withoutPane).includes(targetId)) return appended(withoutPane);
  const direction: SplitDirection =
    zone === "left" || zone === "right" ? "horizontal" : "vertical";
  const before = zone === "left" || zone === "top";
  return replacePane(withoutPane, targetId, {
    type: "split",
    direction,
    ratio: 0.5,
    first: before ? leaf : { type: "pane", pane_id: targetId },
    second: before ? { type: "pane", pane_id: targetId } : leaf
  });
}

export function removePane(
  layout: WorkspaceLayout | null,
  paneId: string
): WorkspaceLayout | null {
  if (!layout) return null;
  if (layout.type === "pane") return layout.pane_id === paneId ? null : layout;
  const first = removePane(layout.first, paneId);
  const second = removePane(layout.second, paneId);
  if (!first) return second;
  if (!second) return first;
  return { ...layout, first, second };
}

export function leafOrder(layout: WorkspaceLayout): string[] {
  return layout.type === "pane"
    ? [layout.pane_id]
    : [...leafOrder(layout.first), ...leafOrder(layout.second)];
}

export function layoutRects(layout: WorkspaceLayout | null): Record<string, PaneRect> {
  const result: Record<string, PaneRect> = {};
  if (layout) collectRects(layout, { left: 0, top: 0, width: 100, height: 100 }, result);
  return result;
}

export function layoutDividers(layout: WorkspaceLayout | null): SplitDivider[] {
  const result: SplitDivider[] = [];
  if (layout) {
    collectDividers(
      layout,
      { left: 0, top: 0, width: 100, height: 100 },
      [],
      result
    );
  }
  return result;
}

export function setSplitRatio(
  layout: WorkspaceLayout | null,
  path: number[],
  ratio: number
): WorkspaceLayout | null {
  if (!layout || layout.type === "pane") return layout;
  if (path.length === 0) {
    return { ...layout, ratio: clampRatio(ratio) };
  }
  const [branch, ...remaining] = path;
  // Only 0 and 1 address a real branch; anything else is a malformed path and
  // must not silently resize the second subtree.
  if (branch !== 0 && branch !== 1) return layout;
  return branch === 0
    ? { ...layout, first: setSplitRatio(layout.first, remaining, ratio) ?? layout.first }
    : { ...layout, second: setSplitRatio(layout.second, remaining, ratio) ?? layout.second };
}

function collectRects(
  layout: WorkspaceLayout,
  rect: PaneRect,
  result: Record<string, PaneRect>
) {
  if (layout.type === "pane") {
    result[layout.pane_id] = rect;
    return;
  }
  const ratio = splitRatio(layout);
  if (layout.direction === "horizontal") {
    const firstWidth = rect.width * ratio;
    collectRects(layout.first, { ...rect, width: firstWidth }, result);
    collectRects(
      layout.second,
      { ...rect, left: rect.left + firstWidth, width: rect.width - firstWidth },
      result
    );
  } else {
    const firstHeight = rect.height * ratio;
    collectRects(layout.first, { ...rect, height: firstHeight }, result);
    collectRects(
      layout.second,
      { ...rect, top: rect.top + firstHeight, height: rect.height - firstHeight },
      result
    );
  }
}

function collectDividers(
  layout: WorkspaceLayout,
  rect: PaneRect,
  path: number[],
  result: SplitDivider[]
) {
  if (layout.type === "pane") return;
  const ratio = splitRatio(layout);
  const position =
    layout.direction === "horizontal"
      ? rect.left + rect.width * ratio
      : rect.top + rect.height * ratio;
  result.push({ path, direction: layout.direction, parent: rect, position });
  if (layout.direction === "horizontal") {
    const firstWidth = rect.width * ratio;
    collectDividers(layout.first, { ...rect, width: firstWidth }, [...path, 0], result);
    collectDividers(
      layout.second,
      { ...rect, left: rect.left + firstWidth, width: rect.width - firstWidth },
      [...path, 1],
      result
    );
  } else {
    const firstHeight = rect.height * ratio;
    collectDividers(layout.first, { ...rect, height: firstHeight }, [...path, 0], result);
    collectDividers(
      layout.second,
      { ...rect, top: rect.top + firstHeight, height: rect.height - firstHeight },
      [...path, 1],
      result
    );
  }
}

function splitRatio(layout: Extract<WorkspaceLayout, { type: "split" }>): number {
  return clampRatio(layout.ratio ?? 0.5);
}

function clampRatio(ratio: number): number {
  return Math.max(0.1, Math.min(0.9, Number.isFinite(ratio) ? ratio : 0.5));
}

function replacePane(
  layout: WorkspaceLayout,
  paneId: string,
  replacement: WorkspaceLayout
): WorkspaceLayout {
  if (layout.type === "pane") return layout.pane_id === paneId ? replacement : layout;
  return {
    ...layout,
    first: replacePane(layout.first, paneId, replacement),
    second: replacePane(layout.second, paneId, replacement)
  };
}

function prune(
  layout: WorkspaceLayout | null,
  allowed: Set<string>
): WorkspaceLayout | null {
  if (!layout) return null;
  if (layout.type === "pane") return allowed.has(layout.pane_id) ? layout : null;
  const first = prune(layout.first, allowed);
  const second = prune(layout.second, allowed);
  if (!first) return second;
  if (!second) return first;
  return { ...layout, first, second };
}
