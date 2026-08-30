//! `matter_overview` — one read-only overview on a blocking worker.

use std::fs;
use std::io;
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::{is_encrypted_matter, load_case_overview_on, Matter, OverviewOptions};
use serde::Serialize;

use crate::error::CommandError;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatterOverviewResponse {
    pub name: String,
    pub matter_id: String,
    pub schema_version: u32,
    pub generated_at: String,
    pub sources: u64,
    pub processed: u64,
    pub exceptions: u64,
    pub unreviewed: u64,
    pub privileged: u64,
    pub withhold: u64,
    pub custodians: u64,
    pub custodians_plus: bool,
    pub other_custodians_item_count: u64,
    pub produced: u64,
}

/// Map root `metadata` failures: only true absence is `not_found`.
pub(crate) fn map_root_metadata_err(root: &str, err: io::Error) -> CommandError {
    if err.kind() == io::ErrorKind::NotFound {
        CommandError::not_found(format!("Matter root not found: {root}"))
    } else {
        CommandError::failed(format!("Cannot access matter root {root}: {err}"))
    }
}

fn ensure_root_accessible(root: &str) -> Result<(), CommandError> {
    match fs::metadata(Path::new(root)) {
        Ok(_) => Ok(()),
        Err(e) => Err(map_root_metadata_err(root, e)),
    }
}

pub fn matter_overview_blocking(root: &str) -> Result<MatterOverviewResponse, CommandError> {
    ensure_root_accessible(root)?;
    let utf8 = Utf8PathBuf::from_path_buf(Path::new(root).to_path_buf())
        .map_err(|_| CommandError::failed(format!("Matter root is not valid UTF-8: {root}")))?;
    load_overview_at(&utf8)
}

fn load_overview_at(root: &Utf8Path) -> Result<MatterOverviewResponse, CommandError> {
    // Fail closed before any open_* so encrypted roots never hit PassphraseRequired.
    if is_encrypted_matter(root) {
        return Err(CommandError::encrypted(
            "Encrypted matters are not opened in this chrome; use Dedupe Desk.",
        ));
    }
    let matter = Matter::open_for_read(root).map_err(|e| CommandError::failed(e.to_string()))?;
    let info = matter
        .info()
        .map_err(|e| CommandError::failed(e.to_string()))?;
    let ov = load_case_overview_on(&matter, &OverviewOptions::default())
        .map_err(|e| CommandError::failed(e.to_string()))?;
    let produced = matter
        .count_produced_items()
        .map_err(|e| CommandError::failed(e.to_string()))?;
    Ok(MatterOverviewResponse {
        name: info.name,
        matter_id: info.id,
        schema_version: info.schema_version,
        generated_at: ov.generated_at,
        sources: ov.totals.sources_total,
        processed: ov.totals.top_level_items,
        exceptions: ov.errors.total,
        unreviewed: ov.review.unreviewed_count,
        privileged: ov.privilege.claimed,
        withhold: ov.privilege.withhold,
        custodians: ov.by_custodian.len() as u64,
        custodians_plus: ov.other_custodians_count > 0,
        other_custodians_item_count: ov.other_custodians_count,
        produced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use matter_core::SCHEMA_VERSION;
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    #[test]
    fn empty_matter_zeros() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "EmptyOv").expect("create");
        let resp = matter_overview_blocking(root.as_str()).expect("overview");
        assert_eq!(resp.sources, 0);
        assert_eq!(resp.processed, 0);
        assert_eq!(resp.exceptions, 0);
        assert_eq!(resp.unreviewed, 0);
        assert_eq!(resp.privileged, 0);
        assert_eq!(resp.withhold, 0);
        assert_eq!(resp.custodians, 0);
        assert!(!resp.custodians_plus);
        assert_eq!(resp.produced, 0);
        assert_eq!(resp.schema_version, SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 39);
        assert!(!resp.generated_at.is_empty());
        assert_eq!(resp.name, "EmptyOv");
    }

    #[test]
    fn insert_source_bumps_sources_processed_stays_zero() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "SrcOv").expect("create");
        {
            let matter = Matter::open(&root).expect("open write");
            matter
                .insert_source(r"C:\exports\synth", "folder", "imported", None)
                .expect("insert_source");
        }
        let resp = matter_overview_blocking(root.as_str()).expect("overview");
        assert_eq!(resp.sources, 1, "Sources chip must reflect insert_source");
        assert_eq!(
            resp.processed, 0,
            "Processed is top_level_items; insert_source alone must leave it 0"
        );
    }

    #[test]
    fn missing_root_not_found() {
        let tmp = tempdir().expect("tempdir");
        let missing = tmp.path().join("no-such-matter");
        let err = matter_overview_blocking(&missing.to_string_lossy()).expect_err("missing");
        assert_eq!(err.kind, "not_found");
    }

    #[test]
    fn metadata_permission_denied_maps_to_failed() {
        // Windows admin-locked paths are awkward in CI; map the ErrorKind contract directly.
        let err = map_root_metadata_err(
            r"C:\locked\matter",
            io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        );
        assert_eq!(err.kind, "failed");
        assert!(err.message.to_lowercase().contains("access"));
    }

    #[test]
    fn metadata_not_found_maps_to_not_found() {
        let err = map_root_metadata_err(
            r"C:\missing\matter",
            io::Error::new(io::ErrorKind::NotFound, "gone"),
        );
        assert_eq!(err.kind, "not_found");
    }

    #[test]
    fn encrypted_returns_kind_without_open() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = parent.join("EncOv");
        {
            let _m = Matter::create_encrypted(&root, "EncOv", "test-passphrase-0110")
                .expect("create encrypted");
        }
        assert!(is_encrypted_matter(&root));
        let err = matter_overview_blocking(root.as_str()).expect_err("encrypted");
        assert_eq!(err.kind, "encrypted");
        // Fail-closed copy must not imply a passphrase dialog exists here.
        assert!(!err.message.to_lowercase().contains("passphrase"));
    }

    #[test]
    fn csp_config_has_wasm_unsafe_eval() {
        let conf = include_str!("../tauri.conf.json");
        let v: serde_json::Value = serde_json::from_str(conf).expect("parse tauri.conf.json");
        let script_src = v["app"]["security"]["csp"]["script-src"]
            .as_str()
            .expect("script-src string");
        assert!(
            script_src.contains("'wasm-unsafe-eval'"),
            "script-src must include 'wasm-unsafe-eval', got {script_src}"
        );
        let connect = v["app"]["security"]["csp"]["connect-src"]
            .as_str()
            .expect("connect-src");
        assert!(connect.contains("ipc:"));
        assert!(connect.contains("http://ipc.localhost"));
        let conf_l = conf.to_lowercase();
        assert!(!conf_l.contains("fonts.googleapis.com"));
        assert!(!conf_l.contains("fonts.gstatic.com"));
    }
}
