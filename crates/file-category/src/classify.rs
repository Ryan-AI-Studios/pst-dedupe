//! Classification pipeline (priority order from spec §3.4).

use crate::category::{Category, CategoryMethod, Classification, Confidence, TAXONOMY_V1};
use crate::extension::category_from_extension;
use crate::magic::{classify_from_magic, refine_mime};
use crate::mime_map::{category_from_mime, guess_mime_from_path, is_generic_or_empty_mime};

/// Inputs for pure classification (no I/O).
#[derive(Debug, Clone, Default)]
pub struct ClassifyInput<'a> {
    pub path: Option<&'a str>,
    pub mime_type: Option<&'a str>,
    pub role: Option<&'a str>,
    pub message_class: Option<&'a str>,
    /// First ≤64 KiB of native CAS (optional).
    pub head_bytes: Option<&'a [u8]>,
    /// Existing `file_category` (for extractor-refine respect).
    pub current_category: Option<&'a str>,
    /// When true (default non-force), keep decisive closed-set categories.
    pub respect_extractor_refine: bool,
}

/// Classify using the locked priority pipeline.
///
/// 1. Structural / message_class  
/// 2. Extractor refine (optional)  
/// 3. Magic (specific beats extension; ZIP/OLE → §3.4.1)  
/// 4. MIME (stored or guessed)  
/// 5. Extension  
/// 6. Fallback: unrecognized if no signals else other  
pub fn classify(input: &ClassifyInput<'_>) -> Classification {
    // Priority 1 — structural / message class (parent rows; not attachment filename override).
    if let Some(mc) = input.message_class.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(cat) = category_from_message_class(mc) {
            return Classification::new(cat, CategoryMethod::MessageClass, Confidence::High);
        }
    }

    // Priority 2 — keep decisive extractor / prior closed-set category.
    if input.respect_extractor_refine {
        if let Some(raw) = input.current_category {
            if Category::is_decisive_existing(Some(raw)) {
                if let Some(cat) = Category::parse_loose(raw) {
                    return Classification::new(cat, CategoryMethod::Extractor, Confidence::High);
                }
            }
        }
    }

    // Priority 3 — magic bytes.
    if let Some(head) = input.head_bytes {
        if !head.is_empty() {
            if let Some(c) = classify_from_magic(head, input.path, input.mime_type) {
                return c;
            }
        }
    }

    // Priority 4 — stored MIME, else mime_guess from path.
    let stored_mime = input.mime_type.map(str::trim).filter(|s| !s.is_empty());
    if let Some(m) = stored_mime {
        // Generic container MIME still needs §3.4.1 when we have path (no head).
        if let Some(c) = classify_container_mime(m, input.path, stored_mime) {
            return c;
        }
        if let Some(cat) = category_from_mime(m) {
            return Classification::new(cat, CategoryMethod::Mime, Confidence::Medium)
                .with_mime(None);
        }
    } else if let Some(path) = input.path {
        if let Some(guessed) = guess_mime_from_path(path) {
            if let Some(c) = classify_container_mime(&guessed, input.path, None) {
                return c;
            }
            if let Some(cat) = category_from_mime(&guessed) {
                return Classification::new(cat, CategoryMethod::Mime, Confidence::Medium)
                    .with_mime(Some(guessed));
            }
        }
    }

    // Priority 5 — extension table.
    if let Some(path) = input.path {
        if let Some(cat) = category_from_extension(path) {
            let mime = if is_generic_or_empty_mime(input.mime_type) {
                guess_mime_from_path(path)
            } else {
                None
            };
            return Classification::new(cat, CategoryMethod::Extension, Confidence::Medium)
                .with_mime(mime);
        }
    }

    // Priority 6 — fallback.
    let has_signal = input.path.map(|p| !p.trim().is_empty()).unwrap_or(false)
        || input
            .mime_type
            .map(|m| !m.trim().is_empty())
            .unwrap_or(false)
        || input.head_bytes.map(|b| !b.is_empty()).unwrap_or(false)
        || input
            .message_class
            .map(|m| !m.trim().is_empty())
            .unwrap_or(false);

    if has_signal {
        Classification::new(Category::Other, CategoryMethod::Fallback, Confidence::Low)
    } else {
        Classification::new(
            Category::Unrecognized,
            CategoryMethod::Fallback,
            Confidence::Low,
        )
    }
}

/// Convenience: classify from path + optional mime only (insert hooks).
pub fn classify_path_mime(path: Option<&str>, mime_type: Option<&str>) -> Classification {
    classify(&ClassifyInput {
        path,
        mime_type,
        respect_extractor_refine: false,
        ..Default::default()
    })
}

/// Convenience: classify with optional head bytes (post-CAS attachment path).
pub fn classify_with_head(
    path: Option<&str>,
    mime_type: Option<&str>,
    head_bytes: Option<&[u8]>,
) -> Classification {
    classify(&ClassifyInput {
        path,
        mime_type,
        head_bytes,
        respect_extractor_refine: false,
        ..Default::default()
    })
}

fn category_from_message_class(mc: &str) -> Option<Category> {
    let lower = mc.to_ascii_lowercase();
    if lower.starts_with("ipm.appointment") || lower.starts_with("ipm.schedule.meeting") {
        return Some(Category::Calendar);
    }
    if lower.starts_with("ipm.contact") {
        return Some(Category::Contact);
    }
    if lower.starts_with("ipm.note")
        || lower.starts_with("ipm.post")
        || lower.starts_with("ipm.sticky")
        || lower == "ipm"
        || lower.starts_with("ipm.report")
        || lower.starts_with("report.")
        || lower.starts_with("ipm.distlist")
    {
        return Some(Category::Email);
    }
    // Generic IPM.* message classes default to email (parent rows).
    if lower.starts_with("ipm.") {
        return Some(Category::Email);
    }
    None
}

/// When MIME is generic ZIP/OLE without head bytes, use extension disambiguation.
fn classify_container_mime(
    mime: &str,
    path: Option<&str>,
    current_mime: Option<&str>,
) -> Option<Classification> {
    let base = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    use crate::mime_map::{is_generic_ole_mime, is_generic_zip_mime};

    if is_generic_zip_mime(&base) {
        if let Some(p) = path {
            if let Some(cat) = category_from_extension(p) {
                if matches!(
                    cat,
                    Category::Document
                        | Category::Spreadsheet
                        | Category::Presentation
                        | Category::Archive
                ) {
                    return Some(
                        Classification::new(
                            cat,
                            CategoryMethod::ContainerTiebreak,
                            Confidence::Medium,
                        )
                        .with_mime(refine_mime(current_mime, Some(mime))),
                    );
                }
            }
        }
        return Some(
            Classification::new(Category::Archive, CategoryMethod::Mime, Confidence::Medium)
                .with_mime(refine_mime(current_mime, Some("application/zip"))),
        );
    }

    if is_generic_ole_mime(&base) {
        if let Some(p) = path {
            if let Some(cat) = category_from_extension(p) {
                if cat != Category::Archive {
                    return Some(
                        Classification::new(
                            cat,
                            CategoryMethod::ContainerTiebreak,
                            Confidence::Medium,
                        )
                        .with_mime(None),
                    );
                }
            }
        }
        return Some(
            Classification::new(Category::Other, CategoryMethod::Mime, Confidence::Low)
                .with_mime(refine_mime(current_mime, Some("application/x-ole-storage"))),
        );
    }

    None
}

/// Public re-export of taxonomy constant for callers.
pub fn taxonomy_id() -> &'static str {
    TAXONOMY_V1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_matrix() {
        let cases = [
            ("memo.docx", Category::Document),
            ("sheet.xlsx", Category::Spreadsheet),
            ("deck.pptx", Category::Presentation),
            ("a.pdf", Category::Pdf),
            ("x.png", Category::Image),
            ("data.zip", Category::Archive),
            ("a.exe", Category::Executable),
            ("meet.ics", Category::Calendar),
            ("box.pst", Category::Pst),
            ("nums.csv", Category::Spreadsheet),
            ("notes.txt", Category::Document),
            ("mail.msg", Category::Email),
            ("legacy.doc", Category::Document),
        ];
        for (path, expected) in cases {
            let c = classify_path_mime(Some(path), None);
            assert_eq!(c.category, expected, "path={path}");
        }
    }

    #[test]
    fn message_class_calendar() {
        let c = classify(&ClassifyInput {
            message_class: Some("IPM.Appointment"),
            path: Some("should_not_override.docx"),
            respect_extractor_refine: false,
            ..Default::default()
        });
        assert_eq!(c.category, Category::Calendar);
        assert_eq!(c.method, CategoryMethod::MessageClass);
    }

    #[test]
    fn message_class_email_default() {
        let c = classify(&ClassifyInput {
            message_class: Some("IPM.Note"),
            path: Some("foo.pdf"),
            respect_extractor_refine: false,
            ..Default::default()
        });
        assert_eq!(c.category, Category::Email);
    }

    #[test]
    fn extractor_refine_kept() {
        let c = classify(&ClassifyInput {
            path: Some("x.bin"),
            current_category: Some("document"),
            respect_extractor_refine: true,
            ..Default::default()
        });
        assert_eq!(c.category, Category::Document);
        assert_eq!(c.method, CategoryMethod::Extractor);
    }

    #[test]
    fn attachment_not_kept_as_refine() {
        let c = classify(&ClassifyInput {
            path: Some("report.pdf"),
            current_category: Some("attachment"),
            respect_extractor_refine: true,
            ..Default::default()
        });
        assert_eq!(c.category, Category::Pdf);
        assert_ne!(c.category.as_str(), "attachment");
    }

    #[test]
    fn empty_signals_unrecognized() {
        let c = classify(&ClassifyInput::default());
        assert_eq!(c.category, Category::Unrecognized);
    }

    #[test]
    fn role_attachment_still_gets_content_category() {
        let c = classify(&ClassifyInput {
            path: Some("invoice.pdf"),
            role: Some("attachment"),
            respect_extractor_refine: false,
            ..Default::default()
        });
        assert_eq!(c.category, Category::Pdf);
    }
}
