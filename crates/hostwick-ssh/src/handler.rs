use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use russh::Channel;
use russh::client::{self, ChannelOpenHandle, Msg, Session};
use russh::keys::{PublicKey, PublicKeyOrCertificate};

use crate::interaction::{HostKeyDecision, HostKeyInfo, HostKeyStatus, Interaction};
use crate::known_hosts::{KnownHosts, fingerprint};
use crate::x11::X11Forwarding;

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
    /// Set when we asked for X11 forwarding on this connection; X11 channels
    /// are refused otherwise.
    pub x11: Option<Arc<X11Forwarding>>,
    /// Whether we asked for agent forwarding. A server opening agent
    /// channels we never requested is refused: it would get our keys.
    pub agent_forwarding: bool,
    /// "Local agent unavailable" is reported once per connection.
    pub agent_warned: Arc<AtomicBool>,
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

    async fn server_channel_open_x11(
        &mut self,
        channel: Channel<Msg>,
        originator_address: &str,
        originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let Some(x11) = self.x11.clone() else {
            tracing::warn!(
                originator_address,
                originator_port,
                "server opened an X11 channel we did not ask for; rejecting"
            );
            drop(reply);
            return Ok(());
        };
        tracing::debug!(originator_address, originator_port, "X11 connection");
        // Connecting to the local display happens in its own task so a slow
        // X server never stalls the session's event loop.
        tokio::spawn(x11.serve(channel, reply));
        Ok(())
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.agent_forwarding {
            tracing::warn!("server opened an agent channel we did not ask for; rejecting");
            drop(reply);
            return Ok(());
        }
        let interaction = self.interaction.clone();
        let warned = self.agent_warned.clone();
        tokio::spawn(async move {
            let mut local = match crate::agent::connect_stream().await {
                Ok(s) => s,
                Err(e) => {
                    if !warned.swap(true, Ordering::SeqCst) {
                        interaction.notice(&format!("agent forwarding: {e}"));
                    }
                    reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
                    return;
                }
            };
            reply.accept().await;
            let mut remote = channel.into_stream();
            match tokio::io::copy_bidirectional(&mut remote, &mut local).await {
                Ok(_) => tracing::debug!("forwarded agent connection closed"),
                Err(e) => tracing::debug!(error = %e, "forwarded agent connection ended"),
            }
        });
        Ok(())
    }
}
