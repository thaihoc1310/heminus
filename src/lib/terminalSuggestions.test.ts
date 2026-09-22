import { describe, expect, it } from "vitest";
import {
  buildTerminalSuggestions,
  highlightedCommand,
  reconcileRenderedCommandInput,
  updateCommandInput
} from "./terminalSuggestions";
import type { Snippet } from "./types";

const snippet: Snippet = {
  id: "snippet-1",
  title: "Ping Google",
  command: "ping google.com",
  description: "",
  favorite: true,
  created_at: "2026-01-01T00:00:00Z"
};

describe("terminal suggestions", () => {
  it("places matching snippets before matching command history", () => {
    expect(
      buildTerminalSuggestions("ping", [snippet], ["uname -a", "ping -c 2 worker"])
    ).toEqual([
      { kind: "snippet", command: "ping google.com", detail: "Ping Google" },
      { kind: "history", command: "ping -c 2 worker", detail: "History" }
    ]);
  });

  it("waits for the configured minimum number of typed characters", () => {
    expect(buildTerminalSuggestions("p", [snippet], ["pwd"])).toEqual([]);
    expect(buildTerminalSuggestions("pi", [snippet], ["pwd"])).toEqual([
      { kind: "snippet", command: "ping google.com", detail: "Ping Google" }
    ]);
    expect(buildTerminalSuggestions("p", [snippet], ["pwd"], 8, 1)).toEqual([
      { kind: "snippet", command: "ping google.com", detail: "Ping Google" },
      { kind: "history", command: "pwd", detail: "History" }
    ]);
  });

  it("tracks typed commands, backspace, and submissions", () => {
    expect(updateCommandInput("ping go", "\x7foogle.com\r")).toEqual({
      input: "",
      submitted: ["ping google.com"]
    });
  });

  it("ignores SGR mouse reports from terminal applications", () => {
    expect(updateCommandInput("snip", "\x1b[<35;42;9M")).toEqual({
      input: "snip",
      submitted: []
    });
    expect(updateCommandInput("snip", "\x1b[<0;42;9m")).toEqual({
      input: "snip",
      submitted: []
    });
  });

  it("ignores application-mode arrows and Alt+key sequences", () => {
    expect(updateCommandInput("snip", "\x1bOA\x1bOD\x1bb\x1b")).toEqual({
      input: "snip",
      submitted: []
    });
  });

  it("uses the command rendered by shell completion before saving history", () => {
    expect(reconcileRenderedCommandInput("cd Wor", "cd Workspace/")).toBe("cd Workspace/");
    expect(reconcileRenderedCommandInput("printf complete", "printf com")).toBe("printf complete");
  });

  it("splits a command into highlighted segments", () => {
    expect(highlightedCommand("sudo ping google.com", "ping")).toEqual([
      { text: "sudo ", match: false },
      { text: "ping", match: true },
      { text: " google.com", match: false }
    ]);
  });
});
