# Changelog

## 0.4.0

**Desktop: think less, click less**
- Command palette (Ctrl+Shift+P / Ctrl+Shift+K, ⌘K / ⌘⇧P, or the magnifier in the sidebar): fuzzy search over saved hosts, recent connections, unsaved `~/.ssh/config` hosts, a typed `user@host:port` (connect or save), every app action with its shortcut, snippets and open tabs; matched letters highlighted, ranking boosted by how often and how recently things were used (stored in the settings database); Enter terminal, Ctrl/⌘+Enter files, Shift+Enter new split, Alt+Enter edit; grouped results, at most 50, instant with thousands of hosts.
- Quick connect suggests saved hosts, recent quick connections and ssh_config aliases as you type; Enter picks the highlighted one or connects to the typed address, Ctrl/⌘+Enter opens files.
- Favourites (star on the row or right-click; stored as a host tag, so they sync) and the last eight connections in collapsible *Favourites* and *Recent* sections above the folder tree.
- Folders are a real tree: nested, collapsible (remembered), created empty (*New folder*), renamed in place, deleted with their hosts moving up a level. Drag hosts onto a folder (or the *Hosts* header) to move them.
- Multi-selection in the host list (Ctrl/⌘+click, Shift+click, Shift+arrows, Space, Ctrl/⌘+A); bulk edit of user, port, key files, jump hosts, proxy, folder, keepalive, file protocol and terminal type (only changed fields are applied, protocol-aware); bulk delete with one confirmation; F2 renames a host or folder in place.
- Open a folder or a selection in tabs, in a grid (one tab, evenly tiled) or in a grid with broadcast on; asks first above twelve hosts. Connections start in parallel; key-passphrase and keyboard-interactive prompts now name their host.
- Keyboard shortcut cheat sheet (F1 and Ctrl+/ outside terminals, Ctrl+Shift+/ anywhere, the ? button): grouped, searchable, platform-correct. Context menus show shortcuts right-aligned.
- First run: a welcome screen finds `~/.ssh/config`, PuTTY and WinSCP sessions, WinSCP.ini, a Solar-PuTTY export, agent keys and `~/.ssh` key files (in the background) and imports the ticked ones at once; later under Import… → *Scan this computer…*. Chosen key files are used by quick connect and new hosts.
- Palette action to switch between dark and light theme.
- Host list keys changed to match the palette: Enter terminal, Ctrl/⌘+Enter files (was Shift+Enter), Shift+Enter new split, Alt+Enter edit, F2 rename (was edit).

**Terminal**
- Drop files onto an SSH terminal to upload them to the shell's current folder (folders recursively, through the transfer panel); a small dialog asks when the folder is unknown. The terminal is outlined while files are dragged over it.
- ZMODEM (`sz`/`rz`) and trzsz (`tsz`/`trz`) in the terminal, implemented in `hostwick-term` and tested against real lrzsz and trzsz: download asks where to save, upload opens a file picker, progress card, Esc/Ctrl+C cancel, back-pressure instead of buffering, no overwriting, sanitised remote names, no binary noise after a cancel. trzsz directory transfers are declined; ZMODEM is limited to files under 4 GB.
- Clickable output: URLs, absolute/`~`/`./relative` paths, IPv4/IPv6 and host names. Ctrl/⌘+click opens, right-click adds *Open link / Copy link*, *Open in file browser / Download… / Copy path*, *Connect to… / Copy address*. Needs Shift while a program uses the mouse. Setting: *Clickable links and paths*.
- Production safety: a per-host *Environment* (none, development, staging, production, custom colour) shown as a coloured stripe on the tab and pane header and a line above the terminal, plus a red *PRODUCTION – host* ribbon for production. Multi-line or large pastes (over 8 KB) ask first with a preview, *Paste as single line* and, with broadcast on, the number of receiving terminals. Setting: *Confirm multi-line paste* (Always by default, Production hosts only, Never); production hosts always ask.
- Bell marker on background tabs and a *command finished* notification for commands longer than N seconds (default 10, OSC 133 from the shell integration, which now also emits command start/end for bash and zsh); clicking it focuses the tab. Uses the operating system notification when the window is in the background.
- Status bar under every terminal (setting, on by default): address, protocol, current folder, session log indicator, connection time, latency (SSH keepalive round trip), and optional server load (CPU, memory, disk, load average every 5 s; Linux and macOS; never on telnet/serial; only while visible).
- *Enable now* wherever the app says the terminal has not reported its folder (*Open files here*, drop upload, the terminal menu): turns shell integration on for the host and sets it up in the running shell without reconnecting.
- JetBrains Mono (OFL 1.1) regular, bold and italic is bundled and is the default terminal font on every system (about 830 KB; licence in `apps/desktop/assets/fonts/OFL.txt`).
- Host editor: *Environment* row. Settings → Terminal: status bar, server load, clickable links, paste confirmation, finished-command alerts and their threshold.
- The shell-integration hint in the host editor now lists what it enables.

**Desktop: tabs, windows, files, transfers**
- Tabs: Ctrl+Alt/⌘+1…8 jump to a tab and Ctrl+Alt/⌘+9 to the last (also with the terminal focused; see *Security and reliability* for why not plain Ctrl+digit); middle-click closes; drag to reorder (drop marker) or Ctrl+Shift+←/→; double-click the empty tab bar for the palette; the bar scrolls when crowded and a ⌄ button lists all tabs; drag a tab onto a pane edge to move its panes into a split.
- Reopen closed tab (last ten, with layout, host and scrollback and a "— reconnected —" marker), including tabs closed by *Close others/all* or with a window.
- Windows: *Move to new window* / *Move to window ▸* in the tab menu, or drag a tab out of the tab bar. Windows share the vault, settings, connections and transfers; closing the last window quits; positions, sizes and tab assignment are restored.
- Zoom: Ctrl/⌘+= / + / 0, Ctrl+Alt+- (⌘-) and Ctrl+wheel change the terminal font per tab (with a brief HUD, restored with the tab). Settings → Appearance → *Interface size* (90–150 %) scales the whole UI.
- File browser: clickable breadcrumb path bar (click the empty part or Ctrl+L to type; POSIX, Windows and UNC); bookmarks per host, also for the local side (star / Ctrl+D, dropdown; saved with the host so they sync); type-to-filter with a header filter box; Space quick preview of text (encoding fallback), images and file details, read in ranges from remote hosts.
- Transfers: pause/resume per transfer and *Pause all* (resumes from `.filepart`, SCP restarts), retry one or all failed, cancel all, *Show in folder*, speed and ETA, a collapsible summary line, and a notification when a batch finishes while Hostwick is in the background or took over 10 s.
- **Changed shortcuts**: *Reopen closed tab* is Ctrl+Shift+T (⌘⇧T); *Duplicate tab* moved to Ctrl+Shift+U (⌘T); marking a file row is Ctrl+Space / Insert (plain Space is now the preview); typing in a file pane filters instead of jumping.
- Transfer engine: `TransferEngine::pause/resume/pause_all/resume_all/retry`, `TransferEvent::{Paused, PausedAll}` and `Queued.replaces`; `HostProfile.bookmarks`.

**SSH**
- `Connection::ping()` measures the SSH keepalive round trip.
- `hostwick_ssh::agent_key_names()` lists the running agent's keys (with a timeout).

**Security and reliability**
- ZMODEM: a data subpacket without a terminator is capped at 16 KiB; longer is answered with a resync (ZRPOS) instead of buffering without limit (a peer sending ZDATA forever used to grow memory until the process died).
- trzsz: every decompressed message is limited (`#NAME`, `#CFG`, `#FAIL`, `#MD5`: 8 KiB or 64 bytes; file data: the server's announced buffer size, at most 16 MiB), so a few KB of zlib can no longer expand to gigabytes. Binary `#DATA:<n>` is rejected up front when `n` exceeds twice the buffer size (the escape worst case), data past the announced file size or an empty chunk is refused, and the line reader stops at 24 MiB.
- Downloads (ZMODEM and trzsz) are written to a private `name.part` file (`create_new`, mode `0600` on Unix) and renamed to their final, never-existing name only after ZEOF at the right position or a matching trzsz MD5. Cancel, an error, the other side cancelling, a new ZFILE in the middle of a file, the task being dropped or a failed checksum delete the `.part` file; no truncated file under the real name any more.
- Download names: Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`9`, `LPT1`–`9`, with any extension, any case) get an underscore after the stem; Unicode format characters (bidi overrides and isolates, zero-width characters, BOM, soft hyphen, tag characters, line/paragraph separators) are removed; trailing dots and spaces go; names are limited to 200 bytes.
- `unique_path` can no longer hand out a name that exists: files are created with `create_new` in a loop (no exists-then-create race, a dangling symbolic link is never followed), and running out of numbers is an error or a fresh name, never an existing file.
- **Shortcuts no longer steal shell keys** (regression of this release's tab and zoom shortcuts). With a terminal focused, the app takes no key that the terminal sends to the shell. Tab jump is Ctrl+Alt+1…9 (⌘1…9 on macOS), zoom out is Ctrl+Alt+- (⌘-); Ctrl+2…8 and Ctrl+- are control characters (NUL, ESC, FS, GS, RS, US, DEL; Ctrl+- is readline's undo) and Alt+digit is readline's numeric argument and tmux's `M-1`. Ctrl+= and Ctrl+0 never mapped to anything and stay. Plain Ctrl+1…9 and Ctrl+- still work outside terminals (host list, file browser). `Ctrl+Alt+digit/-` and `Ctrl+Shift+letter` are now reserved for the app and not sent to the shell; Ctrl+Tab is the tab switch and no longer sends Tab. Text typed with Ctrl+Alt (AltGr layouts) is never taken. A test walks every key with every modifier combination and fails when an app shortcut shares a key with the terminal encoder (Ctrl+F6, which leaves a terminal without the mouse, and Alt+arrows between split panes are the two documented exceptions).
- Paste into a broadcast group asks when any receiving terminal is a production host (not only the source), names them in the dialog, and applies the *Confirm multi-line paste* setting to the whole group. Text with control characters other than tab and line breaks is confirmed as well.
- Dropping files on a terminal always shows *Upload N items (size) to user@host* with the folder editable, and the default for existing files is *Skip* (*Keep both* and *Overwrite* are offered). An OSC 7 folder whose host does not match the session (or the host the shell first reported) is ignored, and one that cannot be tied to the host is flagged as unchecked.
- A pending ZMODEM/trzsz offer (or a running transfer) is dropped when the session ends or reconnects, so it no longer swallows keys; Enter reconnects again. Results of an older session's transfer are ignored.
- Clickable links: only `http`, `https`, `ftp` and `ftps` go to the operating system; `file:` (this computer only) is shown in the file manager after a confirmation, never opened; `ssh:` and `sftp:` fill in quick connect from a strictly checked `user@host:port`; everything else can only be copied.
- *Command finished* notifications from the operating system never contain the command; the in-app toast shows a short, cleaned copy.
- Image preview checks the declared size from the header (PNG, JPEG, GIF, WebP, SVG width/height/viewBox) before decoding: at most 16 384 pixels a side and 64 million pixels; SVGs with entity declarations are not decoded. Otherwise the file details are shown.
- The status bar's latency and metrics probes time out after 10 s (connection lookup included) and always clear their busy flag, so one unresponsive server no longer freezes the figures.

## 0.3.0

**Security and reliability**
- Jump hosts never receive the destination's saved password or a delegated Kerberos ticket: each hop uses the agent, key files and keyboard-interactive, and asks for its own password (named after the jump host).
- File names from servers are checked before they become paths: entries named `..`, or containing `/`, `\`, NUL or a drive prefix (`C:`), and on Windows reserved device names (`CON`, `NUL`, `COM1`…) and names ending in a dot or space, are left out of every listing (SFTP and SCP) with a warning in the file browser and the sync report. Downloads, recursive copies, sync plans and the external editor also refuse any path that would leave its folder. A malicious server, or a Linux file called `..\..\Startup\x.bat` downloaded on Windows, can no longer write outside the target folder.
- Mirror sync (*Delete*) refuses to run when its source folder does not exist (a typo used to plan deleting the whole destination), and the desktop always shows the plan before running deletions. Deletions now run before copies, so a case-only rename on Windows/macOS cannot delete the freshly copied file; two-way sync reports names that differ only in letter case as a conflict instead of letting one overwrite the other.
- Edit in external editor: uploads go to a temporary file next to the original, are size-checked, take the original's permissions and owner, and are renamed over it, so a dropped connection never leaves a truncated file on the server. Only when the folder is not writable, the owner cannot be kept, or the path is a link is the file rewritten in place (size-checked, and the status says *rewritten in place*).
- Edited copies live in a private (`0700`, files `0600`) folder in the app's data directory instead of the shared temp folder; closing a files tab keeps (and names) the folder when edits never reached the server; leftover folders are removed only when the instance that owns them has exited (file lock), never while in use. The SCP upload spool is private too.
- Accepting a changed host key replaces this host's key of that type instead of the line russh reported (which skips comment lines and could point at another host's entry); `@revoked` keys in known_hosts are refused with a clear message.
- Helper commands (checksums, folder sizes, sudo detection, the SCP fallback's `ls`/`mv`/`chmod`/…) run under `/bin/sh` whatever the login shell is: a file name crafted for fish's quoting rules can no longer inject commands. The SCP fallback lists with `ls -b` where available, so a line break in a file name cannot fake listing lines, and the `ls` parser no longer panics on unusual output (fuzzed).
- The desktop vault locks itself (instead of writing back a stale copy) when the vault file was changed by another program and cannot be re-read, e.g. after `hostwick vault passwd`.
- *Duplicate host* no longer shares saved-password slots with the original: the copy asks for its own, and remembering or forgetting a password never touches another host's.
- Settings sync: times more than 5 minutes ahead of this computer's clock are pulled back (and written back that way), so a computer with a wrong clock cannot make its deletions beat every later edit; a host edited after a deletion keeps winning.
- The database backup before a schema upgrade gets a unique name and never keeps Hostwick from starting.
- `hostwick script` uses the host's file protocol (SCP fallback, sudo) for every command; `rm`, `chmod` and `ln` no longer open a separate plain SFTP channel. Atomic replace (`mv -f` / rename) for transfers where the protocol allows, and a no-data-loss swap otherwise.
- X11 forwarding documentation now says what it does: like PuTTY and OpenSSH's trusted `ssh -Y`, forwarded clients get full access to your display.
- Minimum Rust version 1.89 (file locks from the standard library).

**Accessibility**
- Screen-reader names and roles throughout the desktop app (AccessKit → UI Automation, NSAccessibility, AT-SPI): icon buttons, the saved-host list ("prod/web01, SSH, deploy@10.0.0.5", selection), tabs as a tab list ("web01, connected, tab 2 of 3"), file panes as grids of rows, editor sections as toggle buttons, labelled fields, checkboxes and choice buttons with their state, hints, inline errors, dialog titles and bodies, split dividers (with increment/decrement), notifications as alerts.
- Terminal: "Terminal: <title>"; new output is announced when it settles (not for full-screen programs, and without the echo of what you typed or the shell-integration setup line); Ctrl+Shift+O (⌘⇧O) reads the latest output again; Ctrl+Shift+A (⌘⇧A) shows the scrollback as plain text for reading by line. Both are also in the terminal menu.
- Keyboard: arrow keys, Home/End, PageUp/PageDown in the host list (Enter terminal, Shift+Enter files, F2 edit, Delete with confirmation); Left/Right/Enter/Delete in the tab bar; Ctrl+F6 / Ctrl+Shift+F6 between host list, tab bar and session; Shift+F10 or the Menu key opens the context menu of the focused host, tab, file or terminal; Space toggles a file row; focus rings on the host list, tabs and editor sections.
- Escape closes every dialog even after the focused field disappeared (switching sections in the host editor used to lose focus); Enter saves the host editor and settings.
- Muted text in the light theme darkened to #595959 (at least 5.5:1 on every panel; it was 3.8:1 on buttons). Checkbox labels no longer clip the tails of g, p, y.

**Languages**
- The desktop app in English, German, Spanish, French, Brazilian Portuguese, Hindi, Simplified Chinese and Japanese: follows the system language (`sys-locale`), or Settings → Appearance → Language (native names); applied on save without a restart. Fluent files in `apps/desktop/locales/` with plurals and named placeholders; sizes and decimal commas follow the language. Tests check every language is complete and consistent and that every key used in the code exists.

**Desktop app**
- Split panes: drag a divider (or Alt+Shift+arrow) to resize, with a 120 px minimum; sizes are saved with the tabs and restored.
- A restored file browser follows its terminal again (and opens in the folder it was showing).
- Notifications are Hostwick's own toasts, drawn above dialogs, auto-dismissed (longer for warnings and errors), with a close button.
- Host editor → Connection → *Test connection* for SOCKS/HTTP proxies: connects through the proxy to the host (or first jump host) off the UI thread and shows the server greeting or the exact error.
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
