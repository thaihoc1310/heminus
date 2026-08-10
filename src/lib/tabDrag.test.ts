import { describe, expect, it } from "vitest";
import { hasPassedTopTabDragThreshold } from "./tabDrag";

describe("top tab dragging", () => {
  it("does not begin on a hold or small pointer movement", () => {
    expect(hasPassedTopTabDragThreshold(20, 30, 20, 30)).toBe(false);
    expect(hasPassedTopTabDragThreshold(20, 30, 25, 34)).toBe(false);
    expect(hasPassedTopTabDragThreshold(20, 30, 28, 30)).toBe(true);
  });
});
