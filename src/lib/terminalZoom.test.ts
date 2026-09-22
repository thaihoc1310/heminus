import { describe, expect, it } from "vitest";
import {
  DEFAULT_TERMINAL_FONT_SIZE,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  clampTerminalFontSize,
  nextTerminalFontSize,
  terminalZoomActionFromKeyboard,
  terminalZoomActionFromWheel
} from "./terminalZoom";

function keyEvent(
  key: string,
  extras: Partial<Pick<KeyboardEvent, "code" | "ctrlKey" | "metaKey" | "altKey">> = {}
) {
  return {
    key,
    code: extras.code ?? "",
    ctrlKey: extras.ctrlKey ?? false,
    metaKey: extras.metaKey ?? false,
    altKey: extras.altKey ?? false
  };
}

describe("terminal font zoom", () => {
  it("clamps font sizes to the range the host record already accepts", () => {
    expect(clampTerminalFontSize(Number.NaN)).toBe(DEFAULT_TERMINAL_FONT_SIZE);
    expect(clampTerminalFontSize(8)).toBe(MIN_TERMINAL_FONT_SIZE);
    expect(clampTerminalFontSize(40)).toBe(MAX_TERMINAL_FONT_SIZE);
    expect(clampTerminalFontSize(15.4)).toBe(15);
  });

  it("steps one pixel and resets to the default size", () => {
    expect(nextTerminalFontSize(14, "in")).toBe(15);
    expect(nextTerminalFontSize(14, "out")).toBe(13);
    expect(nextTerminalFontSize(9, "out")).toBe(MIN_TERMINAL_FONT_SIZE);
    expect(nextTerminalFontSize(32, "in")).toBe(MAX_TERMINAL_FONT_SIZE);
    expect(nextTerminalFontSize(22, "reset")).toBe(DEFAULT_TERMINAL_FONT_SIZE);
  });

  it("reads Ctrl/Cmd − + 0, including = and the numpad", () => {
    expect(terminalZoomActionFromKeyboard(keyEvent("+", { ctrlKey: true }))).toBe("in");
    expect(terminalZoomActionFromKeyboard(keyEvent("=", { ctrlKey: true }))).toBe("in");
    expect(terminalZoomActionFromKeyboard(keyEvent("Add", { ctrlKey: true, code: "NumpadAdd" }))).toBe("in");
    expect(terminalZoomActionFromKeyboard(keyEvent("-", { ctrlKey: true }))).toBe("out");
    expect(terminalZoomActionFromKeyboard(keyEvent("_", { ctrlKey: true }))).toBeNull();
    expect(terminalZoomActionFromKeyboard(keyEvent("0", { metaKey: true }))).toBe("reset");
    expect(terminalZoomActionFromKeyboard(keyEvent("0", { ctrlKey: true, code: "Numpad0" }))).toBe("reset");
    expect(terminalZoomActionFromKeyboard(keyEvent("-", { altKey: true, ctrlKey: true }))).toBeNull();
    expect(terminalZoomActionFromKeyboard(keyEvent("-"))).toBeNull();
  });

  it("zooms from a modifier wheel without treating a flat scroll as a step", () => {
    expect(terminalZoomActionFromWheel({ ctrlKey: true, metaKey: false, deltaY: -80 })).toBe("in");
    expect(terminalZoomActionFromWheel({ ctrlKey: false, metaKey: true, deltaY: 40 })).toBe("out");
    expect(terminalZoomActionFromWheel({ ctrlKey: true, metaKey: false, deltaY: 0 })).toBeNull();
    expect(terminalZoomActionFromWheel({ ctrlKey: false, metaKey: false, deltaY: -80 })).toBeNull();
  });
});
