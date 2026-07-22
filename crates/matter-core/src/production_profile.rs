//! Named production profiles (track **0060**).
//!
//! Profiles are **config** (load-file dialect, field map, Bates prefix/pad, layout,
//! packaging, bound QC pack) — not a produce fork. Built-ins live in code; matter-local
//! user profiles are stored in `production_profiles` (schema v38).
//!
//! **Not legal advice:** built-ins are technical packaging templates; operators must
//! validate against the actual ESI protocol.
//!
//! Bates **start number is job-time only** and is rejected if present in a profile body.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, Matter};
use crate::AuditEventInput;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Profile body document version accepted by this crate.
pub const PRODUCTION_PROFILE_BODY_VERSION: u32 = 1;

/// Maximum serialized body size (bytes).
pub const PRODUCTION_PROFILE_BODY_MAX_BYTES: usize = 256 * 1024;

/// Default production profile slug (0040 packaging defaults + US date formats).
pub const BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1: &str = "us_concordance_native_text_v1";
/// Concordance delimiters + Relativity-oriented header aliases.
pub const BUILTIN_US_CONCORDANCE_REL_ALIAS_V1: &str = "us_concordance_rel_alias_v1";
/// Same packaging as default + strict privilege QC pack.
pub const BUILTIN_US_STRICT_QC_CONCORDANCE_V1: &str = "us_strict_qc_concordance_v1";

/// Reserved built-in slugs (user cannot upsert these).
pub const RESERVED_PRODUCTION_PROFILE_SLUGS: &[&str] = &[
    BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1,
    BUILTIN_US_CONCORDANCE_REL_ALIAS_V1,
    BUILTIN_US_STRICT_QC_CONCORDANCE_V1,
];

const BUILTIN_ID_PREFIX: &str = "builtin:";

/// Canonical engine load-file source fields (0040 DAT order).
pub const CANONICAL_DAT_SOURCES: &[&str] = &[
    "BEGBATES",
    "ENDBATES",
    "CONTROL_NUMBER",
    "ITEM_ID",
    "PARENT_ITEM_ID",
    "FAMILY_ID",
    "CUSTODIAN",
    "FILE_NAME",
    "FILE_EXT",
    "FILE_CATEGORY",
    "MIME_TYPE",
    "FILE_SIZE",
    "SHA256",
    "DATE_SENT",
    "DATE_RECEIVED",
    "DATE_CREATED",
    "FROM",
    "TO",
    "CC",
    "BCC",
    "SUBJECT",
    "NATIVE_PATH",
    "TEXT_PATH",
    "HAS_REDACTED_TEXT",
    "WITHHELD",
    "PROD_STATUS",
];

/// Privilege / work-product fields never allowed in field maps (fail closed).
pub const FORBIDDEN_FIELD_SOURCES: &[&str] = &[
    "PRIVILEGE_DESCRIPTION",
    "PRIVILEGE_BASIS",
    "PRIVILEGE_BASIS_NARRATIVE",
    "BASIS_DESCRIPTION",
    "PRIVILEGE_NOTES",
    "NOTES",
    "NOTE_BODY",
    "HIGHLIGHT_QUOTE",
    "WORK_PRODUCT",
    "ATTORNEY_NOTES",
    "REVIEW_NOTES",
];

/// QC pack: current 0041 defaults (alias `default_production_qc_v1` accepted).
pub const QC_PACK_DEFAULT_V1: &str = "qc_default_v1";
/// QC pack: withheld/family incompleteness escalated to Error.
pub const QC_PACK_STRICT_PRIVILEGE_V1: &str = "qc_strict_privilege_v1";
/// QC pack: missing native / zero-size as Error; softer missing text for binaries.
pub const QC_PACK_NATIVE_HEAVY_V1: &str = "qc_native_heavy_v1";
/// Legacy 0041 profile string (maps to [`QC_PACK_DEFAULT_V1`]).
pub const QC_PACK_LEGACY_DEFAULT: &str = "default_production_qc_v1";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One field-map entry: engine source → load-file header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldMapEntry {
    /// Canonical engine field id (e.g. `BEGBATES`, `DATE_SENT`).
    pub source: String,
    /// Header written to the load file.
    pub header: String,
    /// When false, column is omitted.
    #[serde(default = "default_true")]
    pub include: bool,
    /// Optional chrono strftime for datetime sources (e.g. `%m/%d/%Y`).
    #[serde(default)]
    pub date_format: Option<String>,
    /// Optional IANA timezone for datetime conversion before format.
    #[serde(default)]
    pub timezone: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Load-file section of a production profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadFileConfig {
    /// Dialect id: `concordance_dat_v1` | `relativity_field_alias_v1` | …
    #[serde(default = "default_dialect")]
    pub dialect: String,
    /// Encoding label (P0: `utf-8` only).
    #[serde(default = "default_encoding")]
    pub encoding: String,
    /// Ordered field map.
    pub field_map: Vec<FieldMapEntry>,
}

fn default_dialect() -> String {
    "concordance_dat_v1".into()
}

fn default_encoding() -> String {
    "utf-8".into()
}

/// Bates / control numbering config (**no start number**).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatesConfig {
    #[serde(default = "default_bates_prefix")]
    pub prefix: String,
    #[serde(default = "default_pad_width")]
    pub pad_width: u32,
    /// `name_by_bates` → all artifacts share Bates stem (native + text).
    #[serde(default = "default_filename_mode")]
    pub filename_mode: String,
}

fn default_bates_prefix() -> String {
    "PROD".into()
}

fn default_pad_width() -> u32 {
    6
}

fn default_filename_mode() -> String {
    "name_by_bates".into()
}

/// Volume folder layout names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutConfig {
    #[serde(default = "default_data_dir")]
    pub data: String,
    #[serde(default = "default_natives_dir")]
    pub natives: String,
    #[serde(default = "default_text_dir")]
    pub text: String,
}

fn default_data_dir() -> String {
    "DATA".into()
}

fn default_natives_dir() -> String {
    "NATIVES".into()
}

fn default_text_dir() -> String {
    "TEXT".into()
}

/// Packaging toggles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackagingConfig {
    #[serde(default = "default_true")]
    pub include_csv_twin: bool,
    #[serde(default = "default_true")]
    pub export_eml_if_missing_native: bool,
    #[serde(default)]
    pub expand_family: bool,
}

impl Default for PackagingConfig {
    fn default() -> Self {
        Self {
            include_csv_twin: true,
            export_eml_if_missing_native: true,
            expand_family: false,
        }
    }
}

/// Bound QC pack + defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionQcConfig {
    #[serde(default = "default_qc_pack")]
    pub pack_id: String,
    #[serde(default = "default_true")]
    pub require_qc_pass: bool,
}

fn default_qc_pack() -> String {
    QC_PACK_DEFAULT_V1.into()
}

impl Default for ProductionQcConfig {
    fn default() -> Self {
        Self {
            pack_id: default_qc_pack(),
            require_qc_pass: true,
        }
    }
}

/// Versioned production profile body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionProfileBody {
    pub version: u32,
    pub load_file: LoadFileConfig,
    pub bates: BatesConfig,
    pub layout: LayoutConfig,
    #[serde(default)]
    pub packaging: PackagingConfig,
    #[serde(default)]
    pub qc: ProductionQcConfig,
}

/// DTO for a built-in or matter-local production profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionProfile {
    /// `builtin:us_concordance_native_text_v1` or user id (`ppr_…`).
    pub id: String,
    /// `None` for built-ins.
    pub matter_id: Option<String>,
    pub slug: String,
    pub label: String,
    pub jurisdiction_tag: Option<String>,
    pub body: ProductionProfileBody,
    pub is_builtin: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Input for [`Matter::upsert_production_profile`].
#[derive(Debug, Clone)]
pub struct ProductionProfileInput {
    pub id: Option<String>,
    pub slug: String,
    pub label: String,
    pub jurisdiction_tag: Option<String>,
    /// Raw body JSON (validated + normalized).
    pub body_json: String,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Built-in profile id for a reserved slug (`builtin:us_…`).
pub fn production_builtin_id(slug: &str) -> String {
    format!("{BUILTIN_ID_PREFIX}{slug}")
}

/// Strip `builtin:` prefix when present.
pub fn strip_production_builtin_prefix(id_or_slug: &str) -> &str {
    id_or_slug
        .strip_prefix(BUILTIN_ID_PREFIX)
        .unwrap_or(id_or_slug)
}

/// Normalize legacy / alias QC pack ids to the canonical pack id.
pub fn normalize_qc_pack_id(pack_or_profile: &str) -> String {
    let s = pack_or_profile.trim();
    match s {
        "" | QC_PACK_LEGACY_DEFAULT | "default" => QC_PACK_DEFAULT_V1.into(),
        QC_PACK_DEFAULT_V1 | QC_PACK_STRICT_PRIVILEGE_V1 | QC_PACK_NATIVE_HEAVY_V1 => s.into(),
        other => other.to_string(),
    }
}

/// Known QC pack ids (built-in).
pub fn known_qc_pack_ids() -> &'static [&'static str] {
    &[
        QC_PACK_DEFAULT_V1,
        QC_PACK_STRICT_PRIVILEGE_V1,
        QC_PACK_NATIVE_HEAVY_V1,
        QC_PACK_LEGACY_DEFAULT,
    ]
}

/// SHA-256 hex of stable JSON of a resolved profile body (for audit / config hash).
pub fn production_profile_config_hash(body: &ProductionProfileBody) -> Result<String> {
    let json = serde_json::to_string(body)
        .map_err(|e| Error::Other(format!("profile body serialize: {e}")))?;
    let digest = Sha256::digest(json.as_bytes());
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Identity field map: source == header for every canonical field (0040 order).
pub fn identity_field_map() -> Vec<FieldMapEntry> {
    CANONICAL_DAT_SOURCES
        .iter()
        .map(|s| FieldMapEntry {
            source: (*s).to_string(),
            header: (*s).to_string(),
            include: true,
            date_format: None,
            timezone: None,
        })
        .collect()
}

/// Identity field map with US date formats on DATE_* fields.
pub fn us_date_field_map() -> Vec<FieldMapEntry> {
    CANONICAL_DAT_SOURCES
        .iter()
        .map(|s| {
            let (date_format, timezone) = if s.starts_with("DATE_") {
                (
                    Some("%m/%d/%Y".to_string()),
                    Some("America/New_York".to_string()),
                )
            } else {
                (None, None)
            };
            FieldMapEntry {
                source: (*s).to_string(),
                header: (*s).to_string(),
                include: true,
                date_format,
                timezone,
            }
        })
        .collect()
}

/// Relativity-oriented header aliases (header rename only; same sources).
pub fn relativity_alias_field_map() -> Vec<FieldMapEntry> {
    CANONICAL_DAT_SOURCES
        .iter()
        .map(|s| {
            let header = relativity_header_for(s);
            let (date_format, timezone) = if s.starts_with("DATE_") {
                (
                    Some("%m/%d/%Y".to_string()),
                    Some("America/New_York".to_string()),
                )
            } else {
                (None, None)
            };
            FieldMapEntry {
                source: (*s).to_string(),
                header,
                include: true,
                date_format,
                timezone,
            }
        })
        .collect()
}

fn relativity_header_for(source: &str) -> String {
    let mapped = match source {
        "BEGBATES" => "Control Number",
        "ENDBATES" => "End Control Number",
        "CONTROL_NUMBER" => "Control Number Alt",
        "ITEM_ID" => "Item ID",
        "PARENT_ITEM_ID" => "Parent Item ID",
        "FAMILY_ID" => "Family ID",
        "CUSTODIAN" => "Custodian",
        "FILE_NAME" => "File Name",
        "FILE_EXT" => "File Extension",
        "FILE_CATEGORY" => "File Type",
        "MIME_TYPE" => "MIME Type",
        "FILE_SIZE" => "File Size",
        "SHA256" => "SHA256 Hash",
        "DATE_SENT" => "Date Sent",
        "DATE_RECEIVED" => "Date Received",
        "DATE_CREATED" => "Date Created",
        "FROM" => "From",
        "TO" => "To",
        "CC" => "CC",
        "BCC" => "BCC",
        "SUBJECT" => "Subject",
        "NATIVE_PATH" => "Native Path",
        "TEXT_PATH" => "Extracted Text Path",
        "HAS_REDACTED_TEXT" => "Has Redacted Text",
        "WITHHELD" => "Withheld",
        "PROD_STATUS" => "Production Status",
        other => other,
    };
    mapped.to_string()
}

fn default_layout() -> LayoutConfig {
    LayoutConfig {
        data: default_data_dir(),
        natives: default_natives_dir(),
        text: default_text_dir(),
    }
}

fn default_bates() -> BatesConfig {
    BatesConfig {
        prefix: default_bates_prefix(),
        pad_width: default_pad_width(),
        filename_mode: default_filename_mode(),
    }
}

/// Default body matching 0040 packaging + US date formats (built-in default).
pub fn default_production_profile_body() -> ProductionProfileBody {
    ProductionProfileBody {
        version: PRODUCTION_PROFILE_BODY_VERSION,
        load_file: LoadFileConfig {
            dialect: default_dialect(),
            encoding: default_encoding(),
            field_map: us_date_field_map(),
        },
        bates: default_bates(),
        layout: default_layout(),
        packaging: PackagingConfig::default(),
        qc: ProductionQcConfig::default(),
    }
}

fn rel_alias_body() -> ProductionProfileBody {
    ProductionProfileBody {
        version: PRODUCTION_PROFILE_BODY_VERSION,
        load_file: LoadFileConfig {
            dialect: "relativity_field_alias_v1".into(),
            encoding: default_encoding(),
            field_map: relativity_alias_field_map(),
        },
        bates: default_bates(),
        layout: default_layout(),
        packaging: PackagingConfig::default(),
        qc: ProductionQcConfig::default(),
    }
}

fn strict_qc_body() -> ProductionProfileBody {
    let mut body = default_production_profile_body();
    body.qc.pack_id = QC_PACK_STRICT_PRIVILEGE_V1.into();
    body
}

fn make_builtin(
    slug: &str,
    label: &str,
    jurisdiction_tag: &str,
    body: ProductionProfileBody,
) -> ProductionProfile {
    ProductionProfile {
        id: production_builtin_id(slug),
        matter_id: None,
        slug: slug.to_string(),
        label: label.to_string(),
        jurisdiction_tag: Some(jurisdiction_tag.to_string()),
        body,
        is_builtin: true,
        created_at: None,
        updated_at: None,
    }
}

/// All built-in production profiles (code constants).
pub fn builtin_production_profiles() -> Vec<ProductionProfile> {
    vec![
        make_builtin(
            BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1,
            "US Concordance native+text (default)",
            "us_federal",
            default_production_profile_body(),
        ),
        make_builtin(
            BUILTIN_US_CONCORDANCE_REL_ALIAS_V1,
            "US Concordance + Relativity header aliases",
            "us_federal",
            rel_alias_body(),
        ),
        make_builtin(
            BUILTIN_US_STRICT_QC_CONCORDANCE_V1,
            "US Concordance + strict privilege QC",
            "us_federal",
            strict_qc_body(),
        ),
    ]
}

/// Look up a single built-in by slug or `builtin:slug`.
pub fn builtin_production_profile(slug: &str) -> Option<ProductionProfile> {
    let bare = strip_production_builtin_prefix(slug);
    builtin_production_profiles()
        .into_iter()
        .find(|p| p.slug == bare)
}

/// True when `source` is a known canonical DAT field.
pub fn is_canonical_source(source: &str) -> bool {
    CANONICAL_DAT_SOURCES
        .iter()
        .any(|s| s.eq_ignore_ascii_case(source))
}

/// True when `source` is a forbidden privilege / work-product field.
pub fn is_forbidden_source(source: &str) -> bool {
    FORBIDDEN_FIELD_SOURCES
        .iter()
        .any(|s| s.eq_ignore_ascii_case(source))
}

/// Reject raw JSON that embeds Bates start under any common key.
fn reject_bates_start_in_raw(root: &Value) -> Result<()> {
    // Top-level bates object
    if let Some(bates) = root.get("bates") {
        for key in [
            "start_at",
            "bates_start",
            "start",
            "start_number",
            "next_seq",
        ] {
            if bates.get(key).is_some() {
                return Err(Error::Other(format!(
                    "production profile must not embed Bates start ('bates.{key}'); \
                     start number is a job-time parameter only"
                )));
            }
        }
    }
    // Also reject top-level keys
    for key in ["start_at", "bates_start", "bates_start_at"] {
        if root.get(key).is_some() {
            return Err(Error::Other(format!(
                "production profile must not embed Bates start ('{key}'); \
                 start number is a job-time parameter only"
            )));
        }
    }
    Ok(())
}

/// Validate chrono strftime-ish format (non-empty; must contain at least one `%`).
fn validate_date_format(fmt: &str) -> Result<()> {
    let t = fmt.trim();
    if t.is_empty() {
        return Err(Error::Other(
            "date_format must be non-empty when set".into(),
        ));
    }
    if !t.contains('%') {
        return Err(Error::Other(format!(
            "date_format '{t}' must be a chrono strftime pattern (contain '%')"
        )));
    }
    Ok(())
}

/// Validate IANA timezone name parses via chrono-tz.
fn validate_timezone(tz: &str) -> Result<()> {
    let t = tz.trim();
    if t.is_empty() {
        return Err(Error::Other("timezone must be non-empty when set".into()));
    }
    if t.parse::<chrono_tz::Tz>().is_err() {
        return Err(Error::Other(format!(
            "unknown IANA timezone '{t}' (fail closed)"
        )));
    }
    Ok(())
}

/// Parse and validate a production profile body JSON string.
pub fn parse_production_profile_body(json: &str) -> Result<ProductionProfileBody> {
    if json.len() > PRODUCTION_PROFILE_BODY_MAX_BYTES {
        return Err(Error::Other(format!(
            "production profile body exceeds max size ({} bytes)",
            PRODUCTION_PROFILE_BODY_MAX_BYTES
        )));
    }

    let root: Value = serde_json::from_str(json)
        .map_err(|e| Error::Other(format!("invalid production profile body JSON: {e}")))?;

    reject_bates_start_in_raw(&root)?;

    let body: ProductionProfileBody = serde_json::from_value(root)
        .map_err(|e| Error::Other(format!("invalid production profile body: {e}")))?;

    validate_production_profile_body(&body)?;
    Ok(body)
}

/// Validate a parsed production profile body.
pub fn validate_production_profile_body(body: &ProductionProfileBody) -> Result<()> {
    if body.version != PRODUCTION_PROFILE_BODY_VERSION {
        return Err(Error::Other(format!(
            "unknown production profile body version: {} (expected {PRODUCTION_PROFILE_BODY_VERSION})",
            body.version
        )));
    }

    let dialect = body.load_file.dialect.trim();
    match dialect {
        "concordance_dat_v1" | "relativity_field_alias_v1" | "csv_twin_v1" => {}
        other => {
            return Err(Error::Other(format!(
                "unknown load-file dialect '{other}' \
                 (supported: concordance_dat_v1, relativity_field_alias_v1, csv_twin_v1)"
            )));
        }
    }

    let enc = body.load_file.encoding.trim().to_ascii_lowercase();
    if enc != "utf-8" && enc != "utf8" {
        return Err(Error::Other(format!(
            "unsupported load-file encoding '{enc}' (P0 supports utf-8 only)"
        )));
    }

    if body.load_file.field_map.is_empty() {
        return Err(Error::Other(
            "production profile field_map must not be empty".into(),
        ));
    }

    let mut included = 0u32;
    for entry in &body.load_file.field_map {
        let src = entry.source.trim();
        if src.is_empty() {
            return Err(Error::Other(
                "field_map entry source must be non-empty".into(),
            ));
        }
        if is_forbidden_source(src) {
            return Err(Error::Other(format!(
                "forbidden field source '{src}' (privilege / work-product columns are blocked)"
            )));
        }
        if !is_canonical_source(src) {
            return Err(Error::Other(format!(
                "unknown field source '{src}' (fail closed)"
            )));
        }
        if entry.include {
            included += 1;
            if entry.header.trim().is_empty() {
                return Err(Error::Other(format!(
                    "field_map header for source '{src}' must be non-empty when included"
                )));
            }
        }
        if let Some(ref fmt) = entry.date_format {
            validate_date_format(fmt)?;
        }
        if let Some(ref tz) = entry.timezone {
            validate_timezone(tz)?;
        }
    }
    if included == 0 {
        return Err(Error::Other(
            "production profile field_map must include at least one column".into(),
        ));
    }

    let prefix = body.bates.prefix.trim();
    if prefix.is_empty() {
        return Err(Error::Other("bates.prefix must be non-empty".into()));
    }
    if prefix
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
    {
        return Err(Error::Other(
            "bates.prefix may only contain ASCII alphanumeric, '_' or '-'".into(),
        ));
    }
    if body.bates.pad_width == 0 || body.bates.pad_width > 12 {
        return Err(Error::Other("bates.pad_width must be 1..=12".into()));
    }
    let mode = body.bates.filename_mode.trim();
    if mode != "name_by_bates" {
        return Err(Error::Other(format!(
            "unknown bates.filename_mode '{mode}' (supported: name_by_bates)"
        )));
    }

    for (label, folder) in [
        ("layout.data", body.layout.data.as_str()),
        ("layout.natives", body.layout.natives.as_str()),
        ("layout.text", body.layout.text.as_str()),
    ] {
        let t = folder.trim();
        if t.is_empty() {
            return Err(Error::Other(format!("{label} must be non-empty")));
        }
        if t.contains('/') || t.contains('\\') || t.contains("..") {
            return Err(Error::Other(format!(
                "{label} must be a single folder segment (no path separators)"
            )));
        }
    }

    let pack = normalize_qc_pack_id(&body.qc.pack_id);
    if !known_qc_pack_ids().contains(&pack.as_str())
        && pack != QC_PACK_DEFAULT_V1
        && pack != QC_PACK_STRICT_PRIVILEGE_V1
        && pack != QC_PACK_NATIVE_HEAVY_V1
    {
        // Allow unknown pack ids only if they match known list after normalize —
        // fail closed for free-form unknowns that are not the three built-ins.
        return Err(Error::Other(format!(
            "unknown qc.pack_id '{pack}' (supported: {QC_PACK_DEFAULT_V1}, \
             {QC_PACK_STRICT_PRIVILEGE_V1}, {QC_PACK_NATIVE_HEAVY_V1})"
        )));
    }

    Ok(())
}

/// Serialize a validated body to stable JSON.
pub fn production_profile_body_to_json(body: &ProductionProfileBody) -> Result<String> {
    serde_json::to_string(body)
        .map_err(|e| Error::Other(format!("production profile body serialize: {e}")))
}

/// Included field headers in profile order.
pub fn included_headers(body: &ProductionProfileBody) -> Vec<String> {
    body.load_file
        .field_map
        .iter()
        .filter(|e| e.include)
        .map(|e| e.header.clone())
        .collect()
}

/// Included field sources in profile order.
pub fn included_sources(body: &ProductionProfileBody) -> Vec<String> {
    body.load_file
        .field_map
        .iter()
        .filter(|e| e.include)
        .map(|e| e.source.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Date formatting (UTC → timezone → strftime)
// ---------------------------------------------------------------------------

/// Format a datetime field for a load file.
///
/// - When `date_format` is set: parse RFC3339 → convert to `timezone` (or UTC) → format.
/// - When unset: engine default UTC ISO `YYYY-MM-DDTHH:MM:SSZ`.
/// - Unparsable / zone-less inputs → empty string (never invent a timezone).
pub fn format_load_datetime(
    raw: Option<&str>,
    date_format: Option<&str>,
    timezone: Option<&str>,
) -> Result<String> {
    let Some(s) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(String::new());
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) else {
        return Ok(String::new());
    };
    let utc = dt.with_timezone(&chrono::Utc);

    let Some(fmt) = date_format.map(str::trim).filter(|t| !t.is_empty()) else {
        use chrono::{SecondsFormat, Utc};
        return Ok(utc
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true));
    };

    if let Some(tz_name) = timezone.map(str::trim).filter(|t| !t.is_empty()) {
        let tz: chrono_tz::Tz = tz_name.parse().map_err(|_| {
            Error::Other(format!("unknown IANA timezone '{tz_name}' at format time"))
        })?;
        let local = utc.with_timezone(&tz);
        return Ok(local.format(fmt).to_string());
    }
    Ok(utc.format(fmt).to_string())
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// List built-ins + matter-local production profiles.
    pub fn list_production_profiles(&self) -> Result<Vec<ProductionProfile>> {
        let mut out = builtin_production_profiles();
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, slug, label, jurisdiction_tag, body_json, created_at, updated_at \
             FROM production_profiles WHERE matter_id = ?1 ORDER BY slug ASC",
        )?;
        let rows = stmt.query_map(params![self.id()], map_production_profile_row)?;
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Resolve by id (`builtin:…`, user uuid) or bare slug.
    pub fn get_production_profile(&self, id_or_slug: &str) -> Result<ProductionProfile> {
        let key = id_or_slug.trim();
        if key.is_empty() {
            return Err(Error::Other(
                "production profile id/slug cannot be empty".into(),
            ));
        }

        if let Some(p) = builtin_production_profile(key) {
            return Ok(p);
        }

        if let Some(p) = self.load_user_production_profile_by_id(key)? {
            return Ok(p);
        }

        if let Some(p) = self.load_user_production_profile_by_slug(key)? {
            return Ok(p);
        }

        Err(Error::Other(format!("production profile not found: {key}")))
    }

    /// Insert or update a matter-local production profile. Reserved built-in slugs rejected.
    pub fn upsert_production_profile(
        &self,
        input: ProductionProfileInput,
    ) -> Result<ProductionProfile> {
        let now = now_rfc3339();
        let slug = input.slug.trim();
        if slug.is_empty() {
            return Err(Error::Other(
                "production profile slug cannot be empty".into(),
            ));
        }
        if !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(Error::Other(
                "production profile slug may only contain ASCII alphanumeric, '_' or '-'".into(),
            ));
        }
        if RESERVED_PRODUCTION_PROFILE_SLUGS.contains(&slug) {
            return Err(Error::Other(format!(
                "slug '{slug}' is reserved for a built-in production profile"
            )));
        }

        let label = input.label.trim();
        if label.is_empty() {
            return Err(Error::Other(
                "production profile label cannot be empty".into(),
            ));
        }

        let body = parse_production_profile_body(&input.body_json)?;
        let body_json = production_profile_body_to_json(&body)?;
        if body_json.len() > PRODUCTION_PROFILE_BODY_MAX_BYTES {
            return Err(Error::Other(format!(
                "production profile body exceeds max size ({} bytes)",
                PRODUCTION_PROFILE_BODY_MAX_BYTES
            )));
        }

        let profile = if let Some(ref id) = input.id {
            if id.starts_with(BUILTIN_ID_PREFIX) {
                return Err(Error::Other(
                    "cannot upsert a built-in production profile".into(),
                ));
            }
            let existing = self.get_production_profile(id)?;
            if existing.is_builtin {
                return Err(Error::Other(
                    "cannot upsert a built-in production profile".into(),
                ));
            }
            if existing.matter_id.as_deref() != Some(self.id()) {
                return Err(Error::Other(format!(
                    "production profile {id} belongs to another matter"
                )));
            }
            let clash: Option<String> = self
                .connection()
                .query_row(
                    "SELECT id FROM production_profiles WHERE matter_id = ?1 AND slug = ?2 AND id != ?3",
                    params![self.id(), slug, id],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "production profile slug already exists in matter: {slug}"
                )));
            }
            self.connection().execute(
                "UPDATE production_profiles SET slug = ?1, label = ?2, jurisdiction_tag = ?3, \
                 body_json = ?4, updated_at = ?5 WHERE id = ?6",
                params![slug, label, input.jurisdiction_tag, body_json, now, id],
            )?;
            self.load_user_production_profile_by_id(id)?
                .ok_or_else(|| {
                    Error::Other(format!("production profile not found after update: {id}"))
                })?
        } else {
            let clash: Option<String> = self
                .connection()
                .query_row(
                    "SELECT id FROM production_profiles WHERE matter_id = ?1 AND slug = ?2",
                    params![self.id(), slug],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "production profile slug already exists in matter: {slug}"
                )));
            }
            let id = new_id("ppr");
            self.connection().execute(
                "INSERT INTO production_profiles \
                 (id, matter_id, slug, label, jurisdiction_tag, body_json, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    self.id(),
                    slug,
                    label,
                    input.jurisdiction_tag,
                    body_json,
                    now,
                    now
                ],
            )?;
            self.load_user_production_profile_by_id(&id)?
                .ok_or_else(|| {
                    Error::Other(format!("production profile not found after insert: {id}"))
                })?
        };

        let config_hash = production_profile_config_hash(&profile.body)?;
        let _ = self.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "production_profile.upsert".into(),
            entity: format!("production_profile:{}", profile.id),
            params_json: json!({
                "id": profile.id,
                "slug": profile.slug,
                "label": profile.label,
                "version": profile.body.version,
                "config_hash": config_hash,
                "qc_pack_id": profile.body.qc.pack_id,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;

        Ok(profile)
    }

    /// Delete a matter-local production profile. Built-ins cannot be deleted.
    pub fn delete_production_profile(&self, id_or_slug: &str) -> Result<()> {
        let key = id_or_slug.trim();
        if key.starts_with(BUILTIN_ID_PREFIX) || builtin_production_profile(key).is_some() {
            return Err(Error::Other(
                "cannot delete a built-in production profile".into(),
            ));
        }
        let existing = self.get_production_profile(key)?;
        if existing.is_builtin {
            return Err(Error::Other(
                "cannot delete a built-in production profile".into(),
            ));
        }
        if existing.matter_id.as_deref() != Some(self.id()) {
            return Err(Error::Other(format!(
                "production profile {key} belongs to another matter"
            )));
        }

        self.connection().execute(
            "DELETE FROM production_profiles WHERE id = ?1",
            params![existing.id],
        )?;

        let _ = self.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "production_profile.delete".into(),
            entity: format!("production_profile:{}", existing.id),
            params_json: json!({
                "id": existing.id,
                "slug": existing.slug,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;

        Ok(())
    }

    fn load_user_production_profile_by_id(&self, id: &str) -> Result<Option<ProductionProfile>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, slug, label, jurisdiction_tag, body_json, created_at, updated_at \
             FROM production_profiles WHERE id = ?1 AND matter_id = ?2",
        )?;
        let mut rows = stmt.query(params![id, self.id()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_production_profile_row(row)?))
        } else {
            Ok(None)
        }
    }

    fn load_user_production_profile_by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<ProductionProfile>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, slug, label, jurisdiction_tag, body_json, created_at, updated_at \
             FROM production_profiles WHERE matter_id = ?1 AND slug = ?2",
        )?;
        let mut rows = stmt.query(params![self.id(), slug])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_production_profile_row(row)?))
        } else {
            Ok(None)
        }
    }
}

fn map_production_profile_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProductionProfile> {
    let body_json: String = row.get(5)?;
    let body = parse_production_profile_body(&body_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    Ok(ProductionProfile {
        id: row.get(0)?,
        matter_id: Some(row.get(1)?),
        slug: row.get(2)?,
        label: row.get(3)?,
        jurisdiction_tag: row.get(4)?,
        body,
        is_builtin: false,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_include_three_slugs() {
        let list = builtin_production_profiles();
        assert_eq!(list.len(), 3);
        let slugs: Vec<_> = list.iter().map(|p| p.slug.as_str()).collect();
        assert!(slugs.contains(&BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1));
        assert!(slugs.contains(&BUILTIN_US_CONCORDANCE_REL_ALIAS_V1));
        assert!(slugs.contains(&BUILTIN_US_STRICT_QC_CONCORDANCE_V1));
    }

    #[test]
    fn default_body_has_us_dates_and_default_qc() {
        let body = default_production_profile_body();
        assert_eq!(body.qc.pack_id, QC_PACK_DEFAULT_V1);
        let date = body
            .load_file
            .field_map
            .iter()
            .find(|e| e.source == "DATE_SENT")
            .expect("DATE_SENT");
        assert_eq!(date.date_format.as_deref(), Some("%m/%d/%Y"));
        assert_eq!(date.timezone.as_deref(), Some("America/New_York"));
    }

    #[test]
    fn rejects_start_at_in_body() {
        let mut body = default_production_profile_body();
        let json = serde_json::to_value(&body).unwrap();
        let mut obj = json.as_object().unwrap().clone();
        let mut bates = obj.get("bates").unwrap().as_object().unwrap().clone();
        bates.insert("start_at".into(), json!(1));
        obj.insert("bates".into(), Value::Object(bates));
        let s = serde_json::to_string(&Value::Object(obj)).unwrap();
        let err = parse_production_profile_body(&s).unwrap_err();
        assert!(err.to_string().contains("start"));
        // silence unused
        body.version = 1;
    }

    #[test]
    fn rejects_unknown_source() {
        let mut body = default_production_profile_body();
        body.load_file.field_map[0].source = "NOT_A_FIELD".into();
        let err = validate_production_profile_body(&body).unwrap_err();
        assert!(err.to_string().contains("unknown field source"));
    }

    #[test]
    fn rejects_privilege_field() {
        let mut body = default_production_profile_body();
        body.load_file.field_map.push(FieldMapEntry {
            source: "PRIVILEGE_DESCRIPTION".into(),
            header: "Priv Desc".into(),
            include: true,
            date_format: None,
            timezone: None,
        });
        let err = validate_production_profile_body(&body).unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }

    #[test]
    fn rejects_bad_timezone() {
        let mut body = default_production_profile_body();
        body.load_file.field_map[13].timezone = Some("Not/A_Zone".into());
        let err = validate_production_profile_body(&body).unwrap_err();
        assert!(err.to_string().contains("timezone"));
    }

    #[test]
    fn format_us_date_differs_from_iso() {
        let raw = Some("2026-07-21T15:00:00Z");
        let iso = format_load_datetime(raw, None, None).unwrap();
        assert_eq!(iso, "2026-07-21T15:00:00Z");
        let us = format_load_datetime(raw, Some("%m/%d/%Y"), Some("America/New_York")).unwrap();
        // 15:00 UTC → 11:00 EDT on Jul 21
        assert_eq!(us, "07/21/2026");
        let uk = format_load_datetime(raw, Some("%d/%m/%Y"), Some("Europe/London")).unwrap();
        assert_eq!(uk, "21/07/2026");
        assert_ne!(us, uk);
    }

    #[test]
    fn normalize_legacy_pack() {
        assert_eq!(
            normalize_qc_pack_id("default_production_qc_v1"),
            QC_PACK_DEFAULT_V1
        );
        assert_eq!(normalize_qc_pack_id(""), QC_PACK_DEFAULT_V1);
    }

    #[test]
    fn rel_alias_headers_differ() {
        let id_map = us_date_field_map();
        let rel = relativity_alias_field_map();
        assert_eq!(id_map.len(), rel.len());
        assert_ne!(id_map[0].header, rel[0].header);
        assert_eq!(rel[0].header, "Control Number");
    }
}
