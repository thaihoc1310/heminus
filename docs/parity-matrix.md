# Desktop parity matrix

| Area | Status |
|---|---|
| Local terminal | Implemented |
| Multiple terminal tabs | Implemented |
| Search, links, copy/paste, multiline-paste guard | Implemented |
| SSH password/key/passphrase flows | Implemented through OpenSSH and native OS keyring |
| SSH key-file identities | Implemented with app-private key storage; the system agent and default keys are disabled |
| Key generation and PEM import | Ed25519 generation plus drag-and-drop OpenSSH/RSA/DSA/EC/PKCS#8 PEM import into the private vault |
| Strict known-host verification | Implemented through an app-private known-host file |
| Local OpenSSH config integration | Intentionally unsupported; Heminus never reads it |
| Proxy/jump chains | Implemented as an ordered app-owned multi-hop chain with per-hop saved password or vault-key authentication |
| SFTP browse/upload/download/rename/delete/mkdir | Implemented with partial-file publication |
| Local/remote/dynamic forwarding | Implemented with SSH failure diagnostics |
| Host search/groups/tags | Implemented with nested, movable groups |
| Host connection profiles | Address/general/credentials, ordered jump hosts, environment variables, and terminal theme |
| Snippets | Implemented with inline terminal completion |
| Session and command history | Connection metadata plus per-host command completion history; terminal output is not recorded |
| Split terminal workspaces and broadcast input | Closable/reorderable tabs, nested drag-to-split layout, restore, and safety confirmations |
| Encrypted credential storage | Implemented through the OS-native credential service; SQLite never stores secrets |
| Mosh/Telnet/Serial/FIDO2 UI | Planned |
| Cloud/team sync and multiplayer | Out of scope for the local desktop release |
