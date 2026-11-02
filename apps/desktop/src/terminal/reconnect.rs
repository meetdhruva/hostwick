//! Auto-reconnect: when to try again, and whether a session end counts as a
//! drop (worth reconnecting) or a deliberate end.

use std::time::Duration;

use hostwick_term::TransportKind;

/// Wait before attempt `attempt` (0-based): 1 s, 2 s, 5 s, 10 s, then every
/// 30 s.
pub fn delay(attempt: u32) -> Duration {
    Duration::from_secs(match attempt {
        0 => 1,
        1 => 2,
        2 => 5,
        3 => 10,
        _ => 30,
    })
}

/// How a session ended, as seen by the output parser.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndSeen {
    /// The server reported an exit status or signal (`exit`, logout).
    pub exit_status: bool,
    /// The transport reported a failure (reset, timeout, unplugged).
    pub failure: bool,
}

/// Whether an ended session was dropped rather than ended on purpose.
/// SSH: no exit status means the link went away. Telnet has no exit status:
/// a clean close is a logout, only a transport failure is a drop. Serial
/// ports are not reconnected automatically.
pub fn is_drop(kind: TransportKind, end: EndSeen) -> bool {
    match kind {
        TransportKind::Ssh => !end.exit_status,
        TransportKind::Telnet => end.failure,
        TransportKind::Serial => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{EndSeen, delay, is_drop};
    use hostwick_term::TransportKind;

    #[test]
    fn schedule_backs_off_to_thirty_seconds() {
        let secs: Vec<u64> = (0..7).map(|a| delay(a).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 5, 10, 30, 30, 30]);
    }

    #[test]
    fn only_real_drops_reconnect() {
        let exit = EndSeen {
            exit_status: true,
            failure: false,
        };
        let lost = EndSeen::default();
        let failed = EndSeen {
            exit_status: false,
            failure: true,
        };
        assert!(!is_drop(TransportKind::Ssh, exit));
        assert!(is_drop(TransportKind::Ssh, lost));
        assert!(!is_drop(TransportKind::Telnet, lost));
        assert!(is_drop(TransportKind::Telnet, failed));
        assert!(!is_drop(TransportKind::Serial, failed));
    }
}
