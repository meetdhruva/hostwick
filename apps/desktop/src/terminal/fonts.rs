//! The bundled terminal font: JetBrains Mono (SIL Open Font License 1.1,
//! see `assets/fonts/OFL.txt`), regular, bold and italic. It is the default
//! on every system, so the terminal looks the same everywhere; characters
//! it lacks (CJK, Devanagari, symbols) come from the system's fallback
//! fonts as before.

use std::borrow::Cow;

use gpui_kit::{App, FontFeatures};

/// Family name of the bundled font.
pub const FAMILY: &str = "JetBrains Mono";

const FILES: [&[u8]; 3] = [
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic.ttf"),
];

/// Make the bundled font available to the text system (call once at
/// startup).
pub fn register(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = FILES.iter().map(|b| Cow::Borrowed(*b)).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        tracing::warn!(error = %e, "cannot load the bundled terminal font");
    }
}

/// Programming ligatures are off: a terminal shows what is in the cells
/// (`!=` stays two characters), and column alignment stays exact.
pub fn features() -> FontFeatures {
    FontFeatures::disable_ligatures()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_files_are_fonts() {
        for f in FILES {
            // TrueType / OpenType header.
            assert!(f.starts_with(&[0, 1, 0, 0]) || f.starts_with(b"OTTO"));
            assert!(f.len() > 100_000);
        }
        assert_eq!(FAMILY, "JetBrains Mono");
    }
}
