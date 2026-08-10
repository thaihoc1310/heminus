import type { WorkspaceLayout } from "./types";

export const workspaceTabPrefix = "workspace:";

export interface RuntimeWorkspace {
  id: string;
  name: string;
  paneIds: string[];
  layout: WorkspaceLayout | null;
  activePaneId: string | null;
  focusedPaneId: string | null;
  broadcastPaneIds: string[];
}

export function workspaceTabId(workspaceId: string): string {
  return `${workspaceTabPrefix}${workspaceId}`;
}

export function workspaceIdFromTabId(tabId: string): string | null {
  return tabId.startsWith(workspaceTabPrefix)
    ? tabId.slice(workspaceTabPrefix.length) || null
    : null;
}

export function findWorkspaceForPane(
  workspaces: RuntimeWorkspace[],
  paneId: string
): RuntimeWorkspace | null {
  return workspaces.find((workspace) => workspace.paneIds.includes(paneId)) ?? null;
}

