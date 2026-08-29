import { describe, expect, it } from "vitest";
import {
  drainTerminalInput,
  normalizeLinuxTerminalInput,
  resetLinuxTerminalComposition
} from "./terminalInput";

describe("normalizeLinuxTerminalInput", () => {
  it("converts WebKitGTK IME non-breaking spaces to shell separators", () => {
    expect(normalizeLinuxTerminalInput("rm\u00a0-rf\u00a0test.py")).toBe("rm -rf test.py");
  });

  it("leaves ordinary terminal input unchanged", () => {
    expect(normalizeLinuxTerminalInput("rm -rf test.py")).toBe("rm -rf test.py");
  });

  it("preserves non-breaking spaces in bracketed paste", () => {
    const paste = "\x1b[200~hello\u00a0world\x1b[201~";
    expect(normalizeLinuxTerminalInput(paste)).toBe(paste);
  });

  it("clears committed textarea residue before a new IME composition", () => {
    const textarea = { value: "ls -ls " };
    resetLinuxTerminalComposition(textarea);
    expect(textarea.value).toBe("");
  });
});

describe("drainTerminalInput", () => {
  it("joins queued chunks in order", () => {
    const encoder = new TextEncoder();
    const chunks = [encoder.encode("echo "), encoder.encode("hi"), encoder.encode("\r")];
    expect(drainTerminalInput(chunks)).toEqual([...encoder.encode("echo hi\r")]);
  });

  it("returns an empty payload for an empty queue", () => {
    expect(drainTerminalInput([])).toEqual([]);
  });

  it("handles a paste far larger than the call-argument limit", () => {
    // `queue.push(...bytes)` used to throw RangeError past ~128k arguments,
    // which lost the whole paste from inside xterm's onData handler.
    const paste = new TextEncoder().encode("x".repeat(1_000_000));
    const bytes = drainTerminalInput([paste]);
    expect(bytes).toHaveLength(1_000_000);
    expect(bytes[0]).toBe(paste[0]);
    expect(bytes[bytes.length - 1]).toBe(paste[paste.length - 1]);
  });
});
