//! `hostwick known-hosts list|remove|add`.

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use hostwick_core::Target;
use hostwick_ssh::{KnownHostEntry, KnownHosts};

use crate::app::App;

#[derive(Subcommand)]
pub enum KnownHostsCmd {
    /// List trusted host keys (optionally only those for HOST[:PORT]).
    List {
        host: Option<String>,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Forget a host's keys, like `ssh-keygen -R` (hashed entries too), or
    /// remove lines by number with `--line N`.
    Remove {
        /// HOST, HOST:PORT or [HOST]:PORT.
        host: Option<String>,
        /// Line number from `known-hosts list` (repeatable).
        #[arg(long = "line", value_name = "N")]
        lines: Vec<usize>,
    },
    /// Trust a key for a host: `add HOST[:PORT] ssh-ed25519 AAAA...` or
    /// `add HOST[:PORT] /path/to/key.pub`.
    Add {
        host: String,
        /// The public key line, or a file holding it.
        #[arg(required = true, num_args = 1..)]
        key: Vec<String>,
    },
}

/// `web`, `web:2222`, `[::1]:2222` → (host, port).
pub(crate) fn host_port(s: &str) -> Result<(String, u16)> {
    let t = Target::parse(s)?;
    if t.user.is_some() {
        bail!("give just the host (and port), without a user name");
    }
    Ok((t.host.clone(), t.port_or_default()))
}

fn entry_json(e: &KnownHostEntry) -> serde_json::Value {
    serde_json::json!({
        "line": e.line,
        "marker": e.marker.map(|m| m.as_str()),
        "hosts": e.hosts,
        "hashed": e.hashed,
        "key_type": e.key_type,
        "fingerprint": e.fingerprint,
        "comment": e.comment,
    })
}

pub fn run(app: &App, cmd: KnownHostsCmd) -> Result<i32> {
    let kh = KnownHosts::new(&app.paths.known_hosts);
    match cmd {
        KnownHostsCmd::List { host, json } => {
            let entries = match &host {
                Some(h) => {
                    let (h, p) = host_port(h)?;
                    kh.entries_for(&h, p)?
                }
                None => kh.entries()?,
            };
            if json {
                let v: Vec<_> = entries.iter().map(entry_json).collect();
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(0);
            }
            if entries.is_empty() {
                eprintln!("no entries in {}", kh.path().display());
                return Ok(0);
            }
            println!(
                "{:>5}  {:<32} {:<22} {:<50} COMMENT",
                "LINE", "HOST", "TYPE", "FINGERPRINT"
            );
            for e in &entries {
                let hosts = match e.marker {
                    Some(m) => format!("{} {}", m.as_str(), e.hosts_display()),
                    None => e.hosts_display(),
                };
                println!(
                    "{:>5}  {:<32} {:<22} {:<50} {}",
                    e.line, hosts, e.key_type, e.fingerprint, e.comment
                );
            }
        }
        KnownHostsCmd::Remove { host, lines } => {
            if host.is_none() && lines.is_empty() {
                bail!("give a host to forget, or --line N");
            }
            if let Some(h) = host {
                let (name, port) = host_port(&h)?;
                let n = kh.remove_host(&name, port)?;
                if n == 0 && lines.is_empty() {
                    eprintln!("{h}: not found in {}", kh.path().display());
                    return Ok(1);
                }
                println!(
                    "removed {n} entr{} for {h}",
                    if n == 1 { "y" } else { "ies" }
                );
            }
            if !lines.is_empty() {
                let n = kh.remove_lines(&lines)?;
                println!("removed {n} line(s)");
                if n < lines.len() {
                    return Ok(1);
                }
            }
        }
        KnownHostsCmd::Add { host, key } => {
            let (name, port) = host_port(&host)?;
            let joined = key.join(" ");
            let text = if key.len() == 1 && std::path::Path::new(&joined).is_file() {
                std::fs::read_to_string(&joined).with_context(|| format!("reading {joined}"))?
            } else {
                joined
            };
            let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            let pk = hostwick_keys::PublicKey::from_openssh(line.trim())
                .context("expected a public key such as `ssh-ed25519 AAAA... comment`")?;
            if kh.add(&name, port, &pk)? {
                println!(
                    "added {} key {} for {host}",
                    pk.algorithm().as_str(),
                    hostwick_keys::fingerprint(&pk)
                );
            } else {
                println!("{host} already trusts that key");
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts() -> Result<()> {
        assert_eq!(host_port("web")?, ("web".into(), 22));
        assert_eq!(host_port("web:2222")?, ("web".into(), 2222));
        assert_eq!(host_port("[::1]:2200")?, ("::1".into(), 2200));
        assert!(host_port("me@web").is_err());
        Ok(())
    }
}
