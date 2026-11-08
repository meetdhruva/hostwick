//! Quick preview (Space on a file, like macOS Quick Look): what kind of
//! file it is, and its text decoded for display.
//!
//! This file holds the pure parts: classifying by name and first bytes, and
//! decoding text (UTF-8 with or without a byte-order mark, UTF-16, and
//! Windows-1252 as the fallback for legacy 8-bit files). The view itself is
//! drawn by `FilesView` (see `render_preview` in `mod.rs`).

/// How much of a text file is read for the preview.
pub const TEXT_LIMIT: usize = 256 * 1024;
/// Images larger than this are not loaded (their details are shown).
pub const IMAGE_LIMIT: u64 = 20 * 1024 * 1024;
/// Most lines kept for display, so a huge one-line file stays cheap.
pub const MAX_LINES: usize = 6000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Image(ImageKind),
    /// Anything else: only its details are shown.
    Binary,
}

/// The image type the first bytes say it is.
pub fn sniff_image(head: &[u8]) -> Option<ImageKind> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageKind::Png)
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageKind::Jpeg)
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some(ImageKind::Gif)
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some(ImageKind::Webp)
    } else {
        let text = String::from_utf8_lossy(&head[..head.len().min(2048)]);
        let t = text.trim_start_matches('\u{feff}').trim_start();
        (t.starts_with("<svg") || (t.starts_with("<?xml") && t.contains("<svg")))
            .then_some(ImageKind::Svg)
    }
}

/// Is this plausibly text? No NUL bytes (unless it is UTF-16 with a byte
/// order mark) and few control characters.
pub fn looks_like_text(head: &[u8]) -> bool {
    if head.is_empty() || bom_utf16(head).is_some() {
        return true;
    }
    if head.contains(&0) {
        return false;
    }
    // Text allows only a trickle of control characters, whatever the
    // encoding (control bytes are valid UTF-8, so check them first).
    let control = head
        .iter()
        .filter(|b| **b < 0x20 && !matches!(**b, b'\t' | b'\n' | b'\r' | 0x0C | 0x1B))
        .count();
    control * 100 <= head.len()
}

/// What to show for a file whose first bytes are `head`. The bytes decide,
/// not the name: a `.png` that is not a PNG is not an image.
pub fn classify(head: &[u8]) -> Kind {
    if let Some(k) = sniff_image(head) {
        return Kind::Image(k);
    }
    if looks_like_text(head) {
        Kind::Text
    } else {
        Kind::Binary
    }
}

/// How the text was encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// The fallback for 8-bit text that is not UTF-8.
    Windows1252,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows-1252",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    pub encoding: Encoding,
}

fn bom_utf16(b: &[u8]) -> Option<Encoding> {
    match b {
        [0xFF, 0xFE, ..] => Some(Encoding::Utf16Le),
        [0xFE, 0xFF, ..] => Some(Encoding::Utf16Be),
        _ => None,
    }
}

/// The part of `b` that is valid UTF-8, allowing a character cut off at
/// the end when `b` is only the start of a longer file (`cut`). `None` when
/// it is not UTF-8.
fn valid_utf8_prefix(b: &[u8], cut: bool) -> Option<&str> {
    match std::str::from_utf8(b) {
        Ok(s) => Some(s),
        // Only an incomplete sequence at the very end of a cut-off read is
        // forgiven.
        Err(e) if cut && e.error_len().is_none() => std::str::from_utf8(&b[..e.valid_up_to()]).ok(),
        Err(_) => None,
    }
}

/// Windows-1252 for 0x80..=0x9F (the rest equals Latin-1).
const CP1252: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{8D}', '\u{017D}', '\u{8F}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{9D}', '\u{017E}', '\u{0178}',
];

fn from_windows_1252(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x80..=0x9F => CP1252[usize::from(c - 0x80)],
            c => char::from(c),
        })
        .collect()
}

/// Decode `bytes`, the start of a file (`cut` = the file continues past
/// them): byte-order marks first, then UTF-8, else Windows-1252. Never
/// fails; text that cannot be decoded exactly still shows.
pub fn decode_text(bytes: &[u8], cut: bool) -> Decoded {
    if let Some(enc) = bom_utf16(bytes) {
        let body = &bytes[2..];
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|p| {
                if enc == Encoding::Utf16Le {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        let text = char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect();
        return Decoded {
            text,
            encoding: enc,
        };
    }
    let (body, bom) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => (rest, true),
        None => (bytes, false),
    };
    match valid_utf8_prefix(body, cut) {
        Some(s) => Decoded {
            text: s.to_owned(),
            encoding: if bom {
                Encoding::Utf8Bom
            } else {
                Encoding::Utf8
            },
        },
        None => Decoded {
            text: from_windows_1252(body),
            encoding: Encoding::Windows1252,
        },
    }
}

/// Lines for display: tabs become spaces, carriage returns go, at most
/// [`MAX_LINES`]. The flag says lines were left out.
pub fn display_lines(text: &str) -> (Vec<String>, bool) {
    let mut lines: Vec<String> = text
        .lines()
        .take(MAX_LINES + 1)
        .map(|l| l.replace('\t', "    ").replace('\r', ""))
        .collect();
    let more = lines.len() > MAX_LINES;
    lines.truncate(MAX_LINES);
    (lines, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn images_are_recognised_by_content() {
        assert_eq!(sniff_image(PNG), Some(ImageKind::Png));
        assert_eq!(
            sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10]),
            Some(ImageKind::Jpeg)
        );
        assert_eq!(sniff_image(b"GIF89a\x01\0"), Some(ImageKind::Gif));
        assert_eq!(
            sniff_image(b"RIFF\x10\0\0\0WEBPVP8 "),
            Some(ImageKind::Webp)
        );
        assert_eq!(
            sniff_image(b"<?xml version=\"1.0\"?>\n<svg xmlns=\"x\"/>"),
            Some(ImageKind::Svg)
        );
        assert_eq!(
            sniff_image(b"  <svg viewBox=\"0 0 1 1\">"),
            Some(ImageKind::Svg)
        );
        // Other XML is text, not an image.
        assert_eq!(sniff_image(b"<?xml version=\"1.0\"?><note/>"), None);
        assert_eq!(sniff_image(b"RIFF....WAVE"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn classification_prefers_the_bytes() {
        assert_eq!(classify(PNG), Kind::Image(ImageKind::Png));
        // Named .png but plain text: shown as text, not as a broken image.
        assert_eq!(classify(b"just words"), Kind::Text);
        assert_eq!(classify(b"hello\nworld\n"), Kind::Text);
        assert_eq!(classify(b""), Kind::Text);
        assert_eq!(classify(&[0, 1, 2, 3, 0xFF, 0]), Kind::Binary);
        assert_eq!(classify(&[0x1F, 0x8B, 8, 0, 0, 0, 0, 0]), Kind::Binary);
        // Latin-1 / cp1252 text with accents is still text.
        assert_eq!(classify(b"caf\xe9 cr\xe8me"), Kind::Text);
        // UTF-16 with a BOM contains NULs and is still text.
        assert_eq!(classify(b"\xff\xfeh\0i\0"), Kind::Text);
        // A lot of control characters: binary.
        assert_eq!(classify(&[1u8, 2, 3, 4, 5, 6, 7, 8, 65, 66]), Kind::Binary);
    }

    #[test]
    fn utf8_with_and_without_a_bom() {
        let d = decode_text("héllo €".as_bytes(), false);
        assert_eq!((d.text.as_str(), d.encoding), ("héllo €", Encoding::Utf8));
        let mut with = vec![0xEF, 0xBB, 0xBF];
        with.extend_from_slice("bom ✓".as_bytes());
        let d = decode_text(&with, false);
        assert_eq!((d.text.as_str(), d.encoding), ("bom ✓", Encoding::Utf8Bom));
    }

    #[test]
    fn a_character_cut_by_the_read_limit_is_not_an_error() {
        let full = "ab€cd".as_bytes(); // € is 3 bytes: e2 82 ac
        // Cut in the middle of the euro sign, as a 256 KB read would.
        let cut = &full[..3];
        let d = decode_text(cut, true);
        assert_eq!((d.text.as_str(), d.encoding), ("ab", Encoding::Utf8));
        // The same bytes as a complete file are not UTF-8.
        assert_eq!(decode_text(cut, false).encoding, Encoding::Windows1252);
        // Garbage in the middle is not forgiven even when cut.
        let d = decode_text(b"ab\xffcd", true);
        assert_eq!(d.encoding, Encoding::Windows1252);
    }

    #[test]
    fn utf16_and_windows_1252() {
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("hi é".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let d = decode_text(&le, false);
        assert_eq!((d.text.as_str(), d.encoding), ("hi é", Encoding::Utf16Le));
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("日本".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        let d = decode_text(&be, false);
        assert_eq!((d.text.as_str(), d.encoding), ("日本", Encoding::Utf16Be));
        // Smart quotes and the euro sign live in 0x80..0x9F in cp1252.
        let d = decode_text(b"\x93quoted\x94 \x80 5 caf\xe9", false);
        assert_eq!(d.text, "\u{201C}quoted\u{201D} \u{20AC} 5 café");
        assert_eq!(d.encoding, Encoding::Windows1252);
        // An odd trailing byte in UTF-16 is dropped, not a panic.
        let odd = [0xFF, 0xFE, b'a', 0, b'b'];
        assert_eq!(decode_text(&odd, true).text, "a");
        // Lone surrogates become the replacement character.
        let bad = [0xFF, 0xFE, 0x00, 0xD8, b'x', 0];
        assert_eq!(decode_text(&bad, false).text, "\u{FFFD}x");
    }

    #[test]
    fn display_lines_are_bounded_and_clean() {
        let (l, more) = display_lines("a\tb\r\nc\n");
        assert_eq!(l, vec!["a    b".to_owned(), "c".to_owned()]);
        assert!(!more);
        let big = "x\n".repeat(MAX_LINES + 50);
        let (l, more) = display_lines(&big);
        assert_eq!(l.len(), MAX_LINES);
        assert!(more);
        assert_eq!(display_lines("").0.len(), 0);
    }
}
