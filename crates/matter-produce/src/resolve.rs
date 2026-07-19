//! Resolve native and text bytes for production packaging.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use dedup_engine::exporter::{export_eml, EmlMessage};
use matter_core::{join_addrs_json, path_basename, Item, Matter};
use sha2::{Digest, Sha256};

use crate::error::{ProduceError, Result};
use crate::params::ProduceParams;

/// Result of writing a native file.
#[derive(Debug, Clone)]
pub struct NativeArtifact {
    /// Extension without leading dot (e.g. `eml`, `pdf`).
    pub file_ext: String,
    pub mime_type: String,
    pub file_size: u64,
    pub sha256: String,
    /// Absolute path written.
    pub abs_path: String,
}

/// Result of writing text (or none).
#[derive(Debug, Clone)]
pub struct TextArtifact {
    pub has_redacted: bool,
    /// SHA-256 of the bytes written under TEXT/ (for resume integrity).
    pub sha256: String,
}

/// Derive a safe extension for a native from path / mime / default.
pub fn extension_from_item(item: &Item) -> String {
    if let Some(path) = item.path.as_deref() {
        let base = path_basename(Some(path));
        if let Some((_, ext)) = base.rsplit_once('.') {
            let e = ext.trim().to_ascii_lowercase();
            if !e.is_empty() && e.len() <= 16 && e.chars().all(|c| c.is_ascii_alphanumeric()) {
                return e;
            }
        }
    }
    if let Some(mime) = item.mime_type.as_deref() {
        if let Some(ext) = mime_to_ext(mime) {
            return ext.to_string();
        }
    }
    "bin".into()
}

fn mime_to_ext(mime: &str) -> Option<&'static str> {
    let m = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    match m.as_str() {
        "message/rfc822" => Some("eml"),
        "application/pdf" => Some("pdf"),
        "text/plain" => Some("txt"),
        "text/html" => Some("html"),
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "application/vnd.ms-outlook" | "application/msi.outlook" => Some("msg"),
        "text/calendar" | "application/ics" => Some("ics"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some("docx"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Some("pptx"),
        _ => None,
    }
}

/// Whether item looks like email (category or mime).
pub fn is_email_like(item: &Item) -> bool {
    let cat = item
        .file_category
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if cat == "email" || cat == "message" || cat == "mail" {
        return true;
    }
    let mime = item.mime_type.as_deref().unwrap_or("").to_ascii_lowercase();
    mime.starts_with("message/")
        || mime.contains("outlook")
        || item.message_id.is_some()
        || item.from_addr.is_some()
}

/// Stream-copy CAS native to dest; hash while writing.
pub fn copy_cas_native(matter: &Matter, digest: &str, dest: &Path) -> Result<NativeArtifact> {
    let mut reader = matter.cas().open_read(digest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    file.flush()?;
    let sha = hex_encode(hasher.finalize().as_ref());
    Ok(NativeArtifact {
        file_ext: String::new(), // filled by caller
        mime_type: String::new(),
        file_size: total,
        sha256: sha,
        abs_path: dest.display().to_string(),
    })
}

/// Write export-only EML for email without native.
///
/// `body` must already be the production text (redacted when redactions apply).
/// Callers must not pass original text when `redaction_count > 0`.
pub fn write_synthetic_eml(
    item: &Item,
    dest_dir: &Path,
    filename: &str,
    body: String,
) -> Result<NativeArtifact> {
    let to = join_addrs_json(item.to_addrs_json.as_deref());
    let msg = EmlMessage {
        message_id: item.message_id.clone(),
        subject: item
            .subject
            .clone()
            .or_else(|| item.title.clone())
            .unwrap_or_default(),
        sender: item.from_addr.clone().unwrap_or_default(),
        recipients: to,
        date: item.sent_at.clone().or_else(|| item.received_at.clone()),
        body,
        filename: filename.to_string(),
    };
    let path = export_eml(dest_dir, &msg).map_err(|e| ProduceError::Other(format!("eml: {e}")))?;
    let bytes = fs::read(&path)?;
    let sha = hex_encode(Sha256::digest(&bytes).as_ref());
    Ok(NativeArtifact {
        file_ext: "eml".into(),
        mime_type: "message/rfc822".into(),
        file_size: bytes.len() as u64,
        sha256: sha,
        abs_path: path.display().to_string(),
    })
}

/// Load EML body text from the correct CAS digest.
///
/// - `redaction_count > 0` → `redacted_text_sha256` only (fail closed if missing/unavailable)
/// - else → `text_sha256` when present
/// - empty body only when no digest is recorded
///
/// When a digest exists, CAS read failures are propagated (not swallowed as empty).
pub fn load_body_for_eml(
    matter: &Matter,
    item: &Item,
) -> Result<std::result::Result<String, String>> {
    const CAP: u64 = 16 * 1024 * 1024;

    let digest: Option<&str> = if item.redaction_count > 0 {
        let Some(sha) = item
            .redacted_text_sha256
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            // Never fall back to original text_sha256.
            return Ok(Err("redacted_text_missing".into()));
        };
        Some(sha)
    } else {
        item.text_sha256
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };

    match digest {
        Some(sha) => {
            let b = matter.get_bytes_capped(sha, CAP).map_err(|e| {
                ProduceError::Other(format!("EML body CAS read failed for {sha}: {e}"))
            })?;
            Ok(Ok(String::from_utf8_lossy(&b).into_owned()))
        }
        None => Ok(Ok(String::new())),
    }
}

/// Resolve and write native for an item. Returns None when missing and cannot EML.
///
/// When generating synthetic EML, `eml_body` is used when `Some` (already-resolved
/// production text, including redacted). When `None`, body is loaded via
/// [`load_body_for_eml`] (fail-closed for redactions).
pub fn resolve_native(
    matter: &Matter,
    item: &Item,
    params: &ProduceParams,
    natives_dir: &Path,
    control: &str,
    eml_body: Option<String>,
) -> Result<std::result::Result<NativeArtifact, String>> {
    if let Some(sha) = item
        .native_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let ext = extension_from_item(item);
        let filename = format!("{control}.{ext}");
        let dest = natives_dir.join(&filename);
        let mut art = copy_cas_native(matter, sha, &dest)?;
        art.file_ext = ext;
        art.mime_type = item
            .mime_type
            .clone()
            .unwrap_or_else(|| guess_mime(&art.file_ext).to_string());
        return Ok(Ok(art));
    }

    if params.export_eml_if_missing_native && is_email_like(item) {
        let body = match eml_body {
            Some(b) => b,
            None => match load_body_for_eml(matter, item)? {
                Ok(b) => b,
                Err(reason) => return Ok(Err(reason)),
            },
        };
        let filename = format!("{control}.eml");
        let art = write_synthetic_eml(item, natives_dir, &filename, body)?;
        return Ok(Ok(art));
    }

    Ok(Err("missing_native".into()))
}

fn guess_mime(ext: &str) -> &'static str {
    match ext {
        "eml" => "message/rfc822",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ics" => "text/calendar",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

/// Resolve and write TEXT for an item.
///
/// Returns `Err(reason)` for fail/skip conditions (`redacted_text_missing`).
/// Returns `Ok(None)` when no text is available (native-only ok).
pub fn resolve_text(
    matter: &Matter,
    item: &Item,
    text_dir: &Path,
    control: &str,
) -> Result<std::result::Result<Option<TextArtifact>, String>> {
    let needs_redacted = item.redaction_count > 0;
    if needs_redacted {
        let Some(sha) = item
            .redacted_text_sha256
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Ok(Err("redacted_text_missing".into()));
        };
        // NEVER fall back to text_sha256.
        let written = write_text_from_cas(matter, sha, text_dir, control)?;
        return Ok(Ok(Some(TextArtifact {
            has_redacted: true,
            sha256: written,
        })));
    }

    if let Some(sha) = item
        .text_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let written = write_text_from_cas(matter, sha, text_dir, control)?;
        return Ok(Ok(Some(TextArtifact {
            has_redacted: false,
            sha256: written,
        })));
    }

    Ok(Ok(None))
}

/// Write TEXT from CAS; returns SHA-256 of the written bytes.
fn write_text_from_cas(
    matter: &Matter,
    digest: &str,
    text_dir: &Path,
    control: &str,
) -> Result<String> {
    let mut reader = matter.cas().open_read(digest)?;
    fs::create_dir_all(text_dir)?;
    let dest = text_dir.join(format!("{control}.txt"));
    let mut file = File::create(&dest)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
    }
    file.flush()?;
    Ok(hex_encode(hasher.finalize().as_ref()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

// Bring Read into scope for copy_cas_native.
use io::Read;
