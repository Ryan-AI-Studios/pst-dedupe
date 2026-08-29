//! Export outcome classification (track **0078**).
//!
//! Pure functions: exit codes and fidelity are derived from data-path inputs only
//! (never from logs/stderr). Precedence selects the **integer**; reasons are cumulative.

use dedup_engine::integrity::PreflightRecommendation;
use serde::{Deserialize, Serialize};

use crate::error::CliExit;

/// Inputs to the pure export-success / classify gate (honesty).
#[derive(Debug, Clone, Copy)]
pub struct ExportOkInput {
    pub scan_ok: bool,
    pub verify_ok: bool,
    pub export_err_absent: bool,
    pub export_partial: bool,
    pub messages_written_total: u64,
    pub unique: u64,
    pub attach_failed_total: u64,
    /// Body soft-fail count when tracked (unique-eml / future); 0 when unknown.
    pub body_soft_fail_total: u64,
    pub report_ok: bool,
}

/// Terminal fidelity of an export operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExportFidelity {
    /// All hard-fail dimensions clean and no attach/body soft-fail.
    #[default]
    Complete,
    /// Message-complete artifact; attachment/body soft-failures recorded.
    Partial,
    /// Artifact absent or untrustworthy (hard fail dimensions).
    Failed,
}

impl ExportFidelity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

/// Closed vocabulary for on-disk / deliverable disposition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    /// Artifact at `--out` is the full deliverable.
    Complete,
    /// Message-complete soft-fail deliverable (exit 64 path) — ship only with disclosure.
    /// Never used for hard-fail / incomplete writes (those use [`InvalidInPlace`] or quarantine).
    PartialRetained,
    /// Incomplete; renamed to `.partial`; `--out` free for retry.
    PartialQuarantined,
    /// Bytes still at `--out` but untrustworthy / must not ship: cancel quarantine failed,
    /// or hard-fail after write (incomplete, verify fail, count mismatch, etc.).
    /// Orchestrator must purge or quarantine manually before retry; not a deliverable.
    InvalidInPlace,
    /// Nothing written.
    #[default]
    Absent,
}

impl ArtifactState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::PartialRetained => "partial_retained",
            Self::PartialQuarantined => "partial_quarantined",
            Self::InvalidInPlace => "invalid_in_place",
            Self::Absent => "absent",
        }
    }
}

/// Opt-in gate on 0077 `export_risk` rank (default **off**).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RiskGate {
    /// Gate disabled — never exit 65 from risk alone.
    #[default]
    Off,
    /// Fire when risk rank ≥ `ok` (always, for any computed risk including Ok).
    /// Practically unused; kept for enum completeness / explicit `ok` arg.
    Ok,
    /// Fire when risk rank ≥ `re_export_recommended`.
    ReExportRecommended,
    /// Fire when risk rank ≥ `not_export_ready`.
    NotExportReady,
}

impl RiskGate {
    /// Parse CLI value for `--fail-on-export-risk`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ok" => Some(Self::Ok),
            "re_export_recommended" | "re-export-recommended" => Some(Self::ReExportRecommended),
            "not_export_ready" | "not-export-ready" => Some(Self::NotExportReady),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Ok => "ok",
            Self::ReExportRecommended => "re_export_recommended",
            Self::NotExportReady => "not_export_ready",
        }
    }

    fn threshold_rank(self) -> Option<u8> {
        match self {
            Self::Off => None,
            Self::Ok => Some(PreflightRecommendation::Ok.rank()),
            Self::ReExportRecommended => Some(PreflightRecommendation::ReExportRecommended.rank()),
            Self::NotExportReady => Some(PreflightRecommendation::NotExportReady.rank()),
        }
    }
}

/// What the process should tell its caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportOutcome {
    pub fidelity: ExportFidelity,
    pub exit: CliExit,
    /// Stable machine codes, worst-first; never PST-derived strings.
    pub reasons: Vec<&'static str>,
    pub cancelled: bool,
}

/// Closed `exit_reason` vocabulary (0078).
pub mod reason {
    pub const ATTACH_SOFT_FAIL: &str = "ATTACH_SOFT_FAIL";
    pub const BODY_SOFT_FAIL: &str = "BODY_SOFT_FAIL";
    pub const COUNT_MISMATCH: &str = "COUNT_MISMATCH";
    pub const VERIFY_FAILED: &str = "VERIFY_FAILED";
    pub const REPORT_WRITE_FAILED: &str = "REPORT_WRITE_FAILED";
    pub const SCAN_FAILED: &str = "SCAN_FAILED";
    pub const RISK_GATE: &str = "RISK_GATE";
    pub const CANCELLED: &str = "CANCELLED";
}

/// Classify export outcome from data-path inputs.
///
/// **Integer precedence** (worst-first): cancelled → 130; hard fail → 1; risk gate → 65;
/// partial → 64 (if `fail_on_partial`) else 0; else 0 Complete.
///
/// **Reasons are cumulative** — every observed condition is recorded (except cancellation,
/// which suppresses finding-style reasons and reports only `CANCELLED`).
pub fn classify_export(
    i: ExportOkInput,
    risk: PreflightRecommendation,
    risk_gate: RiskGate,
    fail_on_partial: bool,
    cancelled: bool,
) -> ExportOutcome {
    // Cancellation: suppress findings; only CANCELLED.
    if cancelled {
        return ExportOutcome {
            fidelity: ExportFidelity::Failed,
            exit: CliExit::Cancelled,
            reasons: vec![reason::CANCELLED],
            cancelled: true,
        };
    }

    let hard_scan = !i.scan_ok;
    let hard_verify = !i.verify_ok;
    let hard_export_err = !i.export_err_absent;
    let hard_partial = i.export_partial;
    let hard_count = i.messages_written_total != i.unique;
    let hard_report = !i.report_ok;
    let hard_fail =
        hard_scan || hard_verify || hard_export_err || hard_partial || hard_count || hard_report;

    let attach_soft = i.attach_failed_total > 0;
    let body_soft = i.body_soft_fail_total > 0;

    let risk_blocked = risk_gate
        .threshold_rank()
        .is_some_and(|thr| risk.rank() >= thr);

    // Collect reasons in precedence order (hard → risk → soft). Do not short-circuit.
    let mut reasons: Vec<&'static str> = Vec::new();
    if hard_scan {
        reasons.push(reason::SCAN_FAILED);
    }
    if hard_verify {
        reasons.push(reason::VERIFY_FAILED);
    }
    if hard_count || hard_partial || hard_export_err {
        // Incomplete / untrustworthy write bucket (closed vocab has no EXPORT_FAILED).
        if !reasons.contains(&reason::COUNT_MISMATCH) {
            reasons.push(reason::COUNT_MISMATCH);
        }
    }
    if hard_report {
        reasons.push(reason::REPORT_WRITE_FAILED);
    }
    if risk_blocked {
        reasons.push(reason::RISK_GATE);
    }
    if attach_soft {
        reasons.push(reason::ATTACH_SOFT_FAIL);
    }
    if body_soft {
        reasons.push(reason::BODY_SOFT_FAIL);
    }

    let fidelity = if hard_fail {
        ExportFidelity::Failed
    } else if attach_soft || body_soft {
        ExportFidelity::Partial
    } else {
        ExportFidelity::Complete
    };

    let exit = if hard_fail {
        CliExit::Generic
    } else if risk_blocked {
        CliExit::ExportRiskBlocked
    } else if fidelity == ExportFidelity::Partial {
        if fail_on_partial {
            CliExit::PartialFidelity
        } else {
            CliExit::Success
        }
    } else {
        CliExit::Success
    };

    ExportOutcome {
        fidelity,
        exit,
        reasons,
        cancelled: false,
    }
}

/// 0078 integer precedence rank (higher = worse). Not raw `u8` of [`CliExit`].
fn cli_exit_precedence_rank(exit: CliExit) -> u8 {
    match exit {
        CliExit::Cancelled => 5,
        CliExit::Generic => 4,
        CliExit::ExportRiskBlocked => 3,
        CliExit::PartialFidelity => 2,
        CliExit::Success => 0,
        // Treat other application codes as hard-fail class for merge.
        CliExit::Usage | CliExit::Busy | CliExit::JobFailed | CliExit::MatterIo => 4,
    }
}

/// Worse of two exits by 0078 precedence: `130 > 1 > 65 > 64 > 0`.
pub fn worse_cli_exit(a: CliExit, b: CliExit) -> CliExit {
    if cli_exit_precedence_rank(a) >= cli_exit_precedence_rank(b) {
        a
    } else {
        b
    }
}

/// Merge exit-reason codes worst-first without duplicates (stable within each side).
pub fn merge_exit_reasons(primary: &[String], secondary: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in primary.iter().chain(secondary.iter()) {
        if !out.iter().any(|x| x == s) {
            out.push(s.clone());
        }
    }
    out
}

/// Whether automation may **retry** this unique-export outcome (0082 D-0078-retryable).
///
/// `true` only for clearly transient classes:
/// - operator cancel (`CliExit::Cancelled` / `cancelled`)
/// - matter/transient IO (`CliExit::MatterIo`)
/// - summary error codes that denote disk / write IO (not integrity)
///
/// `false` for permanent failures: usage, risk gate, partial fidelity exit,
/// verify/count/report hard fails, schema/passphrase/audit-style codes, and success.
///
/// **Writer IO:** production maps [`pst_writer::WriterError::Io`] (and local disk
/// prep failures) to error code `write_io`. Classify still records
/// [`reason::COUNT_MISMATCH`] for incomplete writes, so transient codes are
/// checked **before** permanent reason codes — otherwise `write_io` would
/// incorrectly stay non-retryable.
///
/// **No new exit integers** — this is a summary JSON field only.
pub fn summary_is_retryable(
    exit: CliExit,
    cancelled: bool,
    exit_reasons: &[&str],
    error_code: Option<&str>,
) -> bool {
    // Cancel-retry class.
    if cancelled || exit == CliExit::Cancelled {
        return true;
    }
    // Permanent: risk gate, partial fidelity, usage, busy, job failed.
    if matches!(
        exit,
        CliExit::ExportRiskBlocked
            | CliExit::PartialFidelity
            | CliExit::Usage
            | CliExit::Busy
            | CliExit::JobFailed
            | CliExit::Success
    ) {
        return false;
    }
    // Explicit permanent error codes (schema / passphrase / audit / fidelity).
    if let Some(code) = error_code {
        let c = code.to_ascii_lowercase();
        if matches!(
            c.as_str(),
            "auditchainbroken"
                | "audit_chain_broken"
                | "schema"
                | "passphrase"
                | "export_risk"
                | "fidelity"
                | "partial_fidelity"
                | "usage"
                | "risk_gate"
                | "export"
        ) {
            return false;
        }
        // Transient IO / cancel classes — before permanent *reason* codes so
        // write_io remains retryable when COUNT_MISMATCH is also recorded for
        // an incomplete volume write.
        if matches!(
            c.as_str(),
            "io" | "disk" | "write_io" | "cancelled" | "cancel" | "matter_io" | "matter-io"
        ) {
            return true;
        }
    }
    // Permanent reason codes even under Generic (when no transient error code).
    if exit_reasons.iter().any(|r| {
        matches!(
            *r,
            reason::RISK_GATE
                | reason::VERIFY_FAILED
                | reason::COUNT_MISMATCH
                | reason::SCAN_FAILED
                | reason::REPORT_WRITE_FAILED
        )
    }) {
        return false;
    }
    // MatterIo is the dedicated transient IO band (exit 5).
    if exit == CliExit::MatterIo {
        return true;
    }
    // Default: permanent (do not loop automation on unknown Generic hard fails).
    false
}

/// Derive [`ArtifactState`] from classified outcome + on-disk disposition.
///
/// **Hard-fail + bytes at `--out` → [`ArtifactState::InvalidInPlace`]** (not
/// [`ArtifactState::PartialRetained`]): closed vocab has no “failed-retained”;
/// `partial_retained` is reserved for message-complete soft-fail deliverables.
/// Incomplete / untrustworthy hard-fail artifacts must not be treated as shippable —
/// purge or quarantine manually.
pub fn artifact_state_for(
    outcome: &ExportOutcome,
    bytes_written: bool,
    quarantine: QuarantineResult,
) -> ArtifactState {
    if outcome.cancelled {
        if !bytes_written {
            return ArtifactState::Absent;
        }
        return match quarantine {
            QuarantineResult::NotAttempted | QuarantineResult::Failed => {
                ArtifactState::InvalidInPlace
            }
            QuarantineResult::Succeeded => ArtifactState::PartialQuarantined,
            QuarantineResult::NoVolumes => ArtifactState::Absent,
        };
    }
    match outcome.fidelity {
        ExportFidelity::Complete => {
            if bytes_written {
                ArtifactState::Complete
            } else {
                // Zero-message complete export is still a valid empty deliverable path.
                ArtifactState::Complete
            }
        }
        ExportFidelity::Partial => ArtifactState::PartialRetained,
        ExportFidelity::Failed => {
            if bytes_written {
                // Hard-fail with bytes still at `--out`: incomplete / untrustworthy.
                // Spec closed vocab has no failed-retained; orchestrator must not ship.
                ArtifactState::InvalidInPlace
            } else {
                ArtifactState::Absent
            }
        }
    }
}

/// Result of cancel-time volume quarantine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum QuarantineResult {
    #[default]
    NotAttempted,
    /// No volume files existed to rename.
    NoVolumes,
    Succeeded,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_base() -> ExportOkInput {
        ExportOkInput {
            scan_ok: true,
            verify_ok: true,
            export_err_absent: true,
            export_partial: false,
            messages_written_total: 5,
            unique: 5,
            attach_failed_total: 0,
            body_soft_fail_total: 0,
            report_ok: true,
        }
    }

    #[test]
    fn complete_success() {
        let o = classify_export(
            ok_base(),
            PreflightRecommendation::Ok,
            RiskGate::Off,
            true,
            false,
        );
        assert_eq!(o.fidelity, ExportFidelity::Complete);
        assert_eq!(o.exit, CliExit::Success);
        assert!(o.reasons.is_empty());
    }

    #[test]
    fn worse_cli_exit_generic_beats_partial_and_cancelled_beats_generic() {
        // Raw u8 max would wrongly prefer PartialFidelity(64) over Generic(1).
        assert_eq!(
            worse_cli_exit(CliExit::PartialFidelity, CliExit::Generic),
            CliExit::Generic
        );
        assert_eq!(
            worse_cli_exit(CliExit::Generic, CliExit::PartialFidelity),
            CliExit::Generic
        );
        assert_eq!(
            worse_cli_exit(CliExit::Cancelled, CliExit::Generic),
            CliExit::Cancelled
        );
        assert_eq!(
            worse_cli_exit(CliExit::Generic, CliExit::Cancelled),
            CliExit::Cancelled
        );
        assert_eq!(
            worse_cli_exit(CliExit::ExportRiskBlocked, CliExit::PartialFidelity),
            CliExit::ExportRiskBlocked
        );
    }

    #[test]
    fn attach_soft_fail_partial_64() {
        let mut i = ok_base();
        i.attach_failed_total = 3;
        let o = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false);
        assert_eq!(o.fidelity, ExportFidelity::Partial);
        assert_eq!(o.exit, CliExit::PartialFidelity);
        assert_eq!(o.reasons, vec![reason::ATTACH_SOFT_FAIL]);
    }

    /// DoD-12 unique-eml fidelity proof: attach soft-fail with all hard dimensions clean
    /// (same input shape unique-eml feeds into [`classify_export`]) → Partial + 64 + ATTACH_SOFT_FAIL.
    #[test]
    fn unique_eml_style_attach_soft_fail_is_partial_64() {
        // Mirrors unique-eml post-write gate: scan/verify/report ok, count match,
        // no export_partial / export_err; only attach_failed_total > 0 (body soft-fail 0).
        let i = ExportOkInput {
            scan_ok: true,
            verify_ok: true,
            export_err_absent: true,
            export_partial: false,
            messages_written_total: 12,
            unique: 12,
            attach_failed_total: 2,
            body_soft_fail_total: 0,
            report_ok: true,
        };
        let o = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false);
        assert_eq!(
            o.fidelity,
            ExportFidelity::Partial,
            "unique-eml attach soft-fail must be Partial fidelity"
        );
        assert_eq!(
            o.exit,
            CliExit::PartialFidelity,
            "default fail-on-partial → exit 64"
        );
        assert_eq!(o.exit as u8, 64);
        assert_eq!(o.reasons, vec![reason::ATTACH_SOFT_FAIL]);
        assert!(!o.cancelled);
        // Soft-fail deliverable disposition for orchestrators.
        assert_eq!(
            artifact_state_for(&o, true, QuarantineResult::NotAttempted),
            ArtifactState::PartialRetained
        );
    }

    #[test]
    fn allow_partial_fidelity_exit_0() {
        let mut i = ok_base();
        i.attach_failed_total = 1;
        let o = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, false, false);
        assert_eq!(o.fidelity, ExportFidelity::Partial);
        assert_eq!(o.exit, CliExit::Success);
        assert_eq!(o.reasons, vec![reason::ATTACH_SOFT_FAIL]);
    }

    #[test]
    fn hard_fail_count_mismatch() {
        let mut i = ok_base();
        i.messages_written_total = 4;
        let o = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false);
        assert_eq!(o.fidelity, ExportFidelity::Failed);
        assert_eq!(o.exit, CliExit::Generic);
        assert!(o.reasons.contains(&reason::COUNT_MISMATCH));
    }

    #[test]
    fn risk_gate_not_export_ready_65() {
        let o = classify_export(
            ok_base(),
            PreflightRecommendation::NotExportReady,
            RiskGate::NotExportReady,
            true,
            false,
        );
        assert_eq!(o.fidelity, ExportFidelity::Complete);
        assert_eq!(o.exit, CliExit::ExportRiskBlocked);
        assert_eq!(o.reasons, vec![reason::RISK_GATE]);
    }

    #[test]
    fn risk_gate_off_no_65() {
        let o = classify_export(
            ok_base(),
            PreflightRecommendation::NotExportReady,
            RiskGate::Off,
            true,
            false,
        );
        assert_eq!(o.exit, CliExit::Success);
        assert!(!o.reasons.contains(&reason::RISK_GATE));
    }

    #[test]
    fn cancelled_outranks_all_only_cancelled_reason() {
        let mut i = ok_base();
        i.attach_failed_total = 10;
        let o = classify_export(
            i,
            PreflightRecommendation::NotExportReady,
            RiskGate::NotExportReady,
            true,
            true,
        );
        assert_eq!(o.exit, CliExit::Cancelled);
        assert_eq!(o.reasons, vec![reason::CANCELLED]);
        assert!(o.cancelled);
    }

    #[test]
    fn cumulative_risk_and_attach() {
        let mut i = ok_base();
        i.attach_failed_total = 2;
        let o = classify_export(
            i,
            PreflightRecommendation::ReExportRecommended,
            RiskGate::ReExportRecommended,
            true,
            false,
        );
        assert_eq!(o.exit, CliExit::ExportRiskBlocked);
        assert_eq!(o.reasons, vec![reason::RISK_GATE, reason::ATTACH_SOFT_FAIL]);
        assert_eq!(o.fidelity, ExportFidelity::Partial);
    }

    #[test]
    fn hard_fail_outranks_risk_and_partial() {
        let mut i = ok_base();
        i.attach_failed_total = 1;
        i.verify_ok = false;
        let o = classify_export(
            i,
            PreflightRecommendation::NotExportReady,
            RiskGate::NotExportReady,
            true,
            false,
        );
        assert_eq!(o.exit, CliExit::Generic);
        assert!(o.reasons.contains(&reason::VERIFY_FAILED));
        assert!(o.reasons.contains(&reason::RISK_GATE));
        assert!(o.reasons.contains(&reason::ATTACH_SOFT_FAIL));
        // Precedence order: hard before risk before soft.
        let v = o.reasons.iter().position(|r| *r == reason::VERIFY_FAILED);
        let r = o.reasons.iter().position(|r| *r == reason::RISK_GATE);
        let a = o
            .reasons
            .iter()
            .position(|r| *r == reason::ATTACH_SOFT_FAIL);
        assert!(v < r && r < a);
    }

    /// DoD-14 refinement: every class non-zero today remains non-zero after.
    #[test]
    fn refinement_assertion_non_zero_stays_non_zero() {
        // Today (baseline): attach fail → 1, hard fail → 1, cancel → 1.
        // After: attach → 64, hard → 1, cancel → 130 — all non-zero.
        let classes: Vec<ExportOutcome> = vec![
            {
                let mut i = ok_base();
                i.attach_failed_total = 1;
                classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false)
            },
            {
                let mut i = ok_base();
                i.export_partial = true;
                classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false)
            },
            {
                let mut i = ok_base();
                i.verify_ok = false;
                classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false)
            },
            {
                let mut i = ok_base();
                i.scan_ok = false;
                classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false)
            },
            {
                let mut i = ok_base();
                i.report_ok = false;
                classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false)
            },
            {
                let mut i = ok_base();
                i.messages_written_total = 0;
                classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false)
            },
            classify_export(
                ok_base(),
                PreflightRecommendation::Ok,
                RiskGate::Off,
                true,
                true,
            ),
        ];
        for o in classes {
            assert_ne!(
                o.exit as u8, 0,
                "refinement violation: {:?} would exit 0",
                o
            );
        }
        // Complete stays 0.
        let ok = classify_export(
            ok_base(),
            PreflightRecommendation::Ok,
            RiskGate::Off,
            true,
            false,
        );
        assert_eq!(ok.exit as u8, 0);
    }

    #[test]
    fn ok_consistent_with_fidelity() {
        let mut i = ok_base();
        i.attach_failed_total = 1;
        let o = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false);
        let ok = o.fidelity == ExportFidelity::Complete;
        assert!(!ok);
        assert_eq!(o.fidelity, ExportFidelity::Partial);
    }

    #[test]
    fn compute_export_ok_via_classify_matches_legacy() {
        // Mirror unique_pst_cmd::compute_export_ok re-expression.
        let check = |i: ExportOkInput| {
            classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false).fidelity
                == ExportFidelity::Complete
        };
        assert!(check(ok_base()));
        let mut bad = ok_base();
        bad.attach_failed_total = 1;
        assert!(!check(bad));
        let mut bad = ok_base();
        bad.report_ok = false;
        assert!(!check(bad));
        let mut bad = ok_base();
        bad.export_partial = true;
        assert!(!check(bad));
        let mut bad = ok_base();
        bad.messages_written_total = 4;
        assert!(!check(bad));
    }

    #[test]
    fn artifact_state_complete_and_partial() {
        let complete = classify_export(
            ok_base(),
            PreflightRecommendation::Ok,
            RiskGate::Off,
            true,
            false,
        );
        assert_eq!(
            artifact_state_for(&complete, true, QuarantineResult::NotAttempted),
            ArtifactState::Complete
        );
        assert_eq!(
            artifact_state_for(&complete, false, QuarantineResult::NotAttempted),
            ArtifactState::Complete
        );

        let mut i = ok_base();
        i.attach_failed_total = 1;
        let partial = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false);
        assert_eq!(
            artifact_state_for(&partial, true, QuarantineResult::NotAttempted),
            ArtifactState::PartialRetained
        );
    }

    /// Hard-fail with bytes still at `--out` is InvalidInPlace (must not ship), not PartialRetained.
    #[test]
    fn artifact_state_hard_fail_with_bytes_is_invalid_in_place() {
        let mut i = ok_base();
        i.verify_ok = false;
        let failed = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false);
        assert_eq!(failed.fidelity, ExportFidelity::Failed);
        assert_eq!(
            artifact_state_for(&failed, true, QuarantineResult::NotAttempted),
            ArtifactState::InvalidInPlace
        );
        assert_eq!(
            artifact_state_for(&failed, false, QuarantineResult::NotAttempted),
            ArtifactState::Absent
        );

        // Count mismatch after write — same disposition.
        let mut i = ok_base();
        i.messages_written_total = 3;
        let failed = classify_export(i, PreflightRecommendation::Ok, RiskGate::Off, true, false);
        assert_eq!(
            artifact_state_for(&failed, true, QuarantineResult::NotAttempted),
            ArtifactState::InvalidInPlace
        );
    }

    #[test]
    fn artifact_state_cancelled_path_unchanged() {
        let cancelled = classify_export(
            ok_base(),
            PreflightRecommendation::Ok,
            RiskGate::Off,
            true,
            true,
        );
        assert_eq!(
            artifact_state_for(&cancelled, false, QuarantineResult::NotAttempted),
            ArtifactState::Absent
        );
        assert_eq!(
            artifact_state_for(&cancelled, true, QuarantineResult::Succeeded),
            ArtifactState::PartialQuarantined
        );
        assert_eq!(
            artifact_state_for(&cancelled, true, QuarantineResult::Failed),
            ArtifactState::InvalidInPlace
        );
        assert_eq!(
            artifact_state_for(&cancelled, true, QuarantineResult::NotAttempted),
            ArtifactState::InvalidInPlace
        );
        assert_eq!(
            artifact_state_for(&cancelled, true, QuarantineResult::NoVolumes),
            ArtifactState::Absent
        );
    }

    /// 0082 DoD-8: retryable true/false boundaries (no new exit integers).
    #[test]
    fn retryable_cancel_true() {
        assert!(summary_is_retryable(
            CliExit::Cancelled,
            true,
            &[reason::CANCELLED],
            Some("cancelled")
        ));
        assert!(summary_is_retryable(CliExit::Cancelled, false, &[], None));
    }

    #[test]
    fn retryable_matter_io_true() {
        assert!(summary_is_retryable(CliExit::MatterIo, false, &[], None));
        assert!(summary_is_retryable(
            CliExit::Generic,
            false,
            &[],
            Some("write_io")
        ));
    }

    /// Writer Io hard-fail path: classify records COUNT_MISMATCH for incomplete
    /// write, but `write_io` / `io` summary codes must still be retryable.
    #[test]
    fn retryable_write_io_true_even_with_count_mismatch_reason() {
        assert!(summary_is_retryable(
            CliExit::Generic,
            false,
            &[reason::COUNT_MISMATCH],
            Some("write_io")
        ));
        assert!(summary_is_retryable(
            CliExit::Generic,
            false,
            &[reason::COUNT_MISMATCH],
            Some("io")
        ));
        assert!(summary_is_retryable(
            CliExit::Generic,
            false,
            &[reason::COUNT_MISMATCH],
            Some("disk")
        ));
    }

    #[test]
    fn retryable_permanent_false() {
        assert!(!summary_is_retryable(CliExit::Success, false, &[], None));
        assert!(!summary_is_retryable(
            CliExit::ExportRiskBlocked,
            false,
            &[reason::RISK_GATE],
            None
        ));
        assert!(!summary_is_retryable(
            CliExit::PartialFidelity,
            false,
            &[reason::ATTACH_SOFT_FAIL],
            None
        ));
        assert!(!summary_is_retryable(
            CliExit::Generic,
            false,
            &[reason::VERIFY_FAILED],
            None
        ));
        // Permanent layout/export failure: COUNT_MISMATCH + untyped "export".
        assert!(!summary_is_retryable(
            CliExit::Generic,
            false,
            &[reason::COUNT_MISMATCH],
            Some("export")
        ));
        // True count mismatch without transient code.
        assert!(!summary_is_retryable(
            CliExit::Generic,
            false,
            &[reason::COUNT_MISMATCH],
            None
        ));
        assert!(!summary_is_retryable(
            CliExit::Generic,
            false,
            &[],
            Some("audit_chain_broken")
        ));
        assert!(!summary_is_retryable(
            CliExit::Generic,
            false,
            &[],
            Some("passphrase")
        ));
        assert!(!summary_is_retryable(
            CliExit::Generic,
            false,
            &[],
            Some("schema")
        ));
        assert!(!summary_is_retryable(CliExit::Usage, false, &[], None));
    }
}
