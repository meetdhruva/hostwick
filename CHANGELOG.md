# Changelog

## Unreleased

**Networking**
- Outbound proxy for the first hop: SOCKS5 (with user/password, host names resolved by the proxy), SOCKS4/4a and HTTP `CONNECT` (Basic auth). Password from the vault or a prompt; clear errors such as `proxy 10.0.0.1:1080 refused: connection not allowed by ruleset`.
- X11 forwarding (`hostwick ssh -X`), OpenSSH-style untrusted: fake cookie on the server, real one (from `xauth`) only locally; `$DISPLAY`, `unix:N`, `host:N`, macOS launchd sockets, and `localhost:0` (VcXsrv/Xming) on Windows. A missing X server is reported once and the session carries on.
- Agent forwarding now serves the forwarded channels (Unix `SSH_AUTH_SOCK`; Windows Pageant, then the OpenSSH agent pipe).
- OpenSSH certificates: `id_ed25519-cert.pub` next to a key is used automatically (falling back to the plain key), certificates held by the agent are offered as certificates; RSA certificates use `rsa-sha2-*-cert`.
- Kerberos / GSSAPI (`-K`, or the Kerberos auth method): the system GSS-API library on Linux/macOS (loaded at run time), SSPI on Windows; optional ticket delegation. No ticket or library: skipped with the reason (e.g. "no Kerberos ticket — run kinit").
- FIDO security keys (`sk-ssh-ed25519`, `sk-ecdsa`) work through the agent, with a "touch your security key" prompt; security-key files get a clear "add it to ssh-agent / Pageant" message.
- CLI: `--proxy URL`, `-X`, `-K`; `hosts add` also takes `--proxy-credential`, `-A`, `-X`.

**Scripting and automation**
- `hostwick script FILE|-`: WinSCP-style batch scripts — `open` (saved host, `user@host:port` or `sftp://` URL; `-hostkey=SHA256:…` pinning, `-privatekey`, `-password`), `cd`/`lcd`/`pwd`/`lpwd`/`ls`/`lls`/`stat`, `put`/`get` with `-r`, `-n` and wildcards, `mkdir`, `rm -r`, `rmdir`, `mv`, `chmod`, `ln -s`, `sync local|remote --delete -x`, `call`, `echo`, `option batch abort|continue`, `option confirm`, `exit`. `%VAR%` from `-D` and the environment, `%1%` parameters, quoting; errors name the script line and set exit code 1. Example in `docs/examples/deploy-script.txt`.
- `--json` for `hosts list`, `known-hosts list`, `ls`, `keyinfo`, script listings (`option json on`) and a JSON summary for `sync`.

**Keys**
- New `hostwick-keys` crate and `hostwick keygen` / `keyconv` / `keyinfo`: Ed25519, ECDSA P-256/384/521 and RSA keys; OpenSSH (bcrypt-pbkdf + aes256-ctr) and PuTTY `.ppk` v3 (Argon2id + aes256-cbc + HMAC-SHA-256) or v2; convert OpenSSH ↔ PPK (and legacy PEM), change or remove passphrases, SHA256 fingerprints and randomart. Verified against `puttygen` and `ssh-keygen`.

**known_hosts**
- `hostwick known-hosts list|remove|add`: list entries (hashed, `@cert-authority`, `@revoked`, fingerprints), forget a host like `ssh-keygen -R` (hashed and `[host]:port` entries, only that host on multi-host lines), remove by line, add keys. Rewrites are atomic and keep other lines byte for byte.

**Settings sync**
- Saved hosts, deletions and app settings sync through a folder kept in step by OneDrive, Dropbox, iCloud Drive, Syncthing or git (`hostwick settings-sync sync|push|pull`). One encrypted file (Argon2id + XChaCha20-Poly1305), newest edit wins per host, deletions propagate, cloud "conflicted copies" are merged and cleaned up. Vault secrets are never synced.
- The profile database moves to schema v2 (deletion records); a backup of the old database is kept next to it.

## 0.2.0

**Keys and import**
- PuTTY `.ppk` private keys (v2 and v3, plain or passphrase-protected) are read directly.
- Failed logins list every method tried and why (e.g. `key work.ppk: wrong passphrase; password: rejected`).
- `~/.ssh/config` import follows `Include` (wildcards, loops ignored).
- PuTTY import keeps proxy-command jump hosts, terminal type and proxy users; unsupported proxies are reported per session.
- Solar-PuTTY JSON import in the desktop app; on Windows, PuTTY `.reg` and `WinSCP.ini` files can be imported as well as the registry.
- Import report dialog: added, updated and anything that needs attention, instead of a bare count. Re-importing keeps your local notes and session-log settings.

**Desktop app**
- Right-click menus on hosts, tabs, terminals and files.
- Full host editor (new and edit): key files with Browse…, jump hosts, port forwards, keepalive, terminal type, session log, notes, legacy algorithms. Duplicate host.
- Settings: dark/light theme, terminal font and size, scrollback, right-click behaviour (menu or PuTTY paste), copy-on-select, session-log defaults, app log level.
- Session logging (PuTTY-style `&S/&H/&Y&M&D/&T` file names, printable or raw), per host, for all sessions, or on demand; ● LOG badge.
- App log file, daily rotation, level changeable at runtime; Open log folder.
- File browser: rename, copy path, upload here, local-time timestamps, `rwxr-xr-x` permissions.
- Reconnect from the tab menu, the ended-session banner, `Ctrl+Shift+R`, or Enter.
- Keyboard shortcuts for new host, duplicate/close/cycle tabs, broadcast and settings.
- App icon on Windows (embedded in the .exe), macOS (.app bundle) and Linux (`install.sh` adds a menu entry with icon).

## 0.1.0

First release: SSH terminal, SFTP, tunnels, jump hosts, transfer engine, directory sync (`hostwick sync`, Keep up to date), vault, importers, desktop app and CLI for Windows, macOS and Linux.
