import { Channel, invoke } from "@tauri-apps/api/core";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DetachedWindowPayload,
  Host,
  Identity,
  KeyMaterial,
  KnownHostEntry,
  LocalEntry,
  PortForward,
  PrivateKeyImport,
  RemoteEntry,
  SessionRecord,
  SavedWorkspace,
  SftpOpenResult,
  Snippet,
  TerminalTabTransfer,
  TerminalTabTransferResult,
  TerminalEvent,
  TransferEvent,
  TunnelState,
  VaultGroup
} from "./types";
import { matchesSearch } from "./format";

const isTauri = "__TAURI_INTERNALS__" in window;

export async function createDetachedTerminalWindow(
  payload: DetachedWindowPayload
): Promise<string> {
  if (!isTauri) throw new Error("Detached windows require the desktop app");
  return invoke<string>("create_detached_terminal_window", {
    payload: JSON.stringify(payload),
    title: payload.title
  });
}

export async function takeDetachedTerminalPayload(): Promise<DetachedWindowPayload | null> {
  if (!isTauri) return null;
  const payload = await invoke<string | null>("take_detached_terminal_payload");
  return payload ? JSON.parse(payload) as DetachedWindowPayload : null;
}

export async function transferTerminalTab(
  payload: DetachedWindowPayload,
  screenX: number,
  screenY: number
): Promise<TerminalTabTransferResult> {
  if (!isTauri) throw new Error("Moving tabs between windows requires the desktop app");
  return invoke<TerminalTabTransferResult>("transfer_terminal_tab", {
    payload: JSON.stringify(payload),
    title: payload.title,
    screenX,
    screenY
  });
}

export async function listenForTerminalTabTransfers(
  onTransfer: (transfer: TerminalTabTransfer) => void
): Promise<UnlistenFn> {
  if (!isTauri) return () => {};
  return listen<{ payload: string; clientX: number; clientY: number }>(
    "terminal-tab-transfer",
    (event) => {
      onTransfer({
        payload: JSON.parse(event.payload.payload) as DetachedWindowPayload,
        clientX: event.payload.clientX,
        clientY: event.payload.clientY
      });
    }
  );
}

export async function listHosts(query = ""): Promise<Host[]> {
  if (!isTauri) return demoHosts.filter((host) => searchable(host, query));
  return invoke<Host[]>("list_hosts", { query: query || null });
}

export async function saveHost(host: Host): Promise<Host> {
  if (!isTauri) return host;
  return invoke<Host>("save_host", { host });
}

export async function deleteHost(id: string): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("delete_host", { id });
}

export async function listGroups(): Promise<VaultGroup[]> {
  if (!isTauri) return [];
  return invoke<VaultGroup[]>("list_groups");
}

export async function saveGroup(group: VaultGroup): Promise<VaultGroup> {
  if (!isTauri) return group;
  return invoke<VaultGroup>("save_group", { group });
}

export async function deleteGroup(id: string): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("delete_group", { id });
}

export async function listWorkspaces(): Promise<SavedWorkspace[]> {
  if (!isTauri) return [];
  return invoke<SavedWorkspace[]>("list_workspaces");
}

export async function saveWorkspace(workspace: SavedWorkspace): Promise<SavedWorkspace> {
  if (!isTauri) return workspace;
  return invoke<SavedWorkspace>("save_workspace", { workspace });
}

export async function deleteWorkspace(id: string): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("delete_workspace", { id });
}

export async function listIdentities(): Promise<Identity[]> {
  if (!isTauri) return [];
  return invoke<Identity[]>("list_identities");
}

export async function saveIdentity(identity: Identity): Promise<Identity> {
  if (!isTauri) return identity;
  return invoke<Identity>("save_identity", { identity });
}

export async function deleteIdentity(id: string): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("delete_identity", { id });
}

export async function setIdentitySecret(id: string, secret: string): Promise<boolean> {
  return invoke<boolean>("set_identity_secret", { id, secret });
}

export async function deleteIdentitySecret(id: string): Promise<boolean> {
  return invoke<boolean>("delete_identity_secret", { id });
}

export async function readKeyMaterial(id: string): Promise<KeyMaterial> {
  return invoke<KeyMaterial>("read_key_material", { id });
}

export async function generateSshKey(
  label: string,
  fileName: string,
  comment: string,
  passphrase: string | null
): Promise<Identity> {
  return invoke<Identity>("generate_ssh_key", {
    label,
    fileName,
    comment,
    passphrase
  });
}

export async function importPrivateKey(path: string): Promise<PrivateKeyImport> {
  return invoke<PrivateKeyImport>("import_private_key", { path });
}

export async function listSnippets(): Promise<Snippet[]> {
  if (!isTauri) return [];
  return invoke<Snippet[]>("list_snippets");
}

export async function saveSnippet(snippet: Snippet): Promise<Snippet> {
  if (!isTauri) return snippet;
  const saved = await invoke<Snippet>("save_snippet", { snippet });
  await emit("snippets-changed");
  return saved;
}

export async function deleteSnippet(id: string): Promise<boolean> {
  if (!isTauri) return true;
  const deleted = await invoke<boolean>("delete_snippet", { id });
  if (deleted) await emit("snippets-changed");
  return deleted;
}

export async function listenForSnippetChanges(
  onChanged: () => void
): Promise<UnlistenFn> {
  if (!isTauri) return () => {};
  return listen("snippets-changed", onChanged);
}

export async function listCommandHistory(
  hostId: string | null,
  limit = 200
): Promise<string[]> {
  if (!isTauri) {
    const history = JSON.parse(
      localStorage.getItem(`heminus-command-history:${hostId ?? "local"}`) ?? "[]"
    ) as string[];
    return history.slice(0, limit);
  }
  return invoke<string[]>("list_command_history", { hostId, limit });
}

export async function listAllCommandHistory(limit = 300): Promise<string[]> {
  if (!isTauri) {
    const commands = new Set<string>();
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (!key?.startsWith("heminus-command-history:")) continue;
      const history = JSON.parse(localStorage.getItem(key) ?? "[]") as string[];
      history.forEach((command) => commands.add(command));
    }
    return [...commands].slice(0, limit);
  }
  return invoke<string[]>("list_all_command_history", { limit });
}

export async function recordCommandHistory(
  hostId: string | null,
  command: string
): Promise<boolean> {
  if (!isTauri) {
    const key = `heminus-command-history:${hostId ?? "local"}`;
    const history = await listCommandHistory(hostId, 200);
    localStorage.setItem(key, JSON.stringify([command, ...history.filter((item) => item !== command)]));
    return true;
  }
  return invoke<boolean>("record_command_history", { hostId, command });
}

export async function deleteCommandHistory(command: string): Promise<boolean> {
  if (!isTauri) {
    let deleted = false;
    for (let index = localStorage.length - 1; index >= 0; index -= 1) {
      const key = localStorage.key(index);
      if (!key?.startsWith("heminus-command-history:")) continue;
      const history = JSON.parse(localStorage.getItem(key) ?? "[]") as string[];
      const next = history.filter((item) => item !== command);
      if (next.length === history.length) continue;
      localStorage.setItem(key, JSON.stringify(next));
      deleted = true;
    }
    return deleted;
  }
  const deleted = await invoke<boolean>("delete_command_history", { command });
  if (deleted) await emit("command-history-changed");
  return deleted;
}

export async function listenForCommandHistoryChanges(
  onChanged: () => void
): Promise<UnlistenFn> {
  if (!isTauri) return () => {};
  return listen("command-history-changed", onChanged);
}

export async function listPortForwards(): Promise<PortForward[]> {
  if (!isTauri) return [];
  return invoke<PortForward[]>("list_port_forwards");
}

export async function savePortForward(rule: PortForward): Promise<PortForward> {
  if (!isTauri) return rule;
  return invoke<PortForward>("save_port_forward", { rule });
}

export async function deletePortForward(id: string): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("delete_port_forward", { id });
}

export async function startTunnel(rule: PortForward, host: Host): Promise<void> {
  return invoke("tunnel_start", { rule, host });
}

export async function stopTunnel(id: string): Promise<boolean> {
  return invoke<boolean>("tunnel_stop", { id });
}

export async function tunnelStates(): Promise<TunnelState[]> {
  if (!isTauri) return [];
  return invoke<TunnelState[]>("tunnel_states");
}

export async function listLocalEntries(path?: string): Promise<LocalEntry[]> {
  if (!isTauri) return [];
  return invoke<LocalEntry[]>("list_local_entries", { path: path ?? null });
}

export async function createLocalDirectory(path: string): Promise<void> {
  return invoke("create_local_directory", { path });
}

export async function renameLocalEntry(oldPath: string, newPath: string): Promise<void> {
  return invoke("rename_local_entry", { oldPath, newPath });
}

export async function removeLocalEntry(path: string, isDirectory: boolean): Promise<void> {
  return invoke("remove_local_entry", { path, isDirectory });
}

export async function setLocalPermissions(path: string, mode: number): Promise<void> {
  return invoke("set_local_permissions", { path, mode });
}

export async function openLocalEntry(
  path: string,
  application: string | null = null
): Promise<void> {
  return invoke("open_local_entry", { path, application });
}

export async function homeDirectory(): Promise<string> {
  if (!isTauri) return "/";
  return invoke<string>("home_directory");
}

export async function listKnownHosts(): Promise<KnownHostEntry[]> {
  if (!isTauri) return [];
  return invoke<KnownHostEntry[]>("list_known_hosts");
}

export async function deleteKnownHostEntries(lineNumbers: number[]): Promise<number> {
  if (!isTauri) return lineNumbers.length;
  return invoke<number>("delete_known_host_entries", { lineNumbers });
}

export async function listSessions(limit = 200): Promise<SessionRecord[]> {
  if (!isTauri) return [];
  return invoke<SessionRecord[]>("list_sessions", { limit });
}

export async function disconnectSession(historyId: string): Promise<boolean> {
  if (!isTauri) return false;
  return invoke<boolean>("terminal_disconnect_history", { historyId });
}

export async function openTerminal(
  rows: number,
  cols: number,
  channel: Channel<TerminalEvent>,
  host: Host | null = null,
  sessionTitle?: string
): Promise<string> {
  return invoke<string>("terminal_open", {
    rows,
    cols,
    host,
    onEvent: channel,
    sessionTitle
  });
}

export async function renameTerminalSession(id: string, title: string): Promise<boolean> {
  return invoke<boolean>("terminal_rename", { id, title });
}

export async function attachTerminal(
  id: string,
  onEvent: (event: TerminalEvent) => void
): Promise<UnlistenFn> {
  const eventName = `terminal-session-${id}`;
  const unlisten = await listen<TerminalEvent>(eventName, (event) => {
    onEvent(event.payload);
  });
  try {
    await invoke<boolean>("terminal_attach", { id });
    return unlisten;
  } catch (cause) {
    unlisten();
    throw cause;
  }
}

export async function writeTerminal(id: string, bytes: number[]): Promise<void> {
  return invoke("terminal_write", { id, bytes });
}

export async function resizeTerminal(id: string, rows: number, cols: number): Promise<void> {
  return invoke("terminal_resize", { id, rows, cols });
}

export async function closeTerminal(id: string): Promise<boolean> {
  return invoke<boolean>("terminal_close", { id });
}

export async function openSftpSession(host: Host): Promise<SftpOpenResult> {
  return invoke<SftpOpenResult>("sftp_open", { host });
}

export async function listRemoteEntries(id: string, path: string): Promise<RemoteEntry[]> {
  return invoke<RemoteEntry[]>("sftp_list", { id, path });
}

export async function closeSftpSession(id: string): Promise<boolean> {
  return invoke<boolean>("sftp_close", { id });
}

export async function createRemoteDirectory(id: string, path: string): Promise<void> {
  return invoke("sftp_create_dir", { id, path });
}

export async function removeRemoteEntry(
  id: string,
  path: string,
  isDirectory: boolean
): Promise<void> {
  return invoke("sftp_remove", { id, path, isDirectory });
}

export async function renameRemoteEntry(
  id: string,
  oldPath: string,
  newPath: string
): Promise<void> {
  return invoke("sftp_rename", { id, oldPath, newPath });
}

export async function setRemotePermissions(
  id: string,
  path: string,
  mode: number
): Promise<void> {
  return invoke("sftp_set_permissions", { id, path, mode });
}

export async function uploadFile(
  id: string,
  localPath: string,
  remotePath: string,
  channel: Channel<TransferEvent>
): Promise<void> {
  return invoke("sftp_upload", { id, localPath, remotePath, onProgress: channel });
}

export async function downloadFile(
  id: string,
  remotePath: string,
  localPath: string,
  channel: Channel<TransferEvent>
): Promise<void> {
  return invoke("sftp_download", { id, remotePath, localPath, onProgress: channel });
}

function searchable(host: Host, query: string): boolean {
  return matchesSearch([host.label, host.address, host.username, host.group_name], query);
}

const now = new Date().toISOString();
const demoHosts: Host[] = [
  {
    id: "b0c4b4c4-03fc-4de7-adc7-e3258fc00db1",
    label: "Local Ubuntu",
    address: "127.0.0.1",
    port: 22,
    username: "thaihoc",
    group_name: "This device",
    group_id: null,
    tags: ["local"],
    color: "blue",
    identity_id: null,
    jump_host_ids: [],
    environment: [],
    terminal_theme: "heminus_dark",
    terminal_font_size: 14,
    created_at: now,
    updated_at: now
  },
  {
    id: "0201aa7b-28bc-4e45-8f8b-943bcf17eaaa",
    label: "Development",
    address: "dev.example.local",
    port: 22,
    username: "ubuntu",
    group_name: "Infrastructure",
    group_id: null,
    tags: ["development"],
    color: "emerald",
    identity_id: null,
    jump_host_ids: [],
    environment: [],
    terminal_theme: "kanagawa_wave",
    terminal_font_size: 14,
    created_at: now,
    updated_at: now
  },
  {
    id: "f843ca37-9521-46d8-954e-651f2435c752",
    label: "Production",
    address: "prod.example.local",
    port: 22,
    username: "deploy",
    group_name: "Infrastructure",
    group_id: null,
    tags: ["production"],
    color: "rose",
    identity_id: null,
    jump_host_ids: [],
    environment: [],
    terminal_theme: "gruvbox_dark",
    terminal_font_size: 14,
    created_at: now,
    updated_at: now
  }
];
