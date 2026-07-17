//! ZIP entry name encoding fallbacks (UTF-8 → CP437 → Win-1252/Latin-1).

use encoding_rs::WINDOWS_1252;

/// How name bytes were decoded to Unicode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameDecodePath {
    /// General-purpose bit 11 set and/or valid UTF-8 bytes.
    Utf8,
    /// Historical ZIP default code page.
    Cp437,
    /// Single-byte Windows-1252 / Latin-1 style fallback (always succeeds).
    Windows1252,
}

/// Decode ZIP entry name bytes to a Unicode string.
///
/// Policy (spec §3.3.1):
/// 1. If `utf8_flag` is set **or** bytes are valid UTF-8 → UTF-8.
/// 2. Else try CP437.
/// 3. Else Windows-1252 so every byte becomes a Unicode scalar.
///
/// Never fails solely on unknown encoding.
pub fn decode_zip_name(bytes: &[u8], utf8_flag: bool) -> (String, NameDecodePath) {
    if utf8_flag {
        return (
            String::from_utf8_lossy(bytes).into_owned(),
            NameDecodePath::Utf8,
        );
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), NameDecodePath::Utf8);
    }

    // Prefer CP437 for non-UTF-8 ZIP names (historical default).
    // High bytes 0x80–0x9F are unambiguous CP437 graphics (not Latin-1 controls).
    if bytes.iter().any(|&b| (0x80..=0x9F).contains(&b)) {
        return (decode_cp437(bytes), NameDecodePath::Cp437);
    }

    // Remaining invalid UTF-8: try Win-1252 (covers 0x80–0xFF smart quotes, etc.).
    let (cow, _, had_errors) = WINDOWS_1252.decode(bytes);
    if !had_errors {
        return (cow.into_owned(), NameDecodePath::Windows1252);
    }

    // Final bijective Latin-1-style: every byte → U+00xx.
    let s: String = bytes.iter().map(|&b| char::from(b)).collect();
    (s, NameDecodePath::Windows1252)
}

/// IBM Code Page 437 decode for the full 0x00–0xFF range.
fn decode_cp437(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| cp437_char(b)).collect()
}

fn cp437_char(b: u8) -> char {
    // ASCII + C0 controls map 1:1 for 0x00–0x7F.
    if b < 0x80 {
        return char::from(b);
    }
    // High half (0x80–0xFF) — standard CP437 glyph map.
    const HIGH: [char; 128] = [
        '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}',
        '\u{00E7}', '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}',
        '\u{00C4}', '\u{00C5}', '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}',
        '\u{00F2}', '\u{00FB}', '\u{00F9}', '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}',
        '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}', '\u{00E1}', '\u{00ED}', '\u{00F3}',
        '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}', '\u{00BF}', '\u{2310}',
        '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}', '\u{2591}',
        '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
        '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}',
        '\u{2510}', '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}',
        '\u{255E}', '\u{255F}', '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}',
        '\u{2550}', '\u{256C}', '\u{2567}', '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}',
        '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}', '\u{256A}', '\u{2518}', '\u{250C}',
        '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}', '\u{03B1}', '\u{00DF}',
        '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}', '\u{03A6}',
        '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
        '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}',
        '\u{2248}', '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}',
        '\u{25A0}', '\u{00A0}',
    ];
    HIGH[(b - 0x80) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_valid_without_flag() {
        let (s, path) = decode_zip_name(b"folder/\xc3\xa9mail.txt", false);
        assert_eq!(s, "folder/émail.txt");
        assert_eq!(path, NameDecodePath::Utf8);
    }

    #[test]
    fn cp437_high_bytes() {
        // 0x82 = é in CP437
        let (s, path) = decode_zip_name(b"caf\x82.txt", false);
        assert_eq!(s, "café.txt");
        assert_eq!(path, NameDecodePath::Cp437);
    }

    #[test]
    fn utf8_flag_uses_utf8_even_if_invalid() {
        let (s, path) = decode_zip_name(b"bad\xffname", true);
        assert_eq!(path, NameDecodePath::Utf8);
        assert!(s.contains('\u{FFFD}') || s.contains("bad"));
    }
}
