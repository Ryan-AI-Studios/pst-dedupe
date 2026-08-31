//! Built-in production QC rules and default pack.

use std::collections::{HashMap, HashSet};

use matter_core::{
    burn_required_pdf_known, burned_native_fresh, item_looks_like_pdf, Item, Matter,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::params::{QcRuleConfig, QcSeverity, PROFILE_DEFAULT_PRODUCTION_QC_V1};

// ---------------------------------------------------------------------------
// Rule ids
// ---------------------------------------------------------------------------

pub const RULE_BROKEN_FAMILY_ORPHAN_CHILD: &str = "broken_family_orphan_child";
pub const RULE_BROKEN_FAMILY_INCOMPLETE_PARENT: &str = "broken_family_incomplete_parent";
pub const RULE_WITHHELD_IN_SELECTION: &str = "withheld_in_selection";
pub const RULE_WITHHELD_FAMILY_MEMBER: &str = "withheld_family_member";
pub const RULE_REDACTED_TEXT_MISSING: &str = "redacted_text_missing";
pub const RULE_BURNED_NATIVE_MISSING: &str = "burned_native_missing";
pub const RULE_TEXT_REDACT_UNMAPPED_ON_PDF: &str = "text_redact_unmapped_on_pdf";
pub const RULE_MISSING_NATIVE: &str = "missing_native";
pub const RULE_MISSING_TEXT: &str = "missing_text";
pub const RULE_PDF_NEEDS_OCR: &str = "pdf_needs_ocr";
pub const RULE_ZERO_SIZE: &str = "zero_size";
pub const RULE_ITEM_STATUS_ERROR: &str = "item_status_error";
pub const RULE_EMPTY_SELECTION: &str = "empty_selection";
pub const RULE_ONLY_WITHHELD: &str = "only_withheld";
pub const RULE_IMAGE_PAGE_MISSING: &str = "image_page_missing";
pub const RULE_BEG_END_BATES_SPAN: &str = "beg_end_bates_span";
pub const RULE_OPT_ROW_COUNT_MISMATCH: &str = "opt_row_count_mismatch";
pub const RULE_IMAGE_SKIPPED_NATIVE_ONLY: &str = "image_skipped_native_only";
pub const RULE_MULTI_PAGE_TIFF_AS_ARTIFACT: &str = "multi_page_tiff_as_artifact";

/// One finding from a rule evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QcFinding {
    pub rule_id: String,
    pub severity: QcSeverity,
    pub item_id: Option<String>,
    /// Short stable phrase only — never subject/body/paths.
    pub message: String,
}

/// Resolved severity map for evaluation (Off entries still present for lookup).
#[derive(Debug, Clone)]
pub struct ResolvedRules {
    pub profile: String,
    by_id: HashMap<String, QcSeverity>,
}

impl ResolvedRules {
    pub fn severity(&self, rule_id: &str) -> QcSeverity {
        self.by_id.get(rule_id).copied().unwrap_or(QcSeverity::Off)
    }

    pub fn is_enabled(&self, rule_id: &str) -> bool {
        self.severity(rule_id) != QcSeverity::Off
    }

    pub fn to_configs(&self) -> Vec<QcRuleConfig> {
        let mut out: Vec<_> = self
            .by_id
            .iter()
            .map(|(id, sev)| QcRuleConfig {
                id: id.clone(),
                severity: *sev,
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

/// Default pack `qc_default_v1` (alias of legacy `default_production_qc_v1`).
pub fn default_rule_pack() -> Vec<QcRuleConfig> {
    crate::packs::pack_default_v1()
}

/// Merge operator overrides over the default pack.
///
/// Unknown rule ids in overrides are accepted (forward-compatible) but ignored
/// by the evaluator if no matching rule exists.
pub fn resolve_rules(overrides: &[QcRuleConfig]) -> ResolvedRules {
    resolve_rules_for_pack(PROFILE_DEFAULT_PRODUCTION_QC_V1, overrides)
}

/// Merge a named pack + operator overrides.
///
/// `pack_id` accepts canonical ids (`qc_default_v1`, `qc_strict_privilege_v1`, …)
/// and the legacy `default_production_qc_v1` alias. Unknown packs resolve to an
/// empty severity map — callers must validate via [`QcParams::validate_shape`]
/// (fail closed). Prefer known packs only.
pub fn resolve_rules_for_pack(pack_id: &str, overrides: &[QcRuleConfig]) -> ResolvedRules {
    let canonical = crate::packs::canonical_pack_id(pack_id);
    let mut by_id =
        if crate::packs::is_known_pack_id(pack_id) || crate::packs::is_known_pack_id(&canonical) {
            crate::packs::merge_pack_with_overrides(&canonical, overrides)
        } else {
            // Fail-closed: no silent default severities for typos.
            std::collections::HashMap::new()
        };
    // Known pack should never be empty; if it is, use default pack as safety net
    // for the known-id path only.
    if by_id.is_empty()
        && (crate::packs::is_known_pack_id(pack_id) || crate::packs::is_known_pack_id(&canonical))
    {
        by_id = default_rule_pack()
            .into_iter()
            .map(|r| (r.id, r.severity))
            .collect();
        for r in overrides {
            by_id.insert(r.id.clone(), r.severity);
        }
    }
    ResolvedRules {
        profile: canonical,
        by_id,
    }
}

/// Categories where missing text is an **error** (case-insensitive).
fn missing_text_is_error_category(cat: Option<&str>) -> bool {
    matches!(
        cat.map(|c| c.to_ascii_lowercase()).as_deref(),
        Some("email") | Some("document") | Some("spreadsheet") | Some("presentation") | Some("pdf")
    )
}

/// Whether item can use export-only EML (produce-like).
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

fn digest_present(d: Option<&str>) -> bool {
    d.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

fn usable_text(item: &Item) -> bool {
    digest_present(item.text_sha256.as_deref())
        || digest_present(item.redacted_text_sha256.as_deref())
}

/// Set-level empty-selection finding (if enabled and selection is empty).
pub fn empty_selection_finding(rules: &ResolvedRules) -> Option<QcFinding> {
    if rules.is_enabled(RULE_EMPTY_SELECTION) {
        Some(QcFinding {
            rule_id: RULE_EMPTY_SELECTION.into(),
            severity: rules.severity(RULE_EMPTY_SELECTION),
            item_id: None,
            message: "empty selection".into(),
        })
    } else {
        None
    }
}

/// Set-level only-withheld finding after a full scan of candidates.
pub fn only_withheld_finding(
    rules: &ResolvedRules,
    candidate_count: u64,
    withheld_count: u64,
) -> Option<QcFinding> {
    if rules.is_enabled(RULE_ONLY_WITHHELD)
        && candidate_count > 0
        && withheld_count == candidate_count
    {
        Some(QcFinding {
            rule_id: RULE_ONLY_WITHHELD.into(),
            severity: rules.severity(RULE_ONLY_WITHHELD),
            item_id: None,
            message: "all candidates withheld".into(),
        })
    } else {
        None
    }
}

/// Evaluate per-item rules for one candidate (not set-level rules).
///
/// `candidate_set` is the frozen full selection (including not-yet-evaluated ids).
pub fn evaluate_one_item(
    matter: &Matter,
    item: &Item,
    is_withheld: bool,
    candidate_set: &HashSet<&str>,
    rules: &ResolvedRules,
) -> Result<Vec<QcFinding>> {
    let mut findings = Vec::new();
    let id = item.id.as_str();

    // orphan child
    if rules.is_enabled(RULE_BROKEN_FAMILY_ORPHAN_CHILD) {
        if let Some(parent) = item.parent_item_id.as_deref() {
            if !candidate_set.contains(parent) {
                findings.push(QcFinding {
                    rule_id: RULE_BROKEN_FAMILY_ORPHAN_CHILD.into(),
                    severity: rules.severity(RULE_BROKEN_FAMILY_ORPHAN_CHILD),
                    item_id: Some(id.into()),
                    message: "orphan child: parent not in selection".into(),
                });
            }
        }
    }

    // incomplete parent: any non-withheld child not in set
    if rules.is_enabled(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT)
        && has_missing_non_withheld_child(matter, id, candidate_set)?
    {
        findings.push(QcFinding {
            rule_id: RULE_BROKEN_FAMILY_INCOMPLETE_PARENT.into(),
            severity: rules.severity(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT),
            item_id: Some(id.into()),
            message: "incomplete family: non-withheld child missing from selection".into(),
        });
    }

    // withheld in selection
    if rules.is_enabled(RULE_WITHHELD_IN_SELECTION) && is_withheld {
        findings.push(QcFinding {
            rule_id: RULE_WITHHELD_IN_SELECTION.into(),
            severity: rules.severity(RULE_WITHHELD_IN_SELECTION),
            item_id: Some(id.into()),
            message: "withheld item in selection".into(),
        });
    }

    // withheld family member (candidate not withheld, parent or child is)
    if rules.is_enabled(RULE_WITHHELD_FAMILY_MEMBER)
        && !is_withheld
        && family_has_withheld_relative(matter, item)?
    {
        findings.push(QcFinding {
            rule_id: RULE_WITHHELD_FAMILY_MEMBER.into(),
            severity: rules.severity(RULE_WITHHELD_FAMILY_MEMBER),
            item_id: Some(id.into()),
            message: "family member withheld".into(),
        });
    }

    // redacted text missing
    if rules.is_enabled(RULE_REDACTED_TEXT_MISSING)
        && item.redaction_count > 0
        && !digest_present(item.redacted_text_sha256.as_deref())
    {
        findings.push(QcFinding {
            rule_id: RULE_REDACTED_TEXT_MISSING.into(),
            severity: rules.severity(RULE_REDACTED_TEXT_MISSING),
            item_id: Some(id.into()),
            message: "redaction without redacted text artifact".into(),
        });
    }

    // burned native missing / stale fingerprint
    if rules.is_enabled(RULE_BURNED_NATIVE_MISSING) {
        let fp = matter.geom_burn_fingerprint(id)?;
        let is_pdf = matter_core::item_is_pdf_native(matter, item)?;
        if burn_required_pdf_known(item, &fp, is_pdf) && !burned_native_fresh(item, &fp) {
            findings.push(QcFinding {
                rule_id: RULE_BURNED_NATIVE_MISSING.into(),
                severity: rules.severity(RULE_BURNED_NATIVE_MISSING),
                item_id: Some(id.into()),
                message: "burn required without fresh burned native".into(),
            });
        }
    }

    // PDF text redaction with no geometric mapping
    let is_pdf = item_looks_like_pdf(item)
        || (item.redaction_count > 0 && matter_core::item_is_pdf_native(matter, item)?);
    if rules.is_enabled(RULE_TEXT_REDACT_UNMAPPED_ON_PDF)
        && is_pdf
        && item.redaction_count > 0
        && item.geom_redaction_count == 0
    {
        findings.push(QcFinding {
            rule_id: RULE_TEXT_REDACT_UNMAPPED_ON_PDF.into(),
            severity: rules.severity(RULE_TEXT_REDACT_UNMAPPED_ON_PDF),
            item_id: Some(id.into()),
            message: "pdf text redaction with no geometric boxes".into(),
        });
    }

    // missing native (non-email)
    if rules.is_enabled(RULE_MISSING_NATIVE)
        && !digest_present(item.native_sha256.as_deref())
        && !is_email_like(item)
    {
        findings.push(QcFinding {
            rule_id: RULE_MISSING_NATIVE.into(),
            severity: rules.severity(RULE_MISSING_NATIVE),
            item_id: Some(id.into()),
            message: "missing native for non-email item".into(),
        });
    }

    // missing text (taxonomy-aware)
    if rules.is_enabled(RULE_MISSING_TEXT) && !usable_text(item) {
        let configured = rules.severity(RULE_MISSING_TEXT);
        // Off already filtered by is_enabled.
        // If configured Error → force error; if Warn → use taxonomy.
        let taxonomy = if missing_text_is_error_category(item.file_category.as_deref()) {
            QcSeverity::Error
        } else {
            QcSeverity::Warn
        };
        // Off already filtered by is_enabled; Error forces error; Warn uses taxonomy.
        if configured != QcSeverity::Off {
            let severity = if configured == QcSeverity::Error {
                QcSeverity::Error
            } else {
                taxonomy
            };
            findings.push(QcFinding {
                rule_id: RULE_MISSING_TEXT.into(),
                severity,
                item_id: Some(id.into()),
                message: "missing usable text".into(),
            });
        }
    }

    // pdf needs ocr
    if rules.is_enabled(RULE_PDF_NEEDS_OCR) && item.pdf_needs_ocr == 1 {
        findings.push(QcFinding {
            rule_id: RULE_PDF_NEEDS_OCR.into(),
            severity: rules.severity(RULE_PDF_NEEDS_OCR),
            item_id: Some(id.into()),
            message: "pdf needs ocr".into(),
        });
    }

    // zero size
    if rules.is_enabled(RULE_ZERO_SIZE) {
        if let Some(sz) = item.size_bytes {
            if sz == 0 {
                findings.push(QcFinding {
                    rule_id: RULE_ZERO_SIZE.into(),
                    severity: rules.severity(RULE_ZERO_SIZE),
                    item_id: Some(id.into()),
                    message: "zero size_bytes".into(),
                });
            }
        }
    }

    // Native-only kinds in an image-profile pack (Warn, never Error on this rule).
    if rules.is_enabled(RULE_IMAGE_SKIPPED_NATIVE_ONLY) && is_image_native_only_kind(item) {
        findings.push(QcFinding {
            rule_id: RULE_IMAGE_SKIPPED_NATIVE_ONLY.into(),
            severity: rules.severity(RULE_IMAGE_SKIPPED_NATIVE_ONLY),
            item_id: Some(id.into()),
            message: "native-only; no print-to-TIFF".into(),
        });
    }

    // item status error/partial
    if rules.is_enabled(RULE_ITEM_STATUS_ERROR) {
        let st = item.status.to_ascii_lowercase();
        if st == "error" || st == "partial" {
            findings.push(QcFinding {
                rule_id: RULE_ITEM_STATUS_ERROR.into(),
                severity: rules.severity(RULE_ITEM_STATUS_ERROR),
                item_id: Some(id.into()),
                message: format!("item status {st}"),
            });
        }
    }

    Ok(findings)
}

/// Evaluate all enabled rules against the candidate set.
///
/// When `cancel` returns true between items, returns [`crate::QcError::Cancelled`].
pub fn evaluate_candidates(
    matter: &Matter,
    candidate_ids: &[String],
    rules: &ResolvedRules,
) -> Result<Vec<QcFinding>> {
    evaluate_candidates_with_cancel(matter, candidate_ids, rules, None)
}

/// Like [`evaluate_candidates`], checking `cancel` every item (load + per-item rules).
pub fn evaluate_candidates_with_cancel(
    matter: &Matter,
    candidate_ids: &[String],
    rules: &ResolvedRules,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<Vec<QcFinding>> {
    let mut findings = Vec::new();
    let candidate_set: HashSet<&str> = candidate_ids.iter().map(String::as_str).collect();

    // Set-level: empty selection
    if candidate_ids.is_empty() {
        if let Some(f) = empty_selection_finding(rules) {
            findings.push(f);
        }
        return Ok(findings);
    }

    let mut withheld_count: u64 = 0;
    for id in candidate_ids {
        if cancel.map(|c| c()).unwrap_or(false) {
            return Err(crate::QcError::Cancelled);
        }
        let item = matter.get_item(id)?;
        let is_withheld = matter.item_is_withheld(id)?;
        if is_withheld {
            withheld_count += 1;
        }
        findings.extend(evaluate_one_item(
            matter,
            &item,
            is_withheld,
            &candidate_set,
            rules,
        )?);
    }

    if let Some(f) = only_withheld_finding(rules, candidate_ids.len() as u64, withheld_count) {
        findings.push(f);
    }

    findings.extend(evaluate_image_volume_rules(matter, rules, candidate_ids)?);

    Ok(findings)
}

/// Spreadsheet / email / OOXML: DAT native only (no OPT / TIFF).
fn is_image_native_only_kind(item: &Item) -> bool {
    if is_email_like(item) {
        return true;
    }
    let path = item.path.as_deref().unwrap_or("").to_ascii_lowercase();
    let mime = item.mime_type.as_deref().unwrap_or("").to_ascii_lowercase();
    let cat = item
        .file_category
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext_hit = path.ends_with(".xls")
        || path.ends_with(".xlsx")
        || path.ends_with(".csv")
        || path.ends_with(".tsv")
        || path.ends_with(".docx")
        || path.ends_with(".pptx");
    let mime_hit = mime.contains("spreadsheet")
        || mime.contains("excel")
        || mime.contains("csv")
        || mime.contains("officedocument.wordprocessing")
        || mime.contains("officedocument.presentation");
    let cat_hit = cat == "spreadsheet" || cat == "presentation";
    ext_hit || mime_hit || cat_hit
}

/// PDF / raster image natives that must have TIFF pages on an image volume.
fn is_qc_image_eligible(item: &Item, bytes: &[u8]) -> bool {
    if is_image_native_only_kind(item) {
        return false;
    }
    let path = item.path.as_deref().unwrap_or("").to_ascii_lowercase();
    let mime = item.mime_type.as_deref().unwrap_or("").to_ascii_lowercase();
    if path.ends_with(".pdf")
        || path.ends_with(".tif")
        || path.ends_with(".tiff")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".png")
        || mime.contains("application/pdf")
        || mime.contains("image/tiff")
        || mime.contains("image/tif")
        || mime.contains("image/jpeg")
        || mime.contains("image/jpg")
        || mime.contains("image/png")
    {
        return true;
    }
    looks_like_pdf_magic(bytes)
        || looks_like_jpeg_magic(bytes)
        || looks_like_png_magic(bytes)
        || looks_like_tiff_magic(bytes)
}

fn looks_like_pdf_magic(bytes: &[u8]) -> bool {
    let n = bytes.len().min(16);
    bytes[..n].windows(5).any(|w| w == b"%PDF-")
}

fn looks_like_jpeg_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8
}

fn looks_like_png_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47
}

fn looks_like_tiff_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && ((bytes[0] == b'I' && bytes[1] == b'I' && bytes[2] == 0x2A && bytes[3] == 0)
            || (bytes[0] == b'M' && bytes[1] == b'M' && bytes[2] == 0 && bytes[3] == 0x2A))
}

/// Post-volume image rules. Skip when no production volume / OPT exists so
/// preflight on an empty corpus does not Error because IMAGE.opt is absent.
pub(crate) fn evaluate_image_volume_rules(
    matter: &Matter,
    rules: &ResolvedRules,
    candidate_ids: &[String],
) -> Result<Vec<QcFinding>> {
    let need_span = rules.is_enabled(RULE_BEG_END_BATES_SPAN);
    let need_missing = rules.is_enabled(RULE_IMAGE_PAGE_MISSING);
    let need_opt = rules.is_enabled(RULE_OPT_ROW_COUNT_MISMATCH);
    let need_multi = rules.is_enabled(RULE_MULTI_PAGE_TIFF_AS_ARTIFACT);
    if !need_span && !need_missing && !need_opt && !need_multi {
        return Ok(Vec::new());
    }
    // Empty candidate list means empty selection, not "all items".
    // Matches evaluate_candidates_with_cancel's early return.
    if candidate_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = matter.connection().prepare(
        "SELECT id, output_root, bates_prefix, profile_slug FROM production_sets WHERE matter_id = ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![matter.id()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut findings = Vec::new();
    for r in rows {
        let (set_id, output_root, prefix, profile_slug) = r?;
        if !production_set_has_images(matter, &set_id, profile_slug.as_deref())? {
            continue;
        }
        findings.extend(evaluate_one_image_volume(
            matter,
            rules,
            &set_id,
            output_root.as_deref(),
            &prefix,
            candidate_ids,
            need_span,
            need_missing,
            need_opt,
            need_multi,
        )?);
    }
    Ok(findings)
}

/// Image volume rules apply only to image-profile sets (or leftover image pages).
fn production_set_has_images(
    matter: &Matter,
    set_id: &str,
    profile_slug: Option<&str>,
) -> Result<bool> {
    if let Some(slug) = profile_slug.map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(profile) = matter.get_production_profile(slug) {
            return Ok(profile.body.packaging.include_images);
        }
    }
    let n: i64 = matter.connection().query_row(
        "SELECT COUNT(*) FROM production_image_pages WHERE production_set_id = ?1",
        rusqlite::params![set_id],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_one_image_volume(
    matter: &Matter,
    rules: &ResolvedRules,
    set_id: &str,
    output_root: Option<&str>,
    prefix: &str,
    candidate_ids: &[String],
    need_span: bool,
    need_missing: bool,
    need_opt: bool,
    need_multi: bool,
) -> Result<Vec<QcFinding>> {
    let candidates: HashSet<&str> = candidate_ids.iter().map(String::as_str).collect();
    let mut findings = Vec::new();
    let mut stmt = matter.connection().prepare(
        "SELECT item_id, control_number, end_bates, COALESCE(page_count, 0) \
         FROM production_items WHERE production_set_id = ?1 AND status = 'ok'",
    )?;
    let item_rows = stmt.query_map(rusqlite::params![set_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut sum_pages: u64 = 0;
    let mut any_image_pages = false;
    let mut out_of_scope_image_item = false;
    for r in item_rows {
        let (item_id, control, end, pc) = r?;
        if !candidates.is_empty() && !candidates.contains(item_id.as_str()) {
            if pc >= 1 {
                out_of_scope_image_item = true;
            }
            continue;
        }
        if pc < 1 {
            if need_missing {
                if let Ok(item) = matter.get_item(&item_id) {
                    let bytes = item
                        .native_sha256
                        .as_deref()
                        .and_then(|h| matter.get_bytes(h).ok())
                        .unwrap_or_default();
                    if is_qc_image_eligible(&item, &bytes) {
                        findings.push(QcFinding {
                            rule_id: RULE_IMAGE_PAGE_MISSING.into(),
                            severity: rules.severity(RULE_IMAGE_PAGE_MISSING),
                            item_id: Some(item_id.clone()),
                            message: "image-eligible page missing TIF or OPT row".into(),
                        });
                    }
                }
            }
            continue;
        }
        any_image_pages = true;
        sum_pages += pc as u64;
        if need_span {
            let end_s = match end.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => s,
                None => control.as_str(),
            };
            if let (Some(beg), Some(end_seq)) = (
                parse_bates_seq(prefix, &control),
                parse_bates_seq(prefix, end_s),
            ) {
                if end_seq.saturating_sub(beg).saturating_add(1) != pc as u64 {
                    findings.push(QcFinding {
                        rule_id: RULE_BEG_END_BATES_SPAN.into(),
                        severity: rules.severity(RULE_BEG_END_BATES_SPAN),
                        item_id: Some(item_id.clone()),
                        message: "beg/end Bates span does not match page_count".into(),
                    });
                }
            }
        }
        if need_missing {
            let n_written: i64 = matter.connection().query_row(
                "SELECT COUNT(*) FROM production_image_pages \
                 WHERE production_set_id = ?1 AND item_id = ?2",
                rusqlite::params![set_id, &item_id],
                |row| row.get(0),
            )?;
            if n_written != pc {
                findings.push(QcFinding {
                    rule_id: RULE_IMAGE_PAGE_MISSING.into(),
                    severity: rules.severity(RULE_IMAGE_PAGE_MISSING),
                    item_id: Some(item_id.clone()),
                    message: "image-eligible page missing TIF or OPT row".into(),
                });
            }
            if let Some(root) = output_root.map(str::trim).filter(|s| !s.is_empty()) {
                let mut pip = matter.connection().prepare(
                    "SELECT relpath, sha256 FROM production_image_pages \
                     WHERE production_set_id = ?1 AND item_id = ?2",
                )?;
                let pages = pip.query_map(rusqlite::params![set_id, &item_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                for p in pages {
                    let (rel, expected_sha) = p?;
                    match read_volume_rel(root, &rel) {
                        None => {
                            findings.push(QcFinding {
                                rule_id: RULE_IMAGE_PAGE_MISSING.into(),
                                severity: rules.severity(RULE_IMAGE_PAGE_MISSING),
                                item_id: Some(item_id.clone()),
                                message: "image-eligible page missing TIF or OPT row".into(),
                            });
                        }
                        Some(bytes) => {
                            let digest = Sha256::digest(&bytes);
                            let disk: String = digest.iter().map(|b| format!("{b:02x}")).collect();
                            if !disk.eq_ignore_ascii_case(expected_sha.trim()) {
                                findings.push(QcFinding {
                                    rule_id: RULE_IMAGE_PAGE_MISSING.into(),
                                    severity: rules.severity(RULE_IMAGE_PAGE_MISSING),
                                    item_id: Some(item_id.clone()),
                                    message: "image-eligible page missing TIF or OPT row".into(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let opt_path = output_root.and_then(|root| {
        let p = std::path::Path::new(root).join("IMAGE.opt");
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    });

    if need_opt && !out_of_scope_image_item {
        let n_lines = if let Some(path) = opt_path.as_ref() {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            text.lines().filter(|l| !l.trim().is_empty()).count() as u64
        } else if any_image_pages {
            // Completed image volume with persisted pages but no OPT: treat as 0 lines.
            0
        } else {
            // Preflight / empty volume: skip (OPT not written yet).
            return Ok(findings);
        };
        if n_lines != sum_pages {
            findings.push(QcFinding {
                rule_id: RULE_OPT_ROW_COUNT_MISMATCH.into(),
                severity: rules.severity(RULE_OPT_ROW_COUNT_MISMATCH),
                item_id: None,
                message: "OPT line count does not match image page_count".into(),
            });
        }
    }

    if need_multi && any_image_pages {
        if let Some(root) = output_root.map(str::trim).filter(|s| !s.is_empty()) {
            let mut pip = matter.connection().prepare(
                "SELECT item_id, relpath FROM production_image_pages WHERE production_set_id = ?1",
            )?;
            let pages = pip.query_map(rusqlite::params![set_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for p in pages {
                let (item_id, rel) = p?;
                if !candidates.is_empty() && !candidates.contains(item_id.as_str()) {
                    continue;
                }
                if let Some(bytes) = read_volume_rel(root, &rel) {
                    if tiff_ifd_count_le(&bytes) > 1 {
                        findings.push(QcFinding {
                            rule_id: RULE_MULTI_PAGE_TIFF_AS_ARTIFACT.into(),
                            severity: rules.severity(RULE_MULTI_PAGE_TIFF_AS_ARTIFACT),
                            item_id: Some(item_id),
                            message: "produced image has more than one IFD".into(),
                        });
                    }
                }
            }
        }
    }
    Ok(findings)
}

fn parse_bates_seq(prefix: &str, control: &str) -> Option<u64> {
    let rest = control.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

fn read_volume_rel(root: &str, rel: &str) -> Option<Vec<u8>> {
    let normalized = rel.replace('/', "\\");
    let mut abs = std::path::PathBuf::from(root);
    for p in normalized.split('\\').filter(|s| !s.is_empty()) {
        if p == ".." || p.contains(':') {
            return None;
        }
        abs.push(p);
    }
    std::fs::read(abs).ok()
}

fn tiff_ifd_count_le(bytes: &[u8]) -> u32 {
    if bytes.len() < 8 {
        return 0;
    }
    let le = bytes[0] == b'I' && bytes[1] == b'I' && bytes[2] == 0x2A && bytes[3] == 0;
    let be = bytes[0] == b'M' && bytes[1] == b'M' && bytes[2] == 0 && bytes[3] == 0x2A;
    if !le && !be {
        return 0;
    }
    let u16_at = |off: usize| -> Option<u16> {
        let b = bytes.get(off..off + 2)?;
        Some(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |off: usize| -> Option<u32> {
        let b = bytes.get(off..off + 4)?;
        Some(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    let mut next = u32_at(4).unwrap_or(0);
    let mut n = 0u32;
    let mut guard = 0u32;
    while next != 0 && guard < 1024 {
        let off = next as usize;
        let count = match u16_at(off) {
            Some(c) => c as usize,
            None => break,
        };
        let after = off
            .saturating_add(2)
            .saturating_add(count.saturating_mul(12));
        next = u32_at(after).unwrap_or(0);
        n = n.saturating_add(1);
        guard += 1;
    }
    n
}

/// Withhold lookup that treats missing items as not withheld.
///
/// Orphan / broken-family rules already cover parents missing from the
/// selection; relative withhold checks must not hard-error on a dangling
/// `parent_item_id` (or a child id that vanished mid-scan).
fn item_is_withheld_loose(matter: &Matter, item_id: &str) -> Result<bool> {
    match matter.item_is_withheld(item_id) {
        Ok(v) => Ok(v),
        Err(matter_core::Error::ItemNotFound(_)) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn has_missing_non_withheld_child(
    matter: &Matter,
    parent_id: &str,
    candidate_set: &HashSet<&str>,
) -> Result<bool> {
    let mut stmt = matter
        .connection()
        .prepare("SELECT id FROM items WHERE matter_id = ?1 AND parent_item_id = ?2")?;
    let rows = stmt.query_map(rusqlite::params![matter.id(), parent_id], |row| {
        row.get::<_, String>(0)
    })?;
    for r in rows {
        let child_id = r?;
        if candidate_set.contains(child_id.as_str()) {
            continue;
        }
        if item_is_withheld_loose(matter, &child_id)? {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

fn family_has_withheld_relative(matter: &Matter, item: &Item) -> Result<bool> {
    if let Some(parent) = item.parent_item_id.as_deref() {
        if item_is_withheld_loose(matter, parent)? {
            return Ok(true);
        }
    }
    let mut stmt = matter
        .connection()
        .prepare("SELECT id FROM items WHERE matter_id = ?1 AND parent_item_id = ?2")?;
    let rows = stmt.query_map(rusqlite::params![matter.id(), item.id], |row| {
        row.get::<_, String>(0)
    })?;
    for r in rows {
        let child_id = r?;
        if item_is_withheld_loose(matter, &child_id)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pack_has_orphan_error() {
        let r = resolve_rules(&[]);
        assert_eq!(
            r.severity(RULE_BROKEN_FAMILY_ORPHAN_CHILD),
            QcSeverity::Error
        );
        assert_eq!(
            r.severity(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT),
            QcSeverity::Warn
        );
        assert_eq!(r.severity(RULE_IMAGE_PAGE_MISSING), QcSeverity::Off);
        assert_eq!(r.severity(RULE_IMAGE_SKIPPED_NATIVE_ONLY), QcSeverity::Off);
    }

    #[test]
    fn override_off_disables() {
        let r = resolve_rules(&[QcRuleConfig {
            id: RULE_ZERO_SIZE.into(),
            severity: QcSeverity::Off,
        }]);
        assert!(!r.is_enabled(RULE_ZERO_SIZE));
    }

    #[test]
    fn missing_text_error_categories() {
        assert!(missing_text_is_error_category(Some("email")));
        assert!(missing_text_is_error_category(Some("PDF")));
        assert!(missing_text_is_error_category(Some("document")));
        assert!(!missing_text_is_error_category(Some("image")));
        assert!(!missing_text_is_error_category(None));
    }
}
