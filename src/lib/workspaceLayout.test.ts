import { describe, expect, it } from "vitest";
import {
  layoutDividers,
  layoutRects,
  leafOrder,
  normalizeLayout,
  removePane,
  setSplitRatio,
  splitPane
} from "./workspaceLayout";

describe("workspace layout", () => {
  it("creates nested directional splits and stable rectangles", () => {
    let layout = splitPane(null, "a", null, "right");
    layout = splitPane(layout, "b", "a", "right");
    layout = splitPane(layout, "c", "b", "bottom");

    expect(leafOrder(layout)).toEqual(["a", "b", "c"]);
    expect(layoutRects(layout)).toEqual({
      a: { left: 0, top: 0, width: 50, height: 100 },
      b: { left: 50, top: 0, width: 50, height: 50 },
      c: { left: 50, top: 50, width: 50, height: 50 }
    });
  });

  it("collapses removed panes and restores missing panes", () => {
    let layout = splitPane(null, "a", null, "right");
    layout = splitPane(layout, "b", "a", "right");
    expect(removePane(layout, "a")).toEqual({ type: "pane", pane_id: "b" });
    expect(leafOrder(normalizeLayout(null, ["a", "b"])!)).toEqual(["a", "b"]);
  });

  it("resizes nested splits without changing pane order", () => {
    let layout = splitPane(null, "a", null, "right");
    layout = splitPane(layout, "b", "a", "right");
    layout = splitPane(layout, "c", "b", "bottom");
    layout = setSplitRatio(layout, [], 0.3)!;
    layout = setSplitRatio(layout, [1], 0.6)!;

    expect(leafOrder(layout)).toEqual(["a", "b", "c"]);
    expect(layoutRects(layout)).toEqual({
      a: { left: 0, top: 0, width: 30, height: 100 },
      b: { left: 30, top: 0, width: 70, height: 60 },
      c: { left: 30, top: 60, width: 70, height: 40 }
    });
    expect(layoutDividers(layout)).toEqual([
      {
        path: [],
        direction: "horizontal",
        parent: { left: 0, top: 0, width: 100, height: 100 },
        position: 30
      },
      {
        path: [1],
        direction: "vertical",
        parent: { left: 30, top: 0, width: 70, height: 100 },
        position: 60
      }
    ]);
  });

  it("clamps divider ratios so panes stay usable", () => {
    let layout = splitPane(splitPane(null, "a", null, "right"), "b", "a", "right");
    layout = setSplitRatio(layout, [], 2)!;
    expect(layoutRects(layout).a.width).toBe(90);
    layout = setSplitRatio(layout, [], -1)!;
    expect(layoutRects(layout).a.width).toBe(10);
  });
});
