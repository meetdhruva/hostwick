//! SSH layer for Hostwick.
//!
//! Design: russh already runs each SSH session as its own task and hands out a
//! cloneable [`russh::client::Handle`] that talks to it over channels — an
//! actor. [`Connection`] wraps that handle with Hostwick's policies (host-key
//! verification, the auth chain, jump hosts, forwards, keepalive) and is cheap
//! to clone, so a terminal tab, an SFTP pane and a tunnel can all share one
//! authenticated link.

mod agent;
mod auth;
mod connection;
mod error;
mod exec;
mod exec_stream;
mod forward;
mod gssapi;
mod handler;
mod interaction;
mod known_hosts;
mod pool;
mod proxy;
mod shell;
mod x11;

pub use agent::agent_key_names;
pub use connection::{ConnectContext, Connection, connect_via_proxy};
pub use error::SshError;
pub use exec::ExecOutput;
pub use exec_stream::{ExecStream, ExitStatus, wait_exit};
pub use forward::ForwardHandle;
pub use interaction::{
    Credentials, HostKeyDecision, HostKeyInfo, HostKeyStatus, Interaction, NoCredentials, SecretUse,
};
pub use known_hosts::{KnownHostEntry, KnownHostMarker, KnownHosts};
pub use pool::ConnectionPool;
pub use shell::{PtyOptions, ShellCommand, ShellEvent, ShellSession};
pub use x11::{X11Display, X11Target};

/// Re-exported for crates that open raw channels (e.g. SFTP).
pub use russh::{ChannelStream, client::Msg as ClientMsg};
