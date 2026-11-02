use std::path::{Path, PathBuf};
use std::sync::Arc;

use hostwick_core::{AuthMethod, ExposeSecret};
use russh::MethodKind;
use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};

use crate::handler::ClientHandler;
use crate::interaction::{Credentials, Interaction};
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

    for method in methods {
        let kind = match method {
            AuthMethod::Agent | AuthMethod::KeyFile { .. } => MethodKind::PublicKey,
            AuthMethod::Password { .. } => MethodKind::Password,
            AuthMethod::KeyboardInteractive => MethodKind::KeyboardInteractive,
        };
        if !allowed.is_empty() && !allowed.contains(&kind) {
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
        };
        match result {
            Ok(Step::Done) => return Ok(()),
            Ok(Step::Continue { remaining }) => {
                if let Some(r) = remaining {
                    allowed = r;
                }
            }
            // A broken key file or missing agent shouldn't stop the next method.
            Err(e @ (SshError::Key { .. } | SshError::Agent(_))) => {
                tracing::info!(error = %e, "auth method unavailable, trying next");
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
    for id in identities {
        let key = id.public_key().into_owned();
        let res = handle
            .authenticate_publickey_with(ctx.user, key, hash, &mut agent)
            .await
            .map_err(|e| SshError::Agent(e.to_string()))?;
        last = step(res);
        if matches!(last, Step::Done) {
            return Ok(last);
        }
    }
    Ok(last)
}

fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| path.to_owned()),
        Err(_) => path.to_owned(),
    }
}

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
    if text.contains("PuTTY-User-Key-File") {
        return Err(key_err(
            "PuTTY .ppk keys are not read directly yet; load it into Pageant or convert it with `puttygen key.ppk -O private-openssh -o key`".into(),
        ));
    }
    match russh::keys::decode_secret_key(&text, None) {
        Ok(k) => Ok(k),
        Err(russh::keys::Error::KeyIsEncrypted) => {
            let pass = match passphrase_credential.and_then(|c| ctx.credentials.get(c)) {
                Some(p) => Some(p),
                None => ctx.interaction.key_passphrase(&path).await,
            };
            let pass = pass.ok_or_else(|| key_err("passphrase required".into()))?;
            russh::keys::decode_secret_key(&text, Some(pass.expose_secret()))
                .map_err(|e| key_err(e.to_string()))
        }
        Err(e) => Err(key_err(e.to_string())),
    }
}

async fn try_key_file(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthCtx<'_>,
    path: &Path,
    passphrase_credential: Option<&str>,
) -> Result<Step, SshError> {
    let key = load_key(path, passphrase_credential, ctx).await?;
    let hash = if key.algorithm().is_rsa() {
        rsa_hash(handle).await
    } else {
        None
    };
    let res = handle
        .authenticate_publickey(ctx.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
        .await?;
    Ok(step(res))
}

async fn try_password(
    handle: &mut Handle<ClientHandler>,
    ctx: &AuthCtx<'_>,
    credential: Option<&str>,
) -> Result<Step, SshError> {
    let pass = match credential.and_then(|c| ctx.credentials.get(c)) {
        Some(p) => Some(p),
        None => ctx.interaction.password(ctx.user, ctx.host).await,
    };
    let Some(pass) = pass else {
        return Ok(Step::Continue { remaining: None });
    };
    let res = handle
        .authenticate_password(ctx.user, pass.expose_secret().to_owned())
        .await?;
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
