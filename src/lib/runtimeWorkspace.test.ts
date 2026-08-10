import { describe, expect, it } from "vitest";
import {
  findWorkspaceForPane,
  workspaceIdFromTabId,
  workspaceTabId,
  type RuntimeWorkspace
} from "./runtimeWorkspace";

function workspace(id: string, paneIds: string[]): RuntimeWorkspace {
  return {
    id,
    name: `Workspace ${id}`,
    paneIds,
    layout: null,
    activePaneId: paneIds[0] ?? null,
    focusedPaneId: null,
    broadcastPaneIds: []
  };
}

describe("runtime workspaces", () => {
  it("uses a distinct top-tab id for every workspace", () => {
    expect(workspaceTabId("one")).toBe("workspace:one");
    expect(workspaceTabId("two")).not.toBe(workspaceTabId("one"));
    expect(workspaceIdFromTabId(workspaceTabId("two"))).toBe("two");
    expect(workspaceIdFromTabId("terminal-id")).toBeNull();
  });

  it("finds pane ownership across multiple workspaces", () => {
    const workspaces = [workspace("one", ["a", "b"]), workspace("two", ["c", "d"])];
    expect(findWorkspaceForPane(workspaces, "d")?.id).toBe("two");
    expect(findWorkspaceForPane(workspaces, "missing")).toBeNull();
  });
});
