<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "../../components/Icon.svelte";
  import PermissionEditor from "./PermissionEditor.svelte";
  import { confirmDialog, promptDialog } from "../../lib/dialog";
  import { setElementDragPreview } from "../../lib/dragPreview";
  import { humanFileSize } from "../../lib/format";
  import {
    closeSftpSession,
    createLocalDirectory,
    createRemoteDirectory,
    downloadFile,
    homeDirectory,
    listGroups,
    listHosts,
    listLocalEntries,
    listRemoteEntries,
    openLocalEntry,
    openSftpSession,
    removeLocalEntry,
    removeRemoteEntry,
    renameLocalEntry,
    renameRemoteEntry,
    setLocalPermissions,
    setRemotePermissions,
    uploadFile
  } from "../../lib/ipc";
  import type {
    Host,
    LocalEntry,
    RemoteEntry,
    SftpOpenResult,
    TransferEvent,
    VaultGroup
  } from "../../lib/types";

  type PaneName = "local" | "remote";
  type SortKey = "name" | "modified" | "size" | "kind";
  type SortDirection = "ascending" | "descending";
  type BrowserEntry = LocalEntry | RemoteEntry;

  let {
    initialHostId = "",
    autoConnect = false
  }: { initialHostId?: string; autoConnect?: boolean } = $props();

  let hosts = $state<Host[]>([]);
  let groups = $state<VaultGroup[]>([]);
  let selectedHostId = $state("");
  let selectedHostGroupId = $state<string | null>(null);
  let hostQuery = $state("");
  let localEntries = $state<LocalEntry[]>([]);
  let remoteEntries = $state<RemoteEntry[]>([]);
  let selectedLocal = $state<LocalEntry | null>(null);
  let selectedRemote = $state<RemoteEntry | null>(null);
  let localLoading = $state(true);
  let remoteLoading = $state(false);
  let connecting = $state(false);
  let error = $state("");
  let homePath = $state("");
  let currentPath = $state("");
  let remotePath = $state("");
  let remoteHome = $state("");
  let session = $state<SftpOpenResult | null>(null);
  let localQuery = $state("");
  let remoteQuery = $state("");
  let showHidden = $state(false);
  let transferLabel = $state("");
  let transferProgress = $state(0);
  let transferDirection = $state("");
  let transferTransferred = $state(0);
  let transferTotal = $state(0);
  let transferStartedAt = $state(0);
  let transferStatus = $state<"idle" | "running" | "completed" | "failed">("idle");
  let transferFailure = $state("");
  let transferDismissTimer: number | null = null;
  let localActionsOpen = $state(false);
  let remoteActionsOpen = $state(false);
  let localHistory = $state<string[]>([]);
  let localHistoryIndex = $state(-1);
  let remoteHistory = $state<string[]>([]);
  let remoteHistoryIndex = $state(-1);
  let localSortKey = $state<SortKey>("name");
  let localSortDirection = $state<SortDirection>("ascending");
  let remoteSortKey = $state<SortKey>("name");
  let remoteSortDirection = $state<SortDirection>("ascending");
  let contextMenu = $state<{
    pane: PaneName;
    entry: BrowserEntry;
    x: number;
    y: number;
  } | null>(null);
  let dragSource = $state<PaneName | null>(null);
  let draggedEntry = $state<BrowserEntry | null>(null);
  let dropTarget = $state<PaneName | null>(null);
  let permissionTarget = $state<{
    pane: PaneName;
    entry: BrowserEntry;
  } | null>(null);

  const visibleLocalEntries = $derived(
    sortEntries(
      localEntries.filter(
        (entry) =>
          (showHidden || !entry.name.startsWith(".")) &&
          entry.name.toLowerCase().includes(localQuery.trim().toLowerCase())
      ),
      localSortKey,
      localSortDirection
    )
  );
  const visibleRemoteEntries = $derived(
    sortEntries(
      remoteEntries.filter(
        (entry) =>
          (showHidden || !entry.name.startsWith(".")) &&
          entry.name.toLowerCase().includes(remoteQuery.trim().toLowerCase())
      ),
      remoteSortKey,
      remoteSortDirection
    )
  );
  const localBreadcrumbs = $derived(makeBreadcrumbs(currentPath));
  const remoteBreadcrumbs = $derived(makeBreadcrumbs(remotePath));
  const transferRate = $derived(
    transferStartedAt > 0
      ? transferTransferred / Math.max(0.25, (Date.now() - transferStartedAt) / 1000)
      : 0
  );

  onMount(() => {
    const closeContextMenu = () => (contextMenu = null);
    const closeContextMenuOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") contextMenu = null;
    };
    window.addEventListener("click", closeContextMenu);
    window.addEventListener("keydown", closeContextMenuOnEscape);
    void (async () => {
      try {
        [homePath, hosts, groups] = await Promise.all([
          homeDirectory(),
          listHosts(),
          listGroups()
        ]);
        selectedHostId = hosts.some((host) => host.id === initialHostId)
          ? initialHostId
          : (hosts[0]?.id ?? "");
        await openLocalDirectory(homePath);
        if (autoConnect && selectedHostId) await connectRemote();
      } catch (cause) {
        error = errorMessage(cause);
      } finally {
        localLoading = false;
      }
    })();
    return () => {
      window.removeEventListener("click", closeContextMenu);
      window.removeEventListener("keydown", closeContextMenuOnEscape);
      if (transferDismissTimer !== null) window.clearTimeout(transferDismissTimer);
      if (session) void closeSftpSession(session.id);
    };
  });

  function makeBreadcrumbs(current: string) {
    if (!current) return [];
    const result = [{ name: "/", path: "/" }];
    let path = "";
    for (const name of current.split("/").filter(Boolean)) {
      path += `/${name}`;
      result.push({ name, path });
    }
    return result;
  }

  function rememberPath(
    history: string[],
    historyIndex: number,
    path: string
  ): { history: string[]; index: number } {
    if (history[historyIndex] === path) return { history, index: historyIndex };
    const next = [...history.slice(0, historyIndex + 1), path];
    return { history: next, index: next.length - 1 };
  }

  async function openLocalDirectory(path: string, remember = true) {
    localLoading = true;
    error = "";
    try {
      localEntries = await listLocalEntries(path);
      currentPath = path;
      selectedLocal = null;
      if (remember) {
        const next = rememberPath(localHistory, localHistoryIndex, path);
        localHistory = next.history;
        localHistoryIndex = next.index;
      }
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      localLoading = false;
    }
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

  function directHostGroups(): VaultGroup[] {
    return groups
      .filter((group) => group.parent_id === selectedHostGroupId)
      .sort((left, right) => left.name.localeCompare(right.name));
  }

  function selectableHosts(): Host[] {
    const normalized = hostQuery.trim().toLowerCase();
    const groupIds = selectedHostGroupId
      ? groupAndDescendantIds(selectedHostGroupId)
      : null;
    return hosts.filter((host) => {
      const insideGroup = !groupIds || (host.group_id !== null && groupIds.has(host.group_id));
      const matches =
        !normalized ||
        [host.label, host.address, host.username].some((value) =>
          value.toLowerCase().includes(normalized)
        );
      return insideGroup && matches;
    });
  }

  function groupHostCount(id: string): number {
    const ids = groupAndDescendantIds(id);
    return hosts.filter((host) => host.group_id && ids.has(host.group_id)).length;
  }

  function currentHostGroup(): VaultGroup | undefined {
    return groups.find((group) => group.id === selectedHostGroupId);
  }

  function hostGroupBack() {
    selectedHostGroupId = currentHostGroup()?.parent_id ?? null;
  }

  async function connectRemote(hostId?: string) {
    if (hostId) selectedHostId = hostId;
    const host = hosts.find((candidate) => candidate.id === selectedHostId);
    if (!host || connecting) return;
    connecting = true;
    error = "";
    try {
      if (session) await closeSftpSession(session.id);
      session = await openSftpSession(host);
      remoteHome = session.homePath;
      remoteHistory = [];
      remoteHistoryIndex = -1;
      await openRemoteDirectory(remoteHome);
    } catch (cause) {
      session = null;
      error = errorMessage(cause);
    } finally {
      connecting = false;
    }
  }

  async function disconnectRemote() {
    if (session) await closeSftpSession(session.id);
    session = null;
    remoteEntries = [];
    selectedRemote = null;
    remoteHistory = [];
    remoteHistoryIndex = -1;
  }

  async function openRemoteDirectory(path: string, remember = true) {
    if (!session) return;
    remoteLoading = true;
    error = "";
    try {
      remoteEntries = await listRemoteEntries(session.id, path);
      remotePath = path;
      selectedRemote = null;
      if (remember) {
        const next = rememberPath(remoteHistory, remoteHistoryIndex, path);
        remoteHistory = next.history;
        remoteHistoryIndex = next.index;
      }
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      remoteLoading = false;
    }
  }

  function localParent() {
    if (!currentPath || currentPath === "/") return;
    void openLocalDirectory(parentPath(currentPath));
  }

  function remoteParent() {
    if (!remotePath || remotePath === "/") return;
    void openRemoteDirectory(parentPath(remotePath));
  }

  function parentPath(path: string): string {
    const normalized = path.replace(/\/+$/, "");
    return normalized.slice(0, normalized.lastIndexOf("/")) || "/";
  }

  async function navigateLocalHistory(offset: number) {
    const index = localHistoryIndex + offset;
    const path = localHistory[index];
    if (!path) return;
    localHistoryIndex = index;
    await openLocalDirectory(path, false);
  }

  async function navigateRemoteHistory(offset: number) {
    const index = remoteHistoryIndex + offset;
    const path = remoteHistory[index];
    if (!path) return;
    remoteHistoryIndex = index;
    await openRemoteDirectory(path, false);
  }

  function fileInfo(name: string, isDirectory: boolean) {
    if (isDirectory) return { icon: "folder", kind: "folder", tone: "folder" };
    const extension = name.includes(".") ? name.split(".").at(-1)?.toLowerCase() ?? "" : "";
    if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tif", "tiff"].includes(extension)) {
      return { icon: "file-image", kind: `${extension.toUpperCase()} image`, tone: "image" };
    }
    if (extension === "pdf") {
      return { icon: "file-pdf", kind: "PDF document", tone: "pdf" };
    }
    if (["doc", "docx", "odt", "rtf"].includes(extension)) {
      return { icon: "file-text", kind: "document", tone: "document" };
    }
    if (["xls", "xlsx", "ods", "csv"].includes(extension)) {
      return { icon: "file-sheet", kind: "spreadsheet", tone: "sheet" };
    }
    if (["ppt", "pptx", "odp"].includes(extension)) {
      return { icon: "file-presentation", kind: "presentation", tone: "presentation" };
    }
    if (["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "deb", "rpm"].includes(extension)) {
      return { icon: "archive", kind: "archive", tone: "archive" };
    }
    if (["mp4", "mkv", "mov", "avi", "webm", "m4v"].includes(extension)) {
      return { icon: "file-video", kind: "video", tone: "video" };
    }
    if (["mp3", "wav", "flac", "ogg", "m4a", "aac"].includes(extension)) {
      return { icon: "file-audio", kind: "audio", tone: "audio" };
    }
    if (
      [
        "js", "ts", "jsx", "tsx", "svelte", "vue", "html", "css", "scss", "rs", "go",
        "py", "java", "kt", "c", "h", "cpp", "hpp", "sh", "bash", "zsh", "sql"
      ].includes(extension)
    ) {
      return { icon: "code", kind: "source code", tone: "code" };
    }
    if (
      ["txt", "md", "log", "json", "yaml", "yml", "toml", "ini", "conf", "xml"].includes(extension)
    ) {
      return { icon: "file-text", kind: "text", tone: "text" };
    }
    return { icon: "file", kind: extension ? `${extension.toUpperCase()} file` : "file", tone: "file" };
  }

  function sortEntries<T extends BrowserEntry>(
    entries: T[],
    key: SortKey,
    direction: SortDirection
  ): T[] {
    const factor = direction === "ascending" ? 1 : -1;
    return [...entries].sort((left, right) => {
      let comparison = 0;
      if (key === "name") {
        comparison = left.name.localeCompare(right.name, undefined, {
          numeric: true,
          sensitivity: "base"
        });
      } else if (key === "modified") {
        comparison = (left.modifiedUnixMs ?? -1) - (right.modifiedUnixMs ?? -1);
      } else if (key === "size") {
        comparison = (left.size ?? -1) - (right.size ?? -1);
      } else {
        comparison = fileInfo(left.name, left.isDirectory).kind.localeCompare(
          fileInfo(right.name, right.isDirectory).kind
        );
      }
      if (comparison === 0) {
        comparison = left.name.localeCompare(right.name, undefined, {
          numeric: true,
          sensitivity: "base"
        });
      }
      return comparison * factor;
    });
  }

  function toggleSort(pane: PaneName, key: SortKey) {
    if (pane === "local") {
      if (localSortKey === key) {
        localSortDirection =
          localSortDirection === "ascending" ? "descending" : "ascending";
      } else {
        localSortKey = key;
        localSortDirection = "ascending";
      }
    } else if (remoteSortKey === key) {
      remoteSortDirection =
        remoteSortDirection === "ascending" ? "descending" : "ascending";
    } else {
      remoteSortKey = key;
      remoteSortDirection = "ascending";
    }
  }

  function sortMarker(pane: PaneName, key: SortKey): string {
    const activeKey = pane === "local" ? localSortKey : remoteSortKey;
    const direction =
      pane === "local" ? localSortDirection : remoteSortDirection;
    if (activeKey !== key) return "";
    return direction === "ascending" ? "↑" : "↓";
  }

  function showContextMenu(
    event: MouseEvent,
    pane: PaneName,
    entry: BrowserEntry
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (pane === "local") selectedLocal = entry as LocalEntry;
    else selectedRemote = entry as RemoteEntry;
    const width = 230;
    const height = 340;
    contextMenu = {
      pane,
      entry,
      x: Math.min(event.clientX, window.innerWidth - width - 8),
      y: Math.min(event.clientY, window.innerHeight - height - 8)
    };
  }

  function startEntryDrag(
    event: DragEvent,
    pane: PaneName,
    entry: BrowserEntry
  ) {
    dragSource = pane;
    draggedEntry = entry;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "copy";
      event.dataTransfer.setData("text/plain", entry.name);
      setElementDragPreview(event);
    }
  }

  function finishEntryDrag() {
    dragSource = null;
    draggedEntry = null;
    dropTarget = null;
  }

  function dragOverPane(event: DragEvent, pane: PaneName) {
    if (!session || !dragSource || dragSource === pane || !draggedEntry) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
    dropTarget = pane;
  }

  function dragLeavePane(event: DragEvent, pane: PaneName) {
    const current = event.currentTarget as HTMLElement;
    const next = event.relatedTarget as Node | null;
    if (!next || !current.contains(next)) {
      if (dropTarget === pane) dropTarget = null;
    }
  }

  async function dropOnPane(event: DragEvent, pane: PaneName) {
    event.preventDefault();
    const source = dragSource;
    const entry = draggedEntry;
    finishEntryDrag();
    if (!session || !source || source === pane || !entry) return;
    if (source === "local") {
      await uploadEntry(entry as LocalEntry);
    } else {
      await downloadEntry(entry as RemoteEntry);
    }
  }

  function formatModified(value: number | null): string {
    if (!value) return "--";
    return new Intl.DateTimeFormat(undefined, {
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit"
    }).format(new Date(value));
  }

  function formatPermissions(
    mode: number | null,
    isDirectory: boolean,
    isSymlink: boolean
  ): string {
    if (mode === null) return "";
    const type = isSymlink ? "l" : isDirectory ? "d" : "-";
    const flags = ["r", "w", "x"];
    let permissions = "";
    for (let shift = 8; shift >= 0; shift -= 1) {
      permissions += mode & (1 << shift) ? flags[(8 - shift) % 3] : "-";
    }
    return `${type}${permissions}`;
  }

  async function uploadSelected() {
    if (!selectedLocal) return;
    await uploadEntry(selectedLocal);
  }

  async function uploadEntry(entry: LocalEntry) {
    if (!session) return;
    const destination = joinRemote(remotePath, entry.name);
    if (
      remoteEntries.some((candidate) => candidate.name === entry.name) &&
      !(await confirmDialog({
        title: "Replace remote item?",
        message: `Merge or replace “${entry.name}” on the remote host?`,
        confirmLabel: "Replace"
      }))
    ) {
      return;
    }
    await runTransfer(
      `Uploading ${entry.name}`,
      "Local → Remote",
      (channel) => uploadFile(session!.id, entry.path, destination, channel),
      () => openRemoteDirectory(remotePath)
    );
  }

  async function downloadSelected() {
    if (!selectedRemote) return;
    await downloadEntry(selectedRemote);
  }

  async function downloadEntry(
    entry: RemoteEntry,
    openAfter: string | null | undefined = undefined
  ) {
    if (!session) return;
    const destination = `${currentPath}/${entry.name}`;
    if (
      localEntries.some((candidate) => candidate.name === entry.name) &&
      !(await confirmDialog({
        title: "Replace local item?",
        message: `Merge or replace “${entry.name}” in the local folder?`,
        confirmLabel: "Replace"
      }))
    ) {
      return;
    }
    await runTransfer(
      `Downloading ${entry.name}`,
      "Remote → Local",
      (channel) => downloadFile(session!.id, entry.path, destination, channel),
      () => openLocalDirectory(currentPath),
      openAfter === undefined || entry.isDirectory
        ? undefined
        : () => openLocalEntry(destination, openAfter)
    );
  }

  async function runTransfer(
    label: string,
    direction: string,
    action: (channel: Channel<TransferEvent>) => Promise<void>,
    refresh: () => Promise<void>,
    afterSuccess?: () => Promise<void>
  ) {
    if (transferStatus === "running") {
      error = "Wait for the current transfer to finish before starting another one.";
      return;
    }
    if (transferDismissTimer !== null) {
      window.clearTimeout(transferDismissTimer);
      transferDismissTimer = null;
    }
    transferLabel = label;
    transferProgress = 0;
    transferDirection = direction;
    transferTransferred = 0;
    transferTotal = 0;
    transferStartedAt = Date.now();
    transferStatus = "running";
    transferFailure = "";
    error = "";
    const channel = new Channel<TransferEvent>();
    channel.onmessage = (event) => {
      transferTransferred = event.transferred;
      transferTotal = event.total;
      transferProgress = event.total > 0 ? event.transferred / event.total : 1;
    };
    try {
      await action(channel);
      transferProgress = 1;
      transferTransferred = transferTotal || transferTransferred;
      transferStatus = "completed";
      await refresh();
      await afterSuccess?.();
      transferDismissTimer = window.setTimeout(() => dismissTransfer(), 4500);
    } catch (cause) {
      transferFailure = errorMessage(cause);
      transferStatus = "failed";
      error = transferFailure;
    }
  }

  function dismissTransfer() {
    if (transferStatus === "running") return;
    transferLabel = "";
    transferProgress = 0;
    transferDirection = "";
    transferTransferred = 0;
    transferTotal = 0;
    transferStartedAt = 0;
    transferStatus = "idle";
    transferFailure = "";
    transferDismissTimer = null;
  }

  async function openEntry(pane: PaneName, entry: BrowserEntry, chooseApplication = false) {
    contextMenu = null;
    if (entry.isDirectory) {
      if (pane === "local") await openLocalDirectory(entry.path);
      else await openRemoteDirectory(entry.path);
      return;
    }
    let application: string | null = null;
    if (chooseApplication) {
      application = await promptDialog({
        title: "Open with",
        label: "Application command",
        placeholder: "For example: code, libreoffice, evince",
        confirmLabel: "Open"
      });
      if (!application) return;
    }
    if (pane === "local") {
      try {
        await openLocalEntry(entry.path, application);
      } catch (cause) {
        error = errorMessage(cause);
      }
    } else {
      await downloadEntry(entry as RemoteEntry, application);
    }
  }

  async function contextCopy() {
    const menu = contextMenu;
    contextMenu = null;
    if (!menu) return;
    if (menu.pane === "local") await uploadEntry(menu.entry as LocalEntry);
    else await downloadEntry(menu.entry as RemoteEntry);
  }

  async function contextRename() {
    const pane = contextMenu?.pane;
    contextMenu = null;
    if (pane === "local") await renameSelectedLocal();
    else if (pane === "remote") await renameSelectedRemote();
  }

  async function contextDelete() {
    const pane = contextMenu?.pane;
    contextMenu = null;
    if (pane === "local") await deleteSelectedLocal();
    else if (pane === "remote") await deleteSelectedRemote();
  }

  async function contextRefresh() {
    const pane = contextMenu?.pane;
    contextMenu = null;
    if (pane === "local") await openLocalDirectory(currentPath);
    else if (pane === "remote") await openRemoteDirectory(remotePath);
  }

  async function contextNewFolder() {
    const pane = contextMenu?.pane;
    contextMenu = null;
    if (pane === "local") await newLocalFolder();
    else if (pane === "remote") await newRemoteFolder();
  }

  async function contextPermissions() {
    const pane = contextMenu?.pane;
    contextMenu = null;
    if (pane === "local") await editLocalPermissions();
    else if (pane === "remote") await editRemotePermissions();
  }

  async function newRemoteFolder() {
    if (!session) return;
    const name = await promptDialog({
      title: "New remote folder",
      label: "Folder name",
      placeholder: "New folder",
      confirmLabel: "Create"
    });
    if (!name || name.includes("/")) return;
    try {
      await createRemoteDirectory(session.id, joinRemote(remotePath, name));
      await openRemoteDirectory(remotePath);
    } catch (cause) {
      error = errorMessage(cause);
    }
  }

  async function newLocalFolder() {
    const name = await promptDialog({
      title: "New local folder",
      label: "Folder name",
      placeholder: "New folder",
      confirmLabel: "Create"
    });
    if (!name || name.includes("/")) return;
    try {
      await createLocalDirectory(`${currentPath}/${name}`);
      await openLocalDirectory(currentPath);
    } catch (cause) {
      error = errorMessage(cause);
    }
  }

  async function renameSelectedLocal() {
    if (!selectedLocal) return;
    const name = await promptDialog({
      title: "Rename local item",
      label: "New name",
      initialValue: selectedLocal.name,
      confirmLabel: "Rename"
    });
    if (!name || name.includes("/") || name === selectedLocal.name) return;
    try {
      await renameLocalEntry(selectedLocal.path, `${currentPath}/${name}`);
      await openLocalDirectory(currentPath);
    } catch (cause) {
      error = errorMessage(cause);
    }
  }

  async function deleteSelectedLocal() {
    if (!selectedLocal) return;
    if (
      !(await confirmDialog({
        title: "Delete local item?",
        message: `Delete “${selectedLocal.name}” from this device?`,
        confirmLabel: "Delete",
        danger: true
      }))
    ) return;
    try {
      await removeLocalEntry(selectedLocal.path, selectedLocal.isDirectory);
      await openLocalDirectory(currentPath);
    } catch (cause) {
      error = errorMessage(cause);
    }
  }

  async function editLocalPermissions() {
    if (!selectedLocal) return;
    permissionTarget = { pane: "local", entry: selectedLocal };
  }

  async function editRemotePermissions() {
    if (!session || !selectedRemote) return;
    permissionTarget = { pane: "remote", entry: selectedRemote };
  }

  async function savePermissions(mode: number) {
    const target = permissionTarget;
    if (!target) return;
    try {
      if (target.pane === "local") {
        await setLocalPermissions(target.entry.path, mode);
        await openLocalDirectory(currentPath);
      } else {
        if (!session) throw new Error("The SFTP session is no longer connected.");
        await setRemotePermissions(session.id, target.entry.path, mode);
        await openRemoteDirectory(remotePath);
      }
      permissionTarget = null;
    } catch (cause) {
      error = errorMessage(cause);
      throw cause;
    }
  }

  async function renameSelectedRemote() {
    if (!session || !selectedRemote) return;
    const name = await promptDialog({
      title: "Rename remote item",
      label: "New name",
      initialValue: selectedRemote.name,
      confirmLabel: "Rename"
    });
    if (!name || name.includes("/") || name === selectedRemote.name) return;
    try {
      await renameRemoteEntry(
        session.id,
        selectedRemote.path,
        joinRemote(remotePath, name)
      );
      await openRemoteDirectory(remotePath);
    } catch (cause) {
      error = errorMessage(cause);
    }
  }

  async function deleteSelectedRemote() {
    if (!session || !selectedRemote) return;
    if (
      !(await confirmDialog({
        title: "Delete remote item?",
        message: `Delete “${selectedRemote.name}” from the remote host?`,
        confirmLabel: "Delete",
        danger: true
      }))
    ) return;
    try {
      await removeRemoteEntry(
        session.id,
        selectedRemote.path,
        selectedRemote.isDirectory
      );
      await openRemoteDirectory(remotePath);
    } catch (cause) {
      error = errorMessage(cause);
    }
  }

  function joinRemote(parent: string, child: string): string {
    return parent.endsWith("/") ? `${parent}${child}` : `${parent}/${child}`;
  }

  function errorMessage(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="sftp-page">
  {#if error}<div class="sftp-banner error-message">{error}</div>{/if}

  <div class="sftp-split">
    <div
      class="file-pane"
      class:drop-active={dropTarget === "local"}
      role="region"
      aria-label="Local files"
      ondragover={(event) => dragOverPane(event, "local")}
      ondragleave={(event) => dragLeavePane(event, "local")}
      ondrop={(event) => void dropOnPane(event, "local")}
    >
      <div class="file-pane-header">
        <div class="pane-identity">
          <span class="host-badge mini blue"><Icon name="folder" size={16} /></span>
          <strong>Local</strong>
        </div>
        <div class="toolbar-spacer"></div>
        <label class="file-filter">
          <Icon name="search" size={16} />
          <input aria-label="Filter local files" placeholder="Filter" bind:value={localQuery} />
        </label>
        <div class="file-actions">
          <button
            class="actions-trigger"
            class:active={localActionsOpen}
            onclick={() => (localActionsOpen = !localActionsOpen)}
          >Actions <Icon name="chevron" size={14} /></button>
          {#if localActionsOpen}
            <div class="file-actions-menu">
              <button disabled={!session || !selectedLocal} onclick={() => void uploadSelected()}>
                Copy to target directory
              </button>
              <button disabled={!selectedLocal} onclick={() => void renameSelectedLocal()}>Rename</button>
              <button class="danger" disabled={!selectedLocal} onclick={() => void deleteSelectedLocal()}>Delete</button>
              <div></div>
              <button onclick={() => void openLocalDirectory(currentPath)}>Refresh</button>
              <button onclick={() => void newLocalFolder()}>New Folder</button>
              <button disabled={!selectedLocal} onclick={() => void editLocalPermissions()}>Edit Permissions</button>
            </div>
          {/if}
        </div>
      </div>
      <div class="breadcrumb">
        <button
          class="history-button"
          title="Back"
          disabled={localHistoryIndex <= 0}
          onclick={() => void navigateLocalHistory(-1)}
        ><Icon name="arrow-left" size={16} /></button>
        <button
          class="history-button"
          title="Forward"
          disabled={localHistoryIndex >= localHistory.length - 1}
          onclick={() => void navigateLocalHistory(1)}
        ><Icon name="chevron-right" size={16} /></button>
        {#each localBreadcrumbs as crumb, index (crumb.path)}
          {#if index > 0}<Icon name="chevron" size={14} />{/if}
          <button class="crumb-button" onclick={() => void openLocalDirectory(crumb.path)}>
            {#if index === 0}<Icon name="folder" size={16} />{/if}{crumb.name}
          </button>
        {/each}
      </div>
      <div class="file-header">
        <button onclick={() => toggleSort("local", "name")}>
          Name <span>{sortMarker("local", "name")}</span>
        </button>
        <button onclick={() => toggleSort("local", "modified")}>
          Date Modified <span>{sortMarker("local", "modified")}</span>
        </button>
        <button onclick={() => toggleSort("local", "size")}>
          Size <span>{sortMarker("local", "size")}</span>
        </button>
        <button onclick={() => toggleSort("local", "kind")}>
          Kind <span>{sortMarker("local", "kind")}</span>
        </button>
      </div>
      <div class="file-list">
        {#if localLoading}
          <div class="loading-row">Reading local files…</div>
        {:else}
          {#if currentPath !== "/"}
            <button class="file-row parent-row" onclick={localParent}>
              <span class="file-name">
                <span class="file-kind-icon"><Icon name="folder" size={18} /></span>
                <span class="file-name-copy"><strong>..</strong></span>
              </span>
              <span>--</span><span>--</span><span>folder</span>
            </button>
          {/if}
          {#each visibleLocalEntries as entry (entry.path)}
            <button
              class="file-row"
              class:selected={selectedLocal?.path === entry.path}
              draggable="true"
              onclick={() => (selectedLocal = entry)}
              ondblclick={() => entry.isDirectory && openLocalDirectory(entry.path)}
              oncontextmenu={(event) => showContextMenu(event, "local", entry)}
              ondragstart={(event) => startEntryDrag(event, "local", entry)}
              ondragend={finishEntryDrag}
            >
              <span class="file-name">
                <span class="file-kind-icon {fileInfo(entry.name, entry.isDirectory).tone}">
                  <Icon name={fileInfo(entry.name, entry.isDirectory).icon} size={18} />
                </span>
                <span class="file-name-copy">
                  <strong>{entry.name}</strong>
                  <small>{formatPermissions(entry.mode, entry.isDirectory, entry.isSymlink)}</small>
                </span>
              </span>
              <span>{formatModified(entry.modifiedUnixMs)}</span>
              <span>{entry.isDirectory ? "--" : humanFileSize(entry.size)}</span>
              <span>{fileInfo(entry.name, entry.isDirectory).kind}</span>
            </button>
          {/each}
        {/if}
      </div>
      {#if dropTarget === "local"}
        <div class="file-drop-overlay">
          <Icon name="arrow-down" size={30} />
          <strong>Drop files here</strong>
          <span>Copy to {currentPath}</span>
        </div>
      {/if}
    </div>

    <div
      class="file-pane remote-pane"
      class:drop-active={dropTarget === "remote"}
      role="region"
      aria-label="Remote files"
      ondragover={(event) => dragOverPane(event, "remote")}
      ondragleave={(event) => dragLeavePane(event, "remote")}
      ondrop={(event) => void dropOnPane(event, "remote")}
    >
      {#if !session}
        <div class="sftp-host-picker">
          <header>
            <button
              class="host-picker-back"
              title="Parent group"
              disabled={!selectedHostGroupId}
              onclick={hostGroupBack}
            >←</button>
            <div>
              <strong>{connecting ? "Connecting…" : "Select Host"}</strong>
              <span>{currentHostGroup()?.name ?? "Local vault"}</span>
            </div>
            <span class="local-badge"><Icon name="folder" size={15} /> Local</span>
          </header>
          <label class="host-picker-search">
            <Icon name="search" size={17} />
            <input aria-label="Search SFTP hosts" placeholder="Search" bind:value={hostQuery} />
          </label>
          <div class="host-picker-scroll">
            {#if directHostGroups().length > 0}
              <section>
                <h2>Groups</h2>
                <div class="host-picker-list">
                  {#each directHostGroups() as group (group.id)}
                    <button
                      class="host-picker-row"
                      onclick={() => (selectedHostGroupId = group.id)}
                    >
                      <span class="host-badge group-badge"><Icon name="grid" size={20} /></span>
                      <span>
                        <strong>{group.name}</strong>
                        <small>{groupHostCount(group.id)} host{groupHostCount(group.id) === 1 ? "" : "s"}</small>
                      </span>
                      <Icon name="chevron-right" size={15} />
                    </button>
                  {/each}
                </div>
              </section>
            {/if}
            <section>
              <h2>Hosts</h2>
              <div class="host-picker-list">
                {#each selectableHosts() as host (host.id)}
                  <button
                    class="host-picker-row"
                    disabled={connecting}
                    onclick={() => void connectRemote(host.id)}
                  >
                    <span class="host-badge {host.color}"><Icon name="terminal" size={19} /></span>
                    <span>
                      <strong>{host.label}</strong>
                      <small>{host.username}@{host.address}</small>
                    </span>
                  </button>
                {:else}
                  <p class="host-picker-empty">No matching hosts.</p>
                {/each}
              </div>
            </section>
          </div>
        </div>
      {:else}
        <div class="file-pane-header">
          <div class="pane-identity">
            <span class="host-badge mini amber"><Icon name="terminal" size={16} /></span>
            <strong>{hosts.find((host) => host.id === selectedHostId)?.label ?? "Remote"}</strong>
          </div>
          <div class="toolbar-spacer"></div>
          <label class="file-filter">
            <Icon name="search" size={16} />
            <input aria-label="Filter remote files" placeholder="Filter" bind:value={remoteQuery} />
          </label>
          <div class="file-actions">
            <button
              class="actions-trigger"
              class:active={remoteActionsOpen}
              onclick={() => (remoteActionsOpen = !remoteActionsOpen)}
            >Actions <Icon name="chevron" size={14} /></button>
            {#if remoteActionsOpen}
              <div class="file-actions-menu align-right">
                <button disabled={!selectedRemote} onclick={() => void downloadSelected()}>
                  Copy to target directory
                </button>
                <button disabled={!selectedRemote} onclick={() => void renameSelectedRemote()}>Rename</button>
                <button class="danger" disabled={!selectedRemote} onclick={() => void deleteSelectedRemote()}>Delete</button>
                <div></div>
                <button onclick={() => void openRemoteDirectory(remotePath)}>Refresh</button>
                <button onclick={() => void newRemoteFolder()}>New Folder</button>
                <button onclick={() => (showHidden = !showHidden)}>
                  {showHidden ? "Hide Hidden Files" : "Show Hidden Files"}
                </button>
                <button disabled={!selectedRemote} onclick={() => void editRemotePermissions()}>Edit Permissions</button>
                <button class="danger" onclick={() => void disconnectRemote()}>Close</button>
              </div>
            {/if}
          </div>
        </div>
        <div class="remote-toolbar">
          <div class="breadcrumb">
            <button
              class="history-button"
              title="Back"
              disabled={remoteHistoryIndex <= 0}
              onclick={() => void navigateRemoteHistory(-1)}
            ><Icon name="arrow-left" size={16} /></button>
            <button
              class="history-button"
              title="Forward"
              disabled={remoteHistoryIndex >= remoteHistory.length - 1}
              onclick={() => void navigateRemoteHistory(1)}
            ><Icon name="chevron-right" size={16} /></button>
            {#each remoteBreadcrumbs as crumb, index (crumb.path)}
              {#if index > 0}<Icon name="chevron" size={14} />{/if}
              <button class="crumb-button" onclick={() => void openRemoteDirectory(crumb.path)}>
                {#if index === 0}<Icon name="folder" size={16} />{/if}{crumb.name}
              </button>
            {/each}
          </div>
        </div>
        <div class="file-header">
          <button onclick={() => toggleSort("remote", "name")}>
            Name <span>{sortMarker("remote", "name")}</span>
          </button>
          <button onclick={() => toggleSort("remote", "modified")}>
            Date Modified <span>{sortMarker("remote", "modified")}</span>
          </button>
          <button onclick={() => toggleSort("remote", "size")}>
            Size <span>{sortMarker("remote", "size")}</span>
          </button>
          <button onclick={() => toggleSort("remote", "kind")}>
            Kind <span>{sortMarker("remote", "kind")}</span>
          </button>
        </div>
        <div class="file-list">
          {#if remoteLoading}
            <div class="loading-row">Reading remote files…</div>
          {:else}
            {#if remotePath !== "/"}
              <button class="file-row parent-row" onclick={remoteParent}>
                <span class="file-name">
                  <span class="file-kind-icon folder"><Icon name="folder" size={18} /></span>
                  <span class="file-name-copy"><strong>..</strong></span>
                </span>
                <span>--</span><span>--</span><span>folder</span>
              </button>
            {/if}
            {#each visibleRemoteEntries as entry (entry.path)}
              <button
                class="file-row"
                class:selected={selectedRemote?.path === entry.path}
                draggable="true"
                onclick={() => (selectedRemote = entry)}
                ondblclick={() => entry.isDirectory && openRemoteDirectory(entry.path)}
                oncontextmenu={(event) => showContextMenu(event, "remote", entry)}
                ondragstart={(event) => startEntryDrag(event, "remote", entry)}
                ondragend={finishEntryDrag}
              >
                <span class="file-name">
                  <span class="file-kind-icon {fileInfo(entry.name, entry.isDirectory).tone}">
                    <Icon name={fileInfo(entry.name, entry.isDirectory).icon} size={18} />
                  </span>
                  <span class="file-name-copy">
                    <strong>{entry.name}</strong>
                    <small>{formatPermissions(entry.mode, entry.isDirectory, entry.isSymlink)}</small>
                  </span>
                </span>
                <span>{formatModified(entry.modifiedUnixMs)}</span>
                <span>{entry.isDirectory ? "--" : humanFileSize(entry.size)}</span>
                <span>{fileInfo(entry.name, entry.isDirectory).kind}</span>
              </button>
            {/each}
          {/if}
        </div>
        {#if dropTarget === "remote"}
          <div class="file-drop-overlay">
            <Icon name="arrow-down" size={30} />
            <strong>Drop files here</strong>
            <span>Copy to {remotePath}</span>
          </div>
        {/if}
      {/if}
    </div>
  </div>

  {#if transferLabel}
    <div class="transfer-shelf" class:failed={transferStatus === "failed"}>
      <span class="transfer-job-icon">
        <Icon name={transferStatus === "completed" ? "shield" : "forward"} size={19} />
      </span>
      <div class="transfer-job-copy">
        <header>
          <strong>{transferLabel}</strong>
          <span>{transferDirection}</span>
          <b>
            {transferStatus === "completed"
              ? "Completed"
              : transferStatus === "failed"
                ? "Failed"
                : transferTotal > 0
                  ? `${Math.round(transferProgress * 100)}%`
                  : "Preparing…"}
          </b>
        </header>
        <div
          class="transfer-progress-track"
          class:indeterminate={transferStatus === "running" && transferTotal === 0}
        >
          <i style:width={`${Math.round(transferProgress * 100)}%`}></i>
        </div>
        <footer>
          {#if transferStatus === "failed"}
            <span title={transferFailure}>{transferFailure}</span>
          {:else if transferTotal > 0}
            <span>{humanFileSize(transferTransferred)} of {humanFileSize(transferTotal)}</span>
            {#if transferStatus === "running" && transferRate > 0}
              <span>{humanFileSize(Math.round(transferRate))}/s</span>
            {/if}
          {:else if transferStatus === "completed"}
            <span>Folder structure copied</span>
          {:else}
            <span>Calculating the total folder size</span>
          {/if}
        </footer>
      </div>
      {#if transferStatus !== "running"}
        <button class="transfer-dismiss" title="Dismiss" onclick={dismissTransfer}>
          <Icon name="close" size={16} />
        </button>
      {/if}
    </div>
  {/if}

  {#if permissionTarget}
    <PermissionEditor
      path={permissionTarget.entry.path}
      initialMode={permissionTarget.entry.mode ??
        (permissionTarget.entry.isDirectory ? 0o755 : 0o644)}
      owner={permissionTarget.entry.owner}
      group={permissionTarget.entry.group}
      onclose={() => (permissionTarget = null)}
      onsave={savePermissions}
    />
  {/if}

  {#if contextMenu}
    <div
      class="file-context-menu"
      style:left={`${contextMenu.x}px`}
      style:top={`${contextMenu.y}px`}
      role="menu"
      tabindex="-1"
    >
      <button onclick={() => void openEntry(contextMenu!.pane, contextMenu!.entry)}>Open</button>
      <button
        disabled={contextMenu.entry.isDirectory}
        onclick={() => void openEntry(contextMenu!.pane, contextMenu!.entry, true)}
      >Open with…</button>
      <div></div>
      <button disabled={!session} onclick={() => void contextCopy()}>
        Copy to target directory
      </button>
      <button onclick={() => void contextRename()}>Rename</button>
      <button class="danger" onclick={() => void contextDelete()}>Delete</button>
      <div></div>
      <button onclick={() => void contextRefresh()}>Refresh</button>
      <button onclick={() => void contextNewFolder()}>New Folder</button>
      <button onclick={() => void contextPermissions()}>Edit Permissions</button>
    </div>
  {/if}
</section>
