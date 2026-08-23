import { describe, expect, it } from "vitest";
import {
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
