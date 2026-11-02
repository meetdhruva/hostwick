//! Domain types shared by every Hostwick crate.
//!
//! This crate does no I/O. Adapters (`hostwick-ssh`, `hostwick-store`, ...)
//! depend on it; it depends on nothing of theirs.

mod backoff;
mod profile;
mod target;

pub use backoff::Backoff;
pub use profile::{
    AuthMethod, Endpoint, ForwardSpec, HostProfile, ProfileId, ProfileSource, ProfileStore,
    StoreError,
};
pub use target::{Target, TargetParseError};

/// Re-exported so downstream crates agree on one secret type.
pub use secrecy::{ExposeSecret, SecretString};

/// Default SSH port.
pub const DEFAULT_PORT: u16 = 22;
