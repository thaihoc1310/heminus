import { describe, expect, it } from "vitest";
import { humanFileSize, matchesSearch } from "./format";

describe("humanFileSize", () => {
  it("formats byte boundaries without buffering file contents", () => {
    expect(humanFileSize(null)).toBe("—");
    expect(humanFileSize(512)).toBe("512 B");
    expect(humanFileSize(1536)).toBe("1.5 KB");
    expect(humanFileSize(5 * 1024 ** 2)).toBe("5.0 MB");
  });
});

describe("matchesSearch", () => {
  it("matches trimmed, case-insensitive host metadata", () => {
    expect(matchesSearch(["Production", "deploy", "10.0.0.8"], "  PROD ")).toBe(true);
    expect(matchesSearch(["Production", null], "staging")).toBe(false);
    expect(matchesSearch([], " ")).toBe(true);
  });
});
