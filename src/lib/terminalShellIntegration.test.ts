import { describe, expect, it } from "vitest";
import { consumeShellCompletionMarkers } from "./terminalShellIntegration";

describe("terminal shell integration", () => {
  it("reads successful and failed command completion markers", () => {
    expect(
      consumeShellCompletionMarkers(
        "",
        "output\u001b]633;D;0\u0007more\u001b]133;D;127\u001b\\"
      )
    ).toEqual({
      remainder: "",
      exitCodes: [0, 127]
    });
  });

  it("keeps a split marker until its terminator arrives", () => {
    const first = consumeShellCompletionMarkers("", "output\u001b]633;D");
    expect(first).toEqual({
      remainder: "\u001b]633;D",
      exitCodes: []
    });
    expect(consumeShellCompletionMarkers(first.remainder, ";0\u0007prompt")).toEqual({
      remainder: "",
      exitCodes: [0]
    });
  });

  it("reports completion without a trustworthy exit code as unknown", () => {
    expect(consumeShellCompletionMarkers("", "\u001b]133;D\u0007")).toEqual({
      remainder: "",
      exitCodes: [null]
    });
  });
});
