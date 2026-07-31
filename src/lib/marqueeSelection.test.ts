import { describe, expect, it } from "vitest";
import { rectanglesIntersect, selectionRect } from "./marqueeSelection";

describe("marquee selection geometry", () => {
  it("normalizes a rectangle dragged in any direction", () => {
    expect(selectionRect(80, 60, 20, 10)).toEqual({ left: 20, top: 10, right: 80, bottom: 60 });
  });

  it("selects intersecting cards but not separated cards", () => {
    const marquee = { left: 10, top: 10, right: 50, bottom: 50 };
    expect(rectanglesIntersect(marquee, { left: 40, top: 40, right: 80, bottom: 80 })).toBe(true);
    expect(rectanglesIntersect(marquee, { left: 60, top: 60, right: 80, bottom: 80 })).toBe(false);
  });
});
