//! Shell commands that answer questions faster on the server than over the
//! file protocol (checksums, folder sizes, user names). Each returns `None`
//! when the tool is missing or its output looks wrong, so callers fall back
//! to the protocol.

use hostwick_ssh::{Connection, ExecOutput, ExecStream, SshError};

use crate::shell::{path_arg, portable};
use crate::{DiskUsage, FsError, HashAlgo};

/// Run a POSIX `sh` script on the server, whatever the login shell is
/// (see [`portable`]). Every helper command goes through here.
pub(crate) async fn sh(conn: &Connection, script: &str) -> Result<ExecOutput, SshError> {
    conn.exec(&portable(script)).await
}

/// [`sh`] with the script's stdin/stdout as a byte stream.
pub(crate) async fn sh_stream(conn: &Connection, script: &str) -> Result<ExecStream, SshError> {
    conn.open_exec_stream(&portable(script)).await
}

/// Hex digest computed on the server (`sha256sum`, `shasum`, `md5sum`, `md5`).
pub(crate) async fn checksum(conn: &Connection, path: &str, algo: HashAlgo) -> Option<String> {
    let p = path_arg(path);
    let cmd = match algo {
        HashAlgo::Sha256 => format!(
            "sha256sum {p} 2>/dev/null || shasum -a 256 {p} 2>/dev/null || sha256 -q {p} 2>/dev/null"
        ),
        HashAlgo::Md5 => format!("md5sum {p} 2>/dev/null || md5 -q {p} 2>/dev/null"),
    };
    let out = sh(conn, &cmd).await.ok()?;
    parse_digest(&out.stdout_lossy(), algo)
}

pub(crate) fn parse_digest(out: &str, algo: HashAlgo) -> Option<String> {
    // GNU/busybox prefix a backslash when the name needed escaping.
    let first = out.lines().next()?.trim_start_matches('\\');
    let digest = first.split_whitespace().next()?;
    (digest.len() == algo.hex_len() && digest.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| digest.to_ascii_lowercase())
}

/// Folder size with one `find` (GNU `-printf`). `None` if unsupported.
pub(crate) async fn disk_usage(conn: &Connection, path: &str) -> Option<DiskUsage> {
    let p = path_arg(path);
    let cmd = format!(
        "find {p} -mindepth 1 -printf '%y %s\\n' 2>/dev/null | \
         awk 'BEGIN{{f=0;d=0;b=0}} $1==\"d\"{{d++;next}} $1==\"f\"{{f++;b+=$2;next}} {{f++}} END{{printf \"%d %d %.0f\\n\", f, d, b}}'; \
         [ -d {p} ] && find {p} -maxdepth 0 -printf ok 2>/dev/null"
    );
    let out = sh(conn, &cmd).await.ok()?;
    parse_du(&out.stdout_lossy())
}

pub(crate) fn parse_du(out: &str) -> Option<DiskUsage> {
    let mut lines = out.lines();
    let counts = lines.next()?;
    // The second line proves `find -printf` worked (busybox prints nothing).
    if !lines.next().is_some_and(|l| l.trim() == "ok") {
        return None;
    }
    let mut it = counts.split_whitespace().map(|x| x.parse::<u64>().ok());
    Some(DiskUsage {
        files: it.next()??,
        dirs: it.next()??,
        bytes: it.next()??,
        complete: true,
    })
}

/// User and group names for numeric ids (`getent`, else `/etc/passwd`).
pub(crate) async fn lookup_ids(
    conn: &Connection,
    uid: Option<u32>,
    gid: Option<u32>,
) -> (Option<String>, Option<String>) {
    let q = |db: &str, file: &str, id: Option<u32>| match id {
        Some(id) => format!(
            "(getent {db} {id} 2>/dev/null || awk -F: '$3=={id}' {file}) | head -n 1 | cut -d: -f1"
        ),
        None => "echo".to_owned(),
    };
    let cmd = format!(
        "{}; {}",
        q("passwd", "/etc/passwd", uid),
        q("group", "/etc/group", gid)
    );
    let Ok(out) = sh(conn, &cmd).await else {
        return (None, None);
    };
    let text = out.stdout_lossy();
    let mut lines = text.lines().map(|l| l.trim().to_owned());
    let nonempty = |s: Option<String>| s.filter(|s| !s.is_empty());
    let user = nonempty(lines.next());
    let group = nonempty(lines.next());
    (uid.and(user), gid.and(group))
}

/// Run a command that should succeed silently; its stderr becomes the error.
pub(crate) async fn run(conn: &Connection, path: &str, cmd: &str) -> Result<(), FsError> {
    let out = sh(conn, cmd).await?;
    if out.exit_status == Some(0) {
        return Ok(());
    }
    Err(command_error(path, &String::from_utf8_lossy(&out.stderr)))
}

/// Map `ls`/`rm`/`mkdir` messages onto [`FsError`] kinds.
pub(crate) fn command_error(path: &str, stderr: &str) -> FsError {
    let msg = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("command failed")
        .to_owned();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("no such file") || lower.contains("not found") {
        FsError::NotFound(path.to_owned())
    } else if lower.contains("permission denied") || lower.contains("operation not permitted") {
        FsError::PermissionDenied(path.to_owned())
    } else {
        FsError::Remote {
            path: path.to_owned(),
            message: msg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests() {
        let h = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            parse_digest(&format!("{h}  /tmp/a b\n"), HashAlgo::Sha256).as_deref(),
            Some(h)
        );
        assert_eq!(
            parse_digest(&format!("\\{h}  /tmp/a\\nb\n"), HashAlgo::Sha256).as_deref(),
            Some(h)
        );
        assert_eq!(parse_digest("", HashAlgo::Sha256), None);
        assert_eq!(
            parse_digest("sha256sum: x: No such file", HashAlgo::Sha256),
            None
        );
        assert_eq!(
            parse_digest("D41D8CD98F00B204E9800998ECF8427E\n", HashAlgo::Md5).as_deref(),
            Some("d41d8cd98f00b204e9800998ecf8427e")
        );
    }

    #[test]
    fn du_output() {
        assert_eq!(
            parse_du("3 2 1234\nok"),
            Some(DiskUsage {
                files: 3,
                dirs: 2,
                bytes: 1234,
                complete: true
            })
        );
        // busybox: find has no -printf, so no "ok".
        assert_eq!(parse_du("0 0 0\n"), None);
        assert_eq!(parse_du(""), None);
    }

    #[test]
    fn errors() {
        assert!(
            command_error("/x", "rm: cannot remove '/x': No such file or directory").is_not_found()
        );
        assert!(matches!(
            command_error(
                "/x",
                "mkdir: cannot create directory '/x': Permission denied\n"
            ),
            FsError::PermissionDenied(_)
        ));
        assert!(matches!(
            command_error("/x", "rmdir: failed to remove '/x': Directory not empty"),
            FsError::Remote { message, .. } if message.contains("not empty")
        ));
    }
}
