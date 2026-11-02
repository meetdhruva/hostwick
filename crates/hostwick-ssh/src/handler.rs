use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use russh::Channel;
use russh::client::{self, ChannelOpenHandle, Msg, Session};
use russh::keys::{PublicKey, PublicKeyOrCertificate};

use crate::interaction::{HostKeyDecision, HostKeyInfo, HostKeyStatus, Interaction};
use crate::known_hosts::{KnownHosts, fingerprint};

/// Where the server-side end of a `-R` forward should be delivered locally.
pub(crate) type RemoteForwardMap = Arc<Mutex<HashMap<u32, (String, u16)>>>;

/// Outcome of host-key checking, read back by `Connection::connect` to turn
/// russh's generic "unknown key" error into a clear message.
#[derive(Debug, Default)]
pub(crate) struct VerifyOutcome {
    pub rejected: bool,
}

pub(crate) struct ClientHandler {
    pub host: String,
    pub port: u16,
    pub known_hosts: Option<KnownHosts>,
    pub interaction: Arc<dyn Interaction>,
    pub outcome: Arc<Mutex<VerifyOutcome>>,
    pub remote_forwards: RemoteForwardMap,
}

impl ClientHandler {
    fn reject(&self) -> Result<bool, russh::Error> {
        if let Ok(mut o) = self.outcome.lock() {
            o.rejected = true;
        }
        Ok(false)
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.interaction.banner(banner);
        Ok(())
    }

    async fn check_server_key(
        &mut self,
        presented: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // We never advertise certificate host-key algorithms, so a certificate
        // here is unexpected; verify its underlying key like a plain key.
        let key: PublicKey = match presented {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::from(cert.public_key().clone()),
        };

        let status = match &self.known_hosts {
            Some(kh) => match kh.check(&self.host, self.port, &key) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "known_hosts unreadable; treating key as unknown");
                    HostKeyStatus::Unknown
                }
            },
            None => HostKeyStatus::Unknown,
        };
        if status == HostKeyStatus::Known {
            return Ok(true);
        }

        let info = HostKeyInfo {
            host: self.host.clone(),
            port: self.port,
            algorithm: key.algorithm().to_string(),
            fingerprint: fingerprint(&key),
            status: status.clone(),
        };
        match self.interaction.host_key(&info).await {
            HostKeyDecision::Reject => self.reject(),
            HostKeyDecision::AcceptOnce => Ok(true),
            HostKeyDecision::AcceptAndRemember => {
                if let Some(kh) = &self.known_hosts {
                    let saved = match status {
                        HostKeyStatus::Changed { line } => {
                            kh.replace(line, &self.host, self.port, &key)
                        }
                        _ => kh.learn(&self.host, self.port, &key),
                    };
                    if let Err(e) = saved {
                        tracing::warn!(error = %e, "could not save host key; trusting for this session only");
                    }
                }
                Ok(true)
            }
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        originator_address: &str,
        originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let target = self
            .remote_forwards
            .lock()
            .ok()
            .and_then(|m| m.get(&connected_port).cloned());
        let Some((host, port)) = target else {
            tracing::warn!(
                connected_address,
                connected_port,
                "unexpected forwarded connection; rejecting"
            );
            drop(reply); // dropping rejects
            return Ok(());
        };
        reply.accept().await;
        tracing::debug!(%originator_address, originator_port, %host, port, "remote forward connection");
        tokio::spawn(async move {
            match tokio::net::TcpStream::connect((host.as_str(), port)).await {
                Ok(mut tcp) => {
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
                }
                Err(e) => {
                    tracing::warn!(%host, port, error = %e, "remote forward: local target unreachable")
                }
            }
        });
        Ok(())
    }
}
