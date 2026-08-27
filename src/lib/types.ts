export type HostColor = "blue" | "violet" | "rose" | "amber" | "emerald" | "slate";
export type TerminalTheme =
  | "heminus_dark"
  | "gruvbox_dark"
  | "kanagawa_wave"
  | "hacker_blue"
  | "paper_light"
  | "flexoki_dark"
  | "flexoki_light"
  | "kanagawa_lotus"
  | "hacker_green"
  | "hacker_red"
  | "rose_pine_moon"
  | "rose_pine_dawn"
  | "catppuccin_mocha"
  | "tokyo_night"
  | "tokyo_day"
  | "solarized_dark"
  | "solarized_light"
  | "dracula"
  | "monokai";

export interface EnvironmentVariable {
  name: string;
  value: string;
}

export type ProxyKind = "http" | "socks5";

export interface HostProxy {
  kind: ProxyKind;
  hostname: string;
  port: number;
  username: string | null;
  secret_stored: boolean;
}

export const proxyDefaultPort: Record<ProxyKind, number> = {
  http: 3128,
  socks5: 1080
};

export const proxyKindLabel: Record<ProxyKind, string> = {
  http: "HTTP",
  socks5: "SOCKS5"
};

export interface Host {
  id: string;
  label: string;
  address: string;
  port: number;
  username: string;
  group_name: string | null;
  group_id: string | null;
  tags: string[];
  color: HostColor;
  identity_id: string | null;
  jump_host_ids: string[];
  environment: EnvironmentVariable[];
  proxy: HostProxy | null;
  terminal_theme: TerminalTheme;
  terminal_font_size: number;
  created_at: string;
  updated_at: string;
}

export interface TerminalAppearance {
  theme: TerminalTheme;
  fontSize: number;
}

export interface DetachedTerminalSpec {
  id: string;
  title: string;
  hostId: string | null;
  sessionId: string | null;
  appearance: TerminalAppearance;
}

export interface DetachedWorkspaceSpec {
  name: string;
  paneIds: string[];
  layout: WorkspaceLayout | null;
  activePaneId: string | null;
}

export interface DetachedWindowPayload {
  title: string;
  tabs: DetachedTerminalSpec[];
  workspace: DetachedWorkspaceSpec | null;
  cwd?: string | null;
}

export interface TerminalTabTransfer {
  payload: DetachedWindowPayload;
  clientX: number;
  clientY: number;
}

export interface TerminalTabTransferResult {
  targetLabel: string;
  createdWindow: boolean;
  transferred: boolean;
}

export interface TerminalTabPointerState {
  screenX: number;
  screenY: number;
  clientX: number;
  clientY: number;
  primaryPressed: boolean;
}

export interface TerminalCommandRequest {
  id: string;
  command: string;
  run: boolean;
}

export type IdentityKind = "agent" | "key_file" | "password";

export interface Identity {
  id: string;
  label: string;
  kind: IdentityKind;
  username: string | null;
  key_path: string | null;
  secret_stored: boolean;
  created_at: string;
}

export interface VaultGroup {
  id: string;
  name: string;
  parent_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface WorkspacePane {
  id: string;
  host_id: string | null;
  title: string;
}

export type SplitDirection = "horizontal" | "vertical";

export type WorkspaceLayout =
  | { type: "pane"; pane_id: string }
  | {
      type: "split";
      direction: SplitDirection;
      ratio?: number;
      first: WorkspaceLayout;
      second: WorkspaceLayout;
    };

export interface SavedWorkspace {
  id: string;
  name: string;
  panes: WorkspacePane[];
  layout: WorkspaceLayout | null;
  split: boolean;
  broadcast: boolean;
  active_pane_id: string | null;
  updated_at: string;
}

export interface PrivateKeyImport {
  identity: Identity;
  format: string;
  encrypted: boolean;
  alreadyPresent: boolean;
}

export interface KeyMaterial {
  privateKey: string;
  publicKey: string | null;
  certificate: string | null;
}

export interface LocalEntry {
  name: string;
  path: string;
  isDirectory: boolean;
  isSymlink: boolean;
  size: number | null;
  modifiedUnixMs: number | null;
  mode: number | null;
  owner: string | null;
  group: string | null;
}

export interface LocalBreadcrumb {
  name: string;
  path: string;
}

export interface LocalPathInfo {
  path: string;
  parentPath: string | null;
  breadcrumbs: LocalBreadcrumb[];
}

export interface PlatformCapabilities {
  localPermissionEditing: boolean;
  customApplicationOpen: boolean;
  foreignProcessTermination: boolean;
  sshAvailable: boolean;
  sshKeygenAvailable: boolean;
  localShell: string | null;
}

export interface RemoteEntry {
  name: string;
  path: string;
  isDirectory: boolean;
  isSymlink: boolean;
  size: number | null;
  modifiedUnixMs: number | null;
  mode: number | null;
  owner: string | null;
  group: string | null;
}

export interface SftpOpenResult {
  id: string;
  homePath: string;
}

export interface TransferEvent {
  transferred: number;
  total: number;
}

export interface Snippet {
  id: string;
  title: string;
  command: string;
  description: string;
  favorite: boolean;
  created_at: string;
}

export type ForwardKind = "local" | "remote" | "dynamic";

export interface PortForward {
  id: string;
  name: string;
  kind: ForwardKind;
  bind_host: string;
  bind_port: number;
  destination_host: string | null;
  destination_port: number | null;
  host_id: string;
  enabled: boolean;
  created_at: string;
}

export interface TunnelState {
  id: string;
  running: boolean;
  exitCode: number | null;
  message: string | null;
}

export interface TunnelPortProcess {
  pid: number;
  name: string;
  command: string;
  requiresElevation: boolean;
}

export interface KnownHostEntry {
  hosts: string;
  keyType: string;
  fingerprint: string;
  lineNumber: number;
  hashed: boolean;
  resolved: boolean;
}

export type SessionStatus = "connecting" | "connected" | "disconnected" | "failed";

export interface SessionRecord {
  id: string;
  host_id: string | null;
  title: string;
  started_at: string;
  ended_at: string | null;
  status: SessionStatus;
}

export type ConnectionLogLevel = "debug" | "info" | "warning" | "error";

export type ConnectionStage =
  | "connecting"
  | "handshake"
  | "authenticating"
  | "authenticated"
  | "ready"
  | "failed";

export interface ConnectionLogEntry {
  /** Index into the connection's hops: jump hosts in order, then the host. */
  hop: number;
  level: ConnectionLogLevel;
  message: string;
  stage: ConnectionStage | null;
}

export type TerminalEvent =
  | { kind: "output"; bytes: number[] }
  | { kind: "exit" }
  | { kind: "disconnect" }
  | { kind: "error"; message: string }
  | { kind: "hops"; labels: string[] }
  | ({ kind: "log" } & ConnectionLogEntry);

/** A process a terminal session started and would leave behind when closed. */
export interface SessionProcess {
  pid: number;
  name: string;
  command: string;
  leader: boolean;
}

export type MainPage =
  | "hosts"
  | "keychain"
  | "forwarding"
  | "snippets"
  | "known-hosts"
  | "logs"
  | "sftp"
  | "terminal"
  | "new-tab";
