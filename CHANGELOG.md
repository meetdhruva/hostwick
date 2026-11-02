# Changelog

## Unreleased

**Desktop app**
- Saved passwords: password, key-passphrase, sudo and proxy prompts offer *Remember*; the first save creates a vault (master password with a strength hint), later sessions unlock it once and log in automatically. Uses the CLI's vault file; secrets are stored only after the server accepted them; key passphrases are saved per key file. A saved password that stopped working is asked for again with *Update the saved password* — no loops. Settings → Saved passwords: lock now, auto-lock after idle minutes, change the master password, remove entries.
- Auto-reconnect when an SSH or telnet connection drops (never after `exit`, a login failure or a host-key problem): 1, 2, 5, 10, then every 30 s, with a *Reconnecting in 5 s… [Now] [Cancel]* banner; the scrollback stays (a marker line shows the reconnect) and the login script runs again. Manual reconnect keeps the scrollback too.
- Restore tabs at startup, split layout included; restored tabs connect when first shown.
- Split panes: split right (Ctrl+Shift+D / ⌘D) or down (Ctrl+Shift+E / ⌘⇧D), nested, resizable, focus by click or Alt+arrows, Ctrl+Shift+W closes the focused pane; broadcast reaches every pane.
- *Open files here*: the file browser opens at the terminal's folder and follows its `cd`s (toggle in the toolbar and tab menu); a hint points to Shell integration when no folder is known.
- Host editor in sections (General, Authentication, Connection, Terminal, Files, Advanced) exposing every host option: protocol picker (SSH/Telnet/Serial) with serial settings and a port picker, Kerberos, saved-secret state with *Forget*, SOCKS/HTTP proxy, agent and X11 forwarding (display validated), file protocol, sudo, login script validated as you type, shell integration, per-host snippets; problems are shown inline and Save jumps to them.
- Settings in sections, adding mouse reporting and the button bar, a keyword-highlighting rule editor with live preview, global snippets, auto-reconnect / restore tabs, external editor with Browse…, saved passwords, and settings sync (folder, passphrase kept in the vault, Sync now / push / pull, last result; automatic at startup, after changes and every 5 minutes). Paths and the log level stay per computer.
- Tools menu: *Generate key…* (OpenSSH or PPK v3/v2, fingerprint, randomart, copy the public key, *Use in host…*), *Inspect or convert key…*, *Known hosts…* (search, remove lines or a host), *Saved passwords…*, *Settings sync…*. Key work runs off the UI thread.
- Connection notices (X11 unavailable, touch your security key, unusable key files, failed forwards) appear as notifications, de-duplicated and rate-limited.
- Sidebar: SSH/TEL/COM badge per host, long names and serial device paths truncated with a tooltip; empty-state shortcut hints updated; app shortcuts work before any terminal has focus.

**SSH**
- `Interaction::secret_outcome` reports whether a password or key passphrase (typed or saved) was accepted; a rejected saved password or passphrase is asked for once instead of skipping the method.

**Terminal**
- Telnet and serial hosts alongside SSH (new `hostwick-term` crate: one transport interface for all three). Telnet negotiates echo, SGA, binary, terminal type and window size (re-sent on resize), refuses unknown options per RFC 854/1143 without loops, handles `CR NUL`/`CR LF`, echoes locally when the server doesn't, uses TCP keepalive, a connect timeout and the host's proxy. Serial: baud/data/parity/stop bits and flow control, exclusive open, port listing, clear errors for missing, busy and permission-denied ports (the `dialout` hint on Linux) and for an unplugged adapter. Send break (menu, Ctrl+Break) on both. Tab titles name the protocol; Files on a telnet/serial host explains that it needs SSH.
- Find in scrollback (Ctrl+Shift+F / ⌘F): text or regex, case toggle, `3 of 17`, all visible matches highlighted, Enter/Shift+Enter and F3/Shift+F3, Esc closes. Counting runs off the UI thread in short locked chunks.
- Mouse reporting for htop, mc, vim, tmux: press/release/drag/motion/wheel with modifiers in SGR, UTF-8 or X10 encoding; Shift selects instead; wheel sends arrows in full-screen programs without mouse mode (alternate scroll); off switch in Settings.
- Keyword highlighting from the settings' rules, on visible rows only, foreground only, cached per row; selection, cursor and search stay on top. The default "success" rule now also matches `active (running)`.
- Current folder from OSC 7 (`TerminalView::cwd()`, `TerminalEvent::CwdChanged`); *Shell integration* installs a bash/zsh prompt hook (keeping existing hooks, kept out of history and off the screen).
- Login scripts run after connecting (`@wait`, `@sleep`; timeouts shown as a notice; cancellable), snippets in a button bar above the terminal and a right-click submenu (bracketed paste, optional Enter, broadcast-aware). Right-click → Copy current folder.

**File manager**
- Two-pane commander (local | remote, WinSCP style) with sortable columns, multi-selection, keyboard operation (F2/F4/F5/F6/F7, Del, Enter, Backspace, Tab, Ctrl+A, Ctrl+R, Alt+Enter) and a single-pane mode; the local folder is remembered per host.
- Drag and drop between panes, onto folders, and from the OS file manager onto either pane (local rows can also be dragged out on macOS/Wayland).
- Edit in an external editor: download to a per-tab temp folder, upload on every save (content hash, not timestamps; rename-on-save editors work), conflict prompt (overwrite / download theirs / save as copy) when the server copy changed, "Edits" list with status and stop, clean-up when the tab closes. Uploads keep the file's owner and permissions.
- Properties dialog: permissions as checkboxes + octal (with recursive apply), owner/group by id with names, folder size, SHA-256, symlink target; New symlink.
- sudo: browse as root (or another user) through `sudo sftp-server`, with passwordless sudo *and* sudo passwords (prompted or from the vault); wrong password, not in sudoers and requiretty are reported clearly. The sftp-server path is detected from sshd_config or the usual locations.
- SCP fallback for servers without SFTP (busybox, routers, ESXi): listings from `ls -la` (GNU, BSD/macOS and busybox formats), operations via quoted shell commands, contents via `scp -f`/`-t`; copies restart instead of resuming.
- Synchronize dialog: local→remote, remote→local or both (newer wins, never deletes, conflicts reported), optional mirror delete, excludes, a reviewable checklist with totals, conflicts settled per file; runs through the transfer queue. "Keep up to date" starts from the same dialog.
- CLI: `hostwick sync --both` and script `sync both`; `cp`/`sync` honor the host's file protocol and sudo settings.

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
