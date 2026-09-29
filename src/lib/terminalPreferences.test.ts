// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import {
  formatTerminalShortcut,
  matchesTerminalShortcut,
  normalizeSuggestionMinimumCharacters,
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

  it("normalizes the minimum suggestion character count", () => {
    expect(normalizeSuggestionMinimumCharacters(undefined)).toBe(2);
    expect(normalizeSuggestionMinimumCharacters(0)).toBe(1);
    expect(normalizeSuggestionMinimumCharacters(4.6)).toBe(5);
    expect(normalizeSuggestionMinimumCharacters(20)).toBe(10);
  });
});

describe("history suggestions shortcut", () => {
  it("moves the old Ctrl+Shift+H default off the pane-focus chord", async () => {
    localStorage.setItem(
      "heminus-terminal-preferences",
      JSON.stringify({ historySuggestionsShortcut: "Ctrl+Shift+H" })
    );
    vi.resetModules();
    const preferences = await import("./terminalPreferences");
    expect(get(preferences.terminalPreferences).historySuggestionsShortcut).toBe("Ctrl+Shift+S");
    localStorage.clear();
  });

  it("refuses chords Heminus already uses and keeps custom ones", async () => {
    vi.resetModules();
    const preferences = await import("./terminalPreferences");
    preferences.setHistorySuggestionsShortcut("Ctrl+Shift+L");
    expect(get(preferences.terminalPreferences).historySuggestionsShortcut).toBe("Ctrl+Shift+S");
    preferences.setHistorySuggestionsShortcut("Ctrl+Alt+Y");
    expect(get(preferences.terminalPreferences).historySuggestionsShortcut).toBe("Ctrl+Alt+Y");
    localStorage.clear();
  });
});
