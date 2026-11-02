//! Keystroke → bytes, following xterm conventions.
//!
//! Only keys that are *not* plain text are encoded here (Enter, arrows,
//! function keys, Ctrl/Alt combinations). Printable text — including IME
//! composition — arrives through the platform input handler instead, so input
//! methods for Chinese, Japanese, Korean etc. work.

/// Modifier state, decoupled from GPUI so the encoder is unit-testable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Cmd on macOS, Win/Super elsewhere: never sent to the remote.
    pub platform: bool,
}

/// xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
fn mod_param(m: Mods) -> u8 {
    1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl)
}

fn has_mods(m: Mods) -> bool {
    m.shift || m.alt || m.ctrl
}

/// Whether Alt acts as Meta (ESC prefix). On macOS Option composes
/// characters (ß, é), so it is left to the input handler there.
const ALT_IS_META: bool = !cfg!(target_os = "macos");

/// Encode `key` (GPUI key name, e.g. "a", "enter", "up", "f5") with `mods`.
/// `app_cursor` is DECCKM (application cursor keys), set by full-screen apps.
pub fn encode(key: &str, mods: Mods, app_cursor: bool) -> Option<Vec<u8>> {
    if mods.platform {
        return None;
    }
    let csi_final = |c: char| -> Vec<u8> {
        if has_mods(mods) {
            format!("\x1b[1;{}{c}", mod_param(mods)).into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if has_mods(mods) {
            format!("\x1b[{n};{}~", mod_param(mods)).into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    let meta = |bytes: &[u8]| -> Vec<u8> {
        if mods.alt && ALT_IS_META {
            let mut v = vec![0x1b];
            v.extend_from_slice(bytes);
            v
        } else {
            bytes.to_vec()
        }
    };

    let out = match key {
        "enter" => meta(b"\r"),
        "tab" if mods.shift => b"\x1b[Z".to_vec(),
        "tab" => meta(b"\t"),
        "escape" => meta(b"\x1b"),
        "backspace" if mods.ctrl => meta(b"\x08"),
        "backspace" => meta(b"\x7f"),
        "space" if mods.ctrl => meta(b"\x00"),
        "space" if mods.alt && ALT_IS_META => b"\x1b ".to_vec(),
        "up" => csi_final('A'),
        "down" => csi_final('B'),
        "right" => csi_final('C'),
        "left" => csi_final('D'),
        "home" => csi_final('H'),
        "end" => csi_final('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" | "f2" | "f3" | "f4" => {
            let c = match key {
                "f1" => 'P',
                "f2" => 'Q',
                "f3" => 'R',
                _ => 'S',
            };
            if has_mods(mods) {
                format!("\x1b[1;{}{c}", mod_param(mods)).into_bytes()
            } else {
                format!("\x1bO{c}").into_bytes()
            }
        }
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        k if mods.ctrl && k.chars().count() == 1 => {
            let c = k.chars().next()?.to_ascii_lowercase();
            let byte = match c {
                'a'..='z' => c as u8 - b'a' + 1,
                '@' | '2' => 0x00,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '-' | '7' => 0x1f,
                '8' | '?' => 0x7f,
                _ => return None,
            };
            meta(&[byte])
        }
        // Alt+printable: ESC prefix (Meta). The character itself would also
        // reach the input handler, so the caller must stop propagation.
        k if mods.alt && ALT_IS_META && k.chars().count() == 1 => {
            let c = k.chars().next()?;
            let c = if mods.shift {
                c.to_ascii_uppercase()
            } else {
                c
            };
            let mut v = vec![0x1b];
            let mut buf = [0u8; 4];
            v.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            v
        }
        _ => return None,
    };
    Some(out)
}

/// Text pasted into the terminal: newlines become CR (what Enter sends), and
/// in bracketed-paste mode the text is wrapped so shells don't execute it
/// line by line. ESC bytes are stripped from pasted text so a malicious
/// clipboard cannot end the bracket early and inject commands.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let clean: String = normalized.chars().filter(|c| *c != '\x1b').collect();
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        normalized.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ctrl: bool, alt: bool, shift: bool) -> Mods {
        Mods {
            ctrl,
            alt,
            shift,
            platform: false,
        }
    }

    fn enc(key: &str, mods: Mods) -> Option<String> {
        encode(key, mods, false).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    #[test]
    fn named_keys() {
        let none = Mods::default();
        assert_eq!(enc("enter", none).as_deref(), Some("\r"));
        assert_eq!(enc("backspace", none).as_deref(), Some("\x7f"));
        assert_eq!(enc("tab", m(false, false, true)).as_deref(), Some("\x1b[Z"));
        assert_eq!(enc("up", none).as_deref(), Some("\x1b[A"));
        assert_eq!(encode("up", none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            enc("left", m(true, false, false)).as_deref(),
            Some("\x1b[1;5D")
        );
        assert_eq!(
            enc("right", m(false, true, true)).as_deref(),
            Some("\x1b[1;4C")
        );
        assert_eq!(enc("delete", none).as_deref(), Some("\x1b[3~"));
        assert_eq!(
            enc("pageup", m(false, false, true)).as_deref(),
            Some("\x1b[5;2~")
        );
        assert_eq!(enc("f1", none).as_deref(), Some("\x1bOP"));
        assert_eq!(enc("f5", none).as_deref(), Some("\x1b[15~"));
        assert_eq!(
            enc("f12", m(true, false, false)).as_deref(),
            Some("\x1b[24;5~")
        );
    }

    #[test]
    fn control_combinations() {
        let ctrl = m(true, false, false);
        assert_eq!(encode("c", ctrl, false), Some(vec![3]));
        assert_eq!(encode("C", ctrl, false), Some(vec![3]));
        assert_eq!(encode("[", ctrl, false), Some(vec![0x1b]));
        assert_eq!(encode("space", ctrl, false), Some(vec![0]));
        assert_eq!(encode("1", ctrl, false), None);
    }

    #[test]
    fn plain_text_is_left_to_the_input_handler() {
        assert_eq!(encode("a", Mods::default(), false), None);
        assert_eq!(encode("space", Mods::default(), false), None);
        assert_eq!(
            encode(
                "c",
                Mods {
                    platform: true,
                    ..Default::default()
                },
                false
            ),
            None
        );
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn alt_is_meta() {
        assert_eq!(
            encode("b", m(false, true, false), false),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(
            encode("enter", m(false, true, false), false),
            Some(b"\x1b\r".to_vec())
        );
    }

    #[test]
    fn paste_is_safe() {
        assert_eq!(paste_bytes("a\nb\r\n", false), b"a\rb\r".to_vec());
        assert_eq!(
            paste_bytes("ls\x1b[201~; rm -rf ~\n", true),
            b"\x1b[200~ls[201~; rm -rf ~\r\x1b[201~".to_vec()
        );
    }
}
