//! Format detection for OOXML / legacy Office.

use crate::error::{Error, Result};
use crate::limits::methods;
use crate::zip_safe::{open_zip, try_read_named_entry};

/// Detected office format for extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeFormat {
    Docx,
    Xlsx,
    Pptx,
}

impl OfficeFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
        }
    }

    pub fn method(self) -> &'static str {
        match self {
            Self::Docx => methods::DOCX_XML_V1,
            Self::Xlsx => methods::CALAMINE_XLSX_V1,
            Self::Pptx => methods::PPTX_XML_V1,
        }
    }

    pub fn file_category(self) -> &'static str {
        match self {
            Self::Docx => file_category::Category::Document.as_str(),
            Self::Xlsx => file_category::Category::Spreadsheet.as_str(),
            Self::Pptx => file_category::Category::Presentation.as_str(),
        }
    }
}

/// Sniff format from path extension, optional mime, and/or bytes.
pub fn detect_format(
    path: Option<&str>,
    mime_type: Option<&str>,
    bytes: Option<&[u8]>,
) -> Result<Option<OfficeFormat>> {
    if let Some(p) = path {
        if let Some(fmt) = from_extension(p) {
            // Legacy OLE extensions → honest error when bytes look OLE, else still reject.
            if is_legacy_extension(p) {
                return Err(Error::UnsupportedLegacy(format!(
                    "legacy extension on path '{p}'"
                )));
            }
            return Ok(Some(fmt));
        }
        if is_legacy_extension(p) {
            return Err(Error::UnsupportedLegacy(format!(
                "legacy Office extension on path '{p}'"
            )));
        }
    }

    if let Some(m) = mime_type {
        if let Some(fmt) = from_mime(m) {
            return Ok(Some(fmt));
        }
    }

    if let Some(b) = bytes {
        if looks_like_ole(b) {
            return Err(Error::UnsupportedLegacy(
                "OLE compound document (legacy .doc/.xls/.ppt)".into(),
            ));
        }
        if let Some(fmt) = sniff_ooxml(b)? {
            return Ok(Some(fmt));
        }
    }

    Ok(None)
}

/// Extension-only eligibility (no error for non-office).
pub fn from_extension(path: &str) -> Option<OfficeFormat> {
    let lower = path.to_ascii_lowercase();
    // Strip archive markers like `foo.zip!/bar.docx`
    let leaf = lower.rsplit(['/', '\\', '!']).next().unwrap_or(&lower);
    let ext = leaf.rsplit('.').next()?;
    match ext {
        "docx" | "docm" | "dotx" | "dotm" => Some(OfficeFormat::Docx),
        "xlsx" | "xlsm" | "xltx" | "xltm" => Some(OfficeFormat::Xlsx),
        "pptx" | "pptm" | "potx" | "potm" | "ppsx" | "ppsm" => Some(OfficeFormat::Pptx),
        _ => None,
    }
}

pub fn is_legacy_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let leaf = lower.rsplit(['/', '\\', '!']).next().unwrap_or(&lower);
    let ext = leaf.rsplit('.').next().unwrap_or("");
    matches!(ext, "doc" | "xls" | "ppt" | "dot" | "xlt" | "pot" | "pps")
}

pub fn from_mime(mime: &str) -> Option<OfficeFormat> {
    let m = mime.to_ascii_lowercase();
    if m.contains("wordprocessingml") {
        Some(OfficeFormat::Docx)
    } else if m.contains("spreadsheetml") {
        Some(OfficeFormat::Xlsx)
    } else if m.contains("presentationml") {
        Some(OfficeFormat::Pptx)
    } else {
        None
    }
}

/// OLE compound document magic: `D0 CF 11 E0 A1 B1 1A E1`.
pub fn looks_like_ole(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[0..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
}

/// ZIP local file header magic `PK\x03\x04` (or empty archive variants).
pub fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == b'P' && bytes[1] == b'K'
}

/// Detect encrypted OOXML (EncryptedPackage / EncryptionInfo).
pub fn is_encrypted_ooxml(bytes: &[u8]) -> Result<bool> {
    if !looks_like_zip(bytes) {
        return Ok(false);
    }
    let mut archive = match open_zip(bytes) {
        Ok(a) => a,
        Err(_) => return Ok(false),
    };
    // Common encryption markers
    for name in ["EncryptionInfo", "EncryptedPackage", "\u{6}DataSpaces"] {
        if archive.by_name(name).is_ok() {
            return Ok(true);
        }
    }
    // Also check Content_Types for encrypted package override
    if let Some(ct) = try_read_named_entry(&mut archive, "[Content_Types].xml")? {
        let s = String::from_utf8_lossy(&ct);
        if s.contains("encrypted-package") || s.contains("EncryptedPackage") {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Sniff OOXML via `[Content_Types].xml` overrides.
pub fn sniff_ooxml(bytes: &[u8]) -> Result<Option<OfficeFormat>> {
    if !looks_like_zip(bytes) {
        return Ok(None);
    }
    if is_encrypted_ooxml(bytes)? {
        return Err(Error::Encrypted("password-encrypted OOXML package".into()));
    }
    let mut archive = open_zip(bytes)?;
    let Some(ct) = try_read_named_entry(&mut archive, "[Content_Types].xml")? else {
        return Ok(None);
    };
    let s = String::from_utf8_lossy(&ct).to_ascii_lowercase();
    if !s.contains("content_types") && !s.contains("types") {
        // still may be content types without that token — continue
    }
    if s.contains("wordprocessingml") || s.contains("/word/") {
        return Ok(Some(OfficeFormat::Docx));
    }
    if s.contains("spreadsheetml") || s.contains("/xl/") {
        return Ok(Some(OfficeFormat::Xlsx));
    }
    if s.contains("presentationml") || s.contains("/ppt/") {
        return Ok(Some(OfficeFormat::Pptx));
    }
    // Fallback: presence of well-known parts
    if archive.by_name("word/document.xml").is_ok() {
        return Ok(Some(OfficeFormat::Docx));
    }
    if archive.by_name("xl/workbook.xml").is_ok() {
        return Ok(Some(OfficeFormat::Xlsx));
    }
    if archive.by_name("ppt/presentation.xml").is_ok() {
        return Ok(Some(OfficeFormat::Pptx));
    }
    Ok(None)
}

/// True when path/mime suggests an office-eligible item (without reading bytes).
pub fn is_office_eligible_meta(path: Option<&str>, mime_type: Option<&str>) -> bool {
    if let Some(p) = path {
        if from_extension(p).is_some() {
            return true;
        }
        // legacy is "eligible" for the job to attempt and error honestly
        if is_legacy_extension(p) {
            return true;
        }
    }
    if let Some(m) = mime_type {
        if from_mime(m).is_some() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_matrix() {
        assert_eq!(from_extension("a.DOCX"), Some(OfficeFormat::Docx));
        assert_eq!(from_extension("a.docm"), Some(OfficeFormat::Docx));
        assert_eq!(from_extension("a.xlsx"), Some(OfficeFormat::Xlsx));
        assert_eq!(from_extension("a.pptx"), Some(OfficeFormat::Pptx));
        assert!(from_extension("a.txt").is_none());
        assert!(is_legacy_extension("memo.doc"));
    }

    #[test]
    fn ole_magic() {
        let mut b = vec![0u8; 16];
        b[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        assert!(looks_like_ole(&b));
        let err = detect_format(Some("x.doc"), None, Some(&b)).unwrap_err();
        assert_eq!(err.code(), "unsupported_legacy_office");
    }
}
