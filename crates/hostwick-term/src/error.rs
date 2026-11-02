use std::time::Duration;

/// Why a telnet or serial session could not be opened (or failed later).
#[derive(Debug, thiserror::Error)]
pub enum TermError {
    #[error("cannot connect to {host}:{port}: {reason}")]
    Connect {
        host: String,
        port: u16,
        #[source]
        reason: std::io::Error,
    },
    #[error("timed out after {}s connecting to {host}:{port}", .after.as_secs())]
    Timeout {
        host: String,
        port: u16,
        after: Duration,
    },
    #[error("serial port {port} not found{available}")]
    SerialNotFound {
        port: String,
        /// Pre-formatted, e.g. ` (available: COM1, COM4)`.
        available: String,
    },
    #[error(
        "serial port {0} is in use by another program (close PuTTY, screen, minicom or the other terminal using it)"
    )]
    SerialBusy(String),
    #[error("permission denied opening serial port {port}{hint}")]
    SerialPermission { port: String, hint: String },
    #[error("serial port {port}: {reason}")]
    Serial { port: String, reason: String },
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("session closed")]
    Closed,
}
