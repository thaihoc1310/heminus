import { expect, it } from "vitest";
import { trimSelectionLineEnds } from "./terminalSelection";

it("drops padding at line ends and keeps everything else", () => {
  expect(trimSelectionLineEnds("ls -la      \nREADME.md   \t\n")).toBe("ls -la\nREADME.md\n");
  expect(trimSelectionLineEnds("a  b   \r\nc   ")).toBe("a  b\r\nc");
  expect(trimSelectionLineEnds("  indented\n\n    \nlast")).toBe("  indented\n\n\nlast");
  expect(trimSelectionLineEnds("no padding")).toBe("no padding");
});
