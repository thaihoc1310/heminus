// @vitest-environment jsdom
import { cleanup, fireEvent, render, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "./App.svelte";
import * as ipc from "./lib/ipc";

vi.mock("./lib/ipc");
vi.mock("./lib/dragPreview");
vi.mock("./features/terminal/TerminalPane.svelte", () => ({ default: () => {} }));
vi.hoisted(() => {
  window.history.replaceState(null, "", "/?detached=test");
  Object.defineProperty(navigator, "userAgent", { value: "Linux", configurable: true });
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  window.PointerEvent = class extends MouseEvent {
    pointerId: number;
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 1;
    }
  } as typeof PointerEvent;
  HTMLElement.prototype.setPointerCapture = () => {};
  HTMLElement.prototype.hasPointerCapture = () => false;
  document.elementsFromPoint = () => [];
});
afterEach(cleanup);

beforeEach(() => {
  vi.clearAllMocks();
  for (const listen of [ipc.listenForIdentityChanges, ipc.listenForSnippetChanges,
    ipc.listenForCommandHistoryChanges, ipc.listenForTerminalTabTransfers,
    ipc.listenForNativeTerminalTabDrags]) {
    vi.mocked(listen).mockResolvedValue(() => {});
  }
  vi.mocked(ipc.listHosts).mockResolvedValue([]);
  vi.mocked(ipc.listIdentities).mockResolvedValue([]);
  vi.mocked(ipc.listGroups).mockResolvedValue([]);
  vi.mocked(ipc.terminalProcesses).mockResolvedValue([]);
  vi.mocked(ipc.takeDetachedTerminalPayload).mockResolvedValue({
    title: "Workspace",
    tabs: ["a", "b"].map((id) => ({
      id, title: id, hostId: null, sessionId: `session-${id}`,
      appearance: { theme: "heminus_dark", fontSize: 14 }
    })),
    workspace: { name: "Workspace", paneIds: ["a", "b"], layout: null, activePaneId: "a" }
  });
  vi.mocked(ipc.takeTerminalTabDrop).mockResolvedValue(null);
  vi.mocked(ipc.recordTerminalTabLanding).mockResolvedValue();
});

function dragData() {
  return {
    dropEffect: "none",
    effectAllowed: "all",
    files: [],
    items: [],
    types: [] as string[],
    clearData() {},
    getData() {
      return "";
    },
    setData() {},
    setDragImage() {}
  };
}

it("toggles terminal focus with F11 without closing the workspace", async () => {
  const view = render(App);
  await view.findByRole("group", { name: "Workspace terminal tab" });
  const app = view.container.querySelector(".application")!;
  expect(view.container.querySelectorAll(".terminal-instance")).toHaveLength(2);
  expect(app.classList.contains("terminal-focus")).toBe(false);
  await fireEvent.click(view.getByRole("button", { name: "Hide header (F11)" }));
  await waitFor(() => expect(app.classList.contains("terminal-focus")).toBe(true));
  expect(view.container.querySelectorAll(".terminal-instance")).toHaveLength(2);
  await fireEvent.click(view.getByRole("button", { name: "Show header (F11)" }));
  await waitFor(() => expect(app.classList.contains("terminal-focus")).toBe(false));
  await fireEvent.keyDown(window, { key: "F11" });
  await waitFor(() => expect(app.classList.contains("terminal-focus")).toBe(true));
  await fireEvent.keyDown(window, { key: "F11" });
  await waitFor(() => expect(app.classList.contains("terminal-focus")).toBe(false));
  expect(ipc.closeTerminal).not.toHaveBeenCalled();
});

it("detaches the workspace from its context menu with both panes", async () => {
  const view = render(App);
  const workspace = await view.findByRole("group", { name: "Workspace terminal tab" });
  await fireEvent.contextMenu(workspace);
  await fireEvent.click(view.getByRole("menuitem", { name: "Detach" }));
  await waitFor(() => expect(ipc.createDetachedTerminalWindow).toHaveBeenCalledOnce());
  expect(vi.mocked(ipc.createDetachedTerminalWindow).mock.calls[0][0].workspace?.paneIds)
    .toEqual(["a", "b"]);
  expect(vi.mocked(ipc.createDetachedTerminalWindow).mock.calls[0][0].tabs.map((tab) => tab.id))
    .toEqual(["a", "b"]);
  expect(vi.mocked(ipc.createDetachedTerminalWindow).mock.calls[0][0].tabs.map((tab) => tab.sessionId))
    .toEqual(["session-a", "session-b"]);
  await waitFor(() => expect(view.queryByRole("group", { name: "Workspace terminal tab" })).toBeNull());
});

it("keeps the workspace when creating the detached window fails", async () => {
  vi.mocked(ipc.createDetachedTerminalWindow).mockRejectedValueOnce(new Error("Window failed"));
  const view = render(App);
  const workspace = await view.findByRole("group", { name: "Workspace terminal tab" });
  await fireEvent.contextMenu(workspace);
  await fireEvent.click(view.getByRole("menuitem", { name: "Detach" }));
  await view.findByText("Window failed");
  expect(view.getByRole("group", { name: "Workspace terminal tab" })).toBeTruthy();
  expect(ipc.closeTerminal).not.toHaveBeenCalled();
});

it.each([true, false])("uses the GTK drag result when Linux has no screen coordinates (outside=%s)", async (outside) => {
  vi.mocked(ipc.takeTerminalTabDrop).mockResolvedValueOnce({ kind: outside ? "outside" : "cancelled" });
  const view = render(App);
  const workspace = await view.findByRole("group", { name: "Workspace terminal tab" });
  const dataTransfer = dragData();
  await fireEvent.dragStart(workspace, { dataTransfer, clientX: 20, clientY: 20, screenX: 0, screenY: 0 });
  await fireEvent.dragEnd(workspace, { dataTransfer, clientX: 0, clientY: 0, screenX: 0, screenY: 0 });
  if (outside) {
    await waitFor(() => expect(ipc.createDetachedTerminalWindow).toHaveBeenCalledOnce());
  } else {
    expect(ipc.createDetachedTerminalWindow).not.toHaveBeenCalled();
    expect(view.getByRole("group", { name: "Workspace terminal tab" })).toBeTruthy();
  }
  expect(ipc.transferTerminalTab).not.toHaveBeenCalled();
});

it("does not detach a cancelled Linux tab drag", async () => {
  vi.mocked(ipc.takeTerminalTabDrop).mockResolvedValueOnce({ kind: "cancelled" });
  const view = render(App);
  const workspace = await view.findByRole("group", { name: "Workspace terminal tab" });
  const dataTransfer = dragData();
  await fireEvent.dragStart(workspace, { dataTransfer, clientX: 20, clientY: 20, screenX: 0, screenY: 0 });
  await fireEvent.dragEnd(workspace, { dataTransfer, clientX: 0, clientY: 0, screenX: 0, screenY: 0 });
  expect(ipc.createDetachedTerminalWindow).not.toHaveBeenCalled();
});

it("moves a tab dropped on another window there instead of detaching it", async () => {
  vi.mocked(ipc.takeTerminalTabDrop)
    .mockResolvedValueOnce(null)
    .mockResolvedValueOnce({ kind: "landed", targetLabel: "main", clientX: 40, clientY: 12 });
  const view = render(App);
  const workspace = await view.findByRole("group", { name: "Workspace terminal tab" });
  const dataTransfer = dragData();
  await fireEvent.dragStart(workspace, { dataTransfer, clientX: 20, clientY: 20, screenX: 0, screenY: 0 });
  await fireEvent.dragEnd(workspace, { dataTransfer, clientX: 0, clientY: 0, screenX: 0, screenY: 0 });
  await waitFor(() => expect(ipc.transferTerminalTabTo).toHaveBeenCalledOnce());
  const [label, payload, x, y] = vi.mocked(ipc.transferTerminalTabTo).mock.calls[0];
  expect([label, x, y]).toEqual(["main", 40, 12]);
  expect(payload.tabs.map((tab) => tab.sessionId)).toEqual(["session-a", "session-b"]);
  expect(ipc.createDetachedTerminalWindow).not.toHaveBeenCalled();
  await waitFor(() => expect(view.queryByRole("group", { name: "Workspace terminal tab" })).toBeNull());
});

it("accepts a tab dragged in from another window and reports the drop", async () => {
  const view = render(App);
  await view.findByRole("group", { name: "Workspace terminal tab" });
  const dataTransfer = { ...dragData(), types: ["application/x-heminus-terminal-tab"] };
  const over = new Event("dragover", { cancelable: true, bubbles: true });
  Object.assign(over, { dataTransfer, clientX: 5, clientY: 6 });
  window.dispatchEvent(over);
  expect(over.defaultPrevented).toBe(true);
  const drop = new Event("drop", { cancelable: true, bubbles: true });
  Object.assign(drop, { dataTransfer, clientX: 5, clientY: 6 });
  window.dispatchEvent(drop);
  await waitFor(() => expect(ipc.recordTerminalTabLanding).toHaveBeenCalledWith(5, 6));
});

it("stops idle shells before the window closes", async () => {
  const view = render(App);
  await view.findByRole("group", { name: "Workspace terminal tab" });
  await fireEvent.click(view.getByTitle("Close"));
  await waitFor(() => expect(ipc.closeTerminal).toHaveBeenCalledWith("session-a"));
  expect(ipc.closeTerminal).toHaveBeenCalledWith("session-b");
});

it("asks about every running process when the window is closed", async () => {
  vi.mocked(ipc.terminalProcesses).mockImplementation(async (id) => (
    id === "session-a"
      ? [{ pid: 42, name: "sleep", command: "sleep 100", leader: false }]
      : id === "session-b"
        ? [{ pid: 43, name: "sleep", command: "sleep 200", leader: false }]
        : []
  ));
  const view = render(App);
  await view.findByRole("group", { name: "Workspace terminal tab" });
  await fireEvent.click(view.getByTitle("Close"));
  expect(await view.findByRole("heading", { name: "Close this window?" })).toBeTruthy();
  expect(view.getByText("sleep 100")).toBeTruthy();
  expect(view.getByText("sleep 200")).toBeTruthy();
  expect(view.queryByRole("button", { name: "Keep some running…" })).toBeNull();
  await fireEvent.click(view.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(view.queryByRole("heading", { name: "Close this window?" })).toBeNull());
  expect(view.getByRole("group", { name: "Workspace terminal tab" })).toBeTruthy();
  expect(ipc.closeTerminal).not.toHaveBeenCalled();
});

it("kills every running process when the window is closed and confirmed", async () => {
  vi.mocked(ipc.terminalProcesses).mockImplementation(async (id) => (
    id === "session-a"
      ? [{ pid: 42, name: "sleep", command: "sleep 100", leader: false }]
      : id === "session-b"
        ? [{ pid: 43, name: "sleep", command: "sleep 200", leader: false }]
        : []
  ));
  const view = render(App);
  await view.findByRole("group", { name: "Workspace terminal tab" });
  await fireEvent.click(view.getByTitle("Close"));
  await fireEvent.click(await view.findByRole("button", { name: "Stop & close" }));
  await waitFor(() => expect(ipc.closeTerminal).toHaveBeenCalledWith("session-a", true));
  await waitFor(() => expect(ipc.closeTerminal).toHaveBeenCalledWith("session-b", true));
  await waitFor(() => expect(view.queryByRole("group", { name: "Workspace terminal tab" })).toBeNull());
});
