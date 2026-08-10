# Heminus

Heminus is a lightweight, local-first SSH, SFTP, terminal, and port-forwarding
workspace for Ubuntu and Windows 11. It uses Tauri 2, Svelte 5, Rust, the
platform webview, and the system OpenSSH client.

The interface and all user-facing messages are in English.

## Current features

- Saved hosts with search, nested/movable groups, tags, and reusable identities
- Multi-select host operations, drag-to-group organization, duplication, and tag filters
- Structured host profiles with environment variables, ordered app-owned jump-host chains,
  and per-host terminal themes
- Multiple persistent local or SSH terminal panes, closable/reorderable tabs,
  drag-to-split nested layouts, guarded broadcast input, and workspace restoration
- Native tab dragging between Heminus windows, detachable terminal workspaces,
  and standalone local-terminal windows
- Terminal search, clickable links, command suggestions from snippets/history,
  configurable suggestion thresholds and shortcuts, and confirmation before multiline paste
- Interactive OpenSSH authentication, host-key verification, key passphrases,
  and app-isolated connection settings
- A private SSH runtime that never reads system/user SSH config, known hosts,
  default identity files, or the system SSH agent
- Native OS-keyring storage for passwords and key passphrases
- Ed25519 key generation and OpenSSH/PEM private-key import into the Heminus
  vault by drag and drop
- Dual-pane SFTP with remote browsing, directory operations, rename, delete,
  cancellable streaming transfers, progress reporting, and host switching
- Local, remote, and dynamic/SOCKS forwarding with live state and occupied-port handling
- Snippets with local persistence and inline terminal completion
- SHA-256 fingerprints from Heminus's private known-host vault
- Session history metadata without terminal-output recording
- Strict local Content Security Policy and a minimal Tauri capability set

Cloud sync, team vaults, Mosh, Telnet, Serial, FIDO2, and mobile clients are not
part of this local desktop build.

## Install

Download the current installers from the
[latest GitHub release](https://github.com/thaihoc1310/heminus/releases/latest).

On Ubuntu x86_64:

```bash
sudo apt install ./Heminus_0.1.2_amd64.deb
```

The package depends on Ubuntu's `libwebkit2gtk-4.1-0`, `libgtk-3-0`, and
`openssh-client`. Installing the local package with `apt` resolves these
dependencies automatically. The system SSH executable is used only as a
transport engine. Heminus passes an isolated configuration on every invocation
and does not consume the machine's SSH configuration, known hosts, default
keys, or agent.

On Windows 11 x86-64, run `Heminus_0.1.2_x64-setup.exe`. Microsoft OpenSSH
Client and WebView2 Runtime are required for the complete feature set. The
current installer is unsigned, so Windows may show a SmartScreen warning.
See [Windows development and release notes](docs/windows.md) for setup,
limitations, and release requirements.

## Development

Requirements:

- Rust 1.95.0 with the native GNU/Linux or MSVC toolchain
- Node.js 22+
- pnpm 10.33+
- Tauri's platform development dependencies

Commands:

```bash
pnpm install --frozen-lockfile
pnpm check
pnpm test
cargo test --workspace --all-targets
pnpm tauri dev
pnpm run tauri:build:ubuntu # Ubuntu
pnpm run tauri:build:windows # Windows
```

## Manual acceptance checklist

1. Create and edit a host, restart Heminus, and confirm it persists.
2. Open two local terminal tabs. Run `htop`, `vim`, and `tmux`, switch between
   tabs, then close each tab.
3. Connect to an SSH host and verify first-use and changed-host-key behavior.
4. Generate an encrypted Ed25519 key, drag and drop a PEM private key, store a
   passphrase, assign each identity, and connect.
5. Connect SFTP, browse both panes, upload and download a large file, confirm
   both replace prompts, create a folder, rename it, and delete it.
6. Start and stop one local, remote, and dynamic forwarding rule.
7. Save a snippet, type part of its command in a terminal, select it from the
   popup, restart the app, and confirm both the snippet and matching command
   history remain available.
8. Check Known Hosts and Logs after the SSH session.
9. Start a tunnel with an occupied local port and confirm the conflict dialog
   identifies the process and offers the supported recovery action.
10. Repeat the terminal and SFTP checks on both Wayland and X11 if available.
11. Create nested groups, move a group, rename a parent, restart, and confirm
    the paths and host assignments persist.
12. Open multiple local and SSH panes, enable split mode, confirm the broadcast
    warning, reorder tabs, drag tabs onto all four pane edges, save the
    workspace, restart, and restore it.
13. Drag a top-level tab and an inner workspace pane between windows, then drag
    each outside every Heminus window and confirm a detached window is created.

Passwords and key passphrases are stored only through the operating system's
encrypted credential service and are never written to Heminus's SQLite
database. Private-key identities point only to copies inside the private
Heminus data directory. Dragged source files are left untouched; vault copies
are validated and restricted to mode `0600` on Linux or a private ACL on
Windows. Generated keys also live in that vault.
