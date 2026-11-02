# ADR 0001 — Architecture and stack

Date: 2026-11-02 · Status: accepted

## Context

Replace Solar-PuTTY, PuTTY and WinSCP with one app. Priorities, in order:
no hangs, performance (latency and throughput), identical behaviour on
Windows, macOS and Linux.

## Decisions

1. **Pure-Rust SSH (`russh`) on Tokio**, `ring` crypto backend (no CMake/NASM on
   Windows). One `Connection` per host, cloned into every tab, file pane and
   tunnel; russh's session task is the per-connection actor.
2. **Hexagonal layout.** `hostwick-core` has no I/O. `RemoteFs` is the port for
   file operations (SFTP adapter, local adapter); the transfer engine only sees
   the port, so it is tested without a server and can gain SCP/S3 adapters.
3. **Native GPU UI: GPUI via `gpui-kit` =0.8.1** (pins the matching `gpui-pre`
   snapshot used by `gpui-component`), with `alacritty_terminal` for VT
   parsing. Chosen over Tauri because WebKitGTK on Linux would make terminal
   performance OS-dependent, and over egui/Iced for the proven terminal and
   component set. Versions are pinned exactly; GPUI is 0.x.
4. **Threading model.** UI thread never awaits network I/O. Work is spawned on
   the Tokio runtime and its `JoinHandle` awaited from GPUI tasks. Terminal
   output is parsed on Tokio under a `FairMutex`; the UI is woken at most once
   per batch. All queues toward the network are bounded (backpressure reaches
   the server); keystrokes use an ordered unbounded queue (tiny, must not block).
5. **Transfers** write `name.filepart`, verify size, then rename. Resume checks
   the partial file's last 64 KiB against the source. Retries use exponential
   backoff with full jitter.
6. **Storage.** SQLite (WAL) with JSON profile documents and ordered,
   append-only migrations; the DB is backed up before migrating. Vault is a
   separate file: Argon2id → XChaCha20-Poly1305 with the header as AAD.

## Consequences

- GPUI upgrades are deliberate work (pinned versions).
- Panics in one session task surface as a `JoinError`, not a crashed app
  (`panic = "unwind"` in release).
