//! Versioned candidate sidecar for scan-once reuse (track 0145).
//!
//! Schema `scan_candidates_v1`. Default `scan --json` is not this file.

use std::fs;
use std::path::{Path, PathBuf};

use dedup_engine::keepset::RecoverableScanItem;
use dedup_engine::{DedupeScope, IdentityLevel, Tier1Verify};
use serde::{Deserialize, Serialize};

use crate::attach_probe::path_mtime_and_size;
use crate::error::{CliError, Result};
use crate::paths::{paths_equal, paths_equal_resolved};
use crate::scan::{ScanOptions, ScanSummary};
use dedup_engine::integrity::ScanMode;

/// Frozen sidecar schema. Reject any other value, including newer.
pub const SCAN_CANDIDATES_SCHEMA: &str = "scan_candidates_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputStat {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
}

/// Canonical identity + scan options captured at emit time.
///
/// Copied from [`dedup_engine::GroupingContext`] + [`ScanOptions`]. Do not serde
/// `GroupingContext` itself.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScanReuseFingerprint {
    pub tier2_enabled: bool,
    pub scope: DedupeScope,
    pub tier1_authority: bool,
    pub require_readable_body: bool,
    pub identity: IdentityLevel,
    pub tier1_verify: Tier1Verify,
    pub allow_degenerate_tier2: bool,
    pub allow_crc_suspect_tier2: bool,
    pub allow_cross_mid_tier2: bool,
    pub tier1_backfill: bool,
    pub ignore_inline_attachments: bool,
    pub include_attachments: bool,
    pub mode: ScanMode,
    pub allow_failed_files: bool,
    pub max_skip_rate: f64,
    pub max_crc_skip_rate: f64,
    pub max_failed_file_rate: f64,
    pub max_attach_fail_rate: f64,
    pub strong_hash_attach_max_attaches: u64,
    pub strong_hash_attach_max_bytes: u64,
    pub strong_hash_attach_per_attach_max_bytes: u64,
    pub deep_attach_preflight: bool,
    pub deep_attach_level: String,
    pub deep_attach_max_attaches: u64,
    pub deep_attach_max_probe_bytes: u64,
    pub deep_attach_per_attach_max_bytes: u64,
    pub deep_attach_max_probe_time_ms: u64,
    pub deep_attach_max_open_psts: usize,
    pub deep_attach_max_peer_probes_per_group: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanCandidatesV1 {
    pub schema: String,
    pub input_files: Vec<String>,
    pub input_path_sort_order: Vec<String>,
    pub inputs_stat: Vec<InputStat>,
    pub fingerprint: ScanReuseFingerprint,
    pub summary: ScanSummary,
    pub candidates: Vec<RecoverableScanItem>,
}

pub fn fingerprint_from_options(opts: &ScanOptions) -> ScanReuseFingerprint {
    let g = &opts.grouping;
    ScanReuseFingerprint {
        tier2_enabled: g.tier2_enabled,
        scope: g.scope,
        tier1_authority: g.tier1_authority,
        require_readable_body: g.require_readable_body,
        identity: g.identity,
        tier1_verify: g.tier1_verify,
        allow_degenerate_tier2: g.allow_degenerate_tier2,
        allow_crc_suspect_tier2: g.allow_crc_suspect_tier2,
        allow_cross_mid_tier2: g.allow_cross_mid_tier2,
        tier1_backfill: g.tier1_backfill,
        ignore_inline_attachments: g.ignore_inline_attachments,
        include_attachments: opts.include_attachments,
        mode: opts.mode,
        allow_failed_files: opts.allow_failed_files,
        max_skip_rate: opts.thresholds.max_skip_rate,
        max_crc_skip_rate: opts.thresholds.max_crc_skip_rate,
        max_failed_file_rate: opts.thresholds.max_failed_file_rate,
        max_attach_fail_rate: opts.thresholds.max_attach_fail_rate,
        strong_hash_attach_max_attaches: opts.strong_hash_attach_max_attaches,
        strong_hash_attach_max_bytes: opts.strong_hash_attach_max_bytes,
        strong_hash_attach_per_attach_max_bytes: opts.strong_hash_attach_per_attach_max_bytes,
        deep_attach_preflight: opts.deep_attach_preflight,
        deep_attach_level: opts.deep_attach_level.clone(),
        deep_attach_max_attaches: opts.deep_attach_max_attaches,
        deep_attach_max_probe_bytes: opts.deep_attach_max_probe_bytes,
        deep_attach_per_attach_max_bytes: opts.deep_attach_per_attach_max_bytes,
        deep_attach_max_probe_time_ms: opts.deep_attach_max_probe_time_ms,
        deep_attach_max_open_psts: opts.deep_attach_max_open_psts,
        deep_attach_max_peer_probes_per_group: opts.deep_attach_max_peer_probes_per_group,
    }
}

pub fn collect_input_stats(paths: &[PathBuf]) -> Result<Vec<InputStat>> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let path = p.display().to_string();
        let (mtime, size) = path_mtime_and_size(&path);
        if mtime == 0 && size == 0 {
            return Err(CliError::Usage(format!(
                "cannot read size/mtime for scan_candidates_v1 input: {path}"
            )));
        }
        out.push(InputStat { path, size, mtime });
    }
    Ok(out)
}

pub fn path_list(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

pub fn write_scan_candidates(path: &Path, artifact: &ScanCandidatesV1) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                CliError::Msg(format!(
                    "create --emit-candidates parent {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }
    let json = serde_json::to_string_pretty(artifact).map_err(CliError::from)?;
    fs::write(path, json)
        .map_err(|e| CliError::Msg(format!("write --emit-candidates {}: {e}", path.display())))?;
    Ok(())
}

pub fn load_scan_candidates(path: &Path) -> Result<ScanCandidatesV1> {
    let raw = fs::read_to_string(path)
        .map_err(|e| CliError::Msg(format!("read --from-scan-json {}: {e}", path.display())))?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        CliError::Usage(format!(
            "--from-scan-json {}: invalid JSON ({e})",
            path.display()
        ))
    })?;
    let schema = value.get("schema").and_then(|s| s.as_str()).unwrap_or("");
    if schema != SCAN_CANDIDATES_SCHEMA {
        let got = if schema.is_empty() {
            "missing schema"
        } else {
            schema
        };
        return Err(CliError::Usage(format!(
            "--from-scan-json expects schema {SCAN_CANDIDATES_SCHEMA} (got {got}; \
             scan --json and keep-set --json envelopes are not reuse artifacts)"
        )));
    }
    serde_json::from_value(value).map_err(|e| {
        CliError::Usage(format!(
            "--from-scan-json {}: not a valid {SCAN_CANDIDATES_SCHEMA} envelope ({e})",
            path.display()
        ))
    })
}

pub fn guard_candidates_path(path: &Path, protected: &[PathBuf]) -> Result<()> {
    if protected
        .iter()
        .any(|p| paths_equal_resolved(path, p) || paths_equal(path, p))
    {
        return Err(CliError::Usage(format!(
            "candidates path collides with an input or output path: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn order_matches(current: &[PathBuf], recorded: &[String]) -> bool {
    if current.len() != recorded.len() {
        return false;
    }
    current.iter().zip(recorded.iter()).all(|(c, r)| {
        let rec = Path::new(r);
        paths_equal(c, rec) || paths_equal_resolved(c, rec)
    })
}

pub fn verify_freshness(paths: &[PathBuf], stats: &[InputStat]) -> Result<()> {
    for p in paths {
        let rec = stats.iter().find(|st| {
            let rec = Path::new(&st.path);
            paths_equal(p, rec) || paths_equal_resolved(p, rec)
        });
        let Some(rec) = rec else {
            return Err(CliError::Usage(format!(
                "--from-scan-json input missing from artifact stats: {}",
                p.display()
            )));
        };
        let key = p.display().to_string();
        let (mtime, size) = path_mtime_and_size(&key);
        if mtime == 0 && size == 0 {
            return Err(CliError::Usage(format!(
                "--from-scan-json cannot read size/mtime for {}",
                p.display()
            )));
        }
        if size != rec.size || mtime != rec.mtime {
            return Err(CliError::Usage(format!(
                "--from-scan-json stale input (size/mtime changed): {}",
                p.display()
            )));
        }
    }
    Ok(())
}

pub fn format_candidates_line(path: &Path, n: usize) -> String {
    format!("  candidates: {} ({n} items)", path.display())
}

pub fn format_from_candidates_line(path: &Path, n: usize) -> String {
    format!("  from_candidates: {} ({n} items)", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dedup_engine::integrity::IntegrityThresholds;
    use dedup_engine::GroupingContext;

    fn dummy_opts() -> ScanOptions {
        ScanOptions {
            grouping: GroupingContext::default(),
            thresholds: IntegrityThresholds::default(),
            ..ScanOptions::default()
        }
    }

    #[test]
    fn fingerprint_roundtrip_eq() {
        let opts = dummy_opts();
        let fp = fingerprint_from_options(&opts);
        let json = serde_json::to_string(&fp).expect("ser");
        let back: ScanReuseFingerprint = serde_json::from_str(&json).expect("de");
        assert_eq!(fp, back);
    }

    #[test]
    fn load_rejects_wrong_schema() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("scan.json");
        fs::write(
            &p,
            r#"{"ok":true,"schema":"scan_integrity_v1","summary":{}}"#,
        )
        .expect("write");
        let err = load_scan_candidates(&p).expect_err("schema");
        let msg = err.to_string();
        assert!(msg.contains(SCAN_CANDIDATES_SCHEMA), "{msg}");
        assert!(msg.contains("scan_integrity_v1"), "{msg}");
    }

    #[test]
    fn load_rejects_extra_envelope_key() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("c.json");
        // Minimal invalid extra key; schema check passes then deny_unknown_fields.
        fs::write(
            &p,
            r#"{"schema":"scan_candidates_v1","extra":1,"input_files":[],"input_path_sort_order":[],"inputs_stat":[],"fingerprint":{},"summary":{},"candidates":[]}"#,
        )
        .expect("write");
        let err = load_scan_candidates(&p).expect_err("extra");
        assert!(err.to_string().contains("scan_candidates_v1"));
    }
}
