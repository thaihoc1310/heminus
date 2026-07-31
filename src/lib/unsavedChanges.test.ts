import { describe, expect, it } from "vitest";
import { draftChanged, draftSnapshot } from "./unsavedChanges";

describe("unsaved changes", () => {
  it("detects nested draft edits", () => {
    const baseline = draftSnapshot({ label: "Server", tags: ["prod"] });
    expect(draftChanged(baseline, { label: "Server", tags: ["prod"] })).toBe(false);
    expect(draftChanged(baseline, { label: "Server", tags: ["prod", "ssh"] })).toBe(true);
  });

  it("does not mark an editor dirty before a baseline exists", () => {
    expect(draftChanged(null, { label: "Server" })).toBe(false);
  });
});
