//! Domain types shared by every Hostwick crate.
//!
//! This crate does no I/O. Adapters (`hostwick-ssh`, `hostwick-store`, ...)
//! depend on it; it depends on nothing of theirs.

mod backoff;
mod options;
mod profile;
mod sessionlog;
mod target;

pub use backoff::Backoff;
pub use options::{
    FileProtocol, FlowControl, HighlightRule, Parity, Protocol, ProxyKind, ProxySpec, ScriptStep,
    SerialSettings, Snippet, SudoOptions, parse_login_script,
};
pub use profile::{
    AuthMethod, Endpoint, ForwardSpec, HostProfile, ProfileId, ProfileSource, ProfileStore,
    StoreError,
};
pub use sessionlog::{LogMode, LogVars, SessionLog, expand_log_path};
pub use target::{Target, TargetParseError};

/// Re-exported so downstream crates agree on one secret type.
pub use secrecy::{ExposeSecret, SecretString};

/// Default SSH port.
pub const DEFAULT_PORT: u16 = 22;
