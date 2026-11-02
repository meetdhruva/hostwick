use std::io::{IsTerminal, Read};
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use hostwick_ssh::{Connection, PtyOptions, ShellCommand, ShellEvent, ShellSession};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::SshArgs;
use crate::app::{App, parse_forwards};

pub async fn run(app: &App, a: SshArgs) -> Result<i32> {
    let mut profile = app.resolve(&a.target, &a.conn)?;
    profile
        .forwards
        .extend(parse_forwards(&a.local, &a.remote, &a.dynamic)?);
    profile.agent_forwarding |= a.agent;
    profile.x11_forwarding |= a.x11;
    let conn = app.connect(&profile).await?;

    let mut forwards = Vec::new();
    for spec in &profile.forwards {
        let h = conn.start_forward(spec).await?;
        match (h.local_addr(), h.remote_port()) {
            (Some(addr), _) => eprintln!("forwarding {spec} (listening on {addr})"),
            (_, Some(port)) => eprintln!("forwarding {spec} (server port {port})"),
            _ => eprintln!("forwarding {spec}"),
        }
        forwards.push(h);
    }

    if a.no_command {
        eprintln!("Tunnels are up. Press Ctrl+C to close.");
        wait_until_closed_or_interrupted(&conn).await;
        return Ok(0);
    }

    let command = (!a.command.is_empty()).then(|| a.command.join(" "));
    let use_pty = std::io::stdin().is_terminal() && (command.is_none() || a.tty);
    let code = if use_pty {
        interactive(&conn, command.as_deref(), &profile.term).await?
    } else {
        piped(&conn, command.as_deref().unwrap_or("")).await?
    };
    drop(forwards);
    conn.disconnect().await;
    Ok(code)
}

async fn wait_until_closed_or_interrupted(conn: &Connection) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tick.tick() => if conn.is_closed() {
                eprintln!("connection closed");
                break;
            },
        }
    }
}

/// Blocking stdin reader on its own thread, so a slow or idle stdin never
/// stalls the async runtime.
fn spawn_stdin(tx: mpsc::Sender<ShellCommand>, send_eof: bool) {
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => {
                    if send_eof {
                        let _ = tx.blocking_send(ShellCommand::Eof);
                    }
                    break;
                }
                Ok(n) => {
                    if tx
                        .blocking_send(ShellCommand::Data(Bytes::copy_from_slice(&buf[..n])))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}

async fn interactive(conn: &Connection, command: Option<&str>, term: &str) -> Result<i32> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let pty = PtyOptions {
        term: term.to_owned(),
        cols: u32::from(cols),
        rows: u32::from(rows),
    };
    let session = match command {
        Some(cmd) => conn.open_exec(cmd, Some(pty)).await?,
        None => conn.open_shell(pty).await?,
    };
    let _raw = RawMode::enable()?;
    pump(session, true, Some((cols, rows))).await
}

async fn piped(conn: &Connection, command: &str) -> Result<i32> {
    let session = conn.open_exec(command, None).await?;
    if std::io::stdin().is_terminal() {
        session.send(ShellCommand::Eof).await.ok();
        pump_no_stdin(session).await
    } else {
        pump(session, false, None).await
    }
}

async fn pump_no_stdin(session: ShellSession) -> Result<i32> {
    let (_tx, rx) = session.split();
    drain(rx, None, None).await
}

async fn pump(session: ShellSession, interactive: bool, size: Option<(u16, u16)>) -> Result<i32> {
    let (tx, rx) = session.split();
    spawn_stdin(tx.clone(), !interactive);
    drain(rx, Some(tx), size).await
}

async fn drain(
    mut rx: mpsc::Receiver<ShellEvent>,
    resize_tx: Option<mpsc::Sender<ShellCommand>>,
    mut size: Option<(u16, u16)>,
) -> Result<i32> {
    let mut stdout = tokio::io::stdout();
    let mut stderr = tokio::io::stderr();
    let mut code: Option<i32> = None;
    // Polling the size works the same on every OS (no SIGWINCH on Windows).
    let mut resize_tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(ShellEvent::Stdout(b)) => {
                    stdout.write_all(&b).await.context("writing stdout")?;
                    stdout.flush().await.ok();
                }
                Some(ShellEvent::Stderr(b)) => {
                    stderr.write_all(&b).await.ok();
                    stderr.flush().await.ok();
                }
                Some(ShellEvent::Exit(c)) => code = Some(i32::try_from(c).unwrap_or(255)),
                Some(ShellEvent::ExitSignal(sig)) => {
                    tracing::info!(%sig, "remote process killed by signal");
                    code.get_or_insert(255);
                }
                Some(ShellEvent::Closed) | None => break,
            },
            _ = resize_tick.tick(), if size.is_some() => {
                if let (Ok(now), Some(tx)) = (crossterm::terminal::size(), &resize_tx)
                    && Some(now) != size
                {
                    size = Some(now);
                    let _ = tx.send(ShellCommand::Resize { cols: u32::from(now.0), rows: u32::from(now.1) }).await;
                }
            }
        }
    }
    Ok(code.unwrap_or(0))
}

/// Raw terminal mode for the duration of an interactive session; restored on
/// drop, including on error paths.
struct RawMode {
    #[cfg(windows)]
    console: Option<console::Saved>,
}

impl RawMode {
    fn enable() -> Result<Self> {
        crossterm::terminal::enable_raw_mode().context("enabling raw terminal mode")?;
        Ok(Self {
            #[cfg(windows)]
            console: console::enable_vt(),
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(saved) = self.console.take() {
            console::restore(saved);
        }
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Windows consoles only produce VT key sequences (arrows, F-keys) when
/// `ENABLE_VIRTUAL_TERMINAL_INPUT` is on; servers expect them.
#[cfg(windows)]
#[allow(unsafe_code)]
mod console {
    use windows_sys::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    };

    pub struct Saved {
        input: CONSOLE_MODE,
        output: CONSOLE_MODE,
    }

    pub fn enable_vt() -> Option<Saved> {
        // SAFETY: GetStdHandle/GetConsoleMode/SetConsoleMode on this process's
        // own standard handles; each mode is read before being modified and
        // the pointers are to valid stack locals.
        unsafe {
            let hin = GetStdHandle(STD_INPUT_HANDLE);
            let hout = GetStdHandle(STD_OUTPUT_HANDLE);
            let (mut input, mut output): (CONSOLE_MODE, CONSOLE_MODE) = (0, 0);
            if GetConsoleMode(hin, &mut input) == 0 || GetConsoleMode(hout, &mut output) == 0 {
                return None;
            }
            SetConsoleMode(hin, input | ENABLE_VIRTUAL_TERMINAL_INPUT);
            SetConsoleMode(hout, output | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            Some(Saved { input, output })
        }
    }

    pub fn restore(saved: Saved) {
        // SAFETY: as above; restores the modes captured by `enable_vt`.
        unsafe {
            SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), saved.input);
            SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), saved.output);
        }
    }
}
