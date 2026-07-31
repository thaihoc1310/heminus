import { describe, expect, it } from "vitest";
import { terminalTheme, terminalThemes } from "./terminalThemes";

const ansiColors = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite"
] as const;

describe("terminal themes", () => {
  it("defines a complete xterm palette and matching pane chrome for every theme", () => {
    expect(terminalThemes).toHaveLength(19);
    expect(new Set(terminalThemes.map((theme) => theme.id)).size).toBe(terminalThemes.length);
    for (const theme of terminalThemes) {
      expect(theme.palette.background).toMatch(/^#[0-9a-f]{6}$/i);
      expect(theme.palette.foreground).toMatch(/^#[0-9a-f]{6}$/i);
      expect(theme.palette.cursor).toMatch(/^#[0-9a-f]{6}$/i);
      expect(theme.palette.cursorAccent).toBe(theme.palette.background);
      expect(theme.palette.scrollbarSliderBackground).not.toBe("");
      expect(theme.palette.scrollbarSliderHoverBackground).not.toBe("");
      expect(theme.palette.scrollbarSliderActiveBackground).not.toBe("");
      for (const color of ansiColors) {
        expect(theme.palette[color], `${theme.id}.${color}`).toMatch(/^#[0-9a-f]{6}$/i);
      }
      expect(theme.chrome.headerBackground).not.toBe("");
      expect(theme.chrome.headerForeground).not.toBe("");
      expect(theme.chrome.headerMuted).not.toBe("");
      expect(theme.chrome.headerBackground).toBe(theme.palette.background);
      expect(theme.chrome.headerForeground).toBe(theme.palette.foreground);
    }
  });

  it("uses a neutral foreground for the default theme", () => {
    const theme = terminalTheme(undefined);
    expect(theme.id).toBe("heminus_dark");
    expect(theme.palette.foreground).toBe("#c9d1d9");
    expect(theme.chrome.headerForeground).toBe("#c9d1d9");
  });

  it("keeps light theme chrome light instead of reusing dark chrome", () => {
    const theme = terminalTheme("paper_light");
    expect(theme.chrome.headerBackground).toBe("#f7f3e8");
    expect(theme.chrome.headerForeground).toBe("#282b32");
  });
});
