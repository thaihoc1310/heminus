import { describe, expect, it } from "vitest";
import {
  TerminalMouseTracking,
  encodeWheelReports,
  wheelReportPosition
} from "./terminalWheel";

describe("application wheel reports", () => {
  it("keeps the wheel off until the app asks for button tracking", () => {
    const tracking = new TerminalMouseTracking();
    tracking.setPrivateMode(1006, true);
    expect(tracking.reportsWheel).toBe(false);
    tracking.setPrivateMode(9, true);
    expect(tracking.reportsWheel).toBe(false);
    tracking.setPrivateMode(1000, true);
    expect(tracking.reportsWheel).toBe(true);
    expect(tracking.encoding).toBe("sgr");
    tracking.setPrivateMode(1003, true);
    expect(tracking.protocol).toBe("any");
    tracking.setPrivateMode(1002, false);
    expect(tracking.reportsWheel).toBe(false);
    tracking.setPrivateMode(1000, true);
    tracking.setPrivateMode(1016, true);
    expect(tracking.encoding).toBe("sgr-pixels");
    tracking.reset();
    expect(tracking.reportsWheel).toBe(false);
    expect(tracking.encoding).toBe("default");
  });

  it("turns a slow trackpad swipe into one row per cell, not one row per flick", () => {
    const tracking = new TerminalMouseTracking();
    const cell = 16;
    expect(tracking.consume(5, 0, cell, 40)).toBe(0);
    expect(tracking.consume(5, 0, cell, 40)).toBe(0);
    expect(tracking.consume(6, 0, cell, 40)).toBe(1);
    expect(tracking.consume(48, 0, cell, 40)).toBe(3);
    expect(tracking.consume(-8, 0, cell, 40)).toBe(0);
    expect(tracking.consume(-8, 0, cell, 40)).toBe(-1);
  });

  it("passes line-mode notches through and caps a single huge delta", () => {
    const tracking = new TerminalMouseTracking();
    expect(tracking.consume(3, 1, 16, 40)).toBe(3);
    expect(tracking.consume(1, 2, 16, 40)).toBe(40);
    expect(tracking.consume(10_000, 0, 16, 40)).toBe(40);
  });

  it("encodes one SGR report per row under the pointer", () => {
    const down = encodeWheelReports({
      lines: 2,
      position: { col: 3, row: 4, x: 30, y: 70 },
      encoding: "sgr",
      cols: 80,
      rows: 24,
      ctrl: false,
      alt: true,
      shift: false
    });
    expect(down).toBe("\x1b[<73;4;5M\x1b[<73;4;5M");
    expect(
      encodeWheelReports({
        lines: -1,
        position: { col: 0, row: 0, x: 12, y: 8 },
        encoding: "sgr-pixels",
        cols: 80,
        rows: 24,
        ctrl: true,
        alt: false,
        shift: false
      })
    ).toBe("\x1b[<80;12;8M");
  });

  it("clamps the cell and refuses an X10 coordinate the encoding cannot hold", () => {
    const position = wheelReportPosition(1000, -20, { left: 10, top: 30, width: 200, height: 100 }, 10, 20);
    expect(position).toEqual({ col: 19, row: 0, x: 199, y: 0 });
    expect(
      encodeWheelReports({
        lines: 1,
        position: { col: 300, row: 0, x: 0, y: 0 },
        encoding: "default",
        cols: 400,
        rows: 24,
        ctrl: false,
        alt: false,
        shift: false
      })
    ).toBe("");
  });
});
