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

Sidebar with saved hosts (folders, search, quick connect), terminal and file
tabs, a transfer queue with progress and cancel, and broadcast input to every
open terminal. Host-key, password, passphrase and MFA prompts appear as dialogs.

**Right-click everywhere**

| Where | Menu |
|---|---|
| Saved host | Open terminal, Open files, Edit…, Duplicate, Copy address, Delete… |
| Tab | Reconnect, Duplicate, Open files/terminal, Close, Close others, Close all |
| Terminal | Copy, Paste, Select all, Clear scrollback, Log session to file… / Stop, Open files, Duplicate, Reconnect |
| File | Open, Edit (F4), Upload/Download (F5), Move (F6), Rename… (F2), Delete…, New folder… (F7), New symlink…, Copy path, Refresh, Properties (Alt+Enter) |

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
  a temporary folder under its own name and opened with *Settings → External
  editor* (`{file}` is replaced by the path, otherwise appended; empty = the
  system's default app). Every save is uploaded — changes are detected by
  content hash, not timestamps, so editors that save by rename and time-zone
  differences do not matter. If the server copy changed meanwhile you choose:
  *Overwrite*, *Download theirs* or *Save as copy*. Uploads write into the
  existing file, so its owner and permissions stay. The **Edits** button lists
  open files with their state (watching, uploading, uploaded 12:01, error) and
  stops watching; temporary copies are removed when the tab closes.
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

**Host editor** (New host… or right-click → Edit…): name, folder, host, port,
user, key files (OpenSSH or `.ppk`, with Browse…), jump hosts
(`user@bastion:22, inner`), port forwards (`L8080:localhost:80 R9000:localhost:3000 D1080`,
`-L 8080:…` also accepted), keepalive, terminal type, session log, notes and
legacy algorithms for old devices.

**Settings** (gear icon, `Ctrl+,` / `⌘,`): dark/light theme, terminal font and
size, scrollback, right-click behaviour, copy-on-select, session logging
defaults, app log level (applies immediately) and *Open log folder*.

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
| Ctrl+Shift+N | New host |
| Ctrl+Shift+T | Duplicate current tab |
| Ctrl+Shift+W | Close tab |
| Ctrl+Shift+R | Reconnect |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+Shift+B | Toggle broadcast input |
| Ctrl+, | Settings |
| Ctrl+Shift+C / V, Shift+Insert | Copy / paste in the terminal |
| Shift+PageUp / PageDown | Scroll back |
| Enter on an ended session | Reconnect |

When a connection fails, the error lists every authentication method tried and
why it failed (for example `key work.ppk: wrong passphrase; password: rejected`).

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
  without being asked are refused. X11 forwarding works like OpenSSH's
  untrusted `ssh -X`: the server only sees a random fake cookie, which is
  checked and swapped for the real one locally.
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
installed.

The e2e suite covers password/key/encrypted-key auth, host-key TOFU and
rejection, PTY shells with resize, `-L`/`-D`/`-R` forwards, jump-host chains,
connection sharing, and a pipelined 24 MiB SFTP round trip.

## What has been verified

- Linux: full test suite, end-to-end suite against OpenSSH, CLI by hand, and
  the desktop app driven under a virtual display (connect, host-key and
  password dialogs, colors/CJK/full-screen apps, a 300k-line output flood,
  broadcast input, file browsing, new folder, rename, delete, every
  right-click menu, host editor, settings (theme, font, log level applied
  live), session logging, import report, shortcuts, reconnect).
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

- The desktop app does not yet unlock the vault (the CLI does); it prompts.
- Terminal mouse reporting (for `htop`/`mc` mouse clicks) and OSC 52 clipboard.
- FTP/S3 adapters behind `RemoteFs`.
- Native file pickers (upload/download in the desktop app) were not exercised
  headless; the transfer engine behind them is tested.
- Installers (MSI/DMG), code signing and signed auto-update.
