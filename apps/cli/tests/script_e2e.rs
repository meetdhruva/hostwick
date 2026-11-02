//! End-to-end test of `hostwick script` (and the `--json` outputs) against
//! a real OpenSSH server.
//!
//! Skipped unless `HOSTWICK_TOOLS_E2E_PORT` is set. Needs:
//! - `HOSTWICK_TOOLS_E2E_PORT`, `HOSTWICK_TOOLS_E2E_USER`
//! - `HOSTWICK_TOOLS_E2E_KEY`: a passphrase-less private key for that user
//! - `HOSTWICK_TOOLS_E2E_HOST_PUB` (optional): the server's host public key,
//!   to test `open -hostkey=`
//!
//! `HOSTWICK_TOOLS_E2E_HOST` defaults to 127.0.0.1.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Env {
    host: String,
    port: String,
    user: String,
    key: PathBuf,
    host_pub: Option<PathBuf>,
}

fn env() -> Option<Env> {
    let port = std::env::var("HOSTWICK_TOOLS_E2E_PORT").ok()?;
    let v = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
    Some(Env {
        host: std::env::var("HOSTWICK_TOOLS_E2E_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port,
        user: v("HOSTWICK_TOOLS_E2E_USER"),
        key: v("HOSTWICK_TOOLS_E2E_KEY").into(),
        host_pub: std::env::var("HOSTWICK_TOOLS_E2E_HOST_PUB")
            .ok()
            .map(Into::into),
    })
}

struct Ctx {
    env: Env,
    home: tempfile::TempDir,
    local: tempfile::TempDir,
    base: String,
}

impl Ctx {
    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_hostwick"));
        c.env("HOSTWICK_HOME", self.home.path())
            .env_remove("HOSTWICK_LOG")
            .current_dir(self.local.path());
        c
    }

    /// Run a script given on stdin with the standard variables defined.
    fn script(&self, extra: &[&str], text: &str) -> Output {
        let mut c = self.cmd();
        c.args(["--batch", "script", "-"])
            .args(["-D", &format!("USER={}", self.env.user)])
            .args(["-D", &format!("HOST={}", self.env.host)])
            .args(["-D", &format!("PORT={}", self.env.port)])
            .args(["-D", &format!("KEY={}", self.env.key.display())])
            .args(["-D", &format!("BASE={}", self.base)])
            .args(["-D", &format!("LOCAL={}", self.local.path().display())])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        }
        child.wait_with_output().unwrap()
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn ok(o: &Output) {
    assert!(
        o.status.success(),
        "exit {:?}\nstdout:\n{}\nstderr:\n{}",
        o.status.code(),
        text(&o.stdout),
        text(&o.stderr)
    );
}

/// All JSON values printed one after another.
fn json_values(s: &str) -> Vec<serde_json::Value> {
    serde_json::Deserializer::from_str(s)
        .into_iter::<serde_json::Value>()
        .map(Result::unwrap)
        .collect()
}

fn names(v: &serde_json::Value) -> Vec<String> {
    let mut n: Vec<String> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_owned())
        .collect();
    n.sort();
    n
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

const OPEN: &str = "open %USER%@%HOST%:%PORT% -privatekey=\"%KEY%\"";

#[test]
fn script_end_to_end() {
    let Some(env) = env() else {
        eprintln!("skipping: HOSTWICK_TOOLS_E2E_PORT not set");
        return;
    };
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let ctx = Ctx {
        env,
        home: tempfile::tempdir().unwrap(),
        local: tempfile::tempdir().unwrap(),
        base: format!("hw-script-{}-{nonce}", std::process::id()),
    };
    let l = ctx.local.path();
    write(&l.join("a.log"), "alpha\n");
    write(&l.join("b.log"), "bravo\n");
    write(&l.join("c.txt"), "charlie\n");
    write(&l.join("with space.txt"), "spaced\n");
    write(&l.join("sub/dir/deep.txt"), "deep\n");
    write(&l.join("mirror/keep.txt"), "keep\n");
    write(&l.join("mirror/skip.tmp"), "tmp\n");
    std::fs::create_dir_all(l.join("back")).unwrap();

    // Host key pinned with -hostkey (no known_hosts entry, --batch): the
    // connection only works because the fingerprint matches.
    let hostkey = ctx.env.host_pub.as_ref().map(|p| {
        let k = hostwick_keys::PublicKey::from_openssh(std::fs::read_to_string(p).unwrap().trim())
            .unwrap();
        hostwick_keys::fingerprint(&k)
    });
    let open = match &hostkey {
        Some(fp) => format!("{OPEN} -hostkey=\"ssh-ed25519 255 {fp}\""),
        None => OPEN.to_owned(),
    };
    let accept: &[&str] = if hostkey.is_some() {
        &[]
    } else {
        &["--accept-new"]
    };
    let mut args = vec!["--", "first-param"];
    args.splice(0..0, accept.iter().copied());

    let main = format!(
        r#"# Hostwick script e2e
option batch abort
option confirm off
{open}
echo connected %1% 100%%
mkdir %BASE%
cd %BASE%
pwd
lcd "%LOCAL%"
lpwd
put *.log
put "with space.txt" "renamed file.txt"
put -r sub
mkdir up
put c.txt up/
option json on
ls *.log
option json off
ls
chmod 600 a.log
ln -s a.log link.log
mv b.log up/
call test -L link.log && stat -c 'mode=%%a' a.log
call echo "from-remote  two"; echo to-stderr >&2
get a.log "%LOCAL%/back/"
get -r sub "%LOCAL%/back/subcopy"
get "renamed file.txt" "%LOCAL%/back/plain.txt"
sync remote "%LOCAL%/mirror" mirror --delete -x *.tmp
exit
echo never printed
"#
    );
    let o = ctx.script(&args, &main);
    ok(&o);
    let out = text(&o.stdout);
    assert!(out.contains("connected first-param 100%"), "{out}");
    assert!(out.contains(&format!("/{}\n", ctx.base)), "pwd: {out}");
    assert!(out.contains(&format!("{}\n", l.display())), "lpwd: {out}");
    assert!(out.contains("mode=600"), "chmod/ln -s/call: {out}");
    assert!(
        out.contains("from-remote  two"),
        "call keeps quoting: {out}"
    );
    assert!(text(&o.stderr).contains("to-stderr"));
    assert!(!out.contains("never printed"));
    // The JSON listing of *.log, then the text listing.
    let first_json = out.find('[').unwrap();
    let json_end = out[first_json..].find("\n]").unwrap() + first_json + 2;
    let listed = json_values(&out[first_json..json_end]);
    assert_eq!(names(&listed[0]), vec!["a.log", "b.log"]);
    assert!(out[json_end..].contains("renamed file.txt"));

    assert_eq!(
        std::fs::read_to_string(l.join("back/a.log")).unwrap(),
        "alpha\n"
    );
    assert_eq!(
        std::fs::read_to_string(l.join("back/subcopy/dir/deep.txt")).unwrap(),
        "deep\n"
    );
    assert_eq!(
        std::fs::read_to_string(l.join("back/plain.txt")).unwrap(),
        "spaced\n"
    );

    // Check the remote side with a second script (stdin, --json).
    let check = format!("{open}\ncd %BASE%\nls\nls up\nls mirror\nls sub/dir\nstat link.log\n");
    let o = ctx.script(&["--json"], &check);
    ok(&o);
    let v = json_values(&text(&o.stdout));
    assert_eq!(v.len(), 5, "{}", text(&o.stdout));
    assert_eq!(
        names(&v[0]),
        vec![
            "a.log",
            "link.log",
            "mirror",
            "renamed file.txt",
            "sub",
            "up"
        ]
    );
    assert_eq!(names(&v[1]), vec!["b.log", "c.txt"]);
    assert_eq!(names(&v[2]), vec!["keep.txt"], "sync -x *.tmp");
    assert_eq!(names(&v[3]), vec!["deep.txt"]);
    let a = v[0]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "a.log")
        .unwrap();
    assert_eq!(a["permissions"].as_u64().map(|m| m & 0o777), Some(0o600));
    assert_eq!(a["size"], 6);

    // Errors abort at the failing line with its number...
    let failing = format!("{open}\ncd %BASE%\ncd does-not-exist\necho after\n");
    let o = ctx.script(&[], &failing);
    assert_eq!(o.status.code(), Some(1));
    let err = text(&o.stderr);
    assert!(err.contains("<stdin>:3:"), "{err}");
    assert!(err.contains("does-not-exist"), "{err}");
    assert!(!text(&o.stdout).contains("after"));

    // ...or are counted with `option batch continue`.
    let cont = format!(
        "option batch continue\n{open}\ncd %BASE%\ncall false\nrm nothing-here\necho after\n"
    );
    let o = ctx.script(&[], &cont);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stdout).contains("after"));
    let err = text(&o.stderr);
    assert!(
        err.contains("<stdin>:4:") && err.contains("status 1"),
        "{err}"
    );
    assert!(err.contains("<stdin>:5:"), "{err}");
    assert!(err.contains("2 command(s) failed"), "{err}");

    // Undefined variables and unknown commands name the line too.
    let o = ctx.script(&[], "echo ok\nput %NOPE_NOT_DEFINED%\n");
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o.stderr).contains("<stdin>:2:"));
    let o = ctx.script(&[], "frobnicate\n");
    assert!(text(&o.stderr).contains("unknown command `frobnicate`"));

    if let Some(fp) = &hostkey {
        // A wrong pinned key is refused.
        let wrong = format!("{OPEN} -hostkey=SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n");
        let o = ctx.script(&[], &wrong);
        assert_eq!(o.status.code(), Some(1));
        let err = text(&o.stderr);
        assert!(
            err.contains("does not match -hostkey") && err.contains(fp.as_str()),
            "{err}"
        );
    }

    // `hostwick ls --json` and `hostwick sync --json` on the same server.
    let target = format!("{}@{}:{}/mirror", ctx.env.user, ctx.env.host, ctx.base);
    let common = |c: &mut Command| {
        c.args(["--batch", "--accept-new"]);
    };
    let mut c = ctx.cmd();
    common(&mut c);
    let o = c
        .args(["sync", "--json", "-p", &ctx.env.port, "-i"])
        .arg(&ctx.env.key)
        .args(["-x", "*.tmp", "./mirror", &target])
        .output()
        .unwrap();
    ok(&o);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["up_to_date"], true, "{v}");
    write(&l.join("mirror/new.txt"), "new\n");
    let mut c = ctx.cmd();
    common(&mut c);
    let o = c
        .args(["sync", "--json", "-p", &ctx.env.port, "-i"])
        .arg(&ctx.env.key)
        .args(["-x", "*.tmp", "./mirror", &target])
        .output()
        .unwrap();
    ok(&o);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["copied"], serde_json::json!(["new.txt"]), "{v}");
    assert_eq!(v["bytes"], 4);

    // `rm` removes links (even dangling ones), never what they point to.
    let links = format!(
        "{open}\ncd %BASE%\nmkdir keepme\nput c.txt keepme/\nln -s keepme dirlink\nln -s gone dangling\nrm dirlink\nrm dangling\nls keepme\nls\n"
    );
    let o = ctx.script(&["--json"], &links);
    ok(&o);
    let v = json_values(&text(&o.stdout));
    assert_eq!(names(&v[0]), vec!["c.txt"]);
    assert!(!names(&v[1]).contains(&"dirlink".to_owned()));
    assert!(!names(&v[1]).contains(&"dangling".to_owned()));

    // Clean up the remote directory.
    let o = ctx.script(&[], &format!("{open}\nrm -r %BASE%\n"));
    ok(&o);
}
