//! Magic-byte detection: `infer` + built-ins + OOXML peek + OLE disambiguation.

use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::category::{Category, CategoryMethod, Classification, Confidence};
use crate::extension::category_from_extension;
use crate::mime_map::{category_from_mime, is_generic_or_empty_mime};

/// Max CAS head used for magic (spec: ≤64 KiB).
pub const MAGIC_HEAD_MAX: usize = 64 * 1024;

/// Bounded ZIP entry count for OOXML peek.
const MAX_ZIP_ENTRIES_PEEK: usize = 256;
/// Cap inflated Content_Types read.
const MAX_CONTENT_TYPES_BYTES: u64 = 256 * 1024;

/// Built-in / infer magic result before container disambiguation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MagicKind {
    Pdf,
    Png,
    Jpeg,
    Gif,
    Webp,
    Tiff,
    Bmp,
    Pe,
    Zip,
    Ole,
    /// Other specific type from infer (mime string).
    Other {
        mime: String,
    },
}

/// Detect magic kind from head bytes. Prefer built-ins for stability.
pub fn detect_magic(head: &[u8]) -> Option<MagicKind> {
    if head.is_empty() {
        return None;
    }
    if looks_like_pdf(head) {
        return Some(MagicKind::Pdf);
    }
    if looks_like_png(head) {
        return Some(MagicKind::Png);
    }
    if looks_like_jpeg(head) {
        return Some(MagicKind::Jpeg);
    }
    if looks_like_gif(head) {
        return Some(MagicKind::Gif);
    }
    if looks_like_webp(head) {
        return Some(MagicKind::Webp);
    }
    if looks_like_tiff(head) {
        return Some(MagicKind::Tiff);
    }
    if looks_like_bmp(head) {
        return Some(MagicKind::Bmp);
    }
    if looks_like_pe(head) {
        return Some(MagicKind::Pe);
    }
    if looks_like_zip(head) {
        return Some(MagicKind::Zip);
    }
    if looks_like_ole(head) {
        return Some(MagicKind::Ole);
    }
    // Fall back to infer for other types (audio/video/etc.).
    if let Some(t) = infer::get(head) {
        let mime = t.mime_type().to_string();
        let m = mime.to_ascii_lowercase();
        if m == "application/pdf" {
            return Some(MagicKind::Pdf);
        }
        if m == "image/png" {
            return Some(MagicKind::Png);
        }
        if m == "image/jpeg" {
            return Some(MagicKind::Jpeg);
        }
        if m == "application/zip" || m == "application/x-zip-compressed" {
            return Some(MagicKind::Zip);
        }
        if m.contains("ole") || m == "application/x-ole-storage" || m == "application/cdfv2" {
            return Some(MagicKind::Ole);
        }
        if m == "application/x-msdownload"
            || m == "application/vnd.microsoft.portable-executable"
            || m == "application/x-dosexec"
        {
            return Some(MagicKind::Pe);
        }
        return Some(MagicKind::Other { mime });
    }
    None
}

/// Apply §3.4.1: specific magic is decisive; ZIP/OLE need disambiguation.
pub fn classify_from_magic(
    head: &[u8],
    path: Option<&str>,
    current_mime: Option<&str>,
) -> Option<Classification> {
    let kind = detect_magic(head)?;
    match kind {
        MagicKind::Pdf => Some(
            Classification::new(Category::Pdf, CategoryMethod::Magic, Confidence::High)
                .with_mime(refine_mime(current_mime, Some("application/pdf"))),
        ),
        MagicKind::Png => Some(
            Classification::new(Category::Image, CategoryMethod::Magic, Confidence::High)
                .with_mime(refine_mime(current_mime, Some("image/png"))),
        ),
        MagicKind::Jpeg => Some(
            Classification::new(Category::Image, CategoryMethod::Magic, Confidence::High)
                .with_mime(refine_mime(current_mime, Some("image/jpeg"))),
        ),
        MagicKind::Gif => Some(
            Classification::new(Category::Image, CategoryMethod::Magic, Confidence::High)
                .with_mime(refine_mime(current_mime, Some("image/gif"))),
        ),
        MagicKind::Webp => Some(
            Classification::new(Category::Image, CategoryMethod::Magic, Confidence::High)
                .with_mime(refine_mime(current_mime, Some("image/webp"))),
        ),
        MagicKind::Tiff => Some(
            Classification::new(Category::Image, CategoryMethod::Magic, Confidence::High)
                .with_mime(refine_mime(current_mime, Some("image/tiff"))),
        ),
        MagicKind::Bmp => Some(
            Classification::new(Category::Image, CategoryMethod::Magic, Confidence::High)
                .with_mime(refine_mime(current_mime, Some("image/bmp"))),
        ),
        MagicKind::Pe => Some(
            Classification::new(
                Category::Executable,
                CategoryMethod::Magic,
                Confidence::High,
            )
            .with_mime(refine_mime(
                current_mime,
                Some("application/vnd.microsoft.portable-executable"),
            )),
        ),
        MagicKind::Zip => classify_zip_container(head, path, current_mime),
        MagicKind::Ole => classify_ole_container(path, current_mime),
        MagicKind::Other { mime } => {
            // Unknown specific magic without a MIME map — not decisive alone.
            category_from_mime(&mime).map(|cat| {
                Classification::new(cat, CategoryMethod::Magic, Confidence::Medium)
                    .with_mime(refine_mime(current_mime, Some(&mime)))
            })
        }
    }
}

/// ZIP magic: OOXML peek, else extension tie-break, else archive.
fn classify_zip_container(
    head: &[u8],
    path: Option<&str>,
    current_mime: Option<&str>,
) -> Option<Classification> {
    if let Some(ooxml) = peek_ooxml(head) {
        let (cat, mime) = match ooxml {
            OoxmlKind::Document => (
                Category::Document,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            OoxmlKind::Spreadsheet => (
                Category::Spreadsheet,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            OoxmlKind::Presentation => (
                Category::Presentation,
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
        };
        return Some(
            Classification::new(cat, CategoryMethod::MagicOoxml, Confidence::High)
                .with_mime(refine_mime(current_mime, Some(mime))),
        );
    }
    // Extension tie-break for ZIP/OOXML collision.
    if let Some(p) = path {
        if let Some(cat) = category_from_extension(p) {
            // OOXML extensions still win as document/spreadsheet/presentation.
            if matches!(
                cat,
                Category::Document
                    | Category::Spreadsheet
                    | Category::Presentation
                    | Category::Archive
            ) {
                let mime = ooxml_mime_for_ext_category(cat).or_else(|| {
                    if cat == Category::Archive {
                        Some("application/zip")
                    } else {
                        None
                    }
                });
                return Some(
                    Classification::new(cat, CategoryMethod::ContainerTiebreak, Confidence::Medium)
                        .with_mime(refine_mime(current_mime, mime)),
                );
            }
        }
    }
    // Bare zip / no office ext → archive.
    Some(
        Classification::new(Category::Archive, CategoryMethod::Magic, Confidence::Medium)
            .with_mime(refine_mime(current_mime, Some("application/zip"))),
    )
}

/// OLE/CFB: never one-bucket; disambiguate by extension/mime.
fn classify_ole_container(
    path: Option<&str>,
    current_mime: Option<&str>,
) -> Option<Classification> {
    if let Some(p) = path {
        if let Some(cat) = category_from_extension(p) {
            // .msg → email; .doc/.xls/.ppt → office; never force archive for OLE.
            if cat != Category::Archive {
                let mime = match cat {
                    Category::Email => Some("application/vnd.ms-outlook"),
                    Category::Document => Some("application/msword"),
                    Category::Spreadsheet => Some("application/vnd.ms-excel"),
                    Category::Presentation => Some("application/vnd.ms-powerpoint"),
                    _ => None,
                };
                return Some(
                    Classification::new(cat, CategoryMethod::ContainerTiebreak, Confidence::Medium)
                        .with_mime(refine_mime(current_mime, mime)),
                );
            }
        }
    }
    // MIME may already say outlook/msword etc.
    if let Some(m) = current_mime {
        if let Some(cat) = category_from_mime(m) {
            return Some(
                Classification::new(cat, CategoryMethod::Mime, Confidence::Medium).with_mime(None),
            );
        }
    }
    // Unknown OLE → other (not archive).
    Some(
        Classification::new(Category::Other, CategoryMethod::Magic, Confidence::Low)
            .with_mime(refine_mime(current_mime, Some("application/x-ole-storage"))),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OoxmlKind {
    Document,
    Spreadsheet,
    Presentation,
}

/// Bounded peek for OOXML markers. Never panics; returns None on any failure.
fn peek_ooxml(bytes: &[u8]) -> Option<OoxmlKind> {
    if !looks_like_zip(bytes) {
        return None;
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).ok()?;
    if archive.len() > MAX_ZIP_ENTRIES_PEEK {
        return None;
    }

    // Content_Types.xml
    if let Ok(entry) = archive.by_name("[Content_Types].xml") {
        let mut limited = entry.take(MAX_CONTENT_TYPES_BYTES.saturating_add(1));
        let mut buf = Vec::new();
        if limited.read_to_end(&mut buf).is_ok() && (buf.len() as u64) <= MAX_CONTENT_TYPES_BYTES {
            let s = String::from_utf8_lossy(&buf).to_ascii_lowercase();
            if s.contains("wordprocessingml") || s.contains("/word/") {
                return Some(OoxmlKind::Document);
            }
            if s.contains("spreadsheetml") || s.contains("/xl/") {
                return Some(OoxmlKind::Spreadsheet);
            }
            if s.contains("presentationml") || s.contains("/ppt/") {
                return Some(OoxmlKind::Presentation);
            }
        }
    }

    // Well-known parts by name (re-open names via by_name).
    if archive.by_name("word/document.xml").is_ok() {
        return Some(OoxmlKind::Document);
    }
    if archive.by_name("xl/workbook.xml").is_ok() {
        return Some(OoxmlKind::Spreadsheet);
    }
    if archive.by_name("ppt/presentation.xml").is_ok() {
        return Some(OoxmlKind::Presentation);
    }

    // Prefix scan of entry names (cheap, bounded by entry count already checked).
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let name = entry.name().to_ascii_lowercase();
            if name.starts_with("word/") {
                return Some(OoxmlKind::Document);
            }
            if name.starts_with("xl/") {
                return Some(OoxmlKind::Spreadsheet);
            }
            if name.starts_with("ppt/") {
                return Some(OoxmlKind::Presentation);
            }
        }
    }
    None
}

fn ooxml_mime_for_ext_category(cat: Category) -> Option<&'static str> {
    match cat {
        Category::Document => {
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        }
        Category::Spreadsheet => {
            Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
        }
        Category::Presentation => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
        }
        _ => None,
    }
}

/// Only fill NULL/empty/generic with a stronger mime.
pub fn refine_mime(current: Option<&str>, candidate: Option<&str>) -> Option<String> {
    let cand = candidate.map(str::trim).filter(|s| !s.is_empty())?;
    if is_generic_or_empty_mime(current) {
        return Some(cand.to_string());
    }
    None
}

pub fn looks_like_pdf(b: &[u8]) -> bool {
    b.len() >= 4 && &b[0..4] == b"%PDF"
}

pub fn looks_like_png(b: &[u8]) -> bool {
    b.len() >= 8 && b[0..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
}

pub fn looks_like_jpeg(b: &[u8]) -> bool {
    b.len() >= 3 && b[0] == 0xff && b[1] == 0xd8 && b[2] == 0xff
}

pub fn looks_like_gif(b: &[u8]) -> bool {
    b.len() >= 6 && (&b[0..6] == b"GIF87a" || &b[0..6] == b"GIF89a")
}

pub fn looks_like_webp(b: &[u8]) -> bool {
    b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP"
}

pub fn looks_like_tiff(b: &[u8]) -> bool {
    b.len() >= 4 && ((&b[0..4] == b"II*\0") || (&b[0..4] == b"MM\0*"))
}

pub fn looks_like_bmp(b: &[u8]) -> bool {
    b.len() >= 2 && b[0] == b'B' && b[1] == b'M'
}

pub fn looks_like_zip(b: &[u8]) -> bool {
    b.len() >= 4 && &b[0..2] == b"PK" && (b[2] == 0x03 || b[2] == 0x05 || b[2] == 0x07)
}

/// OLE compound file magic `D0 CF 11 E0 A1 B1 1A E1`.
pub fn looks_like_ole(b: &[u8]) -> bool {
    b.len() >= 8 && b[0..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
}

/// PE: MZ header (optional deeper PE check skipped for short heads).
pub fn looks_like_pe(b: &[u8]) -> bool {
    b.len() >= 2 && b[0] == b'M' && b[1] == b'Z'
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    fn minimal_ooxml_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut cursor);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn pdf_magic_beats_docx_extension() {
        let head = b"%PDF-1.4 fake";
        let c = classify_from_magic(head, Some("invoice.docx"), None).unwrap();
        assert_eq!(c.category, Category::Pdf);
        assert_eq!(c.method, CategoryMethod::Magic);
    }

    #[test]
    fn zip_ooxml_docx_not_archive() {
        let bytes = minimal_ooxml_zip(&[
            (
                "[Content_Types].xml",
                br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
                <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
                </Types>"#,
            ),
            ("word/document.xml", b"<w:document/>"),
        ]);
        let c = classify_from_magic(&bytes, Some("memo.docx"), None).unwrap();
        assert_eq!(c.category, Category::Document);
        assert_eq!(c.method, CategoryMethod::MagicOoxml);
    }

    #[test]
    fn bare_zip_is_archive() {
        let bytes = minimal_ooxml_zip(&[("readme.txt", b"hi")]);
        let c = classify_from_magic(&bytes, Some("data.zip"), None).unwrap();
        assert_eq!(c.category, Category::Archive);
    }

    #[test]
    fn ole_msg_is_email() {
        let mut head = vec![0u8; 16];
        head[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        let c = classify_from_magic(&head, Some("note.msg"), None).unwrap();
        assert_eq!(c.category, Category::Email);
    }

    #[test]
    fn ole_doc_is_document() {
        let mut head = vec![0u8; 16];
        head[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        let c = classify_from_magic(&head, Some("legacy.doc"), None).unwrap();
        assert_eq!(c.category, Category::Document);
    }

    #[test]
    fn ole_xls_is_spreadsheet() {
        let mut head = vec![0u8; 16];
        head[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        let c = classify_from_magic(&head, Some("legacy.xls"), None).unwrap();
        assert_eq!(c.category, Category::Spreadsheet);
        assert_eq!(c.method, CategoryMethod::ContainerTiebreak);
    }

    #[test]
    fn ole_ppt_is_presentation() {
        let mut head = vec![0u8; 16];
        head[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        let c = classify_from_magic(&head, Some("legacy.ppt"), None).unwrap();
        assert_eq!(c.category, Category::Presentation);
        assert_eq!(c.method, CategoryMethod::ContainerTiebreak);
    }

    #[test]
    fn ole_unknown_is_other_not_archive() {
        let mut head = vec![0u8; 16];
        head[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        let c = classify_from_magic(&head, Some("blob.bin"), None).unwrap();
        assert_eq!(c.category, Category::Other);
        assert_ne!(c.category, Category::Archive);
    }
}
