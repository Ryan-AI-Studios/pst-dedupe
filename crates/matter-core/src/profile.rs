//! Named processing profiles (track **0043**).
//!
//! Profiles store a versioned body of stage kind → `{enabled, params}`. Execution
//! order is always [`CANONICAL_STAGE_ORDER`] — never JSON/map/array order.
//! Built-ins are code constants (not DB rows); user profiles live in
//! `processing_profiles` (schema v23).

use std::collections::{BTreeMap, HashSet};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, Matter};
use crate::AuditEventInput;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Profile body document version accepted by this crate.
pub const PROFILE_BODY_VERSION: u32 = 1;

/// Maximum serialized body size (bytes) for hostile JSON rejection.
pub const PROFILE_BODY_MAX_BYTES: usize = 256 * 1024;

/// Job kind for sequential profile execution (`process-runner`).
pub const JOB_KIND_PROFILE_RUN: &str = "profile_run";

/// Engine-hardcoded stage order for `profile_run`. Never trust user/JSON order.
pub const CANONICAL_STAGE_ORDER: &[&str] = &[
    "classify",
    "office_extract",
    "pdf_extract",
    "ics_extract",
    "ocr",
    "fts_index",
    "dedupe",
    "thread",
    "neardup",
    "cull",
    "promote",
];

/// Built-in profile name: classify + extract + reduce + promote; OCR/neardup/FTS off.
pub const BUILTIN_STANDARD: &str = "standard";
/// Built-in: [`BUILTIN_STANDARD`] + OCR enabled.
pub const BUILTIN_WITH_OCR: &str = "with_ocr";
/// Built-in: classify + office/pdf/ics extract only.
pub const BUILTIN_EXTRACT_ONLY: &str = "extract_only";
/// Built-in: dedupe + thread + cull + promote only.
pub const BUILTIN_REDUCE_ONLY: &str = "reduce_only";

/// Reserved built-in names (user cannot upsert these names).
pub const RESERVED_BUILTIN_NAMES: &[&str] = &[
    BUILTIN_STANDARD,
    BUILTIN_WITH_OCR,
    BUILTIN_EXTRACT_ONLY,
    BUILTIN_REDUCE_ONLY,
];

const BUILTIN_ID_PREFIX: &str = "builtin:";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One stage in a profile body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageSpec {
    pub enabled: bool,
    #[serde(default = "empty_object")]
    pub params: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

/// Versioned profile body (map of kind → stage). Map is order-free for storage;
/// execution uses [`CANONICAL_STAGE_ORDER`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileBody {
    pub version: u32,
    pub stages: BTreeMap<String, StageSpec>,
}

/// DTO for a built-in or user processing profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessingProfile {
    /// `builtin:standard` or user id (`pfl_…`).
    pub id: String,
    /// `None` for built-ins.
    pub matter_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub body: ProfileBody,
    pub is_builtin: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub created_by: Option<String>,
}

/// One planned stage for sequential run (canonical order ∩ enabled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagePlan {
    pub kind: String,
    pub enabled: bool,
    pub params_json: String,
}

/// Input for [`Matter::upsert_processing_profile`].
#[derive(Debug, Clone)]
pub struct ProcessingProfileInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    /// Raw body JSON (map or array form; validated + normalized).
    pub body_json: String,
    pub created_by: Option<String>,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// True when `kind` is in the profile P0 allowlist.
pub fn is_allowlisted_stage(kind: &str) -> bool {
    CANONICAL_STAGE_ORDER.contains(&kind)
}

/// Built-in profile id for a reserved name (`builtin:standard`).
pub fn builtin_id(name: &str) -> String {
    format!("{BUILTIN_ID_PREFIX}{name}")
}

/// Strip `builtin:` prefix when present.
pub fn strip_builtin_prefix(id_or_name: &str) -> &str {
    id_or_name
        .strip_prefix(BUILTIN_ID_PREFIX)
        .unwrap_or(id_or_name)
}

/// Parse and validate a profile body JSON string.
///
/// Accepts:
/// - `{ "version": 1, "stages": { "classify": { "enabled": true, "params": {} }, … } }`
/// - `{ "version": 1, "stages": [ { "kind": "classify", "enabled": true, "params": {} }, … ] }`
///
/// Always normalizes to a map. Rejects unknown version/kind, duplicates, oversize.
pub fn parse_profile_body(json: &str) -> Result<ProfileBody> {
    if json.len() > PROFILE_BODY_MAX_BYTES {
        return Err(Error::Other(format!(
            "profile body exceeds max size ({} bytes)",
            PROFILE_BODY_MAX_BYTES
        )));
    }

    let root: Value = serde_json::from_str(json)
        .map_err(|e| Error::Other(format!("invalid profile body JSON: {e}")))?;

    let version = root
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::Other("profile body missing version".into()))?;
    if version != u64::from(PROFILE_BODY_VERSION) {
        return Err(Error::Other(format!(
            "unknown profile body version: {version} (expected {PROFILE_BODY_VERSION})"
        )));
    }

    let stages_val = root
        .get("stages")
        .ok_or_else(|| Error::Other("profile body missing stages".into()))?;

    let stages = match stages_val {
        Value::Object(map) => normalize_stages_map(map)?,
        Value::Array(arr) => normalize_stages_array(arr)?,
        _ => {
            return Err(Error::Other(
                "profile body stages must be an object or array".into(),
            ));
        }
    };

    let body = ProfileBody {
        version: PROFILE_BODY_VERSION,
        stages,
    };
    validate_body_params(&body)?;
    Ok(body)
}

fn normalize_stages_map(map: &Map<String, Value>) -> Result<BTreeMap<String, StageSpec>> {
    let mut out = BTreeMap::new();
    for (kind, val) in map {
        if !is_allowlisted_stage(kind) {
            return Err(Error::Other(format!("unknown profile stage kind: {kind}")));
        }
        // Stage map values must be objects — reject null / arrays / scalars.
        if !val.is_object() {
            return Err(Error::Other(format!(
                "stage {kind} entry must be a JSON object"
            )));
        }
        let spec = parse_stage_spec(kind, val)?;
        out.insert(kind.clone(), spec);
    }
    Ok(out)
}

fn normalize_stages_array(arr: &[Value]) -> Result<BTreeMap<String, StageSpec>> {
    let mut out = BTreeMap::new();
    let mut seen = HashSet::new();
    for entry in arr {
        if !entry.is_object() {
            return Err(Error::Other(
                "stage array entry must be a JSON object".into(),
            ));
        }
        let kind = entry
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Other("stage array entry missing kind".into()))?
            .to_string();
        if !is_allowlisted_stage(&kind) {
            return Err(Error::Other(format!("unknown profile stage kind: {kind}")));
        }
        if !seen.insert(kind.clone()) {
            return Err(Error::Other(format!(
                "duplicate profile stage kind: {kind}"
            )));
        }
        let spec = parse_stage_spec(&kind, entry)?;
        out.insert(kind, spec);
    }
    Ok(out)
}

/// Known keys allowed on a stage object (map or array form).
const STAGE_OBJECT_KEYS: &[&str] = &["enabled", "params", "kind"];

fn parse_stage_spec(kind: &str, val: &Value) -> Result<StageSpec> {
    let obj = val
        .as_object()
        .ok_or_else(|| Error::Other(format!("stage {kind} entry must be a JSON object")))?;

    for key in obj.keys() {
        if !STAGE_OBJECT_KEYS.contains(&key.as_str()) {
            return Err(Error::Other(format!(
                "stage {kind} has unknown field '{key}' (allowed: enabled, params)"
            )));
        }
    }

    let enabled = match obj.get("enabled") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            return Err(Error::Other(format!(
                "stage {kind} enabled must be a boolean"
            )));
        }
    };

    let params = match obj.get("params") {
        None => empty_object(),
        // Null is not a valid params object — require {} for defaults.
        Some(Value::Null) => {
            return Err(Error::Other(format!(
                "stage {kind} params must be a JSON object (use {{}} for defaults)"
            )));
        }
        Some(Value::Object(_)) => obj.get("params").cloned().unwrap_or_else(empty_object),
        Some(_) => {
            return Err(Error::Other(format!(
                "stage {kind} params must be a JSON object"
            )));
        }
    };

    Ok(StageSpec { enabled, params })
}

// ---------------------------------------------------------------------------
// Per-kind param mirrors (`deny_unknown_fields`) — keep in sync with handlers.
// matter-core must not depend on stage crates; these are structural contracts.
// ---------------------------------------------------------------------------

fn positive_batch(v: u64) -> std::result::Result<(), String> {
    if v >= 1 {
        Ok(())
    } else {
        Err("batch_size must be >= 1".into())
    }
}

// Fields are only read for validation side-effects; suppress dead_code noise.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyParamsMirror {
    #[serde(default)]
    force: bool,
    #[serde(default = "default_batch_100")]
    batch_size: u64,
    #[serde(default = "default_true")]
    use_magic: bool,
    #[serde(default)]
    in_review_only: bool,
    #[serde(default = "default_true")]
    respect_extractor_refine: bool,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OfficeParamsMirror {
    #[serde(default)]
    force: bool,
    #[serde(default = "default_batch_50")]
    batch_size: u64,
    #[serde(default = "default_office_formats")]
    formats: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForceBatchMirror {
    #[serde(default)]
    force: bool,
    #[serde(default = "default_batch_50")]
    batch_size: u64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OcrParamsMirror {
    #[serde(default)]
    force: bool,
    #[serde(default = "default_batch_20")]
    batch_size: u64,
    #[serde(default = "default_lang")]
    lang: String,
    #[serde(default = "default_max_pages")]
    max_pages: u64,
    #[serde(default = "default_dpi")]
    dpi: u64,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    tesseract_path: Option<String>,
    #[serde(default)]
    tessdata_dir: Option<String>,
    #[serde(default)]
    pdf_renderer_path: Option<String>,
    #[serde(default = "default_engine")]
    engine: String,
    #[serde(default = "default_psm")]
    psm: u64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FtsParamsMirror {
    #[serde(default)]
    reset: bool,
    #[serde(default = "default_batch_100")]
    batch_size: u64,
    #[serde(default = "default_fts_scope")]
    scope: String,
    #[serde(default = "default_writer_heap")]
    writer_heap_bytes: u64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DedupeParamsMirror {
    #[serde(default = "default_true")]
    use_message_id: bool,
    #[serde(default = "default_true")]
    use_logical_hash: bool,
    #[serde(default = "default_family_policy")]
    family_policy: String,
    #[serde(default = "default_true")]
    reset: bool,
    #[serde(default = "default_batch_500")]
    batch_size: u64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThreadParamsMirror {
    #[serde(default = "default_true")]
    use_headers: bool,
    #[serde(default = "default_true")]
    use_subject_fallback: bool,
    #[serde(default = "default_true")]
    use_conversation_index: bool,
    #[serde(default = "default_true")]
    reset: bool,
    #[serde(default = "default_batch_500")]
    batch_size: u64,
    #[serde(default = "default_true")]
    family_inherit: bool,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NeardupParamsMirror {
    #[serde(default = "default_shingle_k")]
    shingle_k: u64,
    #[serde(default = "default_cjk_n")]
    cjk_char_n: u64,
    #[serde(default = "default_num_hashes")]
    num_hashes: u64,
    #[serde(default = "default_num_bands")]
    num_bands: u64,
    #[serde(default = "default_rows_per_band")]
    rows_per_band: u64,
    #[serde(default = "default_threshold")]
    threshold: f64,
    /// Fixed hash seed (matches matter-neardup DEFAULT_HASH_SEED default).
    #[serde(default = "default_hash_seed")]
    hash_seed: u64,
    #[serde(default = "default_true")]
    skip_exact_duplicates: bool,
    #[serde(default = "default_true")]
    ignore_numbers: bool,
    #[serde(default = "default_min_chars")]
    min_chars: u64,
    #[serde(default = "default_true")]
    reset: bool,
    #[serde(default = "default_batch_200")]
    batch_size: u64,
    #[serde(default = "default_true")]
    include_attachments: bool,
    #[serde(default)]
    strip_email_quotes: bool,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CullParamsMirror {
    #[serde(default)]
    preset_name: Option<String>,
    #[serde(default)]
    preset_id: Option<String>,
    /// Inline rules are not accepted on profile bodies (P0: use preset_name/id).
    /// Presence is rejected via deny_unknown_fields when we omit this field —
    /// keep optional and error if set so hand-authored JSON fails closed.
    #[serde(default)]
    rules: Option<Value>,
    #[serde(default = "default_true")]
    reset: bool,
    #[serde(default = "default_batch_500")]
    batch_size: u64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PromoteParamsMirror {
    #[serde(default = "default_policy")]
    policy: String,
    #[serde(default = "default_review_set")]
    review_set_name: String,
    #[serde(default = "default_true")]
    expand_families: bool,
    #[serde(default = "default_true")]
    reset: bool,
    #[serde(default = "default_batch_500")]
    batch_size: u64,
    #[serde(default)]
    require_dedupe: bool,
    #[serde(default)]
    fail_if_empty: bool,
    #[serde(default)]
    expand_threads: bool,
}

fn default_true() -> bool {
    true
}
fn default_batch_20() -> u64 {
    20
}
fn default_batch_50() -> u64 {
    50
}
fn default_batch_100() -> u64 {
    100
}
fn default_batch_200() -> u64 {
    200
}
fn default_batch_500() -> u64 {
    500
}
fn default_office_formats() -> Vec<String> {
    vec!["docx".into(), "xlsx".into(), "pptx".into()]
}
fn default_lang() -> String {
    "eng".into()
}
fn default_max_pages() -> u64 {
    500
}
fn default_dpi() -> u64 {
    200
}
fn default_engine() -> String {
    "tesseract".into()
}
fn default_psm() -> u64 {
    1
}
fn default_fts_scope() -> String {
    "all_with_text".into()
}
fn default_writer_heap() -> u64 {
    52_428_800
}
fn default_family_policy() -> String {
    "suppress_children_with_parent".into()
}
fn default_policy() -> String {
    "auto".into()
}
fn default_review_set() -> String {
    "Review Corpus".into()
}
fn default_shingle_k() -> u64 {
    5
}
fn default_cjk_n() -> u64 {
    2
}
fn default_num_hashes() -> u64 {
    128
}
fn default_num_bands() -> u64 {
    16
}
fn default_rows_per_band() -> u64 {
    8
}
fn default_threshold() -> f64 {
    0.80
}
fn default_min_chars() -> u64 {
    80
}
fn default_hash_seed() -> u64 {
    // ASCII "ND_mh_v1" packed — keep in sync with matter-neardup DEFAULT_HASH_SEED.
    0x4E44_5F6D_685F_7631
}

/// Typed param validation per allowlisted kind (`deny_unknown_fields` mirrors).
fn validate_body_params(body: &ProfileBody) -> Result<()> {
    for (kind, spec) in &body.stages {
        if !spec.params.is_object() {
            return Err(Error::Other(format!(
                "stage {kind} params must be a JSON object"
            )));
        }
        // Empty params use handler defaults — except stages with outer-enabled
        // product rules that require nested intent (OCR).
        if spec.params.as_object().is_some_and(|m| m.is_empty()) {
            if kind == "ocr" && spec.enabled {
                return Err(Error::Other(
                    "ocr stage enabled requires params.enabled=true (empty params default OCR off)"
                        .into(),
                ));
            }
            continue;
        }
        let json = spec.params.to_string();
        let err = |e: serde_json::Error| Error::Other(format!("stage {kind} params invalid: {e}"));
        match kind.as_str() {
            "classify" => {
                let p: ClassifyParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage classify: {m}")))?;
                let _ = (
                    p.force,
                    p.use_magic,
                    p.in_review_only,
                    p.respect_extractor_refine,
                );
            }
            "office_extract" => {
                let p: OfficeParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage office_extract: {m}")))?;
                if p.formats.is_empty() {
                    return Err(Error::Other(
                        "stage office_extract formats must be non-empty".into(),
                    ));
                }
                for f in &p.formats {
                    let l = f.to_ascii_lowercase();
                    if !matches!(l.as_str(), "docx" | "xlsx" | "pptx") {
                        return Err(Error::Other(format!(
                            "stage office_extract unsupported format '{f}'"
                        )));
                    }
                }
            }
            "pdf_extract" | "ics_extract" => {
                let p: ForceBatchMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage {kind}: {m}")))?;
                let _ = p.force;
            }
            "ocr" => {
                let p: OcrParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage ocr: {m}")))?;
                if spec.enabled && !p.enabled {
                    return Err(Error::Other(
                        "ocr stage enabled but nested params.enabled is false".into(),
                    ));
                }
                if p.lang.trim().is_empty() {
                    return Err(Error::Other("ocr lang must not be empty".into()));
                }
            }
            "fts_index" => {
                let p: FtsParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage fts_index: {m}")))?;
                if p.scope != "all_with_text" {
                    return Err(Error::Other(format!(
                        "fts_index scope must be all_with_text (got {})",
                        p.scope
                    )));
                }
                // matter-search rejects heaps under ~15 MiB.
                if p.writer_heap_bytes < 15_000_000 {
                    return Err(Error::Other(
                        "fts_index writer_heap_bytes must be >= 15000000".into(),
                    ));
                }
                let _ = p.reset;
            }
            "dedupe" => {
                let p: DedupeParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage dedupe: {m}")))?;
                let fp = p.family_policy.as_str();
                if !matches!(fp, "suppress_children_with_parent" | "parents_only") {
                    return Err(Error::Other(format!("dedupe family_policy unknown: {fp}")));
                }
                let _ = (p.use_message_id, p.use_logical_hash, p.reset);
            }
            "thread" => {
                let p: ThreadParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage thread: {m}")))?;
                let _ = (
                    p.use_headers,
                    p.use_subject_fallback,
                    p.use_conversation_index,
                    p.reset,
                    p.family_inherit,
                );
            }
            "neardup" => {
                let p: NeardupParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage neardup: {m}")))?;
                if p.shingle_k == 0 || p.cjk_char_n == 0 || p.num_hashes == 0 {
                    return Err(Error::Other(
                        "neardup shingle_k/cjk_char_n/num_hashes must be >= 1".into(),
                    ));
                }
                if p.num_bands == 0 || p.rows_per_band == 0 {
                    return Err(Error::Other(
                        "neardup num_bands and rows_per_band must be >= 1".into(),
                    ));
                }
                if p.num_bands.saturating_mul(p.rows_per_band) != p.num_hashes {
                    return Err(Error::Other(format!(
                        "neardup num_bands * rows_per_band must equal num_hashes ({} * {} != {})",
                        p.num_bands, p.rows_per_band, p.num_hashes
                    )));
                }
                if !(0.0..=1.0).contains(&p.threshold) {
                    return Err(Error::Other(
                        "neardup threshold must be between 0 and 1".into(),
                    ));
                }
                if p.strip_email_quotes {
                    return Err(Error::Other(
                        "neardup strip_email_quotes is not implemented in P0 (leave false)".into(),
                    ));
                }
                let _ = (
                    p.hash_seed,
                    p.skip_exact_duplicates,
                    p.ignore_numbers,
                    p.min_chars,
                    p.reset,
                    p.include_attachments,
                );
            }
            "cull" => {
                let p: CullParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage cull: {m}")))?;
                if let Some(ref name) = p.preset_name {
                    if name.is_empty() {
                        return Err(Error::Other(
                            "cull preset_name must not be empty when present".into(),
                        ));
                    }
                }
                if p.rules.is_some() {
                    return Err(Error::Other(
                        "cull inline rules are not supported in processing profiles (use preset_name or preset_id)"
                            .into(),
                    ));
                }
                let _ = (p.preset_id, p.reset);
            }
            "promote" => {
                let p: PromoteParamsMirror = serde_json::from_str(&json).map_err(err)?;
                positive_batch(p.batch_size)
                    .map_err(|m| Error::Other(format!("stage promote: {m}")))?;
                const PROMOTE_POLICIES: &[&str] = &[
                    "auto",
                    "cull_included",
                    "unique_only",
                    "unique_plus_family",
                    "all_extracted",
                    "cull_included_plus_family",
                ];
                if !PROMOTE_POLICIES.contains(&p.policy.as_str()) {
                    return Err(Error::Other(format!(
                        "unknown promote policy '{}'",
                        p.policy
                    )));
                }
                if p.expand_threads {
                    return Err(Error::Other(
                        "promote expand_threads is reserved (track 0056); set false".into(),
                    ));
                }
                let _ = (
                    p.review_set_name,
                    p.expand_families,
                    p.reset,
                    p.require_dedupe,
                    p.fail_if_empty,
                );
            }
            other => {
                return Err(Error::Other(format!(
                    "internal: unvalidated allowlisted kind {other}"
                )));
            }
        }
    }
    Ok(())
}

/// Serialize a body to stable JSON (BTreeMap order).
pub fn profile_body_to_json(body: &ProfileBody) -> Result<String> {
    serde_json::to_string(body).map_err(|e| Error::Other(format!("serialize profile body: {e}")))
}

/// Plan: `CANONICAL_STAGE_ORDER ∩ enabled` only.
pub fn profile_stage_plan(body: &ProfileBody) -> Vec<StagePlan> {
    let mut plan = Vec::new();
    for &kind in CANONICAL_STAGE_ORDER {
        if let Some(spec) = body.stages.get(kind) {
            if spec.enabled {
                let params_json = spec.params.to_string();
                plan.push(StagePlan {
                    kind: kind.to_string(),
                    enabled: true,
                    params_json,
                });
            }
        }
    }
    plan
}

/// Expand params JSON for a single stage kind from a profile body.
pub fn expand_profile_stage(body: &ProfileBody, kind: &str) -> Result<String> {
    if !is_allowlisted_stage(kind) {
        return Err(Error::Other(format!("unknown profile stage kind: {kind}")));
    }
    let spec = body
        .stages
        .get(kind)
        .ok_or_else(|| Error::Other(format!("stage {kind} not present in profile body")))?;
    if !spec.enabled {
        return Err(Error::Other(format!("stage {kind} is disabled in profile")));
    }
    Ok(spec.params.to_string())
}

// ---------------------------------------------------------------------------
// Built-in bodies
// ---------------------------------------------------------------------------

fn stage(enabled: bool, params: Value) -> StageSpec {
    StageSpec { enabled, params }
}

fn classify_params() -> Value {
    json!({
        "force": false,
        "batch_size": 100,
        "use_magic": true,
        "in_review_only": false,
        "respect_extractor_refine": true
    })
}

fn office_params() -> Value {
    json!({
        "force": false,
        "batch_size": 50,
        "formats": ["docx", "xlsx", "pptx"]
    })
}

fn pdf_params() -> Value {
    json!({
        "force": false,
        "batch_size": 50
    })
}

fn ics_params() -> Value {
    json!({
        "force": false,
        "batch_size": 50
    })
}

fn ocr_params(enabled: bool) -> Value {
    json!({
        "force": false,
        "batch_size": 20,
        "lang": "eng",
        "max_pages": 500,
        "dpi": 200,
        "enabled": enabled,
        "engine": "tesseract"
    })
}

fn fts_params() -> Value {
    json!({
        "reset": false,
        "batch_size": 100,
        "scope": "all_with_text",
        "writer_heap_bytes": 52_428_800
    })
}

fn dedupe_params() -> Value {
    json!({
        "use_message_id": true,
        "use_logical_hash": true,
        "family_policy": "suppress_children_with_parent",
        "reset": false,
        "batch_size": 500
    })
}

fn thread_params() -> Value {
    json!({
        "use_headers": true,
        "use_subject_fallback": true,
        "use_conversation_index": true,
        "reset": false,
        "batch_size": 500,
        "family_inherit": true
    })
}

fn neardup_params() -> Value {
    json!({
        "shingle_k": 5,
        "cjk_char_n": 2,
        "num_hashes": 128,
        "num_bands": 16,
        "rows_per_band": 8,
        "threshold": 0.80,
        "skip_exact_duplicates": true,
        "ignore_numbers": true,
        "min_chars": 80,
        "reset": false,
        "batch_size": 200,
        "include_attachments": true,
        "strip_email_quotes": false
    })
}

fn cull_params() -> Value {
    json!({
        "preset_name": "unique_only",
        "reset": false,
        "batch_size": 500
    })
}

fn promote_params() -> Value {
    json!({
        "policy": "auto",
        "review_set_name": "Review Corpus",
        "expand_families": true,
        "reset": false,
        "batch_size": 500,
        "require_dedupe": false
    })
}

fn standard_body(ocr_enabled: bool) -> ProfileBody {
    let mut stages = BTreeMap::new();
    stages.insert("classify".into(), stage(true, classify_params()));
    stages.insert("office_extract".into(), stage(true, office_params()));
    stages.insert("pdf_extract".into(), stage(true, pdf_params()));
    stages.insert("ics_extract".into(), stage(true, ics_params()));
    stages.insert("ocr".into(), stage(ocr_enabled, ocr_params(ocr_enabled)));
    stages.insert("fts_index".into(), stage(false, fts_params()));
    stages.insert("dedupe".into(), stage(true, dedupe_params()));
    stages.insert("thread".into(), stage(true, thread_params()));
    stages.insert("neardup".into(), stage(false, neardup_params()));
    stages.insert("cull".into(), stage(true, cull_params()));
    stages.insert("promote".into(), stage(true, promote_params()));
    ProfileBody {
        version: PROFILE_BODY_VERSION,
        stages,
    }
}

fn extract_only_body() -> ProfileBody {
    let mut stages = BTreeMap::new();
    stages.insert("classify".into(), stage(true, classify_params()));
    stages.insert("office_extract".into(), stage(true, office_params()));
    stages.insert("pdf_extract".into(), stage(true, pdf_params()));
    stages.insert("ics_extract".into(), stage(true, ics_params()));
    stages.insert("ocr".into(), stage(false, ocr_params(false)));
    stages.insert("fts_index".into(), stage(false, fts_params()));
    stages.insert("dedupe".into(), stage(false, dedupe_params()));
    stages.insert("thread".into(), stage(false, thread_params()));
    stages.insert("neardup".into(), stage(false, neardup_params()));
    stages.insert("cull".into(), stage(false, cull_params()));
    stages.insert("promote".into(), stage(false, promote_params()));
    ProfileBody {
        version: PROFILE_BODY_VERSION,
        stages,
    }
}

fn reduce_only_body() -> ProfileBody {
    let mut stages = BTreeMap::new();
    stages.insert("classify".into(), stage(false, classify_params()));
    stages.insert("office_extract".into(), stage(false, office_params()));
    stages.insert("pdf_extract".into(), stage(false, pdf_params()));
    stages.insert("ics_extract".into(), stage(false, ics_params()));
    stages.insert("ocr".into(), stage(false, ocr_params(false)));
    stages.insert("fts_index".into(), stage(false, fts_params()));
    stages.insert("dedupe".into(), stage(true, dedupe_params()));
    stages.insert("thread".into(), stage(true, thread_params()));
    stages.insert("neardup".into(), stage(false, neardup_params()));
    stages.insert("cull".into(), stage(true, cull_params()));
    stages.insert("promote".into(), stage(true, promote_params()));
    ProfileBody {
        version: PROFILE_BODY_VERSION,
        stages,
    }
}

fn make_builtin(name: &str, description: &str, body: ProfileBody) -> ProcessingProfile {
    ProcessingProfile {
        id: builtin_id(name),
        matter_id: None,
        name: name.to_string(),
        description: Some(description.to_string()),
        body,
        is_builtin: true,
        created_at: None,
        updated_at: None,
        created_by: None,
    }
}

/// All built-in processing profiles (code constants).
pub fn builtin_profiles() -> Vec<ProcessingProfile> {
    vec![
        make_builtin(
            BUILTIN_STANDARD,
            "Classify + office/pdf/ics extract + dedupe + thread + cull unique_only + promote auto. OCR/neardup/FTS off. Cumulative (reset:false).",
            standard_body(false),
        ),
        make_builtin(
            BUILTIN_WITH_OCR,
            "standard + OCR enabled (lang eng). Requires Tesseract when run.",
            standard_body(true),
        ),
        make_builtin(
            BUILTIN_EXTRACT_ONLY,
            "Classify + office/pdf/ics extract only (no reduce/promote).",
            extract_only_body(),
        ),
        make_builtin(
            BUILTIN_REDUCE_ONLY,
            "Dedupe + thread + cull + promote only. Cumulative (reset:false).",
            reduce_only_body(),
        ),
    ]
}

/// Look up a single built-in by name or `builtin:name`.
pub fn builtin_profile(name: &str) -> Option<ProcessingProfile> {
    let bare = strip_builtin_prefix(name);
    builtin_profiles().into_iter().find(|p| p.name == bare)
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// List built-ins + user profiles for this matter.
    pub fn list_processing_profiles(&self) -> Result<Vec<ProcessingProfile>> {
        let mut out = builtin_profiles();
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, name, description, body_json, created_at, updated_at, created_by \
             FROM processing_profiles WHERE matter_id = ?1 ORDER BY name ASC",
        )?;
        let rows = stmt.query_map(params![self.id()], map_profile_row)?;
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Resolve by id (`builtin:standard`, user uuid) or bare built-in / user name.
    pub fn get_processing_profile(&self, id_or_name: &str) -> Result<ProcessingProfile> {
        let key = id_or_name.trim();
        if key.is_empty() {
            return Err(Error::Other("profile id/name cannot be empty".into()));
        }

        // Built-in by id or bare name.
        if let Some(p) = builtin_profile(key) {
            return Ok(p);
        }

        // User by id.
        if let Some(p) = self.load_user_profile_by_id(key)? {
            return Ok(p);
        }

        // User by name.
        if let Some(p) = self.load_user_profile_by_name(key)? {
            return Ok(p);
        }

        Err(Error::Other(format!("processing profile not found: {key}")))
    }

    /// Insert or update a user profile. Reserved built-in names are rejected.
    pub fn upsert_processing_profile(
        &self,
        input: ProcessingProfileInput,
    ) -> Result<ProcessingProfile> {
        let now = now_rfc3339();
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::Other(
                "processing profile name cannot be empty".into(),
            ));
        }
        if RESERVED_BUILTIN_NAMES.contains(&name) {
            return Err(Error::Other(format!(
                "name '{name}' is reserved for a built-in profile"
            )));
        }

        let body = parse_profile_body(&input.body_json)?;
        let body_json = profile_body_to_json(&body)?;
        if body_json.len() > PROFILE_BODY_MAX_BYTES {
            return Err(Error::Other(format!(
                "profile body exceeds max size ({} bytes)",
                PROFILE_BODY_MAX_BYTES
            )));
        }

        let profile = if let Some(ref id) = input.id {
            if id.starts_with(BUILTIN_ID_PREFIX) {
                return Err(Error::Other(
                    "cannot upsert a built-in processing profile".into(),
                ));
            }
            let existing = self.get_processing_profile(id)?;
            if existing.is_builtin {
                return Err(Error::Other(
                    "cannot upsert a built-in processing profile".into(),
                ));
            }
            if existing.matter_id.as_deref() != Some(self.id()) {
                return Err(Error::Other(format!(
                    "processing profile {id} belongs to another matter"
                )));
            }
            let clash: Option<String> = self
                .connection()
                .query_row(
                    "SELECT id FROM processing_profiles WHERE matter_id = ?1 AND name = ?2 AND id != ?3",
                    params![self.id(), name, id],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "processing profile name already exists in matter: {name}"
                )));
            }
            self.connection().execute(
                "UPDATE processing_profiles SET name = ?1, description = ?2, body_json = ?3, \
                 updated_at = ?4, created_by = COALESCE(?5, created_by) WHERE id = ?6",
                params![
                    name,
                    input.description,
                    body_json,
                    now,
                    input.created_by,
                    id
                ],
            )?;
            self.load_user_profile_by_id(id)?.ok_or_else(|| {
                Error::Other(format!("processing profile not found after update: {id}"))
            })?
        } else {
            let clash: Option<String> = self
                .connection()
                .query_row(
                    "SELECT id FROM processing_profiles WHERE matter_id = ?1 AND name = ?2",
                    params![self.id(), name],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "processing profile name already exists in matter: {name}"
                )));
            }
            let id = new_id("pfl");
            self.connection().execute(
                "INSERT INTO processing_profiles (id, matter_id, name, description, body_json, \
                 created_at, updated_at, created_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    self.id(),
                    name,
                    input.description,
                    body_json,
                    now,
                    now,
                    input.created_by
                ],
            )?;
            self.load_user_profile_by_id(&id)?.ok_or_else(|| {
                Error::Other(format!("processing profile not found after insert: {id}"))
            })?
        };

        // Canonical plan order (not BTreeMap key order) for defensibility.
        let enabled: Vec<String> = profile_stage_plan(&profile.body)
            .into_iter()
            .map(|s| s.kind)
            .collect();
        let _ = self.append_audit(AuditEventInput {
            actor: input.created_by.clone().unwrap_or_else(|| "system".into()),
            action: "profile.upsert".into(),
            entity: format!("processing_profile:{}", profile.id),
            params_json: json!({
                "id": profile.id,
                "name": profile.name,
                "version": profile.body.version,
                "enabled_stages": enabled,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;

        Ok(profile)
    }

    /// Delete a user profile. Built-ins cannot be deleted. Clears default if matched.
    pub fn delete_processing_profile(&self, id: &str) -> Result<()> {
        let key = id.trim();
        if key.starts_with(BUILTIN_ID_PREFIX) || builtin_profile(key).is_some() {
            return Err(Error::Other(
                "cannot delete a built-in processing profile".into(),
            ));
        }
        let existing = self.get_processing_profile(key)?;
        if existing.is_builtin {
            return Err(Error::Other(
                "cannot delete a built-in processing profile".into(),
            ));
        }
        if existing.matter_id.as_deref() != Some(self.id()) {
            return Err(Error::Other(format!(
                "processing profile {key} belongs to another matter"
            )));
        }

        self.connection().execute(
            "DELETE FROM processing_profiles WHERE id = ?1",
            params![existing.id],
        )?;

        // Clear matter default if it pointed at this profile.
        if let Ok(Some(default_id)) = self.get_default_processing_profile_id() {
            if default_id == existing.id {
                let _ = self.set_default_processing_profile(None);
            }
        }

        let _ = self.append_audit(AuditEventInput {
            actor: existing
                .created_by
                .clone()
                .unwrap_or_else(|| "system".into()),
            action: "profile.delete".into(),
            entity: format!("processing_profile:{}", existing.id),
            params_json: json!({
                "id": existing.id,
                "name": existing.name,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        Ok(())
    }

    /// Set or clear the matter's default processing profile id.
    pub fn set_default_processing_profile(&self, profile_id: Option<&str>) -> Result<()> {
        if let Some(id) = profile_id {
            // Validate resolvable (built-in or user).
            let _ = self.get_processing_profile(id)?;
            self.connection().execute(
                "UPDATE matters SET default_profile_id = ?1 WHERE id = ?2",
                params![id, self.id()],
            )?;
        } else {
            self.connection().execute(
                "UPDATE matters SET default_profile_id = NULL WHERE id = ?1",
                params![self.id()],
            )?;
        }
        Ok(())
    }

    /// Current default profile id, if set.
    pub fn get_default_processing_profile_id(&self) -> Result<Option<String>> {
        let id: Option<String> = self.connection().query_row(
            "SELECT default_profile_id FROM matters WHERE id = ?1",
            params![self.id()],
            |row| row.get(0),
        )?;
        Ok(id.filter(|s| !s.is_empty()))
    }

    fn load_user_profile_by_id(&self, id: &str) -> Result<Option<ProcessingProfile>> {
        self.connection()
            .query_row(
                "SELECT id, matter_id, name, description, body_json, created_at, updated_at, created_by \
                 FROM processing_profiles WHERE id = ?1 AND matter_id = ?2",
                params![id, self.id()],
                map_profile_row,
            )
            .optional()
            .map_err(Error::from)
    }

    fn load_user_profile_by_name(&self, name: &str) -> Result<Option<ProcessingProfile>> {
        self.connection()
            .query_row(
                "SELECT id, matter_id, name, description, body_json, created_at, updated_at, created_by \
                 FROM processing_profiles WHERE matter_id = ?1 AND name = ?2",
                params![self.id(), name],
                map_profile_row,
            )
            .optional()
            .map_err(Error::from)
    }
}

fn map_profile_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProcessingProfile> {
    let body_json: String = row.get(4)?;
    let body = parse_profile_body(&body_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })?;
    Ok(ProcessingProfile {
        id: row.get(0)?,
        matter_id: Some(row.get(1)?),
        name: row.get(2)?,
        description: row.get(3)?,
        body,
        is_builtin: false,
        created_at: Some(row.get(5)?),
        updated_at: Some(row.get(6)?),
        created_by: row.get(7)?,
    })
}
