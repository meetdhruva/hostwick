//! End-to-end tests for sudo SFTP, the SCP fallback and file properties,
//! against real OpenSSH servers.
//!
//! Skipped unless `HOSTWICK_FILES_PORT` is set; `scripts/e2e-files.sh`
//! creates the users (`hwfiles` password sudo, `hwfiles2` NOPASSWD,
//! `hwfiles3` no sudo, `hwfiles4` requiretty) and starts the servers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hostwick_core::{AuthMethod, FileProtocol, HostProfile, SecretString, SudoOptions};
use hostwick_sftp::{
    EntryKind, FsError, HashAlgo, LocalFs, RemoteFs, ScpFs, SftpFs, SftpOptions, SudoError,
    open_remote_fs, open_remote_fs_with,
};
use hostwick_ssh::{
    ConnectContext, Connection, HostKeyDecision, HostKeyInfo, Interaction, NoCredentials,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Env {
    port: u16,
    nosftp_port: u16,
    key: String,
    password: String,
}

fn env() -> Option<Env> {
    let port = std::env::var("HOSTWICK_FILES_PORT").ok()?.parse().ok()?;
    let v = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
    Some(Env {
        port,
        nosftp_port: v("HOSTWICK_FILES_NOSFTP_PORT").parse().unwrap(),
        key: v("HOSTWICK_FILES_KEY"),
        password: v("HOSTWICK_FILES_PASSWORD"),
    })
}

macro_rules! require_env {
    () => {
        match env() {
            Some(e) => e,
            None => {
                eprintln!("skipping: HOSTWICK_FILES_PORT not set");
                return;
            }
        }
    };
}

struct Accept;

#[async_trait]
impl Interaction for Accept {
    async fn host_key(&self, _: &HostKeyInfo) -> HostKeyDecision {
        HostKeyDecision::AcceptOnce
    }
    async fn password(&self, _: &str, _: &str) -> Option<SecretString> {
        None
    }
    async fn key_passphrase(&self, _: &Path) -> Option<SecretString> {
        None
    }
    async fn keyboard_interactive(
        &self,
        _: &str,
        _: &str,
        _: &[(String, bool)],
    ) -> Option<Vec<String>> {
        None
    }
}

fn profile(e: &Env, user: &str, port: u16) -> HostProfile {
    let mut p = HostProfile::new("files-e2e", "127.0.0.1");
    p.port = port;
    p.user = Some(user.into());
    p.auth = vec![AuthMethod::KeyFile {
        path: e.key.clone().into(),
        passphrase_credential: None,
    }];
    p
}

async fn connect(e: &Env, user: &str, port: u16) -> Connection {
    let mut ctx = ConnectContext::new(Arc::new(Accept), Arc::new(NoCredentials));
    ctx.known_hosts = None;
    ctx.connect_timeout = Duration::from_secs(10);
    Connection::connect(&profile(e, user, port), &ctx)
        .await
        .unwrap()
}

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

async fn write_all(fs: &dyn RemoteFs, path: &str, data: &[u8]) {
    let mut w = fs
        .open_write_sized(path, 0, data.len() as u64)
        .await
        .unwrap();
    w.write_all(data).await.unwrap();
    w.shutdown().await.unwrap();
}

async fn read_all(fs: &dyn RemoteFs, path: &str) -> Vec<u8> {
    let mut r = fs.open_read(path, 0).await.unwrap();
    let mut v = Vec::new();
    r.read_to_end(&mut v).await.unwrap();
    v
}

/// Root-only work through an elevated session, then clean up.
async fn exercise_root(fs: &dyn RemoteFs, tag: &str) {
    let dir = format!("/root/hostwick-e2e-{tag}");
    let _ = fs.remove_file(&format!("{dir}/f")).await;
    let _ = fs.remove_dir(&dir).await;
    // /root is not readable by the login user: proves we are root.
    fs.list_dir("/root").await.unwrap();
    fs.mkdir(&dir).await.unwrap();
    write_all(fs, &format!("{dir}/f"), b"root data").await;
    let st = fs.stat(&format!("{dir}/f")).await.unwrap().unwrap();
    assert_eq!((st.uid, st.size), (Some(0), 9));
    assert_eq!(read_all(fs, &format!("{dir}/f")).await, b"root data");
    fs.remove_file(&format!("{dir}/f")).await.unwrap();
    fs.remove_dir(&dir).await.unwrap();
}

#[tokio::test]
async fn sudo_with_password() {
    let e = require_env!();
    let conn = connect(&e, "hwfiles", e.port).await;
    let sudo = SudoOptions {
        enabled: true,
        ..Default::default()
    };
    // No password: told to ask for one.
    let err = SftpFs::open_sudo(&conn, &sudo, None).await.err().unwrap();
    assert!(
        matches!(err, FsError::Sudo(SudoError::PasswordRequired)),
        "{err}"
    );
    // Wrong password: reported as such, quickly enough.
    let err = SftpFs::open_sudo(&conn, &sudo, Some(pw("not-the-password")))
        .await
        .err()
        .unwrap();
    assert!(
        matches!(err, FsError::Sudo(SudoError::WrongPassword)),
        "{err}"
    );
    // Right password.
    let fs = SftpFs::open_sudo(&conn, &sudo, Some(pw(&e.password)))
        .await
        .unwrap();
    assert_eq!(fs.sudo_user(), Some("root"));
    assert_eq!(fs.protocol(), "SFTP (sudo root)");
    exercise_root(&fs, "pw").await;

    // The prompting helper: wrong first, then right.
    let mut answers = vec![pw(&e.password), pw("wrong")];
    let mut asked = Vec::new();
    let fs = open_remote_fs_with(&conn, FileProtocol::Auto, Some(&sudo), |why| {
        asked.push(why);
        let a = answers.pop();
        async move { a }
    })
    .await
    .unwrap();
    assert_eq!(asked, vec![None, Some(SudoError::WrongPassword)]);
    exercise_root(fs.as_ref(), "ask").await;

    // As another user than root.
    let as_user = SudoOptions {
        enabled: true,
        user: "hwfiles2".into(),
        ..Default::default()
    };
    let fs = SftpFs::open_sudo(&conn, &as_user, Some(pw(&e.password)))
        .await
        .unwrap();
    let home = fs.canonicalize(".").await.unwrap();
    assert!(
        home.ends_with("hwfiles2") || home.starts_with("/home/hwfiles"),
        "{home}"
    );
}

#[tokio::test]
async fn sudo_nopasswd_and_refusals() {
    let e = require_env!();
    let sudo = SudoOptions {
        enabled: true,
        ..Default::default()
    };
    let conn = connect(&e, "hwfiles2", e.port).await;
    let fs = SftpFs::open_sudo(&conn, &sudo, None).await.unwrap();
    exercise_root(&fs, "nopasswd").await;
    // A password given for NOPASSWD sudo is simply not used.
    let fs = SftpFs::open_sudo(&conn, &sudo, Some(pw("unused")))
        .await
        .unwrap();
    fs.list_dir("/root").await.unwrap();
    // Explicit server path.
    let custom = SudoOptions {
        sftp_server: "/usr/lib/openssh/sftp-server".into(),
        ..sudo.clone()
    };
    SftpFs::open_sudo(&conn, &custom, None).await.unwrap();
    let missing = SudoOptions {
        sftp_server: "/nonexistent/sftp-server".into(),
        ..sudo.clone()
    };
    let err = SftpFs::open_sudo(&conn, &missing, None)
        .await
        .err()
        .unwrap();
    assert!(
        matches!(err, FsError::Sudo(SudoError::NoSftpServer(_))),
        "{err}"
    );

    let conn = connect(&e, "hwfiles3", e.port).await;
    let err = SftpFs::open_sudo(&conn, &sudo, Some(pw(&e.password)))
        .await
        .err()
        .unwrap();
    assert!(
        matches!(err, FsError::Sudo(SudoError::NotAllowed(ref m)) if m.contains("sudoers")),
        "{err}"
    );

    let conn = connect(&e, "hwfiles4", e.port).await;
    let err = SftpFs::open_sudo(&conn, &sudo, None).await.err().unwrap();
    assert!(
        matches!(err, FsError::Sudo(SudoError::RequiresTty)),
        "{err}"
    );
}

#[tokio::test]
async fn sftp_properties() {
    let e = require_env!();
    let conn = connect(&e, "hwfiles2", e.port).await;
    let fs = SftpFs::open(&conn, &SftpOptions::default()).await.unwrap();
    let home = fs.canonicalize(".").await.unwrap();
    properties(&fs, &home, "sftp").await;
}

/// chmod, chown (to our own group), symlinks, checksums, folder sizes.
async fn properties(fs: &dyn RemoteFs, home: &str, tag: &str) {
    let dir = format!("{home}/props-{tag}");
    let _ = cleanup(fs, &dir).await;
    fs.mkdir(&dir).await.unwrap();
    fs.mkdir(&format!("{dir}/sub dir")).await.unwrap();
    let f = format!("{dir}/it's a file.txt");
    write_all(fs, &f, b"hello\n").await;
    write_all(fs, &format!("{dir}/sub dir/b"), b"12345").await;

    fs.set_permissions(&f, 0o640).await.unwrap();
    let st = fs.stat(&f).await.unwrap().unwrap();
    assert_eq!(st.permissions, Some(0o640));
    let (uid, gid) = (st.uid.unwrap(), st.gid.unwrap());
    fs.set_owner(&f, None, Some(gid)).await.unwrap();
    let (u, g) = fs.lookup_ids(Some(uid), Some(gid)).await;
    assert_eq!(u.as_deref(), Some("hwfiles2"));
    assert_eq!(g.as_deref(), Some("hwfiles2"));
    // Not allowed: give the file to root.
    assert!(fs.set_owner(&f, Some(0), None).await.is_err());

    let link = format!("{dir}/link");
    fs.symlink("it's a file.txt", &link).await.unwrap();
    assert_eq!(fs.read_link(&link).await.unwrap(), "it's a file.txt");
    let listed = fs.list_dir(&dir).await.unwrap();
    let l = listed.iter().find(|x| x.name == "link").unwrap();
    assert_eq!(l.kind, EntryKind::Symlink);

    assert_eq!(
        fs.checksum(&f, HashAlgo::Sha256).await.unwrap(),
        "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
    );
    assert_eq!(
        fs.checksum(&f, HashAlgo::Md5).await.unwrap(),
        "b1946ac92492d2347c6235b4d2611184"
    );
    let du = fs.disk_usage(&dir).await.unwrap();
    assert_eq!((du.files, du.dirs, du.bytes), (3, 1, 6 + 5), "{du:?}");
    // The generic walk agrees (symlinks count as files, sized by the link).
    let walked = hostwick_sftp::walk_disk_usage(fs, &dir).await.unwrap();
    assert_eq!((walked.files, walked.dirs), (3, 1));

    cleanup(fs, &dir).await.unwrap();
}

async fn cleanup(fs: &dyn RemoteFs, dir: &str) -> Result<(), FsError> {
    for name in ["it's a file.txt", "link", "sub dir/b", "x", "y z", "moved"] {
        let _ = fs.remove_file(&format!("{dir}/{name}")).await;
    }
    let _ = fs.remove_dir(&format!("{dir}/sub dir")).await;
    fs.remove_dir(dir).await
}

#[tokio::test]
async fn scp_fallback() {
    let e = require_env!();
    let conn = connect(&e, "hwfiles2", e.nosftp_port).await;
    // No subsystem: SFTP fails fast, Auto falls back to SCP.
    let err = SftpFs::open(&conn, &SftpOptions::default())
        .await
        .err()
        .unwrap();
    assert!(matches!(err, FsError::Ssh(_)), "{err}");
    let p = profile(&e, "hwfiles2", e.nosftp_port);
    let fs = open_remote_fs(&conn, &p, None).await.unwrap();
    assert_eq!(fs.protocol(), "SCP");

    let home = fs.canonicalize(".").await.unwrap();
    assert_eq!(home, "/home/hwfiles2");
    let dir = format!("{home}/scp-test");
    let _ = cleanup(fs.as_ref(), &dir).await;
    fs.mkdir(&dir).await.unwrap();
    assert!(fs.stat(&dir).await.unwrap().unwrap().is_dir());
    assert!(fs.stat(&format!("{dir}/nope")).await.unwrap().is_none());

    // Contents both ways, including an empty file and odd names.
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    write_all(fs.as_ref(), &format!("{dir}/y z"), &data).await;
    assert_eq!(read_all(fs.as_ref(), &format!("{dir}/y z")).await, data);
    write_all(fs.as_ref(), &format!("{dir}/x"), b"").await;
    assert_eq!(read_all(fs.as_ref(), &format!("{dir}/x")).await, b"");
    // Unknown size: spooled.
    let mut w = fs.open_write(&format!("{dir}/x"), 0).await.unwrap();
    w.write_all(b"spooled").await.unwrap();
    w.shutdown().await.unwrap();
    assert_eq!(read_all(fs.as_ref(), &format!("{dir}/x")).await, b"spooled");
    // No resume over SCP.
    assert!(
        fs.open_read(&format!("{dir}/x"), 3)
            .await
            .err()
            .unwrap()
            .is_unsupported()
    );
    assert!(
        fs.open_write(&format!("{dir}/x"), 3)
            .await
            .err()
            .unwrap()
            .is_unsupported()
    );
    // Missing source.
    let err = fs
        .open_read(&format!("{dir}/missing"), 0)
        .await
        .err()
        .unwrap();
    assert!(err.is_not_found(), "{err}");

    let list = fs.list_dir(&dir).await.unwrap();
    let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["x", "y z"]);
    assert_eq!(list[1].size, 300_000);
    let local_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(list[1].modified.unwrap().abs_diff(local_now) < 120);

    fs.set_modified(&format!("{dir}/x"), 1_600_000_000)
        .await
        .unwrap();
    assert_eq!(
        fs.stat(&format!("{dir}/x"))
            .await
            .unwrap()
            .unwrap()
            .modified,
        Some(1_600_000_000)
    );
    // Rename never replaces.
    assert!(
        fs.rename(&format!("{dir}/x"), &format!("{dir}/y z"))
            .await
            .is_err()
    );
    fs.rename(&format!("{dir}/x"), &format!("{dir}/moved"))
        .await
        .unwrap();
    fs.remove_file(&format!("{dir}/moved")).await.unwrap();
    fs.remove_file(&format!("{dir}/y z")).await.unwrap();
    assert!(
        fs.remove_file(&format!("{dir}/y z"))
            .await
            .err()
            .unwrap()
            .is_not_found()
    );

    properties(fs.as_ref(), &home, "scp").await;
    cleanup(fs.as_ref(), &dir).await.unwrap();

    // Forcing SFTP on this server is a clear error; forcing SCP works.
    let mut sftp_only = p.clone();
    sftp_only.file_protocol = FileProtocol::Sftp;
    assert!(open_remote_fs(&conn, &sftp_only, None).await.is_err());
    let scp = ScpFs::open(&conn).await.unwrap();
    assert!(!scp.list_dir("/").await.unwrap().is_empty());
    // And sudo over SCP is refused with a reason.
    let mut sudo_scp = p.clone();
    sudo_scp.file_protocol = FileProtocol::Scp;
    sudo_scp.sudo.enabled = true;
    assert!(
        open_remote_fs(&conn, &sudo_scp, None)
            .await
            .err()
            .unwrap()
            .is_unsupported()
    );
    // …but sudo SFTP works without the subsystem (it runs the binary).
    sudo_scp.file_protocol = FileProtocol::Auto;
    let root = open_remote_fs(&conn, &sudo_scp, None).await.unwrap();
    root.list_dir("/root").await.unwrap();
}

#[tokio::test]
/// Runs everywhere (no server needed).
async fn local_properties() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFs::new(dir.path());
    fs.mkdir("/d").await.unwrap();
    write_all(&fs, "/d/f", b"hello\n").await;
    assert_eq!(
        fs.checksum("/d/f", HashAlgo::Sha256).await.unwrap(),
        "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
    );
    let du = fs.disk_usage("/").await.unwrap();
    assert_eq!((du.files, du.dirs, du.bytes, du.complete), (1, 1, 6, true));
    #[cfg(unix)]
    {
        fs.set_permissions("/d/f", 0o600).await.unwrap();
        assert_eq!(
            fs.stat("/d/f").await.unwrap().unwrap().permissions,
            Some(0o600)
        );
        fs.symlink("f", "/d/l").await.unwrap();
        assert_eq!(fs.read_link("/d/l").await.unwrap(), "f");
        let st = fs.stat("/d/f").await.unwrap().unwrap();
        fs.set_owner("/d/f", st.uid, st.gid).await.unwrap();
        let (u, _) = fs.lookup_ids(Some(0), None).await;
        assert_eq!(u.as_deref(), Some("root"));
    }
}

/// Hostile file names on a real server, over SCP (escaped `ls`) and SFTP:
/// a line break cannot fake a listing line, backslash names are skipped
/// (they climb out of folders on Windows), and a name crafted to break
/// out of fish's quoting is just a name to every helper command.
#[tokio::test]
async fn hostile_names_over_scp_and_sftp() {
    let e = require_env!();
    for port in [e.nosftp_port, e.port] {
        let conn = connect(&e, "hwfiles3", port).await;
        let p = profile(&e, "hwfiles3", port);
        let fs = open_remote_fs(&conn, &p, None).await.unwrap();
        let home = fs.canonicalize(".").await.unwrap();
        let dir = format!("{home}/hostile-{port}");
        let fake = "x\n-rw-r--r-- 1 a b 5 1700000000 fake";
        let fishy = "\\';touch PWNED;#";
        let backslash = "..\\..\\evil.bat";
        let setup = format!(
            "rm -rf {d}; mkdir {d} && cd {d} && printf 'a' > \"$(printf 'x\\n-rw-r--r-- 1 a b 5 1700000000 fake')\" && printf 'b' > {f} && printf 'c' > {b}",
            d = hostwick_sftp::shell::quote(&dir),
            f = hostwick_sftp::shell::quote(fishy),
            b = hostwick_sftp::shell::quote(backslash),
        );
        let out = conn.exec(&setup).await.unwrap();
        assert_eq!(
            out.exit_status,
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let listing = fs.list_dir_checked(&dir).await.unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, [fake], "{port}: injected line?");
        // Both names with a backslash are skipped (they would be paths on
        // Windows).
        let mut skipped: Vec<&str> = listing.skipped.iter().map(|(n, _)| n.as_str()).collect();
        skipped.sort_unstable();
        assert_eq!(skipped, [backslash, fishy], "{port}");

        // Helper commands treat the crafted name as data.
        let fishy_path = format!("{dir}/{fishy}");
        let sum = fs.checksum(&fishy_path, HashAlgo::Sha256).await.unwrap();
        assert_eq!(
            sum,
            "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d"
        );
        let du = fs.disk_usage(&dir).await.unwrap();
        assert_eq!(du.files, 3);
        assert!(
            !conn
                .exec(&format!(
                    "test -e {}/PWNED",
                    hostwick_sftp::shell::quote(&dir)
                ))
                .await
                .unwrap()
                .exit_status
                .eq(&Some(0))
        );

        // Replace atomically, links, lstat.
        write_all(fs.as_ref(), &format!("{dir}/new"), b"replacement").await;
        fs.rename_replace(&format!("{dir}/new"), &fishy_path)
            .await
            .unwrap();
        assert_eq!(read_all(fs.as_ref(), &fishy_path).await, b"replacement");
        assert!(fs.stat(&format!("{dir}/new")).await.unwrap().is_none());
        fs.symlink(&fishy_path, &format!("{dir}/sym"))
            .await
            .unwrap();
        assert_eq!(
            fs.lstat(&format!("{dir}/sym")).await.unwrap().unwrap().kind,
            EntryKind::Symlink
        );
        assert_eq!(
            fs.stat(&format!("{dir}/sym")).await.unwrap().unwrap().kind,
            EntryKind::File
        );
        fs.hard_link(&fishy_path, &format!("{dir}/hard"))
            .await
            .unwrap();
        assert_eq!(
            read_all(fs.as_ref(), &format!("{dir}/hard")).await,
            b"replacement"
        );

        let _ = conn
            .exec(&format!("rm -rf {}", hostwick_sftp::shell::quote(&dir)))
            .await;
    }
}
