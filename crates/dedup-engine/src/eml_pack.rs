//! Unique EML pack writer (`eml_pack_v1`) — multipart MIME, volume batching, UTC Date.
//!
//! Driven by post-promotion keep-set winners only (no re-dedupe). Source PSTs are
//! never mutated; attach bytes are streamed via [`AttachStreamSource`].
//!
//! ## Wire form
//! - Headers, blank lines, MIME boundaries, and base64 lines use CRLF (`\r\n`).
//! - Soft attach open/stream failures **skip** the part entirely (no fake body).
//! - Embedded messages use `Content-Type: message/rfc822` with 8bit/binary raw
//!   transfer (never base64). Full nested MAPI re-parse remains residual
//!   `D-0067-embedded-depth`.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::keepset::{CanonicalAttachment, CanonicalMessage, FamilyPolicy, MessageLocus};
use crate::util::filetime_to_unix;

/// Stable JSON schema identifier for EML pack manifests.
pub const EML_PACK_SCHEMA: &str = "eml_pack_v1";

/// Absolute path length budget (safety margin under classic Windows MAX_PATH 260).
/// Counted in UTF-16 code units (Windows path length semantics).
pub const ABS_PATH_BUDGET: usize = 250;

/// Default max `.eml` files per volume directory.
pub const DEFAULT_FILES_PER_VOLUME: u32 = 10_000;

/// Clamp bounds for `--files-per-volume`.
pub const FILES_PER_VOLUME_MIN: u32 = 1_000;
pub const FILES_PER_VOLUME_MAX: u32 = 50_000;

/// PidTagAttachMethod = ATTACH_EMBEDDED_MSG (MS-PST / MAPI).
pub const ATTACH_EMBEDDED_MSG: i32 = 0x0000_0005;

/// Default max nested embedded-message depth (deeper → residual flag).
pub const DEFAULT_MAX_EMBEDDED_DEPTH: u32 = 3;

/// Manifest / row reason when one or more attachment parts were soft-skipped.
pub const REASON_ATTACH_PART_FAILED: &str = "ATTACH_PART_FAILED";

// ─── Options / results ──────────────────────────────────────────────────────

/// Options controlling EML serialization of a canonical message.
#[derive(Clone, Debug)]
pub struct EmlWriteOpts {
    pub family_policy: FamilyPolicy,
    /// Max recursive depth for nested message/rfc822 extraction (P0 residual).
    pub max_embedded_depth: u32,
}

impl Default for EmlWriteOpts {
    fn default() -> Self {
        Self {
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            max_embedded_depth: DEFAULT_MAX_EMBEDDED_DEPTH,
        }
    }
}

/// Per-message write stats (also used for manifest rows).
#[derive(Clone, Debug, Default)]
pub struct EmlWriteResult {
    pub attachments_file_written: u64,
    pub embedded_messages_written: u64,
    pub attachments_failed: u64,
    /// True when an embedded part was labeled message/rfc822 but not recursively parsed.
    pub embedded_message_unparsed: bool,
}

/// EML pack write errors.
#[derive(Debug)]
pub enum EmlWriteError {
    Io(io::Error),
    PathBudget(String),
    Other(String),
}

impl fmt::Display for EmlWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "eml write io: {e}"),
            Self::PathBudget(s) => write!(f, "eml path budget: {s}"),
            Self::Other(s) => write!(f, "eml write: {s}"),
        }
    }
}

impl std::error::Error for EmlWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for EmlWriteError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Opens attachment binary streams by parent locus + attach NID (streaming handle).
///
/// Implementations must not load multi-GB payloads into a single `Vec`.
/// Soft failures (missing stream, open error) must surface as `Err` so the pack
/// writer can **skip** the part without inventing a body (H1).
pub trait AttachStreamSource {
    fn open_attach(
        &mut self,
        parent: &MessageLocus,
        attach_nid: u64,
    ) -> Result<Box<dyn Read>, EmlWriteError>;
}

/// No-op stream source (uses only in-memory `CanonicalAttachment.data` when present).
pub struct NullAttachStreamSource;

impl AttachStreamSource for NullAttachStreamSource {
    fn open_attach(
        &mut self,
        _parent: &MessageLocus,
        attach_nid: u64,
    ) -> Result<Box<dyn Read>, EmlWriteError> {
        Err(EmlWriteError::Other(format!(
            "no attach stream source for attach_nid={attach_nid}"
        )))
    }
}

// ─── CRLF helpers (MIME wire form) ──────────────────────────────────────────

/// Write `s` followed by CRLF (`\r\n`). Used for headers, boundaries, blank lines.
pub fn write_crlf_line<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    w.write_all(s.as_bytes())?;
    w.write_all(b"\r\n")?;
    Ok(())
}

fn write_crlf_blank<W: Write>(w: &mut W) -> io::Result<()> {
    w.write_all(b"\r\n")?;
    Ok(())
}

/// Normalize text MIME body line endings to CRLF without double-converting `\r\n`.
///
/// - lone `\n` → `\r\n`
/// - lone `\r` → `\r\n`
/// - existing `\r\n` preserved as a single CRLF
/// - non-empty body always ends with CRLF
pub fn normalize_text_body_crlf(s: &str) -> String {
    let bytes = normalize_body_crlf_bytes(s.as_bytes());
    // SAFETY: input was UTF-8; we only insert ASCII CR/LF.
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Byte-oriented CRLF normalization for text/html (and plain) MIME bodies.
pub fn normalize_body_crlf_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().saturating_add(8));
    let mut i = 0usize;
    while i < data.len() {
        match data[i] {
            b'\r' => {
                if i + 1 < data.len() && data[i + 1] == b'\n' {
                    out.extend_from_slice(b"\r\n");
                    i += 2;
                } else {
                    out.extend_from_slice(b"\r\n");
                    i += 1;
                }
            }
            b'\n' => {
                out.extend_from_slice(b"\r\n");
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    if !out.is_empty() && !out.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn write_plain_body<W: Write>(w: &mut W, plain: &str) -> io::Result<()> {
    w.write_all(normalize_text_body_crlf(plain).as_bytes())
}

fn write_html_body<W: Write>(w: &mut W, html: &[u8]) -> io::Result<()> {
    w.write_all(&normalize_body_crlf_bytes(html))
}

// ─── UTC Date ───────────────────────────────────────────────────────────────

/// Format a Windows FILETIME as RFC 5322 Date with **UTC +0000 only**.
///
/// Never applies the host local timezone. Missing/invalid times return `None`.
pub fn format_date_utc_filetime(ft: i64) -> Option<String> {
    let unix = filetime_to_unix(ft);
    format_date_utc_unix(unix)
}

/// Format Unix seconds as RFC 5322 Date with **UTC +0000 only**.
pub fn format_date_utc_unix(unix_secs: i64) -> Option<String> {
    let dt = Utc.timestamp_opt(unix_secs, 0).single()?;
    // RFC 5322: `Mon, 02 Jan 2006 15:04:05 +0000`
    Some(dt.format("%a, %d %b %Y %H:%M:%S +0000").to_string())
}

// ─── Volume helpers ─────────────────────────────────────────────────────────

/// Clamp files-per-volume to the locked range [1000, 50000] (CLI flag boundary only).
///
/// [`VolumePackWriter`] accepts any `files_per_volume ≥ 1` so unit tests can use
/// small rollover values without bypassing construction.
pub fn clamp_files_per_volume(n: u32) -> u32 {
    n.clamp(FILES_PER_VOLUME_MIN, FILES_PER_VOLUME_MAX)
}

/// Reject volume prefixes that are not a single safe path component.
///
/// Rules (P1 path-traversal):
/// - non-empty
/// - no path separators (`\`, `/`)
/// - not `.` or `..`
/// - no absolute / drive forms (`:`, leading `\\`)
/// - no control characters
/// - only ASCII alphanumeric, underscore, or hyphen
pub fn validate_volume_prefix(prefix: &str) -> Result<(), EmlWriteError> {
    if prefix.is_empty() {
        return Err(EmlWriteError::Other(
            "volume prefix must not be empty".into(),
        ));
    }
    if prefix == "." || prefix == ".." {
        return Err(EmlWriteError::Other(format!(
            "volume prefix must not be '.' or '..' (got {prefix:?})"
        )));
    }
    if prefix.contains('/') || prefix.contains('\\') {
        return Err(EmlWriteError::Other(format!(
            "volume prefix must not contain path separators (got {prefix:?})"
        )));
    }
    if prefix.contains(':') {
        return Err(EmlWriteError::Other(format!(
            "volume prefix must not contain drive/absolute markers (got {prefix:?})"
        )));
    }
    if prefix.chars().any(|c| c.is_control()) {
        return Err(EmlWriteError::Other(
            "volume prefix must not contain control characters".into(),
        ));
    }
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(EmlWriteError::Other(format!(
            "volume prefix must be ASCII alphanumeric/underscore/hyphen only (got {prefix:?})"
        )));
    }
    Ok(())
}

/// Volume directory name: `VOL001`, `VOL002`, … (zero-padded to at least 3 digits).
///
/// Caller must pass a prefix already accepted by [`validate_volume_prefix`].
pub fn volume_dirname(volume_index: u32, prefix: &str) -> String {
    // volume_index is 1-based.
    let width = if volume_index >= 1000 {
        // grow as needed for large packs
        ((volume_index as f64).log10().floor() as usize) + 1
    } else {
        3
    };
    format!("{prefix}{volume_index:0width$}")
}

/// 1-based volume number for a 0-based global export index.
pub fn volume_for_index(index0: u64, files_per_volume: u32) -> u32 {
    let fpv = files_per_volume.max(1) as u64;
    ((index0 / fpv) + 1) as u32
}

// ─── Filename / MAX_PATH ────────────────────────────────────────────────────

/// Build a deterministic, Windows-safe EML filename that keeps abs path ≤ budget.
///
/// Pattern: `{000001}_{id12}_{safe_subject}.eml`
/// Truncates subject first (down to 0); never drops counter or id fragment.
///
/// Path budget is measured in **UTF-16 code units** (Windows MAX_PATH semantics).
pub fn make_eml_pack_filename(
    index: u64,
    msg: &CanonicalMessage,
    abs_dir: &Path,
) -> Result<String, EmlWriteError> {
    make_eml_pack_filename_with_collision(index, msg, abs_dir, None)
}

/// Like [`make_eml_pack_filename`], optionally appending `-{n}` before `.eml` for collisions.
pub fn make_eml_pack_filename_with_collision(
    index: u64,
    msg: &CanonicalMessage,
    abs_dir: &Path,
    collision_n: Option<u32>,
) -> Result<String, EmlWriteError> {
    let id_frag = id_fragment(msg);
    let counter = format!("{index:06}");
    let subject_raw = msg.subject.as_deref().unwrap_or("");
    let safe_full = sanitize_filename_component(subject_raw);

    let abs_base = abs_path_len(abs_dir);
    let sep = 1usize; // '\'
    let coll_suffix = collision_n.map(|n| format!("-{n}")).unwrap_or_default();
    let coll_len = utf16_units(&coll_suffix);

    // Fixed: counter_id[.eml] or counter_id_subject[-N].eml
    // Pattern without subject: `{counter}_{id_frag}{coll}.eml`
    let fixed_no_subj = counter.len() + 1 + id_frag.len() + coll_len + 4; // .eml
                                                                          // With subject: `{counter}_{id_frag}_{subj}{coll}.eml` — one extra underscore
    let fixed_with_subj_overhead = fixed_no_subj + 1; // '_' before subject

    let mut subject_budget = ABS_PATH_BUDGET
        .saturating_sub(abs_base)
        .saturating_sub(sep)
        .saturating_sub(fixed_with_subj_overhead);

    for _ in 0..4 {
        let name = if subject_budget == 0 || safe_full.is_empty() {
            format!("{counter}_{id_frag}{coll_suffix}.eml")
        } else {
            let subj = truncate_chars(&safe_full, subject_budget);
            if subj.is_empty() {
                format!("{counter}_{id_frag}{coll_suffix}.eml")
            } else {
                format!("{counter}_{id_frag}_{subj}{coll_suffix}.eml")
            }
        };

        let full_len = abs_base + sep + utf16_units(&name);
        if full_len <= ABS_PATH_BUDGET {
            return Ok(name);
        }
        let over = full_len - ABS_PATH_BUDGET;
        if subject_budget == 0 {
            return Err(EmlWriteError::PathBudget(format!(
                "absolute path still exceeds {ABS_PATH_BUDGET} after subject truncation \
                 (utf16_len={full_len}, dir={})",
                abs_dir.display()
            )));
        }
        subject_budget = subject_budget.saturating_sub(over.max(1));
    }

    // Final attempt with empty subject.
    let name = format!("{counter}_{id_frag}{coll_suffix}.eml");
    let full_len = abs_base + sep + utf16_units(&name);
    if full_len <= ABS_PATH_BUDGET {
        Ok(name)
    } else {
        Err(EmlWriteError::PathBudget(format!(
            "absolute path exceeds {ABS_PATH_BUDGET} even with empty subject \
             (utf16_len={full_len}, dir={})",
            abs_dir.display()
        )))
    }
}

fn id_fragment(msg: &CanonicalMessage) -> String {
    if let Some(mih) = msg.edrm_mih_hex.as_deref().filter(|s| !s.is_empty()) {
        return mih.chars().take(12).collect();
    }
    let hex = hex_encode(&msg.content_hash);
    if hex.chars().any(|c| c != '0') {
        return hex.chars().take(12).collect();
    }
    format!("{:x}", msg.locus.nid)
}

fn sanitize_filename_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    // Avoid reserved DOS device names as the entire stem fragment.
    let upper = trimmed.to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6"
        | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6"
        | "LPT7" | "LPT8" | "LPT9" => format!("_{trimmed}"),
        _ => trimmed.to_string(),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    s.chars().take(max).collect()
}

/// UTF-16 code unit count (Windows path length semantics).
fn utf16_units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Absolute path length in UTF-16 code units (Windows MAX_PATH budget).
pub fn abs_path_len(path: &Path) -> usize {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    utf16_units(&abs.to_string_lossy())
}

// ─── Header sanitization ────────────────────────────────────────────────────

/// Strip CR/LF and other control characters from unstructured header values (M1).
pub fn sanitize_header_value(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

// ─── Manifest ───────────────────────────────────────────────────────────────

/// Root pack manifest (`eml_pack_v1`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmlPackManifest {
    pub schema: String,
    pub keep_set_schema: String,
    pub policy: String,
    pub family_policy: String,
    pub files_per_volume: u32,
    pub date_tz: String,
    pub created_from: EmlPackCreatedFrom,
    pub stats: EmlPackStats,
    pub messages: Vec<EmlPackMessageRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmlPackCreatedFrom {
    pub inputs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EmlPackStats {
    pub eml_written: u64,
    pub unique: u64,
    pub volumes: u64,
    pub materialize_failed: u64,
    pub attach_parts_written: u64,
    pub embedded_messages_written: u64,
    pub attach_parts_failed: u64,
    pub degraded_messages: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmlPackMessageRow {
    pub eml_relpath: String,
    pub source_path: String,
    pub folder: String,
    pub nid: u64,
    pub message_id_norm: Option<String>,
    pub edrm_mih: Option<String>,
    pub content_hash_hex: String,
    pub degraded: bool,
    pub degraded_reasons: Vec<String>,
    pub body_incomplete: bool,
    pub body_unavailable: bool,
    pub attachment_count: u64,
    pub attachments_file_written: u64,
    pub embedded_messages_written: u64,
    pub attachments_failed: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub embedded_message_unparsed: bool,
}

impl EmlPackManifest {
    pub fn new(
        policy: &str,
        family_policy: &str,
        files_per_volume: u32,
        inputs: Vec<String>,
    ) -> Self {
        Self {
            schema: EML_PACK_SCHEMA.to_string(),
            keep_set_schema: crate::keepset::KEEP_SET_SCHEMA.to_string(),
            policy: policy.to_string(),
            family_policy: family_policy.to_string(),
            files_per_volume,
            date_tz: "UTC".to_string(),
            created_from: EmlPackCreatedFrom { inputs },
            stats: EmlPackStats::default(),
            messages: Vec::new(),
        }
    }
}

/// Write manifest JSON (pretty) to `path`.
pub fn write_eml_pack_manifest(
    path: &Path,
    manifest: &EmlPackManifest,
) -> Result<(), EmlWriteError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let file = File::create(path)?;
    let mut wtr = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut wtr, manifest)
        .map_err(|e| EmlWriteError::Other(format!("manifest json: {e}")))?;
    wtr.write_all(b"\n")?;
    wtr.flush()?;
    Ok(())
}

/// Merge fidelity + pack write failures into honest degraded flags (M4).
pub fn merge_pack_degraded(
    fidelity_degraded: bool,
    fidelity_reasons: Vec<String>,
    write: &EmlWriteResult,
) -> (bool, Vec<String>) {
    let mut degraded = fidelity_degraded;
    let mut reasons = fidelity_reasons;
    if write.attachments_failed > 0 {
        degraded = true;
        if !reasons.iter().any(|r| r == REASON_ATTACH_PART_FAILED) {
            reasons.push(REASON_ATTACH_PART_FAILED.to_string());
        }
    }
    (degraded, reasons)
}

// ─── Writer ─────────────────────────────────────────────────────────────────

/// Write one canonical message as a MIME `.eml` file.
///
/// Attachment bytes are streamed via `attach_streams` (or in-memory `data` when present).
/// `parents_only` omits all attachment / embedded parts.
/// Soft attach failures skip the part (H1); no fake error body is written.
pub fn write_canonical_eml(
    out_path: &Path,
    msg: &CanonicalMessage,
    attach_streams: &mut dyn AttachStreamSource,
    opts: &EmlWriteOpts,
) -> Result<EmlWriteResult, EmlWriteError> {
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let file = File::create(out_path)?;
    let mut w = BufWriter::new(file);
    let result = write_canonical_eml_to(&mut w, msg, attach_streams, opts, 0)?;
    w.flush()?;
    Ok(result)
}

fn write_canonical_eml_to<W: Write>(
    w: &mut W,
    msg: &CanonicalMessage,
    attach_streams: &mut dyn AttachStreamSource,
    opts: &EmlWriteOpts,
    depth: u32,
) -> Result<EmlWriteResult, EmlWriteError> {
    let mut result = EmlWriteResult::default();

    // ── Headers ────────────────────────────────────────────────────────────
    write_headers(w, msg)?;

    let want_attaches = opts.family_policy == FamilyPolicy::KeepAttachmentsWithParent
        && !msg.attachments.is_empty();

    let has_plain = msg
        .body_plain
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let has_html = msg
        .body_html
        .as_ref()
        .map(|b| !b.is_empty())
        .unwrap_or(false);

    if want_attaches {
        // Prepare attach parts first so soft failures never emit empty mixed parts (H1).
        // If every attach fails, fall back to plain/alternative-only structure.
        let prepared = prepare_attachments(msg, attach_streams, opts, depth, &mut result)?;
        if prepared.is_empty() {
            write_body_only_structure(w, msg, has_plain, has_html)?;
        } else {
            let boundary = make_boundary("mixed", msg.locus.nid);
            write_crlf_line(w, "MIME-Version: 1.0")?;
            write_crlf_line(
                w,
                &format!("Content-Type: multipart/mixed; boundary=\"{boundary}\""),
            )?;
            write_crlf_blank(w)?;
            write_crlf_line(w, &format!("--{boundary}"))?;
            write_body_part(w, msg, has_plain, has_html)?;
            for part in prepared {
                write_crlf_line(w, &format!("--{boundary}"))?;
                write_prepared_part(w, part, &mut result)?;
            }
            write_crlf_line(w, &format!("--{boundary}--"))?;
        }
    } else {
        write_body_only_structure(w, msg, has_plain, has_html)?;
    }

    Ok(result)
}

fn write_body_only_structure<W: Write>(
    w: &mut W,
    msg: &CanonicalMessage,
    has_plain: bool,
    has_html: bool,
) -> Result<(), EmlWriteError> {
    if has_plain && has_html {
        let boundary = make_boundary("alt", msg.locus.nid);
        write_crlf_line(w, "MIME-Version: 1.0")?;
        write_crlf_line(
            w,
            &format!("Content-Type: multipart/alternative; boundary=\"{boundary}\""),
        )?;
        write_crlf_blank(w)?;
        write_alternative_parts(w, msg, &boundary)?;
        write_crlf_line(w, &format!("--{boundary}--"))?;
    } else if has_html && !has_plain {
        write_crlf_line(w, "MIME-Version: 1.0")?;
        write_crlf_line(w, "Content-Type: text/html; charset=UTF-8")?;
        write_crlf_line(w, "Content-Transfer-Encoding: 8bit")?;
        write_crlf_blank(w)?;
        if let Some(html) = &msg.body_html {
            write_html_body(w, html)?;
        }
    } else {
        // Plain only (or empty body with flags already in X-headers).
        write_crlf_line(w, "MIME-Version: 1.0")?;
        write_crlf_line(w, "Content-Type: text/plain; charset=UTF-8")?;
        write_crlf_line(w, "Content-Transfer-Encoding: 8bit")?;
        write_crlf_blank(w)?;
        if let Some(plain) = &msg.body_plain {
            write_plain_body(w, plain)?;
        }
    }
    Ok(())
}

fn write_headers<W: Write>(w: &mut W, msg: &CanonicalMessage) -> Result<(), EmlWriteError> {
    if let Some(mid) = msg.message_id.as_deref().filter(|s| !s.is_empty()) {
        let mid = sanitize_header_value(mid.trim());
        if mid.starts_with('<') && mid.ends_with('>') {
            write_crlf_line(w, &format!("Message-ID: {mid}"))?;
        } else {
            write_crlf_line(w, &format!("Message-ID: <{mid}>"))?;
        }
    }
    let subject = msg.subject.as_deref().unwrap_or("");
    write_crlf_line(w, &format!("Subject: {}", encode_header_value(subject)))?;
    if let Some(from) = msg.sender.as_deref().filter(|s| !s.is_empty()) {
        write_crlf_line(w, &format!("From: {}", sanitize_header_value(from)))?;
    }
    if let Some(to) = msg.display_to.as_deref().filter(|s| !s.is_empty()) {
        write_crlf_line(w, &format!("To: {}", sanitize_header_value(to)))?;
    }
    if let Some(cc) = msg.display_cc.as_deref().filter(|s| !s.is_empty()) {
        write_crlf_line(w, &format!("Cc: {}", sanitize_header_value(cc)))?;
    }
    if let Some(bcc) = msg.display_bcc.as_deref().filter(|s| !s.is_empty()) {
        write_crlf_line(w, &format!("Bcc: {}", sanitize_header_value(bcc)))?;
    }
    if let Some(ft) = msg.submit_time {
        if let Some(date) = format_date_utc_filetime(ft) {
            write_crlf_line(w, &format!("Date: {date}"))?;
        }
    }
    write_crlf_line(
        w,
        &format!(
            "X-Pst-Dedupe-Source: {}",
            sanitize_header_value(&msg.locus.source_path)
        ),
    )?;
    write_crlf_line(
        w,
        &format!(
            "X-Pst-Dedupe-Folder: {}",
            sanitize_header_value(&msg.locus.folder_path)
        ),
    )?;
    write_crlf_line(w, &format!("X-Pst-Dedupe-Nid: {:#x}", msg.locus.nid))?;
    if let Some(mih) = msg.edrm_mih_hex.as_deref().filter(|s| !s.is_empty()) {
        write_crlf_line(
            w,
            &format!("X-Pst-Dedupe-Edrm-Mih: {}", sanitize_header_value(mih)),
        )?;
    }
    if msg.fidelity.degraded {
        let reasons = msg
            .fidelity
            .degraded_reasons
            .iter()
            .map(|r| sanitize_header_value(r.as_str()))
            .collect::<Vec<_>>()
            .join(",");
        write_crlf_line(w, &format!("X-Pst-Dedupe-Degraded: {reasons}"))?;
    }
    write_crlf_line(
        w,
        &format!(
            "X-Pst-Dedupe-Body-Incomplete: {}",
            if msg.body_incomplete { "true" } else { "false" }
        ),
    )?;
    Ok(())
}

fn write_body_part<W: Write>(
    w: &mut W,
    msg: &CanonicalMessage,
    has_plain: bool,
    has_html: bool,
) -> Result<(), EmlWriteError> {
    if has_plain && has_html {
        let boundary = make_boundary("alt", msg.locus.nid.wrapping_add(1));
        write_crlf_line(
            w,
            &format!("Content-Type: multipart/alternative; boundary=\"{boundary}\""),
        )?;
        write_crlf_blank(w)?;
        write_alternative_parts(w, msg, &boundary)?;
        write_crlf_line(w, &format!("--{boundary}--"))?;
    } else if has_html {
        write_crlf_line(w, "Content-Type: text/html; charset=UTF-8")?;
        write_crlf_line(w, "Content-Transfer-Encoding: 8bit")?;
        write_crlf_blank(w)?;
        if let Some(html) = &msg.body_html {
            write_html_body(w, html)?;
        }
        write_crlf_blank(w)?;
    } else {
        write_crlf_line(w, "Content-Type: text/plain; charset=UTF-8")?;
        write_crlf_line(w, "Content-Transfer-Encoding: 8bit")?;
        write_crlf_blank(w)?;
        if let Some(plain) = &msg.body_plain {
            write_plain_body(w, plain)?;
        }
        write_crlf_blank(w)?;
    }
    Ok(())
}

fn write_alternative_parts<W: Write>(
    w: &mut W,
    msg: &CanonicalMessage,
    boundary: &str,
) -> Result<(), EmlWriteError> {
    // plain first (RFC 2046 recommendation)
    write_crlf_line(w, &format!("--{boundary}"))?;
    write_crlf_line(w, "Content-Type: text/plain; charset=UTF-8")?;
    write_crlf_line(w, "Content-Transfer-Encoding: 8bit")?;
    write_crlf_blank(w)?;
    if let Some(plain) = &msg.body_plain {
        write_plain_body(w, plain)?;
    }
    write_crlf_blank(w)?;
    write_crlf_line(w, &format!("--{boundary}"))?;
    write_crlf_line(w, "Content-Type: text/html; charset=UTF-8")?;
    write_crlf_line(w, "Content-Transfer-Encoding: 8bit")?;
    write_crlf_blank(w)?;
    if let Some(html) = &msg.body_html {
        write_html_body(w, html)?;
    }
    write_crlf_blank(w)?;
    Ok(())
}

// ─── Attachment prepare / write ─────────────────────────────────────────────

enum AttachBody {
    Memory(Vec<u8>),
    Stream(Box<dyn Read>),
}

struct PreparedPart {
    embedded: bool,
    filename: String,
    /// MIME type for non-embedded file parts (ignored for embedded → message/rfc822).
    mime: String,
    body: AttachBody,
    /// Embedded residual: labeled rfc822 but not recursively re-parsed as CanonicalMessage.
    unparsed: bool,
}

fn prepare_attachments(
    parent: &CanonicalMessage,
    attach_streams: &mut dyn AttachStreamSource,
    opts: &EmlWriteOpts,
    depth: u32,
    result: &mut EmlWriteResult,
) -> Result<Vec<PreparedPart>, EmlWriteError> {
    let mut out = Vec::new();
    for att in &parent.attachments {
        match prepare_one_attach(parent, att, attach_streams, opts, depth) {
            Ok(part) => out.push(part),
            Err(e) => {
                // Soft skip: no headers, no boundary, no fake body (H1).
                tracing_soft_attach_fail(parent, att, &e);
                result.attachments_failed += 1;
                // Embedded open failures still mark residual unparsed honesty flag.
                if is_embedded_message(att) {
                    result.embedded_message_unparsed = true;
                }
            }
        }
    }
    Ok(out)
}

fn prepare_one_attach(
    parent: &CanonicalMessage,
    att: &CanonicalAttachment,
    attach_streams: &mut dyn AttachStreamSource,
    opts: &EmlWriteOpts,
    depth: u32,
) -> Result<PreparedPart, EmlWriteError> {
    let embedded = is_embedded_message(att);
    let filename = if att.filename.is_empty() {
        if embedded {
            "embedded.eml".to_string()
        } else {
            "attachment.bin".to_string()
        }
    } else {
        sanitize_filename_component(&att.filename)
    };

    let body = open_attach_body(parent, att, attach_streams)?;

    if embedded {
        // Residual D-0067-embedded-depth: no full nested MAPI CanonicalMessage re-parse.
        // Always label message/rfc822 + raw 8bit; unparsed honesty flag.
        let _ = (opts, depth); // depth reserved for future recursive CanonicalMessage write
        Ok(PreparedPart {
            embedded: true,
            filename,
            mime: "message/rfc822".into(),
            body,
            unparsed: true,
        })
    } else {
        let mime = att
            .mime
            .as_deref()
            .filter(|m| !m.is_empty())
            .map(sanitize_header_value)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "application/octet-stream".into());
        let is_rfc822 = mime.to_ascii_lowercase().contains("message/rfc822");
        Ok(PreparedPart {
            embedded: is_rfc822,
            filename,
            mime: if is_rfc822 {
                "message/rfc822".into()
            } else {
                mime
            },
            body,
            unparsed: is_rfc822,
        })
    }
}

fn open_attach_body(
    parent: &CanonicalMessage,
    att: &CanonicalAttachment,
    attach_streams: &mut dyn AttachStreamSource,
) -> Result<AttachBody, EmlWriteError> {
    // Prefer in-memory small payload when present (tests / small probes).
    if let Some(data) = &att.data {
        return Ok(AttachBody::Memory(data.clone()));
    }
    if !att.stream_available {
        return Err(EmlWriteError::Other(
            "attachment stream not available".into(),
        ));
    }
    let nid = att.attach_nid.ok_or_else(|| {
        EmlWriteError::Other("attachment missing attach_nid for stream open".into())
    })?;
    let reader = attach_streams.open_attach(&parent.locus, nid)?;
    Ok(AttachBody::Stream(reader))
}

fn write_prepared_part<W: Write>(
    w: &mut W,
    part: PreparedPart,
    result: &mut EmlWriteResult,
) -> Result<(), EmlWriteError> {
    let filename_hdr = encode_header_value(&part.filename);

    if part.embedded {
        // RFC 2046: message/rfc822 body CTE is 7bit/8bit/binary only — never base64 (H2/H3).
        write_crlf_line(w, "Content-Type: message/rfc822")?;
        write_crlf_line(
            w,
            &format!("Content-Disposition: attachment; filename=\"{filename_hdr}\""),
        )?;
        write_crlf_line(w, "Content-Transfer-Encoding: 8bit")?;
        write_crlf_blank(w)?;
        stream_attach_raw(w, part.body)?;
        write_crlf_blank(w)?;
        result.embedded_messages_written += 1;
        if part.unparsed {
            result.embedded_message_unparsed = true;
        }
    } else {
        let ctype = sanitize_header_value(&part.mime);
        write_crlf_line(w, &format!("Content-Type: {ctype}"))?;
        write_crlf_line(
            w,
            &format!("Content-Disposition: attachment; filename=\"{filename_hdr}\""),
        )?;
        write_crlf_line(w, "Content-Transfer-Encoding: base64")?;
        write_crlf_blank(w)?;
        stream_attach_base64_body(w, part.body)?;
        write_crlf_blank(w)?;
        result.attachments_file_written += 1;
    }
    Ok(())
}

fn stream_attach_raw<W: Write>(w: &mut W, body: AttachBody) -> Result<(), EmlWriteError> {
    match body {
        AttachBody::Memory(data) => {
            w.write_all(&data)?;
        }
        AttachBody::Stream(mut reader) => {
            io::copy(&mut reader, w)?;
        }
    }
    Ok(())
}

fn stream_attach_base64_body<W: Write>(w: &mut W, body: AttachBody) -> Result<(), EmlWriteError> {
    match body {
        AttachBody::Memory(data) => {
            write!(w, "{}", base64_encode_chunked(&data))?;
        }
        AttachBody::Stream(mut reader) => {
            stream_base64_from_reader(w, &mut *reader)?;
        }
    }
    Ok(())
}

fn tracing_soft_attach_fail(
    parent: &CanonicalMessage,
    att: &CanonicalAttachment,
    err: &EmlWriteError,
) {
    // Soft path: pack writer skips the part. CLI materializer / host may log.
    let _ = (parent, att, err);
}

/// Stream base64 from a Read in 3-byte groups, wrapping lines at 76 chars (CRLF).
pub fn stream_base64_from_reader<W: Write, R: Read + ?Sized>(
    w: &mut W,
    reader: &mut R,
) -> Result<(), EmlWriteError> {
    let mut in_buf = [0u8; 3 * 1024]; // multiple of 3
    let mut carry = [0u8; 2];
    let mut carry_len = 0usize;
    let mut line_cols = 0usize;

    loop {
        let n = reader.read(&mut in_buf)?;
        if n == 0 {
            break;
        }
        // Combine carry + new bytes.
        let mut combined = Vec::with_capacity(carry_len + n);
        combined.extend_from_slice(&carry[..carry_len]);
        combined.extend_from_slice(&in_buf[..n]);
        let full_triples = combined.len() / 3;
        let rem = combined.len() % 3;
        let encode_len = full_triples * 3;
        write_base64_wrapped(w, &combined[..encode_len], &mut line_cols)?;
        carry[..rem].copy_from_slice(&combined[encode_len..]);
        carry_len = rem;
    }
    if carry_len > 0 {
        write_base64_wrapped(w, &carry[..carry_len], &mut line_cols)?;
    }
    if line_cols > 0 {
        write_crlf_blank(w)?;
    }
    Ok(())
}

fn write_base64_wrapped<W: Write>(
    w: &mut W,
    data: &[u8],
    line_cols: &mut usize,
) -> Result<(), EmlWriteError> {
    const LINE: usize = 76;
    let encoded = base64_encode(data);
    for ch in encoded.chars() {
        if *line_cols >= LINE {
            write_crlf_blank(w)?;
            *line_cols = 0;
        }
        write!(w, "{ch}")?;
        *line_cols += 1;
    }
    Ok(())
}

/// Encode bytes as base64 with CRLF line wrap at 76 (string form for small buffers).
pub fn base64_encode_chunked(data: &[u8]) -> String {
    let raw = base64_encode(data);
    let mut out = String::with_capacity(raw.len() + raw.len() / 76 * 2 + 2);
    for (i, chunk) in raw.as_bytes().chunks(76).enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        // SAFETY: base64 alphabet is ASCII
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
    }
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    out
}

pub(crate) fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        result.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

/// Embedded when attach_method == ATTACH_EMBEDDED_MSG (0x5) or MIME is message/rfc822.
fn is_embedded_message(att: &CanonicalAttachment) -> bool {
    if att.attach_method == Some(ATTACH_EMBEDDED_MSG) {
        return true;
    }
    att.mime
        .as_deref()
        .map(|m| m.to_ascii_lowercase().contains("message/rfc822"))
        .unwrap_or(false)
}

fn make_boundary(kind: &str, nid: u64) -> String {
    format!("----=_PstDedupe_{kind}_{nid:x}_")
}

fn encode_header_value(value: &str) -> String {
    let value = sanitize_header_value(value);
    if value.is_ascii() {
        value
    } else {
        let encoded = base64_encode(value.as_bytes());
        format!("=?UTF-8?B?{encoded}?=")
    }
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

// ─── Volume pack helper ─────────────────────────────────────────────────────

/// Tracks volume directories while writing a pack.
#[derive(Debug)]
pub struct VolumePackWriter {
    pub out_root: PathBuf,
    pub files_per_volume: u32,
    pub volume_prefix: String,
    pub next_index: u64, // 1-based export counter
    pub current_volume: u32,
    pub files_in_volume: u32,
    pub volumes_created: u64,
}

impl VolumePackWriter {
    /// Create a volume pack writer.
    ///
    /// `files_per_volume` is stored as `max(1, n)` — **no CLI clamp**. Call
    /// [`clamp_files_per_volume`] at the CLI flag boundary. Prefix must pass
    /// [`validate_volume_prefix`] (single safe path component).
    pub fn new(
        out_root: PathBuf,
        files_per_volume: u32,
        volume_prefix: String,
    ) -> Result<Self, EmlWriteError> {
        validate_volume_prefix(&volume_prefix)?;
        Ok(Self {
            out_root,
            files_per_volume: files_per_volume.max(1),
            volume_prefix,
            next_index: 1,
            current_volume: 0,
            files_in_volume: 0,
            volumes_created: 0,
        })
    }

    /// Test helper: same as [`Self::new`] but documents intent for small
    /// `files_per_volume` values used only in unit tests.
    pub fn new_for_test(
        out_root: PathBuf,
        files_per_volume: u32,
        volume_prefix: String,
    ) -> Result<Self, EmlWriteError> {
        Self::new(out_root, files_per_volume, volume_prefix)
    }

    /// Ensure current volume has capacity; create next volume dir if needed.
    /// Returns absolute path to the volume directory.
    pub fn ensure_volume_dir(&mut self) -> Result<PathBuf, EmlWriteError> {
        if self.current_volume == 0 || self.files_in_volume >= self.files_per_volume {
            self.current_volume += 1;
            self.files_in_volume = 0;
            self.volumes_created += 1;
            let name = volume_dirname(self.current_volume, &self.volume_prefix);
            let dir = self.out_root.join(&name);
            fs::create_dir_all(&dir)?;
            Ok(dir)
        } else {
            let name = volume_dirname(self.current_volume, &self.volume_prefix);
            Ok(self.out_root.join(name))
        }
    }

    /// Allocate next EML path under the appropriate volume. Returns (abs_path, relpath).
    ///
    /// Collision suffixes (`-2`, `-3`, …) re-budget subject truncation so the final
    /// absolute path stays within [`ABS_PATH_BUDGET`] (M5).
    pub fn next_eml_path(
        &mut self,
        msg: &CanonicalMessage,
    ) -> Result<(PathBuf, String), EmlWriteError> {
        let vol_dir = self.ensure_volume_dir()?;
        let mut collision_n: Option<u32> = None;
        let mut n = 2u32;
        let candidate = loop {
            let name =
                make_eml_pack_filename_with_collision(self.next_index, msg, &vol_dir, collision_n)?;
            if !vol_dir.join(&name).exists() {
                break name;
            }
            collision_n = Some(n);
            n += 1;
            if n > 10_000 {
                return Err(EmlWriteError::Other(format!(
                    "too many filename collisions for index {}",
                    self.next_index
                )));
            }
        };
        let abs = vol_dir.join(&candidate);
        // Final budget check (UTF-16).
        let full_len = abs_path_len(&abs);
        if full_len > ABS_PATH_BUDGET {
            return Err(EmlWriteError::PathBudget(format!(
                "absolute path exceeds {ABS_PATH_BUDGET} after collision suffix \
                 (utf16_len={full_len}, path={})",
                abs.display()
            )));
        }
        let vol_name = volume_dirname(self.current_volume, &self.volume_prefix);
        let rel = format!("{vol_name}/{candidate}");
        self.next_index += 1;
        self.files_in_volume += 1;
        Ok((abs, rel))
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::RecoverableIntegrity;
    use crate::keepset::MessageLocus;
    use std::io::Cursor;

    fn locus() -> MessageLocus {
        MessageLocus {
            source_path: r"C:\src\a.pst".into(),
            source_pst: "a.pst".into(),
            folder_path: "/Inbox".into(),
            nid: 0x21,
            is_orphaned: false,
        }
    }

    fn base_msg() -> CanonicalMessage {
        CanonicalMessage {
            locus: locus(),
            message_id: Some("mid@test".into()),
            subject: Some("Hello".into()),
            sender: Some("a@b.com".into()),
            display_to: Some("c@d.com".into()),
            display_cc: None,
            display_bcc: None,
            // 2006-01-02 15:04:05 UTC as FILETIME
            submit_time: Some({
                let unix = Utc
                    .with_ymd_and_hms(2006, 1, 2, 15, 4, 5)
                    .single()
                    .expect("fixed dt")
                    .timestamp();
                unix_to_filetime(unix)
            }),
            size: Some(100),
            message_class: None,
            body_plain: Some("plain body".into()),
            body_html: None,
            attachments: vec![],
            fidelity: RecoverableIntegrity::clean(),
            message_id_norm: Some("mid@test".into()),
            content_hash: [0xab; 32],
            edrm_mih_hex: Some("deadbeefcafebabe".into()),
            body_incomplete: false,
            body_unavailable: false,
        }
    }

    fn unix_to_filetime(unix: i64) -> i64 {
        (unix + 11_644_473_600) * 10_000_000
    }

    struct MapAttachSource {
        map: std::collections::HashMap<u64, Vec<u8>>,
    }

    impl AttachStreamSource for MapAttachSource {
        fn open_attach(
            &mut self,
            _parent: &MessageLocus,
            attach_nid: u64,
        ) -> Result<Box<dyn Read>, EmlWriteError> {
            let data = self
                .map
                .get(&attach_nid)
                .cloned()
                .ok_or_else(|| EmlWriteError::Other(format!("missing {attach_nid}")))?;
            Ok(Box::new(Cursor::new(data)))
        }
    }

    #[test]
    fn date_utc_fixed_filetime_plus0000() {
        // Build FILETIME from a known UTC instant so the test is TZ-independent.
        let unix = Utc
            .with_ymd_and_hms(2006, 1, 2, 15, 4, 5)
            .single()
            .expect("fixed dt")
            .timestamp();
        let ft = unix_to_filetime(unix);
        let s = format_date_utc_filetime(ft).expect("date");
        assert!(s.ends_with(" +0000"), "must end with +0000, got {s}");
        assert_eq!(s, "Mon, 02 Jan 2006 15:04:05 +0000");
    }

    #[test]
    fn date_unix_epoch() {
        let s = format_date_utc_unix(0).expect("date");
        assert_eq!(s, "Thu, 01 Jan 1970 00:00:00 +0000");
    }

    #[test]
    fn plain_structure() {
        let msg = base_msg();
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("Content-Type: text/plain; charset=UTF-8"));
        assert!(s.contains("plain body"));
        assert!(s.contains("Date: Mon, 02 Jan 2006 15:04:05 +0000"));
        assert!(!s.contains("multipart/"));
    }

    #[test]
    fn alternative_structure() {
        let mut msg = base_msg();
        msg.body_html = Some(b"<p>hi</p>".to_vec());
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("multipart/alternative"));
        assert!(s.contains("text/plain"));
        assert!(s.contains("text/html"));
        assert!(s.contains("<p>hi</p>"));
    }

    #[test]
    fn mixed_with_file_attach_base64() {
        let mut msg = base_msg();
        msg.attachments.push(CanonicalAttachment {
            filename: "note.txt".into(),
            size: 5,
            mime: Some("text/plain".into()),
            data: Some(b"Hello".to_vec()),
            stream_available: true,
            attach_nid: Some(100),
            attach_method: Some(1), // by value
        });
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        let res = write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("multipart/mixed"));
        assert!(s.contains("Content-Transfer-Encoding: base64"));
        assert!(s.contains("SGVsbG8=")); // Hello
        assert_eq!(res.attachments_file_written, 1);
        assert_eq!(res.embedded_messages_written, 0);
    }

    #[test]
    fn embedded_message_rfc822_not_octet_stream() {
        let mut msg = base_msg();
        msg.attachments.push(CanonicalAttachment {
            filename: "nested.eml".into(),
            size: 20,
            mime: None,
            data: Some(b"From: x\r\n\r\nbody".to_vec()),
            stream_available: true,
            attach_nid: Some(200),
            attach_method: Some(ATTACH_EMBEDDED_MSG),
        });
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        let res = write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert!(
            s.contains("Content-Type: message/rfc822"),
            "must be message/rfc822, got:\n{s}"
        );
        // Must not use base64 CTE on message/rfc822 (H2).
        assert!(
            !part_has_base64_cte_for_rfc822(&s),
            "message/rfc822 must not use base64 CTE:\n{s}"
        );
        assert!(s.contains("Content-Transfer-Encoding: 8bit"));
        assert!(s.contains("From: x"));
        assert_eq!(res.embedded_messages_written, 1);
        assert!(res.embedded_message_unparsed);
        assert_eq!(res.attachments_file_written, 0);
    }

    /// True if any message/rfc822 part block advertises base64 CTE.
    fn part_has_base64_cte_for_rfc822(eml: &str) -> bool {
        // Scan part headers roughly: after Content-Type: message/rfc822, before blank line.
        let bytes = eml.as_bytes();
        let mut i = 0;
        while let Some(rel) = eml[i..].find("Content-Type: message/rfc822") {
            let start = i + rel;
            // Find end of headers (CRLF CRLF or LF LF)
            let rest = &eml[start..];
            let header_end = rest
                .find("\r\n\r\n")
                .or_else(|| rest.find("\n\n"))
                .unwrap_or(rest.len());
            let headers = &rest[..header_end];
            if headers
                .to_ascii_lowercase()
                .contains("content-transfer-encoding: base64")
            {
                return true;
            }
            i = start + header_end + 1;
            if i >= bytes.len() {
                break;
            }
        }
        false
    }

    #[test]
    fn soft_attach_fail_skips_part_no_fake_body() {
        let mut msg = base_msg();
        msg.attachments.push(CanonicalAttachment {
            filename: "missing.bin".into(),
            size: 10,
            mime: Some("application/octet-stream".into()),
            data: None,
            stream_available: true,
            attach_nid: Some(999),
            attach_method: Some(1),
        });
        let mut src = NullAttachStreamSource; // open will fail
        let mut buf = Vec::new();
        let res = write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert_eq!(res.attachments_failed, 1);
        assert_eq!(res.attachments_file_written, 0);
        // No fake error payload.
        assert!(
            !s.contains("attach open failed"),
            "must not write fake error body:\n{s}"
        );
        assert!(!s.contains("missing.bin"));
        // All attaches failed → not multipart/mixed with empty attach.
        assert!(
            !s.contains("multipart/mixed"),
            "all-fail should not force mixed:\n{s}"
        );
        assert!(s.contains("plain body"));
    }

    #[test]
    fn soft_fail_one_attach_keeps_other() {
        let mut msg = base_msg();
        msg.attachments.push(CanonicalAttachment {
            filename: "good.txt".into(),
            size: 5,
            mime: Some("text/plain".into()),
            data: Some(b"Hello".to_vec()),
            stream_available: true,
            attach_nid: Some(1),
            attach_method: Some(1),
        });
        msg.attachments.push(CanonicalAttachment {
            filename: "bad.bin".into(),
            size: 1,
            mime: Some("application/octet-stream".into()),
            data: None,
            stream_available: true,
            attach_nid: Some(2),
            attach_method: Some(1),
        });
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        let res = write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert_eq!(res.attachments_file_written, 1);
        assert_eq!(res.attachments_failed, 1);
        assert!(s.contains("multipart/mixed"));
        assert!(s.contains("good.txt"));
        assert!(!s.contains("bad.bin"));
        assert!(!s.contains("attach open failed"));
        assert!(s.contains("SGVsbG8="));
    }

    #[test]
    fn crlf_after_headers() {
        let msg = base_msg();
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        // Subject line should end with CRLF
        let s = String::from_utf8_lossy(&buf);
        assert!(
            s.contains("Subject: Hello\r\n"),
            "headers must use CRLF:\n{s:?}"
        );
        assert!(
            s.contains("MIME-Version: 1.0\r\n"),
            "MIME-Version must use CRLF"
        );
        // Header/body separator is blank CRLF line
        assert!(
            buf.windows(4).any(|w| w == b"\r\n\r\n"),
            "must have CRLF blank line between headers and body"
        );
    }

    #[test]
    fn header_sanitize_strips_crlf() {
        let mut msg = base_msg();
        msg.sender = Some("evil\r\nBcc: attacker@x.com".into());
        msg.subject = Some("sub\r\nX-Injected: yes".into());
        msg.locus.folder_path = "/Inbox\r\nX-Bad: 1".into();
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        // Control chars stripped → no header injection (no CRLF-split new header lines).
        assert!(!s.contains("\r\nBcc:"));
        assert!(!s.contains("\nBcc:"));
        assert!(!s.contains("\r\nX-Injected:"));
        assert!(!s.contains("\r\nX-Bad:"));
        assert!(s.contains("From: evilBcc: attacker@x.com"));
        assert!(s.contains("Subject: subX-Injected: yes"));
        assert!(s.contains("X-Pst-Dedupe-Folder: /InboxX-Bad: 1"));
    }

    #[test]
    fn merge_pack_degraded_on_attach_fail() {
        let wres = EmlWriteResult {
            attachments_failed: 2,
            ..Default::default()
        };
        let (deg, reasons) = merge_pack_degraded(false, vec![], &wres);
        assert!(deg);
        assert!(reasons.iter().any(|r| r == REASON_ATTACH_PART_FAILED));
    }

    #[test]
    fn parents_only_omits_attach_parts() {
        let mut msg = base_msg();
        msg.attachments.push(CanonicalAttachment {
            filename: "a.bin".into(),
            size: 3,
            mime: Some("application/octet-stream".into()),
            data: Some(vec![1, 2, 3]),
            stream_available: true,
            attach_nid: Some(1),
            attach_method: Some(1),
        });
        let mut src = NullAttachStreamSource;
        let opts = EmlWriteOpts {
            family_policy: FamilyPolicy::ParentsOnly,
            max_embedded_depth: 3,
        };
        let mut buf = Vec::new();
        let res = write_canonical_eml_to(&mut buf, &msg, &mut src, &opts, 0).expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert!(!s.contains("multipart/mixed"));
        assert!(!s.contains("a.bin"));
        assert_eq!(res.attachments_file_written, 0);
        assert_eq!(res.embedded_messages_written, 0);
    }

    /// Production path: CLI uses the same `VolumePackWriter::new` / `next_eml_path` /
    /// `ensure_volume_dir` API. `new_for_test` is a thin alias so unit tests can set
    /// `files_per_volume=2` without the CLI clamp [1000, 50000].
    #[test]
    fn production_volume_pack_writer_rollover_via_next_eml_path_fpv2() {
        let dir = tempfile::tempdir().expect("tmp");
        // Writer accepts fpv=2 (clamp is CLI-only); new_for_test → Self::new.
        let mut pack = VolumePackWriter::new_for_test(dir.path().to_path_buf(), 2, "VOL".into())
            .expect("pack");
        let msg = base_msg();
        let mut paths = Vec::new();
        for _ in 0..3 {
            let (abs, rel) = pack.next_eml_path(&msg).expect("path");
            File::create(&abs).expect("touch");
            paths.push(rel);
        }
        assert!(paths[0].starts_with("VOL001/"));
        assert!(paths[1].starts_with("VOL001/"));
        assert!(paths[2].starts_with("VOL002/"));
        assert_eq!(pack.volumes_created, 2);
        assert!(dir.path().join("VOL001").is_dir());
        assert!(dir.path().join("VOL002").is_dir());
    }

    #[test]
    fn deterministic_relpaths_same_order() {
        let dir = tempfile::tempdir().expect("tmp");
        let dir_a = dir.path().join("det_a");
        let dir_b = dir.path().join("det_b");
        fs::create_dir_all(&dir_a).expect("mkdir a");
        fs::create_dir_all(&dir_b).expect("mkdir b");
        let mut pa = VolumePackWriter::new(dir_a, 1000, "VOL".into()).expect("pa");
        let mut pb = VolumePackWriter::new(dir_b, 1000, "VOL".into()).expect("pb");
        let mut rels1 = Vec::new();
        let mut rels2 = Vec::new();
        for i in 0..3u64 {
            let mut m = base_msg();
            m.subject = Some(format!("Subj{i}"));
            m.content_hash = [i as u8; 32];
            m.edrm_mih_hex = Some(format!("{:016x}", i * 0x1111));
            let (abs_a, r_a) = pa.next_eml_path(&m).expect("a");
            File::create(&abs_a).expect("touch a");
            let (abs_b, r_b) = pb.next_eml_path(&m).expect("b");
            File::create(&abs_b).expect("touch b");
            rels1.push(r_a);
            rels2.push(r_b);
        }
        assert_eq!(rels1, rels2);
        assert!(rels1[0].ends_with(".eml"));
        assert_ne!(rels1[0], rels1[1]);
    }

    #[test]
    fn path_budget_deep_out_long_subject() {
        let deep = PathBuf::from(
            r"C:\Users\operator\Documents\ClientMatter\Case2026\Export\UniqueEml\Batch01\Run",
        );
        // Ensure deep path is already long
        assert!(abs_path_len(&deep) > 50);
        let mut msg = base_msg();
        msg.subject = Some("A".repeat(200));
        let name = make_eml_pack_filename(1, &msg, &deep).expect("name");
        assert!(name.starts_with("000001_"));
        assert!(name.ends_with(".eml"));
        // counter + id fragment must remain
        assert!(name.contains("deadbeefcafe") || name.contains("abababababab"));
        let full = deep.join(&name);
        let len = abs_path_len(&full);
        assert!(
            len <= ABS_PATH_BUDGET,
            "abs path len {len} > {ABS_PATH_BUDGET}: {}",
            full.display()
        );
    }

    #[test]
    fn collision_suffix_rebudgets() {
        let dir = tempfile::tempdir().expect("tmp");
        let vol = dir.path();
        let mut msg = base_msg();
        msg.subject = Some("A".repeat(200));
        let base = make_eml_pack_filename(1, &msg, vol).expect("base");
        // Touch base so collision path is taken
        File::create(vol.join(&base)).expect("touch");
        let mut pack = VolumePackWriter::new(vol.to_path_buf(), 1000, "VOL".into()).expect("pack");
        // Force volume dir = out_root/VOL001; seed collision inside that volume.
        let (abs, rel) = pack.next_eml_path(&msg).expect("path");
        assert!(abs_path_len(&abs) <= ABS_PATH_BUDGET);
        assert!(rel.ends_with(".eml"));
        // Create collision in volume and ensure next gets -2 within budget
        File::create(&abs).expect("touch2");
        let mut msg2 = msg.clone();
        msg2.subject = Some("A".repeat(200));
        // Same index counter will advance; for same subject different index — just check -n path
        let name_coll = make_eml_pack_filename_with_collision(1, &msg, vol, Some(2)).expect("coll");
        assert!(name_coll.contains("-2"));
        let full = vol.join(&name_coll);
        assert!(abs_path_len(&full) <= ABS_PATH_BUDGET);
    }

    #[test]
    fn utf16_path_budget_counts_surrogate_pairs() {
        // U+10000 is one Unicode scalar but two UTF-16 code units.
        let s = "\u{10000}";
        assert_eq!(s.chars().count(), 1);
        assert_eq!(utf16_units(s), 2);
        // Absolute path built only from that char (Windows drive-less absolute is rare;
        // measure the string used by budget math directly).
        let path_str = format!(r"C:\{s}");
        assert_eq!(utf16_units(&path_str), 3 + 2); // C:\ + 2 units
        assert!(utf16_units(&path_str) > path_str.chars().count());
    }

    #[test]
    fn base64_encode_correctness() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(base64_encode(b"Hi"), "SGk=");
        assert_eq!(base64_encode(b"Hey"), "SGV5");
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn stream_base64_matches_batch() {
        let data: Vec<u8> = (0u8..200).collect();
        let mut out = Vec::new();
        stream_base64_from_reader(&mut out, &mut Cursor::new(&data)).expect("stream");
        let streamed = String::from_utf8_lossy(&out);
        let streamed_flat: String = streamed
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
        assert_eq!(streamed_flat, base64_encode(&data));
        // CRLF line wraps for long output
        assert!(out.windows(2).any(|w| w == b"\r\n"));
    }

    #[test]
    fn volume_dirname_padding() {
        assert_eq!(volume_dirname(1, "VOL"), "VOL001");
        assert_eq!(volume_dirname(12, "VOL"), "VOL012");
        assert_eq!(volume_dirname(999, "VOL"), "VOL999");
        assert_eq!(volume_dirname(1000, "VOL"), "VOL1000");
    }

    #[test]
    fn clamp_files_per_volume_bounds() {
        assert_eq!(clamp_files_per_volume(2), FILES_PER_VOLUME_MIN);
        assert_eq!(clamp_files_per_volume(10_000), 10_000);
        assert_eq!(clamp_files_per_volume(100_000), FILES_PER_VOLUME_MAX);
    }

    #[test]
    fn stream_source_used_when_no_inline_data() {
        let mut msg = base_msg();
        msg.attachments.push(CanonicalAttachment {
            filename: "x.bin".into(),
            size: 5,
            mime: Some("application/octet-stream".into()),
            data: None,
            stream_available: true,
            attach_nid: Some(42),
            attach_method: Some(1),
        });
        let mut src = MapAttachSource {
            map: [(42, b"Hello".to_vec())].into_iter().collect(),
        };
        let mut buf = Vec::new();
        let res = write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("SGVsbG8="));
        assert_eq!(res.attachments_file_written, 1);
    }

    #[test]
    fn filename_keeps_counter_and_hash() {
        let msg = base_msg();
        let dir = PathBuf::from(r"C:\out");
        let name = make_eml_pack_filename(42, &msg, &dir).expect("name");
        assert!(name.starts_with("000042_"));
        assert!(name.contains("deadbeefcafe"));
        assert!(name.ends_with(".eml"));
    }

    #[test]
    fn embedded_soft_fail_skips_no_fake_rfc822_body() {
        let mut msg = base_msg();
        msg.attachments.push(CanonicalAttachment {
            filename: "nested.eml".into(),
            size: 0,
            mime: None,
            data: None,
            stream_available: true,
            attach_nid: Some(55),
            attach_method: Some(ATTACH_EMBEDDED_MSG),
        });
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        let res = write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert_eq!(res.attachments_failed, 1);
        assert_eq!(res.embedded_messages_written, 0);
        // Honesty: soft-failed embedded still sets residual unparsed flag.
        assert!(
            res.embedded_message_unparsed,
            "failed embedded must set embedded_message_unparsed"
        );
        assert!(!s.contains("message/rfc822"));
        assert!(!s.contains("attach open failed"));
    }

    #[test]
    fn volume_prefix_rejects_path_traversal() {
        for bad in [
            r"..\escape",
            "../escape",
            r"C:\x",
            "C:x",
            "VOL/../x",
            "VOL\\x",
            ".",
            "..",
            "vol name",
            "vol\0x",
            "",
            "vol.dot",
        ] {
            assert!(
                validate_volume_prefix(bad).is_err(),
                "expected reject for {bad:?}"
            );
            assert!(
                VolumePackWriter::new(PathBuf::from(r"C:\out"), 1000, bad.into()).is_err(),
                "VolumePackWriter must reject {bad:?}"
            );
        }
        assert!(validate_volume_prefix("VOL").is_ok());
        assert!(validate_volume_prefix("pack_01").is_ok());
        assert!(validate_volume_prefix("A-B").is_ok());
    }

    #[test]
    fn volume_prefix_malicious_stays_under_out_root() {
        let dir = tempfile::tempdir().expect("tmp");
        let out_root = dir.path().to_path_buf();
        // Malicious prefixes must fail construction — never create dirs outside out_root.
        for bad in [r"..\escape", r"C:\x", "../x", r"..\..\windows"] {
            let err = VolumePackWriter::new(out_root.clone(), 2, bad.into());
            assert!(err.is_err(), "must reject {bad}");
        }
        let mut pack =
            VolumePackWriter::new(out_root.clone(), 2, "VOL".into()).expect("safe prefix");
        let msg = base_msg();
        let (abs, rel) = pack.next_eml_path(&msg).expect("path");
        assert!(
            abs.starts_with(&out_root),
            "volume path must stay under out_root: {} vs {}",
            abs.display(),
            out_root.display()
        );
        assert!(rel.starts_with("VOL001/"));
        // No escape directory created beside out_root.
        assert!(!dir.path().parent().unwrap().join("escape").exists());
    }

    #[test]
    fn body_text_crlf_normalization() {
        assert_eq!(normalize_text_body_crlf("a\nb"), "a\r\nb\r\n");
        assert_eq!(normalize_text_body_crlf("a\rb"), "a\r\nb\r\n");
        assert_eq!(normalize_text_body_crlf("a\r\nb"), "a\r\nb\r\n");
        assert_eq!(normalize_text_body_crlf("a\r\n"), "a\r\n");
        assert_eq!(normalize_text_body_crlf("plain"), "plain\r\n");
        assert_eq!(normalize_text_body_crlf(""), "");
        // No double-convert existing CRLF sequences.
        assert_eq!(normalize_text_body_crlf("a\r\n\r\nb"), "a\r\n\r\nb\r\n");

        let mut msg = base_msg();
        msg.body_plain = Some("line1\nline2".into());
        let mut src = NullAttachStreamSource;
        let mut buf = Vec::new();
        write_canonical_eml_to(&mut buf, &msg, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write");
        let s = String::from_utf8_lossy(&buf);
        assert!(
            s.contains("line1\r\nline2\r\n"),
            "plain body must use CRLF:\n{s:?}"
        );
        // Must not leave bare LF without CR in the body region.
        let body_idx = s.find("line1").expect("body");
        let body = &s[body_idx..];
        let bare_lf = body
            .as_bytes()
            .windows(2)
            .any(|w| w[0] != b'\r' && w[1] == b'\n');
        assert!(!bare_lf, "no bare LF in body region:\n{body:?}");

        let mut msg_html = base_msg();
        msg_html.body_plain = None;
        msg_html.body_html = Some(b"<p>a\nb</p>".to_vec());
        let mut buf2 = Vec::new();
        write_canonical_eml_to(&mut buf2, &msg_html, &mut src, &EmlWriteOpts::default(), 0)
            .expect("write html");
        let s2 = String::from_utf8_lossy(&buf2);
        assert!(
            s2.contains("<p>a\r\nb</p>\r\n"),
            "html body must use CRLF:\n{s2:?}"
        );
    }

    #[test]
    fn export_counters_follow_feed_order() {
        // Counters are assigned by VolumePackWriter feed order. unique-eml must
        // feed winners in keep_set.winners order (path+nid sort). This unit test
        // proves counter assignment tracks the sequence of next_eml_path calls.
        let dir = tempfile::tempdir().expect("tmp");
        let mut pack =
            VolumePackWriter::new(dir.path().to_path_buf(), 1000, "VOL".into()).expect("pack");
        let mut msgs = Vec::new();
        // Intentionally reverse path order relative to natural nid order.
        for (path, nid) in [("C:/z.pst", 1u64), ("C:/a.pst", 2u64)] {
            let mut m = base_msg();
            m.locus.source_path = path.into();
            m.locus.nid = nid;
            m.subject = Some(format!("n{nid}"));
            m.content_hash = [nid as u8; 32];
            m.edrm_mih_hex = Some(format!("{:012x}", nid));
            msgs.push(m);
        }
        // Keep-set sort would order a.pst before z.pst — feed in that order.
        msgs.sort_by(|a, b| {
            a.locus
                .source_path
                .cmp(&b.locus.source_path)
                .then(a.locus.nid.cmp(&b.locus.nid))
        });
        let mut rels = Vec::new();
        for m in &msgs {
            let (abs, rel) = pack.next_eml_path(m).expect("path");
            File::create(&abs).expect("touch");
            rels.push(rel);
        }
        assert!(
            rels[0].starts_with("VOL001/000001_"),
            "first keep-set winner → counter 1: {}",
            rels[0]
        );
        assert!(
            rels[1].starts_with("VOL001/000002_"),
            "second keep-set winner → counter 2: {}",
            rels[1]
        );
        // path order: a.pst (nid 2) first
        assert!(msgs[0].locus.source_path.contains("a.pst"));
        assert!(msgs[1].locus.source_path.contains("z.pst"));
    }
}
