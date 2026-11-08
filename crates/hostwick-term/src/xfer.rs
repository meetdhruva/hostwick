//! Shared plumbing for in-terminal file transfers (ZMODEM, trzsz): the
//! byte link to the remote program, progress reports and errors.
//!
//! While a transfer runs, the terminal view hands every byte the remote
//! program prints to the transfer (instead of the screen) and the transfer
//! writes its answers through the session's bounded input queue, so a slow
//! network slows the file reader instead of filling memory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use hostwick_ssh::ShellCommand;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// The remote program's output (in) and its input (out).
pub struct Link {
    pub rx: mpsc::Receiver<Bytes>,
    pub tx: mpsc::Sender<ShellCommand>,
    /// Bytes read from `rx` that the protocol has not consumed yet.
    pub(crate) spare: Vec<u8>,
}

impl Link {
    pub fn new(rx: mpsc::Receiver<Bytes>, tx: mpsc::Sender<ShellCommand>) -> Self {
        Self {
            rx,
            tx,
            spare: Vec::new(),
        }
    }

    pub async fn send(&self, data: Vec<u8>) -> Result<(), XferError> {
        if data.is_empty() {
            return Ok(());
        }
        self.tx
            .send(ShellCommand::Data(Bytes::from(data)))
            .await
            .map_err(|_| XferError::Closed)
    }

    /// Everything received but not consumed (to show in the terminal after
    /// the transfer), including what is still queued.
    pub fn leftover(&mut self) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.spare);
        while let Ok(b) = self.rx.try_recv() {
            out.extend_from_slice(&b);
        }
        out
    }
}

/// How far a transfer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// The file being transferred.
    pub name: String,
    pub done: u64,
    pub total: u64,
    /// 1-based index of the file, and how many there are (0 = unknown).
    pub index: usize,
    pub count: usize,
}

/// A transfer ended without finishing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum XferError {
    #[error("cancelled")]
    Cancelled,
    #[error("the other side cancelled")]
    RemoteCancelled,
    #[error("the connection closed")]
    Closed,
    #[error("no answer from the other side")]
    Timeout,
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    Io(String),
    /// The remote program reported a failure (trzsz).
    #[error("{0}")]
    Remote(String),
}

impl From<std::io::Error> for XferError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// What a finished transfer produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Files written (download) or sent (upload).
    pub files: Vec<PathBuf>,
    pub bytes: u64,
}

pub type ProgressFn<'a> = &'a mut (dyn FnMut(&Progress) + Send);

/// A file to send.
#[derive(Debug, Clone)]
pub struct SendFile {
    pub path: PathBuf,
    /// Name the other side sees (relative, `/`-separated for folders).
    pub name: String,
}

impl SendFile {
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(Self {
            path: path.to_owned(),
            name,
        })
    }
}

/// Wait for the next chunk from the remote side, or the end of the
/// transfer: cancel, timeout or closed link.
pub(crate) async fn next_chunk(
    link: &mut Link,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<Bytes, XferError> {
    if cancel.is_cancelled() {
        return Err(XferError::Cancelled);
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(XferError::Cancelled),
        r = tokio::time::timeout(timeout, link.rx.recv()) => match r {
            Ok(Some(b)) => Ok(b),
            Ok(None) => Err(XferError::Closed),
            Err(_) => Err(XferError::Timeout),
        },
    }
}

/// Unicode "format" characters (category Cf) plus the line and paragraph
/// separators: invisible, but they reorder or hide text (a bidi override
/// makes `invoice\u{202e}fdp.exe` look like `invoiceexe.pdf`).
pub fn is_invisible_format(c: char) -> bool {
    matches!(c as u32,
        0x00AD
        | 0x0600..=0x0605
        | 0x061C
        | 0x06DD
        | 0x070F
        | 0x0890..=0x0891
        | 0x08E2
        | 0x180E
        | 0x200B..=0x200F
        | 0x2028..=0x202E
        | 0x2060..=0x2064
        | 0x2066..=0x206F
        | 0xFEFF
        | 0xFFF9..=0xFFFB
        | 0x110BD
        | 0x110CD
        | 0x13430..=0x1343F
        | 0x1BCA0..=0x1BCA3
        | 0x1D173..=0x1D17A
        | 0xE0001
        | 0xE0020..=0xE007F)
}

/// Windows device names, which cannot be files whatever their extension.
fn is_reserved_device(stem: &str) -> bool {
    let up = stem.trim_end_matches([' ', '.']).to_uppercase();
    matches!(
        up.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) || {
        let digits = |rest: &str| {
            let mut it = rest.chars();
            matches!(
                (it.next(), it.next()),
                (Some('1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
            )
        };
        up.strip_prefix("COM").is_some_and(digits) || up.strip_prefix("LPT").is_some_and(digits)
    }
}

/// Longest name (in bytes) that is kept; leaves room for ` (n)` and for the
/// `.part` working name inside the 255 byte limit of common file systems.
const MAX_NAME_BYTES: usize = 200;

/// A file name that is safe to create in a download folder: the last path
/// component only, no control or invisible formatting characters, no
/// characters Windows rejects, no trailing dots or spaces, never a Windows
/// device name, never empty, `.` or `..`.
pub fn safe_file_name(raw: &str) -> Option<String> {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !is_invisible_format(*c))
        .map(|c| {
            // Windows drive prefixes and reserved characters.
            if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        return None;
    }
    // `CON`, `con.txt`, `Nul.tar.gz`: the device name is what comes before
    // the first dot.
    let (stem, ext) = match cleaned.find('.') {
        Some(i) if i > 0 => cleaned.split_at(i),
        _ => (cleaned, ""),
    };
    let mut stem = stem.trim_end().to_owned();
    if is_reserved_device(&stem) {
        stem.push('_');
    }
    let mut name = format!("{stem}{ext}");
    if name.len() > MAX_NAME_BYTES {
        // Keep a short extension, shorten the stem.
        let ext = if ext.len() <= 16 { ext } else { "" };
        let room = MAX_NAME_BYTES - ext.len();
        let mut cut = room.min(stem.len());
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        stem.truncate(cut);
        name = format!("{stem}{ext}");
    }
    Some(name)
}

fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// The names to try for `name` in turn: itself, then `stem (1).ext`, `(2)`…
fn candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = split_ext(name);
    std::iter::once(name.to_owned()).chain((1..10_000).map(move |n| format!("{stem} ({n}){ext}")))
}

/// What [`create_unique`] would probably pick now: `dir/name`, or
/// `dir/name (1).ext`, `(2)`… when that exists. Advisory only (a name shown
/// to the user or the remote program); the file is made by [`create_unique`]
/// or [`PartFile::commit`], which cannot race. Never returns a path that
/// exists, not even after running out of numbers.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    for c in candidates(name) {
        let p = dir.join(&c);
        if std::fs::symlink_metadata(&p).is_err() {
            return p;
        }
    }
    let (stem, ext) = split_ext(name);
    let mut n = u64::from(std::process::id());
    loop {
        let p = dir.join(format!("{stem}-{n}{ext}"));
        if std::fs::symlink_metadata(&p).is_err() {
            return p;
        }
        n += 1;
    }
}

/// Open `path` for writing, failing when anything (even a dangling link)
/// is already there. `private`: owner-only on Unix.
fn create_new(path: &Path, private: bool) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt as _;
        o.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    o.open(path)
}

/// Create `dir/name`, or `dir/name (1).ext`, `(2)`… when that exists, with
/// `create_new` so an existing file is never opened, whatever happens in
/// between. Returns the empty file and its path.
pub fn create_unique(dir: &Path, name: &str) -> std::io::Result<(std::fs::File, PathBuf)> {
    for c in candidates(name) {
        let p = dir.join(&c);
        match create_new(&p, false) {
            Ok(f) => return Ok((f, p)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    let (stem, ext) = split_ext(name);
    let pid = std::process::id();
    for n in 0..10_000u32 {
        let p = dir.join(format!("{stem}-{pid}-{n}{ext}"));
        match create_new(&p, false) {
            Ok(f) => return Ok((f, p)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("no free name for {name} in {}", dir.display()),
    ))
}

/// A download in progress: written to a `.part` file (created with
/// `create_new`, owner-only on Unix) and moved to its final, unique name
/// only by [`PartFile::commit`]. Dropped without committing (cancel, error,
/// a new file starting mid-file, the task being aborted) the `.part` file is
/// deleted, so a truncated download never sits under the real name.
pub struct PartFile {
    file: Option<tokio::fs::File>,
    path: PathBuf,
    dir: PathBuf,
    name: String,
}

impl PartFile {
    /// Start a download of `name` (already made safe) into `dir`.
    pub fn create(dir: &Path, name: &str) -> std::io::Result<Self> {
        let mut base = name.to_owned();
        while base.len() > MAX_NAME_BYTES {
            base.pop();
        }
        for n in 0..10_000u32 {
            let part = if n == 0 {
                format!("{base}.part")
            } else {
                format!("{base}.{n}.part")
            };
            let p = dir.join(part);
            match create_new(&p, true) {
                Ok(f) => {
                    return Ok(Self {
                        file: Some(tokio::fs::File::from_std(f)),
                        path: p,
                        dir: dir.to_owned(),
                        name: name.to_owned(),
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("no free working name for {name}"),
        ))
    }

    /// Where the data goes while the download runs.
    pub fn part_path(&self) -> &Path {
        &self.path
    }

    pub fn file(&mut self) -> &mut tokio::fs::File {
        // `file` is only taken by `commit`, which consumes `self`.
        self.file
            .as_mut()
            .unwrap_or_else(|| unreachable!("part file used after commit"))
    }

    /// The download is complete: flush, set the modification time, move to
    /// the final name (never replacing an existing file) and return it.
    pub async fn commit(
        mut self,
        mtime: Option<std::time::SystemTime>,
    ) -> std::io::Result<PathBuf> {
        use tokio::io::AsyncWriteExt as _;
        let mut file = self
            .file
            .take()
            .unwrap_or_else(|| unreachable!("part file committed twice"));
        file.flush().await?;
        let std_file = file.into_std().await;
        if let Some(t) = mtime {
            let _ = std_file.set_modified(t);
        }
        drop(std_file);
        let (dir, name, part) = (self.dir.clone(), self.name.clone(), self.path.clone());
        let r = tokio::task::spawn_blocking(move || publish(&part, &dir, &name))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if r.is_ok() {
            // The part file is gone (renamed); nothing for Drop to delete.
            self.path = PathBuf::new();
        }
        r
    }
}

/// Reserve the first free final name with `create_new` (the empty file
/// takes the user's default permissions), then rename the finished `.part`
/// file over that reservation.
fn publish(part: &Path, dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let (reservation, target) = create_unique(dir, name)?;
    #[cfg(unix)]
    if let Ok(m) = reservation.metadata() {
        let _ = std::fs::set_permissions(part, m.permissions());
    }
    drop(reservation);
    match std::fs::rename(part, &target) {
        Ok(()) => Ok(target),
        Err(e) => {
            let _ = std::fs::remove_file(&target);
            Err(e)
        }
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        self.file = None;
        if !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_file_name("a.txt").as_deref(), Some("a.txt"));
        assert_eq!(
            safe_file_name("../../etc/passwd").as_deref(),
            Some("passwd")
        );
        assert_eq!(
            safe_file_name("C:\\Windows\\x.exe").as_deref(),
            Some("x.exe")
        );
        assert_eq!(safe_file_name("..").as_deref(), None);
        assert_eq!(safe_file_name("").as_deref(), None);
        assert_eq!(safe_file_name("dir/").as_deref(), None);
        assert_eq!(safe_file_name("a\u{7}b").as_deref(), Some("ab"));
        assert_eq!(safe_file_name("a:b").as_deref(), Some("a_b"));
    }

    #[test]
    fn windows_device_names_get_an_underscore() {
        for (raw, want) in [
            ("CON", "CON_"),
            ("con", "con_"),
            ("NUL.txt", "NUL_.txt"),
            ("Prn.tar.gz", "Prn_.tar.gz"),
            ("aux.", "aux_"),
            ("COM1", "COM1_"),
            ("com9.log", "com9_.log"),
            ("LPT3.x", "LPT3_.x"),
            ("lpt1 .txt", "lpt1_.txt"),
            ("CONIN$", "CONIN$_"),
            ("dir/CON.txt", "CON_.txt"),
            // Not device names.
            ("COM0", "COM0"),
            ("COM10", "COM10"),
            ("CONSOLE", "CONSOLE"),
            ("com1x.txt", "com1x.txt"),
            ("my.con", "my.con"),
            ("nulled", "nulled"),
        ] {
            assert_eq!(safe_file_name(raw).as_deref(), Some(want), "{raw}");
        }
    }

    #[test]
    fn invisible_formatting_characters_are_stripped() {
        // Right-to-left override, isolates, zero width, BOM, soft hyphen,
        // tag characters, line separator.
        assert_eq!(
            safe_file_name("invoice\u{202e}fdp.exe").as_deref(),
            Some("invoicefdp.exe")
        );
        assert_eq!(
            safe_file_name("a\u{2066}b\u{2069}c\u{200b}d\u{feff}e\u{ad}f\u{e0041}g\u{2028}h")
                .as_deref(),
            Some("abcdefgh")
        );
        // Ordinary non-ASCII text stays.
        assert_eq!(
            safe_file_name("grüße ✓ 日本語 עברית.txt").as_deref(),
            Some("grüße ✓ 日本語 עברית.txt")
        );
        // A name that is nothing but invisible characters is refused.
        assert_eq!(safe_file_name("\u{202e}\u{200b}"), None);
    }

    #[test]
    fn trailing_dots_and_spaces_are_removed() {
        assert_eq!(
            safe_file_name("report.txt. . ").as_deref(),
            Some("report.txt")
        );
        assert_eq!(safe_file_name("x \u{202e}.").as_deref(), Some("x"));
        assert_eq!(safe_file_name("...").as_deref(), None);
        assert_eq!(safe_file_name(" . ").as_deref(), None);
        assert_eq!(safe_file_name(".bashrc").as_deref(), Some(".bashrc"));
    }

    #[test]
    fn very_long_names_are_shortened_on_a_char_boundary() {
        let long = format!("{}.tar.gz", "é".repeat(300));
        let n = safe_file_name(&long).unwrap_or_default();
        assert!(n.len() <= MAX_NAME_BYTES, "{}", n.len());
        assert!(n.ends_with(".gz"));
    }

    #[test]
    fn unique_names_do_not_overwrite() {
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unique_path(d.path(), "f.txt"), d.path().join("f.txt"));
        std::fs::write(d.path().join("f.txt"), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unique_path(d.path(), "f.txt"), d.path().join("f (1).txt"));
        std::fs::write(d.path().join("f (1).txt"), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unique_path(d.path(), "f.txt"), d.path().join("f (2).txt"));
        assert_eq!(unique_path(d.path(), ".bashrc"), d.path().join(".bashrc"));
    }

    #[test]
    fn create_unique_never_opens_an_existing_file() {
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(d.path().join("f.txt"), b"keep").unwrap_or_else(|e| panic!("{e}"));
        let (_f, p) = create_unique(d.path(), "f.txt").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p, d.path().join("f (1).txt"));
        let (_g, p2) = create_unique(d.path(), "f.txt").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p2, d.path().join("f (2).txt"));
        assert_eq!(
            std::fs::read(d.path().join("f.txt")).unwrap_or_default(),
            b"keep"
        );
    }

    #[test]
    fn create_unique_survives_exhausted_numbers_without_returning_an_existing_path() {
        // Every `(n)` name taken: the fallback must still find a new file.
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(d.path().join("g"), b"0").unwrap_or_else(|e| panic!("{e}"));
        for n in 1..10_000 {
            std::fs::write(d.path().join(format!("g ({n})")), b"x")
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let before = std::fs::read_dir(d.path()).map(|r| r.count()).unwrap_or(0);
        let hint = unique_path(d.path(), "g");
        assert!(!hint.exists(), "{hint:?}");
        let (_f, p) = create_unique(d.path(), "g").unwrap_or_else(|e| panic!("{e}"));
        assert!(p.exists());
        let after = std::fs::read_dir(d.path()).map(|r| r.count()).unwrap_or(0);
        assert_eq!(after, before + 1);
        // The pre-existing files kept their content.
        assert_eq!(std::fs::read(d.path().join("g")).unwrap_or_default(), b"0");
    }

    #[test]
    fn create_unique_does_not_follow_a_dangling_link() {
        #[cfg(unix)]
        {
            let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
            let victim = d.path().join("victim");
            std::os::unix::fs::symlink(&victim, d.path().join("f.txt"))
                .unwrap_or_else(|e| panic!("{e}"));
            let (_f, p) = create_unique(d.path(), "f.txt").unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(p, d.path().join("f (1).txt"));
            assert!(!victim.exists(), "the link target must not be created");
        }
    }

    #[test]
    fn concurrent_creators_get_distinct_files() {
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dir = d.path().to_owned();
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || create_unique(&dir, "same.bin").map(|(_, p)| p))
            })
            .collect();
        let mut paths: Vec<PathBuf> = handles
            .into_iter()
            .filter_map(|h| h.join().ok().and_then(Result::ok))
            .collect();
        assert_eq!(paths.len(), 16);
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 16);
    }

    fn names_in(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    #[tokio::test]
    async fn part_files_appear_under_the_real_name_only_when_committed() {
        use tokio::io::AsyncWriteExt as _;
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut p = PartFile::create(d.path(), "a.bin").unwrap_or_else(|e| panic!("{e}"));
        p.file()
            .write_all(b"hello")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(names_in(d.path()), ["a.bin.part"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(p.part_path())
                .map(|m| m.permissions().mode() & 0o777)
                .unwrap_or(0);
            assert_eq!(mode, 0o600);
        }
        let done = p.commit(None).await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(done, d.path().join("a.bin"));
        assert_eq!(names_in(d.path()), ["a.bin"]);
        assert_eq!(std::fs::read(&done).unwrap_or_default(), b"hello");
    }

    #[tokio::test]
    async fn dropping_a_part_file_deletes_it() {
        use tokio::io::AsyncWriteExt as _;
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut p = PartFile::create(d.path(), "a.bin").unwrap_or_else(|e| panic!("{e}"));
        p.file()
            .write_all(b"half")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        drop(p);
        assert!(names_in(d.path()).is_empty());
    }

    #[tokio::test]
    async fn committing_never_replaces_an_existing_file_and_part_names_do_not_clash() {
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(d.path().join("a.bin"), b"old").unwrap_or_else(|e| panic!("{e}"));
        // A stale part file from an earlier crash is left alone too.
        std::fs::write(d.path().join("a.bin.part"), b"stale").unwrap_or_else(|e| panic!("{e}"));
        let p1 = PartFile::create(d.path(), "a.bin").unwrap_or_else(|e| panic!("{e}"));
        let p2 = PartFile::create(d.path(), "a.bin").unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(p1.part_path(), p2.part_path());
        let a = p1.commit(None).await.unwrap_or_else(|e| panic!("{e}"));
        let b = p2.commit(None).await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(a, d.path().join("a (1).bin"));
        assert_eq!(b, d.path().join("a (2).bin"));
        assert_eq!(
            std::fs::read(d.path().join("a.bin")).unwrap_or_default(),
            b"old"
        );
        assert_eq!(
            std::fs::read(d.path().join("a.bin.part")).unwrap_or_default(),
            b"stale"
        );
    }
}

// ---- spotting the start of a transfer in terminal output ---------------

/// What started: a ZMODEM peer (`sz`/`rz`) or a trzsz server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    Zmodem(crate::zmodem::Start),
    Trzsz(crate::trzsz::Magic),
}

/// The result of looking at a chunk of output.
#[derive(Debug, PartialEq, Eq)]
pub enum Scan {
    /// Nothing special: show the chunk as it is.
    Pass,
    /// Show `pass`; if `start` is set a transfer began and `rest` is what
    /// belongs to it (ZMODEM: from the first header byte; trzsz: after the
    /// magic line).
    Split {
        pass: Vec<u8>,
        start: Option<(Detected, Vec<u8>)>,
    },
}

/// Finds transfer starts in output, even when split over several chunks.
/// A few bytes that might be the beginning of a start sequence are held
/// back until the next chunk shows what they are.
#[derive(Debug, Default)]
pub struct Detector {
    held: Vec<u8>,
}

impl Detector {
    pub fn scan(&mut self, chunk: &[u8]) -> Scan {
        use crate::{trzsz, zmodem};
        let joined;
        let buf: &[u8] = if self.held.is_empty() {
            chunk
        } else {
            joined = [self.held.as_slice(), chunk].concat();
            self.held.clear();
            &joined
        };
        let z = zmodem::find_start(buf);
        let t = trzsz::find_magic(buf);
        // Whichever comes first.
        match (z, t) {
            (Some((zi, kind)), t) if t.as_ref().is_none_or(|(ti, _, _)| zi < *ti) => {
                return Scan::Split {
                    pass: buf[..zi].to_vec(),
                    start: Some((Detected::Zmodem(kind), buf[zi..].to_vec())),
                };
            }
            (_, Some((ti, end, magic))) => {
                return Scan::Split {
                    pass: buf[..ti].to_vec(),
                    start: Some((Detected::Trzsz(magic), buf[end..].to_vec())),
                };
            }
            _ => {}
        }
        let hold = zmodem::partial_start_len(buf).max(trzsz::partial_magic_len(buf));
        if hold == 0 {
            return if std::ptr::eq(buf.as_ptr(), chunk.as_ptr()) {
                Scan::Pass
            } else {
                Scan::Split {
                    pass: buf.to_vec(),
                    start: None,
                }
            };
        }
        let cut = buf.len() - hold;
        self.held = buf[cut..].to_vec();
        Scan::Split {
            pass: buf[..cut].to_vec(),
            start: None,
        }
    }

    /// Bytes are being held back.
    pub fn is_holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// Output ended, paused or a transfer is not wanted: give back what was
    /// held.
    pub fn flush(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod detector_tests {
    use super::*;
    use crate::zmodem::{Start, ZRINIT, ZRQINIT, hex_header};

    #[test]
    fn plain_output_passes_untouched() {
        let mut d = Detector::default();
        assert_eq!(d.scan(b"hello\r\n$ "), Scan::Pass);
        assert_eq!(d.scan(b"a * b ** c"), Scan::Pass);
    }

    #[test]
    fn zmodem_start_splits_the_chunk() {
        let mut d = Detector::default();
        let mut wire = b"rz\r".to_vec();
        wire.extend(hex_header(ZRQINIT, [0; 4]));
        match d.scan(&wire) {
            Scan::Split {
                pass,
                start: Some((Detected::Zmodem(Start::Download), rest)),
            } => {
                assert_eq!(pass, b"rz\r");
                assert!(rest.starts_with(b"**\x18B00"));
            }
            other => panic!("{other:?}"),
        }
        let up = hex_header(ZRINIT, [0, 0, 0, 0x23]);
        assert!(matches!(
            d.scan(&up),
            Scan::Split {
                start: Some((Detected::Zmodem(Start::Upload), _)),
                ..
            }
        ));
    }

    #[test]
    fn a_start_split_over_chunks_is_still_found() {
        let wire = [b"text".to_vec(), hex_header(ZRQINIT, [0; 4])].concat();
        for cut in 1..12 {
            let mut d = Detector::default();
            let mut shown = Vec::new();
            let mut found = None;
            for part in [&wire[..cut], &wire[cut..]] {
                if found.is_some() {
                    break;
                }
                match d.scan(part) {
                    Scan::Pass => shown.extend_from_slice(part),
                    Scan::Split { pass, start } => {
                        shown.extend(pass);
                        if start.is_some() {
                            found = start;
                        }
                    }
                }
            }
            assert_eq!(shown, b"text", "cut {cut}");
            assert!(found.is_some(), "cut {cut}");
        }
    }

    #[test]
    fn trzsz_magic_line_is_found_and_hidden() {
        let mut d = Detector::default();
        let wire = b"\x1b7::TRZSZ:TRANSFER:R:1.1.7:1234567890123:9\r\nmore";
        match d.scan(wire) {
            Scan::Split {
                pass,
                start: Some((Detected::Trzsz(m), rest)),
            } => {
                assert_eq!(pass, b"\x1b7");
                assert_eq!(rest, b"more");
                assert_eq!(m.mode, crate::trzsz::Mode::Upload);
            }
            other => panic!("{other:?}"),
        }
        // Split mid-line.
        let mut d = Detector::default();
        assert!(matches!(
            d.scan(b"x::TRZSZ:TRANSFER:S:1.1"),
            Scan::Split { start: None, .. }
        ));
        assert!(matches!(
            d.scan(b".7:123\r\n"),
            Scan::Split {
                start: Some((Detected::Trzsz(_), _)),
                ..
            }
        ));
    }

    #[test]
    fn held_bytes_are_released_when_they_turn_out_harmless() {
        let mut d = Detector::default();
        let first = d.scan(b"ab**");
        assert!(matches!(&first, Scan::Split { pass, start: None } if pass == b"ab"));
        match d.scan(b" cd") {
            Scan::Split { pass, start: None } => assert_eq!(pass, b"** cd"),
            other => panic!("{other:?}"),
        }
        assert!(d.flush().is_empty());
    }
}

/// Helpers for tests: in-memory links and a program on the other end of a
/// pipe.
#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// Two links wired to each other.
    pub fn loopback() -> (Link, Link) {
        let (a_out, mut a_out_rx) = mpsc::channel::<ShellCommand>(256);
        let (b_out, mut b_out_rx) = mpsc::channel::<ShellCommand>(256);
        let (to_a, a_in) = mpsc::channel::<Bytes>(256);
        let (to_b, b_in) = mpsc::channel::<Bytes>(256);
        tokio::spawn(async move {
            while let Some(c) = a_out_rx.recv().await {
                if let ShellCommand::Data(d) = c {
                    let _ = to_b.send(d).await;
                }
            }
        });
        tokio::spawn(async move {
            while let Some(c) = b_out_rx.recv().await {
                if let ShellCommand::Data(d) = c {
                    let _ = to_a.send(d).await;
                }
            }
        });
        (Link::new(a_in, a_out), Link::new(b_in, b_out))
    }

    /// Run `args` (stdin/stdout piped, no terminal) in `cwd` and link to it.
    pub async fn spawn_link(args: &[&str], cwd: &Path) -> (Link, tokio::process::Child) {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let mut child = tokio::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("{}: {e}", args[0]));
        let mut stdin = child.stdin.take().unwrap_or_else(|| panic!("stdin"));
        let mut stdout = child.stdout.take().unwrap_or_else(|| panic!("stdout"));
        let (out, mut out_rx) = mpsc::channel::<ShellCommand>(256);
        let (to_link, from_child) = mpsc::channel::<Bytes>(256);
        tokio::spawn(async move {
            while let Some(c) = out_rx.recv().await {
                if let ShellCommand::Data(d) = c
                    && stdin.write_all(&d).await.is_err()
                {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if to_link
                            .send(Bytes::copy_from_slice(&buf[..n]))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        (Link::new(from_child, out), child)
    }
}
