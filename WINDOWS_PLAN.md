# Heminus Windows Plan

- Status: implementation landed (2026-08-04); Authenticode signing and manual release acceptance remain open
- Current baseline: Ubuntu 22.04/24.04
- Primary release target: supported Windows 11 releases, x86-64
- Compatibility target: Windows 10 22H2 is best-effort and does not block release because its regular support ended on 2025-10-14
- Goal: make Windows a first-class desktop target without weakening Heminus's SSH isolation or regressing Ubuntu

## 1. Scope and Release Decisions

The first Windows release must support:

- local PowerShell and `cmd.exe` terminals through ConPTY;
- SSH, SFTP, and local/remote/dynamic forwarding through Microsoft OpenSSH Client;
- password, unencrypted-key, and encrypted-key authentication, including jump-host chains;
- app-private known hosts, SSH keys, and generated connection configuration;
- Windows Credential Manager for passwords and key passphrases;
- local file browsing and transfer operations inside the Windows user profile;
- a signed, per-user NSIS x86-64 installer;
- automated Rust/frontend checks on Ubuntu and Windows;
- a Windows release build produced on a native Windows CI runner.

The first Windows release deliberately excludes:

- WSL as the default local terminal engine;
- Windows ARM64 and x86 builds;
- Windows ACL editing in the file permission editor;
- browsing arbitrary drive roots, UNC shares, or network-hosted profiles;
- elevated termination of foreign processes;
- MSI, Microsoft Store distribution, and an offline installer;
- automatic application updates;
- Windows remote-host detection/icons, which is independent of running Heminus on Windows;
- cloud/team sync or other product-scope expansion.

Release packaging starts with one installer format: NSIS. MSI can be added later
if enterprise deployment requires it; maintaining two installers is not a first-release gate.

## 2. Current Portability Assessment

### Reusable without architectural change

- The Svelte frontend structure, xterm.js terminal rendering, lazy feature panes, and IPC wrappers.
- Rust domain and storage crates, including SQLite persistence and migrations through `ProjectDirs`.
- Tauri 2 application shell and capability allowlist.
- App-owned host, identity, known-host, jump-chain, snippet, session, and workspace models.
- `portable-pty` as the PTY abstraction, subject to ConPTY runtime validation.
- OpenSSH-compatible key formats and the existing staged SFTP transfer model.

### Confirmed Windows blockers and gaps

- `package.json` and `src-tauri/tauri.conf.json` only build `.deb` bundles.
- `src-tauri/Cargo.toml` enables only the Linux Secret Service backend for `keyring`.
- `terminal.rs` assumes `$SHELL`, `/bin/bash`, `-l`, `HOME`, and bash `PROMPT_COMMAND`.
- Terminal, SFTP, tunnel, and key-management code resolve `ssh`/`ssh-keygen` through bare executable names.
- `ssh_runtime.rs` builds nested `ProxyCommand` values with POSIX shell quoting.
- Askpass uses the Heminus executable itself and writes the secret to stdout. This must be tested in a release GUI-subsystem build, not only a debug console build.
- `DISPLAY=heminus:0` is a Unix compatibility workaround and must not leak into Windows-specific behavior.
- Tunnel diagnostics and foreign-process termination depend on `fuser`, `pkexec`, `/proc`, and `/usr/bin/kill`.
- Local file opening defaults to `xdg-open`, and the current "Open with" UI expects a command name.
- Home resolution reads `HOME`; frontend SFTP code joins local paths with `/` and treats `/` as the only root.
- Backend local-file commands do not enforce the documented user-home boundary consistently, while transfer entry points do. Windows reparse points make this inconsistency security-relevant.
- Unix owner/group/mode data is unavailable on Windows. The UI must rely on an explicit capability, not an assumed mode value.
- `main.rs` has no release Windows GUI-subsystem handling, so an unwanted console window is possible.
- There is no Windows CI, installer build, WebView2 packaging decision, code-signing flow, or Windows troubleshooting documentation.

Linux-only remote-host icons are not a Windows client blocker and should not be
mixed into the critical path for this port.

## 3. Architecture Decisions

### 3.1 Platform boundary

Add a small `src-tauri/src/platform/` module with `cfg(unix)` and `cfg(windows)`
implementations. Keep the public surface narrow and organized by responsibility:

- paths: user-profile resolution, canonical containment, private directory/file hardening;
- shell: trusted local-shell discovery and platform-specific arguments;
- SSH tools: `ssh`/`ssh-keygen` discovery, version/preflight information;
- desktop open: native default-application behavior;
- process: TCP listener inspection, process metadata, and supported termination;
- supervision: child-process grouping and cleanup.

Feature modules should consume this layer instead of directly reading Unix
environment variables, naming system binaries, or accessing platform process APIs.
Do not build one oversized `platform.rs` switchboard.

Expose a serializable platform-capabilities command to the frontend. At minimum it
must report local permission editing, foreign-process termination, available local
shells, and SSH-tool availability. Unsupported actions should be absent or disabled,
not offered and then rejected after the user completes a workflow.

### 3.2 Executable discovery and process safety

On Windows, prefer Microsoft OpenSSH from the system OpenSSH directory and verify
that `ssh.exe` and `ssh-keygen.exe` come from the same installation. A controlled
development/test override may be supported, followed by a documented `PATH`
fallback; the selected paths and versions must be observable without logging secrets.

Never construct a shell command line for SSH, file opening, or process management.
Pass executable paths and arguments through process APIs. Use Windows APIs rather
than invoking PowerShell, `cmd`, `taskkill`, or `netstat` for runtime operations.

Use a Windows Job Object or an equivalent process-tree abstraction with kill-on-close
semantics for Heminus-owned shells and OpenSSH children. Closing a terminal, SFTP
session, tunnel, detached window, or the application must not leave child processes.

### 3.3 SSH runtime and jump hosts

Preserve the current isolation guarantees:

- do not read user or system SSH configuration;
- do not read user or system known-host files;
- do not use default identity files or the system SSH agent;
- pass only the host, identity, and environment selected in Heminus;
- store secrets only in the operating-system credential service;
- never place passwords or passphrases in generated files, arguments, logs, or SQLite.

Replace nested shell-quoted `ProxyCommand` strings with a per-connection, app-owned
OpenSSH config containing synthetic aliases for the target and jump hosts. Invoke
OpenSSH with that file explicitly, duplicate all isolation options in it, harden its
permissions, and remove it after the connection exits. The generated config may
contain host metadata and vault key paths, but no secrets.

The implementation spike must prove that `ProxyJump`/alias resolution supports the
ordered multi-hop and per-hop credential behavior Heminus already exposes. If the
built-in Windows OpenSSH version cannot do this reliably, stop and choose a different
transport strategy before refactoring the production path.

### 3.4 Local filesystem boundary

Keep the documented local-file boundary: the canonical local Windows user profile is
the browsing and transfer root. Do not add arbitrary drive, UNC, or network-profile
browsing in this port; report an actionable unsupported-path error instead.

All local list/create/rename/delete/open/upload/download entry points must call one
backend containment policy. Resolve parents and reparse points, reject escapes, and
protect the profile root from mutation. Path joining, parent calculation, and
breadcrumbs must come from Rust `PathBuf` logic or structured IPC data, never from
frontend slash concatenation.

### 3.5 Packaging

Use separate package scripts while keeping Linux behavior intact:

- `tauri:build:ubuntu`: `tauri build --bundles deb`;
- `tauri:build:windows`: `tauri build --bundles nsis`.

Build the Windows installer on native Windows. Use per-user NSIS installation for the
first release, keep Tauri's online WebView2 bootstrapper behavior, and add code signing
before public distribution. MSI and offline WebView2 packaging remain follow-up work.

## 4. Implementation Phases

Execution order: Phase 0 -> Phase 1 -> Phases 2 and 3 in parallel -> Phase 4 ->
Phases 5 and 6 -> Phase 7.

### Phase 0: Native Windows feasibility spike

- Prepare a clean Windows 11 x86-64 VM and document the exact development prerequisites:
  - Rust stable MSVC satisfying workspace `rust-version = 1.95`;
  - Visual Studio 2022 Build Tools with Desktop development with C++ and a Windows SDK;
  - Node.js 22+, pnpm 10+, and WebView2 Runtime;
  - Microsoft OpenSSH Client optional feature.
- Compile the workspace for `x86_64-pc-windows-msvc` before production refactoring.
- Run a minimal Tauri development build and a release GUI-subsystem build.
- Prove these runtime assumptions with disposable test code or a spike branch:
  - `portable-pty` opens, resizes, interrupts, and closes PowerShell through ConPTY;
  - Windows Credential Manager performs an isolated set/get/delete round trip;
  - the release Heminus executable can act as askpass and return a secret over stdout;
  - Microsoft OpenSSH honors the required isolation options;
  - app-generated aliases support one-hop and multi-hop connections;
  - a key under a path containing spaces and non-ASCII characters is accepted after ACL hardening.
- Record the tested Windows/OpenSSH versions and any minimum version in `docs/windows.md`.

Acceptance gate:

- The workspace compiles natively on Windows.
- ConPTY, Credential Manager, askpass, and a one-hop SSH connection all have successful proof-of-concept results.
- No unresolved spike result invalidates the architecture in Section 3.

### Phase 1: Build, dependency, and platform foundations

- Add Ubuntu and Windows CI jobs from clean lockfiles.
- Run frontend checks on both platforms and Rust tests/clippy on both native targets.
- Split Tauri build scripts by platform and remove the `.deb`-only default from shared configuration.
- Configure target-specific `keyring` features:
  - Linux: `v1` plus `zbus-secret-service-keyring-store`;
  - Windows: `v1` plus `windows-native-keyring-store`.
- Add only the Windows API crate/features required for ACLs, Job Objects, process information, and TCP tables.
- Introduce the platform modules and migrate home, shell, desktop-open, SSH-tool, and process lookup calls.
- Add the platform-capabilities IPC contract and frontend type tests.
- Add unit tests for executable discovery, path normalization, capability reporting, and unsupported behavior.

Acceptance gate:

- `pnpm check`, `pnpm test`, `pnpm build`, `cargo test --workspace`, and clippy pass on Ubuntu and Windows.
- The app starts on Windows even when OpenSSH Client is absent; only SSH-dependent features are unavailable.
- Ubuntu behavior remains unchanged and the `.deb` still builds.

### Phase 2: Local terminal and process lifecycle

- Discover the local shell in a deterministic order: trusted PowerShell 7 installation,
  system Windows PowerShell, then `%ComSpec%`/`cmd.exe`.
- Pass shell-specific arguments only. PowerShell should load the user's normal profile;
  Windows shells must never receive Unix `-l` behavior.
- Start the shell in the resolved user profile and preserve host-provided environment variables.
- Keep bash OSC completion markers on Unix. For the first Windows release, record the
  submitted command but leave local-shell exit status unknown unless a PowerShell marker
  implementation can preserve existing prompt functions and passes focused tests.
- Validate ConPTY startup, resize, Ctrl+C, copy/paste, multiline-paste guard, Unicode/IME
  input, large output, shell exit, detached-window transfer, and app shutdown.
- Supervise the complete local process tree and test forced application exit for orphans.

Acceptance gate:

- PowerShell and fallback `cmd.exe` sessions open and close reliably.
- Paths with spaces and non-ASCII user-profile names work.
- Closing a tab, detached window, or Heminus leaves no owned shell process running.
- Existing Ubuntu local terminal behavior and shell completion history still pass.

### Phase 3: Credential store, key tools, and ACLs

- Enable and initialize the Windows native keyring backend.
- Return actionable errors when enterprise policy or the current account blocks credential storage.
- Resolve `ssh-keygen.exe` through the platform layer for generation, validation, and public-key derivation.
- Harden the SSH vault and every private/generated connection file with Windows ACLs.
- Validate ACL ownership and access with the same `ssh.exe` that Heminus will launch.
- Test Ed25519 generation, supported private-key imports, encrypted keys, wrong passphrases,
  secret deletion, duplicate labels, paths with spaces, and non-ASCII paths.
- Add a Windows credential round-trip integration test that always removes its temporary entry.

Acceptance gate:

- Passwords and passphrases survive app restart and can be deleted from Credential Manager.
- Imported/generated private keys work with Microsoft OpenSSH and are rejected if ACL hardening is intentionally removed in the test fixture.
- No secret value appears in SQLite, logs, process arguments, generated config, or renderer persistence.

### Phase 4: SSH, SFTP, askpass, and jump-host chains

- Add feature-level OpenSSH preflight with the selected executable paths and versions.
- Validate `-F` isolation, private known-host files, strict host-key checking, hashing,
  disabled default identities, and disabled agent behavior on Windows.
- Replace `ProxyCommand` generation with app-owned alias/config generation and cleanup.
- Keep `DISPLAY` handling Unix-only and validate `SSH_ASKPASS_REQUIRE=force` on Windows.
- Test askpass for target password, target key passphrase, jump-host password, jump-host
  key passphrase, ambiguous prompts, cancellation, and credential lookup failure.
- Use the resolved `ssh.exe` for interactive terminal, SFTP subsystem, and tunnel flows.
- Validate SFTP stream startup and child cleanup on authentication, protocol, and network failures.
- Avoid depending exclusively on English OpenSSH stderr; test at least one non-English Windows locale.

Acceptance gate:

- Password, unencrypted-key, and encrypted-key SSH login work.
- One-hop and multi-hop chains work with different credentials per hop.
- First-use and changed-host-key behavior matches the existing Ubuntu behavior.
- SFTP browse and basic file operations work through direct and jump-host connections.
- A test user SSH config, known-host file, default key, and running agent cannot influence Heminus connections.
- No generated config or `ssh.exe` process remains after connection teardown.

### Phase 5: Local filesystem and SFTP UI

- Replace frontend local-path construction and `/` root checks with backend-provided
  current path, parent path, and breadcrumb data.
- Apply canonical profile containment to every local IPC operation and every transfer source/destination.
- Test Windows drive paths, rejected UNC/network-profile roots, reparse points/junctions, symlinks,
  case-only rename, reserved names, locked files, read-only files, and long paths.
- Use native default-application opening. Hide the command-name "Open with" flow on
  Windows until a proper native application picker is implemented.
- Use capabilities to hide local Unix permission editing while keeping remote SFTP mode editing.
- Verify staged download publication and rollback under Windows rename, antivirus lock,
  cancellation, and destination-already-exists behavior.

Acceptance gate:

- Local browsing starts at the user profile and cannot navigate or mutate outside it.
- Creating, renaming, deleting, opening, uploading, and downloading works for supported Windows paths.
- Junction/reparse-point escape tests fail closed.
- Cancelled or failed downloads do not publish partial destinations or lose the prior file.
- Local permission editing is absent; remote Unix permission editing remains available.

### Phase 6: Tunnels and port-conflict diagnostics

- Launch all forwarding processes through the resolved `ssh.exe` and shared supervisor.
- Preserve start/stop/state behavior for local, remote, and dynamic forwarding.
- Replace `fuser`/`/proc` with Windows TCP table and process APIs for IPv4 and IPv6 listeners.
- Return PID and process name when access permits; treat command-line details as optional.
- Stop Heminus-owned tunnel trees reliably.
- Do not offer elevated foreign-process termination. Same-user foreign termination may be
  enabled only if ownership checks and native API tests are reliable; otherwise report the conflict without a Stop action.

Acceptance gate:

- Local, remote, and dynamic tunnels start and stop on Windows.
- Occupied-port diagnostics identify an accessible listener without invoking a shell command.
- App exit and tunnel stop leave no owned `ssh.exe` or proxy child process.
- Inaccessible/elevated listeners produce a clear diagnostic without an elevation prompt.

### Phase 7: Desktop UX, installer, and release hardening

- Build the release executable as a GUI application and confirm no console window appears;
  re-run askpass tests in that exact build mode.
- Validate the custom title bar: drag, edge resize, maximize/restore, Windows snap behavior,
  multi-monitor transitions, keyboard access, and 100/125/150/200 percent DPI.
- If the frameless window cannot provide reliable snap, resize, or accessibility behavior,
  use native decorations on Windows while retaining the current Ubuntu frame.
- Validate terminal Ctrl+C versus copy behavior, context menus, focus traversal, and high-contrast mode.
- Configure per-user x86-64 NSIS and the online WebView2 bootstrapper.
- Add Authenticode signing for the application and installer; verify signatures in CI/release checks.
- Build release artifacts on native Windows and retain checksums and build metadata.
- Test install, launch, repair/reinstall, version upgrade, uninstall, and documented data/credential retention behavior.
- Update `README.md`, `docs/security.md`, `docs/parity-matrix.md`, and `docs/windows.md`.

Acceptance gate:

- A signed NSIS installer installs without elevation, launches without a console window,
  upgrades over the prior version, and uninstalls cleanly.
- WebView2 missing/outdated behavior follows the documented installer path.
- User data survives upgrade, and uninstall retention behavior is explicit.
- The UI remains usable at common DPI settings and with keyboard-only navigation.
- Ubuntu packaging, title bar behavior, and documentation remain intact.

## 5. Verification Matrix

### Automated on Ubuntu and Windows

- `pnpm install --frozen-lockfile`
- `pnpm check`
- `pnpm test`
- `pnpm build`
- `cargo fmt --all -- --check`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`

### Platform packaging

- Ubuntu: `pnpm run tauri:build:ubuntu`
- Windows: `pnpm run tauri:build:windows`
- Windows release: verify Authenticode signature and publish a SHA-256 checksum.

### Manual Windows release acceptance

1. Install the signed NSIS build on a clean, supported Windows 11 x86-64 VM.
2. Launch it with OpenSSH Client installed, then launch without OpenSSH and verify local-only features still work.
3. Verify no console window appears in the release build.
4. Open local PowerShell and fallback `cmd.exe` terminals; resize, interrupt, paste, detach, reattach, and close them.
5. Repeat terminal and file tests from a Windows account whose profile path contains spaces and non-ASCII characters.
6. Connect with password, unencrypted private key, and encrypted private key authentication.
7. Connect through one-hop and multi-hop chains using different credential types per hop.
8. Verify first-use and changed-host-key behavior.
9. Prove Heminus ignores user/system SSH config, known hosts, default keys, and the agent.
10. Browse SFTP through direct and jump-host connections.
11. Upload/download files and folders containing spaces, non-ASCII names, empty files, and large files.
12. Cancel and fail transfers; verify partial-file cleanup and replacement rollback.
13. Browse local files from the profile root and verify drive/root/junction escapes are blocked.
14. Create, case-rename, delete, and open local files/folders; repeat with a locked/read-only destination.
15. Confirm local permissions are hidden and remote Unix permissions remain editable.
16. Start and stop local, remote, and dynamic forwarding rules over IPv4 and IPv6.
17. Start a tunnel on an occupied port and verify diagnostics for same-user and elevated listeners.
18. Close Heminus during active terminal, SFTP, and tunnel work; verify no owned child processes remain.
19. Save snippets/workspaces, restart, and restore state.
20. Exercise custom window controls, snap behavior, multiple monitors, keyboard navigation, high contrast, and all target DPI values.
21. Upgrade the installed build and verify database, vault, known hosts, and credentials remain usable.
22. Uninstall and verify behavior matches the documented data/credential retention policy.
23. Run a non-blocking smoke pass on Windows 10 22H2 if the project still chooses to ship it as best-effort compatibility.

## 6. Risk Register

| Risk | Impact | Mitigation / decision gate |
|---|---|---|
| Release GUI executable cannot return askpass output as expected | Password/passphrase authentication fails | Prove this in Phase 0 and re-test the signed release build; split out a tiny helper executable if needed |
| Built-in OpenSSH versions differ across Windows installations | Isolation options or jump chains fail | Record/version-check the executable and define a tested minimum after the spike |
| Generated jump-host config accidentally reads system defaults | Security model regresses | Generate complete aliases, invoke only with explicit `-F`, and test against hostile user/system config fixtures |
| Windows ACLs are too broad or too narrow | Keys are exposed or rejected by OpenSSH | Use native ACL handling and validate with the selected `ssh.exe` under the current user |
| ConPTY input/resize/exit differs from Unix PTY | Terminal hangs or loses input | Focused Windows integration tests plus release-build manual checks |
| Descendant processes survive parent termination | Orphaned shells, proxies, or tunnels | Windows Job Object/process-tree supervision with shutdown tests |
| Junctions/reparse points escape the profile root | Local file policy bypass | One canonical backend containment policy with adversarial path tests |
| Windows rename/locking semantics break transfer publication | Existing destination is lost or partial data is exposed | Staging/backup rollback tests with locks, antivirus interference, cancellation, and replacement |
| Localized OpenSSH output breaks error classification | Misleading or missing diagnostics | Prefer exit/state signals and stable patterns; test a non-English Windows locale |
| OpenSSH optional feature cannot be installed in managed environments | SSH features unavailable | Keep local terminal usable and provide actionable admin/offline installation guidance |
| Unsigned builds trigger SmartScreen | Users cannot trust or easily launch releases | Authenticode-sign public releases and verify signatures before publishing |
| Frameless window conflicts with Windows snap/DPI/accessibility | Poor desktop behavior | Make native Windows decorations the explicit fallback decision |

## 7. Release Gate

Windows support is ready when:

- all automated checks pass on native Ubuntu and Windows CI;
- a signed x86-64 NSIS installer is reproducibly built on native Windows;
- the supported Windows 11 manual checklist passes, including a non-English/profile-path run;
- local terminal, SSH, SFTP, forwarding, key-vault, credential, and jump-host flows pass;
- Heminus preserves isolation from system SSH config, known hosts, default keys, and agent;
- secrets never reach SQLite, logs, process arguments, generated files, renderer persistence, or terminal history;
- all owned shell/OpenSSH process trees are cleaned up on normal close and forced app exit;
- local filesystem IPC cannot escape the canonical user profile through path syntax or reparse points;
- incomplete SFTP transfers never publish corrupted destinations or lose the previous destination;
- Ubuntu `.deb` packaging and existing Ubuntu acceptance behavior remain green;
- Windows limitations and dependency/setup errors are documented plainly;
- MSI, ARM64, automatic updates, offline install, and Windows 10 compatibility are not treated as blockers for the first Windows 11 release.

## 8. Reference Material

- [Microsoft: Windows 10 Home and Pro lifecycle](https://learn.microsoft.com/en-us/lifecycle/products/windows-10-home-and-pro)
- [Microsoft: OpenSSH for Windows overview](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh-overview)
- [Microsoft: OpenSSH optional-feature installation constraints](https://learn.microsoft.com/en-us/troubleshoot/windows-server/system-management-components/cant-install-openssh-features)
- [Tauri: Windows installer and WebView2 options](https://v2.tauri.app/distribute/windows-installer/)
- [Tauri: Windows code signing](https://v2.tauri.app/distribute/sign/windows/)
- [`keyring` 4.1.5 feature flags](https://docs.rs/crate/keyring/4.1.5/features)
