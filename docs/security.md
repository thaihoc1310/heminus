# Security notes

- The renderer cannot open arbitrary sockets or invoke a shell plugin.
- Native operations are explicit Tauri commands; window permissions are
  allowlisted in `src-tauri/capabilities/default.json`.
- Production uses a local-only CSP and disables devtools and public source maps.
- Tauri's optional prototype freezing is left at its default because xterm.js
  and WebKitGTK require mutable library-owned prototypes during initialization.
  Native exposure remains constrained by the CSP and capability allowlist.
- SSH command arguments are passed as separate process arguments, never by
  interpolating a shell command.
- OpenSSH performs host-key verification against Heminus's private
  `known_hosts` file. User and global OpenSSH known-host files are explicitly
  disabled.
- SSH passwords and key passphrases are stored through the OS-native credential
  service. SQLite contains only identity metadata and a credential-presence bit.
- OpenSSH receives secrets through a short-lived askpass bridge.
  The process environment contains only candidate identity UUIDs and prompt
  match metadata, never a secret.
- Direct SSH processes use `-F none`. Jump-host processes use a short-lived,
  app-owned `-F` config with synthetic aliases. Both paths disable the system
  agent and default identities and receive only data selected in Heminus.
- Key-file identities store a path inside Heminus's private SSH vault. Dragged
  source files are validated, copied into the vault, and left unchanged; vault
  copies are restricted to mode `0600` on Unix or a private ACL on Windows.
- Local file browsing and transfer destinations are restricted to the user's
  home directory.
- Uploads and downloads use randomized hidden partial files and publish the
  requested destination only after a successful transfer.
- Existing remote files are temporarily backed up during replacement and
  restored if publishing the new upload fails.
- Multiline terminal paste requires explicit confirmation.
- Session history stores connection metadata, and command completion history
  stores submitted command lines per host. Terminal output is not logged, and
  input entered at detected password/passphrase/token prompts is never added to
  command history.
- Tunnel processes and SFTP transports are terminated when their manager is
  dropped. Windows terminal, SFTP, tunnel, and ProxyJump descendants are also
  contained in per-session Job Objects with kill-on-close behavior.

This is an implementation review, not an independent security audit.
