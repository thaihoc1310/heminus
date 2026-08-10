# Desktop parity matrix

| Area | Ubuntu | Windows 11 x86-64 |
|---|---|---|
| Local terminal | Login shell | PowerShell 7, Windows PowerShell, then `cmd.exe` through ConPTY |
| Multiple terminal tabs | Implemented | Implemented |
| Search, links, copy/paste, multiline-paste guard | Implemented | Implemented |
| SSH password/key/passphrase flows | OpenSSH plus Secret Service | Microsoft OpenSSH plus Windows Credential Manager |
| SSH key-file identities | App-private vault; agent/default keys disabled | App-private vault with private ACL; agent/default keys disabled |
| Key generation and PEM import | Implemented | Implemented with Microsoft `ssh-keygen.exe` validation |
| Strict known-host verification | App-private known hosts | App-private known hosts |
| Local OpenSSH config integration | Intentionally unsupported | Intentionally unsupported |
| Proxy/jump chains | App-owned `ProxyJump` config | App-owned `ProxyJump` config |
| SFTP browse/upload/download/rename/delete/mkdir | Implemented | Implemented |
| Local file browsing | Canonical home boundary | Canonical user-profile boundary; drive/UNC roots unsupported |
| Local permission editing | Implemented | Hidden; Windows ACL editing is not exposed |
| Default application open | Implemented | Implemented |
| Custom command-based open | Implemented | Hidden pending a native application picker |
| Local/remote/dynamic forwarding | Implemented | Implemented |
| Occupied-port diagnostics | PID/process plus supported termination flow | PID/process; terminate externally in Task Manager |
| Owned process-tree cleanup | Manager cleanup | Per-session Windows Job Objects |
| Host search/groups/tags | Implemented | Implemented |
| Host connection profiles | Implemented | Implemented |
| Snippets | Implemented | Implemented |
| Session and command history | Implemented; terminal output is not recorded | Implemented; terminal output is not recorded |
| Split workspaces and broadcast input | Implemented | Implemented |
| Installer | Ubuntu `.deb` | Per-user x86-64 NSIS; public artifacts require Authenticode signing |
| Mosh/Telnet/Serial/FIDO2 | Planned | Planned |
| Cloud/team sync | Out of scope | Out of scope |
