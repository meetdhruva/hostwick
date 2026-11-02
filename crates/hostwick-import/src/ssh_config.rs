use hostwick_core::{AuthMethod, Endpoint, ForwardSpec, HostProfile, ProfileSource, Target};

use std::path::{Path, PathBuf};

use crate::ImportResult;

struct Block {
    patterns: Vec<String>,
    /// (keyword lowercased, value)
    options: Vec<(String, String)>,
}

/// Read an OpenSSH config file with every `Include` expanded in place, the
/// way `ssh` does: relative paths are under `~/.ssh`, `*`/`?` wildcards in
/// the file name match in sorted order, missing files are ignored.
pub fn load_ssh_config(path: &Path) -> std::io::Result<String> {
    let ssh_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out = String::new();
    inline(path, &ssh_dir, &mut Vec::new(), &mut out)?;
    Ok(out)
}

/// `stack` holds the files being expanded, so include loops stop.
fn inline(
    path: &Path,
    ssh_dir: &Path,
    stack: &mut Vec<PathBuf>,
    out: &mut String,
) -> std::io::Result<()> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if stack.contains(&key) || stack.len() >= 16 {
        return Ok(());
    }
    let text = crate::decode_text(&std::fs::read(path)?);
    stack.push(key);
    for line in text.lines() {
        let include = split_kv(line.trim())
            .filter(|(k, _)| k.eq_ignore_ascii_case("include"))
            .map(|(_, v)| v);
        let Some(value) = include else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        for pattern in words(&value) {
            for file in expand_include(&pattern, ssh_dir) {
                // An unreadable included file is skipped, like ssh does.
                let _ = inline(&file, ssh_dir, stack, out);
            }
        }
    }
    stack.pop();
    Ok(())
}

fn expand_include(pattern: &str, ssh_dir: &Path) -> Vec<PathBuf> {
    let p = if let Some(rest) = pattern.strip_prefix("~/") {
        match ssh_dir.parent() {
            Some(home) => home.join(rest),
            None => return vec![],
        }
    } else {
        let p = PathBuf::from(pattern);
        if p.is_absolute() { p } else { ssh_dir.join(p) }
    };
    let Some(name) = p.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
        return vec![];
    };
    if !name.contains(['*', '?']) {
        return if p.is_file() { vec![p] } else { vec![] };
    }
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| e.file_name().to_str().is_some_and(|n| wildcard(&name, n)))
        .map(|e| e.path())
        .collect();
    files.sort();
    files
}

/// `*` and `?` matching (OpenSSH `match_pattern`).
fn wildcard(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Parse an OpenSSH client config into one profile per concrete `Host`
/// alias, applying OpenSSH's rule that the first value obtained for a
/// keyword wins across every matching block (so `Host *` defaults at the
/// end apply to everything above them).
pub fn parse_ssh_config(text: &str) -> ImportResult {
    let mut out = ImportResult::default();
    let mut blocks: Vec<Block> = vec![Block {
        patterns: vec!["*".into()],
        options: vec![],
    }];
    let mut in_match = false;

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((kw, value)) = split_kv(line) else {
            continue;
        };
        let kw = kw.to_ascii_lowercase();
        match kw.as_str() {
            "host" => {
                in_match = false;
                blocks.push(Block {
                    patterns: words(&value),
                    options: vec![],
                });
            }
            "match" => {
                in_match = true;
                out.warnings.push(format!(
                    "line {}: `Match` blocks are not imported",
                    lineno + 1
                ));
            }
            "include" => out.warnings.push(format!(
                "line {}: `Include {value}` not followed (use load_ssh_config to read includes)",
                lineno + 1
            )),
            _ if in_match => {}
            _ => {
                if let Some(b) = blocks.last_mut() {
                    b.options.push((kw, value));
                }
            }
        }
    }

    let mut aliases: Vec<String> = Vec::new();
    for b in &blocks {
        for p in &b.patterns {
            if !p.contains(['*', '?', '!']) && !aliases.contains(p) {
                aliases.push(p.clone());
            }
        }
    }

    for alias in aliases {
        let resolved = resolve(&blocks, &alias);
        let get = |k: &str| {
            resolved
                .iter()
                .find(|(kw, _)| kw == k)
                .map(|(_, v)| v.clone())
        };
        let all = |k: &str| {
            resolved
                .iter()
                .filter(|(kw, _)| kw == k)
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>()
        };

        let host = get("hostname")
            .map(|h| h.replace("%h", &alias))
            .unwrap_or_else(|| alias.clone());
        let mut p = HostProfile::new(alias.clone(), host);
        p.source = ProfileSource::SshConfig {
            alias: alias.clone(),
        };
        if let Some(port) = get("port") {
            match port.parse() {
                Ok(n) => p.port = n,
                Err(_) => out.warnings.push(format!("{alias}: bad Port `{port}`")),
            }
        }
        p.user = get("user");
        let mut auth = vec![AuthMethod::Agent];
        for key in all("identityfile") {
            auth.push(AuthMethod::KeyFile {
                path: key.into(),
                passphrase_credential: None,
            });
        }
        auth.push(AuthMethod::Password { credential: None });
        auth.push(AuthMethod::KeyboardInteractive);
        p.auth = auth;

        if let Some(jump) = get("proxyjump").filter(|j| !j.eq_ignore_ascii_case("none")) {
            for hop in jump.split(',') {
                match Target::parse(hop.trim_start_matches("ssh://")) {
                    Ok(t) => p.jump.push(Endpoint {
                        port: t.port_or_default(),
                        host: t.host,
                        user: t.user,
                    }),
                    Err(e) => out
                        .warnings
                        .push(format!("{alias}: bad ProxyJump `{hop}`: {e}")),
                }
            }
        }
        if get("proxycommand").is_some_and(|c| !c.eq_ignore_ascii_case("none")) {
            out.warnings.push(format!(
                "{alias}: ProxyCommand is not supported; use ProxyJump"
            ));
        }
        if let Some(v) = get("serveraliveinterval").and_then(|v| v.parse().ok()) {
            p.keepalive_secs = v;
        }
        p.agent_forwarding = get("forwardagent").is_some_and(|v| v.eq_ignore_ascii_case("yes"));

        for (kw, flag) in [("localforward", 'L'), ("remoteforward", 'R')] {
            for v in all(kw) {
                // `LocalForward [bind:]port host:hostport` → `-L` syntax.
                let spec = words(&v).join(":");
                match ForwardSpec::parse(flag, &spec) {
                    Ok(f) => p.forwards.push(f),
                    Err(e) => out.warnings.push(format!("{alias}: {e}")),
                }
            }
        }
        for v in all("dynamicforward") {
            match ForwardSpec::parse('D', &v) {
                Ok(f) => p.forwards.push(f),
                Err(e) => out.warnings.push(format!("{alias}: {e}")),
            }
        }
        out.profiles.push(p);
    }
    out
}

/// Options for `alias`, in precedence order (first match per keyword wins;
/// multi-valued keywords like IdentityFile keep all occurrences).
fn resolve(blocks: &[Block], alias: &str) -> Vec<(String, String)> {
    const MULTI: &[&str] = &[
        "identityfile",
        "localforward",
        "remoteforward",
        "dynamicforward",
    ];
    let mut out: Vec<(String, String)> = Vec::new();
    for b in blocks.iter().filter(|b| block_matches(b, alias)) {
        for (k, v) in &b.options {
            if MULTI.contains(&k.as_str()) || !out.iter().any(|(ek, _)| ek == k) {
                out.push((k.clone(), v.clone()));
            }
        }
    }
    out
}

fn block_matches(b: &Block, alias: &str) -> bool {
    let mut matched = false;
    for p in &b.patterns {
        if let Some(neg) = p.strip_prefix('!') {
            if glob(neg, alias) {
                return false;
            }
        } else if glob(p, alias) {
            matched = true;
        }
    }
    matched
}

/// `*` and `?` wildcards, iterative with backtracking (no recursion blowup).
fn glob(pat: &str, s: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn split_kv(line: &str) -> Option<(String, String)> {
    let idx = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let (k, rest) = line.split_at(idx);
    let v = rest.trim_start().trim_start_matches('=').trim();
    let v = v
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(v);
    Some((k.to_owned(), v.to_owned()))
}

fn words(v: &str) -> Vec<String> {
    v.split_whitespace().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const CFG: &str = r#"
# work
Host bastion
    HostName bastion.example.com
    User ops
    IdentityFile ~/.ssh/work_ed25519

Host db-* !db-legacy
    ProxyJump ops@bastion.example.com:2222
    LocalForward 5432 localhost:5432

Host db-primary db-legacy
    HostName %h.internal
    Port=2200

Host web
    HostName 10.0.0.10
    DynamicForward 127.0.0.1:1080
    RemoteForward 0.0.0.0:9000 localhost:3000
    ProxyCommand nc -X 5 -x proxy:1080 %h %p

Match host foo
    User shouldnotapply

Host *
    User defaultuser
    ServerAliveInterval 15
    IdentityFile ~/.ssh/id_ed25519
    ForwardAgent yes
"#;

    #[test]
    fn first_value_wins_and_star_defaults_apply() {
        let r = parse_ssh_config(CFG);
        let names: Vec<_> = r.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["bastion", "db-primary", "db-legacy", "web"]);

        let b = &r.profiles[0];
        assert_eq!(b.host, "bastion.example.com");
        assert_eq!(
            b.user.as_deref(),
            Some("ops"),
            "block value beats Host * default"
        );
        assert_eq!(b.keepalive_secs, 15);
        assert!(b.agent_forwarding);
        // Both identity files, in order.
        assert_eq!(
            b.auth
                .iter()
                .filter(|a| matches!(a, AuthMethod::KeyFile { .. }))
                .count(),
            2
        );

        let primary = &r.profiles[1];
        assert_eq!(
            (primary.host.as_str(), primary.port),
            ("db-primary.internal", 2200)
        );
        assert_eq!(primary.user.as_deref(), Some("defaultuser"));
        assert_eq!(
            primary.jump,
            vec![Endpoint {
                host: "bastion.example.com".into(),
                port: 2222,
                user: Some("ops".into())
            }]
        );
        assert_eq!(
            primary.forwards[0].to_string(),
            "L 127.0.0.1:5432 -> localhost:5432"
        );

        let legacy = &r.profiles[2];
        assert!(legacy.jump.is_empty(), "negated pattern excluded db-legacy");
        assert!(legacy.forwards.is_empty());

        let web = &r.profiles[3];
        assert_eq!(web.forwards.len(), 2);
        assert_eq!(
            web.forwards[0].to_string(),
            "R 0.0.0.0:9000 -> localhost:3000"
        );
        assert_eq!(web.forwards[1].to_string(), "D 127.0.0.1:1080 (SOCKS5)");

        assert!(r.warnings.iter().any(|w| w.contains("Match")));
        assert!(r.warnings.iter().any(|w| w.contains("ProxyCommand")));
        assert!(
            r.profiles
                .iter()
                .all(|p| p.user.as_deref() != Some("shouldnotapply"))
        );
    }

    #[test]
    fn glob_matching() {
        assert!(glob("db-*", "db-x"));
        assert!(glob("*.example.com", "a.b.example.com"));
        assert!(glob("h?st", "host"));
        assert!(!glob("h?st", "hoost"));
        assert!(glob("*", ""));
        assert!(!glob("a*b", "acd"));
        assert!(glob("a*b*c", "aXbYc"));
    }

    #[test]
    fn hostile_input_does_not_panic() {
        for s in [
            "Host",
            "HostName",
            "=",
            "Host \"",
            "Port",
            "Host *\nPort 99999999",
            "\u{0}\u{FFFF}",
        ] {
            let _ = parse_ssh_config(s);
        }
    }

    #[test]
    fn includes_are_inlined_with_wildcards() -> std::io::Result<()> {
        let home = tempfile::tempdir()?;
        let ssh = home.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("conf.d"))?;
        std::fs::write(
            ssh.join("config"),
            "Include conf.d/*.conf ~/.ssh/extra missing\nHost main\n  HostName m\n",
        )?;
        std::fs::write(ssh.join("conf.d/b.conf"), "Host b\n  HostName hb\n")?;
        std::fs::write(ssh.join("conf.d/a.conf"), "Host a\n  HostName ha\n")?;
        std::fs::write(ssh.join("conf.d/skip.txt"), "Host skip\n")?;
        std::fs::write(ssh.join("extra"), "Include extra\nHost x\n  HostName hx\n")?; // self-loop
        let text = load_ssh_config(&ssh.join("config"))?;
        let r = parse_ssh_config(&text);
        let names: Vec<_> = r.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "x", "main"], "{text}");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(
            wildcard("*.conf", "a.conf")
                && !wildcard("*.conf", "a.txt")
                && wildcard("a?c*", "abcde")
        );
        Ok(())
    }
}
