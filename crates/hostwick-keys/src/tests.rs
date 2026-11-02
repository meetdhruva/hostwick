#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

use super::*;

fn s(v: &str) -> SecretString {
    SecretString::from(v.to_owned())
}

fn same_key(a: &PrivateKey, b: &PrivateKey) {
    assert_eq!(a.public_key().key_data(), b.public_key().key_data());
    match (a.key_data(), b.key_data()) {
        // PuTTY canonicalises RSA keys to p > q (recomputing iqmp), so the
        // factors may come back swapped; the key itself is the same.
        (ssh_key::private::KeypairData::Rsa(x), ssh_key::private::KeypairData::Rsa(y)) => {
            let (x, y) = (x.private(), y.private());
            assert_eq!(x.d(), y.d(), "RSA private exponent differs");
            assert!(
                (x.p(), x.q()) == (y.p(), y.q()) || (x.p(), x.q()) == (y.q(), y.p()),
                "RSA factors differ"
            );
        }
        (x, y) => assert_eq!(x, y, "private keys differ"),
    }
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
}

/// Cheap Argon2 for the many round trips here.
const FAST: PpkKdf = PpkKdf {
    memory_kib: 256,
    passes: 1,
    parallelism: 1,
};

fn quick_types() -> Vec<KeyType> {
    vec![
        KeyType::Ed25519,
        KeyType::EcdsaP256,
        KeyType::EcdsaP384,
        KeyType::EcdsaP521,
    ]
}

#[test]
fn key_type_parsing() {
    assert_eq!(KeyType::parse("ed25519", None).unwrap(), KeyType::Ed25519);
    assert_eq!(KeyType::parse("ECDSA", None).unwrap(), KeyType::EcdsaP256);
    assert_eq!(
        KeyType::parse("ecdsa", Some(521)).unwrap(),
        KeyType::EcdsaP521
    );
    assert_eq!(
        KeyType::parse("ecdsa-sha2-nistp384", None).unwrap(),
        KeyType::EcdsaP384
    );
    assert_eq!(
        KeyType::parse("rsa", None).unwrap(),
        KeyType::Rsa { bits: 3072 }
    );
    assert_eq!(
        KeyType::parse("rsa", Some(4096)).unwrap().bits(),
        4096,
        "rsa 4096"
    );
    assert!(KeyType::parse("rsa", Some(1024)).is_err());
    assert!(KeyType::parse("ecdsa", Some(512)).is_err());
    assert!(KeyType::parse("ed25519", Some(384)).is_err());
    let dsa = KeyType::parse("dsa", None).unwrap_err().to_string();
    assert!(dsa.contains("insecure"), "{dsa}");
    assert!(KeyType::parse("foo", None).is_err());
    assert_eq!(KeyType::EcdsaP384.label(), "ECDSA P-384");
    assert_eq!(KeyType::Rsa { bits: 2048 }.default_file_name(), "id_rsa");
}

#[test]
fn generate_every_type_and_round_trip_openssh() {
    let mut types = quick_types();
    types.push(KeyType::Rsa { bits: 2048 });
    for t in types {
        let k = generate(t, "me@host").unwrap();
        assert_eq!(k.comment().as_str_lossy(), "me@host");
        assert_eq!(key_bits(k.public_key()), t.bits(), "{t:?}");

        let plain = to_openssh(&k, None).unwrap();
        assert!(plain.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        same_key(&k, &parse_any(&plain, None).unwrap());

        let enc = to_openssh(&k, Some(&s("pw"))).unwrap();
        assert!(matches!(
            parse_any(&enc, None),
            Err(KeyError::PassphraseRequired)
        ));
        assert!(matches!(
            parse_any(&enc, Some(&s("nope"))),
            Err(KeyError::WrongPassphrase)
        ));
        let back = parse_any(&enc, Some(&s("pw"))).unwrap();
        same_key(&k, &back);
        assert_eq!(back.comment().as_str_lossy(), "me@host");
        // An empty passphrase means "not encrypted".
        assert!(
            !PrivateKey::from_openssh(to_openssh(&k, Some(&s(""))).unwrap().as_str())
                .unwrap()
                .is_encrypted()
        );
    }
}

#[test]
fn ppk_round_trips_through_ssh_key_decoder() {
    for t in quick_types() {
        let k = generate(t, "ppk test").unwrap();
        for v in [PpkVersion::V2, PpkVersion::V3] {
            let plain = to_ppk(&k, None, v, &FAST).unwrap();
            assert!(plain.contains("Encryption: none"));
            same_key(&k, &parse_any(&plain, None).unwrap());

            let enc = to_ppk(&k, Some(&s("secret")), v, &FAST).unwrap();
            assert!(enc.contains("Encryption: aes256-cbc"));
            assert_eq!(enc.contains("Argon2-Salt"), v == PpkVersion::V3);
            assert!(matches!(
                parse_any(&enc, None),
                Err(KeyError::PassphraseRequired)
            ));
            assert!(matches!(
                parse_any(&enc, Some(&s("wrong"))),
                Err(KeyError::WrongPassphrase)
            ));
            let back = parse_any(&enc, Some(&s("secret"))).unwrap();
            same_key(&k, &back);
            assert_eq!(back.comment().as_str_lossy(), "ppk test");

            let info = inspect_text(&enc, None).unwrap();
            assert_eq!(info.format, KeyFormat::Ppk(v));
            assert!(info.encrypted);
            assert_eq!(info.comment, "ppk test");
            assert_eq!(info.fingerprint, fingerprint(k.public_key()));
        }
    }
}

#[test]
fn rsa_ppk_round_trip() {
    let k = generate(KeyType::Rsa { bits: 2048 }, "rsa").unwrap();
    let enc = to_ppk(&k, Some(&s("pw")), PpkVersion::V3, &FAST).unwrap();
    same_key(&k, &parse_any(&enc, Some(&s("pw"))).unwrap());
    let v2 = to_ppk(&k, None, PpkVersion::V2, &FAST).unwrap();
    same_key(&k, &parse_any(&v2, None).unwrap());
}

#[test]
fn tampered_unencrypted_ppk_is_reported_as_damaged() {
    let k = generate(KeyType::Ed25519, "x").unwrap();
    let text = to_ppk(&k, None, PpkVersion::V3, &FAST).unwrap();
    let tampered = text.replace("Comment: x", "Comment: y");
    let err = parse_any(&tampered, None).unwrap_err().to_string();
    assert!(err.contains("damaged"), "{err}");
}

#[test]
fn files_permissions_overwrite_and_inspect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    let k = generate(KeyType::Ed25519, "files").unwrap();
    write_openssh(&k, &path, Some(&s("pw")), false).unwrap();
    assert!(public_path(&path).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    // No overwrite without force; nothing left behind.
    assert!(matches!(
        write_openssh(&k, &path, None, false),
        Err(KeyError::Exists(_))
    ));
    assert!(matches!(
        write_public(k.public_key(), &public_path(&path), false),
        Err(KeyError::Exists(_))
    ));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);

    // The comment of an encrypted OpenSSH key comes from the .pub file.
    let info = inspect(&path).unwrap();
    assert_eq!(info.format, KeyFormat::OpenSsh);
    assert!(info.encrypted);
    assert_eq!(info.comment, "files");
    assert_eq!(info.type_label, "ED25519");
    assert_eq!(info.bits, 256);
    assert_eq!(info.algorithm, "ssh-ed25519");
    let pubinfo = inspect(&public_path(&path)).unwrap();
    assert_eq!(pubinfo.format, KeyFormat::Public);
    assert_eq!(pubinfo.fingerprint, info.fingerprint);

    // Change, then remove, the passphrase in place.
    assert_eq!(
        change_passphrase(&path, Some(&s("pw")), Some(&s("new"))).unwrap(),
        KeyFormat::OpenSsh
    );
    assert!(matches!(
        load_any(&path, Some(&s("pw"))),
        Err(KeyError::WrongPassphrase)
    ));
    change_passphrase(&path, Some(&s("new")), None).unwrap();
    same_key(&k, &load_any(&path, None).unwrap());

    let ppk = dir.path().join("k.ppk");
    write_ppk(&k, &ppk, Some(&s("a")), PpkVersion::V3, false).unwrap();
    assert!(write_ppk(&k, &ppk, None, PpkVersion::V3, false).is_err());
    assert_eq!(
        change_passphrase(&ppk, Some(&s("a")), Some(&s("b"))).unwrap(),
        KeyFormat::Ppk(PpkVersion::V3)
    );
    same_key(&k, &load_any(&ppk, Some(&s("b"))).unwrap());
    write_ppk(&k, &ppk, None, PpkVersion::V2, true).unwrap();
    assert_eq!(
        inspect(&ppk).unwrap().format,
        KeyFormat::Ppk(PpkVersion::V2)
    );

    let err = load_any(&public_path(&path), None).unwrap_err();
    assert!(matches!(err, KeyError::PublicOnly(_)), "{err}");
    std::fs::write(dir.path().join("junk"), "hello").unwrap();
    assert!(matches!(
        load_any(&dir.path().join("junk"), None),
        Err(KeyError::Format(_))
    ));
}

#[test]
fn public_line_and_fingerprint() {
    let k = generate(KeyType::Ed25519, "a b").unwrap();
    let line = public_openssh(k.public_key());
    assert!(line.starts_with("ssh-ed25519 AAAA"));
    assert!(line.ends_with(" a b"));
    assert!(fingerprint(k.public_key()).starts_with("SHA256:"));
    let art = randomart(k.public_key());
    assert!(art.starts_with("+--[ED25519 256]--+\n"), "{art}");
    assert!(art.ends_with("+----[SHA256]-----+"), "{art}");
    assert_eq!(art.lines().count(), 11);
}

// ---- interoperability with the real tools (skipped when not installed) ----

fn run(cmd: &mut Command) -> std::process::Output {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn pass_file(dir: &Path, pass: &str) -> std::path::PathBuf {
    let p = dir.join(format!("pass-{}", pass.len()));
    std::fs::write(&p, pass).unwrap();
    p
}

#[test]
fn puttygen_reads_our_ppk_files() {
    if !have("puttygen") {
        eprintln!("skipping: puttygen not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut types = quick_types();
    types.push(KeyType::Rsa { bits: 2048 });
    for (i, t) in types.into_iter().enumerate() {
        let k = generate(t, &format!("interop {i}")).unwrap();
        for v in [PpkVersion::V2, PpkVersion::V3] {
            for pass in [None, Some("pütty pass")] {
                let ppk = dir
                    .path()
                    .join(format!("k{i}-{v:?}-{}.ppk", pass.is_some()));
                // Default (puttygen-strength) KDF for real files.
                write_ppk(&k, &ppk, pass.map(s).as_ref(), v, false).unwrap();
                let out = dir.path().join(format!("{}.openssh", ppk.display()));
                let mut cmd = Command::new("puttygen");
                cmd.arg(&ppk);
                if let Some(p) = pass {
                    cmd.arg("--old-passphrase").arg(pass_file(dir.path(), p));
                }
                run(cmd.args(["-O", "private-openssh-new", "-o"]).arg(&out));
                // puttygen keeps the passphrase on export.
                let back = load_any(&out, pass.map(s).as_ref()).unwrap();
                same_key(&k, &back);
                assert_eq!(back.comment().as_str_lossy(), format!("interop {i}"));

                // `puttygen -L` prints the same authorized_keys line.
                let mut cmd = Command::new("puttygen");
                cmd.arg(&ppk);
                if let Some(p) = pass {
                    cmd.arg("--old-passphrase").arg(pass_file(dir.path(), p));
                }
                let listed = run(cmd.arg("-L"));
                assert_eq!(
                    String::from_utf8_lossy(&listed.stdout).trim(),
                    public_openssh(k.public_key())
                );
            }
        }
    }
}

#[test]
fn we_read_puttygen_and_ssh_keygen_files() {
    let dir = tempfile::tempdir().unwrap();
    if have("puttygen") {
        let pass = pass_file(dir.path(), "from puttygen");
        for (t, b) in [("ed25519", "256"), ("ecdsa", "384"), ("rsa", "2048")] {
            let ppk = dir.path().join(format!("{t}.ppk"));
            run(Command::new("puttygen")
                .args(["-t", t, "-b", b, "-C", "made by puttygen", "-o"])
                .arg(&ppk)
                .arg("--new-passphrase")
                .arg(&pass));
            let openssh = dir.path().join(format!("{t}.openssh"));
            run(Command::new("puttygen")
                .arg(&ppk)
                .arg("--old-passphrase")
                .arg(&pass)
                .args(["-O", "private-openssh-new", "-o"])
                .arg(&openssh));
            let ours = load_any(&ppk, Some(&s("from puttygen"))).unwrap();
            same_key(
                &ours,
                &load_any(&openssh, Some(&s("from puttygen"))).unwrap(),
            );
            let info = inspect(&ppk).unwrap();
            assert!(info.encrypted);
            assert_eq!(info.comment, "made by puttygen");
            // And PPK v2 from puttygen.
            let v2 = dir.path().join(format!("{t}-v2.ppk"));
            run(Command::new("puttygen")
                .arg(&ppk)
                .arg("--old-passphrase")
                .arg(&pass)
                .arg("--new-passphrase")
                .arg(&pass)
                .args([
                    "--reencrypt",
                    "--ppk-param",
                    "version=2",
                    "-O",
                    "private",
                    "-o",
                ])
                .arg(&v2));
            same_key(&ours, &load_any(&v2, Some(&s("from puttygen"))).unwrap());
        }
    } else {
        eprintln!("skipping puttygen part: not installed");
    }

    if have("ssh-keygen") {
        // Our encrypted OpenSSH key opens in ssh-keygen, and the randomart
        // matches `ssh-keygen -lv`.
        let k = generate(KeyType::Ed25519, "kg").unwrap();
        let path = dir.path().join("ours");
        write_openssh(&k, &path, Some(&s("kgpass")), false).unwrap();
        let out = run(Command::new("ssh-keygen")
            .args(["-y", "-P", "kgpass", "-f"])
            .arg(&path));
        let printed = String::from_utf8_lossy(&out.stdout);
        let ours = public_openssh(k.public_key());
        assert!(ours.starts_with(printed.trim()), "{printed} vs {ours}");
        let out = run(Command::new("ssh-keygen")
            .arg("-lvf")
            .arg(public_path(&path)));
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(&fingerprint(k.public_key())));
        assert!(text.contains(&randomart(k.public_key())), "{text}");

        // Legacy PEM written by ssh-keygen (plain and encrypted).
        for (name, pass) in [("pem", ""), ("pem-enc", "pempass")] {
            let p = dir.path().join(name);
            run(Command::new("ssh-keygen")
                .args([
                    "-q", "-t", "rsa", "-b", "2048", "-m", "PEM", "-N", pass, "-f",
                ])
                .arg(&p));
            let pw = (!pass.is_empty()).then(|| s(pass));
            let k = load_any(&p, pw.as_ref()).unwrap();
            let info = inspect(&p).unwrap();
            assert_eq!(info.format, KeyFormat::Pem);
            assert_eq!(info.encrypted, !pass.is_empty());
            assert_eq!(info.fingerprint, fingerprint(k.public_key()));
            if pw.is_some() {
                assert!(matches!(
                    load_any(&p, Some(&s("bad"))),
                    Err(KeyError::WrongPassphrase)
                ));
            }
        }
    } else {
        eprintln!("skipping ssh-keygen part: not installed");
    }
}
