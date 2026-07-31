# Heminus

Heminus is a lightweight, local-first SSH, SFTP, terminal, and port-forwarding
workspace for Ubuntu. It uses Tauri 2, Svelte 5, Rust, the system WebKitGTK
runtime, and the system OpenSSH client.

The interface and all user-facing messages are in English.

## Current features

- Saved hosts with search, nested/movable groups, tags, and reusable identities
- Structured host profiles with environment variables, ordered app-owned jump-host chains,
  and per-host terminal themes
- Multiple persistent local or SSH terminal panes, closable/reorderable tabs,
  drag-to-split nested layouts, guarded broadcast input, and workspace restoration
- Terminal search, clickable links, command suggestions from snippets/history,
  and confirmation before multiline paste
- Interactive OpenSSH authentication, host-key verification, key passphrases,
  and app-isolated connection settings
- A private SSH runtime that never reads system/user SSH config, known hosts,
  default identity files, or the system SSH agent
- Native OS-keyring storage for passwords and key passphrases
- Ed25519 key generation and OpenSSH/PEM private-key import into the Heminus
  vault by drag and drop
- Dual-pane SFTP with remote browsing, directory operations, rename, delete,
  streaming upload/download, and progress reporting
- Local, remote, and dynamic/SOCKS forwarding
- Snippets with local persistence and inline terminal completion
- SHA-256 fingerprints from Heminus's private known-host vault
- Session history metadata without terminal-output recording
- Strict local Content Security Policy and a minimal Tauri capability set

Cloud sync, team vaults, Mosh, Telnet, Serial, FIDO2, and mobile clients are not
part of this local desktop build.

## Install

On Ubuntu x86_64:

```bash
sudo apt install ./target/release/bundle/deb/Heminus_0.1.0_amd64.deb
```

The package depends on Ubuntu's `libwebkit2gtk-4.1-0`, `libgtk-3-0`, and
`openssh-client`. Installing the local package with `apt` resolves these
dependencies automatically. The system SSH executable is used only as a
transport engine. Heminus passes an isolated configuration on every invocation
and does not consume the machine's SSH configuration, known hosts, default
keys, or agent.

## Development

Requirements:

- Rust stable
- Node.js 22+
- pnpm 10+
- Tauri's Ubuntu system dependencies

Commands:

```bash
pnpm install
pnpm check
pnpm test
cargo test --workspace
pnpm tauri dev
pnpm tauri build --bundles deb
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
9. Start a tunnel with an occupied local port and confirm the SSH error is
   shown in the inspector.
10. Repeat the terminal and SFTP checks on both Wayland and X11 if available.
11. Create nested groups, move a group, rename a parent, restart, and confirm
    the paths and host assignments persist.
12. Open multiple local and SSH panes, enable split mode, confirm the broadcast
    warning, reorder tabs, drag tabs onto all four pane edges, save the
    workspace, restart, and restore it.

Passwords and key passphrases are stored only through the operating system's
encrypted credential service and are never written to Heminus's SQLite
database. Private-key identities point only to copies inside the private
Heminus data directory. Dragged source files are left untouched; vault copies
are validated and restricted to mode `0600` on Linux. Generated keys also live
in that vault.
