//! `hostwick script` — WinSCP-style batch scripts for automation.
//!
//! ```text
//! # deploy.txt — run with: hostwick script deploy.txt -D env=prod
//! option batch abort
//! option confirm off
//! open deploy@web-%env%.example.com -hostkey="SHA256:..."
//! cd /srv/app
//! lcd build
//! put -r -n *.js static/
//! call systemctl --user restart app
//! exit
//! ```
//!
//! One command per line, `#` comments, `"..."` quoting (`""` is a literal
//! quote), `%name%` variables from `-D name=value`, positional `%1%`, `%2%`
//! and the environment (`%%` is a literal `%`). With `option batch abort`
//! (the default) the first failing command stops the script with exit code
//! 1 and a message naming the line; with `option batch continue` every line
//! runs and the exit code is 1 if any failed.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use clap::Args;
use hostwick_core::{AuthMethod, SecretString};
use hostwick_sftp::{Entry, LocalFs, RemoteFs, SftpFs, SftpOptions, join, path_to_slash};
use hostwick_ssh::{Connection, HostKeyDecision, HostKeyInfo, Interaction};
use hostwick_transfer::{
    EngineConfig, ExistingPolicy, Job, SyncMode, SyncOptions, TransferEngine, sync_once,
};

use crate::ConnOpts;
use crate::app::App;
use crate::cp::{drive, human, print_entry, print_summary};

pub const HELP: &str = "\
Script commands (one per line; `#` comments; \"quoted paths\"; %NAME% from -D or the
environment, %1% %2% from parameters after `--`, %% for a literal %):
  open HOST | USER@HOST[:PORT] | sftp://USER[:PASSWORD]@HOST[:PORT]/[DIR]
       [-hostkey=\"SHA256:...\"] [-privatekey=FILE] [-passphrase=...] [-password=...] [-port=N]
  close                      exit [CODE]
  cd [DIR]   lcd DIR   pwd   lpwd   ls [PATH|MASK]   lls [DIR]   stat PATH
  put [-r] [-n] LOCAL... [REMOTE]       get [-r] [-n] REMOTE... [LOCAL]
  mkdir [-p] DIR...   rm [-r] PATH...   rmdir DIR...   mv SOURCE... TARGET
  chmod OCTAL PATH...   ln -s TARGET LINK
  sync remote|local [LOCAL-DIR] [REMOTE-DIR] [--delete] [-x PATTERN]... [-n]
  call SHELL-COMMAND         echo TEXT
  option batch abort|continue   option confirm on|off   option json on|off   option echo on|off
Wildcards * and ? work in the last path component (put, get, ls, rm, mv, chmod).
-n skips files whose size and time already match; `option confirm on` never overwrites.
With `option batch abort` (default) the first error stops the script with exit code 1.";

#[derive(Args)]
pub struct ScriptArgs {
    /// Script file, or `-` to read it from standard input.
    file: String,
    /// Define a variable for `%NAME%` (repeatable). Environment variables
    /// can be used too; -D wins.
    #[arg(short = 'D', long = "define", value_name = "NAME=VALUE")]
    defines: Vec<String>,
    /// `ls`, `lls` and `stat` print JSON (like `option json on`).
    #[arg(long)]
    json: bool,
    /// Parameters for `%1%`, `%2%`, ... (after `--`).
    #[arg(last = true)]
    params: Vec<String>,
}

// ---------------------------------------------------------------- parsing --

/// Variables for `%name%` substitution; unknown names fall back to the
/// environment.
#[derive(Default)]
pub(crate) struct Vars(HashMap<String, String>);

impl Vars {
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }
}

fn is_var_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Replace `%name%` with its value; `%%` is a literal `%`; a `%` that does
/// not start a variable reference is kept as is.
pub(crate) fn substitute(line: &str, vars: &Vars) -> Result<String> {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(0) => {
                out.push('%');
                rest = &after[1..];
            }
            Some(j) if is_var_name(&after[..j]) => {
                let name = &after[..j];
                let value = vars.get(name).ok_or_else(|| {
                    anyhow!("undefined variable %{name}% (define it with -D {name}=VALUE)")
                })?;
                out.push_str(&value);
                rest = &after[j + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// Split on whitespace; `"..."` groups, `""` inside quotes is a literal `"`.
/// Backslashes are ordinary characters (Windows paths).
pub(crate) fn tokenize(line: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('"') if chars.peek() == Some(&'"') => {
                            chars.next();
                            cur.push('"');
                        }
                        Some('"') => break,
                        Some(c) => cur.push(c),
                        None => bail!("unterminated quote"),
                    }
                }
            }
            c if c.is_whitespace() => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            c => {
                in_token = true;
                cur.push(c);
            }
        }
    }
    if in_token {
        out.push(cur);
    }
    Ok(out)
}

/// `*` and `?` wildcards (no character classes).
pub(crate) fn glob_match(pattern: &str, name: &str, case_insensitive: bool) -> bool {
    let fold = |s: &str| -> Vec<char> {
        if case_insensitive {
            s.to_lowercase().chars().collect()
        } else {
            s.chars().collect()
        }
    };
    let (p, n) = (fold(pattern), fold(name));
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

fn has_glob(s: &str) -> bool {
    s.contains(['*', '?'])
}

/// Options and positional arguments of one command. `flags` lists the
/// accepted switches; those in `with_value` take the next token (or
/// `=value`).
struct Parsed {
    switches: HashMap<String, Option<String>>,
    multi: Vec<(String, String)>,
    args: Vec<String>,
}

impl Parsed {
    fn has(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.switches.contains_key(*n))
    }
    fn value(&self, name: &str) -> Option<&str> {
        self.switches.get(name).and_then(|v| v.as_deref())
    }
}

fn parse_args(tokens: &[String], flags: &[&str], with_value: &[&str]) -> Result<Parsed> {
    let mut p = Parsed {
        switches: HashMap::new(),
        multi: Vec::new(),
        args: Vec::new(),
    };
    let mut it = tokens.iter();
    let mut only_args = false;
    while let Some(t) = it.next() {
        if only_args || !t.starts_with('-') || t == "-" {
            p.args.push(t.clone());
            continue;
        }
        if t == "--" {
            only_args = true;
            continue;
        }
        let (name, inline) = match t.split_once('=') {
            Some((n, v)) => (n, Some(v.to_owned())),
            None => (t.as_str(), None),
        };
        if with_value.contains(&name) {
            let v = match inline {
                Some(v) => v,
                None => it
                    .next()
                    .cloned()
                    .ok_or_else(|| anyhow!("{name} needs a value"))?,
            };
            p.multi.push((name.to_owned(), v.clone()));
            p.switches.insert(name.to_owned(), Some(v));
        } else if flags.contains(&name) && inline.is_none() {
            p.switches.insert(name.to_owned(), None);
        } else {
            bail!("unknown option {t}");
        }
    }
    Ok(p)
}

/// `sftp://user:pass@host:port/path` (WinSCP), or a saved host or
/// `user@host[:port]`. Returns (target, password, initial directory).
fn parse_open_target(s: &str) -> (String, Option<String>, Option<String>) {
    let Some(rest) = s
        .strip_prefix("sftp://")
        .or_else(|| s.strip_prefix("scp://"))
        .or_else(|| s.strip_prefix("ssh://"))
    else {
        return (s.to_owned(), None, None);
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], Some(&rest[i..])),
        None => (rest, None),
    };
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let (user, pass) = match userinfo.map(|u| u.split_once(':')) {
        Some(Some((u, p))) => (Some(percent_decode(u)), Some(percent_decode(p))),
        Some(None) => (userinfo.map(percent_decode), None),
        None => (None, None),
    };
    let target = match user {
        Some(u) => format!("{u}@{hostport}"),
        None => hostport.to_owned(),
    };
    let path = path.filter(|p| *p != "/").map(|p| {
        // WinSCP: `/path` is relative to home unless it starts with `//`.
        match p.strip_prefix("//") {
            Some(abs) => format!("/{abs}"),
            None => p.trim_start_matches('/').to_owned(),
        }
    });
    (target, pass, path)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `SHA256:abc=`, `ssh-ed25519 255 SHA256:abc` or bare base64, several
/// separated by `;`. `*` accepts any key.
fn parse_hostkeys(v: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for part in v.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        if part == "*" {
            out.push("*".into());
            continue;
        }
        let fp = part
            .split_whitespace()
            .find_map(|t| t.strip_prefix("SHA256:"))
            .or_else(|| {
                part.split_whitespace()
                    .last()
                    .filter(|t| !t.contains(':') && t.len() >= 40)
            });
        match fp {
            Some(fp) => out.push(fp.trim_end_matches('=').to_owned()),
            None => bail!(
                "-hostkey: expected a SHA-256 fingerprint (`SHA256:...`, as `ssh-keygen -l` \
                 prints it); MD5 fingerprints are not supported"
            ),
        }
    }
    Ok(out)
}

/// Everything after the command word, as written (quotes kept).
fn rest_of_line(line: &str) -> &str {
    let t = line.trim_start();
    let end = t.find(char::is_whitespace).unwrap_or(t.len());
    t[end..].trim()
}

/// Single-quote for a POSIX shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn file_name(p: &str) -> String {
    p.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(p)
        .to_owned()
}

// ------------------------------------------------------------ interaction --

/// `open` switches on top of the terminal prompts: a password from the URL
/// or `-password`, a key passphrase, and expected host-key fingerprints.
struct ScriptInteraction {
    inner: Arc<dyn Interaction>,
    password: Option<SecretString>,
    passphrase: Option<SecretString>,
    hostkeys: Vec<String>,
}

#[async_trait]
impl Interaction for ScriptInteraction {
    async fn host_key(&self, info: &HostKeyInfo) -> HostKeyDecision {
        if self.hostkeys.is_empty() {
            return self.inner.host_key(info).await;
        }
        let got = info
            .fingerprint
            .trim_start_matches("SHA256:")
            .trim_end_matches('=');
        if self.hostkeys.iter().any(|k| k == "*") {
            eprintln!(
                "warning: -hostkey=* accepts any host key; {} is {}",
                info.host, info.fingerprint
            );
            return HostKeyDecision::AcceptOnce;
        }
        if self.hostkeys.iter().any(|k| k == got) {
            return HostKeyDecision::AcceptOnce;
        }
        eprintln!(
            "host key for {} is {} {}, which does not match -hostkey",
            info.host, info.algorithm, info.fingerprint
        );
        HostKeyDecision::Reject
    }

    async fn password(&self, user: &str, host: &str) -> Option<SecretString> {
        match &self.password {
            Some(p) => Some(p.clone()),
            None => self.inner.password(user, host).await,
        }
    }

    async fn key_passphrase(&self, path: &Path) -> Option<SecretString> {
        match &self.passphrase {
            Some(p) => Some(p.clone()),
            None => self.inner.key_passphrase(path).await,
        }
    }

    async fn keyboard_interactive(
        &self,
        name: &str,
        instructions: &str,
        prompts: &[(String, bool)],
    ) -> Option<Vec<String>> {
        // A single hidden prompt is almost always the password (PAM).
        if let (Some(p), [(_, false)]) = (&self.password, prompts) {
            return Some(vec![
                hostwick_core::ExposeSecret::expose_secret(p).to_owned(),
            ]);
        }
        self.inner
            .keyboard_interactive(name, instructions, prompts)
            .await
    }

    fn banner(&self, text: &str) {
        self.inner.banner(text);
    }

    fn notice(&self, text: &str) {
        self.inner.notice(text);
    }
}

// --------------------------------------------------------------- runtime --

struct Session {
    label: String,
    conn: Connection,
    fs: Arc<dyn RemoteFs>,
    /// Second SFTP channel for chmod/symlink, opened on first use.
    raw: Option<russh_sftp::client::SftpSession>,
    home: String,
    cwd: String,
}

impl Session {
    async fn close(self) {
        if let Some(raw) = &self.raw {
            let _ = raw.close().await;
        }
        self.conn.disconnect().await;
    }
}

enum Flow {
    Next,
    Exit(i32),
}

struct Runner<'a> {
    app: &'a App,
    session: Option<Session>,
    lcwd: PathBuf,
    local: Arc<dyn RemoteFs>,
    abort_on_error: bool,
    confirm: bool,
    json: bool,
    echo: bool,
}

fn local_case_insensitive() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

impl Runner<'_> {
    fn session(&mut self) -> Result<&mut Session> {
        self.session
            .as_mut()
            .ok_or_else(|| anyhow!("no session is open (use `open` first)"))
    }

    fn rpath(&self, p: &str) -> Result<String> {
        let s = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow!("no session is open (use `open` first)"))?;
        Ok(if p.starts_with('/') {
            p.to_owned()
        } else if p == "~" {
            s.home.clone()
        } else if let Some(r) = p.strip_prefix("~/") {
            join(&s.home, r)
        } else if p == "." || p.is_empty() {
            s.cwd.clone()
        } else {
            join(&s.cwd, p)
        })
    }

    fn lpath(&self, p: &str) -> PathBuf {
        let expanded = match p.strip_prefix("~") {
            Some(r) if r.is_empty() || r.starts_with(['/', '\\']) => std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| PathBuf::from(h).join(r.trim_start_matches(['/', '\\'])))
                .unwrap_or_else(|| PathBuf::from(p)),
            _ => PathBuf::from(p),
        };
        if expanded.is_absolute() {
            expanded
        } else {
            self.lcwd.join(expanded)
        }
    }

    fn fs(&self) -> Result<Arc<dyn RemoteFs>> {
        Ok(self
            .session
            .as_ref()
            .ok_or_else(|| anyhow!("no session is open (use `open` first)"))?
            .fs
            .clone())
    }

    /// Remote paths for `arg`, expanding a wildcard in the last component.
    async fn remote_matches(&self, arg: &str) -> Result<Vec<(String, Entry)>> {
        let fs = self.fs()?;
        let path = self.rpath(arg)?;
        if !has_glob(&file_name(&path)) {
            let e = fs
                .stat(&path)
                .await?
                .ok_or_else(|| anyhow!("{path}: no such file or directory"))?;
            return Ok(vec![(path, e)]);
        }
        let dir = hostwick_sftp::parent(&path);
        let pattern = file_name(&path);
        let out: Vec<_> = fs
            .list_dir(&dir)
            .await?
            .into_iter()
            .filter(|e| glob_match(&pattern, &e.name, false))
            .map(|e| (join(&dir, &e.name), e))
            .collect();
        if out.is_empty() {
            bail!("no remote files match {path}");
        }
        Ok(out)
    }

    /// Local paths for `arg`, expanding a wildcard in the last component.
    fn local_matches(&self, arg: &str) -> Result<Vec<PathBuf>> {
        let path = self.lpath(arg);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !has_glob(&name) {
            if !path.exists() {
                bail!("{}: no such file or directory", path.display());
            }
            return Ok(vec![path]);
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(Result::ok)
            .filter(|e| {
                glob_match(
                    &name,
                    &e.file_name().to_string_lossy(),
                    local_case_insensitive(),
                )
            })
            .map(|e| e.path())
            .collect();
        out.sort();
        if out.is_empty() {
            bail!("no local files match {}", path.display());
        }
        Ok(out)
    }

    fn policy(&self, newer: bool) -> ExistingPolicy {
        if self.confirm {
            ExistingPolicy::Skip
        } else if newer {
            ExistingPolicy::SkipIfSame
        } else {
            ExistingPolicy::Overwrite
        }
    }

    fn print_entries(&self, entries: &[Entry]) -> Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(entries)?);
        } else {
            for e in entries {
                print_entry(e);
            }
        }
        Ok(())
    }

    /// Run one command. `line` is the substituted line, for commands that
    /// take the rest of it verbatim (`echo`, `call`).
    async fn exec(&mut self, line: &str, tokens: Vec<String>) -> Result<Flow> {
        let rest = rest_of_line(line);
        let (cmd, args) = match tokens.split_first() {
            Some((c, a)) => (c.to_ascii_lowercase(), a.to_vec()),
            None => return Ok(Flow::Next),
        };
        match cmd.as_str() {
            "open" => self.open(&args).await?,
            "close" => match self.session.take() {
                Some(s) => {
                    let label = s.label.clone();
                    s.close().await;
                    eprintln!("closed {label}");
                }
                None => bail!("no session is open"),
            },
            "exit" | "bye" | "quit" => {
                let code = match args.first() {
                    Some(c) => c
                        .parse()
                        .map_err(|_| anyhow!("exit code must be a number"))?,
                    None => 0,
                };
                return Ok(Flow::Exit(code));
            }
            "echo" => println!("{rest}"),
            "option" => self.option(&args)?,
            "pwd" => println!("{}", self.session()?.cwd),
            "lpwd" => println!("{}", self.lcwd.display()),
            "cd" => {
                let target = match args.as_slice() {
                    [] => self.session()?.home.clone(),
                    [p] => self.rpath(p)?,
                    _ => bail!("usage: cd [DIRECTORY]"),
                };
                let fs = self.fs()?;
                let abs = fs.canonicalize(&target).await?;
                match fs.stat(&abs).await? {
                    Some(e) if e.is_dir() => self.session()?.cwd = abs,
                    Some(_) => bail!("{target} is not a directory"),
                    None => bail!("{target}: no such directory"),
                }
            }
            "lcd" => {
                let [p] = args.as_slice() else {
                    bail!("usage: lcd DIRECTORY")
                };
                let path = self.lpath(p);
                if !path.is_dir() {
                    bail!("{}: no such directory", path.display());
                }
                self.lcwd = std::path::absolute(&path)?;
            }
            "ls" | "dir" => {
                let fs = self.fs()?;
                let entries = match args.as_slice() {
                    [] => {
                        let cwd = self.session()?.cwd.clone();
                        fs.list_dir(&cwd).await?
                    }
                    [p] if has_glob(p) => self
                        .remote_matches(p)
                        .await?
                        .into_iter()
                        .map(|(_, e)| e)
                        .collect(),
                    [p] => {
                        let path = self.rpath(p)?;
                        match fs.stat(&path).await? {
                            Some(e) if !e.is_dir() => vec![e],
                            Some(_) => fs.list_dir(&path).await?,
                            None => bail!("{path}: no such file or directory"),
                        }
                    }
                    _ => bail!("usage: ls [PATH|MASK]"),
                };
                self.print_entries(&entries)?;
            }
            "lls" | "ldir" => {
                let dir = match args.as_slice() {
                    [] => self.lcwd.clone(),
                    [p] => self.lpath(p),
                    _ => bail!("usage: lls [DIRECTORY]"),
                };
                let entries = self.local.list_dir(&path_to_slash(&dir)).await?;
                self.print_entries(&entries)?;
            }
            "stat" => {
                let [p] = args.as_slice() else {
                    bail!("usage: stat PATH")
                };
                let path = self.rpath(p)?;
                let e = self
                    .fs()?
                    .stat(&path)
                    .await?
                    .ok_or_else(|| anyhow!("{path}: no such file or directory"))?;
                self.print_entries(std::slice::from_ref(&e))?;
            }
            "put" => self.put(&args).await?,
            "get" => self.get(&args).await?,
            "mkdir" => {
                let p = parse_args(&args, &["-p"], &[])?;
                if p.args.is_empty() {
                    bail!("usage: mkdir [-p] DIRECTORY...");
                }
                let fs = self.fs()?;
                for d in &p.args {
                    let path = self.rpath(d)?;
                    if p.has(&["-p"]) {
                        fs.mkdir_all(&path).await?;
                    } else {
                        fs.mkdir(&path).await?;
                    }
                }
            }
            "rmdir" => {
                if args.is_empty() {
                    bail!("usage: rmdir DIRECTORY...");
                }
                let fs = self.fs()?;
                for d in &args {
                    let path = self.rpath(d)?;
                    fs.remove_dir(&path).await?;
                }
            }
            "rm" | "del" => {
                let p = parse_args(&args, &["-r", "-R", "--recursive", "-f"], &[])?;
                if p.args.is_empty() {
                    bail!("usage: rm [-r] PATH...");
                }
                let recursive = p.has(&["-r", "-R", "--recursive"]);
                let fs = self.fs()?;
                for a in &p.args {
                    // A link (even a dangling one) is removed itself; `stat`
                    // would follow it.
                    let direct = self.rpath(a)?;
                    if !has_glob(&file_name(&direct))
                        && self
                            .raw_sftp()
                            .await?
                            .symlink_metadata(direct.clone())
                            .await
                            .is_ok_and(|m| m.is_symlink())
                    {
                        fs.remove_file(&direct).await?;
                        continue;
                    }
                    for (path, e) in self.remote_matches(a).await? {
                        // Listings report links as links, never as folders.
                        if e.is_dir() {
                            if !recursive {
                                bail!("{path} is a directory (use rm -r)");
                            }
                            remove_tree(fs.as_ref(), &path).await?;
                        } else {
                            fs.remove_file(&path).await?;
                        }
                    }
                }
            }
            "mv" | "rename" => {
                if args.len() < 2 {
                    bail!("usage: mv SOURCE... TARGET");
                }
                let fs = self.fs()?;
                let (sources, target) = args.split_at(args.len() - 1);
                let target = self.rpath(&target[0])?;
                let target_dir = fs.stat(&target).await?.is_some_and(|e| e.is_dir());
                let mut moves = Vec::new();
                for s in sources {
                    moves.extend(self.remote_matches(s).await?);
                }
                if moves.len() > 1 && !target_dir {
                    bail!("{target}: moving several files needs an existing directory");
                }
                for (path, _) in moves {
                    let to = if target_dir {
                        join(&target, &file_name(&path))
                    } else {
                        target.clone()
                    };
                    fs.rename(&path, &to).await?;
                }
            }
            "chmod" => {
                let [mode, paths @ ..] = args.as_slice() else {
                    bail!("usage: chmod OCTAL-MODE PATH...")
                };
                if paths.is_empty() {
                    bail!("usage: chmod OCTAL-MODE PATH...");
                }
                let mode = u32::from_str_radix(mode, 8)
                    .ok()
                    .filter(|m| *m <= 0o7777)
                    .ok_or_else(|| anyhow!("chmod: `{mode}` is not an octal mode like 644"))?;
                let mut targets = Vec::new();
                for p in paths {
                    targets.extend(self.remote_matches(p).await?);
                }
                let raw = self.raw_sftp().await?;
                for (path, _) in targets {
                    let mut attrs = russh_sftp::protocol::FileAttributes::empty();
                    attrs.permissions = Some(mode);
                    raw.set_metadata(path.clone(), attrs)
                        .await
                        .map_err(|e| anyhow!("{path}: {e}"))?;
                }
            }
            "ln" | "symlink" => {
                let p = parse_args(&args, &["-s"], &[])?;
                let symbolic = p.has(&["-s"]) || cmd == "symlink";
                let [target, link] = p.args.as_slice() else {
                    bail!("usage: ln -s TARGET LINK")
                };
                let link = self.rpath(link)?;
                // A relative symlink target is stored as written.
                let target = if symbolic {
                    target.clone()
                } else {
                    self.rpath(target)?
                };
                self.link(&target, &link, symbolic).await?;
            }
            "call" | "!" => {
                if args.is_empty() {
                    bail!("usage: call COMMAND");
                }
                let s = self.session()?;
                let command = format!("cd {} && {rest}", shell_quote(&s.cwd));
                let out = s.conn.exec(&command).await?;
                use std::io::Write;
                std::io::stdout().write_all(&out.stdout)?;
                std::io::stdout().flush()?;
                std::io::stderr().write_all(&out.stderr)?;
                match (out.exit_status, out.exit_signal) {
                    (Some(0), _) => {}
                    (Some(code), _) => bail!("command exited with status {code}"),
                    (None, Some(sig)) => bail!("command killed by signal {sig}"),
                    (None, None) => bail!("command ended without an exit status"),
                }
            }
            "sync" | "synchronize" => self.sync(&args).await?,
            other => bail!("unknown command `{other}`"),
        }
        Ok(Flow::Next)
    }

    fn option(&mut self, args: &[String]) -> Result<()> {
        let [name, value] = args else {
            bail!("usage: option NAME VALUE");
        };
        let on_off = |v: &str| match v.to_ascii_lowercase().as_str() {
            "on" | "yes" | "true" => Ok(true),
            "off" | "no" | "false" => Ok(false),
            _ => Err(anyhow!("option {name}: expected on or off, not `{v}`")),
        };
        match name.to_ascii_lowercase().as_str() {
            "batch" => {
                self.abort_on_error = match value.to_ascii_lowercase().as_str() {
                    "abort" | "on" | "off" => true,
                    "continue" => false,
                    v => bail!("option batch: expected abort or continue, not `{v}`"),
                }
            }
            "confirm" => self.confirm = on_off(value)?,
            "json" => self.json = on_off(value)?,
            "echo" => self.echo = on_off(value)?,
            "transfer" => match value.to_ascii_lowercase().as_str() {
                "binary" | "automatic" => {}
                v => bail!("option transfer {v}: only binary transfers are supported"),
            },
            other => bail!("unknown option `{other}` (batch, confirm, json, echo, transfer)"),
        }
        Ok(())
    }

    async fn open(&mut self, args: &[String]) -> Result<()> {
        let p = parse_args(
            args,
            &[],
            &[
                "-hostkey",
                "-password",
                "-privatekey",
                "-passphrase",
                "-port",
            ],
        )?;
        let [target] = p.args.as_slice() else {
            bail!(
                "usage: open HOST|USER@HOST[:PORT]|sftp://USER@HOST/ [-hostkey=FINGERPRINT] [-privatekey=FILE] [-password=...]"
            )
        };
        if let Some(old) = self.session.take() {
            old.close().await;
        }
        let (target, url_password, initial) = parse_open_target(target);
        let opts = ConnOpts {
            port: p
                .value("-port")
                .map(|v| v.parse().map_err(|_| anyhow!("-port: bad port `{v}`")))
                .transpose()?,
            identity: p
                .multi
                .iter()
                .filter(|(k, _)| k == "-privatekey")
                .map(|(_, v)| self.lpath(v))
                .collect(),
            ..Default::default()
        };
        let mut profile = self.app.resolve(&target, &opts)?;
        let password = p
            .value("-password")
            .map(str::to_owned)
            .or(url_password)
            .map(SecretString::from);
        if password.is_some()
            && !profile
                .auth
                .iter()
                .any(|a| matches!(a, AuthMethod::Password { .. }))
        {
            profile.auth.push(AuthMethod::Password { credential: None });
        }
        let hostkeys = match p.value("-hostkey") {
            Some(v) => parse_hostkeys(v)?,
            None => Vec::new(),
        };
        let mut ctx = self.app.context(&profile)?;
        ctx.interaction = Arc::new(ScriptInteraction {
            inner: ctx.interaction.clone(),
            password,
            passphrase: p
                .value("-passphrase")
                .map(|v| SecretString::from(v.to_owned())),
            hostkeys,
        });
        let conn = Connection::connect(&profile, &ctx)
            .await
            .with_context(|| format!("connecting to {}", profile.path()))?;
        let fs = SftpFs::open(&conn, &SftpOptions::default())
            .await
            .context("starting SFTP")?;
        let fs: Arc<dyn RemoteFs> = Arc::new(fs);
        let home = fs.canonicalize(".").await?;
        let cwd = match initial {
            Some(i) if i.starts_with('/') => fs.canonicalize(&i).await?,
            Some(i) => fs.canonicalize(&join(&home, &i)).await?,
            None => home.clone(),
        };
        let label = format!(
            "{}@{}:{}",
            profile.user_or_local(),
            profile.host,
            profile.port
        );
        eprintln!("connected to {label}");
        self.session = Some(Session {
            label,
            conn,
            fs,
            raw: None,
            home,
            cwd,
        });
        Ok(())
    }

    async fn raw_sftp(&mut self) -> Result<&russh_sftp::client::SftpSession> {
        let s = self.session()?;
        if s.raw.is_none() {
            let stream = s.conn.open_subsystem("sftp").await?;
            let raw = russh_sftp::client::SftpSession::new(stream)
                .await
                .map_err(|e| anyhow!("starting SFTP: {e}"))?;
            s.raw = Some(raw);
        }
        s.raw
            .as_ref()
            .ok_or_else(|| anyhow!("SFTP session unavailable"))
    }

    async fn link(&mut self, target: &str, link: &str, symbolic: bool) -> Result<()> {
        let raw = self.raw_sftp().await?;
        if !symbolic {
            if !raw
                .hardlink(target.to_owned(), link.to_owned())
                .await
                .map_err(|e| anyhow!("{link}: {e}"))?
            {
                bail!("this server does not support hard links (use ln -s)");
            }
            return Ok(());
        }
        // OpenSSH's sftp-server takes SSH_FXP_SYMLINK arguments in the
        // opposite order to the specification (target first). It is the
        // server that implements statvfs@openssh.com.
        let openssh = raw.fs_info(".".to_owned()).await.ok().flatten().is_some();
        let res = if openssh {
            raw.symlink(target.to_owned(), link.to_owned()).await
        } else {
            raw.symlink(link.to_owned(), target.to_owned()).await
        };
        res.map_err(|e| anyhow!("{link}: {e}"))
    }

    async fn put(&mut self, args: &[String]) -> Result<()> {
        let p = parse_args(
            args,
            &["-r", "-R", "--recursive", "-n", "--newer", "-neweronly"],
            &[],
        )?;
        if p.args.is_empty() {
            bail!("usage: put [-r] [-n] LOCAL... [REMOTE]");
        }
        let recursive = p.has(&["-r", "-R", "--recursive"]);
        let policy = self.policy(p.has(&["-n", "--newer", "-neweronly"]));
        let (sources, target) = if p.args.len() == 1 {
            (&p.args[..], None)
        } else {
            let (s, t) = p.args.split_at(p.args.len() - 1);
            (s, Some(t[0].as_str()))
        };
        let mut files = Vec::new();
        for s in sources {
            files.extend(self.local_matches(s)?);
        }
        let fs = self.fs()?;
        let (dir, exact) = match target {
            None => (Some(self.session()?.cwd.clone()), None),
            Some(t) => {
                let rt = self.rpath(t)?;
                match fs.stat(&rt).await? {
                    Some(e) if e.is_dir() => (Some(rt), None),
                    _ if t.ends_with('/') => bail!("{rt}: no such remote directory"),
                    _ if files.len() > 1 => {
                        bail!("{rt}: uploading several files needs an existing directory")
                    }
                    _ => (None, Some(rt)),
                }
            }
        };
        let engine = TransferEngine::new(EngineConfig::default());
        let events = engine.subscribe();
        let mut ids = Vec::new();
        for f in files {
            let src = path_to_slash(&std::path::absolute(&f)?);
            let name = file_name(&src);
            let dst = match (&dir, &exact) {
                (Some(d), _) => join(d, &name),
                (None, Some(e)) => e.clone(),
                (None, None) => bail!("no target"),
            };
            if f.is_dir() {
                if !recursive {
                    bail!("{} is a directory (use put -r)", f.display());
                }
                ids.extend(
                    engine
                        .enqueue_tree(self.local.clone(), &src, fs.clone(), &dst, policy)
                        .await?,
                );
            } else {
                ids.push(engine.enqueue(Job {
                    src: self.local.clone(),
                    src_path: src,
                    dst: fs.clone(),
                    dst_path: dst,
                    policy,
                }));
            }
        }
        finish_transfers(&engine, events, &ids).await
    }

    async fn get(&mut self, args: &[String]) -> Result<()> {
        let p = parse_args(
            args,
            &["-r", "-R", "--recursive", "-n", "--newer", "-neweronly"],
            &[],
        )?;
        if p.args.is_empty() {
            bail!("usage: get [-r] [-n] REMOTE... [LOCAL]");
        }
        let recursive = p.has(&["-r", "-R", "--recursive"]);
        let policy = self.policy(p.has(&["-n", "--newer", "-neweronly"]));
        let (sources, target) = if p.args.len() == 1 {
            (&p.args[..], None)
        } else {
            let (s, t) = p.args.split_at(p.args.len() - 1);
            (s, Some(t[0].as_str()))
        };
        let mut files = Vec::new();
        for s in sources {
            files.extend(self.remote_matches(s).await?);
        }
        let (dir, exact) = match target {
            None => (Some(self.lcwd.clone()), None),
            Some(t) => {
                let lt = self.lpath(t);
                if lt.is_dir() {
                    (Some(lt), None)
                } else if t.ends_with(['/', '\\']) {
                    bail!("{}: no such local directory", lt.display());
                } else if files.len() > 1 {
                    bail!(
                        "{}: downloading several files needs an existing directory",
                        lt.display()
                    );
                } else {
                    (None, Some(lt))
                }
            }
        };
        let fs = self.fs()?;
        let engine = TransferEngine::new(EngineConfig::default());
        let events = engine.subscribe();
        let mut ids = Vec::new();
        for (src, e) in files {
            let dst = match (&dir, &exact) {
                (Some(d), _) => d.join(file_name(&src)),
                (None, Some(x)) => x.clone(),
                (None, None) => bail!("no target"),
            };
            let dst = path_to_slash(&std::path::absolute(&dst)?);
            if e.is_dir() {
                if !recursive {
                    bail!("{src} is a directory (use get -r)");
                }
                ids.extend(
                    engine
                        .enqueue_tree(fs.clone(), &src, self.local.clone(), &dst, policy)
                        .await?,
                );
            } else {
                ids.push(engine.enqueue(Job {
                    src: fs.clone(),
                    src_path: src,
                    dst: self.local.clone(),
                    dst_path: dst,
                    policy,
                }));
            }
        }
        finish_transfers(&engine, events, &ids).await
    }

    async fn sync(&mut self, args: &[String]) -> Result<()> {
        let p = parse_args(
            args,
            &[
                "--delete",
                "-delete",
                "-mirror",
                "-n",
                "--dry-run",
                "-preview",
            ],
            &["-x", "--exclude"],
        )?;
        let [direction, rest @ ..] = p.args.as_slice() else {
            bail!("usage: sync local|remote [LOCAL-DIR] [REMOTE-DIR] [--delete] [-x PATTERN]...")
        };
        let local_dir = match rest.first() {
            Some(l) => self.lpath(l),
            None => self.lcwd.clone(),
        };
        let remote_dir = match rest.get(1) {
            Some(r) => self.rpath(r)?,
            None => self.session()?.cwd.clone(),
        };
        if rest.len() > 2 {
            bail!("sync takes at most two directories");
        }
        let local_dir = path_to_slash(&std::path::absolute(&local_dir)?);
        let fs = self.fs()?;
        let (src, src_root, dst, dst_root) = match direction.to_ascii_lowercase().as_str() {
            // WinSCP: "remote" updates the remote side from the local one.
            "remote" => (self.local.clone(), local_dir, fs, remote_dir),
            "local" => (fs, remote_dir, self.local.clone(), local_dir),
            "both" => return self.sync_both(fs, local_dir, remote_dir, &p).await,
            other => bail!("sync {other}: expected `local`, `remote` or `both`"),
        };
        match src.stat(&src_root).await? {
            Some(e) if e.is_dir() => {}
            Some(_) => bail!("{src_root} is not a directory"),
            None => bail!("{src_root}: no such directory"),
        }
        let dry_run = p.has(&["-n", "--dry-run", "-preview"]);
        let opts = SyncOptions {
            mode: if p.has(&["--delete", "-delete", "-mirror"]) {
                SyncMode::Mirror
            } else {
                SyncMode::Update
            },
            excludes: p.multi.iter().map(|(_, v)| v.clone()).collect(),
            dry_run,
        };
        let engine = TransferEngine::new(EngineConfig::default());
        let r = sync_once(&engine, src, &src_root, dst, &dst_root, &opts).await?;
        let (copy, delete) = if dry_run {
            ("would copy  ", "would delete")
        } else {
            ("copied      ", "deleted     ")
        };
        for f in &r.copied {
            eprintln!("{copy} {f}");
        }
        for f in &r.deleted {
            eprintln!("{delete} {f}");
        }
        for (f, e) in &r.failed {
            eprintln!("FAILED       {f}: {e}");
        }
        eprintln!(
            "sync {src_root} → {dst_root}: {} copied ({}), {} deleted, {} failed",
            r.copied.len(),
            human(r.bytes),
            r.deleted.len(),
            r.failed.len()
        );
        if !r.failed.is_empty() {
            bail!("{} file(s) failed to sync", r.failed.len());
        }
        Ok(())
    }
}

impl Runner<'_> {
    /// WinSCP `synchronize both`: newer wins each way, nothing deleted,
    /// conflicts reported.
    async fn sync_both(
        &mut self,
        fs: Arc<dyn RemoteFs>,
        local_dir: String,
        remote_dir: String,
        p: &Parsed,
    ) -> Result<()> {
        if p.has(&["--delete", "-delete", "-mirror"]) {
            bail!("sync both never deletes; drop --delete");
        }
        let plan = hostwick_transfer::build_plan(
            self.local.as_ref(),
            &local_dir,
            fs.as_ref(),
            &remote_dir,
            hostwick_transfer::PlanOptions {
                direction: hostwick_transfer::SyncDirection::Both,
                delete: false,
                excludes: p.multi.iter().map(|(_, v)| v.clone()).collect(),
            },
        )
        .await?;
        let conflicts: Vec<_> = plan
            .items
            .iter()
            .filter(|i| i.kind == hostwick_transfer::ActionKind::Conflict)
            .collect();
        if p.has(&["-n", "--dry-run", "-preview"]) {
            for it in &plan.items {
                eprintln!(
                    "would {:<14} {}",
                    it.kind.label().to_lowercase(),
                    it.rel_path
                );
            }
            return Ok(());
        }
        let engine = TransferEngine::new(EngineConfig::default());
        let r = hostwick_transfer::execute_plan(
            &engine,
            self.local.clone(),
            fs,
            &plan,
            |_, _| true,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;
        for f in &r.uploaded {
            eprintln!("uploaded     {f}");
        }
        for f in &r.downloaded {
            eprintln!("downloaded   {f}");
        }
        for c in &conflicts {
            eprintln!("CONFLICT     {}: {}", c.rel_path, c.reason.describe());
        }
        for (f, e) in &r.failed {
            eprintln!("FAILED       {f}: {e}");
        }
        eprintln!(
            "sync both {local_dir} ⇄ {remote_dir}: {} ({})",
            r.summary(),
            human(r.bytes)
        );
        if !r.failed.is_empty() {
            bail!("{} file(s) failed to sync", r.failed.len());
        }
        Ok(())
    }
}

async fn finish_transfers(
    engine: &TransferEngine,
    events: tokio::sync::broadcast::Receiver<hostwick_transfer::TransferEvent>,
    ids: &[hostwick_transfer::TransferId],
) -> Result<()> {
    let summary = drive(engine, events, ids).await;
    print_summary(&summary);
    if summary.failed > 0 {
        bail!("{} transfer(s) failed", summary.failed);
    }
    Ok(())
}

/// `rm -r`: files and links first, then directories deepest first. Links
/// are removed, never followed.
async fn remove_tree(fs: &dyn RemoteFs, root: &str) -> Result<()> {
    let mut dirs = vec![root.to_owned()];
    let mut i = 0;
    while i < dirs.len() {
        let dir = dirs[i].clone();
        for e in fs.list_dir(&dir).await? {
            let path = join(&dir, &e.name);
            if e.is_dir() {
                dirs.push(path);
            } else {
                fs.remove_file(&path).await?;
            }
        }
        i += 1;
    }
    for d in dirs.iter().rev() {
        fs.remove_dir(d).await?;
    }
    Ok(())
}

pub async fn run(app: &App, a: ScriptArgs) -> Result<i32> {
    let (name, text) = if a.file == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .context("reading the script from standard input")?;
        ("<stdin>".to_owned(), s)
    } else {
        let bytes =
            std::fs::read(&a.file).with_context(|| format!("cannot read script {}", a.file))?;
        (a.file.clone(), hostwick_import::decode_text(&bytes))
    };
    let mut vars = Vars::default();
    for (i, p) in a.params.iter().enumerate() {
        vars.0.insert((i + 1).to_string(), p.clone());
    }
    for d in &a.defines {
        let (k, v) = d
            .split_once('=')
            .ok_or_else(|| anyhow!("-D expects NAME=VALUE, got `{d}`"))?;
        if !is_var_name(k) {
            bail!("-D: `{k}` is not a valid variable name (letters, digits, _)");
        }
        vars.0.insert(k.to_owned(), v.to_owned());
    }
    let mut runner = Runner {
        app,
        session: None,
        lcwd: std::env::current_dir().context("current directory")?,
        local: Arc::new(LocalFs::system()),
        abort_on_error: true,
        confirm: false,
        json: a.json,
        echo: false,
    };
    let mut failures = 0usize;
    let mut code = 0;
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let result = async {
            let line = substitute(trimmed, &vars)?;
            if runner.echo {
                eprintln!("> {line}");
            }
            let tokens = tokenize(&line)?;
            runner.exec(&line, tokens).await
        }
        .await;
        match result {
            Ok(Flow::Next) => {}
            Ok(Flow::Exit(c)) => {
                code = c;
                break;
            }
            Err(e) => {
                failures += 1;
                eprintln!("hostwick script: {name}:{line_no}: `{trimmed}`: {e:#}");
                if runner.abort_on_error {
                    eprintln!("hostwick script: aborted (option batch abort)");
                    code = 1;
                    break;
                }
            }
        }
    }
    if let Some(s) = runner.session.take() {
        s.close().await;
    }
    if code == 0 && failures > 0 {
        eprintln!("hostwick script: {failures} command(s) failed");
        code = 1;
    }
    Ok(code)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vars {
        Vars(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn substitution() -> Result<()> {
        let v = vars(&[("env", "prod"), ("1", "first")]);
        assert_eq!(substitute("open web-%env%", &v)?, "open web-prod");
        assert_eq!(
            substitute("echo %1% 100%% 50% done", &v)?,
            "echo first 100% 50% done"
        );
        assert_eq!(substitute("echo %not a var%", &v)?, "echo %not a var%");
        let err = substitute("put %missing_var_xyz%", &v)
            .unwrap_err()
            .to_string();
        assert!(err.contains("-D missing_var_xyz="), "{err}");
        // Environment fallback.
        let path = std::env::var("PATH").unwrap_or_default();
        assert_eq!(substitute("%PATH%", &v)?, path);
        Ok(())
    }

    #[test]
    fn tokenizing() -> Result<()> {
        assert_eq!(
            tokenize(r#"put  "my file.txt" C:\dir\  "say ""hi""" """#)?,
            vec!["put", "my file.txt", r"C:\dir\", r#"say "hi""#, ""]
        );
        assert_eq!(tokenize("a\"b c\"d")?, vec!["ab cd"]);
        assert!(tokenize(r#"put "oops"#).is_err());
        assert!(tokenize("   ")?.is_empty());
        Ok(())
    }

    #[test]
    fn globbing() {
        assert!(glob_match("*.log", "app.log", false));
        assert!(!glob_match("*.log", "app.log.1", false));
        assert!(glob_match("a?c*", "abcdef", false));
        assert!(glob_match("*", "", false));
        assert!(!glob_match("*.LOG", "app.log", false));
        assert!(glob_match("*.LOG", "app.log", true));
        assert!(glob_match("*a*b*", "xxaxxbxx", false));
        assert!(!glob_match("*a*b", "xxaxxbxx", false));
    }

    #[test]
    fn option_parsing() -> Result<()> {
        let t = |s: &str| tokenize(s).unwrap_or_default();
        let p = parse_args(
            &t("-r -x '*.tmp' --exclude=.git -- -odd a b"),
            &["-r"],
            &["-x", "--exclude"],
        )?;
        assert!(p.has(&["-r"]));
        assert_eq!(p.multi.len(), 2);
        assert_eq!(p.multi[1].1, ".git");
        assert_eq!(p.args, vec!["-odd", "a", "b"]);
        assert!(parse_args(&t("-z a"), &["-r"], &[]).is_err());
        assert!(parse_args(&t("-x"), &[], &["-x"]).is_err());
        Ok(())
    }

    #[test]
    fn open_targets() -> Result<()> {
        assert_eq!(parse_open_target("web"), ("web".into(), None, None));
        assert_eq!(
            parse_open_target("sftp://me:p%40ss@host:2222/"),
            ("me@host:2222".into(), Some("p@ss".into()), None)
        );
        assert_eq!(
            parse_open_target("sftp://me@host/srv/www"),
            ("me@host".into(), None, Some("srv/www".into()))
        );
        assert_eq!(
            parse_open_target("scp://host//etc"),
            ("host".into(), None, Some("/etc".into()))
        );
        assert_eq!(
            parse_hostkeys("ssh-ed25519 255 SHA256:abc=; SHA256:def")?,
            vec!["abc", "def"]
        );
        assert_eq!(parse_hostkeys("*")?, vec!["*"]);
        assert!(parse_hostkeys("ssh-rsa 2048 aa:bb:cc").is_err());
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(rest_of_line("  call  echo \"a  b\" "), "echo \"a  b\"");
        assert_eq!(rest_of_line("echo"), "");
        assert_eq!(file_name("/a/b/"), "b");
        assert_eq!(percent_decode("a%2Fb%zz"), "a/b%zz");
        Ok(())
    }
}
