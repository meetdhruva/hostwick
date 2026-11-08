# Hostwick desktop: English (source language).
#
# Keys are grouped by the part of the app that shows them. Placeholders
# are named ({ $name }); counts select plural forms ({ $count -> ... }).
# Keep product and protocol names (SSH, SFTP, SCP, sudo, Kerberos, PuTTY,
# WinSCP, X11, SOCKS) as they are in every language.

## General

error-internal = internal error: { $error }

## Main window

sidebar-search-placeholder = Search hosts
split-only-terminals = Only terminal tabs can be split.
files-follow-no-folder = The terminal has not reported its folder yet. Turn on “Shell integration” in Edit host → Terminal so the file browser can follow it (bash and zsh).
host-copy-name = { $name } (copy)
size-b = B
size-kib = KiB
size-mib = MiB
size-gib = GiB
size-tib = TiB
protocol-ssh = SSH
protocol-telnet = Telnet
protocol-serial = Serial
sidebar-vault-unlocked-tooltip = Saved passwords are unlocked — click to lock
sidebar-settings-tooltip = Settings ({ $keys })
sidebar-open-terminal = Terminal
sidebar-open-files = Files
sidebar-edit-host = Edit host
sidebar-delete-host = Delete host
sidebar-new-host = New host…
tab-files-title = Files: { $name }
broadcast = Broadcast
broadcast-on = Broadcast: ON
broadcast-tooltip = Send keystrokes to every open terminal ({ $keys })
shortcut-new-host = new host
shortcut-duplicate-tab = duplicate tab
shortcut-close-pane = close pane/tab
shortcut-reconnect = reconnect
shortcut-next-tab = next tab
shortcut-split-right = split right
shortcut-split-down = split down
shortcut-move-between-panes = move between panes
shortcut-broadcast = broadcast
shortcut-find = find
shortcut-settings = settings
empty-title = No open sessions
empty-hint-connect = Double-click a host, or type user@host in the quick-connect box.
empty-hint-menus = Right-click hosts, tabs, files and terminals for more actions.
empty-hint-import = Import from ~/.ssh/config, PuTTY, WinSCP or Solar-PuTTY with Import… (bottom left).
pane-connecting = Connecting to { $host }…
pane-retry = Retry
transfer-progress = { $done } of { $total } — { $rate }/s
transfer-done = done — { $size }
transfer-skipped = skipped
transfer-cancelled = cancelled
transfer-failed = failed: { $error }
transfer-cancel = Cancel
transfers-title = { $count ->
    [one] Transfers — { $count } active
   *[other] Transfers — { $count } active
}
transfers-clear = Clear finished

## Common buttons

button-cancel = Cancel
button-delete = Delete
button-ok = OK
button-save = Save
button-close = Close
button-browse = Browse…

## Menus, import

host-delete-title = Delete host
host-delete-confirm = Delete the saved host “{ $name }”?
sidebar-import = Import…
sidebar-tools = Tools
import-no-home = no home directory
import-pick-putty = PuTTY .reg export
import-pick-solar = Solar-PuTTY export (.json)
import-pick-winscp = WinSCP.ini
import-failed = Import failed: { $error }
import-summary = { $added } added, { $updated } updated, { $notes } with notes
import-done = Imported: { $summary }
import-report-title = Import report
import-report-attention = Needs attention ({ $count })
import-report-added = Added ({ $count })
import-report-updated = Updated ({ $count })
import-report-note = Hosts listed under “Needs attention” were still imported unless the note says skipped. Right-click a host → Edit… to adjust it.
import-menu-ssh-config = OpenSSH ~/.ssh/config
import-menu-putty-registry = PuTTY (this PC's saved sessions)
import-menu-winscp-registry = WinSCP (this PC's saved sites)
import-menu-putty-file = PuTTY .reg export file…
import-menu-winscp-file = WinSCP.ini file…
import-menu-solar-file = Solar-PuTTY .json export…
menu-open-terminal = Open terminal
menu-open-files = Open files (SFTP)
menu-edit = Edit…
menu-duplicate = Duplicate
menu-copy-address = Copy address
menu-delete = Delete…
menu-reconnect = Reconnect
menu-split-right = Split right
menu-split-down = Split down
menu-send-break = Send break
menu-follow-terminal = Follow terminal folder
menu-close = Close
menu-close-others = Close others
menu-close-all = Close all
tools-generate-key = Generate key…
tools-inspect-key = Inspect or convert key…
tools-known-hosts = Known hosts…
tools-saved-passwords = Saved passwords…
tools-settings-sync = Settings sync…
button-continue = Continue

## Connection prompts

notice-connection-title = Connection
hostkey-changed-title = Host key changed!
hostkey-unknown-title = Unknown host
hostkey-changed-body = WARNING: the host key for { $host } has CHANGED.

    This can mean someone is intercepting the connection (man-in-the-middle), or the server was reinstalled.

    New { $algorithm } key:
    { $fingerprint }

    Only continue if you know the key was changed on purpose.
hostkey-unknown-body = This is the first connection to { $host }.

    { $algorithm } key fingerprint:
    { $fingerprint }

    Verify it with the server's administrator before trusting it.
hostkey-reject = Reject
hostkey-trust-once = Trust once
hostkey-replace = Replace saved key
hostkey-trust-remember = Trust and remember
remember-hint-unlocked = Saved encrypted in your password vault.
remember-hint-locked = You'll be asked for your vault's master password.
remember-hint-create = You'll create a password vault with a master password.
prompt-server-auth-title = Server authentication
prompt-password-title = Password for { $user }@{ $host }
prompt-password-label = Password
prompt-passphrase-label = Passphrase
prompt-sudo-title = sudo password for { $who }
prompt-sudo-wrong-title = Wrong password. sudo password for { $who }
secret-rejected-password = The saved password was not accepted. Type the current one.
secret-rejected-proxy = The saved proxy password was not accepted. Type the current one.
secret-rejected-passphrase = The saved passphrase was not accepted. Type the current one.
secret-rejected-sudo = sudo did not accept the saved password.
secret-update-password = Update the saved password
secret-update-proxy = Update the saved proxy password
secret-update-passphrase = Update the saved passphrase
secret-update-sudo = Update the saved sudo password
secret-remember-password = Remember password
secret-remember-proxy = Remember proxy password
secret-remember-passphrase = Remember passphrase
secret-remember-sudo = Remember sudo password
vault-reason-host = { $name } has saved passwords. Unlock them with your master password.
vault-reason-store = Unlock your saved passwords to store this one.
vault-reason-create = Create a vault for saved passwords. One master password protects them; it is never stored.
notice-not-saved-locked = Not saved: the password vault is locked.
notice-save-password-failed = Could not save the password: { $error }
notice-update-host-failed = Could not update { $name }: { $error }
files-need-ssh = Files need SSH: { $name } is a { $protocol } connection, which cannot transfer files.
files-root-failed = Could not open as root: { $error }

## Settings sync

sync-no-changes = no changes here
sync-added = { $count } added
sync-updated = { $count } updated
sync-removed = { $count } removed
sync-settings-updated = settings updated
sync-mode-sync = Sync
sync-mode-push = Push
sync-mode-pull = Pull
sync-summary = { $mode }: { $count ->
    [one] { $count } host in sync
   *[other] { $count } hosts in sync
}; { $changes }
sync-file-updated = sync file updated
vault-reason-sync-startup = Unlock your saved passwords to sync settings (the sync passphrase is kept there).
vault-reason-sync = Unlock your saved passwords to sync settings.
sync-no-folder = Choose a sync folder in Settings → Settings sync first.
sync-no-passphrase = no sync passphrase saved — set it in Settings → Settings sync
sync-passphrase-title = Settings sync passphrase
sync-passphrase-label = Sync passphrase
sync-passphrase-wrong-title = Wrong sync passphrase — enter the one your other computers use
sync-wrong-passphrase = wrong sync passphrase
sync-last-failed = failed: { $error }
sync-done = Settings sync — { $summary }
sync-failed = Settings sync failed: { $error }

## Password vault

cred-sync-passphrase = Settings sync passphrase
cred-login-password = login password
cred-sudo-password = sudo password
cred-proxy-password = proxy password
cred-key-passphrase = passphrase for { $file }
cred-key-unused = Key passphrase — { $path } (no host uses it)
cred-host-deleted = Unused (its host was deleted)
strength-too-short = Too short — use at least 8 characters
strength-strong-passphrase = Strong passphrase
strength-weak = Weak — longer is better (a few words work well)
strength-fair = Fair
strength-strong = Strong
vault-locked-error = the vault is locked
vault-current-wrong = The current master password is wrong.
vault-current-master = Current master password
vault-master = Master password
vault-new-master = New master password
vault-repeat = Repeat it
vault-enter-password = Enter a password.
vault-too-weak = Use at least 8 characters — a few unrelated words work well.
vault-mismatch = The two passwords do not match.
vault-wrong-master = Wrong master password.
vault-strength-empty = At least 8 characters; a few unrelated words work well.
vault-never-stored = The master password is never stored. If you forget it, saved passwords cannot be recovered.
vault-unlocking = Unlocking…
vault-encrypting = Encrypting…
vault-path = Vault: { $path } (shared with the hostwick command)
vault-title-unlock = Unlock saved passwords
vault-title-create = Create a password vault
vault-title-change = Change master password
vault-not-now = Not now
vault-unlock = Unlock
vault-create = Create vault
vault-change = Change

## Host editor

form-forward-needs-spec = `{ $token }` needs a port specification
form-jump-invalid = jump host `{ $host }`: { $error }
form-port-invalid = { $field } must be a number from 1 to 65535
form-baud-invalid = Speed (baud) must be a number such as 9600 or 115200
form-bits-invalid = Data bits must be 5–8 and stop bits 1 or 2
host-section-general = General
host-section-auth = Authentication
host-section-connection = Connection
host-section-terminal = Terminal
host-section-files = Files
host-section-advanced = Advanced
host-folder-placeholder = prod/web (optional)
host-host-placeholder = 10.0.0.5 or host.example.com
host-user-placeholder = deploy (empty = your local user name)
host-keepalive-placeholder = seconds, 0 = off
host-optional-placeholder = optional
host-x11-placeholder-windows = empty = localhost:0 (VcXsrv, Xming)
host-x11-placeholder = empty = $DISPLAY
host-script-example = Example:
host-snippets-empty = No snippets for this host. Global snippets are in Settings → Snippets.
host-log-placeholder = empty = off, or e.g. &S-&Y&M&D-&T.log
host-sftp-server-placeholder = empty = detect (/usr/lib/openssh/sftp-server, …)
host-error-no-device = Choose the serial port (device)
host-error-no-host = Enter the host name or address
host-error-no-auth = Choose at least one way to log in
host-error-forwards = Port forwards: { $error }
host-error-keepalive = Keepalive must be seconds (0 to 3600)
host-error-proxy-host = Enter the proxy's host name
host-error-x11 = X11 display: { $error }
host-error-script = Login script, { $error }
host-saved-forget-pending = Will be forgotten when you save: { $what }
host-saved-locked = Saved in your vault (locked): { $what }
host-saved = Saved in your vault: { $what }
host-saved-missing = No longer in your vault, you'll be asked: { $what }
host-saved-keep = Keep
host-saved-forget = Forget
host-what-password = password
host-what-key-passphrase = key passphrase
host-what-proxy-password = proxy password
host-what-sudo-password = sudo password
host-pick-key = Private key (OpenSSH or .ppk)
host-name = Name
host-folder = Folder
host-protocol = Protocol
host-host = Host
host-port = Port
host-user = User
host-notes = Notes
host-telnet-hint = Telnet sends everything unencrypted; use it for network gear on a trusted network. The user name is sent if the server asks for it.
host-ports-looking = Looking…
host-ports-none = No ports found
host-ports-count = Ports ({ $count })
host-ports-none-menu = No serial ports found — type the device name
host-device = Port (device)
host-ports-refresh = Look for serial ports again
host-device-hint-windows = COM3, COM4… Pick one or type its name.
host-device-hint = /dev/ttyUSB0, /dev/ttyACM0, /dev/serial/by-id/… Pick one or type its path.
host-baud = Speed (baud)
host-data-bits = Data bits
host-parity = Parity
host-parity-none = None
host-parity-odd = Odd
host-parity-even = Even
host-stop-bits = Stop bits
host-flow = Flow control
host-flow-none = None
host-serial-none-linux = No ports found. USB adapters appear as /dev/ttyUSB*; you may need to be in the dialout group.
host-serial-hint = Most consoles use 9600 8N1 or 115200 8N1 without flow control.
host-auth-not-ssh = Telnet and serial connections log in inside the terminal. Use a login script (Terminal) to automate it.
host-key-files = Key files
host-keys-hint = OpenSSH or PuTTY .ppk keys, comma-separated. A certificate next to a key (id_ed25519-cert.pub) is used automatically. Security keys (sk-…) work through the agent.
host-auth-agent = Keys in the SSH agent (Pageant, ssh-agent, 1Password…)
host-auth-password = Password
host-auth-remember-hint = Tick “Remember password” when asked to log in automatically next time.
host-auth-kbd = Keyboard-interactive (one-time codes, MFA)
host-auth-kerberos = Kerberos (single sign-on)
host-auth-kerberos-forward = Forward my Kerberos ticket to the server
host-auth-kerberos-hint-windows = Uses your Windows domain login.
host-auth-kerberos-hint = Uses the ticket from kinit (see klist). Without one, the next method is tried.
host-auth-order = Methods are tried in this order: Kerberos, agent, key files, password, keyboard-interactive.
host-connection-serial = Serial ports are local: no proxy, jump hosts or forwards.
host-proxy = Proxy
host-proxy-none = None
host-proxy-host-port = Proxy host / port
host-proxy-port = Proxy port
host-proxy-user = Proxy user
host-proxy-password-hint = The proxy password is asked for when connecting (tick Remember to save it).
host-jump-hosts = Jump hosts
host-jump-hint = Comma-separated, outermost first (like ssh -J).
host-forwards = Port forwards
host-forwards-hint = L = local → remote, R = remote → local, D = SOCKS proxy. Started with the terminal.
host-keepalive = Keepalive (s)
host-agent-forwarding = Forward my SSH agent (git on the server uses my keys)
host-agent-forwarding-hint = Only on servers you trust: their admins can use your keys while you are connected.
host-x11 = Forward X11 (show remote GUI programs here)
host-x11-display = X display
host-x11-hint-windows = Needs an X server such as VcXsrv or Xming running.
host-x11-hint-macos = Needs XQuartz.
host-x11-hint = Uses your desktop's X server (or XWayland).
host-terminal-type = Terminal type
host-login-script = Login script
host-login-script-hint = One line per command, typed after login. @wait TEXT waits for output (10 s), @wait 30000 TEXT for longer, @sleep 500 pauses; # starts a comment.
host-shell-integration = Shell integration (bash, zsh)
host-shell-integration-hint = Lets the file browser follow the terminal's folder (right-click → Open files here). Adds a prompt hook quietly when the shell starts; nothing is installed on the server.
host-snippets-title = Snippets for this host
host-session-log = Session log
host-session-log-hint = Tokens: &S session, &H host, &P port, &U user, &Y&M&D date, &T time. Relative paths go to the log folder.
host-raw-log = Log raw output (with escape codes)
host-files-not-ssh = The file browser needs SSH.
host-file-protocol = File protocol
host-file-protocol-auto = Auto
host-file-protocol-hint = Auto uses SFTP and falls back to SCP on servers without it (routers, ESXi, busybox).
host-sudo = Open the file browser as root (sudo)
host-sudo-user = As user
host-sudo-hint = Works with passwordless sudo or a sudo password (asked for, or saved). “Reopen as root” in the browser switches at any time.
host-legacy = Allow legacy algorithms (ssh-rsa/SHA-1, old key exchange) for old devices
host-legacy-hint = Only for old switches, routers and appliances that need them.
host-id = Host id: { $id }
host-title-edit = Edit host
host-title-new = New host
loading = Loading…

## Settings

settings-title = Settings
settings-section-appearance = Appearance
settings-section-terminal = Terminal
settings-section-snippets = Snippets
settings-section-connections = Connections
settings-section-files = Files
settings-section-logging = Session logging
settings-section-passwords = Saved passwords
settings-section-sync = Settings sync
settings-section-diagnostics = Diagnostics
settings-language = Language
settings-language-system = System ({ $language })
settings-language-hint = Applies as soon as you save. Dialogs that are open keep their language until they are opened again.
settings-theme = Theme
settings-theme-dark = Dark
settings-theme-light = Light
settings-terminal-font = Terminal font
settings-font-size = Font size
settings-font-placeholder = empty = system monospace
settings-font-hint = Font changes apply to open terminals when you save.
settings-editor-placeholder = empty = the system's default app (e.g. code -w { "{file}" })
settings-auto-lock-placeholder = 0 = keep unlocked until Hostwick quits
settings-sync-folder-placeholder = OneDrive, Dropbox, iCloud Drive or git folder
settings-sync-pass-saved = Saved in your vault — type to change
settings-sync-pass-placeholder = Same passphrase on every computer
settings-snippets-empty = No snippets yet. Snippets appear as buttons above every terminal and under right-click → Snippets.
settings-remove-failed = Could not remove it: { $error }
settings-error-font-size = Terminal → Font size must be a number from 6 to 48
settings-error-scrollback = Terminal → Scrollback must be a whole number from 100 to 1000000
settings-error-auto-lock = Saved passwords → Lock after must be minutes (0 to 1440)
settings-error-rules = Terminal → { $error }
settings-error-sync-folder = Settings sync → the folder { $folder } does not exist
settings-folder-missing = The folder { $folder } does not exist.
settings-sync-pass-short = Use a sync passphrase of at least 8 characters.
settings-sync-pass-save-failed = Could not save the passphrase: { $error }
vault-reason-sync-passphrase = The sync passphrase is kept in your password vault.
settings-save-failed = Cannot save settings: { $error }
settings-scrollback = Scrollback lines
settings-right-click = Right-click
settings-right-click-menu = Show menu
settings-right-click-paste = Paste (PuTTY)
settings-copy-on-select = Copy selected text automatically
settings-mouse = Send mouse clicks and scrolling to programs (htop, mc, vim)
settings-mouse-hint = Hold Shift to select text while a program uses the mouse.
settings-button-bar = Show the snippet button bar above terminals
settings-highlighting = Keyword highlighting
settings-highlighting-hint = Colours matching text on screen. Earlier rules win where they overlap.
settings-snippets-hint = Available in every terminal. Hosts can add their own in Edit host → Terminal.
settings-auto-reconnect = Reconnect automatically when a connection drops
settings-auto-reconnect-hint = SSH and telnet: after 1, 2, 5, 10, then every 30 seconds, keeping the scrollback. Not after you type exit, nor after a login or host-key failure.
settings-restore-tabs = Reopen my tabs when Hostwick starts
settings-restore-tabs-hint = Including split panes and their sizes. Tabs connect when you first switch to them.
settings-editor = External editor
settings-editor-pick = Editor program
settings-editor-hint = Used by Edit (F4). { "{file}" } is replaced by the file's path, otherwise the path is added at the end. Use the editor's wait flag (code -w, subl -w) if it has one.
settings-log-all = Log every session to a file
settings-log-folder = Log folder
settings-log-content = Log content
settings-log-printable = Printable text
settings-log-raw = Raw (with escape codes)
settings-log-hint = A host's own session-log setting (Edit host → Terminal) overrides these.
settings-log-level = App log level
settings-open-log-folder = Open log folder
settings-data-folder = Data folder: { $path }
settings-vault-none = No vault yet. Tick “Remember” when Hostwick asks for a password, or create one now.
settings-vault-locked = Locked. Saved passwords are used after you unlock them.
settings-vault-unlocked = Unlocked for this session.
settings-vault-unlock = Unlock…
settings-vault-create = Create vault…
settings-vault-lock = Lock now
settings-vault-change = Change master password…
settings-vault-empty = Nothing saved yet.
settings-vault-remove = Remove
settings-auto-lock = Lock after (min)
settings-auto-lock-hint = Idle minutes before saved passwords are locked again; 0 keeps them unlocked until Hostwick quits.
settings-vault-file = Encrypted with Argon2id + XChaCha20-Poly1305 in { $path } — the same vault as `hostwick vault`. Never synced.
settings-sync-never = Never synced on this computer.
settings-sync-intro = Hosts and settings go into one encrypted file in a folder your cloud drive or git keeps in step. Saved passwords never leave this computer.
settings-sync-folder = Folder
settings-sync-folder-pick = Sync folder
settings-sync-passphrase = Passphrase
settings-sync-passphrase-hint = Kept in your password vault. Every computer needs the same one.
settings-sync-auto = Sync at startup, after changes and every 5 minutes
settings-sync-now = Sync now
settings-sync-push = Replace sync file
settings-sync-push-tooltip = Make the sync file match this computer (also changes its passphrase)
settings-sync-pull = Replace this computer's
settings-sync-pull-tooltip = Make this computer's hosts and settings match the sync file
settings-sync-last = Last sync

## Snippet and highlight-rule editors

snippet-label-placeholder = Button label
snippet-text-placeholder = Text to send (several lines are pasted as one block)
snippet-press-enter = Press Enter
snippet-move-up = Move up
snippet-move-down = Move down
editor-remove = Remove
snippet-add = Add snippet
rule-error-empty = empty pattern
rule-error-color = colour must look like #ff8800, not `{ $color }`
rule-error-regex = bad regular expression
rule-pattern-placeholder = word or regular expression
rule-error = Highlight `{ $pattern }`: { $error }
rule-enabled = Enabled
rule-regex-on = Regular expression (click for a plain word)
rule-regex-off = Plain word, any case (click for a regular expression)
rule-add = Add rule
rule-defaults = Restore defaults

## Terminal

script-timeout = waited { $after } for “{ $text }”, which never appeared
script-session-ended = the session ended first
search-too-complex = pattern too complex
term-exit-status = exited with status { $code }
term-exit-signal = killed by signal { $signal }
term-connection-lost = connection lost
term-connection-closed = connection closed
term-reconnected-marker = reconnected { $time }
term-reconnect-failed = reconnect failed: { $error }
term-break-failed = Could not send break
term-no-break = { $protocol } sessions have no break signal
term-script-not-run = Login script not run: { $error }
term-script-stopped = Script stopped: { $error }
search-placeholder = Find in scrollback
search-no-results = No results
search-position = { $index } of { $total }
search-matches = { $count ->
    [one] { $count } match
   *[other] { $count } matches
}
search-match-case = Match case
search-regex = Regular expression
search-older = Previous (older) match — Enter, F3
search-newer = Next (newer) match — Shift+Enter, Shift+F3
search-close = Close (Esc)
term-reconnect-in = Connection lost{ $reason }. Reconnecting in { $seconds } s…
term-reconnect-attempt = Reconnecting (attempt { $attempt })…
term-log-badge = LOG
term-reconnect-now = Now
term-session-ended = Session ended — { $reason }. Press Enter to reconnect.
term-menu-copy = Copy
term-menu-paste = Paste
term-menu-select-all = Select all
term-menu-find = Find…
term-menu-snippets = Snippets
term-menu-copy-folder = Copy current folder
term-menu-stop-script = Stop script
term-menu-clear = Clear scrollback
term-menu-stop-log = Stop session log
term-menu-start-log = Log session to file…
term-menu-files-here = Open files here
term-menu-duplicate = Duplicate session
term-menu-reconnect-ended = Reconnect (session ended)

## File browser

files-move = Move
files-upload = Upload
files-download = Download
files-move-confirm = { $count ->
    [one] Move “{ $name }” to { $dir }?
   *[other] Move { $count } items to { $dir }?
}
files-upload-confirm = { $count ->
    [one] Upload “{ $name }” to { $dir }?
   *[other] Upload { $count } items to { $dir }?
}
files-download-confirm = { $count ->
    [one] Download “{ $name }” to { $dir }?
   *[other] Download { $count } items to { $dir }?
}
files-same-place = { $name }: source and destination are the same
files-skipped-unsafe = Not shown, because their names are unsafe on this computer: { $names }
files-not-removed = { $name }: copied, but not removed: { $error }
files-pick-download = Download here
files-exists = “{ $name }” already exists
files-delete-title = Delete
files-delete-folder-confirm = Permanently delete the folder “{ $name }” and everything in it?
files-delete-file-confirm = Permanently delete the file “{ $name }”?
files-delete-many-confirm = Permanently delete { $count } items (folders with everything in them)?
files-name-invalid = Enter a name without “/”
files-new-folder = New folder
files-create = Create
files-folder-name = Folder name
files-rename = Rename
files-new-name = New name
files-new-symlink = New symlink
files-symlink-placeholder = target -> link name
files-symlink-format = Write it as “target -> link name”
files-symlink-empty = The target is empty
edit-conflict-title = Changed on the server
edit-conflict-body = “{ $name }” was changed on the server since you opened it. What should happen with your saved version?
edit-conflict-later = Decide later
edit-conflict-theirs = Download theirs
edit-conflict-copy = Save as copy
edit-conflict-overwrite = Overwrite
edit-editor-failed = could not start the editor “{ $program }”: { $error }
keep-up-to-date = up to date
keep-synced = { $count ->
    [one] synced { $count } file, { $size }
   *[other] synced { $count } files, { $size }
}
keep-failed = { $count } failed
keep-error = error (retrying): { $error }
keep-starting = starting…
keep-status = Keeping { $local } → { $remote } up to date: { $status }
edits = Edits
edits-busy = Edits ({ $count }): { $status }
edits-tooltip = Files open in an editor; saves upload automatically
edits-stop = Stop watching
edits-stop-all = Stop watching all
files-hide-local = Hide local
files-show-local = Show local
files-local-tooltip = Show or hide the local computer's files
files-upload-pick = Upload…
files-synchronize = Synchronize…
files-synchronize-tooltip = Compare folders, review the changes, then run them
files-keep-stop = Stop keeping up to date
files-keep = Keep up to date…
files-keep-tooltip = Upload changes from a local folder into a remote folder automatically
files-following = Following terminal
files-follow = Follow terminal
files-follow-tooltip = Show the folder the terminal “{ $terminal }” is in, as it changes
files-root-switching = Switching…
files-root-on = Root: on
files-root-reopen = Reopen as root
files-root-tooltip = Browse the server as root (or the host's sudo user) through sudo
files-path-placeholder = /path
files-this-computer = This computer
files-selected-of = { $selected } of { $count } selected ({ $size })
files-item-count = { $count ->
    [one] { $count } item
   *[other] { $count } items
}
files-selected = { $count ->
    [one] { $count } item
   *[other] { $count } items
}
files-root-badge = ROOT
files-up-tooltip = Up (Backspace)
files-refresh-tooltip = Refresh (Ctrl+R)
files-mkdir-tooltip = New folder (F7)
files-col-name = Name
files-col-size = Size
files-col-modified = Modified
files-col-mode = Mode
files-download-pick = Download…
files-menu-open = Open
files-menu-edit = Edit
files-menu-move = Move to other side
files-menu-rename = Rename…
files-menu-new-folder = New folder…
files-menu-new-symlink = New symlink…
files-menu-copy-path = Copy path
files-menu-refresh = Refresh
files-menu-properties = Properties
edit-status-downloading = downloading…
edit-status-watching = watching
edit-status-uploading = uploading…
edit-status-uploaded = uploaded { $time }
edit-status-uploaded-in-place = uploaded { $time } (rewritten in place)
edit-status-conflict = changed on the server — waiting for you
edit-status-error = error: { $error }
edit-gone = { $path }: no longer exists
edit-kept-folder = Edited files that were not uploaded were kept in { $path }
edit-is-folder = { $path } is a folder
edit-missing-after-upload = { $path }: missing after upload
edit-size-mismatch = { $path }: server has { $server } bytes after uploading { $uploaded }
edit-no-copy-name = no free name for the copy

## File properties

props-title = Properties
props-applying = Applying…
props-apply = Apply
props-calculating = calculating…
props-error = error: { $error }
props-usage = { $size } ({ $bytes } bytes) in { $files ->
    [one] { $files } file
   *[other] { $files } files
}, { $folders ->
    [one] { $folders } folder
   *[other] { $folders } folders
}
props-usage-partial = (stopped early: too many entries)
props-bad-id = “{ $id }” is not a numeric id
props-bad-octal = Permissions must be octal, e.g. 0644
props-kind-folder = Folder
props-kind-file = File
props-kind-symlink = Symbolic link → { $target }
props-kind-special = Special file
props-unknown-name = (unknown name)
props-not-reported = not reported by the server
props-read = Read
props-write = Write
props-execute = Execute
props-owner = Owner
props-group = Group
props-others = Others
props-special = Special
props-octal = Octal
props-calculate = Calculate
props-size = { $size } ({ $bytes } bytes)
props-location = Location
props-type = Type
props-checksum = Checksum
props-unknown = unknown
props-permissions = Permissions
props-recursive = Apply to everything inside (files keep their execute bits)

## Synchronize dialog

files-synchronize-title = Synchronize
sync-keep = Keep remote up to date
sync-keep-tooltip = Watch the local folder and upload every change (local → remote only)
sync-comparing = Comparing…
sync-preview = Preview
sync-run-checked = Synchronize checked
sync-exclude-placeholder = e.g. .git, *.tmp, node_modules
sync-choose-folders = Choose both folders
sync-use-local = Use local
sync-use-remote = Use remote
sync-total-uploads = { $count ->
    [one] { $count } upload, { $size }
   *[other] { $count } uploads, { $size }
}
sync-total-downloads = { $count ->
    [one] { $count } download, { $size }
   *[other] { $count } downloads, { $size }
}
sync-total-deletions = { $count ->
    [one] { $count } deletion
   *[other] { $count } deletions
}
sync-total-conflicts = { $count ->
    [one] { $count } conflict — pick a side or leave it
   *[other] { $count } conflicts — pick a side or leave them
}
sync-in-sync = Both folders are already in sync.
sync-checked = Checked: { $items }
sync-local-folder = Local folder
sync-remote-folder = Remote folder
sync-direction = Direction
sync-dir-up = Local → remote
sync-dir-down = Remote → local
sync-dir-both = Both (newer wins)
sync-mirror = Delete files missing from the source (mirror)
sync-exclude = Exclude
sync-comparing-folders = Comparing folders…
sync-result = Synchronize: { $summary }
sync-running = Synchronizing…
sync-run-failed = Synchronize failed: { $error }
sync-kind-upload = Upload
sync-kind-download = Download
sync-kind-delete-local = Delete local
sync-kind-delete-remote = Delete remote
sync-kind-conflict = Conflict
sync-kind-skip = Skip
sync-reason-new = new
sync-reason-newer = newer
sync-reason-size = size differs
sync-reason-differs = differs
sync-reason-not-in-source = not in source
sync-reason-both-changed = changed on both sides (same time, different size)
sync-reason-unknown = cannot compare (no timestamp)
sync-reason-destination-newer = destination is newer
sync-reason-new-folder = new folder
sync-reason-case-conflict = another file differs only in letter case
sync-review-deletions = This synchronization deletes files. Check the list below, then press Synchronize again.
sync-report-uploaded = { $count } uploaded
sync-report-downloaded = { $count } downloaded
sync-report-deleted = { $count } deleted
sync-report-folders = { $count ->
    [one] { $count } folder created
   *[other] { $count } folders created
}
sync-report-failed = { $count } failed
sync-report-nothing = nothing to do

## Key tools

keyformat-openssh-hint = For OpenSSH, Hostwick, Git and most tools (a .pub file is written too).
keyformat-ppk3-hint = For PuTTY 0.75+ and WinSCP (Argon2-protected).
keyformat-ppk2-hint = For older PuTTY, WinSCP and FileZilla versions.
key-fingerprint = Fingerprint: { $fingerprint }
keygen-title = Generate key
keygen-generate = Generate
keygen-passphrase-placeholder = Passphrase (recommended; empty = none)
keygen-repeat = Repeat
keygen-mismatch = The passphrases do not match.
keygen-no-path = Choose where to save the key.
keygen-exists = { $path } already exists. Choose another name, or tick “Overwrite”.
keygen-used-in-host = { $name } now logs in with { $path }. Add the public key to the server's ~/.ssh/authorized_keys.
keygen-ed25519-recommended = Ed25519 (recommended)
keygen-saved = Saved { $file } (private key) and { $file }.pub in { $dir }
keygen-public-key = Public key (for ~/.ssh/authorized_keys):
keygen-copy-public = Copy public key
keygen-use-in-host = Use in host…
keygen-type = Type
keygen-comment = Comment
keygen-format = Format
keygen-save-as = Save as
keygen-overwrite = Overwrite an existing file
keygen-generating-rsa = Generating… RSA 4096 can take a few seconds.
keygen-generating = Generating…
keyconv-title = Inspect or convert key
keyconv-path-placeholder = ~/.ssh/id_ed25519 or C:\keys\work.ppk
keyconv-current-placeholder = Current passphrase (if it has one)
keyconv-new-placeholder = New passphrase (empty = none)
keyconv-repeat-placeholder = Repeat new passphrase
keyconv-output = Output file
keyconv-mismatch = The new passphrases do not match.
keyconv-pass-changed = Passphrase changed ({ $format }).
keyconv-pass-removed = Passphrase removed ({ $format }).
keyconv-wrote = Wrote { $path } ({ $format }).
keyconv-needs-passphrase = This key has a passphrase: type it under “Current passphrase”.
keyconv-wrong-passphrase = Wrong passphrase.
keyconv-exists = { $path } already exists; choose another output file.
keyconv-key-file = Key file
keyconv-type-bits = { $kind } { $bits } bits ({ $algorithm })
keyconv-protected = Protected
keyconv-public-only = public key only
keyconv-encrypted = yes, with a passphrase
keyconv-no-passphrase = no passphrase
keyconv-fingerprint = Fingerprint
keyconv-inspect = Inspect
keyconv-current = Current
keyconv-new = New passphrase
keyconv-change = Change passphrase
keyconv-convert-to = Convert to
keyconv-convert = Convert
keyconv-keeps-passphrase = Keeps the current passphrase unless you type a new one.
keyconv-working = Working…
kh-title = Known hosts
kh-search-placeholder = Search host, key type or fingerprint
kh-forget-placeholder = host or host:port
kh-nothing-matched = Nothing matched; the file is unchanged.
kh-removed = { $count ->
    [one] Removed { $count } key.
   *[other] Removed { $count } keys.
}
kh-type-host = Type a host name, or host:port.
kh-col-line = Line
kh-col-hosts = Host(s)
kh-empty = No known hosts yet. Keys are added when you trust a server.
kh-no-match = Nothing matches the search.
kh-reload = Reload
kh-remove-selected = Remove selected ({ $count })
kh-forget-host = Forget host
kh-forget-button = Remove all its keys
kh-footer = { $path } — { $shown } of { $total } shown. Hashed names are matched when you forget a host by name.
files-opening-root = opening files as root
files-opening = opening the file browser

## Keyboard and screen readers

sidebar-vault-lock = Lock saved passwords
sidebar-quick-connect = Quick connect (user@host:port, Enter)
sidebar-hosts-label = Saved hosts
sidebar-hosts-keys = Arrow keys move (Shift+arrows select several), Left and Right fold folders. Enter opens a terminal, Ctrl+Enter the files, Shift+Enter a new split, Alt+Enter edits, F2 renames, Delete removes, Ctrl+A selects all, Shift+F10 opens the menu.
tabs-label = Open sessions
tabs-keys = Left and Right switch tabs, Enter goes into the tab, Delete closes it, Shift+F10 opens its menu. Ctrl+F6 moves between the host list, the tabs and the session. Ctrl+Shift+Left and Right move the tab.
tab-state-not-connected = not connected yet
tab-state-connected = connected
tab-state-disconnected = disconnected
tab-spoken = { $title }, { $state }, tab { $index } of { $count }
tab-close = Close { $title }
pane-divider = Pane divider

## Proxy test

vault-reason-proxy-test = Unlock your saved passwords to use the saved proxy password.
proxy-test-closed = The proxy connected, but { $target } closed the connection at once.
proxy-test-ok-ssh = Connected to { $target } through the proxy. The server says: { $server }
proxy-test-ok = Connected to { $target } through the proxy (the server is not an SSH server).
proxy-test-ok-quiet = Connected to { $target } through the proxy (the server has not said anything yet).
proxy-test-read-failed = Connected to { $target } through the proxy, then: { $error }
host-proxy-test = Test connection
host-proxy-testing = Testing…
host-proxy-test-tooltip = Connect through the proxy to this host's port (the first jump host, if any) and report the result
host-baud-common = Common speeds
dismiss = Dismiss
term-no-output = No output yet.
term-text-view-title = Terminal output: { $title }
term-text-view-hint = { $lines ->
    [one] { $lines } line. Read it with the arrow keys; Escape goes back to the terminal.
   *[other] { $lines } lines. Read them with the arrow keys; Escape goes back to the terminal.
}
term-a11y-label = Terminal: { $title }
term-a11y-keys = New output is read out when it settles. { $read } reads the latest output again, { $view } shows all output as text. Ctrl+F6 leaves the terminal.
term-menu-read-output = Read latest output
term-menu-text-view = Show output as text…
files-path-label = Folder: { $side }
files-list-label = { $side }: { $dir }
files-list-keys = Arrow keys move, Shift+arrows extend the selection, Ctrl+Space or Insert adds or removes a row, Space previews the file, typing filters the list, Enter opens, Backspace goes up, Ctrl+L types a path, Ctrl+D bookmarks the folder, F5 copies to the other side, F6 moves, F2 renames, Delete deletes, Alt+Enter shows properties, Tab switches sides, Shift+F10 opens the menu.
files-kind-link = link
a11y-sections = Sections
a11y-section-problem = { $section } (needs attention)
kh-row-label = Line { $line }: { $hosts }
rule-color = Colour (#rrggbb)
toast-info = Information
toast-success = Done
toast-warning = Warning
toast-error = Error
toast-close = Close notification
menu-close-pane = Close pane
pane-files-title = Files: { $host }

## Command palette, sidebar, bulk edit, welcome (0.4)

prompt-passphrase-title-host = Passphrase for { $path } ({ $host })
prompt-server-auth-title-host = Server authentication — { $host }
act-broadcast = Turn broadcast input on or off
act-new-folder = New folder
act-quick-connect = Quick connect…
act-settings-section = Settings: { $section }
act-toggle-theme = Switch between dark and light theme
bulk-applies = { $count ->
    [one] Applies to { $count } host
   *[other] Applies to { $count } hosts
}
bulk-apply = Apply
bulk-changed = will change
bulk-done = { $count ->
    [one] Updated { $count } host
   *[other] Updated { $count } hosts
}
bulk-error-proxy = Proxy: { $error }
bulk-intro = { $count ->
    [one] Change the settings of { $count } host.
   *[other] Change the settings of { $count } hosts at once.
} A field shows the value they all share; an empty field means they differ. Only fields you change are saved.
bulk-keep = Keep as is
bulk-mixed = differs — leave empty to keep
bulk-only-ssh = Only SSH hosts are changed ({ $ssh } of { $total }).
bulk-proxy-hint = socks5://[user@]host:1080, socks4://… or http://…; “none” removes the proxy.
bulk-serial-skipped = { $count ->
    [one] { $count } serial host keeps everything except its folder.
   *[other] { $count } serial hosts keep everything except their folder.
}
bulk-title = { $count ->
    [one] Edit { $count } host
   *[other] Edit { $count } hosts
}
folder-delete-body = { $count ->
    [one] Delete the folder “{ $name }”? Its { $count } host and any subfolders move to “{ $target }”.
   *[other] Delete the folder “{ $name }”? Its { $count } hosts and any subfolders move to “{ $target }”.
}
folder-delete-ok = Delete folder
folder-delete-title = Delete folder
folder-new-name = New folder
group-broadcast-on = Broadcast is on: what you type goes to every terminal. { $keys } turns it off.
group-confirm-body = { $count ->
    [one] This opens { $count } connection.
   *[other] This opens { $count } connections at the same time.
} Questions such as passwords come one at a time, each naming its host.
group-confirm-open = Open all
group-confirm-title = { $count ->
    [one] Open { $count } host?
   *[other] Open { $count } hosts?
}
hosts-delete-body = These saved hosts will be deleted: { $names }.
hosts-delete-body-more = These saved hosts will be deleted: { $names } and { $more } more.
hosts-delete-title = { $count ->
    [one] Delete { $count } host?
   *[other] Delete { $count } hosts?
}
hosts-save-failed = Could not save: { $error }
import-menu-scan = Scan this computer…
keys-palette = Command palette
keys-shortcuts = Keyboard shortcuts
keys-new-host = New host
keys-settings = Settings
keys-broadcast = Broadcast input to all terminals
keys-cycle-focus = Move focus: host list, tabs, session
keys-context-menu = Context menu of the focused item
keys-duplicate-tab = Duplicate tab
keys-close-pane = Close the pane, then the tab
keys-reconnect = Reconnect
keys-next-tab = Next tab
keys-prev-tab = Previous tab
keys-split-right = Split right
keys-split-down = Split down
keys-move-pane = Move between split panes
keys-resize-pane = Resize the focused pane
keys-host-open = Open a terminal
keys-host-files = Open the files
keys-host-split = Open in a new split
keys-host-edit = Edit (several hosts: edit together)
keys-host-rename = Rename a host or folder
keys-host-delete = Delete
keys-host-select-all = Select all hosts
keys-host-multi = Select several hosts
keys-host-range = Select a range of hosts
keys-copy = Copy
keys-paste = Paste
keys-find = Find in scrollback
keys-find-next = Previous / next match
keys-scroll-back = Scroll back
keys-select-in-app = Select text while a program uses the mouse
keys-send-break = Send break (telnet, serial)
keys-read-output = Read the latest output again (screen readers)
keys-text-view = Output as plain text
keys-files-open = Open
keys-files-up = Parent folder
keys-files-edit = Edit in your editor
keys-files-copy = Upload / download
keys-files-move = Move
keys-files-rename = Rename
keys-files-new-folder = New folder
keys-files-delete = Delete
keys-files-refresh = Refresh
keys-files-switch = Other side
keys-files-properties = Properties
keys-files-select-all = Select all
keys-files-toggle-row = Mark the row
keys-group-app = App
keys-group-tabs = Tabs and panes
keys-group-hosts = Host list
keys-group-terminal = Terminal
keys-group-files = Files
keys-search = Search shortcuts
keys-note = Plain Ctrl+letter keys always go to the shell. Inside a terminal, F1 and Ctrl+/ belong to the program (htop, mc); Ctrl+Shift+/ opens this list from anywhere. On macOS ⌘ takes the place of Ctrl+Shift.
menu-bulk-edit = { $count ->
    [one] Edit { $count } host…
   *[other] Edit { $count } hosts…
}
menu-clear-recent = Clear recent connections
menu-delete-folder = Delete folder…
menu-delete-hosts = { $count ->
    [one] Delete { $count } host…
   *[other] Delete { $count } hosts…
}
menu-favourite = Add to favourites
menu-unfavourite = Remove from favourites
menu-new-subfolder = New subfolder
menu-open-all-files = Open all in file browsers
menu-open-all-grid = Open all in a grid
menu-open-all-grid-broadcast = Open all in a grid with broadcast on
menu-open-all-tabs = Open all in tabs
menu-open-all-tabs-count = Open all in tabs ({ $count })
menu-open-split = Open in a new split
menu-rename = Rename
palette-connect-to = Connect to { $target }
palette-save-as-host = Save { $target } as a host…
palette-group-actions = Actions
palette-group-connect = Connect
palette-group-snippets = Snippets
palette-group-ssh-config = From ~/.ssh/config
palette-group-tabs = Open tabs
palette-hint-open = open
palette-hint-files = files
palette-hint-split = split
palette-hint-edit = edit
palette-hint-close = close
palette-no-terminal = Snippets are typed into a terminal; open one first.
palette-nothing = Nothing found
palette-placeholder = Search hosts and actions, or type user@host:port
palette-quick-detail = quick connect
palette-results = Results
palette-title = Command palette
quick-hint = ↵ connect · { $files } files · ↑↓ choose
quick-suggestions = Suggestions
rename-no-slash = Names cannot contain / or \.
shortcut-palette = command palette
shortcut-all-shortcuts = all shortcuts
sidebar-drag-hosts = { $count ->
    [one] { $count } host
   *[other] { $count } hosts
}
sidebar-favourite-toggle = Favourite: { $name }
sidebar-folder-spoken = { $count ->
    [one] Folder { $name }, { $count } host
   *[other] Folder { $name }, { $count } hosts
}
sidebar-help-tooltip = Keyboard shortcuts ({ $keys })
sidebar-palette-tooltip = Search hosts and actions ({ $keys })
sidebar-quick-spoken = Recent quick connection { $target }
sidebar-section-favourites = Favourites
sidebar-section-recent = Recent
sidebar-section-hosts = Hosts
sidebar-selected-count = { $count } selected
welcome-title = Welcome to Hostwick
welcome-intro = Hostwick can bring along the hosts that other SSH tools saved on this computer. Tick what to import; the other programs are not changed.
welcome-scanning = Looking for saved sessions…
welcome-found = Found on this computer
welcome-src-ssh = { $count ->
    [one] OpenSSH config ({ $path }): { $count } host
   *[other] OpenSSH config ({ $path }): { $count } hosts
}
welcome-src-putty = { $count ->
    [one] PuTTY sessions: { $count } host
   *[other] PuTTY sessions: { $count } hosts
}
welcome-src-winscp-registry = { $count ->
    [one] WinSCP sites: { $count } host
   *[other] WinSCP sites: { $count } hosts
}
welcome-src-winscp-ini = { $count ->
    [one] WinSCP sites ({ $path }): { $count } host
   *[other] WinSCP sites ({ $path }): { $count } hosts
}
welcome-src-solar = { $count ->
    [one] Solar-PuTTY export ({ $path }): { $count } host
   *[other] Solar-PuTTY export ({ $path }): { $count } hosts
}
welcome-keys = { $count ->
    [one] Use my key file from ~/.ssh for quick connect and new hosts
   *[other] Use my { $count } key files from ~/.ssh for quick connect and new hosts
}
welcome-agent = { $count ->
    [one] { $count } key is ready in your SSH agent (Pageant, ssh-agent…); hosts use it automatically.
   *[other] { $count } keys are ready in your SSH agent (Pageant, ssh-agent…); hosts use them automatically.
}
welcome-agent-asking = Asking the SSH agent for its keys…
welcome-agent-none = No SSH agent with keys is running.
welcome-nothing = No sessions saved by other SSH tools were found. Add a host, or type user@host in quick connect.
welcome-note = You can do this again at any time: Import… → Scan this computer.
welcome-import = Import selected
welcome-skip = Skip
welcome-continue = Continue

# --- v0.4 tabs/files ---
files-bookmark-add = Bookmark this folder
files-bookmark-remove = Remove the bookmark for this folder
files-bookmark-remove-menu = Remove bookmark
files-bookmarks = Bookmarks
files-bookmarks-empty = No bookmarks yet. Click the star to add this folder.
files-crumb-go = Go to { $folder }
files-crumbs-more = Higher folders
files-filter-label = Filter files in { $side }
files-filter-placeholder = Filter…
files-filtered-count = { $count } of { $total } shown
files-path-click-to-edit = Click to type a path (Ctrl+L)
keys-files-bookmark = Bookmark this folder
keys-files-filter = Filter the list (just start typing)
keys-files-path = Type a path
keys-files-preview = Quick preview
keys-reopen-tab = Reopen closed tab
keys-tab-number = Go to tab 1–8 · last tab (9)
keys-zoom-in = Zoom in (terminal text)
keys-zoom-out = Zoom out (terminal text)
keys-zoom-reset = Reset zoom
menu-move-new-window = Move to new window
menu-move-to-window = Move to window
preview-close = Close preview
preview-empty = This file is empty.
preview-error = Cannot read this file: { $error }
preview-folder = This is a folder.
preview-hint = Space or Esc closes · ↑ ↓ show the next file · PageUp / PageDown scroll
preview-label = Preview of { $name }
preview-no-preview = No preview for this kind of file.
preview-text-cut = { $encoding } · only the first { $limit } are shown
preview-text-info = { $encoding }
preview-too-large = This image is larger than { $limit }, so it is not shown.
settings-ui-scale = Interface size
settings-ui-scale-hint = Scales text, buttons and dialogs everywhere (90–150 %). Applies to this computer only.
shortcut-jump-tab = go to tab
shortcut-reopen-tab = reopen closed tab
tab-nothing-to-reopen = There is no closed tab to reopen.
tab-reopen-host-gone = That tab's host no longer exists, so it cannot be reopened.
tabs-list = All tabs
transfer-pause = Pause
transfer-paused = paused at { $done } of { $total }
transfer-resume = Resume
transfer-retry = Retry
transfer-show-in-folder = Show in folder
transfer-waiting = waiting…
transfers-batch-done = { $count ->
    [one] Transferred { $count } file ({ $size })
   *[other] Transferred { $count } files ({ $size })
}
transfers-batch-failed = { $done } done, { $failed } failed
transfers-batch-title = Transfers finished
transfers-cancel-all = Cancel all
transfers-collapse = Collapse the transfer list
transfers-expand = Expand the transfer list
transfers-pause-all = Pause all
transfers-resume-all = Resume all
transfers-retry-failed = Retry failed
transfers-sum-done = { $count } done
transfers-sum-eta = ETA { $eta }
transfers-sum-failed = { $count } failed
transfers-sum-paused = { $count } paused
transfers-sum-running = { $count } running
transfers-sum-speed = { $rate }/s
transfers-sum-waiting = { $count } waiting
window-gone = That window has been closed.
window-name = Window { $n }
window-name-with-tab = Window { $n } — { $tab }
zoom-level = Zoom { $percent } %
