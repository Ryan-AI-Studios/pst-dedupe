//! Produce-side QC gate helpers (fresh + passed).

use matter_core::{qc_run_is_fresh, selection_fingerprint, Matter, QcRunRecord};

/// Why the QC gate blocks produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QcGateBlock {
    /// No `qc_runs` row for this matter/scope.
    Missing,
    /// Last run has `passed = false`.
    Failed {
        error_count: u64,
        warn_count: u64,
        run_id: String,
    },
    /// Selection fingerprint / count / scope mismatch vs last run.
    Stale {
        run_id: String,
        stored_count: u64,
        current_count: u64,
    },
}

impl QcGateBlock {
    /// Operator-facing message (no client paths/subjects).
    pub fn message(&self) -> String {
        match self {
            Self::Missing => {
                "QC required: no production QC run found; run production QC before produce"
                    .into()
            }
            Self::Failed {
                error_count,
                warn_count,
                run_id,
            } => format!(
                "QC failed: last run {run_id} has {error_count} error(s), {warn_count} warning(s); fix findings and re-run QC"
            ),
            Self::Stale {
                run_id,
                stored_count,
                current_count,
            } => format!(
                "QC stale: selection changed since run {run_id} (was {stored_count} candidates, now {current_count}); re-run QC"
            ),
        }
    }
}

/// Load latest QC run for scope and check freshness against current candidates.
///
/// Returns `Ok(None)` when gate allows produce; `Ok(Some(block))` when blocked.
pub fn check_qc_gate(
    matter: &Matter,
    scope: &str,
    current_candidate_ids: &[String],
) -> Result<Option<QcGateBlock>, matter_core::Error> {
    let Some(stored) = matter.load_latest_qc_run_for_scope(Some(scope))? else {
        // Fall back to any latest run — still may be stale on scope.
        let Some(any) = matter.load_latest_qc_run()? else {
            return Ok(Some(QcGateBlock::Missing));
        };
        return Ok(Some(classify_block(&any, scope, current_candidate_ids)));
    };
    if qc_run_is_fresh(&stored, scope, current_candidate_ids) {
        return Ok(None);
    }
    Ok(Some(classify_block(&stored, scope, current_candidate_ids)))
}

fn classify_block(
    stored: &QcRunRecord,
    current_scope: &str,
    current_candidate_ids: &[String],
) -> QcGateBlock {
    if !stored.passed {
        return QcGateBlock::Failed {
            error_count: stored.error_count,
            warn_count: stored.warn_count,
            run_id: stored.id.clone(),
        };
    }
    // passed but not fresh → stale (scope/count/fp)
    let _ = selection_fingerprint(current_candidate_ids);
    let _ = current_scope;
    QcGateBlock::Stale {
        run_id: stored.id.clone(),
        stored_count: stored.candidate_count,
        current_count: current_candidate_ids.len() as u64,
    }
}
