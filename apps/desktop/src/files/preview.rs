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
/// Widest or tallest image that is decoded. A few KB of PNG can claim to be
/// 60000 x 60000 pixels, which is 14 GB of memory once decoded.
pub const MAX_IMAGE_SIDE: u64 = 16_384;
/// Most pixels decoded (RGBA: 4 bytes each, so 256 MB at the limit).
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;
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

/// The pixel size an image file says it has, from its header (all of the
/// formats put it in the first bytes). SVG: its `width`/`height`, else its
/// `viewBox`; an SVG that names neither has the default size.
pub fn image_size(head: &[u8], kind: ImageKind) -> Option<(u64, u64)> {
    let be16 = |i: usize| {
        head.get(i..i + 2)
            .map(|b| u64::from(u16::from_be_bytes([b[0], b[1]])))
    };
    let le16 = |i: usize| {
        head.get(i..i + 2)
            .map(|b| u64::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let be32 = |i: usize| {
        head.get(i..i + 4)
            .map(|b| u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])))
    };
    match kind {
        ImageKind::Png => {
            // Signature, then the IHDR chunk: length, "IHDR", width, height.
            (head.get(12..16)? == b"IHDR").then_some(())?;
            Some((be32(16)?, be32(20)?))
        }
        ImageKind::Gif => Some((le16(6)?, le16(8)?)),
        ImageKind::Jpeg => {
            let mut i = 2;
            while i + 4 <= head.len() {
                if head[i] != 0xFF {
                    return None;
                }
                let marker = head[i + 1];
                match marker {
                    0xFF => {
                        i += 1;
                        continue;
                    }
                    0x01 | 0xD0..=0xD8 => {
                        i += 2;
                        continue;
                    }
                    0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                        return Some((be16(i + 7)?, be16(i + 5)?));
                    }
                    _ => {}
                }
                i += 2 + usize::from(u16::from_be_bytes([*head.get(i + 2)?, *head.get(i + 3)?]));
            }
            None
        }
        ImageKind::Webp => match head.get(12..16)? {
            b"VP8 " => {
                (head.get(23..26)? == [0x9d, 0x01, 0x2a]).then_some(())?;
                Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff))
            }
            b"VP8L" => {
                let b = head.get(21..25)?;
                let (b0, b1, b2, b3) = (
                    u64::from(b[0]),
                    u64::from(b[1]),
                    u64::from(b[2]),
                    u64::from(b[3]),
                );
                Some((
                    ((b1 & 0x3f) << 8 | b0) + 1,
                    ((b3 & 0x0f) << 10 | b2 << 2 | b1 >> 6) + 1,
                ))
            }
            b"VP8X" => {
                let w = head.get(24..27)?;
                let h = head.get(27..30)?;
                let n = |b: &[u8]| u64::from(b[0]) | u64::from(b[1]) << 8 | u64::from(b[2]) << 16;
                Some((n(w) + 1, n(h) + 1))
            }
            _ => None,
        },
        ImageKind::Svg => svg_size(head),
    }
}

/// Size of an SVG from the attributes of its root element.
fn svg_size(head: &[u8]) -> Option<(u64, u64)> {
    let text = String::from_utf8_lossy(&head[..head.len().min(64 * 1024)]);
    let start = text.find("<svg")?;
    let tag = &text[start..];
    let end = tag.find('>').unwrap_or(tag.len());
    let tag = &tag[..end];
    let attr = |name: &str| -> Option<String> {
        let mut from = 0;
        while let Some(i) = tag[from..].find(name) {
            let at = from + i;
            from = at + name.len();
            let before_ok = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
            let rest = tag[from..].trim_start();
            if before_ok && let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim_start();
                let q = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
                let body = &rest[1..];
                return body.find(q).map(|e| body[..e].to_owned());
            }
        }
        None
    };
    // Absolute lengths only; `100%` has no pixel size of its own.
    let len = |v: &str| -> Option<f64> {
        let v = v.trim();
        if let Ok(n) = v.parse::<f64>() {
            return (n.is_finite() && n >= 0.0).then_some(n);
        }
        let (num, unit) = v.split_at(
            v.find(|c: char| c.is_ascii_alphabetic() || c == '%')
                .unwrap_or(v.len()),
        );
        let n: f64 = num.trim().parse().ok()?;
        let k = match unit {
            "" | "px" => 1.0,
            "pt" => 4.0 / 3.0,
            "pc" => 16.0,
            "mm" => 96.0 / 25.4,
            "cm" => 96.0 / 2.54,
            "in" => 96.0,
            _ => return None,
        };
        (n.is_finite() && n >= 0.0).then_some(n * k)
    };
    // A length that is there but makes no sense is not guessed at.
    let side = |name: &str| -> Result<Option<f64>, ()> {
        match attr(name) {
            None => Ok(None),
            Some(v) if v.trim().ends_with('%') => Ok(None),
            Some(v) => len(&v).map(Some).ok_or(()),
        }
    };
    let mut w = side("width").ok()?;
    let mut h = side("height").ok()?;
    if let Some(vb) = attr("viewBox") {
        let nums: Vec<f64> = vb
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect();
        if let [_, _, vw, vh] = nums[..] {
            if w.is_none() && h.is_none() {
                (w, h) = (Some(vw.abs()), Some(vh.abs()));
            } else {
                // One side given: the other follows the aspect ratio.
                if w.is_none() {
                    w = h.map(|h| if vh > 0.0 { h * vw / vh } else { 0.0 });
                }
                if h.is_none() {
                    h = w.map(|w| if vw > 0.0 { w * vh / vw } else { 0.0 });
                }
            }
        }
    }
    // Neither: the default size (300 x 150).
    let clamp = |v: f64| {
        if v >= u64::MAX as f64 {
            u64::MAX
        } else {
            v.ceil() as u64
        }
    };
    Some((clamp(w.unwrap_or(300.0)), clamp(h.unwrap_or(150.0))))
}

/// Whether an image may be handed to the decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCheck {
    Ok,
    /// Too many pixels (or a side too long) for the size the header gives.
    TooBig(u64, u64),
    /// The size could not be read, or the file is something the decoder is
    /// not to be given (an SVG with entity declarations).
    Unknown,
}

/// Look at an image's declared size before decoding it.
pub fn check_image(bytes: &[u8], kind: ImageKind) -> ImageCheck {
    if kind == ImageKind::Svg {
        let head = &bytes[..bytes.len().min(64 * 1024)];
        if head.windows(8).any(|w| w.eq_ignore_ascii_case(b"<!ENTITY")) {
            return ImageCheck::Unknown;
        }
    }
    let Some((w, h)) = image_size(bytes, kind) else {
        return ImageCheck::Unknown;
    };
    if w == 0 || h == 0 {
        return ImageCheck::Unknown;
    }
    if w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE || w.saturating_mul(h) > MAX_IMAGE_PIXELS {
        ImageCheck::TooBig(w, h)
    } else {
        ImageCheck::Ok
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

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 6, 0, 0, 0]);
        v
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        // SOI, an APP0 segment, then SOF0.
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 6, b'J', b'F', b'I', b'F'];
        v.extend([0xFF, 0xC0, 0, 17, 8]);
        v.extend(h.to_be_bytes());
        v.extend(w.to_be_bytes());
        v.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        v
    }

    #[test]
    fn image_sizes_come_from_the_headers() {
        assert_eq!(image_size(&png(640, 480), ImageKind::Png), Some((640, 480)));
        assert_eq!(
            image_size(&jpeg(1920, 1080), ImageKind::Jpeg),
            Some((1920, 1080))
        );
        let gif = [b"GIF89a".as_slice(), &[0x40, 0x01, 0xC8, 0x00]].concat();
        assert_eq!(image_size(&gif, ImageKind::Gif), Some((320, 200)));
        // WebP: lossy, lossless and extended.
        let mut vp8 = b"RIFF\0\0\0\0WEBPVP8 \0\0\0\0\0\0\0".to_vec();
        vp8.extend([0x9d, 0x01, 0x2a, 0x00, 0x04, 0x00, 0x03]);
        assert_eq!(image_size(&vp8, ImageKind::Webp), Some((1024, 768)));
        let mut vp8l = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0".to_vec();
        // width 100, height 50 (stored minus one)
        let (w, h) = (99u32, 49u32);
        vp8l.push(0x2f);
        vp8l.extend((w | h << 14).to_le_bytes());
        assert_eq!(image_size(&vp8l, ImageKind::Webp), Some((100, 50)));
        let mut vp8x = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        vp8x.extend([0xff, 0x0f, 0x00, 0xff, 0x07, 0x00]);
        assert_eq!(image_size(&vp8x, ImageKind::Webp), Some((4096, 2048)));
        // Truncated headers say nothing.
        assert_eq!(image_size(&png(1, 1)[..15], ImageKind::Png), None);
        assert_eq!(image_size(b"GIF89a", ImageKind::Gif), None);
    }

    #[test]
    fn svg_size_is_width_height_or_view_box() {
        let size = |s: &str| image_size(s.as_bytes(), ImageKind::Svg);
        assert_eq!(
            size(r#"<svg width="200" height="100" xmlns="x"/>"#),
            Some((200, 100))
        );
        assert_eq!(
            size(r#"<svg viewBox="0 0 24 24" xmlns="x"/>"#),
            Some((24, 24))
        );
        assert_eq!(size(r#"<svg viewBox="0,0,400.5,10"/>"#), Some((401, 10)));
        assert_eq!(size(r#"<svg width='2in' height='1in'/>"#), Some((192, 96)));
        // Percentages have no size of their own: the viewBox decides.
        assert_eq!(
            size(r#"<svg width="100%" height="100%" viewBox="0 0 50 40"/>"#),
            Some((50, 40))
        );
        // One side: the other follows the aspect ratio.
        assert_eq!(
            size(r#"<svg width="100" viewBox="0 0 50 25"/>"#),
            Some((100, 50))
        );
        // Nothing at all: the default size.
        assert_eq!(size("<svg/>"), Some((300, 150)));
        // stroke-width is not width.
        assert_eq!(
            size(r#"<svg stroke-width="999999" viewBox="0 0 10 10"/>"#),
            Some((10, 10))
        );
    }

    #[test]
    fn huge_images_are_refused_before_decoding() {
        // A 70-byte PNG that claims 60000 x 60000.
        assert_eq!(
            check_image(&png(60_000, 60_000), ImageKind::Png),
            ImageCheck::TooBig(60_000, 60_000)
        );
        // Within a side limit but over the pixel budget.
        assert_eq!(
            check_image(&png(10_000, 10_000), ImageKind::Png),
            ImageCheck::TooBig(10_000, 10_000)
        );
        // One very long side.
        assert_eq!(
            check_image(&png(100_000, 2), ImageKind::Png),
            ImageCheck::TooBig(100_000, 2)
        );
        assert_eq!(
            check_image(&jpeg(40_000, 40_000), ImageKind::Jpeg),
            ImageCheck::TooBig(40_000, 40_000)
        );
        // Ordinary sizes pass, up to the limits.
        assert_eq!(
            check_image(&png(4000, 3000), ImageKind::Png),
            ImageCheck::Ok
        );
        assert_eq!(
            check_image(&png(8000, 8000), ImageKind::Png),
            ImageCheck::Ok
        );
        assert_eq!(check_image(&png(16_384, 1), ImageKind::Png), ImageCheck::Ok);
        // Unreadable or empty sizes are not decoded either.
        assert_eq!(check_image(&png(0, 5), ImageKind::Png), ImageCheck::Unknown);
        assert_eq!(
            check_image(b"\x89PNG\r\n\x1a\nxx", ImageKind::Png),
            ImageCheck::Unknown
        );
    }

    #[test]
    fn huge_svgs_and_entity_bombs_are_refused() {
        let svg = |s: &str| check_image(s.as_bytes(), ImageKind::Svg);
        assert_eq!(svg(r#"<svg viewBox="0 0 24 24"/>"#), ImageCheck::Ok);
        assert_eq!(
            svg(r#"<svg width="1000000" height="1000000"/>"#),
            ImageCheck::TooBig(1_000_000, 1_000_000)
        );
        assert_eq!(
            svg(r#"<svg viewBox="0 0 99999 99999"/>"#),
            ImageCheck::TooBig(99_999, 99_999)
        );
        assert_eq!(
            svg(r#"<svg width="1e308" height="1e308"/>"#),
            ImageCheck::TooBig(u64::MAX, u64::MAX)
        );
        // A length that makes no sense is not guessed at.
        assert_eq!(
            svg(r#"<svg width="wide" height="10"/>"#),
            ImageCheck::Unknown
        );
        assert_eq!(svg(r#"<svg width="-5" height="10"/>"#), ImageCheck::Unknown);
        let bomb = r#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaa"><!ENTITY b "&a;&a;&a;">]><svg viewBox="0 0 1 1"/>"#;
        assert_eq!(svg(bomb), ImageCheck::Unknown);
    }

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
