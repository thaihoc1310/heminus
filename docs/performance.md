# Performance snapshot

Measured on 2026-07-28 on Ubuntu 24.04 x86_64. These numbers describe this
machine and display configuration; they are not universal benchmarks.

| Metric | Result |
|---|---:|
| Installed release binary | 10.3 MB |
| Debian package | 4.4 MB |
| Initial JavaScript | 33.1 KB gzip |
| Initial CSS | 6.1 KB gzip |
| Lazy xterm.js chunk | 97.7 KB gzip |
| Idle process count | 3 |
| Previous-build idle total PSS, 2560×1382 window | about 299 MB |
| Reference Termius total PSS on the same machine | about 647 MB |

The package and frontend sizes were re-measured after native keyring, key
generation, nested group, and split-workspace support. The PSS figure is the
last non-visual performance run and should be rechecked during manual acceptance.

The Vault route does not load xterm.js. Terminal, SFTP, Keychain, Forwarding,
Snippets, Known Hosts, and Logs are separate lazy chunks. SFTP transfers use a
64 KiB buffer and report progress at roughly 1 MiB intervals. Terminal input is
coalesced for 4 ms and writes are serialized.

The original 150 MB idle-PSS target is not met by WebKitGTK on this high-DPI
desktop. Heminus is still roughly half the measured PSS of the Electron-based
reference. Disabling WebKit compositing reduced PSS but was not enabled because
it can hurt terminal rendering latency and smoothness.

## Local terminal check, 2026-09-23

On this Ubuntu machine, a fresh local-terminal window used about 248 MiB PSS
across Heminus and its two WebKit processes. A fresh GNOME Terminal window used
about 31 MiB PSS across its launcher and server. Shells and child programs were
excluded from both totals. The fixed WebKit cost dominates the difference.

After 20 MiB of terminal output in the WebKit smoke harness, reducing xterm
scrollback from 20,000 to 5,000 lines reduced WebKitWebProcess PSS from about
203 to 173 MiB. This harness runs from Vite under Xvfb; it measures the effect
of the scrollback cap, not release-build idle memory. The backend also discards
replay bytes after the renderer acknowledges them, so active sessions no longer
retain up to 2 MiB of already rendered output each.
