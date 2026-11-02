//! Terminal color palette and resolution of cell colors to RGB.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// Dark palette tuned for contrast on a near-black background.
const ANSI: [Rgb; 16] = [
    rgb(0x1d, 0x1f, 0x21), // black
    rgb(0xe0, 0x6c, 0x75), // red
    rgb(0x98, 0xc3, 0x79), // green
    rgb(0xe5, 0xc0, 0x7b), // yellow
    rgb(0x61, 0xaf, 0xef), // blue
    rgb(0xc6, 0x78, 0xdd), // magenta
    rgb(0x56, 0xb6, 0xc2), // cyan
    rgb(0xc8, 0xcc, 0xd4), // white
    rgb(0x5c, 0x63, 0x70), // bright black
    rgb(0xff, 0x7b, 0x86), // bright red
    rgb(0xb5, 0xe8, 0x90), // bright green
    rgb(0xff, 0xd6, 0x8a), // bright yellow
    rgb(0x82, 0xc4, 0xff), // bright blue
    rgb(0xde, 0x99, 0xf0), // bright magenta
    rgb(0x7c, 0xd8, 0xe3), // bright cyan
    rgb(0xf0, 0xf2, 0xf5), // bright white
];

pub const FOREGROUND: Rgb = rgb(0xd7, 0xda, 0xe0);
pub const BACKGROUND: Rgb = rgb(0x14, 0x16, 0x1a);
pub const CURSOR: Rgb = rgb(0x82, 0xc4, 0xff);
pub const SELECTION: Rgb = rgb(0x3a, 0x4a, 0x66);

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

/// xterm 256-color index → RGB (16–231 color cube, 232–255 grayscale).
pub fn indexed(i: u8) -> Rgb {
    match i {
        0..=15 => ANSI[usize::from(i)],
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            rgb(v, v, v)
        }
    }
}

fn dim(c: Rgb) -> Rgb {
    rgb(
        (u16::from(c.r) * 2 / 3) as u8,
        (u16::from(c.g) * 2 / 3) as u8,
        (u16::from(c.b) * 2 / 3) as u8,
    )
}

/// Resolve a cell color, honoring palette overrides set by the remote (OSC 4/10/11).
pub fn resolve(color: Color, overrides: &Colors) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(i) => overrides[usize::from(i)].unwrap_or_else(|| indexed(i)),
        Color::Named(n) => {
            if let Some(c) = overrides[n as usize] {
                return c;
            }
            match n {
                NamedColor::Foreground | NamedColor::BrightForeground => FOREGROUND,
                NamedColor::Background => BACKGROUND,
                NamedColor::Cursor => CURSOR,
                NamedColor::DimForeground => dim(FOREGROUND),
                other => {
                    let idx = other as usize;
                    if idx < 16 {
                        ANSI[idx]
                    } else {
                        // Dim variants follow the specials and map onto 0–7.
                        let base = (idx - NamedColor::DimBlack as usize) % 8;
                        dim(ANSI[base])
                    }
                }
            }
        }
    }
}

pub fn to_hsla(c: Rgb) -> gpui_kit::Hsla {
    gpui_kit::Rgba {
        r: f32::from(c.r) / 255.0,
        g: f32::from(c.g) / 255.0,
        b: f32::from(c.b) / 255.0,
        a: 1.0,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_and_grays() {
        assert_eq!(indexed(16), rgb(0, 0, 0));
        assert_eq!(indexed(231), rgb(255, 255, 255));
        assert_eq!(indexed(196), rgb(255, 0, 0));
        assert_eq!(indexed(232), rgb(8, 8, 8));
        assert_eq!(indexed(255), rgb(238, 238, 238));
    }

    #[test]
    fn overrides_win() {
        let mut colors = Colors::default();
        assert_eq!(resolve(Color::Named(NamedColor::Red), &colors), ANSI[1]);
        colors[1] = Some(rgb(1, 2, 3));
        assert_eq!(
            resolve(Color::Named(NamedColor::Red), &colors),
            rgb(1, 2, 3)
        );
        assert_eq!(resolve(Color::Indexed(1), &colors), rgb(1, 2, 3));
        assert_eq!(
            resolve(Color::Named(NamedColor::DimRed), &Colors::default()),
            dim(ANSI[1])
        );
    }
}
