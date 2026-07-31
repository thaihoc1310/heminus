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
