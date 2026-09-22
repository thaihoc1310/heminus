import type { TerminalSnapshot } from "./types";

// Only live panes register here. Snapshotting is requested before their window
// is created, so the old renderer is still available to drain and serialize.
export const terminalTransfers = new Map<string, {
  prepare: () => Promise<TerminalSnapshot>;
  resume: () => Promise<void>;
}>();

/**
 * Detaches still in flight, by session. A pane remounted in the same window
 * must attach after them, or the old pane's late detach cuts off the new one.
 */
export const pendingDetaches = new Map<string, Promise<unknown>>();
