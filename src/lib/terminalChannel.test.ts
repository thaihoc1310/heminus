import { describe, expect, it } from "vitest";
import { decodeTerminalMessage } from "./terminalChannel";

describe("decodeTerminalMessage", () => {
  it("reads raw output without copying it through JSON", () => {
    const bytes = new Uint8Array([0x1b, 0x5b, 0x30, 0x6d, 0x68, 0x69]);
    const decoded = decodeTerminalMessage(bytes.buffer);

    expect(decoded.kind).toBe("output");
    if (decoded.kind !== "output") throw new Error("expected output");
    expect([...decoded.data]).toEqual([...bytes]);
  });

  it("passes control events through unchanged", () => {
    const exit = { kind: "exit" } as const;
    expect(decodeTerminalMessage(exit)).toBe(exit);

    const log = {
      kind: "log",
      hop: 1,
      level: "error",
      message: "Connection refused",
      stage: "failed"
    } as const;
    expect(decodeTerminalMessage(log)).toBe(log);
  });

  it("treats an empty buffer as output rather than a control event", () => {
    // The pane only calls markConnected() when a chunk is non-empty, so this
    // must still land on the output branch to keep that check meaningful.
    const decoded = decodeTerminalMessage(new ArrayBuffer(0));
    expect(decoded.kind).toBe("output");
    if (decoded.kind !== "output") throw new Error("expected output");
    expect(decoded.data).toHaveLength(0);
  });
});
