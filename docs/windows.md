# Windows development and release notes

Heminus targets supported Windows 11 x86-64 releases. Windows 10 22H2 is a
best-effort compatibility target and is not a release gate.

## Requirements

- Rust 1.95.0 with the `x86_64-pc-windows-msvc` toolchain
- Visual Studio 2022 Build Tools with Desktop development with C++ and a Windows SDK
- Node.js 22 or newer and pnpm 10.33
- Microsoft Edge WebView2 Runtime
- Microsoft OpenSSH Client (`ssh.exe` and `ssh-keygen.exe`)

Install OpenSSH Client from Settings > System > Optional features. Heminus
prefers `%SystemRoot%\System32\OpenSSH` and falls back to `PATH`. Local terminal
and local data features remain available when OpenSSH is missing; SSH, SFTP,
key validation, and forwarding report an installation error.

## Commands

```powershell
pnpm install --frozen-lockfile
pnpm check
pnpm test
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
pnpm run tauri:build:windows
```

The installer is written to `target\release\bundle\nsis`. It is a per-user
x86-64 installer and downloads the WebView2 bootstrapper when required.

The local implementation verification baseline was Windows 11 x86-64 build
26200, Rust 1.95.0 MSVC, Visual Studio Build Tools 2022 17.14, and Microsoft
OpenSSH for Windows 9.5p2. CI repeats the automated suite on `windows-2025`.

## Windows behavior

- Local terminals use PowerShell 7, Windows PowerShell, then `%ComSpec%`.
- SSH connections use Microsoft OpenSSH with app-owned known hosts, keys, and
  generated jump-host configuration. User SSH config, default keys, and the
  system agent are disabled.
- Passwords and key passphrases use Windows Credential Manager.
- Local file access is restricted to the canonical user profile. Drive roots,
  UNC paths, network profiles, and reparse-point escapes are rejected.
- Default application opening is supported. Custom command-based "Open with"
  and local Unix permission editing are hidden.
- Occupied tunnel ports show the owning process when Windows exposes it.
  Heminus does not terminate another application's process on Windows.
- Terminal, SFTP, tunnel, and ProxyJump descendants are assigned to per-session
  Windows Job Objects and are terminated when the owning session closes.

## Release requirements

CI creates an unsigned NSIS artifact for validation. Public installers require
an Authenticode certificate and must be signed before publication. Verify the
signature and generate a SHA-256 checksum from the final signed file. Signing
credentials and certificate material must not be committed to this repository.

Release QA must cover a clean Windows 11 VM, a profile path containing spaces
and non-ASCII characters, 100/125/150/200 percent DPI, multi-monitor window
movement, OpenSSH absent/present states, one-hop and multi-hop authentication,
transfer cancellation, occupied ports, upgrade, and uninstall behavior.

## Troubleshooting

- `Microsoft OpenSSH Client is not installed`: install the Optional Feature,
  then restart Heminus.
- `Bad permissions` for a private key: re-import or regenerate the key so the
  vault copy receives a private Windows ACL.
- A port is already in use: close the process shown by Heminus in Task Manager,
  then start the tunnel again.
- WebView2 launch or install failures: update the Microsoft Edge WebView2
  Runtime and rerun the installer.
