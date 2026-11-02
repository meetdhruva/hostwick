//! Kerberos single sign-on (`gssapi-with-mic`, RFC 4462).
//!
//! The platform security library does the Kerberos work: the system GSS-API
//! library (MIT/Heimdal on Linux and BSD, `GSS.framework` on macOS), loaded
//! at run time so Hostwick still starts on machines without it, or SSPI on
//! Windows (the domain login). This module adapts it to russh's
//! [`russh::GssapiAuthenticator`].
//!
//! Before talking to the server we create the security context and its
//! first token. That step needs a ticket and the server's principal, so a
//! missing library or `kinit` fails right there as "method unavailable" and
//! the auth chain moves on without having sent anything.

use std::time::Duration;

use russh::{GssapiAuthenticator, GssapiError, GssapiStep};

use crate::SshError;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

/// DER-encoded Kerberos V5 mechanism OID 1.2.840.113554.1.2.2, as sent in
/// `gssapi-with-mic` requests.
pub(crate) const KRB5_OID_DER: &[u8] = &[
    0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x12, 0x01, 0x02, 0x02,
];

/// A Kerberos exchange can involve a KDC round trip per step.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// Output of one `init_sec_context` call.
#[derive(Debug, Default)]
pub(crate) struct Token {
    pub token: Vec<u8>,
    pub complete: bool,
    /// The context can produce a MIC (integrity flag granted).
    pub integrity: bool,
}

/// Kerberos is unavailable or failed; the message is for the person.
#[derive(Debug)]
pub(crate) struct GssFailure(pub String);

/// The platform security context. Blocking calls; run them off the runtime.
pub(crate) trait SecurityContext: Send + Sized + 'static {
    /// Create a context for `host@<host>` and produce the first token.
    fn initiate(host: &str, delegate: bool) -> Result<(Self, Token), GssFailure>;
    fn step(&mut self, input: &[u8]) -> Result<Token, GssFailure>;
    fn mic(&mut self, data: &[u8]) -> Result<Vec<u8>, GssFailure>;
}

/// Errors from inside the russh exchange.
#[derive(Debug, thiserror::Error)]
pub(crate) enum GssAuthError {
    #[error("connection closed during Kerberos authentication")]
    Send(#[from] russh::SendError),
    #[error("{0}")]
    Gss(String),
}

pub(crate) struct KerberosAuth<C: SecurityContext = platform::Context> {
    ctx: Option<C>,
    first: Option<Token>,
    /// Last error the server reported, for the "rejected" message.
    pub server_error: Option<String>,
}

impl KerberosAuth<platform::Context> {
    /// Prepare authentication to `host` (the name as typed; the Kerberos
    /// library canonicalizes it per krb5.conf). Fails with
    /// [`SshError::Gssapi`] when there is no library, ticket or principal.
    pub(crate) async fn prepare(host: &str, delegate: bool) -> Result<Self, SshError> {
        Self::prepare_with(host, delegate).await
    }
}

impl<C: SecurityContext> KerberosAuth<C> {
    pub(crate) async fn prepare_with(host: &str, delegate: bool) -> Result<Self, SshError> {
        let host = host.to_owned();
        let (ctx, first) = blocking(move || C::initiate(&host, delegate))
            .await
            .map_err(|e| SshError::Gssapi(e.0))?;
        Ok(Self {
            ctx: Some(ctx),
            first: Some(first),
            server_error: None,
        })
    }

    async fn run<T: Send + 'static>(
        &mut self,
        f: impl FnOnce(&mut C) -> Result<T, GssFailure> + Send + 'static,
    ) -> Result<T, GssAuthError> {
        let mut ctx = self
            .ctx
            .take()
            .ok_or_else(|| GssAuthError::Gss("Kerberos context already failed".into()))?;
        let (ctx, res) = blocking(move || {
            let r = f(&mut ctx);
            Ok((ctx, r))
        })
        .await
        .map_err(|e| GssAuthError::Gss(e.0))?;
        self.ctx = Some(ctx);
        res.map_err(|e| GssAuthError::Gss(e.0))
    }

    async fn finish(&mut self, t: Token, mic_data: Vec<u8>) -> Result<GssapiStep, GssAuthError> {
        let token = (!t.token.is_empty()).then_some(t.token);
        if !t.complete {
            return Ok(GssapiStep::Continue {
                token: token.unwrap_or_default(),
            });
        }
        let mic = if t.integrity {
            Some(self.run(move |c| c.mic(&mic_data)).await?)
        } else {
            None
        };
        Ok(GssapiStep::Complete { token, mic })
    }
}

/// Run a blocking security-library call on the blocking pool, bounded by
/// [`STEP_TIMEOUT`] (a dead KDC must not hang the connection).
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, GssFailure> + Send + 'static,
) -> Result<T, GssFailure> {
    match tokio::time::timeout(STEP_TIMEOUT, tokio::task::spawn_blocking(f)).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => Err(GssFailure(format!("internal error: {e}"))),
        Err(_) => Err(GssFailure(format!(
            "no answer from the Kerberos library within {}s (is the KDC reachable?)",
            STEP_TIMEOUT.as_secs()
        ))),
    }
}

impl<C: SecurityContext> GssapiAuthenticator for KerberosAuth<C> {
    type Error = GssAuthError;

    async fn gssapi_step(
        &mut self,
        selected_mechanism: Option<Vec<u8>>,
        input_token: Option<Vec<u8>>,
        mic_data: Vec<u8>,
    ) -> Result<GssapiStep, Self::Error> {
        if let Some(mech) = &selected_mechanism
            && mech != KRB5_OID_DER
        {
            return Err(GssAuthError::Gss(
                "server picked a GSS-API mechanism we did not offer".into(),
            ));
        }
        let t = match (self.first.take(), input_token) {
            // Reply to SSH_MSG_USERAUTH_GSSAPI_RESPONSE: our first token.
            (Some(first), None) => first,
            (_, Some(input)) => self.run(move |c| c.step(&input)).await?,
            (None, None) => {
                return Err(GssAuthError::Gss(
                    "server sent no token to continue the Kerberos exchange".into(),
                ));
            }
        };
        self.finish(t, mic_data).await
    }

    async fn gssapi_error(&mut self, error: GssapiError) {
        if let GssapiError::Status { message, .. } = error {
            tracing::info!(%message, "server reported a GSS-API error");
            self.server_error = Some(message);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// Two-round fake mechanism: first token "a", then expects "b" and
    /// completes with "c"; the MIC is the data reversed.
    struct Fake {
        round: u8,
    }

    impl SecurityContext for Fake {
        fn initiate(host: &str, _delegate: bool) -> Result<(Self, Token), GssFailure> {
            if host == "noticket" {
                return Err(GssFailure("no Kerberos ticket — run kinit".into()));
            }
            Ok((
                Self { round: 0 },
                Token {
                    token: b"a".to_vec(),
                    complete: false,
                    integrity: true,
                },
            ))
        }
        fn step(&mut self, input: &[u8]) -> Result<Token, GssFailure> {
            self.round += 1;
            if input != b"b" {
                return Err(GssFailure("bad token".into()));
            }
            Ok(Token {
                token: b"c".to_vec(),
                complete: true,
                integrity: true,
            })
        }
        fn mic(&mut self, data: &[u8]) -> Result<Vec<u8>, GssFailure> {
            Ok(data.iter().rev().copied().collect())
        }
    }

    #[tokio::test]
    async fn exchange_produces_tokens_and_mic() {
        let mut a = KerberosAuth::<Fake>::prepare_with("h", false)
            .await
            .unwrap();
        let s = a
            .gssapi_step(Some(KRB5_OID_DER.to_vec()), None, b"xy".to_vec())
            .await
            .unwrap();
        assert_eq!(
            s,
            GssapiStep::Continue {
                token: b"a".to_vec()
            }
        );
        let s = a
            .gssapi_step(None, Some(b"b".to_vec()), b"xy".to_vec())
            .await
            .unwrap();
        assert_eq!(
            s,
            GssapiStep::Complete {
                token: Some(b"c".to_vec()),
                mic: Some(b"yx".to_vec())
            }
        );
    }

    #[tokio::test]
    async fn missing_ticket_is_unavailable_and_wrong_mech_rejected() {
        let e = KerberosAuth::<Fake>::prepare_with("noticket", false)
            .await
            .err()
            .unwrap();
        assert!(
            matches!(e, SshError::Gssapi(ref m) if m.contains("kinit")),
            "{e}"
        );

        let mut a = KerberosAuth::<Fake>::prepare_with("h", false)
            .await
            .unwrap();
        let e = a
            .gssapi_step(Some(vec![6, 1, 0]), None, Vec::new())
            .await
            .unwrap_err();
        assert!(e.to_string().contains("mechanism"));
    }
}
