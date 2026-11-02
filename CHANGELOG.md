# Changelog

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
