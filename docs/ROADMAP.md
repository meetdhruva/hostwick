# Hostwick competitive gap analysis and roadmap

Researched 2026-11-02 against WinSCP, PuTTY, Solar-PuTTY, MobaXterm, Termius,
Tabby, SecureCRT, Bitvise, KiTTY and FileZilla. "Have" = in Hostwick v0.2.0.

## 1. What users rely on most (and whether we have it)

| Daily-use feature | Who has it | Hostwick |
|---|---|---|
| Tabs, many sessions in one window | everyone except PuTTY | Have |
| Saved sessions in folders, search | all | Have |
| Saved passwords / auto-login | Solar-PuTTY, MobaXterm, Termius, WinSCP (master password) | **CLI vault only; desktop prompts every time** |
| Edit remote file in my editor, auto-upload on save | WinSCP, MobaXterm | **Missing** |
| Two-pane local/remote browser with drag and drop | WinSCP (Commander), FileZilla, MobaXterm | **Missing** (remote pane + OS dialogs only) |
| Synchronize with preview, keep remote up to date | WinSCP | Partial (CLI dry-run; desktop "Keep up to date" upload-only) |
| SFTP panel that follows the terminal's folder | MobaXterm | **Missing** |
| Auto-reconnect when the link drops | Solar-PuTTY, MobaXterm | Manual only (button, Enter, Ctrl+Shift+R) |
| Post-login scripts / snippets / button bar | Solar-PuTTY, SecureCRT, Termius, KiTTY | **Missing** |
| Split panes / tiling | MobaXterm, SecureCRT, Tabby | **Missing** |
| Send input to all sessions | MobaXterm, SecureCRT | Have (broadcast) |
| Search scrollback, keyword highlighting | SecureCRT, MobaXterm, Xshell | **Missing** |
| Mouse in htop/mc/vim | most modern terminals | **Missing** |
| Port-forward manager | MobaXterm, Bitvise, Termius | Have (per host, started with terminal) — no standalone tunnel view |
| Session logging | PuTTY, SecureCRT | Have |
| Import from PuTTY/WinSCP/ssh_config | some | Have (plus Solar-PuTTY, .ppk) |
| Scripting / automation | WinSCP (.NET, scripts), SecureCRT (Python) | CLI only |

## 2. Protocol and auth gaps

| Gap | Who needs it | Notes |
|---|---|---|
| sudo / root file access over SFTP | everyone who can't log in as root | WinSCP only via `sudo sftp-server` with NOPASSWD; Cyberduck ticket open since 2014, FileZilla tickets too. **Big differentiator if we support password sudo.** |
| SOCKS/HTTP proxy to reach the server | corporate networks, PuTTY imports (we already warn) | |
| X11 forwarding | MobaXterm users (GUI apps from Linux) | Needs a local X server on Windows; forward to existing one on mac/Linux. |
| Agent forwarding | git on servers | Option hidden until channels are served. |
| Kerberos/GSSAPI | AD enterprises (WinSCP, Bitvise, PuTTY) | |
| OpenSSH certificates, FIDO2 (sk-) keys | security-conscious orgs | |
| SCP fallback | routers, ESXi, embedded boxes without SFTP | |
| Telnet, serial | network engineers (PuTTY, Solar-PuTTY, Tabby) | |
| FTP/FTPS, WebDAV, S3 | WinSCP users | Lower priority; `RemoteFs` port makes it additive. |
| Mosh, RDP, VNC | MobaXterm | Out of scope for now. |
| Key generation/conversion (PuTTYgen), known-hosts manager | PuTTY users | |

## 3. What people complain about, and our answer

| Product | Complaint | Hostwick answer |
|---|---|---|
| WinSCP | Windows only; developer has declined Linux/macOS for years | Native on all three already |
| WinSCP | sudo needs passwordless sudoers + no `requiretty` | Password sudo for the file browser |
| WinSCP | External-editor saves sometimes not uploaded (timezone bug, tabbed editors) | Detect saves by content hash, not mtime; warn if the remote changed meanwhile |
| PuTTY | One window per session, no tabs, no file transfer, Load/Save settings dance, tiny fonts on HiDPI | Have tabs, SFTP, editor, HiDPI |
| Solar-PuTTY | Registration form to download, Windows only, closed | Free, open, no account |
| MobaXterm | Crashes/freezes after a connection closes; SFTP panel struggles with large files; cluttered UI; free tier caps sessions; subscription | Async core, pipelined SFTP, no caps — keep it that way |
| Termius | ~$120/yr; device sync is paywalled; needs a cloud account | Free sync via your own folder (OneDrive/Dropbox/git), end-to-end encrypted |
| Tabby | Electron, ~1 GB RAM for a few windows (vs ~50 MB SecureCRT) | Native GPU UI; publish measured memory |
| FileZilla | Bundled adware in the Windows installer | Signed installers, nothing bundled, ever |

## 4. Roadmap

### v0.3 — the daily-driver release
1. Saved passwords and passphrases in the desktop (vault + master password, "remember" checkbox on prompts; OS keychain optional).
2. Edit in local editor with auto-upload (hash-based save detection, conflict warning, multiple files, any editor).
3. Two-pane browser (local | remote) with drag and drop inside the app and from/to the OS.
4. Auto-reconnect with backoff; restore tabs on restart.
5. Scrollback search (Ctrl+Shift+F) and terminal mouse reporting.
6. Properties dialog: chmod/owner, size of folder, checksum; create symlink.

### v0.4 — the power-user release
1. sudo file access (password or NOPASSWD), per host.
2. File browser follows the terminal folder (OSC 7 / shell integration).
3. Synchronize dialog with a reviewable plan, two-way mode, desktop dry-run.
4. Snippets and post-login scripts; per-host button bar.
5. Split panes.
6. SOCKS5/HTTP proxy; agent forwarding; keyword highlighting.

### v0.5 — breadth and trust
1. Signed MSI/DMG/AppImage, auto-update.
2. Encrypted settings sync through a user-chosen folder.
3. SCP fallback, telnet, serial; X11 forwarding.
4. Kerberos/GSSAPI, OpenSSH certificates, FIDO2 keys; key generator and known-hosts manager.
5. Scripting file mode for the CLI (WinSCP-style batch), JSON output.
6. Accessibility pass (screen readers) and translations.

## Sources
- WinSCP features: https://winscp.net/eng/docs/introduction
- WinSCP sudo/su limits: https://winscp.net/eng/docs/faq_su
- WinSCP Linux/macOS requests: https://winscp.net/forum/viewtopic.php?p=108677
- WinSCP external-editor upload bug: https://winscp.net/forum/viewtopic.php?p=106534
- Cyberduck sudo request (since 2014): https://trac.cyberduck.io/ticket/8230
- MobaXterm features: https://mobaxterm-en.mobatek.net/documentation.html
- MobaXterm reviews: https://capterra.com/p/209929/MobaXTerm/reviews/
- Solar-PuTTY: https://www.solarwinds.com/free-tools/solar-putty
- SecureCRT: https://vandyke.com/products/securecrt/key_features.html
- Termius complaints: https://termai.sh/blog/termius-alternatives
- Tabby memory: https://github.com/Eugeny/tabby/discussions/8776
- PuTTY shortcomings and alternatives: https://comparitech.com/net-admin/best-putty-alternatives-ssh-clients
- FileZilla installer controversy: https://tildes.net/~tech/3bx/filezilla_bundles_malware_dev_doubles_down_on_false_positive
- Mac WinSCP equivalent / accessibility: https://www.applevis.com/forum/macos-mac-apps/there-accessible-mac-equivalent-winscp
