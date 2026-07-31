# Heminus Desktop Plan

Status: active development  
Primary platform: Ubuntu 22.04 and 24.04, x86-64  
Architecture decision: Tauri 2 + Svelte 5 + TypeScript + Rust  
Product direction: a lightweight, local-first SSH workspace inspired by Termius

## 1. Product principles

Heminus should provide the important daily workflows of a modern SSH client
without copying Termius branding, proprietary assets, or source code.

The priorities are:

1. Security and predictable connection behavior.
2. Low package size and reasonable memory use.
3. A polished interface that remains responsive with many saved hosts.
4. Local-first data ownership.
5. A codebase that can later support Windows and macOS.

The first release is not a cloud/team clone. Account sync, shared encrypted
vaults, role-based access, and multiplayer terminals require a separate
protocol and threat model.

## 2. Chosen architecture

```text
Svelte 5 + TypeScript
        │
        │ typed Tauri commands and bounded channels
        ▼
Tauri 2
        │
        ▼
Rust application core
├── OpenSSH and portable PTY terminal sessions
├── structured SFTP transport
├── SSH port-forward processes
├── SQLite persistence
├── OS-native encrypted credential storage
├── app-private Ed25519 key generation and import
├── app-private known-host parsing and fingerprints
└── home-directory-confined filesystem operations
```

### Why this stack

- Tauri uses the operating system WebView instead of bundling Chromium.
- Svelte has a small runtime and is suitable for a custom, highly polished UI.
- Rust owns SSH, SFTP, PTY, filesystem, persistence, and process lifecycle.
- HTML and CSS make the reference layout practical to reproduce and refine.
- The frontend and most Rust code can be reused on Windows and macOS later.

### Explicitly rejected alternatives

| Option | Reason |
|---|---|
| Electron | Highest package and runtime memory overhead |
| GTK4 + VTE | Excellent Linux integration, but substantially more platform-specific |
| Wails + Go | No meaningful advantage over the existing Rust core |
| egui/Iced/Slint | Harder to achieve the desired desktop UI and terminal ecosystem |

The unused GTK4, libadwaita, and GTK4 VTE development packages from the
previous direction have been removed. WebKitGTK and Tauri build dependencies
remain because the selected architecture requires them.

## 3. Runtime and security boundaries

- The renderer never receives private-key contents.
- The Ubuntu package declares `openssh-client` as a hard dependency so remote
  features never silently rely on an undeclared system executable.
- Key identities store metadata and an absolute path inside the private Heminus
  SSH vault only.
- Passwords and key passphrases are stored only in the OS-native credential
  service. SQLite stores a boolean presence flag, never the secret.
- The askpass bridge passes only an identity UUID to child processes and reads
  the secret directly from the credential service.
- Dragged private keys are format-checked and copied into the Heminus vault at
  mode `0600` on Unix. The source file is not modified.
- Every SSH process ignores user/system config, default keys, and the system
  agent. Host-key verification uses Heminus's private `known_hosts`.
- SSH arguments are passed as separate process arguments, never assembled into
  a shell command.
- Local SFTP operations are confined to the canonicalized home directory.
- Downloads and uploads use hidden partial files and publish the destination
  only after the transfer succeeds.
- Terminal output is not written to session history.
- Production builds use a strict Content Security Policy, minimal Tauri
  capabilities, no remote code, and disabled development tools.
- Terminal and transfer data use bounded chunks rather than buffering complete
  streams in memory.

## 4. Current implementation

### Application shell and visual system

- [x] One Tauri window and one WebView.
- [x] English-only application copy.
- [x] Custom light vault interface and dark terminal interface.
- [x] Top-level Vaults, SFTP, and persistent terminal tabs.
- [x] Sidebar navigation and right-side inspectors.
- [x] Lazy-loaded feature panes and terminal runtime.
- [x] Responsive host grids, empty states, forms, menus, and progress feedback.

### Hosts and local data

- [x] SQLite database with WAL mode and migrations.
- [x] Host create, edit, delete, search, tags, nested/movable groups, and color.
- [x] Identity selection per host.
- [x] Explicit host creation independent of local OpenSSH configuration.
- [x] Structured host details with environment variables,
      one app-owned jump host, and a per-host terminal theme.
- [x] Recent connection metadata and session status.
- [x] One local host is seeded on a fresh database; fake remote hosts are not.

### Terminal and SSH

- [x] Local shell through a native PTY.
- [x] Remote SSH through the system OpenSSH client.
- [x] Password, passphrase, keyboard-interactive, and private-vault key-file flows.
- [x] Isolation from user/system OpenSSH config, known hosts, default keys, and agent.
- [x] Multiple persistent terminal tabs.
- [x] Up to 16 nested split terminal panes, closable/reorderable tabs,
      drag-to-split placement, guarded broadcast input, automatic workspace
      saving, and explicit restoration.
- [x] Resize, copy, paste, search, clickable web links, scrollback, and theme.
- [x] Confirmation before multiline paste.
- [x] Chunked terminal output and coalesced serialized input.
- [x] Session cleanup and Linux PTY EOF handling.

### Keychain and known hosts

- [x] Reusable password and key-file identities.
- [x] App-private generated and imported key storage.
- [x] Known-host list from Heminus's private vault.
- [x] SHA-256 key fingerprints.
- [x] OS-native keyring integration for passwords and key passphrases.
- [x] Ed25519 key generation with optional encrypted private keys.
- [x] Drag-and-drop OpenSSH, RSA, DSA, EC, and PKCS#8 PEM private-key import.
- [ ] Encrypted local vault for secrets not already managed by OpenSSH.

### SFTP

- [x] Two-pane local and remote browser.
- [x] Real local navigation confined to the home directory.
- [x] Structured remote listing over an OpenSSH SFTP subsystem.
- [x] Create directory, rename, and delete.
- [x] Streaming upload and download with 64 KiB buffers.
- [x] Transfer progress events.
- [x] Hidden partial files and safe destination publication.
- [x] Replace confirmation for existing files.
- [x] Filtering and optional dotfile visibility.
- [ ] Transfer cancellation and retry queue.
- [ ] Drag and drop.
- [ ] Permission editing and remote file editing.

### Port forwarding, snippets, and logs

- [x] Local, remote, and dynamic forwarding definitions.
- [x] Start and stop forwarding processes.
- [x] Snippet create, edit, delete, favorite, and inline terminal completion.
- [x] Per-host command history merged into terminal completion suggestions.
- [x] Connection-history metadata without terminal contents.
- [x] Capture and present actionable tunnel startup diagnostics.
- [ ] Optional explicit session logging with redaction controls.

## 5. Delivery phases

### Phase 0 — architecture migration

Status: complete.

- Replace the abandoned GTK4/VTE prototype with Tauri and Svelte.
- Preserve Rust domain and storage logic.
- Remove unused system development libraries.
- Establish strict Tauri capabilities and production bundling.

### Phase 1 — usable local-first desktop release

Status: feature-complete for the current scope; hardening is in progress.

- Hosts, identities, known hosts, terminal sessions, SFTP, tunnels, snippets,
  and history.
- App-owned host, key, credential, and known-host data with no local OpenSSH
  configuration integration.
- `.deb` packaging for Ubuntu x86-64.
- Automated type, unit, lint, build, and security checks.
- Manual UI and real-server acceptance testing by the user.

### Phase 2 — hardening and daily-use polish

Status: in progress.

- Add transfer cancellation and better failure recovery.
- Improve tunnel and SFTP authentication diagnostics.
- Add settings for font, theme, scrollback, and shortcuts.
- Add command palette and keyboard navigation.
- Continue keyring failure-mode and locked-session testing.
- Test large host lists, many tabs, large files, unstable connections, and
  process cleanup.
- Validate Ubuntu 22.04 and 24.04 packaging on clean systems.

### Phase 3 — advanced desktop parity

Status: partially complete.

- [x] Nested groups with create, move, rename, delete, and host-path updates.
- [x] Split terminal workspaces, restoration, and guarded broadcast input.
- [ ] Workspace templates and inherited group connection settings.
- CSV import/export.
- Remote editor and file-permission controls.
- Mosh, Telnet, Serial, and hardware-backed keys.
- ARM64 builds and other desktop operating systems.

### Phase 4 — optional cloud/team product

Status: intentionally out of scope until separately designed.

- End-to-end encrypted sync and device enrollment.
- Team vaults and access control.
- Device revocation and recovery.
- Shared session workflows.
- Independent security review and protocol documentation.

## 6. Performance budgets

The budgets are guardrails, not claims that WebKitGTK is free.

| Metric | Target | Current measured result |
|---|---:|---:|
| Ubuntu `.deb` | below 35 MB | 4.4 MB |
| Initial frontend JavaScript, gzip | below 50 KB | 33.1 KB |
| Terminal lazy chunk, gzip | below 120 KB | about 98 KB |
| Idle total PSS at reference window size | below Termius by at least 35% | about 299 MB vs about 647 MB |
| Terminal input batching delay | 8 ms or less | 4 ms |
| SFTP stream buffer | 256 KiB or less | 64 KiB |

The WebKit renderer dominates memory on Linux, especially at high DPI. A
compositing-disabled mode measured lower, but it is not enabled because smooth
terminal rendering is more important than winning an artificial idle metric.

## 7. Verification strategy

### Automated checks run during development

- `pnpm check`
- `pnpm test`
- `pnpm build`
- `pnpm audit --prod`
- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `pnpm tauri:build`

### Manual acceptance owned by the user

- Visual comparison at common window sizes and display scaling.
- Keyboard, focus, selection, clipboard, and terminal behavior.
- Password, agent, key-file, jump-host, and proxy connections.
- SFTP upload/download/replace against real servers.
- Tunnel connectivity from local applications.
- Suspend/resume, network loss, reconnect, and multi-tab workflows.

Manual UI testing is deliberately not reported as automated coverage.

## 8. Release gate

The first public package is ready when:

- every automated check passes;
- no secrets appear in logs, history, or renderer persistence;
- process cleanup is verified after closing tabs and the application;
- failed transfers do not publish incomplete destination files;
- package dependencies install on clean Ubuntu 22.04 and 24.04 systems;
- the user's manual acceptance checklist passes;
- known limitations are documented rather than hidden.
