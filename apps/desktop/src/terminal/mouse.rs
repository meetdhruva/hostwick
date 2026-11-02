//! Mouse reporting (xterm protocol) for programs that ask for it: htop, mc,
//! vim with `mouse=a`, tmux.
//!
//! Which events are sent depends on the mode the program set (click only,
//! drag, or all motion); how they are encoded depends on the encoding it
//! asked for: SGR (`CSI < b ; x ; y M/m`, any size), UTF-8, or the legacy
//! X10 bytes (columns/rows up to 223).

use alacritty_terminal::term::TermMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Press,
    Release,
    /// Motion with `Some(button)` held, or with no button (`None`).
    Motion,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Should this event be reported at all, given the terminal's modes?
pub fn wants(mode: TermMode, kind: Kind, button: Option<Button>) -> bool {
    match kind {
        Kind::Press | Kind::Release => mode.intersects(TermMode::MOUSE_MODE),
        Kind::Motion if button.is_some() => {
            mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
        }
        Kind::Motion => mode.contains(TermMode::MOUSE_MOTION),
    }
}

/// Encode one event. `col`/`row` are 0-based cell coordinates. Returns None
/// when the legacy encoding cannot express the position.
pub fn encode(
    mode: TermMode,
    kind: Kind,
    button: Option<Button>,
    mods: MouseMods,
    col: usize,
    row: usize,
) -> Option<Vec<u8>> {
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let mut code: u32 = match button {
        Some(Button::Left) => 0,
        Some(Button::Middle) => 1,
        Some(Button::Right) => 2,
        Some(Button::WheelUp) => 64,
        Some(Button::WheelDown) => 65,
        Some(Button::WheelLeft) => 66,
        Some(Button::WheelRight) => 67,
        // Motion with no button.
        None => 3,
    };
    // Legacy encodings have no per-button release: it is "button 3".
    if kind == Kind::Release && !sgr {
        code = 3;
    }
    if kind == Kind::Motion {
        code += 32;
    }
    if mods.shift {
        code += 4;
    }
    if mods.alt {
        code += 8;
    }
    if mods.ctrl {
        code += 16;
    }
    let (x, y) = (col + 1, row + 1);
    if sgr {
        let fin = if kind == Kind::Release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{fin}").into_bytes());
    }
    let mut out = b"\x1b[M".to_vec();
    if mode.contains(TermMode::UTF8_MOUSE) {
        for v in [code as usize + 32, x + 32, y + 32] {
            let c = char::from_u32(u32::try_from(v).ok()?)?;
            if v > 2047 {
                return None;
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    } else {
        for v in [code as usize + 32, x + 32, y + 32] {
            out.push(u8::try_from(v).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: MouseMods = MouseMods {
        shift: false,
        alt: false,
        ctrl: false,
    };

    #[test]
    fn sgr_encoding() {
        let m = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            encode(m, Kind::Press, Some(Button::Left), NONE, 0, 0),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        assert_eq!(
            encode(m, Kind::Release, Some(Button::Right), NONE, 9, 4),
            Some(b"\x1b[<2;10;5m".to_vec())
        );
        let ctrl_alt = MouseMods {
            ctrl: true,
            alt: true,
            shift: false,
        };
        assert_eq!(
            encode(m, Kind::Press, Some(Button::WheelDown), ctrl_alt, 299, 99),
            Some(b"\x1b[<89;300;100M".to_vec())
        );
        assert_eq!(
            encode(m, Kind::Motion, Some(Button::Left), NONE, 1, 1),
            Some(b"\x1b[<32;2;2M".to_vec())
        );
        assert_eq!(
            encode(m, Kind::Motion, None, NONE, 1, 1),
            Some(b"\x1b[<35;2;2M".to_vec())
        );
    }

    #[test]
    fn legacy_and_utf8_encoding() {
        let m = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            encode(m, Kind::Press, Some(Button::Left), NONE, 0, 0),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        assert_eq!(
            encode(m, Kind::Release, Some(Button::Left), NONE, 2, 3),
            Some(vec![0x1b, b'[', b'M', 35, 35, 36])
        );
        // Beyond column 223 the legacy encoding cannot report.
        assert_eq!(
            encode(m, Kind::Press, Some(Button::Left), NONE, 230, 0),
            None
        );
        let u = m | TermMode::UTF8_MOUSE;
        let e = encode(u, Kind::Press, Some(Button::Left), NONE, 230, 0).unwrap_or_default();
        assert_eq!(String::from_utf8_lossy(&e), "\x1b[M ć!");
    }

    #[test]
    fn which_events_are_wanted() {
        let click = TermMode::MOUSE_REPORT_CLICK;
        let drag = TermMode::MOUSE_DRAG;
        let all = TermMode::MOUSE_MOTION;
        assert!(wants(click, Kind::Press, Some(Button::Left)));
        assert!(!wants(click, Kind::Motion, Some(Button::Left)));
        assert!(wants(drag, Kind::Motion, Some(Button::Left)));
        assert!(!wants(drag, Kind::Motion, None));
        assert!(wants(all, Kind::Motion, None));
        assert!(!wants(TermMode::empty(), Kind::Press, Some(Button::Left)));
    }
}
