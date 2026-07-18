//! Top-level extract API.

use crate::detect::{self, OfficeFormat};
use crate::docx::extract_docx;
use crate::error::{Error, Result};
use crate::limits::MAX_NATIVE_INPUT_BYTES;
use crate::pptx::extract_pptx;
use crate::xlsx::extract_xlsx;
use crate::ExtractedText;

/// Extract plain text from office bytes.
///
/// Detection order: explicit `path` / `mime_type` hints, then byte sniff.
/// Wraps format parsers so callers can still use `catch_unwind` at the job
/// boundary for panic isolation.
pub fn extract_office(
    data: &[u8],
    path: Option<&str>,
    mime_type: Option<&str>,
) -> Result<ExtractedText> {
    if data.len() as u64 > MAX_NATIVE_INPUT_BYTES {
        return Err(Error::limit(format!(
            "native size {} exceeds max {MAX_NATIVE_INPUT_BYTES}",
            data.len()
        )));
    }

    // Legacy OLE short-circuit
    if detect::looks_like_ole(data) {
        return Err(Error::UnsupportedLegacy(
            "OLE compound document (legacy Office)".into(),
        ));
    }
    if let Some(p) = path {
        if detect::is_legacy_extension(p) {
            return Err(Error::UnsupportedLegacy(format!(
                "legacy extension on '{p}'"
            )));
        }
    }

    if detect::is_encrypted_ooxml(data)? {
        return Err(Error::Encrypted("password-encrypted OOXML".into()));
    }

    let format = detect::detect_format(path, mime_type, Some(data))?
        .ok_or_else(|| Error::parse("not a supported OOXML office format"))?;

    extract_format(data, format)
}

/// Extract with a known format (skips re-detect).
pub fn extract_format(data: &[u8], format: OfficeFormat) -> Result<ExtractedText> {
    if data.len() as u64 > MAX_NATIVE_INPUT_BYTES {
        return Err(Error::limit(format!(
            "native size {} exceeds max {MAX_NATIVE_INPUT_BYTES}",
            data.len()
        )));
    }
    match format {
        OfficeFormat::Docx => extract_docx(data),
        OfficeFormat::Xlsx => extract_xlsx(data),
        OfficeFormat::Pptx => extract_pptx(data),
    }
}

/// Panic-isolating wrapper for job use. Converts panics to parse errors.
pub fn extract_office_catch_unwind(
    data: &[u8],
    path: Option<&str>,
    mime_type: Option<&str>,
) -> Result<ExtractedText> {
    let data_owned = data.to_vec();
    let path_owned = path.map(|s| s.to_string());
    let mime_owned = mime_type.map(|s| s.to_string());
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_office(&data_owned, path_owned.as_deref(), mime_owned.as_deref())
    })) {
        Ok(r) => r,
        Err(payload) => {
            let msg = panic_message(payload);
            Err(Error::parse(format!("parser panic isolated: {msg}")))
        }
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}
