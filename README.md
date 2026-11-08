<img src="packaging/icons/hostwick.png" width="96" alt="Hostwick icon">

# Hostwick

A fast, reliable SSH terminal, SFTP file manager and tunnel manager in one Rust
app — built to replace Solar-PuTTY, PuTTY and WinSCP on Windows, macOS and Linux.

- **One login per host.** Terminal tabs, the file browser and tunnels share one
  authenticated SSH connection (one host-key check, one MFA prompt).
- **No hangs.** All network work runs on a Tokio runtime, never the UI thread.
  Queues are bounded end to end, every request has a timeout, keepalives detect
  dead links, and terminal output is parsed off-thread with at most one repaint
  per frame.
- **Fast transfers.** 64 SFTP requests in flight per file (like OpenSSH),
  several files in parallel, resumable `.filepart` writes, retries with backoff.
- **Native GPU UI.** GPUI (Zed's UI framework) via `gpui-kit`, with the
  Alacritty terminal core. No webview.
- **Brings your sessions and keys.** Imports `~/.ssh/config` (with `Include`),
  PuTTY (registry or `.reg`), WinSCP (registry or `WinSCP.ini`) and Solar-PuTTY
  JSON exports, and reads PuTTY `.ppk` keys (v2 and v3, encrypted or not)
  directly — no conversion.

![Terminal](docs/screenshots/terminal.png)

![File browser](docs/screenshots/files.png)

| Host editor | Settings | Import report |
|---|---|---|
| ![Host editor](docs/screenshots/host-editor.png) | ![Settings](docs/screenshots/settings.png) | ![Import report](docs/screenshots/import-report.png) |

## Layout

```
crates/
  hostwick-core      domain types (profiles, targets, forwards, backoff) — no I/O
  hostwick-ssh       connection, auth chain, known_hosts, shell/exec, forwards, pool
  hostwick-sftp      RemoteFs port + SFTP (also via sudo), SCP and local-disk adapters
  hostwick-transfer  transfer engine: resume, retry, concurrency, rate limit, sync plan
  hostwick-vault     encrypted credential vault (Argon2id + XChaCha20-Poly1305)
  hostwick-store     SQLite profile store with migrations; app paths; portable mode
  hostwick-import    ssh_config, PuTTY, WinSCP and Solar-PuTTY importers
  hostwick-keys      key generation and conversion: OpenSSH, PuTTY .ppk v2/v3
  hostwick-term      terminal transports behind one interface: SSH shells, telnet, serial ports
apps/
  cli                `hostwick` command-line client
  desktop            `hostwick-desktop` GUI (GPUI + gpui-component + alacritty_terminal)
tests/fixtures       sample ssh_config, UTF-16 PuTTY .reg, WinSCP.ini
scripts/e2e.sh       throwaway sshd + end-to-end tests
```

Dependencies point inward: apps → adapters → `hostwick-core`. The UI never
touches `russh` directly, which is why the CLI and GUI share all behaviour and
why the transfer engine is tested against two local filesystems.

## Build

Stable Rust (edition 2024; built and tested with 1.97).

```sh
cargo build --release                      # core crates + CLI
cargo build --release -p hostwick-desktop  # GUI
```

Linux GUI builds need: `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev
libvulkan-dev libfontconfig-dev libfreetype-dev libx11-xcb-dev libxcb1-dev`.
Windows and macOS need nothing extra (the SSH crypto uses `ring`, so no CMake or
NASM).

## CLI

```sh
hostwick ssh admin@10.0.0.5                  # interactive shell
hostwick ssh web -- 'systemctl status nginx' # run a command; exit code is passed through
tar c . | hostwick ssh web 'tar x -C /srv'   # stdin is piped
hostwick ssh -N -L 5432:db:5432 -D 1080 bastion    # tunnels only
hostwick ssh -J ops@bastion db-01            # jump host(s), comma-separated

hostwick cp -r ./site web:/srv/www           # upload (scp semantics)
hostwick cp web:/var/log/syslog .            # download; interrupted copies resume
hostwick cp --existing newer -j 8 --limit 4096 -r src web:dst

hostwick sync ./site web:/srv/www            # WinSCP "Synchronize": copy new/changed files
hostwick sync --delete -x .git -x '*.tmp' ./site web:/srv/www   # mirror: also delete extras
hostwick sync -n --delete ./site web:/srv/www  # dry run: show what would change
hostwick sync --watch ./site web:/srv/www    # "Keep remote directory up to date" (Ctrl+C stops)
hostwick sync web:/var/log/app ./logs        # either direction

hostwick hosts add web 10.0.0.5 -l deploy -i ~/.ssh/id_ed25519 --folder prod
hostwick import putty                        # Windows: reads the registry
hostwick import putty sessions.reg           # anywhere: from `reg export`
hostwick import winscp WinSCP.ini
hostwick import ssh-config

hostwick vault init && hostwick vault set prod-db
hostwick hosts add db 10.0.0.9 --password-credential prod-db

hostwick script deploy.txt -D env=prod       # WinSCP-style batch script (see below)

hostwick keygen                              # ~/.ssh/id_ed25519 + .pub (passphrase prompted)
hostwick keygen -t rsa -b 4096 --ppk -f work.ppk -C me@work   # PuTTY key + work.pub
hostwick keyconv work.ppk                    # PuTTY → OpenSSH (`work` + `work.pub`)
hostwick keyconv ~/.ssh/id_ed25519 --to ppk2 # → id_ed25519.ppk for old PuTTY/WinSCP
hostwick keyconv id_ed25519 --new-passphrase # change ('' removes) the passphrase in place
hostwick keyinfo work.ppk --randomart        # type, bits, SHA256 fingerprint, comment, encrypted?

hostwick known-hosts list                    # line, hosts or (hashed), type, fingerprint
hostwick known-hosts remove web:2222         # like `ssh-keygen -R`, hashed entries too
hostwick known-hosts add web ssh-ed25519 AAAA...   # or a .pub file

hostwick settings-sync sync --folder ~/Dropbox/Hostwick   # later just `settings-sync sync`

hostwick hosts list --json                   # also: known-hosts list, ls, sync, keyinfo
```

`--batch` never prompts (for scripts); `--accept-new` trusts first-seen host
keys but never changed ones.

## Batch scripts (WinSCP-style)

`hostwick script FILE` (or `-` for stdin) runs one command per line, like
`winscp.com /script`: `open`, `close`, `cd`, `lcd`, `pwd`, `lpwd`, `ls`, `lls`,
`stat`, `put [-r] [-n]`, `get [-r] [-n]`, `mkdir [-p]`, `rm [-r]`, `rmdir`, `mv`,
`chmod 640 FILE`, `ln -s TARGET LINK`, `sync remote|local [--delete] [-x PATTERN]`,
`call SHELL-COMMAND` (output printed; a non-zero exit is an error), `echo`,
`option batch abort|continue`, `option confirm on|off`, `option json on|off`,
`exit`.

- `# comments`, `"quoted paths with spaces"`, `*`/`?` wildcards in `put`/`get`/`ls`/`rm`/`mv`/`chmod`.
- `%NAME%` comes from `-D NAME=VALUE` or the environment, `%1%`, `%2%` from
  parameters after `--`; `%%` is a literal `%`.
- `open` takes a saved host, `user@host[:port]` or WinSCP's
  `sftp://user[:password]@host[:port]/dir`, plus `-hostkey="SHA256:..."` (pin
  the server key; `*` accepts any), `-privatekey=FILE`, `-passphrase=`,
  `-password=`, `-port=`.
- `option batch abort` (default): the first error stops the script, exit
  code 1, with `script.txt:12: ...` naming the line. `continue` runs every
  line and still exits 1 if anything failed.
- `option confirm on` never overwrites existing files (they are skipped);
  `-n` skips files whose size and time already match.
- Transfers use the same engine as `cp`/`sync` (parallel, resumable).

See [`docs/examples/deploy-script.txt`](docs/examples/deploy-script.txt).

```sh
hostwick --batch script deploy.txt -D env=prod -- v1.4.2
echo 'open web
ls /var/log/*.log' | hostwick --batch script --json -
```

## Keys

`hostwick keygen` makes Ed25519 (default), ECDSA P-256/384/521 or RSA
2048–16384 keys. OpenSSH keys are encrypted like `ssh-keygen`
(bcrypt-pbkdf + aes256-ctr); `--ppk` writes PuTTY format 3 (Argon2id,
aes256-cbc, HMAC-SHA-256) and `--ppk2` format 2. Files are `0600` on Unix and
never overwritten without `--force`. `keyconv` converts OpenSSH, `.ppk` and
legacy PEM keys either way and changes or removes passphrases; output is
checked against `puttygen` and `ssh-keygen` in the tests.

## Settings sync

Point every computer at a folder your cloud drive (OneDrive, Dropbox, iCloud
Drive, Syncthing) or a git checkout keeps in sync:

```sh
export HOSTWICK_SYNC_PASSPHRASE=...           # or type it when asked
hostwick settings-sync sync --folder ~/OneDrive/Hostwick
```

- Saved hosts, deletions and app settings go into one file,
  `hostwick-sync.hwsync`, encrypted with the sync passphrase (Argon2id +
  XChaCha20-Poly1305). Vault secrets never leave the computer; hosts only
  name their vault entries.
- `sync` merges both ways: per host the newest edit wins, and a deletion
  wins over older edits. `push` makes the file match this computer (and
  re-keys it with a new passphrase); `pull` makes this computer match the file.
- When two computers write at once, cloud clients keep a "conflicted copy"
  (`hostwick-sync (… conflicted copy …).hwsync`,
  `hostwick-sync.sync-conflict-….hwsync`, `hostwick-sync-PCNAME.hwsync`);
  the next sync merges and removes them.

## Download

Prebuilt Windows, macOS and Linux builds are attached to each
[GitHub Release](https://github.com/meetdhruva/hostwick/releases). Pushing a
`v*` tag — or pushing to the `release` branch, which tags the version from
`Cargo.toml` — builds and publishes them (`.github/workflows/release.yml`).

## Directory sync

Like WinSCP's *Synchronize* and *Keep remote directory up to date*:

- One-way, either direction, files compared by size and modification time
  (copies keep timestamps, so a second run is a no-op).
- `--both` (WinSCP "Both"): the newer copy of each file wins in either
  direction and nothing is ever deleted. A file whose timestamps match but
  whose sizes differ was changed on both sides: it is reported as a conflict,
  never guessed at. `-n` prints the plan instead.
- `--delete` mirrors: removes destination files and folders missing from the
  source. Without it nothing on the destination is ever deleted.
- `--exclude` globs (`*.tmp`, `.git`, `build/out/*`); excluded files are
  neither copied nor deleted.
- `--watch` polls the source (default every 2 s) and syncs on change. Polling
  works identically on every OS and on network drives; when the source is
  local, sub-second timestamps catch even same-size edits. Errors such as a
  dropped connection are reported and retried, never fatal.
- In the desktop app: **Synchronize…** in the file browser (preview as a
  checklist, then run), and **Keep up to date…**.

## Desktop app

```sh
cargo run --release -p hostwick-desktop
```

**Command palette** (Ctrl+Shift+P or Ctrl+Shift+K; ⌘K or ⌘⇧P on macOS; the
magnifier in the sidebar header): one fuzzy search over saved hosts, recent
connections, `~/.ssh/config` hosts not saved yet, a typed `user@host:port`
(connect without saving, or *Save as a host…*), every app action with its
shortcut (new host or folder, split, close pane, reconnect, broadcast, each
settings section, key generator, known hosts, imports, *Scan this computer*,
dark/light theme, find, open files here…), the focused terminal's snippets and
the open tabs. Matched letters are highlighted; what you use often and
recently ranks higher (kept in the settings database). Enter opens a terminal
(or runs the action), Ctrl/⌘+Enter the files, Shift+Enter a new split beside
the focused terminal, Alt+Enter edits the host; arrows, PageUp/PageDown, the
mouse and Esc work as expected. It stays instant with thousands of hosts.

**Quick connect** suggests saved hosts, recent quick connections and
ssh_config aliases while you type (same matching as the palette): Enter takes
the highlighted one (or connects to what you typed when it is clearly an
address), Ctrl/⌘+Enter opens files, Up/Down choose, Esc closes the list.

**Favourites and recents**: star a host (the star on its row, or right-click →
*Add to favourites*; it is a host tag, so settings sync carries it) and it
appears under *Favourites* at the top of the sidebar; *Recent* lists the last
eight connections (saved hosts and quick connections). Both sections, and every
folder, fold and stay folded.

**Folders and many hosts at once**: folders nest (`prod/eu`) and can be
created empty (*New folder* button, palette or the *Hosts* header menu),
renamed in place (F2) or deleted (their hosts and subfolders move up one
level, after asking). Select several hosts with Ctrl/⌘+click, Shift+click,
Shift+arrows or Ctrl/⌘+A; drag them onto a folder (or onto the *Hosts* header to
take them out of folders); *Edit N hosts…* changes user, port, key files, jump
hosts, proxy, folder, keepalive, file protocol or terminal type of all of them
— a field shows the shared value, and only fields you change are applied;
deleting asks once. F2 renames a host in place (Enter saves, Esc cancels).
Right-click a folder or a selection → *Open all in tabs*, *Open all in a
grid* (one tab, panes tiled as evenly as possible) or *… with broadcast on*;
above twelve hosts Hostwick asks first. Connections start in parallel;
password and other questions come one at a time, each naming its host.

**First run**: with no saved hosts, the main area offers to import what this
computer already has — `~/.ssh/config`, PuTTY sessions (registry), WinSCP
sites (registry or `WinSCP.ini`), a Solar-PuTTY JSON export in Documents,
Downloads or Desktop — with the number of hosts in each, the keys your SSH
agent or Pageant holds, and the key files in `~/.ssh` (tick to use them for
quick connect and new hosts). One *Import selected* button, or *Skip*. The
same screen is under Import… → *Scan this computer…*. The search runs in the
background; sources that are missing are not shown.

**Cheat sheet**: F1 (or Ctrl+/ outside a terminal, Ctrl+Shift+/ anywhere, or
the ? button) lists every shortcut, grouped and searchable, written the way
your platform writes keys.

Sidebar with saved hosts (folders, search, quick connect, an SSH/TEL/COM badge
per host), terminal and file tabs with split panes, a transfer queue with
progress and cancel, and broadcast input to every open terminal. Host-key,
password, passphrase and MFA prompts appear as dialogs.

**Saved passwords.** Password, key-passphrase, sudo and proxy prompts have a
*Remember* checkbox. The first time, Hostwick offers to create a vault (a
master password with a strength hint; the same encrypted `vault.json` the
`hostwick vault` command uses); later it asks once per session to unlock it,
then logs in by itself. A secret is saved only after the server accepted it.
If a saved password stops working, Hostwick asks again (with *Update the saved
password* ticked) instead of failing or looping. Settings → *Saved passwords*:
lock now, auto-lock after N idle minutes, change the master password, and
remove saved entries (listed by host and purpose, never shown). Key
passphrases are saved per key file, so every host using the key benefits.
The lock icon in the sidebar header shows (and locks) an unlocked vault.

**Auto-reconnect.** When an SSH or telnet connection drops (not when you type
`exit`), the terminal shows *Connection lost. Reconnecting in 5 s… [Now]
[Cancel]* and retries after 1, 2, 5, 10, then every 30 seconds, keeping the
scrollback (a `── reconnected 12:01:02 ──` line marks the spot) and running
the login script again. It stops at a login or host-key failure.
Settings → Connections turns it off.

**Restore tabs.** Open tabs (with their split layout and pane sizes, and a
file browser still following its terminal) come back at the next start; each connects when you first switch to it, so starting Hostwick is not
a storm of password prompts. Quick-connect tabs are restored from their
`user@host:port`.

**Split panes** (MobaXterm/Tabby style): Ctrl+Shift+D splits the current
terminal to the right, Ctrl+Shift+E downwards (⌘D / ⌘⇧D on macOS), also from
the terminal and tab menus. Splits nest; drag the dividers (or
Alt+Shift+arrow; sizes are remembered, no pane gets smaller than 120 px);
click a pane (or Alt+arrow) to focus it; Ctrl+Shift+W closes the focused pane, then the tab.
A new pane opens the same host over the same SSH connection. Broadcast input
reaches every pane.

**Files follow the terminal.** Right-click a terminal → *Open files here*
opens the file browser at the shell's current folder and keeps following it
as you `cd` (*Following terminal* in the toolbar or the tab menu turns it
off). The folder comes from OSC 7; turn on *Shell integration* in Edit host →
Terminal for bash/zsh.

**Tools** (wrench icon): *Generate key…* (Ed25519, ECDSA, RSA 2048–4096;
OpenSSH or PuTTY `.ppk` v3/v2; fingerprint, randomart, public key with Copy,
and *Use in host…*), *Inspect or convert key…* (format, type, fingerprint;
change or remove the passphrase; convert OpenSSH ↔ PPK), *Known hosts…*
(search, remove selected lines or every key of a host), *Saved passwords…*
and *Settings sync…*.

**Notifications** for things the connection wants you to know without
stopping: X11 not available, touch your security key, a key file that could
not be used, a saved forward that failed (repeats are suppressed).

Menu items show their shortcut on the right.

**Right-click everywhere**

| Where | Menu |
|---|---|
| Saved host | Open terminal, Open files, Open in a new split, Edit…, Rename, Duplicate, Add to / Remove from favourites, Copy address, Delete… |
| Several selected hosts | Open all in tabs / in a grid / in a grid with broadcast on / in file browsers, Edit N hosts…, favourites, Delete N hosts… |
| Folder | Open all in tabs / in a grid / in a grid with broadcast on, New subfolder, Rename, Delete folder… |
| *Hosts* / *Favourites* / *Recent* header | New folder / open all favourites / Clear recent connections |
| Tab | Reconnect, Split right/down, Send break (telnet/serial), Follow terminal folder (linked file browser), Duplicate, Open files/terminal, Close, Close others, Close all |
| Terminal | Copy, Paste, Select all, Find…, Snippets ▸, Copy current folder, Send break (telnet/serial), Stop script, Clear scrollback, Log session to file… / Stop, Open files here / Open files (SSH), Split right, Split down, Duplicate, Reconnect |
| File | Open, Edit (F4), Upload/Download (F5), Move (F6), Rename… (F2), Delete…, New folder… (F7), New symlink…, Copy path, Refresh, Properties (Alt+Enter) |

**Terminal**

- **SSH, telnet and serial.** A host's *protocol* picks the transport; tabs
  say which (`router (telnet)`, `switch (/dev/ttyUSB0 9600 8N1)`), and
  reconnect works the same for all three.
  - *Telnet* (RFC 854/855): negotiates echo, suppress-go-ahead, binary,
    terminal type and window size (sent again on every resize), refuses
    other options without negotiation loops, handles `CR NUL`/`CR LF`
    and sends Enter as `CR LF`; echoes locally when the server does not.
    TCP keepalive, a connect timeout, and the host's SOCKS/HTTP proxy are
    honoured. A host still on port 22 is taken to mean 23.
  - *Serial*: baud, data bits, parity, stop bits and XON/XOFF or RTS/CTS
    flow control; DTR/RTS raised on open; ports are opened exclusively.
    Clear errors for a missing port (with the ports that do exist), a port
    in use, and permission denied (on Linux: add yourself to `dialout`).
    An unplugged adapter ends the session with the reason on screen.
  - *Send break* (right-click, or Ctrl+Break) on telnet (`IAC BRK`) and
    serial (a 300 ms line break) — for ROMMON, bootloaders, SysRq.
  - The file browser needs SSH; on a telnet or serial host it says so.
- **Find in scrollback** (Ctrl+Shift+F / ⌘F, or right-click → Find…): plain
  text or regex, case-sensitive toggle, `3 of 17`, every match on screen
  highlighted and the current one stands out. Enter/F3 goes to the previous
  (older) match, Shift+Enter/Shift+F3 to the next; Esc closes. Counting runs
  in the background, so a huge scrollback never freezes the window.
- **Mouse in htop, mc, vim (`:set mouse=a`), tmux**: clicks, drags, motion and
  the wheel go to programs that ask for them (SGR, UTF-8 and X10 encodings,
  with Shift/Alt/Ctrl). Hold **Shift** to select text instead, like xterm.
  Without mouse mode the wheel sends arrow keys in full-screen programs
  (`less`, `man`). *Settings → Mouse reporting* turns it off.
- **Keyword highlighting**: text matching *Settings → highlight rules* is
  coloured (errors red, warnings amber, success green by default; plain
  words match whole words, case-insensitively; regexes as written). Only the
  colour of the text changes, and only on visible rows — a 300k-line flood
  is not slowed down.
- **Current folder** (OSC 7): *Shell integration* in the host editor types a
  one-time hook into bash or zsh after login so the shell reports its
  folder; existing `PROMPT_COMMAND`/`precmd` hooks are kept, the line starts
  with a space (out of history with `HISTCONTROL=ignorespace` or zsh's
  `HIST_IGNORE_SPACE`) and is erased from the screen. Shells that already
  send OSC 7 (fish, many distro configs) need nothing. Right-click → *Copy
  current folder*. The same hook also reports where each command starts and
  ends (OSC 133, bash and zsh), which powers the *command finished* alerts;
  anywhere the app says the terminal has not reported its folder (*Open files
  here*, following the terminal, dropping files) there is an **Enable now**
  button: it turns the setting on for that host (saved with the profile) and
  sets the hook up in the running shell, no reconnect.
- **Drop files onto a terminal to upload them** (SSH). They go to the shell's
  current folder (OSC 7); folders are sent recursively, through the normal
  transfer engine and panel (resume, parallel, speed shown). While files
  are dragged over the terminal it is outlined and says where they will go.
  When the folder is not known a small dialog asks (*Upload to:*, prefilled
  with the home folder) and offers *Enable now*. Telnet and serial sessions
  explain that there is nothing to upload to. (Dragging rows out of the
  built-in file pane onto a terminal is not supported.)
- **ZMODEM and trzsz in the terminal.** `sz`/`rz` (lrzsz) and `tsz`/`trz`
  (trzsz) are detected from their start sequences: a download asks where to
  save (the Downloads folder by default, *Change…* to pick another), an
  upload opens a file picker, a progress card shows speed and size, and
  normal output resumes afterwards. Esc, Ctrl+C or *Cancel* stop it (as does
  the remote side with five CANs); a cancelled ZMODEM transfer does not leave
  binary noise on screen. Files are never overwritten (`name (1).ext`), names
  from the remote are sanitised, and the remote side is held back by flow
  control instead of buffering a file in memory. Limits: ZMODEM positions are
  32 bits (files under 4 GB), and trzsz directory transfers (`trz -d`) are
  declined.
- **Clickable output.** URLs, absolute paths, `~/…` and `./relative` paths
  (resolved against the shell's folder), IPv4/IPv6 addresses and host names
  are recognised under the pointer. URLs are underlined on hover and open
  with Ctrl/⌘+click; the other kinds are underlined while Ctrl/⌘ is held.
  Right-click adds entries on top of the usual menu: *Open link* / *Copy
  link*; *Open in file browser* (reveals the file in a files tab) /
  *Download…* / *Copy path*; *Connect to …* (fills quick connect) / *Copy
  address*. Detection runs for the line under the pointer only when it moves
  to another cell, selection, search and keyword highlighting are unchanged,
  and with mouse reporting on (htop, vim) hold Shift. *Settings → Terminal →
  Clickable links…* turns it off.
- **Production safety.** *Edit host → General → Environment*: none, development
  (green), staging (amber), production (red) or a custom `#rrggbb` colour. The
  colour shows as a stripe on the tab, a bar at the pane header and a 3 px
  line above the terminal; production hosts add a red ribbon *PRODUCTION –
  host* that stays visible. **Paste protection:** pasting several lines (or
  more than 8 KB) shows a dialog with the first lines, the target host and
  *Paste* / *Paste as single line* / *Cancel*; bracketed paste is still used.
  *Settings → Terminal → Confirm multi-line paste*: *Always* (default),
  *Production hosts only* or *Never*. Production hosts always ask. With
  broadcast on, the dialog says how many other terminals receive the paste.
- **Bell and command-finished alerts.** A bell (BEL) in a tab or pane you are
  not looking at, or while the window is in the background, puts a bell on
  the tab. With shell integration, a command that takes longer than N seconds
  (default 10) raises a notification such as "web01: make build finished
  (exit 0, 2m 13s)"; clicking it focuses that tab. It is an operating system
  notification when the window is not in front and an in-app toast with a
  *Show* button otherwise. *Settings → Terminal*: on/off and the threshold.
- **Status bar** under every terminal (*Settings → Terminal*, on by
  default): `user@host:port`, protocol, current folder, session log
  indicator, connection time and latency. Latency is an SSH keepalive round
  trip (a timed `true` every 30 s on servers that do not answer keepalives);
  both are measured only while the terminal is visible. Optional **server
  load** (off by default) adds CPU, memory, root disk and load average every
  5 s from one small read-only command over the same connection (Linux
  `/proc`; macOS `vm_stat`/`sysctl`; elsewhere shown as n/a). Nothing is ever
  run on telnet or serial. Narrow split panes drop the least important
  items first.
- **Terminal font.** JetBrains Mono (SIL Open Font License 1.1, regular, bold
  and italic, embedded; the licence is in `apps/desktop/assets/fonts/OFL.txt`)
  is the default on every system, without ligatures. Settings → *Terminal
  font* overrides it; CJK and Devanagari come from the system's fallback
  fonts.
- **Login scripts** (host editor): run after the session opens — one command
  per line, `@wait text` (or `@wait 5000 text`) waits for output,
  `@sleep 500` pauses, `#` comments. A wait that times out shows a notice in
  the terminal; right-click → *Stop script* cancels.
- **Snippets and button bar**: snippets from Settings (all hosts) and the
  host editor (that host) appear as a slim button row above the terminal
  (tooltip shows the text) and under right-click → *Snippets*. Multi-line
  snippets go in as a bracketed paste when the shell supports it; *run*
  presses Enter after. With broadcast on they go to every terminal.
  *Settings → Show button bar* hides the row.

**File browser** (WinSCP Commander style): this computer on the left, the
server on the right (*Hide local* gives a single remote pane; the local folder
is remembered per host).

- Sortable columns (click a header), multi-selection (click, Ctrl/⌘+click,
  Shift+click, Ctrl+A) and keys: Enter open, Backspace up, F5 copy to the other
  side, F6 move, F2 rename, F4 edit, F7 new folder, Del delete, Ctrl+R refresh,
  Tab switch side, Alt+Enter properties, arrows/Home/End/PageUp/PageDown.
- **Drag and drop** rows between the panes to copy them, onto a folder row to
  copy (or, within one side, move) into it, and drop files and folders from
  your file manager onto either side. Dragging *out* to the file manager works
  for local rows on macOS and Wayland; remote rows (and X11/Windows) cannot be
  dragged out — use Download.
- **Edit in your editor** (F4 or right-click → Edit): the file is downloaded to
  a private folder (`0700`, in the app's data directory) under its own name
  and opened with *Settings → External
  editor* (`{file}` is replaced by the path, otherwise appended; empty = the
  system's default app). Every save is uploaded — changes are detected by
  content hash, not timestamps, so editors that save by rename and time-zone
  differences do not matter. If the server copy changed meanwhile you choose:
  *Overwrite*, *Download theirs* or *Save as copy*. Each upload goes to a
  temporary file next to the original, is size-checked, gets the original's
  permissions and owner, and is renamed over it — a dropped connection never
  leaves a truncated file. Only when that is impossible (folder not writable,
  owner cannot be kept, a symbolic link) is the file rewritten in place, and
  the status says so. The **Edits** button lists open files with their state
  (watching, uploading, uploaded 12:01, error) and stops watching; the copies
  are removed when the tab closes, unless some edits never reached the server
  — then the folder is kept and you are told where.
- **Properties**: type, size and *Calculate* for folders (computed on the
  server with `find` when possible), modified time, owner/group (names and
  numeric ids, editable), permissions as checkboxes kept in step with an octal
  field (plus setuid/setgid/sticky), *Apply to everything inside* (files keep
  their execute bits), SHA-256 on demand (`sha256sum` on the server when
  available), symlink target. *New symlink…* creates links.
- **Root**: *Reopen as root* runs the server's `sftp-server` through
  `sudo -u root` (or the host's sudo user). Passwordless sudo and sudo with a
  password both work — the password is asked for in a dialog (or taken from
  the vault) and fed to sudo only after its prompt appears; wrong password,
  "not in sudoers" and `requiretty` are reported as such. A red **ROOT** badge
  marks the elevated side. Hosts with *sudo* enabled open elevated.
- **Servers without SFTP** (routers, ESXi, busybox): with the file protocol on
  *Auto* the browser falls back to SCP and shell commands (`ls`, `mv`, `chmod`
  …); everything above works except resuming interrupted transfers.

Prefer PuTTY's "right-click pastes"? Settings → Right-click → *Paste (PuTTY)*.

**Host editor** (New host… or right-click → Edit…), in sections:
*General* (name, folder, protocol SSH/Telnet/Serial — switching adjusts the
fields and the default port —, host and port, or for serial the device with a
port picker, speed, data bits, parity, stop bits, flow control; notes),
*Authentication* (key files with Browse…, `-cert.pub` certificates picked up
automatically, agent, password with its saved state and *Forget*,
keyboard-interactive, Kerberos single sign-on with ticket forwarding),
*Connection* (SOCKS5/SOCKS4/HTTP proxy with *Test connection*, which
connects through it to the host — or the first jump host — and shows the
server's greeting or the exact error; jump hosts, port forwards
`L8080:localhost:80 R9000:localhost:3000 D1080`, keepalive, agent forwarding,
X11 forwarding and display), *Terminal* (terminal type, login script checked
as you type, shell integration, the host's snippets, session log), *Files*
(Auto/SFTP/SCP, open as root through sudo, sudo user, sftp-server path) and
*Advanced* (legacy algorithms). Problems are shown next to the field, and
Save jumps to the section that needs attention.

**Settings** (gear icon, `Ctrl+,` / `⌘,`), in sections: *Appearance* (language,
theme, terminal font and size), *Terminal* (scrollback, right-click, copy-on-select,
mouse reporting, button bar, keyword-highlighting rules with regex toggle,
colour swatch and a live preview), *Snippets* (global), *Connections*
(auto-reconnect, restore tabs), *Files* (external editor with Browse…),
*Session logging*, *Saved passwords*, *Settings sync* (folder with Browse…,
passphrase kept in the vault, Sync now / replace either side, last result;
syncs at startup, a few seconds after hosts or settings change and every 5
minutes) and *Diagnostics* (app log level, log folder). Paths, the editor and
the log level stay per computer; everything else syncs.

**Logging**

- *Session logs* (like PuTTY's): per host in the host editor, for every
  session in Settings, or on demand from the terminal's right-click menu.
  File names take PuTTY tokens — `&S` session, `&H` host, `&P` port, `&U` user,
  `&Y&M&D` date, `&T` time — so `&S-&Y&M&D.log` gives `prod-web-20261102.log`.
  *Printable text* strips escape codes; *Raw* keeps them for replay. A **● LOG**
  badge shows while a terminal is being logged.
- *App log*: daily-rotated `hostwick.YYYY-MM-DD.log` (14 kept) in the log
  folder, level set in Settings (`HOSTWICK_LOG` overrides it).

**Shortcuts** (`⌘` instead of `Ctrl+Shift` on macOS — plain `Ctrl+letter` keys
always go to the shell):

| Keys | Action |
|---|---|
| Ctrl+Shift+P or Ctrl+Shift+K (⌘K / ⌘⇧P) | Command palette |
| F1, Ctrl+/ (outside terminals), Ctrl+Shift+/ (anywhere) | Keyboard shortcut cheat sheet |
| Ctrl+Shift+N | New host |
| Ctrl+Shift+T | Duplicate current tab |
| Ctrl+Shift+D / Ctrl+Shift+E (⌘D / ⌘⇧D) | Split the terminal right / down |
| Alt+←→↑↓ | Move between split panes |
| Ctrl+Shift+W | Close the focused pane, then the tab |
| Ctrl+Shift+R | Reconnect |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+Shift+B | Toggle broadcast input |
| Ctrl+, | Settings |
| Ctrl+Shift+C / V, Shift+Insert | Copy / paste in the terminal |
| Shift+PageUp / PageDown | Scroll back |
| Ctrl+Shift+F | Find in scrollback |
| Enter / Shift+Enter, F3 / Shift+F3 | Previous (older) / next match while finding |
| Esc (in the find bar) | Close find |
| Shift+click / drag | Select text even when a program has the mouse |
| Ctrl+click (⌘+click) | Open the URL or path under the pointer |
| Esc or Ctrl+C during a ZMODEM/trzsz transfer | Cancel it |
| Ctrl+Break | Send break (telnet, serial) |
| Enter on an ended session | Reconnect (or reconnect now while waiting) |
| Alt+Shift+←→↑↓ | Resize the focused split pane |
| Ctrl+F6 / Ctrl+Shift+F6 | Move focus: host list → tabs → session (and back) |
| Shift+F10 or the Menu key | Context menu of the focused host, tab, file or terminal |
| Ctrl+Shift+O (⌘⇧O) | Read the terminal's latest output again (screen readers) |
| Ctrl+Shift+A (⌘⇧A) | Terminal output as plain text, to read line by line |

When a connection fails, the error lists every authentication method tried and
why it failed (for example `key work.ppk: wrong passphrase; password: rejected`).

### Accessibility and languages

**Keyboard.** Everything works without a mouse. Tab and Shift+Tab move
through the sidebar, the tab bar and the open tab; inside a terminal Tab
belongs to the shell, so Ctrl+F6 / Ctrl+Shift+F6 moves between the host
list, the tab bar and the session. In the host list the arrows, Home/End and
PageUp/PageDown move (Shift extends the selection, Left/Right fold folders),
Enter opens a terminal, Ctrl+Enter the files, Shift+Enter a new split,
Alt+Enter edits, F2 renames, Space marks, Ctrl+A marks all, Delete deletes
(after a confirmation). In the tab bar Left/Right
switch tabs, Enter or Down goes into the tab, Delete closes it. Shift+F10 or
the Menu key opens the context menu of whatever has focus (host, tab, file
row, terminal). File lists add Space to toggle a row. Dialogs: Escape
closes, also while a text field has focus; Enter confirms (Save in the host
editor and settings). The focused element has a visible ring.

**Screen readers.** Hostwick's toolkit (GPUI) talks to the platform
accessibility API through AccessKit: UI Automation on Windows (Narrator,
NVDA, JAWS), NSAccessibility on macOS (VoiceOver), AT-SPI on Linux (Orca).
Every control has a name and a role: icon buttons say what they do; saved
hosts are a list whose items read like "prod/web01, SSH, deploy@10.0.0.5"
and report selection; tabs are a tab list ("web01, connected, tab 2 of 3");
file panes are a grid of rows; section switchers in the editors are toggle
buttons; checkboxes, toggles, hints, errors and dialog titles are exposed;
notifications are alerts. A terminal is announced as "Terminal: <title>";
new output is read once it settles (a live region; full-screen programs
are not read), Ctrl+Shift+O reads the latest output again and Ctrl+Shift+A
opens the scrollback as plain text for reading by line, word and character.

What has actually been checked, per platform:

- *Linux*: the accessibility tree was inspected through AT-SPI (the
  interface Orca uses) under X11: names, roles, selection and toggle
  states, dialog contents, context menus opened with Shift+F10, and the
  terminal's output announcements. Orca itself has not been run against
  Hostwick yet.
- *Windows and macOS*: the same AccessKit tree is handed to UI Automation
  and NSAccessibility, but no screen reader has been tried there yet.
- Known gaps: dialogs have no accessible name of their own (the toolkit
  does not offer one) — the title is the dialog's first text; the terminal
  is not exposed as an editable text with a caret (use Ctrl+Shift+A for
  that); a few strings of the widget toolkit (text-field context menu,
  "Show password") stay in English except in Portuguese and Chinese.

**Text size and contrast.** Display scaling is followed on every platform;
GNOME's "Large text" applies on X11 (it changes the X DPI), not on
Wayland, and Windows' separate "Make text bigger" setting is not read.
Muted text meets WCAG AA (4.5:1) in both themes.

**Languages.** English, Deutsch, Español, Français, Português (Brasil),
हिन्दी, 简体中文 and 日本語. Hostwick follows the system language at first;
Settings → Appearance → Language picks another, applied as soon as you
save (dialogs that are already open switch when reopened). Product and
protocol names (SSH, SFTP, sudo, Kerberos, PuTTY…) are kept as they are,
and messages from servers and the SSH layer (for example authentication
errors) are shown as received, in English. Hindi and the CJK languages need
fonts with those scripts: Windows and macOS have them; on Linux install
Noto (`fonts-noto-core` and `fonts-noto-cjk` on Debian/Ubuntu,
`google-noto-sans-devanagari-fonts google-noto-sans-cjk-fonts` on
Fedora). Which Chinese or Japanese glyph shapes the fallback font uses
follows the system locale on Linux.

The texts live in `apps/desktop/locales/<language>.ftl` ([Fluent](https://projectfluent.org/)
files, with plurals and named placeholders); the code asks for them with
`t!("key")` or `t!("key", name = value)`. Tests check that every language has
every message with the same placeholders and none empty, that every key
used in the code exists, and that every message in `en.ftl` is used. To add
a language, copy `en.ftl`, translate it, and add one line to `LANGUAGES` in
`apps/desktop/src/i18n.rs`.

### Install

- **Windows**: unzip anywhere and run `hostwick-desktop.exe`.
- **macOS**: unzip and move `Hostwick.app` to Applications.
- **Linux**: extract and run `./install.sh` (installs to `~/.local`, adds the
  app and its icon to your applications menu), or run `./hostwick-desktop`
  directly.

## Security

- Host keys are always verified against `known_hosts` (shared with OpenSSH).
  First-seen keys need explicit trust; a changed key shows a man-in-the-middle
  warning and requires deliberately replacing the saved key.
- Legacy algorithms (`ssh-rsa`/SHA-1 host keys, SHA-1 DH) are off unless a
  profile opts in for old network gear.
- Secrets are `SecretString`s, zeroized on drop, never logged. The vault file
  authenticates its KDF parameters, writes atomically, and is `0600` on Unix.
- Agent and X11 forwarding are off by default and only ever go to the
  destination host, never to jump hosts. Agent or X11 channels a server opens
  without being asked are refused. X11 forwarding works like PuTTY's and
  OpenSSH's *trusted* `ssh -Y`: the server only sees a random fake cookie,
  which is checked and swapped for the real one locally, but forwarded X
  clients get full access to your display (they can read keystrokes and take
  screenshots), so only forward X11 to servers you trust.
- `russh` ≥ 0.62.5 is required (CVE-2026-73429 and a second fix); this tree
  uses 0.65. Run `cargo deny check` in CI.

## Data locations

`hostwick paths` prints them. Default: the OS data directory plus
`~/.ssh/known_hosts`. **Portable mode:** put an empty `hostwick.portable` file
next to the executable and everything lives in `./data` beside it.
`HOSTWICK_HOME=/some/dir` makes a fully self-contained setup.

## Tests

```sh
cargo test --workspace     # unit + integration tests
scripts/e2e.sh             # starts sshd on 127.0.0.1:2222, runs the e2e suite
scripts/e2e-net.sh         # sshd on :2223 (+ KDC, Xvfb, agent, tinyproxy, dante):
                           # proxies, X11/agent forwarding, certificates, FIDO, Kerberos
scripts/e2e-files.sh       # sshd on :2230/:2231 (SCP only) with sudo users: sudo SFTP
                           # (password, NOPASSWD, refusals), SCP, properties, two-way sync
# `hostwick script` and the --json outputs against any OpenSSH server:
HOSTWICK_TOOLS_E2E_PORT=2250 HOSTWICK_TOOLS_E2E_USER=me HOSTWICK_TOOLS_E2E_KEY=~/.ssh/test_key \
  HOSTWICK_TOOLS_E2E_HOST_PUB=/etc/ssh/ssh_host_ed25519_key.pub \
  cargo test -p hostwick-cli --test script_e2e
```

The key tests also run real `puttygen` and `ssh-keygen` when they are
installed, and the shell-integration test runs real `bash` and `zsh`.

Terminal transports (`hostwick-term`): telnet is always tested against an
in-process fake server; these also use real ones:

```sh
# a pty pair as a "serial cable" (needs socat)
HOSTWICK_SERIAL_E2E=1 cargo test -p hostwick-term --test serial
# a real telnetd whose login program is a shell, on loopback, e.g.
#   socat TCP-LISTEN:2241,bind=127.0.0.1,reuseaddr,fork EXEC:"/usr/sbin/telnetd -h -E /bin/sh",nofork
HOSTWICK_TELNETD=127.0.0.1:2241 cargo test -p hostwick-term --test telnet
```

The e2e suite covers password/key/encrypted-key auth, host-key TOFU and
rejection, PTY shells with resize, `-L`/`-D`/`-R` forwards, jump-host chains,
connection sharing, and a pipelined 24 MiB SFTP round trip.

## What has been verified

- Linux: full test suite, end-to-end suite against OpenSSH, CLI by hand, and
  the desktop app driven under a virtual display (connect, host-key and
  password dialogs, colors/CJK/full-screen apps, a 300k-line output flood,
  broadcast input, file browsing, new folder, rename, delete, every
  right-click menu, host editor, settings (theme, font, log level applied
  live), session logging, import report, shortcuts, reconnect). For 0.3:
  vault create/unlock/remember/auto-login, a changed server password
  recovered through the *update saved password* prompt, auto-reconnect after
  killing the server session (and with the server down, then back), no
  reconnect after `exit`, restored tabs across restarts, splits with
  broadcast, the file browser following `cd`, every host-editor and settings
  section in both themes, key generation (the key logs in with OpenSSH) and
  *Use in host*, known-hosts removal, and settings sync between two
  `HOSTWICK_HOME`s (hosts and theme). Accessibility and languages: every
  language on the main window, host editor, settings and a context menu,
  German and Hindi on every dialog, both themes; keyboard-only use (host
  list, tab bar, Ctrl+F6, Shift+F10 menus, Escape with a focused field);
  dragging split dividers and their sizes plus a following file browser
  coming back after a restart; the proxy test through a local SOCKS5
  proxy and against a closed port; the AT-SPI tree and terminal
  announcements (see *Accessibility and languages*).
- Windows and macOS: not run yet. The code paths are cross-platform and CI
  builds and tests all three OSes; the Windows-only pieces (Pageant/OpenSSH
  agent pipe, registry importers, console VT mode in the CLI) need a first run
  on a real machine.

## Known gaps / roadmap

- Remote-side change detection finer than one second (SFTP reports whole
  seconds).
- Dragging remote files out of the window to the OS file manager (GPUI can
  only hand over files that already exist on disk; local rows work on macOS
  and Wayland).

- Screen readers: no run yet with NVDA/Narrator, VoiceOver or Orca; the
  terminal has no caret-navigable text (use the plain-text view).
- OSC 52 clipboard.
- FTP/S3 adapters behind `RemoteFs`.
- Native file pickers (upload/download in the desktop app) were not exercised
  headless; the transfer engine behind them is tested.
- Installers (MSI/DMG), code signing and signed auto-update.
