use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hostwick_core::{AuthMethod, ExposeSecret};
use russh::MethodKind;
use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::agent::AgentIdentity;
use russh::keys::{Algorithm, Certificate, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey};

use crate::gssapi::{self, GssAuthError, KerberosAuth};
use crate::handler::ClientHandler;
use crate::interaction::{Credentials, Interaction, SecretUse};
use crate::known_hosts::fingerprint;
use crate::{SshError, agent};

pub(crate) struct AuthCtx<'a> {
    pub user: &'a str,
    pub host: &'a str,
    pub interaction: &'a dyn Interaction,
    pub credentials: &'a dyn Credentials,
}

/// Runs the configured auth methods in order, like OpenSSH's
/// `PreferredAuthentications`, until the server reports full success.
///
/// Starts with `none` to learn which methods the server accepts, so the person
/// is never prompted for a password the server would refuse anyway. Handles
/// partial success (e.g. publickey + OTP) by continuing down the list.
pub(crate) async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    methods: &[AuthMethod],
    ctx: &AuthCtx<'_>,
) -> Result<(), SshError> {
    let mut allowed: Vec<MethodKind> = match handle.authenticate_none(ctx.user).await? {
        AuthResult::Success => return Ok(()),
        AuthResult::Failure {
            remaining_methods, ..
        } => remaining_methods.to_vec(),
    };

    let mut tried: Vec<String> = Vec::new();
    for method in methods {
        let label = match method {
            AuthMethod::Agent => "agent".to_owned(),
            AuthMethod::KeyFile { path, .. } => format!(
                "key {}",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            AuthMethod::Password { .. } => "password".to_owned(),
            AuthMethod::KeyboardInteractive => "keyboard-interactive".to_owned(),
            AuthMethod::Gssapi { .. } => "kerberos".to_owned(),
        };
        let kind = match method {
            AuthMethod::Agent | AuthMethod::KeyFile { .. } => MethodKind::PublicKey,
            AuthMethod::Password { .. } => MethodKind::Password,
            AuthMethod::KeyboardInteractive => MethodKind::KeyboardInteractive,
            AuthMethod::Gssapi { .. } => MethodKind::GssapiWithMic,
        };
        if !allowed.is_empty() && !allowed.contains(&kind) {
            tried.push(format!("{label}: not offered by server"));
            continue;
        }
        let result = match method {
            AuthMethod::Agent => try_agent(handle, ctx).await,
            AuthMethod::KeyFile {
                path,
                passphrase_credential,
            } => try_key_file(handle, ctx, path, passphrase_credential.as_deref()).await,
            AuthMethod::Password { credential } => {
                try_password(handle, ctx, credential.as_deref()).await
            }
            AuthMethod::KeyboardInteractive => try_keyboard_interactive(handle, ctx).await,
            AuthMethod::Gssapi { delegate } => try_gssapi(handle, ctx, *delegate).await,
        };
        match result {
            Ok(Step::Done) => return Ok(()),
            Ok(Step::Continue { remaining }) => {
                tried.push(match &remaining {
                    Some(_) => format!("{label}: rejected"),
                    None => format!("{label}: skipped (nothing entered)"),
                });
                if let Some(r) = remaining {
                    allowed = r;
                }
            }
            // A broken key file, missing agent or missing Kerberos ticket
            // shouldn't stop the next method.
            Err(e @ (SshError::Key { .. } | SshError::Agent(_) | SshError::Gssapi(_))) => {
                tracing::info!(error = %e, "auth method unavailable, trying next");
                let reason = match &e {
                    SshError::Key { reason, .. } => reason.clone(),
                    SshError::Agent(r) | SshError::Gssapi(r) => r.clone(),
                    _ => e.to_string(),
                };
                if matches!(e, SshError::Key { .. } | SshError::Gssapi(_)) {
                    ctx.interaction.notice(&format!("{label}: {reason}"));
                }
                tried.push(format!("{label}: {reason}"));
            }
            Err(e) => return Err(e),
        }
    }

    Err(SshError::AuthFailed {
        user: ctx.user.to_owned(),
        host: ctx.host.to_owned(),
        allowed: allowed
            .iter()
            .map(|m| <&str>::from(m).to_owned())
            .collect::<Vec<_>>()
            .join(", "),
        tried: if tried.is_empty() {
            "nothing configured".into()
        } else {
            tried.join("; ")
        },
    })
}

enum Step {
    Done,
    Continue { remaining: Option<Vec<MethodKind>> },
}

fn step(r: AuthResult) -> Step {
    match r {
        AuthResult::Success => Step::Done,
        AuthResult::Failure {
            remaining_methods,
            partial_success,
        } => {
            if partial_success {
                tracing::info!("partial auth success; further methods required");
            }
            Step::Continue {
                remaining: Some(remaining_methods.to_vec()),
            }
        }
    }
}

async fn rsa_hash(handle: &Handle<ClientHandler>) -> Option<russh::keys::HashAlg> {
    handle
        .best_supported_rsa_hash()
        .await
        .ok()
        .flatten()
        .flatten()
}

async fn try_agent(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthCtx<'_>,
) -> Result<Step, SshError> {
    let mut agent = agent::connect().await?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| SshError::Agent(e.to_string()))?;
    if identities.is_empty() {
        return Err(SshError::Agent("agent holds no keys".into()));
    }
    let hash = rsa_hash(handle).await;
    let mut last = Step::Continue { remaining: None };
    let mut sign_errors = Vec::new();
    for id in identities {
        let mut signer = AgentSigner {
            agent: &mut agent,
            interaction: ctx.interaction,
        };
        // Certificates (e.g. from `ssh-add` of a key with its -cert.pub)
        // are offered as certificates; the agent signs with the key.
        let res = match &id {
            AgentIdentity::Certificate { certificate, .. } => {
                handle
                    .authenticate_certificate_with(ctx.user, certificate.clone(), hash, &mut signer)
                    .await
            }
            AgentIdentity::PublicKey { key, .. } => {
                handle
                    .authenticate_publickey_with(ctx.user, key.clone(), hash, &mut signer)
                    .await
            }
        };
        match res {
            Ok(r) => {
                last = step(r);
                if matches!(last, Step::Done) {
                    return Ok(last);
                }
            }
            // The agent could not sign with this key (declined confirmation,
            // security key not touched, ...). The handle stays usable: try
            // the next identity.
            Err(e) => {
                let k = id.public_key();
                tracing::info!(error = %e, key = %fingerprint(&k), "agent could not sign");
                sign_errors.push(format!("{} {}: {e}", k.algorithm(), fingerprint(&k)));
            }
        }
    }
    if matches!(last, Step::Continue { remaining: None }) && !sign_errors.is_empty() {
        return Err(SshError::Agent(format!(
            "agent could not sign ({})",
            sign_errors.join("; ")
        )));
    }
    Ok(last)
}

fn is_security_key(alg: &Algorithm) -> bool {
    matches!(alg, Algorithm::SkEd25519 | Algorithm::SkEcdsaSha2NistP256)
}

/// Signs through the agent; for FIDO security keys it first tells the
/// person to touch the key, since the agent blocks until they do.
struct AgentSigner<'a> {
    agent: &'a mut agent::Agent,
    interaction: &'a dyn Interaction,
}

impl russh::Signer for AgentSigner<'_> {
    type Error = russh::AgentAuthError;

    fn auth_sign(
        &mut self,
        key: &AgentIdentity,
        hash_alg: Option<HashAlg>,
        to_sign: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        let public = key.public_key();
        if is_security_key(&public.algorithm()) {
            self.interaction.notice(&format!(
                "Confirm user presence for key {} {}: touch your security key",
                public.algorithm(),
                fingerprint(&public)
            ));
        }
        russh::Signer::auth_sign(self.agent, key, hash_alg, to_sign)
    }
}

/// Signs certificate authentication with a key loaded from disk. Unlike
/// russh's built-in certificate method this honours the negotiated RSA hash
/// (`rsa-sha2-512-cert-v01@openssh.com`), which modern servers require.
struct LocalKeySigner(Arc<PrivateKey>);

#[derive(Debug, thiserror::Error)]
enum LocalSignError {
    #[error(transparent)]
    Send(#[from] russh::SendError),
    #[error("signing failed: {0}")]
    Sign(String),
}

impl russh::Signer for LocalKeySigner {
    type Error = LocalSignError;

    fn auth_sign(
        &mut self,
        _key: &AgentIdentity,
        hash_alg: Option<HashAlg>,
        mut to_sign: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        let res = sign_local(&self.0, hash_alg, &to_sign).and_then(|blob| {
            let len = u32::try_from(blob.len()).map_err(|e| LocalSignError::Sign(e.to_string()))?;
            to_sign.extend_from_slice(&len.to_be_bytes());
            to_sign.extend_from_slice(&blob);
            Ok(to_sign)
        });
        std::future::ready(res)
    }
}

/// SSH signature blob (`string alg, string sig`) over `data`.
fn sign_local(
    key: &PrivateKey,
    hash_alg: Option<HashAlg>,
    data: &[u8],
) -> Result<Vec<u8>, LocalSignError> {
    use russh::keys::ssh_key::encoding::Encode;
    use russh::keys::ssh_key::private::KeypairData;
    use signature::Signer;
    let err = |e: &dyn std::fmt::Display| LocalSignError::Sign(e.to_string());
    let sig: russh::keys::ssh_key::Signature = match key.key_data() {
        KeypairData::Rsa(kp) => (kp, hash_alg).try_sign(data).map_err(|e| err(&e))?,
        _ => key.try_sign(data).map_err(|e| err(&e))?,
    };
    sig.encode_vec().map_err(|e| err(&e))
}

fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| path.to_owned()),
        Err(_) => path.to_owned(),
    }
}

const SECURITY_KEY_FILE: &str = "security-key (FIDO) files need the key to be touched; \
     add it to ssh-agent / Pageant instead (`ssh-add <file>`)";

pub(crate) async fn load_key(
    path: &Path,
    passphrase_credential: Option<&str>,
    ctx: &AuthCtx<'_>,
) -> Result<PrivateKey, SshError> {
    let path = expand_tilde(path);
    let key_err = |reason: String| SshError::Key {
        path: path.display().to_string(),
        reason,
    };
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| key_err(e.to_string()))?;
    if text.starts_with("PuTTY-User-Key-File") {
        return load_ppk(&text, passphrase_credential, ctx, &path).await;
    }
    // The public half of an OpenSSH key file is readable without the
    // passphrase: catch FIDO keys before prompting for anything.
    if let Ok(k) = PrivateKey::from_openssh(&text)
        && is_security_key(&k.algorithm())
    {
        return Err(key_err(SECURITY_KEY_FILE.into()));
    }
    match russh::keys::decode_secret_key(&text, None) {
        Ok(k) => Ok(k),
        Err(russh::keys::Error::KeyIsEncrypted) => {
            let used = SecretUse::KeyPassphrase(&path);
            // A saved passphrase first; if it no longer fits (the key was
            // re-encrypted), ask once instead of failing.
            if let Some(saved) = passphrase_credential.and_then(|c| ctx.credentials.get(c)) {
                match russh::keys::decode_secret_key(&text, Some(saved.expose_secret())) {
                    Ok(k) => {
                        ctx.interaction.secret_outcome(used, true, true);
                        return Ok(k);
                    }
                    Err(_) => ctx.interaction.secret_outcome(used, true, false),
                }
            }
            let pass = ctx
                .interaction
                .key_passphrase(&path)
                .await
                .ok_or_else(|| key_err("passphrase required".into()))?;
            let res = russh::keys::decode_secret_key(&text, Some(pass.expose_secret()))
                .map_err(|e| key_err(e.to_string()));
            ctx.interaction.secret_outcome(used, false, res.is_ok());
            res
        }
        Err(e) => Err(key_err(e.to_string())),
    }
}

/// PuTTY `.ppk` (format 2 or 3, plain or AES-256 encrypted).
async fn load_ppk(
    text: &str,
    passphrase_credential: Option<&str>,
    ctx: &AuthCtx<'_>,
    path: &Path,
) -> Result<PrivateKey, SshError> {
    let key_err = |reason: String| SshError::Key {
        path: path.display().to_string(),
        reason,
    };
    let encrypted = !text.lines().any(|l| l.trim() == "Encryption: none");
    let decode = |passphrase: Option<String>| {
        PrivateKey::from_ppk(text, passphrase).map_err(|e| {
            let msg = e.to_string();
            if encrypted && msg.to_lowercase().contains("mac") {
                key_err("wrong passphrase".into())
            } else {
                key_err(msg)
            }
        })
    };
    if !encrypted {
        return decode(None);
    }
    let used = SecretUse::KeyPassphrase(path);
    if let Some(saved) = passphrase_credential.and_then(|c| ctx.credentials.get(c)) {
        match decode(Some(saved.expose_secret().to_owned())) {
            Ok(k) => {
                ctx.interaction.secret_outcome(used, true, true);
                return Ok(k);
            }
            // Stale saved passphrase: ask once below.
            Err(_) => ctx.interaction.secret_outcome(used, true, false),
        }
    }
    let pass = ctx
        .interaction
        .key_passphrase(path)
        .await
        .ok_or_else(|| key_err("passphrase required".into()))?;
    let res = decode(Some(pass.expose_secret().to_owned()));
    ctx.interaction.secret_outcome(used, false, res.is_ok());
    res
}

async fn try_key_file(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthCtx<'_>,
    path: &Path,
    passphrase_credential: Option<&str>,
) -> Result<Step, SshError> {
    let key = Arc::new(load_key(path, passphrase_credential, ctx).await?);
    let hash = if key.algorithm().is_rsa() {
        rsa_hash(handle).await
    } else {
        None
    };
    // An OpenSSH certificate next to the key (id_ed25519-cert.pub) goes
    // first; if the server won't take it, the plain key may still work.
    if let Some(cert) = find_certificate(&expand_tilde(path), key.public_key(), ctx).await {
        let res = handle
            .authenticate_certificate_with(ctx.user, cert, hash, &mut LocalKeySigner(key.clone()))
            .await;
        match res {
            Ok(AuthResult::Success) => return Ok(Step::Done),
            Ok(
                r @ AuthResult::Failure {
                    partial_success: true,
                    ..
                },
            ) => return Ok(step(r)),
            Ok(AuthResult::Failure { .. }) => {
                tracing::info!("server rejected the certificate; trying the plain key");
            }
            Err(e) => {
                tracing::warn!(error = %e, "certificate authentication failed; trying the plain key")
            }
        }
    }
    let res = handle
        .authenticate_publickey(ctx.user, PrivateKeyWithHashAlg::new(key, hash))
        .await?;
    Ok(step(res))
}

/// OpenSSH's naming: `<key>-cert.pub`; for `name.ppk`/`.pem`/`.key` also
/// `name-cert.pub`.
pub(crate) fn certificate_paths(key_path: &Path) -> Vec<PathBuf> {
    let mut appended = key_path.as_os_str().to_owned();
    appended.push("-cert.pub");
    let mut v = vec![PathBuf::from(appended)];
    if let (Some(stem), Some(ext)) = (key_path.file_stem(), key_path.extension())
        && ["ppk", "pem", "key"]
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
    {
        let mut name = stem.to_owned();
        name.push("-cert.pub");
        v.push(key_path.with_file_name(name));
    }
    v
}

/// Why a certificate can't be used with this key right now, if it can't.
pub(crate) fn certificate_problem(cert: &Certificate, key: &PublicKey, now: u64) -> Option<String> {
    if cert.public_key() != key.key_data() {
        return Some("is for a different key".into());
    }
    if now < cert.valid_after() {
        return Some("is not valid yet".into());
    }
    if now >= cert.valid_before() {
        return Some("has expired".into());
    }
    None
}

async fn find_certificate(
    key_path: &Path,
    key: &PublicKey,
    ctx: &AuthCtx<'_>,
) -> Option<Certificate> {
    for p in certificate_paths(key_path) {
        let Ok(text) = tokio::fs::read_to_string(&p).await else {
            continue;
        };
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cert = match Certificate::from_openssh(text.trim()) {
            Ok(c) => c,
            Err(e) => {
                ctx.interaction.notice(&format!(
                    "certificate {name}: unreadable ({e}); using the plain key"
                ));
                continue;
            }
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Some(problem) = certificate_problem(&cert, key, now) {
            ctx.interaction.notice(&format!(
                "certificate {name} {problem}; using the plain key"
            ));
            continue;
        }
        tracing::debug!(cert = %p.display(), key_id = cert.key_id(), "using OpenSSH certificate");
        return Some(cert);
    }
    None
}

async fn try_gssapi(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthCtx<'_>,
    delegate: bool,
) -> Result<Step, SshError> {
    let mut auth = KerberosAuth::prepare(ctx.host, delegate).await?;
    let res = handle
        .authenticate_gssapi_with_mic(ctx.user, vec![gssapi::KRB5_OID_DER.to_vec()], &mut auth)
        .await;
    match res {
        Ok(r) => {
            if let (AuthResult::Failure { .. }, Some(msg)) = (&r, &auth.server_error) {
                tracing::info!(%msg, "server rejected Kerberos");
            }
            Ok(step(r))
        }
        Err(GssAuthError::Send(_)) => Err(SshError::Closed),
        Err(GssAuthError::Gss(m)) => Err(SshError::Gssapi(m)),
    }
}

async fn try_password(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthCtx<'_>,
    credential: Option<&str>,
) -> Result<Step, SshError> {
    let used = SecretUse::Password {
        user: ctx.user,
        host: ctx.host,
    };
    let accepted = |r: &AuthResult| {
        matches!(
            r,
            AuthResult::Success
                | AuthResult::Failure {
                    partial_success: true,
                    ..
                }
        )
    };
    if let Some(saved) = credential.and_then(|c| ctx.credentials.get(c)) {
        let res = handle
            .authenticate_password(ctx.user, saved.expose_secret().to_owned())
            .await?;
        let ok = accepted(&res);
        ctx.interaction.secret_outcome(used, true, ok);
        if ok {
            return Ok(step(res));
        }
        // The saved password is stale (changed on the server): ask once
        // rather than moving on as if password auth were impossible.
        let Some(pass) = ctx.interaction.password(ctx.user, ctx.host).await else {
            return Ok(step(res));
        };
        let res = handle
            .authenticate_password(ctx.user, pass.expose_secret().to_owned())
            .await?;
        ctx.interaction.secret_outcome(used, false, accepted(&res));
        return Ok(step(res));
    }
    let Some(pass) = ctx.interaction.password(ctx.user, ctx.host).await else {
        return Ok(Step::Continue { remaining: None });
    };
    let res = handle
        .authenticate_password(ctx.user, pass.expose_secret().to_owned())
        .await?;
    ctx.interaction.secret_outcome(used, false, accepted(&res));
    Ok(step(res))
}

async fn try_keyboard_interactive(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthCtx<'_>,
) -> Result<Step, SshError> {
    let mut resp = handle
        .authenticate_keyboard_interactive_start(ctx.user, None::<String>)
        .await?;
    // Servers may send several rounds (password, then OTP); cap to avoid loops.
    for _ in 0..8 {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(Step::Done),
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods, ..
            } => {
                return Ok(Step::Continue {
                    remaining: Some(remaining_methods.to_vec()),
                });
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                let answers = if prompts.is_empty() {
                    Vec::new()
                } else {
                    let p: Vec<(String, bool)> =
                        prompts.iter().map(|p| (p.prompt.clone(), p.echo)).collect();
                    match ctx
                        .interaction
                        .keyboard_interactive(&name, &instructions, &p)
                        .await
                    {
                        Some(a) => a,
                        None => return Ok(Step::Continue { remaining: None }),
                    }
                };
                resp = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await?;
            }
        }
    }
    Ok(Step::Continue { remaining: None })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::interaction::{HostKeyDecision, HostKeyInfo, NoCredentials};
    use hostwick_core::SecretString;
    use russh::keys::ssh_key::{certificate, private};

    fn keypair(seed: u8) -> PrivateKey {
        PrivateKey::from(private::Ed25519Keypair::from_seed(&[seed; 32]))
    }

    fn cert_for(key: &PrivateKey, after: u64, before: u64) -> Certificate {
        let mut b = certificate::Builder::new(
            vec![0u8; 32],
            key.public_key().key_data().clone(),
            after,
            before,
        )
        .unwrap();
        b.valid_principal("alice").unwrap();
        b.sign(&keypair(99)).unwrap()
    }

    #[test]
    fn certificate_paths_follow_openssh_naming() {
        assert_eq!(
            certificate_paths(Path::new("/k/id_ed25519")),
            vec![PathBuf::from("/k/id_ed25519-cert.pub")]
        );
        assert_eq!(
            certificate_paths(Path::new("/k/work.ppk")),
            vec![
                PathBuf::from("/k/work.ppk-cert.pub"),
                PathBuf::from("/k/work-cert.pub")
            ]
        );
    }

    #[test]
    fn certificate_must_match_key_and_be_current() {
        let key = keypair(1);
        let cert = cert_for(&key, 1000, 2000);
        assert_eq!(certificate_problem(&cert, key.public_key(), 1500), None);
        assert_eq!(
            certificate_problem(&cert, key.public_key(), 999).as_deref(),
            Some("is not valid yet")
        );
        assert_eq!(
            certificate_problem(&cert, key.public_key(), 2000).as_deref(),
            Some("has expired")
        );
        assert_eq!(
            certificate_problem(&cert, keypair(2).public_key(), 1500).as_deref(),
            Some("is for a different key")
        );
    }

    #[test]
    fn local_signer_appends_signature_blob() {
        let key = Arc::new(keypair(5));
        let blob = sign_local(&key, None, b"data").unwrap();
        // string "ssh-ed25519" + string(64-byte signature)
        assert_eq!(&blob[..4], &11u32.to_be_bytes());
        assert_eq!(&blob[4..15], b"ssh-ed25519");
        assert_eq!(blob.len(), 4 + 11 + 4 + 64);
    }

    struct Quiet;
    #[async_trait::async_trait]
    impl Interaction for Quiet {
        async fn host_key(&self, _: &HostKeyInfo) -> HostKeyDecision {
            HostKeyDecision::Reject
        }
        async fn password(&self, _: &str, _: &str) -> Option<SecretString> {
            None
        }
        async fn key_passphrase(&self, _: &Path) -> Option<SecretString> {
            panic!("must not ask for a passphrase for a security-key file");
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

    #[tokio::test]
    async fn security_key_files_are_refused_with_advice() {
        use russh::keys::ssh_key::{LineEnding, public};
        let pk = private::Ed25519Keypair::from_seed(&[3u8; 32]).public;
        let sk =
            private::SkEd25519::new(public::SkEd25519::new(pk, "ssh:"), 1, vec![9; 32]).unwrap();
        let key = PrivateKey::new(private::KeypairData::SkEd25519(sk), "sk").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519_sk");
        std::fs::write(&path, key.to_openssh(LineEnding::LF).unwrap().as_bytes()).unwrap();
        let ctx = AuthCtx {
            user: "u",
            host: "h",
            interaction: &Quiet,
            credentials: &NoCredentials,
        };
        let err = load_key(&path, None, &ctx).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("add it to ssh-agent / Pageant instead"),
            "{err}"
        );
    }
}
