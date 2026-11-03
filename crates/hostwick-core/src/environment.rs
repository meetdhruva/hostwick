//! What a host is used for, so the terminal can show it: a coloured stripe
//! on the tab, a ribbon over the terminal and (for production) a warning
//! before a multi-line paste.

use serde::{Deserialize, Serialize};

/// The kind of host. `Custom` carries a `#rrggbb` colour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "color", rename_all = "snake_case")]
pub enum Environment {
    #[default]
    None,
    Dev,
    Staging,
    Prod,
    /// Own colour, `#rrggbb`.
    Custom(String),
}

impl Environment {
    /// The stripe colour as RGB; `None` for no environment (or an
    /// unreadable custom colour).
    pub fn rgb(&self) -> Option<(u8, u8, u8)> {
        match self {
            Self::None => None,
            Self::Dev => Some((0x2e, 0xa0, 0x43)),
            Self::Staging => Some((0xd9, 0x8e, 0x04)),
            Self::Prod => Some((0xd0, 0x2f, 0x2f)),
            Self::Custom(c) => parse_hex_color(c),
        }
    }

    pub fn is_prod(&self) -> bool {
        matches!(self, Self::Prod)
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// `#rgb` or `#rrggbb` (the `#` is optional).
pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some((byte(0)?, byte(2)?, byte(4)?)),
        3 => {
            let n = |i: usize| {
                u8::from_str_radix(h.get(i..i + 1)?, 16)
                    .ok()
                    .map(|v| v * 17)
            };
            Some((n(0)?, n(1)?, n(2)?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(Environment::None.rgb(), None);
        assert!(Environment::Prod.is_prod());
        assert_eq!(
            Environment::Custom("#ff8000".into()).rgb(),
            Some((255, 128, 0))
        );
        assert_eq!(Environment::Custom("0af".into()).rgb(), Some((0, 170, 255)));
        assert_eq!(Environment::Custom("nope".into()).rgb(), None);
        assert_eq!(Environment::Custom("#ééé".into()).rgb(), None);
    }

    #[test]
    fn serde_round_trip_and_default() {
        for e in [
            Environment::None,
            Environment::Dev,
            Environment::Prod,
            Environment::Custom("#123456".into()),
        ] {
            let j = serde_json::to_string(&e).unwrap_or_default();
            assert_eq!(serde_json::from_str::<Environment>(&j).ok(), Some(e));
        }
    }
}
