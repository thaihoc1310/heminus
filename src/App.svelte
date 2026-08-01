<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
  import AppDialog from "./components/AppDialog.svelte";
  import CollectionControls from "./components/CollectionControls.svelte";
  import CustomSelect from "./components/CustomSelect.svelte";
  import Icon from "./components/Icon.svelte";
  import HostIcon from "./components/HostIcon.svelte";
  import TerminalToolsSidebar from "./features/terminal/TerminalToolsSidebar.svelte";
  import { confirmDialog, promptDialog } from "./lib/dialog";
  import {
    setElementDragPreview,
    setNativeTabDragPreview
  } from "./lib/dragPreview";
  import {
    createDetachedTerminalWindow,
    deleteGroup,
    deleteHost,
    listHosts,
    listIdentities,
    listGroups,
    listenForCommandHistoryChanges,
    listenForSnippetChanges,
    listenForTerminalTabTransfers,
    saveHost,
    saveGroup,
    saveIdentity,
    setIdentitySecret,
    takeDetachedTerminalPayload,
    transferTerminalTab,
    renameTerminalSession,
    writeTerminal
  } from "./lib/ipc";
  import type {
    DetachedWindowPayload,
    Host,
    Identity,
    MainPage,
    TerminalAppearance,
    TerminalCommandRequest,
    VaultGroup,
    WorkspaceLayout
  } from "./lib/types";
  import {
    type DropZone,
    type SplitDivider,
    layoutDividers,
    layoutRects,
    leafOrder,
    normalizeLayout,
    removePane as removeLayoutPane,
    setSplitRatio,
    splitPane
  } from "./lib/workspaceLayout";
  import { terminalTheme, terminalThemes } from "./lib/terminalThemes";
  import { parseQuickConnectInput } from "./lib/quickConnect";
  import { beginMarqueeSelection } from "./lib/marqueeSelection";
  import {
    cleanHostTag,
    collectHostTags,
    dedupeHostTags,
    hostHasSelectedTags,
    removeHostTag,
    renameHostTag
  } from "./lib/hostTags";
  import {
    confirmDiscardChanges,
    draftChanged,
    draftSnapshot
  } from "./lib/unsavedChanges";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "./lib/collection";

  const appWindow = "__TAURI_INTERNALS__" in window ? getCurrentWindow() : null;
  const detachedMode = new URLSearchParams(window.location.search).has("detached");
  const vaultPages = new Set<MainPage>([
    "hosts",
    "keychain",
    "forwarding",
    "snippets",
    "known-hosts",
    "logs"
  ]);
  const navigation: Array<{ page: MainPage; label: string; icon: string }> = [
    { page: "hosts", label: "Hosts", icon: "server" },
    { page: "keychain", label: "Keychain", icon: "key" },
    { page: "forwarding", label: "Port Forwarding", icon: "forward" },
    { page: "snippets", label: "Snippets", icon: "code" },
    { page: "known-hosts", label: "Known Hosts", icon: "shield" },
    { page: "logs", label: "Logs", icon: "clock" }
  ];
  interface TerminalTab {
    id: string;
    title: string;
    host: Host | null;
    appearance: TerminalAppearance;
    resumeSessionId: string | null;
  }
  type HostSubeditor = "environment" | "chain" | "theme";
  type WindowResizeDirection =
    | "East"
    | "North"
    | "NorthEast"
    | "NorthWest"
    | "South"
    | "SouthEast"
    | "SouthWest"
    | "West";

  function observeTopTab(node: HTMLElement) {
    const update = () => {
      node.classList.toggle("icon-only", node.getBoundingClientRect().width <= 64);
    };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    update();
    return {
      destroy() {
        observer.disconnect();
      }
    };
  }

  let page = $state<MainPage>(detachedMode ? "new-tab" : "hosts");
  let hosts = $state<Host[]>([]);
  let identities = $state<Identity[]>([]);
  let groups = $state<VaultGroup[]>([]);
  let selectedGroupId = $state<string | null>(null);
  let selected = $state<Host | null>(null);
  let inspectorOpen = $state(false);
  let hostEditorMenuOpen = $state(false);
  let newHostMenuOpen = $state(false);
  let credentialMode = $state<"password" | "key">("password");
  let hostPassword = $state("");
  let showHostPassword = $state(false);
  let hostSubeditor = $state<HostSubeditor | null>(null);
  let hostSubeditorClosing = $state(false);
  let hostEditorBaseline = $state<string | null>(null);
  let hostSubeditorBaseline = $state<string | null>(null);
  let themeSubeditorBaseline = $state<TerminalAppearance | null>(null);
  let environmentDraft = $state<Array<{ name: string; value: string }>>([]);
  let jumpHostDraft = $state<string[]>([]);
  let chainPickerOpen = $state(false);
  let hostContextMenu = $state<{ host: Host; x: number; y: number } | null>(null);
  let bulkHostContextMenu = $state<{ x: number; y: number } | null>(null);
  let groupContextMenu = $state<{ group: VaultGroup; x: number; y: number } | null>(null);
  let selectedHostIds = $state<string[]>([]);
  let vaultDrag = $state<
    | { kind: "host"; ids: string[] }
    | { kind: "group"; id: string }
    | null
  >(null);
  let vaultDropGroupId = $state<string | null>(null);
  let suppressVaultCardClick = false;
  let hostCollectionView = $state<CollectionView>("grid");
  let hostCollectionSort = $state<CollectionSort>("az");
  let hostTagFilterOpen = $state(false);
  let hostTagFilterQuery = $state("");
  let selectedHostTags = $state<string[]>([]);
  let hostTagDraft = $state("");
  let hostTagInputFocused = $state(false);
  let terminalContextMenu = $state<{ tab: TerminalTab; x: number; y: number } | null>(null);
  let query = $state("");
  let newTabQuery = $state("");
  let newTabSearchInput = $state<HTMLInputElement>();
  let loading = $state(true);
  let saving = $state(false);
  let message = $state("");
  let messageIsError = $state(false);
  let TerminalComponent = $state<any>(null);
  let SftpComponent = $state<any>(null);
  let sftpHostId = $state("");
  let sftpAutoConnect = $state(false);
  let SnippetsComponent = $state<any>(null);
  let ForwardingComponent = $state<any>(null);
  let KeychainComponent = $state<any>(null);
  let KnownHostsComponent = $state<any>(null);
  let LogsComponent = $state<any>(null);
  let terminalLoadError = $state("");
  let terminalTabs = $state<TerminalTab[]>([]);
  let terminalToolsOpen = $state(false);
  let terminalToolsMounted = $state(false);
  let terminalToolsCloseTimer: number | null = null;
  let terminalHistoryVersion = $state(0);
  let terminalSnippetVersion = $state(0);
  let terminalCommandRequests = $state<Record<string, TerminalCommandRequest>>({});
  let topTabOrder = $state<string[]>(detachedMode ? [] : ["vault", "sftp"]);
  let activeTerminalId = $state<string | null>(null);
  let terminalSessionIds = $state<Record<string, string>>({});
  let detachingPaneIds = $state<string[]>([]);
  let workspaceName = $state("Workspace");
  let splitMode = $state(false);
  let workspaceActive = $state(false);
  let workspacePaneIds = $state<string[]>([]);
  let broadcastPaneIds = $state<string[]>([]);
  let workspaceFocusedPaneId = $state<string | null>(null);
  let workspaceLayout = $state<WorkspaceLayout | null>(null);
  let draggedTabId = $state<string | null>(null);
  let paneDropTarget = $state<{ paneId: string; zone: DropZone } | null>(null);
  let paneExtractTarget = $state(false);
  let terminalGridElement = $state<HTMLDivElement>();
  let activeDividerPath = $state<string | null>(null);
  let panePointerDrag = $state<{
    sourceId: string;
    startX: number;
    startY: number;
    currentX: number;
    currentY: number;
    dragging: boolean;
  } | null>(null);
  let topTabPointerDrag = $state<{
    sourceId: string;
    startX: number;
    startY: number;
    currentX: number;
    currentY: number;
    screenX: number;
    screenY: number;
    dragging: boolean;
  } | null>(null);
  let topTabDropTarget = $state<{
    id: string;
    after: boolean;
    merge: boolean;
  } | null>(null);
  let suppressTopTabClick = false;
  let detachedInitialized = $state(!detachedMode);
  let managerEditorDirty = $state<Record<string, boolean>>({});
  const appearanceSaveTimers = new Map<string, number>();
  const localTerminalAppearanceKey = "heminus-local-terminal-appearance";
  const terminalTabDragMime = "application/x-heminus-terminal-tab";

  $effect(() => {
    if (page !== "new-tab") return;
    const frame = window.requestAnimationFrame(() => {
      newTabSearchInput?.focus({ preventScroll: true });
    });
    return () => window.cancelAnimationFrame(frame);
  });

  onMount(() => {
    if (detachedMode) void initializeDetachedWindow();
    else void refreshHosts();
    let transferListenerDisposed = false;
    let removeTransferListener: (() => void) | null = null;
    let removeSnippetListener: (() => void) | null = null;
    let removeCommandHistoryListener: (() => void) | null = null;
    void listenForSnippetChanges(() => {
      terminalSnippetVersion += 1;
    }).then((unlisten) => {
      if (transferListenerDisposed) unlisten();
      else removeSnippetListener = unlisten;
    }).catch((cause) => showMessage(cause, true));
    void listenForCommandHistoryChanges(() => {
      terminalHistoryVersion += 1;
    }).then((unlisten) => {
      if (transferListenerDisposed) unlisten();
      else removeCommandHistoryListener = unlisten;
    }).catch((cause) => showMessage(cause, true));
    void listenForTerminalTabTransfers((transfer) => {
      void acceptTerminalTabTransfer(
        transfer.payload,
        transfer.clientX,
        transfer.clientY
      ).then(async () => {
        await tick();
        await appWindow?.setFocus();
      }).catch((cause) => showMessage(cause, true));
    }).then((unlisten) => {
      if (transferListenerDisposed) unlisten();
      else removeTransferListener = unlisten;
    }).catch((cause) => showMessage(cause, true));
    const onKeydown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        void switchPage("new-tab");
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "t") {
        event.preventDefault();
        void openTerminal();
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "e") {
        event.preventDefault();
        toggleSplit();
      }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "b") {
        event.preventDefault();
        toggleBroadcast();
      }
      if (event.key === "Escape") hostContextMenu = null;
      if (event.key === "Escape") bulkHostContextMenu = null;
      if (event.key === "Escape") groupContextMenu = null;
      if (event.key === "Escape") terminalContextMenu = null;
    };
    window.addEventListener("keydown", onKeydown);
    return () => {
      transferListenerDisposed = true;
      removeTransferListener?.();
      removeSnippetListener?.();
      removeCommandHistoryListener?.();
      window.removeEventListener("keydown", onKeydown);
      if (terminalToolsCloseTimer !== null) {
        window.clearTimeout(terminalToolsCloseTimer);
      }
      for (const timer of appearanceSaveTimers.values()) window.clearTimeout(timer);
      appearanceSaveTimers.clear();
    };
  });

  async function initializeDetachedWindow() {
    await refreshHosts();
    try {
      const payload = await takeDetachedTerminalPayload();
      if (!payload?.tabs.length) {
        await openTerminal();
        return;
      }
      terminalTabs = payload.tabs.map((spec) => {
        const host = spec.hostId
          ? hosts.find((candidate) => candidate.id === spec.hostId) ?? null
          : null;
        return {
          id: spec.id,
          title: spec.title,
          host: host ? cloneHost(host) : null,
          appearance: { ...spec.appearance },
          resumeSessionId: spec.sessionId
        };
      });
      await loadTerminalComponent();
      const workspace = payload.workspace;
      if (workspace && terminalTabs.length > 1) {
        workspacePaneIds = workspace.paneIds.filter((id) =>
          terminalTabs.some((tab) => tab.id === id)
        );
        if (workspacePaneIds.length > 1) {
          workspaceName = workspace.name || "Workspace";
          workspaceLayout = normalizeLayout(workspace.layout, workspacePaneIds);
          splitMode = true;
          workspaceActive = true;
          topTabOrder = ["workspace"];
          activeTerminalId = workspace.activePaneId &&
            workspacePaneIds.includes(workspace.activePaneId)
            ? workspace.activePaneId
            : workspacePaneIds[0];
        }
      }
      if (!splitMode) {
        workspacePaneIds = [];
        workspaceLayout = null;
        workspaceActive = false;
        topTabOrder = terminalTabs.map((tab) => tab.id);
        activeTerminalId = terminalTabs[0]?.id ?? null;
      }
      page = activeTerminalId ? "terminal" : "new-tab";
    } catch (cause) {
      showMessage(cause, true);
      await openTerminal();
    } finally {
      detachedInitialized = true;
      await tick();
      await appWindow?.show();
      await appWindow?.setFocus();
    }
  }

  async function acceptTerminalTabTransfer(
    payload: DetachedWindowPayload,
    clientX: number,
    clientY: number
  ) {
    if (!payload.tabs.length) return;
    const tabTarget = document
      .elementsFromPoint(clientX, clientY)
      .map((element) => element.closest<HTMLElement>("[data-top-tab-id]"))
      .find((element) => element?.dataset.topTabId !== undefined);
    const targetId = tabTarget?.dataset.topTabId ?? null;
    const targetBounds = tabTarget?.getBoundingClientRect();
    const insertAfter = Boolean(
      targetBounds && clientX > targetBounds.left + targetBounds.width / 2
    );
    const knownIds = new Set(terminalTabs.map((tab) => tab.id));
    const incomingSpecs = payload.tabs.filter((spec) => !knownIds.has(spec.id));
    if (!incomingSpecs.length) return;
    const needsHostRefresh = incomingSpecs.some(
      (spec) =>
        spec.hostId &&
        !hosts.some((candidate) => candidate.id === spec.hostId)
    );
    const hostCatalog = needsHostRefresh ? await listHosts("") : hosts;
    const incomingTabs = incomingSpecs.map((spec): TerminalTab => {
      const host = spec.hostId
        ? hostCatalog.find((candidate) => candidate.id === spec.hostId) ?? null
        : null;
      return {
        id: spec.id,
        title: spec.title,
        host: host ? cloneHost(host) : null,
        appearance: { ...spec.appearance },
        resumeSessionId: spec.sessionId
      };
    });
    terminalTabs = [...terminalTabs, ...incomingTabs];
    await loadTerminalComponent();

    const incomingIds = incomingTabs.map((tab) => tab.id);
    const nextOrder = topTabOrder.filter((id) => !incomingIds.includes(id));
    const targetIndex = targetId ? nextOrder.indexOf(targetId) : -1;
    const insertAt =
      targetIndex >= 0
        ? targetIndex + (insertAfter ? 1 : 0)
        : nextOrder.length;
    const incomingWorkspaceIds = payload.workspace?.paneIds.filter((id) =>
      incomingIds.includes(id)
    ) ?? [];
    const canRestoreWorkspace =
      incomingWorkspaceIds.length > 1 && workspacePaneIds.length < 2;
    if (canRestoreWorkspace && payload.workspace) {
      const orderWithoutWorkspace = nextOrder.filter((id) => id !== "workspace");
      orderWithoutWorkspace.splice(
        Math.min(insertAt, orderWithoutWorkspace.length),
        0,
        "workspace"
      );
      topTabOrder = orderWithoutWorkspace;
      workspaceName = payload.workspace.name || "Workspace";
      workspacePaneIds = incomingWorkspaceIds;
      workspaceLayout = normalizeLayout(
        payload.workspace.layout,
        incomingWorkspaceIds
      );
      splitMode = true;
      workspaceActive = true;
      workspaceFocusedPaneId = null;
      activeTerminalId =
        payload.workspace.activePaneId &&
        incomingWorkspaceIds.includes(payload.workspace.activePaneId)
          ? payload.workspace.activePaneId
          : incomingWorkspaceIds[0];
    } else {
      nextOrder.splice(
        Math.min(insertAt, nextOrder.length),
        0,
        ...incomingIds
      );
      topTabOrder = nextOrder;
      workspaceActive = false;
      activeTerminalId = incomingIds[0] ?? activeTerminalId;
    }
    page = "terminal";
  }

  async function refreshHosts() {
    loading = true;
    try {
      [hosts, identities, groups] = await Promise.all([
        listHosts(""),
        listIdentities(),
        listGroups()
      ]);
      message = "";
    } catch (cause) {
      showMessage(cause, true);
    } finally {
      loading = false;
    }
  }

  async function openQuickConnectEditor() {
    try {
      const target = parseQuickConnectInput(query);
      const matches = await listHosts(query);
      const existing = matches.find(
        (host) =>
          host.address.toLowerCase() === target.address.toLowerCase() &&
          host.username.toLowerCase() === target.username.toLowerCase() &&
          host.port === target.port
      );
      if (existing) {
        await editHost(existing);
        return;
      }

      query = "";
      await refreshHosts();
      if (!(await newHost()) || !selected) return;
      selected.label = target.address;
      selected.address = target.address;
      selected.username = target.username;
      selected.port = target.port;
      message = "Review the SSH details, choose a credential, then connect.";
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  function hostEditorState() {
    return { selected, credentialMode, hostPassword };
  }

  function rememberHostEditorBaseline() {
    hostEditorBaseline = draftSnapshot(hostEditorState());
  }

  function hostEditorDirty(): boolean {
    return inspectorOpen && draftChanged(hostEditorBaseline, hostEditorState());
  }

  async function canLeaveHostEditor(): Promise<boolean> {
    return confirmDiscardChanges(hostEditorDirty());
  }

  function discardHostEditor() {
    hostSubeditor = null;
    hostSubeditorBaseline = null;
    themeSubeditorBaseline = null;
    inspectorOpen = false;
    hostEditorMenuOpen = false;
    hostTagDraft = "";
    hostTagInputFocused = false;
    hostEditorBaseline = null;
    if (selected && !hosts.some((host) => host.id === selected?.id)) selected = null;
  }

  async function chooseHost(host: Host) {
    if (!(await canLeaveHostEditor())) return;
    hostEditorBaseline = null;
    selected = cloneHost(host);
    inspectorOpen = false;
    message = `${host.username}@${host.address}:${host.port}`;
    messageIsError = false;
  }

  async function editHost(host: Host) {
    if (inspectorOpen && !(await canLeaveHostEditor())) return;
    selected = cloneHost(host);
    prepareCredentialEditor(host);
    hostSubeditor = null;
    inspectorOpen = true;
    hostEditorMenuOpen = false;
    hostTagDraft = "";
    hostTagInputFocused = false;
    message = `${host.username}@${host.address}:${host.port}`;
    messageIsError = false;
    rememberHostEditorBaseline();
  }

  async function openHostContextMenu(event: MouseEvent, host: Host) {
    event.preventDefault();
    if (selectedHostIds.length > 1 && selectedHostIds.includes(host.id)) {
      hostContextMenu = null;
      groupContextMenu = null;
      bulkHostContextMenu = {
        x: Math.max(10, Math.min(event.clientX, window.innerWidth - 248)),
        y: Math.max(10, Math.min(event.clientY, window.innerHeight - 272))
      };
      return;
    }
    if (!(await canLeaveHostEditor())) return;
    selectedHostIds = [host.id];
    bulkHostContextMenu = null;
    groupContextMenu = null;
    hostEditorBaseline = null;
    selected = cloneHost(host);
    inspectorOpen = false;
    const width = 248;
    const height = 326;
    hostContextMenu = {
      host: cloneHost(host),
      x: Math.max(10, Math.min(event.clientX, window.innerWidth - width - 10)),
      y: Math.max(10, Math.min(event.clientY, window.innerHeight - height - 10))
    };
  }

  function openGroupContextMenu(event: MouseEvent, group: VaultGroup) {
    event.preventDefault();
    hostContextMenu = null;
    bulkHostContextMenu = null;
    const width = 248;
    const height = 184;
    groupContextMenu = {
      group: { ...group },
      x: Math.max(10, Math.min(event.clientX, window.innerWidth - width - 10)),
      y: Math.max(10, Math.min(event.clientY, window.innerHeight - height - 10))
    };
  }

  function takeContextHost(): Host | null {
    const host = hostContextMenu?.host ?? null;
    hostContextMenu = null;
    return host;
  }

  function takeContextGroup(): VaultGroup | null {
    const group = groupContextMenu?.group ?? null;
    groupContextMenu = null;
    return group;
  }

  async function duplicateHost(host: Host) {
    if (!(await canLeaveHostEditor())) return;
    const now = new Date().toISOString();
    try {
      const duplicate = await saveHost({
        ...cloneHost(host),
        id: crypto.randomUUID(),
        label: `${host.label} copy`,
        created_at: now,
        updated_at: now
      });
      await refreshHosts();
      selected = cloneHost(duplicate);
      prepareCredentialEditor(duplicate);
      inspectorOpen = true;
      message = "Duplicated locally";
      messageIsError = false;
      rememberHostEditorBaseline();
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function copyHostLink(host: Host) {
    const address = host.address.includes(":") ? `[${host.address}]` : host.address;
    try {
      await navigator.clipboard.writeText(
        `ssh://${encodeURIComponent(host.username)}@${address}:${host.port}`
      );
      message = "SSH link copied";
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function closeInspector() {
    if (!(await canLeaveHostEditor())) return;
    discardHostEditor();
  }

  async function newHost(): Promise<boolean> {
    if (!(await canLeaveHostEditor())) return false;
    newHostMenuOpen = false;
    const now = new Date().toISOString();
    selected = {
      id: crypto.randomUUID(),
      label: "",
      address: "",
      port: 22,
      username: "ubuntu",
      group_name: null,
      group_id: selectedGroupId,
      tags: [],
      color: "blue",
      identity_id: null,
      jump_host_ids: [],
      environment: [],
      terminal_theme: "heminus_dark",
      terminal_font_size: 14,
      created_at: now,
      updated_at: now
    };
    selected.group_name = selectedGroupId ? groupPath(selectedGroupId) : null;
    credentialMode = "password";
    hostPassword = "";
    showHostPassword = false;
    hostSubeditor = null;
    inspectorOpen = true;
    hostEditorMenuOpen = false;
    hostTagDraft = "";
    hostTagInputFocused = false;
    message = "Connection details stay on this device.";
    messageIsError = false;
    rememberHostEditorBaseline();
    requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#host-address")?.focus());
    return true;
  }

  async function persistHost(): Promise<Host | null> {
    if (!selected || saving) return null;
    saving = true;
    try {
      validateSelectedHostDraft();
      await persistSelectedCredential();
      selected.group_name = selected.group_id ? groupPath(selected.group_id) : null;
      selected.updated_at = new Date().toISOString();
      const saved = await saveHost(selected);
      selected = cloneHost(saved);
      terminalTabs = terminalTabs.map((tab) =>
        tab.host?.id === saved.id ? { ...tab, host: cloneHost(saved) } : tab
      );
      await refreshHosts();
      message = "Saved locally";
      messageIsError = false;
      rememberHostEditorBaseline();
      return saved;
    } catch (cause) {
      showMessage(cause, true);
      return null;
    } finally {
      saving = false;
    }
  }

  function validateSelectedHostDraft() {
    if (!selected) return;
    if (!selected.label.trim()) throw new Error("Enter a label for this host");
    if (!selected.address.trim()) throw new Error("Enter an IP address or hostname");
    if (!selected.username.trim()) throw new Error("Enter an SSH username");
    if (!Number.isInteger(selected.port) || selected.port < 1 || selected.port > 65535) {
      throw new Error("SSH port must be between 1 and 65535");
    }
    if (selected.jump_host_ids.length > 16) {
      throw new Error("A connection chain can contain at most 16 jump hosts");
    }
    if (
      selected.jump_host_ids.includes(selected.id) ||
      new Set(selected.jump_host_ids).size !== selected.jump_host_ids.length
    ) {
      throw new Error("A connection chain cannot contain this host or duplicate jump hosts");
    }
    const names = new Set<string>();
    for (const variable of selected.environment) {
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(variable.name) || /[\0\r\n]/.test(variable.value)) {
        throw new Error("Environment variable names and values must be valid SSH SetEnv values");
      }
      if (names.has(variable.name)) {
        throw new Error(`Environment variable “${variable.name}” is duplicated`);
      }
      names.add(variable.name);
    }
  }

  async function connectSelectedHost() {
    const saved = await persistHost();
    if (!saved) return;
    void openTerminal(saved);
  }

  function openEnvironmentEditor() {
    if (!selected) return;
    environmentDraft = selected.environment.map((variable) => ({ ...variable }));
    hostSubeditorClosing = false;
    hostSubeditor = "environment";
    hostSubeditorBaseline = draftSnapshot(environmentDraft);
    themeSubeditorBaseline = null;
  }

  function openChainEditor() {
    if (!selected) return;
    jumpHostDraft = [...selected.jump_host_ids];
    chainPickerOpen = false;
    hostSubeditorClosing = false;
    hostSubeditor = "chain";
    hostSubeditorBaseline = draftSnapshot(jumpHostDraft);
    themeSubeditorBaseline = null;
  }

  function openThemeEditor() {
    if (!selected) return;
    hostSubeditorClosing = false;
    hostSubeditor = "theme";
    themeSubeditorBaseline = {
      theme: selected.terminal_theme,
      fontSize: selected.terminal_font_size
    };
    hostSubeditorBaseline = draftSnapshot(themeSubeditorBaseline);
  }

  function hostSubeditorTitle(): string {
    if (hostSubeditor === "environment") return "Environment Variables";
    if (hostSubeditor === "chain") return "Edit Chain";
    return "Select Color Theme";
  }

  function hostSubeditorState(): unknown {
    if (hostSubeditor === "environment") return environmentDraft;
    if (hostSubeditor === "chain") return jumpHostDraft;
    if (hostSubeditor === "theme" && selected) {
      return { theme: selected.terminal_theme, fontSize: selected.terminal_font_size };
    }
    return null;
  }

  function hostSubeditorDirty(): boolean {
    return Boolean(hostSubeditor) && draftChanged(hostSubeditorBaseline, hostSubeditorState());
  }

  async function closeHostSubeditor(save: boolean) {
    if (!selected || !hostSubeditor || hostSubeditorClosing) return;
    if (!save && !(await confirmDiscardChanges(hostSubeditorDirty()))) return;
    if (save && hostSubeditor === "environment") {
      if (!environmentDraftValid()) return;
      selected.environment = environmentDraft.map((variable) => ({ ...variable }));
    }
    if (save && hostSubeditor === "chain") selected.jump_host_ids = [...jumpHostDraft];
    if (!save && hostSubeditor === "theme" && themeSubeditorBaseline) {
      selected.terminal_theme = themeSubeditorBaseline.theme;
      selected.terminal_font_size = themeSubeditorBaseline.fontSize;
    }
    hostSubeditorBaseline = null;
    hostSubeditorClosing = true;
    window.setTimeout(() => {
      hostSubeditor = null;
      hostSubeditorClosing = false;
      hostSubeditorBaseline = null;
      themeSubeditorBaseline = null;
      chainPickerOpen = false;
    }, 180);
  }

  function addEnvironmentDraft() {
    environmentDraft.push({ name: "", value: "" });
  }

  function removeEnvironmentDraft(index: number) {
    environmentDraft.splice(index, 1);
  }

  function environmentDraftValid(): boolean {
    const names = new Set<string>();
    return environmentDraft.every((variable) => {
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(variable.name) || /[\0\r\n]/.test(variable.value)) {
        return false;
      }
      if (names.has(variable.name)) return false;
      names.add(variable.name);
      return true;
    });
  }

  function environmentSummary(): string {
    if (!selected?.environment.length) return "Not configured";
    return `${selected.environment.length} variable${selected.environment.length === 1 ? "" : "s"}`;
  }

  function chainSummary(): string {
    if (!selected?.jump_host_ids.length) return "Direct";
    return selected.jump_host_ids
      .map((id) => hosts.find((host) => host.id === id)?.label ?? "Missing host")
      .join(" → ");
  }

  function jumpHostOptions(): Host[] {
    if (!selected) return [];
    if (jumpHostDraft.length >= 16) return [];
    const selectedId = selected.id;
    return hosts.filter(
      (host) => host.id !== selectedId && !jumpHostDraft.includes(host.id)
    );
  }

  function addJumpHost(id: string) {
    if (jumpHostDraft.length >= 16 || jumpHostDraft.includes(id)) return;
    jumpHostDraft.unshift(id);
    chainPickerOpen = false;
  }

  function chainEntryLabel(): string {
    const firstJumpHost = hosts.find((host) => host.id === jumpHostDraft[0]);
    return firstJumpHost?.label ?? selected?.label ?? selected?.address ?? "this host";
  }

  function removeJumpHost(index: number) {
    jumpHostDraft.splice(index, 1);
  }

  function identitySummary(): string {
    const identity = identities.find((candidate) => candidate.id === selected?.identity_id);
    if (!identity) {
      return credentialMode === "password"
        ? "Enter a password to save it for one-click access"
        : "Select a key from Keychain";
    }
    if (identity.kind === "key_file") {
      return identity.secret_stored ? "Vault key · passphrase protected" : "Vault key";
    }
    if (identity.kind === "password") {
      return identity.secret_stored ? "Password stored in Keychain" : "Password not stored";
    }
    return "Legacy identity · unsupported";
  }

  function prepareCredentialEditor(host: Host) {
    const identity = identities.find((candidate) => candidate.id === host.identity_id);
    credentialMode = identity?.kind === "key_file" ? "key" : "password";
    hostPassword = "";
    showHostPassword = false;
  }

  function setCredentialMode(mode: "password" | "key") {
    if (!selected || credentialMode === mode) return;
    credentialMode = mode;
    selected.identity_id = null;
    hostPassword = "";
    showHostPassword = false;
  }

  async function persistSelectedCredential() {
    if (!selected) return;
    if (["127.0.0.1", "localhost", "::1"].includes(selected.address.trim().toLowerCase())) return;
    const identity = identities.find((candidate) => candidate.id === selected?.identity_id);
    if (credentialMode === "key") {
      if (!identity || identity.kind !== "key_file") {
        throw new Error("Select a private key from Keychain");
      }
      return;
    }

    let passwordIdentity = identity?.kind === "password" ? identity : null;
    if (!passwordIdentity) {
      if (!hostPassword) {
        throw new Error("Enter a password so this host can connect with one click");
      }
      passwordIdentity = await saveIdentity({
        id: crypto.randomUUID(),
        label: `${selected.label.trim() || selected.address.trim() || "Host"} password`,
        kind: "password",
        username: null,
        key_path: null,
        secret_stored: false,
        created_at: new Date().toISOString()
      });
      selected.identity_id = passwordIdentity.id;
    }
    if (!passwordIdentity.secret_stored && !hostPassword) {
      throw new Error("This password identity has no stored password");
    }
    if (hostPassword) {
      await setIdentitySecret(passwordIdentity.id, hostPassword);
      hostPassword = "";
    }
  }

  function jumpHostCredentialLabel(host: Host): string {
    const identity = identities.find((candidate) => candidate.id === host.identity_id);
    if (identity?.kind === "password") {
      return identity.secret_stored ? "password" : "password missing";
    }
    if (identity?.kind === "key_file") {
      return identity.secret_stored ? "protected key" : "key";
    }
    return "credential required";
  }

  function groupRows(): Array<{ group: VaultGroup; depth: number; path: string }> {
    const rows: Array<{ group: VaultGroup; depth: number; path: string }> = [];
    const visited = new Set<string>();
    const append = (parentId: string | null, depth: number, prefix: string) => {
      for (const group of groups
        .filter((candidate) => candidate.parent_id === parentId)
        .sort((left, right) => left.name.localeCompare(right.name))) {
        if (visited.has(group.id)) continue;
        visited.add(group.id);
        const path = prefix ? `${prefix} / ${group.name}` : group.name;
        rows.push({ group, depth, path });
        append(group.id, depth + 1, path);
      }
    };
    append(null, 0, "");
    for (const group of groups) {
      if (!visited.has(group.id)) rows.push({ group, depth: 0, path: group.name });
    }
    return rows;
  }

  function groupPath(id: string): string {
    return groupRows().find((row) => row.group.id === id)?.path ?? "Unknown group";
  }

  function groupAndDescendantIds(id: string): Set<string> {
    const ids = new Set([id]);
    let changed = true;
    while (changed) {
      changed = false;
      for (const group of groups) {
        if (group.parent_id && ids.has(group.parent_id) && !ids.has(group.id)) {
          ids.add(group.id);
          changed = true;
        }
      }
    }
    return ids;
  }

  function movableGroupRows(groupId: string) {
    const blocked = groupAndDescendantIds(groupId);
    return groupRows().filter((row) => !blocked.has(row.group.id));
  }

  const visibleHostList = $derived.by(() => {
    let candidates = hosts;
    if (selectedGroupId) {
      const ids = groupAndDescendantIds(selectedGroupId);
      candidates = hosts.filter((host) => host.group_id && ids.has(host.group_id));
    }
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (normalizedQuery) {
      candidates = candidates.filter((host) =>
        [
          host.label,
          host.address,
          host.username,
          `${host.username}@${host.address}`,
          `${host.address}@${host.username}`,
          host.group_name ?? "",
          ...host.tags
        ].some((value) => value.toLocaleLowerCase().includes(normalizedQuery))
      );
    }
    if (selectedHostTags.length) {
      candidates = candidates.filter((host) => hostHasSelectedTags(host, selectedHostTags));
    }
    return [...candidates].sort((left, right) =>
      compareCollectionItems(
        left.label,
        right.label,
        left.created_at,
        right.created_at,
        hostCollectionSort
      )
    );
  });

  function allHostTags(): string[] {
    return collectHostTags(hosts);
  }

  function visibleHostTagFilters(): string[] {
    const needle = hostTagFilterQuery.trim().toLocaleLowerCase();
    return allHostTags().filter((tag) => !needle || tag.toLocaleLowerCase().includes(needle));
  }

  function editorHostTagSuggestions(): string[] {
    if (!selected) return [];
    const selectedKeys = new Set(selected.tags.map((tag) => tag.toLocaleLowerCase()));
    const needle = cleanHostTag(hostTagDraft).toLocaleLowerCase();
    return allHostTags()
      .filter((tag) => !selectedKeys.has(tag.toLocaleLowerCase()))
      .filter((tag) => !needle || tag.toLocaleLowerCase().includes(needle))
      .slice(0, 6);
  }

  function addTagToSelected(tag: string) {
    if (!selected) return;
    selected.tags = dedupeHostTags([...selected.tags, tag]);
    hostTagDraft = "";
  }

  function acceptHostTagDraft() {
    const tag = cleanHostTag(hostTagDraft);
    if (!tag) return;
    const existing = allHostTags().find(
      (candidate) => candidate.toLocaleLowerCase() === tag.toLocaleLowerCase()
    );
    addTagToSelected(existing ?? tag);
  }

  function removeTagFromSelected(tag: string) {
    if (!selected) return;
    selected.tags = removeHostTag(selected.tags, tag);
  }

  function toggleHostTagFilter(tag: string) {
    const active = selectedHostTags.some(
      (candidate) => candidate.toLocaleLowerCase() === tag.toLocaleLowerCase()
    );
    selectedHostTags = active
      ? removeHostTag(selectedHostTags, tag)
      : dedupeHostTags([...selectedHostTags, tag]);
    selectedHostIds = [];
  }

  async function renameSavedHostTag(tag: string) {
    const next = cleanHostTag(
      (await promptDialog({
        title: "Rename tag",
        label: "Tag name",
        initialValue: tag,
        confirmLabel: "Rename"
      })) ?? ""
    );
    if (!next || next === tag) return;
    try {
      const affected = hosts.filter((host) => hostHasSelectedTags(host, [tag]));
      await Promise.all(affected.map((host) => saveHost({
        ...cloneHost(host),
        tags: renameHostTag(host.tags, tag, next),
        updated_at: new Date().toISOString()
      })));
      if (selected) selected.tags = renameHostTag(selected.tags, tag, next);
      selectedHostTags = renameHostTag(selectedHostTags, tag, next);
      await refreshHosts();
      message = `Tag renamed to “${next}”`;
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function deleteSavedHostTag(tag: string) {
    const affected = hosts.filter((host) => hostHasSelectedTags(host, [tag]));
    if (!(await confirmDialog({
      title: `Delete “${tag}”?`,
      message: `Remove this tag from ${affected.length} host${affected.length === 1 ? "" : "s"}?`,
      confirmLabel: "Delete tag",
      danger: true
    }))) return;
    try {
      await Promise.all(affected.map((host) => saveHost({
        ...cloneHost(host),
        tags: removeHostTag(host.tags, tag),
        updated_at: new Date().toISOString()
      })));
      if (selected) selected.tags = removeHostTag(selected.tags, tag);
      selectedHostTags = removeHostTag(selectedHostTags, tag);
      await refreshHosts();
      message = `Tag “${tag}” deleted`;
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  function directChildGroups(): VaultGroup[] {
    const normalizedQuery = query.trim().toLowerCase();
    return groups
      .filter((group) => group.parent_id === selectedGroupId)
      .filter(
        (group) =>
          !normalizedQuery ||
          group.name.toLowerCase().includes(normalizedQuery) ||
          groupHostCount(group.id) > 0
      )
      .sort((left, right) =>
        compareCollectionItems(
          left.name,
          right.name,
          left.created_at,
          right.created_at,
          hostCollectionSort
        )
      );
  }

  function groupHostCount(id: string): number {
    const ids = groupAndDescendantIds(id);
    return hosts.filter((host) => host.group_id && ids.has(host.group_id)).length;
  }

  function groupBreadcrumb(): VaultGroup[] {
    const path: VaultGroup[] = [];
    const visited = new Set<string>();
    let current = groups.find((group) => group.id === selectedGroupId);
    while (current && !visited.has(current.id)) {
      visited.add(current.id);
      path.unshift(current);
      current = current.parent_id
        ? groups.find((group) => group.id === current?.parent_id)
        : undefined;
    }
    return path;
  }

  function selectGroup(id: string | null) {
    selectedGroupId = id;
    selected = null;
    inspectorOpen = false;
  }

  async function createGroup() {
    newHostMenuOpen = false;
    const name = (
      await promptDialog({
        title: selectedGroupId ? "New nested group" : "New group",
        message: selectedGroupId
          ? `Create inside “${groupPath(selectedGroupId)}”.`
          : "Create in your Personal vault.",
        label: "Group name",
        placeholder: "Production"
      })
    )?.trim();
    if (!name) return;
    const now = new Date().toISOString();
    try {
      const group = await saveGroup({
        id: crypto.randomUUID(),
        name,
        parent_id: selectedGroupId,
        created_at: now,
        updated_at: now
      });
      await refreshHosts();
      selectedGroupId = group.id;
      message = "Group created";
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function renameGroup(group: VaultGroup) {
    const name = (
      await promptDialog({
        title: "Rename group",
        label: "Group name",
        initialValue: group.name,
        confirmLabel: "Rename"
      })
    )?.trim();
    if (!name || name === group.name) return;
    try {
      await saveGroup({ ...group, name, updated_at: new Date().toISOString() });
      await refreshHosts();
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function moveGroup(group: VaultGroup, parentId: string | null) {
    if (!group || group.parent_id === parentId) return;
    if (parentId && groupAndDescendantIds(group.id).has(parentId)) {
      showMessage("A group cannot be moved inside itself or one of its child groups.", true);
      return;
    }
    try {
      await saveGroup({
        ...group,
        parent_id: parentId,
        updated_at: new Date().toISOString()
      });
      await refreshHosts();
      message = parentId ? `Group moved inside “${groupPath(parentId)}”` : "Group moved to the root";
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function removeGroup(group: VaultGroup) {
    if (
      !(await confirmDialog({
        title: "Delete group?",
        message: `Delete “${groupPath(group.id)}”? Child groups move to the root and hosts become ungrouped.`,
        confirmLabel: "Delete",
        danger: true
      }))
    ) {
      return;
    }
    try {
      await deleteGroup(group.id);
      if (selectedGroupId === group.id) selectedGroupId = null;
      await refreshHosts();
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function moveHost(host: Host, groupId: string | null) {
    if (host.group_id === groupId) return;
    try {
      await saveHost({
        ...cloneHost(host),
        group_id: groupId,
        group_name: groupId ? groupPath(groupId) : null,
        updated_at: new Date().toISOString()
      });
      await refreshHosts();
      message = groupId ? `Host moved inside “${groupPath(groupId)}”` : "Host moved to the root";
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  function selectedHosts(): Host[] {
    const ids = new Set(selectedHostIds);
    return hosts.filter((host) => ids.has(host.id));
  }

  async function handleHostCardClick(event: MouseEvent, host: Host) {
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      selectedHostIds = selectedHostIds.includes(host.id)
        ? selectedHostIds.filter((id) => id !== host.id)
        : [...selectedHostIds, host.id];
      return;
    }
    selectedHostIds = [host.id];
    await chooseHost(host);
  }

  async function moveSelectedHosts(groupId: string | null) {
    const targets = selectedHosts().filter((host) => host.group_id !== groupId);
    bulkHostContextMenu = null;
    if (!targets.length) return;
    try {
      await Promise.all(targets.map((host) => saveHost({
        ...cloneHost(host),
        group_id: groupId,
        group_name: groupId ? groupPath(groupId) : null,
        updated_at: new Date().toISOString()
      })));
      await refreshHosts();
      message = `${targets.length} hosts moved${groupId ? ` to “${groupPath(groupId)}”` : " to the root"}`;
      messageIsError = false;
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function connectSelectedHosts() {
    const targets = selectedHosts();
    bulkHostContextMenu = null;
    for (const host of targets) await openTerminal(host);
  }

  async function removeSelectedHosts() {
    const targets = selectedHosts();
    bulkHostContextMenu = null;
    if (!targets.length || !(await confirmDialog({
      title: `Remove ${targets.length} hosts?`,
      message: "Remove every selected host from this device?",
      confirmLabel: "Remove all",
      danger: true
    }))) return;
    try {
      await Promise.all(targets.map((host) => deleteHost(host.id)));
      if (selected && selectedHostIds.includes(selected.id)) discardHostEditor();
      selectedHostIds = [];
      await refreshHosts();
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  function startHostMarquee(event: PointerEvent) {
    beginMarqueeSelection(
      event,
      event.currentTarget as HTMLElement,
      selectedHostIds,
      (ids) => (selectedHostIds = ids)
    );
  }

  function startVaultDrag(
    event: DragEvent,
    kind: "host" | "group",
    id: string
  ) {
    suppressVaultCardClick = true;
    const ids = kind === "host"
      ? selectedHostIds.includes(id) && selectedHostIds.length > 1
        ? [...selectedHostIds]
        : [id]
      : [];
    if (kind === "host") {
      selectedHostIds = ids;
      vaultDrag = { kind: "host", ids };
    } else {
      vaultDrag = { kind: "group", id };
    }
    vaultDropGroupId = null;
    event.dataTransfer?.setData("application/x-heminus-vault-item", `${kind}:${id}`);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
      setElementDragPreview(event, kind === "host" ? ids.length : 1);
    }
  }

  function canDropVaultItem(groupId: string): boolean {
    if (!vaultDrag) return false;
    if (vaultDrag.kind === "host") {
      return vaultDrag.ids.some((id) =>
        hosts.some((host) => host.id === id && host.group_id !== groupId)
      );
    }
    return (
      vaultDrag.id !== groupId &&
      !groupAndDescendantIds(vaultDrag.id).has(groupId)
    );
  }

  function dragOverGroup(event: DragEvent, groupId: string) {
    if (!canDropVaultItem(groupId)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    vaultDropGroupId = groupId;
  }

  async function dropIntoGroup(event: DragEvent, group: VaultGroup) {
    event.preventDefault();
    if (!canDropVaultItem(group.id) || !vaultDrag) return;
    const item = vaultDrag;
    vaultDrag = null;
    vaultDropGroupId = null;
    if (item.kind === "host") {
      selectedHostIds = [...item.ids];
      await moveSelectedHosts(group.id);
      return;
    }
    const source = groups.find((candidate) => candidate.id === item.id);
    if (source) await moveGroup(source, group.id);
  }

  function finishVaultDrag() {
    vaultDrag = null;
    vaultDropGroupId = null;
    window.setTimeout(() => {
      suppressVaultCardClick = false;
    });
  }

  function assignSelectedGroup(groupId: string | null) {
    if (!selected) return;
    selected.group_id = groupId;
    selected.group_name = groupId ? groupPath(groupId) : null;
  }

  async function removeHost() {
    if (!selected || !hosts.some((host) => host.id === selected?.id)) return;
    if (
      !(await confirmDialog({
        title: "Remove host?",
        message: `Remove “${selected.label}” from this device?`,
        confirmLabel: "Remove",
        danger: true
      }))
    ) return;
    try {
      await deleteHost(selected.id);
      selected = null;
      await refreshHosts();
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  function defaultTerminalAppearance(): TerminalAppearance {
    return {
      theme: "heminus_dark",
      fontSize: 14
    };
  }

  function hostTerminalAppearance(host: Host): TerminalAppearance {
    return {
      theme: host.terminal_theme,
      fontSize: host.terminal_font_size || 14
    };
  }

  function localTerminalAppearance(): TerminalAppearance {
    try {
      const saved = JSON.parse(
        localStorage.getItem(localTerminalAppearanceKey) ?? "null"
      ) as Partial<TerminalAppearance> | null;
      const fallback = defaultTerminalAppearance();
      return {
        theme: saved?.theme ?? fallback.theme,
        fontSize: Math.max(9, Math.min(32, saved?.fontSize ?? fallback.fontSize))
      };
    } catch {
      return defaultTerminalAppearance();
    }
  }

  async function openTerminal(host: Host | null = null, pageChangePrepared = false) {
    if (!pageChangePrepared && !(await preparePageChange("terminal"))) return;
    if (terminalTabs.length >= 16) {
      message = "Heminus can keep at most 16 terminal sessions open.";
      return;
    }
    const tab: TerminalTab = {
      id: crypto.randomUUID(),
      title: host?.label ?? "Local Terminal",
      host: host ? cloneHost(host) : null,
      appearance: host ? hostTerminalAppearance(host) : localTerminalAppearance(),
      resumeSessionId: null
    };
    terminalTabs.push(tab);
    topTabOrder.push(tab.id);
    activeTerminalId = tab.id;
    workspaceActive = false;
    page = "terminal";
    await loadTerminalComponent();
  }

  async function loadTerminalComponent() {
    if (TerminalComponent) return;
    terminalLoadError = "";
    try {
      TerminalComponent = (await import("./features/terminal/TerminalPane.svelte")).default;
    } catch (cause) {
      terminalLoadError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function activateStandaloneTerminal(id: string) {
    if (!(await preparePageChange("terminal"))) return;
    activeTerminalId = id;
    workspaceActive = false;
    page = "terminal";
  }

  async function activateWorkspace() {
    if (!splitMode || workspacePaneIds.length < 2) return;
    if (!(await preparePageChange("terminal"))) return;
    workspaceActive = true;
    if (!activeTerminalId || !workspacePaneIds.includes(activeTerminalId)) {
      activeTerminalId = workspacePaneIds[0] ?? null;
    }
    page = "terminal";
  }

  function activateTerminalPane(id: string) {
    activeTerminalId = id;
    if (workspacePaneIds.includes(id)) workspaceActive = true;
    page = "terminal";
  }

  function openTerminalContextMenu(event: MouseEvent, tab: TerminalTab) {
    event.preventDefault();
    const width = 226;
    const height = tab.id === "workspace" ? 146 : 252;
    terminalContextMenu = {
      tab,
      x: Math.max(10, Math.min(event.clientX, window.innerWidth - width - 10)),
      y: Math.max(10, Math.min(event.clientY, window.innerHeight - height - 10))
    };
  }

  async function renameTerminalTab(tab: TerminalTab) {
    const name = (
      await promptDialog({
        title: "Rename terminal",
        label: "Terminal name",
        initialValue: tab.title,
        confirmLabel: "Rename"
      })
    )?.trim();
    if (!name) return;
    const target = terminalTabs.find((candidate) => candidate.id === tab.id);
    if (!target) return;
    const sessionId = terminalSessionIds[tab.id] ?? tab.resumeSessionId;
    if (sessionId) {
      try {
        await renameTerminalSession(sessionId, name);
      } catch (cause) {
        showMessage(cause, true);
        return;
      }
    }
    target.title = name;
  }

  async function splitTerminalTab(tab: TerminalTab) {
    const targetId = tab.id;
    await openTerminal(tab.host);
    const newId = activeTerminalId;
    if (!newId || newId === targetId) return;
    createWorkspace([targetId, newId], targetId, "bottom");
  }

  function detachedTabSpec(tab: TerminalTab) {
    return {
      id: tab.id,
      title: tab.title,
      hostId: tab.host?.id ?? null,
      sessionId: terminalSessionIds[tab.id] ?? null,
      appearance: { ...tab.appearance }
    };
  }

  async function detachTerminalTab(tab: TerminalTab) {
    const payload: DetachedWindowPayload = {
      title: tab.title || "Terminal",
      tabs: [detachedTabSpec(tab)],
      workspace: null
    };
    try {
      await createDetachedTerminalWindow(payload);
      detachingPaneIds = [...new Set([...detachingPaneIds, tab.id])];
      closeTerminalTab(tab.id);
      window.setTimeout(() => {
        detachingPaneIds = detachingPaneIds.filter((id) => id !== tab.id);
      }, 2_000);
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  async function detachWorkspace() {
    const tabs = workspacePaneIds
      .map((id) => terminalTabs.find((tab) => tab.id === id))
      .filter((tab): tab is TerminalTab => Boolean(tab));
    if (tabs.length < 2) return;
    const payload: DetachedWindowPayload = {
      title: workspaceName || "Workspace",
      tabs: tabs.map(detachedTabSpec),
      workspace: {
        name: workspaceName || "Workspace",
        paneIds: tabs.map((tab) => tab.id),
        layout: normalizeLayout(workspaceLayout, tabs.map((tab) => tab.id)),
        activePaneId: activeTerminalId
      }
    };
    try {
      await createDetachedTerminalWindow(payload);
      const ids = tabs.map((tab) => tab.id);
      detachingPaneIds = [...new Set([...detachingPaneIds, ...ids])];
      closeCurrentWorkspace();
      window.setTimeout(() => {
        detachingPaneIds = detachingPaneIds.filter((id) => !ids.includes(id));
      }, 2_000);
    } catch (cause) {
      showMessage(cause, true);
    }
  }

  function transferableTopTab(sourceId: string): {
    payload: DetachedWindowPayload;
    paneIds: string[];
  } | null {
    if (sourceId === "workspace") {
      const tabs = workspacePaneIds
        .map((id) => terminalTabs.find((tab) => tab.id === id))
        .filter((tab): tab is TerminalTab => Boolean(tab));
      if (tabs.length < 2) return null;
      return {
        payload: {
          title: workspaceName || "Workspace",
          tabs: tabs.map(detachedTabSpec),
          workspace: {
            name: workspaceName || "Workspace",
            paneIds: tabs.map((tab) => tab.id),
            layout: normalizeLayout(workspaceLayout, tabs.map((tab) => tab.id)),
            activePaneId: activeTerminalId
          }
        },
        paneIds: tabs.map((tab) => tab.id)
      };
    }
    const tab = terminalTabs.find((candidate) => candidate.id === sourceId);
    if (!tab) return null;
    return {
      payload: {
        title: tab.title || "Terminal",
        tabs: [detachedTabSpec(tab)],
        workspace: null
      },
      paneIds: [tab.id]
    };
  }

  async function transferDraggedTopTab(
    sourceId: string,
    screenX: number,
    screenY: number
  ) {
    const transfer = transferableTopTab(sourceId);
    if (!transfer) return;
    const { payload, paneIds } = transfer;
    detachingPaneIds = [...new Set([...detachingPaneIds, ...paneIds])];
    try {
      await transferTerminalTab(payload, screenX, screenY);
      if (sourceId === "workspace") closeCurrentWorkspace();
      else closeTerminalTab(sourceId);
      if (detachedMode && terminalTabs.length === 0) {
        window.setTimeout(() => void appWindow?.close(), 0);
      }
      window.setTimeout(() => {
        detachingPaneIds = detachingPaneIds.filter((id) => !paneIds.includes(id));
      }, 2_000);
    } catch (cause) {
      detachingPaneIds = detachingPaneIds.filter((id) => !paneIds.includes(id));
      showMessage(cause, true);
    }
  }

  function closeCurrentWorkspace() {
    for (const id of [...workspacePaneIds]) closeTerminalTab(id);
  }

  function focusWorkspacePane(id: string) {
    activeTerminalId = id;
    workspaceActive = true;
    workspaceFocusedPaneId = workspaceFocusedPaneId === id ? null : id;
  }

  function closeTerminalTab(id: string) {
    const index = terminalTabs.findIndex((tab) => tab.id === id);
    if (index < 0) return;
    const wasWorkspacePane = workspacePaneIds.includes(id);
    terminalTabs.splice(index, 1);
    topTabOrder = topTabOrder.filter((tabId) => tabId !== id);
    if (wasWorkspacePane) {
      workspacePaneIds = workspacePaneIds.filter((paneId) => paneId !== id);
      broadcastPaneIds = broadcastPaneIds.filter((paneId) => paneId !== id);
      workspaceLayout = removeLayoutPane(workspaceLayout, id);
      dissolveWorkspaceIfNeeded();
    }
    if (activeTerminalId === id) {
      if (workspaceActive && workspacePaneIds.length >= 2) {
        activeTerminalId = workspacePaneIds[0] ?? null;
      } else {
        const next = standaloneTerminalTabs()[Math.min(index, standaloneTerminalTabs().length - 1)];
        activeTerminalId = next?.id ?? workspacePaneIds[0] ?? null;
        workspaceActive = !next && workspacePaneIds.length >= 2;
      }
      if (!activeTerminalId) page = "new-tab";
    }
    const { [id]: _, ...remaining } = terminalSessionIds;
    terminalSessionIds = remaining;
    const { [id]: _request, ...remainingRequests } = terminalCommandRequests;
    terminalCommandRequests = remainingRequests;
    if (terminalTabs.length === 0) closeTerminalTools();
  }

  function standaloneTerminalTabs(): TerminalTab[] {
    return terminalTabs.filter((tab) => !workspacePaneIds.includes(tab.id));
  }

  function createWorkspace(
    paneIds: string[],
    targetId: string | null = null,
    zone: DropZone = "right"
  ) {
    const validIds = [...new Set(paneIds)].filter((id) =>
      terminalTabs.some((tab) => tab.id === id)
    );
    if (validIds.length < 2) return;
    const positions = validIds
      .map((id) => topTabOrder.indexOf(id))
      .filter((position) => position >= 0);
    const insertAt = positions.length > 0 ? Math.min(...positions) : topTabOrder.length;
    topTabOrder = topTabOrder.filter((id) => !validIds.includes(id) && id !== "workspace");
    topTabOrder.splice(Math.min(insertAt, topTabOrder.length), 0, "workspace");
    workspacePaneIds = validIds;
    const firstId = targetId && validIds.includes(targetId) ? targetId : validIds[0];
    workspaceLayout = splitPane(null, firstId, null, "right");
    for (const id of validIds.filter((candidate) => candidate !== firstId)) {
      workspaceLayout = splitPane(workspaceLayout, id, firstId, zone);
    }
    splitMode = true;
    workspaceActive = true;
    broadcastPaneIds = [];
    activeTerminalId = validIds.at(-1) ?? firstId;
    workspaceFocusedPaneId = null;
  }

  function dissolveWorkspaceIfNeeded() {
    if (workspacePaneIds.length >= 2) return;
    const remainingId = workspacePaneIds[0] ?? null;
    const workspaceIndex = topTabOrder.indexOf("workspace");
    topTabOrder = topTabOrder.filter((id) => id !== "workspace");
    if (remainingId && !topTabOrder.includes(remainingId)) {
      topTabOrder.splice(
        workspaceIndex >= 0 ? Math.min(workspaceIndex, topTabOrder.length) : topTabOrder.length,
        0,
        remainingId
      );
    }
    workspacePaneIds = [];
    workspaceLayout = null;
    splitMode = false;
    workspaceActive = false;
    broadcastPaneIds = [];
    workspaceFocusedPaneId = null;
  }

  function registerTerminalSession(paneId: string, sessionId: string) {
    terminalSessionIds = { ...terminalSessionIds, [paneId]: sessionId };
    const tab = terminalTabs.find((candidate) => candidate.id === paneId);
    if (tab) tab.resumeSessionId = null;
  }

  function unregisterTerminalSession(paneId: string) {
    const { [paneId]: _, ...remaining } = terminalSessionIds;
    terminalSessionIds = remaining;
  }

  function activeTerminalTab(): TerminalTab | null {
    return terminalTabs.find((tab) => tab.id === activeTerminalId) ?? null;
  }

  function activeTerminalAppearance(): TerminalAppearance {
    return activeTerminalTab()?.appearance ?? defaultTerminalAppearance();
  }

  function openTerminalTools() {
    if (terminalToolsCloseTimer !== null) {
      window.clearTimeout(terminalToolsCloseTimer);
      terminalToolsCloseTimer = null;
    }
    terminalToolsMounted = true;
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        terminalToolsOpen = true;
      });
    });
  }

  function closeTerminalTools() {
    terminalToolsOpen = false;
    if (terminalToolsCloseTimer !== null) {
      window.clearTimeout(terminalToolsCloseTimer);
    }
    terminalToolsCloseTimer = window.setTimeout(() => {
      terminalToolsMounted = false;
      terminalToolsCloseTimer = null;
    }, 230);
  }

  function toggleTerminalTools() {
    if (terminalToolsOpen) closeTerminalTools();
    else openTerminalTools();
  }

  function terminalToolsStyle(): string {
    const theme = terminalTheme(activeTerminalAppearance().theme);
    return [
      `--terminal-tool-background:${theme.palette.background}`,
      `--terminal-tool-foreground:${theme.palette.foreground}`,
      `--terminal-tool-muted:${theme.chrome.headerMuted}`,
      `--terminal-tool-border:${theme.chrome.headerBorder}`,
      `--terminal-tool-card:${theme.chrome.headerBackground}`,
      `--terminal-tool-hover:${theme.chrome.controlHover}`,
      `--terminal-tool-active:${theme.chrome.activeBackground}`,
      `--terminal-tool-accent:${theme.chrome.activeForeground}`,
      `--terminal-tool-selection:${theme.palette.selectionBackground ?? theme.chrome.activeBackground}`
    ].join(";");
  }

  function updateTerminalAppearance(patch: Partial<TerminalAppearance>) {
    const tab = activeTerminalTab();
    if (!tab) return;
    tab.appearance = {
      ...tab.appearance,
      ...patch,
      fontSize: Math.max(9, Math.min(32, patch.fontSize ?? tab.appearance.fontSize))
    };
    if (!tab.host) {
      localStorage.setItem(localTerminalAppearanceKey, JSON.stringify(tab.appearance));
      return;
    }
    tab.host.terminal_theme = tab.appearance.theme;
    tab.host.terminal_font_size = tab.appearance.fontSize;
    const storedHost = hosts.find((host) => host.id === tab.host?.id);
    if (storedHost) {
      storedHost.terminal_theme = tab.appearance.theme;
      storedHost.terminal_font_size = tab.appearance.fontSize;
    }
    if (selected?.id === tab.host.id) {
      selected.terminal_theme = tab.appearance.theme;
      selected.terminal_font_size = tab.appearance.fontSize;
    }
    const hostId = tab.host.id;
    const existingTimer = appearanceSaveTimers.get(hostId);
    if (existingTimer !== undefined) window.clearTimeout(existingTimer);
    const snapshot = cloneHost(tab.host);
    const timer = window.setTimeout(() => {
      appearanceSaveTimers.delete(hostId);
      void saveHost(snapshot)
        .then((saved) => {
          const index = hosts.findIndex((host) => host.id === saved.id);
          if (index >= 0) hosts[index] = saved;
        })
        .catch((cause) => showMessage(cause, true));
    }, 260);
    appearanceSaveTimers.set(hostId, timer);
  }

  function requestTerminalCommand(paneId: string, command: string, run: boolean) {
    if (!command.trim() || !terminalTabs.some((tab) => tab.id === paneId)) return;
    terminalCommandRequests = {
      ...terminalCommandRequests,
      [paneId]: {
        id: crypto.randomUUID(),
        command,
        run
      }
    };
  }

  function runInActiveTerminal(command: string, run: boolean) {
    if (!activeTerminalId) return;
    requestTerminalCommand(activeTerminalId, command, run);
  }

  function runInAllTerminals(command: string) {
    for (const tab of terminalTabs) {
      requestTerminalCommand(tab.id, command, true);
    }
  }

  function commandHistoryRecorded() {
    terminalHistoryVersion += 1;
  }

  function broadcastTerminalBytes(sourcePaneId: string, bytes: number[]) {
    if (!broadcastPaneIds.includes(sourcePaneId) || bytes.length === 0) return;
    for (const [paneId, sessionId] of Object.entries(terminalSessionIds)) {
      if (paneId !== sourcePaneId && broadcastPaneIds.includes(paneId)) {
        void writeTerminal(sessionId, bytes).catch((cause) => {
          console.error("Broadcast write failed", cause);
        });
      }
    }
  }

  function toggleSplit() {
    if (splitMode) {
      activateWorkspace();
      return;
    }
    const paneIds = standaloneTerminalTabs().map((tab) => tab.id);
    if (paneIds.length < 2) return;
    createWorkspace(paneIds);
  }

  async function toggleBroadcast(paneId = activeTerminalId) {
    if (!paneId || !workspacePaneIds.includes(paneId)) return;
    if (broadcastPaneIds.length === 0) {
      if (!splitMode) toggleSplit();
      if (!splitMode || workspacePaneIds.length < 2) return;
      if (
        !(await confirmDialog({
          title: "Enable broadcast input?",
          message:
            "Every keystroke will be sent to all open terminal panes. Commands may run on multiple hosts.",
          confirmLabel: "Enable"
        }))
      ) {
        return;
      }
      workspaceActive = true;
      broadcastPaneIds = [...workspacePaneIds];
      return;
    }
    broadcastPaneIds = broadcastPaneIds.includes(paneId)
      ? broadcastPaneIds.filter((id) => id !== paneId)
      : [...broadcastPaneIds, paneId];
  }

  async function renameWorkspace() {
    const name = (
      await promptDialog({
        title: "Rename workspace",
        label: "Workspace name",
        initialValue: workspaceName,
        confirmLabel: "Rename"
      })
    )?.trim();
    if (!name) return;
    workspaceName = name;
  }

  function finishTabDrag() {
    draggedTabId = null;
    paneDropTarget = null;
    paneExtractTarget = false;
    topTabDropTarget = null;
  }

  function extractWorkspacePane(sourceId: string) {
    if (!workspacePaneIds.includes(sourceId)) {
      finishTabDrag();
      return;
    }
    const workspaceIndex = topTabOrder.indexOf("workspace");
    workspacePaneIds = workspacePaneIds.filter((id) => id !== sourceId);
    broadcastPaneIds = broadcastPaneIds.filter((id) => id !== sourceId);
    workspaceLayout = removeLayoutPane(workspaceLayout, sourceId);
    topTabOrder = topTabOrder.filter((id) => id !== sourceId);
    topTabOrder.splice(
      workspaceIndex >= 0 ? workspaceIndex + 1 : topTabOrder.length,
      0,
      sourceId
    );
    dissolveWorkspaceIfNeeded();
    activeTerminalId = sourceId;
    workspaceActive = false;
    workspaceFocusedPaneId = null;
    page = "terminal";
    finishTabDrag();
  }

  function updatePaneDropTarget(
    clientX: number,
    clientY: number,
    element: HTMLDivElement,
    paneId: string
  ) {
    const rect = element.getBoundingClientRect();
    const x = (clientX - rect.left) / rect.width;
    const y = (clientY - rect.top) / rect.height;
    const distances: Array<[DropZone, number]> = [
      ["left", x],
      ["right", 1 - x],
      ["top", y],
      ["bottom", 1 - y]
    ];
    distances.sort((left, right) => left[1] - right[1]);
    paneDropTarget = { paneId, zone: distances[0][0] };
  }

  function movePaneIntoPane(
    sourceId: string | null | undefined,
    targetId: string,
    zone: DropZone
  ) {
    if (!sourceId || sourceId === targetId) {
      finishTabDrag();
      return;
    }
    if (!workspacePaneIds.includes(targetId)) {
      createWorkspace([targetId, sourceId], targetId, zone);
      finishTabDrag();
      return;
    }
    if (!workspacePaneIds.includes(sourceId)) {
      workspacePaneIds = [...workspacePaneIds, sourceId];
      topTabOrder = topTabOrder.filter((id) => id !== sourceId);
      if (!topTabOrder.includes("workspace")) topTabOrder.push("workspace");
    }
    workspaceLayout = splitPane(workspaceLayout, sourceId, targetId, zone);
    const order = leafOrder(workspaceLayout);
    workspacePaneIds.sort((left, right) => order.indexOf(left) - order.indexOf(right));
    splitMode = true;
    workspaceActive = true;
    activeTerminalId = sourceId;
    workspaceFocusedPaneId = null;
    finishTabDrag();
  }

  function startPanePointerDrag(event: PointerEvent, sourceId: string) {
    if (event.button !== 0) return;
    event.preventDefault();
    workspaceFocusedPaneId = null;
    activeTerminalId = sourceId;
    panePointerDrag = {
      sourceId,
      startX: event.clientX,
      startY: event.clientY,
      currentX: event.clientX,
      currentY: event.clientY,
      dragging: false
    };
  }

  function movePanePointerDrag(event: PointerEvent) {
    if (!panePointerDrag) return;
    panePointerDrag.currentX = event.clientX;
    panePointerDrag.currentY = event.clientY;
    const moved = Math.hypot(
      event.clientX - panePointerDrag.startX,
      event.clientY - panePointerDrag.startY
    );
    if (!panePointerDrag.dragging && moved < 6) return;
    event.preventDefault();
    panePointerDrag.dragging = true;
    draggedTabId = panePointerDrag.sourceId;
    const target = document
      .elementsFromPoint(event.clientX, event.clientY)
      .map((element) => element.closest<HTMLElement>(".terminal-instance"))
      .find((element) => element?.dataset.paneId !== undefined);
    const targetId = target?.dataset.paneId;
    if (!target || !targetId || targetId === panePointerDrag.sourceId) {
      paneDropTarget = null;
      paneExtractTarget =
        workspacePaneIds.includes(panePointerDrag.sourceId) &&
        document
          .elementsFromPoint(event.clientX, event.clientY)
          .some((element) => Boolean(element.closest(".titlebar")));
      return;
    }
    paneExtractTarget = false;
    updatePaneDropTarget(
      event.clientX,
      event.clientY,
      target as HTMLDivElement,
      targetId
    );
  }

  function finishPanePointerDrag() {
    if (!panePointerDrag) return;
    const target = paneDropTarget;
    const extract = paneExtractTarget;
    const sourceId = panePointerDrag.sourceId;
    panePointerDrag = null;
    if (target) {
      movePaneIntoPane(sourceId, target.paneId, target.zone);
    } else if (extract) {
      extractWorkspacePane(sourceId);
    } else {
      finishTabDrag();
    }
  }

  function startTopTabNativeDrag(event: DragEvent, sourceId: string) {
    if (
      (event.target as HTMLElement).closest(".terminal-tab-close") ||
      !event.dataTransfer
    ) {
      event.preventDefault();
      return;
    }
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData(terminalTabDragMime, sourceId);
    topTabPointerDrag = {
      sourceId,
      startX: event.clientX,
      startY: event.clientY,
      currentX: event.clientX,
      currentY: event.clientY,
      screenX: event.screenX,
      screenY: event.screenY,
      dragging: true
    };
    draggedTabId = sourceId;
    const terminalTab = terminalTabs.find((tab) => tab.id === sourceId);
    const icon =
      sourceId === "vault"
        ? "vault"
        : sourceId === "sftp"
          ? "folder"
          : sourceId === "workspace"
            ? "grid"
            : "terminal";
    const label =
      sourceId === "vault"
        ? "Vaults"
        : sourceId === "sftp"
          ? "SFTP"
          : sourceId === "workspace"
            ? workspaceName || "Workspace"
            : terminalTab?.title ?? "Terminal";
    const themedTab =
      terminalTab ??
      terminalTabs.find((tab) => tab.id === activeTerminalId) ??
      null;
    const theme = terminalTheme(themedTab?.appearance.theme);
    setNativeTabDragPreview(event, {
      label,
      icon,
      background: theme.chrome.headerBackground,
      activeBackground: theme.chrome.activeBackground,
      foreground: theme.chrome.headerForeground,
      border: theme.chrome.headerBorder
    });
  }

  function moveTopTabNativeDrag(event: DragEvent) {
    if (!topTabPointerDrag) return;
    if (event.clientX !== 0 || event.clientY !== 0) {
      topTabPointerDrag.currentX = event.clientX;
      topTabPointerDrag.currentY = event.clientY;
    }
    if (event.screenX !== 0 || event.screenY !== 0) {
      topTabPointerDrag.screenX = event.screenX;
      topTabPointerDrag.screenY = event.screenY;
    }
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    const elements = document.elementsFromPoint(event.clientX, event.clientY);
    const sourceIsTerminal = terminalTabs.some((tab) => tab.id === topTabPointerDrag?.sourceId);
    const paneTarget = elements
      .map((element) => element.closest<HTMLElement>(".terminal-instance"))
      .find((element) => element?.dataset.paneId !== undefined);
    const paneId = paneTarget?.dataset.paneId;
    if (
      sourceIsTerminal &&
      paneTarget &&
      paneId &&
      paneId !== topTabPointerDrag.sourceId
    ) {
      topTabDropTarget = null;
      updatePaneDropTarget(
        event.clientX,
        event.clientY,
        paneTarget as HTMLDivElement,
        paneId
      );
      return;
    }
    paneDropTarget = null;
    const tabTarget = elements
      .map((element) => element.closest<HTMLElement>("[data-top-tab-id]"))
      .find((element) => element?.dataset.topTabId !== undefined);
    const targetId = tabTarget?.dataset.topTabId;
    if (!tabTarget || !targetId || targetId === topTabPointerDrag.sourceId) {
      topTabDropTarget = null;
      return;
    }
    const bounds = tabTarget.getBoundingClientRect();
    topTabDropTarget = {
      id: targetId,
      after: event.clientX > bounds.left + bounds.width / 2,
      merge:
        targetId === "workspace" &&
        sourceIsTerminal &&
        !workspacePaneIds.includes(topTabPointerDrag.sourceId)
    };
  }

  function finishTopTabDrop(event: DragEvent) {
    if (!topTabPointerDrag) return;
    const sourceId = topTabPointerDrag.sourceId;
    const paneTarget = paneDropTarget;
    const tabTarget = topTabDropTarget;
    topTabPointerDrag = null;
    suppressTopTabClick = true;
    window.setTimeout(() => (suppressTopTabClick = false), 0);
    if (paneTarget) {
      movePaneIntoPane(sourceId, paneTarget.paneId, paneTarget.zone);
      return;
    }
    if (tabTarget?.merge) {
      const targetId =
        workspacePaneIds.includes(activeTerminalId ?? "")
          ? activeTerminalId
          : workspacePaneIds[0];
      if (targetId) movePaneIntoPane(sourceId, targetId, "right");
      else finishTabDrag();
      return;
    }
    if (tabTarget) reorderTopTab(sourceId, tabTarget.id, tabTarget.after);
    finishTabDrag();
  }

  function finishTopTabNativeDrag(event: DragEvent) {
    if (!topTabPointerDrag) return;
    const sourceId = topTabPointerDrag.sourceId;
    const screenX =
      event.screenX !== 0 || event.screenY !== 0
        ? event.screenX
        : topTabPointerDrag.screenX;
    const screenY =
      event.screenX !== 0 || event.screenY !== 0
        ? event.screenY
        : topTabPointerDrag.screenY;
    topTabPointerDrag = null;
    finishTabDrag();
    if (
      Number.isFinite(screenX) &&
      Number.isFinite(screenY) &&
      (screenX !== 0 || screenY !== 0) &&
      transferableTopTab(sourceId)
    ) {
      void transferDraggedTopTab(sourceId, screenX, screenY);
    }
  }

  function reorderTopTab(sourceId: string, targetId: string, after: boolean) {
    if (
      sourceId === targetId ||
      !topTabOrder.includes(sourceId) ||
      !topTabOrder.includes(targetId)
    ) return;
    topTabOrder = topTabOrder.filter((id) => id !== sourceId);
    const targetIndex = topTabOrder.indexOf(targetId);
    topTabOrder.splice(targetIndex + (after ? 1 : 0), 0, sourceId);
  }

  function handleGlobalPointerMove(event: PointerEvent) {
    movePanePointerDrag(event);
  }

  function handleGlobalPointerEnd(event: PointerEvent) {
    finishPanePointerDrag();
  }

  function handleGlobalPointerCancel(event: PointerEvent) {
    finishPanePointerDrag();
  }

  function hasTerminalTabDrag(event: DragEvent): boolean {
    return Array.from(event.dataTransfer?.types ?? []).includes(terminalTabDragMime);
  }

  function handleGlobalTabDrag(event: DragEvent) {
    if (!topTabPointerDrag) return;
    if (event.clientX !== 0 || event.clientY !== 0) {
      topTabPointerDrag.currentX = event.clientX;
      topTabPointerDrag.currentY = event.clientY;
    }
    if (event.screenX !== 0 || event.screenY !== 0) {
      topTabPointerDrag.screenX = event.screenX;
      topTabPointerDrag.screenY = event.screenY;
    }
  }

  function handleGlobalTabDragOver(event: DragEvent) {
    if (topTabPointerDrag) {
      moveTopTabNativeDrag(event);
      return;
    }
    if (hasTerminalTabDrag(event)) {
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    }
  }

  function handleGlobalTabDrop(event: DragEvent) {
    if (!topTabPointerDrag && !hasTerminalTabDrag(event)) return;
    event.preventDefault();
    if (topTabPointerDrag) finishTopTabDrop(event);
  }

  function terminalPaneStyle(id: string): string {
    if (!splitMode || !workspaceActive || !workspacePaneIds.includes(id)) return "";
    if (workspaceFocusedPaneId === id) {
      return "left:0;top:0;width:100%;height:100%";
    }
    const layout = normalizeLayout(workspaceLayout, workspacePaneIds);
    const rect = layoutRects(layout)[id] ?? { left: 0, top: 0, width: 100, height: 100 };
    return `left:${rect.left}%;top:${rect.top}%;width:${rect.width}%;height:${rect.height}%`;
  }

  function workspaceDividers(): SplitDivider[] {
    if (!splitMode || !workspaceActive || workspaceFocusedPaneId !== null) return [];
    const layout = normalizeLayout(workspaceLayout, workspacePaneIds);
    return layoutDividers(layout);
  }

  function dividerPath(divider: SplitDivider): string {
    return divider.path.join(".");
  }

  function activeDividerColor(): string {
    const activeTab =
      terminalTabs.find((tab) => tab.id === activeTerminalId) ??
      terminalTabs.find((tab) => workspacePaneIds.includes(tab.id));
    return terminalTheme(activeTab?.appearance.theme).chrome.activeForeground;
  }

  function dividerStyle(divider: SplitDivider): string {
    const themeStyle = `--workspace-divider-color:${activeDividerColor()}`;
    if (divider.direction === "horizontal") {
      return `${themeStyle};left:${divider.position}%;top:${divider.parent.top}%;height:${divider.parent.height}%`;
    }
    return `${themeStyle};left:${divider.parent.left}%;top:${divider.position}%;width:${divider.parent.width}%`;
  }

  function startDividerResize(event: PointerEvent, divider: SplitDivider) {
    event.preventDefault();
    event.stopPropagation();
    activeDividerPath = dividerPath(divider);
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function resizeDivider(event: PointerEvent, divider: SplitDivider) {
    if (
      activeDividerPath !== dividerPath(divider) ||
      !terminalGridElement
    ) return;
    event.preventDefault();
    const bounds = terminalGridElement.getBoundingClientRect();
    const pointerPercent =
      divider.direction === "horizontal"
        ? ((event.clientX - bounds.left) / bounds.width) * 100
        : ((event.clientY - bounds.top) / bounds.height) * 100;
    const origin =
      divider.direction === "horizontal" ? divider.parent.left : divider.parent.top;
    const span =
      divider.direction === "horizontal" ? divider.parent.width : divider.parent.height;
    workspaceLayout = setSplitRatio(
      workspaceLayout,
      divider.path,
      (pointerPercent - origin) / span
    );
  }

  function finishDividerResize(event: PointerEvent) {
    const element = event.currentTarget as HTMLElement;
    if (element.hasPointerCapture(event.pointerId)) {
      element.releasePointerCapture(event.pointerId);
    }
    activeDividerPath = null;
  }

  async function openSftp(hostId = "", autoConnect = false) {
    sftpHostId = hostId;
    sftpAutoConnect = autoConnect;
    if (!SftpComponent) {
      SftpComponent = (await import("./features/sftp/SftpPane.svelte")).default;
    }
    page = "sftp";
  }

  async function openSnippets() {
    if (!SnippetsComponent) {
      SnippetsComponent = (await import("./features/snippets/SnippetsPane.svelte")).default;
    }
    page = "snippets";
  }

  async function openForwarding() {
    if (!ForwardingComponent) {
      ForwardingComponent = (await import("./features/forwarding/ForwardingPane.svelte")).default;
    }
    page = "forwarding";
  }

  async function openKeychain() {
    if (!KeychainComponent) {
      KeychainComponent = (await import("./features/keychain/KeychainPane.svelte")).default;
    }
    page = "keychain";
  }

  async function openKnownHosts() {
    if (!KnownHostsComponent) {
      KnownHostsComponent = (await import("./features/known-hosts/KnownHostsPane.svelte")).default;
    }
    page = "known-hosts";
  }

  async function openLogs() {
    if (!LogsComponent) {
      LogsComponent = (await import("./features/logs/LogsPane.svelte")).default;
    }
    page = "logs";
  }

  function showMessage(cause: unknown, error: boolean) {
    message = cause instanceof Error ? cause.message : String(cause);
    messageIsError = error;
  }

  function setManagerEditorDirty(editor: MainPage, dirty: boolean) {
    if (managerEditorDirty[editor] === dirty) return;
    managerEditorDirty = { ...managerEditorDirty, [editor]: dirty };
  }

  function currentEditorDirty(): boolean {
    if (page === "hosts") return hostEditorDirty() || hostSubeditorDirty();
    return Boolean(managerEditorDirty[page]);
  }

  async function preparePageChange(next: MainPage): Promise<boolean> {
    if (next === page) return true;
    if (!(await confirmDiscardChanges(currentEditorDirty()))) return false;
    if (page === "hosts" && inspectorOpen) discardHostEditor();
    return true;
  }

  async function switchPage(next: MainPage) {
    if (!(await preparePageChange(next))) return;
    if (next === "terminal") {
      if (activeTerminalId) page = "terminal";
      else void openTerminal(null, true);
    }
    else if (next === "sftp") void openSftp();
    else if (next === "snippets") void openSnippets();
    else if (next === "forwarding") void openForwarding();
    else if (next === "keychain") void openKeychain();
    else if (next === "known-hosts") void openKnownHosts();
    else if (next === "logs") void openLogs();
    else page = next;
  }

  function formatHostMeta(host: Host): string {
    return `ssh, ${host.username} · ${host.address}`;
  }

  function localTerminalMatchesNewTabQuery(): boolean {
    const normalized = newTabQuery.trim().toLowerCase();
    return !normalized || ["local terminal", "local", "shell", "this device"].some(
      (value) => value.includes(normalized)
    );
  }

  function filteredNewTabHosts(): Host[] {
    const normalized = newTabQuery.trim().toLowerCase();
    return hosts.filter((host) =>
      [host.label, host.address, host.username, host.group_name ?? ""].some(
        (value) => value.toLowerCase().includes(normalized)
      )
    );
  }

  function openFirstNewTabResult() {
    if (localTerminalMatchesNewTabQuery()) {
      void openTerminal();
      return;
    }
    const firstHost = filteredNewTabHosts()[0];
    if (firstHost) void openTerminal(firstHost);
  }

  function workspaceTabOrder(): number {
    const position = topTabOrder.indexOf("workspace");
    return position >= 0 ? position : 2;
  }

  function topTabDragPreview(): { icon: string; label: string; style: string } {
    const drag =
      topTabPointerDrag?.dragging
        ? topTabPointerDrag
        : panePointerDrag?.dragging
          ? panePointerDrag
          : null;
    if (!drag) return { icon: "terminal", label: "", style: "" };
    const terminalTab = terminalTabs.find((tab) => tab.id === drag.sourceId);
    const icon =
      drag.sourceId === "vault"
        ? "vault"
        : drag.sourceId === "sftp"
          ? "folder"
          : drag.sourceId === "workspace"
            ? "grid"
            : "terminal";
    const label =
      drag.sourceId === "vault"
        ? "Vaults"
        : drag.sourceId === "sftp"
          ? "SFTP"
          : drag.sourceId === "workspace"
            ? workspaceName || "Workspace"
            : terminalTab?.title ?? "Terminal";
    const previewWidth = 168;
    const left = drag.currentX - previewWidth / 2;
    const top = drag.currentY - 16;
    const themedTab =
      terminalTab ??
      terminalTabs.find((tab) => tab.id === activeTerminalId) ??
      null;
    const theme = terminalTheme(themedTab?.appearance.theme);
    const style = [
      `left:${left}px`,
      `top:${top}px`,
      `--drag-tab-background:${theme.chrome.headerBackground}`,
      `--drag-tab-active:${theme.chrome.activeBackground}`,
      `--drag-tab-foreground:${theme.chrome.headerForeground}`,
      `--drag-tab-border:${theme.chrome.headerBorder}`
    ].join(";");
    return { icon, label, style };
  }

  function singleTerminalChromeStyle(): string {
    if (page !== "terminal" || workspaceActive || !activeTerminalId) return "";
    const tab = terminalTabs.find((candidate) => candidate.id === activeTerminalId);
    if (!tab) return "";
    const theme = terminalTheme(tab.appearance.theme);
    return [
      `--app-terminal-background:${theme.palette.background}`,
      `--app-terminal-header-foreground:${theme.chrome.headerForeground}`,
      `--app-terminal-header-muted:${theme.chrome.headerMuted}`,
      `--app-terminal-header-border:${theme.chrome.headerBorder}`,
      `--app-terminal-header-hover:${theme.chrome.controlHover}`,
      `--app-terminal-header-active:${theme.chrome.activeBackground}`,
      `--app-terminal-header-accent:${theme.chrome.activeForeground}`
    ].join(";");
  }

  function cloneHost(host: Host): Host {
    return {
      ...host,
      tags: [...host.tags],
      jump_host_ids: [...host.jump_host_ids],
      environment: host.environment.map((variable) => ({ ...variable }))
    };
  }

  function startWindowResize(
    event: PointerEvent,
    direction: WindowResizeDirection
  ) {
    if (event.button !== 0 || !appWindow) return;
    event.preventDefault();
    event.stopPropagation();
    void appWindow.startResizeDragging(direction);
  }
</script>

<svelte:window
  onclick={() => {
    hostContextMenu = null;
    bulkHostContextMenu = null;
    groupContextMenu = null;
    terminalContextMenu = null;
    hostEditorMenuOpen = false;
    newHostMenuOpen = false;
    hostTagFilterOpen = false;
    hostTagInputFocused = false;
  }}
  onpointermove={handleGlobalPointerMove}
  onpointerup={handleGlobalPointerEnd}
  onpointercancel={handleGlobalPointerCancel}
  ondrag={handleGlobalTabDrag}
  ondragover={handleGlobalTabDragOver}
  ondrop={handleGlobalTabDrop}
/>

<div
  class="application"
  class:detached-window={detachedMode}
  class:detached-initialized={detachedInitialized}
  class:terminal-surface={page === "terminal"}
  class:single-terminal-surface={page === "terminal" && !workspaceActive && Boolean(activeTerminalId)}
  class:dragging-terminal-tab={Boolean(panePointerDrag?.dragging)}
  style={singleTerminalChromeStyle()}
>
  <div
    class="window-resize-handle resize-north"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "North")}
  ></div>
  <div
    class="window-resize-handle resize-east"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "East")}
  ></div>
  <div
    class="window-resize-handle resize-south"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "South")}
  ></div>
  <div
    class="window-resize-handle resize-west"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "West")}
  ></div>
  <div
    class="window-resize-handle resize-north-west"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "NorthWest")}
  ></div>
  <div
    class="window-resize-handle resize-north-east"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "NorthEast")}
  ></div>
  <div
    class="window-resize-handle resize-south-east"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "SouthEast")}
  ></div>
  <div
    class="window-resize-handle resize-south-west"
    aria-hidden="true"
    onpointerdown={(event) => startWindowResize(event, "SouthWest")}
  ></div>
  <header class="titlebar" class:pane-extract-target={paneExtractTarget}>
    {#if !detachedMode}
      <button class="title-icon" title="Menu"><Icon name="menu" size={20} /></button>
    {/if}
    <nav class="top-tabs" aria-label="Main tabs">
      {#if !detachedMode}
        <button
          use:observeTopTab
          class:active={vaultPages.has(page)}
          class:dragging={draggedTabId === "vault"}
          class:drop-before={topTabDropTarget?.id === "vault" && !topTabDropTarget.after}
          class:drop-after={topTabDropTarget?.id === "vault" && topTabDropTarget.after}
          data-top-tab-id="vault"
          style:order={topTabOrder.indexOf("vault")}
          draggable="true"
          ondragstart={(event) => startTopTabNativeDrag(event, "vault")}
          ondragend={finishTopTabNativeDrag}
          onclick={() => {
            if (!suppressTopTabClick) switchPage("hosts");
          }}
          aria-current={vaultPages.has(page) ? "page" : undefined}
        >
          <Icon name="vault" /><span>Vaults</span>
        </button>
        <button
          use:observeTopTab
          class:active={page === "sftp"}
          class:dragging={draggedTabId === "sftp"}
          class:drop-before={topTabDropTarget?.id === "sftp" && !topTabDropTarget.after}
          class:drop-after={topTabDropTarget?.id === "sftp" && topTabDropTarget.after}
          data-top-tab-id="sftp"
          style:order={topTabOrder.indexOf("sftp")}
          draggable="true"
          ondragstart={(event) => startTopTabNativeDrag(event, "sftp")}
          ondragend={finishTopTabNativeDrag}
          onclick={() => {
            if (!suppressTopTabClick) switchPage("sftp");
          }}
          aria-current={page === "sftp" ? "page" : undefined}
        >
          <Icon name="folder" /><span>SFTP</span>
        </button>
      {/if}
      {#if splitMode && workspacePaneIds.length > 1}
        <div
          use:observeTopTab
          class="terminal-tab-shell"
          class:expanded={page === "terminal" && workspaceActive}
          role="group"
          aria-label="Workspace terminal tab"
          class:dragging={draggedTabId === "workspace"}
          class:drop-before={topTabDropTarget?.id === "workspace" && !topTabDropTarget.after && !topTabDropTarget.merge}
          class:drop-after={topTabDropTarget?.id === "workspace" && topTabDropTarget.after && !topTabDropTarget.merge}
          class:drop-merge={topTabDropTarget?.id === "workspace" && topTabDropTarget.merge}
          data-top-tab-id="workspace"
          style:order={workspaceTabOrder()}
          draggable="true"
          ondragstart={(event) => startTopTabNativeDrag(event, "workspace")}
          ondragend={finishTopTabNativeDrag}
          oncontextmenu={(event) => openTerminalContextMenu(event, {
            id: "workspace",
            title: workspaceName,
            host: null,
            appearance: activeTerminalAppearance(),
            resumeSessionId: null
          })}
        >
          <button
            class="terminal-tab"
            class:active={page === "terminal" && workspaceActive}
            aria-current={page === "terminal" && workspaceActive ? "page" : undefined}
            onclick={() => {
              if (!suppressTopTabClick) activateWorkspace();
            }}
            onauxclick={(event) => event.button === 1 && closeCurrentWorkspace()}
          >
            <Icon name="grid" /><span>{workspaceName || "Workspace"}</span>
          </button>
          <button
            class="terminal-tab-close"
            title="Close workspace"
            onclick={(event) => {
              event.stopPropagation();
              closeCurrentWorkspace();
            }}
          ><Icon name="close" size={13} /></button>
        </div>
      {/if}
      {#each standaloneTerminalTabs() as tab (tab.id)}
        <div
          use:observeTopTab
          class="terminal-tab-shell"
          class:expanded={page === "terminal" && !workspaceActive && activeTerminalId === tab.id}
          role="group"
          aria-label={`${tab.title} terminal tab`}
          class:dragging={draggedTabId === tab.id}
          class:drop-before={topTabDropTarget?.id === tab.id && !topTabDropTarget.after}
          class:drop-after={topTabDropTarget?.id === tab.id && topTabDropTarget.after}
          data-top-tab-id={tab.id}
          style:order={topTabOrder.indexOf(tab.id)}
          draggable="true"
          ondragstart={(event) => startTopTabNativeDrag(event, tab.id)}
          ondragend={finishTopTabNativeDrag}
          oncontextmenu={(event) => openTerminalContextMenu(event, tab)}
        >
          <button
            class="terminal-tab"
            class:active={page === "terminal" && !workspaceActive && activeTerminalId === tab.id}
            aria-current={page === "terminal" && !workspaceActive && activeTerminalId === tab.id ? "page" : undefined}
            onclick={() => {
              if (!suppressTopTabClick) activateStandaloneTerminal(tab.id);
            }}
            onauxclick={(event) => {
              if (event.button === 1) closeTerminalTab(tab.id);
            }}
          >
            <HostIcon hostId={tab.host?.id} /><span>{tab.title}</span>
          </button>
          <button
            class="terminal-tab-close"
            title={`Close ${tab.title}`}
            onclick={(event) => {
              event.stopPropagation();
              closeTerminalTab(tab.id);
            }}
          ><Icon name="close" size={13} /></button>
        </div>
      {/each}
    </nav>
    <button class="add-tab" title="New tab" onclick={() => switchPage("new-tab")}>
      <Icon name="plus" />
    </button>
    <div class="titlebar-space" data-tauri-drag-region></div>
    {#if !detachedMode}
      <button class="title-icon" title="Notifications"><Icon name="bell" size={18} /></button>
    {/if}
    {#if page === "terminal" && activeTerminalId}
      <button
        class="title-icon terminal-tools-toggle"
        class:active={terminalToolsOpen}
        title={terminalToolsOpen ? "Close terminal tools" : "Open terminal tools"}
        aria-pressed={terminalToolsOpen}
        onclick={toggleTerminalTools}
      ><Icon name="sidebar" size={18} /></button>
    {/if}
    <div class="window-controls">
      <button title="Minimize" onclick={() => appWindow?.minimize()}><Icon name="minimize" /></button>
      <button title="Maximize" onclick={() => appWindow?.toggleMaximize()}><Icon name="maximize" size={16} /></button>
      <button class="close" title="Close" onclick={() => appWindow?.close()}><Icon name="close" /></button>
    </div>
  </header>

  {#if panePointerDrag?.dragging}
    {@const dragPreview = topTabDragPreview()}
    <div
      class="top-tab-drag-preview"
      style={dragPreview.style}
      aria-hidden="true"
    >
      <Icon name={dragPreview.icon} size={16} />
      <span>{dragPreview.label}</span>
    </div>
  {/if}

  <div class="workspace">
    {#if vaultPages.has(page)}
      <aside class="sidebar">
        <nav aria-label="Vault sections">
          {#each navigation as item}
            <button
              class:active={page === item.page}
              onclick={() => switchPage(item.page)}
              aria-current={page === item.page ? "page" : undefined}
            >
              <Icon name={item.icon} size={19} />
              <span>{item.label}</span>
            </button>
          {/each}
        </nav>
        <div class="sidebar-footer">
          <span class="sync-dot"></span>
          <span>Local vault</span>
        </div>
      </aside>
    {/if}

    {#if terminalTabs.length > 0}
      <main
        class:hidden-page={page !== "terminal"}
        class:with-terminal-tools={terminalToolsMounted && page === "terminal" && Boolean(activeTerminalId)}
        class="full-page terminal-page persistent-terminals"
        style={terminalToolsStyle()}
      >
        <div
          class:split-workspace={splitMode && workspaceActive}
          class:resizing-workspace={activeDividerPath !== null}
          class:dragging-workspace-pane={panePointerDrag?.dragging}
          class="terminal-grid"
          bind:this={terminalGridElement}
        >
          {#each terminalTabs as tab (tab.id)}
            <div
              role="region"
              aria-label={`${tab.title} terminal pane`}
              class:hidden-terminal={workspaceActive
                ? !workspacePaneIds.includes(tab.id)
                : activeTerminalId !== tab.id}
              class:hidden-workspace-pane={workspaceActive
                && workspacePaneIds.includes(tab.id)
                && workspaceFocusedPaneId !== null
                && workspaceFocusedPaneId !== tab.id}
              class:active-pane={workspaceActive && activeTerminalId === tab.id}
              class:drag-source={panePointerDrag?.dragging && panePointerDrag.sourceId === tab.id}
              class:drop-left={paneDropTarget?.paneId === tab.id && paneDropTarget.zone === "left"}
              class:drop-right={paneDropTarget?.paneId === tab.id && paneDropTarget.zone === "right"}
              class:drop-top={paneDropTarget?.paneId === tab.id && paneDropTarget.zone === "top"}
              class:drop-bottom={paneDropTarget?.paneId === tab.id && paneDropTarget.zone === "bottom"}
              class="terminal-instance"
              data-pane-id={tab.id}
              style={terminalPaneStyle(tab.id)}
            >
              {#if TerminalComponent}
                <TerminalComponent
                  paneId={tab.id}
                  title={tab.title}
                  host={tab.host}
                  appearance={tab.appearance}
                  snippetVersion={terminalSnippetVersion}
                  historyVersion={terminalHistoryVersion}
                  resumeSessionId={tab.resumeSessionId}
                  commandRequest={terminalCommandRequests[tab.id] ?? null}
                  workspace={splitMode && workspaceActive && workspacePaneIds.includes(tab.id)}
                  broadcast={broadcastPaneIds.includes(tab.id)}
                  onClose={() => closeTerminalTab(tab.id)}
                  onBroadcast={() => toggleBroadcast(tab.id)}
                  onFocus={() => focusWorkspacePane(tab.id)}
                  onHeaderPointerDown={(event: PointerEvent) => startPanePointerDrag(event, tab.id)}
                  onSessionReady={registerTerminalSession}
                  onSessionClosed={unregisterTerminalSession}
                  onUserInput={broadcastTerminalBytes}
                  onActivate={activateTerminalPane}
                  onCommandRecorded={commandHistoryRecorded}
                  shouldPreserveSession={() => detachingPaneIds.includes(tab.id)}
                />
              {:else}
                <div class="terminal-runtime-state" class:error={Boolean(terminalLoadError)}>
                  <Icon name="terminal" size={25} />
                  <strong>{terminalLoadError ? "Terminal runtime failed" : "Loading terminal…"}</strong>
                  {#if terminalLoadError}<span>{terminalLoadError}</span>{/if}
                </div>
              {/if}
            </div>
          {/each}
          {#each workspaceDividers() as divider (dividerPath(divider))}
            <div
              class="workspace-divider"
              class:horizontal={divider.direction === "horizontal"}
              class:vertical={divider.direction === "vertical"}
              class:active={activeDividerPath === dividerPath(divider)}
              style={dividerStyle(divider)}
              role="separator"
              aria-orientation={divider.direction === "horizontal" ? "vertical" : "horizontal"}
              aria-label="Resize terminal panes"
              onpointerdown={(event) => startDividerResize(event, divider)}
              onpointermove={(event) => resizeDivider(event, divider)}
              onpointerup={finishDividerResize}
              onpointercancel={finishDividerResize}
            ></div>
          {/each}
        </div>
        {#if terminalToolsMounted && page === "terminal" && activeTerminalId}
          <div
            class="terminal-tools-slot"
            class:open={terminalToolsOpen}
          >
            <TerminalToolsSidebar
              appearance={activeTerminalAppearance()}
              historyVersion={terminalHistoryVersion}
              onappearancechange={updateTerminalAppearance}
              onrun={(command) => runInActiveTerminal(command, true)}
              onpaste={(command) => runInActiveTerminal(command, false)}
              onrunall={runInAllTerminals}
              onclose={closeTerminalTools}
              onopenfull={() => {
                closeTerminalTools();
                void openSnippets();
              }}
            />
          </div>
        {/if}
      </main>
    {/if}

    {#if page === "hosts"}
      <main class="host-workspace" class:with-inspector={inspectorOpen}>
        <section class="host-center">
          <div class="search-row">
            <Icon name="search" size={19} />
            <input
              aria-label="Find a host"
              placeholder="Find a host or type user@hostname…"
              bind:value={query}
              onkeydown={(event) => {
                if (event.key === "Enter" && query.trim()) {
                  event.preventDefault();
                  void openQuickConnectEditor();
                }
              }}
            />
            <button
              class="search-connect"
              disabled={!query.trim()}
              onclick={() => void openQuickConnectEditor()}
            >Connect</button>
          </div>
          <div class="content-toolbar">
            <div class="split-create">
              <button class="secondary-action split-create-main" onclick={newHost}>
                <Icon name="plus" /> New host
              </button>
              <button
                class="secondary-action split-create-toggle"
                class:active={newHostMenuOpen}
                title="More host options"
                aria-label="More host options"
                aria-expanded={newHostMenuOpen}
                onclick={(event) => {
                  event.stopPropagation();
                  newHostMenuOpen = !newHostMenuOpen;
                }}
              ><Icon name="chevron" size={16} /></button>
              {#if newHostMenuOpen}
                <div class="create-dropdown host-create-dropdown">
                  <button onclick={createGroup}>
                    <Icon name="folder" size={17} /><span>New Group</span>
                  </button>
                </div>
              {/if}
            </div>
            <button class="quiet-button" onclick={() => openTerminal()}><Icon name="terminal" /> Terminal</button>
            <div class="toolbar-spacer"></div>
            <CollectionControls
              view={hostCollectionView}
              sort={hostCollectionSort}
              onviewchange={(view) => (hostCollectionView = view)}
              onsortchange={(sort) => (hostCollectionSort = sort)}
            />
            <div class="host-tag-filter-anchor">
              <button
                class="icon-button"
                class:active={hostTagFilterOpen || selectedHostTags.length > 0}
                title="Filter by tags"
                aria-label="Filter hosts by tags"
                aria-expanded={hostTagFilterOpen}
                onclick={(event) => {
                  event.stopPropagation();
                  hostTagFilterOpen = !hostTagFilterOpen;
                }}
              >
                <Icon name="tag" />
                {#if selectedHostTags.length > 0}
                  <span class="tag-filter-count">{selectedHostTags.length}</span>
                {/if}
              </button>
              {#if hostTagFilterOpen}
                <div
                  class="host-tag-filter-menu"
                  role="dialog"
                  aria-label="Filter hosts by tags"
                  tabindex="-1"
                  onclick={(event) => event.stopPropagation()}
                  onkeydown={(event) => event.stopPropagation()}
                >
                  <label class="tag-filter-search">
                    <Icon name="search" size={17} />
                    <input
                      aria-label="Search tags"
                      placeholder="Search tags"
                      bind:value={hostTagFilterQuery}
                    />
                  </label>
                  <div class="tag-filter-list">
                    {#each visibleHostTagFilters() as tag (tag)}
                      <div class="tag-filter-row">
                        <button
                          class="tag-filter-toggle"
                          class:active={selectedHostTags.some((candidate) => candidate.toLocaleLowerCase() === tag.toLocaleLowerCase())}
                          onclick={() => toggleHostTagFilter(tag)}
                        >
                          <span class="tag-filter-check">
                            {#if selectedHostTags.some((candidate) => candidate.toLocaleLowerCase() === tag.toLocaleLowerCase())}
                              <Icon name="check" size={14} strokeWidth={2.4} />
                            {/if}
                          </span>
                          <strong>{tag}</strong>
                        </button>
                        <div class="tag-filter-actions">
                          <button title={`Rename ${tag}`} aria-label={`Rename ${tag}`} onclick={() => void renameSavedHostTag(tag)}>
                            <Icon name="edit" size={16} />
                          </button>
                          <button class="danger" title={`Delete ${tag}`} aria-label={`Delete ${tag}`} onclick={() => void deleteSavedHostTag(tag)}>
                            <Icon name="trash" size={16} />
                          </button>
                        </div>
                      </div>
                    {:else}
                      <p class="tag-filter-empty">No tags found</p>
                    {/each}
                  </div>
                  <button
                    class="tag-filter-clear"
                    disabled={selectedHostTags.length === 0}
                    onclick={() => {
                      selectedHostTags = [];
                      selectedHostIds = [];
                    }}
                  >Clear selection</button>
                </div>
              {/if}
            </div>
          </div>
          <div
            class="host-scroll"
            role="region"
            aria-label="Host selection area"
            onpointerdown={startHostMarquee}
          >
            <div class="host-content">
              {#if selectedGroupId}
                <nav class="group-breadcrumb" aria-label="Group path">
                  <button onclick={() => selectGroup(null)}>All hosts</button>
                  {#each groupBreadcrumb() as group, index (group.id)}
                    <Icon name="chevron-right" size={13} />
                    <button
                      class:current={index === groupBreadcrumb().length - 1}
                      onclick={() => selectGroup(group.id)}
                    >{group.name}</button>
                  {/each}
                </nav>
              {/if}

              {#if loading}
                <div class="host-grid">
                  {#each [1, 2, 3, 4, 5, 6] as _}
                    <div class="host-card skeleton"></div>
                  {/each}
                </div>
              {:else}
                {#if directChildGroups().length > 0}
                  <section class="vault-section">
                    <header><h2>Groups</h2></header>
                    <div class="host-grid" class:list-view={hostCollectionView === "list"}>
                      {#each directChildGroups() as group (group.id)}
                        <button
                          class="host-card group-card"
                          class:vault-drop-target={vaultDropGroupId === group.id}
                          class:vault-dragging={vaultDrag?.kind === "group" && vaultDrag.id === group.id}
                          title={`Open ${group.name}`}
                          draggable="true"
                          onclick={() => {
                            if (!suppressVaultCardClick) selectGroup(group.id);
                          }}
                          oncontextmenu={(event) => openGroupContextMenu(event, group)}
                          ondragstart={(event) => startVaultDrag(event, "group", group.id)}
                          ondragover={(event) => dragOverGroup(event, group.id)}
                          ondragleave={(event) => {
                            if (event.target === event.currentTarget && vaultDropGroupId === group.id) {
                              vaultDropGroupId = null;
                            }
                          }}
                          ondrop={(event) => void dropIntoGroup(event, group)}
                          ondragend={finishVaultDrag}
                        >
                          <span class="host-badge group-badge"><Icon name="grid" size={22} /></span>
                          <span class="host-copy">
                            <strong>{group.name}</strong>
                            <small>{groupHostCount(group.id)} host{groupHostCount(group.id) === 1 ? "" : "s"}</small>
                          </span>
                          <Icon name="chevron-right" size={16} />
                        </button>
                      {/each}
                    </div>
                  </section>
                {/if}

                {#if visibleHostList.length > 0}
                  <section class="vault-section">
                    <header>
                      <h2>Hosts</h2>
                      <span>{visibleHostList.length} shown · {hosts.length} saved</span>
                    </header>
                    <div
                      class="host-grid selectable-grid"
                      role="list"
                      class:list-view={hostCollectionView === "list"}
                    >
                      {#each visibleHostList as host (host.id)}
                        <article
                          class="host-card"
                          class:selected={selectedHostIds.includes(host.id)}
                          class:vault-dragging={vaultDrag?.kind === "host" && vaultDrag.ids.includes(host.id)}
                          data-select-id={host.id}
                          draggable="true"
                          ondragstart={(event) => startVaultDrag(event, "host", host.id)}
                          ondragend={finishVaultDrag}
                          oncontextmenu={(event) => openHostContextMenu(event, host)}
                        >
                          <button
                            class="host-card-main"
                            onclick={(event) => {
                              if (!suppressVaultCardClick) void handleHostCardClick(event, host);
                            }}
                            ondblclick={() => void openTerminal(host)}
                          >
                            <span class="host-badge {host.color}"><HostIcon hostId={host.id} size={21} /></span>
                            <span class="host-copy">
                              <strong>{host.label}</strong>
                              <small>{formatHostMeta(host)}</small>
                            </span>
                          </button>
                          <button
                            class="host-edit"
                            title={`Edit ${host.label}`}
                            onclick={(event) => {
                              event.stopPropagation();
                              void editHost(host);
                            }}
                          ><Icon name="edit" size={17} /></button>
                        </article>
                      {/each}
                    </div>
                  </section>
                {/if}

                {#if directChildGroups().length === 0 && visibleHostList.length === 0}
                  <div class="empty-page compact-empty">
                    <div class="empty-glyph"><Icon name={query.trim() ? "search" : "server"} size={30} /></div>
                    <h2>{query.trim() ? "No matching hosts" : "This group is empty"}</h2>
                    <p>
                      {query.trim()
                        ? "Try another label, address, username, or use Connect for a new target."
                        : "Add a host or create a nested group."}
                    </p>
                    {#if !query.trim()}
                      <div class="empty-actions">
                        <button class="secondary-action" onclick={newHost}>New host</button>
                        <button class="quiet-button" onclick={createGroup}>New group</button>
                      </div>
                    {/if}
                  </div>
                {/if}
              {/if}
            </div>
          </div>
        </section>

        {#if inspectorOpen}
          <aside class="inspector">
            <header>
              <div class="host-editor-heading">
                <strong>{selected && hosts.some((host) => host.id === selected?.id) ? "Host Details" : "New Host"}</strong>
                <span><Icon name="vault" size={13} /> Personal vault</span>
              </div>
              <div class="host-editor-header-actions">
                {#if selected && hosts.some((host) => host.id === selected?.id)}
                  <div class="host-editor-menu-anchor">
                    <button
                      class:active={hostEditorMenuOpen}
                      class="icon-button"
                      title="Host actions"
                      onclick={(event) => {
                        event.stopPropagation();
                        hostEditorMenuOpen = !hostEditorMenuOpen;
                      }}
                    >
                      <Icon name="more" />
                    </button>
                    {#if hostEditorMenuOpen}
                      <div class="host-editor-menu">
                        <button onclick={() => selected && void connectSelectedHost()}>
                          <Icon name="connect" /> Connect
                        </button>
                        <button onclick={() => selected && void duplicateHost(selected)}>
                          <Icon name="copy" /> Duplicate
                        </button>
                        <button class="danger-copy" onclick={removeHost}>
                          <Icon name="trash" /> Remove
                        </button>
                      </div>
                    {/if}
                  </div>
                {/if}
                <button class="icon-button" title="Close editor" onclick={closeInspector}>
                  <Icon name="close" />
                </button>
              </div>
            </header>
            {#if selected}
            <div class="inspector-scroll">
              <section class="host-editor-card address-card">
                <h2>Address</h2>
                <div class="host-address-field">
                  <span class="host-address-badge {selected.color}">
                    <Icon name="server" size={24} />
                  </span>
                  <input
                    id="host-address"
                    aria-label="IP address or hostname"
                    bind:value={selected.address}
                    placeholder="IP address or hostname"
                    spellcheck="false"
                  />
                </div>
              </section>

              <section class="host-editor-card">
                <h2>General</h2>
                <div class="editor-control with-icon">
                  <Icon name="edit" size={16} />
                  <input id="host-label" aria-label="Host label" bind:value={selected.label} placeholder="Label" />
                </div>
                <div class="editor-control with-icon">
                  <Icon name="folder" size={16} />
                  <CustomSelect
                    value={selected.group_id ?? ""}
                    options={[
                      { value: "", label: "No parent group" },
                      ...groupRows().map((row) => ({
                        value: row.group.id,
                        label: `${"— ".repeat(row.depth)}${row.group.name}`
                      }))
                    ]}
                    ariaLabel="Parent group"
                    onchange={(value) => assignSelectedGroup(value || null)}
                  />
                </div>
                <div
                  class="editor-control with-icon host-tags-control"
                  role="group"
                  aria-label="Host tags"
                >
                  <Icon name="tag" size={16} />
                  <div class="host-tags-input">
                    {#each selected.tags as tag (tag.toLocaleLowerCase())}
                      <span class="host-tag-chip">
                        {tag}
                        <button
                          title={`Remove ${tag}`}
                          aria-label={`Remove ${tag}`}
                          onclick={(event) => {
                            event.stopPropagation();
                            removeTagFromSelected(tag);
                          }}
                        ><Icon name="close" size={12} strokeWidth={2.4} /></button>
                      </span>
                    {/each}
                    <input
                      id="host-tags"
                      aria-label="Add a tag"
                      bind:value={hostTagDraft}
                      placeholder={selected.tags.length ? "Add tag" : "Tags"}
                      onfocus={() => (hostTagInputFocused = true)}
                      onclick={(event) => event.stopPropagation()}
                      onkeydown={(event) => {
                        if (event.key === "Enter" || event.key === ",") {
                          event.preventDefault();
                          acceptHostTagDraft();
                        } else if (event.key === "Backspace" && !hostTagDraft && selected?.tags.length) {
                          removeTagFromSelected(selected.tags[selected.tags.length - 1]);
                        } else if (event.key === "Escape") {
                          hostTagInputFocused = false;
                        }
                      }}
                    />
                  </div>
                </div>
                {#if hostTagInputFocused && (hostTagDraft.trim() || editorHostTagSuggestions().length > 0)}
                  {@const cleanDraftTag = cleanHostTag(hostTagDraft)}
                  <div
                    class="host-tag-editor-menu"
                    role="listbox"
                    aria-label="Available tags"
                    tabindex="-1"
                    onclick={(event) => event.stopPropagation()}
                    onkeydown={(event) => event.stopPropagation()}
                  >
                    {#each editorHostTagSuggestions() as tag (tag)}
                      <button role="option" aria-selected="false" onclick={() => addTagToSelected(tag)}>
                        <span>Add tag</span><strong>{tag}</strong>
                      </button>
                    {/each}
                    {#if cleanDraftTag && !allHostTags().some((tag) => tag.toLocaleLowerCase() === cleanDraftTag.toLocaleLowerCase())}
                      <button class="create-tag-option" role="option" aria-selected="false" onclick={acceptHostTagDraft}>
                        <span>Create Tag</span><strong>{cleanDraftTag}</strong>
                      </button>
                    {/if}
                  </div>
                {/if}
                <div class="host-color-row">
                  <span>Host color</span>
                  <div>
                    {#each ["blue", "violet", "rose", "amber", "emerald", "slate"] as color}
                      <button
                        class="host-color-dot {color}"
                        class:selected={selected.color === color}
                        title={color}
                        aria-label={`Use ${color}`}
                        onclick={() => selected && (selected.color = color as Host["color"])}
                      ></button>
                    {/each}
                  </div>
                </div>
              </section>

              <section class="host-editor-card ssh-card">
                <div class="ssh-port-heading">
                  <h2>SSH on</h2>
                  <input
                    id="host-port"
                    aria-label="SSH port"
                    type="number"
                    min="1"
                    max="65535"
                    bind:value={selected.port}
                  />
                  <strong>port</strong>
                </div>
                <div class="editor-divider"></div>
                <h3>Credentials</h3>
                <div class="credential-kind" role="group" aria-label="Authentication type">
                  <button
                    class:active={credentialMode === "password"}
                    onclick={() => setCredentialMode("password")}
                  >
                    <Icon name="shield" size={15} /> Password
                  </button>
                  <button
                    class:active={credentialMode === "key"}
                    onclick={() => setCredentialMode("key")}
                  >
                    <Icon name="key" size={15} /> Key
                  </button>
                </div>
                <div class="editor-control with-icon">
                  <Icon name="server" size={16} />
                  <input id="host-user" aria-label="Username" bind:value={selected.username} placeholder="Username" />
                </div>
                <div class="editor-control with-icon">
                  <Icon name={credentialMode === "password" ? "shield" : "key"} size={16} />
                  <CustomSelect
                    value={selected.identity_id ?? ""}
                    options={[
                      {
                        value: "",
                        label:
                          credentialMode === "password"
                            ? "New password for this host"
                            : "Select a vault key"
                      },
                      ...identities
                        .filter(
                          (identity) =>
                            identity.kind ===
                            (credentialMode === "password" ? "password" : "key_file")
                        )
                        .map((identity) => ({
                          value: identity.id,
                          label: identity.label
                        }))
                    ]}
                    ariaLabel="SSH identity"
                    onchange={(value) => {
                      if (!selected) return;
                      selected.identity_id = value || null;
                      hostPassword = "";
                    }}
                  />
                </div>
                {#if credentialMode === "password"}
                  <div class="editor-control with-icon password-control">
                    <Icon name="key" size={16} />
                    <input
                      id="host-password"
                      aria-label="SSH password"
                      type={showHostPassword ? "text" : "password"}
                      autocomplete="new-password"
                      bind:value={hostPassword}
                      placeholder={identities.find((identity) => identity.id === selected?.identity_id)?.secret_stored
                        ? "Stored · enter to replace"
                        : "Password"}
                    />
                    <button
                      class="password-visibility"
                      title={showHostPassword ? "Hide password" : "Show password"}
                      onclick={() => (showHostPassword = !showHostPassword)}
                    >
                      <Icon name={showHostPassword ? "eye-off" : "eye"} size={16} />
                    </button>
                  </div>
                {/if}
                <div class="credential-summary">
                  <span><Icon name="shield" size={15} /> {identitySummary()}</span>
                  <button class="text-button" onclick={openKeychain}>Manage Keychain</button>
                </div>
              </section>

              <section class="host-editor-card">
                <h2>Connection options</h2>
                <button class="option-row option-link" onclick={openChainEditor}>
                  <span class="option-icon"><Icon name="forward" size={17} /></span>
                  <span><strong>Host chain</strong><small>Connect through ordered jump hosts</small></span>
                  <span class="option-value">{chainSummary()}</span>
                  <Icon name="chevron-right" size={16} />
                </button>

                <button class="option-row option-link" onclick={openEnvironmentEditor}>
                  <span class="option-icon"><Icon name="code" size={17} /></span>
                  <span><strong>Environment variables</strong><small>Sent with OpenSSH SetEnv</small></span>
                  <span class="option-value">{environmentSummary()}</span>
                  <Icon name="chevron-right" size={16} />
                </button>

                <div class="read-only-option">
                  <span class="option-icon"><Icon name="code" size={17} /></span>
                  <span><strong>Encoding</strong><small>UTF-8</small></span>
                </div>
              </section>

              <section class="host-editor-card">
                <h2>Terminal theme</h2>
                <button class="theme-summary-row" onclick={openThemeEditor}>
                  <span
                    class="theme-preview"
                    style={`--theme-bg:${terminalTheme(selected.terminal_theme).preview[0]};--theme-fg:${terminalTheme(selected.terminal_theme).preview[1]};--theme-a:${terminalTheme(selected.terminal_theme).preview[2]};--theme-b:${terminalTheme(selected.terminal_theme).preview[3]}`}
                  >
                    <i></i><i></i><i></i><i></i>
                  </span>
                  <span>
                    <strong>{terminalTheme(selected.terminal_theme).label}</strong>
                    <small>{terminalTheme(selected.terminal_theme).description}</small>
                  </span>
                  <Icon name="chevron-right" size={17} />
                </button>
              </section>

              {#if message}
                <p class:error-message={messageIsError} class="form-message">{message}</p>
              {/if}
            </div>
            <div class="host-editor-footer">
              <button
                class="quiet-button"
                disabled={saving || !selected.label.trim() || !selected.address.trim() || !selected.username.trim()}
                onclick={() => void persistHost()}
              >
                <Icon name="save" /> {saving ? "Saving…" : "Save"}
              </button>
              <button
                class="connect-action"
                disabled={saving || !selected.label.trim() || !selected.address.trim() || !selected.username.trim()}
                onclick={connectSelectedHost}
              >
                Connect <Icon name="connect" />
              </button>
            </div>
            {/if}
            {#if selected && hostSubeditor}
              <section
                class="host-subeditor"
                class:closing={hostSubeditorClosing}
                aria-label={hostSubeditorTitle()}
              >
                <header>
                  <button
                    class="icon-button subeditor-back"
                    title="Back without saving"
                    onclick={() => closeHostSubeditor(false)}
                  ><Icon name="arrow-left" size={21} /></button>
                  <strong>{hostSubeditorTitle()}</strong>
                  {#if hostSubeditor !== "theme"}
                    <button
                      class="subeditor-save"
                      disabled={hostSubeditor === "environment" && !environmentDraftValid()}
                      onclick={() => closeHostSubeditor(true)}
                    >Save</button>
                  {:else}
                    <span></span>
                  {/if}
                </header>

                <div class="host-subeditor-scroll">
                  {#if hostSubeditor === "environment"}
                    <section class="subeditor-card environment-intro">
                      <div>
                        <h2>Set environment variables for {selected.label || "this host"}.</h2>
                        <p>Some SSH servers only allow variables with prefixes such as <code>LC_</code> and <code>LANG_</code>.</p>
                      </div>
                      <button class="subeditor-wide-action" onclick={addEnvironmentDraft}>
                        <Icon name="plus" size={17} /> Add a variable
                      </button>
                    </section>

                    {#if environmentDraft.length === 0}
                      <section class="subeditor-empty">
                        <span class="option-icon"><Icon name="code" size={20} /></span>
                        <strong>No environment variables</strong>
                        <small>Add one, then save to return to the host.</small>
                      </section>
                    {:else}
                      <div class="subeditor-variable-list">
                        {#each environmentDraft as variable, index}
                          <section class="subeditor-card variable-card">
                            <div class="variable-card-heading">
                              <h2>Variable {index + 1}</h2>
                              <button
                                class="icon-button danger"
                                title={`Remove variable ${index + 1}`}
                                onclick={() => removeEnvironmentDraft(index)}
                              ><Icon name="trash" size={16} /></button>
                            </div>
                            <input
                              aria-label={`Variable ${index + 1} name`}
                              bind:value={variable.name}
                              placeholder="Variable"
                              spellcheck="false"
                            />
                            <input
                              aria-label={`Variable ${index + 1} value`}
                              bind:value={variable.value}
                              placeholder="Value"
                              spellcheck="false"
                            />
                          </section>
                        {/each}
                      </div>
                    {/if}
                  {:else if hostSubeditor === "chain"}
                    <section class="subeditor-card chain-intro">
                      <p>
                        {jumpHostDraft.length
                          ? "Adding another host will create a connection to"
                          : "Adding a host will create a connection to"}
                        <strong>{chainEntryLabel()}</strong>.
                      </p>
                      <button
                        class="subeditor-wide-action"
                        onclick={() => (chainPickerOpen = !chainPickerOpen)}
                        disabled={jumpHostOptions().length === 0}
                      >
                        <Icon name="plus" size={17} />
                        Add a Host
                      </button>
                      {#if chainPickerOpen}
                        <div class="chain-picker">
                          {#if jumpHostOptions().length === 0}
                            <p>No other saved hosts are available.</p>
                          {:else}
                            {#each jumpHostOptions() as host (host.id)}
                              <button
                                onclick={() => addJumpHost(host.id)}
                              >
                                <span class="host-badge mini {host.color}">
                                  <HostIcon hostId={host.id} size={15} />
                                </span>
                                <span>
                                  <strong>{host.label}</strong>
                                  <small>{host.username}@{host.address} · {jumpHostCredentialLabel(host)}</small>
                                </span>
                                <Icon name="plus" size={16} />
                              </button>
                            {/each}
                          {/if}
                        </div>
                      {/if}
                    </section>

                    <div class="chain-path">
                      {#if jumpHostDraft.length > 0}
                        <Icon name="arrow-down" size={24} />
                      {/if}
                      {#each jumpHostDraft as jumpHostId, index (jumpHostId)}
                        {@const jumpHost = hosts.find((host) => host.id === jumpHostId)}
                        {#if jumpHost}
                          <article class="chain-host-card">
                            <span class="host-badge {jumpHost.color}">
                              <HostIcon hostId={jumpHost.id} size={21} />
                            </span>
                            <span>
                              <strong>{jumpHost.label}</strong>
                              <small>{jumpHost.username}@{jumpHost.address} · {jumpHostCredentialLabel(jumpHost)}</small>
                            </span>
                            <button
                              class="icon-button danger"
                              title={`Remove jump host ${index + 1}`}
                              onclick={() => removeJumpHost(index)}
                            ><Icon name="close" size={16} /></button>
                          </article>
                          <Icon name="arrow-down" size={24} />
                        {/if}
                      {/each}
                      <article class="chain-host-card destination">
                        <span class="host-badge {selected.color}">
                          <HostIcon hostId={selected.id} size={21} />
                        </span>
                        <span>
                          <strong>{selected.label || "New Host"}</strong>
                          <small>{selected.username}@{selected.address || "Address not set"}</small>
                        </span>
                      </article>
                      <p class="chain-security-note">
                        Each jump host uses its own saved Password or Key. Connect to every hop
                        directly once to trust its host key in the Heminus vault.
                      </p>
                    </div>
                  {:else}
                    <div class="theme-picker-list">
                      {#each terminalThemes as theme (theme.id)}
                        <button
                          class="theme-picker-option"
                          class:selected={selected.terminal_theme === theme.id}
                          onclick={() => {
                            if (!selected) return;
                            selected.terminal_theme = theme.id;
                            closeHostSubeditor(true);
                          }}
                        >
                          <span
                            class="theme-preview large"
                            style={`--theme-bg:${theme.preview[0]};--theme-fg:${theme.preview[1]};--theme-a:${theme.preview[2]};--theme-b:${theme.preview[3]}`}
                          >
                            <i></i><i></i><i></i><i></i>
                          </span>
                          <span>
                            <strong>{theme.label}</strong>
                            <small>{theme.description}</small>
                          </span>
                          {#if selected.terminal_theme === theme.id}
                            <Icon name="check" size={17} />
                          {/if}
                        </button>
                      {/each}
                    </div>
                  {/if}
                </div>

                {#if hostSubeditor === "chain"}
                  <footer>
                    <button
                      class="subeditor-clear"
                      disabled={jumpHostDraft.length === 0}
                      onclick={() => (jumpHostDraft = [])}
                    >Clear chain</button>
                  </footer>
                {/if}
              </section>
            {/if}
          </aside>
        {/if}
      </main>
    {:else if page === "sftp"}
      <main class="full-page">
        {#if SftpComponent}
          <SftpComponent initialHostId={sftpHostId} autoConnect={sftpAutoConnect} />
        {:else}
          <div class="page-loading">Loading SFTP…</div>
        {/if}
      </main>
    {:else if page === "terminal"}
      <div class="terminal-route-placeholder"></div>
    {:else if page === "snippets"}
      <main class="full-page">
        {#if SnippetsComponent}
          <SnippetsComponent
            ondirtychange={(dirty: boolean) => setManagerEditorDirty("snippets", dirty)}
          />
        {:else}
          <div class="page-loading">Loading snippets…</div>
        {/if}
      </main>
    {:else if page === "forwarding"}
      <main class="full-page">
        {#if ForwardingComponent}
          <ForwardingComponent
            ondirtychange={(dirty: boolean) => setManagerEditorDirty("forwarding", dirty)}
          />
        {:else}
          <div class="page-loading">Loading port forwarding…</div>
        {/if}
      </main>
    {:else if page === "keychain"}
      <main class="full-page">
        {#if KeychainComponent}
          <KeychainComponent
            ondirtychange={(dirty: boolean) => setManagerEditorDirty("keychain", dirty)}
          />
        {:else}
          <div class="page-loading">Loading keychain…</div>
        {/if}
      </main>
    {:else if page === "known-hosts"}
      <main class="full-page">
        {#if KnownHostsComponent}
          <KnownHostsComponent />
        {:else}
          <div class="page-loading">Loading known hosts…</div>
        {/if}
      </main>
    {:else if page === "logs"}
      <main class="full-page">
        {#if LogsComponent}
          <LogsComponent />
        {:else}
          <div class="page-loading">Loading session history…</div>
        {/if}
      </main>
    {:else if page === "new-tab"}
      <main class="new-tab-page">
        <div class="command-search">
          <Icon name="search" />
          <input
            aria-label="Search hosts, terminal panes, or workspaces"
            placeholder="Search hosts or open terminals"
            bind:value={newTabQuery}
            bind:this={newTabSearchInput}
            onkeydown={(event) => {
              if (event.key === "Enter") openFirstNewTabResult();
            }}
          />
          <kbd>Ctrl K</kbd>
        </div>
        <section class="recent-panel">
          <header><h2>Connections</h2></header>
          {#if localTerminalMatchesNewTabQuery()}
            <button class="recent-row local-terminal-row" onclick={() => void openTerminal()}>
              <span class="host-badge mini emerald"><Icon name="terminal" size={16} /></span>
              <strong>Local Terminal</strong>
              <span>This device</span>
            </button>
          {/if}
          {#each filteredNewTabHosts() as host (host.id)}
            <button class="recent-row" onclick={() => void openTerminal(host)}>
              <span class="host-badge mini {host.color}"><HostIcon hostId={host.id} size={16} /></span>
              <strong>{host.label}</strong>
              <span>{host.group_name ?? "Personal"}</span>
            </button>
          {/each}
        </section>
      </main>
    {:else}
      <main class="feature-page">
        <div class="empty-glyph"><Icon name={navigation.find((item) => item.page === page)?.icon ?? "vault"} size={30} /></div>
        <h1>{navigation.find((item) => item.page === page)?.label}</h1>
        <p>This feature is being prepared for the next release.</p>
        <button class="secondary-action"><Icon name="plus" /> Create new</button>
      </main>
    {/if}
  </div>

  {#if terminalContextMenu}
    <div
      class="host-context-menu terminal-context-menu"
      style:left={`${terminalContextMenu.x}px`}
      style:top={`${terminalContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      {#if terminalContextMenu.tab.id === "workspace"}
        <button
          role="menuitem"
          onclick={() => {
            terminalContextMenu = null;
            renameWorkspace();
          }}
        ><Icon name="edit" size={17} /><span>Rename</span></button>
        <button
          role="menuitem"
          onclick={() => {
            terminalContextMenu = null;
            void detachWorkspace();
          }}
        ><Icon name="detach" size={17} /><span>Detach</span></button>
        <button
          class="danger"
          role="menuitem"
          onclick={() => {
            terminalContextMenu = null;
            closeCurrentWorkspace();
          }}
        ><Icon name="close" size={17} /><span>Close</span></button>
      {:else}
        <button
          role="menuitem"
          onclick={() => {
            const tab = terminalContextMenu?.tab;
            terminalContextMenu = null;
            if (tab) void openTerminal(tab.host);
          }}
        ><Icon name="copy" size={17} /><span>Duplicate</span></button>
        <button
          role="menuitem"
          onclick={() => {
            const tab = terminalContextMenu?.tab;
            terminalContextMenu = null;
            if (tab) renameTerminalTab(tab);
          }}
        ><Icon name="edit" size={17} /><span>Rename</span></button>
        <button
          role="menuitem"
          onclick={() => {
            const tab = terminalContextMenu?.tab;
            terminalContextMenu = null;
            if (tab) void splitTerminalTab(tab);
          }}
        ><Icon name="grid" size={17} /><span>Split horizontally</span></button>
        <button
          role="menuitem"
          onclick={() => {
            const tab = terminalContextMenu?.tab;
            terminalContextMenu = null;
            if (tab) void detachTerminalTab(tab);
          }}
        ><Icon name="detach" size={17} /><span>Detach</span></button>
        <div class="context-separator"></div>
        <button
          class="danger"
          role="menuitem"
          onclick={() => {
            const id = terminalContextMenu?.tab.id;
            terminalContextMenu = null;
            if (id) closeTerminalTab(id);
          }}
        ><Icon name="close" size={17} /><span>Close</span></button>
      {/if}
    </div>
  {/if}

  {#if hostContextMenu}
    <div
      class="host-context-menu"
      class:submenu-left={hostContextMenu.x > window.innerWidth - 450}
      style:left={`${hostContextMenu.x}px`}
      style:top={`${hostContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <button
        role="menuitem"
        onclick={() => {
          const host = takeContextHost();
          if (host) void openTerminal(host);
        }}
      >
        <Icon name="connect" size={17} /><span>Quick Connect</span><kbd>↵</kbd>
      </button>
      <div class="context-submenu">
        <button role="menuitem" aria-haspopup="menu">
          <Icon name="connect" size={17} /><span>Connect</span><Icon name="chevron-right" size={14} />
        </button>
        <div class="context-submenu-panel" role="menu">
          <button
            role="menuitem"
            onclick={() => {
              const host = takeContextHost();
              if (host) void openTerminal(host);
            }}
          >with SSH</button>
          <button
            role="menuitem"
            onclick={() => {
              const host = takeContextHost();
              if (host) void openSftp(host.id, true);
            }}
          >with SFTP</button>
        </div>
      </div>
      <button
        role="menuitem"
        onclick={() => {
          const host = takeContextHost();
          if (host) editHost(host);
        }}
      >
        <Icon name="edit" size={17} /><span>Edit Host Details</span><kbd>E</kbd>
      </button>
      <button
        role="menuitem"
        onclick={() => {
          const host = takeContextHost();
          if (host) void duplicateHost(host);
        }}
      >
        <Icon name="copy" size={17} /><span>Duplicate</span>
      </button>
      <div class="context-separator"></div>
      <div class="context-submenu">
        <button role="menuitem" aria-haspopup="menu">
          <Icon name="folder" size={17} /><span>Move to</span><Icon name="chevron-right" size={14} />
        </button>
        <div class="context-submenu-panel move-destination-menu" role="menu">
          <button
            role="menuitem"
            disabled={hostContextMenu.host.group_id === null}
            onclick={() => {
              const host = takeContextHost();
              if (host) void moveHost(host, null);
            }}
          >Root</button>
          {#each groupRows() as row (row.group.id)}
            <button
              role="menuitem"
              disabled={hostContextMenu.host.group_id === row.group.id}
              style:padding-left={`${10 + row.depth * 12}px`}
              onclick={() => {
                const host = takeContextHost();
                if (host) void moveHost(host, row.group.id);
              }}
            >{row.group.name}</button>
          {/each}
        </div>
      </div>
      <div class="context-separator"></div>
      <button
        class="danger"
        role="menuitem"
        onclick={() => {
          const host = takeContextHost();
          if (host) {
            selected = cloneHost(host);
            void removeHost();
          }
        }}
      >
        <Icon name="trash" size={17} /><span>Remove</span>
      </button>
    </div>
  {/if}

  {#if bulkHostContextMenu && selectedHostIds.length > 1}
    <div
      class="host-context-menu bulk-host-context-menu"
      style:left={`${bulkHostContextMenu.x}px`}
      style:top={`${bulkHostContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <div class="bulk-menu-heading">{selectedHostIds.length} hosts selected</div>
      <button role="menuitem" onclick={() => void connectSelectedHosts()}>
        <Icon name="connect" size={17} /><span>Connect all</span>
      </button>
      <div class="context-submenu">
        <button role="menuitem" aria-haspopup="menu">
          <Icon name="folder" size={17} /><span>Move all to</span><Icon name="chevron-right" size={14} />
        </button>
        <div class="context-submenu-panel move-destination-menu" role="menu">
          <button role="menuitem" onclick={() => void moveSelectedHosts(null)}>Root</button>
          {#each groupRows() as row (row.group.id)}
            <button
              role="menuitem"
              style:padding-left={`${10 + row.depth * 12}px`}
              onclick={() => void moveSelectedHosts(row.group.id)}
            >{row.group.name}</button>
          {/each}
        </div>
      </div>
      <div class="context-separator"></div>
      <button class="danger" role="menuitem" onclick={() => void removeSelectedHosts()}>
        <Icon name="trash" size={17} /><span>Remove all</span>
      </button>
    </div>
  {/if}

  {#if groupContextMenu}
    <div
      class="host-context-menu group-context-menu"
      class:submenu-left={groupContextMenu.x > window.innerWidth - 450}
      style:left={`${groupContextMenu.x}px`}
      style:top={`${groupContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <button
        role="menuitem"
        onclick={() => {
          const group = takeContextGroup();
          if (group) void renameGroup(group);
        }}
      >
        <Icon name="edit" size={17} /><span>Edit Group Details</span><kbd>E</kbd>
      </button>
      <div class="context-submenu">
        <button role="menuitem" aria-haspopup="menu">
          <Icon name="folder" size={17} /><span>Move to</span><Icon name="chevron-right" size={14} />
        </button>
        <div class="context-submenu-panel move-destination-menu" role="menu">
          <button
            role="menuitem"
            disabled={groupContextMenu.group.parent_id === null}
            onclick={() => {
              const group = takeContextGroup();
              if (group) void moveGroup(group, null);
            }}
          >Root</button>
          {#each movableGroupRows(groupContextMenu.group.id) as row (row.group.id)}
            <button
              role="menuitem"
              disabled={groupContextMenu.group.parent_id === row.group.id}
              style:padding-left={`${10 + row.depth * 12}px`}
              onclick={() => {
                const group = takeContextGroup();
                if (group) void moveGroup(group, row.group.id);
              }}
            >{row.group.name}</button>
          {/each}
        </div>
      </div>
      <div class="context-separator"></div>
      <button
        class="danger"
        role="menuitem"
        onclick={() => {
          const group = takeContextGroup();
          if (group) void removeGroup(group);
        }}
      >
        <Icon name="trash" size={17} /><span>Remove</span>
      </button>
    </div>
  {/if}
</div>

<AppDialog />
