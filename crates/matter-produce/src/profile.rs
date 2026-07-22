//! Resolve production profiles and apply job-param overlays (track **0060**).
//!
//! Precedence: **job param > profile > engine default**.
//! Bates start is **job-only** and never taken from a profile.

use matter_core::{
    builtin_production_profile, default_production_profile_body, production_profile_config_hash,
    Matter, ProductionProfile, ProductionProfileBody, BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1,
};
use serde::{Deserialize, Serialize};

use crate::error::{ProduceError, Result};
use crate::params::ProduceParams;

/// Resolved production packaging config after profile + job overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedProduceConfig {
    /// Profile slug used (built-in or matter-local).
    pub profile_slug: String,
    /// Profile id (`builtin:…` or user id).
    pub profile_id: String,
    /// SHA-256 of the resolved profile body JSON (before job overlay of packaging).
    pub config_hash: String,
    /// Effective body after job param overlay (bates prefix/pad, packaging, qc).
    pub body: ProductionProfileBody,
    /// Job-time Bates start (1-based sequence).
    pub bates_start: u64,
}

/// Resolve profile for a produce run.
///
/// - Missing / empty `production_profile` → default built-in
///   [`BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1`].
/// - Job params override profile fields (prefix, pad, packaging, require_qc_pass).
/// - `bates_start` always comes from job params (default 1).
pub fn resolve_produce_config(
    matter: &Matter,
    params: &ProduceParams,
) -> Result<ResolvedProduceConfig> {
    let slug = params
        .production_profile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1);

    let profile = load_profile(matter, slug)?;

    let mut body = profile.body.clone();
    apply_job_overlay(&mut body, params);

    // Re-validate after overlay (prefix/pad constraints).
    matter_core::validate_production_profile_body(&body)
        .map_err(|e| ProduceError::InvalidParams(format!("resolved profile invalid: {e}")))?;

    // Hash the **post-overlay** body so job overrides are part of audit identity.
    let config_hash = production_profile_config_hash(&body)
        .map_err(|e| ProduceError::InvalidParams(e.to_string()))?;

    let bates_start = params.bates_start.filter(|&n| n >= 1).ok_or_else(|| {
        ProduceError::InvalidParams("bates_start is required (job-time Bates start >= 1)".into())
    })?;

    Ok(ResolvedProduceConfig {
        profile_slug: profile.slug,
        profile_id: profile.id,
        config_hash,
        body,
        bates_start,
    })
}

fn load_profile(matter: &Matter, slug: &str) -> Result<ProductionProfile> {
    // Prefer matter API (union of built-in + local).
    match matter.get_production_profile(slug) {
        Ok(p) => Ok(p),
        Err(_) => {
            // Fall back to built-in only (e.g. schema older than v38 should not happen
            // when SCHEMA_VERSION is 38, but keep a clear error).
            if let Some(p) = builtin_production_profile(slug) {
                Ok(p)
            } else if slug == BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1 {
                Ok(ProductionProfile {
                    id: matter_core::production_builtin_id(BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1),
                    matter_id: None,
                    slug: BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1.into(),
                    label: "US Concordance native+text (default)".into(),
                    jurisdiction_tag: Some("us_federal".into()),
                    body: default_production_profile_body(),
                    is_builtin: true,
                    created_at: None,
                    updated_at: None,
                })
            } else {
                Err(ProduceError::InvalidParams(format!(
                    "production profile not found: {slug}"
                )))
            }
        }
    }
}

/// Apply job params over profile body (job wins when the job field is set).
///
/// Packaging bools and `require_qc_pass` are `Option` on [`ProduceParams`]:
/// `None` means the job omitted the key, so the **profile value is kept**.
/// Bates prefix/pad still apply from job defaults (engine default layer matches
/// built-in profiles; operators override via explicit prefix/seq_width).
fn apply_job_overlay(body: &mut ProductionProfileBody, params: &ProduceParams) {
    if let Some(prefix) = params.bates_prefix_clean() {
        body.bates.prefix = prefix.to_string();
    }
    if let Some(w) = params.seq_width {
        if w > 0 && w <= 12 {
            body.bates.pad_width = w;
        }
    }
    if let Some(v) = params.include_csv_twin {
        body.packaging.include_csv_twin = v;
    }
    if let Some(v) = params.export_eml_if_missing_native {
        body.packaging.export_eml_if_missing_native = v;
    }
    if let Some(v) = params.expand_family {
        body.packaging.expand_family = v;
    }
    if let Some(v) = params.require_qc_pass {
        body.qc.require_qc_pass = v;
    }
    // Optional explicit pack_id override on produce params.
    if let Some(ref pack) = params.qc_pack_id {
        let t = pack.trim();
        if !t.is_empty() {
            body.qc.pack_id = matter_core::normalize_qc_pack_id(t);
        }
    }
}

/// Effective Bates prefix after resolve.
pub fn effective_bates_prefix(cfg: &ResolvedProduceConfig) -> &str {
    cfg.body.bates.prefix.trim()
}

/// Effective pad width after resolve.
pub fn effective_pad_width(cfg: &ResolvedProduceConfig) -> u32 {
    cfg.body.bates.pad_width
}

/// Effective QC pack id after resolve.
pub fn effective_qc_pack_id(cfg: &ResolvedProduceConfig) -> String {
    matter_core::normalize_qc_pack_id(&cfg.body.qc.pack_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::{Matter, BUILTIN_US_CONCORDANCE_REL_ALIAS_V1};

    fn temp_matter() -> (tempfile::TempDir, Matter) {
        let tmp = tempfile::tempdir().unwrap();
        let path = camino::Utf8Path::from_path(tmp.path()).unwrap().join("m");
        let matter = Matter::create(&path, "t").unwrap();
        (tmp, matter)
    }

    #[test]
    fn default_profile_when_unset() {
        let (_t, matter) = temp_matter();
        let params = ProduceParams::default();
        let cfg = resolve_produce_config(&matter, &params).unwrap();
        assert_eq!(cfg.profile_slug, BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1);
        assert_eq!(cfg.bates_start, 1);
        assert!(!cfg.config_hash.is_empty());
    }

    #[test]
    fn job_prefix_overrides_profile() {
        let (_t, matter) = temp_matter();
        let params = ProduceParams {
            bates_prefix: Some("ACME".into()),
            bates_start: Some(5001),
            ..Default::default()
        };
        let cfg = resolve_produce_config(&matter, &params).unwrap();
        assert_eq!(effective_bates_prefix(&cfg), "ACME");
        assert_eq!(cfg.bates_start, 5001);
    }

    #[test]
    fn omitted_job_prefix_keeps_profile_prefix() {
        use matter_core::{
            production_profile_body_to_json, ProductionProfileInput, QC_PACK_DEFAULT_V1,
        };

        let (_t, matter) = temp_matter();
        let mut body = matter_core::default_production_profile_body();
        body.bates.prefix = "ACME".into();
        body.bates.pad_width = 8;
        body.qc.pack_id = QC_PACK_DEFAULT_V1.into();
        matter
            .upsert_production_profile(ProductionProfileInput {
                id: None,
                slug: "firm_acme".into(),
                label: "Firm ACME".into(),
                jurisdiction_tag: None,
                body_json: production_profile_body_to_json(&body).unwrap(),
            })
            .unwrap();
        let params = ProduceParams {
            production_profile: Some("firm_acme".into()),
            bates_start: Some(1),
            // prefix/pad omitted → profile wins
            ..Default::default()
        };
        let cfg = resolve_produce_config(&matter, &params).unwrap();
        assert_eq!(effective_bates_prefix(&cfg), "ACME");
        assert_eq!(effective_pad_width(&cfg), 8);
    }

    #[test]
    fn rel_alias_profile_loads() {
        let (_t, matter) = temp_matter();
        let params = ProduceParams {
            production_profile: Some(BUILTIN_US_CONCORDANCE_REL_ALIAS_V1.into()),
            ..Default::default()
        };
        let cfg = resolve_produce_config(&matter, &params).unwrap();
        assert_eq!(cfg.profile_slug, BUILTIN_US_CONCORDANCE_REL_ALIAS_V1);
        assert_eq!(cfg.body.load_file.dialect, "relativity_field_alias_v1");
    }

    #[test]
    fn matter_local_packaging_survives_empty_job_overlay() {
        use matter_core::{
            production_profile_body_to_json, ProductionProfileInput, QC_PACK_DEFAULT_V1,
        };

        let (_t, matter) = temp_matter();
        let mut body = matter_core::default_production_profile_body();
        body.packaging.include_csv_twin = false;
        body.packaging.export_eml_if_missing_native = false;
        body.qc.pack_id = QC_PACK_DEFAULT_V1.into();
        let body_json = production_profile_body_to_json(&body).unwrap();
        matter
            .upsert_production_profile(ProductionProfileInput {
                id: None,
                slug: "firm_no_csv".into(),
                label: "Firm no CSV".into(),
                jurisdiction_tag: Some("us_federal".into()),
                body_json,
            })
            .unwrap();
        let params = ProduceParams {
            production_profile: Some("firm_no_csv".into()),
            // Job omits packaging knobs (None) — profile must win.
            ..Default::default()
        };
        let cfg = resolve_produce_config(&matter, &params).unwrap();
        assert!(!cfg.body.packaging.include_csv_twin);
        assert!(!cfg.body.packaging.export_eml_if_missing_native);
    }
}
