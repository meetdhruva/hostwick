use std::net::SocketAddr;

use hostwick_core::ForwardSpec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::{Connection, SshError};

/// A running port forward. Dropping it stops the forward (open connections
/// through it finish on their own).
#[derive(Debug)]
pub struct ForwardHandle {
    spec: ForwardSpec,
    /// Local listen address for `-L`/`-D`; for `-R`, the server-side port.
    bound: Option<SocketAddr>,
    remote_port: Option<u32>,
    cancel: CancellationToken,
}

impl ForwardHandle {
    pub fn spec(&self) -> &ForwardSpec {
        &self.spec
    }
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.bound
    }
    pub fn remote_port(&self) -> Option<u32> {
        self.remote_port
    }
    pub fn stop(&self) {
        self.cancel.cancel();
    }
}

impl Drop for ForwardHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn fwd_err(spec: &ForwardSpec, reason: impl ToString) -> SshError {
    SshError::Forward {
        spec: spec.to_string(),
        reason: reason.to_string(),
    }
}

pub(crate) async fn start(conn: Connection, spec: ForwardSpec) -> Result<ForwardHandle, SshError> {
    let cancel = CancellationToken::new();
    match &spec {
        ForwardSpec::Local {
            bind_addr,
            bind_port,
            host,
            host_port,
        } => {
            let listener = TcpListener::bind((bind_addr.as_str(), *bind_port))
                .await
                .map_err(|e| fwd_err(&spec, e))?;
            let bound = listener.local_addr().ok();
            let (host, port) = (host.clone(), *host_port);
            spawn_accept_loop(listener, cancel.clone(), move |tcp, _peer| {
                let conn = conn.clone();
                let host = host.clone();
                async move { pipe_through(conn, tcp, &host, port).await }
            });
            Ok(ForwardHandle {
                spec,
                bound,
                remote_port: None,
                cancel,
            })
        }
        ForwardSpec::Dynamic {
            bind_addr,
            bind_port,
        } => {
            let listener = TcpListener::bind((bind_addr.as_str(), *bind_port))
                .await
                .map_err(|e| fwd_err(&spec, e))?;
            let bound = listener.local_addr().ok();
            spawn_accept_loop(listener, cancel.clone(), move |tcp, _peer| {
                let conn = conn.clone();
                async move { socks5(conn, tcp).await }
            });
            Ok(ForwardHandle {
                spec,
                bound,
                remote_port: None,
                cancel,
            })
        }
        ForwardSpec::Remote {
            bind_addr,
            bind_port,
            host,
            host_port,
        } => {
            let requested = u32::from(*bind_port);
            let assigned = conn
                .handle()
                .tcpip_forward(bind_addr.clone(), requested)
                .await
                .map_err(|e| fwd_err(&spec, e))?;
            // Servers return 0 when they bound exactly what we asked for.
            let port = if assigned == 0 { requested } else { assigned };
            if let Ok(mut m) = conn.remote_forwards().lock() {
                m.insert(port, (host.clone(), *host_port));
            }
            let token = cancel.clone();
            let bind_addr = bind_addr.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                if let Ok(mut m) = conn.remote_forwards().lock() {
                    m.remove(&port);
                }
                let _ = conn.handle().cancel_tcpip_forward(bind_addr, port).await;
            });
            Ok(ForwardHandle {
                spec,
                bound: None,
                remote_port: Some(port),
                cancel,
            })
        }
    }
}

fn spawn_accept_loop<F, Fut>(listener: TcpListener, cancel: CancellationToken, on_conn: F)
where
    F: Fn(TcpStream, SocketAddr) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), SshError>> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                accepted = listener.accept() => match accepted {
                    Ok((tcp, peer)) => {
                        let _ = tcp.set_nodelay(true);
                        let fut = on_conn(tcp, peer);
                        tokio::spawn(async move {
                            if let Err(e) = fut.await {
                                tracing::debug!(%peer, error = %e, "forwarded connection ended with error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
        }
    });
}

async fn pipe_through(
    conn: Connection,
    mut tcp: TcpStream,
    host: &str,
    port: u16,
) -> Result<(), SshError> {
    let ch = conn.open_direct_tcpip(host, port).await?;
    let mut stream = ch.into_stream();
    tokio::io::copy_bidirectional(&mut tcp, &mut stream).await?;
    Ok(())
}

/// Minimal SOCKS5 (RFC 1928): no-auth, CONNECT, IPv4/IPv6/domain.
/// Domain names are resolved by the SSH server, like `ssh -D`.
async fn socks5(conn: Connection, mut tcp: TcpStream) -> Result<(), SshError> {
    let mut head = [0u8; 2];
    tcp.read_exact(&mut head).await?;
    if head[0] != 5 {
        return Err(SshError::Forward {
            spec: "SOCKS".into(),
            reason: "only SOCKS5 is supported".into(),
        });
    }
    let mut methods = vec![0u8; usize::from(head[1])];
    tcp.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        tcp.write_all(&[5, 0xFF]).await?;
        return Ok(());
    }
    tcp.write_all(&[5, 0]).await?;

    let mut req = [0u8; 4];
    tcp.read_exact(&mut req).await?;
    let [ver, cmd, _rsv, atyp] = req;
    let host = match atyp {
        1 => {
            let mut a = [0u8; 4];
            tcp.read_exact(&mut a).await?;
            std::net::Ipv4Addr::from(a).to_string()
        }
        4 => {
            let mut a = [0u8; 16];
            tcp.read_exact(&mut a).await?;
            std::net::Ipv6Addr::from(a).to_string()
        }
        3 => {
            let len = tcp.read_u8().await?;
            let mut name = vec![0u8; usize::from(len)];
            tcp.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        _ => {
            reply(&mut tcp, 8).await?; // address type not supported
            return Ok(());
        }
    };
    let port = tcp.read_u16().await?;
    if ver != 5 || cmd != 1 {
        reply(&mut tcp, 7).await?; // command not supported
        return Ok(());
    }
    match conn.open_direct_tcpip(&host, port).await {
        Ok(ch) => {
            reply(&mut tcp, 0).await?;
            let mut stream = ch.into_stream();
            tokio::io::copy_bidirectional(&mut tcp, &mut stream).await?;
            Ok(())
        }
        Err(e) => {
            reply(&mut tcp, 5).await?; // connection refused
            Err(e)
        }
    }
}

async fn reply(tcp: &mut TcpStream, code: u8) -> std::io::Result<()> {
    tcp.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}
