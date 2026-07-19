//! MIME family → category mapping.

use crate::category::Category;

/// Map a MIME type string to a category when decisive.
///
/// Generic container mimes (`application/zip`, `application/x-ole-storage`) return
/// `None` so the caller can apply §3.4.1 disambiguation.
pub fn category_from_mime(mime: &str) -> Option<Category> {
    let m = mime.trim().to_ascii_lowercase();
    if m.is_empty() {
        return None;
    }
    // Strip parameters.
    let base = m.split(';').next().unwrap_or(&m).trim();

    // Generic containers — not decisive alone (§3.4.1).
    if is_generic_zip_mime(base) || is_generic_ole_mime(base) {
        return None;
    }

    if base == "application/pdf" || base == "application/x-pdf" {
        return Some(Category::Pdf);
    }
    if base.starts_with("image/") {
        return Some(Category::Image);
    }
    if base.starts_with("audio/") || base.starts_with("video/") {
        return Some(Category::Multimedia);
    }
    if base == "text/calendar" || base == "application/ics" {
        return Some(Category::Calendar);
    }
    if base == "text/vcard" || base == "text/x-vcard" {
        return Some(Category::Contact);
    }
    if base == "message/rfc822" || base == "message/rfc2822" {
        return Some(Category::Email);
    }
    // application/vnd.ms-outlook is shared by .msg and sometimes .pst in mime
    // databases — not decisive alone; extension table disambiguates.
    if base == "application/vnd.ms-outlook" {
        return None;
    }
    if base.contains("wordprocessingml")
        || base == "application/msword"
        || base == "application/rtf"
        || base == "text/rtf"
        || base == "text/plain"
        || base == "text/markdown"
    {
        return Some(Category::Document);
    }
    if base.contains("spreadsheetml")
        || base == "application/vnd.ms-excel"
        || base == "text/csv"
        || base == "text/tab-separated-values"
    {
        return Some(Category::Spreadsheet);
    }
    if base.contains("presentationml") || base == "application/vnd.ms-powerpoint" {
        return Some(Category::Presentation);
    }
    if base == "application/x-msdownload"
        || base == "application/x-dosexec"
        || base == "application/vnd.microsoft.portable-executable"
        || base == "application/x-executable"
        || base == "application/x-sharedlib"
    {
        return Some(Category::Executable);
    }
    if base == "application/x-7z-compressed"
        || base == "application/x-rar-compressed"
        || base == "application/vnd.rar"
        || base == "application/x-tar"
        || base == "application/gzip"
        || base == "application/x-gzip"
        || base == "application/x-bzip2"
        || base == "application/x-xz"
    {
        return Some(Category::Archive);
    }
    if base.contains("sqlite") || base == "application/x-msaccess" {
        return Some(Category::Database);
    }
    if base == "application/vnd.ms-outlook-pst"
        || base == "application/vnd.ms-outlook-ost"
        || base == "application/x-msterminal"
    {
        // rare; extension usually wins for .pst
        return Some(Category::Pst);
    }
    None
}

/// True for empty / generic mimes that may be replaced by a stronger guess.
pub fn is_generic_or_empty_mime(mime: Option<&str>) -> bool {
    match mime.map(str::trim).filter(|s| !s.is_empty()) {
        None => true,
        Some(m) => {
            let base = m.split(';').next().unwrap_or(m).trim().to_ascii_lowercase();
            matches!(
                base.as_str(),
                "application/octet-stream"
                    | "application/zip"
                    | "application/x-zip-compressed"
                    | "application/x-ole-storage"
                    | "application/cdfv2"
                    | "binary/octet-stream"
            )
        }
    }
}

pub fn is_generic_zip_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/zip" | "application/x-zip-compressed" | "application/x-zip"
    )
}

pub fn is_generic_ole_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/x-ole-storage"
            | "application/cdfv2"
            | "application/cdfv2-unknown"
            | "application/vnd.ms-office"
    )
}

/// Guess MIME from path via `mime_guess` (extension only).
pub fn guess_mime_from_path(path: &str) -> Option<String> {
    mime_guess::from_path(path)
        .first()
        .map(|m| m.essence_str().to_string())
}
