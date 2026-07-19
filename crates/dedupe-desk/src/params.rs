//! JSON param builders for process-runner job kinds (pure — unit tested).

use std::path::Path;

/// Build ingest start params: `{ "path": "…" }`.
pub fn ingest_params(path: &str) -> String {
    serde_json::json!({ "path": path }).to_string()
}

/// Build extract_pst start params from inventory: `{ "source_id", "pst_item_id" }`.
pub fn extract_pst_item_params(source_id: &str, pst_item_id: &str) -> String {
    serde_json::json!({
        "source_id": source_id,
        "pst_item_id": pst_item_id,
    })
    .to_string()
}

/// Build extract_pst start params from filesystem path under a source.
///
/// Available for path-form extract; the Desk UI prefers inventory `pst_item_id`.
#[allow(dead_code)]
pub fn extract_pst_path_params(source_id: &str, path: &str) -> String {
    serde_json::json!({
        "source_id": source_id,
        "path": path,
    })
    .to_string()
}

/// Default params for matter-level tiered dedupe (`kind = "dedupe"`).
pub fn dedupe_default_params() -> String {
    serde_json::json!({
        "use_message_id": true,
        "use_logical_hash": true,
        "family_policy": "suppress_children_with_parent",
        "reset": true,
        "batch_size": 500
    })
    .to_string()
}

/// Default params for matter-level email threading (`kind = "thread"`).
pub fn thread_default_params() -> String {
    serde_json::json!({
        "use_headers": true,
        "use_subject_fallback": true,
        "use_conversation_index": true,
        "reset": true,
        "batch_size": 500,
        "family_inherit": true
    })
    .to_string()
}

/// Default params for matter-level near-duplicate detection (`kind = "neardup"`).
pub fn neardup_default_params() -> String {
    serde_json::json!({
        "shingle_k": 5,
        "cjk_char_n": 2,
        "num_hashes": 128,
        "num_bands": 16,
        "rows_per_band": 8,
        "threshold": 0.80,
        "skip_exact_duplicates": true,
        "ignore_numbers": true,
        "min_chars": 80,
        "reset": true,
        "batch_size": 200,
        "include_attachments": true,
        "strip_email_quotes": false
    })
    .to_string()
}

/// Built-in cull preset names shown in the desk dropdown.
///
/// `date_window` is intentionally omitted: it needs operator-filled start/end
/// bounds (offset-aware RFC3339). Operators can still use it via JSON params
/// or a user preset that supplies bounds — see `matter-cull` README.
pub const CULL_BUILTIN_PRESETS: &[&str] = &["unique_only", "unique_plus_family", "noise_light"];

/// Selection encoding prefix for matter-saved user presets (`user:<id>`).
pub const CULL_USER_PRESET_PREFIX: &str = "user:";

/// Cull params for a built-in (or named) preset (`kind = "cull"`).
///
/// Default desk selection is `"unique_only"`.
pub fn cull_params_for_preset(preset_name: &str) -> String {
    serde_json::json!({
        "preset_name": preset_name,
        "reset": true,
        "batch_size": 500
    })
    .to_string()
}

/// Cull params for a dropdown selection encoding.
///
/// - Built-ins use bare name (`unique_only`) → `{ "preset_name", ... }`
/// - User presets use `user:<id>` → `{ "preset_id", ... }`
pub fn cull_params_for_selection(sel: &str) -> String {
    if let Some(id) = sel.strip_prefix(CULL_USER_PRESET_PREFIX) {
        serde_json::json!({
            "preset_id": id,
            "reset": true,
            "batch_size": 500
        })
        .to_string()
    } else {
        cull_params_for_preset(sel)
    }
}

/// Default params for matter-level cull (`unique_only`).
///
/// Kept as the stable default JSON shape; desk `start_cull` uses
/// [`cull_params_for_selection`] with the dropdown value.
#[allow(dead_code)]
pub fn cull_default_params() -> String {
    cull_params_for_preset("unique_only")
}

/// Promote policies shown in the desk dropdown.
pub const PROMOTE_POLICIES: &[&str] = &[
    "auto",
    "cull_included",
    "unique_only",
    "unique_plus_family",
    "all_extracted",
    "cull_included_plus_family",
];

/// Default params for promote-to-review (`kind = "promote"`).
///
/// Desk `start_promote` uses [`promote_params_for_policy`] with the dropdown value.
#[allow(dead_code)]
pub fn promote_default_params() -> String {
    promote_params_for_policy("auto")
}

/// Promote params for a named policy (or `auto`).
pub fn promote_params_for_policy(policy: &str) -> String {
    serde_json::json!({
        "policy": policy,
        "review_set_name": "Review Corpus",
        "expand_families": true,
        "reset": true,
        "batch_size": 500,
        "require_dedupe": false
    })
    .to_string()
}

/// Default params for production export (`kind = "produce"`).
#[allow(dead_code)]
pub fn produce_default_params() -> String {
    produce_params("Review Production", "PROD", false, false, true, None)
}

/// Build produce job params JSON.
pub fn produce_params(
    name: &str,
    bates_prefix: &str,
    fail_if_withheld: bool,
    expand_family: bool,
    require_qc_pass: bool,
    output_dir: Option<&str>,
) -> String {
    let mut v = serde_json::json!({
        "scope": "review_corpus",
        "name": name,
        "bates_prefix": bates_prefix,
        "fail_if_withheld": fail_if_withheld,
        "export_eml_if_missing_native": true,
        "include_csv_twin": true,
        "expand_family": expand_family,
        "require_qc_pass": require_qc_pass,
    });
    if let Some(dir) = output_dir.map(str::trim).filter(|s| !s.is_empty()) {
        v["output_dir"] = serde_json::Value::String(dir.to_string());
    }
    v.to_string()
}

/// Default params for production QC (`kind = "qc"`).
///
/// Expand defaults to **false** — when starting QC from the Produce screen,
/// pass the same `expand_family` flag as produce via [`qc_params`] so the
/// selection fingerprint matches (otherwise produce is permanently stale).
#[allow(dead_code)] // retained as the documented expand=false default helper
pub fn qc_default_params() -> String {
    qc_params("review_corpus", false, None)
}

/// Build production QC job params JSON.
///
/// **Contract:** `expand_family_for_scan` must match produce's `expand_family`
/// when QC is used to authorize that produce selection.
pub fn qc_params(scope: &str, expand_family_for_scan: bool, report_dir: Option<&str>) -> String {
    let mut v = serde_json::json!({
        "scope": scope,
        "expand_family_for_scan": expand_family_for_scan,
        "profile": "default_production_qc_v1",
        "rules": [],
    });
    if let Some(dir) = report_dir.map(str::trim).filter(|s| !s.is_empty()) {
        v["report_dir"] = serde_json::Value::String(dir.to_string());
    }
    v.to_string()
}

/// Default params for FTS index build/update (`kind = "fts_index"`, incremental).
pub fn fts_index_default_params() -> String {
    serde_json::json!({
        "reset": false,
        "batch_size": 100,
        "scope": "all_with_text",
        "writer_heap_bytes": 52_428_800
    })
    .to_string()
}

/// Params for full FTS rebuild (`reset: true`) — drop all index handles first.
pub fn fts_index_reset_params() -> String {
    serde_json::json!({
        "reset": true,
        "batch_size": 100,
        "scope": "all_with_text",
        "writer_heap_bytes": 52_428_800
    })
    .to_string()
}

/// Default params for Office OOXML text extract (`kind = "office_extract"`).
pub fn office_extract_default_params() -> String {
    serde_json::json!({
        "force": false,
        "batch_size": 50,
        "formats": ["docx", "xlsx", "pptx"]
    })
    .to_string()
}

/// Default params for PDF text extract (`kind = "pdf_extract"`).
pub fn pdf_extract_default_params() -> String {
    serde_json::json!({
        "force": false,
        "batch_size": 50
    })
    .to_string()
}

/// Default params for OCR (`kind = "ocr"`).
///
/// Pass desk enable flag and tool paths so the job fails closed when OCR is off.
pub fn ocr_default_params(
    enabled: bool,
    tesseract_path: Option<&str>,
    tessdata_dir: Option<&str>,
    pdf_renderer_path: Option<&str>,
) -> String {
    serde_json::json!({
        "force": false,
        "batch_size": 20,
        "lang": "eng",
        "max_pages": 500,
        "dpi": 200,
        "enabled": enabled,
        "tesseract_path": tesseract_path,
        "tessdata_dir": tessdata_dir,
        "pdf_renderer_path": pdf_renderer_path,
        "engine": "tesseract"
    })
    .to_string()
}

/// Default params for ICS calendar extract (`kind = "ics_extract"`).
pub fn ics_extract_default_params() -> String {
    serde_json::json!({
        "force": false,
        "batch_size": 50
    })
    .to_string()
}

/// Default params for file-category classify (`kind = "classify"`).
pub fn classify_default_params() -> String {
    serde_json::json!({
        "force": false,
        "batch_size": 100,
        "use_magic": true,
        "in_review_only": false,
        "respect_extractor_refine": true
    })
    .to_string()
}

/// True when `path` looks like a PST (case-insensitive `.pst` extension).
pub fn looks_like_pst(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pst"))
        .unwrap_or(false)
}

/// True when `path` looks like a ZIP.
pub fn looks_like_zip(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
}

/// Validate a matter display name (non-empty after trim; no path separators).
pub fn validate_matter_name(name: &str) -> Result<&str, String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("Matter name cannot be empty.".into());
    }
    if t.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err("Matter name contains invalid characters.".into());
    }
    Ok(t)
}

/// Human message for runner errors (Busy gets the product copy).
///
/// Durable leftover Running rows and in-process single-flight both surface as
/// `Busy`. Guidance prefers **Resume** / finish the named job rather than only
/// “cancel or wait” (cancel does nothing if nothing is in-process).
pub fn format_runner_error(err: &process_runner::RunnerError) -> String {
    match err {
        process_runner::RunnerError::Busy { job_id } => {
            format!(
                "A job is already active or left Running (job {job_id}). \
                 Use Resume for that job, or wait if it is still processing."
            )
        }
        other => other.to_string(),
    }
}

/// True when an error string looks like transient SQLite lock contention.
pub fn is_transient_sqlite_lock(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("busy")
        || e.contains("locked")
        || e.contains("database is locked")
        || e.contains("sqlite_busy")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_json_shape() {
        let j = ingest_params(r"C:\exports\pkg");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["path"], r"C:\exports\pkg");
    }

    #[test]
    fn extract_item_json_shape() {
        let j = extract_pst_item_params("src1", "itm1");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["source_id"], "src1");
        assert_eq!(v["pst_item_id"], "itm1");
        assert!(v.get("path").is_none());
    }

    #[test]
    fn extract_path_json_shape() {
        let j = extract_pst_path_params("src1", "mail.pst");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["path"], "mail.pst");
    }

    #[test]
    fn dedupe_default_json_shape() {
        let j = dedupe_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["use_message_id"], true);
        assert_eq!(v["use_logical_hash"], true);
        assert_eq!(v["family_policy"], "suppress_children_with_parent");
        assert_eq!(v["reset"], true);
        assert_eq!(v["batch_size"], 500);
    }

    #[test]
    fn thread_default_json_shape() {
        let j = thread_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["use_headers"], true);
        assert_eq!(v["use_subject_fallback"], true);
        assert_eq!(v["use_conversation_index"], true);
        assert_eq!(v["reset"], true);
        assert_eq!(v["batch_size"], 500);
        assert_eq!(v["family_inherit"], true);
    }

    #[test]
    fn neardup_default_json_shape() {
        let j = neardup_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["shingle_k"], 5);
        assert_eq!(v["cjk_char_n"], 2);
        assert_eq!(v["num_hashes"], 128);
        assert_eq!(v["num_bands"], 16);
        assert_eq!(v["rows_per_band"], 8);
        assert_eq!(v["threshold"], 0.80);
        assert_eq!(v["skip_exact_duplicates"], true);
        assert_eq!(v["min_chars"], 80);
        assert_eq!(v["reset"], true);
        assert_eq!(v["batch_size"], 200);
        assert_eq!(v["include_attachments"], true);
        assert_eq!(v["strip_email_quotes"], false);
    }

    #[test]
    fn cull_default_json_shape() {
        let j = cull_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["preset_name"], "unique_only");
        assert_eq!(v["reset"], true);
        assert_eq!(v["batch_size"], 500);
        assert_eq!(CULL_BUILTIN_PRESETS[0], "unique_only");
    }

    #[test]
    fn promote_default_json_shape() {
        let j = promote_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["policy"], "auto");
        assert_eq!(v["review_set_name"], "Review Corpus");
        assert_eq!(v["expand_families"], true);
        assert_eq!(v["reset"], true);
        assert_eq!(v["batch_size"], 500);
        assert_eq!(v["require_dedupe"], false);
        assert_eq!(PROMOTE_POLICIES[0], "auto");
    }

    #[test]
    fn produce_default_json_shape() {
        let j = produce_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["scope"], "review_corpus");
        assert_eq!(v["bates_prefix"], "PROD");
        assert_eq!(v["fail_if_withheld"], false);
        assert_eq!(v["export_eml_if_missing_native"], true);
        assert_eq!(v["include_csv_twin"], true);
        assert_eq!(v["expand_family"], false);
        assert_eq!(v["require_qc_pass"], true);
        assert!(v.get("output_dir").is_none() || v["output_dir"].is_null());
    }

    #[test]
    fn produce_params_with_output_dir() {
        let j = produce_params("P1", "ABC", true, false, false, Some(r"C:\out\prod"));
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["name"], "P1");
        assert_eq!(v["bates_prefix"], "ABC");
        assert_eq!(v["fail_if_withheld"], true);
        assert_eq!(v["require_qc_pass"], false);
        assert_eq!(v["output_dir"], r"C:\out\prod");
    }

    #[test]
    fn qc_default_json_shape() {
        let j = qc_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["scope"], "review_corpus");
        assert_eq!(v["expand_family_for_scan"], false);
        assert_eq!(v["profile"], "default_production_qc_v1");
        assert!(v["rules"].as_array().unwrap().is_empty());
    }

    #[test]
    fn qc_params_with_report_dir() {
        let j = qc_params("item_ids", true, Some(r"C:\out\qc"));
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["scope"], "item_ids");
        assert_eq!(v["expand_family_for_scan"], true);
        assert_eq!(v["report_dir"], r"C:\out\qc");
    }

    #[test]
    fn qc_params_includes_expand_family_for_scan_true_when_requested() {
        // Desk must pass produce_expand_family into QC so fingerprints match.
        let j = qc_params("review_corpus", true, None);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["scope"], "review_corpus");
        assert_eq!(v["expand_family_for_scan"], true);
        assert!(v.get("report_dir").is_none() || v["report_dir"].is_null());
    }

    #[test]
    fn fts_index_params_shapes() {
        let j = fts_index_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["reset"], false);
        assert_eq!(v["batch_size"], 100);
        assert_eq!(v["scope"], "all_with_text");
        assert_eq!(v["writer_heap_bytes"], 52_428_800);

        let r = fts_index_reset_params();
        let rv: serde_json::Value = serde_json::from_str(&r).unwrap();
        assert_eq!(rv["reset"], true);
        assert_eq!(rv["scope"], "all_with_text");
    }

    #[test]
    fn office_extract_params_shape() {
        let j = office_extract_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["force"], false);
        assert_eq!(v["batch_size"], 50);
        assert_eq!(v["formats"][0], "docx");
        assert_eq!(v["formats"][1], "xlsx");
        assert_eq!(v["formats"][2], "pptx");
    }

    #[test]
    fn pdf_extract_params_shape() {
        let j = pdf_extract_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["force"], false);
        assert_eq!(v["batch_size"], 50);
    }

    #[test]
    fn ocr_params_shape_fail_closed_by_default() {
        let j = ocr_default_params(false, None, None, None);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["enabled"], false);
        assert_eq!(v["force"], false);
        assert_eq!(v["batch_size"], 20);
        assert_eq!(v["lang"], "eng");
        assert_eq!(v["max_pages"], 500);
        assert_eq!(v["dpi"], 200);
        assert_eq!(v["engine"], "tesseract");
    }

    #[test]
    fn ocr_params_paths_when_enabled() {
        let j = ocr_default_params(
            true,
            Some(r"C:\tools\tesseract.exe"),
            Some(r"C:\tessdata"),
            Some(r"C:\tools\pdftoppm.exe"),
        );
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["enabled"], true);
        assert_eq!(v["tesseract_path"], r"C:\tools\tesseract.exe");
        assert_eq!(v["tessdata_dir"], r"C:\tessdata");
        assert_eq!(v["pdf_renderer_path"], r"C:\tools\pdftoppm.exe");
    }

    #[test]
    fn ics_extract_params_shape() {
        let j = ics_extract_default_params();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["force"], false);
        assert_eq!(v["batch_size"], 50);
    }

    #[test]
    fn cull_params_for_selection_builtin() {
        let j = cull_params_for_selection("noise_light");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["preset_name"], "noise_light");
        assert!(v.get("preset_id").is_none());
        assert_eq!(v["reset"], true);
        assert_eq!(v["batch_size"], 500);
    }

    #[test]
    fn cull_params_for_selection_user_id() {
        let j = cull_params_for_selection("user:abc-123-def");
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["preset_id"], "abc-123-def");
        assert!(v.get("preset_name").is_none());
        assert_eq!(v["reset"], true);
        assert_eq!(v["batch_size"], 500);
        assert!(
            "user:abc-123-def".starts_with(CULL_USER_PRESET_PREFIX),
            "user selection encoding uses the user: prefix"
        );
    }

    #[test]
    fn extension_helpers() {
        assert!(looks_like_pst("Mail.PST"));
        assert!(looks_like_zip("pkg.zip"));
        assert!(!looks_like_pst("readme.txt"));
    }

    #[test]
    fn name_validation() {
        assert!(validate_matter_name("  Case-42  ").is_ok());
        assert!(validate_matter_name("").is_err());
        assert!(validate_matter_name("a/b").is_err());
    }

    #[test]
    fn transient_lock_detection() {
        assert!(is_transient_sqlite_lock("SQLite error: database is locked"));
        assert!(is_transient_sqlite_lock("busy"));
        assert!(!is_transient_sqlite_lock("no such table"));
    }

    #[test]
    fn busy_error_mentions_resume() {
        let err = process_runner::RunnerError::Busy {
            job_id: "job_x".into(),
        };
        let msg = format_runner_error(&err);
        assert!(msg.to_lowercase().contains("resume"));
        assert!(msg.contains("job_x"));
    }
}
