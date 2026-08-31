//! Pure JSON param builders for chrome `process_start` (Desk shapes, no desk import).

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

/// Build `profile_run` job params from a profile id (`builtin:standard`, …).
pub fn profile_run_params(profile_id: &str) -> String {
    serde_json::json!({
        "profile_id": profile_id,
        "stop_on_stage_failure": true
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_params_shape() {
        let v: serde_json::Value =
            serde_json::from_str(&ingest_params(r"C:\exports\pkg.zip")).unwrap();
        assert_eq!(v["path"], r"C:\exports\pkg.zip");
        assert!(v.get("source_id").is_none());
    }

    #[test]
    fn extract_pst_item_params_shape() {
        let v: serde_json::Value =
            serde_json::from_str(&extract_pst_item_params("src_1", "itm_pst")).unwrap();
        assert_eq!(v["source_id"], "src_1");
        assert_eq!(v["pst_item_id"], "itm_pst");
    }

    #[test]
    fn profile_run_params_builtin_standard_shape() {
        let v: serde_json::Value =
            serde_json::from_str(&profile_run_params("builtin:standard")).unwrap();
        assert_eq!(v["profile_id"], "builtin:standard");
        assert_eq!(v["stop_on_stage_failure"], true);
        assert!(v.get("profile_name").is_none());
    }
}
