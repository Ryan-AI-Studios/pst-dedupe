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
