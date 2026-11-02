use std::fmt;

use crate::DEFAULT_PORT;

/// A connection target typed by the user: `host`, `user@host`, `host:port`,
/// `user@host:port`, `[v6::addr]:port`, or a saved profile name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TargetParseError {
    #[error("target is empty")]
    Empty,
    #[error("invalid port in target: {0}")]
    BadPort(String),
    #[error("unterminated IPv6 bracket in target")]
    UnterminatedBracket,
}

impl Target {
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }

    pub fn parse(input: &str) -> Result<Self, TargetParseError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(TargetParseError::Empty);
        }
        let (user, rest) = match input.rsplit_once('@') {
            Some((u, r)) if !u.is_empty() => (Some(u.to_owned()), r),
            Some((_, r)) => (None, r),
            None => (None, input),
        };
        if rest.is_empty() {
            return Err(TargetParseError::Empty);
        }

        let (host, port) = if let Some(stripped) = rest.strip_prefix('[') {
            let (h, after) = stripped
                .split_once(']')
                .ok_or(TargetParseError::UnterminatedBracket)?;
            let port = match after.strip_prefix(':') {
                Some(p) => Some(parse_port(p)?),
                None if after.is_empty() => None,
                None => return Err(TargetParseError::BadPort(after.to_owned())),
            };
            (h.to_owned(), port)
        } else if rest.matches(':').count() == 1 {
            let (h, p) = rest.split_once(':').unwrap_or((rest, ""));
            (h.to_owned(), Some(parse_port(p)?))
        } else {
            // Zero colons, or a bare IPv6 address without brackets.
            (rest.to_owned(), None)
        };
        if host.is_empty() {
            return Err(TargetParseError::Empty);
        }
        Ok(Self { user, host, port })
    }
}

fn parse_port(p: &str) -> Result<u16, TargetParseError> {
    p.parse::<u16>()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| TargetParseError::BadPort(p.to_owned()))
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(u) = &self.user {
            write!(f, "{u}@")?;
        }
        if self.host.contains(':') {
            write!(f, "[{}]", self.host)?;
        } else {
            write!(f, "{}", self.host)?;
        }
        if let Some(p) = self.port {
            write!(f, ":{p}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(user: Option<&str>, host: &str, port: Option<u16>) -> Target {
        Target {
            user: user.map(str::to_owned),
            host: host.to_owned(),
            port,
        }
    }

    #[test]
    fn parses_common_forms() {
        assert_eq!(
            Target::parse("example.com"),
            Ok(t(None, "example.com", None))
        );
        assert_eq!(
            Target::parse("root@10.0.0.1"),
            Ok(t(Some("root"), "10.0.0.1", None))
        );
        assert_eq!(Target::parse("box:2222"), Ok(t(None, "box", Some(2222))));
        assert_eq!(
            Target::parse("ops@db.internal:2200"),
            Ok(t(Some("ops"), "db.internal", Some(2200)))
        );
        assert_eq!(Target::parse("[::1]:22"), Ok(t(None, "::1", Some(22))));
        assert_eq!(Target::parse("fe80::1"), Ok(t(None, "fe80::1", None)));
        assert_eq!(
            Target::parse("first.last@corp@bastion"),
            Ok(t(Some("first.last@corp"), "bastion", None))
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Target::parse(""), Err(TargetParseError::Empty));
        assert_eq!(Target::parse("user@"), Err(TargetParseError::Empty));
        assert!(matches!(
            Target::parse("h:0"),
            Err(TargetParseError::BadPort(_))
        ));
        assert!(matches!(
            Target::parse("h:99999"),
            Err(TargetParseError::BadPort(_))
        ));
        assert_eq!(
            Target::parse("[::1"),
            Err(TargetParseError::UnterminatedBracket)
        );
    }

    #[test]
    fn display_round_trips() {
        for s in ["a@b:23", "b", "[::1]:22", "x@[fe80::2]"] {
            assert_eq!(Target::parse(s).map(|t| t.to_string()).as_deref(), Ok(s));
        }
    }
}
