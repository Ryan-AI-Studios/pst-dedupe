//! # matter-qc
//!
//! Pre-production **QC engine** (track **0041**):
//!
//! 1. Select candidates (same scopes as produce: `review_corpus` / `item_ids`)
//! 2. Evaluate built-in rules (broken family, withhold, redacted text, natives, …)
//! 3. Write findings CSV pack under `exports/qc/`
//! 4. Persist `qc_runs` with selection fingerprint for the produce gate
//!
//! ## Contracts
//!
//! - Findings never include subject/body/paths — item_id + short rule messages only
//! - `passed` = zero Error-severity findings (warnings allowed)
//! - Produce gate uses [`matter_core::qc_run_is_fresh`] (count + fingerprint + scope)
//! - Packaging remains **matter-produce** (0040); QC does not write volumes
//!
//! ## Job
//!
//! Kind [`JOB_KIND_QC`] (`"qc"`). Option C: no `create_job` inside the engine.

#![forbid(unsafe_code)]

pub mod error;
pub mod gate;
pub mod params;
pub mod report;
pub mod rules;
pub mod run;
pub mod select;

pub use error::{QcError, Result};
pub use gate::{check_qc_gate, QcGateBlock};
pub use params::{
    QcParams, QcRuleConfig, QcSeverity, PROFILE_DEFAULT_PRODUCTION_QC_V1, SCOPE_ITEM_IDS,
    SCOPE_REVIEW_CORPUS,
};
pub use report::{count_severities, default_qc_report_dir, write_qc_report, QcReportMeta};
pub use rules::{
    default_rule_pack, evaluate_candidates, is_email_like, resolve_rules, QcFinding, ResolvedRules,
    RULE_BROKEN_FAMILY_INCOMPLETE_PARENT, RULE_BROKEN_FAMILY_ORPHAN_CHILD, RULE_EMPTY_SELECTION,
    RULE_ITEM_STATUS_ERROR, RULE_MISSING_NATIVE, RULE_MISSING_TEXT, RULE_ONLY_WITHHELD,
    RULE_PDF_NEEDS_OCR, RULE_REDACTED_TEXT_MISSING, RULE_WITHHELD_FAMILY_MEMBER,
    RULE_WITHHELD_IN_SELECTION, RULE_ZERO_SIZE,
};
pub use run::{run_production_qc, QcOutcome, QcReport, QcSummary, JOB_KIND_QC, QC_STAGE};
pub use select::select_item_ids;
