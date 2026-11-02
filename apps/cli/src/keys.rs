//! `hostwick keygen`, `keyconv` and `keyinfo` — ssh-keygen and puttygen in
//! one place.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use hostwick_core::{ExposeSecret, SecretString};
use hostwick_keys::{KeyError, KeyFormat, KeyType, PpkVersion};

use crate::app::App;

#[derive(Args)]
pub struct KeygenArgs {
    /// Key type: ed25519 (default), ecdsa or rsa.
    #[arg(short = 't', long = "type", default_value = "ed25519")]
    kind: String,
    /// Key size: ecdsa 256 (default), 384 or 521; rsa 2048-16384 (default 3072).
    #[arg(short = 'b', long)]
    bits: Option<u32>,
    /// Comment stored with the key (default: user@computer).
    #[arg(short = 'C', long)]
    comment: Option<String>,
    /// Output file (default: ~/.ssh/id_<type>, plus .ppk with --ppk).
    #[arg(short = 'f', long = "file")]
    file: Option<PathBuf>,
    /// Save as a PuTTY .ppk (format 3) instead of OpenSSH; the OpenSSH .pub
    /// file is written too.
    #[arg(long, conflicts_with = "ppk2")]
    ppk: bool,
    /// Save as a PuTTY .ppk format 2 (PuTTY before 0.75, old WinSCP/FileZilla).
    #[arg(long)]
    ppk2: bool,
    /// Passphrase; "" for none. Prompted for when omitted. Note: other local
    /// users can see command-line arguments; prefer the prompt.
    #[arg(short = 'N', long = "passphrase")]
    passphrase: Option<String>,
    /// Replace existing files.
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutFormat {
    /// OpenSSH (`-----BEGIN OPENSSH PRIVATE KEY-----`) plus a .pub file.
    Openssh,
    /// PuTTY .ppk format 3.
    Ppk,
    /// PuTTY .ppk format 2.
    Ppk2,
}

#[derive(Args)]
pub struct KeyconvArgs {
    /// Key file: OpenSSH, PuTTY .ppk (v2/v3) or legacy PEM.
    input: PathBuf,
    /// Output format (default: PuTTY for OpenSSH/PEM input, OpenSSH for .ppk).
    #[arg(long, value_enum)]
    to: Option<OutFormat>,
    /// Output file (default: `key` ↔ `key.ppk`; the same file when the
    /// format does not change, e.g. to change the passphrase).
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
    /// Current passphrase (prompted for when needed).
    #[arg(short = 'P', long = "passphrase")]
    passphrase: Option<String>,
    /// Protect the output with a new passphrase. Without a value it is
    /// prompted for; "" removes the passphrase. Default: keep the current one.
    #[arg(long = "new-passphrase", num_args = 0..=1, value_name = "PASSPHRASE")]
    new_passphrase: Option<Option<String>>,
    /// Replace the comment.
    #[arg(short = 'C', long)]
    comment: Option<String>,
    /// Replace an existing output file.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
pub struct KeyinfoArgs {
    /// Private key (any format) or .pub file.
    file: PathBuf,
    /// Also draw the fingerprint's randomart picture.
    #[arg(long)]
    randomart: bool,
    /// Machine-readable output.
    #[arg(long)]
    json: bool,
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_owned())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".into())
}

fn ssh_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("no home directory; use -f to choose where to save the key")?;
    Ok(PathBuf::from(home).join(".ssh"))
}

/// `~/.ssh` is created private, like OpenSSH does.
fn ensure_private_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn new_passphrase(app: &App, given: Option<String>, what: &str) -> Result<SecretString> {
    if let Some(p) = given {
        return Ok(secret(&p));
    }
    if app.policy.batch {
        bail!("--batch: give the {what} with -N (\"\" for none)");
    }
    let p = crate::tty::read_secret("Enter passphrase (empty for no passphrase): ")?;
    let again = crate::tty::read_secret("Enter same passphrase again: ")?;
    if p.expose_secret() != again.expose_secret() {
        bail!("passphrases do not match");
    }
    Ok(p)
}

fn current_passphrase(app: &App, path: &Path, given: Option<String>) -> Result<SecretString> {
    if let Some(p) = given {
        return Ok(secret(&p));
    }
    if app.policy.batch {
        bail!(
            "{} is passphrase-protected; give it with -P",
            path.display()
        );
    }
    crate::tty::read_secret(&format!("Enter passphrase for {}: ", path.display()))
}

fn friendly(e: KeyError, path: &Path) -> anyhow::Error {
    match e {
        KeyError::Exists(p) => {
            anyhow::anyhow!("{} already exists (use --force to replace it)", p.display())
        }
        KeyError::WrongPassphrase => anyhow::anyhow!("wrong passphrase for {}", path.display()),
        other => anyhow::Error::new(other).context(path.display().to_string()),
    }
}

fn print_saved(private: &Path, public: Option<&Path>, key: &hostwick_keys::PublicKey) {
    println!(
        "Your identification has been saved in {}",
        private.display()
    );
    if let Some(p) = public {
        println!("Your public key has been saved in {}", p.display());
    }
    println!("The key fingerprint is:");
    println!(
        "{} {}",
        hostwick_keys::fingerprint(key),
        key.comment().as_str_lossy()
    );
    println!("The key's randomart image is:");
    println!("{}", hostwick_keys::randomart(key));
}

pub fn keygen(app: &App, a: KeygenArgs) -> Result<i32> {
    let kind = KeyType::parse(&a.kind, a.bits)?;
    let ppk = if a.ppk2 {
        Some(PpkVersion::V2)
    } else if a.ppk {
        Some(PpkVersion::V3)
    } else {
        None
    };
    let path = match a.file {
        Some(f) => f,
        None => {
            let dir = ssh_dir()?;
            ensure_private_dir(&dir)?;
            let name = kind.default_file_name();
            dir.join(if ppk.is_some() {
                format!("{name}.ppk")
            } else {
                name.to_owned()
            })
        }
    };
    // `work.ppk` → `work.pub`; `id_ed25519` → `id_ed25519.pub`.
    let pub_path = if ppk.is_some()
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ppk"))
    {
        path.with_extension("pub")
    } else {
        hostwick_keys::public_path(&path)
    };
    if !a.force {
        for p in [&path, &pub_path] {
            if p.exists() {
                bail!("{} already exists (use --force to replace it)", p.display());
            }
        }
    }
    let comment = a.comment.unwrap_or_else(|| {
        format!(
            "{}@{}",
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "me".into()),
            computer_name()
        )
    });
    let pass = new_passphrase(app, a.passphrase, "passphrase")?;
    eprintln!("Generating {} key pair...", kind.label());
    let key = hostwick_keys::generate(kind, &comment)?;
    match ppk {
        Some(v) => {
            hostwick_keys::write_ppk(&key, &path, Some(&pass), v, a.force)
                .map_err(|e| friendly(e, &path))?;
            hostwick_keys::write_public(key.public_key(), &pub_path, a.force)
                .map_err(|e| friendly(e, &pub_path))?;
        }
        None => hostwick_keys::write_openssh(&key, &path, Some(&pass), a.force)
            .map_err(|e| friendly(e, &path))?,
    }
    print_saved(&path, Some(&pub_path), key.public_key());
    Ok(0)
}

fn default_output(input: &Path, from: KeyFormat, to: OutFormat) -> PathBuf {
    let is_ppk = input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ppk"));
    match (from, to) {
        (KeyFormat::Ppk(PpkVersion::V3), OutFormat::Ppk)
        | (KeyFormat::Ppk(PpkVersion::V2), OutFormat::Ppk2)
        | (KeyFormat::OpenSsh | KeyFormat::Pem, OutFormat::Openssh) => input.to_path_buf(),
        (_, OutFormat::Ppk | OutFormat::Ppk2) => {
            if is_ppk {
                input.to_path_buf()
            } else {
                let mut s = input.as_os_str().to_owned();
                s.push(".ppk");
                PathBuf::from(s)
            }
        }
        (_, OutFormat::Openssh) => {
            if is_ppk {
                input.with_extension("")
            } else {
                input.to_path_buf()
            }
        }
    }
}

pub fn keyconv(app: &App, a: KeyconvArgs) -> Result<i32> {
    let info = hostwick_keys::inspect(&a.input).map_err(|e| friendly(e, &a.input))?;
    if info.format == KeyFormat::Public {
        bail!(
            "{} is a public key; convert the private key instead",
            a.input.display()
        );
    }
    let old = if info.encrypted {
        Some(current_passphrase(app, &a.input, a.passphrase)?)
    } else {
        None
    };
    let mut key =
        hostwick_keys::load_any(&a.input, old.as_ref()).map_err(|e| friendly(e, &a.input))?;
    if let Some(c) = &a.comment {
        hostwick_keys::set_comment(&mut key, c);
    }
    let to = a.to.unwrap_or(match info.format {
        KeyFormat::Ppk(_) => OutFormat::Openssh,
        _ => OutFormat::Ppk,
    });
    let output = a
        .output
        .unwrap_or_else(|| default_output(&a.input, info.format, to));
    let in_place = std::path::absolute(&output).ok() == std::path::absolute(&a.input).ok();
    let force = a.force || in_place;
    let new = match a.new_passphrase {
        None => old,
        Some(None) => Some(new_passphrase(app, None, "new passphrase")?),
        Some(Some(p)) => Some(secret(&p)),
    };
    let encrypted = new.as_ref().is_some_and(|p| !p.expose_secret().is_empty());
    match to {
        OutFormat::Openssh => {
            hostwick_keys::write_openssh(&key, &output, new.as_ref(), force)
                .map_err(|e| friendly(e, &output))?;
            println!(
                "saved OpenSSH key{} to {} (public key: {})",
                if encrypted { " (encrypted)" } else { "" },
                output.display(),
                hostwick_keys::public_path(&output).display()
            );
        }
        OutFormat::Ppk | OutFormat::Ppk2 => {
            let v = if to == OutFormat::Ppk2 {
                PpkVersion::V2
            } else {
                PpkVersion::V3
            };
            hostwick_keys::write_ppk(&key, &output, new.as_ref(), v, force)
                .map_err(|e| friendly(e, &output))?;
            println!(
                "saved PuTTY {} key{} to {}",
                if v == PpkVersion::V2 {
                    "PPK v2"
                } else {
                    "PPK v3"
                },
                if encrypted { " (encrypted)" } else { "" },
                output.display()
            );
        }
    }
    Ok(0)
}

pub fn keyinfo(a: KeyinfoArgs) -> Result<i32> {
    let info = hostwick_keys::inspect(&a.file).map_err(|e| friendly(e, &a.file))?;
    let public = hostwick_keys::public_openssh(&info.public_key);
    if a.json {
        let v = serde_json::json!({
            "file": a.file.display().to_string(),
            "format": info.format.to_string(),
            "type": info.type_label,
            "algorithm": info.algorithm,
            "bits": info.bits,
            "fingerprint": info.fingerprint,
            "comment": info.comment,
            "encrypted": info.encrypted,
            "public_key": public,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(0);
    }
    println!("file:        {}", a.file.display());
    println!("format:      {}", info.format);
    println!("type:        {} ({})", info.type_label, info.algorithm);
    println!("bits:        {}", info.bits);
    println!("fingerprint: {}", info.fingerprint);
    println!(
        "comment:     {}",
        if info.comment.is_empty() {
            "(none)"
        } else {
            &info.comment
        }
    );
    println!("encrypted:   {}", if info.encrypted { "yes" } else { "no" });
    println!("public key:  {public}");
    if a.randomart {
        println!("{}", hostwick_keys::randomart(&info.public_key));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_outputs() {
        let p = Path::new;
        assert_eq!(
            default_output(p("/k/id_rsa"), KeyFormat::OpenSsh, OutFormat::Ppk),
            p("/k/id_rsa.ppk")
        );
        assert_eq!(
            default_output(
                p("/k/work.ppk"),
                KeyFormat::Ppk(PpkVersion::V3),
                OutFormat::Openssh
            ),
            p("/k/work")
        );
        assert_eq!(
            default_output(
                p("/k/work.ppk"),
                KeyFormat::Ppk(PpkVersion::V3),
                OutFormat::Ppk
            ),
            p("/k/work.ppk")
        );
        assert_eq!(
            default_output(
                p("/k/work.ppk"),
                KeyFormat::Ppk(PpkVersion::V3),
                OutFormat::Ppk2
            ),
            p("/k/work.ppk")
        );
        assert_eq!(
            default_output(p("/k/id"), KeyFormat::OpenSsh, OutFormat::Openssh),
            p("/k/id")
        );
        assert_eq!(
            default_output(p("/k/old.pem"), KeyFormat::Pem, OutFormat::Openssh),
            p("/k/old.pem")
        );
    }
}
