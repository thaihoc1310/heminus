import { describe, expect, it } from "vitest";
import {
  formatTerminalShortcut,
  matchesTerminalShortcut,
  shortcutFromKeyboardEvent
} from "./terminalPreferences";

function shortcutEvent(
  key: string,
  modifiers: Partial<Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey">> = {}
) {
  return {
    key,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...modifiers
  };
}

describe("terminal preferences shortcuts", () => {
  it("normalizes configurable keyboard shortcuts", () => {
    const event = shortcutEvent("h", { ctrlKey: true, shiftKey: true });
    expect(shortcutFromKeyboardEvent(event)).toBe("Ctrl+Shift+H");
    expect(formatTerminalShortcut("Ctrl+Shift+H")).toBe("Ctrl Shift H");
  });

  it("requires a system modifier and matches every modifier exactly", () => {
    expect(shortcutFromKeyboardEvent(shortcutEvent("h", { shiftKey: true }))).toBeNull();
    expect(
      matchesTerminalShortcut(
        shortcutEvent("h", { ctrlKey: true, shiftKey: true }),
        "Ctrl+Shift+H"
      )
    ).toBe(true);
    expect(
      matchesTerminalShortcut(
        shortcutEvent("h", { ctrlKey: true, altKey: true, shiftKey: true }),
        "Ctrl+Shift+H"
      )
    ).toBe(false);
  });
});
