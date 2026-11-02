//! SFTP as another user (usually root) through `sudo`.
//!
//! The SFTP server binary is started with `sudo` over an exec channel and the
//! SFTP protocol runs over its stdin/stdout. Unlike WinSCP, a sudo password
//! is supported, not only `NOPASSWD` rules:
//!
//! 1. `sudo -n -u USER -- SERVER` with stdin closed: succeeds without a
//!    password, or says "a password is required" (or names another problem:
//!    not in sudoers, `requiretty`, no such server).
//! 2. With a password: `sudo -S -p MARKER …` is run once to *check* it — the
//!    password is written only after the unique marker prompt appears on
//!    stderr, then stdin is closed, so a wrong password costs exactly one
//!    failed attempt and is reported as such.
//! 3. The real session: `sudo -n` if sudo now has cached credentials, else
//!    `sudo -S` with the (verified) password fed after the prompt.
//!
//! The password is never logged, echoed or put on a command line.

use std::time::Duration;

use bytes::Bytes;
use hostwick_core::{ExposeSecret, SecretString, SudoOptions};
use hostwick_ssh::{Connection, ExecStream, ExitStatus};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

use crate::FsError;
use crate::shell::quote;

/// Where distributions install OpenSSH's `sftp-server`, most likely first.
pub const SFTP_SERVER_CANDIDATES: &[&str] = &[
    "/usr/lib/openssh/sftp-server",
    "/usr/libexec/openssh/sftp-server",
    "/usr/lib/ssh/sftp-server",
    "/usr/libexec/sftp-server",
    "/usr/lib/sftp-server",
];

/// Time for each step (PAM can be slow, LDAP slower).
const STEP_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SudoError {
    /// No password was given and sudo needs one. Ask and try again.
    #[error("sudo needs a password")]
    PasswordRequired,
    #[error("sudo: wrong password")]
    WrongPassword,
    #[error("sudo refused: {0}")]
    NotAllowed(String),
    #[error(
        "sudo on this server requires a terminal (`Defaults requiretty`); ask the administrator to add `Defaults:<user> !requiretty`"
    )]
    RequiresTty,
    #[error("sudo is not installed on the server")]
    NotInstalled,
    #[error(
        "no SFTP server program found on the server (tried {0}); set its path in the host's sudo options"
    )]
    NoSftpServer(String),
    #[error("the password cannot contain a line break")]
    BadPassword,
    #[error("sudo did not answer within {0} s")]
    Timeout(u64),
    #[error("sudo failed: {0}")]
    Failed(String),
}

/// The `Subsystem sftp` command from `sshd_config` text (`None` when absent
/// or `internal-sftp`, which is not a program sudo can run). Keywords are
/// case-insensitive; the first match wins, as in sshd.
pub fn parse_sftp_subsystem(config: &str) -> Option<String> {
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(kw), Some(name)) = (words.next(), words.next()) else {
            continue;
        };
        if !kw.eq_ignore_ascii_case("subsystem") || name != "sftp" {
            continue;
        }
        let cmd: Vec<&str> = words.collect();
        return match cmd.first() {
            None => None,
            Some(&"internal-sftp") => None,
            Some(_) => Some(cmd.join(" ")),
        };
    }
    None
}

/// The program to run under sudo: the configured override, else what
/// `sshd_config` names (if it exists), else the first well-known path that
/// does.
pub async fn detect_sftp_server(conn: &Connection, configured: &str) -> Result<String, FsError> {
    if !configured.trim().is_empty() {
        return Ok(configured.trim().to_owned());
    }
    let cfg = conn
        .exec("cat /etc/ssh/sshd_config /etc/ssh/sshd_config.d/*.conf 2>/dev/null")
        .await?;
    let from_config = parse_sftp_subsystem(&cfg.stdout_lossy());
    let mut candidates: Vec<String> = Vec::new();
    if let Some(c) = &from_config
        && let Some(prog) = c.split_whitespace().next()
    {
        candidates.push(prog.to_owned());
    }
    candidates.extend(SFTP_SERVER_CANDIDATES.iter().map(|s| (*s).to_owned()));
    let script = format!(
        "for p in {}; do if [ -x \"$p\" ]; then echo \"$p\"; exit 0; fi; done; command -v sftp-server",
        candidates
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let out = conn.exec(&script).await?;
    let found = out
        .stdout_lossy()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    if found.is_empty() {
        return Err(SudoError::NoSftpServer(candidates.join(", ")).into());
    }
    // Keep the arguments sshd_config gives its own server (e.g. `-l INFO`).
    if let Some(c) = from_config
        && c.split_whitespace().next() == Some(found.as_str())
    {
        return Ok(c);
    }
    Ok(found)
}

/// A prompt marker no file name or message will contain.
fn new_marker() -> String {
    use rand::Rng as _;
    let n: u64 = rand::thread_rng().r#gen();
    format!("[hostwick-sudo-{n:016x}]:")
}

pub(crate) struct SudoCommand {
    user: String,
    server: String,
}

impl SudoCommand {
    pub(crate) fn new(opts: &SudoOptions, server: String) -> Self {
        let user = if opts.user.trim().is_empty() {
            "root".to_owned()
        } else {
            opts.user.trim().to_owned()
        };
        Self { user, server }
    }

    /// Never prompts; fails if a password would be needed.
    fn noninteractive(&self) -> String {
        format!("sudo -n -u {} -- {}", quote(&self.user), self.server)
    }

    /// Reads the password from stdin after printing `marker` to stderr.
    fn with_password(&self, marker: &str) -> String {
        format!(
            "sudo -S -p {} -u {} -- {}",
            quote(marker),
            quote(&self.user),
            self.server
        )
    }

    pub(crate) fn user(&self) -> &str {
        &self.user
    }
}

/// What a failed sudo run printed, as an error a person can act on.
pub(crate) fn classify(
    stderr: &str,
    exit: &ExitStatus,
    marker: Option<&str>,
    server: &str,
) -> SudoError {
    let lower = stderr.to_ascii_lowercase();
    let prompts = marker.map_or(0, |m| stderr.matches(m).count());
    if matches!(exit, ExitStatus::Code(127)) && !lower.contains(&server.to_ascii_lowercase()) {
        return SudoError::NotInstalled;
    }
    if lower.contains("must have a tty") || lower.contains("terminal is required") {
        return SudoError::RequiresTty;
    }
    if lower.contains("not in the sudoers")
        || lower.contains("is not allowed to")
        || lower.contains("may not run sudo")
        || lower.contains("not allowed to run sudo")
    {
        return SudoError::NotAllowed(first_meaningful_line(stderr));
    }
    if lower.contains("incorrect password")
        || lower.contains("sorry, try again")
        || lower.contains("authentication failure")
        || prompts >= 2
    {
        return SudoError::WrongPassword;
    }
    if lower.contains("password is required") {
        return SudoError::PasswordRequired;
    }
    if lower.contains("command not found") || lower.contains("no such file") {
        return SudoError::NoSftpServer(server.to_owned());
    }
    let line = first_meaningful_line(stderr);
    SudoError::Failed(if line.is_empty() {
        exit.to_string()
    } else {
        line
    })
}

/// The most useful line of sudo's chatter (skips the lecture and host
/// warnings), without our prompt marker.
fn first_meaningful_line(stderr: &str) -> String {
    stderr
        .lines()
        .map(|l| l.trim())
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with("[hostwick-sudo-")
                && !l.contains("unable to resolve host")
                && !l.starts_with("We trust you")
                && !l.starts_with('#')
                && !l.starts_with("Administrator")
        })
        .find(|l| l.starts_with("sudo:") || l.contains("sudoers") || l.contains("not allowed"))
        .or_else(|| stderr.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("")
        .trim_start_matches("sudo: ")
        .to_owned()
}

/// Stderr collected while watching for the prompt.
#[derive(Default)]
pub(crate) struct Stderr {
    pub text: String,
}

impl Stderr {
    fn push(&mut self, b: &Bytes) {
        self.text.push_str(&String::from_utf8_lossy(b));
    }

    fn prompts(&self, marker: &str) -> usize {
        self.text.matches(marker).count()
    }
}

/// Wait until sudo has printed the prompt `marker` for the `n`th time.
/// Returns early with an error if the command exits first.
pub(crate) async fn wait_for_prompt(
    stderr: &mut mpsc::UnboundedReceiver<Bytes>,
    exit: &mut watch::Receiver<Option<ExitStatus>>,
    seen: &mut Stderr,
    marker: &str,
    n: usize,
    server: &str,
) -> Result<(), SudoError> {
    let wait = async {
        loop {
            if seen.prompts(marker) >= n {
                return Ok(());
            }
            tokio::select! {
                chunk = stderr.recv() => match chunk {
                    Some(b) => seen.push(&b),
                    None => {
                        let status = hostwick_ssh::wait_exit(exit).await;
                        return Err(classify(&seen.text, &status, Some(marker), server));
                    }
                },
                status = hostwick_ssh::wait_exit(exit) => {
                    // Take whatever stderr is still queued before deciding.
                    while let Ok(b) = stderr.try_recv() {
                        seen.push(&b);
                    }
                    if seen.prompts(marker) >= n {
                        return Ok(());
                    }
                    return Err(classify(&seen.text, &status, Some(marker), server));
                }
            }
        }
    };
    tokio::time::timeout(STEP_TIMEOUT, wait)
        .await
        .unwrap_or(Err(SudoError::Timeout(STEP_TIMEOUT.as_secs())))
}

/// Write the password and a newline; the buffer is wiped afterwards.
pub(crate) async fn send_password<W: AsyncWrite + Unpin>(
    w: &mut W,
    password: &SecretString,
) -> Result<(), SudoError> {
    let pw = password.expose_secret();
    if pw.contains('\n') || pw.contains('\r') {
        return Err(SudoError::BadPassword);
    }
    let mut buf = Vec::with_capacity(pw.len() + 1);
    buf.extend_from_slice(pw.as_bytes());
    buf.push(b'\n');
    let res = w.write_all(&buf).await;
    buf.fill(0);
    res.map_err(|e| SudoError::Failed(format!("writing to sudo: {e}")))?;
    w.flush()
        .await
        .map_err(|e| SudoError::Failed(format!("writing to sudo: {e}")))
}

/// Outcome of `sudo -n` with stdin closed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Probe {
    Passwordless,
    NeedsPassword,
}

pub(crate) async fn probe(conn: &Connection, cmd: &SudoCommand) -> Result<Probe, FsError> {
    let out = tokio::time::timeout(STEP_TIMEOUT, conn.exec(&cmd.noninteractive()))
        .await
        .map_err(|_| SudoError::Timeout(STEP_TIMEOUT.as_secs()))??;
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let status = match (&out.exit_status, &out.exit_signal) {
        (Some(c), _) => ExitStatus::Code(*c),
        (None, Some(s)) => ExitStatus::Signal(s.clone()),
        _ => ExitStatus::Closed,
    };
    probe_result(&stderr, &status, &cmd.server)
}

/// `sftp-server` exits 0 on end of input; sudo -n says what is missing.
pub(crate) fn probe_result(
    stderr: &str,
    status: &ExitStatus,
    server: &str,
) -> Result<Probe, FsError> {
    match classify(stderr, status, None, server) {
        _ if status.success() => Ok(Probe::Passwordless),
        SudoError::PasswordRequired => Ok(Probe::NeedsPassword),
        e => Err(e.into()),
    }
}

/// Check the password with a throwaway run: prompt, password, end of input.
/// The server sees end of input right away and exits 0; a wrong password
/// makes sudo prompt again, read nothing, and give up.
pub(crate) async fn validate(
    conn: &Connection,
    cmd: &SudoCommand,
    password: &SecretString,
) -> Result<(), FsError> {
    let marker = new_marker();
    let stream = conn.open_exec_stream(&cmd.with_password(&marker)).await?;
    let (mut stdio, mut stderr, mut exit) = stream.into_parts();
    let mut seen = Stderr::default();
    wait_for_prompt(&mut stderr, &mut exit, &mut seen, &marker, 1, &cmd.server).await?;
    send_password(&mut stdio, password).await?;
    let _ = stdio.shutdown().await;
    validation_outcome(&mut stderr, &mut exit, &mut seen, &marker, &cmd.server).await?;
    Ok(())
}

pub(crate) async fn validation_outcome(
    stderr: &mut mpsc::UnboundedReceiver<Bytes>,
    exit: &mut watch::Receiver<Option<ExitStatus>>,
    seen: &mut Stderr,
    marker: &str,
    server: &str,
) -> Result<(), SudoError> {
    let wait = async {
        let status = hostwick_ssh::wait_exit(exit).await;
        while let Some(b) = stderr.recv().await {
            seen.push(&b);
        }
        status
    };
    let status = tokio::time::timeout(STEP_TIMEOUT, wait)
        .await
        .map_err(|_| SudoError::Timeout(STEP_TIMEOUT.as_secs()))?;
    if status.success() && seen.prompts(marker) <= 1 {
        Ok(())
    } else {
        Err(classify(&seen.text, &status, Some(marker), server))
    }
}

/// The stream for the real session, with the password already fed when
/// one is needed. Returns the marker to watch for (a second prompt means
/// the password was refused after all).
pub(crate) async fn start(
    conn: &Connection,
    cmd: &SudoCommand,
    password: Option<&SecretString>,
) -> Result<(ExecStream, Option<String>), FsError> {
    let password = match password {
        Some(p) if probe(conn, cmd).await? == Probe::NeedsPassword => p,
        _ => return Ok((conn.open_exec_stream(&cmd.noninteractive()).await?, None)),
    };
    let marker = new_marker();
    let mut stream = conn.open_exec_stream(&cmd.with_password(&marker)).await?;
    let mut seen = Stderr::default();
    wait_for_prompt(
        &mut stream.stderr,
        &mut stream.exit,
        &mut seen,
        &marker,
        1,
        &cmd.server,
    )
    .await?;
    send_password(&mut stream.stdio, password).await?;
    Ok((stream, Some(marker)))
}

/// Watch stderr while the SFTP handshake runs: resolves only on a failure
/// sign (another prompt, "Sorry, try again", an exit).
pub(crate) async fn watch_failure(
    stderr: &mut mpsc::UnboundedReceiver<Bytes>,
    exit: &mut watch::Receiver<Option<ExitStatus>>,
    marker: Option<&str>,
    server: &str,
) -> SudoError {
    let mut seen = Stderr::default();
    loop {
        tokio::select! {
            chunk = stderr.recv() => match chunk {
                Some(b) => {
                    seen.push(&b);
                    let lower = seen.text.to_ascii_lowercase();
                    if marker.is_some_and(|m| seen.prompts(m) >= 1)
                        || lower.contains("sorry, try again")
                        || lower.contains("incorrect password")
                    {
                        return SudoError::WrongPassword;
                    }
                }
                None => {
                    let status = hostwick_ssh::wait_exit(exit).await;
                    return classify(&seen.text, &status, marker, server);
                }
            },
            status = hostwick_ssh::wait_exit(exit) => {
                // Let the last stderr arrive.
                let _ = tokio::time::timeout(Duration::from_millis(500), async {
                    while let Some(b) = stderr.recv().await {
                        seen.push(&b);
                    }
                }).await;
                return classify(&seen.text, &status, marker, server);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[test]
    fn subsystem_lines() {
        let cfg = "# Subsystem sftp /commented\n\
                   Port 22\n\
                   subsystem   sftp  /usr/libexec/openssh/sftp-server -l INFO\n\
                   Subsystem sftp /second";
        assert_eq!(
            parse_sftp_subsystem(cfg).as_deref(),
            Some("/usr/libexec/openssh/sftp-server -l INFO")
        );
        assert_eq!(parse_sftp_subsystem("Subsystem\tsftp\tinternal-sftp"), None);
        assert_eq!(parse_sftp_subsystem("Subsystem other /x"), None);
        assert_eq!(parse_sftp_subsystem(""), None);
    }

    #[test]
    fn commands_quote_the_user_and_marker() {
        let opts = SudoOptions {
            user: "o'brien".into(),
            ..Default::default()
        };
        let c = SudoCommand::new(&opts, "/usr/lib/sftp-server".into());
        assert_eq!(
            c.noninteractive(),
            "sudo -n -u 'o'\\''brien' -- /usr/lib/sftp-server"
        );
        assert!(c.with_password("[m]:").starts_with("sudo -S -p '[m]:' -u "));
        let root = SudoCommand::new(&SudoOptions::default(), "s".into());
        assert_eq!(root.user(), "root");
    }

    #[test]
    fn classifies_sudo_messages() {
        let s = "/usr/lib/openssh/sftp-server";
        let c1 = ExitStatus::Code(1);
        assert_eq!(
            classify("sudo: a password is required\n", &c1, None, s),
            SudoError::PasswordRequired
        );
        assert!(matches!(
            classify("hwfiles is not in the sudoers file.  This incident will be reported.\n", &c1, None, s),
            SudoError::NotAllowed(m) if m.contains("sudoers")
        ));
        assert!(matches!(
            classify(
                "Sorry, user bob is not allowed to execute '/usr/lib/openssh/sftp-server' as root on h.\n",
                &c1,
                None,
                s
            ),
            SudoError::NotAllowed(_)
        ));
        assert_eq!(
            classify(
                "sudo: sorry, you must have a tty to run sudo\n",
                &c1,
                None,
                s
            ),
            SudoError::RequiresTty
        );
        assert_eq!(
            classify(
                "[m]:Sorry, try again.\n[m]:\nsudo: no password was provided\nsudo: 1 incorrect password attempt\n",
                &c1,
                Some("[m]:"),
                s
            ),
            SudoError::WrongPassword
        );
        assert_eq!(
            classify(
                "bash: line 1: sudo: command not found\n",
                &ExitStatus::Code(127),
                None,
                s
            ),
            SudoError::NotInstalled
        );
        assert_eq!(
            classify(
                "sudo: /usr/lib/openssh/sftp-server: command not found\n",
                &c1,
                None,
                s
            ),
            SudoError::NoSftpServer(s.into())
        );
        assert!(matches!(
            classify("sudo: something odd\n", &c1, None, s),
            SudoError::Failed(m) if m == "something odd"
        ));
        assert!(matches!(
            probe_result("", &ExitStatus::Code(0), s),
            Ok(Probe::Passwordless)
        ));
        assert!(matches!(
            probe_result("sudo: a password is required\n", &c1, s),
            Ok(Probe::NeedsPassword)
        ));
    }

    /// Scripted sudo: the password must be written only after the marker,
    /// even when the marker arrives split across chunks after other output.
    #[tokio::test]
    async fn password_waits_for_the_marker() {
        let marker = "[hostwick-sudo-00]:";
        let (err_tx, mut err_rx) = mpsc::unbounded_channel();
        let (_exit_tx, mut exit_rx) = watch::channel(None);
        let (mut ours, mut theirs) = tokio::io::duplex(1024);
        let mut seen = Stderr::default();

        err_tx
            .send(Bytes::from_static(
                b"\nWe trust you have received the usual lecture\n[hostwick-su",
            ))
            .unwrap_or_else(|_| panic!("send"));
        let waiter = tokio::spawn(async move {
            let r = wait_for_prompt(&mut err_rx, &mut exit_rx, &mut seen, marker, 1, "s").await;
            (r, err_rx, exit_rx, seen)
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished(), "half a marker is not a prompt");
        err_tx
            .send(Bytes::from_static(b"do-00]:"))
            .unwrap_or_else(|_| panic!("send"));
        let (r, mut err_rx, _, mut seen) = waiter.await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r, Ok(()));

        send_password(&mut ours, &SecretString::from("s3cr3t pw".to_owned()))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let mut buf = [0u8; 10];
        theirs
            .read_exact(&mut buf)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&buf, b"s3cr3t pw\n");

        // Wrong password: sudo complains and prompts again, then exits.
        let (exit_tx, mut exit_rx) = watch::channel(None);
        err_tx
            .send(Bytes::from_static(
                b"Sorry, try again.\n[hostwick-sudo-00]:",
            ))
            .unwrap_or_else(|_| panic!("send"));
        err_tx
            .send(Bytes::from_static(b"sudo: 1 incorrect password attempt\n"))
            .unwrap_or_else(|_| panic!("send"));
        drop(err_tx);
        let _ = exit_tx.send(Some(ExitStatus::Code(1)));
        let r = validation_outcome(&mut err_rx, &mut exit_rx, &mut seen, marker, "s").await;
        assert_eq!(r, Err(SudoError::WrongPassword));
    }

    #[tokio::test]
    async fn exit_before_prompt_is_classified() {
        let (err_tx, mut err_rx) = mpsc::unbounded_channel();
        let (exit_tx, mut exit_rx) = watch::channel(None);
        err_tx
            .send(Bytes::from_static(
                b"sudo: sorry, you must have a tty to run sudo\n",
            ))
            .unwrap_or_else(|_| panic!("send"));
        let _ = exit_tx.send(Some(ExitStatus::Code(1)));
        let mut seen = Stderr::default();
        let r = wait_for_prompt(&mut err_rx, &mut exit_rx, &mut seen, "[m]:", 1, "s").await;
        assert_eq!(r, Err(SudoError::RequiresTty));
    }

    #[tokio::test]
    async fn successful_check_and_failure_watch() {
        let (err_tx, mut err_rx) = mpsc::unbounded_channel::<Bytes>();
        let (exit_tx, mut exit_rx) = watch::channel(None);
        let mut seen = Stderr::default();
        seen.text.push_str("[m]:");
        drop(err_tx);
        let _ = exit_tx.send(Some(ExitStatus::Code(0)));
        assert_eq!(
            validation_outcome(&mut err_rx, &mut exit_rx, &mut seen, "[m]:", "s").await,
            Ok(())
        );

        let (err_tx, mut err_rx) = mpsc::unbounded_channel::<Bytes>();
        let (_exit_tx, mut exit_rx) = watch::channel(None);
        err_tx
            .send(Bytes::from_static(b"[m]:"))
            .unwrap_or_else(|_| panic!("send"));
        assert_eq!(
            watch_failure(&mut err_rx, &mut exit_rx, Some("[m]:"), "s").await,
            SudoError::WrongPassword
        );
        assert!(
            !send_password(
                &mut tokio::io::sink(),
                &SecretString::from("a\nb".to_owned())
            )
            .await
            .is_ok()
        );
    }
}
